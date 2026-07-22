// P2.2: broadcast path — encoder thread pulls the broadcast-bus PCM ring, encodes
// to MP3 with one stateful lame handle, and a tokio streamer task sends Ultravox
// 0x7000 frames to the server. Realtime pacing is inherent: the audio callback
// pushes at realtime, so the whole chain is throttled to realtime (no bursting).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::encode::{EncoderConfig, Mp3Encoder};
use crate::events;
use crate::stream::ultravox::{self, UltravoxConfig};

/// MP3 must be sent in chunks under the Ultravox max payload (16377).
const MAX_PAYLOAD: usize = 16_000;
/// Encode 0.1s of stereo at a time (44100 Hz).
const ENCODE_FRAMES: usize = 4410;

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
}

/// Connect Ultravox and run the encoder + streamer against `rx` (broadcast PCM).
pub async fn start(
    mut rx: rtrb::Consumer<[f32; 2]>,
    uvox_cfg: UltravoxConfig,
    enc_cfg: EncoderConfig,
    app: Option<AppHandle>,
) -> Result<BroadcastHandle, String> {
    emit_status(&app, StreamStatus::Connecting);

    let mut stream = match ultravox::connect(&uvox_cfg).await {
        Ok(s) => s,
        Err(e) => {
            emit_status(&app, StreamStatus::Error { message: e.clone() });
            return Err(e);
        }
    };
    emit_status(&app, StreamStatus::Connected);

    let stop = Arc::new(AtomicBool::new(false));
    let (mp3_tx, mut mp3_rx) = mpsc::channel::<Vec<u8>>(64);

    // Encoder thread: ring -> lame -> MP3 chunks (paced by realtime producer).
    let stop_enc = stop.clone();
    std::thread::spawn(move || {
        let mut enc = match Mp3Encoder::new(&enc_cfg) {
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

    // Streamer task: MP3 chunks -> Ultravox 0x7000 frames.
    let stop_str = stop.clone();
    let streamer = tokio::spawn(async move {
        loop {
            if stop_str.load(Ordering::Relaxed) {
                break;
            }
            match mp3_rx.recv().await {
                Some(chunk) => {
                    let _ = ultravox::send_mp3(&mut stream, &chunk).await;
                }
                None => break, // encoder thread dropped the sender
            }
        }
    });

    Ok(BroadcastHandle { stop, streamer })
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
