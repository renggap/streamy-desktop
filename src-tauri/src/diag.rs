use std::path::PathBuf;
use std::sync::Mutex;

/// Simple append-only app log for diagnosing issues on machines without a
/// console (e.g. the release Windows build). Written next to app data.
static LOG_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn init(path: PathBuf) {
    if let Ok(mut g) = LOG_PATH.lock() {
        *g = Some(path);
    }
}

pub fn log(msg: impl AsRef<str>) {
    let line = format!("{}\n", msg.as_ref());
    if let Ok(g) = LOG_PATH.lock() {
        if let Some(p) = g.as_ref() {
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(p)
                .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()));
        }
    }
    // Also to stderr for dev runs.
    eprint!("{}", line);
}

pub fn log_path() -> String {
    LOG_PATH
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .map(|p| p.display().to_string())
        .unwrap_or_default()
}

/// Atomically replace `path` with `contents` (write temp + rename), so a
/// crash mid-write never leaves settings.json/profiles.json/playlist.json
/// truncated. Same-filesystem rename is atomic on Linux/macOS/Windows.
pub fn write_atomic(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(contents.as_bytes())?;
    f.sync_all()?;
    std::fs::rename(&tmp, path)
}
