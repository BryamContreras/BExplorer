use std::collections::HashSet;
#[cfg(target_os = "linux")]
use std::path::Path;
use std::path::PathBuf;

use crate::fs::explorer::{self, EntryKind, FileEntry};
use crate::utils::errors::{BExplorerError, Result};

#[derive(Clone, Debug)]
pub struct TrashMutationOutcome {
    pub count: usize,
    pub original_paths: Vec<PathBuf>,
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
pub fn list_entries() -> Result<Vec<FileEntry>> {
    let items = native_items()?;
    Ok(items.into_iter().map(trash_item_entry).collect())
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
pub fn list_entries() -> Result<Vec<FileEntry>> {
    Err(BExplorerError::Operation(
        "La papelera aún no está disponible en este sistema".into(),
    ))
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
pub fn restore_items(paths: &[PathBuf]) -> Result<TrashMutationOutcome> {
    let items = selected_items(paths)?;
    let original_paths = items
        .iter()
        .map(::trash::TrashItem::original_path)
        .collect::<Vec<_>>();
    let count = items.len();
    ::trash::os_limited::restore_all(items).map_err(|error| {
        BExplorerError::Operation(format!("No se pudo restaurar desde la papelera: {error}"))
    })?;
    Ok(TrashMutationOutcome {
        count,
        original_paths,
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
pub fn restore_items(_paths: &[PathBuf]) -> Result<TrashMutationOutcome> {
    Err(BExplorerError::Operation(
        "Restaurar desde la papelera aún no está disponible en este sistema".into(),
    ))
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
pub fn purge_items(paths: &[PathBuf]) -> Result<TrashMutationOutcome> {
    let items = selected_items(paths)?;
    purge_resolved_items(items)
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
pub fn purge_items(_paths: &[PathBuf]) -> Result<TrashMutationOutcome> {
    Err(BExplorerError::Operation(
        "Eliminar desde la papelera aún no está disponible en este sistema".into(),
    ))
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
pub fn empty() -> Result<TrashMutationOutcome> {
    let items = native_items()?;
    purge_resolved_items(items)
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
pub fn empty() -> Result<TrashMutationOutcome> {
    Err(BExplorerError::Operation(
        "Vaciar la papelera aún no está disponible en este sistema".into(),
    ))
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
fn trash_item_entry(item: ::trash::TrashItem) -> FileEntry {
    let metadata = ::trash::os_limited::metadata(&item).ok();
    let (kind, size) = match metadata.map(|metadata| metadata.size) {
        Some(::trash::TrashItemSize::Entries(_)) => (EntryKind::Folder, None),
        Some(::trash::TrashItemSize::Bytes(size)) => (EntryKind::File, Some(size)),
        None => (EntryKind::Other, None),
    };
    let name = item.name.to_string_lossy().into_owned();
    let original_path = item.original_path();
    let deleted_at =
        chrono::DateTime::<chrono::Utc>::from_timestamp(item.time_deleted, 0).map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        });

    FileEntry {
        name,
        path: explorer::trash_item_path(&item.id),
        kind,
        category: explorer::classify_file_category(&original_path),
        drive_kind: None,
        file_system: String::new(),
        free_space: None,
        size,
        percent_full: None,
        // In the Recycle Bin these two detail columns intentionally represent
        // the original location and deletion date.
        modified: Some(item.original_parent.to_string_lossy().into_owned()),
        created: deleted_at,
        is_hidden: false,
    }
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
fn selected_items(paths: &[PathBuf]) -> Result<Vec<::trash::TrashItem>> {
    if paths.is_empty() {
        return Err(BExplorerError::Operation(
            "No hay elementos seleccionados en la papelera".into(),
        ));
    }
    if paths.iter().any(|path| !explorer::is_trash_item_path(path)) {
        return Err(BExplorerError::Operation(
            "La selección contiene elementos que no pertenecen a la papelera".into(),
        ));
    }

    let requested = paths.iter().cloned().collect::<HashSet<_>>();
    let selected = native_items()?
        .into_iter()
        .filter(|item| requested.contains(&explorer::trash_item_path(&item.id)))
        .collect::<Vec<_>>();

    if selected.len() != requested.len() {
        return Err(BExplorerError::Operation(
            "Algunos elementos ya no están disponibles en la papelera".into(),
        ));
    }
    Ok(selected)
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
fn purge_resolved_items(items: Vec<::trash::TrashItem>) -> Result<TrashMutationOutcome> {
    let original_paths = items
        .iter()
        .map(::trash::TrashItem::original_path)
        .collect::<Vec<_>>();
    let count = items.len();
    ::trash::os_limited::purge_all(items.iter()).map_err(|error| {
        BExplorerError::Operation(format!(
            "No se pudieron eliminar los elementos de la papelera: {error}"
        ))
    })?;
    Ok(TrashMutationOutcome {
        count,
        original_paths,
    })
}

#[cfg(target_os = "windows")]
pub(crate) fn native_items() -> Result<Vec<::trash::TrashItem>> {
    ::trash::os_limited::list()
        .map_err(|error| BExplorerError::Operation(format!("No se pudo leer la papelera: {error}")))
}

#[cfg(all(
    unix,
    not(target_os = "linux"),
    not(target_os = "macos"),
    not(target_os = "ios"),
    not(target_os = "android")
))]
pub(crate) fn native_items() -> Result<Vec<::trash::TrashItem>> {
    ::trash::os_limited::list()
        .map_err(|error| BExplorerError::Operation(format!("No se pudo leer la papelera: {error}")))
}

/// The upstream FreeDesktop implementation probes every mount point while
/// listing the trash. Desktop FUSE mounts such as GVFS can block indefinitely
/// during that probe. Enumerate the standard trash folders directly and only
/// inspect physical filesystem mount points; the resulting `TrashItem` values
/// retain the exact native IDs expected by `trash` for restore and purge.
#[cfg(target_os = "linux")]
pub(crate) fn native_items() -> Result<Vec<::trash::TrashItem>> {
    let mut items = Vec::new();
    for (folder, top_directory) in freedesktop_trash_folders() {
        items.extend(list_freedesktop_folder(&folder, &top_directory));
    }
    Ok(items)
}

#[cfg(target_os = "linux")]
fn freedesktop_trash_folders() -> Vec<(PathBuf, PathBuf)> {
    use std::os::unix::fs::MetadataExt;

    let mut folders = Vec::new();
    let mut seen = HashSet::new();
    let base_dirs = directories::BaseDirs::new();
    if let Some(base_dirs) = &base_dirs {
        let home_trash = base_dirs.data_dir().join("Trash");
        seen.insert(home_trash.clone());
        folders.push((home_trash, PathBuf::from("/")));
    }

    let uid = base_dirs
        .as_ref()
        .and_then(|base_dirs| std::fs::metadata(base_dirs.home_dir()).ok())
        .map(|metadata| metadata.uid())
        .unwrap_or(0);
    let Ok(mountinfo) = std::fs::read_to_string("/proc/self/mountinfo") else {
        return folders;
    };
    for line in mountinfo.lines() {
        let Some((mount_fields, filesystem_fields)) = line.split_once(" - ") else {
            continue;
        };
        let Some(mount_field) = mount_fields.split_whitespace().nth(4) else {
            continue;
        };
        let mut filesystem_fields = filesystem_fields.split_whitespace();
        let (Some(file_system), Some(source)) =
            (filesystem_fields.next(), filesystem_fields.next())
        else {
            continue;
        };
        if !linux_mount_can_host_trash(file_system, source) {
            continue;
        }

        let mount_point = decode_mountinfo_path(mount_field);
        for folder in [
            mount_point.join(".Trash").join(uid.to_string()),
            mount_point.join(format!(".Trash-{uid}")),
        ] {
            if seen.insert(folder.clone()) {
                folders.push((folder, mount_point.clone()));
            }
        }
    }
    folders
}

#[cfg(target_os = "linux")]
fn linux_mount_can_host_trash(file_system: &str, source: &str) -> bool {
    source.starts_with("/dev/")
        || matches!(
            file_system,
            "bcachefs"
                | "btrfs"
                | "ext2"
                | "ext3"
                | "ext4"
                | "exfat"
                | "f2fs"
                | "fuseblk"
                | "jfs"
                | "ntfs"
                | "ntfs3"
                | "reiserfs"
                | "vfat"
                | "xfs"
                | "zfs"
        )
}

#[cfg(target_os = "linux")]
fn list_freedesktop_folder(folder: &Path, top_directory: &Path) -> Vec<::trash::TrashItem> {
    let info_folder = folder.join("info");
    let files_folder = folder.join("files");
    let Ok(entries) = std::fs::read_dir(info_folder) else {
        return Vec::new();
    };

    entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let info_path = entry.path();
            let is_trash_info = entry.file_type().ok()?.is_file()
                && info_path
                    .extension()
                    .is_some_and(|extension| extension == "trashinfo");
            if !is_trash_info {
                return None;
            }
            let trashed_path = files_folder.join(info_path.file_stem()?);
            if std::fs::symlink_metadata(&trashed_path).is_err() {
                return None;
            }
            parse_trash_info(&info_path, top_directory)
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn parse_trash_info(info_path: &Path, top_directory: &Path) -> Option<::trash::TrashItem> {
    use chrono::TimeZone;

    let contents = std::fs::read_to_string(info_path).ok()?;
    let mut original_path = None;
    let mut time_deleted = -1;
    for line in contents.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "Path" => {
                let decoded = decode_percent_path(value.trim());
                original_path = Some(if decoded.is_absolute() {
                    decoded
                } else {
                    top_directory.join(decoded)
                });
            }
            "DeletionDate" => {
                let parsed =
                    chrono::NaiveDateTime::parse_from_str(value.trim(), "%Y-%m-%dT%H:%M:%S")
                        .ok()?;
                time_deleted = chrono::Local
                    .from_local_datetime(&parsed)
                    .earliest()?
                    .timestamp();
            }
            _ => {}
        }
    }

    let original_path = original_path?;
    Some(::trash::TrashItem {
        id: info_path.as_os_str().to_owned(),
        name: original_path.file_name()?.to_owned(),
        original_parent: original_path.parent()?.to_owned(),
        time_deleted,
    })
}

#[cfg(target_os = "linux")]
fn decode_percent_path(value: &str) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;

    let value = value.strip_prefix("file://").unwrap_or(value);
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
        {
            decoded.push((high << 4) | low);
            index += 3;
            continue;
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    PathBuf::from(std::ffi::OsString::from_vec(decoded))
}

#[cfg(target_os = "linux")]
fn decode_mountinfo_path(value: &str) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;

    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\'
            && index + 3 < bytes.len()
            && bytes[index + 1..index + 4]
                .iter()
                .all(|byte| matches!(byte, b'0'..=b'7'))
        {
            let octal = (bytes[index + 1] - b'0') * 64
                + (bytes[index + 2] - b'0') * 8
                + (bytes[index + 3] - b'0');
            decoded.push(octal);
            index += 4;
            continue;
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    PathBuf::from(std::ffi::OsString::from_vec(decoded))
}

#[cfg(target_os = "linux")]
fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_trash() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "bexplorer-trash-test-{}-{unique}",
            std::process::id()
        ))
    }

    #[test]
    fn parses_freedesktop_items_without_probing_virtual_mounts() {
        let root = temporary_trash();
        let trash_folder = root.join("Trash");
        std::fs::create_dir_all(trash_folder.join("info")).expect("create trash info");
        std::fs::create_dir_all(trash_folder.join("files")).expect("create trash files");
        std::fs::write(trash_folder.join("files").join("photo.jpg"), b"image")
            .expect("create trashed file");
        std::fs::write(
            trash_folder.join("info").join("photo.jpg.trashinfo"),
            "[Trash Info]\nPath=/home/example/Pictures/My%20Photo.jpg\nDeletionDate=2026-07-26T17:00:00\n",
        )
        .expect("create trash info file");

        let items = list_freedesktop_folder(&trash_folder, Path::new("/"));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "My Photo.jpg");
        assert_eq!(
            items[0].original_parent,
            PathBuf::from("/home/example/Pictures")
        );
        assert!(Path::new(&items[0].id).ends_with("Trash/info/photo.jpg.trashinfo"));
        assert!(items[0].time_deleted > 0);

        std::fs::remove_dir_all(root).expect("remove temporary trash");
    }

    #[test]
    fn resolves_relative_mounted_trash_paths_against_the_mount_root() {
        let root = temporary_trash();
        let trash_folder = root.join("Trash");
        std::fs::create_dir_all(trash_folder.join("info")).expect("create trash info");
        std::fs::create_dir_all(trash_folder.join("files").join("folder"))
            .expect("create trashed folder");
        std::fs::write(
            trash_folder.join("info").join("folder.trashinfo"),
            "[Trash Info]\nPath=Documents/Old%20Folder\nDeletionDate=2026-07-26T17:00:00\n",
        )
        .expect("create trash info file");

        let items = list_freedesktop_folder(&trash_folder, Path::new("/media/example-drive"));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "Old Folder");
        assert_eq!(
            items[0].original_parent,
            PathBuf::from("/media/example-drive/Documents")
        );

        std::fs::remove_dir_all(root).expect("remove temporary trash");
    }

    #[test]
    fn excludes_desktop_fuse_mounts_from_trash_discovery() {
        assert!(!linux_mount_can_host_trash("fuse.gvfsd-fuse", "gvfsd-fuse"));
        assert!(!linux_mount_can_host_trash("fuse.portal", "portal"));
        assert!(linux_mount_can_host_trash("ext4", "/dev/sdb1"));
        assert!(linux_mount_can_host_trash("zfs", "tank/home"));
        assert_eq!(
            decode_mountinfo_path("/media/My\\040Drive"),
            PathBuf::from("/media/My Drive")
        );
    }
}
