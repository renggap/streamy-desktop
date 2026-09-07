use streamy_desktop::engine::deck::Deck;

#[test]
fn streams_mp3_and_produces_signal() {
    let mut d = Deck::new();
    d.open("tests/fixtures/tone-2s-44100-stereo.mp3", 48000)
        .unwrap();
    d.playing = true;
    let mut max = 0.0f32;
    let mut frames = 0usize;
    for _ in 0..10000 {
        let [l, r] = d.next_frame();
        max = max.max(l.abs()).max(r.abs());
        frames += 1;
    }
    assert!(max > 0.1, "expected tone signal, got max={max}");
    assert!(frames == 10000);
}

#[test]
fn ends_when_exhausted() {
    let mut d = Deck::new();
    d.open("tests/fixtures/tone-2s-44100-stereo.mp3", 48000)
        .unwrap();
    d.playing = true;
    // 2s @ 48000 out rate -> ~96000 output frames; pull well past that.
    for _ in 0..120_000 {
        let _ = d.next_frame();
        if !d.playing {
            return; // ended cleanly
        }
    }
    panic!("deck did not signal end");
}

#[test]
fn seek_then_signal() {
    let mut d = Deck::new();
    d.open("tests/fixtures/tone-60s.mp3", 48000).unwrap();
    d.playing = true;
    for _ in 0..200 {
        let _ = d.next_frame();
    }
    d.seek(0.5, 48000);
    let mut max = 0.0f32;
    for _ in 0..2000 {
        let [l, _] = d.next_frame();
        max = max.max(l.abs());
    }
    assert!(max > 0.1, "expected signal after seek, got max={max}");
}

#[test]
fn open_bad_path_errors() {
    let mut d = Deck::new();
    assert!(d.open("tests/fixtures/nope.mp3", 48000).is_err());
}
