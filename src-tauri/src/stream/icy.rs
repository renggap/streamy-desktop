// P2.6: Icecast/ICY HTTP source protocol. Opens a TCP connection, sends the
// SOURCE handshake with icy-* headers + basic auth, then streams encoded audio
// bytes. Works against Icecast and (per the design) a Shoutcast v2 DNAS.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub struct IcySourceConfig {
    pub host: String,
    pub port: u16,
    pub mount: String,
    pub user: String,
    pub password: String,
    pub content_type: String, // e.g. "audio/mpeg"
    pub icy_name: String,
    pub icy_genre: String,
    pub icy_url: String,
    pub icy_pub: bool,
    pub icy_br: u32, // kbps
}

/// Connect + run the ICY source handshake. On success the owned `TcpStream`
/// is returned for streaming audio bytes.
pub async fn connect(cfg: &IcySourceConfig) -> Result<TcpStream, String> {
    let mut stream = TcpStream::connect((cfg.host.as_str(), cfg.port))
        .await
        .map_err(|e| e.to_string())?;

    let auth = base64_encode(format!("{}:{}", cfg.user, cfg.password).as_bytes());
    let req = format!(
        "SOURCE {mount} HTTP/1.1\r\n\
         Host: {host}:{port}\r\n\
         Content-Type: {ct}\r\n\
         Authorization: Basic {auth}\r\n\
         icy-name: {name}\r\n\
         icy-genre: {genre}\r\n\
         icy-url: {url}\r\n\
         icy-pub: {pub_}\r\n\
         icy-br: {br}\r\n\
         \r\n",
        mount = cfg.mount,
        host = cfg.host,
        port = cfg.port,
        ct = cfg.content_type,
        auth = auth,
        name = cfg.icy_name,
        genre = cfg.icy_genre,
        url = cfg.icy_url,
        pub_ = cfg.icy_pub as u8,
        br = cfg.icy_br,
    );
    stream
        .write_all(req.as_bytes())
        .await
        .map_err(|e| e.to_string())?;

    // Read enough of the response to see the status line.
    let mut buf = vec![0u8; 128];
    let n = stream.read(&mut buf).await.map_err(|e| e.to_string())?;
    let resp = std::str::from_utf8(&buf[..n]).unwrap_or("");
    let status = resp.lines().next().unwrap_or("");
    if !resp.contains(" 200 ") {
        return Err(format!("ICY source rejected: {status}"));
    }
    Ok(stream)
}

/// Minimal RFC 4648 base64 encoder (no new dep needed).
pub fn base64_encode(input: &[u8]) -> String {
    const TBL: &[u8] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
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

    #[test]
    fn base64_known_vectors() {
        assert_eq!(base64_encode(b"user:pass"), "dXNlcjpwYXNz");
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"a"), "YQ==");
        assert_eq!(base64_encode(b"ab"), "YWI=");
        assert_eq!(base64_encode(b"abc"), "YWJj");
    }
}
