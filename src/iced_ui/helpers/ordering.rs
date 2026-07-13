use super::*;
pub(in crate::iced_ui) fn normalize_tabs(
    mut tabs: Vec<usize>,
    fallback: usize,
    len: usize,
) -> Vec<usize> {
    tabs.retain(|index| *index < len);
    if tabs.is_empty() {
        tabs.push(fallback.min(len.saturating_sub(1)));
    }
    tabs
}

pub(in crate::iced_ui) fn rebase_tab_index(index: usize, removed: usize) -> Option<usize> {
    match index.cmp(&removed) {
        std::cmp::Ordering::Less => Some(index),
        std::cmp::Ordering::Equal => None,
        std::cmp::Ordering::Greater => Some(index - 1),
    }
}

pub(in crate::iced_ui) fn rebase_tab_indices(indices: &[usize], removed: usize) -> Vec<usize> {
    indices
        .iter()
        .filter_map(|index| rebase_tab_index(*index, removed))
        .collect()
}

pub(in crate::iced_ui) fn expanded_render_limit(current: usize, total: usize) -> usize {
    current.saturating_add(RENDER_BATCH_SIZE).min(total)
}

pub(in crate::iced_ui) fn compare_entries_for_view(
    left: &FileEntry,
    right: &FileEntry,
    group_mode: GroupMode,
    group_ascending: bool,
    sort_column: TableColumn,
    sort_ascending: bool,
) -> std::cmp::Ordering {
    let container_order = right.kind.is_container().cmp(&left.kind.is_container());
    let group_order = compare_entries_by_group(left, right, group_mode);
    let group_order = if group_ascending {
        group_order
    } else {
        group_order.reverse()
    };

    let primary_order = if group_mode == GroupMode::None {
        container_order
    } else {
        group_order
    };
    let secondary_order = if group_mode == GroupMode::None {
        std::cmp::Ordering::Equal
    } else {
        container_order
    };

    primary_order
        .then(secondary_order)
        .then_with(|| {
            let order = compare_entries_by_column(left, right, sort_column);
            if sort_ascending {
                order
            } else {
                order.reverse()
            }
        })
        .then_with(|| explorer::compare_names_case_insensitive(&left.name, &right.name))
}

pub(in crate::iced_ui) fn compare_entries_by_group(
    left: &FileEntry,
    right: &FileEntry,
    group_mode: GroupMode,
) -> std::cmp::Ordering {
    match group_mode {
        GroupMode::None => std::cmp::Ordering::Equal,
        GroupMode::Name => group_name_bucket(&left.name).cmp(&group_name_bucket(&right.name)),
        GroupMode::Type => left
            .type_label()
            .to_lowercase()
            .cmp(&right.type_label().to_lowercase()),
        GroupMode::TotalSize => compare_optional_u64(left.size, right.size),
        GroupMode::FreeSpace => compare_optional_u64(left.free_space, right.free_space),
    }
}

pub(in crate::iced_ui) fn compare_entries_by_column(
    left: &FileEntry,
    right: &FileEntry,
    column: TableColumn,
) -> std::cmp::Ordering {
    match column {
        TableColumn::Name => explorer::compare_names_case_insensitive(&left.name, &right.name),
        TableColumn::Type => left
            .type_label()
            .to_lowercase()
            .cmp(&right.type_label().to_lowercase()),
        TableColumn::Size => compare_optional_u64(left.size, right.size),
        TableColumn::Modified => {
            compare_optional_string(left.modified.as_deref(), right.modified.as_deref())
        }
        TableColumn::Created => {
            compare_optional_string(left.created.as_deref(), right.created.as_deref())
        }
    }
}

pub(in crate::iced_ui) fn compare_optional_u64(
    left: Option<u64>,
    right: Option<u64>,
) -> std::cmp::Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

pub(in crate::iced_ui) fn compare_optional_string(
    left: Option<&str>,
    right: Option<&str>,
) -> std::cmp::Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.cmp(right),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

pub(in crate::iced_ui) fn entry_group_label(entry: &FileEntry, mode: GroupMode) -> String {
    match mode {
        GroupMode::None => String::new(),
        GroupMode::Name => group_name_bucket(&entry.name),
        GroupMode::Type => entry.type_label(),
        GroupMode::TotalSize => size_group_label(entry.size),
        GroupMode::FreeSpace => size_group_label(entry.free_space),
    }
}

pub(in crate::iced_ui) fn group_name_bucket(name: &str) -> String {
    name.chars()
        .next()
        .map(|ch| ch.to_uppercase().collect::<String>())
        .unwrap_or_else(|| "#".into())
}

pub(in crate::iced_ui) fn size_group_label(size: Option<u64>) -> String {
    let Some(size) = size else {
        return "Sin tamano".into();
    };
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if size < MB {
        "Menos de 1 MB".into()
    } else if size < 100 * MB {
        "1 MB - 100 MB".into()
    } else if size < GB {
        "100 MB - 1 GB".into()
    } else if size < 100 * GB {
        "1 GB - 100 GB".into()
    } else {
        "Mas de 100 GB".into()
    }
}

pub(in crate::iced_ui) fn visual_view_metrics(mode: ViewMode) -> VisualViewMetrics {
    match mode {
        ViewMode::Tiles => VisualViewMetrics {
            cell_width: 246.0,
            cell_height: 76.0,
            icon_size: 46.0,
            preview_height: 46.0,
            spacing: 7.0,
            grid_padding: 12.0,
            tile: true,
        },
        ViewMode::SmallIcons | ViewMode::List => VisualViewMetrics {
            cell_width: 140.0,
            cell_height: 72.0,
            icon_size: 32.0,
            preview_height: 38.0,
            spacing: 8.0,
            grid_padding: 14.0,
            tile: false,
        },
        ViewMode::MediumIcons => VisualViewMetrics {
            cell_width: 170.0,
            cell_height: 112.0,
            icon_size: 58.0,
            preview_height: 70.0,
            spacing: 10.0,
            grid_padding: 14.0,
            tile: false,
        },
        ViewMode::LargeIcons => VisualViewMetrics {
            cell_width: 230.0,
            cell_height: 160.0,
            icon_size: 96.0,
            preview_height: 112.0,
            spacing: 12.0,
            grid_padding: 14.0,
            tile: false,
        },
        ViewMode::ExtraLargeIcons => VisualViewMetrics {
            cell_width: 330.0,
            cell_height: 236.0,
            icon_size: 160.0,
            preview_height: 184.0,
            spacing: 14.0,
            grid_padding: 14.0,
            tile: false,
        },
        ViewMode::Details => VisualViewMetrics {
            cell_width: 180.0,
            cell_height: DETAIL_ROW_HEIGHT,
            icon_size: 18.0,
            preview_height: 18.0,
            spacing: 0.0,
            grid_padding: 0.0,
            tile: false,
        },
    }
}

pub(in crate::iced_ui) fn visual_min_cell_width(mode: ViewMode) -> f32 {
    match mode {
        ViewMode::Tiles => 220.0,
        ViewMode::SmallIcons | ViewMode::List => 112.0,
        ViewMode::MediumIcons => 136.0,
        ViewMode::LargeIcons => 198.0,
        ViewMode::ExtraLargeIcons => 270.0,
        ViewMode::Details => 180.0,
    }
}

pub(in crate::iced_ui) fn visual_label_height(font_size: f32) -> f32 {
    (font_size * 2.55).ceil()
}

pub(in crate::iced_ui) fn view_menu_modes() -> [ViewMode; 6] {
    [
        ViewMode::Details,
        ViewMode::Tiles,
        ViewMode::SmallIcons,
        ViewMode::MediumIcons,
        ViewMode::LargeIcons,
        ViewMode::ExtraLargeIcons,
    ]
}

pub(in crate::iced_ui) fn adjacent_view_mode(mode: ViewMode, larger: bool) -> ViewMode {
    let modes = view_menu_modes();
    let index = modes
        .iter()
        .position(|candidate| *candidate == mode)
        .unwrap_or(0);
    let next = if larger {
        (index + 1).min(modes.len() - 1)
    } else {
        index.saturating_sub(1)
    };
    modes[next]
}

pub(in crate::iced_ui) fn view_mode_label(mode: ViewMode) -> &'static str {
    match mode {
        ViewMode::Details => "Detalles",
        ViewMode::Tiles => "Mosaicos",
        ViewMode::SmallIcons => "Iconos Pequenos",
        ViewMode::MediumIcons => "Iconos Medianos",
        ViewMode::LargeIcons => "Iconos Grandes",
        ViewMode::ExtraLargeIcons => "Iconos Muy Grandes",
        ViewMode::List => "Lista",
    }
}

pub(in crate::iced_ui) fn view_mode_label_english(mode: ViewMode) -> &'static str {
    match mode {
        ViewMode::Details => "Details",
        ViewMode::Tiles => "Tiles",
        ViewMode::SmallIcons => "Small icons",
        ViewMode::MediumIcons => "Medium icons",
        ViewMode::LargeIcons => "Large icons",
        ViewMode::ExtraLargeIcons => "Extra large icons",
        ViewMode::List => "List",
    }
}

pub(in crate::iced_ui) fn view_mode_icon(mode: ViewMode) -> &'static str {
    match mode {
        ViewMode::Details => "view-details",
        ViewMode::Tiles => "view-tiles",
        ViewMode::SmallIcons | ViewMode::MediumIcons => "view-grid-small",
        ViewMode::LargeIcons | ViewMode::ExtraLargeIcons => "view-grid-large",
        ViewMode::List => "view-list",
    }
}

pub(in crate::iced_ui) fn ellipsize_to_width(value: &str, width: f32, font_size: f32) -> String {
    let estimated_char_width = (font_size * 0.58).max(1.0);
    let max_chars = (width / estimated_char_width).floor().max(4.0) as usize;
    ellipsize_text(value, max_chars)
}

pub(in crate::iced_ui) fn two_line_ellipsize_to_width(
    value: &str,
    width: f32,
    font_size: f32,
) -> String {
    let estimated_char_width = (font_size * 0.58).max(1.0);
    let line_chars = (width / estimated_char_width).floor().max(6.0) as usize;
    let max_chars = line_chars.saturating_mul(2).max(8);
    let text = ellipsize_text(value, max_chars);
    if text.chars().count() <= line_chars {
        return text;
    }

    let break_at = two_line_break_index(&text, line_chars);
    let first = text[..break_at].trim_end();
    let second = text[break_at..].trim_start();
    format!("{first}\n{second}")
}

pub(in crate::iced_ui) fn two_line_break_index(value: &str, line_chars: usize) -> usize {
    let char_count = value.chars().count();
    if char_count <= line_chars {
        return value.len();
    }

    let min_break = (line_chars as f32 * 0.58).floor().max(1.0) as usize;
    let preferred = value
        .char_indices()
        .take(line_chars + 1)
        .enumerate()
        .filter_map(|(char_index, (byte_index, character))| {
            (char_index >= min_break && character.is_whitespace()).then_some(byte_index)
        })
        .last();

    preferred.unwrap_or_else(|| {
        value
            .char_indices()
            .nth(line_chars)
            .map(|(byte_index, _)| byte_index)
            .unwrap_or(value.len())
    })
}

pub(in crate::iced_ui) fn estimated_column_width(
    chars: usize,
    font_size: f32,
    extra: f32,
    min_width: f32,
    max_width: f32,
) -> f32 {
    let estimated_char_width = (font_size * 0.58).max(1.0);
    (chars as f32 * estimated_char_width + extra).clamp(min_width, max_width)
}

pub(in crate::iced_ui) fn clamp_detail_column_width(column: TableColumn, width: f32) -> f32 {
    match column {
        TableColumn::Name => width.clamp(DETAIL_NAME_MIN_WIDTH, DETAIL_NAME_MAX_WIDTH),
        TableColumn::Type => width.clamp(DETAIL_TYPE_MIN_WIDTH, DETAIL_TYPE_MAX_WIDTH),
        TableColumn::Size => width.clamp(DETAIL_SIZE_MIN_WIDTH, DETAIL_SIZE_MAX_WIDTH),
        TableColumn::Modified => width.clamp(DETAIL_DATE_MIN_WIDTH, DETAIL_DATE_MAX_WIDTH),
        TableColumn::Created => width,
    }
}

pub(in crate::iced_ui) fn ellipsize_text(value: &str, max_chars: usize) -> String {
    let count = value.chars().count();
    if count <= max_chars {
        return value.to_string();
    }
    let keep = max_chars.saturating_sub(3).max(1);
    let mut text = value.chars().take(keep).collect::<String>();
    text.push_str("...");
    text
}
