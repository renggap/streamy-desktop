# streamy-desktop

Winamp-style desktop broadcaster: dual decks + live mic over Shoutcast v1/v2 /
Icecast, with MP3/AAC encoding and local recording. Tauri v2 (Rust core,
Svelte 5 UI).

## Features

- **Dual decks** with per-deck 3-band EQ, equal-power crossfader, cue-free
  quick mix. MP3/FLAC/WAV decode via Symphonia, resampled to the output
  device rate (linear interpolation).
- **Live mic** with auto-duck (music RMS drives gain), rate-matched
  resampling (a 48 kHz mic into a 44.1 kHz output stays in pitch), and
  optional RNNoise suppression (noise cancelling) on the broadcast path.
  The mic is broadcast-only: it never plays on the local monitor/speakers
  (no feedback/echo), and its level still shows on the input meter.
- **Broadcast** MP3 (Shine, fixed-point) or AAC-LC (fdk-aac-encoder).
  Protocol auto-detect on connect: **Shoutcast v1** (ICY password handshake
  — default, works on DNAS1 and DNAS2) → **Ultravox 2.1** (DNAS2 DJ login /
  stream id > 1) → **Icecast / Liquidsoap harbor**. AzuraCast profiles
  connect Icecast directly. Metadata follows the live protocol (v1
  admin.cgi, v2 in-band, Icecast /admin/metadata). Auto-reconnect with fresh
  handshake; drop-oldest tap keeps the encoder realtime if the uplink stalls
  (audience skips forward, never freezes).
- **Record** the broadcast mix to MP3/AAC locally; recording works without
  being connected.
- **Meters**: stereo peak/RMS for music and mic, blocky Winamp-style bars.
- Profiles (dedicated add/edit screen with password reveal), playlist
  (persisted, dedup'd), persistent stream title (survives disconnect and
  reload), settings, diagnostics log — all written atomically (temp +
  rename).

## Layout

```
src/               Svelte 5 UI (App.svelte + lib/ components)
src-tauri/src/
  engine/          audio core: engine loop, decks, EQ, rings, meter, mic
  encode/          Shine MP3 + AAC wrappers (rate fallback to 44.1k)
  stream/          Shoutcast v1/v2 + Icecast clients, reconnect, metadata
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
cargo test            # 123 tests pass; 7 live-server tests stay #[ignore]d
                      # behind env creds (see src-tauri/tests/stream_live.rs
                      # and stream/ module tests)
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

Then run the ignored live tests (see `src-tauri/tests/stream_live.rs` and the
`#[ignore]`d tests in `src-tauri/src/stream/` for env vars — `STREAMY_ICE_*`
for Icecast, `STREAMY_V1_*` for Shoutcast) or add a profile in the UI
pointing at `http://localhost:8200`.

## Build installers

CI (`.github/workflows/build-windows.yml`) builds the Windows MSI/NSIS on
every push and stamps `run_number` as the patch version. `test.yml` runs
`cargo test --locked` + `cargo fmt --check` + `cargo clippy -D warnings`
+ `svelte-check` on every push/PR; `cargo test --locked` also runs on
`windows-latest` before any installer is packaged.

```bash
npm run tauri build
```

## Notes / known limits

- Shoutcast v1 source binds to stream id 1; profiles needing a different sid
  or a DJ account resolve automatically via Ultravox (v2).
- Decode and file I/O currently happen on the realtime audio thread; fine at
  current buffer sizes but the next refactor should move decode to a worker
  thread feeding a ring (see `docs/audit-*.md` for the plan).
- Recording is broadcast-mix only (no per-source stems, no AAC+ADTS raw).
- No repeat/shuffle modes, no per-deck EQ presets (design stretch goals).
