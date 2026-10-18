// P2.2: broadcast path — encoder thread pulls the broadcast-bus PCM ring, encodes
// to MP3 with one stateful lame handle, and a tokio streamer task ships frames
// to the server. Realtime pacing is inherent: the audio callback pushes at
// realtime, so the whole chain is throttled to realtime (no bursting).
//
// The streamer is protocol-agnostic via the `Source` enum: Ultravox (DNAS2)
// sends binary 0x5A data frames with in-band metadata, Icecast/Liquidsoap-harbor
// streams raw codec bytes and pushes metadata over a separate admin HTTP GET.
//
// P2.7: the streamer owns a reconnect loop. On a mid-stream socket error it emits
// Disconnected → Connecting, waits with exponential backoff, re-handshakes, and
// resumes. The initial connect is still surfaced to the caller (Connect button).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
/// UI event sink. The broadcast core is UI-free; the app passes a closure
/// that forwards to the Tauri event bus. This keeps `tauri` (and its muda/
/// comctl32-v6 webview deps) out of pure-network test binaries — without
/// this seam, test exes imported TaskDialogIndirect (comctl32 v6 only) and
/// died with STATUS_ENTRYPOINT_NOT_FOUND on real Windows (Wine's comctl32
/// exports it unconditionally, masking the bug).
pub type EventEmitter = Arc<dyn Fn(&str, serde_json::Value) + Send + Sync>;

use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::diag;
use crate::encode::{self, EncoderConfig};
use crate::engine::ring::StereoRingConsumer;
use crate::events::{self, StreamStats};
use crate::stream::icy::IcySourceConfig;
use crate::stream::sc_v1::ScV1Config;
use crate::stream::ultravox::{self, UltravoxConfig};

/// MP3 must be sent in chunks under the Ultravox max payload (16377).
const MAX_PAYLOAD: usize = 16_000;
/// Encode 0.1s of stereo at a time (44100 Hz).
const ENCODE_FRAMES: usize = 4410;

/// Default reconnect backoff: start here, double each failure, cap at max.
const BACKOFF_BASE: Duration = Duration::from_millis(500);
const BACKOFF_MAX: Duration = Duration::from_secs(30);
const HANDSHAKE_TIMEOUT_SECS: u64 = 15;

#[derive(Serialize, Clone)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum StreamStatus {
    Connecting,
    Connected,
    Disconnected,
    Error { message: String },
}

/// Connection parameters for the source protocol. `Auto` tries SHOUTcast v1
/// (ICY) first — DNAS1 and DNAS2 both accept it — then Ultravox 2.1 (DNAS2,
/// needed for DJ login / sid > 1), then the Icecast HTTP SOURCE handshake
/// (Icecast 2.x, Liquidsoap harbor). Built from a `ConnectionProfile` +
/// resolved password in `connect_stream`.
#[allow(clippy::large_enum_variant)] // one instance per connect; see ponytail below
pub enum StreamConfig {
    Ultravox(UltravoxConfig),
    Icecast(IcySourceConfig),
    ScV1(ScV1Config),
    Auto {
        sc_v1: ScV1Config,
        ultravox: UltravoxConfig,
        icecast: IcySourceConfig,
    },
}
// ponytail: unboxed Auto (~500B) is fine — one instance per connect, moved
// once into the streamer task. Box if it ever lands in a hot path.

/// A live source connection owning its transfer socket. Ultravox keeps
/// everything on one socket (metadata is in-band); Icecast holds the audio
/// socket and remembers the admin cfg for metadata GETs.
#[allow(clippy::large_enum_variant)] // both variants socket-sized; no boxing win
enum Source {
    Ultravox(TcpStream),
    ScV1 {
        stream: TcpStream,
        admin: ScV1Config,
    },
    Icecast {
        stream: TcpStream,
        admin: IcySourceConfig,
    },
}

/// Admin metadata channel for sources whose title updates ride a separate
/// connection (Icecast /admin/metadata, SHOUTcast v1 admin.cgi). Ultravox is
/// absent because its metadata is in-band on the audio socket.
#[derive(Clone)]
pub enum AdminMetadata {
    Icecast(IcySourceConfig),
    ScV1(ScV1Config),
}

impl AdminMetadata {
    pub async fn update(&self, title: &str) -> Result<(), String> {
        match self {
            AdminMetadata::Icecast(cfg) => crate::stream::icy::send_metadata(cfg, title).await,
            AdminMetadata::ScV1(cfg) => crate::stream::sc_v1::send_metadata(cfg, title).await,
        }
    }
}

impl Source {
    async fn send_audio(&mut self, mp3: &[u8]) -> Result<(), String> {
        match self {
            Source::Ultravox(s) => ultravox::send_mp3(s, mp3).await,
            Source::ScV1 { stream, .. } => {
                stream.write_all(mp3).await.map_err(|e| e.to_string())
            }
            Source::Icecast { stream, .. } => {
                stream.write_all(mp3).await.map_err(|e| e.to_string())
            }
        }
    }

    async fn send_metadata(&mut self, title: &str) -> Result<(), String> {
        match self {
            Source::Ultravox(s) => ultravox::send_metadata(s, title).await,
            Source::ScV1 { admin, .. } => {
                crate::stream::sc_v1::send_metadata(admin, title).await
            }
            Source::Icecast { admin, .. } => crate::stream::icy::send_metadata(admin, title).await,
        }
    }

    /// Whether a metadata-send error means the audio socket itself is dead and
    /// must be reconnected. Ultravox sends metadata in-band (an error = socket
    /// dead); Icecast/ScV1 metadata is a separate HTTP GET (non-fatal).
    fn metadata_fatal(&self) -> bool {
        matches!(self, Source::Ultravox(_))
    }
}

pub struct BroadcastHandle {
    stop: Arc<AtomicBool>,
    streamer: JoinHandle<()>,
    pub metadata_tx: mpsc::Sender<String>,
    /// Resolved admin cfg when the live source updates titles over a separate
    /// connection (Icecast `/admin/metadata`, SHOUTcast v1 `admin.cgi`) — lets
    /// `set_stream_title` push metadata direct, independent of the audio
    /// streamer task. `None` for Ultravox (metadata is in-band on the audio
    /// socket, so it goes through `metadata_tx`).
    pub admin_metadata: Option<AdminMetadata>,
}

/// Connect and run the encoder + streamer against `rx` (broadcast PCM),
/// reconnecting with default backoff on mid-stream drops.
pub async fn start(
    rx: StereoRingConsumer,
    cfg: StreamConfig,
    enc_cfg: EncoderConfig,
    app: Option<EventEmitter>,
) -> Result<BroadcastHandle, String> {
    start_with_opts(
        rx,
        cfg,
        enc_cfg,
        app,
        BACKOFF_BASE,
        BACKOFF_MAX,
        HANDSHAKE_TIMEOUT_SECS,
    )
    .await
}

/// As `start` but with explicit backoff/timeout — used by tests to keep the
/// reconnect dance fast and deterministic.
pub async fn start_with_opts(
    rx: StereoRingConsumer,
    cfg: StreamConfig,
    enc_cfg: EncoderConfig,
    app: Option<EventEmitter>,
    backoff_base: Duration,
    backoff_max: Duration,
    handshake_timeout_secs: u64,
) -> Result<BroadcastHandle, String> {
    emit_status(&app, StreamStatus::Connecting);

    // Build the encoder up front: a codec init failure (e.g. fdk-aac not
    // available) must surface to the caller instead of silently killing the
    // encoder thread after the UI already shows ON AIR with zero audio.
    let enc = match encode::build(&enc_cfg) {
        Ok(e) => e,
        Err(e) => {
            emit_status(&app, StreamStatus::Error { message: e.clone() });
            return Err(e);
        }
    };

    // Initial connect: failure here is returned to the caller (Connect button
    // shows it). Mid-stream drops are handled by the reconnect loop below.
    let mut cfg = cfg;
    let mut source = match connect_source(&cfg, handshake_timeout_secs).await {
        Ok(s) => s,
        Err(e) => {
            diag::log(format!("initial connect failed: {e}"));
            emit_status(&app, StreamStatus::Error { message: e.clone() });
            return Err(e);
        }
    };
    // Latch Auto → concrete protocol so reconnects use the resolved path and
    // don't re-probe (and so we know which metadata path to use).
    if matches!(cfg, StreamConfig::Auto { .. }) {
        let latched = match (&cfg, &source) {
            (StreamConfig::Auto { ultravox, .. }, Source::Ultravox(_)) => {
                StreamConfig::Ultravox(ultravox.clone())
            }
            (StreamConfig::Auto { icecast, .. }, Source::Icecast { .. }) => {
                StreamConfig::Icecast(icecast.clone())
            }
            (StreamConfig::Auto { sc_v1, .. }, Source::ScV1 { .. }) => {
                StreamConfig::ScV1(sc_v1.clone())
            }
            _ => unreachable!("Auto must resolve to a concrete source"),
        };
        cfg = latched;
    }
    // Icecast/v1 metadata goes direct (separate HTTP GET); Ultravox is in-band.
    let admin_metadata = match &source {
        Source::Icecast { admin, .. } => Some(AdminMetadata::Icecast(admin.clone())),
        Source::ScV1 { admin, .. } => Some(AdminMetadata::ScV1(admin.clone())),
        _ => None,
    };
    emit_status(&app, StreamStatus::Connected);

    let stop = Arc::new(AtomicBool::new(false));
    let (mp3_tx, mut mp3_rx) = mpsc::channel::<Vec<u8>>(64);
    let (metadata_tx, mut metadata_rx) = mpsc::channel::<String>(16);

    // Encoder thread: ring -> encoder -> MP3/AAC chunks (paced by realtime
    // producer). During a reconnect the channel fills and blocking_send
    // backpressures this thread (and in turn the audio ring) — a brief pause,
    // not a stall. The encoder was built above so init failures are surfaced.
    let stop_enc = stop.clone();
    std::thread::spawn(move || {
        let mut enc = enc;
        let mut buf: Vec<f32> = Vec::with_capacity(ENCODE_FRAMES * 2);
        loop {
            if stop_enc.load(Ordering::Relaxed) {
                break;
            }
            while let Some(frame) = rx.pop() {
                buf.push(frame[0]);
                buf.push(frame[1]);
                if buf.len() >= ENCODE_FRAMES * 2 {
                    break;
                }
            }
            if buf.len() >= ENCODE_FRAMES * 2 {
                if let Ok(mp3) = enc.encode(&buf) {
                    push_chunks(&mp3_tx, &mp3);
                }
                buf.clear();
            } else {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        // Drain remainder + flush on stop.
        if !buf.is_empty() {
            if let Ok(mp3) = enc.encode(&buf) {
                push_chunks(&mp3_tx, &mp3);
            }
        }
        if let Ok(mp3) = enc.flush() {
            push_chunks(&mp3_tx, &mp3);
        }
    });

    // Streamer task: MP3 chunks -> server; metadata on demand. On socket error,
    // reconnect with growing backoff and resume.
    let stop_str = stop.clone();
    let streamer = tokio::spawn(async move {
        // Per-session upload counter: fresh per start_with_opts, survives
        // reconnects (run_stream accumulates into it between drops).
        let mut total_bytes = 0u64;
        loop {
            if stop_str.load(Ordering::Relaxed) {
                break;
            }
            let dropped = run_stream(
                &mut source,
                &mut mp3_rx,
                &mut metadata_rx,
                &stop_str,
                &app,
                &mut total_bytes,
            )
            .await;
            if stop_str.load(Ordering::Relaxed) {
                break;
            }
            if !dropped {
                break; // clean stop or encoder dropped its sender
            }
            // Mid-stream drop: announce and reconnect with backoff.
            emit_status(&app, StreamStatus::Disconnected);
            emit_status(&app, StreamStatus::Connecting);
            let mut delay = backoff_base;
            loop {
                if stop_str.load(Ordering::Relaxed) {
                    break;
                }
                if sleep_interruptible(delay, &stop_str).await {
                    break;
                }
                // Race the handshake against stop so Disconnect returns
                // promptly even when a reconnect attempt is mid-flight against
                // an unresponsive server (otherwise stop() blocks up to the
                // handshake timeout).
                let outcome = tokio::select! {
                    r = connect_source(&cfg, handshake_timeout_secs) => Some(r),
                    _ = wait_for_stop(&stop_str) => None,
                };
                match outcome {
                    Some(Ok(s)) => {
                        source = s;
                        emit_status(&app, StreamStatus::Connected);
                        break;
                    }
                    Some(Err(e)) => {
                        diag::log(format!("reconnect failed: {e}"));
                        delay = (delay * 2).min(backoff_max);
                    }
                    None => break, // stop requested mid-handshake
                }
            }
        }
    });

    Ok(BroadcastHandle {
        stop,
        streamer,
        metadata_tx,
        admin_metadata,
    })
}

/// Stream until the socket errors (returns `true` → caller reconnects) or stop is
/// requested / the encoder dropped its sender (returns `false`). Icecast
/// metadata failures are logged but do not tear down the audio socket. Emits
/// per-second upload stats (`stream_stats`) while streaming; `total_bytes` is
/// the per-session counter shared with the caller across reconnects.
async fn run_stream(
    source: &mut Source,
    mp3_rx: &mut mpsc::Receiver<Vec<u8>>,
    metadata_rx: &mut mpsc::Receiver<String>,
    stop: &Arc<AtomicBool>,
    app: &Option<EventEmitter>,
    total_bytes: &mut u64,
) -> bool {
    let mut last_bytes = 0u64;
    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        tokio::select! {
            _ = ticker.tick() => {
                emit_stats(app, *total_bytes, last_bytes);
                last_bytes = *total_bytes;
            },
            chunk = mp3_rx.recv() => match chunk {
                Some(c) => {
                    *total_bytes += c.len() as u64;
                    if source.send_audio(&c).await.is_err() {
                        return true;
                    }
                }
                None => return false,
            },
            title = metadata_rx.recv() => {
                if let Some(t) = title {
                    if let Err(e) = source.send_metadata(&t).await {
                        if source.metadata_fatal() {
                            return true;
                        }
                        diag::log(format!("metadata update failed (non-fatal): {e}"));
                    }
                }
            }
        }
    }
}

/// Sleep for `d`, ticking every 100ms so `stop` is observed promptly. Returns
/// true if stop was requested during the wait.
async fn sleep_interruptible(d: Duration, stop: &Arc<AtomicBool>) -> bool {
    let mut remaining = d;
    let tick = Duration::from_millis(100);
    while remaining > Duration::ZERO {
        if stop.load(Ordering::Relaxed) {
            return true;
        }
        let step = remaining.min(tick);
        tokio::time::sleep(step).await;
        remaining -= step;
    }
    false
}

/// Resolves once `stop` is set. Used to race a blocking handshake so that
/// `BroadcastHandle::stop()` returns promptly even when a reconnect is in flight.
async fn wait_for_stop(stop: &Arc<AtomicBool>) {
    let tick = Duration::from_millis(100);
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        tokio::time::sleep(tick).await;
    }
}

/// Run the Ultravox handshake with a timeout cap.
async fn connect_ultravox(c: &UltravoxConfig, timeout_secs: u64) -> Result<Source, String> {
    let s =
        match tokio::time::timeout(Duration::from_secs(timeout_secs), ultravox::connect(c)).await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err(format!("connect timed out after {timeout_secs}s")),
        };
    Ok(Source::Ultravox(s))
}

/// Run the Icecast handshake with a timeout cap.
async fn connect_icecast(c: &IcySourceConfig, timeout_secs: u64) -> Result<Source, String> {
    let s = match tokio::time::timeout(
        Duration::from_secs(timeout_secs),
        crate::stream::icy::connect(c),
    )
    .await
    {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return Err(e),
        Err(_) => return Err(format!("connect timed out after {timeout_secs}s")),
    };
    Ok(Source::Icecast {
        stream: s,
        admin: c.clone(),
    })
}

/// Run the SHOUTcast v1 handshake with a timeout cap.
async fn connect_sc_v1(c: &ScV1Config, timeout_secs: u64) -> Result<Source, String> {
    let s = match tokio::time::timeout(
        Duration::from_secs(timeout_secs),
        crate::stream::sc_v1::connect(c),
    )
    .await
    {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return Err(e),
        Err(_) => return Err(format!("connect timed out after {timeout_secs}s")),
    };
    Ok(Source::ScV1 {
        stream: s,
        admin: c.clone(),
    })
}

/// Run the source handshake with a cap so a silent server can't hang Connect.
/// `Auto` tries SHOUTcast v1 first (DNAS1 + DNAS2 both accept it — the
/// classic source handshake), then Ultravox 2.1 (DNAS2 only; needed when a
/// DJ account or stream id > 1 is configured), then Icecast/Liquidsoap.
async fn connect_source(cfg: &StreamConfig, timeout_secs: u64) -> Result<Source, String> {
    match cfg {
        StreamConfig::Ultravox(c) => connect_ultravox(c, timeout_secs).await,
        StreamConfig::Icecast(c) => connect_icecast(c, timeout_secs).await,
        StreamConfig::ScV1(c) => connect_sc_v1(c, timeout_secs).await,
        StreamConfig::Auto {
            sc_v1,
            ultravox,
            icecast,
        } => {
            // v1 first: DNAS1 only speaks it and DNAS2 still accepts it, so
            // this is the default source handshake. An Icecast/Liquidsoap
            // server answers the password line with an HTTP 4xx error line
            // (not "OK2") and drops the socket — fast rejection, so the
            // fallback is immediate.
            let probe_secs = timeout_secs.min(4);
            if let Ok(s) = connect_sc_v1(sc_v1, probe_secs).await {
                return Ok(s);
            }
            // Ultravox next (DNAS2): needed when the profile carries a DJ
            // account (user) or a stream id that v1 can't select. Non-Ultravox
            // servers reject the binary handshake fast (no 0x5A sync).
            if let Ok(s) = connect_ultravox(ultravox, probe_secs).await {
                return Ok(s);
            }
            connect_icecast(icecast, timeout_secs).await
        }
    }
}

fn push_chunks(tx: &mpsc::Sender<Vec<u8>>, mp3: &[u8]) {
    for chunk in mp3.chunks(MAX_PAYLOAD) {
        if tx.blocking_send(chunk.to_vec()).is_err() {
            return;
        }
    }
}

impl BroadcastHandle {
    pub async fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.streamer.await;
    }
}

fn emit_status(app: &Option<EventEmitter>, status: StreamStatus) {
    if let Some(a) = app {
        a(
            events::STREAM_STATUS,
            serde_json::to_value(status).unwrap_or_default(),
        );
    }
}

fn kbps_from_delta(bytes: u64) -> f64 {
    (bytes * 8) as f64 / 1000.0
}

fn emit_stats(app: &Option<EventEmitter>, total_bytes: u64, last_bytes: u64) {
    if let Some(a) = app {
        let v = serde_json::to_value(StreamStats {
            kbps: kbps_from_delta(total_bytes.saturating_sub(last_bytes)),
            total_bytes,
        })
        .unwrap_or_default();
        a(events::STREAM_STATS, v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::icy::IcySourceConfig;
    use crate::stream::ultravox::UltravoxConfig;
    use std::env;

    fn envs() -> Option<(String, u16, String, String, String)> {
        let host = env::var("STREAMY_ICE_HOST").ok()?;
        let port: u16 = env::var("STREAMY_ICE_PORT").ok()?.parse().ok()?;
        let user = env::var("STREAMY_ICE_USER").ok()?;
        let pass = env::var("STREAMY_ICE_PASS").ok()?;
        let mount = env::var("STREAMY_ICE_MOUNT").unwrap_or_else(|_| "/".into());
        Some((host, port, user, pass, mount))
    }

    fn sc_v1_from_env() -> Option<ScV1Config> {
        Some(ScV1Config {
            host: env::var("STREAMY_V1_HOST").ok()?,
            port: env::var("STREAMY_V1_PORT").ok()?.parse().ok()?,
            password: env::var("STREAMY_V1_PASS").ok()?,
            icy_name: "Streamy v1 auto test".into(),
            icy_genre: "Test".into(),
            icy_url: "".into(),
            icy_pub: true,
            icy_br: env::var("STREAMY_V1_BR")
                .ok()
                .and_then(|b| b.parse().ok())
                .unwrap_or(128),
        })
    }

    /// Live: Auto-detect against a SHOUTcast DNAS must accept the v1 password
    /// handshake first and resolve to the ScV1 source (never probing Ultravox
    /// or Icecast). Creds via env, never committed.
    ///   cargo test --lib auto_detect_prefers_v1_on_dnas -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn auto_detect_prefers_v1_on_dnas() {
        let sc_v1 = match sc_v1_from_env() {
            Some(c) => c,
            None => {
                eprintln!("skip: set STREAMY_V1_HOST/PORT/PASS");
                return;
            }
        };
        let cfg = StreamConfig::Auto {
            sc_v1,
            ultravox: UltravoxConfig {
                host: "unreachable.invalid".into(),
                port: 1,
                stream_id: 1,
                password: "x".into(),
                user: String::new(),
                content_type: "audio/mpeg".into(),
                bitrate_kbps: 128,
                name: "Streamy".into(),
                genre: "".into(),
                url: "".into(),
            },
            icecast: IcySourceConfig {
                host: "unreachable.invalid".into(),
                port: 1,
                mount: "/stream".into(),
                user: "x".into(),
                password: "x".into(),
                content_type: "audio/mpeg".into(),
                icy_name: "Streamy".into(),
                icy_genre: "".into(),
                icy_url: "".into(),
                icy_pub: true,
                icy_br: 128,
                sample_rate: 44100,
            },
        };
        let mut src = connect_source(&cfg, 15).await.expect("v1-first auto connect");
        assert!(
            matches!(src, Source::ScV1 { .. }),
            "auto-detect must resolve to SHOUTcast v1 on a DNAS"
        );

        // Exercise the audio write path with a real MP3 chunk.
        let br = if let StreamConfig::Auto { sc_v1, .. } = &cfg {
            sc_v1.icy_br
        } else {
            128
        };
        let enc_cfg = crate::encode::EncoderConfig {
            sample_rate: 44100,
            channels: 2,
            bitrate_kbps: br,
            codec: crate::encode::Codec::Mp3,
        };
        let mut enc = crate::encode::build(&enc_cfg).expect("encoder");
        let two_pi = 2.0 * std::f64::consts::PI;
        let mut samples = Vec::with_capacity(4410 * 2);
        for i in 0..4410usize {
            let v = (two_pi * 440.0 * (i as f64 / 44100.0)).sin() as f32 * 0.5;
            samples.push(v);
            samples.push(v);
        }
        let mp3 = enc.encode(&samples).expect("encode");
        src.send_audio(&mp3).await.expect("audio write");

        // Direct admin.cgi metadata update (set_stream_title's path for v1).
        if let Source::ScV1 { admin, .. } = &src {
            crate::stream::sc_v1::send_metadata(admin, "Streamy v1 auto live")
                .await
                .expect("metadata");
        }
    }

    /// Live: the actual user requirement — when v1 fails (bad password),
    /// Auto must fall through to Ultravox 2.1 and connect on the same DNAS.
    ///   cargo test --lib auto_detect_falls_back_to_v2 -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn auto_detect_falls_back_to_v2() {
        let mut sc_v1 = match sc_v1_from_env() {
            Some(c) => c,
            None => {
                eprintln!("skip: set STREAMY_V1_HOST/PORT/PASS");
                return;
            }
        };
        sc_v1.password = "definitely-wrong".into(); // force v1 failure
        let ultravox = UltravoxConfig {
            host: sc_v1.host.clone(),
            port: sc_v1.port,
            stream_id: 1,
            password: std::env::var("STREAMY_V1_PASS").unwrap(),
            user: String::new(),
            content_type: "audio/mpeg".into(),
            bitrate_kbps: sc_v1.icy_br,
            name: "Streamy fallback test".into(),
            genre: "Test".into(),
            url: "".into(),
        };
        let cfg = StreamConfig::Auto {
            sc_v1,
            ultravox,
            icecast: IcySourceConfig {
                host: "unreachable.invalid".into(),
                port: 1,
                mount: "/stream".into(),
                user: "x".into(),
                password: "x".into(),
                content_type: "audio/mpeg".into(),
                icy_name: "Streamy".into(),
                icy_genre: "".into(),
                icy_url: "".into(),
                icy_pub: true,
                icy_br: 128,
                sample_rate: 44100,
            },
        };
        let src = connect_source(&cfg, 15)
            .await
            .expect("v1 failure must fall back to Ultravox");
        assert!(
            matches!(src, Source::Ultravox(_)),
            "auto-detect must fall back to Ultravox when v1 rejects the password"
        );
    }

    /// Live: Auto-detect against a Liquidsoap harbor must reject the Ultravox
    /// probe and fall back to Icecast, resolving to the Icecast source. Then
    /// exercise audio write + the direct (non-channel) metadata path that
    /// `set_stream_title` uses for Icecast. Creds via env, never committed.
    ///   cargo test --lib auto_detect_resolves_icecast_on_harbor -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn auto_detect_resolves_icecast_on_harbor() {
        let (host, port, user, pass, mount) = match envs() {
            Some(v) => v,
            None => {
                eprintln!("skip: set STREAMY_ICE_HOST/PORT/USER/PASS[/MOUNT]");
                return;
            }
        };
        let icecast = IcySourceConfig {
            host: host.clone(),
            port,
            mount,
            user: user.clone(),
            password: pass.clone(),
            content_type: "audio/mpeg".into(),
            icy_name: "Streamy auto-detect test".into(),
            icy_genre: "Test".into(),
            icy_url: "".into(),
            icy_pub: true,
            icy_br: 128,
            sample_rate: 44100,
        };
        // Bogus v1 + Ultravox cfgs — the harbor rejects the v1 password line
        // and the Ultravox binary handshake (no 0x5A sync), forcing the
        // Icecast fallback.
        let ultravox = UltravoxConfig {
            host: host.clone(),
            port,
            stream_id: 1,
            password: pass.clone(),
            user: user.clone(),
            content_type: "audio/mpeg".into(),
            bitrate_kbps: 128,
            name: "Streamy".into(),
            genre: "".into(),
            url: "".into(),
        };
        let cfg = StreamConfig::Auto {
            sc_v1: ScV1Config {
                host: host.clone(),
                port,
                password: pass.clone(),
                icy_name: "Streamy auto test".into(),
                icy_genre: "Test".into(),
                icy_url: "".into(),
                icy_pub: true,
                icy_br: 128,
            },
            ultravox,
            icecast,
        };
        let mut src = connect_source(&cfg, 15)
            .await
            .expect("auto connect should resolve");
        assert!(
            matches!(src, Source::Icecast { .. }),
            "auto-detect must resolve to Icecast on a harbor"
        );

        // Exercise the audio write path with a real MP3 chunk.
        let enc_cfg = crate::encode::EncoderConfig {
            sample_rate: 44100,
            channels: 2,
            bitrate_kbps: 128,
            codec: crate::encode::Codec::Mp3,
        };
        let mut enc = crate::encode::build(&enc_cfg).expect("encoder");
        let two_pi = 2.0 * std::f64::consts::PI;
        let mut samples = Vec::with_capacity(4410 * 2);
        for i in 0..4410usize {
            let v = (two_pi * 440.0 * (i as f64 / 44100.0)).sin() as f32 * 0.5;
            samples.push(v);
            samples.push(v);
        }
        let mp3 = enc.encode(&samples).expect("encode");
        src.send_audio(&mp3).await.expect("audio write");

        // Direct metadata GET (the path set_stream_title uses for Icecast).
        if let Source::Icecast { admin, .. } = &src {
            crate::stream::icy::send_metadata(admin, "Streamy auto-detect live")
                .await
                .expect("metadata");
        }
    }

    #[test]
    fn kbps_from_delta_is_instant_rate() {
        // 1000 bytes in 1s = 8 kbps (1000-bit kb convention).
        assert_eq!(kbps_from_delta(1000), 8.0);
        assert_eq!(kbps_from_delta(0), 0.0);
        // 16_000 bytes/s ≈ 128 kbps.
        assert!((kbps_from_delta(16_000) - 128.0).abs() < 0.5);
    }
}
