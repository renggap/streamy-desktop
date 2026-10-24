// P3.1/3.2/3.4: recorder — WAV header math, s16 conversion, byte-identical MP3,
// and disk-full (write error → Err, stream path untouched by architecture).

use std::sync::atomic::AtomicBool;

use streamy_desktop::encode::Codec;
use streamy_desktop::encode::EncoderConfig;
use streamy_desktop::record::{frame_to_s16le, run, wav_header, RecFormat};

/// Static output-rate watch for run(): most tests never hot-swap.
fn rate_watch(r: u32) -> std::sync::Arc<std::sync::atomic::AtomicU32> {
    std::sync::Arc::new(std::sync::atomic::AtomicU32::new(r))
}

#[test]
fn wav_header_is_canonical_44_bytes() {
    let h = wav_header(1600, 44100, 2);
    assert_eq!(h.len(), 44);
    assert_eq!(&h[0..4], b"RIFF");
    assert_eq!(&h[8..12], b"WAVE");
    assert_eq!(&h[12..16], b"fmt ");
    assert_eq!(&h[36..40], b"data");
    // RIFF size = 36 + data; data size field.
    assert_eq!(&h[4..8], &(36u32 + 1600).to_le_bytes());
    assert_eq!(&h[40..44], &1600u32.to_le_bytes());
    // PCM, stereo, 44100, 16-bit.
    assert_eq!(u16::from_le_bytes([h[20], h[21]]), 1);
    assert_eq!(u16::from_le_bytes([h[22], h[23]]), 2);
    let mut sr = [0u8; 4];
    sr.copy_from_slice(&h[24..28]);
    assert_eq!(u32::from_le_bytes(sr), 44100);
    assert_eq!(u16::from_le_bytes([h[34], h[35]]), 16);
}

#[test]
fn frame_to_s16le_clamps_and_converts() {
    assert_eq!(frame_to_s16le([0.0, 0.0]), [0, 0, 0, 0]);
    // +1.0 → 32767 = 0x7F_FF (LE).
    assert_eq!(frame_to_s16le([1.0, 1.0]), [0xFF, 0x7F, 0xFF, 0x7F]);
    // -1.0 → -32767 = 0x80_01 (LE) under round(); clamp keeps in i16 range.
    let l = frame_to_s16le([-1.0, -1.0]);
    assert_eq!(i16::from_le_bytes([l[0], l[1]]), -32767);
    // Over-range clamps to ±32767.
    let c = frame_to_s16le([5.0, -5.0]);
    assert_eq!(i16::from_le_bytes([c[0], c[1]]), 32767);
}

/// Push `n` silent frames, run WAV, assert header + exact data length.
#[test]
fn wav_run_writes_header_and_all_samples() {
    let (tx, rx) = streamy_desktop::engine::ring::StereoRing::new(32768);
    let n = 4410 * 2; // 0.2s
    for _ in 0..n {
        tx.push([0.0, 0.0]);
    }
    let stop = AtomicBool::new(true); // already done feeding
    let mut out = std::io::Cursor::new(Vec::<u8>::new());
    run(
        &rx,
        RecFormat::Wav,
        &EncoderConfig {
            sample_rate: 44100,
            channels: 2,
            bitrate_kbps: 192,
            codec: Codec::Mp3,
        },
        44100,
        &rate_watch(44100),
        &mut out,
        &stop,
    )
    .unwrap();
    let got = out.get_ref();
    assert_eq!(got.len(), 44 + n * 4);
    assert_eq!(&got[4..8], &(36u32 + (n * 4) as u32).to_le_bytes());
    assert_eq!(&got[40..44], &((n * 4) as u32).to_le_bytes());
}

/// Recorder MP3 output must be byte-identical to a direct encode of the same PCM
/// in ENC_FRAMES chunks (deterministic encoder + deterministic chunking).
#[test]
fn mp3_run_is_byte_identical_to_direct_encode() {
    let n = 4410 * 2; // exactly two full chunks
    let pcm: Vec<[f32; 2]> = (0..n)
        .map(|i| {
            let s = ((i as f32) * 2.0 * std::f32::consts::PI * 440.0 / 44100.0).sin() * 0.3;
            [s, s]
        })
        .collect();

    let (tx, rx) = streamy_desktop::engine::ring::StereoRing::new(32768);
    for f in &pcm {
        tx.push(*f);
    }
    let stop = AtomicBool::new(true);
    let mut rec_out = std::io::Cursor::new(Vec::<u8>::new());
    let cfg = EncoderConfig {
        sample_rate: 44100,
        channels: 2,
        bitrate_kbps: 192,
        codec: Codec::Mp3,
    };
    run(
        &rx,
        RecFormat::Mp3,
        &cfg,
        44100,
        &rate_watch(44100),
        &mut rec_out,
        &stop,
    )
    .unwrap();

    // Direct encode mirroring the recorder's fixed-chunk loop.
    use streamy_desktop::encode::Mp3Encoder;
    let mut enc = Mp3Encoder::new(&cfg).unwrap();
    let mut direct = Vec::<u8>::new();
    let mut flat: Vec<f32> = Vec::with_capacity(n * 2);
    for f in &pcm {
        flat.push(f[0]);
        flat.push(f[1]);
    }
    let mut i = 0;
    while i + 4410 * 2 <= flat.len() {
        direct.extend_from_slice(&enc.encode(&flat[i..i + 4410 * 2]).unwrap());
        i += 4410 * 2;
    }
    direct.extend_from_slice(&enc.flush().unwrap());

    assert!(!rec_out.get_ref().is_empty(), "recorder produced no bytes");
    assert!(!direct.is_empty());
    assert_eq!(
        rec_out.into_inner(),
        direct,
        "recorder MP3 must match direct encode byte-for-byte"
    );
}

/// A sink that fails once it has accepted `cap` bytes — stands in for a full disk.
struct FailingSink {
    cap: usize,
    written: usize,
}

impl std::io::Write for FailingSink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        if self.written + b.len() > self.cap {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WriteZero,
                "disk full",
            ));
        }
        self.written += b.len();
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl std::io::Seek for FailingSink {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.written = match pos {
            std::io::SeekFrom::Start(p) => p as usize,
            std::io::SeekFrom::Current(d) => (self.written as i64 + d).max(0) as usize,
            std::io::SeekFrom::End(d) => (self.cap as i64 + d).max(0) as usize,
        };
        Ok(self.written as u64)
    }
}

/// P3.4: a write error surfaces from run() as Err. The broadcast/stream path is a
/// separate ring + encoder (no shared mutable state), so it is unaffected by
/// construction — verified here by run() returning Err, not by panicking.
#[test]
fn run_propagates_write_error() {
    let (tx, rx) = streamy_desktop::engine::ring::StereoRing::new(32768);
    for i in 0..(4410 * 4) {
        let s = ((i as f32) * 0.01).sin();
        tx.push([s, s]);
    }
    let stop = AtomicBool::new(false); // keep running so it hits the cap mid-stream
    let mut sink = FailingSink {
        cap: 4096,
        written: 0,
    };
    let cfg = EncoderConfig {
        sample_rate: 44100,
        channels: 2,
        bitrate_kbps: 192,
        codec: Codec::Mp3,
    };
    let res = run(
        &rx,
        RecFormat::Mp3,
        &cfg,
        44100,
        &rate_watch(44100),
        &mut sink,
        &stop,
    );
    assert!(res.is_err(), "write error must propagate, got {res:?}");
    let msg = res.unwrap_err();
    assert!(
        msg.contains("disk full") || msg.contains("WriteZero") || msg.contains("write"),
        "msg: {msg}"
    );
}

#[test]
fn wav_take_stops_when_output_rate_changes() {
    // Device hot-swap mid-take: the WAV header declares one rate for the
    // whole file, so a change must end the take with a clear error instead
    // of writing a pitch-shifted tail.
    let (tx, rx) = streamy_desktop::engine::ring::StereoRing::new(32768);
    for _ in 0..(4410 * 2) {
        tx.push([0.1, -0.1]);
    }
    let rate: std::sync::Arc<std::sync::atomic::AtomicU32> =
        std::sync::Arc::new(std::sync::atomic::AtomicU32::new(44100));
    rate.store(48000, std::sync::atomic::Ordering::Relaxed); // the hot-swap
    let stop = AtomicBool::new(true); // ring fully fed already
    let mut out = std::io::Cursor::new(Vec::<u8>::new());
    let res = run(
        &rx,
        RecFormat::Wav,
        &EncoderConfig {
            sample_rate: 44100,
            channels: 2,
            bitrate_kbps: 192,
            codec: Codec::Mp3,
        },
        44100,
        &rate,
        &mut out,
        &stop,
    );
    let err = res.expect_err("rate change must stop the take");
    assert!(err.contains("rate changed"), "unexpected error: {err}");
    // Header still patched: playable partial file starting with RIFF/WAVE.
    let buf = out.into_inner();
    assert_eq!(&buf[0..4], b"RIFF");
    assert_eq!(&buf[8..12], b"WAVE");
}
