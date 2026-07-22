# Streamy Desktop — Design

**Date:** 2026-07-22
**Status:** Approved (brainstorm phase complete)
**Next step:** writing-plans → implementation plan

## 1. Goal

A Winamp-style desktop audio app that **plays files and captures a mic, mixes them, and live-streams the result to a Shoutcast server**. Doubles as a DJ broadcast console and a player. Records the broadcast to disk.

## 2. Requirements (locked)

- **Platform:** Windows only.
- **Core stack:** Rust + Tauri.
- **Sources:** stream from **file (playlist)**, from **mic**, or **both** (DJ mix). User picks an input device (mic chooser).
- **Broadcast path:** playlist + live mic mix → encode → Shoutcast.
- **Codecs:** MP3 + AAC. **Bitrate chooser** (per-codec presets + custom).
- **Server protocol:** Shoutcast v2. Ship **ICY/HTTP source** now (accepted by v2 DNAS); implement **Ultravox 2.1** handshake when the user-supplied protocol docs arrive (true v2 / SID).
- **Recording:** record the broadcast to file (MP3 / AAC / lossless WAV).
- **Profiles:** connection-profile management (server login CRUD, multiple, persisted). Passwords in OS keyring.
- **UI:** existing component library (**Svelte 5 + Vite + shadcn-svelte + Tailwind**), themed to a retro Winamp look. Custom-build only domain widgets (VU meters, faders, transport) that no library provides. Do **not** hand-roll standard chrome.

## 3. Architecture

Two layers. **Audio never crosses into the WebView.**

- **Frontend** (Tauri WebView, Svelte 5): skin, playlist, deck transport, mic/input chooser, output chooser, source toggle, bitrate/codec chooser, profile manager, connect dialog, recording controls, level meters, status. Svelte stores fed by Tauri events.
- **Backend** (Rust core): owns the realtime audio thread + encoder + recorder + streamer. Tauri commands in; events out.

IPC contract:
- **Commands** (UI → core): `play`, `pause`, `seek`, `load_track`, `next`, `prev`, `set_input_device`, `set_output_device`, `set_source_mode`, `set_gain`, `set_broadcast_send`, `set_monitor_send`, `set_crossfade`, `set_encoder_config`, `connect`, `disconnect`, `start_recording`, `stop_recording`, profile CRUD, `list_input_devices`, `list_output_devices`.
- **Events** (core → UI, ~30 fps for meters): `levels`, `now_playing`, `stream_status`, `recording_status`, `error`.

## 4. Components (Rust core)

- **AudioEngine** — owns cpal streams + the single mixer callback. Enumerates and selects input/output devices (mic chooser + monitor chooser). Hot-swap re-init. Persists last-used device ids.
- **Playlist** — file list: add/remove/reorder, next/prev, repeat/shuffle. Feeds the active deck.
- **Deck (A, later B)** — `symphonia` decode → PCM at engine sample rate. Play/pause/seek/volume/pitch.
- **MicInput** — cpal input stream (WASAPI) bound to the chosen device; gain; optional auto-duck.
- **Mixer (two-bus)** — each source has a **monitor send** and a **broadcast send** (on/off + level). Produces:
  - **Monitor bus** → cpal output (what the DJ hears, independent).
  - **Broadcast bus** → Encoder (what the audience hears).
  - `rubato` resamples each source into the engine sample rate.
  - Per-channel 3-band EQ + master gain + A↔B crossfade (Deck B lands in P5).
- **Encoder** — pulls PCM from the broadcast tap, feeds `mp3lame-sys` (MP3) / `fdk-aac-sys` (AAC) at the chosen bitrate/sample-rate/channels. Output = chunked encoded frames. `EncoderConfig { codec, bitrate_kbps, sample_rate, channels }`.
- **Recorder** — tees the encoder's output to a file. MP3 `.mp3` / AAC ADTS `.aac` (both self-framing → concat). Optional lossless **WAV** tap off broadcast PCM. Filename `<timestamp>_<station>.<ext>`. Optional auto-split on track change (later).
- **Shoutcaster** — tokio task. Opens socket, sends ICY/HTTP source handshake (`icy-name`, `icy-genre`, `icy-br`, `icy-url`, `icy-pub`, content-type), streams encoded frames. Reconnect/backoff. Updates now-playing metadata. Ultravox 2.1 variant added in P6 from user docs.
- **ProfileStore** — CRUD for connection profiles; persist JSON in Tauri app-data dir. Passwords via `keyring` crate → Windows Credential Manager. Plaintext fallback + warning if keyring unavailable.
- **MetadataResolver** — tags via `symphonia`/`lofty`; manual override.
- **Bridge** — Tauri command dispatch + event emission. Meter samples computed in the mixer, throttled to ~30 fps.

Data shapes:
```
ConnectionProfile { name, host, port, user, password(keyring), stream_id(SID),
                    protocol: ICY | Ultravox, mount, agent }
EncoderConfig    { codec: MP3 | AAC, bitrate_kbps, sample_rate, channels }
SourceMode       { File | Mic | Both }   // sets broadcast sends; sends remain first-class
```

## 5. Data flow

```
Deck A (from Playlist) ─┐
Deck B                  ─┤  each ─[monitor_send]──→ Monitor bus ─→ cpal out (headphones)
Mic (chosen input dev)  ─┘  each ─[broadcast_send]─→ Broadcast bus ─→ Encoder (LAME/FDK @ bitrate)
                  (rubato resample per source → engine SR)                            ├→ Shoutcaster → socket (ICY / later Ultravox 2.1)
                                                                                     └→ Recorder → file (.mp3 / .aac / .wav)
```

Recorder + Shoutcaster consume the **same** encoder output → file equals what the audience hears.

## 6. Threading model (decides whether audio glitches)

- **Audio callback thread** (cpal, realtime priority): mix → write monitor → push broadcast frames into an SPSC ring. **No allocations, no locks except the lock-free ring, no syscalls.** Pure DSP.
- **Encoder thread**: pulls from the ring → `lame_encode` / `fdk_aac_encode` (allocations ok) → pushes encoded bytes to a tokio channel.
- **Streamer task** (tokio): bytes → socket. **Never blocks the audio thread.**
- **UI control queue**: lock-free; gains/seeks applied at the top of the callback.

The ring decouples callback time from network jitter → glitch-free audio over a flaky uplink.

## 7. Error handling

- **Mic unplug / selected input device gone** → "no mic" state, event, stream continues (music only). Auto-retry / fallback to default device; mic chooser highlights.
- **No input devices** → mic chooser empty, Mic / Both modes disabled; File mode unaffected.
- **Swap input device while live** → re-init mic stream only (~ms gap on mic); file broadcast keeps running.
- **Output device loss** → monitor fails silently (log); broadcast unaffected.
- **Encoder failure** → fatal for that session; stop stream, surface error.
- **Network drop / 401 / wrong password** → exponential backoff (cap 30 s); the ring absorbs short drops. 3× auth failure → stop + surface.
- **Underrun (ring full, slow uplink)** → default **drop-oldest PCM** (audience hears a brief skip, DJ unaffected). Toggle "stop stream on underrun" for quality mode.
- **Bad / unreadable track** → skip, log, continue playlist.
- **Disk full / write fail** → stop recording, event; **stream keeps going**.
- **Keyring unavailable** → plaintext fallback, surface warning.
- **Bitrate/codec change while live** → blocked. Shoutcast mount fixes content-type + bitrate at connect; changing = disconnect + reconnect. UI disables the control while live, or marks "applies next connect."
- All errors → typed enum → UI event + `tracing` to a rotating log file.

## 8. Testing

- **Mixer / decode:** known PCM (sine → RMS/FFT), crossfade curves, EQ response, resampler identity when SR matches.
- **Encoder:** assert MP3/AAC frame-sync headers parse, bitrate/size within bounds; gold-file compare for one sample.
- **Bitrate matrix:** encode fixed PCM at each preset, assert output size ≈ `bitrate × duration`.
- **Recorder:** output file parses (frame headers); byte-identical to a captured encoder run.
- **Shoutcaster:** tokio `TcpListener` mock → assert handshake headers + byte-for-byte payload match; test reconnect/backoff/auth-fail via a fake server that drops connections.
- **Profiles:** CRUD round-trip; keyring store/load; plaintext fallback path.
- **Realtime ring:** property test — SPSC order preserved, drop policy honored under burst.
- **No hardware in CI:** inject synthetic PCM instead of a real mic/deck.
- **Smoke:** local Icecast (Docker) loopback — validates the ICY protocol though the target is Shoutcast.
- **Frontend:** Vitest for meter/fader component logic; Tauri command contracts mocked.

## 9. Tech stack

**Backend (Rust):** `cpal` (I/O), `symphonia` (decode), `rubato` (resample), `mp3lame-sys` (MP3), `fdk-aac-sys` + bundled libfdk-aac DLL (AAC), `tokio` (runtime/net), raw tokio TCP for ICY source, `tracing` (logs), `serde` (config), `rtrb` or `crossbeam-queue` (lock-free ring), `lofty` (tags), `keyring` (Credential Manager), `directories` (app-data paths). Tauri v2 shell + IPC.

**Frontend:** Svelte 5 (Vite) + **shadcn-svelte** (+ `bits-ui`) + Tailwind. Custom: VU meters, crossfader knob, transport. CSS theme for the retro Winamp skin.

**Svelte, not SvelteKit** — single-window SPA; SvelteKit's SSR/adapter adds friction. Views via a Tabs component, not a router. (Upgrade path: SvelteKit + `@sveltejs/adapter-static` if multi-window/routing is ever needed.)

## 10. Phasing (vertical slices, each end-to-end runnable)

- **P0** — Tauri shell + **Playlist** + 1 deck (symphonia) → cpal monitor. *Hear a file.*
- **P1** — Two-bus Mixer + Mic + **input/output device choosers** + **Source toggle (File/Mic/Both)** + level meters. *(mic-only / file-only wired to monitor)*
- **P2 — milestone: live.** MP3 encoder + **bitrate chooser** + **connection profiles** (keyring) + ICY source + now-playing metadata + reconnect/backoff.
- **P3** — **Recording** (MP3/AAC + WAV).
- **P4** — **AAC encoder** + codec switch.
- **P5** — Deck B + crossfade + EQ + ducking + skin polish.
- **P6** — **Ultravox 2.1** handshake → true Shoutcast v2 / SID. *Gated on user-supplied protocol docs — slot earlier if they arrive.*

## 11. Constraints & risks

- **Ultravox 2.1 (P6) blocks on the user's protocol docs.** ICY (P2) works against a v2 DNAS meanwhile, so live streaming is not blocked.
- **Bitrate/codec change = reconnect event.** Cannot hot-swap mid-stream.
- **AAC licensing.** libfdk-aac is best quality but ships no binaries; bundle a prebuilt Windows DLL. Fallback: FFmpeg-native AAC (weaker quality).
- **libshout / FFmpeg do not speak Ultravox 2.1.** True v2 source is custom code → relies on the incoming docs.

## 12. Open (non-blocking)

- Skin asset direction (classic Winamp vs. modern-retro).
- Recorder auto-split-on-track-change: include in P3 or defer.
- Broadcast limiter/compressor for loudness: optional P7.
