//! Atomic JSON file updates with inter-process locking.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use fs2::FileExt;

use crate::error::{Result, TermorgError};

/// Run `mutate` under an exclusive lock on `path.lock`, then write JSON atomically.
///
/// Temp file is unique per writer (`path.tmp.<pid>.<nanos>`) to avoid rename races.
pub fn update_json_file<F>(path: &Path, mutate: F) -> Result<()>
where
    F: FnOnce(&mut String) -> Result<()>,
{
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(TermorgError::Io)?;
    }
    let lock_path = lock_path_for(path);
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(TermorgError::Io)?;
    lock.lock_exclusive().map_err(TermorgError::Io)?;

    let mut raw = if path.exists() {
        fs::read_to_string(path).map_err(TermorgError::Io)?
    } else {
        String::new()
    };
    mutate(&mut raw)?;
    atomic_write(path, raw.as_bytes())?;
    // lock released on drop
    drop(lock);
    Ok(())
}

/// Read file under exclusive lock (simple, MSRV-friendly; short critical section).
pub fn read_json_file(path: &Path) -> Result<String> {
    if !path.exists() {
        return Ok(String::new());
    }
    let lock_path = lock_path_for(path);
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(TermorgError::Io)?;
    lock.lock_exclusive().map_err(TermorgError::Io)?;
    let mut f = File::open(path).map_err(TermorgError::Io)?;
    let mut raw = String::new();
    f.read_to_string(&mut raw).map_err(TermorgError::Io)?;
    drop(lock);
    Ok(raw)
}

fn lock_path_for(path: &Path) -> PathBuf {
    path.with_extension(format!(
        "{}lock",
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| format!("{e}."))
            .unwrap_or_default()
    ))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = path.with_extension(format!("tmp.{}.{}", std::process::id(), nanos));
    {
        let mut f = File::create(&tmp).map_err(TermorgError::Io)?;
        f.write_all(bytes).map_err(TermorgError::Io)?;
        f.sync_all().map_err(TermorgError::Io)?;
    }
    fs::rename(&tmp, path).map_err(TermorgError::Io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::thread;

    #[test]
    fn concurrent_updates_preserve_all_appends() {
        let dir = std::env::temp_dir().join(format!("termorg-persist-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("data.json");
        fs::write(&path, "[]").unwrap();

        let n = 20;
        let barrier = Arc::new(Barrier::new(n));
        let path = Arc::new(path);
        let mut handles = vec![];
        for i in 0..n {
            let barrier = Arc::clone(&barrier);
            let path = Arc::clone(&path);
            handles.push(thread::spawn(move || {
                barrier.wait();
                update_json_file(&path, |raw| {
                    let mut v: Vec<u32> = if raw.trim().is_empty() {
                        Vec::new()
                    } else {
                        serde_json::from_str(raw).unwrap_or_default()
                    };
                    v.push(i as u32);
                    v.sort_unstable();
                    *raw = serde_json::to_string(&v).unwrap();
                    Ok(())
                })
                .unwrap();
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        let raw = fs::read_to_string(&*path).unwrap();
        let v: Vec<u32> = serde_json::from_str(&raw).unwrap();
        assert_eq!(v.len(), n, "lost updates: {v:?}");
        let _ = fs::remove_dir_all(&dir);
    }
}
