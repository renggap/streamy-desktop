
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
