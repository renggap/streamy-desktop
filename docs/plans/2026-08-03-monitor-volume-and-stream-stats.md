# Monitor File Volume + Stream Stats Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a monitor-only volume control for local file playback (broadcast unaffected) and a per-session network data indicator (instant kb/s + total MB) shown while streaming.

**Architecture:** Two independent features. (1) The engine's `render_buffer` gains a `monitor_file_gain` field applied only to the deck frame on the monitor bus; a new command + API + NowPlaying slider control it, persisted in `settings.json`. (2) The broadcast streamer task counts audio payload bytes written to the socket and emits a `stream_stats` event every second; the frontend listens and displays rate + total in `BroadcastBar`, reset per connect session.

**Tech Stack:** Rust (Tauri v2 commands, tokio streamer, rtrb rings), Svelte 5 (runes + stores), TypeScript, Tailwind.

---

### Task 1: Engine — `monitor_file_gain` field + Action

**Files:**
- Modify: `src-tauri/src/engine/engine.rs`

**Step 1: Write the failing test**

In the `mod tests` block (after `file_mode_mutes_mic_on_broadcast_but_keeps_monitor`, ~line 562), add:

```rust
#[test]
fn monitor_file_gain_scales_monitor_only() {
    let (mut s, mut brx, _rrx) = state_with_rings();
    // Both source modes send the deck to both buses at unity; file gain must
    // only affect the monitor (out buffer), not broadcast (ring).
    s.monitor_file_gain = 0.5;
    let mut buf = vec![0.0f32; 8192];
    s.render_buffer(&mut buf, 2);
    // Monitor (buf) is halved vs. the same render at unity.
    let mut out_max = 0.0f32;
    for &v in &buf {
        out_max = out_max.max(v.abs());
    }
    let mut bmax = 0.0f32;
    while let Ok(f) = brx.pop() {
        bmax = bmax.max(f[0].abs()).max(f[1].abs());
    }
    // The fixture is a ~0.9 peak tone. At gain 0.5 monitor peaks ~0.45,
    // broadcast stays ~0.9 (unity).
    assert!(
        (0.35..=0.55).contains(&out_max),
        "monitor should be ~half, max={out_max}"
    );
    assert!(bmax > 0.7, "broadcast must stay at unity, max={bmax}");
}
```

**Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test monitor_file_gain_scales_monitor_only`
Expected: FAIL to compile — `EngineState` has no field `monitor_file_gain`.

**Step 3: Implement**

Add to the `Action` enum (near `SetVolume`):

```rust
SetMonitorFileGain(f32),
```

Add to `EngineState` (after `volume: f32`):

```rust
/// Gain for the DJ's own monitor path only (local file playback). Broadcast
/// is NOT affected. 1.0 = unity.
monitor_file_gain: f32,
```

In `render_buffer`, after `let deck_frame = [a[0] * ga + b[0] * gb, a[1] * ga + b[1] * gb];` (mic-duck RMS uses the unscaled `deck_frame`; then):

```rust
// Monitor-only file gain: affects what the DJ hears, never broadcast.
let mon = mix_bus(
    &[[deck_frame[0] * s.monitor_file_gain, deck_frame[1] * s.monitor_file_gain], mic_out],
    &[sends.decks_monitor, sends.mic_monitor],
);
```

(Note: the existing code builds `mon` and `bcast` from `deck_frame`; `bcast` keeps using the unscaled `deck_frame`. Replace the `mon` line only.)

Update the `Action::SetVolume(v) => s.volume = v.clamp(0.0, 2.0),` arm to also handle the new action:

```rust
Action::SetVolume(v) => s.volume = v.clamp(0.0, 2.0),
Action::SetMonitorFileGain(v) => s.monitor_file_gain = v.clamp(0.0, 2.0),
```

Update `EngineState::new()` (init `monitor_file_gain: 1.0`) and the test fixture `state_with_rings()` (add `monitor_file_gain: 1.0`).

Add the public setter on `AudioEngine` (after `set_volume`, ~line 263):

```rust
/// Monitor-only gain for local file playback, 0.0..=2.0 (1.0 = unity).
/// Broadcast is unaffected.
pub fn set_monitor_file_gain(&self, gain: f32) -> Result<(), String> {
    self.push(Action::SetMonitorFileGain(gain.clamp(0.0, 2.0)))
}
```

**Step 4: Run test to verify it passes**

Run: `cd src-tauri && cargo test monitor_file_gain_scales_monitor_only`
Expected: PASS.

Then run the full engine suite: `cargo test engine::` — all green.

**Step 5: Commit**

```bash
git add src-tauri/src/engine/engine.rs
git commit -m "feat(engine): monitor-only file gain (broadcast unaffected)"
```

---

### Task 2: Settings — persist `monitor_file_volume`

**Files:**
- Modify: `src-tauri/src/settings.rs`

**Step 1: Write the failing test**

Add a unit test in `src-tauri/src/settings.rs` (check if a `#[cfg(test)] mod tests` exists; if not, create one at the end of the file):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_settings_json_without_monitor_file_volume_loads_at_unity() {
        // Simulates a pre-feature settings.json (no monitor_file_volume key).
        let json = r#"{"volume":0.8}"#;
        let s: Settings = serde_json::from_str(json).expect("parse");
        assert_eq!(s.monitor_file_volume, 1.0, "missing key must default to unity");
        assert_eq!(s.volume, 0.8);
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test old_settings_json_without_monitor_file_volume`
Expected: FAIL to compile — `Settings` has no field `monitor_file_volume`.

**Step 3: Implement**

Add to `Settings` (after `volume: f32`):

```rust
/// Monitor-only gain for local file playback (1.0 = unity). Broadcast
/// unaffected. Persisted so the DJ's listening level survives restarts.
#[serde(default = "default_volume")]
pub monitor_file_volume: f32,
```

`default_volume()` already returns `1.0` — reuse it. Update `Default` impl:

```rust
monitor_file_volume: default_volume(),
```

Add a setter on `SettingsStore` (after `set_volume`):

```rust
/// Monitor-only gain for local file playback, 0.0..=2.0 (1.0 = unity).
pub fn set_monitor_file_volume(&self, gain: f32) -> Result<(), String> {
    let mut g = self.inner.lock().map_err(|e| e.to_string())?;
    g.monitor_file_volume = gain;
    self.persist(&g)
}
```

**Step 4: Run test to verify it passes**

Run: `cd src-tauri && cargo test old_settings_json_without_monitor_file_volume`
Expected: PASS.

Run: `cargo test` — full suite green (no other tests assert the settings shape).

**Step 5: Commit**

```bash
git add src-tauri/src/settings.rs
git commit -m "feat(settings): persist monitor file volume (default unity)"
```

---

### Task 3: Command — `set_monitor_file_volume` + `get_settings` return

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`

**Step 1: Write the failing test**

`commands.rs` has integration tests in `src-tauri/tests/commands.rs`. Add there:

```rust
#[test]
fn set_monitor_file_volume_persists_gain() {
    // Requires a SettingsStore in a temp dir; follow the existing pattern in
    // tests/commands.rs for constructing one (grep for SettingsStore usage).
    // Assert settings.get().monitor_file_volume == 0.5 after
    // set_monitor_file_volume(50.0), and that engine state accepts it.
}
```

If no SettingsStore test harness exists in `tests/commands.rs`, instead verify via the unit test in Task 2 (persistence) and rely on the engine test (Task 1) for the action. Keep this test only if it fits the existing harness with minimal scaffolding.

**Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --test commands`
Expected: the new test fails/compiles-error until implemented.

**Step 3: Implement**

In `commands.rs`, after `set_volume` (~line 251):

```rust
#[tauri::command]
pub fn set_monitor_file_volume(
    volume_pct: f32,
    engine: State<'_, AudioEngine>,
    settings: State<'_, SettingsStore>,
) -> Result<(), String> {
    let gain = (volume_pct / 100.0).clamp(0.0, 2.0);
    settings.set_monitor_file_volume(gain)?;
    engine.set_monitor_file_gain(gain)
}
```

`get_settings` already returns `Settings` (which now Serializes `monitor_file_volume`) — no change needed there.

In `src-tauri/src/lib.rs`, register the command in `invoke_handler` (after `commands::set_volume,`):

```rust
commands::set_monitor_file_volume,
```

**Step 4: Run test to verify it passes**

Run: `cd src-tauri && cargo test --test commands && cargo test`
Expected: PASS.

**Step 5: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(commands): set_monitor_file_volume command + registration"
```

---

### Task 4: Frontend — monitor file volume slider in NowPlaying

**Files:**
- Modify: `src/lib/api.ts`
- Modify: `src/lib/stores.ts`
- Modify: `src/lib/components/NowPlaying.svelte`
- Modify: `src/App.svelte`

**Step 1: Write the failing test**

The repo has no frontend test infra (per design, out of scope). Verification is `npm run check` + manual reasoning; the "failing test" for frontend is type-check failure before the API exists.

Run: `npm run check`
Expected: FAIL — `setMonitorFileVolume` is not exported / `monitorFileVolume` store missing.

**Step 2: Implement**

`src/lib/api.ts` (after `setVolume`):

```ts
export const setMonitorFileVolume = (volumePct: number) =>
  invoke<void>('set_monitor_file_volume', { volumePct });
```

`src/lib/stores.ts` (after `volume`):

```ts
/** Monitor-only volume for local file playback, percent (0..=200, default 100). */
export const monitorFileVolume = writable<number>(100);
```

`src/lib/components/NowPlaying.svelte`:
- Import `monitorFileVolume` from `../stores` and `setMonitorFileVolume` from `../api`.
- Add a debounce timer + handler mirroring `DeviceChoosers.onVolume` (120 ms):

```ts
let volTimer: ReturnType<typeof setTimeout> | undefined;
function onFileVolume(e: Event) {
  const pct = parseInt((e.currentTarget as HTMLInputElement).value, 10);
  monitorFileVolume.set(pct);
  clearTimeout(volTimer);
  volTimer = setTimeout(() => {
    setMonitorFileVolume(pct).catch(() => {});
  }, 120);
}
```

- Add the slider row after the seek bar (`</header>` closing div, inside the header):

```svelte
<div class="mt-2 flex items-center gap-2">
  <span class="text-[10px] uppercase tracking-wider text-zinc-500">File vol</span>
  <input
    type="range"
    min="0"
    max="200"
    step="1"
    value={$monitorFileVolume}
    oninput={onFileVolume}
    class="streamy-range flex-1"
    aria-label="Local file volume"
  />
  <span class="w-10 text-right text-[10px] tabular-nums text-zinc-500">{$monitorFileVolume}%</span>
</div>
```

`src/App.svelte`: import `monitorFileVolume` from `./lib/stores`; in the `getSettings()` restore block (after the `s.volume` line), add:

```ts
if (typeof s.monitor_file_volume === 'number')
  monitorFileVolume.set(Math.round(s.monitor_file_volume * 100));
```

**Step 3: Verify**

Run: `npm run check` — 0 errors, 0 warnings.
Run: `npm run build` — clean.

**Step 4: Commit**

```bash
git add src/lib/api.ts src/lib/stores.ts src/lib/components/NowPlaying.svelte src/App.svelte
git commit -m "feat(ui): monitor-only file volume slider in NowPlaying"
```

---

### Task 5: Backend — stream stats event (`stream_stats`)

**Files:**
- Modify: `src-tauri/src/events.rs`
- Modify: `src-tauri/src/stream/broadcast.rs`

**Step 1: Write the failing test**

Add a unit test in `src-tauri/src/stream/broadcast.rs` `#[cfg(test)]` module (near the bottom):

```rust
#[test]
fn kbps_from_delta_is_instant_rate() {
    // 1000 bytes in 1s = 8 kbps (1000-bit kb convention).
    assert_eq!(kbps_from_delta(1000), 8.0);
    assert_eq!(kbps_from_delta(0), 0.0);
    // 16_000 bytes/s ≈ 128 kbps.
    assert!((kbps_from_delta(16_000) - 128.0).abs() < 0.5);
}
```

**Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test kbps_from_delta`
Expected: FAIL to compile — `kbps_from_delta` does not exist.

**Step 3: Implement**

`src-tauri/src/events.rs` — add const + struct:

```rust
pub const STREAM_STATS: &str = "stream_stats";

#[derive(serde::Serialize, Clone)]
pub struct StreamStats {
    /// Instant upload rate, kbit/s (1000-bit kb), over the last second.
    pub kbps: f64,
    /// Total audio payload bytes written this session (since connect).
    pub total_bytes: u64,
}
```

`src-tauri/src/stream/broadcast.rs`:
- Import: `use crate::events::{self, StreamStats};` (already imports `events`).
- Add helper near `emit_status`:

```rust
fn kbps_from_delta(bytes: u64) -> f64 {
    (bytes * 8) as f64 / 1000.0
}

fn emit_stats(app: &Option<AppHandle>, total_bytes: u64, last_bytes: u64) {
    if let Some(a) = app {
        let _ = a.emit(
            events::STREAM_STATS,
            StreamStats {
                kbps: kbps_from_delta(total_bytes.saturating_sub(last_bytes)),
                total_bytes,
            },
        );
    }
}
```

- Change `run_stream` signature and body:

```rust
async fn run_stream(
    source: &mut Source,
    mp3_rx: &mut mpsc::Receiver<Vec<u8>>,
    metadata_rx: &mut mpsc::Receiver<String>,
    stop: &Arc<AtomicBool>,
    app: &Option<AppHandle>,
    total_bytes: &mut u64,
) -> bool {
    let mut last_bytes = 0u64;
    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        tokio::select! {
            _ = ticker.tick() => {
                emit_stats(app, *total_bytes, last_bytes);
                last_bytes = *total_bytes;
            },
            chunk = mp3_rx.recv() => match chunk {
                Some(c) => {
                    *total_bytes += c.len() as u64;
                    if source.send_audio(&c).await.is_err() {
                        return true;
                    }
                }
                None => return false,
            },
            title = metadata_rx.recv() => {
                if let Some(t) = title {
                    if let Err(e) = source.send_metadata(&t).await {
                        if source.metadata_fatal() {
                            return true;
                        }
                        diag::log(format!("metadata update failed (non-fatal): {e}"));
                    }
                }
            }
        }
    }
}
```

- Update the caller in `start_with_opts` (the streamer task):

```rust
let mut total_bytes = 0u64;
let streamer = tokio::spawn(async move {
    loop {
        if stop_str.load(Ordering::Relaxed) {
            break;
        }
        let dropped = run_stream(
            &mut source,
            &mut mp3_rx,
            &mut metadata_rx,
            &stop_str,
            &app,
            &mut total_bytes,
        )
        .await;
        ...
```

(The counter lives inside the streamer task closure, so it's fresh per `start_with_opts` → per-session.)

**Step 4: Run test to verify it passes**

Run: `cd src-tauri && cargo test kbps_from_delta`
Expected: PASS.

Then `cargo test` — full suite green (existing stream tests unaffected).

**Step 5: Commit**

```bash
git add src-tauri/src/events.rs src-tauri/src/stream/broadcast.rs
git commit -m "feat(stream): emit per-session upload stats (kb/s + total bytes)"
```

---

### Task 6: Frontend — stream stats display in BroadcastBar

**Files:**
- Modify: `src/lib/stores.ts`
- Modify: `src/App.svelte`
- Modify: `src/lib/components/BroadcastBar.svelte`

**Step 1: Implement (no failing-test infra; type-check gates it)**

`src/lib/stores.ts`:

```ts
export interface StreamStats {
  kbps: number;
  total_bytes: number;
}
export const streamStats = writable<StreamStats>({ kbps: 0, total_bytes: 0 });
```

`src/App.svelte`:
- Import `streamStats` from `./lib/stores` and type `StreamStats` from stores.
- In `onMount`, add a listener after the `stream_status` listener:

```ts
const stats = listen<StreamStats>('stream_stats', (e) => streamStats.set(e.payload));
```

- Reset to zero when a new session starts: in the `status` listener, if `e.payload.state === 'connecting' || e.payload.state === 'disconnected'`, do `streamStats.set({ kbps: 0, total_bytes: 0 })`.
- Add `stats.then((u) => u());` to the cleanup.

`src/lib/components/BroadcastBar.svelte`:
- Import `streamStats` from `../stores`.
- Add a derived format helper:

```ts
const statsLine = $derived(
  connected
    ? `▲ ${$streamStats.kbps.toFixed(1)} kb/s · ${($streamStats.total_bytes / (1024 * 1024)).toFixed(1)} MB`
    : ''
);
```

- Render it under the status line (the `<div class="mt-2 text-center ...">` block, after the `{#if connected} ... {:else} idle {/if}`):

```svelte
{#if connected}
  <div class="mt-1 text-[10px] tabular-nums text-cyan-400">{statsLine}</div>
{/if}
```

**Step 2: Verify**

Run: `npm run check` — 0 errors, 0 warnings.
Run: `npm run build` — clean.

**Step 3: Commit**

```bash
git add src/lib/stores.ts src/App.svelte src/lib/components/BroadcastBar.svelte
git commit -m "feat(ui): live upload rate + session MB in BroadcastBar"
```

---

## Final verification

Run, in order (never two cargo commands in parallel — target dir lock):
1. `cd /home/streamy-desktop/src-tauri && cargo test` — full suite green.
2. `cd /home/streamy-desktop && npm run check` — 0 errors, 0 warnings.
3. `cd /home/streamy-desktop && npm run build` — clean.
4. `cd /home/streamy-desktop/src-tauri && cargo clippy --all-targets` — no new warnings beyond the documented pre-existing baseline (1 `approx_constant` deny in engine/eq.rs + 6 warnings in commands.rs/engine/mod.rs/playlist/model.rs/stream files).

If anything drifted, fix and commit.

## Out of scope

- Broadcast-side volume (listeners always hear file playback at unity).
- Per-track / per-deck-A/B faders.
- Persisting the data counter across app restarts.
- Frontend Vitest infrastructure.
