use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::utils::errors::{BExplorerError, Result};

pub use crate::platform::shell::{FormatDriveIdentity, FormatDriveOutcome};

#[derive(Clone, Debug)]
pub struct TrashUndoRecord {
    pub id: OsString,
    pub original_path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct TrashDeleteOutcome {
    pub count: usize,
    pub undo_records: Vec<TrashUndoRecord>,
    pub cancelled: bool,
}

#[derive(Clone, Debug, Default)]
pub struct DeleteProgress {
    pub current_name: String,
    pub completed_items: usize,
    pub total_items: usize,
}

#[derive(Clone, Debug)]
pub struct DeleteControl {
    cancel: Arc<AtomicBool>,
    progress: Arc<Mutex<DeleteProgress>>,
}

impl DeleteControl {
    pub fn new() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            progress: Arc::new(Mutex::new(DeleteProgress::default())),
        }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn progress(&self) -> DeleteProgress {
        self.progress
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn update_progress(&self, update: impl FnOnce(&mut DeleteProgress)) {
        let mut progress = self
            .progress
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        update(&mut progress);
    }
}

impl Default for DeleteControl {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PermanentDeleteOutcome {
    pub count: usize,
    pub completed_items: usize,
    pub cancelled: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub enum ElevatedDeleteKind {
    Trash,
    Permanent,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum ElevatedFileAction {
    CreateFolder { parent: PathBuf, name: String },
    CreateFile { parent: PathBuf, name: String },
    Rename { path: PathBuf, name: String },
}

#[derive(Debug, Deserialize, Serialize)]
struct ElevatedDeleteRequest {
    paths: Vec<PathBuf>,
    kind: ElevatedDeleteKind,
}

#[derive(Debug, Deserialize, Serialize)]
struct ElevatedFileActionRequest {
    action: ElevatedFileAction,
}

pub fn error_message_is_permission_denied(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("access is denied")
        || message.contains("acceso denegado")
        || message.contains("permission denied")
        // Windows IFileOperation deliberately hides the HRESULT for a
        // rejected recycle operation and reports only that it was aborted.
        // The normal trash attempt has already failed, so offer the same
        // explicit elevation retry used for a regular access-denied error.
        || message.contains("some operations were aborted")
        || message.contains("os error 5")
        || message.contains("0x80070005")
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
const ELEVATED_DELETE_HELPER_ARG: &str = "--bexplorer-elevated-delete-helper";

#[cfg(any(target_os = "windows", target_os = "linux"))]
pub fn run_elevated_delete(paths: &[PathBuf], kind: ElevatedDeleteKind) -> Result<usize> {
    let (request_path, result_path) = elevated_delete_paths();
    let request = ElevatedDeleteRequest {
        paths: paths.to_vec(),
        kind,
    };
    crate::utils::atomic_file::write(&request_path, &serde_json::to_vec(&request)?)?;
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&result_path)?;
    let exit_code = crate::platform::shell::run_elevated_current_exe(&[
        OsString::from(ELEVATED_DELETE_HELPER_ARG),
        request_path.clone().into_os_string(),
        result_path.clone().into_os_string(),
    ]);
    let _ = fs::remove_file(&request_path);
    let decoded = fs::read(&result_path).ok().and_then(|bytes| {
        serde_json::from_slice::<std::result::Result<usize, String>>(&bytes).ok()
    });
    let _ = fs::remove_file(&result_path);
    if let Some(result) = decoded {
        return result.map_err(BExplorerError::Operation);
    }
    match exit_code {
        Ok(code) => Err(BExplorerError::Operation(format!(
            "Elevated delete failed with exit code {code}"
        ))),
        Err(error) => Err(error),
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn run_elevated_delete(_paths: &[PathBuf], _kind: ElevatedDeleteKind) -> Result<usize> {
    Err(BExplorerError::Operation(
        "Elevated deletion is currently available on Windows and Linux only".into(),
    ))
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
const ELEVATED_FILE_ACTION_HELPER_ARG: &str = "--bexplorer-elevated-file-action-helper";

#[cfg(any(target_os = "windows", target_os = "linux"))]
pub fn run_elevated_file_action(action: &ElevatedFileAction) -> Result<PathBuf> {
    let (request_path, result_path) = elevated_file_action_paths();
    let request = ElevatedFileActionRequest {
        action: action.clone(),
    };
    crate::utils::atomic_file::write(&request_path, &serde_json::to_vec(&request)?)?;
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&result_path)?;
    let exit_code = crate::platform::shell::run_elevated_current_exe(&[
        OsString::from(ELEVATED_FILE_ACTION_HELPER_ARG),
        request_path.clone().into_os_string(),
        result_path.clone().into_os_string(),
    ]);
    let _ = fs::remove_file(&request_path);
    let decoded = fs::read(&result_path).ok().and_then(|bytes| {
        serde_json::from_slice::<std::result::Result<PathBuf, String>>(&bytes).ok()
    });
    let _ = fs::remove_file(&result_path);
    if let Some(result) = decoded {
        return result.map_err(BExplorerError::Operation);
    }
    match exit_code {
        Ok(code) => Err(BExplorerError::Operation(format!(
            "Elevated file action failed with exit code {code}"
        ))),
        Err(error) => Err(error),
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn run_elevated_file_action(_action: &ElevatedFileAction) -> Result<PathBuf> {
    Err(BExplorerError::Operation(
        "Elevated file actions are currently available on Windows and Linux only".into(),
    ))
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
pub fn try_run_elevated_file_action_helper_from_args() -> Option<i32> {
    let mut args = std::env::args_os();
    let _exe = args.next();
    if args.next()?.as_os_str() != OsStr::new(ELEVATED_FILE_ACTION_HELPER_ARG) {
        return None;
    }
    let request_path = PathBuf::from(args.next()?);
    let result_path = PathBuf::from(args.next()?);
    Some(run_elevated_file_action_helper(&request_path, &result_path))
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn run_elevated_file_action_helper(request_path: &Path, result_path: &Path) -> i32 {
    let result = (|| -> Result<PathBuf> {
        let request: ElevatedFileActionRequest = serde_json::from_slice(&fs::read(request_path)?)?;
        match request.action {
            ElevatedFileAction::CreateFolder { parent, name } => {
                create_folder_named(&parent, &name)
            }
            ElevatedFileAction::CreateFile { parent, name } => {
                create_empty_file_named(&parent, &name)
            }
            ElevatedFileAction::Rename { path, name } => rename_path(&path, &name),
        }
    })();
    let _ = fs::remove_file(request_path);
    let serialized: std::result::Result<PathBuf, String> =
        result.map_err(|error| error.to_string());
    let wrote_result = serde_json::to_vec(&serialized)
        .ok()
        .and_then(|bytes| crate::utils::atomic_file::write_precreated(result_path, &bytes).ok())
        .is_some();
    if !wrote_result {
        2
    } else if serialized.is_ok() {
        0
    } else {
        1
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn elevated_file_action_paths() -> (PathBuf, PathBuf) {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let base = format!(
        "bexplorer-elevated-file-action-{}-{stamp}",
        std::process::id()
    );
    let temp = std::env::temp_dir();
    (
        temp.join(format!("{base}.request.json")),
        temp.join(format!("{base}.result.json")),
    )
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
pub fn try_run_elevated_delete_helper_from_args() -> Option<i32> {
    let mut args = std::env::args_os();
    let _exe = args.next();
    if args.next()?.as_os_str() != OsStr::new(ELEVATED_DELETE_HELPER_ARG) {
        return None;
    }
    let request_path = PathBuf::from(args.next()?);
    let result_path = PathBuf::from(args.next()?);
    Some(run_elevated_delete_helper(&request_path, &result_path))
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn run_elevated_delete_helper(request_path: &Path, result_path: &Path) -> i32 {
    let result = (|| -> Result<usize> {
        let request: ElevatedDeleteRequest = serde_json::from_slice(&fs::read(request_path)?)?;
        match request.kind {
            ElevatedDeleteKind::Trash => delete_to_trash(&request.paths),
            ElevatedDeleteKind::Permanent => delete_permanently(&request.paths),
        }
    })();
    let _ = fs::remove_file(request_path);
    let serialized: std::result::Result<usize, String> = result.map_err(|error| error.to_string());
    let wrote_result = serde_json::to_vec(&serialized)
        .ok()
        .and_then(|bytes| crate::utils::atomic_file::write_precreated(result_path, &bytes).ok())
        .is_some();
    if !wrote_result {
        2
    } else if serialized.is_ok() {
        0
    } else {
        1
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn elevated_delete_paths() -> (PathBuf, PathBuf) {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let base = format!("bexplorer-elevated-delete-{}-{stamp}", std::process::id());
    let temp = std::env::temp_dir();
    (
        temp.join(format!("{base}.request.json")),
        temp.join(format!("{base}.result.json")),
    )
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasteMode {
    Copy,
    Move,
}

pub fn open_path(path: &Path) -> Result<()> {
    crate::platform::shell::open_path(path)
}

pub fn can_mount_disk_image(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase);

    #[cfg(target_os = "windows")]
    return extension.is_some_and(|extension| matches!(extension.as_str(), "iso" | "vhd" | "vhdx"));

    #[cfg(target_os = "macos")]
    return extension.is_some_and(|extension| matches!(extension.as_str(), "dmg" | "iso" | "img"));

    #[cfg(all(unix, not(target_os = "macos")))]
    return extension.is_some_and(|extension| matches!(extension.as_str(), "iso" | "img"));

    #[cfg(not(any(target_os = "windows", unix)))]
    {
        let _ = extension;
        false
    }
}

pub fn mount_disk_image(path: &Path) -> Result<()> {
    crate::platform::shell::mount_disk_image(path)
}

pub fn mounted_disk_image_root(path: &Path) -> Result<PathBuf> {
    crate::platform::shell::mounted_disk_image_root(path)
}

pub fn suppress_file_explorer_windows_at(path: &Path) -> Result<()> {
    crate::platform::shell::suppress_file_explorer_windows_at(path)
}

pub fn eject_drive(path: &Path) -> Result<()> {
    crate::platform::shell::eject_drive(path)
}

pub fn available_format_filesystems(path: &Path) -> Vec<String> {
    crate::platform::shell::available_format_filesystems(path)
}

pub fn format_drive_identity(path: &Path) -> Result<Option<FormatDriveIdentity>> {
    crate::platform::shell::format_drive_identity(path)
}

pub fn format_drive(
    path: &Path,
    filesystem: &str,
    label: &str,
    quick: bool,
    allocation_unit_size: Option<u64>,
    expected_identity: Option<&FormatDriveIdentity>,
) -> Result<FormatDriveOutcome> {
    crate::platform::shell::format_drive(
        path,
        filesystem,
        label,
        quick,
        allocation_unit_size,
        expected_identity,
    )
}

#[allow(dead_code)]
pub fn paste_paths(sources: &[PathBuf], destination: &Path, mode: PasteMode) -> Result<usize> {
    if !destination.is_dir() {
        return Err(BExplorerError::InvalidPath(destination.to_path_buf()));
    }

    let mut completed = 0;
    for source in sources {
        if !source.exists() {
            continue;
        }

        let Some(name) = source.file_name() else {
            continue;
        };

        let target = unique_destination(&destination.join(name), source.is_dir());
        match mode {
            PasteMode::Copy => copy_recursively(source, &target)?,
            PasteMode::Move => move_recursively(source, &target)?,
        }
        completed += 1;
    }

    Ok(completed)
}

pub fn delete_to_trash(paths: &[PathBuf]) -> Result<usize> {
    let mut completed = 0;
    for path in paths {
        if !path.exists() {
            continue;
        }
        trash::delete(path).map_err(|error| {
            BExplorerError::Operation(format!(
                "Could not move {} to trash: {error}",
                path.display()
            ))
        })?;
        completed += 1;
    }
    Ok(completed)
}

/// Move items to the native system trash and retain the native identities
/// needed to restore exactly this deletion. The identity is obtained from the
/// trash implementation rather than guessing the trash filename.
pub fn delete_to_trash_with_undo(paths: &[PathBuf]) -> Result<TrashDeleteOutcome> {
    delete_to_trash_with_undo_controlled(paths, &DeleteControl::new())
}

pub fn delete_to_trash_with_undo_controlled(
    paths: &[PathBuf],
    control: &DeleteControl,
) -> Result<TrashDeleteOutcome> {
    #[cfg(any(
        target_os = "windows",
        all(
            unix,
            not(target_os = "macos"),
            not(target_os = "ios"),
            not(target_os = "android")
        )
    ))]
    let previous_ids = crate::fs::trash::native_items()
        .unwrap_or_default()
        .into_iter()
        .map(|item| item.id)
        .collect::<HashSet<_>>();

    let total_items = paths.iter().filter(|path| path.exists()).count();
    control.update_progress(|progress| {
        progress.completed_items = 0;
        progress.total_items = total_items;
    });
    let mut count = 0;
    for path in paths {
        if control.is_cancelled() {
            break;
        }
        if !path.exists() {
            continue;
        }
        let current_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        control.update_progress(|progress| progress.current_name = current_name);
        trash::delete(path).map_err(|error| {
            BExplorerError::Operation(format!(
                "Could not move {} to trash: {error}",
                path.display()
            ))
        })?;
        count += 1;
        control.update_progress(|progress| progress.completed_items = count);
    }
    let cancelled = control.is_cancelled() && count < total_items;

    #[cfg(any(
        target_os = "windows",
        all(
            unix,
            not(target_os = "macos"),
            not(target_os = "ios"),
            not(target_os = "android")
        )
    ))]
    {
        let originals = paths.iter().cloned().collect::<HashSet<_>>();
        let undo_records = crate::fs::trash::native_items()
            .unwrap_or_default()
            .into_iter()
            .filter(|item| !previous_ids.contains(&item.id))
            .filter_map(|item| {
                let original_path = item.original_path();
                originals
                    .contains(&original_path)
                    .then_some(TrashUndoRecord {
                        id: item.id,
                        original_path,
                    })
            })
            .collect();
        Ok(TrashDeleteOutcome {
            count,
            undo_records,
            cancelled,
        })
    }

    #[cfg(not(any(
        target_os = "windows",
        all(
            unix,
            not(target_os = "macos"),
            not(target_os = "ios"),
            not(target_os = "android")
        )
    )))]
    {
        // The trash crate uses the native macOS trash APIs for deletion, but
        // its cross-platform restoration API is not available there.
        Ok(TrashDeleteOutcome {
            count,
            undo_records: Vec::new(),
            cancelled,
        })
    }
}

pub fn restore_from_trash(records: &[TrashUndoRecord]) -> Result<usize> {
    if records.is_empty() {
        return Err(BExplorerError::Operation(
            "No hay elementos de la papelera para restaurar".into(),
        ));
    }

    #[cfg(any(
        target_os = "windows",
        all(
            unix,
            not(target_os = "macos"),
            not(target_os = "ios"),
            not(target_os = "android")
        )
    ))]
    {
        let ids = records
            .iter()
            .map(|record| record.id.clone())
            .collect::<HashSet<_>>();
        let items = crate::fs::trash::native_items()?
            .into_iter()
            .filter(|item| ids.contains(&item.id))
            .collect::<Vec<_>>();
        if items.len() != records.len() {
            return Err(BExplorerError::Operation(
                "Algunos elementos ya no están disponibles en la papelera".into(),
            ));
        }
        let count = items.len();
        trash::os_limited::restore_all(items).map_err(|error| {
            BExplorerError::Operation(format!("No se pudo restaurar desde la papelera: {error}"))
        })?;
        Ok(count)
    }

    #[cfg(not(any(
        target_os = "windows",
        all(
            unix,
            not(target_os = "macos"),
            not(target_os = "ios"),
            not(target_os = "android")
        )
    )))]
    {
        let _ = records;
        Err(BExplorerError::Operation(
            "La restauración desde la papelera aún no está disponible en este sistema".into(),
        ))
    }
}

/// Revert a completed move without replacing a file that appeared at the
/// original location after the move.
pub fn move_paths_back(moves: &[(PathBuf, PathBuf)]) -> Result<usize> {
    if moves.is_empty() {
        return Err(BExplorerError::Operation(
            "No hay elementos movidos para devolver".into(),
        ));
    }
    for (moved_path, original_path) in moves {
        if !moved_path.exists() {
            return Err(BExplorerError::InvalidPath(moved_path.clone()));
        }
        if original_path.exists() {
            return Err(BExplorerError::Operation(format!(
                "No se puede deshacer porque ya existe {}",
                original_path.display()
            )));
        }
    }
    for (moved_path, original_path) in moves {
        if let Some(parent) = original_path.parent() {
            fs::create_dir_all(parent)?;
        }
        move_recursively(moved_path, original_path)?;
    }
    Ok(moves.len())
}

pub fn delete_permanently(paths: &[PathBuf]) -> Result<usize> {
    Ok(delete_permanently_with_control(paths, &DeleteControl::new())?.count)
}

pub fn delete_permanently_with_control(
    paths: &[PathBuf],
    control: &DeleteControl,
) -> Result<PermanentDeleteOutcome> {
    control.update_progress(|progress| *progress = DeleteProgress::default());
    let Some(total_items) = count_delete_entries(paths, control)? else {
        return Ok(PermanentDeleteOutcome {
            count: 0,
            completed_items: 0,
            cancelled: true,
        });
    };
    control.update_progress(|progress| progress.total_items = total_items);

    let mut completed_roots = 0;
    for path in paths {
        if control.is_cancelled() {
            return Ok(PermanentDeleteOutcome {
                count: completed_roots,
                completed_items: control.progress().completed_items,
                cancelled: true,
            });
        }
        if delete_path_with_progress(path, control)? {
            completed_roots += 1;
        } else {
            return Ok(PermanentDeleteOutcome {
                count: completed_roots,
                completed_items: control.progress().completed_items,
                cancelled: true,
            });
        }
    }

    Ok(PermanentDeleteOutcome {
        count: completed_roots,
        completed_items: control.progress().completed_items,
        cancelled: false,
    })
}

fn count_delete_entries(paths: &[PathBuf], control: &DeleteControl) -> Result<Option<usize>> {
    let mut pending = paths.to_vec();
    let mut total = 0_usize;
    while let Some(path) = pending.pop() {
        if control.is_cancelled() {
            return Ok(None);
        }
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        total = total.saturating_add(1);
        control.update_progress(|progress| {
            progress.current_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_owned();
        });
        if metadata.file_type().is_dir() {
            for entry in fs::read_dir(path)? {
                pending.push(entry?.path());
            }
        }
    }
    Ok(Some(total))
}

fn delete_path_with_progress(path: &Path, control: &DeleteControl) -> Result<bool> {
    enum PendingDelete {
        Visit(PathBuf),
        RemoveDirectory(PathBuf),
    }

    let mut pending = vec![PendingDelete::Visit(path.to_path_buf())];
    while let Some(item) = pending.pop() {
        if control.is_cancelled() {
            return Ok(false);
        }
        let (path, directory) = match item {
            PendingDelete::Visit(path) => {
                let metadata = match fs::symlink_metadata(&path) {
                    Ok(metadata) => metadata,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(error) => return Err(error.into()),
                };
                if metadata.file_type().is_dir() {
                    let children = fs::read_dir(&path)?
                        .map(|entry| entry.map(|entry| entry.path()))
                        .collect::<std::io::Result<Vec<_>>>()?;
                    pending.push(PendingDelete::RemoveDirectory(path));
                    pending.extend(children.into_iter().map(PendingDelete::Visit));
                    continue;
                }
                (path, false)
            }
            PendingDelete::RemoveDirectory(path) => (path, true),
        };

        control.update_progress(|progress| {
            progress.current_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_owned();
        });
        if directory {
            fs::remove_dir(&path)?;
        } else if let Err(file_error) = fs::remove_file(&path) {
            if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
                fs::remove_dir(&path).map_err(|_| file_error)?;
            } else {
                return Err(file_error.into());
            }
        }
        control.update_progress(|progress| {
            progress.completed_items = progress.completed_items.saturating_add(1);
        });
    }
    Ok(true)
}

pub fn rename_path(path: &Path, new_name: &str) -> Result<PathBuf> {
    let new_name = new_name.trim();
    if new_name.is_empty() {
        return Err(BExplorerError::Operation("Name cannot be empty".into()));
    }

    if is_drive_root(path) {
        crate::platform::set_volume_label(path, new_name)?;
        return Ok(path.to_path_buf());
    }

    let parent = path
        .parent()
        .ok_or_else(|| BExplorerError::InvalidPath(path.to_path_buf()))?;
    let target = parent.join(new_name);
    fs::rename(path, &target)?;
    Ok(target)
}

fn is_drive_root(path: &Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        let path = path.display().to_string().replace('/', "\\");
        let trimmed = path.trim_end_matches('\\');
        let mut chars = trimmed.chars();
        chars
            .next()
            .is_some_and(|letter| letter.is_ascii_alphabetic())
            && chars.next() == Some(':')
            && chars.next().is_none()
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        false
    }
}

pub fn create_folder_named(parent: &Path, name: &str) -> Result<PathBuf> {
    let target = unique_destination(&parent.join(name), true);
    fs::create_dir_all(&target)?;
    Ok(target)
}

pub fn create_empty_file_named(parent: &Path, name: &str) -> Result<PathBuf> {
    let target = unique_destination(&parent.join(name), false);
    let mut file = fs::File::create(&target)?;
    file.flush()?;
    Ok(target)
}

#[allow(dead_code)]
pub fn duplicate_path(path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| BExplorerError::InvalidPath(path.to_path_buf()))?;
    let Some(name) = path.file_name() else {
        return Err(BExplorerError::InvalidPath(path.to_path_buf()));
    };
    let target = unique_destination(&parent.join(name), path.is_dir());
    copy_recursively(path, &target)?;
    Ok(target)
}

fn copy_recursively(source: &Path, target: &Path) -> Result<()> {
    if source.is_dir() {
        fs::create_dir_all(target)?;
        for item in fs::read_dir(source)? {
            let item = item?;
            let child_source = item.path();
            let child_target = target.join(item.file_name());
            copy_recursively(&child_source, &child_target)?;
        }
    } else {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, target)?;
    }
    Ok(())
}

#[allow(dead_code)]
fn move_recursively(source: &Path, target: &Path) -> Result<()> {
    match fs::rename(source, target) {
        Ok(()) => Ok(()),
        Err(_) => {
            copy_recursively(source, target)?;
            if source.is_dir() {
                fs::remove_dir_all(source)?;
            } else {
                fs::remove_file(source)?;
            }
            Ok(())
        }
    }
}

fn unique_destination(base: &Path, is_dir: bool) -> PathBuf {
    if !base.exists() {
        return base.to_path_buf();
    }

    let parent = base.parent().unwrap_or_else(|| Path::new(""));
    let stem = base
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("Copy");
    let extension = base.extension().and_then(|value| value.to_str());

    for index in 1..10_000 {
        let candidate_name = if is_dir {
            format!("{stem} copy {index}")
        } else if let Some(extension) = extension {
            format!("{stem} copy {index}.{extension}")
        } else {
            format!("{stem} copy {index}")
        };
        let candidate = parent.join(candidate_name);
        if !candidate.exists() {
            return candidate;
        }
    }

    base.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_test_dir(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("bexplorer-{name}-{}-{stamp}", std::process::id()));
        fs::create_dir_all(&dir).expect("create temp test dir");
        dir
    }

    #[test]
    fn permission_errors_are_recognized_for_delete_elevation() {
        assert!(error_message_is_permission_denied(
            "I/O error: Acceso denegado. (os error 5)"
        ));
        assert!(error_message_is_permission_denied("Permission denied"));
        assert!(error_message_is_permission_denied(
            "Could not move item to trash: Some operations were aborted"
        ));
        assert!(!error_message_is_permission_denied("File not found"));
    }

    #[test]
    fn elevated_delete_requests_preserve_the_delete_kind() {
        let request = ElevatedDeleteRequest {
            paths: vec![PathBuf::from("protected.txt")],
            kind: ElevatedDeleteKind::Permanent,
        };
        let restored: ElevatedDeleteRequest =
            serde_json::from_slice(&serde_json::to_vec(&request).expect("serialize request"))
                .expect("deserialize request");
        assert!(matches!(restored.kind, ElevatedDeleteKind::Permanent));
        assert_eq!(restored.paths, request.paths);
    }

    #[test]
    fn elevated_file_actions_preserve_the_requested_operation() {
        let action = ElevatedFileAction::Rename {
            path: PathBuf::from("protected.txt"),
            name: "renamed.txt".into(),
        };
        let request = ElevatedFileActionRequest {
            action: action.clone(),
        };
        let restored: ElevatedFileActionRequest =
            serde_json::from_slice(&serde_json::to_vec(&request).expect("serialize request"))
                .expect("deserialize request");
        assert!(matches!(restored.action, ElevatedFileAction::Rename { .. }));
        assert!(matches!(action, ElevatedFileAction::Rename { .. }));
    }

    #[test]
    fn creates_folder_and_unique_folder_name() {
        let dir = temp_test_dir("folder");
        let first = create_folder_named(&dir, "Nueva carpeta").expect("create first folder");
        let second = create_folder_named(&dir, "Nueva carpeta").expect("create unique folder");

        assert!(first.is_dir());
        assert!(second.is_dir());
        assert_ne!(first, second);

        fs::remove_dir_all(dir).expect("cleanup temp test dir");
    }

    #[test]
    fn creates_text_document_and_unique_file_name() {
        let dir = temp_test_dir("text-document");
        let first = create_empty_file_named(&dir, "Nuevo documento de texto.txt")
            .expect("create first text document");
        let second = create_empty_file_named(&dir, "Nuevo documento de texto.txt")
            .expect("create unique text document");

        assert!(first.is_file());
        assert!(second.is_file());
        assert_ne!(first, second);

        fs::remove_dir_all(dir).expect("cleanup temp test dir");
    }

    #[test]
    fn moves_items_back_to_their_exact_original_location() {
        let root = temp_test_dir("undo-move");
        let original_parent = root.join("original");
        let moved_parent = root.join("moved");
        fs::create_dir_all(&original_parent).expect("create original parent");
        fs::create_dir_all(&moved_parent).expect("create moved parent");
        let original = original_parent.join("document.txt");
        let moved = moved_parent.join("document.txt");
        fs::write(&original, b"undo me").expect("write source");
        fs::rename(&original, &moved).expect("simulate move");

        let restored = move_paths_back(&[(moved.clone(), original.clone())]).expect("undo move");

        assert_eq!(restored, 1);
        assert_eq!(fs::read(&original).expect("read restored"), b"undo me");
        assert!(!moved.exists());
        fs::remove_dir_all(root).expect("cleanup temp dir");
    }

    #[test]
    fn permanent_directory_delete_reports_each_nested_entry() {
        let root = temp_test_dir("delete-progress");
        let target = root.join("large-folder");
        let nested = target.join("nested");
        fs::create_dir_all(&nested).expect("create nested folder");
        fs::write(target.join("first.txt"), b"first").expect("write first file");
        fs::write(nested.join("second.txt"), b"second").expect("write second file");
        let control = DeleteControl::new();

        let outcome = delete_permanently_with_control(std::slice::from_ref(&target), &control)
            .expect("delete folder with progress");

        assert_eq!(outcome.count, 1);
        assert_eq!(outcome.completed_items, 4);
        assert!(!outcome.cancelled);
        assert!(!target.exists());
        let progress = control.progress();
        assert_eq!(progress.total_items, 4);
        assert_eq!(progress.completed_items, 4);
        fs::remove_dir_all(root).expect("cleanup delete progress directory");
    }

    #[test]
    fn pre_cancelled_permanent_delete_preserves_the_folder() {
        let root = temp_test_dir("delete-cancel");
        let target = root.join("keep-folder");
        fs::create_dir_all(&target).expect("create folder to preserve");
        fs::write(target.join("important.txt"), b"keep").expect("write preserved file");
        let control = DeleteControl::new();
        control.cancel();

        let outcome = delete_permanently_with_control(std::slice::from_ref(&target), &control)
            .expect("cancel folder deletion");

        assert_eq!(outcome.count, 0);
        assert_eq!(outcome.completed_items, 0);
        assert!(outcome.cancelled);
        assert_eq!(
            fs::read(target.join("important.txt")).expect("read preserved file"),
            b"keep"
        );
        fs::remove_dir_all(root).expect("cleanup cancelled delete directory");
    }

    #[cfg(unix)]
    #[test]
    fn permanent_delete_does_not_follow_directory_symlinks() {
        let root = temp_test_dir("delete-symlink");
        let target = root.join("delete-me");
        let external = root.join("keep-me");
        fs::create_dir_all(&target).expect("create target folder");
        fs::create_dir_all(&external).expect("create external folder");
        fs::write(external.join("important.txt"), b"keep").expect("write external file");
        std::os::unix::fs::symlink(&external, target.join("external-link"))
            .expect("create directory symlink");

        let outcome =
            delete_permanently_with_control(std::slice::from_ref(&target), &DeleteControl::new())
                .expect("delete folder containing symlink");

        assert_eq!(outcome.count, 1);
        assert!(!target.exists());
        assert_eq!(
            fs::read(external.join("important.txt")).expect("read external file"),
            b"keep"
        );
        fs::remove_dir_all(root).expect("cleanup symlink delete directory");
    }

    #[test]
    fn only_offers_disk_images_supported_by_the_current_platform() {
        #[cfg(target_os = "windows")]
        {
            assert!(can_mount_disk_image(Path::new("backup.vhdx")));
            assert!(!can_mount_disk_image(Path::new("backup.qcow2")));
        }
        #[cfg(target_os = "macos")]
        {
            assert!(can_mount_disk_image(Path::new("installer.dmg")));
            assert!(!can_mount_disk_image(Path::new("backup.vhdx")));
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            assert!(can_mount_disk_image(Path::new("installer.iso")));
            assert!(!can_mount_disk_image(Path::new("backup.vmdk")));
        }
    }
}
