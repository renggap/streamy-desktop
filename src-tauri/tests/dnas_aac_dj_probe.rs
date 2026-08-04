//! Live probe: DJ over Liquidsoap harbor (Icecast SOURCE) with AAC 24kbps,
//! then read what the downstream Shoutcast DNAS reports. Run:
//!   cargo test --test dnas_aac_dj_probe -- --ignored --nocapture
//! (requires network access to the configured server)

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use streamy_desktop::encode::{build, Codec, EncoderConfig};
use streamy_desktop::stream::icy::{connect, IcySourceConfig};

const HOST: &str = "az1.radioislam.my.id";
const HARBOR_PORT: u16 = 8035; // Liquidsoap source harbor (DJ relay)
const DNAS_PORT: u16 = 8030; // Shoutcast DNAS
const DJ_USER: &str = "jcode";
const DJ_PASS: &str = "jcode12345";
const BITRATE_KBPS: u32 = 24;
const SAMPLE_RATE: u32 = 44_100;

/// Pull `<BITRATE>N</BITRATE>` (and SAMPLERATE / CONTENT) from a DNAS stats XML.
fn stats_field(stats: &str, field: &str) -> String {
    stats
        .split(&format!("<{field}>"))
        .nth(1)
        .and_then(|s| s.split(&format!("</{field}>")).next())
        .unwrap_or("?")
        .to_string()
}

async fn http_get(host: &str, port: u16, path: &str) -> String {
    let mut s = tokio::net::TcpStream::connect((host, port)).await.unwrap();
    let req = format!(
        "GET {path} HTTP/1.0\r\nHost: {host}:{port}\r\nUser-Agent: streamy-probe\r\n\r\n"
    );
    s.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).await.unwrap();
    String::from_utf8_lossy(&buf).to_string()
}

#[tokio::test]
#[ignore]
async fn dnas_aac_dj_bitrate_probe() {
    // 1. Connect to the harbor with DJ creds (app's own icy handshake).
    let cfg = IcySourceConfig {
        host: HOST.into(),
        port: HARBOR_PORT,
        mount: "/".into(),
        user: DJ_USER.into(),
        password: DJ_PASS.into(),
        content_type: "audio/aacp".into(),
        icy_name: "Streamy AAC 24 probe".into(),
        icy_genre: "Test".into(),
        icy_url: String::new(),
        icy_pub: false,
        icy_br: BITRATE_KBPS,
        sample_rate: SAMPLE_RATE,
    };
    let mut stream = connect(&cfg).await.expect("harbor SOURCE handshake");

    // 2. Encode 6s of 440Hz tone with the real app encoder (fdk-aac, 24kbps).
    let enc_cfg = EncoderConfig {
        sample_rate: SAMPLE_RATE,
        channels: 2,
        bitrate_kbps: BITRATE_KBPS,
        codec: Codec::Aac,
    };
    let mut enc = build(&enc_cfg).expect("aac encoder");
    let mut out = Vec::new();
    let chunk = 4096usize;
    let total_frames = SAMPLE_RATE as usize * 6; // 6s of stereo frames
    let mut i = 0usize;
    while i < total_frames {
        let end = (i + chunk).min(total_frames);
        let mut samples = Vec::with_capacity((end - i) * 2);
        for n in i..end {
            let t = n as f32 / SAMPLE_RATE as f32;
            let v = (0.5 * (2.0 * std::f32::consts::PI * 440.0 * t).sin()) as f32;
            samples.push(v);
            samples.push(v);
        }
        out.extend_from_slice(&enc.encode(&samples).unwrap());
        i = end;
    }
    out.extend_from_slice(&enc.flush().unwrap());
    let actual_kbps = (out.len() as f64 * 8.0) / 1000.0 / 6.0;
    println!("PROBE encoder: AAC {BITRATE_KBPS} kbps -> {} bytes/6s = {actual_kbps:.1} kbps actual", out.len());

    // 3. Stream the AAC to the harbor, chunked + lightly paced.
    for chunk in out.chunks(4096) {
        stream.write_all(chunk).await.expect("write aac");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    // Hold the source briefly so the DNAS reflects it, then read stats.
    tokio::time::sleep(Duration::from_secs(2)).await;
    drop(stream);

    let stats = http_get(HOST, DNAS_PORT, "/stats?sid=1").await;
    let reported = stats_field(&stats, "BITRATE");
    let sr = stats_field(&stats, "SAMPLERATE");
    let content = stats_field(&stats, "CONTENT");
    println!(
        "PROBE DNAS: configured={BITRATE_KBPS} kbps server_reported={reported} kbps samplerate={sr} content={content}"
    );
    println!("PROBE VERDICT: {}", if reported == "24" { "MATCH (24)" } else { "MISMATCH (not 24)" });
}
