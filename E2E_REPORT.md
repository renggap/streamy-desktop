
## Regression pass on Joko UI build (2026-08-30, post 4d2a2a6)

- Profile: e2e-shoutcast (localStorage-persisted default after fix), MP3 128 kbps
- CONNECT (lime) -> DNAS /7.html = `0,1,79,1000,0,128` (source up, ~128 kb/s)
- In-band title 0x3902 -> /stats `<SONGTITLE>Radio Islam UI</SONGTITLE>`
- DISCONNECT (red) -> /7.html = `0,0,79,1000,0,0` (clean down)
- `initial connect failed` count this session: 0

Harness notes: stale GTK "Open File" dialogs from earlier sessions were stealing button
presses (hover worked, clicks did not); recovered by restarting Xvfb. WebKitGTK select
popups confirmed clickable only by pixel-position inside the popup window.
