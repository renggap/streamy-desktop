use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

/// Small persisted app settings (last-used input/output device names, recording
/// dir, master volume). `Default` is impl'd manually so a fresh install gets
/// unity volume — deriving Default would give 0.0 (silent).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub input_device: Option<String>,
    #[serde(default)]
    pub output_device: Option<String>,
    /// Where recordings land; defaults to <app-data>/recordings when None.
    #[serde(default)]
    pub recording_dir: Option<String>,
    /// Master output gain (1.0 = unity / 100%). 0.0..=2.0.
    #[serde(default = "default_volume")]
    pub volume: f32,
    /// Monitor-only gain for local file playback (1.0 = unity). Broadcast
    /// unaffected. Persisted so the DJ's listening level survives restarts.
    #[serde(default = "default_volume")]
    pub monitor_file_volume: f32,
    /// Mic noise suppression (RNNoise) for the broadcast voice path.
    #[serde(default)]
    pub mic_denoise: bool,
}

fn default_volume() -> f32 {
    1.0
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            input_device: None,
            output_device: None,
            recording_dir: None,
            volume: default_volume(),
            monitor_file_volume: default_volume(),
            mic_denoise: false,
        }
    }
}

pub struct SettingsStore {
    path: PathBuf,
    inner: Mutex<Settings>,
}

impl SettingsStore {
    pub fn new(dir: PathBuf) -> Self {
        let path = dir.join("settings.json");
        let inner = match fs::read_to_string(&path) {
            Err(_) => Settings::default(), // first run
            Ok(s) => serde_json::from_str(&s).unwrap_or_else(|e| {
                crate::diag::log(format!(
                    "WARN: settings.json corrupt ({}); using defaults",
                    e
                ));
                Settings::default()
            }),
        };
        Self {
            path,
            inner: Mutex::new(inner),
        }
    }

    pub fn get(&self) -> Settings {
        match self.inner.lock() {
            Ok(g) => g.clone(),
            Err(_) => {
                crate::diag::log("WARN: settings mutex poisoned; returning defaults");
                Settings::default()
            }
        }
    }

    pub fn set_input_device(&self, name: String) -> Result<(), String> {
        let mut g = self.inner.lock().map_err(|e| e.to_string())?;
        g.input_device = Some(name);
        self.persist(&g)
    }

    pub fn set_output_device(&self, name: Option<String>) -> Result<(), String> {
        let mut g = self.inner.lock().map_err(|e| e.to_string())?;
        g.output_device = name;
        self.persist(&g)
    }

    pub fn set_recording_dir(&self, dir: Option<String>) -> Result<(), String> {
        let mut g = self.inner.lock().map_err(|e| e.to_string())?;
        g.recording_dir = dir;
        self.persist(&g)
    }

    /// Master gain, 0.0..=2.0 (1.0 = unity).
    pub fn set_volume(&self, gain: f32) -> Result<(), String> {
        let mut g = self.inner.lock().map_err(|e| e.to_string())?;
        g.volume = gain;
        self.persist(&g)
    }

    /// Monitor-only gain for local file playback, 0.0..=2.0 (1.0 = unity).
    pub fn set_monitor_file_volume(&self, gain: f32) -> Result<(), String> {
        let mut g = self.inner.lock().map_err(|e| e.to_string())?;
        g.monitor_file_volume = gain;
        self.persist(&g)
    }

    /// Mic noise suppression on/off (persisted).
    pub fn set_mic_denoise(&self, on: bool) -> Result<(), String> {
        let mut g = self.inner.lock().map_err(|e| e.to_string())?;
        g.mic_denoise = on;
        self.persist(&g)
    }

    fn persist(&self, s: &Settings) -> Result<(), String> {
        crate::diag::write_atomic(
            &self.path,
            &serde_json::to_string_pretty(s).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> std::path::PathBuf {
        // Unique per invocation: pid + thread + global counter. Fixed
        // names collided across test reruns and teardown raced parallel
        // tests (seen as flaky "File not found" under Wine stress).
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let d = std::env::temp_dir().join(format!(
            "streamy-settings-test-{}-{:?}-{}",
            std::process::id(),
            std::thread::current().id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn first_run_missing_settings_is_silent_and_defaults() {
        let d = tempdir();
        let s = SettingsStore::new(d.clone()).get();
        assert_eq!(s.volume, 1.0, "first run must be unity volume, not silent");
        assert!(s.input_device.is_none());
    }

    #[test]
    fn corrupt_settings_json_still_loads_defaults() {
        let d = tempdir();
        std::fs::write(d.join("settings.json"), b"{ not json").unwrap();
        let s = SettingsStore::new(d.clone()).get();
        assert_eq!(s.volume, 1.0, "corrupt file must fall back to defaults");
    }

    #[test]
    fn old_settings_json_without_monitor_file_volume_loads_at_unity() {
        // Simulates a pre-feature settings.json (no monitor_file_volume key).
        let json = r#"{"volume":0.8}"#;
        let s: Settings = serde_json::from_str(json).expect("parse");
        assert_eq!(
            s.monitor_file_volume, 1.0,
            "missing key must default to unity"
        );
        assert_eq!(s.volume, 0.8);
    }

    #[test]
    fn old_settings_json_without_mic_denoise_defaults_off() {
        let json = r#"{"volume":1.0}"#;
        let s: Settings = serde_json::from_str(json).expect("parse");
        assert!(
            !s.mic_denoise,
            "denoise must default off for existing users"
        );
    }
}
