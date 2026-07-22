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
