use streamy_desktop::profile::store::{ConnectionProfile, ProfileStore, Protocol};
use std::fs;

fn tmp_store(tag: &str) -> (ProfileStore, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "streamy-profile-test-{}-{}",
        std::process::id(),
        tag
    ));
    let _ = fs::remove_dir_all(&dir);
    (ProfileStore::new_no_keyring(dir.clone()), dir)
}

fn sample(name: &str) -> ConnectionProfile {
    ConnectionProfile {
        name: name.into(),
        host: "h".into(),
        port: 8000,
        user: "source".into(),
        mount: "/stream".into(),
        protocol: Protocol::Icy,
        stream_id: 1,
        station_name: "Streamy".into(),
        genre: "Misc".into(),
        url: "http://x".into(),
        public: true,
        plaintext_pw: None,
    }
}

#[test]
fn crud_roundtrip_plaintext_fallback() {
    let (s, dir) = tmp_store("crud");
    s.save(sample("live"), "secret".into()).unwrap();

    let list = s.list();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "live");
    assert_eq!(list[0].plaintext_pw, None, "list must not expose password");

    assert_eq!(s.get_password("live").unwrap(), "secret");

    s.delete("live").unwrap();
    assert!(s.list().is_empty());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn save_replaces_existing_by_name() {
    let (s, dir) = tmp_store("replace");
    let mut p = sample("p");
    s.save(p.clone(), "pw1".to_string()).unwrap();
    p.host = "b".into();
    s.save(p, "pw2".to_string()).unwrap();

    let list = s.list();
    assert_eq!(list.len(), 1, "replace must not duplicate");
    assert_eq!(list[0].host, "b");
    assert_eq!(s.get_password("p").unwrap(), "pw2");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn delete_missing_errors() {
    let (s, dir) = tmp_store("delete");
    assert!(s.delete("nope").is_err());
    fs::remove_dir_all(&dir).ok();
}
