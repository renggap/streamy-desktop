//! Live Ultravox 2.1 probes against a real DNAS2 server. Not run by default.
//!   STREAMY_HOST=… STREAMY_PORT=… STREAMY_PASS=… [STREAMY_SID=1] \
//!     cargo test --test stream_live -- --nocapture --ignored

use std::f32::consts::PI;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use streamy_desktop::encode::{encode, Codec, EncoderConfig};
use streamy_desktop::stream::broadcast;
use streamy_desktop::stream::ultravox::{connect, send_mp3, UltravoxConfig};

fn cfg() -> UltravoxConfig {
    UltravoxConfig {
        host: std::env::var("STREAMY_HOST").expect("STREAMY_HOST"),
        port: std::env::var("STREAMY_PORT")
            .expect("STREAMY_PORT")
            .parse()
            .expect("port"),
        stream_id: std::env::var("STREAMY_SID")
            .unwrap_or_else(|_| "1".into())
            .parse()
            .expect("sid"),
        password: std::env::var("STREAMY_PASS").expect("STREAMY_PASS"),
        content_type: "audio/mpeg".into(),
        bitrate_kbps: 128,
        name: "Streamy probe".into(),
        genre: "Test".into(),
        url: "http://streamy".into(),
    }
}

fn one_second_tone_mp3() -> Vec<u8> {
    let sr = 44_100usize;
    let mut samples = Vec::with_capacity(sr * 2);
    for i in 0..sr {
        let v = (0.9 * (2.0 * PI * 440.0 * i as f32 / sr as f32).sin()) as f32;
        samples.push(v);
        samples.push(v);
    }
    encode(
        &samples,
        &EncoderConfig {
            sample_rate: 44_100,
            channels: 2,
            bitrate_kbps: 128,
            codec: Codec::Mp3,
        },
    )
    .expect("encode")
}

async fn http_get(host: &str, port: u16, path: &str) -> String {
    let mut s = TcpStream::connect((host, port)).await.expect("stats connect");
    let req = format!(
        "GET {path} HTTP/1.0\r\nHost: {host}\r\nUser-Agent: streamy-probe\r\n\r\n"
    );
    s.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).await.unwrap();
    String::from_utf8_lossy(&buf).to_string()
}

#[tokio::test]
#[ignore]
async fn probe_ultravox_handshake_live() {
    match connect(&cfg()).await {
        Ok(_) => println!("ULTRAVOX handshake ACCEPTED (standby/data mode)"),
        Err(e) => println!("ULTRAVOX handshake REJECTED: {e}"),
    }
}

#[tokio::test]
#[ignore]
async fn stream_and_verify_live() {
    let cfg = cfg();
    let mut s = connect(&cfg).await.expect("handshake");
    let mp3 = one_second_tone_mp3();
    println!("encoded {} bytes/s of MP3", mp3.len());

    // Stream ~4 seconds of the tone, chunked under the 16377 max payload and
    // paced so we don't burst the server's buffer.
    const CHUNK: usize = 4096;
    for sec in 0..4 {
        let mut i = 0;
        while i < mp3.len() {
            let end = (i + CHUNK).min(mp3.len());
            send_mp3(&mut s, &mp3[i..end]).await.expect("send_mp3");
            i = end;
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        println!("sent second {}", sec + 1);
    }

    // Ask the server for stream stats while still connected.
    let stats = http_get(&cfg.host, cfg.port, "/stats?sid=1").await;
    println!("--- /stats?sid=1 ---\n{stats}");
    let live = stats.contains("<STREAMSTATUS>1</STREAMSTATUS>");
    println!("STREAMSTATUS live = {live}");
    // Keep the source briefly so the listener path could catch it, then drop.
    tokio::time::sleep(Duration::from_secs(1)).await;
    drop(s);
}

#[tokio::test]
#[ignore]
async fn broadcast_ring_drives_server_live() {
    // Drives the broadcast path (ring -> encoder thread -> streamer -> Ultravox)
    // with a realtime 440Hz tone, then checks the server went live.
    let uvox = cfg();
    let host = uvox.host.clone();
    let port = uvox.port;
    let enc_cfg = EncoderConfig {
        sample_rate: 44_100,
        channels: 2,
        bitrate_kbps: 128,
        codec: Codec::Mp3,
    };

    let (mut tx, rx) = rtrb::RingBuffer::<[f32; 2]>::new(16384);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_p = stop.clone();
    std::thread::spawn(move || {
        let sr = 44_100.0_f64;
        let mut i: u64 = 0;
        while !stop_p.load(Ordering::Relaxed) {
            for _ in 0..4410 {
                // 0.1s of frames
                let v = (0.9 * (2.0 * std::f64::consts::PI * 440.0 * i as f64 / sr).sin()) as f32;
                let _ = tx.push([v, v]);
                i += 1;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    });

    let handle = broadcast::start(rx, uvox, enc_cfg, None)
        .await
        .expect("broadcast start");
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Push a now-playing title and verify the server picks it up.
    handle
        .metadata_tx
        .send("Streamy Test Title".to_string())
        .await
        .expect("send metadata");
    tokio::time::sleep(Duration::from_secs(1)).await;

    let stats = http_get(&host, port, "/stats?sid=1").await;
    println!("--- broadcast /stats ---\n{stats}");
    let live = stats.contains("<STREAMSTATUS>1</STREAMSTATUS>");
    let title_ok = stats.contains("Streamy Test Title");
    println!("broadcast STREAMSTATUS live = {live}; title_ok = {title_ok}");

    stop.store(true, Ordering::Relaxed);
    handle.stop().await;
    assert!(live, "server did not report the stream as live");
    assert!(title_ok, "SONGTITLE not updated");
}
