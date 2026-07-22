// P2.2: broadcast path — encoder thread pulls the broadcast-bus PCM ring, encodes
// to MP3 with one stateful lame handle, and a tokio streamer task sends Ultravox
// 0x7000 frames to the server. Realtime pacing is inherent: the audio callback
// pushes at realtime, so the whole chain is throttled to realtime (no bursting).
//
// P2.7: the streamer owns a reconnect loop. On a mid-stream socket error it emits
// Disconnected → Connecting, waits with exponential backoff, re-handshakes, and
// resumes. The initial connect is still surfaced to the caller (Connect button).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::encode::{self, EncoderConfig};
use crate::events;
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

pub struct BroadcastHandle {
    stop: Arc<AtomicBool>,
    streamer: JoinHandle<()>,
    pub metadata_tx: mpsc::Sender<String>,
}

/// Connect Ultravox and run the encoder + streamer against `rx` (broadcast PCM),
/// reconnecting with default backoff on mid-stream drops.
pub async fn start(
    rx: rtrb::Consumer<[f32; 2]>,
    uvox_cfg: UltravoxConfig,
    enc_cfg: EncoderConfig,
    app: Option<AppHandle>,
) -> Result<BroadcastHandle, String> {
    start_with_opts(rx, uvox_cfg, enc_cfg, app, BACKOFF_BASE, BACKOFF_MAX, HANDSHAKE_TIMEOUT_SECS).await
}

/// As `start` but with explicit backoff/timeout — used by tests to keep the
/// reconnect dance fast and deterministic.
pub async fn start_with_opts(
    mut rx: rtrb::Consumer<[f32; 2]>,
    uvox_cfg: UltravoxConfig,
    enc_cfg: EncoderConfig,
    app: Option<AppHandle>,
    backoff_base: Duration,
    backoff_max: Duration,
    handshake_timeout_secs: u64,
) -> Result<BroadcastHandle, String> {
    emit_status(&app, StreamStatus::Connecting);

    // Initial connect: failure here is returned to the caller (Connect button
    // shows it). Mid-stream drops are handled by the reconnect loop below.
    let mut stream = match connect_with_timeout(&uvox_cfg, handshake_timeout_secs).await {
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

    // Streamer task: MP3 chunks -> Ultravox 0x7000 frames; metadata on demand.
    // On socket error, reconnect with growing backoff and resume.
    let stop_str = stop.clone();
    let streamer = tokio::spawn(async move {
        loop {
            if stop_str.load(Ordering::Relaxed) {
                break;
            }
            let dropped = run_stream(&mut stream, &mut mp3_rx, &mut metadata_rx, &stop_str).await;
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
                match connect_with_timeout(&uvox_cfg, handshake_timeout_secs).await {
                    Ok(s) => {
                        stream = s;
                        emit_status(&app, StreamStatus::Connected);
                        break;
                    }
                    Err(_) => {
                        delay = (delay * 2).min(backoff_max);
                    }
                }
            }
        }
    });

    Ok(BroadcastHandle { stop, streamer, metadata_tx })
}

/// Stream until the socket errors (returns `true` → caller reconnects) or stop is
/// requested / the encoder dropped its sender (returns `false`).
async fn run_stream(
    stream: &mut TcpStream,
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
                    if ultravox::send_mp3(stream, &c).await.is_err() {
                        return true;
                    }
                }
                None => return false,
            },
            title = metadata_rx.recv() => {
                if let Some(t) = title {
                    if ultravox::send_metadata(stream, &t).await.is_err() {
                        return true;
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

/// Run the Ultravox handshake with a cap so a silent server can't hang Connect.
async fn connect_with_timeout(cfg: &UltravoxConfig, timeout_secs: u64) -> Result<TcpStream, String> {
    match tokio::time::timeout(Duration::from_secs(timeout_secs), ultravox::connect(cfg)).await {
        Ok(Ok(s)) => Ok(s),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(format!("connect timed out after {timeout_secs}s")),
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
