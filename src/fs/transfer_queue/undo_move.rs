use super::*;

use crate::fs::transfer_undo;

#[cfg(target_os = "linux")]
pub(crate) fn rename_without_replace(source: &Path, target: &Path) -> std::io::Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};

    renameat_with(CWD, source, CWD, target, RenameFlags::NOREPLACE).map_err(Into::into)
}

#[cfg(windows)]
pub(crate) fn rename_without_replace(source: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};
    use windows::core::PCWSTR;

    fn extended_path(path: &Path) -> std::io::Result<Vec<u16>> {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let parent = if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        };
        let filename = path.file_name().ok_or(std::io::ErrorKind::InvalidInput)?;
        Ok(fs::canonicalize(parent)?
            .join(filename)
            .as_os_str()
            .encode_wide()
            .chain([0])
            .collect())
    }

    let source = extended_path(source)?;
    let target = extended_path(target)?;
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(target.as_ptr()),
            MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|error| std::io::Error::from_raw_os_error((error.code().0 as u32 & 0xffff) as i32))
}

#[cfg(target_os = "macos")]
pub(crate) fn rename_without_replace(source: &Path, target: &Path) -> std::io::Result<()> {
    use std::ffi::{CString, c_char, c_int, c_uint};
    use std::os::unix::ffi::OsStrExt;

    unsafe extern "C" {
        fn renamex_np(from: *const c_char, to: *const c_char, flags: c_uint) -> c_int;
    }
    const RENAME_EXCL: c_uint = 0x0000_0004;

    let source = CString::new(source.as_os_str().as_bytes())?;
    let target = CString::new(target.as_os_str().as_bytes())?;
    if unsafe { renamex_np(source.as_ptr(), target.as_ptr(), RENAME_EXCL) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub(crate) fn rename_without_replace(_source: &Path, _target: &Path) -> std::io::Result<()> {
    Err(std::io::ErrorKind::Unsupported.into())
}

pub(crate) fn revert_completed_move(item: &TransferCompletedRoot) -> Result<()> {
    let snapshot = transfer_undo::snapshot(item)?;
    snapshot.validate_target(&item.target)?;
    snapshot.validate_original(&item.source)?;

    match rename_without_replace(&item.target, &item.source) {
        Ok(()) => {
            crate::utils::atomic_file::sync_parent(&item.target);
            crate::utils::atomic_file::sync_parent(&item.source);
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::CrossesDevices => {
            revert_move_across_volumes(item, snapshot)
        }
        Err(error) => Err(error.into()),
    }
}

fn revert_move_across_volumes(
    item: &TransferCompletedRoot,
    snapshot: &transfer_undo::TransferUndoTarget,
) -> Result<()> {
    let destination = item.source.parent().expect("original parent was validated");
    let directory = unused_transfer_sibling(&destination.join("transfer"), "undo");
    metadata::create_directory(&directory)?;
    let staged_item = directory.join("item");
    let job = TransferJob {
        id: 0,
        sources: vec![item.target.clone()],
        destination: destination.to_path_buf(),
        kind: TransferKind::Move,
        conflict_policy: ConflictPolicy::Skip,
    };
    let (messages, receiver) = std::sync::mpsc::channel();
    drop(receiver);
    let mut runtime = TransferRuntime::new(0, 0);
    let control = TransferControl::new();
    let result = (|| {
        copy_path(
            &job,
            &item.target,
            &staged_item,
            &messages,
            &control,
            &mut runtime,
        )?;
        snapshot.validate_target(&item.target)?;
        snapshot.validate_original(&item.source)?;
        rename_without_replace(&staged_item, &item.source)?;
        replacement::sync_parent(&item.source)?;

        // The restored item is now durable. The regular move helper removes
        // only source entries that still match the inventory it copied.
        local::remove_copied_source(&item.target, &runtime.copied_sources, &control).map_err(
            |error| {
                BExplorerError::Operation(format!(
                    "Restored {} but could not remove all of the old copy at {}: {error}",
                    item.source.display(),
                    item.target.display()
                ))
            },
        )?;
        crate::utils::atomic_file::sync_parent(&item.target);
        Ok(())
    })();
    if let Err(error) = remove_source(&directory) {
        crate::utils::log::error(format!(
            "Could not remove undo staging directory {}: {error}",
            directory.display()
        ));
    }
    result
}
