use std::fs;
use std::hash::Hash;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::transfer_queue::{self, TransferCompletedRoot, TransferKind};
use crate::utils::errors::{BExplorerError, Result};

type Digest = [u8; 16];

/// A metadata fingerprint captured by the transfer worker after it publishes
/// a local item. Includes every descendant without following symbolic links.
/// Only the digest crosses the elevation boundary, even for a large tree.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TransferUndoTarget {
    target_parent: Digest,
    original_parent: Option<Digest>,
    contents: Digest,
}

impl TransferUndoTarget {
    pub(crate) fn capture_with_check(
        path: &Path,
        mut check: impl FnMut() -> Result<()>,
    ) -> Result<Self> {
        let target_parent = parent_digest(path)?;
        let mut tree = MetadataDigest::new();
        for entry in walkdir::WalkDir::new(path)
            .follow_root_links(false)
            .sort_by_file_name()
        {
            check()?;
            let entry = entry.map_err(|error| BExplorerError::Operation(error.to_string()))?;
            let current_path = entry.path();
            let relative_path = current_path.strip_prefix(path).map_err(|error| {
                BExplorerError::Operation(format!("Could not track transfer target: {error}"))
            })?;
            tree.add(relative_path.as_os_str().as_encoded_bytes());
            tree.add(&metadata_digest(current_path)?);
        }
        if target_parent != parent_digest(path)? {
            return Err(changed(path));
        }
        Ok(Self {
            target_parent,
            original_parent: None,
            contents: tree.finalize(),
        })
    }

    pub(crate) fn with_original_parent(mut self, original: &Path) -> Result<Self> {
        self.original_parent = Some(parent_digest(original)?);
        Ok(self)
    }

    pub(crate) fn validate_target(&self, path: &Path) -> Result<()> {
        let current = Self::capture_with_check(path, || Ok(())).map_err(|_| changed(path))?;
        if self.target_parent != current.target_parent || self.contents != current.contents {
            return Err(changed(path));
        }
        Ok(())
    }

    pub(crate) fn validate_original(&self, original: &Path) -> Result<()> {
        let parent = parent_digest(original).map_err(|_| changed(original))?;
        if self.original_parent != Some(parent) {
            return Err(changed(original));
        }
        match fs::symlink_metadata(original) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
            Ok(_) => Err(BExplorerError::Operation(format!(
                "Cannot undo the move because this location is already occupied: {}",
                original.display()
            ))),
        }
    }
}

/// The count identifies the completed prefix so a failed batch can retain
/// just its pending items for retry.
#[derive(Clone, Debug)]
pub(crate) struct UndoOutcome {
    pub(crate) completed: usize,
    pub(crate) failure: Option<String>,
}

impl UndoOutcome {
    pub(crate) fn complete(completed: usize) -> Self {
        Self {
            completed,
            failure: None,
        }
    }
}

pub(crate) fn undo_transfer(
    kind: TransferKind,
    items: &[TransferCompletedRoot],
) -> Result<UndoOutcome> {
    undo_transfer_with(kind, items, recycle_copy)
}

fn undo_transfer_with(
    kind: TransferKind,
    items: &[TransferCompletedRoot],
    mut recycle: impl FnMut(&Path) -> Result<()>,
) -> Result<UndoOutcome> {
    // Preflight the entire batch so a known conflict leaves every item alone.
    for item in items {
        let snapshot = snapshot(item)?;
        snapshot.validate_target(&item.target)?;
        if kind == TransferKind::Move {
            snapshot.validate_original(&item.source)?;
        }
    }
    for (index, item) in items.iter().enumerate() {
        let result = match kind {
            TransferKind::Copy => snapshot(item)?
                .validate_target(&item.target)
                .and_then(|()| recycle(&item.target)),
            TransferKind::Move => transfer_queue::revert_completed_move(item),
        };
        if let Err(error) = result {
            return Ok(UndoOutcome {
                completed: index,
                failure: Some(error.to_string()),
            });
        }
    }
    Ok(UndoOutcome::complete(items.len()))
}

pub(crate) fn snapshot(item: &TransferCompletedRoot) -> Result<&TransferUndoTarget> {
    item.undo_target
        .as_ref()
        .ok_or_else(|| changed(&item.target))
}

fn changed(path: &Path) -> BExplorerError {
    BExplorerError::Operation(format!(
        "Cannot safely undo this transfer because the item or its location changed: {}",
        path.display()
    ))
}

fn parent_digest(path: &Path) -> Result<Digest> {
    let parent = path
        .parent()
        .ok_or_else(|| BExplorerError::InvalidPath(path.to_path_buf()))?;
    let parent = fs::canonicalize(parent)?;
    if !parent.is_dir() {
        return Err(BExplorerError::InvalidPath(parent));
    }
    let mut digest = MetadataDigest::new();
    digest.add(&transfer_queue::path_identity(&parent)?);
    Ok(digest.finalize())
}

fn metadata_digest(path: &Path) -> Result<Digest> {
    let mut digest = MetadataDigest::new();
    let metadata = fs::symlink_metadata(path)?;
    transfer_queue::ensure_supported(path, &metadata)?;
    #[cfg(windows)]
    {
        let file = transfer_queue::open_metadata_handle(path)?;
        windows_basic_metadata(&file, &mut digest)?;
        digest.add(&transfer_queue::opened_file_identity(&file)?);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        digest.add(&(
            metadata.dev(),
            metadata.ino(),
            metadata.ctime(),
            metadata.ctime_nsec(),
            metadata.mode(),
            metadata.uid(),
            metadata.gid(),
        ));
    }
    // On Unix these times are nanosecond-resolution data from the same stat.
    // On Windows FILE_BASIC_INFO adds change time, which catches edits even
    // if an application resets the modification time to its earlier value.
    digest.add(&metadata.modified()?);
    digest.add(&metadata.len());
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        digest.add(&fs::read_link(path)?.as_os_str().as_encoded_bytes());
    } else if !file_type.is_dir() && !file_type.is_file() {
        return Err(BExplorerError::InvalidPath(path.to_path_buf()));
    }
    digest.add(&(
        file_type.is_dir(),
        file_type.is_file(),
        file_type.is_symlink(),
    ));
    Ok(digest.finalize())
}

#[cfg(windows)]
fn windows_basic_metadata(file: &fs::File, digest: &mut MetadataDigest) -> Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        FILE_BASIC_INFO, FileBasicInfo, GetFileInformationByHandleEx,
    };

    let mut basic = FILE_BASIC_INFO::default();
    unsafe {
        GetFileInformationByHandleEx(
            HANDLE(file.as_raw_handle()),
            FileBasicInfo,
            (&mut basic as *mut FILE_BASIC_INFO).cast(),
            std::mem::size_of_val(&basic) as u32,
        )?;
    }
    digest.add(&(basic.ChangeTime, basic.CreationTime, basic.FileAttributes));
    Ok(())
}

// Hash the native path representation and metadata without reading the file
// contents or requiring filenames to be valid Unicode.
struct MetadataDigest(md5::Context);

impl MetadataDigest {
    fn new() -> Self {
        Self(md5::Context::new())
    }

    fn add(&mut self, value: &(impl Hash + ?Sized)) {
        value.hash(self);
    }

    fn finalize(self) -> Digest {
        self.0.compute().0
    }
}

impl std::hash::Hasher for MetadataDigest {
    fn finish(&self) -> u64 {
        let bytes = self.0.clone().compute().0;
        u64::from_ne_bytes(std::array::from_fn(|index| bytes[index]))
    }

    fn write(&mut self, bytes: &[u8]) {
        self.0.consume(bytes);
    }
}

#[cfg(not(windows))]
fn recycle_copy(path: &Path) -> Result<()> {
    trash::delete(path).map_err(|error| recycle_error(path, error))
}

#[cfg(windows)]
fn recycle_copy(path: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize,
    };
    use windows::Win32::UI::Shell::{
        FOF_NO_CONNECTED_ELEMENTS, FOF_NOERRORUI, FOF_SILENT, FOFX_EARLYFAILURE,
        FOFX_RECYCLEONDELETE, FileOperation, IFileOperation, IShellItem,
        SHCreateItemFromParsingName,
    };
    use windows::core::PCWSTR;

    let parent = path.parent().ok_or_else(|| changed(path))?;
    let filename = path.file_name().ok_or_else(|| changed(path))?;
    let canonical = fs::canonicalize(parent)?.join(filename);
    let mut wide: Vec<_> = canonical.as_os_str().encode_wide().collect();
    let verbatim_local = [b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
    if wide.starts_with(&verbatim_local) && wide.get(5) == Some(&(b':' as u16)) {
        wide.drain(..verbatim_local.len());
    }
    // Shell recycling is for local volumes. Leave network items in place.
    if wide.get(1) != Some(&(b':' as u16)) {
        return Err(recycle_error(
            path,
            "the Recycle Bin is unavailable on this path",
        ));
    }
    wide.push(0);

    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()? };
    struct ComApartment;
    impl Drop for ComApartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }
    let _apartment = ComApartment;
    unsafe {
        let operation: IFileOperation =
            CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER)?;
        // FOF_ALLOWUNDO is only a best effort. Request recycling explicitly.
        operation.SetOperationFlags(
            FOFX_RECYCLEONDELETE
                | FOFX_EARLYFAILURE
                | FOF_NOERRORUI
                | FOF_SILENT
                | FOF_NO_CONNECTED_ELEMENTS,
        )?;
        let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None)?;
        operation.DeleteItem(&item, None)?;
        operation.PerformOperations()?;
        if operation.GetAnyOperationsAborted()?.as_bool() {
            return Err(recycle_error(path, "recycling was declined or unavailable"));
        }
    }
    Ok(())
}

fn recycle_error(path: &Path, error: impl std::fmt::Display) -> BExplorerError {
    BExplorerError::Operation(format!("Could not recycle {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use transfer_queue::{ConflictPolicy, TransferControl, TransferJob, TransferMessage};

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            Self::in_directory(&std::env::temp_dir())
        }

        fn in_directory(parent: &Path) -> Self {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = parent.join(format!("bexplorer-undo-{}-{stamp}", std::process::id()));
            fs::create_dir(&root).unwrap();
            Self(root)
        }

        fn directory(&self, name: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::create_dir(&path).unwrap();
            path
        }

        fn file(&self, name: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, b"original").unwrap();
            path
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn run_transfer(
        kind: TransferKind,
        sources: Vec<PathBuf>,
        destination: PathBuf,
    ) -> Vec<TransferCompletedRoot> {
        let (sender, messages) = mpsc::channel();
        transfer_queue::run_transfer(
            TransferJob {
                id: 1,
                sources,
                destination,
                kind,
                conflict_policy: ConflictPolicy::KeepBoth,
            },
            sender,
            TransferControl::new(),
        );
        for message in messages {
            match message {
                TransferMessage::Finished {
                    completed_roots, ..
                } => return completed_roots,
                TransferMessage::Failed { error, .. } => panic!("{error}"),
                _ => {}
            }
        }
        panic!("transfer did not report completion");
    }

    #[test]
    fn copies_are_recycled_only_after_batch_preflight() {
        let fixture = Fixture::new();
        let first = fixture.file("first");
        let second = fixture.file("second");
        let destination = fixture.directory("destination");
        let recycle_bin = fixture.directory("recycle-bin");
        let items = run_transfer(TransferKind::Copy, vec![first, second], destination);
        let outcome = undo_transfer_with(TransferKind::Copy, &items, |path| {
            fs::rename(path, recycle_bin.join(path.file_name().unwrap()))?;
            Ok(())
        })
        .unwrap();
        assert_eq!(outcome.completed, 2);
        assert!(outcome.failure.is_none());
        assert_eq!(fs::read(recycle_bin.join("first")).unwrap(), b"original");
        assert_eq!(fs::read(recycle_bin.join("second")).unwrap(), b"original");
    }

    #[test]
    fn an_edited_copy_is_preserved_even_with_its_original_modified_time() {
        let fixture = Fixture::new();
        let first = fixture.file("first");
        let second = fixture.file("second");
        let items = run_transfer(
            TransferKind::Copy,
            vec![first, second],
            fixture.directory("out"),
        );
        let edited = &items[1].target;
        let modified = fs::metadata(edited).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(25));
        fs::write(edited, b"modified").unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(edited)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(modified))
            .unwrap();
        assert!(
            undo_transfer_with(TransferKind::Copy, &items, |_| panic!("preflight failed")).is_err()
        );
        assert_eq!(fs::read(edited).unwrap(), b"modified");
        assert_eq!(fs::read(&items[0].target).unwrap(), b"original");
    }

    #[test]
    fn a_replacement_file_at_the_same_path_is_preserved() {
        let fixture = Fixture::new();
        let source = fixture.file("source");
        let items = run_transfer(TransferKind::Copy, vec![source], fixture.directory("out"));
        let target = &items[0].target;
        fs::rename(target, fixture.0.join("old-identity")).unwrap();
        fs::write(target, b"original").unwrap();
        assert!(
            undo_transfer_with(TransferKind::Copy, &items, |_| panic!(
                "replacement recycled"
            ))
            .is_err()
        );
        assert_eq!(fs::read(target).unwrap(), b"original");
    }

    #[test]
    fn a_new_nested_file_blocks_copy_undo() {
        let fixture = Fixture::new();
        let source = fixture.directory("source");
        let nested = source.join("nested");
        fs::create_dir(&nested).unwrap();
        fs::write(nested.join("before"), b"original").unwrap();
        let items = run_transfer(TransferKind::Copy, vec![source], fixture.directory("out"));
        let added = items[0].target.join("nested").join("added");
        fs::write(&added, b"new file").unwrap();
        assert!(
            undo_transfer_with(TransferKind::Copy, &items, |_| panic!("folder recycled")).is_err()
        );
        assert_eq!(fs::read(&added).unwrap(), b"new file");
    }

    #[test]
    fn undo_moves_back_to_the_exact_name_and_refuses_an_occupied_original() {
        let fixture = Fixture::new();
        let source = fixture.file("source.txt");
        let destination = fixture.directory("out");
        fs::write(destination.join("source.txt"), b"preexisting").unwrap();
        let items = run_transfer(
            TransferKind::Move,
            vec![source.clone()],
            destination.clone(),
        );
        let renamed = &items[0].target;
        assert_ne!(renamed.file_name(), source.file_name());
        fs::write(&source, b"replacement").unwrap();
        assert!(undo_transfer(TransferKind::Move, &items).is_err());
        assert_eq!(fs::read(&source).unwrap(), b"replacement");
        assert_eq!(fs::read(renamed).unwrap(), b"original");

        fs::remove_file(&source).unwrap();
        let outcome = undo_transfer(TransferKind::Move, &items).unwrap();
        assert_eq!(outcome.completed, 1, "{:?}", outcome.failure);
        assert_eq!(fs::read(source).unwrap(), b"original");
        assert_eq!(
            fs::read(destination.join("source.txt")).unwrap(),
            b"preexisting"
        );
        assert!(!renamed.exists());
    }

    #[test]
    fn rename_does_not_replace_a_file_that_appeared_at_the_destination() {
        let fixture = Fixture::new();
        let source = fixture.file("source");
        let target = fixture.0.join("target");
        fs::write(&target, b"preexisting").unwrap();
        assert!(transfer_queue::rename_without_replace(&source, &target).is_err());
        assert_eq!(fs::read(&source).unwrap(), b"original");
        assert_eq!(fs::read(&target).unwrap(), b"preexisting");
    }

    #[test]
    fn a_recycle_failure_reports_only_the_preceding_completed_items() {
        let fixture = Fixture::new();
        let first = fixture.file("first");
        let second = fixture.file("second");
        let recycle_bin = fixture.directory("recycle-bin");
        let items = run_transfer(
            TransferKind::Copy,
            vec![first, second],
            fixture.directory("out"),
        );
        let outcome = undo_transfer_with(TransferKind::Copy, &items, |path| {
            if path == items[1].target {
                return Err(recycle_error(path, "fixture failure"));
            }
            fs::rename(path, recycle_bin.join("first"))?;
            Ok(())
        })
        .unwrap();
        assert_eq!(outcome.completed, 1);
        assert!(outcome.failure.is_some());
        assert_eq!(fs::read(recycle_bin.join("first")).unwrap(), b"original");
        assert_eq!(fs::read(&items[1].target).unwrap(), b"original");
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    #[ignore = "requires a separate writable volume, selected with BEXPLORER_TEST_UNDO_OTHER_VOLUME on Windows"]
    fn restores_a_move_across_filesystems() {
        let original = Fixture::new();
        #[cfg(target_os = "linux")]
        let other_volume = PathBuf::from("/dev/shm");
        #[cfg(windows)]
        let other_volume = PathBuf::from(
            std::env::var_os("BEXPLORER_TEST_UNDO_OTHER_VOLUME")
                .expect("set a writable fixture location on another volume"),
        );
        let device = Fixture::in_directory(&other_volume);
        let source = original.file("source");
        let rename_error = transfer_queue::rename_without_replace(&source, &device.0.join("probe"))
            .expect_err("the test folders must be on different filesystems");
        assert_eq!(rename_error.kind(), std::io::ErrorKind::CrossesDevices);
        let items = run_transfer(TransferKind::Move, vec![source.clone()], device.0.clone());
        assert!(!source.exists());
        let outcome = undo_transfer(TransferKind::Move, &items).unwrap();
        assert_eq!(outcome.completed, 1, "{:?}", outcome.failure);
        assert_eq!(fs::read(source).unwrap(), b"original");
        assert!(!items[0].target.exists());
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    #[ignore = "requires a native Recycle Bin; only a generated fixture is recycled and restored"]
    fn copies_roundtrip_through_the_native_trash() {
        let fixture = Fixture::new();
        let source = fixture.file("source");
        let items = run_transfer(TransferKind::Copy, vec![source], fixture.directory("out"));
        let target = &items[0].target;
        let outcome = undo_transfer(TransferKind::Copy, &items).unwrap();
        assert_eq!(outcome.completed, 1, "{:?}", outcome.failure);
        assert!(!target.exists());
        let entry = crate::fs::trash::native_items()
            .unwrap()
            .into_iter()
            .find(|entry| &entry.original_path() == target)
            .expect("undo should recycle the fixture without erasing it");
        trash::os_limited::restore_all([entry]).unwrap();
        assert_eq!(fs::read(target).unwrap(), b"original");
    }
}
