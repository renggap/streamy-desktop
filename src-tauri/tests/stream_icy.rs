use streamy_desktop::stream::icy::{connect, IcySourceConfig};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn cfg(host: String, port: u16) -> IcySourceConfig {
    IcySourceConfig {
        host,
        port,
        mount: "/test".into(),
        user: "user".into(),
        password: "pass".into(),
        content_type: "audio/mpeg".into(),
        icy_name: "Streamy".into(),
        icy_genre: "Misc".into(),
        icy_url: "http://streamy.example".into(),
        icy_pub: true,
        icy_br: 128,
        sample_rate: 44100,
    }
}

#[tokio::test]
async fn handshake_sends_icy_headers_and_streams_payload_intact() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let srv = tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut req = vec![0u8; 4096];
        let n = sock.read(&mut req).await.unwrap();
        let req = String::from_utf8_lossy(&req[..n]).to_string();

        sock.write_all(b"HTTP/1.0 200 OK\r\n\r\n").await.unwrap();

        let mut got = Vec::new();
        sock.read_to_end(&mut got).await.unwrap();
        (req, got)
    });

    let mut client = connect(&cfg(addr.ip().to_string(), addr.port()))
        .await
        .expect("handshake");
    let audio: &[u8] = b"\xff\xfb\x90\x00 here are some mp3-ish bytes \xff\xfb";
    client.write_all(audio).await.unwrap();
    drop(client);

    let (req, got) = srv.await.unwrap();
    assert!(req.starts_with("SOURCE /test HTTP/1.1"), "bad request line: {req}");
    assert!(req.contains("Content-Type: audio/mpeg"));
    assert!(req.contains("Authorization: Basic dXNlcjpwYXNz")); // base64("user:pass")
    assert!(req.contains("ice-name: Streamy"));
    assert!(req.contains("ice-genre: Misc"));
    assert!(req.contains("ice-url: http://streamy.example"));
    assert!(req.contains("ice-public: 1"));
    assert!(req.contains("ice-bitrate: 128"));
    assert_eq!(got, audio, "payload must be byte-identical");
}

#[tokio::test]
async fn handshake_reports_rejection() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _srv = tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 256];
        let _ = sock.read(&mut buf).await;
        sock.write_all(b"HTTP/1.0 401 Unauthorized\r\n\r\n").await.unwrap();
    });

    let err = connect(&cfg(addr.ip().to_string(), addr.port()))
        .await
        .err()
        .expect("expected rejection");
    assert!(err.contains("authentication failed"), "unexpected: {err}");
}
