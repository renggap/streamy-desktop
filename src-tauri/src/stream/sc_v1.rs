// SHOUTcast v1 (DNAS1 / ICY) source protocol. The original 1999 wire format:
// connect, send "<password>\r\n", read "OK2\r\n<caps>\r\n\r\n", send the icy-*
// header block, then stream raw MP3 bytes. No HTTP on the source socket.
//
// DNAS2 (SHOUTcast v2) still accepts v1 connections — on the main port they
// bind to stream id 1 (use Ultravox when a specific sid > 1 is needed) and on
// port+1 for the legacy listener/source split. Verified against
// Shoutcast DNAS/posix(linux x64) v2.6.1.777: password-only and "user:pass"
// lines both return OK2; we send password-only (the canonical DNAS1 form).
//
// Now-playing metadata goes to the admin.cgi GET endpoint over its own
// connection, mirroring the Icecast /admin/metadata path (non-fatal on error).

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Clone)]
pub struct ScV1Config {
    pub host: String,
    pub port: u16,
    /// Source password. v1 has no username concept for source accounts.
    pub password: String,
    pub icy_name: String,
    pub icy_genre: String,
    pub icy_url: String,
    pub icy_pub: bool,
    pub icy_br: u32, // kbps
}

/// Build the v1 password line: `"<password>\r\n"`.
pub fn password_line(password: &str) -> String {
    format!("{password}\r\n")
}

/// Build the v1 icy-* header block (terminated by a blank line).
pub fn icy_headers(cfg: &ScV1Config) -> String {
    format!(
        "icy-name:{name}\r\n\
         icy-genre:{genre}\r\n\
         icy-url:{url}\r\n\
         icy-public:{pub_}\r\n\
         icy-br:{br}\r\n\
         \r\n",
        name = cfg.icy_name,
        genre = cfg.icy_genre,
        url = cfg.icy_url,
        pub_ = cfg.icy_pub as u8,
        br = cfg.icy_br,
    )
}

/// Read the v1 auth response up to the blank line ("OK2\r\nicy-caps:N\r\n\r\n").
async fn read_auth_response(stream: &mut TcpStream) -> Result<String, String> {
    let mut buf: Vec<u8> = Vec::with_capacity(64);
    let mut b = [0u8; 1];
    loop {
        let n = stream.read(&mut b).await.map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("server closed connection during v1 auth".into());
        }
        buf.push(b[0]);
        if buf.ends_with(b"\r\n\r\n") || buf.ends_with(b"\n\n") || buf.len() > 1024 {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Connect + run the SHOUTcast v1 source handshake. On success the owned
/// `TcpStream` is returned in audio-transfer mode (write raw MP3 bytes).
pub async fn connect(cfg: &ScV1Config) -> Result<TcpStream, String> {
    let mut stream = TcpStream::connect((cfg.host.as_str(), cfg.port))
        .await
        .map_err(|e| e.to_string())?;
    stream
        .write_all(password_line(&cfg.password).as_bytes())
        .await
        .map_err(|e| e.to_string())?;

    let resp = read_auth_response(&mut stream).await?;
    if resp.starts_with("OK2") {
        stream
            .write_all(icy_headers(cfg).as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        Ok(stream)
    } else if resp.to_lowercase().contains("invalid password") {
        Err("v1 authentication failed (bad source password)".into())
    } else {
        Err(format!("v1 source rejected: {}", resp.trim()))
    }
}

/// Update now-playing via the v1 admin.cgi endpoint (own connection, bounded).
pub async fn send_metadata(cfg: &ScV1Config, title: &str) -> Result<(), String> {
    let path = format!(
        "/admin.cgi?pass={}&mode=updinfo&song={}",
        crate::stream::icy::url_encode(&cfg.password),
        crate::stream::icy::url_encode(title)
    );
    let req = format!(
        "GET {path} HTTP/1.0\r\nHost: {host}:{port}\r\nUser-Agent: Streamy/1.0\r\n\r\n",
        path = path,
        host = cfg.host,
        port = cfg.port,
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

/// Minimal HTTP response-head reader for the admin GET (status + headers).
async fn read_http_head(stream: &mut TcpStream) -> Result<String, String> {
    let mut buf: Vec<u8> = Vec::with_capacity(256);
    let mut b = [0u8; 1];
    loop {
        let n = stream.read(&mut b).await.map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("server closed connection during metadata".into());
        }
        buf.push(b[0]);
        if buf.ends_with(b"\r\n\r\n") || buf.len() > 4096 {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn status_code(head: &str) -> Option<u16> {
    head.lines().next()?.split_whitespace().nth(1)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> ScV1Config {
        ScV1Config {
            host: "h".into(),
            port: 8000,
            password: "pw".into(),
            icy_name: "N".into(),
            icy_genre: "G".into(),
            icy_url: "U".into(),
            icy_pub: true,
            icy_br: 128,
        }
    }

    #[test]
    fn password_line_shape() {
        assert_eq!(password_line("liverelay1234"), "liverelay1234\r\n");
    }

    #[test]
    fn icy_headers_shape() {
        let h = icy_headers(&cfg());
        assert!(h.starts_with("icy-name:N\r\n"));
        assert!(h.contains("icy-genre:G\r\n"));
        assert!(h.contains("icy-public:1\r\n"));
        assert!(h.contains("icy-br:128\r\n"));
        assert!(h.ends_with("\r\n\r\n"));
    }

    #[test]
    fn auth_response_parser_cases() {
        // OK2 + caps block is accepted; password failure maps to a clear error.
        let ok = "OK2\r\nicy-caps:11\r\n\r\n";
        assert!(ok.starts_with("OK2"));
        assert!("invalid password!".to_lowercase().contains("invalid password"));
    }

    #[test]
    fn status_code_parses() {
        assert_eq!(status_code("HTTP/1.0 200 OK\r\n"), Some(200));
        assert_eq!(status_code("garbage"), None);
    }

    /// Bad password on a DNAS answers "Invalid Password\r\n" with NO blank
    /// line — the auth reader must surface that as an error (EOF after the
    /// line), not hang or misparse. Live; creds via env.
    ///   cargo test --lib sc_v1_bad_password_rejected -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn sc_v1_bad_password_rejected() {
        let mut cfg = match cfg_from_env() {
            Some(c) => c,
            None => {
                eprintln!("skip: set STREAMY_V1_HOST/PORT/PASS");
                return;
            }
        };
        cfg.password = "definitely-wrong".into();
        let err = connect(&cfg)
            .await
            .expect_err("bad password must be rejected");
        assert!(
            err.contains("closed") || err.to_lowercase().contains("invalid password"),
            "unexpected error for bad password: {err}"
        );
    }

    fn cfg_from_env() -> Option<ScV1Config> {
        Some(ScV1Config {
            host: std::env::var("STREAMY_V1_HOST").ok()?,
            port: std::env::var("STREAMY_V1_PORT").ok()?.parse().ok()?,
            password: std::env::var("STREAMY_V1_PASS").ok()?,
            icy_name: "Streamy v1 live test".into(),
            icy_genre: "Test".into(),
            icy_url: "".into(),
            icy_pub: true,
            icy_br: std::env::var("STREAMY_V1_BR")
                .ok()
                .and_then(|b| b.parse().ok())
                .unwrap_or(128),
        })
    }

    /// Live integration: v1 handshake + ~2s of MP3 + admin.cgi metadata update.
    /// Creds via env (STREAMY_V1_HOST/PORT/PASS), never committed. Run:
    ///   cargo test --lib sc_v1_live_connect_metadata -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn sc_v1_live_connect_metadata() {
        let cfg = match cfg_from_env() {
            Some(c) => c,
            None => {
                eprintln!("skip: set STREAMY_V1_HOST/PORT/PASS");
                return;
            }
        };
        let mut stream = connect(&cfg).await.expect("v1 connect succeeded");

        let enc_cfg = crate::encode::EncoderConfig {
            sample_rate: 44100,
            channels: 2,
            bitrate_kbps: cfg.icy_br,
            codec: crate::encode::Codec::Mp3,
        };
        let mut enc = crate::encode::build(&enc_cfg).expect("encoder built");
        let frames_per_chunk = 4410usize; // 0.1s @ 44100Hz
        let two_pi = 2.0 * std::f64::consts::PI;
        for ci in 0..20 {
            let mut samples = Vec::with_capacity(frames_per_chunk * 2);
            for i in 0..frames_per_chunk {
                let t = ((ci * frames_per_chunk) + i) as f64 / 44100.0;
                let v = (two_pi * 440.0 * t).sin() as f32 * 0.5;
                samples.push(v);
                samples.push(v);
            }
            let mp3 = enc.encode(&samples).expect("encode chunk");
            stream.write_all(&mp3).await.expect("write audio chunk");
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
        send_metadata(&cfg, "Streamy live test - SHOUTcast v1")
            .await
            .expect("metadata update accepted");
    }
}
