use streamy_desktop::encode::{encode, Codec, EncoderConfig};

#[test]
fn encode_sine_produces_mp3_frames_and_expected_size() {
    let sr = 44_100u32;
    let n = sr as usize; // 1 second of frames
    let mut samples = Vec::with_capacity(n * 2);
    for i in 0..n {
        let v = (0.9 * (2.0 * std::f32::consts::PI * 440.0 * (i as f32) / sr as f32).sin()) as f32;
        samples.push(v);
        samples.push(v); // stereo interleaved
    }

    let out = encode(
        &samples,
        &EncoderConfig {
            sample_rate: sr,
            channels: 2,
            bitrate_kbps: 128,
            codec: Codec::Mp3,
        },
    )
    .expect("encode");

    // MP3 frame sync: 0xFF + high 3 bits set.
    let sync = out
        .windows(2)
        .position(|w| w[0] == 0xFF && (w[1] & 0xE0) == 0xE0);
    assert!(sync.is_some(), "no MP3 frame sync in {} bytes", out.len());

    // 128 kbps * 1s ≈ 16000 bytes.
    assert!(
        (12_000..=20_000).contains(&out.len()),
        "unexpected size {}",
        out.len()
    );
}

#[test]
fn unsupported_bitrate_errors() {
    assert!(encode(&[0.0; 4], &EncoderConfig {
        sample_rate: 44100,
        channels: 2,
        bitrate_kbps: 123,
        codec: Codec::Mp3,
    })
    .is_err());
}
