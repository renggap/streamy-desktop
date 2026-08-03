# Fix Review Issues — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix every issue found in the Streamy Desktop comprehensive code review — 2 high, 5 medium, 7 low — without changing behavior users rely on.

**Architecture:** Changes stay within the existing layering (realtime DSP in `engine/`, protocol in `stream/`, persistence in `profile/`+`settings/`, recorder in `record/`). The two structural fixes are (1) restore the Windows keyring path for stream passwords and (2) make the audio callback never block on a poisoned/std mutex by switching `EngineState` to `parking_lot::Mutex` and documenting the lock discipline. Everything else is test-first cleanup, memory-bounded streaming (WAV recorder, deck sliding window), and hardening (CSP, title clamp, exe sanity check, decode-error logging).

**Tech Stack:** Rust (tauri 2, cpal, rtrb, symphonia, parking_lot), Svelte 5 + TypeScript, Tauri v2, existing test harness (`cargo test` in `src-tauri/`, `npm run check` + `npm run build` at repo root).

**Conventions used throughout:**
- TDD: write the failing test, run it, implement, run again, commit. Each task ends with a commit.
- All commands run from `src-tauri/` (`cd /home/streamy-desktop/src-tauri`) unless noted. The first `cargo test`/`cargo build` after touching `build.rs` or the vendored LAME path recompiles ~19 C files — expected, allow 2–5 min.
- Baseline check (Task 0) must pass before starting.

---

### Task 0: Baseline verification

**Files:** none.

**Step 1:** Run the full Rust test suite.
Run: `cargo test`
Expected: all tests pass (deck_stream, decoder, decoder_mp3, encoder, profile, record, stream_icy, stream_reconnect, plus lib unit tests). LAME C compile happens on first run.

**Step 2:** Run frontend type-check.
Run: `npm run check` (from `/home/streamy-desktop`)
Expected: no errors.

**Step 3:** Commit baseline (if working tree dirty beyond intent).
```bash
git add -A && git commit -m "chore: baseline before review fixes" || true
```

---

### Task 1: 🔴 Restore Windows keyring for profile passwords (plaintext fallback kept)

**Files:**
- Modify: `src-tauri/src/profile/store.rs`
- Test: `src-tauri/tests/profile.rs`

**Problem:** `ProfileStore::new` hardcodes `use_keyring: false`, so all source/DJ passwords land in `profiles.json` as `plaintext_pw`. The `keyring` crate + `KEYRING_SERVICE` are already wired but gated off.

**Step 1: Write the failing test** (add to `tests/profile.rs`)

```rust
#[test]
fn keyring_path_used_when_enabled() {
    let dir = std::env::temp_dir().join(format!(
        "streamy-profile-test-{}-keyring",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let store = ProfileStore::new_with_keyring(dir.clone());
    store.save(sample("kr"), Some("sekrit".to_string())).unwrap();
    // With keyring enabled and unavailable (headless CI), plaintext fallback
    // must still work — the keyring attempt must not break save/get.
    assert_eq!(store.get_password("kr").unwrap(), "sekrit");
    std::fs::remove_dir_all(&dir).ok();
}
```

Run: `cargo test --test profile keyring_path_used_when_enabled`
Expected: FAIL — `new_with_keyring` does not exist.

**Step 2: Implement**

In `store.rs`:
- Add a `pub fn new_with_keyring(dir: PathBuf) -> Self` that sets `use_keyring: true`.
- Keep `new()` (used in `lib.rs`) as-is for now; Task 1 Step 3 flips the app to it.
- In `save()`: when `use_keyring` and `keyring::Entry::set_password` succeeds, set `plaintext_pw = None`; when it fails, fall back to `Some(pw)` (current behavior). `get_password()` already checks keyring first then plaintext. `delete()` already best-effort deletes the keyring entry. No other changes needed.
- The keyring service name is already `streamy-desktop`; on Windows this maps to Credential Manager. On Linux/macOS headless it may fail → fallback path covers it.

**Step 3: Flip the app to keyring-first**

In `src-tauri/src/lib.rs` setup: change `app.manage(ProfileStore::new(data_dir.clone()))` to `ProfileStore::new_with_keyring(data_dir.clone())`.

**Step 4: Run tests**

Run: `cargo test --test profile`
Expected: all profile tests pass (existing CRUD tests unchanged; new test passes via fallback or real keyring).

**Step 5: Commit**

```bash
git add src-tauri/src/profile/store.rs src-tauri/src/lib.rs src-tauri/tests/profile.rs
git commit -m "fix: prefer OS keyring for stream passwords, keep plaintext fallback"
```

---

### Task 2: 🔴 Audio callback must not block on a poisoned/mutex'd EngineState

**Files:**
- Modify: `src-tauri/src/engine/engine.rs` (all `state.lock()` sites)
- Modify: `src-tauri/Cargo.toml` (add `parking_lot = "0.12"`)
- Test: existing tests must stay green; add lock-discipline comment test (doc-only)

**Problem:** `EngineState` lives behind `std::sync::Mutex`. A panic in the callback poisons it, wedging every subsequent command ("poisoned lock" errors) while audio is dead; std `Mutex` also has heavier uncontended fast path than parking_lot.

**Step 1:** Add `parking_lot = "0.12"` to `[dependencies]` in `Cargo.toml`.

**Step 2:** Change the type of `AudioEngine.state` from `Arc<Mutex<EngineState>>` (std) to `Arc<parking_lot::Mutex<EngineState>>`, and update every lock site:
- `audio_loop`: `state.lock()` → returns guard directly (no `Result`). Remove `if let Ok(mut s) = ...` wrappers → `let mut s = state.lock();`.
- callback closure in `build_output_stream`: `match state.lock() { Ok(g) => g, Err(_) => return }` → `state.lock()`.
- `set_broadcast_producer`, `set_record_producer`, `output_rate`: same simplification.
- `app` handle stays std `Mutex` (not on the audio path).

**Step 3: Document the lock discipline** — add this comment block at the top of `engine.rs` (above `EngineState`):

```rust
// LOCKING DISCIPLINE (audio callback safety):
// - The output callback holds `state` for the whole buffer, but NEVER emits
//   events while holding it (emits happen after the guard drops).
// - `state` is mutated from outside the callback only by O(1) field swaps:
//   set_broadcast_producer / set_record_producer / output_rate and the
//   audio_loop rate updates. No I/O, no allocations, no network under `state`.
// - parking_lot::Mutex is non-poisoning: a panic in the callback releases the
//   lock and the next command succeeds instead of wedging the engine.
// - mic input is a separate ring; its producer mutex is never held while
//   pushing the output stream.
```

**Step 4: Run tests**

Run: `cargo test`
Expected: all pass. The engine has no cpal-backed test, so this is a compile + regression check.

**Step 5: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/engine/engine.rs
git commit -m "fix: non-poisoning parking_lot mutex for engine state; document lock discipline"
```

---

### Task 3: 🟠 Stream the WAV recorder to disk (bounded memory)

**Files:**
- Modify: `src-tauri/src/record/mod.rs`
- Test: `src-tauri/tests/record.rs`

**Problem:** `run_wav` accumulates every s16 sample in a `Vec<u8>` and writes the header only on stop → multi-hour shows = GBs of RAM.

**Design:** Write the 44-byte header up front with placeholder sizes, stream data, then `Seek` back and patch the RIFF/data size fields on finish. Requires the sink to be `Write + Seek` (File and `std::io::Cursor<Vec<u8>>` both qualify). On write error, still best-effort patch the header so the partial file is at least playable.

**Step 1: Update the failing tests**

In `tests/record.rs`:
- `wav_run_writes_header_and_all_samples`: change `let mut out = Vec::<u8>::new();` → `let mut out = std::io::Cursor::new(Vec::<u8>::new());`, pass `&mut out`, and assert on `out.get_ref()` instead of `out`:
  ```rust
  let got = out.get_ref();
  assert_eq!(got.len(), 44 + n * 4);
  assert_eq!(&got[40..44], &((n * 4) as u32).to_le_bytes());
  ```
- `run_propagates_write_error`: `FailingSink` must now implement `std::io::Seek` (required by the new `run` bound). Implement `seek` to clamp/return the internal `written` position and `stream_position` to return it. MP3 path never calls seek, so a stub is fine:
  ```rust
  impl std::io::Seek for FailingSink {
      fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
          self.written = match pos {
              std::io::SeekFrom::Start(p) => p as usize,
              std::io::SeekFrom::Current(d) => (self.written as i64 + d).max(0) as usize,
              std::io::SeekFrom::End(d) => (self.cap as i64 + d).max(0) as usize,
          };
          Ok(self.written as u64)
      }
  }
  ```

Run: `cargo test --test record`
Expected: FAIL to compile — `run` currently takes `W: Write`, not `W: Write + Seek`.

**Step 2: Implement streaming WAV**

In `record/mod.rs`:
- Change `run`, `run_wav` (and `run_mp3` for uniformity) to `W: Write + Seek` (File and Cursor satisfy it).
- `run_wav` becomes:
  ```rust
  fn run_wav<W: Write + Seek>(
      rx: &mut rtrb::Consumer<[f32; 2]>,
      sample_rate: u32,
      out: &mut W,
      stop: &AtomicBool,
  ) -> Result<(), String> {
      // Placeholder header; patched with real sizes at the end.
      out.write_all(&wav_header(0, sample_rate, 2)).map_err(|e| e.to_string())?;
      let mut written: u64 = 0;
      let mut err: Option<String> = None;
      loop {
          let mut got = false;
          while let Ok(frame) = rx.pop() {
              let b = frame_to_s16le(frame);
              if let Err(e) = out.write_all(&b) {
                  err = Some(e.to_string());
                  break;
              }
              written += 4;
              got = true;
          }
          if err.is_some() { break; }
          if stop.load(Ordering::Relaxed) && !got { break; }
          if !got { std::thread::sleep(Duration::from_millis(5)); }
      }
      // Best-effort header patch with the final data length (even on error).
      let _ = out.seek(std::io::SeekFrom::Start(0));
      let _ = out.write_all(&wav_header(written as u32, sample_rate, 2));
      let _ = out.flush();
      match err { Some(e) => Err(e), None => Ok(()) }
  }
  ```
- Keep `wav_header` as-is (still used + unit-tested).

**Step 3: Run tests**

Run: `cargo test --test record`
Expected: all pass, including byte-identical MP3 test (unchanged) and header assertions on the Cursor's buffer.

**Step 4: Commit**

```bash
git add src-tauri/src/record/mod.rs src-tauri/tests/record.rs
git commit -m "fix: stream WAV recording to disk with seek-back header patch"
```

---

### Task 4: 🟠 Deck sliding-window trim (bounded memory, no allocation growth in the audio callback)

**Files:**
- Modify: `src-tauri/src/engine/deck.rs`
- Test: in-module `#[cfg(test)]` in `deck.rs` (new)

**Problem:** `Deck.pcm` grows to the whole decoded file (multi-GB for long FLAC) and `SampleBuffer` is allocated per packet inside the callback. The trim makes memory O(lookahead); per-packet allocation remains but is bounded by decode frequency (only on `refill`).

**Step 1: Write the failing test** (new `#[cfg(test)] mod tests` at the bottom of `deck.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm_buffer_stays_bounded_while_playing_long_track() {
        let mut d = Deck::new();
        d.open(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tone-60s.mp3"),
            48000,
        )
        .unwrap();
        d.playing = true;
        // Play ~30 s: pulls far past the trim threshold.
        for _ in 0..(30 * 48000) {
            let _ = d.next_frame();
        }
        // After trimming, buffered frames must be a small multiple of the
        // lookahead window, not proportional to the file length (60 s ≈ 2.8M frames).
        assert!(
            d.pcm.len() / d.channels < 20_000,
            "pcm grew to {} frames",
            d.pcm.len() / d.channels
        );
        // And playback still produces signal after the trim (no window bug).
        let mut max = 0.0f32;
        for _ in 0..2000 {
            let [l, r] = d.next_frame();
            max = max.max(l.abs()).max(r.abs());
        }
        assert!(max > 0.1, "signal lost after trim, max={max}");
    }
}
```

Run: `cargo test --lib deck::tests::pcm_buffer_stays_bounded_while_playing_long_track`
Expected: FAIL — `pcm` grows unbounded (assertion trips).

**Step 2: Implement**

In `deck.rs`:
- Add `const TRIM_THRESHOLD: f64 = 2.0 * 44100.0;` (2 s of frames at 44.1k; scales with file rate via the same constant — use `self.file_rate as f64 * 2.0` instead of a hardcoded constant in the method).
- Add a `fn trim(&mut self)` called at the top of `next_frame` (after `refill`, before reading):
  ```rust
  fn trim(&mut self) {
      // Drop fully-consumed frames so memory stays O(lookahead), not O(file).
      let threshold = self.file_rate as f64 * 2.0;
      if self.read_pos < threshold || self.channels == 0 {
          return;
      }
      let drop = self.read_pos.floor() as usize; // frames behind the read head
      let keep = self.pcm.len() / self.channels - drop;
      if keep <= 2 {
          return; // never trim below the interpolation window
      }
      self.pcm.drain(..drop * self.channels);
      self.read_pos -= drop as f64;
  }
  ```
- `refill`'s `needed = read_pos + 2` stays valid because `read_pos` is decremented by exactly the drained frames.
- `seek()` already clears `pcm` + `read_pos`, so trim state resets naturally.
- Position display (`seek_offset_secs + read_pos / file_rate`) is unaffected: only frames *behind* the read head are dropped.
- The end-of-track check (`read_pos >= nframes && ended`) is unaffected: `nframes` is recomputed after trim.

**Step 3: Run tests**

Run: `cargo test --lib` and `cargo test --test deck_stream`
Expected: new trim test passes; `streams_mp3_and_produces_signal`, `ends_when_exhausted`, `seek_then_signal`, `open_bad_path_errors` all pass.

**Step 4: Commit**

```bash
git add src-tauri/src/engine/deck.rs
git commit -m "fix: sliding-window trim bounds deck memory during playback"
```

---

### Task 5: 🟠 Engine DSP end-to-end test (extract render path, cover source mode / volume / rings)

**Files:**
- Modify: `src-tauri/src/engine/engine.rs`
- Test: in-module `#[cfg(test)]` in `engine.rs` (new)

**Problem:** No test exercises deck → EQ → crossfade → duck → mix → rings. The per-frame DSP is inline in the callback closure.

**Step 1: Extract the per-frame loop into a testable method**

In `EngineState`, add (the callback's inner loop body moves here):

```rust
/// Render one output buffer: decks → EQ → crossfade → duck → monitor/broadcast
/// buses → write to `out`. Returns (monitor, broadcast) per-frame side effects
/// pushed to rings + meter. Pure DSP; no events, no locks beyond caller-held.
fn render_buffer(&mut self, out: &mut [f32]) {
    let sends = sends_for(self.source_mode);
    let channels = out.len().max(1);
    let _ = channels;
    for frame in out.chunks_mut(2.max(1)) {
        // (move the existing per-frame body here verbatim, using self.deck_a/b,
        //  self.eq_a/b, self.crossfade, self.duck_*, self.mic_cons,
        //  self.broadcast_tx, self.record_tx, self.meter, self.volume,
        //  self.write_frame(frame, mon[0], mon[1]))
    }
}
```

The callback closure then becomes: drain actions (unchanged) → `s.render_buffer(out)` → emit outside lock (unchanged). `write_frame` stays a free function.

**Step 2: Write the failing tests** (new `#[cfg(test)] mod tests` in `engine.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> &'static str {
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tone-60s.mp3")
    }

    fn state_with_rings() -> (EngineState, rtrb::Producer<[f32; 2]>, rtrb::Producer<[f32; 2]>) {
        let mut s = EngineState {
            deck_a: Deck::new(), deck_b: Deck::new(),
            active: 0, crossfade: 0.0, crossfade_target: 0.0,
            fade_step: 1.0 / (0.4 * 44100.0),
            eq_a: Eq3Band::new(44100.0), eq_b: Eq3Band::new(44100.0),
            duck_enabled: false, duck_threshold: 0.15, duck_depth_db: 12.0,
            music_rms: 0.0, duck_gain: 1.0, volume: 1.0,
            meter: Meter::new(44100),
            source_mode: SourceMode::Both,
            cons: rtrb::RingBuffer::new(8).1,
            mic_cons: rtrb::RingBuffer::new(8).1,
            broadcast_tx: None, record_tx: None,
        };
        s.deck_a.open(fixture(), 44100).unwrap();
        s.deck_a.playing = true;
        let (btx, _brx) = rtrb::RingBuffer::<[f32; 2]>::new(8192);
        let (rtx, _rrx) = rtrb::RingBuffer::<[f32; 2]>::new(8192);
        s.broadcast_tx = Some(btx.clone());
        s.record_tx = Some(rtx.clone());
        (s, btx, rtx)
    }

    #[test]
    fn render_pushes_tone_to_broadcast_and_record_rings() {
        let (mut s, mut btx, mut rtx) = state_with_rings();
        let mut buf = vec![0.0f32; 1024];
        s.render_buffer(&mut buf);
        let (mut bmax, mut rmax) = (0.0f32, 0.0f32);
        while let Ok(f) = btx.pop() { bmax = bmax.max(f[0].abs()).max(f[1].abs()); }
        while let Ok(f) = rtx.pop() { rmax = rmax.max(f[0].abs()).max(f[1].abs()); }
        assert!(bmax > 0.1, "broadcast ring silent, max={bmax}");
        assert!(rmax > 0.1, "record ring silent, max={rmax}");
    }

    #[test]
    fn file_mode_mutes_mic_on_broadcast_but_keeps_monitor() {
        let (mut s, mut btx, _rtx) = state_with_rings();
        s.source_mode = SourceMode::File;
        // Put a loud frame in the mic ring.
        let (mtx, mut mr) = rtrb::RingBuffer::<[f32; 2]>::new(8);
        mtx.push([0.9, 0.9]).unwrap();
        s.mic_cons = mr;
        let mut buf = vec![0.0f32; 256];
        s.render_buffer(&mut buf);
        let mut bmax = 0.0f32;
        while let Ok(f) = btx.pop() { bmax = bmax.max(f[0].abs()).max(f[1].abs()); }
        // Broadcast carries only the tone deck (~0.9 peak at its loudest), not
        // mic + tone summed: assert it's within the tone's own envelope.
        assert!(bmax <= 0.95 && bmax > 0.1, "broadcast should be tone-only, max={bmax}");
    }
}
```

Run: `cargo test --lib engine::tests`
Expected: FAIL to compile — `render_buffer` does not exist.

**Step 3:** Implement `render_buffer` (Step 1) with the exact body moved from the callback (lines ~383–431 of the current file), then make the callback call it. Keep `write_frame` a free fn; call `write_frame(frame, mon[0], mon[1])` from `render_buffer`.

**Step 4: Run tests**

Run: `cargo test --lib engine::tests` then `cargo test`
Expected: new tests pass; whole suite green.

**Step 5: Commit**

```bash
git add src-tauri/src/engine/engine.rs
git commit -m "test: engine DSP pipeline (source modes, rings) via extracted render_buffer"
```

---

### Task 6: 🟠 Log deck decode errors (distinguish EOF from corruption)

**Files:**
- Modify: `src-tauri/src/engine/deck.rs` (`next_packet`)

**Problem:** Any `next_packet` error is silently treated as end-of-track; a corrupt packet looks like a natural finish.

**Step 1: Write the test** — behavior is unchanged for clean EOF (covered by existing `ends_when_exhausted`), so this is a log-only change. No new test; verify by code review + existing suite.

**Step 2: Implement**

In `next_packet`, replace the bare error arm:

```rust
Err(e) => {
    // Natural end of stream is an IoError(UnexpectedEof); anything else is a
    // decode problem worth logging (corrupt file, unsupported packet).
    let eof = matches!(
        &e,
        symphonia::core::errors::Error::IoError(io_err)
            if io_err.kind() == std::io::ErrorKind::UnexpectedEof
    );
    if !eof {
        crate::diag::log(format!("deck decode error (track may skip): {e:?}"));
    }
    self.ended = true;
    return false;
}
```

**Step 3: Run tests**

Run: `cargo test --test deck_stream`
Expected: all pass (clean EOF still ends the deck).

**Step 4: Commit**

```bash
git add src-tauri/src/engine/deck.rs
git commit -m "fix: log non-EOF deck decode errors instead of silent skip"
```

---

### Task 7: 🟠 Set a real CSP in tauri.conf.json

**Files:**
- Modify: `src-tauri/tauri.conf.json`

**Problem:** `"csp": null`. The app loads only local assets + Tauri IPC, so this is defense-in-depth, but free.

**Step 1:** Replace `"csp": null` with:

```json
"security": {
  "csp": "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; connect-src 'self' ipc: http://ipc.localhost"
}
```

Notes: Svelte/Tailwind inject inline styles → `style-src 'unsafe-inline'` is required. Tauri v2 IPC uses `ipc:` / `http://ipc.localhost` → required in `connect-src`. No external scripts/fonts are loaded. Dev mode (`npm run tauri dev`) serves from `http://localhost:5173`; Tauri does not enforce the production CSP against the dev server.

**Step 2: Verify**

Run: `npm run build` (repo root) — builds the frontend bundle.
Then `cargo test` (compile unaffected).
If a real desktop run is available, launch `npm run tauri dev` and confirm: playlist loads, meters move, settings render. If not available, the build + tests are the gate.

**Step 3: Commit**

```bash
git add src-tauri/tauri.conf.json
git commit -m "sec: set explicit CSP (self + tauri ipc), no external origins"
```

---

### Task 8: 🟡 Remove vestigial ConnectionProfile.protocol (auto-detect owns the decision)

**Files:**
- Modify: `src-tauri/src/profile/store.rs` (struct + `Protocol` enum + `sample()` in tests)
- Modify: `src-tauri/src/commands.rs` (remove `profile.protocol` reference if any — it isn't used; verify)
- Modify: `src/lib/api.ts` (remove `protocol` from `ConnectionProfile`)
- Modify: `src/lib/components/ProfileManager.svelte` (remove `protocol` from `blank()`)
- Test: `src-tauri/tests/profile.rs`

**Step 1: Write the failing test** — add to `tests/profile.rs`:

```rust
#[test]
fn profile_roundtrips_without_protocol_field() {
    let (s, dir) = tmp_store("noprotocol");
    let p = sample("np");
    s.save(p, Some("pw".to_string())).unwrap();
    let list = s.list();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "np");
    std::fs::remove_dir_all(&dir).ok();
}
```

Run: `cargo test --test profile`
Expected: FAIL to compile — `sample()` still sets `protocol`.

**Step 2: Implement**

- `store.rs`: delete the `Protocol` enum and the `protocol` field from `ConnectionProfile`; update `sample()` in `tests/profile.rs` to drop the field.
- `api.ts`: remove `protocol: 'icy' | 'ultravox';` from the interface (backend no longer sends it).
- `ProfileManager.svelte`: remove `protocol: 'ultravox',` from `blank()`.
- `commands.rs`: confirm nothing reads `profile.protocol` (it builds `StreamConfig::Auto` from host/port/etc. only — no change expected).
- Old `profiles.json` files with a `protocol` key: serde ignores unknown fields by default → safe migration.

**Step 3: Run tests + type-check**

Run: `cargo test --test profile` and `npm run check`
Expected: pass.

**Step 4: Commit**

```bash
git add src-tauri/src/profile/store.rs src-tauri/tests/profile.rs src/lib/api.ts src/lib/components/ProfileManager.svelte
git commit -m "refactor: drop vestigial ConnectionProfile.protocol (auto-detect decides)"
```

---

### Task 9: 🟡 Remove dead Playlist repeat/shuffle/remove

**Files:**
- Modify: `src-tauri/src/playlist/model.rs`
- Modify: `src-tauri/src/commands.rs` (no change expected — verify `next` still compiles)
- Test: `src-tauri/src/playlist/model.rs` in-module tests

**Step 1: Update tests** — delete `next_advances_and_wraps_with_repeat` and `shuffle_uses_all_tracks_once`; keep `next_stops_at_end_without_repeat` (rename to `next_stops_at_end`).

**Step 2: Implement**

- Remove `repeat`, `shuffle`, `shuf_state` fields, `set_shuffle`, and `remove` from `Playlist`.
- `next_index` becomes:
  ```rust
  pub fn next_index(&self, current: Option<usize>) -> Option<usize> {
      match current {
          Some(i) if i + 1 < self.tracks.len() => Some(i + 1),
          _ => None,
      }
  }
  ```
- `commands.rs::next` already handles `None` as "end of playlist" — compiles unchanged. Grep for `.repeat`, `.shuffle`, `.set_shuffle`, `.remove(` on playlist across `src-tauri` to confirm no other callers.

**Step 3: Run tests**

Run: `cargo test`
Expected: green.

**Step 4: Commit**

```bash
git add src-tauri/src/playlist/model.rs
git commit -m "refactor: remove dead repeat/shuffle/remove from playlist"
```

---

### Task 10: 🟡 Harden set_stream_title + WebView2 repair download

**Files:**
- Modify: `src-tauri/src/commands.rs`

**Step 1: Write the failing test**

Add to `tests/record.rs`? No — this is commands logic. Since commands take Tauri `State`, extract a pure helper and test it in a new `tests/commands.rs`:

```rust
use streamy_desktop::commands::{clamp_title, looks_like_pe_exe};

#[test]
fn clamp_title_caps_length_and_keeps_utf8() {
    let long = "a".repeat(500);
    let c = clamp_title(long);
    assert_eq!(c.chars().count(), 200);
    // Multi-byte char at the boundary is not split.
    let s = "é".repeat(150);
    let c = clamp_title(s);
    assert!(c.chars().all(|ch| ch == 'é'));
    assert_eq!(c.chars().count(), 200);
}

#[test]
fn pe_exe_check() {
    assert!(looks_like_pe_exe(&[0x4D, 0x5A, 0x90, 0x00]));
    assert!(!looks_like_pe_exe(b"<html>"));
    assert!(!looks_like_pe_exe(&[]));
}
```

Run: `cargo test --test commands`
Expected: FAIL to compile — helpers don't exist.

**Step 2: Implement** (in `commands.rs`, export the helpers as `pub`)

```rust
/// Cap a stream title at 200 chars without splitting UTF-8.
pub fn clamp_title(title: String) -> String {
    title.chars().take(200).collect()
}

/// Cheap sanity check: does this look like a Windows PE executable (MZ header)?
pub fn looks_like_pe_exe(bytes: &[u8]) -> bool {
    bytes.len() >= 2 && bytes[0] == b'M' && bytes[1] == b'Z'
}
```

In `set_stream_title`: `let title = clamp_title(title);` at the top.

In `repair_webview2`:
- After downloading, before writing: `if !looks_like_pe_exe(&buf) { return Err("downloaded file is not a Windows executable".into()); }`
- After `Command::new(&out).spawn()`, best-effort cleanup on a detached thread (the installer needs the file while its UI runs):
  ```rust
  let out2 = out.clone();
  std::thread::spawn(move || {
      std::thread::sleep(std::time::Duration::from_secs(120));
      let _ = std::fs::remove_file(&out2);
  });
  ```

**Step 3: Run tests**

Run: `cargo test --test commands` and `cargo test`
Expected: pass.

**Step 4: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/tests/commands.rs
git commit -m "fix: clamp stream titles; sanity-check and clean up WebView2 bootstrapper"
```

---

### Task 11: 🟡 Add independent XTEA golden vectors

**Files:**
- Modify: `src-tauri/src/stream/ultravox.rs` (test module)

**Step 1: Write the tests** (vectors generated by an independent Python implementation, cross-checked against the existing golden):

```rust
#[test]
fn xtea_multiblock_vectors() {
    // Independent cross-language vectors (Python reference impl):
    assert_eq!(xtea_encipher_hex(b"0123456789abcdef", b"key"), "f88e91fd16781ccc3c2179618d1f0e73");
    assert_eq!(xtea_encipher_hex(b"0123456789abcdefghijklmn", b"secretkey"), "2c4b6945e9051931c98804142a064a27d2a7a1d6327c3ff0");
    assert_eq!(xtea_encipher_hex("pässwörd".as_bytes(), b"1234"), "89393495b864a76553db9412d1b31799");
    assert_eq!(xtea_encipher_hex(b"password", b"this-is-a-very-long-key-over-16-bytes"), "a6e178953174cb08");
    assert_eq!(xtea_encipher_hex(b"liverelay1234", b""), "53b52f6722513f7d9a871496cc6a58a8");
}
```

**Step 2:** Run `cargo test --lib ultravox` — must pass (Rust impl already verified against these during planning).

**Step 3: Commit**

```bash
git add src-tauri/src/stream/ultravox.rs
git commit -m "test: independent XTEA golden vectors (multiblock, unicode, long key)"
```

---

### Task 12: 🟡 Surface SettingsStore poison instead of silent default

**Files:**
- Modify: `src-tauri/src/settings.rs`

**Step 1: Implement** — in `get()`, log when the lock is poisoned rather than silently defaulting:

```rust
pub fn get(&self) -> Settings {
    match self.inner.lock() {
        Ok(g) => g.clone(),
        Err(_) => {
            crate::diag::log("WARN: settings mutex poisoned; returning defaults".to_string());
            Settings::default()
        }
    }
}
```

(No test needed — poison is theoretical; the log makes it diagnosable.)

**Step 2:** Run `cargo test` (compile check).

**Step 3: Commit**

```bash
git add src-tauri/src/settings.rs
git commit -m "fix: log settings mutex poison instead of silent default"
```

---

### Task 13: 🟡 Template dead weight + title/favicon

**Files:**
- Delete: `src/lib/Counter.svelte`
- Delete: `src/assets/svelte.svg`, `src/assets/vite.svg` (verify unreferenced first)
- Delete: `src/assets/hero.png` (verify unreferenced first)
- Modify: `index.html` (`<title>` → `Streamy Desktop`)
- Verify: `public/favicon.svg` or root `favicon.svg` exists (index.html references `/favicon.svg`); if missing, add a minimal one or point to `src-tauri/icons/icon.png` via a copied asset.

**Step 1: Verify unreferenced**

Run: `grep -rn "Counter\|svelte.svg\|vite.svg\|hero.png" src/ index.html`
Expected: only the asset files themselves (no imports). If referenced, stop and re-scope.

**Step 2: Delete + fix title**

```bash
git rm src/lib/Counter.svelte src/assets/svelte.svg src/assets/vite.svg src/assets/hero.png
```

Edit `index.html`: `<title>Streamy Desktop</title>`.

**Step 3: Favicon check**

Run: `ls favicon.svg public/favicon.svg 2>/dev/null || echo missing`
If missing: create `public/favicon.svg` (a 32×32 emerald/cyan rounded square with a ♪) and reference `/favicon.svg`. If a root `favicon.svg` exists, no change.

**Step 4: Verify**

Run: `npm run check` and `npm run build`
Expected: clean.

**Step 5: Commit**

```bash
git add -A
git commit -m "chore: remove template leftovers; fix window title and favicon"
```

---

### Task 14: 🟡 Deduplicate Mp3Encoder free-function encode()

**Files:**
- Modify: `src-tauri/src/encode/mp3.rs`
- Test: `src-tauri/tests/encoder.rs` (existing — must stay green)

**Step 1:** Reimplement `encode()` (the one-shot free function) in terms of `Mp3Encoder` to remove the duplicated FFI init/encode/flush/close:

```rust
pub fn encode(samples: &[f32], cfg: &EncoderConfig) -> Result<Vec<u8>, String> {
    let mut enc = Mp3Encoder::new(cfg)?;
    let mut out = enc.encode(samples)?;
    out.extend_from_slice(&enc.flush()?);
    Ok(out)
}
```

Note: the old function normalized mono→stereo via `cfg.channels`; `Mp3Encoder::new` sets `lame_set_num_channels(2)` and `encode` treats the slice as interleaved stereo. Existing callers (tests + live probes) always pass interleaved stereo, so behavior is unchanged. Keep `bitrate_from_kbps` validation inside `Mp3Encoder::new` (it's there).

**Step 2: Run tests**

Run: `cargo test --test encoder` and `cargo test`
Expected: `encode_sine_produces_mp3_frames_and_expected_size` and `unsupported_bitrate_errors` pass.

**Step 3: Commit**

```bash
git add src-tauri/src/encode/mp3.rs
git commit -m "refactor: one-shot mp3 encode() delegates to Mp3Encoder"
```

---

## Final verification

Run, in order:
1. `cd /home/streamy-desktop/src-tauri && cargo test` — full suite green.
2. `cd /home/streamy-desktop && npm run check` — types clean.
3. `cd /home/streamy-desktop && npm run build` — production bundle builds.
4. If a desktop session is available: `npm run tauri dev` — smoke-test playlist load, meters, a broadcast connect against a local Icecast/Liquidsoap, record 10 s MP3 + WAV, and confirm the WAV file's header sizes are correct (playable in any player).

Then a final commit if anything drifted.

## Out of scope (deliberately)

- Swapping linear resampling for rubato (design-level DSP change, needs its own plan + a real listener test).
- Wiring the currently-unexposed `Playlist.remove` UI / repeat/shuffle UI (feature work, not a fix).
- Frontend Vitest suite (new test infrastructure; the design called for it but it's additive).
- `AppHandle` behind std `Mutex` (not on the audio path).
