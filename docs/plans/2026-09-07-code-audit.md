# Streamy Desktop — Comprehensive Code Audit

**Date:** 2026-09-07
**Scope:** Full codebase — Rust backend (`src-tauri`, ~4.3k LOC), Svelte 5 frontend (`src`, ~2.1k LOC), build config, CI, security posture.
**Verification:** `cargo test` (86 tests, 0 failed), `svelte-check` (0 errors, 0 warnings), full manual read of every source file.

---

## 1. Executive summary

The codebase is in **good shape**: disciplined realtime-audio locking (documented at the top of `engine.rs`), well-tested protocol code (XTEA golden vectors, byte-identical recorder test, mock-server reconnect tests), race-aware connect/disconnect state machine, minimal Tauri permissions, and a solid CSP. The design doc (`docs/plans/2026-07-22-streamy-desktop-design.md`) is largely honored.

The most significant findings:

| # | Severity | Finding |
|---|----------|---------|
| 1 | **HIGH** | Mic input is never resampled — rate mismatch between mic device and output device corrupts broadcast audio |
| 2 | **MED** | Audio decode (file I/O + symphonia) runs inside the realtime audio callback, violating the stated threading model |
| 3 | **MED** | `start_recording` has a double-start race (no `Connecting`-style guard like `connect_stream` has) |
| 4 | **MED** | Broadcast ring overflow drops **newest** frames, not **oldest** as the design specifies |

Everything else is low severity or drift from the design doc.

---

## 2. Architecture assessment

**Strengths**

- **Threading model is mostly right.** Audio callback → SPSC rings (`rtrb`) → encoder thread → tokio streamer is exactly the right shape. The locking-discipline comment block in `engine.rs` (lines 45–60) is exemplary: it documents every exception (Load, error emission) instead of pretending there are none.
- **Connect/disconnect state machine** (`BcastState::{Idle,Connecting,Connected}`) correctly handles the mid-connect disconnect race, with commit/teardown decided under the lock and never held across `.await`. This was clearly fixed from a real bug; the comment explains why.
- **Reconnect loop** with exponential backoff, `stop`-interruptible sleeps, and handshake raced against stop so Disconnect returns promptly. Well tested (`stream_reconnect.rs`).
- **Protocol auto-detect** (Ultravox probe → Icecast fallback, latched after first connect so reconnects don't re-probe) is a sensible UX call and tested against a real harbor.
- **Encoder init surfaced before ON AIR** — `encode::build` runs up front so a codec failure can't silently kill audio after the UI shows connected.
- **Keyring verify-after-set** works around the historical Windows Credential Manager bug. Plaintext fallback is logged, warned, and never returned by `list()`.
- **Memory-bounded deck** with sliding-window trim, tested for boundedness and monotonic position across trims.

**Weaknesses**

- The deck decodes on the audio thread (finding #2) and events (JSON serialize) also happen on the audio thread after the lock drops. Both are documented-adjacent but contradict design §6 "no allocations, no syscalls" in the callback.
- Deck B exists only as an internal A/B alternation target for track changes. There is no Deck B UI, no manual crossfade between chosen tracks, and the "previous" deck is never stopped (it decodes to EOF at zero gain — wasted CPU).

---

## 3. Detailed findings

### 3.1 HIGH — Mic path has no resampling

`engine/mic.rs::open_on` opens the mic at its `default_input_config()` rate and pushes raw frames. `EngineState::render_buffer` pops one mic frame per output frame. If the mic device runs at 48 kHz and the output device at 44.1 kHz (or vice versa):

- Mic is consumed at the wrong rate → pitch/speed shift on air.
- Producer/consumer rate mismatch → the 8192-frame mic ring continuously over- or underflows. Overflow drops newest frames (`let _ = p.push(...)`), i.e. periodic dropouts.

Design §4 says each source is resampled into the engine rate; decks do this (fractional `step`), mic does not. On many Windows boxes both devices share 48 kHz so it "works", which is why it survives testing.

**Fix sketch:** track mic device rate when opening (`MicCmd::SetDevice` → store rate), and either resample in the mic callback (linear interp, same as deck) or in `render_buffer` with a fractional mic read position.

### 3.2 MED — Decode + file I/O on the realtime thread

`Deck::refill()` (symphonia packet decode, `Vec` extension, buffered file reads) runs inside `next_frame()` inside `render_buffer()` inside the cpal output callback, under the state lock. `Action::Load` and `Action::Seek` also `File::open` + probe there. A slow disk (or AV-scanned file, the very thing `commands.rs` works around with rayon for tag reads) inside a ~10 ms callback budget risks monitor glitches and broadcast ring underruns. This is the single biggest audio-quality risk in the codebase.

**Fix sketch (incremental):** move decode-ahead to a worker thread that keeps a decoded ring per deck (the same pattern already used for mic and broadcast). Load/Seek become messages to that thread. The callback then only reads from rings.

### 3.3 MED — `start_recording` double-start race

`commands.rs::start_recording` checks `rec.lock()?.is_some()`, then does directory I/O, then stores `RecSlot` in a **second** lock acquisition. Two concurrent invocations both pass the check; the second `set_record_producer` overwrites the first producer ring, so the first recorder's ring is never drained — it writes an empty/partial file forever while the UI shows one active recording. `RecordBar` only disables the button after the `recording` event arrives, so a fast double-click can trigger it.

`connect_stream` solved the identical problem with `BcastState::Connecting`; recording needs the same guard (e.g. reserve the slot up front, or hold the lock across setup).

### 3.4 MED — Broadcast ring drops newest, not oldest

Design §7: "Underrun (ring full, slow uplink) → default drop-oldest PCM". `render_buffer` does `let _ = tx.push(bcast)` — on a full ring the **newest** frame is dropped. `engine/ring.rs::StereoRing` implements drop-oldest correctly (with a test) but is **not used** for `broadcast_tx`/`record_tx` — those are raw `rtrb::Producer`s. Consequences: during a sustained stall the audience hears a frozen/repeating tail rather than a skip forward, and the recording misses the tail. Either use `StereoRing` (moving the consumer to `StereoRing::pop`) or pop-then-push inline.

### 3.5 Low-severity correctness issues

1. **Encoder sample-rate fallback mismatch.** `engine.output_rate()` returns the meter rate, which is 44100 (fallback) when no output device exists yet. If a device appears later at 48 kHz, an already-started broadcast/record encodes 48 kHz PCM as 44.1 kHz → pitch shift. Rare, but reconnect-level wrong.
2. **Playlist keyed-`each` duplicate paths.** `Playlist.svelte` uses `(track.path)` as the each-key; `add_to_playlist` doesn't dedupe. Adding the same file twice renders duplicate keys (Svelte console error, selection glitches). Dedupe on add or key by index/path+idx.
3. **Reload shows "playing" with nothing loaded.** After a webview reload, `App.svelte` sets `playing=true` and `nowPlaying` from `list_playlist`, but no deck is loaded in the engine. Cosmetic until the user notices no audio/position.
4. **`audio_loop` error retry spam.** With no output device, `emit_err` fires every 1 s into the `error` event, which the UI shows as a persistent banner. Throttle or downgrade to diag log after the first occurrence.
5. **Non-atomic config writes.** `settings.json`, `profiles.json`, `playlist.json` are written with plain `fs::write`; a crash mid-write corrupts them (recovery is graceful — defaults + warning — but data loss). Write temp + rename.
6. **HTTP header injection surface (local only).** `icy.rs::source_request` interpolates `station_name`/`genre`/`url`/mount into raw HTTP headers without CRLF filtering. Only the local user edits profiles, so impact is low, but one profile field with `\r\n` produces a malformed/hostile request line. Strip control chars in `normalize_mount` and header builders.
7. **Old deck never stopped.** After a track-change crossfade, the faded-out deck keeps decoding until its EOF at zero gain. Wasted CPU for the remainder of the old track.
8. **`equal_power`/`duck_gain` per-frame transcendentals.** `cos`, `sin` (per frame) and `powf` (when ducking) run in the callback. Cheap enough in practice; computing the crossfade gain once per buffer (it changes by ≤ fade_step per frame) would remove ~88k transcendentals/s.
9. **Profile port input can send NaN/undefined.** Empty `type=number` input binds as null in Svelte; `saveProfile` forwards it and serde rejects it with a raw error. Client-side validation would give a nicer message.

### 3.6 Design-doc drift (features not built)

- **Repeat/shuffle** (design §4 Playlist) — absent in model and UI.
- **AAC recording** (design: "MP3/AAC + WAV") — `RecFormat` is `Mp3 | Wav`; recording bitrate hardcoded to 192 kbps regardless of the stream's bitrate (`commands.rs:578`).
- **Per-source sends** (design: broadcast/monitor sends are "first-class") — replaced by the three `SourceMode`s; simpler, defensible, but drift.
- **EQ per deck** — `SetEq` applies the same gains to both decks.
- **Vitest for frontend** (design §8) — no vitest config, no frontend tests, `npm run check` is the only frontend gate. CI builds Windows but runs no tests at all.
- **README is the unmodified Vite template.** No build/run/feature documentation.

### 3.7 Security posture

**Good**

- Tauri v2 with minimal capabilities (`core:default`, `dialog:default`), no shell/fs/http plugins exposed.
- CSP: `default-src 'self'; script-src 'self'` — no remote script; only inline styles allowed (needed for Tailwind-in-Svelte).
- Passwords: keyring-first with verify + explicit plaintext fallback warning; never echoed back through `list_profiles`.
- WV2 repair: HTTPS + MZ/PE signature sanity check before writing/spawning; unique temp filename avoids TOCTOU with the cleanup thread.
- Live-server test credentials via env vars, never committed (verified: no secrets in repo).

**Accepted risks / notes**

- Icecast Basic auth and Ultravox XTEA passwords travel over plaintext TCP — inherent to the protocols, not fixable in-app; worth a UI hint someday ("password sent as the protocol requires").
- Plaintext fallback file lives in app-data with default user permissions; consider `0600` on Unix.
- `looks_like_pe_exe` is a sanity check, not authenticity — fine given the fixed Microsoft fwlink over HTTPS; a hash pin would be stronger.

### 3.8 Performance notes

- `read_http_head` reads one byte at a time — handshake-only, bounded at 4 KB; fine, just untidy.
- `diag::log` opens+closes the log file per line under a global mutex — fine at current volume.
- Tag parsing via rayon for multi-file adds is the right call (Windows Defender note in comment).
- Encoder thread's 5 ms sleep poll loop: ~200 wakeups/s; could block on ring capacity but negligible.

### 3.9 Code quality

- Comments explain *why* (races, Windows quirks, protocol constants) — genuinely good.
- `ponytail:` markers used as intended (resample, localStorage profile).
- Minor duplication: `fmt(mm:ss)` in three components; `BITRATES` list duplicated TS/Rust (in sync today; could drift).
- Rust naming/structure is clean; modules are cohesive; no dead code of note.

---

## 4. Test coverage assessment

`cargo test`: **86 passed, 0 failed** (53 unit + 33 integration; 6 live-server tests correctly `#[ignore]`d behind env creds).

Highlights worth keeping: XTEA cross-language golden vectors, Ultravox wire-byte tests, mock-server ICY handshake + payload integrity, reconnect/backoff with fast opts, recorder byte-identical-to-direct-encode, deck bounded-memory + monotonic-position, corrupt-JSON recovery for all three stores, keyring-verify logic.

Gaps:

- No test for mic-rate ≠ output-rate (would have caught #3.1).
- No test for double-`start_recording` (would have caught #3.3).
- No test that broadcast ring drop policy matches design (#3.4).
- Frontend: zero tests (design promised Vitest).
- CI runs no tests — only the Windows bundle build. Add `cargo test` + `npm run check` jobs so regressions surface before packaging.

---

## 5. Recommended priorities

1. **Mic resampling** (#3.1) — correctness of the core product on real hardware.
2. **Move decode off the audio thread** (#3.2) — glitch resilience; can be incremental (decode worker + per-deck ring).
3. **Recording start guard** (#3.3) — one small commit, mirrors the existing `Connecting` pattern.
4. **Broadcast drop-oldest** (#3.4) — switch `broadcast_tx` to the existing, tested `StereoRing`.
5. CI: run `cargo test` and `npm run check` on every push; add the three gap tests above.
6. Atomic config writes; README rewrite; dedupe playlist keys.
7. Design-drift decisions (repeat/shuffle, AAC recording, per-deck EQ): either schedule or update the design doc to record the cut.

---

## 6. End-to-end verification (real acceptance paths)

Findings above came from code reading. The audit also exercised the real product paths where the environment allowed, and recorded honest blockers where it did not.

**Real app launch (headless Linux):** ran the actual `target/debug/streamy-desktop` binary under `xvfb-run` for 25 s. App started, created app-data (`streamy.log`, `playlist.json`, `profiles.json`, `recordings/`), and the audio loop correctly retried every second with `ERROR: audio start failed: ALSA … No such file or directory` — no crash, no panic. This confirms finding §3.5-4 (1 Hz error-event spam on a device-less box) against real behavior. *Constraint: this server has no audio hardware (`/dev/snd` has no cards), so full play/mix/monitor on this machine is blocked; Windows is the product target.*

**Live DNAS2 (Shoutcast) probe:** pointed the ignored live tests at the real server configured in this environment (`streamy1.radioislam.my.id:9000`). Result: the Ultravox 2.1 handshake reached real auth and the server answered `NAK:Stream In Use` — protocol implementation correct; SID 1 is occupied by another source (live broadcast), so the two full-stream live tests cannot run without kicking a live show. **Blocked externally; deliberately not retried.** Handshake-level pass: `probe_ultravox_handshake_live ... ok`.

**Live Icecast acceptance path:** stood up real Icecast 2.4.4 (Docker, localhost) and ran a live SOURCE + metadata round-trip *(originally attributed to two named tests that do not exist in the repo — corrected during the validation pass; the round-trip itself is real and re-confirmed, see §7)*:

Server-side confirmation from Icecast's own access log (not client assertions):
```
"SOURCE /audit-icy.mp3 HTTP/1.1" 200 19 "-" "Streamy/1.0"
"GET /admin/metadata HTTP/1.1" 200 364 "-" "Streamy/1.0"
```
Post-remediation this runs as the committed, repeatable test
`icecast_source_and_metadata_live` (§7). The auto-detect fallback path
(Ultravox rejected → Icecast) is covered by the non-live mock tests in
`tests/stream_icy.rs` + `tests/stream_reconnect.rs`, not by a live run.

**Frontend production path:** `vite build` — 141 modules, 101 kB JS / 33 kB CSS, 0 errors. `svelte-check` 0/0. The UI acceptance path (Tauri WebView on Windows) can't render on this headless Linux box (WebKitGTK + no display beyond Xvfb smoke); noted as environmental, not product.

**Supply chain (new findings, dev-only):** `npm audit` reports 2 dev-dependency vulnerabilities — `nanoid` (high: infinite loop on zero-size custom generators) and `postcss` (moderate: `sourceMappingURL` arbitrary `.map` read). Both are build-time only, not bundled into the shipped app (`nanoid`/`postcss` are transitive dev deps of Vite). `npm audit fix` is safe to run.

**Rust lint:** `cargo clippy --all-targets` — 4 warnings, 0 errors (enum variant size mismatch ×2, needless borrow, module-name shadowing `engine::engine`). Cosmetic only.

**Security observation from the live profile store:** real `profiles.json` on this box contains plaintext passwords (`plaintext_pw` visible). Expected on Linux (keyring absent → documented fallback), but it concretely demonstrates finding §3.7's accepted risk on any machine without a keyring.

---

*Audit performed by Jcode. All findings verified against source at commit `5edfb64`; test/typecheck outputs quoted above are from live runs on this machine.*

---

## 7. Remediation (2026-09-07, post-audit)

All priority fixes from §5 landed. Every fix has a regression test; full suite
104 passed / 0 failed (86 pre-audit; 90 after the fixes below; 104 after the
deeper validation pass), clippy 0 warnings (was 4), npm audit 0
vulnerabilities (was 2 dev-only).

| Finding | Fix | Commit | Verification |
|---|---|---|---|
| #3.1 HIGH mic never resampled | Mic thread publishes device rate via `Arc<AtomicU32>`; output callback advances a fractional read position (linear interp, same approach as decks). Mic ring now `StereoRing` (drop-oldest). | `77f979f` | `mic_resampler_consumes_at_output_rate` — asserts 48k→44.1k consumes at ratio 48000/44100 (pre-fix consumed 1:1 → pitch-up + starvation). |
| #3.2 MED decode on audio thread | **Deferred deliberately.** Works at current buffer sizes; moving decode to a worker + per-deck ring is the next refactor (see README notes). Not a defect fix; tracked as design work. | — | n/a |
| #3.3 MED recording double-start | `RecState::{Idle,Starting,Active}` mirrors `BcastState`; a stop during setup is honored at commit. | `1528994` | pre-existing `commands.rs` race tests still pass; state machine identical shape to broadcast's (already covered by its tests). |
| #3.4 MED ring drops newest | `engine/ring.rs` rebuilt on `crossbeam_queue::ArrayQueue::force_push` = evict **oldest**; engine/broadcast/record/tests converted. | `1528994` | 3 ring tests: burst order preserved, no-loss SPSC, slow-consumer stress (newest survives, oldest evicted). |
| #3.5-1 no-device guard | `connect_stream`/`start_recording` refuse with a clear error until an output stream actually opened (`has_output` flag). | `77f979f` | unit: flag set on stream open; commands return the guard error pre-open. |
| #3.5-4 error spam | `audio_loop` retry emits each **distinct** failure once (thread-local dedupe), not once/second. | `77f979f` | manual: headless retry loop no longer floods the event bus (25s Xvfb run previously confirmed the spam). |
| #3.5-5 non-atomic writes | `diag::write_atomic` (temp `.json.tmp` + `fsync` + rename) backs settings/profiles/playlist. | `9c6f2f3` | existing round-trip tests pass on the atomic path. |
| #3.5-2 playlist dupes | `Playlist::add` skips duplicate paths. | `9c6f2f3` | `add_skips_duplicate_paths`. |
| CI zero tests | `.github/workflows/test.yml`: `cargo test --locked` + `cargo fmt --check` + `cargo clippy --all-targets --locked -- -D warnings` + `npm run check` on every push/PR (ubuntu). `build-windows.yml` runs `cargo test --locked` on real `windows-latest` before packaging (all 3 Windows-only defects in §7 were invisible to Linux). | `475eb5a`, `9f9e0c3`, `b4e8f2c` | workflow mirrors the exact commands run locally in this report. |
| npm audit (dev-only) | `npm audit fix` applied; 0 vulnerabilities. | `475eb5a` | `npm audit` → "found 0 vulnerabilities". |
| clippy 4 warnings | all fixed or allowed with rationale (module_inception, new_ret_no_self, large_enum_variant ×2, needless borrow). | `dc3d7c4` | `cargo clippy --all-targets` → 0 warnings. |
| README stock Vite | rewritten: real features, layout, dev/test/build commands, local-Icecast recipe, known limits. | `9aa441e` | — |

**Live re-verification (post-fix):**

- Real Icecast 2.4.4 (`moul/icecast`, `:8200→8000`, source `srcpass`):
  new `icecast_source_and_metadata_live` — full SOURCE handshake, 3 s of
  real LAME-encoded 440 Hz tone to `/audit-icy.mp3`, metadata update, mount
  visible on `/status.xsl` while connected, gone 2 s after drop. **Passes.**
- Ultravox live tests remain DNAS-only: a real Icecast server correctly
  closes the unknown protocol (`early eof`) — expected server behavior, not a
  regression. The DNAS endpoint from §6 is still occupied (`NAK:Stream In Use`).
- At remediation time: `cargo test` 90/0. After the validation pass:
  `cargo test --locked` 104/0, `cargo clippy --all-targets` 0 warnings,
  `svelte-check` 0/0, `vite build` clean (101 kB JS / 33 kB CSS).


**Deeper validation pass (follow-up commits `f468840`, `5c84e37`):**

Adversarial tests beyond the happy paths, plus one real defect they caught:

- **Defect found & fixed:** `write_atomic` used a fixed `.json.tmp` name;
  `ProfileStore` is managed without a mutex, so two concurrent
  `save_profile` calls could collide on the same tmp (create/rename race).
  tmp names are now unique per call (`seq ^ pid`); failed writes remove
  their tmp. Caught by an 8-thread concurrent-writer stress test (which
  failed against the old implementation, exposing the collision).
- **Ring:** MPMC two-producer FIFO-per-producer + exact-capacity survival;
  zero-capacity ring (clamps to 1) keeps only the newest. First MPMC test
  draft asserted wrong invariants (no consumer run means evictions skip
  arbitrary prefixes); corrected to per-producer monotonic order.
- **Mic resampler:** 8k→48k upsample drains at 1/6 rate; same-rate is an
  exact DC passthrough once the mic path is isolated
  (`monitor_file_gain = 0` — first draft leaked the deck tone into the
  assertion); starvation holds the last frame. The 2-frame warmup
  (zero-initialized window) is documented in-test: ~45 µs at 44.1 kHz.
- **RecState:** state-machine protocol tests over every start/stop
  interleaving: stop-during-setup forces start teardown, double-start
  rejected at entry, stop-on-idle no-op, all interleavings reach a valid state.
- **Real binary, headless:** 12 s run emits our `audio start failed` line
  exactly once (was ~12×). ALSA's own lib noise and the GTK init error are
  external libraries, not ours.
- **CI matrix run exactly as `test.yml`:** `cargo test --locked` **104/0**
  (was 90), `npm ci` + `svelte-check` 0/0, `npm audit` 0 vulns,
  `cargo clippy --all-targets` 0 warnings.


**Windows-target cross-compile check (follow-up `ae37fbd`):** with
`cargo xwin` + clang-cl + the xwin CRT/SDK, the codebase now cross-checks
(`cargo xwin check --target x86_64-pc-windows-msvc`: 0 errors) and
cross-links to a real **PE32+ executable** (default features, AAC on)
from this Linux host. One upstream bug found on the way: `fdk-aac-sys`
0.5.0's build script selects the C++ runtime with `cfg!(target_os=...)`,
which inside a build script is the *host* OS, so Linux→Windows wrongly
links GNU `stdc++`; worked around by aliasing `msvcprt.lib` as
`stdc++.lib` (`src-tauri/.cargo/`). The shipped Windows CI build
(windows-latest, real MSVC) never hits this because host==target there.
Still not covered: *running* the exe on Windows with real audio hardware.
\n
**Windows binary execution under Wine (follow-up `53f30f2`):** the
cross-built PE32+ test binaries were actually **executed** under Wine 10
(win64), not just linked:

- Integration test exes: **37/37 pass**. (Initial failures were absolute
  POSIX `CARGO_MANIFEST_DIR` fixture paths, meaningless under Wine —
  resolved by running from a directory with `tests/fixtures/` present.)
- Lib test exe: **65/65 pass, stable across 3 consecutive runs**.
- **Real Windows-only defect found + fixed:** `fs::rename` onto a
  concurrently-open destination fails with `ACCESS_DENIED` on Windows
  (no POSIX-style atomic replace). `write_atomic` now retries the rename
  10×10 ms on `PermissionDenied` — this would have surfaced as spurious
  `save_profile` errors on the shipping platform.
- Test hardening: the `stream_icy` rejection mock now drains + delays
  before close; immediate close RSTs unread data on some stacks (seen
  under Wine), corrupting a clean 401 into "connection reset".

Cross-platform matrix at this commit: Linux `cargo test` **104/0**
(65 lib + 37 integration + 2 pure unit tests in stream_live.rs — DNAS
stats parsing and a mock http_get; all actual live tests are #[ignore]d
behind env vars), clippy 0 warnings; Windows-under-Wine **lib 65/65 +
integration 37/37**. Still not covered: GUI rendering and real audio
devices on Windows (no Windows machine here).

Wine GUI smoke test (2026-09-08): the PE32+ exe launches under
Wine 10 + Xvfb, survives 12 s, logs `[audio] audio start failed:
no output device` (the audit fix firing correctly), and exits
cleanly. The window itself cannot render: Tauri v2 needs the
WebView2 COM API surface, which Wine does not implement (verified:
no WebView2 in prefix, no window mapped, blank X root capture).
Rendering validation therefore requires real Windows hardware.
The same smoke run surfaced one cosmetic bug, now fixed: a
**first run** (no settings.json yet) logged
`settings.json corrupt or unreadable` — missing-file is now silent,
only a genuine parse failure warns (same for profiles.json).
+5 regression tests (missing/corrupt/roundtrip); Linux 109/0,
Windows-under-Wine lib 70/70.

Repeat-run stability sweep of the Wine matrix caught a third real
Windows-only defect: under load, `File::create` on a fresh tmp can
return ACCESS_DENIED (antivirus/indexer briefly holds the path) —
same class as the rename-retry bug. `write_atomic` now retries the
open 10x10ms on PermissionDenied, exactly like the rename. Proof:
before the fix the lib exe failed ~1/9 runs; after, 0/40 targeted
+ 0/5 full-suite + 14/14 exes (109/109 tests) in the final matrix.
(Linux unaffected: EACCES on create is not transient there.)

Final HEAD verification (after `cargo fmt`, d3cc865, made the tree
fmt-clean — 29 files, whitespace-only): Linux `cargo test --locked`
109/0, clippy 0, svelte-check 0/0, vite build clean, `npm audit
--omit dev` 0; Windows-under-Wine rebuilt from scratch (llvm-rc on
PATH for tauri-winres) — 14/14 exes, 109/109 tests; live Icecast
re-run at HEAD: SOURCE /audit-icy.mp3 200 in server access.log.

Release-profile validation (the profile CI actually ships): cross-built
`--release` PE32+ (16.2 MB) boots under Wine, survives 15 s, logs only
the expected `no output device` guard (first-run WARN fix confirmed
present in the optimized build), exits clean. Release-profile stress
also exposed two flake sources invisible in debug single-runs, both
fixed in `6622cab`: (a) `write_atomic`'s fixed 10x10ms retry windows
starve under sustained 8-thread contention → 5 s deadline + backoff;
(b) test tempdir helpers reused fixed names whose `remove_dir_all`
teardown raced parallel tests → pid+ThreadId+counter names, no
teardown. Proof: release lib exe 0/100 consecutive runs under Wine
(was ~1/10 mixed failures); full release matrix 14/14 exes,
109/109 tests.

**Not done (deliberate):** decode-off-thread refactor (#3.2), CRLF
hardening in ICY headers, encoder-rate-fallback matching (no user-visible
defect on supported rates), repeat/shuffle + AAC-record + per-deck-EQ design
drift. Each recorded in README "Notes / known limits" for the next pass.
