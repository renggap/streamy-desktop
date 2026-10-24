# Streamy Desktop — Comprehensive Code Audit (1.5.0 tree)

**Date:** 2026-10-24
**Scope:** Full codebase at commit `0407a99` (main, immediately after the revert restoring 1.5.0 behavior): Rust backend `src-tauri/src` (~6.4k LOC), integration tests (~1.3k), Svelte 5 frontend `src` (~1.9k), `update-worker/` (Cloudflare Worker), CI workflows, docs, security posture.
**Verification:** `cargo test --locked` → **119 passed / 0 failed / 7 ignored** (lib 80+2i, integration 34+4i, dnas probe 0+1i); `cargo fmt --check` clean; `cargo clippy --all-targets --locked -- -D warnings` clean; `npm run check` (svelte-check + tsc) 0 errors/0 warnings; both CI workflows green on this commit (Linux `test` 5m01s, `build-windows` 13m22s, artifact `streamy-desktop-windows` 16.25 MB). All numbers are measured on this machine, not assumed.
**Method:** Five parallel review passes (correctness, security, code quality, tests/coverage, docs/CI) plus direct verification: every HIGH/MED finding below was re-checked against source in the main session before inclusion, and two conflicting subagent claims were settled by live runs (see §6 note on the aac-off check); the prior audit (`docs/plans/2026-09-07-code-audit.md`) was re-verified finding-by-finding.

---

## 1. Executive summary

The codebase is in **good shape overall**: disciplined realtime-audio locking, well-tested protocol code (XTEA golden vectors, mock-server reconnect tests), atomic persistence with Windows-tolerant retries, a tight Tauri permission surface, and a clean revert (zero 1.5.1 remnants repo-wide, verified by grep from two independent passes). The prior audit's remediations held up — with one notable exception (#3.3, see §5).

The most significant findings:

| # | Severity | Finding | Area |
|---|---|---|---|
| 1 | **HIGH** | Encoder sample rate frozen at connect; device rate change mid-session pitch-shifts live air AND recordings | engine/commands |
| 2 | **MED** | Stop/disconnect issued during connect/start window is silently lost — teardown arms are dead code; the regression "proving" them is itself defective | commands |
| 3 | **MED** | Live DJ credentials hardcoded in committed test file (contradicts prior audit's "never committed" claim) | tests |
| 4 | **MED** | Mic open failures are fully silent and a dead device name is persisted across launches | engine/mic |
| 5 | **MED** | `stream.play()` result ignored then `has_output = true` — guard passes with a callback that never runs → silent ON AIR | engine |
| 6 | **MED** | Plaintext password fallback written world-readable (`0644`) on non-keyring systems | profile store |
| 7 | **MED** | CRLF/header injection via profile fields into Icecast SOURCE request (known-open since 2026-09-07) | stream/icy |
| 8 | **MED** | WebView2 repair: download → magic-byte check → execute, no Authenticode/hash pin, small TOCTOU window | commands |
| 9 | **MED** | Zero-length/undecodable file permanently stalls the deck; auto-advance dies | engine/deck |
| 10 | **MED** | `monitor_file_volume` persisted but never restored at startup | lib |
| 11 | **MED** | `ProfileStore::new` silently disables keyring (insecure-default trap; currently unused) | profile store |
| 12 | **MED** | EQ is global despite two per-deck instances and the design doc's "per-channel EQ" | engine |

No CRITICAL findings. All remaining items are LOW/INFO (§4.4–4.5), dominated by dead code, duplication (map in §4.4), docs drift, and previously accepted risks.

---

## 2. Severity taxonomy

- **CRITICAL** — exploitable remotely, data loss, supply-chain compromise. *(none found)*
- **HIGH** — user-visible corruption of the core product under plausible conditions (device hot-swap, common hardware states).
- **MED** — race or latent bug, security hardening gap with a realistic path, design violation with user impact.
- **LOW** — edge-case correctness, quality/dead code, docs inaccuracies.
- **INFO** — accepted risks, nits, notes for future passes.

---

## 3. Architecture assessment

**Strengths**

- **Threading model is sound and honestly documented.** Audio callback → lock-free `StereoRing` (drop-oldest via `force_push`) → encoder thread → tokio streamer. The LOCKING DISCIPLINE header (`engine.rs:51-66`) documents every exception instead of pretending there are none. The one violation (decode under lock) is a deliberate, documented deferral (prior #3.2).
- **State machines for connect/record** (`BcastState`, `RecState`) have the right shape — check-then-set under one lock, commit decided under the lock, never held across `.await`. (Their one real flaw is finding #2.)
- **Protocol code is well-tested where it counts:** XTEA cross-language golden vectors, wire-byte Ultravox tests, mock-server ICY handshake + payload integrity, reconnect/backoff including a Windows-timer-quantum pacing fix, encoder-init failure surfacing.
- **Persistence is atomic and Windows-hardened:** `diag::write_atomic` (unique tmp, fsync, rename, PermissionDenied retry with 5 s deadline) backs settings/profiles/playlist; an 8-thread stress test guards the tmp-name collision fix.
- **Supply chain (dev side) clean:** no secrets committed *except* finding #3; PAT lives only in the Worker as a runtime secret; `opener` scope verified against crate source (http/https/mailto/tel only — `file:`/`smb:` rejected).
- **Frontend is a small, clean surface:** Svelte 5 runes used consistently, zero `@html`/`innerHTML`/`eval`, CSP locked to `'self'`, localStorage holds only a profile *name* and a notice flag.

**Weaknesses**

- Error paths around device lifecycle (mic open, output `play()`, device loss) are the weakest seam: multiple findings (#4, #5, LOW-1, LOW-6) share the shape "device thing failed → swallow → guards/pass state lie".
- `commands.rs` (899 LOC) concentrates the IPC surface and two state machines; the commit/stop interleavings are only tested through a pure-model mirror that already diverged from the real code (#2).
- Dead code and triplicated patterns are accumulating (§4.4 duplication map) — cheap to pay down now, harder after more features.

---

## 4. Detailed findings

### 4.1 HIGH

#### 1. HIGH — Encoder sample rate frozen at connect; device rate change pitch-shifts air and recordings

**Location:** `src-tauri/src/engine/engine.rs:431-437` + `src-tauri/src/commands.rs:440-445` (broadcast), `:636-640` (recording)
**Evidence:** `audio_loop` rebuilds adapt meter, both decks, `fade_step`, and `out_rate` to the new device config — but the encoder thread isn't in engine state at all; `EncoderConfig.sample_rate` is read once via `engine.output_rate()` at connect/record start and the encoder is never re-initialized.
**Impact:** hot-unplug or headphone-swap (any `Signal::Rebuild`) to a device with a different default rate (44.1k↔48k, common on Windows) makes the callback push new-rate PCM into a LAME/fdk handle still encoding at the old rate → **~8.8% pitch/speed shift on air and in the recording** until manual disconnect; `ice-audio-info` header is also stale.
**Status:** New. Supersedes prior-audit #3.5-1 ("no mismatch found" — true at connect time; the mid-session variant was not visible then).
**Fix sketch:** on rebuild, if `out_rate != enc rate`, tear down + restart the encoder thread (or resample ring→encoder rate). Block-rebuild-while-live is the smallest safe variant.

### 4.2 MED — correctness

#### 2. MED — Stop/disconnect during the start window is silently lost; teardown arms are dead code

**Location:** `src-tauri/src/commands.rs:505-512` (`disconnect_stream` restores `Connecting`), `:705-719` (`stop_recording` restores `Starting`), vs commit arms `:472` (`BcastState::Idle => Commit::TearDown`) and `:668` (`RecState::Idle => true`) — reachable only if stop had set `Idle`, which it never does mid-start.
**Evidence (verified):** both stop paths return the in-flight state unchanged; the commit therefore sees `Connecting`/`Starting`, treats it as "still starting", and **commits Connected/Active anyway** — after the stop already cleared the ring producer. The code comments claim the opposite ("stop already … set Idle — honor that"; "the in-flight start sees Idle at commit") — the comments describe intent, not behavior.
**Impact:** a disconnect/stop landing in the start window returns `Ok`, then start commits with a dead ring → ON AIR / "recording" with no audio until the user stops again. Today this is gated only by the UI hiding buttons while `busy` — latent for any direct invoke or future UI change.
**Fix sketch:** stop must set `Idle` when it sees `Connecting`/`Starting` (or set an abort flag the commit checks); fix the comments.
**Bonus finding:** the regression test `tests/commands.rs::stop_during_setup_honored` calls `stop(St::Starting)` (correctly yielding `Starting`) but then feeds **`St::Idle`** — not stop's output — into `start_commit`, asserting the teardown arm against a state the real protocol cannot produce there. `every_interleaving_reaches_consistent_state` only asserts membership in {Idle, Starting, Active} — trivially true. The tests pass and prove nothing about the real interleaving. Fix the model chain (`start_commit(after_stop.0)`) — it will then fail, correctly, until #2 is fixed.

#### 9. MED — Zero-length / undecodable file permanently stalls the deck

**Location:** `src-tauri/src/engine/deck.rs:186-188` — `if nframes == 0 { return [0.0, 0.0]; }` returns before the end check at `:207`.
**Impact:** a file that opens but yields zero decodable frames (0-byte .mp3, truncated header) never sets `just_ended` → `track_ended` never fires → `App.svelte:111-114` auto-advance never runs → playlist silently stops; the deck also keeps calling refill every frame.
**Fix sketch:** on the early-return path, if `self.ended && nframes == 0` set `playing=false; just_ended=true`.

#### 10. MED — `monitor_file_volume` persisted but never restored

**Location:** `src-tauri/src/lib.rs:101-112` (setup restores device, volume, denoise — no monitor gain) vs `settings.rs:107-111` (persists it); only setter is `commands.rs:295`.
**Impact:** DJ monitor level resets to 100% every launch (monitor blast on relaunch); broadcast level unaffected.
**Fix sketch:** one line in setup: `let _ = engine.set_monitor_file_gain(settings.get().monitor_file_volume.clamp(0.0, 2.0));`

### 4.3 MED — security

#### 3. MED — Live DJ credentials hardcoded in a committed test

**Location:** `src-tauri/tests/dnas_aac_dj_probe.rs:13-17` — `HOST = "az1.radioislam.my.id"`, harbor `8035`, DNAS `8030`, `DJ_USER = "jcode"`, `DJ_PASS = "jcode12345"`.
**Impact:** anyone with repo read access can broadcast to the harbor / read DNAS stats. The test also has **zero assertions** (println verdict only) — it leaks credentials and proves nothing. Contradicts the prior audit's §3.7 claim "credentials via env vars, never committed" — this file postdates that audit. (Private repo mitigates; still a standing credential leak in VCS history and on origin.)
**Status:** New (pre-existing at the v1.5.0 tag; not introduced by the revert).
**Fix:** move to env vars like the other live tests (`STREAMY_DJ_*`), **rotate the password**, and note the secret remains in git history (private-repo acceptable; rewriting history is not warranted).

#### 6. MED — Plaintext password fallback is world-readable

**Location:** `src-tauri/src/profile/store.rs:129` (`plaintext_pw = Some(pw)`) + `src-tauri/src/diag.rs:62` (`File::create` — no mode) → `profiles.json` and its tmp at default umask (0644).
**Impact:** on any machine where the keyring is absent/fails (headless Linux, some setups), stream passwords sit readable by any local user. Windows mitigated by %APPDATA% per-user ACLs. Prior audit flagged "consider 0600" — still open.
**Fix:** `OpenOptions::new().mode(0o600)` (Unix) in `write_atomic` — one line.

#### 7. MED — CRLF/header injection into Icecast SOURCE request (known-open)

**Location:** `src-tauri/src/stream/icy.rs:104-115` — `SOURCE {mount} HTTP/1.1\r\n… ice-name: {name}\r\n ice-url: {url}\r\n` with `station_name`/`genre`/`url`/`host` interpolated raw; `normalize_mount` (`:158-167`) trims/prefixes `/` only. `user`/`password` are base64 → immune. `/admin/metadata` is safe (url-encoded, `:63-68`).
**Impact:** a profile field containing `\r\n` smuggles headers / a second request — against the user's *own configured server*; the only vector is pasting attacker-supplied values (hostile "setup guide"/shared profile). A hostile server cannot write profile fields, so there is no remote-only path. Listed as deliberately-not-done in the 2026-09-07 remediation; still open.
**Fix:** strip/reject CR, LF, NUL in all profile string fields at the `save_profile` trust boundary.

#### 8. MED — WebView2 repair: download → magic check → execute, no signature pin

**Location:** `src-tauri/src/commands.rs:798-841`; URL const `:742` (HTTPS, Microsoft fwlink); `looks_like_pe_exe` `:534-548` checks `MZ`+`PE\0\0` only; writes `%TEMP%/streamy-webview2-setup-{pid}-{ms}.exe` then `Command::new(&out).spawn()`.
**Impact:** executing attacker bytes = full user compromise. TLS excludes MITM; residual risk = compromised MS CDN/CA, or a same-user TOCTOU swap in the tiny write→spawn window. Prior audit accepted with "hash pin would be stronger" — position unchanged, now with the concrete residual enumerated.
**Fix:** verify Authenticode signature or pin SHA-256 before spawn; write with exclusive create.

#### 11. MED — `ProfileStore::new` silently disables the keyring

**Location:** `src-tauri/src/profile/store.rs:41-44` — `new` delegates to the no-keyring constructor. Zero production callers today (`lib.rs:86` uses `new_with_keyring`), but it is an insecure-default trap for future callers.
**Fix:** delete `new`; make `new_with_keyring` the only constructor.

#### 12. MED — EQ is global, not per-deck (design violation)

**Location:** `src-tauri/src/engine/engine.rs:564-567` — `SetEq` sets identical gains on `eq_a` and `eq_b`; the two instances exist only for independent filter state. Design doc (§6) claims "per-channel 3-band EQ".
**Fix:** either implement per-deck gains or collapse to one instance + update the design doc. Don't keep dead flexibility.

#### 4. MED — Mic failures fully silent; dead device persisted

**Location:** `src-tauri/src/engine/mic.rs:39-43` (`set_device` only queues, always Ok), `:86-97` (open returns `None`, no log/event), `:125` (`stream.play().ok()?`); `src-tauri/src/commands.rs:244-253` persists the name; `lib.rs:90-98` restores it each launch.
**Impact:** broadcast app's core input can die with zero UI signal, and every launch silently re-selects a mic that never opens.
**Fix:** reply channel or `Arc<AtomicBool>` open-status + emit an error event; emit like `engine.rs` does.

#### 5. MED — `stream.play()` ignored, then `has_output = true`

**Location:** `src-tauri/src/engine/engine.rs:441-442`.
**Impact:** if cpal `play()` fails, the callback never runs (encoder starves, stream silent) but the `has_output_device()` guard passes → connect/record proceed and encode nothing.
**Fix:** propagate `play()` errors; set `has_output` only on success.

### 4.4 LOW

**Correctness**
1. `has_output` never resets on device loss (`engine.rs:442` — only ever set true) → Connect/Record "succeed" with no callback after hot-unplug with no remaining device. Set `false` in the `Err` arm.
2. Broadcast encoder-thread encode/flush errors swallowed (`broadcast.rs:218,228,232` `if let Ok(...)`) → permanent silent air, Connected status, no diagnostic. Log + emit error + break. (Recording path propagates properly — `record/mod.rs:102-113`.)
3. WAV header sizes truncate at 4 GiB (`record/mod.rs:158,165`, `as u32`) → recordings past ~6.8 h (44.1k stereo s16) wrap RIFF sizes. Cap or refuse past the spec limit.
4. EQ filters never re-based off 44100 (`engine.rs:426-437` updates meter/decks but not EQ; `eq.rs:146-148` biquads built at fixed rate) → on a 48 kHz device, shelf/peak centers shift ~9%. Add `Eq3Band::set_sample_rate`.
5. Mic input stream has no error recovery (`mic.rs:121` — error callback only `eprintln!`) → mic hot-unplug kills capture silently until manual re-select. Output side auto-rebuilds; mic has no equivalent.
6. `ProfileStore` read-modify-write without a mutex (`store.rs:143-149`; `diag.rs:41-42` comment acknowledges) → two concurrent saves can lose one profile. Add `Mutex<()>` across save/delete.
7. Mid-stream reconnect retries forever at base→max backoff, no give-up (`broadcast.rs:266-294`) → a rejecting server (mount taken) gets hammered ≤ every 30 s indefinitely + UI churn. Design said 3-strikes cap; consider max-attempts.
8. TearDown path returns `Ok(())` (`commands.rs:484-489, 668-675`) — connect/stop race reports success for a take that was instantly stopped. Return a distinguishing result or emit Stopped.
9. `let _ = h.stop()` (`commands.rs:673`) and `let _ = self.streamer.await` (`broadcast.rs:443`) swallow final-flush errors / a panicked streamer task (which would strand ON AIR with no reconnect). Log both.

**Security**
10. Corrupt `profiles.json` → silent wipe on next save (`store.rs:68-74` `unwrap_or_else(Vec::new)`) — parse error empties the list; the next save overwrites all other profiles (+ their plaintext passwords). Rename to `profiles.json.bad` before accepting the empty state.
11. Unbounded download buffer in `repair_webview2` (`commands.rs:808-818`) — a compromised endpoint streams forever → OOM. Bail past ~10 MB (bootstrapper ≈ 2 MB).
12. Hostile worker can push a phishing update URL (`update.rs:8,53-65`, `App.svelte:76`) — **mitigated, verified against crate source**: `opener:default` scope only allows `http/https/mailto/tel` globs; `file:`/`smb:` rejected. Fix: require `url` startsWith the repo's GitHub prefix before showing the action.
13. Recording + atomic-write files: `File::create` truncates and follows symlinks with predictable names (`record/mod.rs:199` via `commands.rs:631`; `diag.rs:50-62`) — local attacker with write access to a user-chosen shared recording dir can truncate arbitrary files. `create_new(true)` + random suffix for recordings.
14. `.gitignore` lacks `.env`, `*.dev.vars`, `.wrangler/` (none exist today; PAT is a runtime secret — belt-and-braces).

**Docs/CI** (drift per taxonomy)
15. README test count stale — `README.md:51` "109 tests pass" vs measured **119/0/7**. Exact accounting: the count was correct at commit `732d737` (116 total − 7 ignored); then `8ec8363` added 7 tests (denoise 3, engine 2, mixer 1, settings 1) and `9f6eec6` added 3 (update.rs) with no README bump. The v1.5.0 tag's README was already stale by 10; the revert restored it verbatim (correct for a tree-exact restore — fix on top, not by editing the revert).
16. README Icecast recipe broken — `README.md:66` `-p 8200:8200`; moul/icecast listens on 8000 → as written, `localhost:8200` is unreachable. Use `-p 8200:8000` (the repo's own test comment, `stream_live.rs:234`, has it right).
17. README `:90` points at `docs/audit-*.md` (no such path; audit lives at `docs/plans/…`); `:51-52,71-72` env-var pointer incomplete (`STREAMY_ICE_*` in `icy.rs:278-295`/`broadcast.rs:479-498` not mentioned; dnas probe uses constants today — see #3).
18. `package-lock.json:3` still `1.4.0` while the other three version files say 1.5.0 (the 1.5.1 bump missed it too — pre-existing; `npm install` re-syncs).
19. `update-worker/worker.test.mjs:3` hardcodes `/home/streamy-desktop/update-worker/worker.js` (machine-specific import in a committed file) and no CI step runs the worker tests (8/8 pass locally). Relative import + a 3-line CI job.
20. MSRV `rust-version = "1.77.2"` declared but nothing builds at MSRV (workflows use stable; no rust-toolchain file). Advisory-only today.

**Quality / dead code** (all verified zero-callers)
21. `engine/resample.rs` (`resample_linear`) — dead module (~77 LOC + tests); deck/mic use inline linear interp.
22. Dead default-input path: `engine.rs:379-381` `open_default_input`, `mic.rs:46-50` `open_default`; UI has no "System default" mic option (adding one would also relieve #4).
23. `update::first_paragraph` (`update.rs:41-50`) — own-test-only; worker computes notes.
24. `Eq3Band::gains_db` (`eq.rs:151-153`) — fully dead.
25. Duplication map: JSON store boilerplate ×3 (`settings.rs:55-64,120-126`, `store.rs:65-81`, `playlist/model.rs:48-67` → one `load_json_or_default`/`save_json` in `diag.rs`); frontend `m:ss` formatter ×3 (`NowPlaying:9`, `Playlist:16`, `RecordBar:39`); ring-drain→encode→sleep(5ms) loop ×2 (`broadcast.rs:203-235`, `record/mod.rs:94-118` — extractable, optional); connect timeout wrapper ×2 (`broadcast.rs:383-409`); Rust test `tempdir()` ×2 (`settings.rs:133`, `store.rs:186`).
26. Misc: `BroadcastBar.svelte:6-8` unguarded `localStorage` read (WelcomeModal guards — align); codec-agnostic path named `mp3` (`broadcast.rs:83,195`); redundant `trim_end_matches('\0').trim_end_matches('\u{0}')` (`ultravox.rs:96-99`); split two-line imports ×5 components; recorder hardcodes 192 kbps MP3 regardless of UI bitrate (`commands.rs:636-641`, harmless for WAV).

### 4.5 INFO / accepted risks

- **Cleartext credentials on all stream protocols** (protocol-inherent): Icecast Basic auth over plain TCP (`icy.rs:36,79,102`); Ultravox XTEA password trivially decryptable because the key arrives cleartext in the same stream (`ultravox.rs:157-168`). No TLS option exists in practice. Passive observer recovers the password. Mitigation is user-side (tunnel/VPN); document in README someday.
- **Hostile-server threat model** — verified bounded: `read_frame` u16-capped 64K (`ultravox.rs:72-91`), `read_http_head` hard-capped 4096 (`icy.rs:133-147`), 15 s/4 s handshake timeouts (`broadcast.rs:383-429`). A hostile server can only produce errors/timeouts, no memory-safety or config write-back path.
- **XTEA is textbook Wheeler–Needham** (32 rounds, correct delta/key-indexing; golden vectors); LAME FFI buffer math meets documented minimums (`mp3.rs:99-107` `n*1.25+7200+16`, fixed 4410-frame chunks); hand-rolled base64 is correct RFC 4648 with vectors (`icy.rs:184-221`). `aac.rs` has no unsafe.
- **Decode + file I/O on the realtime thread** — prior #3.2, unchanged, deliberately deferred (documented exception, `engine.rs:56-62`).
- **Old deck keeps decoding** after a track-change crossfade (prior #3.5-7, still open — wasted CPU, bounded). **Per-frame transcendentals** (prior #3.5-8, still open — perf only).
- `ConnectionProfile` derives `Debug` incl. `plaintext_pw` (`store.rs:7-8`) — latent only; grep confirms no `{:?}` site logs a profile. Redact when next touched.
- Stale keyring can shadow updated plaintext (`store.rs:166-179`): keyring holds PW1, plaintext PW2 → reads return PW1. Edge-case wrong-password connect. On plaintext fallback also try keyring delete.
- Suspected, unconfirmed: MP3s lacking `n_frames` get `total_secs = 0.0` (`decoder.rs:52-55`) → no position events, seek always to start (`deck.rs:108`). Existing tests use `position()` conditionally so would not catch it. Worth a probe test.
- `resample_linear` dead (see #21); MP3 path quantizes without `.round()` (WAV rounds); deck position briefly snaps back after natural end (≤2 s); mid-session reload while *paused* shows "playing" (variant of prior #3.5-3, which fixed the nothing-loaded case).
- No LICENSE (`Cargo.toml:6` empty) — fine for a private repo; decide before any public release.
- Untracked `docs/tutorial-pengguna.md` is an Indonesian ops manual for **Streamy Panel** (a different product; its console displaying the source password is that panel's feature, not a leak). Recommendation: move to the Streamy Panel repo; do not commit here.
- AAC ships in the Windows `.exe` (`default = ["aac"]`, workflow builds defaults); both cfg branches compile with 0 warnings (`cargo check` and `--no-default-features --tests`). The long-standing note that AAC is OFF in Windows CI is stale history.

---

## 5. Prior-audit re-verification (`docs/plans/2026-09-07-code-audit.md`)

| Prior finding | Disposition @ 0407a99 | Evidence |
|---|---|---|
| #3.1 HIGH mic never resampled | **Confirmed-fixed** | `engine.rs:173-185` fractional mic position; 4 tests: `mic_resampler_consumes_at_output_rate` (`engine.rs:847`), upsample ratio (`:688`), same-rate passthrough (`:712`), starvation (`:742`) — all pass |
| #3.2 MED decode on audio thread | **Still-open** (deliberately deferred, documented) | `engine.rs:546` under lock; exception documented `engine.rs:56-62` |
| #3.3 MED recording double-start | **Partial → new bug** | Double-start guard works (`commands.rs:601-607`, test `double_start_rejected_at_entry`). Concurrent-stop half is **broken**: dead `Idle` arms (`:668`, `:472`) + defective regression test (finding #2) |
| #3.4 MED ring drops newest | **Confirmed-fixed** | `ring.rs:42-44` `force_push` (drop-oldest); eviction tests `ring.rs:68-78,95-113` |
| #3.5-1 encoder rate fallback | Superseded by new **HIGH** #1 (mid-session variant) | `engine.rs:431-437` vs `commands.rs:440-445` |
| #3.5-2 playlist dupes | **Confirmed-fixed** | `playlist/model.rs:31-38` + `add_skips_duplicate_paths` (`:127`) |
| #3.5-3 reload "playing" nothing loaded | **Confirmed-fixed** (minor paused-reload variant remains, INFO) | `model.rs:58-72`, `App.svelte:81-88` |
| #3.5-4 error retry spam | **Confirmed-fixed** | `engine.rs:449-463` distinct-message dedupe |
| #3.5-5 non-atomic writes | **Confirmed-fixed** | `diag.rs:43-94` + 8-thread stress test (`:122-154`) |
| #3.5-6 CRLF in ICY headers | **Still-open** (deliberate then, still open now) | finding #7 |
| #3.5-7 old deck never stopped | **Still-open** (INFO) | `engine.rs:539-553,151-152` |
| #3.5-8 per-frame transcendentals | **Still-open** (INFO) | `mixer.rs:19-22,34`, `engine.rs:166` |
| #3.5-9 port NaN/undefined | **Moot** | serde rejects NaN; volume/crossfade/seek clamped; only set_eq dB unclamped (bounded UI sliders) |
| §3.7 "creds never committed" | **REGRESSION** | `dnas_aac_dj_probe.rs:13-17` (finding #3) — file postdates the audit |
| Remediation: CI jobs | **Confirmed** | Both workflows verified green on this commit; Windows runs `cargo test --locked` before packaging |
| Remediation: EventEmitter seam, Wine fixes, write_atomic retries | **Confirmed** (spot-verified this session) | `broadcast.rs:25`, `diag.rs:55-66` |
| Remediation #3.5-1 claims "unit: flag set on stream open; commands return the guard error pre-open" | **Test MISSING** — no such test exists in the tree (`tests/commands.rs` covers title/PE/RecState-mirror only) | grep-verified |

---

## 6. Test coverage assessment

**Measured:** 126 test functions — 119 pass / 7 ignored-live (integration 44 incl. 5 ignored; inline `#[cfg(test)]` 82 incl. 2 ignored). The ignored count splits: 2 inline live probes (`icy.rs`, `broadcast.rs`), 4 in `stream_live.rs`, 1 dnas probe. 15 of 31 src files have zero inline tests — notably `src/commands.rs` (the real `BcastState`/`RecState` guards are never executed by any test) and `encode/*`. Frontend: **zero tests**, no runner even in devDependencies (design doc promised Vitest). CI runs no worker tests (LOW #19).

**Strong coverage (verified, asserts read):** XTEA golden + 5 multiblock vectors; Ultravox wire-byte round-trip and auth payload shapes; ICY source_request shape + handshake + payload byte-identity + 401 rejection; reconnect with a mock speaking real Ultravox framing (via the lib's own `encode_frame`/`read_frame`); encoder-init failure surfacing; recorder MP3 **byte-identical to direct encode** (strongest in suite); canonical WAV header; s16le clamp; clamp_title UTF-8 boundary; profile CRUD incl. keyring branch + keep-password-on-edit + legacy JSON + azuracast flag; `write_atomic` 8×50 concurrent-writer stress; corrupt/missing-JSON recovery ×3 stores; deck stream/end/seek/bad-path + bounded memory; mic resampler ×4 (ratio, passthrough, starvation); ring eviction ×5; EQ shelf isolation; equal-power crossfade at 101 points; semver compare; aac ADTS sync/size.

**Weak tests (pass but prove little — read the asserts):**
- `tests/commands.rs::rec_state_protocol` — runs a **local copy** `enum St`, not production code; 3 of its 6 tests are tautological (`double_start_rejected_at_entry` matches constants against each other; `every_interleaving…` exhausts the only 3 variants; `rec_state_pub_enum_shape_unchanged` matches a variant on itself); `stop_during_setup_honored` never chains stop's output into commit (finding #2). A real guard regression would NOT be caught.
- Zero-assert "tests": dnas probe (println only — also finding #3), and the two ignored live probes `probe_ultravox_handshake_live` / `stream_and_verify_live` (println verdicts only; only the 3rd/4th live tests assert).
- `stream_live.rs`'s two non-ignored tests unit-test the **test helpers themselves** (`http_get`, XML scraper) — infra self-testing, not product coverage.
- `mixer.rs::mic_never_reaches_monitor…` loop body is `let _ = mode;` — structural no-op.
- `ring.rs` slow-consumer stress comment promises a "strictly increasing" order assert that does not exist.
- Ignored live tests **silently skip** on missing env vars — `-- --ignored` looks green while testing nothing.

**Gaps (risk → test to add):**
- **HIGH** — Ultravox→Icecast **fallback ordering** (`connect_source`, `broadcast.rs:411`) — post-revert THE protocol path — has zero offline coverage (only the ignored live `auto_detect_resolves_icecast_on_harbor`). Add a dual local-listener test: Ultravox port rejects, Icecast accepts → assert `Source::Icecast`.
- **HIGH** — `start_recording` guard + `BcastState` commit/rollback (`commands.rs:367-482,603`) never executed; extract the commit protocol into pure src functions, unit-test the real ones, delete the mirror enum (this is also the durable fix for #2's test).
- **HIGH** — frontend: add vitest + store smoke tests, wire `npm test` into `test.yml`.
- **MED** — device-rate change during broadcast/record (would catch #1); encoder flush-tail/odd-chunk math (`encode/` has zero unit tests — truncated-audio class); real-path stop-during-connect (mock drop during handshake in `stream_reconnect.rs`).
- **MED** — playlist bad-UTF-8 path / huge list edges (JSON corruption is covered; these aren't).
- **LOW** — CRLF profile fields (would catch #7); WAV u32-wrap header math (unit); reconnect backoff monotonic gaps (generous bounds); meter dBFS conversion.
- **CI matrix note (live-verified this session):** `cargo check --no-default-features --tests` passes clean today — the `Codec::Aac` usage in the dnas probe compiles because the variant exists unconditionally (one pass's contrary claim was disproven by the run) — but no workflow ever builds aac-off, so nothing guards that state.

**Infrastructure:** mocks bind `127.0.0.1:0` (no port flake); best mock (reconnect) speaks real framing but ACKs any control frame and skips auth; sync is polling+sleeps with Wine-tuned constants (documented); no shared `tests/common` helpers — `http_get` ×2, XML scraper ×2, 440 Hz sine inlined ×5. No concurrency-cancel groups or npm cache in workflows; `build-windows.yml` has no PR trigger.

---

## 7. Security posture

**Strong:** minimal Tauri capabilities (`core/dialog/opener:default` only, verified used legitimately); CSP `'self'` everywhere with only inline styles; zero XSS sinks in the frontend; password never returned via `list()` (stripped), never in events or logs; keyring-first with verify-after-set; atomic persistence; protocol parsers bounded and timeout-guarded against hostile servers; FFI buffer math correct; opener URL scope verified against crate source (arbitrary schemes rejected); no secrets in the Worker (runtime secret) or binary.

**Top fixes by value** (cheap → dear): `0600` on profiles.json (one line); strip CRLF at `save_profile` (one filter); move dnas-probe creds to env + rotate; cap WV2 download size (one check); Authenticode/hash pin before spawn; `create_new` for recordings.

**Inherent/accepted:** cleartext source credentials on all protocols (no TLS in practice — user-side tunnel is the mitigation); hostile-server bounded-response risk; XTEA = obfuscation not secrecy; plaintext fallback existence (perms are fixable, the fallback itself is a deliberate availability trade-off).

---

## 8. Remediation table

| # | Sev | Finding (file:line) | Suggested fix | Effort | Regression test to add |
|---|---|---|---|---|---|
| 1 | HIGH | Encoder rate frozen (`engine.rs:431`, `commands.rs:440`) | re-init encoder on rebuild rate change (or block rebuild while live) | M | rebuild at 48k mid-stream → encoded rate matches new out_rate |
| 2 | MED | Lost stop/disconnect (`commands.rs:472,505,668,705`) | stop sets Idle on Connecting/Starting; fix comments | S | **fix the defective model test first**, then real-interleaving test |
| 3 | MED | Hardcoded creds (`dnas_aac_dj_probe.rs:13-17`) | env vars + rotate password | S | n/a (hygiene) |
| 4 | MED | Silent mic failure (`mic.rs:39,86,125`) | open-status + error event | M | set bad device → error event, no persist |
| 5 | MED | `play()` ignored (`engine.rs:441`) | propagate; has_output on success only | S | unit: play-fail → has_output false |
| 6 | MED | World-readable fallback (`diag.rs:62`) | mode 0600 on Unix | S | stat perms after save |
| 7 | MED | CRLF injection (`icy.rs:104`) | strip CR/LF/NUL at save_profile boundary | S | profile with \r\n → request has no smuggled header |
| 8 | MED | WV2 no signature pin (`commands.rs:798`) | Authenticode or SHA-256 pin + size cap | M | non-pinned bytes → refuse |
| 9 | MED | Zero-frame stall (`deck.rs:186`) | set just_ended on early path | S | 0-byte fixture → track_ended fires |
| 10 | MED | Monitor volume not restored (`lib.rs:101`) | one line in setup | S | n/a (manual) |
| 11 | MED | Insecure default ctor (`store.rs:41`) | delete `new` | S | n/a |
| 12 | MED | Global EQ vs design (`engine.rs:564`) | per-deck gains or collapse + doc update | M | per-deck gains differ |
| L1-26 | LOW | see §4.4 | per-item | S each | per-item |

---

## 9. Recommended priorities

1. **#2 + its test** — smallest correctness debt, and the existing "proof" is a lie the suite tells every run; fix model chain → red → fix code → green.
2. **#1** — the only HIGH; user-visible corruption on plausible hardware (Bluetooth/dock hot-swap).
3. **Security cheap set** — #6 (one line), #7 (one filter), #3 (env + rotate), #11 (delete), #14 (gitignore) — one short commit, closes every MED that costs under an hour.
4. **Device-lifecycle honesty** — #5, L1, #4: one theme, one PR (propagate play errors, reset has_output, mic open status).
5. **#9, #10** — two tiny UX-visible correctness fixes.
6. Docs truth pass — L15-18 (README counts/port/path/env, lockfile sync, worker CI + relative import).
7. Dead-code + duplication sweep (L21-25) — mechanical, ~200 LOC deleted.
8. Keep deferred: decode-off-thread (#3.2), per-frame transcendentals, reconnect cap decision, per-deck EQ product decision (#12) — schedule or record the cut in the design doc.

---

*Audit performed 2026-10-24 at commit `0407a99` (1.5.0 tree). Method: four parallel review passes + direct verification of every HIGH/MED finding and all prior-audit dispositions in the main session; all counts quoted are from live runs on this machine (Linux, headless). Windows CI green with artifact `streamy-desktop-windows` (16.25 MB); runtime/audio behavior on real hardware remains user-verified, as on every prior audit.*
