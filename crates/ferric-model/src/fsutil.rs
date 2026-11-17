//! Crash-safe file writes for project files and their audio (audit H1).

use std::io::Write;
use std::path::{Path, PathBuf};

/// Temporary sibling used while `path` is being replaced.
fn temp_sibling(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    path.with_file_name(format!(".{name}.{}.tmp", uuid::Uuid::new_v4().simple()))
}

/// Path of the backup kept beside `path` (`<name>.bak`).
pub fn backup_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    path.with_file_name(format!("{name}.bak"))
}

/// Write `bytes` to `path` without ever leaving a truncated file behind.
///
/// The data goes to a temporary file in the same folder, is flushed to disk,
/// and then atomically replaces `path`. The previous version is kept as
/// `<name>.bak` when `keep_backup` is set. If any step fails, `path` is left
/// exactly as it was and the temporary file is removed.
pub fn write_atomic(path: &Path, bytes: &[u8], keep_backup: bool) -> std::io::Result<()> {
    write_atomic_with(path, keep_backup, |file| file.write_all(bytes))
}

/// Like [`write_atomic`], but the caller streams the content into the file.
pub fn write_atomic_with(
    path: &Path,
    keep_backup: bool,
    fill: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = temp_sibling(path);
    let result = (|| {
        let mut file = std::fs::File::create_new(&tmp)?;
        fill(&mut file)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        if keep_backup && path.is_file() {
            // The backup is a convenience; the atomic rename below is what
            // protects the project, so a failed backup copy does not stop the save.
            if let Err(error) = std::fs::copy(path, backup_path(path)) {
                log_backup_failure(path, &error);
            }
        }
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn log_backup_failure(path: &Path, error: &std::io::Error) {
    eprintln!("FerricDAW: could not update backup of {}: {error}", path.display());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ferric-fsutil-{name}-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn leftovers(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect()
    }

    #[test]
    fn replaces_file_and_keeps_previous_version_as_backup() {
        let dir = scratch("replace");
        let file = dir.join("song.ferric");
        write_atomic(&file, b"first", true).unwrap();
        assert!(!backup_path(&file).exists());
        write_atomic(&file, b"second", true).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"second");
        assert_eq!(std::fs::read(backup_path(&file)).unwrap(), b"first");
        assert!(leftovers(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn failed_write_leaves_original_intact() {
        let dir = scratch("fail");
        let file = dir.join("song.ferric");
        write_atomic(&file, b"good project", true).unwrap();
        let err = write_atomic_with(&file, true, |f| {
            f.write_all(b"half writ")?;
            Err(std::io::Error::other("disk full"))
        });
        assert!(err.is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"good project");
        assert!(leftovers(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn locked_target_fails_without_damage() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = scratch("locked");
        let file = dir.join("song.ferric");
        write_atomic(&file, b"good project", false).unwrap();
        // Another program holds the file open without delete sharing, so the
        // replace must fail and the original must survive untouched.
        let holder = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1) // FILE_SHARE_READ only
            .open(&file)
            .unwrap();
        assert!(write_atomic(&file, b"new", false).is_err());
        drop(holder);
        assert_eq!(std::fs::read(&file).unwrap(), b"good project");
        assert!(leftovers(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
