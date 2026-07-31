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

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::iced_ui) struct VirtualTableRange {
    pub(in crate::iced_ui) start: usize,
    pub(in crate::iced_ui) end: usize,
    pub(in crate::iced_ui) before: f32,
    pub(in crate::iced_ui) after: f32,
}

pub(in crate::iced_ui) fn virtual_table_pixel_window(
    offset_y: f32,
    viewport_height: f32,
    velocity_y: f32,
    row_height: f32,
) -> (f32, f32) {
    let row_height = row_height.max(1.0);
    let viewport_height = if viewport_height.is_finite() && viewport_height > row_height {
        viewport_height
    } else {
        row_height * VIRTUAL_TABLE_FALLBACK_ROWS
    };
    let offset_y = offset_y.max(0.0);
    let screens_per_second = (velocity_y.abs() / viewport_height).min(12.0);
    let directional_screens = (screens_per_second * 0.25).min(VIRTUAL_TABLE_MAX_VELOCITY_SCREENS);
    let before_screens = VIRTUAL_TABLE_OVERSCAN_SCREENS
        + if velocity_y < 0.0 {
            directional_screens
        } else {
            0.0
        };
    let after_screens = VIRTUAL_TABLE_OVERSCAN_SCREENS
        + if velocity_y > 0.0 {
            directional_screens
        } else {
            0.0
        };

    (
        (offset_y - viewport_height * before_screens).max(0.0),
        offset_y + viewport_height * (1.0 + after_screens),
    )
}

pub(in crate::iced_ui) fn virtual_table_range(
    total: usize,
    row_height: f32,
    offset_y: f32,
    viewport_height: f32,
    velocity_y: f32,
) -> VirtualTableRange {
    if total == 0 {
        return VirtualTableRange {
            start: 0,
            end: 0,
            before: 0.0,
            after: 0.0,
        };
    }

    let row_height = row_height.max(1.0);
    let (window_start, window_end) =
        virtual_table_pixel_window(offset_y, viewport_height, velocity_y, row_height);
    let start = ((window_start / row_height).floor() as usize).min(total);
    let end = ((window_end / row_height).ceil() as usize)
        .max(start.saturating_add(1))
        .min(total);

    VirtualTableRange {
        start,
        end,
        before: start as f32 * row_height,
        after: total.saturating_sub(end) as f32 * row_height,
    }
}

pub(in crate::iced_ui) fn sampled_scroll_velocity(
    previous_offset: f32,
    previous_sample: Option<Instant>,
    previous_velocity: f32,
    offset: f32,
    now: Instant,
) -> f32 {
    let delta = offset - previous_offset;
    if delta.abs() < 0.5 {
        return previous_velocity * 0.8;
    }
    let Some(elapsed) = previous_sample.map(|sample| now.saturating_duration_since(sample)) else {
        return 0.0;
    };
    let seconds = elapsed.as_secs_f32();
    if seconds <= f32::EPSILON {
        return previous_velocity;
    }
    let measured = (delta / seconds).clamp(-200_000.0, 200_000.0);
    previous_velocity * 0.35 + measured * 0.65
}

pub(in crate::iced_ui) fn compare_entries_for_view(
    left: &FileEntry,
    right: &FileEntry,
    group_mode: GroupMode,
    group_ascending: bool,
    sort_column: TableColumn,
    sort_ascending: bool,
) -> std::cmp::Ordering {
    let system_drive_order = compare_system_drive_first(left, right);
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

    system_drive_order
        .then(primary_order)
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

pub(in crate::iced_ui) fn compare_system_drive_first(
    left: &FileEntry,
    right: &FileEntry,
) -> std::cmp::Ordering {
    let left_is_system = left.drive_kind == Some(DriveKind::System);
    let right_is_system = right.drive_kind == Some(DriveKind::System);
    right_is_system.cmp(&left_is_system)
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

pub(in crate::iced_ui) fn uses_small_entry_images(mode: ViewMode) -> bool {
    matches!(
        mode,
        ViewMode::Details | ViewMode::SmallIcons | ViewMode::List
    )
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

pub(in crate::iced_ui) fn visual_cell_width_for_font(mode: ViewMode, font_size: f32) -> f32 {
    adaptive_text_surface_width(visual_view_metrics(mode).cell_width, font_size)
}

pub(in crate::iced_ui) fn visual_min_cell_width_for_font(mode: ViewMode, font_size: f32) -> f32 {
    adaptive_text_surface_width(visual_min_cell_width(mode), font_size)
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

pub(in crate::iced_ui) fn ellipsize_to_glyph_width(
    value: &str,
    width: f32,
    font_size: f32,
) -> String {
    if estimated_ui_text_width(value, font_size) <= width {
        return value.to_owned();
    }

    const ELLIPSIS: &str = "...";

    let available_width = (width - estimated_ui_text_width(ELLIPSIS, font_size) - 2.0).max(0.0);
    let mut shortened = String::new();
    let mut used_width = 0.0;
    for character in value.chars() {
        let character_width = estimated_ui_character_width(character, font_size);
        if used_width + character_width > available_width {
            break;
        }
        shortened.push(character);
        used_width += character_width;
    }
    shortened.push_str(ELLIPSIS);
    shortened
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

pub(in crate::iced_ui) fn clamp_detail_column_width(
    column: TableColumn,
    width: f32,
    font_size: f32,
) -> f32 {
    match column {
        TableColumn::Name => width.clamp(
            scaled_ui_metric(DETAIL_NAME_MIN_WIDTH, font_size),
            scaled_ui_metric(DETAIL_NAME_MAX_WIDTH, font_size),
        ),
        TableColumn::Type => width.clamp(
            scaled_ui_metric(DETAIL_TYPE_MIN_WIDTH, font_size),
            scaled_ui_metric(DETAIL_TYPE_MAX_WIDTH, font_size),
        ),
        TableColumn::Size => width.clamp(
            scaled_ui_metric(DETAIL_SIZE_MIN_WIDTH, font_size),
            scaled_ui_metric(DETAIL_SIZE_MAX_WIDTH, font_size),
        ),
        TableColumn::Modified => width.clamp(
            scaled_ui_metric(DETAIL_DATE_MIN_WIDTH, font_size),
            scaled_ui_metric(DETAIL_DATE_MAX_WIDTH, font_size),
        ),
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

#[cfg(test)]
mod virtual_table_tests {
    use super::*;

    #[test]
    fn initial_window_uses_a_safe_fallback_without_rendering_every_row() {
        let range = virtual_table_range(100_000, 20.0, 0.0, 0.0, 0.0);

        assert_eq!(range.start, 0);
        assert_eq!(range.end, 150);
        assert_eq!(range.before, 0.0);
        assert_eq!(range.after, 1_997_000.0);
    }

    #[test]
    fn middle_window_preserves_the_full_scroll_extent() {
        let range = virtual_table_range(10_000, 20.0, 10_000.0, 400.0, 0.0);

        assert_eq!((range.start, range.end), (470, 550));
        assert_eq!(range.before, 9_400.0);
        assert_eq!(range.after, 189_000.0);
        assert!(range.start <= 500 && range.end > 500);
    }

    #[test]
    fn fast_scrolling_expands_overscan_in_the_travel_direction() {
        let resting = virtual_table_range(10_000, 20.0, 10_000.0, 400.0, 0.0);
        let down = virtual_table_range(10_000, 20.0, 10_000.0, 400.0, 4_000.0);
        let up = virtual_table_range(10_000, 20.0, 10_000.0, 400.0, -4_000.0);

        assert_eq!(down.start, resting.start);
        assert!(down.end > resting.end);
        assert!(up.start < resting.start);
        assert_eq!(up.end, resting.end);
    }

    #[test]
    fn direct_scroll_jump_renders_the_destination_instead_of_intermediate_batches() {
        let offset = 1_600_000.0;
        let range = virtual_table_range(100_000, 20.0, offset, 800.0, 0.0);
        let destination = (offset / 20.0) as usize;

        assert!(range.start <= destination);
        assert!(range.end > destination);
        assert!(range.end - range.start < 250);
    }

    #[test]
    fn sampled_velocity_tracks_direction_and_smooths_the_measurement() {
        let now = Instant::now();
        let previous = now - Duration::from_millis(100);

        let down = sampled_scroll_velocity(100.0, Some(previous), 0.0, 500.0, now);
        let up = sampled_scroll_velocity(500.0, Some(previous), 0.0, 100.0, now);

        assert!(down > 0.0);
        assert!(up < 0.0);
    }
}
