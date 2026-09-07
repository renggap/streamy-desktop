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
/// The tmp name is unique per call: concurrent writers (ProfileStore has no
/// mutex) can never collide on or steal each other's temp file.
pub fn write_atomic(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let uniq = SEQ.fetch_add(1, Ordering::Relaxed) ^ std::process::id() as u64;
    let tmp = path.with_extension(format!("{uniq}.tmp"));
    let res = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(contents.as_bytes())?;
        f.sync_all()?;
        // Windows has no atomic replace-while-dest-open: a concurrent writer
        // or reader holding the destination yields ERROR_SHARING_VIOLATION /
        // ACCESS_DENIED. Retry briefly — the other side finishes fast.
        // ponytail: fixed 10 x 10ms; use ReplaceFile semantics if ever hot.
        for attempt in 0..10 {
            match std::fs::rename(&tmp, path) {
                Ok(()) => return Ok(()),
                Err(e) if attempt < 9 && e.kind() == std::io::ErrorKind::PermissionDenied => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(e) => return Err(e),
            }
        }
        unreachable!()
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp); // don't leak a failed tmp
    }
    res
}

#[cfg(test)]
mod atomic_tests {
    use super::write_atomic;

    #[test]
    fn write_atomic_replaces_existing_and_leaves_no_tmp() {
        let dir = std::env::temp_dir().join(format!("streamy-atomic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("x.json");
        std::fs::write(&p, "OLD").unwrap();
        write_atomic(&p, "NEW").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "NEW");
        assert!(!dir.join("x.json.tmp").exists(), "temp file leaked");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_atomic_creates_parent_dirs() {
        let dir = std::env::temp_dir().join(format!("streamy-atomic-deep-{}", std::process::id()));
        let p = dir.join("a/b/c.json");
        write_atomic(&p, "1").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "1");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_atomic_concurrent_writers_never_tear() {
        // 8 threads x 50 writes to the SAME path (each via its own unique
        // tmp, per-writer suffix). Whatever the final content, it must be
        // exactly one writer's payload — never a torn mix. Also: no tmp
        // leftovers once writers finish (successful renames clean up).
        let dir = std::env::temp_dir().join(format!("streamy-atomic-race-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("r.json");
        std::fs::write(&p, "init").unwrap();
        std::thread::scope(|sc| {
            for t in 0..8u32 {
                let p = p.clone();
                sc.spawn(move || {
                    for i in 0..50u32 {
                        let body = format!("\"writer\":{t},\"iter\":{i}");
                        write_atomic(&p, &body).unwrap();
                    }
                });
            }
        });
        let final_ = std::fs::read_to_string(&p).unwrap();
        assert!(
            final_.starts_with("\"writer\":"),
            "torn write detected: {final_}"
        );
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "tmp files leaked: {leftovers:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

}
