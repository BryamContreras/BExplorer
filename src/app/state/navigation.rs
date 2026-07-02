use std::path::{Path, PathBuf};

use crate::app::session::SplitFocus;
use crate::fs::archive::ArchiveFormat;
use crate::fs::{explorer, portable};

pub(super) fn reindex_after_move(idx: usize, from: usize, to: usize) -> usize {
    match idx {
        i if i == from => to,
        i if from < i && to >= i => i - 1,
        i if from > i && to <= i => i + 1,
        i => i,
    }
}

pub(super) fn reindex_on_close(idx: usize, removed: usize) -> usize {
    match idx {
        i if i == removed => 0, // caller should have collapsed the split already
        i if i > removed => i - 1,
        i => i,
    }
}

pub(super) fn reindex_split_tab_list_after_move(list: &mut [usize], from: usize, to: usize) {
    for index in list {
        *index = reindex_after_move(*index, from, to);
    }
}

pub(super) fn reindex_split_tab_list_on_close(list: &mut Vec<usize>, removed: usize) {
    list.retain(|index| *index != removed);
    for index in list {
        if *index > removed {
            *index -= 1;
        }
    }
}

pub(super) fn normalize_split_tabs(
    mut tabs: Vec<usize>,
    active: usize,
    tab_count: usize,
) -> Vec<usize> {
    tabs.retain(|index| *index < tab_count);
    tabs.sort_unstable();
    tabs.dedup();
    if !tabs.contains(&active) && active < tab_count {
        tabs.push(active);
    }
    tabs
}

pub(super) fn opposite_split_focus(focus: SplitFocus) -> SplitFocus {
    match focus {
        SplitFocus::Primary => SplitFocus::Secondary,
        SplitFocus::Secondary => SplitFocus::Primary,
    }
}

pub(super) fn normalize_existing_path(path: &Path) -> Option<PathBuf> {
    path.canonicalize().ok().or_else(|| {
        if path.exists() {
            Some(path.to_path_buf())
        } else {
            None
        }
    })
}

pub(super) fn archive_item_path(path: &Path) -> bool {
    crate::fs::archive_listing::is_inside_archive(path)
}

pub(super) fn archive_file_name_from_input(input: &str, format: ArchiveFormat) -> String {
    let mut name = input
        .trim()
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => ch,
        })
        .collect::<String>();
    name = name.trim().trim_matches('.').trim().to_string();
    if name.is_empty() {
        name = "Archive".to_string();
    }

    let lower = name.to_ascii_lowercase();
    for extension in ["zip", "7z"] {
        let suffix = format!(".{extension}");
        if lower.ends_with(&suffix) {
            name.truncate(name.len().saturating_sub(suffix.len()));
            name = name.trim_end_matches('.').trim().to_string();
            break;
        }
    }
    if name.is_empty() {
        name = "Archive".to_string();
    }

    format!("{name}.{}", format.extension())
}

pub(super) fn password_from_input(input: String) -> Option<String> {
    if input.is_empty() { None } else { Some(input) }
}

pub(super) fn display_path_name(path: &Path) -> String {
    if explorer::is_portable_path(path) {
        return portable::path_name(path);
    }
    if let Some(name) = explorer::virtual_display_name(path) {
        return name;
    }
    path.file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| path.display().to_string())
}

pub(super) fn default_column_widths() -> [f32; 8] {
    [330.0, 128.0, 142.0, 128.0, 110.0, 200.0, 156.0, 260.0]
}
