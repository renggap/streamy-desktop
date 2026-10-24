use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const KEYRING_SERVICE: &str = "streamy-desktop";

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct ConnectionProfile {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub mount: String,
    pub stream_id: u32,
    pub station_name: String,
    pub genre: String,
    pub url: String,
    pub public: bool,
    /// AzuraCast (and other Icecast-DJ) servers accept the DJ/source connection
    /// over the Icecast source protocol, not Ultravox — even when they relay to
    /// a Shoutcast DNAS on the listener side. When true, connect skips the
    /// Ultravox probe and goes straight to Icecast, which is what fixes DJ
    /// login on AzuraCast. Default false: Shoutcast DNAS using Ultravox.
    #[serde(default)]
    pub azuracast: bool,
    /// Stream with the SHOUTcast v1 (ICY) source protocol: tried first, with
    /// the normal Ultravox -> Icecast auto-detect chain kept as fallback.
    /// For DNAS1 servers (or DNAS2 stream id 1); Ultravox is still needed for
    /// DJ accounts / sid > 1, so leave unchecked in that case.
    #[serde(default)]
    pub shoutcast_v1: bool,
    /// Present only when the OS keyring is unavailable (plaintext fallback).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plaintext_pw: Option<String>,
}

impl std::fmt::Debug for ConnectionProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never let a {:?} of a profile leak the plaintext fallback password
        // into logs or error strings.
        f.debug_struct("ConnectionProfile")
            .field("name", &self.name)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("mount", &self.mount)
            .field("stream_id", &self.stream_id)
            .field("station_name", &self.station_name)
            .field("genre", &self.genre)
            .field("url", &self.url)
            .field("public", &self.public)
            .field("azuracast", &self.azuracast)
            .field(
                "plaintext_pw",
                &self.plaintext_pw.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

/// Strip control characters (CR/LF/NUL/...) from a user-supplied field: these
/// are interpolated verbatim into source-protocol request lines and headers,
/// so a stray CRLF would inject an extra header or request line.
fn sanitize_field(s: String) -> String {
    s.chars().filter(|c| !c.is_control()).collect()
}

fn sanitized(mut p: ConnectionProfile) -> ConnectionProfile {
    p.name = sanitize_field(p.name);
    p.host = sanitize_field(p.host);
    p.user = sanitize_field(p.user);
    p.mount = sanitize_field(p.mount);
    p.station_name = sanitize_field(p.station_name);
    p.genre = sanitize_field(p.genre);
    p.url = sanitize_field(p.url);
    p
}

/// JSON-backed profile store. Prefers the OS keyring for passwords;
/// falls back to plaintext in the JSON file when the keyring is unavailable
/// (headless CI, missing keyring backend). `get_password` checks keyring
/// first, then plaintext, so a keyring that works at save time but fails at
/// read time is still covered.
pub struct ProfileStore {
    dir: PathBuf,
    use_keyring: bool,
    /// Serializes the read-modify-write in save/delete: ProfileStore is
    /// shared across IPC tasks and two concurrent saves would lose one.
    lock: parking_lot::Mutex<()>,
}

impl ProfileStore {
    /// Test instance that skips the keyring (exercises the plaintext path).
    pub fn new_no_keyring(dir: PathBuf) -> Self {
        Self {
            dir,
            use_keyring: false,
            lock: parking_lot::Mutex::new(()),
        }
    }

    pub fn new_with_keyring(dir: PathBuf) -> Self {
        Self {
            dir,
            use_keyring: true,
            lock: parking_lot::Mutex::new(()),
        }
    }

    fn path(&self) -> PathBuf {
        self.dir.join("profiles.json")
    }

    fn read_all(&self) -> Vec<ConnectionProfile> {
        match fs::read_to_string(self.path()) {
            Err(_) => Vec::new(), // first run
            Ok(s) => serde_json::from_str(&s)
                .map(|v: Vec<ConnectionProfile>| v.into_iter().map(sanitized).collect())
                .unwrap_or_else(|e| {
                    crate::diag::log(format!(
                        "WARN: profiles.json corrupt ({e}); quarantined as profiles.json.bad"
                    ));
                    // Quarantine instead of silently wiping: the next save would
                    // otherwise overwrite the user's only copy of their profiles.
                    let _ = std::fs::rename(self.path(), self.dir.join("profiles.json.bad"));
                    Vec::new()
                }),
        }
    }

    fn write_all(&self, profiles: &[ConnectionProfile]) -> Result<(), String> {
        let s = serde_json::to_string_pretty(profiles).map_err(|e| e.to_string())?;
        crate::diag::write_atomic(&self.path(), &s).map_err(|e| e.to_string())
    }

    /// Profiles for UI display — never includes plaintext passwords.
    pub fn list(&self) -> Vec<ConnectionProfile> {
        self.read_all()
            .into_iter()
            .map(|mut p| {
                p.plaintext_pw = None;
                p
            })
            .collect()
    }

    /// Save (or replace) a profile. `password`: `Some(pw)` sets it; `None` keeps
    /// the existing stored password (used when editing non-credential fields).
    pub fn save(
        &self,
        mut profile: ConnectionProfile,
        password: Option<String>,
    ) -> Result<(), String> {
        // Serialize the whole read-modify-write against concurrent saves.
        let _rmw = self.lock.lock();
        profile = sanitized(profile);
        match password {
            Some(pw) => {
                let keyring_ok = if self.use_keyring {
                    keyring::Entry::new(KEYRING_SERVICE, &profile.name)
                        .ok()
                        .and_then(|e| {
                            e.set_password(&pw).ok()?;
                            // Verify with a fresh Entry: historical Windows bug
                            // where set succeeds but a later get returns nothing.
                            let verify = keyring::Entry::new(KEYRING_SERVICE, &profile.name)
                                .ok()?
                                .get_password()
                                .ok()?;
                            (verify == pw).then_some(())
                        })
                        .is_some()
                } else {
                    false
                };
                if keyring_ok {
                    profile.plaintext_pw = None;
                } else {
                    if self.use_keyring {
                        crate::diag::log(format!(
                            "profile '{}': OS keyring unavailable, storing password in plaintext",
                            profile.name
                        ));
                        // Drop any stale keyring entry: get_password prefers
                        // the keyring, which would shadow this fresh password.
                        if let Ok(e) = keyring::Entry::new(KEYRING_SERVICE, &profile.name) {
                            let _ = e.delete_credential();
                        }
                    }
                    profile.plaintext_pw = Some(pw);
                }
            }
            None => {
                // Keep whatever password is already stored.
                let existing = self
                    .read_all()
                    .iter()
                    .find(|p| p.name == profile.name)
                    .and_then(|p| p.plaintext_pw.clone());
                profile.plaintext_pw = existing;
            }
        }

        let mut all = self.read_all();
        if let Some(existing) = all.iter_mut().find(|p| p.name == profile.name) {
            *existing = profile;
        } else {
            all.push(profile);
        }
        self.write_all(&all)
    }

    pub fn delete(&self, name: &str) -> Result<(), String> {
        let _rmw = self.lock.lock();
        let mut all = self.read_all();
        let before = all.len();
        all.retain(|p| p.name != name);
        if all.len() == before {
            return Err("no such profile".into());
        }
        if self.use_keyring {
            let _ = keyring::Entry::new(KEYRING_SERVICE, name).and_then(|e| e.delete_credential());
        }
        self.write_all(&all)
    }

    /// Resolve the password for connect: keyring if available, else plaintext.
    pub fn get_password(&self, name: &str) -> Result<String, String> {
        if self.use_keyring {
            if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, name) {
                if let Ok(pw) = entry.get_password() {
                    return Ok(pw);
                }
            }
        }
        self.read_all()
            .iter()
            .find(|p| p.name == name)
            .and_then(|p| p.plaintext_pw.clone())
            .ok_or_else(|| "no password for profile".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> std::path::PathBuf {
        // Unique per invocation (see settings.rs for rationale).
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let d = std::env::temp_dir().join(format!(
            "streamy-profiles-test-{}-{:?}-{}",
            std::process::id(),
            std::thread::current().id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn first_run_missing_profiles_lists_empty() {
        let d = tempdir();
        let store = ProfileStore::new_no_keyring(d.clone());
        assert!(
            store.list().is_empty(),
            "no profiles.json yet => empty list"
        );
    }

    #[test]
    fn corrupt_profiles_json_lists_empty_not_panic() {
        let d = tempdir();
        std::fs::write(d.join("profiles.json"), b"~~~ not json ~~~").unwrap();
        let store = ProfileStore::new_no_keyring(d.clone());
        assert!(store.list().is_empty());
    }

    #[test]
    fn save_then_list_roundtrips_without_password() {
        let d = tempdir();
        let store = ProfileStore::new_no_keyring(d.clone());
        let p = ConnectionProfile {
            name: "test".into(),
            host: "example.com".into(),
            port: 8000,
            user: "source".into(),
            mount: "/test.mp3".into(),
            stream_id: 1,
            station_name: "T".into(),
            genre: "g".into(),
            url: "u".into(),
            public: false,
            azuracast: false,
            shoutcast_v1: false,
            plaintext_pw: None,
        };
        store.save(p, Some("s3cret".into())).expect("save");
        let listed = store.list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "test");
        assert!(
            listed[0].plaintext_pw.is_none(),
            "list() must never include plaintext passwords"
        );
    }
    #[test]
    fn save_strips_crlf_and_control_chars() {
        // CRLF injection: these fields end up verbatim in SOURCE/ICY request
        // lines and headers; a stray \r\n must never survive a save.
        let d = tempdir();
        let store = ProfileStore::new_no_keyring(d.clone());
        store
            .save(
                ConnectionProfile {
                    name: "pwn\r\nDELETE /x".into(),
                    host: "example.com\n".into(),
                    port: 8000,
                    user: "dj\r\nice-name: evil".into(),
                    mount: "/live\r\nGET /admin".into(),
                    stream_id: 1,
                    station_name: "s\u{0}".into(),
                    genre: "g".into(),
                    url: "https://x.example".into(),
                    public: false,
                    azuracast: false,
                    shoutcast_v1: false,
                    plaintext_pw: None,
                },
                Some("pw".into()),
            )
            .unwrap();
        let p = &store.list()[0];
        assert!(!p.name.contains('\r') && !p.name.contains('\n'));
        assert_eq!(p.host, "example.com");
        assert!(!p.user.contains('\r') && !p.user.contains('\n'));
        assert!(!p.mount.contains('\r') && !p.mount.contains('\n'));
        assert!(!p.station_name.contains('\u{0}'));
    }

    #[test]
    fn corrupt_profiles_json_is_quarantined_as_bad() {
        // A corrupt file is renamed aside (not silently wiped by the next
        // save) so the user can recover it.
        let d = tempdir();
        let path = d.join("profiles.json");
        std::fs::write(&path, b"~~~ not json ~~~").unwrap();
        let store = ProfileStore::new_no_keyring(d.clone());
        assert!(store.list().is_empty());
        assert!(
            d.join("profiles.json.bad").exists(),
            "corrupt file must be renamed to profiles.json.bad"
        );
        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn plaintext_fallback_file_is_owner_only() {
        // The plaintext password fallback must not be world-readable.
        let d = tempdir();
        let store = ProfileStore::new_no_keyring(d.clone());
        let mut p = ConnectionProfile {
            name: "secretive".into(),
            host: "example.com".into(),
            port: 8000,
            user: "dj".into(),
            mount: "/live".into(),
            stream_id: 1,
            station_name: "s".into(),
            genre: "g".into(),
            url: String::new(),
            public: false,
            azuracast: false,
            shoutcast_v1: false,
            plaintext_pw: None,
        };
        p.plaintext_pw = Some("hunter2".into());
        store.save(p, None).unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(d.join("profiles.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "profiles.json mode is {:o}",
            mode & 0o777
        );
    }

    #[test]
    fn debug_output_never_leaks_plaintext_password() {
        let mut p = ConnectionProfile {
            name: "n".into(),
            host: "h".into(),
            port: 8000,
            user: "u".into(),
            mount: "/m".into(),
            stream_id: 1,
            station_name: "s".into(),
            genre: "g".into(),
            url: String::new(),
            public: false,
            azuracast: false,
            shoutcast_v1: false,
            plaintext_pw: None,
        };
        p.plaintext_pw = Some("hunter2".into());
        let dbg = format!("{p:?}");
        assert!(!dbg.contains("hunter2"), "Debug leaked the password: {dbg}");
    }
}
