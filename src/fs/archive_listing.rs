use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::utils::errors::Result;

use super::archive::{list_7z_entries, list_zip_entries};
use super::explorer::{EntryKind, FileCategory, FileEntry};

const BROWSABLE_ARCHIVE_EXTENSIONS: &[&str] = &[
    "7z", "apk", "apm", "ar", "arj", "bz2", "bzip2", "cab", "chm", "cpio", "cramfs", "gz", "gzip",
    "ihex", "lha", "lzh", "lzma", "msi", "nsis", "rar", "squashfs", "swm", "tar", "taz", "tbz",
    "tbz2", "tgz", "txz", "wim", "xar", "xz", "z", "zip", "zipx", "zst",
];

const EXTRACTABLE_7Z_EXTENSIONS: &[&str] = &[
    "001", "apfs", "deb", "dmg", "esd", "fat", "hfs", "hfsx", "img", "iso", "mbr", "ntfs", "qcow",
    "qcow2", "rpm", "udf", "vdi", "vhd", "vhdx", "vmdk",
];

/// Check if a path is inside a browsable archive (virtual path).
/// Returns `true` if `path` points to an entry below a supported archive file.
pub fn is_inside_archive(path: &Path) -> bool {
    resolve_archive(path).is_some_and(|(_, internal_path)| !internal_path.as_os_str().is_empty())
}

/// Returns `true` for either an archive root or a virtual path inside one.
pub fn is_archive_navigation_path(path: &Path) -> bool {
    resolve_archive(path).is_some()
}

/// Walk ancestors of `path` to find the first file ancestor that is a
/// browsable archive.  Returns `(archive_path, internal_path)`
/// where `internal_path` is the portion of `path` after the archive file.
///
/// Example:
///   `resolve_archive("C:\\arc.zip\\sub\\file.txt")`
///   => `Some(("C:\\arc.zip", "sub\\file.txt"))`
pub fn resolve_archive(path: &Path) -> Option<(PathBuf, PathBuf)> {
    let mut current = Some(path);
    let mut tail = PathBuf::new();

    while let Some(p) = current {
        p.file_name()?;
        if is_browsable_extension(p) && p.is_file() {
            return Some((p.to_path_buf(), tail));
        }
        let name = p.file_name().unwrap_or_default();
        tail = if tail.as_os_str().is_empty() {
            PathBuf::from(name)
        } else {
            let mut new_tail = PathBuf::from(name);
            new_tail.push(&tail);
            new_tail
        };
        current = p.parent();
    }
    None
}

/// Check if a path refers to a browsable archive file that can be opened by
/// navigating into it. Only real (not virtual) archives are browsable.
pub fn is_browsable_archive(path: &Path) -> bool {
    path.is_file() && is_browsable_extension(path)
}

pub fn has_browsable_archive_extension(path: &Path) -> bool {
    is_browsable_extension(path)
}

pub fn has_extractable_archive_extension(path: &Path) -> bool {
    is_extractable_extension(path)
}

fn is_browsable_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            BROWSABLE_ARCHIVE_EXTENSIONS
                .iter()
                .any(|candidate| ext.eq_ignore_ascii_case(candidate))
        })
}

fn is_extractable_extension(path: &Path) -> bool {
    is_browsable_extension(path)
        || path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| {
                EXTRACTABLE_7Z_EXTENSIONS
                    .iter()
                    .any(|candidate| ext.eq_ignore_ascii_case(candidate))
                    || is_rar_part_extension(ext)
            })
}

fn is_rar_part_extension(ext: &str) -> bool {
    let bytes = ext.as_bytes();
    bytes.len() == 3
        && matches!(bytes[0], b'r' | b'R')
        && bytes[1].is_ascii_digit()
        && bytes[2].is_ascii_digit()
}

/// List the contents of an archive as virtual `FileEntry` values.
/// The `path` must be inside a browsable archive (as determined by
/// `resolve_archive`).
pub fn list_archive_contents(path: &Path) -> Result<Vec<FileEntry>> {
    let Some((archive_path, internal_path)) = resolve_archive(path) else {
        return Err(crate::utils::errors::BExplorerError::Operation(format!(
            "Not inside a browsable archive: {}",
            path.display()
        )));
    };

    let all_entries = if should_use_native_zip_listing(&archive_path) {
        list_zip_entries(&archive_path)?
    } else {
        list_7z_entries(&archive_path)?
    };

    let prefix = normalize_prefix(internal_path.as_os_str().to_str().unwrap_or(""));
    let parent_path = path.to_path_buf();

    let mut entries: Vec<FileEntry> = Vec::new();
    let mut seen_names: BTreeSet<String> = BTreeSet::new();

    for entry in &all_entries {
        let relative = strip_prefix(&entry.name, &prefix);
        if relative.is_none() {
            continue;
        }
        let relative = relative.unwrap();
        if relative.is_empty() {
            continue;
        }

        // only direct children (one level deep)
        let child_name: String = if let Some(slash) = relative.find('/') {
            relative[..slash].to_string()
        } else {
            relative.clone()
        };

        if child_name.is_empty() || !seen_names.insert(child_name.clone()) {
            continue;
        }

        let is_dir = entry.is_dir || relative.contains('/');
        let child_path = parent_path.join(&child_name);

        let kind = if is_dir {
            EntryKind::Folder
        } else {
            EntryKind::File
        };

        let modified = entry.modified.map(|t| {
            let datetime: chrono::DateTime<chrono::Local> = t.into();
            datetime.format("%Y-%m-%d %H:%M").to_string()
        });

        entries.push(FileEntry {
            name: child_name.to_string(),
            path: child_path,
            kind,
            category: FileCategory::Other,
            drive_kind: None,
            file_system: String::new(),
            free_space: None,
            size: if is_dir { None } else { entry.size },
            percent_full: None,
            modified,
            created: None,
            is_hidden: false,
        });
    }

    crate::fs::explorer::sort_entries_by_name(&mut entries);
    Ok(entries)
}

fn normalize_prefix(s: &str) -> String {
    let s = s.replace('\\', "/");
    if !s.is_empty() && !s.ends_with('/') {
        format!("{s}/")
    } else {
        s
    }
}

fn strip_prefix(path: &str, prefix: &str) -> Option<String> {
    let p = path.replace('\\', "/");
    if p == prefix.trim_end_matches('/') {
        return Some(String::new());
    }
    if !p.starts_with(prefix) {
        return None;
    }
    Some(p[prefix.len()..].to_string())
}

fn should_use_native_zip_listing(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_common_7zip_browsable_formats() {
        for name in [
            "payload.zip",
            "payload.7z",
            "payload.rar",
            "payload.tar",
            "payload.tar.gz",
            "payload.zipx",
            "payload.wim",
            "payload.zst",
        ] {
            assert!(
                has_browsable_archive_extension(Path::new(name)),
                "{name} should be recognized as browsable"
            );
        }
    }

    #[test]
    fn leaves_disk_images_to_their_apps() {
        for name in [
            "payload.iso",
            "payload.img",
            "payload.dmg",
            "payload.vhd",
            "payload.vhdx",
            "payload.vmdk",
            "payload.qcow",
            "payload.qcow2",
            "payload.udf",
            "payload.ntfs",
            "payload.fat",
            "payload.hfs",
            "payload.mbr",
        ] {
            assert!(
                !has_browsable_archive_extension(Path::new(name)),
                "{name} should open through the system app instead of the archive browser"
            );
            assert!(
                has_extractable_archive_extension(Path::new(name)),
                "{name} should still offer extraction through 7z"
            );
        }
    }

    #[test]
    fn recognizes_extra_7zip_extractable_formats() {
        for name in [
            "payload.iso",
            "payload.vdi",
            "payload.vhdx",
            "payload.7z.001",
            "payload.r00",
        ] {
            assert!(
                has_extractable_archive_extension(Path::new(name)),
                "{name} should be recognized as extractable"
            );
        }
    }

    #[test]
    fn leaves_document_zip_containers_to_their_apps() {
        for name in ["document.docx", "spreadsheet.xlsx", "slides.pptx"] {
            assert!(
                !has_browsable_archive_extension(Path::new(name)),
                "{name} should not open as an archive"
            );
        }
    }

    #[test]
    fn leaves_linux_packages_to_their_apps() {
        for name in ["package.deb", "package.rpm"] {
            assert!(
                !has_browsable_archive_extension(Path::new(name)),
                "{name} should open through the package installer instead of the archive browser"
            );
            assert!(
                has_extractable_archive_extension(Path::new(name)),
                "{name} should still offer extraction through 7z"
            );
        }
    }

    #[test]
    fn archive_root_is_browsable_but_not_inside_itself() {
        let root =
            std::env::temp_dir().join(format!("bexplorer-archive-listing-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("create temp dir");
        let archive = root.join("sample.rar");
        std::fs::write(&archive, []).expect("create archive placeholder");

        assert!(is_browsable_archive(&archive));
        assert!(is_archive_navigation_path(&archive));
        assert!(!is_inside_archive(&archive));

        let virtual_child = archive.join("folder").join("file.txt");
        assert!(!is_browsable_archive(&virtual_child));
        assert!(is_archive_navigation_path(&virtual_child));
        assert!(is_inside_archive(&virtual_child));

        std::fs::remove_dir_all(root).expect("cleanup temp dir");
    }
}
