// Icecast-2 / Liquidsoap-harbor HTTP source protocol. Opens a TCP connection,
// sends the HTTP `SOURCE <mount>` handshake with `Authorization: Basic` + the
// `ice-*` metadata headers, then streams raw codec bytes over the same socket.
// Verified against Liquidsoap `input.harbor()` (az1.radioislam.my.id) and
// standard Icecast 2.x. Now-playing metadata is pushed to the `/admin/metadata`
// admin endpoint over its own short-lived connection — the source socket itself
// only carries audio.

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Clone)]
pub struct IcySourceConfig {
    pub host: String,
    pub port: u16,
    /// Mountpoint, e.g. "/" or "/stream". Leading slash is enforced.
    pub mount: String,
    /// Basic-auth username. Required by harbor/Icecast-2 source auth; this is
    /// the DJ/source account, paired with the source password below.
    pub user: String,
    pub password: String,
    pub content_type: String, // e.g. "audio/mpeg"
    pub icy_name: String,
    pub icy_genre: String,
    pub icy_url: String,
    pub icy_pub: bool,
    pub icy_br: u32,      // kbps
    pub sample_rate: u32, // for the ice-audio-info header
}

/// Connect + run the Icecast HTTP source handshake. On success the owned
/// `TcpStream` is returned in audio-transfer mode (write raw MP3/AAC bytes).
pub async fn connect(cfg: &IcySourceConfig) -> Result<TcpStream, String> {
    let mut stream = TcpStream::connect((cfg.host.as_str(), cfg.port))
        .await
        .map_err(|e| e.to_string())?;
    let req = source_request(cfg);
    stream
        .write_all(req.as_bytes())
        .await
        .map_err(|e| e.to_string())?;

    let head = read_http_head(&mut stream).await?;
    match status_code(&head).unwrap_or(0) {
        200 => Ok(stream),
        401 => Err("authentication failed (bad source user/password)".into()),
        403 => Err("mountpoint in use or forbidden".into()),
        c => Err(format!(
            "ICY source rejected: HTTP {c} ({})",
            first_line(&head)
        )),
    }
}

/// Update now-playing metadata via the Icecast admin endpoint. Opens its own
/// connection; a failure here does NOT tear down the audio source (the caller
/// treats metadata errors as non-fatal for the Icecast path). The whole round
/// trip is bounded — metadata is best-effort and must never stall the audio
/// streamer (it runs on the same hot select! arm that drains audio).
pub async fn send_metadata(cfg: &IcySourceConfig, title: &str) -> Result<(), String> {
    let mount = normalize_mount(&cfg.mount);
    let path = format!(
        "/admin/metadata?mode=updinfo&mount={}&song={}",
        url_encode(&mount),
        url_encode(title)
    );
    let auth = base64_encode(format!("{}:{}", cfg.user, cfg.password).as_bytes());
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nUser-Agent: Streamy/1.0\r\n\
         Authorization: Basic {auth}\r\nConnection: close\r\n\r\n",
        path = path,
        host = cfg.host,
        port = cfg.port,
        auth = auth,
    );
    let work = async {
        let mut stream = TcpStream::connect((cfg.host.as_str(), cfg.port))
            .await
            .map_err(|e| e.to_string())?;
        stream
            .write_all(req.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        let head = read_http_head(&mut stream).await?;
        match status_code(&head).unwrap_or(0) {
            200 => Ok(()),
            401 => Err("metadata auth failed".into()),
            c => Err(format!("metadata rejected: HTTP {c}")),
        }
    };
    match tokio::time::timeout(Duration::from_secs(5), work).await {
        Ok(r) => r,
        Err(_) => Err("metadata update timed out".into()),
    }
}

/// Build the HTTP `SOURCE` request for the handshake.
fn source_request(cfg: &IcySourceConfig) -> String {
    let mount = normalize_mount(&cfg.mount);
    let auth = base64_encode(format!("{}:{}", cfg.user, cfg.password).as_bytes());
    format!(
        "SOURCE {mount} HTTP/1.1\r\n\
         Host: {host}:{port}\r\n\
         User-Agent: Streamy/1.0\r\n\
         Authorization: Basic {auth}\r\n\
         Content-Type: {ct}\r\n\
         ice-name: {name}\r\n\
         ice-genre: {genre}\r\n\
         ice-url: {url}\r\n\
         ice-public: {pub}\r\n\
         ice-bitrate: {br}\r\n\
         ice-audio-info: ice-samplerate={sr};ice-bitrate={br};ice-channels=2\r\n\
         \r\n",
        mount = mount,
        host = cfg.host,
        port = cfg.port,
        auth = auth,
        ct = cfg.content_type,
        name = cfg.icy_name,
        genre = cfg.icy_genre,
        url = cfg.icy_url,
        pub = cfg.icy_pub as u8,
        br = cfg.icy_br,
        sr = cfg.sample_rate,
    )
}

/// Read the HTTP response head (status line + headers) up to the terminating
/// blank line. Bounded so a malformed server can't hang us forever; the caller
/// also wraps connect in a timeout.
async fn read_http_head(stream: &mut TcpStream) -> Result<String, String> {
    let mut buf: Vec<u8> = Vec::with_capacity(256);
    let mut b = [0u8; 1];
    loop {
        let n = stream.read(&mut b).await.map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("server closed connection during handshake".into());
        }
        buf.push(b[0]);
        if buf.ends_with(b"\r\n\r\n") || buf.len() > 4096 {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn first_line(head: &str) -> &str {
    head.lines().next().unwrap_or("")
}

fn status_code(head: &str) -> Option<u16> {
    first_line(head).split_whitespace().nth(1)?.parse().ok()
}

/// "/" stays "/"; "stream" -> "/stream"; "" -> "/".
fn normalize_mount(m: &str) -> String {
    let m = m.trim();
    if m.is_empty() || m == "/" {
        "/".to_string()
    } else if m.starts_with('/') {
        m.to_string()
    } else {
        format!("/{m}")
    }
}

/// Percent-encode for a query string (RFC 3986 unreserved kept).
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Minimal RFC 4648 base64 encoder (no new dep needed).
pub fn base64_encode(input: &[u8]) -> String {
    const TBL: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
        out.push(TBL[((n >> 18) & 63) as usize] as char);
        out.push(TBL[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            TBL[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TBL[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn base64_known_vectors() {
        assert_eq!(base64_encode(b"user:pass"), "dXNlcjpwYXNz");
        assert_eq!(base64_encode(b"alice:secret"), "YWxpY2U6c2VjcmV0");
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"a"), "YQ==");
        assert_eq!(base64_encode(b"ab"), "YWI=");
        assert_eq!(base64_encode(b"abc"), "YWJj");
    }

    #[test]
    fn url_encode_keeps_unreserved() {
        assert_eq!(url_encode("A-z.0_~"), "A-z.0_~");
    }

    #[test]
    fn url_encode_special() {
        // space -> %20, slash -> %2F, & -> %26, non-ASCII -> UTF-8 percent bytes.
        assert_eq!(url_encode("a b/c&d"), "a%20b%2Fc%26d");
        assert_eq!(url_encode("/"), "%2F");
        assert_eq!(url_encode("é"), "%C3%A9");
    }

    #[test]
    fn normalize_mount_cases() {
        assert_eq!(normalize_mount("/"), "/");
        assert_eq!(normalize_mount(""), "/");
        assert_eq!(normalize_mount("stream"), "/stream");
        assert_eq!(normalize_mount("/stream"), "/stream");
        assert_eq!(normalize_mount(" /live "), "/live");
    }

    #[test]
    fn source_request_shape() {
        let cfg = IcySourceConfig {
            host: "h".into(),
            port: 8000,
            mount: "/stream".into(),
            user: "dj".into(),
            password: "pw".into(),
            content_type: "audio/mpeg".into(),
            icy_name: "N".into(),
            icy_genre: "G".into(),
            icy_url: "U".into(),
            icy_pub: true,
            icy_br: 128,
            sample_rate: 44100,
        };
        let r = source_request(&cfg);
        // Request line + required headers.
        assert!(r.starts_with("SOURCE /stream HTTP/1.1\r\n"));
        assert!(
            r.contains("Authorization: Basic ZGo6cHc=\r\n"),
            "basic auth header"
        );
        assert!(r.contains("Content-Type: audio/mpeg\r\n"));
        assert!(r.contains("ice-public: 1\r\n"));
        assert!(
            r.contains("ice-audio-info: ice-samplerate=44100;ice-bitrate=128;ice-channels=2\r\n")
        );
        assert!(r.ends_with("\r\n\r\n"));
    }

    fn cfg_from_env() -> Option<IcySourceConfig> {
        Some(IcySourceConfig {
            host: env::var("STREAMY_ICE_HOST").ok()?,
            port: env::var("STREAMY_ICE_PORT").ok()?.parse().ok()?,
            user: env::var("STREAMY_ICE_USER").ok()?,
            password: env::var("STREAMY_ICE_PASS").ok()?,
            mount: env::var("STREAMY_ICE_MOUNT").unwrap_or_else(|_| "/".into()),
            content_type: "audio/mpeg".into(),
            icy_name: "Streamy live test".into(),
            icy_genre: "Test".into(),
            icy_url: "".into(),
            icy_pub: true,
            icy_br: 128,
            sample_rate: 44100,
        })
    }

    /// Live integration: connect to a real Icecast/Liquidsoap-harbor server,
    /// push ~2s of MP3 audio, then update metadata. Credentials come from env
    /// (STREAMY_ICE_HOST/PORT/USER/PASS/MOUNT) so nothing is committed. Run:
    ///   cargo test --lib icy_live_connect_metadata -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn icy_live_connect_metadata() {
        let cfg = match cfg_from_env() {
            Some(c) => c,
            None => {
                eprintln!("skip: set STREAMY_ICE_HOST/PORT/USER/PASS[/MOUNT]");
                return;
            }
        };
        let mut stream = connect(&cfg).await.expect("icy connect succeeded");

        let enc_cfg = crate::encode::EncoderConfig {
            sample_rate: 44100,
            channels: 2,
            bitrate_kbps: 128,
            codec: crate::encode::Codec::Mp3,
        };
        let mut enc = crate::encode::build(&enc_cfg).expect("encoder built");
        let frames_per_chunk = 4410usize; // 0.1s @ 44100Hz
        let total_chunks = 20; // 2s of audio
        let two_pi = 2.0 * std::f64::consts::PI;
        for ci in 0..total_chunks {
            let mut samples = Vec::with_capacity(frames_per_chunk * 2);
            for i in 0..frames_per_chunk {
                let t = ((ci * frames_per_chunk) + i) as f64 / 44100.0;
                let v = (two_pi * 440.0 * t).sin() as f32 * 0.5;
                samples.push(v);
                samples.push(v);
            }
            let mp3 = enc.encode(&samples).expect("encode chunk");
            stream.write_all(&mp3).await.expect("write audio chunk");
            // Light pacing so we don't burst 2s in one tick (harbor is fine with
            // it, but this mirrors the realtime producer in the real engine).
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }

        send_metadata(&cfg, "Streamy live test — Icecast")
            .await
            .expect("metadata update accepted");
        // Reaching here means the handshake held through 2s of audio and the
        // metadata admin endpoint returned 200 OK.
    }
}
