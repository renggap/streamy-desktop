// P3: recording. The engine taps its broadcast mix into a record ring (independent
// of streaming — recording works even when not live). A recorder thread drains it
// and writes either MP3 (its own lame encoder) or 16-bit PCM WAV.
//
// ponytail: WAV buffers s16 samples in memory and writes the header on stop
// (avoids a seeking sink). Fine for show-length archives; switch to chunked
// seek-based writes if multi-hour lossless matters.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::encode::{EncoderConfig, Mp3Encoder};

/// Encode 0.1s of stereo at a time (matches the broadcast encoder granularity).
const ENC_FRAMES: usize = 4410;

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RecFormat {
    Mp3,
    Wav,
}

/// Recording lifecycle event sent to the frontend.
#[derive(Serialize, Clone)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum RecordingStatus {
    Recording { path: String },
    Stopped { path: String },
    Error { message: String },
}

/// Convert one stereo float frame to two little-endian s16 samples (4 bytes).
pub fn frame_to_s16le(frame: [f32; 2]) -> [u8; 4] {
    let cvt = |s: f32| (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
    let l = cvt(frame[0]).to_le_bytes();
    let r = cvt(frame[1]).to_le_bytes();
    [l[0], l[1], r[0], r[1]]
}

/// 44-byte canonical WAV header for `data_len` bytes of 16-bit PCM.
pub fn wav_header(data_len: u32, sample_rate: u32, channels: u16) -> Vec<u8> {
    let byte_rate = sample_rate * channels as u32 * 2;
    let block_align = channels * 2;
    let mut h = Vec::with_capacity(44);
    h.extend(b"RIFF");
    h.extend(&(36 + data_len).to_le_bytes());
    h.extend(b"WAVE");
    h.extend(b"fmt ");
    h.extend(&16u32.to_le_bytes());
    h.extend(&1u16.to_le_bytes()); // PCM
    h.extend(&channels.to_le_bytes());
    h.extend(&sample_rate.to_le_bytes());
    h.extend(&byte_rate.to_le_bytes());
    h.extend(&block_align.to_le_bytes());
    h.extend(&16u16.to_le_bytes()); // bits/sample
    h.extend(b"data");
    h.extend(&data_len.to_le_bytes());
    h
}

/// Drain `rx` into `out` in the chosen format until `stop` is set AND the ring is
/// empty, then finalize (flush encoder / write WAV header). Returns the first
/// write/encode error. Generic over the sink so tests can pass an in-memory buffer
/// or a failing writer (disk-full).
pub fn run<W: Write>(
    rx: &mut rtrb::Consumer<[f32; 2]>,
    format: RecFormat,
    enc_cfg: &EncoderConfig,
    sample_rate: u32,
    out: &mut W,
    stop: &AtomicBool,
) -> Result<(), String> {
    match format {
        RecFormat::Mp3 => run_mp3(rx, enc_cfg, out, stop),
        RecFormat::Wav => run_wav(rx, sample_rate, out, stop),
    }
}

fn run_mp3<W: Write>(
    rx: &mut rtrb::Consumer<[f32; 2]>,
    enc_cfg: &EncoderConfig,
    out: &mut W,
    stop: &AtomicBool,
) -> Result<(), String> {
    let mut enc = Mp3Encoder::new(enc_cfg)?;
    let mut buf: Vec<f32> = Vec::with_capacity(ENC_FRAMES * 2);
    loop {
        while let Ok(frame) = rx.pop() {
            buf.push(frame[0]);
            buf.push(frame[1]);
        }
        // Fixed-size chunks so output is byte-identical to a direct encode of the
        // same PCM in ENC_FRAMES chunks, regardless of drain timing.
        while buf.len() >= ENC_FRAMES * 2 {
            let mp3 = enc.encode(&buf[..ENC_FRAMES * 2])?;
            out.write_all(&mp3).map_err(|e| e.to_string())?;
            buf.drain(..ENC_FRAMES * 2);
        }
        if stop.load(Ordering::Relaxed) {
            if !buf.is_empty() {
                let mp3 = enc.encode(&buf)?;
                out.write_all(&mp3).map_err(|e| e.to_string())?;
                buf.clear();
            }
            let mp3 = enc.flush()?;
            out.write_all(&mp3).map_err(|e| e.to_string())?;
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}

fn run_wav<W: Write>(
    rx: &mut rtrb::Consumer<[f32; 2]>,
    sample_rate: u32,
    out: &mut W,
    stop: &AtomicBool,
) -> Result<(), String> {
    let mut pcm: Vec<u8> = Vec::new();
    loop {
        let mut got = false;
        while let Ok(frame) = rx.pop() {
            pcm.extend_from_slice(&frame_to_s16le(frame));
            got = true;
        }
        if stop.load(Ordering::Relaxed) && !got {
            break;
        }
        if !got {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    out.write_all(&wav_header(pcm.len() as u32, sample_rate, 2))
        .map_err(|e| e.to_string())?;
    out.write_all(&pcm).map_err(|e| e.to_string())?;
    out.flush().map_err(|e| e.to_string())?;
    Ok(())
}

pub struct RecorderHandle {
    stop: Arc<AtomicBool>,
    join: JoinHandle<Result<(), String>>,
}

impl RecorderHandle {
    /// Signal stop, await the recorder thread, return its outcome.
    pub fn stop(self) -> Result<(), String> {
        self.stop.store(true, Ordering::Relaxed);
        match self.join.join() {
            Ok(r) => r,
            Err(_) => Err("recorder thread panicked".into()),
        }
    }
}

/// Spawn a recorder thread writing to `path`. `on_error` fires once if the run
/// fails (disk full, etc.) so the UI can react immediately instead of at stop.
pub fn start_file(
    mut rx: rtrb::Consumer<[f32; 2]>,
    format: RecFormat,
    enc_cfg: EncoderConfig,
    sample_rate: u32,
    path: &str,
    on_error: Option<Box<dyn FnOnce(String) + Send>>,
) -> Result<RecorderHandle, String> {
    let mut out = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();
    let join = std::thread::spawn(move || {
        let res = run(&mut rx, format, &enc_cfg, sample_rate, &mut out, &stop_clone);
        let _ = out.flush();
        if let Err(ref e) = res {
            if let Some(cb) = on_error {
                cb(e.clone());
            }
        }
        res
    });
    Ok(RecorderHandle { stop, join })
}
