//! Live Ultravox 2.1 probes against a real DNAS2 server. Not run by default.
//!   STREAMY_HOST=… STREAMY_PORT=… STREAMY_PASS=… [STREAMY_SID=1] \
//!     cargo test --test stream_live -- --nocapture --ignored

use std::f32::consts::PI;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use streamy_desktop::encode::{encode, EncoderConfig};
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
