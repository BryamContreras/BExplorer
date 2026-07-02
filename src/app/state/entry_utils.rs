use std::path::{Path, PathBuf};

use crate::app::state::{FileSort, PendingTransferConflict};
use crate::fs::explorer::{self, EntryKind, FileEntry};
use crate::fs::portable;
use crate::fs::transfer_queue::{TransferJob, TransferKind};

pub(super) fn sort_file_entries(entries: &mut [FileEntry], sort: FileSort, ascending: bool) {
    match sort {
        FileSort::Name => {
            entries.sort_by_cached_key(|entry| {
                (
                    if entry.kind.is_container() {
                        0_u8
                    } else {
                        1_u8
                    },
                    entry.name.to_lowercase(),
                )
            });
        }
        FileSort::Type => {
            entries.sort_by_cached_key(|entry| {
                (
                    if entry.kind.is_container() {
                        0_u8
                    } else {
                        1_u8
                    },
                    entry.type_label(),
                )
            });
        }
        FileSort::Size => {
            entries.sort_by_cached_key(|entry| {
                (
                    if entry.kind.is_container() {
                        0_u8
                    } else {
                        1_u8
                    },
                    entry.size.unwrap_or(0),
                )
            });
        }
        FileSort::Modified => {
            entries.sort_by_cached_key(|entry| {
                (
                    if entry.kind.is_container() {
                        0_u8
                    } else {
                        1_u8
                    },
                    entry.modified.clone(),
                )
            });
        }
    }
    if !ascending {
        reverse_sorted_entry_groups(entries);
    }
}

pub(super) fn reverse_sorted_entry_groups(entries: &mut [FileEntry]) {
    let split = entries.partition_point(|entry| entry.kind.is_container());
    entries[..split].reverse();
    entries[split..].reverse();
}

pub(super) fn visible_entry_index(entries: &[FileEntry], path: &Path) -> Option<usize> {
    entries
        .iter()
        .position(|entry| entry.path.as_path() == path)
}

pub(super) fn normalized_type_select_char(character: char) -> Option<char> {
    if !character.is_alphabetic() {
        return None;
    }
    character.to_lowercase().next()
}

pub(super) fn entry_starts_with(entry: &FileEntry, character: char) -> bool {
    entry
        .name
        .chars()
        .find(|value| !value.is_whitespace())
        .and_then(|value| value.to_lowercase().next())
        == Some(character)
}

pub(super) fn initial_rename_selection_end(path: &Path, file_name: &str) -> usize {
    let full_len = file_name.chars().count();
    if full_len == 0 || path.is_dir() {
        return full_len;
    }

    let name_path = Path::new(file_name);
    let has_extension = name_path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| !extension.is_empty());
    if !has_extension {
        return full_len;
    }

    name_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .map(|stem| stem.chars().count())
        .unwrap_or(full_len)
}

pub(super) fn editable_entry_name(entry: &FileEntry) -> String {
    if entry.kind == EntryKind::Drive {
        return editable_drive_label(&entry.name);
    }

    entry
        .path
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| entry.name.clone())
}

pub(super) fn editable_drive_label(name: &str) -> String {
    let trimmed = name.trim();
    if let Some(start) = trimmed.rfind(" (") {
        let suffix = &trimmed[start + 2..];
        let mut chars = suffix.chars();
        if chars
            .next()
            .is_some_and(|letter| letter.is_ascii_alphabetic())
            && chars.next() == Some(':')
            && chars.next() == Some(')')
            && chars.next().is_none()
        {
            return trimmed[..start].trim_end().to_string();
        }
    }
    trimmed.to_string()
}

pub(super) fn sync_file_clipboard(paths: &[PathBuf], cut: bool) {
    #[cfg(test)]
    {
        let _ = (paths, cut);
    }

    #[cfg(not(test))]
    {
        if paths.iter().all(|path| path.exists())
            && crate::platform::shell::copy_files(paths, cut).is_ok()
        {
            return;
        }

        let text = paths
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        if let Err(error) = crate::platform::shell::copy_text(&text) {
            crate::utils::log::error(format!("Clipboard sync failed: {error}"));
        }
    }
}

pub(super) fn clear_system_clipboard_after_cut() {
    if let Err(error) = crate::platform::shell::clear_clipboard() {
        crate::utils::log::error(format!("Clipboard clear failed: {error}"));
    }
}

#[cfg_attr(test, allow(dead_code))]
pub(super) fn paths_from_clipboard_text(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(|line| line.trim().trim_matches('"'))
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .collect()
}

pub(super) fn pending_transfer_conflict(
    sources: &[PathBuf],
    destination: &Path,
    kind: TransferKind,
    clear_clipboard_on_confirm: bool,
) -> Option<PendingTransferConflict> {
    if explorer::is_portable_path(destination) || !destination.is_dir() {
        return None;
    }

    let mut conflict_count = 0_usize;
    let mut first_conflict_name = None;
    for source in sources {
        let Some(name) = transfer_source_name(source) else {
            continue;
        };
        if destination.join(&name).exists() {
            conflict_count = conflict_count.saturating_add(1);
            first_conflict_name.get_or_insert(name);
        }
    }

    if conflict_count == 0 {
        return None;
    }

    Some(PendingTransferConflict {
        sources: sources.to_vec(),
        destination: destination.to_path_buf(),
        kind,
        conflict_count,
        first_conflict_name: first_conflict_name.unwrap_or_else(|| "Item".into()),
        clear_clipboard_on_confirm,
    })
}

pub(super) fn transfer_source_name(source: &Path) -> Option<String> {
    if explorer::is_portable_path(source) {
        return Some(portable::path_name(source));
    }

    source
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
}

pub(super) fn transfer_can_retry_elevated(job: &TransferJob) -> bool {
    !explorer::is_virtual_path(&job.destination)
        && !explorer::is_portable_path(&job.destination)
        && job.sources.iter().all(|source| {
            !explorer::is_virtual_path(source)
                && !explorer::is_portable_path(source)
                && !crate::fs::archive_listing::is_inside_archive(source)
        })
}

pub(super) fn transfer_error_needs_elevation(error: &str, job: &TransferJob) -> bool {
    if !transfer_can_retry_elevated(job) {
        return false;
    }

    permission_error_needs_elevation(error)
}

pub(super) fn file_operation_error_needs_elevation(error: &str) -> bool {
    permission_error_needs_elevation(error)
}

pub(super) fn permission_error_needs_elevation(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("access denied")
        || lower.contains("acceso denegado")
        || lower.contains("permission denied")
        || lower.contains("e_accessdenied")
        || lower.contains("0x80070005")
        || lower.contains("0x80004005")
        || lower.contains("os error 5")
        || lower.contains("error 5")
        || lower.contains("operations were aborted")
        || lower.contains("operaciones fueron anuladas")
}

pub(super) fn storage_signature(
    entries: &[FileEntry],
) -> Vec<(PathBuf, String, Option<u64>, Option<u64>)> {
    entries
        .iter()
        .map(|entry| {
            (
                entry.path.clone(),
                entry.name.clone(),
                entry.free_space,
                entry.size,
            )
        })
        .collect()
}

pub(super) fn portable_signature(entries: &[explorer::PortableDevice]) -> Vec<(String, String)> {
    entries
        .iter()
        .map(|entry| (entry.id.clone(), entry.name.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::permission_error_needs_elevation;

    #[test]
    fn permission_errors_cover_windows_shell_abort_from_recycle_bin() {
        assert!(permission_error_needs_elevation(
            "Operation error: Could not move C:\\Windows\\System32\\x.txt to trash: Some operations were aborted"
        ));
    }

    #[test]
    fn permission_errors_cover_common_windows_hresult_access_denied() {
        assert!(permission_error_needs_elevation(
            "Windows API error: Acceso denegado. (0x80070005)"
        ));
        assert!(permission_error_needs_elevation(
            "Operation failed: E_ACCESSDENIED"
        ));
    }
}
