use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::utils::errors::{BExplorerError, Result};

const ELEVATED_OPERATION_HELPER_ARG: &str = "--bexplorer-elevated-operation-helper";

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasteMode {
    Copy,
    Move,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum ElevatedFileOperation {
    Rename {
        path: PathBuf,
        new_name: String,
    },
    Delete {
        paths: Vec<PathBuf>,
        permanent: bool,
    },
    CreateFolder {
        parent: PathBuf,
        name: String,
    },
    CreateFile {
        parent: PathBuf,
        name: String,
    },
    Duplicate {
        path: PathBuf,
    },
}

pub fn open_path(path: &Path) -> Result<()> {
    open::that(path).map_err(|error| {
        BExplorerError::Shell(format!("Could not open {}: {error}", path.display()))
    })
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

pub fn run_elevated_file_operation(operation: &ElevatedFileOperation) -> Result<()> {
    let request_path = elevated_operation_request_path();
    let request_json = serde_json::to_string(operation)?;
    fs::write(&request_path, request_json)?;

    let exit_code = crate::platform::shell::run_elevated_current_exe(&[
        OsString::from(ELEVATED_OPERATION_HELPER_ARG),
        request_path.clone().into_os_string(),
    ]);

    if let Ok(code) = exit_code
        && code == 0
    {
        return Ok(());
    }

    if request_path.exists() {
        let _ = fs::remove_file(&request_path);
    }

    match exit_code {
        Ok(code) => Err(BExplorerError::Operation(format!(
            "Elevated operation failed with exit code {code}"
        ))),
        Err(error) => Err(error),
    }
}

pub fn try_run_elevated_operation_helper_from_args() -> Option<i32> {
    let mut args = std::env::args_os();
    let _exe = args.next();
    let marker = args.next()?;
    if marker != OsStr::new(ELEVATED_OPERATION_HELPER_ARG) {
        return None;
    }

    let request_path = PathBuf::from(args.next()?);
    Some(match run_elevated_operation_helper(&request_path) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    })
}

fn run_elevated_operation_helper(request_path: &Path) -> Result<()> {
    let request_json = fs::read_to_string(request_path)?;
    let _ = fs::remove_file(request_path);
    let operation: ElevatedFileOperation =
        serde_json::from_str(request_json.trim_start_matches('\u{feff}')).map_err(|error| {
            BExplorerError::Operation(format!("Elevated operation request decode failed: {error}"))
        })?;
    run_file_operation(&operation)
}

fn run_file_operation(operation: &ElevatedFileOperation) -> Result<()> {
    match operation {
        ElevatedFileOperation::Rename { path, new_name } => {
            rename_path(path, new_name)?;
        }
        ElevatedFileOperation::Delete { paths, permanent } => {
            if *permanent {
                delete_permanently(paths)?;
            } else {
                delete_to_trash(paths)?;
            }
        }
        ElevatedFileOperation::CreateFolder { parent, name } => {
            create_folder_named(parent, name)?;
        }
        ElevatedFileOperation::CreateFile { parent, name } => {
            create_empty_file_named(parent, name)?;
        }
        ElevatedFileOperation::Duplicate { path } => {
            duplicate_path(path)?;
        }
    }
    Ok(())
}

fn elevated_operation_request_path() -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "bexplorer-elevated-operation-{}-{stamp}.json",
        std::process::id()
    ))
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

pub fn delete_permanently(paths: &[PathBuf]) -> Result<usize> {
    let mut completed = 0;
    for path in paths {
        if path.is_dir() {
            fs::remove_dir_all(path)?;
        } else if path.exists() {
            fs::remove_file(path)?;
        }
        completed += 1;
    }
    Ok(completed)
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
        return chars
            .next()
            .is_some_and(|letter| letter.is_ascii_alphabetic())
            && chars.next() == Some(':')
            && chars.next().is_none();
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
}
