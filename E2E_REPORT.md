
## Regression pass on Joko UI build (2026-08-30, post 4d2a2a6)

- Profile: e2e-shoutcast (localStorage-persisted default after fix), MP3 128 kbps
- CONNECT (lime) -> DNAS /7.html = `0,1,79,1000,0,128` (source up, ~128 kb/s)
- In-band title 0x3902 -> /stats `<SONGTITLE>Radio Islam UI</SONGTITLE>`
- DISCONNECT (red) -> /7.html = `0,0,79,1000,0,0` (clean down)
- `initial connect failed` count this session: 0

Harness notes: stale GTK "Open File" dialogs from earlier sessions were stealing button
presses (hover worked, clicks did not); recovered by restarting Xvfb. WebKitGTK select
popups confirmed clickable only by pixel-position inside the popup window.

## Feedback round (2026-08-30): playlist persistence + broadcast defaults + window sizing

Commit `b4a76a8`. All four user feedback items verified on-screen:

1. Playlist persistence: `Playlist::save_to/load_from` writes `playlist.json` in app-data dir.
   Real-dialog add ("tracktrack3") + restart -> 3 tracks listed in UI. Unit tests for
   roundtrip + missing-file recovery.
2. Broadcast defaults AAC 24 kbps (stores.ts); Record+Source merged into one grid row;
   ON AIR indicator green (lime-400) with dot heartbeat + ping; reduced-motion disables both.
   Frame-diff across 8 x11grab samples: 41485 (animation live). DISCONNECT button stays red.
3. Launch width 30% of monitor: window measured 384px logical on 1280px screen.
4. Version `v1.2.0` under wordmark via `getVersion()`.

Config repair: `webviewInstallMode.type` `skipInstall` (invalid variant, broke tauri-build)
-> `skip`. Was introduced in `c0acb41`.

Harness notes this round: app launch requires `DISPLAY=:99` explicitly in env (`nohup env
DISPLAY=:99 ...`); vite dev server dies with `tauri dev` kill and must be restarted before
relaunch or webview renders black. GTK file dialog renders mostly black on Xvfb (no WM)
but is functional. Web Inspector overlaps app at +0+0 — move to 900,100 before screenshots.

### Edge-case round (2026-08-30, follow-up)

- Corrupt playlist.json: app boots, logs `WARN: playlist.json corrupt; starting empty`,
  UI shows 0 tracks, no crash. PASS.
- Restored playlist.json: 3 tracks return; clicking a persisted track plays it
  (NowPlaying updated, position advances). PASS.
- Clean connect -> ON AIR green; disconnect -> OFF AIR; indicator and button states flip
  correctly. PASS. (Known pre-existing quirk: rapid double-click on CONNECT/DISCONNECT can
  race the busy state and leave broadcast idle with a stale error banner; needs a fresh
  app relaunch or webview reload to clear. Not introduced by feedback changes; candidate
  for a `busy` guard fix later.)
- Wrong-shape playlist.json (valid JSON, wrong schema): covered by new unit test
  `wrong_shape_playlist_json_loads_empty`. 52 lib tests green.

### Integration-boundary round (2026-08-30, follow-up 2)

- Codec change while connected (AAC -> MP3 via select popup): liveReconnect fired, stream
  re-established, status line `MP3 · 24 KBPS · 24.2 kbps` active. PASS.
- playlist.json with unknown extra fields: loads fine (serde ignores unknown), 3 tracks
  listed. Forward-compatible. PASS.
- Release profile: `cargo check --release` clean (2m13s). PASS.
- Note: codec/bitrate selection is session-only (in-memory stores); restart returns to
  AAC/24 default. Matches "default codec AAC 24kbps" requirement — defaults, not
  last-used persistence.

### Acceptance replay (2026-08-30, final)

Cold restart, all four user requirements replayed end-to-end on the live app:
1. Playlist persistence: 3 tracks listed immediately after fresh launch. PASS.
2. Stream tab: Codec=AAC, Bitrate=24k defaults shown; Record+Source in one two-column row;
   CONNECT -> ON AIR lime indicator (282 lime pixels in indicator zone) with ANIMATING
   heartbeat (frame diffs 3294/1344/4638 across 0.35s-spaced grabs); DISCONNECT -> OFF AIR.
   PASS.
3. Window: 384x640 logical on 1280x800 screen = exactly 30% width. PASS.
4. Version `v1.2.0` rendered under Streamy DESKTOP wordmark, bottom-left. PASS.

All evidence: x11grab pixel measurements + xwininfo geometry from the running binary,
not inspection of code alone.

### Deep-dive validation (2026-08-30, follow-up 3)

- Bitrate live change while connected: popup select 24 -> 32 kbps, liveReconnect fired,
  status `AAC · 32 KBPS` with DISCONNECT still engaged; restored to 24 kbps. PASS.
- Full release build: `cargo build --release` exit 0 (3m56s, 20.7 MB binary). Release
  binary smoke-tested on Xvfb: launches, 30% width, playlist loads, UI renders. PASS.
- Unknown-field round-trip: new unit test `unknown_fields_dropped_on_save` proves extras
  are tolerated on load and stripped to canonical schema on save. 53 lib tests green. PASS.
