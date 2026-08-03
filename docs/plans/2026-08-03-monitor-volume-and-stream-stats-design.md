# Design: Monitor file volume + network data indicator

**Date:** 2026-08-03
**Status:** Approved (user confirmed both features = Option A, instant rate, persist volume)

## Problem

1. Local file playback has no volume control of its own. The app only has a
   master output volume (Settings → Devices) that applies identically to
   monitor and broadcast, so a DJ cannot turn down the music they hear without
   also lowering what listeners hear.
2. When streaming, there is no visibility into how much data is being
   uploaded: no live rate, no session total.

## Decisions (user-confirmed)

- **Monitor-only file volume:** the volume slider affects only what the DJ
  hears locally (monitor bus). Broadcast listeners always hear file playback
  at unity (they are independent of this control).
- **Per-session stats:** kb/s is an **instant** value recomputed every second
  (not a rolling average); total MB counts from Connect until disconnect and
  resets each session.
- **Volume persists** across app restarts (saved to settings.json, like the
  master volume). The data counter does **not** persist.

---

## Feature 1: Monitor-only local file volume

### Backend (Rust)

- `src-tauri/src/engine/engine.rs`
  - `EngineState` gains `monitor_file_gain: f32` (default `1.0`).
  - New `Action::SetMonitorFileGain(f32)`.
  - In `render_buffer`, after the crossfaded `deck_frame` is computed (mic-duck
    RMS still uses the unscaled frame):
    ```
    deck_mon = deck_frame * monitor_file_gain   // monitor path only
    mon      = mix_bus([deck_mon, mic_out], ...)  // what the DJ hears
    bcast    = mix_bus([deck_frame, mic_out], ...) // listeners: unchanged
    ```
    Master `volume` still multiplies both afterwards. Meters keep showing the
    scaled monitor (they reflect what is heard).
  - `AudioEngine::set_monitor_file_gain(gain: f32) -> Result<(), String>` pushes
    the action (mirror of `set_volume`); clamps 0.0..=2.0.

- `src-tauri/src/settings.rs`
  - `Settings` gains `monitor_file_volume: f32` with
    `#[serde(default = 1.0)]` so existing `settings.json` files load at unity.
  - `get_settings` command returns it.

- `src-tauri/src/commands.rs`
  - New command `set_monitor_file_volume(volume_pct: f32)`:
    `gain = (volume_pct / 100.0).clamp(0.0, 2.0)`, persist via
    `settings.set_monitor_file_volume(gain)?`, then
    `engine.set_monitor_file_gain(gain)`.

### Frontend (Svelte/TS)

- `src/lib/api.ts`: `setMonitorFileVolume(pct: number)` →
  `invoke<void>('set_monitor_file_volume', { volumePct: pct })`.
- `src/lib/stores.ts`: `monitorFileVolume = writable<number>(100)` (percent).
- `src/lib/components/NowPlaying.svelte`: compact volume slider row
  (0–200%, 100 = unity) mirroring the master-volume slider pattern in
  `DeviceChoosers.svelte` (120 ms debounced IPC).
- `src/App.svelte`: restore persisted value on mount from `get_settings`.

---

## Feature 2: Network data indicator (per-session)

### Backend (Rust)

- `src-tauri/src/stream/broadcast.rs`
  - `run_stream` gains a `total_bytes: &mut u64` counter and the
    `app: &Option<AppHandle>`; increments by `c.len()` for each audio chunk
    written. Payload bytes only (Ultravox 6-byte frame header and Icecast
    metadata GETs excluded — audio dominates and keeps the metric
    protocol-fair).
  - A 1-second `tokio::time::interval` arm inside `run_stream`'s select loop
    emits `stream_stats`: `{ kbps, total_bytes }` where
    `kbps = (bytes_this_second * 8) / 1000` — the instant rate, 1000-bit kb
    convention (matches configured-bitrate display).
  - Counter is fresh per `start_with_opts` call → naturally per-session.

- `src-tauri/src/events.rs`
  - New `STREAM_STATS: &str = "stream_stats"` const.
  - `StreamStats { kbps: f64, total_bytes: u64 }` (Serialize).

### Frontend (Svelte/TS)

- `src/lib/stores.ts`: `streamStats` store; reset to zero when `stream_status`
  transitions to `connecting` or `disconnected` (per-session semantics).
- `src/App.svelte`: `listen<StreamStats>('stream_stats', ...)`.
- `src/lib/components/BroadcastBar.svelte`: when connected, show under the
  status line: `▲ 127.8 kb/s · 12.4 MB` (decimal MB, 1 decimal, tabular nums).

---

## Testing

- Engine unit test: `monitor_file_gain_scales_monitor_only` — with gain 0.5,
  rendered monitor is halved, broadcast untouched (reuse `render_buffer` test
  harness).
- Settings: old-JSON-without-field loads at unity (serde default).
- Stats: pure helper `kbps_from_delta(bytes)` unit-tested; existing stream
  tests stay green.
- `npm run check` + full `cargo test` + `cargo clippy --all-targets`
  (no new warnings beyond the documented baseline).

## Out of scope

- No broadcast-side volume (B confirmed: listeners stay at unity).
- No per-track gain, no per-deck A/B faders.
- No persistence of the data counter across app restarts.
