use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::fs::explorer;
use crate::utils::errors::{BExplorerError, Result};

#[cfg(target_os = "windows")]
use crate::platform::PortableDeviceSession;

#[derive(Clone, Debug)]
struct PortableObjectRef {
    device_id: String,
    object_id: String,
}

pub enum PortableTransferEvent<'a> {
    BeforeItem(&'a str),
    Bytes(&'a str, u64),
    FileDone(&'a str),
}

pub fn path_name(path: &Path) -> String {
    explorer::virtual_display_name(path)
        .or_else(|| {
            object_ref(path).and_then(|object| object_info(&object).ok().map(|info| info.name))
        })
        .unwrap_or_else(|| {
            path.file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("Item")
                .to_string()
        })
}

pub fn path_total_bytes(path: &Path) -> u64 {
    let Some(object) = object_ref(path) else {
        return 0;
    };
    #[cfg(target_os = "windows")]
    {
        let Ok(session) = PortableDeviceSession::open(&object.device_id) else {
            return 0;
        };
        object_total_bytes(&session, &object)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = object;
        0
    }
}

pub fn path_file_count(path: &Path) -> usize {
    let Some(object) = object_ref(path) else {
        return 0;
    };
    #[cfg(target_os = "windows")]
    {
        let Ok(session) = PortableDeviceSession::open(&object.device_id) else {
            return 0;
        };
        object_file_count(&session, &object)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = object;
        0
    }
}

pub fn path_is_folder(path: &Path) -> bool {
    let Some(object) = object_ref(path) else {
        return false;
    };
    #[cfg(target_os = "windows")]
    {
        let Ok(session) = PortableDeviceSession::open(&object.device_id) else {
            return false;
        };
        session
            .object_info(&object.object_id)
            .is_ok_and(|info| info.is_folder)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = object;
        false
    }
}

pub fn export_to_local<Event>(source: &Path, target: &Path, on_event: &mut Event) -> Result<usize>
where
    Event: FnMut(PortableTransferEvent<'_>) -> Result<()>,
{
    let object =
        object_ref(source).ok_or_else(|| BExplorerError::InvalidPath(source.to_path_buf()))?;
    #[cfg(target_os = "windows")]
    {
        let session = PortableDeviceSession::open(&object.device_id)?;
        export_object_to_local(&session, &object, target, on_event)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = object;
        let _ = (target, on_event);
        Err(BExplorerError::Operation(
            "Portable devices are only supported on Windows".into(),
        ))
    }
}

pub fn import_from_local<Event>(
    source: &Path,
    destination: &Path,
    on_event: &mut Event,
) -> Result<usize>
where
    Event: FnMut(PortableTransferEvent<'_>) -> Result<()>,
{
    let (device_id, parent_object_id) = explorer::portable_object_from_path(destination)
        .ok_or_else(|| BExplorerError::InvalidPath(destination.to_path_buf()))?;
    let name = source
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("Item")
        .to_string();

    #[cfg(target_os = "windows")]
    {
        let session = PortableDeviceSession::open(&device_id)?;
        import_local_object(&session, source, &parent_object_id, &name, on_event)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (source, device_id, parent_object_id, name, on_event);
        Err(BExplorerError::Operation(
            "Portable devices are only supported on Windows".into(),
        ))
    }
}

pub fn delete_paths(paths: &[PathBuf]) -> Result<usize> {
    let mut by_device: HashMap<String, Vec<String>> = HashMap::new();
    for path in paths {
        let (device_id, object_id) = explorer::portable_object_from_path(path)
            .ok_or_else(|| BExplorerError::InvalidPath(path.clone()))?;
        if object_id == "DEVICE" {
            return Err(BExplorerError::Operation(
                "Cannot delete the portable device root".into(),
            ));
        }
        by_device.entry(device_id).or_default().push(object_id);
    }

    let mut completed = 0;
    for (device_id, object_ids) in by_device {
        completed += delete_objects(&device_id, &object_ids)?;
    }
    Ok(completed)
}

pub fn stage_paths_for_clipboard(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }

    cleanup_old_staging(&clipboard_staging_root(), "portable clipboard");
    let root = clipboard_staging_root().join(new_staging_name());
    fs::create_dir_all(&root)?;

    let mut staged = Vec::new();
    let mut reserved = Vec::new();
    for path in paths {
        if !explorer::is_portable_path(path) {
            return Err(BExplorerError::InvalidPath(path.clone()));
        }
        let name = path_name(path);
        let target = unique_destination(
            &root.join(safe_staging_name(&name)),
            path_is_folder(path),
            &reserved,
        );
        reserved.push(target.clone());
        let mut event = |_event: PortableTransferEvent<'_>| Ok(());
        export_to_local(path, &target, &mut event)?;
        staged.push(target);
    }
    Ok(staged)
}

pub fn stage_file_for_open(path: &Path) -> Result<PathBuf> {
    if !explorer::is_portable_path(path) {
        return Err(BExplorerError::InvalidPath(path.to_path_buf()));
    }

    cleanup_old_staging(&open_staging_root(), "portable open");
    let root = open_staging_root().join(new_staging_name());
    fs::create_dir_all(&root)?;

    let name = path_name(path);
    let target = root.join(safe_child_name(&name));
    let mut event = |_event: PortableTransferEvent<'_>| Ok(());
    export_to_local(path, &target, &mut event)?;
    Ok(target)
}

#[cfg(target_os = "windows")]
fn export_object_to_local<Event>(
    session: &PortableDeviceSession,
    object: &PortableObjectRef,
    target: &Path,
    on_event: &mut Event,
) -> Result<usize>
where
    Event: FnMut(PortableTransferEvent<'_>) -> Result<()>,
{
    let info = session.object_info(&object.object_id)?;
    on_event(PortableTransferEvent::BeforeItem(&info.name))?;
    if info.is_folder {
        fs::create_dir_all(target)?;
        let mut completed = 0;
        for child in session.list_objects(&object.object_id)? {
            let child_ref = PortableObjectRef {
                device_id: object.device_id.clone(),
                object_id: child.id,
            };
            completed += export_object_to_local(
                session,
                &child_ref,
                &target.join(safe_child_name(&child.name)),
                on_event,
            )?;
        }
        return Ok(completed);
    }

    let current_name = info.name.clone();
    session.download_file(&object.object_id, target, |bytes| {
        on_event(PortableTransferEvent::Bytes(&current_name, bytes))
    })?;
    on_event(PortableTransferEvent::FileDone(&info.name))?;
    Ok(1)
}

#[cfg(target_os = "windows")]
fn import_local_object<Event>(
    session: &PortableDeviceSession,
    source: &Path,
    parent_object_id: &str,
    name: &str,
    on_event: &mut Event,
) -> Result<usize>
where
    Event: FnMut(PortableTransferEvent<'_>) -> Result<()>,
{
    on_event(PortableTransferEvent::BeforeItem(name))?;
    let metadata = fs::symlink_metadata(source)?;
    if metadata.is_dir() {
        let folder_id = session.create_folder(parent_object_id, name)?;
        let mut completed = 0;
        for item in fs::read_dir(source)? {
            let item = item?;
            let child_name = item.file_name().to_string_lossy().to_string();
            completed +=
                import_local_object(session, &item.path(), &folder_id, &child_name, on_event)?;
        }
        return Ok(completed);
    }

    if !metadata.is_file() {
        return Ok(0);
    }

    let current_name = name.to_string();
    session.upload_file(parent_object_id, source, name, |bytes| {
        on_event(PortableTransferEvent::Bytes(&current_name, bytes))
    })?;
    on_event(PortableTransferEvent::FileDone(name))?;
    Ok(1)
}

#[cfg(target_os = "windows")]
fn object_total_bytes(session: &PortableDeviceSession, object: &PortableObjectRef) -> u64 {
    let Ok(info) = session.object_info(&object.object_id) else {
        return 0;
    };
    if !info.is_folder {
        return info.size.unwrap_or(0);
    }
    session
        .list_objects(&object.object_id)
        .unwrap_or_default()
        .into_iter()
        .map(|child| {
            object_total_bytes(
                session,
                &PortableObjectRef {
                    device_id: object.device_id.clone(),
                    object_id: child.id,
                },
            )
        })
        .sum()
}

#[cfg(target_os = "windows")]
fn object_file_count(session: &PortableDeviceSession, object: &PortableObjectRef) -> usize {
    let Ok(info) = session.object_info(&object.object_id) else {
        return 0;
    };
    if !info.is_folder {
        return 1;
    }
    session
        .list_objects(&object.object_id)
        .unwrap_or_default()
        .into_iter()
        .map(|child| {
            object_file_count(
                session,
                &PortableObjectRef {
                    device_id: object.device_id.clone(),
                    object_id: child.id,
                },
            )
        })
        .sum()
}

fn object_info(object: &PortableObjectRef) -> Result<crate::platform::PortableObjectInfo> {
    crate::platform::portable_device_object_info(&object.device_id, &object.object_id)
}

fn delete_objects(device_id: &str, object_ids: &[String]) -> Result<usize> {
    crate::platform::portable_delete_objects(device_id, object_ids)
}

fn object_ref(path: &Path) -> Option<PortableObjectRef> {
    let (device_id, object_id) = explorer::portable_object_from_path(path)?;
    Some(PortableObjectRef {
        device_id,
        object_id,
    })
}

fn safe_child_name(name: &str) -> String {
    let value = safe_staging_name(name);
    if value.is_empty() {
        "Item".into()
    } else {
        value
    }
}

fn safe_staging_name(name: &str) -> String {
    name.chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => ch,
        })
        .collect::<String>()
        .trim()
        .trim_end_matches('.')
        .to_string()
}

fn clipboard_staging_root() -> PathBuf {
    std::env::temp_dir().join("bexplorer-mtp-clipboard")
}

fn open_staging_root() -> PathBuf {
    std::env::temp_dir().join("bexplorer-mtp-open")
}

fn new_staging_name() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or_default();
    format!("{}-{timestamp}", std::process::id())
}

fn cleanup_old_staging(root: &Path, label: &str) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    let cutoff = std::time::Duration::from_secs(24 * 60 * 60);
    for entry in entries.flatten() {
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        if modified.elapsed().is_ok_and(|age| age > cutoff) {
            let path = entry.path();
            let result = if metadata.is_dir() {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };
            if let Err(error) = result {
                crate::utils::log::error(format!("Could not clean {label} staging: {error}"));
            }
        }
    }
}

fn unique_destination(base: &Path, is_dir: bool, reserved: &[PathBuf]) -> PathBuf {
    if !base.exists() && !reserved.iter().any(|path| path == base) {
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
        if !candidate.exists() && !reserved.iter().any(|path| path == &candidate) {
            return candidate;
        }
    }

    base.to_path_buf()
}
