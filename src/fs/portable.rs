use std::collections::{HashMap, HashSet};
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

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub enum PortableTransferEvent<'a> {
    TargetCreated(&'a Path),
    BeforeItem(&'a str),
    Bytes(&'a str, u64),
    FileDone(&'a str),
}

#[derive(Clone, Debug)]
struct ExportedObject {
    object: PortableObjectRef,
    name: String,
    is_folder: bool,
    size: Option<u64>,
}

#[derive(Default)]
pub struct PortableExport {
    pub completed_files: usize,
    objects: Vec<ExportedObject>,
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

/// Collect size and file count in one device traversal. A native driver call
/// may itself block; cancellation is observed before each metadata/list call.
pub fn path_totals_cancellable(
    path: &Path,
    mut checkpoint: impl FnMut() -> Result<()>,
) -> Result<(u64, usize)> {
    checkpoint()?;
    let object = object_ref(path).ok_or_else(|| BExplorerError::InvalidPath(path.into()))?;
    #[cfg(target_os = "windows")]
    {
        let session = PortableDeviceSession::open(&object.device_id)?;
        checkpoint()?;
        let convert = |info: crate::platform::windows::PortableObjectInfo| {
            crate::platform::PortableObjectInfo {
                id: info.id,
                name: info.name,
                is_folder: info.is_folder,
                size: info.size,
            }
        };
        let root = convert(session.object_info(&object.object_id)?);
        collect_object_totals(
            object.object_id,
            root,
            |id| {
                session
                    .list_objects(id)
                    .map(|items| items.into_iter().map(convert).collect())
            },
            checkpoint,
        )
    }
    #[cfg(target_os = "linux")]
    {
        let root = object_info(&object)?;
        collect_object_totals(
            object.object_id.clone(),
            root,
            |id| crate::platform::portable_device_objects_result(&object.device_id, id),
            checkpoint,
        )
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = object;
        Err(BExplorerError::Operation(
            "Portable devices are not supported on this platform".into(),
        ))
    }
}

fn collect_object_totals(
    root_id: String,
    root: crate::platform::PortableObjectInfo,
    mut list: impl FnMut(&str) -> Result<Vec<crate::platform::PortableObjectInfo>>,
    mut checkpoint: impl FnMut() -> Result<()>,
) -> Result<(u64, usize)> {
    let mut pending = vec![(root_id, root)];
    let mut visited = HashSet::new();
    let (mut bytes, mut files) = (0_u64, 0_usize);
    while let Some((id, info)) = pending.pop() {
        checkpoint()?;
        if !visited.insert(id.clone()) {
            return Err(BExplorerError::Operation(
                "The portable device returned a cyclic or repeated object reference".into(),
            ));
        }
        if info.is_folder {
            for child in list(&id)? {
                checkpoint()?;
                pending.push((child.id.clone(), child));
            }
        } else {
            bytes = bytes.saturating_add(info.size.unwrap_or(0));
            files = files.saturating_add(1);
        }
    }
    Ok((bytes, files))
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

    #[cfg(target_os = "linux")]
    {
        object_info(&object).is_ok_and(|info| info.is_folder)
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = object;
        false
    }
}

pub fn export_to_local<Event>(source: &Path, target: &Path, on_event: &mut Event) -> Result<usize>
where
    Event: FnMut(PortableTransferEvent<'_>) -> Result<()>,
{
    export_to_local_with_inventory(source, target, on_event).map(|export| export.completed_files)
}

pub fn export_to_local_with_inventory<Event>(
    source: &Path,
    target: &Path,
    on_event: &mut Event,
) -> Result<PortableExport>
where
    Event: FnMut(PortableTransferEvent<'_>) -> Result<()>,
{
    let object =
        object_ref(source).ok_or_else(|| BExplorerError::InvalidPath(source.to_path_buf()))?;
    let mut export = PortableExport::default();
    #[cfg(target_os = "windows")]
    {
        let session = PortableDeviceSession::open(&object.device_id)?;
        export.completed_files =
            export_object_to_local(&session, &object, target, on_event, &mut export.objects)?;
        Ok(export)
    }

    #[cfg(target_os = "linux")]
    {
        export.completed_files =
            export_object_to_local_linux(&object, target, on_event, &mut export.objects)?;
        Ok(export)
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = object;
        let _ = (target, on_event);
        Err(BExplorerError::Operation(
            "Portable devices are not supported on this platform".into(),
        ))
    }
}

/// Remove only the objects actually exported, in postorder. Fresh children
/// can invalidate a move, but can never become additional deletion candidates.
pub fn remove_exported_sources(
    export: &PortableExport,
    mut checkpoint: impl FnMut() -> Result<()>,
) -> Result<()> {
    let Some(first) = export.objects.first() else {
        return Ok(());
    };
    #[cfg(windows)]
    let session = PortableDeviceSession::open(&first.object.device_id)?;
    #[cfg(windows)]
    let get_info = |entry: &ExportedObject| {
        session.object_info(&entry.object.object_id).map(|info| {
            crate::platform::PortableObjectInfo {
                id: info.id,
                name: info.name,
                is_folder: info.is_folder,
                size: info.size,
            }
        })
    };
    #[cfg(not(windows))]
    let get_info = |entry: &ExportedObject| object_info(&entry.object);
    let mut inspect = |entry: &ExportedObject| {
        let current = get_info(entry)?;
        let children = if entry.is_folder {
            #[cfg(windows)]
            let children = session
                .list_objects(&entry.object.object_id)?
                .into_iter()
                .map(|child| child.id)
                .collect();
            #[cfg(not(windows))]
            let children = crate::platform::portable_device_objects_result(
                &entry.object.device_id,
                &entry.object.object_id,
            )?
            .into_iter()
            .map(|child| child.id)
            .collect();
            children
        } else {
            Vec::new()
        };
        Ok((current, children))
    };
    let remove = |entry: &ExportedObject| {
        #[cfg(windows)]
        {
            session
                .delete_objects_without_recursion(std::slice::from_ref(&entry.object.object_id))
                .map(|_| ())
        }
        #[cfg(target_os = "linux")]
        {
            crate::platform::portable_delete_exported_object(
                &entry.object.device_id,
                &entry.object.object_id,
                entry.is_folder,
            )
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = entry;
            Err(BExplorerError::Operation(
                "The copy completed, but this provider cannot safely remove the original".into(),
            ))
        }
    };
    let _ = first;
    remove_export_inventory(&export.objects, &mut inspect, remove, &mut checkpoint)
}

fn remove_export_inventory(
    inventory: &[ExportedObject],
    mut inspect: impl FnMut(
        &ExportedObject,
    ) -> Result<(crate::platform::PortableObjectInfo, Vec<String>)>,
    mut remove: impl FnMut(&ExportedObject) -> Result<()>,
    mut checkpoint: impl FnMut() -> Result<()>,
) -> Result<()> {
    validate_export_inventory(inventory, |entry| {
        checkpoint()?;
        inspect(entry)
    })?;
    for entry in inventory {
        checkpoint()?;
        let (current, children) = inspect(entry)?;
        validate_exported_metadata(entry, &current)?;
        if entry.is_folder && !children.is_empty() {
            return Err(BExplorerError::Operation("The portable source folder received new files; the completed copy and remaining source were preserved".into()));
        }
        remove(entry)?;
    }
    Ok(())
}

fn validate_exported_metadata(
    entry: &ExportedObject,
    current: &crate::platform::PortableObjectInfo,
) -> Result<()> {
    if !entry.is_folder && entry.size.is_none() {
        return Err(BExplorerError::Operation("The copy completed, but the device did not provide a verifiable source size. The original was preserved.".into()));
    }
    if entry.object.object_id != current.id
        || entry.name != current.name
        || entry.is_folder != current.is_folder
        || entry.size != current.size
    {
        return Err(BExplorerError::Operation("The portable source changed after copying; the completed copy and source were preserved".into()));
    }
    Ok(())
}

fn validate_export_inventory(
    inventory: &[ExportedObject],
    mut inspect: impl FnMut(
        &ExportedObject,
    ) -> Result<(crate::platform::PortableObjectInfo, Vec<String>)>,
) -> Result<()> {
    if inventory
        .iter()
        .any(|entry| entry.object.object_id.trim().is_empty() || entry.object.object_id == "DEVICE")
    {
        return Err(BExplorerError::Operation(
            "Cannot move the portable device root; the copy and source were preserved".into(),
        ));
    }
    let copied = inventory
        .iter()
        .map(|entry| entry.object.object_id.as_str())
        .collect::<HashSet<_>>();
    for entry in inventory {
        let (current, children) = inspect(entry)?;
        validate_exported_metadata(entry, &current)?;
        if children.iter().any(|id| !copied.contains(id.as_str())) {
            return Err(BExplorerError::Operation("The portable source folder received new files; the completed copy and source were preserved".into()));
        }
    }
    Ok(())
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

    #[cfg(target_os = "linux")]
    {
        import_local_object_linux(source, &device_id, &parent_object_id, &name, on_event)
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = (source, device_id, parent_object_id, name, on_event);
        Err(BExplorerError::Operation(
            "Portable devices are not supported on this platform".into(),
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
        let target = unique_local_destination(
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
    inventory: &mut Vec<ExportedObject>,
) -> Result<usize>
where
    Event: FnMut(PortableTransferEvent<'_>) -> Result<()>,
{
    let info = session.object_info(&object.object_id)?;
    on_event(PortableTransferEvent::BeforeItem(&info.name))?;
    if info.is_folder {
        fs::create_dir(target)?;
        on_event(PortableTransferEvent::TargetCreated(target))?;
        let mut completed = 0;
        for child in session.list_objects(&object.object_id)? {
            let child_target = export_child_destination(target, &child.name, child.is_folder);
            let child_ref = PortableObjectRef {
                device_id: object.device_id.clone(),
                object_id: child.id,
            };
            completed +=
                export_object_to_local(session, &child_ref, &child_target, on_event, inventory)?;
        }
        inventory.push(ExportedObject {
            object: object.clone(),
            name: info.name,
            is_folder: true,
            size: info.size,
        });
        return Ok(completed);
    }

    let current_name = info.name.clone();
    let mut created = false;
    session.download_file(&object.object_id, target, |bytes| {
        if !created {
            on_event(PortableTransferEvent::TargetCreated(target))?;
            created = true;
        }
        on_event(PortableTransferEvent::Bytes(&current_name, bytes))
    })?;
    verify_download_size(target, info.size)?;
    on_event(PortableTransferEvent::FileDone(&info.name))?;
    inventory.push(ExportedObject {
        object: object.clone(),
        name: info.name,
        is_folder: false,
        size: info.size,
    });
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

#[cfg(target_os = "linux")]
fn export_object_to_local_linux<Event>(
    object: &PortableObjectRef,
    target: &Path,
    on_event: &mut Event,
    inventory: &mut Vec<ExportedObject>,
) -> Result<usize>
where
    Event: FnMut(PortableTransferEvent<'_>) -> Result<()>,
{
    let info = object_info(object)?;
    on_event(PortableTransferEvent::BeforeItem(&info.name))?;
    if info.is_folder {
        fs::create_dir(target)?;
        on_event(PortableTransferEvent::TargetCreated(target))?;
        let mut completed = 0;
        for child in
            crate::platform::portable_device_objects_result(&object.device_id, &object.object_id)?
        {
            let child_target = export_child_destination(target, &child.name, child.is_folder);
            let child_ref = PortableObjectRef {
                device_id: object.device_id.clone(),
                object_id: child.id,
            };
            completed +=
                export_object_to_local_linux(&child_ref, &child_target, on_event, inventory)?;
        }
        inventory.push(ExportedObject {
            object: object.clone(),
            name: info.name,
            is_folder: true,
            size: info.size,
        });
        return Ok(completed);
    }

    let bytes =
        crate::platform::portable_download_file(&object.device_id, &object.object_id, target)?;
    on_event(PortableTransferEvent::TargetCreated(target))?;
    verify_download_size(target, info.size)?;
    if bytes > 0 {
        on_event(PortableTransferEvent::Bytes(&info.name, bytes))?;
    }
    on_event(PortableTransferEvent::FileDone(&info.name))?;
    inventory.push(ExportedObject {
        object: object.clone(),
        name: info.name,
        is_folder: false,
        size: info.size,
    });
    Ok(1)
}

#[cfg(target_os = "linux")]
fn import_local_object_linux<Event>(
    source: &Path,
    device_id: &str,
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
        let folder_id = crate::platform::portable_create_folder(device_id, parent_object_id, name)?;
        let mut completed = 0;
        for item in fs::read_dir(source)? {
            let item = item?;
            let child_name = item.file_name().to_string_lossy().to_string();
            completed += import_local_object_linux(
                &item.path(),
                device_id,
                &folder_id,
                &child_name,
                on_event,
            )?;
        }
        return Ok(completed);
    }

    if !metadata.is_file() {
        return Ok(0);
    }

    crate::platform::portable_upload_file(device_id, parent_object_id, source, name)?;
    if metadata.len() > 0 {
        on_event(PortableTransferEvent::Bytes(name, metadata.len()))?;
    }
    on_event(PortableTransferEvent::FileDone(name))?;
    Ok(1)
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

pub(crate) fn safe_child_name(name: &str) -> String {
    let value = safe_staging_name(name);
    if value.is_empty() {
        "Item".into()
    } else {
        value
    }
}

fn export_child_destination(parent: &Path, name: &str, is_folder: bool) -> PathBuf {
    unique_local_destination(&parent.join(safe_child_name(name)), is_folder, &[])
}

fn verify_download_size(target: &Path, expected: Option<u64>) -> Result<()> {
    if let Some(expected) = expected
        && fs::metadata(target)?.len() != expected
    {
        return Err(BExplorerError::Operation(
            "The downloaded file size differs from the device metadata; the source was preserved"
                .into(),
        ));
    }
    Ok(())
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

pub(crate) fn unique_local_destination(base: &Path, is_dir: bool, reserved: &[PathBuf]) -> PathBuf {
    if !base.exists() && !reserved.iter().any(|path| path == base) {
        return base.to_path_buf();
    }

    let parent = base.parent().unwrap_or_else(|| Path::new(""));
    let stem = base
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("Copy");
    let extension = base.extension().and_then(|value| value.to_str());

    for index in 1_u64.. {
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

    unreachable!("every generated destination name was occupied")
}

#[cfg(test)]
mod totals_tests {
    use super::*;
    use crate::platform::PortableObjectInfo;

    fn info(id: &str, size: Option<u64>) -> PortableObjectInfo {
        PortableObjectInfo {
            id: id.into(),
            name: id.into(),
            is_folder: size.is_none(),
            size,
        }
    }

    fn exported(id: &str, size: Option<u64>) -> ExportedObject {
        ExportedObject {
            object: PortableObjectRef {
                device_id: "test-device".into(),
                object_id: id.into(),
            },
            name: id.into(),
            is_folder: size.is_none(),
            size,
        }
    }

    #[test]
    fn move_inventory_never_adds_a_new_photo_to_its_deletion_plan() {
        use std::cell::RefCell;
        let inventory = vec![exported("old-photo", Some(4)), exported("album", None)];
        for arrived_before_validation in [true, false] {
            let children = RefCell::new(vec!["old-photo".to_string()]);
            if arrived_before_validation {
                children.borrow_mut().push("new-photo".into());
            }
            let removed = RefCell::new(Vec::new());
            let result = remove_export_inventory(
                &inventory,
                |entry| {
                    Ok((
                        info(&entry.object.object_id, entry.size),
                        if entry.is_folder {
                            children.borrow().clone()
                        } else {
                            Vec::new()
                        },
                    ))
                },
                |entry| {
                    removed.borrow_mut().push(entry.object.object_id.clone());
                    assert_eq!(entry.object.object_id, "old-photo");
                    *children.borrow_mut() = vec!["new-photo".into()];
                    Ok(())
                },
                || Ok(()),
            );
            assert!(result.is_err());
            assert!(children.borrow().iter().any(|id| id == "new-photo"));
            assert!(
                !removed
                    .borrow()
                    .iter()
                    .any(|id| id == "new-photo" || id == "album")
            );
            if arrived_before_validation {
                assert!(removed.borrow().is_empty());
            }
        }
    }

    #[test]
    fn exported_device_roots_and_changed_files_are_preserved() {
        let root = [exported("DEVICE", None)];
        assert!(
            validate_export_inventory(&root, |_| panic!(
                "device root must be rejected before inspection"
            ))
            .is_err()
        );
        let inventory = [exported("photo", Some(4))];
        assert!(
            remove_export_inventory(
                &inventory,
                |_| Ok((info("photo", Some(5)), Vec::new())),
                |_| panic!("changed file must not be deleted"),
                || Ok(())
            )
            .is_err()
        );
    }

    #[test]
    fn portable_root_names_always_stay_in_the_destination_directory() {
        let parent = Path::new("destination");
        for name in ["../outside", "/absolute", "C:\\absolute", "..", ".", "a:b"] {
            let safe = safe_child_name(name);
            assert_eq!(Path::new(&safe).components().count(), 1);
            assert!(matches!(
                Path::new(&safe).components().next(),
                Some(std::path::Component::Normal(_))
            ));
            assert_eq!(parent.join(safe).parent(), Some(parent));
        }
    }

    #[test]
    fn device_totals_use_list_metadata_once_and_count_large_files() {
        let mut listed = Vec::new();
        let total = collect_object_totals(
            "root".into(),
            info("root", None),
            |id| {
                listed.push(id.to_string());
                Ok(match id {
                    "root" => vec![info("nested", None), info("empty", Some(0))],
                    "nested" => vec![info("large", Some(u32::MAX as u64 + 1024))],
                    _ => panic!("unexpected metadata request"),
                })
            },
            || Ok(()),
        )
        .unwrap();
        assert_eq!(total, (u32::MAX as u64 + 1024, 2));
        assert_eq!(listed, ["root", "nested"]);
    }

    #[test]
    fn device_count_rejects_cycles_and_stops_on_cancellation() {
        assert!(
            collect_object_totals(
                "root".into(),
                info("root", None),
                |_| Ok(vec![info("root", None)]),
                || Ok(())
            )
            .is_err()
        );
        let mut listed = false;
        assert!(
            collect_object_totals(
                "root".into(),
                info("root", None),
                |_| {
                    listed = true;
                    Ok(Vec::new())
                },
                || Err(BExplorerError::Operation("cancelled".into()))
            )
            .is_err()
        );
        assert!(!listed);
    }

    #[test]
    fn sanitized_device_names_never_overwrite_an_exported_sibling() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "bexplorer-portable-export-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let first = export_child_destination(&root, "a:b.txt", false);
        fs::write(&first, b"first").unwrap();
        let second = export_child_destination(&root, "a_b.txt", false);
        assert_ne!(first, second);
        fs::write(&second, b"other").unwrap();
        assert_eq!(fs::read(first).unwrap(), b"first");
        assert!(verify_download_size(&second, Some(6)).is_err());
        verify_download_size(&second, Some(5)).unwrap();
        fs::remove_file(second).unwrap();
        fs::remove_file(root.join("a_b.txt")).unwrap();
        fs::remove_dir(root).unwrap();
    }
}
