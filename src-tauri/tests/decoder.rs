use streamy_desktop::engine::decoder::decode_file;

#[test]
fn decodes_stereo_wav_to_expected_frame_count() {
    let (samples, sr, channels) =
        decode_file("tests/fixtures/silence-1s-44100-stereo.wav").unwrap();
    assert_eq!(sr, 44100);
    assert_eq!(channels, 2);
    assert_eq!(samples.len(), 44100 * 2);
}
