// P6: Ultravox 2.1 source protocol for SHOUTcast v2 (DNAS2). Reverse-engineered
// reference: binary frames (0x5A sync), XTEA-obfuscated auth, then MP3 data
// frames (0x7000). DNAS2 does NOT accept the Icecast HTTP SOURCE handshake.

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const DELTA: u32 = 0x9E3779B9;

/// XTEA encipher one 8-byte block (32 rounds, big-endian words).
fn encipher_block(mut v0: u32, mut v1: u32, k: &[u32; 4]) -> (u32, u32) {
    let mut sum = 0u32;
    for _ in 0..32 {
        let t = (v1 << 4 ^ v1 >> 5).wrapping_add(v1);
        v0 = v0.wrapping_add(t ^ sum.wrapping_add(k[(sum & 3) as usize]));
        sum = sum.wrapping_add(DELTA);
        let t2 = (v0 << 4 ^ v0 >> 5).wrapping_add(v0);
        v1 = v1.wrapping_add(t2 ^ sum.wrapping_add(k[((sum >> 11) & 3) as usize]));
    }
    (v0, v1)
}

/// XTEA-encipher `data` with ASCII `key` (zero-padded to 16 bytes), output as
/// lowercase hex. Empty data -> empty string. Empty key -> all-zero key (Centova).
pub fn xtea_encipher_hex(data: &[u8], key: &[u8]) -> String {
    if data.is_empty() {
        return String::new();
    }
    let mut kbuf = [0u8; 16];
    let klen = key.len().min(16);
    kbuf[..klen].copy_from_slice(&key[..klen]);
    let k = [
        u32::from_be_bytes(kbuf[0..4].try_into().unwrap()),
        u32::from_be_bytes(kbuf[4..8].try_into().unwrap()),
        u32::from_be_bytes(kbuf[8..12].try_into().unwrap()),
        u32::from_be_bytes(kbuf[12..16].try_into().unwrap()),
    ];
    let pad = (8 - data.len() % 8) % 8;
    let mut dbuf = data.to_vec();
    dbuf.resize(data.len() + pad, 0);
    let mut out = String::with_capacity(dbuf.len() * 2);
    for chunk in dbuf.chunks(8) {
        let v0 = u32::from_be_bytes(chunk[0..4].try_into().unwrap());
        let v1 = u32::from_be_bytes(chunk[4..8].try_into().unwrap());
        let (a, b) = encipher_block(v0, v1, &k);
        out.push_str(&format!("{a:08x}{b:08x}"));
    }
    out
}

/// ASCIIZ payload bytes (string + terminating NUL).
fn asciiz(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

/// Encode one Ultravox frame: [0x5A][0x00][id BE][len BE][payload][0x00].
pub fn encode_frame(id: u16, payload: &[u8]) -> Vec<u8> {
    let len = payload.len() as u16;
    let mut f = Vec::with_capacity(6 + payload.len() + 1);
    f.push(0x5A);
    f.push(0x00);
    f.extend_from_slice(&id.to_be_bytes());
    f.extend_from_slice(&len.to_be_bytes());
    f.extend_from_slice(payload);
    f.push(0x00);
    f
}

/// Read one Ultravox frame -> (id, payload).
pub async fn read_frame<R: AsyncRead + Unpin>(r: &mut R) -> Result<(u16, Vec<u8>), String> {
    let mut hdr = [0u8; 6];
    r.read_exact(&mut hdr).await.map_err(|e| e.to_string())?;
    if hdr[0] != 0x5A {
        return Err(format!("bad sync byte {:#04x}", hdr[0]));
    }
    let id = u16::from_be_bytes([hdr[2], hdr[3]]);
    let len = u16::from_be_bytes([hdr[4], hdr[5]]) as usize;
    let mut payload = vec![0u8; len];
    if len > 0 {
        r.read_exact(&mut payload).await.map_err(|e| e.to_string())?;
    }
    let mut trailer = [0u8; 1];
    r.read_exact(&mut trailer).await.map_err(|e| e.to_string())?;
    Ok((id, payload))
}

/// Read a frame and require its ASCIIZ payload to start with "ACK".
async fn expect_ack<R: AsyncRead + Unpin>(r: &mut R, what: &str) -> Result<String, String> {
    let (id, payload) = read_frame(r).await?;
    let s = String::from_utf8_lossy(&payload)
        .trim_end_matches('\0')
        .trim_end_matches('\u{0}')
        .to_string();
    if s.starts_with("ACK") {
        Ok(s)
    } else {
        Err(format!("{what} rejected: id={id:#06x} resp={s:?}"))
    }
}

async fn send_control<R: AsyncWriteExt + Unpin>(
    s: &mut R,
    id: u16,
    payload: Vec<u8>,
) -> Result<(), String> {
    s.write_all(&encode_frame(id, &payload))
        .await
        .map_err(|e| e.to_string())
}

#[derive(Clone)]
pub struct UltravoxConfig {
    pub host: String,
    pub port: u16,
    pub stream_id: u32,
    /// Source password OR DJ password (when `user` is set, this is the DJ's).
    pub password: String,
    /// DJ userid for DNASv2 DJ login. Empty = source-password connection.
    /// DJ login only works over Ultravox (not ICY).
    pub user: String,
    pub content_type: String, // "audio/mpeg"
    pub bitrate_kbps: u32,
    pub name: String,
    pub genre: String,
    pub url: String,
}

/// Build the Ultravox 2.1 auth payload (message 0x1001):
/// `2.1:<stream_id>:<user_id>:<xtea(password)>\0`
/// Empty `user_id` = source-password login; non-empty = DNASv2 DJ login.
pub fn auth_payload(stream_id: u32, user_id: &str, xtea_password_hex: &str) -> Vec<u8> {
    format!("2.1:{stream_id}:{user_id}:{xtea_password_hex}\0").into_bytes()
}

/// Build the Ultravox 2.1 "Stream Setup" payload (message 0x1002):
/// `<bitrate>:<bitrate>\0` (DNAS expects the source's nominal kbps).
/// Kept as a pure helper so tests can pin the wire value to the configured
/// bitrate — a regression here would make the DNAS display the wrong rate.
pub fn stream_setup_payload(bitrate_kbps: u32) -> Vec<u8> {
    format!("{bitrate_kbps}:{bitrate_kbps}\0").into_bytes()
}

/// Run the Ultravox 2.1 source handshake. On success, returns the owned
/// `TcpStream` in data-transfer mode for streaming audio frames.
pub async fn connect(cfg: &UltravoxConfig) -> Result<TcpStream, String> {
    let mut s = TcpStream::connect((cfg.host.as_str(), cfg.port))
        .await
        .map_err(|e| e.to_string())?;

    // 1. Request Cipher -> "ACK:<key>" (key empty on Centova)
    send_control(&mut s, 0x1009, b"2.1\0".to_vec()).await?;
    let cipher_resp = expect_ack(&mut s, "cipher").await?;
    let key = cipher_resp
        .split_once(':')
        .map(|(_, k)| k)
        .unwrap_or("")
        .as_bytes()
        .to_vec();

    // 2. Authenticate: "2.1:<sid>:<uid>:<xtea(pwd)>" — uid empty = source login,
    // uid = DJ username for DNASv2 DJ login.
    let xpwd = xtea_encipher_hex(cfg.password.as_bytes(), &key);
    let auth = auth_payload(cfg.stream_id, &cfg.user, &xpwd);
    send_control(&mut s, 0x1001, auth).await?;
    expect_ack(&mut s, "auth").await?;

    // 3. Stream MIME Type
    send_control(&mut s, 0x1040, asciiz(&cfg.content_type)).await?;
    expect_ack(&mut s, "mime").await?;
    // 4. Stream Setup
    send_control(&mut s, 0x1002, stream_setup_payload(cfg.bitrate_kbps)).await?;
    expect_ack(&mut s, "setup").await?;
    // 5. Stream Buffer
    send_control(&mut s, 0x1003, b"2048:2048\0".to_vec()).await?;
    expect_ack(&mut s, "buffer").await?;
    // 6-9. ICY metadata
    send_control(&mut s, 0x1100, asciiz(&cfg.name)).await?;
    expect_ack(&mut s, "icy-name").await?;
    send_control(&mut s, 0x1101, asciiz(&cfg.genre)).await?;
    expect_ack(&mut s, "icy-genre").await?;
    send_control(&mut s, 0x1102, asciiz(&cfg.url)).await?;
    expect_ack(&mut s, "icy-url").await?;
    send_control(&mut s, 0x1103, b"1\0".to_vec()).await?;
    expect_ack(&mut s, "icy-public").await?;
    // 10. Max Payload
    send_control(&mut s, 0x1008, b"16377:16377\0".to_vec()).await?;
    expect_ack(&mut s, "max-payload").await?;
    // 11. Standby -> "ACK:Data transfer mode"
    send_control(&mut s, 0x1004, b"\0".to_vec()).await?;
    expect_ack(&mut s, "standby").await?;

    Ok(s)
}

/// Send an MP3 data frame (class 7, type 0x000).
pub async fn send_mp3<R: AsyncWriteExt + Unpin>(s: &mut R, mp3: &[u8]) -> Result<(), String> {
    s.write_all(&encode_frame(0x7000, mp3))
        .await
        .map_err(|e| e.to_string())
}

/// Update now-playing (SONGTITLE) via the admin-free metadata message 0x3902.
pub async fn send_metadata<R: AsyncWriteExt + Unpin>(s: &mut R, title: &str) -> Result<(), String> {
    let esc = title
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><metadata><TIT2>{esc}</TIT2></metadata>"
    );
    let mut payload = Vec::new();
    payload.extend_from_slice(&1u16.to_be_bytes()); // MetadataID
    payload.extend_from_slice(&1u16.to_be_bytes()); // Span
    payload.extend_from_slice(&1u16.to_be_bytes()); // Index
    payload.extend_from_slice(xml.as_bytes());
    s.write_all(&encode_frame(0x3902, &payload))
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xtea_golden_foobar() {
        assert_eq!(xtea_encipher_hex(b"test", b"foobar"), "0797c1d44e31a1f3");
    }

    #[test]
    fn xtea_empty_data_is_empty() {
        assert_eq!(xtea_encipher_hex(b"", b"foobar"), "");
    }

    #[test]
    fn xtea_empty_key_is_zero_key() {
        // Non-empty data with empty key must produce output (zero key path).
        assert!(!xtea_encipher_hex(b"liverelay1234", b"").is_empty());
    }

    #[test]
    fn xtea_multiblock_vectors() {
        // Independent cross-language vectors (Python reference impl).
        assert_eq!(xtea_encipher_hex(b"0123456789abcdef", b"key"), "f88e91fd16781ccc3c2179618d1f0e73");
        assert_eq!(xtea_encipher_hex(b"0123456789abcdefghijklmn", b"secretkey"), "2c4b6945e9051931c98804142a064a27d2a7a1d6327c3ff0");
        assert_eq!(xtea_encipher_hex("pässwörd".as_bytes(), b"1234"), "89393495b864a76553db9412d1b31799");
        assert_eq!(xtea_encipher_hex(b"password", b"this-is-a-very-long-key-over-16-bytes"), "a6e178953174cb08");
        assert_eq!(xtea_encipher_hex(b"liverelay1234", b""), "53b52f6722513f7d9a871496cc6a58a8");
    }

    #[test]
    fn auth_payload_source_login_has_empty_uid() {
        // No DJ userid -> empty field between sid and password (source login).
        let p = auth_payload(1, "", "deadbeef");
        assert_eq!(p, b"2.1:1::deadbeef\0".to_vec());
    }

    #[test]
    fn auth_payload_dj_login_includes_uid() {
        // DJ login: userid populated, DJ password (already xtea'd) follows.
        let p = auth_payload(3, "nadia", "cafe");
        assert_eq!(p, b"2.1:3:nadia:cafe\0".to_vec());
    }

    #[test]
    fn frame_round_trip() {
        let f = encode_frame(0x1009, b"2.1\0");
        assert_eq!(f, vec![0x5A, 0x00, 0x10, 0x09, 0x00, 0x04, 0x32, 0x2E, 0x31, 0x00, 0x00]);
    }

    #[test]
    fn stream_setup_payload_is_bitrate_bitrate() {
        // The DNAS displays this value; it must match the configured bitrate
        // exactly (regression: 24 must never become 32 on the wire).
        assert_eq!(stream_setup_payload(24), b"24:24\0");
        assert_eq!(stream_setup_payload(128), b"128:128\0");
        assert_eq!(stream_setup_payload(320), b"320:320\0");
    }

    #[test]
    fn cipher_request_bytes_match_doc_example() {
        // Doc §10: id 0x1009, payload "2.1\0" (4 bytes).
        let f = encode_frame(0x1009, b"2.1\0");
        // header: 5A 00 10 09 00 04 ; payload 32 2E 31 00 ; trailer 00
        assert_eq!(&f[..6], &[0x5A, 0x00, 0x10, 0x09, 0x00, 0x04]);
        assert_eq!(&f[6..10], &[0x32, 0x2E, 0x31, 0x00]);
        assert_eq!(*f.last().unwrap(), 0x00);
    }
}
