# Streamy Desktop — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a Winamp-style Windows desktop app that plays a file playlist and/or a live mic, mixes them, encodes to MP3/AAC at a chosen bitrate, and live-streams to a Shoutcast v2 server — with connection profiles, recording, and a library-driven Svelte UI.

**Architecture:** Rust core (cpal I/O, symphonia decode, rubato resample, custom two-bus mixer, LAME/FDK encoders, custom ICY→Ultravox source client) behind a Tauri v2 shell. Svelte 5 + shadcn-svelte frontend. Audio never crosses into the WebView; UI sends commands and receives ~30 fps events. A lock-free ring decouples the realtime audio callback from the encoder/network.

**Tech Stack:** Rust + Tauri v2; cpal, symphonia, rubato, rtrb, `lame` (mp3lame-sys vendored), `fdk-aac-sys`, tokio, serde, keyring, lofty, tracing. Frontend: Svelte 5 (Vite) + shadcn-svelte + Tailwind.

**Testing philosophy:** Pure logic (playlist model, mixer math, ring, encoder-frame validation, source-send mapping, meter math, profile store, ICY handshake) is TDD with unit tests. Audio-HW glue (cpal streams) is manual smoke — inject synthetic PCM in tests, never require a real device in CI.

**Scope of this plan:** P0 + P1 fully bite-sized. P2–P6 are ordered task outlines expanded into full detail at the start of each phase (the design lives in `2026-07-22-streamy-desktop-design.md`).

---

## Prerequisites (one-time, on the dev machine)

- **Windows 10/11.**
- **Rust** via rustup (MSVC toolchain: `rustup default stable-x86_64-pc-windows-msvc`).
- **Microsoft C++ Build Tools** (MSVC + Windows SDK) — needed by Tauri, lame vendored build, fdk-aac build. `winget install Microsoft.VisualStudio.2022.BuildTools`.
- **WebView2 Runtime** (preinstalled on Win11; else Evergreen Bootstrapper).
- **Node.js LTS** + npm.

No external codec binaries for P0/P1. P2 brings libmp3lame (built from source via the `lame` crate's vendored feature). P4 brings fdk-aac (built from source via `fdk-aac-sys`).

---

## Conventions

- **Project layout:**
  ```
  streamy-desktop/
    src-tauri/                 # Rust backend
      Cargo.toml
      src/
        main.rs                # tauri entry (thin)
        lib.rs                 # app builder + command registration
        commands.rs            # #[tauri::command] fns
        events.rs              # event name constants + emit helpers
        playlist/{mod.rs,model.rs}
        engine/{mod.rs,engine.rs,device.rs,deck.rs,mic.rs,mixer.rs,ring.rs,decoder.rs,meter.rs}
        encode/{mod.rs,mp3.rs,aac.rs}
        stream/{mod.rs,icy.rs,ultravox.rs}
        record/mod.rs
        profile/{mod.rs,store.rs}
      tests/                   # integration tests
        fixtures/               # committed WAV fixtures (royalty-free)
    src/                        # Svelte frontend
      App.svelte
      lib/{api.ts, stores.ts, components/*, views/*}
    docs/plans/
  ```
- **Commit cadence:** commit after every task (one logical change). Prefix `feat:`, `test:`, `chore:`, `fix:`.
- **TDD loop per pure unit:** write failing test → run (fail) → minimal impl → run (pass) → commit.
- **Crate naming:** `streamy-desktop`.

---

# Phase P0 — Shell + Playlist + 1 Deck → Monitor

**Milestone:** App opens, you add a file to a playlist, press play, and hear it.

### Task P0.1 — Scaffold Tauri v2 + Svelte-TS frontend (in repo root)

**Files:**
- Create: `package.json`, `vite.config.ts`, `src/`, `src-tauri/`

**Step 1: Create the Svelte-TS frontend at repo root**

```bash
cd /root/streamy-desktop
npm create vite@latest . -- --template svelte-ts
npm install
```

**Step 2: Add Tauri v2**

```bash
npm install -D @tauri-apps/cli@^2
npm install @tauri-apps/api@^2
npx tauri init
```
- App name: `Streamy Desktop`
- Window title: `Streamy Desktop`
- Frontend dev URL: `http://localhost:5173`
- Frontend dist: `../dist`
- Dev command: `npm run dev`; Build command: `npm run build`

**Step 3: Configure window + identifier**

Edit `src-tauri/tauri.conf.json`: set `productName` `Streamy Desktop`, `identifier` `com.streamy.desktop`, window `title` `Streamy Desktop`, `width 900`, `height 640`.

**Step 4: Verify**

Run: `npm run tauri dev`
Expected: a desktop window titled **Streamy Desktop** opens showing the Vite/Svelte welcome page. Close it.

**Step 5: Commit**

```bash
git add -A && git commit -m "chore: scaffold Tauri v2 + Svelte-TS frontend"
```

---

### Task P0.2 — Backend skeleton + a ping command

**Files:**
- Modify: `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`
- Create: `src/lib/api.ts`

**Step 1: Write the command**

`src-tauri/src/lib.rs`:
```rust
#[tauri::command]
fn ping() -> &'static str { "pong" }

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![ping])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```
`src-tauri/src/main.rs`:
```rust
fn main() { streamy_desktop::run(); }
```

**Step 2: Frontend wrapper**

`src/lib/api.ts`:
```ts
import { invoke } from '@tauri-apps/api/core';
export const ping = () => invoke<string>('ping');
```

**Step 3: Verify**

Run: `npm run tauri dev`. In `App.svelte`, call `ping()` on a button click and log it. Expected: console shows `pong`. (Remove the button after verifying.)

**Step 4: Commit**

```bash
git add -A && git commit -m "feat: backend skeleton + ping command"
```

---

### Task P0.3 — Playlist model (pure, TDD)

**Files:**
- Create: `src-tauri/src/playlist/mod.rs`, `src-tauri/src/playlist/model.rs`
- Test: `src-tauri/src/playlist/model.rs` (`#[cfg(test)]` module)

**Step 1: Write the failing test**

Append to `model.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    fn tr(i: usize) -> Track { Track { path: format!("/t{i}.wav"), title: format!("t{i}"), artist: "a".into(), duration_secs: 1.0 } }

    #[test]
    fn next_advances_and_wraps_with_repeat() {
        let mut p = Playlist::new();
        p.add(tr(0)); p.add(tr(1)); p.add(tr(2));
        p.repeat = true;
        assert_eq!(p.next_index(Some(0)), 1);
        assert_eq!(p.next_index(Some(2)), 0); // wraps
    }

    #[test]
    fn next_stops_at_end_without_repeat() {
        let mut p = Playlist::new();
        p.add(tr(0)); p.add(tr(1));
        assert_eq!(p.next_index(Some(1)), None);
    }

    #[test]
    fn shuffle_uses_all_tracks_once() {
        let mut p = Playlist::new();
        for i in 0..5 { p.add(tr(i)); }
        p.set_shuffle(true);
        let order: Vec<usize> = (0..5).map(|_| p.next_index(None).unwrap()).collect();
        let mut sorted = order.clone(); sorted.sort();
        assert_eq!(sorted, vec![0,1,2,3,4]);
    }
}
```

**Step 2: Run — verify fail**

```bash
cd src-tauri && cargo test playlist::model
```
Expected: FAIL (types missing).

**Step 3: Minimal implementation**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track { pub path: String, pub title: String, pub artist: String, pub duration_secs: f64 }

#[derive(Debug, Clone)]
pub struct Playlist {
    pub tracks: Vec<Track>,
    pub repeat: bool,
    pub shuffle: bool,
    // ponytail: deterministic shuffle seeded at runtime; Math-free for tests via index rotation.
    shuf_state: usize,
}

impl Playlist {
    pub fn new() -> Self { Self { tracks: vec![], repeat: false, shuffle: false, shuf_state: 0 } }
    pub fn add(&mut self, t: Track) { self.tracks.push(t); }
    pub fn remove(&mut self, i: usize) { self.tracks.remove(i); }
    pub fn set_shuffle(&mut self, on: bool) { self.shuffle = on; self.shuf_state = 0; }

    pub fn next_index(&mut self, current: Option<usize>) -> Option<usize> {
        if self.tracks.is_empty() { return None; }
        if self.shuffle {
            // round-robin permutation starting at shuf_state — full coverage, deterministic
            let n = self.tracks.len();
            let idx = self.shuf_state % n;
            self.shuf_state += 1;
            return Some(idx);
        }
        match current {
            Some(i) if i + 1 < self.tracks.len() => Some(i + 1),
            Some(_) if self.repeat => Some(0),
            _ => None,
        }
    }
}
```

**Step 4: Run — verify pass**

```bash
cargo test playlist::model
```
Expected: PASS (3 tests).

**Step 5: Commit**

```bash
git add -A && git commit -m "feat: playlist model with repeat/shuffle"
```

---

### Task P0.4 — Symphonia decoder (pure-ish, fixture-based)

**Files:**
- Create: `src-tauri/src/engine/decoder.rs`, `src-tauri/src/engine/mod.rs`
- Fixture: `src-tauri/tests/fixtures/silence-1s-44100-stereo.wav`
- Test: `src-tauri/tests/decoder.rs`

**Step 1: Create a royalty-free fixture**

```bash
cd src-tauri
# 1s of silence, 44.1kHz stereo — no codec licensing needed for tests
ffmpeg -y -f lavfi -i "anullsrc=channel_layout=stereo:sample_rate=44100" -t 1 tests/fixtures/silence-1s-44100-stereo.wav
```
(If ffmpeg is absent, generate via the `wav` crate in a one-off `examples/gen_fixture.rs`; commit the resulting file.)

**Step 2: Add deps**

`src-tauri/Cargo.toml`: `symphonia = { version = "0.5", features = ["wav","mp3","flac","ogg","vorbis","aac"] }`

**Step 3: Write the failing test**

`tests/decoder.rs`:
```rust
use streamy_desktop::engine::decoder::decode_file;

#[test]
fn decodes_stereo_wav_to_expected_frame_count() {
    let (samples, sr, channels) = decode_file("tests/fixtures/silence-1s-44100-stereo.wav").unwrap();
    assert_eq!(sr, 44100);
    assert_eq!(channels, 2);
    assert_eq!(samples.len(), 44100 * 2);
}
```

**Step 4: Run — verify fail**

```bash
cargo test --test decoder
```
Expected: FAIL (function missing).

**Step 5: Implement**

`src/engine/decoder.rs`:
```rust
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use std::fs::File;
use std::sync::Arc;

pub fn decode_file(path: &str) -> Result<(Vec<f32>, u32, usize), String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = std::path::Path::new(path).extension().and_then(|s| s.to_str()) { hint.with_extension(ext); }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| e.to_string())?;
    let mut format = probed.format;
    let track = format.tracks().iter().find(|t| t.codec_params.codec != CODEC_TYPE_NULL).ok_or("no track")?;
    let params = track.codec_params.clone();
    let mut decoder = symphonia::default::get_codecs().make(&params, &DecoderOptions::default()).map_err(|e| e.to_string())?;
    let sr = params.sample_rate.ok_or("no sr")?;
    let channels = params.channels.map(|c| c.count()).unwrap_or(2);

    let mut samples: Vec<f32> = Vec::new();
    let mut buf: Option<SampleBuffer<f32>> = None;
    while let Ok(packet) = format.next_packet() {
        let decoded = decoder.decode(&packet).map_err(|e| e.to_string())?;
        let spec = *decoded.spec();
        let nframes = decoded.capacity();
        if buf.is_none() { buf = Some(SampleBuffer::<f32>::new(nframes, spec)); }
        let sbuf = buf.as_mut().unwrap();
        sbuf.copy_interleaved_ref(decoded);
        samples.extend_from_slice(sbuf.samples());
    }
    Ok((samples, sr, channels))
}
```
Add `pub mod engine; pub mod playlist;` to `src-tauri/src/lib.rs` and `pub mod decoder; pub mod ring;` in `engine/mod.rs`.

**Step 6: Run — verify pass**

```bash
cargo test --test decoder
```
Expected: PASS.

**Step 7: Commit**

```bash
git add -A && git commit -m "feat: symphonia decoder (wav/mp3/flac/ogg)"
```

---

### Task P0.5 — Lock-free SPSC ring (pure, TDD)

**Files:**
- Create: `src-tauri/src/engine/ring.rs`
- Test: inline `#[cfg(test)]`

**Step 1: Add dep**

`Cargo.toml`: `rtrb = "0.3"`

**Step 2: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn burst_beyond_capacity_drops_oldest_keeps_order() {
        let mut r = StereoRing::new(4); // 4 frames
        // push 6 frames; capacity 4 → frames 0,1 dropped, 2..5 remain in order
        for i in 0..6i32 { r.push([i as f32, i as f32]); }
        let mut got = Vec::new();
        while let Some(f) = r.pop() { got.push(f[0] as i32); }
        assert_eq!(got, vec![2,3,4,5]);
    }
}
```

**Step 3: Run — verify fail** — `cargo test ring` → FAIL.

**Step 4: Implement**

```rust
use rtrb::{Consumer, Producer, RingBuffer};

pub struct StereoRing {
    tx: Producer<[f32; 2]>,
    rx: Consumer<[f32; 2]>,
}

impl StereoRing {
    pub fn new(frames: usize) -> Self {
        let (tx, rx) = RingBuffer::<[f32; 2]>::new(frames);
        Self { tx, rx }
    }
    /// Push; if full, drop oldest first (broadcast resilience).
    pub fn push(&mut self, frame: [f32; 2]) {
        while self.tx.push(frame).is_err() {
            if self.rx.pop().is_err() { break; } // make room
        }
    }
    pub fn pop(&mut self) -> Option<[f32; 2]> { self.rx.pop().map(|f| *f) }
}
```

**Step 5: Run — verify pass** — `cargo test ring` → PASS.

**Step 6: Commit**

```bash
git add -A && git commit -m "feat: lock-free SPSC ring with drop-oldest"
```

---

### Task P0.6 — Deck + cpal output (HW glue, smoke)

**Files:**
- Create: `src-tauri/src/engine/deck.rs`, `src-tauri/src/engine/engine.rs`

**Step 1: Deck holds decoded samples + read position**

`engine/deck.rs`:
```rust
pub struct Deck {
    pub samples: Vec<f32>,   // interleaved stereo f32
    pub channels: usize,
    pub pos: usize,          // sample index
    pub playing: bool,
}
impl Deck {
    pub fn new() -> Self { Self { samples: vec![], channels: 2, pos: 0, playing: false } }
    pub fn load(&mut self, samples: Vec<f32>, channels: usize) { self.samples = samples; self.channels = channels; self.pos = 0; }
    pub fn next_frame(&mut self) -> [f32; 2] {
        if !self.playing || self.channels == 0 { return [0.0,0.0]; }
        let c = self.channels;
        let frame = match self.channels {
            2 => [self.samples.get(self.pos).copied().unwrap_or(0.0), self.samples.get(self.pos+1).copied().unwrap_or(0.0)],
            1 => { let m = self.samples.get(self.pos).copied().unwrap_or(0.0); [m,m] },
            _ => [0.0,0.0],
        };
        self.pos += c;
        if self.pos + c > self.samples.len() { self.playing = false; self.pos = 0; }
        frame
    }
}
```

**Step 2: Engine builds the cpal OutputStream; callback pulls from the deck**

`engine/engine.rs` — uses `cpal::default_host()`, builds an `OutputStream` + `Stream` at 44100 f32 stereo; in the write callback, borrow a shared `Deck` (behind `Mutex<Deck>` — acceptable here; P1.4 moves metering + the realtime-correct ownership model in) and copy frames into the output buffer.

> ponytail: P0 uses a `Mutex<Deck>` for simplicity. P1 replaces shared state with a lock-free control queue so the callback stays allocation/lock-free.

**Step 3: Smoke verify**

Add Tauri commands `load_track(path)`, `play()`, `pause()` in `commands.rs`; register them. Run `npm run tauri dev`, trigger `load_track` on a real audio file (via a temp button), then `play()`. Expected: **you hear the file.**

**Step 4: Commit**

```bash
git add -A && git commit -m "feat: deck + cpal monitor output (P0 smoke)"
```

---

### Task P0.7 — Playlist commands + now_playing event

**Files:**
- Modify: `src-tauri/src/commands.rs`, `src-tauri/src/events.rs`

**Step 1: Commands**

`commands.rs`:
- `add_to_playlist(paths: Vec<String>)` — decode each (title/duration via `lofty` tags, fallback to filename), append to a shared `Playlist`.
- `play_index(i)` — load track i into deck, set playing, emit `now_playing`.
- `next()` / `prev()` — use `Playlist::next_index`.
- `pause()`, `seek(pct)`.

`events.rs`: `pub const NOW_PLAYING: &str = "now_playing";` + `pub fn emit_now_playing(app, track)`.

**Step 2: Verify** — frontend can add files and play by index; `now_playing` event arrives (log it).

**Step 3: Commit**

```bash
git add -A && git commit -m "feat: playlist commands + now_playing event"
```

---

### Task P0.8 — Frontend: shadcn-svelte setup + Playlist/Transport UI

**Files:**
- Create: `src/lib/components/Playlist.svelte`, `Transport.svelte`, `NowPlaying.svelte`
- Modify: `src/App.svelte`, `src/lib/stores.ts`, `src/lib/api.ts`

**Step 1: Install Tailwind + shadcn-svelte**

```bash
npx svelte-add@latest tailwindcss
npx shadcn-svelte@latest init      # choose defaults (New York, zinc)
npx shadcn-svelte@latest add button select scroll-area slider dialog
npm i @tauri-apps/plugin-dialog    # for file open
```

**Step 2: Stores + API**

`src/lib/stores.ts`: `playlist`, `nowPlaying`, `streamStatus`, `levels`, `recording` writable stores. `api.ts`: typed wrappers for every command (`add_to_playlist`, `play_index`, `next`, `prev`, `pause`, `seek`).

**Step 3: Components**

- `Playlist.svelte`: `ScrollArea` listing tracks; "Add files" → `open` dialog (audio filter) → `add_to_playlist`; click row → `play_index`.
- `Transport.svelte`: Play/Pause/Next/Prev buttons + seek `Slider`.
- `NowPlaying.svelte`: subscribes to `nowPlaying` store (driven by the `now_playing` Tauri event listener wired in `App.svelte`).

**Step 4: Verify (P0 milestone)**

Run `npm run tauri dev`. Add a file → it appears → press Play → **audio plays** → Now Playing updates → seek scrubs. End-to-end.

**Step 5: Commit**

```bash
git add -A && git commit -m "feat(p0): playlist + transport UI (milestone)"
```

---

# Phase P1 — Mixer + Mic + Device Choosers + Source Toggle + Meters

**Milestone:** Pick an input device; switch monitor source between File / Mic / Both; see live level meters.

### Task P1.1 — Two-bus mixer math (pure, TDD)

**Files:** `src-tauri/src/engine/mixer.rs` · Test: inline

**Step 1: Failing test**
```rust
#[test]
fn sums_sends_into_bus() {
    // two sources, broadcast sends [0.5, 0.5], frames full-scale
    let out = mix_bus(&[[0.8,0.8],[0.4,0.4]], &[0.5,0.5]);
    assert!((out[0] - (0.8*0.5 + 0.4*0.5)).abs() < 1e-6);
}
#[test]
fn send_off_contributes_zero() {
    let out = mix_bus(&[[0.8,0.8],[0.4,0.4]], &[1.0, 0.0]);
    assert!((out[0] - 0.8).abs() < 1e-6);
}
```
**Step 2: Run — fail.** **Step 3: Impl**
```rust
pub fn mix_bus(frames: &[[f32;2]], sends: &[f32]) -> [f32;2] {
    let mut l=0f32; let mut r=0f32;
    for (f,s) in frames.iter().zip(sends) { l += f[0]*s; r += f[1]*s; }
    [l,r]
}
```
**Step 4: Pass.** **Step 5: Commit** `feat: two-bus mixer math`.

### Task P1.2 — Realtime ownership refactor (control queue)

**Files:** `engine/engine.rs`, `engine/ring.rs`, new `engine/control.rs`

Replace `Mutex<Deck>` with a lock-free `control::Queue<Action>` (an `rtrb` of `Action` enums: `Gain(f32)`, `SeekPct(f32)`, `SourceMode(...)`, `MicGain(f32)`, …) drained at the top of each callback. Deck state lives entirely inside the callback thread. This makes the callback allocation/lock-free as the design requires. Smoke-verify P0 still works. Commit `refactor: lock-free control queue in audio callback`.

### Task P1.3 — Mic input + device enumeration

**Files:** `engine/device.rs`, `engine/mic.rs`

- `device.rs`: `list_input_devices()` / `list_output_devices()` → `Vec<DeviceInfo{id,name,channels, default_sr}>` via `cpal::host().input_devices()`.
- `mic.rs`: open an `InputStream` on the selected device; push captured frames into an `StereoRing` consumed by the mixer.
- Commands: `list_input_devices`, `list_output_devices`, `set_input_device(id)`, `set_output_device(id)`. Persist last-used ids via `tauri-plugin-store` (or a small JSON in app-data).
- Smoke: pick the mic → meter reacts. Commit `feat: mic input + device choosers`.

### Task P1.4 — Source sends + SourceMode mapping (pure, TDD)

**Files:** `engine/mixer.rs` · Test: inline

**Step 1: Failing test**
```rust
#[test]
fn source_mode_file_mutes_mic_broadcast() {
    let s = sends_for(SourceMode::File);
    assert_eq!(s.decks_broadcast, 1.0);
    assert_eq!(s.mic_broadcast, 0.0);
}
#[test]
fn source_mode_mic_mutes_decks_broadcast() {
    let s = sends_for(SourceMode::Mic);
    assert_eq!(s.decks_broadcast, 0.0);
    assert_eq!(s.mic_broadcast, 1.0);
}
```
**Step 2: Fail.** **Step 3: Impl** `enum SourceMode { File, Mic, Both }` → `struct Sends { decks_broadcast, mic_broadcast, decks_monitor, mic_monitor }` with the mapping (Both = all 1.0; File = decks→both buses, mic→monitor only; Mic = mic→both, decks→monitor only). **Step 4: Pass.** **Step 5: Wire** the sends into the callback's `mix_bus` calls (monitor bus vs broadcast bus). **Step 6: Commit** `feat: source mode → monitor/broadcast sends`.

### Task P1.5 — Level metering (pure + event)

**Files:** `engine/meter.rs` · Test: inline

**Step 1: Failing test** — `meter::peak(&[[0.5,0.5],[0.9,0.1]]) == [0.9,0.5]`.
**Step 2: Fail.** **Step 3: Impl** `peak` + `rms` per channel over a window. **Step 4: Pass.** **Step 5:** In the callback, accumulate a ~33 ms window, then `app.emit(LEVELS, {monitor:[l,r], broadcast:[l,r]})` at ~30 fps (use a frame counter gate). **Step 6: Commit** `feat: level metering → levels event`.

### Task P1.6 — Frontend: device choosers + source toggle + meters

**Files:** `src/lib/components/{DeviceChoosers,Meters,SourceToggle}.svelte`

- `DeviceChoosers`: two `Select`s from `list_input_devices`/`list_output_devices`; on change call `set_input_device`/`set_output_device`.
- `SourceToggle`: shadcn `ToggleGroup` (File / Mic / Both) → `set_source_mode`.
- `Meters`: custom `Meter.svelte` (vertical bar, peak hold tick) bound to the `levels` store — **custom widget**, not from a library.

**Verify (P1 milestone):** monitor follows the source toggle; meters move on mic and on file playback. Commit `feat(p1): choosers + source toggle + meters (milestone)`.

---

# Phase P2 — Milestone: Live (outlined — expand at phase start)

**Milestone:** Stream MP3 at a chosen bitrate to a Shoutcast server using a saved connection profile.

| Task | Files | Key failing test |
|---|---|---|
| P2.1 MP3 encoder wrapper | `encode/mp3.rs` (`lame` crate, vendored) | encode 1 s sine @ 128k → assert frame sync `0xFFE`/`0xFFF`, output bytes ≈ 16000 |
| P2.2 Encoder thread (ring → lame → tokio chan) | `encode/mod.rs` | property: input frame count ≈ output duration |
| P2.3 Bitrate chooser model | `encode/mod.rs` (`EncoderConfig`) | matrix test: each preset → bytes ≈ bitrate×dur |
| P2.4 ProfileStore + keyring | `profile/{store.rs,mod.rs}` | CRUD round-trip; keyring set/get; plaintext fallback |
| P2.5 Profile commands | `commands.rs` | `list_profiles`/`save_profile`/`delete_profile` |
| P2.6 ICY source client | `stream/icy.rs` | mock `TcpListener`: assert `icy-*` headers + byte-identical payload |
| P2.7 Reconnect/backoff | `stream/mod.rs` | fake server drops N times → retries with growing delay, succeeds |
| P2.8 Now-playing metadata | `stream/icy.rs` | assert `StreamTitle` update sent |
| P2.9 Frontend: bitrate chooser, profile manager, connect dialog, status | `src/lib/components/*` | smoke: connect → `stream_status: connected`; listen on a player |

**Notes to expand:** libmp3lame via `lame` crate (vendored build needs MSVC — present). Use raw `tokio::net::TcpStream` for ICY (no reqwest). Build before writing the failure test: confirm the encoder crate builds on Windows.

---

# Phase P3 — Recording (outlined)

| Task | Files | Key test |
|---|---|---|
| P3.1 Recorder: tee encoder output → `.mp3`/`.aac` | `record/mod.rs` | file parses; byte-identical to captured encoder run |
| P3.2 WAV lossless tap (broadcast PCM) | `record/mod.rs` | RIFF header + data length == samples |
| P3.3 Commands + `recording_status` event | `commands.rs`,`events.rs` | state transitions idle→recording→stopped |
| P3.4 Disk-full handling (stop record, keep streaming) | `record/mod.rs` | simulated write error → event, stream unaffected |
| P3.5 Frontend: record button + elapsed indicator | `src/lib/components/*` | smoke |

---

# Phase P4 — AAC encoder + codec switch (outlined)

| Task | Files | Key test |
|---|---|---|
| P4.1 fdk-aac wrapper (`fdk-aac-sys`, MSVC build) | `encode/aac.rs` | encode sine → ADTS frames parse, bytes ≈ bitrate×dur |
| P4.2 Codec switch in `EncoderConfig` | `encode/mod.rs` | format change rebuilds encoder |
| P4.3 Reconnect-on-codec/bitrate-change (mount constraint) | `stream/mod.rs` | changing config while live triggers reconnect |
| P4.4 Frontend: codec select (MP3/AAC) | `src/lib/components/*` | smoke |

**Note:** fdk-aac patent/licensing — flag redistribution terms; bundle built DLL or static link per `fdk-aac-sys`.

---

# Phase P5 — Deck B + Crossfade + EQ + Ducking + Skin (outlined)

| Task | Files | Key test |
|---|---|---|
| P5.1 Deck B + playlist alternation | `engine/deck.rs` | crossfeed between A/B |
| P5.2 Crossfader (A↔B) into monitor + broadcast buses | `engine/mixer.rs` | equal-power curve math |
| P5.3 3-band EQ per channel (biquads) | `engine/mixer.rs` | frequency response at band centers |
| P5.4 Mic auto-duck under music | `engine/mic.rs` | duck gain reduces when music RMS high |
| P5.5 Skin/theme polish (retro Winamp CSS over shadcn tokens) | `src/lib/skin/*` | visual (manual) |

---

# Phase P6 — Ultravox 2.1 source (outlined — gated on user-supplied protocol docs)

| Task | Files | Depends on |
|---|---|---|
| P6.1 Implement Ultravox 2.1 handshake per docs | `stream/ultravox.rs` | **user docs** |
| P6.2 SID/stream-id plumbing in `ConnectionProfile` | `profile/mod.rs` | docs |
| P6.3 Protocol selector ICY↔Ultravox in UI | `src/lib/components/*` | P6.1 |
| P6.4 Integration: live stream to a real Shoutcast v2 DNAS with Ultravox | manual | P6.1–P6.3 |

> P6 does not start until the Ultravox 2.1 documentation is provided. ICY (P2) keeps the app fully functional against a v2 DNAS in the meantime.

---

# Cross-cutting

## How to run

```bash
npm run tauri dev      # dev (hot frontend + Rust rebuild)
npm run tauri build    # release .msi/.exe
```

## How to test

```bash
cd src-tauri
cargo test             # all unit + integration (pure logic; no HW)
cargo test --test icy  # streaming protocol against mock server
```
Frontend: `npm run test` (Vitest) once meters/faders have logic tests.

## Skills referenced during execution

- @superpowers:executing-plans (task-by-task)
- @superpowers:test-driven-development (every pure unit)
- @superpowers:systematic-debugging (when a test or the smoke fails)
- @superpowers:requesting-code-review (at each phase milestone)
