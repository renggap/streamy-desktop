use std::fs;
use streamy_desktop::profile::store::{ConnectionProfile, ProfileStore};

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
        stream_id: 1,
        station_name: "Streamy".into(),
        genre: "Misc".into(),
        url: "http://x".into(),
        public: true,
        azuracast: false,
        plaintext_pw: None,
    }
}

#[test]
fn crud_roundtrip_plaintext_fallback() {
    let (s, dir) = tmp_store("crud");
    s.save(sample("live"), Some("secret".to_string())).unwrap();

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
    s.save(p.clone(), Some("pw1".to_string())).unwrap();
    p.host = "b".into();
    s.save(p, Some("pw2".to_string())).unwrap();

    let list = s.list();
    assert_eq!(list.len(), 1, "replace must not duplicate");
    assert_eq!(list[0].host, "b");
    assert_eq!(s.get_password("p").unwrap(), "pw2");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn save_none_keeps_existing_password() {
    let (s, dir) = tmp_store("keep");
    let mut p = sample("k");
    s.save(p.clone(), Some("orig".to_string())).unwrap();
    // Edit fields, pass None for password -> existing password retained.
    p.host = "newhost".into();
    s.save(p, None).unwrap();
    assert_eq!(s.get_password("k").unwrap(), "orig");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn delete_missing_errors() {
    let (s, dir) = tmp_store("delete");
    assert!(s.delete("nope").is_err());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn profile_roundtrips_without_protocol_field() {
    let (s, dir) = tmp_store("noprotocol");
    let p = sample("np");
    s.save(p, Some("pw".to_string())).unwrap();
    let list = s.list();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "np");
    std::fs::remove_dir_all(&dir).ok();
}

/// Old profiles.json written before the `azuracast` field existed must still
/// load, defaulting to false (Shoutcast/Ultravox). Regression guard for the
/// serde(default) on the new field.
#[test]
fn legacy_profile_without_azuracast_loads_false() {
    let (s, dir) = tmp_store("legacy");
    std::fs::create_dir_all(&dir).unwrap();
    // No `azuracast` key — exactly what an older Streamy wrote to disk.
    std::fs::write(
        dir.join("profiles.json"),
        r#"[{"name":"old","host":"h","port":8000,"user":"source","mount":"/stream","stream_id":1,"station_name":"Streamy","genre":"Misc","url":"http://x","public":true}]"#,
    )
    .unwrap();
    let list = s.list();
    assert_eq!(list.len(), 1);
    assert!(!list[0].azuracast, "missing field must default to false");
    std::fs::remove_dir_all(&dir).ok();
}

/// A profile saved with azuracast=true must round-trip the flag through disk.
#[test]
fn azuracast_flag_roundtrips_true() {
    let (s, dir) = tmp_store("az");
    let mut p = sample("az");
    p.azuracast = true;
    s.save(p, Some("pw".to_string())).unwrap();
    let list = s.list();
    assert_eq!(list.len(), 1);
    assert!(list[0].azuracast, "AzuraCast flag must persist");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn keyring_path_used_when_enabled() {
    let dir = std::env::temp_dir().join(format!(
        "streamy-profile-test-{}-keyring",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let store = ProfileStore::new_with_keyring(dir.clone());
    store
        .save(sample("kr"), Some("sekrit".to_string()))
        .unwrap();

    // Inspect profiles.json on disk via JSON parse — avoids false positives
    // from substring matching on other keys.
    let raw = std::fs::read_to_string(dir.join("profiles.json")).unwrap();
    let json: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let keyring_used = json[0].get("plaintext_pw").is_none();

    if keyring_used {
        // Keyring stored the password; JSON has no plaintext_pw key.
        // get_password should succeed via keyring. On systems where keyring
        // set-verify succeeds at save time (this test's save already handles
        // the round-trip), read-back should also work.
        assert_eq!(store.get_password("kr").unwrap(), "sekrit");
    } else {
        // Keyring unavailable (e.g. headless CI); plaintext fallback in JSON.
        assert_eq!(json[0]["plaintext_pw"].as_str().unwrap(), "sekrit");
        assert_eq!(store.get_password("kr").unwrap(), "sekrit");
    }

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn delete_cleans_keyring_best_effort() {
    let dir = std::env::temp_dir().join(format!(
        "streamy-profile-test-{}-keyring-del",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let store = ProfileStore::new_with_keyring(dir.clone());
    store
        .save(sample("kr"), Some("sekrit".to_string()))
        .unwrap();
    assert_eq!(store.list().len(), 1);

    // Delete must remove the profile from the JSON file even if keyring
    // delete fails (headless CI). The keyring cleanup is best-effort.
    store.delete("kr").unwrap();
    assert!(store.list().is_empty());
    // Confirm the JSON file is empty/cleared.
    let raw = std::fs::read_to_string(dir.join("profiles.json")).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&raw).unwrap(),
        serde_json::json!([])
    );

    std::fs::remove_dir_all(&dir).ok();
}
