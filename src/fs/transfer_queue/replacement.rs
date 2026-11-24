use super::*;

use std::io::ErrorKind;

const RECORD_LIMIT: u64 = 256 * 1024;

#[cfg(unix)]
type NativeUnit = u8;
#[cfg(windows)]
type NativeUnit = u16;
type NativePath = Vec<NativeUnit>;

#[derive(Deserialize, Serialize)]
struct Replacement {
    directory: NativePath,
    directory_id: FileIdentity,
    target: NativePath,
    staging: NativePath,
    backup: NativePath,
    original_id: FileIdentity,
    replacement_id: FileIdentity,
}

impl Replacement {
    fn new(staging: &Path, target: &Path, backup: &Path) -> Result<Self> {
        let parent = target.parent().unwrap_or_else(|| Path::new("."));
        let directory = fs::canonicalize(parent)?;
        Ok(Self {
            directory_id: path_identity(&directory)?,
            directory: encode_path(&directory),
            target: encode_name(target)?,
            staging: encode_name(staging)?,
            backup: encode_name(backup)?,
            original_id: path_identity(target)?,
            replacement_id: path_identity(staging)?,
        })
    }

    fn paths(&self) -> Result<(PathBuf, PathBuf, PathBuf)> {
        let directory = decode_path(&self.directory);
        if !directory.is_absolute() || path_identity(&directory)? != self.directory_id {
            return Err(BExplorerError::InvalidPath(directory));
        }
        let target = decode_name(&self.target)?;
        let staging = decode_name(&self.staging)?;
        let backup = decode_name(&self.backup)?;
        if target == staging || target == backup || staging == backup {
            return Err(BExplorerError::InvalidPath(directory));
        }
        Ok((
            directory.join(target),
            directory.join(staging),
            directory.join(backup),
        ))
    }
}

pub(super) fn commit(staging: &Path, target: &Path) -> Result<()> {
    let staging_metadata = fs::symlink_metadata(staging)?;
    let target_metadata = fs::symlink_metadata(target)?;
    if staging_metadata.is_file() && target_metadata.is_file() {
        crate::utils::atomic_file::replace_file(staging, target)?;
        return sync_parent(target);
    }

    let backup = unused_transfer_sibling(target, "backup");
    let record = Replacement::new(staging, target, &backup)?;
    let (journal_path, journal) = write_record(&record, &journal_directory()?)?;
    let (target, staging, backup) = record.paths()?;
    let result = install(&record, &target, &staging, &backup);

    let resolved = match &result {
        Ok(()) if identity_at(&backup).is_ok_and(|id| id == Some(record.original_id)) => {
            match remove_source(&backup) {
                Ok(()) => sync_parent(&target).is_ok(),
                Err(error) => {
                    crate::utils::log::error(format!(
                        "Replacement completed but its backup remains at {}: {error}",
                        backup.display()
                    ));
                    false
                }
            }
        }
        Ok(()) => false,
        Err(_) => {
            identity_at(&target).is_ok_and(|id| id == Some(record.original_id))
                && identity_at(&backup).is_ok_and(|id| id.is_none())
        }
    };
    drop(journal);
    if resolved {
        remove_record(&journal_path);
    }
    result
}

fn install(record: &Replacement, target: &Path, staging: &Path, backup: &Path) -> Result<()> {
    if path_identity(target)? != record.original_id
        || path_identity(staging)? != record.replacement_id
    {
        return Err(BExplorerError::Operation(format!(
            "The replacement source or existing destination changed at {}",
            target.display()
        )));
    }
    // The record is durable before the original directory entry is changed.
    rename_without_replace(target, backup)?;
    if let Err(error) = sync_parent(target).and_then(|()| {
        rename_without_replace(staging, target)?;
        Ok(())
    }) {
        if let Err(rollback) = rename_without_replace(backup, target) {
            return Err(BExplorerError::Operation(format!(
                "Could not install the replacement ({error}). The original remains at {} because restoring it also failed: {rollback}",
                backup.display()
            )));
        }
        sync_parent(target)?;
        return Err(error);
    }
    sync_parent(target)
}

pub(super) fn sync_parent(path: &Path) -> Result<()> {
    #[cfg(unix)]
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::File::open(parent)?.sync_all()?;
    }
    // Windows rename_without_replace uses MOVEFILE_WRITE_THROUGH.
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn journal_directory() -> Result<PathBuf> {
    #[cfg(not(test))]
    let directory = crate::utils::paths::config_dir()?.join("transfer-recovery");
    #[cfg(test)]
    let directory = std::env::temp_dir().join(format!(
        "bexplorer-replacement-tests-{}",
        std::process::id()
    ));
    Ok(directory)
}

fn write_record(record: &Replacement, directory: &Path) -> Result<(PathBuf, fs::File)> {
    match metadata::create_directory(directory) {
        Ok(()) => sync_parent(directory)?,
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            if !fs::symlink_metadata(directory)?.is_dir() {
                return Err(BExplorerError::InvalidPath(directory.to_path_buf()));
            }
        }
        Err(error) => return Err(error.into()),
    }
    let mut nonce = [0_u8; 16];
    getrandom::fill(&mut nonce).map_err(|error| BExplorerError::Operation(error.to_string()))?;
    let path = directory.join(format!(
        "replacement-{:032x}.json",
        u128::from_ne_bytes(nonce)
    ));
    let bytes = serde_json::to_vec(record)?;
    if bytes.len() as u64 > RECORD_LIMIT {
        return Err(BExplorerError::InvalidPath(decode_path(&record.directory)));
    }
    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)?;
    if let Err(error) = (|| -> Result<()> {
        file.lock()?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        sync_parent(&path)
    })() {
        drop(file);
        remove_record(&path);
        return Err(error);
    }
    Ok((path, file))
}

pub(super) fn recover(destination: Option<&Path>) {
    let Ok(directory) = journal_directory() else {
        return;
    };
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let destination = destination.map(fs::canonicalize).transpose();
    let Ok(destination) = destination else {
        return;
    };
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|kind| kind.is_file())
            || !entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("replacement-") && name.ends_with(".json"))
        {
            continue;
        }
        let path = entry.path();
        if let Err(error) = recover_record(&path, destination.as_deref()) {
            crate::utils::log::error(format!(
                "Replacement recovery remains pending for {}: {error}",
                path.display()
            ));
        }
    }
}

fn recover_record(path: &Path, destination: Option<&Path>) -> Result<()> {
    let mut file = fs::OpenOptions::new().read(true).write(true).open(path)?;
    match file.try_lock() {
        Ok(()) => {}
        Err(fs::TryLockError::WouldBlock) => return Ok(()),
        Err(fs::TryLockError::Error(error)) => return Err(error.into()),
    }
    let mut bytes = Vec::new();
    (&mut file).take(RECORD_LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.is_empty() {
        // A just-created record may still be waiting for the lock before
        // writing. No transfer can start from an empty record.
        return Ok(());
    }
    if bytes.len() as u64 > RECORD_LIMIT {
        return Err(BExplorerError::InvalidPath(path.into()));
    }
    let record: Replacement = serde_json::from_slice(&bytes)?;
    if destination.is_some_and(|destination| decode_path(&record.directory) != destination) {
        return Ok(());
    }
    if let Some(retained) = restore(&record)? {
        crate::utils::log::info(format!(
            "Interrupted replacement recovered. A second copy was kept at {}",
            retained.display()
        ));
    }
    drop(file);
    remove_record(path);
    Ok(())
}

fn restore(record: &Replacement) -> Result<Option<PathBuf>> {
    let (target, staging, backup) = record.paths()?;
    let current = identity_at(&target)?;
    let saved = identity_at(&backup)?;
    match (current, saved) {
        (None, Some(saved)) if saved == record.original_id => {
            rename_without_replace(&backup, &target)?;
            sync_parent(&target)?;
            Ok(identity_at(&staging)?.map(|_| staging))
        }
        (Some(current), None)
            if current == record.original_id || current == record.replacement_id =>
        {
            Ok(identity_at(&staging)?.map(|_| staging))
        }
        (Some(current), Some(saved))
            if current == record.replacement_id && saved == record.original_id =>
        {
            // The replacement was installed before interruption. Keep the
            // original rather than deleting files after a restart.
            Ok(Some(backup))
        }
        _ => Err(BExplorerError::Operation(format!(
            "Could not safely restore {}. Check the retained backup at {} and the replacement at {}",
            target.display(),
            backup.display(),
            staging.display()
        ))),
    }
}

fn identity_at(path: &Path) -> Result<Option<FileIdentity>> {
    match path_identity(path) {
        Ok(identity) => Ok(Some(identity)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn remove_record(path: &Path) {
    let _ = fs::remove_file(path);
    let _ = sync_parent(path);
}

fn encode_name(path: &Path) -> Result<NativePath> {
    path.file_name()
        .map(Path::new)
        .map(encode_path)
        .ok_or_else(|| BExplorerError::InvalidPath(path.into()))
}

fn decode_name(bytes: &[NativeUnit]) -> Result<PathBuf> {
    use std::path::Component;

    let name = decode_path(bytes);
    let mut components = name.components();
    if !matches!(components.next(), Some(Component::Normal(value)) if value == name.as_os_str())
        || components.next().is_some()
    {
        return Err(BExplorerError::InvalidPath(name));
    }
    Ok(name)
}

#[cfg(unix)]
fn encode_path(path: &Path) -> NativePath {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(unix)]
fn decode_path(bytes: &[NativeUnit]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
}

#[cfg(windows)]
fn encode_path(path: &Path) -> NativePath {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().collect()
}

#[cfg(windows)]
fn decode_path(units: &[NativeUnit]) -> PathBuf {
    use std::os::windows::ffi::OsStringExt;
    PathBuf::from(std::ffi::OsString::from_wide(units))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::transfer_queue::local_tests::fixture;

    fn interrupted_replacement(root: &Path) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
        let target = root.join("project");
        let staging = root.join(".bexplorer-staging");
        let backup = root.join(".bexplorer-backup");
        fs::create_dir(&target).unwrap();
        fs::create_dir(&staging).unwrap();
        fs::write(target.join("notes"), b"original").unwrap();
        fs::write(staging.join("notes"), b"replacement").unwrap();
        let record = Replacement::new(&staging, &target, &backup).unwrap();
        let (journal, file) = write_record(&record, &root.join("journals")).unwrap();
        rename_without_replace(&target, &backup).unwrap();
        drop(file);
        (target, staging, backup, journal)
    }

    #[test]
    fn restores_interruption_between_directory_renames() {
        let root = fixture();
        let (target, staging, backup, journal) = interrupted_replacement(&root);
        recover_record(&journal, None).unwrap();
        assert_eq!(fs::read(target.join("notes")).unwrap(), b"original");
        assert_eq!(fs::read(staging.join("notes")).unwrap(), b"replacement");
        assert!(!backup.exists());
        assert!(!journal.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn keeps_both_after_interruption_following_install() {
        let root = fixture();
        let (target, staging, backup, journal) = interrupted_replacement(&root);
        rename_without_replace(&staging, &target).unwrap();
        recover_record(&journal, None).unwrap();
        assert_eq!(fs::read(target.join("notes")).unwrap(), b"replacement");
        assert_eq!(fs::read(backup.join("notes")).unwrap(), b"original");
        assert!(!journal.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn preserves_an_external_entry_that_appeared_after_interruption() {
        let root = fixture();
        let (target, staging, backup, journal) = interrupted_replacement(&root);
        fs::write(&target, b"other editor").unwrap();
        assert!(recover_record(&journal, None).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"other editor");
        assert_eq!(fs::read(backup.join("notes")).unwrap(), b"original");
        assert_eq!(fs::read(staging.join("notes")).unwrap(), b"replacement");
        assert!(journal.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn native_paths_roundtrip_through_a_journal() {
        #[cfg(unix)]
        let name = {
            use std::os::unix::ffi::OsStrExt;
            Path::new(std::ffi::OsStr::from_bytes(b"nonutf8-\xff"))
        };
        #[cfg(windows)]
        let name = Path::new("proyecto-\u{1f4c1}");

        assert_eq!(decode_name(&encode_name(name).unwrap()).unwrap(), name);
        assert!(decode_name(&encode_path(Path::new("../outside"))).is_err());
    }
}
