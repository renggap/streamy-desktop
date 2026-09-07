# streamy-desktop

Winamp-style desktop broadcaster: dual decks + live mic over Shoutcast v2 /
Icecast, with MP3/AAC encoding and local recording. Tauri v2 (Rust core,
Svelte 5 UI).

## Features

- **Dual decks** with per-deck 3-band EQ, equal-power crossfader, cue-free
  quick mix. MP3/FLAC/WAV decode via Symphonia, resampled to the output
  device rate (linear interpolation).
- **Live mic** with auto-duck (music RMS drives gain), mic-monitor gain, and
  rate-matched resampling (a 48 kHz mic into a 44.1 kHz output stays in pitch).
- **Broadcast** MP3 (Shine, fixed-point) or AAC-LC (fdk-aac-encoder) to
  Shoutcast v2 (ICY + ultravox admin) or Icecast (ICY + admin.cgi metadata).
  Auto-reconnect with fresh handshake; drop-oldest tap keeps the encoder
  realtime if the uplink stalls (audience skips forward, never freezes).
- **Record** the broadcast mix to MP3/AAC locally; recording works without
  being connected.
- **Meters**: stereo peak/RMS for music and mic, blocky Winamp-style bars.
- Profiles, playlist (persisted, dedup'd), settings, diagnostics log — all
  written atomically (temp + rename).

## Layout

```
src/               Svelte 5 UI (App.svelte + lib/ components)
src-tauri/src/
  engine/          audio core: engine loop, decks, EQ, rings, meter, mic
  encode/          Shine MP3 + AAC wrappers (rate fallback to 44.1k)
  stream/          Shoutcast v2 / Icecast clients, reconnect, metadata
  record/          file recorder (MP3/AAC)
  playlist/ profile/ settings/   persisted state
  commands.rs      Tauri IPC surface
```

## Develop

```bash
npm install
npm run tauri dev     # desktop app, hot-reload frontend
```

Rust only:

```bash
cd src-tauri
cargo test            # 90 unit tests (2 live-stream tests are #[ignore])
cargo clippy
```

Frontend only:

```bash
npm run check         # svelte-check
npm run build         # vite production build
```

## Test against a local Icecast

```bash
docker run -d --name icecast -p 8200:8200 \
  -e ICECAST_SOURCE_PASSWORD=srcpass -e ICECAST_ADMIN_PASSWORD=adminpass \
  moul/icecast
```

Then run the ignored live tests (see `src-tauri/tests/stream_live.rs` for
env vars) or add a profile in the UI pointing at `http://localhost:8200`.

## Build installers

CI (`.github/workflows/build-windows.yml`) builds the Windows MSI/NSIS on
every push and stamps `run_number` as the patch version. `test.yml` runs
`cargo test --locked` + `svelte-check` on every push/PR.

```bash
npm run tauri build
```

## Notes / known limits

- Decode and file I/O currently happen on the realtime audio thread; fine at
  current buffer sizes but the next refactor should move decode to a worker
  thread feeding a ring (see `docs/audit-*.md` for the plan).
- Recording is broadcast-mix only (no per-source stems, no AAC+ADTS raw).
- No repeat/shuffle modes, no per-deck EQ presets (design stretch goals).
