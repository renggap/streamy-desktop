// Offline fallback-order tests for connect_source: real listeners on
// 127.0.0.1 speak just enough of each protocol to accept or reject, and the
// assertions check which server actually received audio. This is the
// offline (CI-runnable) version of the fallback ordering that previously
// only lived in `#[ignore]`d live tests: Auto without v1 resolves
// Ultravox -> Icecast; with the v1 checkbox set it tries v1 first and still
// falls through on rejection (a checked box never takes the user off air).

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use streamy_desktop::encode::{Codec, EncoderConfig};
use streamy_desktop::engine::ring::StereoRing;
use streamy_desktop::stream::broadcast;
use streamy_desktop::stream::icy::IcySourceConfig;
use streamy_desktop::stream::sc_v1::ScV1Config;
use streamy_desktop::stream::ultravox::UltravoxConfig;

/// Bind, pick the port, drop the listener: connections are refused instantly
/// (stands in for an unreachable server without DNS/timeouts).
async fn dead_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let p = l.local_addr().unwrap().port();
    drop(l);
    p
}

/// Fake SHOUTcast v1 server that ACCEPTS: reads the password line, answers
/// "OK2\r\n\r\n", swallows the icy-* headers, then counts audio bytes.
async fn fake_v1_acceptor(listener: TcpListener, total: Arc<AtomicUsize>) {
    while let Ok((mut sock, _)) = listener.accept().await {
        let total = total.clone();
        tokio::spawn(async move {
            let mut buf = [0u8; 1];
            // password line
            let mut line = Vec::new();
            loop {
                if sock.read(&mut buf).await.unwrap_or(0) == 0 {
                    return;
                }
                line.push(buf[0]);
                if line.ends_with(b"\r\n") {
                    break;
                }
            }
            sock.write_all(b"OK2\r\n\r\n").await.unwrap();
            // icy headers end with a blank line
            let mut head = Vec::new();
            loop {
                if sock.read(&mut buf).await.unwrap_or(0) == 0 {
                    return;
                }
                head.push(buf[0]);
                if head.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            // audio bytes
            let mut audio = [0u8; 4096];
            loop {
                match sock.read(&mut audio).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        total.fetch_add(n, Ordering::SeqCst);
                    }
                }
            }
        });
    }
}

/// Fake v1 server that REJECTS (what an Icecast/Liquidsoap endpoint does with
/// the ICY password line): answer with an HTTP error, then drop.
async fn fake_v1_rejector(listener: TcpListener) {
    while let Ok((mut sock, _)) = listener.accept().await {
        tokio::spawn(async move {
            let mut buf = [0u8; 1];
            let mut line = Vec::new();
            loop {
                if sock.read(&mut buf).await.unwrap_or(0) == 0 {
                    break;
                }
                line.push(buf[0]);
                if line.ends_with(b"\r\n") {
                    break;
                }
            }
            let _ = sock.write_all(b"HTTP/1.0 400 Bad Request\r\n\r\n").await;
            let _ = sock.shutdown().await;
        });
    }
}

/// Fake Icecast SOURCE server: swallows the request head, answers 200, then
/// counts audio bytes.
async fn fake_icecast(listener: TcpListener, total: Arc<AtomicUsize>) {
    while let Ok((mut sock, _)) = listener.accept().await {
        let total = total.clone();
        tokio::spawn(async move {
            let mut buf = [0u8; 1];
            let mut head = Vec::new();
            loop {
                if sock.read(&mut buf).await.unwrap_or(0) == 0 {
                    return;
                }
                head.push(buf[0]);
                if head.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            sock.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await.unwrap();
            let mut audio = [0u8; 4096];
            loop {
                match sock.read(&mut audio).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        total.fetch_add(n, Ordering::SeqCst);
                    }
                }
            }
        });
    }
}

fn v1_cfg(port: u16) -> ScV1Config {
    ScV1Config {
        host: "127.0.0.1".into(),
        port,
        password: "pw".into(),
        icy_name: "fallback test".into(),
        icy_genre: "t".into(),
        icy_url: String::new(),
        icy_pub: false,
        icy_br: 128,
    }
}

fn ice_cfg(port: u16) -> IcySourceConfig {
    IcySourceConfig {
        host: "127.0.0.1".into(),
        port,
        mount: "/live".into(),
        user: "source".into(),
        password: "pw".into(),
        content_type: "audio/mpeg".into(),
        icy_name: "fallback test".into(),
        icy_genre: "t".into(),
        icy_url: String::new(),
        icy_pub: false,
        icy_br: 128,
        sample_rate: 44100,
    }
}

fn uvox_cfg(port: u16) -> UltravoxConfig {
    UltravoxConfig {
        host: "127.0.0.1".into(),
        port,
        stream_id: 1,
        password: String::new(),
        user: String::new(),
        content_type: "audio/mpeg".into(),
        bitrate_kbps: 128,
        name: "t".into(),
        genre: String::new(),
        url: String::new(),
    }
}

fn enc_cfg() -> EncoderConfig {
    EncoderConfig {
        sample_rate: 44100,
        channels: 2,
        bitrate_kbps: 128,
        codec: Codec::Mp3,
    }
}

/// Push sine frames into the ring until `stop` (stands in for the audio
/// callback; the encoder thread drains it in 0.1s chunks).
fn spawn_producer(tx: streamy_desktop::engine::ring::StereoRingProducer, stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let mut i = 0u32;
        while !stop.load(Ordering::Relaxed) {
            for _ in 0..256 {
                let s = ((i as f32) * 2.0 * std::f32::consts::PI * 440.0 / 44100.0).sin() * 0.3;
                tx.push([s, s]);
                i = i.wrapping_add(1);
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    });
}

async fn wait_for_bytes(total: &Arc<AtomicUsize>) -> usize {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let n = total.load(Ordering::SeqCst);
        if n > 8000 || std::time::Instant::now() > deadline {
            return n;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Start + push audio + stop, returning bytes seen by whichever server won.
async fn run_broadcast(
    cfg: broadcast::StreamConfig,
) -> (broadcast::BroadcastHandle, Arc<AtomicBool>) {
    let (tx, rx) = StereoRing::new(16384);
    let rate = Arc::new(std::sync::atomic::AtomicU32::new(44100));
    let handle = broadcast::start_with_opts(
        rx,
        cfg,
        enc_cfg(),
        rate,
        None,
        Duration::from_millis(100),
        Duration::from_millis(400),
        3,
    )
    .await
    .expect("connect");
    let stop = Arc::new(AtomicBool::new(false));
    spawn_producer(tx, stop.clone());
    (handle, stop)
}

/// v1 checkbox set + a server that accepts the ICY handshake: the v1 path
/// wins, and no fallback server is ever contacted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn v1_optin_prefers_v1_when_accepted() {
    let dead = dead_port().await;
    let v1 = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let v1_port = v1.local_addr().unwrap().port();
    let v1_bytes = Arc::new(AtomicUsize::new(0));
    tokio::spawn(fake_v1_acceptor(v1, v1_bytes.clone()));

    let (h, stop) = run_broadcast(broadcast::StreamConfig::Auto {
        sc_v1: Some(v1_cfg(v1_port)),
        ultravox: uvox_cfg(dead),
        icecast: ice_cfg(dead),
    })
    .await;
    let n = wait_for_bytes(&v1_bytes).await;
    h.stop().await;
    stop.store(true, Ordering::Relaxed);
    assert!(n > 8000, "v1 server received only {n} audio bytes");
}

/// v1 checkbox set but the server rejects the ICY handshake (an Icecast in
/// disguise): the normal chain still connects — a checked box never takes
/// the user off air.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn v1_optin_falls_back_to_icecast_when_rejected() {
    let dead = dead_port().await;
    let rej = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let rej_port = rej.local_addr().unwrap().port();
    tokio::spawn(fake_v1_rejector(rej));
    let ice = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ice_port = ice.local_addr().unwrap().port();
    let ice_bytes = Arc::new(AtomicUsize::new(0));
    tokio::spawn(fake_icecast(ice, ice_bytes.clone()));

    let (h, stop) = run_broadcast(broadcast::StreamConfig::Auto {
        sc_v1: Some(v1_cfg(rej_port)),
        ultravox: uvox_cfg(dead),
        icecast: ice_cfg(ice_port),
    })
    .await;
    let n = wait_for_bytes(&ice_bytes).await;
    h.stop().await;
    stop.store(true, Ordering::Relaxed);
    assert!(n > 8000, "icecast fallback received only {n} audio bytes");
}

/// Default (checkbox unset): Auto skips v1 entirely and resolves
/// Ultravox -> Icecast. This is the offline ordering test the audit flagged
/// as missing (previously only live `#[ignore]`d tests covered it).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_without_v1_skips_v1_and_falls_back_to_icecast() {
    let dead = dead_port().await;
    let ice = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ice_port = ice.local_addr().unwrap().port();
    let ice_bytes = Arc::new(AtomicUsize::new(0));
    tokio::spawn(fake_icecast(ice, ice_bytes.clone()));

    let (h, stop) = run_broadcast(broadcast::StreamConfig::Auto {
        sc_v1: None,
        ultravox: uvox_cfg(dead),
        icecast: ice_cfg(ice_port),
    })
    .await;
    let n = wait_for_bytes(&ice_bytes).await;
    h.stop().await;
    stop.store(true, Ordering::Relaxed);
    assert!(n > 8000, "icecast fallback received only {n} audio bytes");
}
