use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const KEYRING_SERVICE: &str = "streamy-desktop";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
    /// Present only when the OS keyring is unavailable (plaintext fallback).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plaintext_pw: Option<String>,
}

/// JSON-backed profile store. Prefers the OS keyring for passwords;
/// falls back to plaintext in the JSON file when the keyring is unavailable
/// (headless CI, missing keyring backend). `get_password` checks keyring
/// first, then plaintext, so a keyring that works at save time but fails at
/// read time is still covered.
pub struct ProfileStore {
    dir: PathBuf,
    use_keyring: bool,
}

impl ProfileStore {
    pub fn new(dir: PathBuf) -> Self {
        Self::new_no_keyring(dir)
    }

    /// Test instance that skips the keyring (exercises the plaintext path).
    pub fn new_no_keyring(dir: PathBuf) -> Self {
        Self {
            dir,
            use_keyring: false,
        }
    }

    pub fn new_with_keyring(dir: PathBuf) -> Self {
        Self {
            dir,
            use_keyring: true,
        }
    }

    fn path(&self) -> PathBuf {
        self.dir.join("profiles.json")
    }

    fn read_all(&self) -> Vec<ConnectionProfile> {
        fs::read_to_string(self.path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| {
                crate::diag::log("WARN: profiles.json corrupt or unreadable; profile list empty");
                Vec::new()
            })
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
    pub fn save(&self, mut profile: ConnectionProfile, password: Option<String>) -> Result<(), String> {
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
        let mut all = self.read_all();
        let before = all.len();
        all.retain(|p| p.name != name);
        if all.len() == before {
            return Err("no such profile".into());
        }
        if self.use_keyring {
            let _ = keyring::Entry::new(KEYRING_SERVICE, name)
                .and_then(|e| e.delete_credential());
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
