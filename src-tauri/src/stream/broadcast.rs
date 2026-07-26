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
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::diag;
use crate::encode::{self, EncoderConfig};
use crate::events;
use crate::stream::icy::IcySourceConfig;
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

/// Connection parameters for either supported source protocol. Built from a
/// `ConnectionProfile` + resolved password in `connect_stream`.
pub enum StreamConfig {
    Ultravox(UltravoxConfig),
    Icecast(IcySourceConfig),
}

/// A live source connection owning its transfer socket. Ultravox keeps
/// everything on one socket (metadata is in-band); Icecast holds the audio
/// socket and remembers the admin cfg for metadata GETs.
enum Source {
    Ultravox(TcpStream),
    Icecast {
        stream: TcpStream,
        admin: IcySourceConfig,
    },
}

impl Source {
    async fn send_audio(&mut self, mp3: &[u8]) -> Result<(), String> {
        match self {
            Source::Ultravox(s) => ultravox::send_mp3(s, mp3).await,
            Source::Icecast { stream, .. } => {
                stream.write_all(mp3).await.map_err(|e| e.to_string())
            }
        }
    }

    async fn send_metadata(&mut self, title: &str) -> Result<(), String> {
        match self {
            Source::Ultravox(s) => ultravox::send_metadata(s, title).await,
            Source::Icecast { admin, .. } => crate::stream::icy::send_metadata(admin, title).await,
        }
    }

    /// Whether a metadata-send error means the audio socket itself is dead and
    /// must be reconnected. Ultravox sends metadata in-band (an error = socket
    /// dead); Icecast metadata is a separate HTTP GET (an error is non-fatal).
    fn metadata_fatal(&self) -> bool {
        matches!(self, Source::Ultravox(_))
    }
}

pub struct BroadcastHandle {
    stop: Arc<AtomicBool>,
    streamer: JoinHandle<()>,
    pub metadata_tx: mpsc::Sender<String>,
}

/// Connect and run the encoder + streamer against `rx` (broadcast PCM),
/// reconnecting with default backoff on mid-stream drops.
pub async fn start(
    rx: rtrb::Consumer<[f32; 2]>,
    cfg: StreamConfig,
    enc_cfg: EncoderConfig,
    app: Option<AppHandle>,
) -> Result<BroadcastHandle, String> {
    start_with_opts(rx, cfg, enc_cfg, app, BACKOFF_BASE, BACKOFF_MAX, HANDSHAKE_TIMEOUT_SECS).await
}

/// As `start` but with explicit backoff/timeout — used by tests to keep the
/// reconnect dance fast and deterministic.
pub async fn start_with_opts(
    mut rx: rtrb::Consumer<[f32; 2]>,
    cfg: StreamConfig,
    enc_cfg: EncoderConfig,
    app: Option<AppHandle>,
    backoff_base: Duration,
    backoff_max: Duration,
    handshake_timeout_secs: u64,
) -> Result<BroadcastHandle, String> {
    emit_status(&app, StreamStatus::Connecting);

    // Initial connect: failure here is returned to the caller (Connect button
    // shows it). Mid-stream drops are handled by the reconnect loop below.
    let mut source = match connect_source(&cfg, handshake_timeout_secs).await {
        Ok(s) => s,
        Err(e) => {
            emit_status(&app, StreamStatus::Error { message: e.clone() });
            return Err(e);
        }
    };
    emit_status(&app, StreamStatus::Connected);

    let stop = Arc::new(AtomicBool::new(false));
    let (mp3_tx, mut mp3_rx) = mpsc::channel::<Vec<u8>>(64);
    let (metadata_tx, mut metadata_rx) = mpsc::channel::<String>(16);

    // Encoder thread: ring -> lame -> MP3 chunks (paced by realtime producer).
    // During a reconnect the channel fills and blocking_send backpressures this
    // thread (and in turn the audio ring) — a brief pause, not a stall.
    let stop_enc = stop.clone();
    std::thread::spawn(move || {
        let mut enc = match encode::build(&enc_cfg) {
            Ok(e) => e,
            Err(_) => return,
        };
        let mut buf: Vec<f32> = Vec::with_capacity(ENCODE_FRAMES * 2);
        loop {
            if stop_enc.load(Ordering::Relaxed) {
                break;
            }
            while let Ok(frame) = rx.pop() {
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
        loop {
            if stop_str.load(Ordering::Relaxed) {
                break;
            }
            let dropped = run_stream(&mut source, &mut mp3_rx, &mut metadata_rx, &stop_str).await;
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

    Ok(BroadcastHandle { stop, streamer, metadata_tx })
}

/// Stream until the socket errors (returns `true` → caller reconnects) or stop is
/// requested / the encoder dropped its sender (returns `false`). Icecast
/// metadata failures are logged but do not tear down the audio socket.
async fn run_stream(
    source: &mut Source,
    mp3_rx: &mut mpsc::Receiver<Vec<u8>>,
    metadata_rx: &mut mpsc::Receiver<String>,
    stop: &Arc<AtomicBool>,
) -> bool {
    loop {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        tokio::select! {
            chunk = mp3_rx.recv() => match chunk {
                Some(c) => {
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

/// Run the source handshake with a cap so a silent server can't hang Connect.
async fn connect_source(cfg: &StreamConfig, timeout_secs: u64) -> Result<Source, String> {
    let timeout = Duration::from_secs(timeout_secs);
    match cfg {
        StreamConfig::Ultravox(c) => {
            let s = match tokio::time::timeout(timeout, ultravox::connect(c)).await {
                Ok(Ok(s)) => s,
                Ok(Err(e)) => return Err(e),
                Err(_) => return Err(format!("connect timed out after {timeout_secs}s")),
            };
            Ok(Source::Ultravox(s))
        }
        StreamConfig::Icecast(c) => {
            let s = match tokio::time::timeout(timeout, crate::stream::icy::connect(c)).await {
                Ok(Ok(s)) => s,
                Ok(Err(e)) => return Err(e),
                Err(_) => return Err(format!("connect timed out after {timeout_secs}s")),
            };
            Ok(Source::Icecast {
                stream: s,
                admin: c.clone(),
            })
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

fn emit_status(app: &Option<AppHandle>, status: StreamStatus) {
    if let Some(a) = app {
        let _ = a.emit(events::STREAM_STATUS, status);
    }
}
