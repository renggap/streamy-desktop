// P4.1: fdk-aac encoder — ADTS output, frame sync, byte count ≈ bitrate×dur.
// Compiled only with the `aac` feature (fdk-aac native build).

#![cfg(feature = "aac")]

use streamy_desktop::encode::{build, Codec, EncoderConfig};

#[test]
fn encode_sine_produces_adts_frames_and_expected_size() {
    let sr = 44_100u32;
    let secs = 2usize;
    let n = sr as usize * secs; // stereo frames
    let mut samples = Vec::with_capacity(n * 2);
    for i in 0..n {
        let v = 0.9 * (2.0 * std::f32::consts::PI * 440.0 * (i as f32) / sr as f32).sin();
        samples.push(v);
        samples.push(v);
    }

    let cfg = EncoderConfig {
        sample_rate: sr,
        channels: 2,
        bitrate_kbps: 128,
        codec: Codec::Aac,
    };
    let mut enc = build(&cfg).expect("aac encoder");

    // Feed in chunks; AAC buffers a lookahead so early calls may emit nothing.
    let mut out = Vec::new();
    let chunk = 4096;
    let mut i = 0;
    while i < samples.len() {
        let end = (i + chunk).min(samples.len());
        out.extend_from_slice(&enc.encode(&samples[i..end]).unwrap());
        i = end;
    }
    out.extend_from_slice(&enc.flush().unwrap());

    // ADTS sync word: 12 one-bits -> 0xFF, 0xFx.
    let sync = out
        .windows(2)
        .position(|w| w[0] == 0xFF && (w[1] & 0xF0) == 0xF0);
    assert!(sync.is_some(), "no ADTS sync in {} bytes", out.len());

    // 128 kbps * 2s ≈ 32000 bytes (ADTS adds ~7B/frame overhead).
    assert!(
        (24_000..=42_000).contains(&out.len()),
        "unexpected aac size {}",
        out.len()
    );
}
