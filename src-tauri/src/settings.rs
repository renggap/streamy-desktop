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
        let inner = fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| {
                crate::diag::log("WARN: settings.json corrupt or unreadable; using defaults");
                Settings::default()
            });
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

    fn persist(&self, s: &Settings) -> Result<(), String> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        fs::write(
            &self.path,
            serde_json::to_string_pretty(s).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    }
}
