use streamy_desktop::engine::decoder::decode_file;

#[test]
fn decodes_mp3_to_expected_duration() {
    let (samples, sr, channels) =
        decode_file("tests/fixtures/tone-2s-44100-stereo.mp3").unwrap();
    assert_eq!(sr, 44100);
    assert_eq!(channels, 2);
    // MP3 has encoder delay/padding; allow ~2 frames (1152*samples/frame) of slop.
    let expected = 2 * 44100 * 2;
    let delta = (samples.len() as isize - expected as isize).unsigned_abs();
    assert!(
        delta < 2 * 2 * 1152,
        "len={} expected~{} delta={}",
        samples.len(),
        expected,
        delta
    );
}
