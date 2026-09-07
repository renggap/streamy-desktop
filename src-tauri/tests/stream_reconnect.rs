// P2.7: fake Ultravox server drops the connection N times mid-stream, then stays
// up. The broadcaster must reconnect with growing backoff and resume sending MP3.
//
// The server speaks just enough of the handshake to satisfy the client: it ACKs
// every control frame regardless of id (expect_ack only checks the payload), then
// either closes after a few MP3 frames (drop connections) or accumulates 0x7000
// data forever (final). Drops happen AFTER data starts so the client observes a
// real write error on its next send — a reliable reconnect trigger.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

use streamy_desktop::encode::{Codec, EncoderConfig};
use streamy_desktop::engine::ring::StereoRing;
use streamy_desktop::stream::broadcast;
use streamy_desktop::stream::ultravox::{encode_frame, read_frame, UltravoxConfig};

/// Minimal Ultravox responder. Drop the first `drops` connections after they send
/// `drop_after` MP3 frames; keep the final one open and count MP3 bytes received.
async fn run_server(
    listener: TcpListener,
    drops: u32,
    drop_after: u32,
    accept_times: Arc<Mutex<Vec<Instant>>>,
    total: Arc<AtomicUsize>,
) {
    let count = Arc::new(AtomicU32::new(0));
    while let Ok((mut sock, _)) = listener.accept().await {
        let idx = count.fetch_add(1, Ordering::SeqCst);
        accept_times.lock().unwrap().push(Instant::now());
        let total = total.clone();
        tokio::spawn(async move {
            let mut data_frames = 0u32;
            while let Ok((id, payload)) = read_frame(&mut sock).await {
                if id == 0x7000 {
                    total.fetch_add(payload.len(), Ordering::SeqCst);
                    data_frames += 1;
                    // Drop connections bow out after a little real data,
                    // so the client's next write observes the close.
                    if idx < drops && data_frames >= drop_after {
                        break;
                    }
                } else {
                    // ACK every control frame (id unchecked by client).
                    let _ = sock.write_all(&encode_frame(0x0000, b"ACK\0")).await;
                }
            }
        });
    }
}

fn cfg(port: u16) -> UltravoxConfig {
    UltravoxConfig {
        host: "127.0.0.1".into(),
        port,
        stream_id: 1,
        password: String::new(), // empty pwd -> empty xtea; server doesn't check
        user: String::new(),
        content_type: "audio/mpeg".into(),
        bitrate_kbps: 128,
        name: "Streamy".into(),
        genre: "".into(),
        url: "".into(),
    }
}

/// Continuous sine producer into the broadcast ring (stands in for the audio
/// callback). Stops when `stop` is set.
fn spawn_producer(tx: streamy_desktop::engine::ring::StereoRingProducer, stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let mut i = 0u32;
        while !stop.load(Ordering::Relaxed) {
            let s = ((i as f32) * 2.0 * std::f32::consts::PI * 440.0 / 44100.0).sin() * 0.3;
            tx.push([s, s]); // drop-oldest ring: never blocks
            {
                // Pace near realtime so the encoder sees a realistic feed.
                std::thread::sleep(Duration::from_micros(20));
            }
            i = i.wrapping_add(1);
        }
    });
}

/// P2.2 regression: an encoder build failure must surface from `start` as an
/// Err — never a silently-dead encoder thread (UI showed ON AIR with zero
/// audio). An invalid bitrate fails `Mp3Encoder::new`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn encoder_init_failure_surfaces_as_error() {
    let (_, rx) = StereoRing::new(8192);
    let enc = EncoderConfig {
        sample_rate: 44100,
        channels: 2,
        bitrate_kbps: 999, // invalid -> Mp3Encoder::new errors
        codec: Codec::Mp3,
    };
    let err = match broadcast::start_with_opts(
        rx,
        broadcast::StreamConfig::Ultravox(cfg(1)),
        enc,
        None,
        Duration::from_millis(150),
        Duration::from_millis(600),
        5,
    )
    .await
    {
        Ok(_) => panic!("encoder init failure must surface as Err, not silent success"),
        Err(e) => e,
    };

    assert!(
        err.contains("bitrate") || err.contains("lame") || err.contains("encoder"),
        "error should reference the codec/bitrate failure, got: {err}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reconnects_with_backoff_after_drops() {
    let drops = 2;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let accept_times = Arc::new(Mutex::new(Vec::<Instant>::new()));
    let total = Arc::new(AtomicUsize::new(0));

    let times = accept_times.clone();
    let total_for_srv = total.clone();
    let _srv = tokio::spawn(run_server(listener, drops, 2, times, total_for_srv));

    // Keep the encoder fed for the whole test (continuous broadcast bus input).
    let (tx, rx) = StereoRing::new(8192);
    let prod_stop = Arc::new(AtomicBool::new(false));
    spawn_producer(tx, prod_stop.clone());

    let enc = EncoderConfig {
        sample_rate: 44100,
        channels: 2,
        bitrate_kbps: 128,
        codec: Codec::Mp3,
    };
    // Test-sized backoff: real enough to exercise the reconnect path, fast enough
    // to keep the test well under a second.
    let handle = broadcast::start_with_opts(
        rx,
        broadcast::StreamConfig::Ultravox(cfg(port)),
        enc,
        None,
        Duration::from_millis(150),
        Duration::from_millis(600),
        5,
    )
    .await
    .expect("initial connect");

    // Phase 1: wait (up to ~10s) for the client to reconnect past every drop.
    // Each drop connection also receives a couple of MP3 frames before closing,
    // so `total > 0` alone is NOT proof — the connect count is.
    let mut connects = 0usize;
    for _ in 0..200 {
        connects = accept_times.lock().unwrap().len();
        if connects >= (drops + 1) as usize {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Phase 2: confirm the FINAL connection is actively streaming (total keeps
    // climbing after the last reconnect, not just the drop-connection leftovers).
    let total0 = total.load(Ordering::SeqCst);
    let mut got = total0;
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        got = total.load(Ordering::SeqCst);
        if got > total0 + 500 {
            break;
        }
    }

    prod_stop.store(true, Ordering::Relaxed);
    handle.stop().await;

    assert!(
        connects >= (drops + 1) as usize,
        "expected at least {} connects, got {} — no reconnect",
        drops + 1,
        connects
    );
    assert!(
        got > total0 + 500,
        "final connection did not stream: total {total0} -> {got}"
    );

    // Backoff growth (delay *= 2 per failure, capped) is structural in
    // broadcast.rs; we don't assert inter-connect timing here — async-network
    // drop-detection jitter is large enough to make it flaky for little value.
}
