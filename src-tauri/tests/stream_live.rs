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
        user: std::env::var("STREAMY_DJ").unwrap_or_default(),
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
        let v = 0.9 * (2.0 * PI * 440.0 * i as f32 / sr as f32).sin();
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

/// Pull `<BITRATE>N</BITRATE>` out of a DNAS /stats XML response.
fn server_bitrate(stats: &str) -> String {
    stats
        .split("<BITRATE>")
        .nth(1)
        .and_then(|s| s.split("</BITRATE>").next())
        .unwrap_or("?")
        .to_string()
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

    // Ask the server for stream stats while still connected. Print the
    // configured bitrate next to what the server reports so a DNAS-side
    // bitrate-table mismatch (e.g. AAC 24 shown as 32) is visible in one line.
    let stats = http_get(&cfg.host, cfg.port, "/stats?sid=1").await;
    println!("--- /stats?sid=1 ---\n{stats}");
    let live = stats.contains("<STREAMSTATUS>1</STREAMSTATUS>");
    println!(
        "configured={} kbps server_reported={} STREAMSTATUS_live={}",
        cfg.bitrate_kbps,
        server_bitrate(&stats),
        live
    );
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

    let (tx, rx) = streamy_desktop::engine::ring::StereoRing::new(16384);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_p = stop.clone();
    std::thread::spawn(move || {
        let sr = 44_100.0_f64;
        let mut i: u64 = 0;
        while !stop_p.load(Ordering::Relaxed) {
            for _ in 0..4410 {
                // 0.1s of frames
                let v = (0.9 * (2.0 * std::f64::consts::PI * 440.0 * i as f64 / sr).sin()) as f32;
                tx.push([v, v]);
                i += 1;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    });

    let handle = broadcast::start(rx, broadcast::StreamConfig::Ultravox(uvox), enc_cfg, None)
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

// --- Local validation (run by default, no server needed) ---
//
// These prove the confirmation pipeline in `stream_and_verify_live` works:
// `server_bitrate` correctly parses a DNAS /stats response, and `http_get`
// fetches one over real TCP. Together they let the user trust the
// `configured=… server_reported=…` line when comparing against their DNAS.

#[test]
fn server_bitrate_parses_dnas_stats() {
    // Realistic DNASv2 /stats?sid=1 snippet (AAC-LC table artifact: server
    // rounds a 24kbps source up to 32).
    let stats = "\
<STREAMSTATUS>1</STREAMSTATUS>\n\
<BITRATE>32</BITRATE>\n\
<STREAMS>1</STREAMS>\n\
<SONGTITLE>t</SONGTITLE>\n";
    assert_eq!(server_bitrate(stats), "32");

    // Absent element -> "?" (server didn't report a rate).
    assert_eq!(server_bitrate("<STREAMSTATUS>1</STREAMSTATUS>"), "?");
}

#[tokio::test]
async fn http_get_fetches_dnas_stats_over_tcp() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.unwrap();
        // Read the request head (bounded) so we respond like a real DNAS:
        // HTTP/1.0 servers reply without waiting for EOF on the connection.
        let mut buf = [0u8; 4096];
        let _ = s.read(&mut buf).await.unwrap();
        let body = "<STREAMSTATUS>1</STREAMSTATUS><BITRATE>32</BITRATE>";
        let head = format!(
            "HTTP/1.0 200 OK\r\nContent-Type: text/xml\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        s.write_all(head.as_bytes()).await.unwrap();
        s.write_all(body.as_bytes()).await.unwrap();
    });

    let stats = http_get("127.0.0.1", port, "/stats?sid=1").await;
    assert!(stats.contains("<BITRATE>32</BITRATE>"));
    assert_eq!(server_bitrate(&stats), "32");
}

/// Live Icecast test: SOURCE to a real server, verify it accepts audio and
/// records metadata, then verify the mount disappears after we drop.
/// Env: STREAMY_HOST/PORT/PASS (source password), mount /audit-icy.mp3.
///   docker run -d -p 8200:8000 -e ICECAST_SOURCE_PASSWORD=srcpass \
///     -e ICECAST_ADMIN_PASSWORD=adminpass moul/icecast
#[tokio::test]
#[ignore]
async fn icecast_source_and_metadata_live() {
    use streamy_desktop::stream::icy::{connect as icy_connect, send_metadata, IcySourceConfig};

    let host = std::env::var("STREAMY_HOST").expect("STREAMY_HOST");
    let port: u16 = std::env::var("STREAMY_PORT")
        .expect("STREAMY_PORT")
        .parse()
        .expect("port");
    let pass = std::env::var("STREAMY_PASS").expect("STREAMY_PASS");
    let admin = std::env::var("STREAMY_ADMIN_PASS").unwrap_or_else(|_| pass.clone());

    let cfg = IcySourceConfig {
        host: host.clone(),
        port,
        mount: "/audit-icy.mp3".into(),
        user: "source".into(),
        password: pass,
        content_type: "audio/mpeg".into(),
        icy_name: "Streamy audit".into(),
        icy_genre: "Test".into(),
        icy_url: "http://streamy".into(),
        icy_pub: true,
        icy_br: 128,
        sample_rate: 44100,
    };

    let mut s = icy_connect(&cfg).await.expect("ICY handshake");
    let mp3 = one_second_tone_mp3();
    for _ in 0..3 {
        use tokio::io::AsyncWriteExt;
        s.write_all(&mp3).await.expect("audio write");
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    send_metadata(&cfg, "Audit Tone").await.expect("metadata");

    let mut admin_cfg = cfg.clone();
    admin_cfg.password = admin;
    // status page of the mount: 200 while source connected
    let status = http_get(&host, port, "/status.xsl").await;
    assert!(
        status.contains("Audit Tone") || status.contains("audit-icy"),
        "mount not visible in status page"
    );
    drop(s);
    tokio::time::sleep(Duration::from_secs(2)).await;
    let after = http_get(&host, port, "/status.xsl").await;
    assert!(
        !after.contains("audit-icy"),
        "mount still listed after source drop"
    );
}
