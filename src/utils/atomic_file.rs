use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::utils::errors::Result;

static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Writes a complete sibling file and only then replaces the destination.
///
/// The temporary file is flushed to stable storage before the rename. On
/// Windows, `MoveFileExW` performs the replacement with write-through instead
/// of deleting the previous file first.
pub fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    let temp_path = temporary_sibling(path, "write");
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        replace_file(&temp_path, path)?;
        sync_parent(path);
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

/// Replaces the contents of a file that the caller already created.
///
/// Elevated helpers use this for their response file. In particular, this
/// must not set `O_CREAT`: Linux can reject an elevated process opening a
/// user-owned file in a sticky directory such as `/tmp` when
/// `fs.protected_regular` is enabled, even though the file already exists.
pub fn write_precreated(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().write(true).truncate(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn temporary_sibling(path: &Path, purpose: &str) -> PathBuf {
    let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("bexplorer");
    path.with_file_name(format!(
        ".{name}.bexplorer-{purpose}-{}-{sequence}",
        std::process::id()
    ))
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn replace_file(source: &Path, destination: &Path) -> Result<()> {
    fs::rename(source, destination)?;
    Ok(())
}

#[cfg(target_os = "windows")]
pub(crate) fn replace_file(source: &Path, destination: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;

    use windows::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    use windows::core::PCWSTR;

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(destination.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )?;
    }
    Ok(())
}

#[cfg(unix)]
pub(crate) fn sync_parent(path: &Path) {
    if let Some(parent) = path.parent()
        && let Ok(directory) = fs::File::open(parent)
    {
        let _ = directory.sync_all();
    }
}

#[cfg(not(unix))]
pub(crate) fn sync_parent(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn repeated_atomic_write_replaces_contents_without_leaving_temporary_files() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "bexplorer-atomic-write-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create atomic write test directory");
        let destination = root.join("config.json");

        write(&destination, b"first").expect("write initial contents");
        write(&destination, b"second").expect("replace contents");

        assert_eq!(fs::read(&destination).expect("read destination"), b"second");
        assert_eq!(
            fs::read_dir(&root)
                .expect("read test directory")
                .filter_map(|entry| entry.ok())
                .count(),
            1
        );
        fs::remove_dir_all(root).expect("cleanup atomic write test directory");
    }

    #[test]
    fn precreated_write_never_creates_a_missing_response_file() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "bexplorer-precreated-write-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create precreated write test directory");
        let response = root.join("result.json");

        assert!(write_precreated(&response, b"missing").is_err());
        fs::File::create(&response).expect("precreate result file");
        write_precreated(&response, br#"{"Ok":1}"#).expect("write precreated result");
        assert_eq!(
            fs::read(&response).expect("read precreated result"),
            br#"{"Ok":1}"#
        );

        fs::remove_dir_all(root).expect("cleanup precreated write test directory");
    }
}
