use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering as AtomicOrdering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use iced::alignment::Horizontal;
use iced::widget::{
    Button, Id, Space, button, container, float, image as iced_image, mouse_area, opaque,
    scrollable, slider, stack, svg, text, text_editor, text_input,
};
use iced::{
    Alignment, Background, Color, ContentFit, Element, Event, Length, Padding, Point, Rectangle,
    Size, Subscription, Task, Theme, Vector, border, event, gradient, keyboard, mouse, window,
};

use crate::app::config::{
    AppConfig, GroupMode, ShortcutAction, ShortcutBinding, ShortcutConfig, SidebarSection,
    ThemePreference, VibrancyMode, ViewMode,
};
use crate::app::operation_host::OperationHostServer;
use crate::app::session::{AppSession, SplitFocus, SplitSession, SplitSide, TabState};
use crate::app::thumbnail_data;
use crate::fs::archive::{
    ArchiveCompressionMethod, ArchiveFormat, ArchiveJob, ArchiveJobKind, ArchiveProgress,
    ArchiveProgressMsg, ArchiveState, ExtractMode,
};
#[cfg(target_os = "windows")]
use crate::fs::defender::{self, DefenderJob};
use crate::fs::defender::{DefenderMessage, DefenderProgress, DefenderScanState, DefenderSummary};
use crate::fs::duplicates::{DuplicateFile, DuplicateScanEvent};
use crate::fs::explorer::{self, DriveKind, EntryKind, FileCategory, FileEntry};
use crate::fs::transfer_queue::{
    self, ConflictPolicy, ElevatedTransferResult, TransferCompletedRoot, TransferControl,
    TransferJob, TransferKind, TransferMessage, TransferProgress, TransferState,
};
use crate::fs::{archive, operations, portable, trash as trash_fs};
use crate::platform::shell;
use crate::utils::errors::BExplorerError;
use crate::utils::paths;

mod advanced;
mod duplicate_cleanup;
mod file_actions;
mod helpers;
mod interaction;
mod navigation;
#[cfg(target_os = "linux")]
mod properties;
mod search_state;
mod update;
mod view;

use helpers::*;

const THIS_PC_LABEL: &str = "This PC";
const TITLE_HEIGHT: f32 = 38.0;
const TITLE_BUTTON_WIDTH: f32 = 32.0;
const TITLE_BUTTON_HEIGHT: f32 = 31.0;
const TITLE_BUTTON_GAP: f32 = 3.0;
const TITLE_ICON_SIZE: f32 = 20.0;
const TITLE_TAB_START_PADDING: f32 = 12.0;
const TAB_HEIGHT: f32 = 34.0;
const TAB_UNDERLINE_HEIGHT: f32 = 2.0;
const TAB_ICON_SIZE: f32 = 20.0;
const TAB_CLOSE_ICON_SIZE: f32 = 11.25;
const TAB_WIDTH: f32 = 212.0;
const TAB_MIN_WIDTH: f32 = 72.0;
const TAB_LEFT_PADDING: f32 = 8.0;
const TAB_RIGHT_PADDING: f32 = 2.0;
const TAB_CLOSE_BUTTON_WIDTH: f32 = 20.0;
const TAB_ICON_TEXT_GAP: f32 = 5.0;
const TAB_DRAG_START_THRESHOLD: f32 = 5.0;
const FILE_DRAG_START_THRESHOLD: f32 = 5.0;
const TAB_SPLIT_DROP_TRIGGER_Y: f32 = TITLE_HEIGHT + 30.0;
// Keep the native handoff deliberately at the physical window edge so an
// in-app drag always retains its visual card and move semantics. A slightly
// wider zone makes it comfortable to reach another application without
// having to aim for the last pixel.
const EXTERNAL_DRAG_EDGE_TRIGGER: f32 = 32.0;
const SCROLLBAR_REVEAL_ZONE: f32 = 14.0;
const SCROLLBAR_FADE_STEP: f32 = 0.12;
// Action icons use a full 20 px raster footprint. The previous 18 px size
// made thin vector strokes look soft, especially on light themes.
const TOOL_ICON_SIZE: f32 = 20.0;
const WINDOW_RADIUS: f32 = 10.0;
const WINDOW_BORDER_WIDTH: f32 = 1.0;
const WINDOW_RESIZE_HANDLE_WIDTH: f32 = 6.0;
const SIDEBAR_MIN_WIDTH: f32 = 132.0;
const SIDEBAR_MAX_WIDTH: f32 = 320.0;
const SIDEBAR_RESIZE_HANDLE_WIDTH: f32 = 4.0;
const SIDEBAR_SECTION_HEIGHT: f32 = 31.0;
const SIDEBAR_SECTION_ICON_SIZE: f32 = 18.0;
const SIDEBAR_ITEM_HEIGHT: f32 = 30.0;
const SIDEBAR_SECTION_DRAG_START_THRESHOLD: f32 = 5.0;
const LAYOUT_ANIMATION_RESPONSE: f32 = 24.0;
const POPUP_ANIMATION_RESPONSE: f32 = 30.0;
const CONTEXT_MENU_ANIMATION_RESPONSE: f32 = 64.0;
const CONTEXT_MENU_INITIAL_SCALE: f32 = 0.98;
const SPLIT_DIVIDER_WIDTH: f32 = 6.0;
const SPLIT_MIN_RATIO: f32 = 0.24;
const SPLIT_MAX_RATIO: f32 = 0.76;
const DETAIL_HEADER_HEIGHT: f32 = 30.0;
const DETAIL_ROW_HEIGHT: f32 = 26.0;
const DETAIL_ICON_SIZE: f32 = 18.0;
const DETAIL_GROUP_HEIGHT: f32 = 26.0;
const DETAIL_NAME_MIN_WIDTH: f32 = 180.0;
const DETAIL_NAME_MAX_WIDTH: f32 = 460.0;
const DETAIL_TYPE_MIN_WIDTH: f32 = 92.0;
const DETAIL_TYPE_MAX_WIDTH: f32 = 230.0;
const DETAIL_SIZE_MIN_WIDTH: f32 = 78.0;
const DETAIL_SIZE_MAX_WIDTH: f32 = 132.0;
const DETAIL_DATE_MIN_WIDTH: f32 = 132.0;
const DETAIL_DATE_MAX_WIDTH: f32 = 196.0;
const DETAIL_COLUMN_HANDLE_WIDTH: f32 = 6.0;
const INITIAL_RENDER_LIMIT: usize = 500;
const RENDER_BATCH_SIZE: usize = 500;
const MAX_SEARCH_EVENTS_PER_TICK: usize = 2;
const STARTUP_BUSY_DELAY: Duration = Duration::from_millis(400);
const RUBBER_BAND_MIN_SIZE: f32 = 4.0;
const TRANSFER_MAX_PARALLEL: usize = 3;
const TRANSFER_CARD_HEIGHT: f32 = 96.0;
const TRANSFER_CARD_GAP: f32 = 8.0;
const TRANSFER_WINDOW_WIDTH: f32 = 540.0;
const TRANSFER_WINDOW_TITLE_HEIGHT: f32 = 30.0;
// Defender displays one progress card. Its native window stays fitted to the
// card and grows only when the result includes error or threat detail lines.
const DEFENDER_WINDOW_BASE_HEIGHT: f32 = 176.0;
const DEFENDER_WINDOW_DETAIL_LINE_HEIGHT: f32 = 26.0;
const DEFENDER_WINDOW_MAX_HEIGHT: f32 = 272.0;
const DEFENDER_CARD_HEIGHT: f32 = 132.0;
const DEFENDER_ERROR_CARD_HEIGHT: f32 = 158.0;
const DEFENDER_THREAT_CARD_HEIGHT: f32 = 58.0;
const DEFENDER_THREAT_CARD_GAP: f32 = 6.0;
const DEFENDER_THREAT_SECTION_GAP: f32 = 10.0;
const DEFENDER_THREAT_WINDOW_BASE_HEIGHT: f32 = 190.0;
const DEFENDER_THREAT_WINDOW_WIDTH: f32 = 620.0;
const DEFENDER_THREAT_WINDOW_VISIBLE_CARD_LIMIT: usize = 5;
const TRANSFER_PROGRESS_BAR_HEIGHT: f32 = 9.0;
const TRANSFER_WINDOW_CARD_PADDING_X: f32 = 4.0;
const TRANSFER_WINDOW_CARD_TOP_GAP: f32 = 2.0;
const TRANSFER_WINDOW_CARD_BOTTOM_PADDING: f32 = 8.0;
const TRANSFER_WINDOW_VISIBLE_CARD_LIMIT: usize = 3;
const DUPLICATE_WINDOW_WIDTH: f32 = 1_080.0;
const DUPLICATE_WINDOW_HEIGHT: f32 = 680.0;
const DUPLICATE_TABLE_HEADER_HEIGHT: f32 = 34.0;
const DUPLICATE_TABLE_ROW_HEIGHT: f32 = 34.0;
const DUPLICATE_TABLE_COLUMN_WIDTHS: [f32; 6] = [250.0, 105.0, 180.0, 205.0, 180.0, 220.0];
const DUPLICATE_TABLE_COLUMN_MIN_WIDTHS: [f32; 6] = [190.0, 70.0, 125.0, 125.0, 135.0, 150.0];
const DUPLICATE_TABLE_COLUMN_HANDLE_WIDTH: f32 = 7.0;
const COLOR_PICKER_WIDTH: f32 = 290.0;
const COLOR_PICKER_PLANE_WIDTH: f32 = 260.0;
const COLOR_PICKER_PLANE_HEIGHT: f32 = 210.0;
const COLOR_PICKER_HUE_WIDTH: f32 = COLOR_PICKER_WIDTH - 24.0;
const COLOR_PICKER_HUE_HEIGHT: f32 = 20.0;
#[cfg(target_os = "linux")]
const PROPERTIES_WINDOW_WIDTH: f32 = 480.0;
#[cfg(target_os = "linux")]
const PROPERTIES_WINDOW_HEIGHT: f32 = 628.0;

const UI_DENSITY_MIN_FONT_SIZE: f32 = 10.0;
const UI_DENSITY_FONT_INTERVAL: f32 = 3.0;
const UI_TEXT_SURFACE_REFERENCE_FONT_SIZE: f32 = 13.0;
const UI_TEXT_SURFACE_WIDTH_PER_FONT_PIXEL: f32 = 12.0;
const UI_MODAL_EDGE_MARGIN: f32 = 24.0;
const UI_TEXT_LINE_HEIGHT_FACTOR: f32 = 1.2;
const INLINE_ICON_OPTICAL_SCALE: f32 = 1.12;

fn ui_density_level(font_size: f32) -> f32 {
    ((font_size.round().clamp(10.0, 18.0) - UI_DENSITY_MIN_FONT_SIZE) / UI_DENSITY_FONT_INTERVAL)
        .floor()
        .max(0.0)
}

fn scaled_ui_metric(base: f32, font_size: f32) -> f32 {
    (base + ui_density_level(font_size)).max(0.0)
}

fn duplicate_table_header_scroll_id() -> Id {
    Id::new("duplicate-table-header-scroll")
}

fn duplicate_table_column_min_width(column: usize, font_size: f32, spanish: bool) -> f32 {
    let label = match (column, spanish) {
        (0, true) => "Nombre",
        (0, false) => "Name",
        (1, true) => "Tamaño",
        (1, false) => "Size",
        (2, true) => "Fecha de creación",
        (2, false) => "Creation date",
        (3, true) => "Fecha de modificación",
        (3, false) => "Modification date",
        (4, true) => "Coincidencia",
        (4, false) => "Match",
        (5, true) => "Ubicación",
        (5, false) => "Location",
        _ => "",
    };
    let content_extra = if column == 0 {
        scaled_ui_metric(16.0 + 8.0, font_size)
    } else {
        0.0
    };
    // Date headers are long but their values keep a fixed compact format. Once
    // the resize handle preserves its real width they only need a modest font
    // reserve, not the deliberately generous allowance used by other labels.
    let (font_reserve, fixed_reserve) = if matches!(column, 2 | 3) {
        (1.08, 4.0)
    } else {
        (1.35, 8.0)
    };
    let rendered_label_reserve =
        estimated_ui_text_width(label, font_size) * font_reserve + fixed_reserve;
    let header_width = rendered_label_reserve
        + scaled_ui_metric(8.0, font_size) * 2.0
        + scaled_ui_metric(DUPLICATE_TABLE_COLUMN_HANDLE_WIDTH, font_size)
        + content_extra;
    scaled_ui_metric(
        DUPLICATE_TABLE_COLUMN_MIN_WIDTHS
            .get(column)
            .copied()
            .unwrap_or_default(),
        font_size,
    )
    .max(header_width.ceil())
}

/// Fixed surfaces that contain text need more horizontal growth than icons and
/// compact controls. The density rule still provides the baseline at small
/// sizes, while larger fonts receive enough width to preserve the same useful
/// layout instead of forcing labels over adjacent controls.
fn adaptive_text_surface_width(base: f32, font_size: f32) -> f32 {
    let font_size = font_size.round().clamp(10.0, 18.0);
    let text_growth = (font_size - UI_TEXT_SURFACE_REFERENCE_FONT_SIZE).max(0.0)
        * UI_TEXT_SURFACE_WIDTH_PER_FONT_PIXEL;
    scaled_ui_metric(base, font_size).max(base + text_growth)
}

fn modal_text_surface_width(base: f32, font_size: f32, window_width: f32) -> f32 {
    let available_width = (window_width - UI_MODAL_EDGE_MARGIN).max(0.0);
    adaptive_text_surface_width(base, font_size).min(available_width)
}

fn estimated_ui_text_width(label: &str, font_size: f32) -> f32 {
    label
        .chars()
        .map(|character| estimated_ui_character_width(character, font_size))
        .sum()
}

fn estimated_ui_character_width(character: char, font_size: f32) -> f32 {
    let width_factor = match character {
        'i' | 'l' | 'I' | 'j' | '.' | ',' | ':' | ';' | '\'' | '|' => 0.28,
        ' ' => 0.31,
        'r' | 't' | 'f' | '-' | '_' => 0.40,
        'm' | 'w' | 'M' | 'W' => 0.82,
        'A'..='Z' => 0.65,
        '0'..='9' => 0.52,
        _ => 0.55,
    };
    width_factor * font_size
}

fn ellipsize_tab_title_to_width(value: &str, width: f32, font_size: f32) -> String {
    if estimated_ui_text_width(value, font_size) <= width {
        return value.to_owned();
    }

    let ellipsis = "...";
    let available = (width - estimated_ui_text_width(ellipsis, font_size) - 2.0).max(0.0);
    let mut result = String::new();
    let mut used = 0.0;
    for character in value.chars() {
        let character_width = estimated_ui_character_width(character, font_size);
        if used + character_width > available {
            break;
        }
        result.push(character);
        used += character_width;
    }
    result.push_str(ellipsis);
    result
}

fn fitted_tab_width(
    area_width: f32,
    tab_count: usize,
    add_button_width: f32,
    spacing: f32,
    preferred_width: f32,
    minimum_width: f32,
) -> f32 {
    if tab_count == 0 {
        return preferred_width;
    }
    let reserved = add_button_width + spacing * tab_count as f32;
    ((area_width - reserved).max(0.0) / tab_count as f32).clamp(minimum_width, preferred_width)
}

fn adaptive_text_slot_width(label: &str, font_size: f32, minimum: f32) -> f32 {
    scaled_ui_metric(minimum, font_size)
        .max((estimated_ui_text_width(label, font_size) + 8.0).ceil())
}

#[cfg(target_os = "linux")]
fn ellipsize_ui_text_to_width(value: &str, width: f32, font_size: f32) -> String {
    if estimated_ui_text_width(value, font_size) <= width {
        return value.to_owned();
    }

    let ellipsis = '…';
    let ellipsis_width = estimated_ui_text_width("…", font_size);
    let available_width = (width - ellipsis_width).max(0.0);
    let mut current_width = 0.0;
    let mut end = 0;
    for (index, character) in value.char_indices() {
        let next_width = estimated_ui_text_width(&character.to_string(), font_size);
        if current_width + next_width > available_width {
            break;
        }
        current_width += next_width;
        end = index + character.len_utf8();
    }

    let prefix = value[..end].trim_end();
    if prefix.is_empty() {
        ellipsis.to_string()
    } else {
        format!("{prefix}{ellipsis}")
    }
}

fn rendered_inline_icon_size(size: f32) -> f32 {
    (size * INLINE_ICON_OPTICAL_SCALE).round().max(1.0)
}

fn ui_text_line_height(font_size: f32) -> f32 {
    (font_size.max(1.0) * UI_TEXT_LINE_HEIGHT_FACTOR).ceil()
}

fn action_button_height(font_size: f32) -> f32 {
    let icon_size = scaled_ui_metric(TOOL_ICON_SIZE, font_size);
    scaled_ui_metric(36.0, font_size).max(ui_text_line_height(font_size).max(icon_size) + 12.0)
}

fn bookmark_button_height(font_size: f32) -> f32 {
    let icon_size = scaled_ui_metric(18.0, font_size);
    scaled_ui_metric(36.0, font_size).max(ui_text_line_height(font_size).max(icon_size) + 10.0)
}

fn localized_entry_count(count: usize, spanish: bool) -> String {
    let noun = match (spanish, count == 1) {
        (true, true) => "elemento",
        (true, false) => "elementos",
        (false, true) => "element",
        (false, false) => "elements",
    };
    format!("{count} {noun}")
}

fn is_entry_count_status(status: &str, count: usize) -> bool {
    let prefix = format!("{count} ");
    status.strip_prefix(&prefix).is_some_and(|noun| {
        matches!(
            noun,
            "element" | "elements" | "elemento" | "elementos" | "item" | "items"
        )
    })
}

fn sidebar_item_height_for_font(font_size: f32) -> f32 {
    let native_icon_height = scaled_ui_metric(18.0, font_size);
    let fallback_icon_height = rendered_inline_icon_size(scaled_ui_metric(16.0, font_size));
    let text_height = ui_text_line_height((font_size - 0.5).max(11.0));
    scaled_ui_metric(SIDEBAR_ITEM_HEIGHT, font_size)
        .max(text_height.max(native_icon_height.max(fallback_icon_height)) + 12.0)
}

fn detail_row_height_for_font(font_size: f32) -> f32 {
    let text_height = ui_text_line_height((font_size - 0.5).max(11.0));
    let icon_height = rendered_inline_icon_size(scaled_ui_metric(DETAIL_ICON_SIZE, font_size));
    scaled_ui_metric(DETAIL_ROW_HEIGHT, font_size).max(text_height.max(icon_height) + 6.0)
}

fn context_quick_button_height(font_size: f32) -> f32 {
    let icon_height = scaled_ui_metric(20.0, font_size);
    let label_height = ui_text_line_height((font_size - 2.0).max(10.0));
    scaled_ui_metric(48.0, font_size).max(icon_height + label_height + 10.0)
}

fn context_menu_row_height(font_size: f32) -> f32 {
    scaled_ui_metric(34.0, font_size).max(ui_text_line_height(font_size) + 10.0)
}

fn context_menu_width(font_size: f32) -> f32 {
    let large_text_width = 258.0 + (font_size - 13.0).max(0.0) * 12.0;
    scaled_ui_metric(258.0, font_size).max(large_text_width)
}

fn context_submenu_rows_height(rows: usize, font_size: f32) -> f32 {
    8.0 + rows as f32 * context_menu_row_height(font_size) + rows.saturating_sub(1) as f32 * 2.0
}

fn context_submenu_parent_offset(
    has_quick_actions: bool,
    preceding_rows: usize,
    preceding_separators: usize,
    font_size: f32,
) -> f32 {
    const MENU_TOP_PADDING: f32 = 4.0;
    const CHILD_SPACING: f32 = 2.0;
    const SEPARATOR_HEIGHT: f32 = 1.0;

    let preceding_children = usize::from(has_quick_actions) + preceding_rows + preceding_separators;
    MENU_TOP_PADDING
        + if has_quick_actions {
            context_quick_button_height(font_size) + 12.0
        } else {
            0.0
        }
        + preceding_rows as f32 * context_menu_row_height(font_size)
        + preceding_separators as f32 * SEPARATOR_HEIGHT
        + preceding_children as f32 * CHILD_SPACING
}

fn adaptive_menu_item_height(label: &str, font_size: f32, menu_width: f32) -> f32 {
    let available_width = (menu_width - 54.0).max(32.0);
    let estimated_width = label.chars().count() as f32 * (font_size * 0.58).max(1.0);
    let line_count = (estimated_width / available_width).ceil().clamp(1.0, 2.0);
    scaled_ui_metric(32.0, font_size).max(ui_text_line_height(font_size) * line_count + 10.0)
}

fn adaptive_menu_list_height(
    labels: &[&str],
    font_size: f32,
    menu_width: f32,
    spacing: f32,
    vertical_padding: f32,
) -> f32 {
    let rows = labels
        .iter()
        .map(|label| adaptive_menu_item_height(label, font_size, menu_width))
        .sum::<f32>();
    let gaps = labels.len().saturating_sub(1) as f32 * spacing;
    rows + gaps + vertical_padding * 2.0
}

fn stacked_text_control_height(base: f32, font_size: f32) -> f32 {
    let content_height =
        ui_text_line_height(font_size) + ui_text_line_height(font_size - 1.0) + 3.0 + 10.0;
    scaled_ui_metric(base, font_size).max(content_height)
}

fn settings_panel_height(font_size: f32, window_height: f32) -> f32 {
    let stacked_height = stacked_text_control_height(44.0, font_size);
    let stacked_height_extra = (stacked_height - scaled_ui_metric(44.0, font_size)).max(0.0);
    let desired_height = scaled_ui_metric(620.0, font_size) + stacked_height_extra * 7.0;
    desired_height.min((window_height - TITLE_HEIGHT - 24.0).max(320.0))
}

fn shortcuts_panel_height(font_size: f32) -> f32 {
    470.0 + ui_density_level(font_size) * 12.0
}

fn about_panel_height(font_size: f32) -> f32 {
    let header_height =
        scaled_ui_metric(TITLE_BUTTON_HEIGHT, font_size).max(ui_text_line_height(font_size + 3.0));
    let app_text_height = ui_text_line_height(font_size + 9.0)
        + ui_text_line_height(font_size)
        + ui_text_line_height(font_size) * 3.0
        + 10.0;
    let app_tile_height = scaled_ui_metric(96.0, font_size).max(app_text_height) + 8.0;
    let repository_height =
        ui_text_line_height(font_size - 1.0) + ui_text_line_height(font_size) + 18.0;
    scaled_ui_metric(245.0, font_size)
        .max(header_height + app_tile_height + repository_height + 48.0)
}

fn keep_process_after_main_window_closes(
    detach_requested: bool,
    operations_in_progress: bool,
) -> bool {
    detach_requested || operations_in_progress
}

fn should_exit_detached_operation_host(
    detached: bool,
    main_window_open: bool,
    operation_lifecycle_active: bool,
    main_window_reactivation_pending: bool,
) -> bool {
    detached
        && !main_window_open
        && !operation_lifecycle_active
        && !main_window_reactivation_pending
}

// Kept in the parent module so all UI workers share the same private state types.
include!("state.rs");

struct BExplorerIced {
    config: AppConfig,
    tabs: Vec<TabState>,
    active_tab: usize,
    split: Option<SplitRuntime>,
    primary: PaneState,
    secondary: PaneState,
    sidebar_storage_entries: Vec<FileEntry>,
    storage_refresh_scheduled: bool,
    trash_has_items: Option<bool>,
    trash_icon_request_id: u64,
    search_mode_menu_open: Option<PaneId>,
    new_menu_open: Option<PaneId>,
    title_menu_open: bool,
    show_menu_open: bool,
    show_menu_parent_hovered: bool,
    show_menu_submenu_hovered: bool,
    view_menu_open: Option<PaneId>,
    group_menu_open: Option<PaneId>,
    keyboard_menu_selection: Option<KeyboardMenuSelection>,
    preview_panel_pane: Option<PaneId>,
    preview_panel_target_pane: Option<PaneId>,
    address_edit: Option<AddressEditState>,
    context_menu: Option<ContextMenuState>,
    context_menu_request_id: u64,
    popup_backdrop: Option<iced_image::Handle>,
    title_submenu_backdrop: Option<iced_image::Handle>,
    color_picker_backdrop: Option<iced_image::Handle>,
    popup_fade_progress: f32,
    popup_fade_target: f32,
    color_picker_fade_progress: f32,
    color_picker_fade_target: f32,
    pending_popup_close: Option<PendingPopupClose>,
    context_archive_submenu: bool,
    context_open_with_submenu: bool,
    context_open_with_parent_hovered: bool,
    context_open_with_submenu_hovered: bool,
    context_send_to_submenu: bool,
    context_send_to_parent_hovered: bool,
    context_send_to_submenu_hovered: bool,
    context_extract_submenu: bool,
    context_new_submenu: bool,
    context_archive_parent_hovered: bool,
    context_archive_submenu_hovered: bool,
    context_new_parent_hovered: bool,
    context_new_submenu_hovered: bool,
    pane_pointer: Option<(PaneId, Point)>,
    current_modifiers: keyboard::Modifiers,
    view_scroll_accumulator: f32,
    system_theme_mode: iced::theme::Mode,
    file_clipboard: Option<FileClipboardState>,
    last_undo_action: Option<UndoAction>,
    last_entry_click: Option<EntryClickState>,
    thumbnail_cache: HashMap<PathBuf, IcedImageState>,
    small_thumbnail_cache: HashMap<PathBuf, IcedImageState>,
    preview_cache: HashMap<PathBuf, IcedImageState>,
    pdf_previews: HashMap<PaneId, PdfPreviewState>,
    native_icon_cache: HashMap<PathBuf, IcedImageState>,
    small_native_icon_cache: HashMap<PathBuf, IcedImageState>,
    transfer_tx: Sender<TransferMessage>,
    transfer_rx: Receiver<TransferMessage>,
    next_transfer_id: u64,
    next_archive_id: u64,
    transfer_queue: VecDeque<QueuedTransferState>,
    active_transfers: HashMap<u64, ActiveTransferState>,
    transfer_progress: HashMap<u64, TransferProgress>,
    transfer_batch_totals: HashMap<PaneId, (u64, u64)>,
    transfer_history: VecDeque<TransferHistoryState>,
    active_deletes: HashMap<u64, ActiveDeleteState>,
    transfer_progress_phase: f32,
    active_archives: HashMap<u64, ActiveArchiveState>,
    archive_history: VecDeque<ArchiveHistoryState>,
    defender_rx: Option<Receiver<DefenderMessage>>,
    defender_cancel: Option<Arc<AtomicBool>>,
    defender_progress: Option<DefenderProgress>,
    defender_summary: Option<DefenderSummary>,
    defender_window_id: Option<window::Id>,
    defender_threats_window_id: Option<window::Id>,
    defender_threat_remediation_pending: bool,
    defender_threat_remediation_message: Option<(String, bool)>,
    duplicate_cleanup: Option<DuplicateCleanupState>,
    duplicate_window_id: Option<window::Id>,
    rename_dialog: Option<RenameState>,
    archive_dialog: Option<ArchiveDialogState>,
    format_dialog: Option<FormatDialogState>,
    error_dialog: Option<ErrorDialogState>,
    permanent_delete_dialog: Option<PendingPermanentDelete>,
    transfer_conflict_dialog: Option<PendingTransferConflict>,
    elevated_transfer_dialog: Option<PendingElevatedTransfer>,
    elevated_delete_dialog: Option<PendingElevatedDelete>,
    elevated_file_action_dialog: Option<PendingElevatedFileAction>,
    pending_new_folder_rename: Option<(PaneId, PathBuf)>,
    pending_reveal_in_new_tab: Option<(PaneId, PathBuf, PathBuf)>,
    pending_file_operations: HashSet<PaneId>,
    mounting_disk_images: HashSet<PathBuf>,
    // A text-input submit and the global key listener can observe the same
    // Enter in either order. Keep Enter from opening the just-renamed item
    // while that event finishes propagating.
    suppress_open_after_rename_until: Option<Instant>,
    rubber_band: Option<RubberBandSelection>,
    file_drag: Option<FileDragState>,
    file_drag_fade_snapshot: Option<FileDragState>,
    file_drag_fade_progress: f32,
    file_drag_fade_target: f32,
    file_drag_suppressed_click: Option<(PaneId, usize)>,
    native_external_drag_active: bool,
    pending_external_file_drops: Vec<PathBuf>,
    external_file_drop_flush_queued: bool,
    tab_drag: Option<TabDragState>,
    sidebar_section_drag: Option<SidebarSectionDragState>,
    color_rgb_inputs: [String; 3],
    sidebar_progress: f32,
    preview_panel_progress: f32,
    last_animation_frame: Option<Instant>,
    color_picker_open: bool,
    accent_plane_dragging: bool,
    accent_plane_pointer: Option<Point>,
    accent_hue_dragging: bool,
    accent_hue_pointer: Option<Point>,
    window_size: Size,
    cursor_position: Point,
    resize_drag: Option<ResizeDrag>,
    settings_open: bool,
    shortcuts_open: bool,
    about_open: bool,
    shortcut_capture: Option<ShortcutAction>,
    sidebar_visible: bool,
    sidebar_pointer_inside: bool,
    window_maximized: bool,
    startup: StartupState,
    main_window_id: Option<window::Id>,
    main_window_detached_for_operations: bool,
    pending_main_window_reactivation: bool,
    operation_host: Option<OperationHostServer>,
    closing_windows: HashSet<window::Id>,
    transfer_window_id: Option<window::Id>,
    transfer_window_item_count: usize,
    archive_window_id: Option<window::Id>,
    archive_window_item_count: usize,
    #[cfg(target_os = "linux")]
    properties_window_id: Option<window::Id>,
    #[cfg(target_os = "linux")]
    properties_window: Option<properties::PropertiesWindowState>,
    #[cfg(target_os = "linux")]
    properties_request_id: u64,
}

pub fn run(initial_path: Option<PathBuf>) -> iced::Result {
    iced::daemon(
        move || BExplorerIced::new(initial_path.clone()),
        BExplorerIced::update,
        BExplorerIced::view_window,
    )
    .title(BExplorerIced::window_title)
    .theme(|app: &BExplorerIced, _window| app.theme())
    .style(BExplorerIced::app_style)
    .subscription(BExplorerIced::subscription)
    .antialiasing(true)
    .run()
}

fn input_event_message(event: Event, status: event::Status, window: window::Id) -> Option<Message> {
    match event {
        Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
            Some(Message::KeyboardModifiersChanged(modifiers))
        }
        Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            physical_key,
            modifiers,
            repeat,
            ..
        // Widgets such as inline rename editors own their Enter key. Do not
        // emit a second global shortcut for a key they already captured.
        }) if !repeat && status == event::Status::Ignored => {
            Some(Message::KeyPressed(window, key, physical_key, modifiers))
        }
        Event::Mouse(mouse::Event::ButtonPressed(_)) => {
            Some(Message::CheckAddressFocus(window))
        }
        Event::Touch(iced::touch::Event::FingerPressed { .. }) => {
            Some(Message::CheckAddressFocus(window))
        }
        Event::Window(window::Event::FileDropped(path)) => Some(Message::ExternalFileDropped(path)),
        _ => None,
    }
}

fn duplicate_column_resize_event_message(
    event: Event,
    _status: event::Status,
    window: window::Id,
) -> Option<Message> {
    match event {
        Event::Mouse(mouse::Event::CursorMoved { position }) => {
            Some(Message::DuplicateResizePointerMoved(window, position))
        }
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
        | Event::Mouse(mouse::Event::CursorLeft) => Some(Message::DuplicateResizeReleased(window)),
        _ => None,
    }
}

fn pointer_moved_beyond(start: Point, current: Point, threshold: f32) -> bool {
    let x = current.x - start.x;
    let y = current.y - start.y;
    x * x + y * y >= threshold * threshold
}

fn popup_backdrop_region_for_screenshot(
    target: &PopupBackdropTarget,
    physical_size: Size<u32>,
    scale_factor: f32,
    font_size: f32,
) -> Rectangle {
    let scale = scale_factor.max(1.0);
    let window_width = physical_size.width as f32 / scale;
    let window_height = physical_size.height as f32 / scale;
    let density = ui_density_level(font_size);
    let centered = |width: f32, height: f32| Rectangle {
        x: ((window_width - width) * 0.5).max(0.0),
        y: ((window_height - height) * 0.5).max(0.0),
        width: width.min(window_width),
        height: height.min(window_height),
    };

    match target {
        PopupBackdropTarget::TitleMenu => Rectangle {
            x: 0.0,
            y: TITLE_HEIGHT,
            width: scaled_ui_metric(220.0, font_size).min(window_width),
            height: (151.0 + density * 4.0).min((window_height - TITLE_HEIGHT).max(0.0)),
        },
        PopupBackdropTarget::NewMenu(pane) => Rectangle {
            x: if matches!(pane, PaneId::Secondary) {
                window_width * 0.5 + scaled_ui_metric(14.0, font_size)
            } else {
                scaled_ui_metric(14.0, font_size)
            },
            y: TITLE_HEIGHT
                + scaled_ui_metric(42.0, font_size)
                + (action_button_height(font_size) + 10.0).max(scaled_ui_metric(46.0, font_size)),
            width: scaled_ui_metric(196.0, font_size).min(window_width),
            height: (78.0 + density * 5.0).min((window_height - TITLE_HEIGHT).max(0.0)),
        },
        PopupBackdropTarget::SearchModeMenu(pane) => Rectangle {
            x: if matches!(pane, PaneId::Secondary) {
                window_width * 0.5
            } else {
                0.0
            },
            y: (window_height
                - scaled_ui_metric(36.0, font_size)
                - scaled_ui_metric(6.0, font_size)
                - (79.0 + density * 5.0))
                .max(0.0),
            width: scaled_ui_metric(
                if window_width < 700.0 {
                    210.0_f32
                } else {
                    260.0_f32
                },
                font_size,
            )
            .min(window_width),
            height: (79.0 + density * 5.0).min(window_height),
        },
        PopupBackdropTarget::ViewMenu(_) => Rectangle {
            x: (window_width
                - scaled_ui_metric(218.0, font_size)
                - scaled_ui_metric(14.0, font_size))
            .max(0.0),
            y: (window_height - 260.0 - density * 13.0).max(0.0),
            width: scaled_ui_metric(218.0, font_size).min(window_width),
            height: (219.0 + density * 13.0).min(window_height),
        },
        PopupBackdropTarget::GroupMenu(_) => Rectangle {
            x: (window_width
                - scaled_ui_metric(220.0, font_size)
                - scaled_ui_metric(104.0, font_size))
            .max(0.0),
            y: (TITLE_HEIGHT + scaled_ui_metric(82.0, font_size)).min(window_height),
            width: scaled_ui_metric(220.0, font_size).min(window_width),
            height: (223.0 + density * 14.0).min(window_height),
        },
        PopupBackdropTarget::Settings => centered(
            modal_text_surface_width(470.0, font_size, window_width),
            settings_panel_height(font_size, window_height),
        ),
        PopupBackdropTarget::Shortcuts => centered(
            modal_text_surface_width(740.0, font_size, window_width),
            shortcuts_panel_height(font_size),
        ),
        PopupBackdropTarget::About => centered(
            modal_text_surface_width(390.0, font_size, window_width),
            about_panel_height(font_size),
        ),
        PopupBackdropTarget::ColorPicker => {
            let mut region = centered(COLOR_PICKER_WIDTH, 400.0);
            region.x = ((window_width - modal_text_surface_width(470.0, font_size, window_width))
                * 0.5
                + scaled_ui_metric(136.0, font_size))
            .min((window_width - region.width).max(0.0))
            .max(0.0);
            region.y = ((window_height - 310.0) * 0.5 + 158.0)
                .min((window_height - 330.0).max(0.0))
                .max(0.0);
            region
        }
        PopupBackdropTarget::Rename(_) => centered(380.0, 164.0),
        PopupBackdropTarget::PermanentDelete(_) => centered(
            modal_text_surface_width(420.0, font_size, window_width),
            176.0,
        ),
        PopupBackdropTarget::Archive(_) => centered(
            modal_text_surface_width(470.0, font_size, window_width),
            382.0,
        ),
        PopupBackdropTarget::Format(_) => centered(
            modal_text_surface_width(480.0, font_size, window_width),
            560.0,
        ),
        PopupBackdropTarget::Error(_) => centered(
            modal_text_surface_width(500.0, font_size, window_width),
            270.0,
        ),
        PopupBackdropTarget::TransferConflict(_) => centered(
            modal_text_surface_width(460.0, font_size, window_width),
            238.0,
        ),
    }
}

impl BExplorerIced {
    fn is_properties_window(&self, id: window::Id) -> bool {
        #[cfg(target_os = "linux")]
        {
            self.properties_window_id == Some(id)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = id;
            false
        }
    }

    fn file_pane_bounds_for_screenshot(&self, pane: PaneId, window_width: f32) -> Rectangle {
        let shared_sidebar_width = if self.sidebar_is_rendered() && !self.uses_split_sidebars() {
            self.current_sidebar_width()
        } else {
            0.0
        };
        let content_x = shared_sidebar_width;
        let content_width = (window_width - content_x).max(0.0);
        let (segment_x, segment_width) = if let Some(split) = &self.split {
            let available = (content_width - SPLIT_DIVIDER_WIDTH).max(0.0);
            let primary_width = (available * split.ratio).round().clamp(0.0, available);
            match pane {
                PaneId::Primary => (content_x, primary_width),
                PaneId::Secondary => (
                    content_x + primary_width + SPLIT_DIVIDER_WIDTH,
                    (available - primary_width).max(0.0),
                ),
            }
        } else {
            (content_x, content_width)
        };
        let pane_sidebar_width = if self.uses_split_sidebars() && self.sidebar_is_rendered() {
            self.current_sidebar_width().min(segment_width)
        } else {
            0.0
        };
        Rectangle {
            x: segment_x + pane_sidebar_width,
            y: TITLE_HEIGHT,
            width: (segment_width - pane_sidebar_width).max(0.0),
            height: (self.window_size.height - TITLE_HEIGHT).max(0.0),
        }
    }

    fn pane_popup_backdrop_region(
        &self,
        target: &PopupBackdropTarget,
        physical_size: Size<u32>,
        scale_factor: f32,
    ) -> Option<Rectangle> {
        let scale = scale_factor.max(1.0);
        let window_width = physical_size.width as f32 / scale;
        let window_height = physical_size.height as f32 / scale;
        let (pane, offset_x, offset_y, width, height) = match target {
            PopupBackdropTarget::NewMenu(pane) => {
                let width = self.ui_metric(196.0);
                let item_spacing = self.ui_metric(2.0);
                let menu_padding = self.ui_metric(6.0);
                let labels = [
                    self.localized("Nueva carpeta", "New folder"),
                    self.localized("Documento de texto", "Text document"),
                ];
                let height = adaptive_menu_list_height(
                    &labels,
                    self.font_size(),
                    width,
                    item_spacing,
                    menu_padding,
                );
                (
                    *pane,
                    self.ui_metric(12.0),
                    self.toolbar_height() + self.action_bar_height(),
                    width,
                    height,
                )
            }
            PopupBackdropTarget::SearchModeMenu(pane) => {
                let width = self.ui_metric(if self.split.is_some() { 210.0 } else { 260.0 });
                let item_spacing = self.ui_metric(3.0);
                let menu_padding = self.ui_metric(6.0);
                let labels = [
                    self.localized("Búsqueda rápida", "Quick search"),
                    self.localized("Búsqueda completa", "Full search"),
                ];
                let height = adaptive_menu_list_height(
                    &labels,
                    self.font_size(),
                    width,
                    item_spacing,
                    menu_padding,
                );
                (
                    *pane,
                    self.ui_metric(14.0),
                    window_height
                        - TITLE_HEIGHT
                        - self.status_bar_height()
                        - self.ui_metric(6.0)
                        - height,
                    width,
                    height,
                )
            }
            PopupBackdropTarget::ViewMenu(pane) => {
                let bounds = self.file_pane_bounds_for_screenshot(*pane, window_width);
                let width = self.ui_metric(218.0);
                let labels = view_menu_modes().map(|mode| {
                    self.localized(view_mode_label(mode), view_mode_label_english(mode))
                });
                let height = adaptive_menu_list_height(
                    &labels,
                    self.font_size(),
                    width,
                    self.ui_metric(3.0),
                    self.ui_metric(6.0),
                );
                return Some(Rectangle {
                    x: (bounds.x + bounds.width - self.ui_metric(14.0) - width).max(bounds.x),
                    y: (window_height - self.ui_metric(38.0) - height).max(TITLE_HEIGHT),
                    width: width.min(bounds.width),
                    height: height.min((window_height - TITLE_HEIGHT).max(0.0)),
                });
            }
            PopupBackdropTarget::GroupMenu(pane) => {
                let bounds = self.file_pane_bounds_for_screenshot(*pane, window_width);
                let width = self.ui_metric(220.0);
                let labels = [
                    self.localized("Ninguno", "None"),
                    self.localized("Tipo", "Type"),
                    self.localized("Nombre", "Name"),
                    self.localized("Tamaño", "Size"),
                    self.localized("Ascendente", "Ascending"),
                    self.localized("Descendente", "Descending"),
                ];
                let item_spacing = self.ui_metric(3.0);
                let height = adaptive_menu_list_height(
                    &labels,
                    self.font_size(),
                    width,
                    item_spacing,
                    self.ui_metric(6.0),
                ) + item_spacing
                    + 1.0;
                return Some(Rectangle {
                    x: (bounds.x + bounds.width - self.ui_metric(104.0) - width).max(bounds.x),
                    y: TITLE_HEIGHT + self.ui_metric(82.0),
                    width: width.min(bounds.width),
                    height: height
                        .min((window_height - TITLE_HEIGHT - self.ui_metric(82.0)).max(0.0)),
                });
            }
            _ => return None,
        };
        let bounds = self.file_pane_bounds_for_screenshot(pane, window_width);
        Some(Rectangle {
            x: (bounds.x + offset_x).min((window_width - width).max(0.0)),
            y: (TITLE_HEIGHT + offset_y)
                .max(TITLE_HEIGHT)
                .min((window_height - height).max(0.0)),
            width: width.min(bounds.width),
            height: height.min((window_height - TITLE_HEIGHT).max(0.0)),
        })
    }

    fn new(initial_path: Option<PathBuf>) -> (Self, Task<Message>) {
        let startup_started_at = Instant::now();
        let mut startup = StartupState::default();
        let mut config = AppConfig::load();
        if !available_vibrancy_modes().contains(&config.vibrancy) {
            #[cfg(target_os = "windows")]
            {
                config.vibrancy = VibrancyMode::Acrylic;
            }
            #[cfg(not(target_os = "windows"))]
            {
                config.vibrancy = VibrancyMode::None;
            }
        }
        config.vibrancy_active = config.vibrancy != VibrancyMode::None;
        let session = AppSession::load();
        startup.mark_restoration_complete();
        let launch_path = initial_path.map(|path| {
            if path.as_os_str() == "~" {
                paths::home_dir().unwrap_or(path)
            } else {
                path
            }
        });
        let (tabs, active_tab, split) = if let Some(path) = launch_path {
            (
                vec![TabState::with_view_mode(Some(path), config.default_view)],
                0,
                None,
            )
        } else {
            let mut tabs = session.tabs;
            if tabs.is_empty() {
                tabs.push(TabState::new(None));
            }
            let active_tab = session.active_tab.min(tabs.len().saturating_sub(1));
            let split = session.split.and_then(|split| {
                if split.tab_a < tabs.len()
                    && split.tab_b < tabs.len()
                    && split.tab_a != split.tab_b
                {
                    Some(SplitRuntime {
                        primary_tabs: normalize_tabs(split.primary_tabs, split.tab_a, tabs.len()),
                        secondary_tabs: normalize_tabs(
                            split.secondary_tabs,
                            split.tab_b,
                            tabs.len(),
                        ),
                        secondary_tab: split.tab_b,
                        focused: split.focused,
                        ratio: split.ratio.clamp(SPLIT_MIN_RATIO, SPLIT_MAX_RATIO),
                    })
                } else {
                    None
                }
            });
            (tabs, active_tab, split)
        };

        let (transfer_tx, transfer_rx) = mpsc::channel();
        let initial_size = Size::new(config.window_size[0], config.window_size[1]);
        let initial_window_maximized = config.window_maximized;
        let color_rgb_inputs = accent_rgb_strings(config.accent_color);
        let preview_panel_pane = config.show_preview_panel.then_some(PaneId::Primary);
        let preview_panel_progress = if config.show_preview_panel { 1.0 } else { 0.0 };
        let mut app = Self {
            sidebar_visible: config.sidebar_visible,
            sidebar_pointer_inside: false,
            sidebar_progress: if config.sidebar_visible { 1.0 } else { 0.0 },
            preview_panel_progress,
            last_animation_frame: None,
            window_size: initial_size,
            cursor_position: Point::new(0.0, 0.0),
            resize_drag: None,
            config,
            tabs,
            active_tab,
            split,
            primary: PaneState::default(),
            secondary: PaneState::default(),
            sidebar_storage_entries: Vec::new(),
            storage_refresh_scheduled: false,
            trash_has_items: None,
            trash_icon_request_id: 0,
            search_mode_menu_open: None,
            new_menu_open: None,
            title_menu_open: false,
            show_menu_open: false,
            show_menu_parent_hovered: false,
            show_menu_submenu_hovered: false,
            view_menu_open: None,
            group_menu_open: None,
            keyboard_menu_selection: None,
            preview_panel_pane,
            preview_panel_target_pane: None,
            address_edit: None,
            context_menu: None,
            context_menu_request_id: 0,
            popup_backdrop: None,
            title_submenu_backdrop: None,
            color_picker_backdrop: None,
            popup_fade_progress: 0.0,
            popup_fade_target: 0.0,
            color_picker_fade_progress: 0.0,
            color_picker_fade_target: 0.0,
            pending_popup_close: None,
            context_archive_submenu: false,
            context_open_with_submenu: false,
            context_open_with_parent_hovered: false,
            context_open_with_submenu_hovered: false,
            context_send_to_submenu: false,
            context_send_to_parent_hovered: false,
            context_send_to_submenu_hovered: false,
            context_extract_submenu: false,
            context_new_submenu: false,
            context_archive_parent_hovered: false,
            context_archive_submenu_hovered: false,
            context_new_parent_hovered: false,
            context_new_submenu_hovered: false,
            pane_pointer: None,
            current_modifiers: keyboard::Modifiers::default(),
            view_scroll_accumulator: 0.0,
            system_theme_mode: iced::theme::Mode::None,
            file_clipboard: None,
            last_undo_action: None,
            last_entry_click: None,
            thumbnail_cache: HashMap::new(),
            small_thumbnail_cache: HashMap::new(),
            preview_cache: HashMap::new(),
            pdf_previews: HashMap::new(),
            native_icon_cache: HashMap::new(),
            small_native_icon_cache: HashMap::new(),
            transfer_tx,
            transfer_rx,
            next_transfer_id: 0,
            next_archive_id: 0,
            transfer_queue: VecDeque::new(),
            active_transfers: HashMap::new(),
            transfer_progress: HashMap::new(),
            transfer_batch_totals: HashMap::new(),
            transfer_history: VecDeque::new(),
            active_deletes: HashMap::new(),
            transfer_progress_phase: 0.0,
            active_archives: HashMap::new(),
            archive_history: VecDeque::new(),
            defender_rx: None,
            defender_cancel: None,
            defender_progress: None,
            defender_summary: None,
            defender_window_id: None,
            defender_threats_window_id: None,
            defender_threat_remediation_pending: false,
            defender_threat_remediation_message: None,
            duplicate_cleanup: None,
            duplicate_window_id: None,
            rename_dialog: None,
            archive_dialog: None,
            format_dialog: None,
            error_dialog: None,
            permanent_delete_dialog: None,
            transfer_conflict_dialog: None,
            elevated_transfer_dialog: None,
            elevated_delete_dialog: None,
            elevated_file_action_dialog: None,
            pending_new_folder_rename: None,
            pending_reveal_in_new_tab: None,
            pending_file_operations: HashSet::new(),
            mounting_disk_images: HashSet::new(),
            suppress_open_after_rename_until: None,
            rubber_band: None,
            file_drag: None,
            file_drag_fade_snapshot: None,
            file_drag_fade_progress: 0.0,
            file_drag_fade_target: 0.0,
            file_drag_suppressed_click: None,
            native_external_drag_active: false,
            pending_external_file_drops: Vec::new(),
            external_file_drop_flush_queued: false,
            tab_drag: None,
            sidebar_section_drag: None,
            color_rgb_inputs,
            settings_open: false,
            shortcuts_open: false,
            about_open: false,
            shortcut_capture: None,
            color_picker_open: false,
            accent_plane_dragging: false,
            accent_plane_pointer: None,
            accent_hue_dragging: false,
            accent_hue_pointer: None,
            window_maximized: initial_window_maximized,
            startup,
            main_window_id: None,
            main_window_detached_for_operations: false,
            pending_main_window_reactivation: false,
            operation_host: None,
            closing_windows: HashSet::new(),
            transfer_window_id: None,
            transfer_window_item_count: 0,
            archive_window_id: None,
            archive_window_item_count: 0,
            #[cfg(target_os = "linux")]
            properties_window_id: None,
            #[cfg(target_os = "linux")]
            properties_window: None,
            #[cfg(target_os = "linux")]
            properties_request_id: 0,
        };

        // Paint the last known storage state immediately. The root load then
        // refreshes this data asynchronously without blocking the first frame.
        app.sidebar_storage_entries =
            explorer::load_storage_cache(app.config.show_hidden_system_drives);

        app.reset_fixed_root_presentation(PaneId::Primary);
        app.sync_pane_search_from_tab(PaneId::Primary);
        if app.split.is_some() {
            app.reset_fixed_root_presentation(PaneId::Secondary);
            app.sync_pane_search_from_tab(PaneId::Secondary);
        }
        let (main_window_id, open_main_window) =
            window::open(main_window_settings(initial_size, initial_window_maximized));
        app.main_window_id = Some(main_window_id);

        let primary_starts_at_storage_root = app.tab_for_pane(PaneId::Primary).path.is_none();
        let secondary_starts_at_storage_root = app
            .split
            .as_ref()
            .is_some_and(|_| app.tab_for_pane(PaneId::Secondary).path.is_none());
        let sidebar_icons = app.queue_sidebar_icons();
        let primary_load = app.start_load(PaneId::Primary);
        app.track_startup_initial_load(PaneId::Primary);
        let mut tasks = vec![
            open_main_window.map(Message::MainWindowOpened),
            primary_load,
            sidebar_icons,
            Task::perform(
                delay(STARTUP_BUSY_DELAY.saturating_sub(startup_started_at.elapsed())),
                |_| Message::StartupBusyThresholdReached,
            ),
        ];
        if !primary_starts_at_storage_root && !secondary_starts_at_storage_root {
            tasks.push(app.refresh_sidebar_storage());
        }
        if matches!(app.config.theme, ThemePreference::System) {
            tasks.push(iced::system::theme().map(Message::SystemThemeChanged));
        }
        if app.split.is_some()
            && (!secondary_starts_at_storage_root || !primary_starts_at_storage_root)
        {
            let secondary_load = app.start_load(PaneId::Secondary);
            app.track_startup_initial_load(PaneId::Secondary);
            tasks.push(secondary_load);
        }
        #[cfg(debug_assertions)]
        if let Some(task) = app.seed_debug_archives_from_env() {
            tasks.push(task);
        }
        (app, Task::batch(tasks))
    }

    #[cfg(debug_assertions)]
    fn seed_debug_archives_from_env(&mut self) -> Option<Task<Message>> {
        let count = std::env::var("BEXPLORER_DEBUG_ARCHIVES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0)
            .min(5);

        if count == 0 {
            return None;
        }

        let step_ms = std::env::var("BEXPLORER_DEBUG_ARCHIVES_STEP_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);

        if step_ms > 0 {
            return Some(Task::batch((0..count).map(|index| {
                Task::perform(
                    async move {
                        delay(Duration::from_millis(step_ms.saturating_mul(index as u64))).await;
                        index
                    },
                    Message::DebugAddArchive,
                )
            })));
        }

        for index in 0..count {
            self.insert_debug_archive(index);
        }

        Some(self.ensure_archive_window_task())
    }

    #[cfg(debug_assertions)]
    fn insert_debug_archive(&mut self, index: usize) {
        self.next_archive_id = self.next_archive_id.saturating_add(1);
        let file_name = format!("minios-trixie-xfce-standard-amd64-5.1.{}.iso", index + 1);
        let source = PathBuf::from(format!("/tmp/{file_name}"));
        let destination = PathBuf::from(format!("/tmp/debug-compression-{}.7z", index + 1));
        let job = ArchiveJob {
            id: self.next_archive_id,
            kind: ArchiveJobKind::Compress,
            format: ArchiveFormat::SevenZip,
            method: ArchiveCompressionMethod::Normal,
            password: None,
            sources: vec![source],
            destination: destination.clone(),
            archive_path: destination,
            extract_mode: ExtractMode::Here,
        };
        let total = 812_400_000_u64 + index as u64 * 47_000_000;
        let completed = total.saturating_mul(38 + index as u64 * 12) / 100;
        let (_sender, receiver) = mpsc::channel();
        self.active_archives.insert(
            job.id,
            ActiveArchiveState {
                job,
                pane: PaneId::Primary,
                receiver,
                cancel: Arc::new(AtomicU32::new(0)),
                progress: ArchiveProgress {
                    completed,
                    total,
                    files: 1 + index as u64,
                    command: "Compress".into(),
                    file_name,
                },
            },
        );
    }

    fn theme(&self) -> Theme {
        if self.is_dark_theme() {
            Theme::Dark
        } else {
            Theme::Light
        }
    }

    fn is_dark_theme(&self) -> bool {
        match self.config.theme {
            ThemePreference::Dark => true,
            ThemePreference::System => matches!(self.system_theme_mode, iced::theme::Mode::Dark),
            ThemePreference::Light | ThemePreference::Gray => false,
        }
    }

    fn requests_linux_surface_blur(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            self.config.vibrancy == VibrancyMode::Blur
                && crate::platform::linux::is_wayland_session()
                && !crate::platform::linux::is_gnome_wayland()
        }
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }

    fn uses_linux_surface_blur(&self) -> bool {
        self.config.vibrancy_active && self.requests_linux_surface_blur()
    }

    fn file_content_background(&self, palette: Palette) -> Color {
        let mut background = palette.table_bg;
        if self.uses_linux_surface_blur() {
            background.a = layered_blur_tint_alpha(palette.page_bg.a);
        }
        background
    }

    fn is_spanish(&self) -> bool {
        self.config.language.eq_ignore_ascii_case("es")
    }

    fn localized(&self, spanish: &'static str, english: &'static str) -> &'static str {
        if self.is_spanish() { spanish } else { english }
    }

    fn app_style(&self, _theme: &Theme) -> iced::theme::Style {
        iced::theme::Style {
            background_color: Color::TRANSPARENT,
            text_color: Palette::from_config(&self.config, self.is_dark_theme()).text,
        }
    }

    fn apply_window_corners_task(&self) -> Task<Message> {
        let mut tasks = Vec::new();
        for id in [
            self.main_window_id,
            self.transfer_window_id,
            self.archive_window_id,
            self.defender_window_id,
            self.defender_threats_window_id,
            self.duplicate_window_id,
        ]
        .into_iter()
        .flatten()
        {
            tasks.push(self.apply_window_corners_task_for(id));
        }
        #[cfg(target_os = "linux")]
        if let Some(id) = self.properties_window_id {
            tasks.push(self.apply_window_corners_task_for(id));
        }
        Task::batch(tasks)
    }

    fn apply_window_corners_task_for(&self, id: window::Id) -> Task<Message> {
        self.apply_window_appearance_task_for(id, true)
    }

    fn close_window_task(&mut self, id: window::Id) -> Task<Message> {
        if self.closing_windows.insert(id) {
            close_window_after_native_cleanup(id)
        } else {
            Task::none()
        }
    }

    fn close_application_task(&mut self, id: window::Id) -> Task<Message> {
        if self.closing_windows.insert(id) {
            close_application_after_native_cleanup(id)
        } else {
            Task::none()
        }
    }

    fn transfer_jobs_in_progress(&self) -> bool {
        !self.active_transfers.is_empty()
            || !self.transfer_queue.is_empty()
            || !self.active_deletes.is_empty()
    }

    fn archive_jobs_in_progress(&self) -> bool {
        !self.active_archives.is_empty()
    }

    fn duplicate_cleanup_window_open(&self) -> bool {
        self.duplicate_cleanup.is_some() && self.duplicate_window_id.is_some()
    }

    fn detachable_operations_in_progress(&self) -> bool {
        self.transfer_jobs_in_progress()
            || self.archive_jobs_in_progress()
            || self.defender_active()
            || self.duplicate_cleanup_window_open()
    }

    fn operation_lifecycle_active(&self) -> bool {
        self.transfer_active()
            || self.archive_active()
            || self.defender_active()
            || self.duplicate_cleanup_window_open()
    }

    fn main_window_is_closing(&self) -> bool {
        self.main_window_id
            .is_some_and(|id| self.closing_windows.contains(&id))
    }

    fn publish_operation_host(&mut self) {
        if self.operation_host.is_none() {
            match OperationHostServer::new() {
                Ok(server) => self.operation_host = Some(server),
                Err(error) => {
                    crate::utils::log::error(format!(
                        "Could not start the background operation host: {error}"
                    ));
                    return;
                }
            }
        }
        let publish_error = self
            .operation_host
            .as_mut()
            .and_then(|host| host.publish().err());
        if let Some(error) = publish_error {
            crate::utils::log::error(format!(
                "Could not publish the background operation host: {error}"
            ));
            self.operation_host = None;
        }
    }

    fn retire_operation_host(&mut self) {
        self.operation_host = None;
    }

    fn ensure_detached_operation_windows_task(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        if self.transfer_jobs_in_progress() {
            tasks.push(self.ensure_transfer_window_task());
        }
        if self.archive_jobs_in_progress() {
            tasks.push(self.ensure_archive_window_task());
        }
        #[cfg(target_os = "windows")]
        if self.defender_active() {
            tasks.push(self.ensure_defender_window_task());
        }
        if self.duplicate_cleanup.is_some() && self.duplicate_window_id.is_none() {
            let (id, open) = window::open(duplicate_cleanup_window_settings(self.font_size()));
            self.duplicate_window_id = Some(id);
            tasks.push(open.map(Message::DuplicateCleanupWindowOpened));
        }
        Task::batch(tasks)
    }

    fn poll_operation_host_activations_task(&mut self) -> Task<Message> {
        if self.main_window_is_closing() {
            return Task::none();
        }
        let activations = self
            .operation_host
            .as_ref()
            .map(OperationHostServer::drain_activations)
            .unwrap_or_default();
        Task::batch(
            activations
                .into_iter()
                .map(|path| self.reactivate_main_window_task(path)),
        )
    }

    fn reactivate_main_window_task(&mut self, path: Option<PathBuf>) -> Task<Message> {
        self.main_window_detached_for_operations = false;
        self.pending_main_window_reactivation = false;
        let path = path.map(|path| {
            if path.as_os_str() == "~" {
                paths::home_dir().unwrap_or(path)
            } else {
                path
            }
        });
        let navigation = path
            .filter(|path| self.tab_for_pane(self.focused_pane()).path.as_ref() != Some(path))
            .map(|path| self.open_path_in_new_tab(self.focused_pane(), Some(path)))
            .unwrap_or_else(Task::none);

        if let Some(id) = self.main_window_id {
            return Task::batch([
                navigation,
                window::minimize(id, false),
                window::gain_focus(id),
            ]);
        }

        let (id, open_window) = window::open(main_window_settings(
            self.window_size,
            self.window_maximized,
        ));
        self.main_window_id = Some(id);
        Task::batch([navigation, open_window.map(Message::MainWindowOpened)])
    }

    fn ensure_main_window_for_attention_task(&mut self) -> Task<Message> {
        if self.main_window_is_closing() {
            self.pending_main_window_reactivation = true;
            return Task::none();
        }
        if self.main_window_id.is_none() {
            return self.reactivate_main_window_task(None);
        }
        self.pending_main_window_reactivation = false;
        Task::none()
    }

    fn detached_operation_host_ready_to_exit(&self) -> bool {
        should_exit_detached_operation_host(
            self.main_window_detached_for_operations,
            self.main_window_id.is_some(),
            self.operation_lifecycle_active(),
            self.pending_main_window_reactivation,
        )
    }

    fn finish_detached_operation_host_task(&mut self) -> Task<Message> {
        if !self.detached_operation_host_ready_to_exit() {
            return Task::none();
        }
        self.retire_operation_host();
        if let Some(id) = self
            .transfer_window_id
            .or(self.archive_window_id)
            .or(self.defender_window_id)
            .or(self.defender_threats_window_id)
            .or(self.duplicate_window_id)
        {
            self.close_application_task(id)
        } else {
            iced::exit()
        }
    }

    fn sync_main_window_maximized_task(&self, id: window::Id) -> Task<Message> {
        window::is_maximized(id).map(move |maximized| Message::WindowMaximizedState(id, maximized))
    }

    fn main_window_corner_radius(&self) -> f32 {
        if self.window_maximized {
            1.0
        } else {
            WINDOW_RADIUS
        }
    }

    fn window_appearance_size(&self, id: window::Id) -> Size {
        if self.main_window_id == Some(id) {
            self.window_size
        } else if self.transfer_window_id == Some(id) {
            self.transfer_window_size()
        } else if self.archive_window_id == Some(id) {
            self.archive_window_size()
        } else if self.defender_window_id == Some(id) {
            self.defender_window_size()
        } else if self.defender_threats_window_id == Some(id) {
            let threat_count = self
                .defender_summary
                .as_ref()
                .map(|summary| summary.threats.len())
                .unwrap_or_default();
            defender_threats_window_size(threat_count)
        } else if self.duplicate_window_id == Some(id) {
            self.duplicate_cleanup
                .as_ref()
                .map(|state| state.window_size)
                .unwrap_or_else(|| duplicate_cleanup_window_size(self.font_size()))
        } else if self.is_properties_window(id) {
            #[cfg(target_os = "linux")]
            {
                properties_window_size(self.font_size())
            }
            #[cfg(not(target_os = "linux"))]
            {
                self.window_size
            }
        } else {
            self.window_size
        }
    }

    fn apply_window_corners_only_task_for(&self, id: window::Id) -> Task<Message> {
        self.apply_window_appearance_task_for(id, false)
    }

    fn prepare_native_file_drag_task_for(&self, id: window::Id) -> Task<Message> {
        window::run(id, move |native_window| {
            if let (Ok(display_handle), Ok(window_handle)) = (
                native_window.display_handle(),
                native_window.window_handle(),
            ) {
                crate::platform::prepare_external_file_drag(
                    display_handle.as_raw(),
                    window_handle.as_raw(),
                );
            }
            Message::Noop
        })
    }

    fn poll_external_file_drag(&mut self) -> Task<Message> {
        let Some(id) = self.main_window_id else {
            self.native_external_drag_active = false;
            return Task::none();
        };
        window::run(id, move |native_window| {
            let result = (|| {
                let display_handle = native_window
                    .display_handle()
                    .map_err(|error| format!("No se pudo acceder a la pantalla nativa: {error}"))?;
                let window_handle = native_window
                    .window_handle()
                    .map_err(|error| format!("No se pudo acceder a la ventana nativa: {error}"))?;
                let active = crate::platform::poll_external_file_drag(
                    display_handle.as_raw(),
                    window_handle.as_raw(),
                )
                .map_err(|error| error.to_string())?;
                let drops = crate::platform::take_external_file_drops(
                    display_handle.as_raw(),
                    window_handle.as_raw(),
                );
                Ok((active, drops))
            })();
            Message::ExternalFileDragPolled(result)
        })
    }

    /// Winit delivers native file drops on every supported desktop backend.
    /// It supplies paths but not a per-item target coordinate, so drops land
    /// in the currently focused pane's directory, matching an empty-area drop.
    fn copy_external_files_into_focused_pane(&mut self, paths: Vec<PathBuf>) -> Task<Message> {
        if paths.is_empty() {
            return Task::none();
        }

        let pane = self.focused_pane();
        let Some(destination) = self.tab_for_pane(pane).path.clone() else {
            self.pane_mut(pane).status = "Suelta los archivos dentro de una carpeta".into();
            return Task::none();
        };
        if paths
            .iter()
            .all(|source| source.parent().is_some_and(|parent| parent == destination))
        {
            self.pane_mut(pane).status = "Los archivos ya están en esa carpeta".into();
            return Task::none();
        }

        let count = paths.len();
        crate::utils::log::info(format!(
            "Queueing {count} external dropped file(s) into {}",
            destination.display()
        ));
        let task = self.request_transfer(pane, paths, destination, TransferKind::Copy, false);
        if self.transfer_conflict_dialog.is_none() {
            self.pane_mut(pane).status =
                format!("Copiando {count} elemento(s) desde otra aplicación");
        }
        task
    }

    fn apply_window_appearance_task_for(
        &self,
        id: window::Id,
        apply_vibrancy: bool,
    ) -> Task<Message> {
        let radius = if self.main_window_id == Some(id) {
            self.main_window_corner_radius()
        } else if self.duplicate_window_id == Some(id)
            && self
                .duplicate_cleanup
                .as_ref()
                .is_some_and(|state| state.window_maximized)
        {
            1.0
        } else {
            WINDOW_RADIUS
        }
        .round()
        .max(1.0) as u32;
        let size = self.window_appearance_size(id);
        let width = size.width.ceil().max(1.0) as u32;
        let height = size.height.ceil().max(1.0) as u32;
        let vibrancy = self.config.vibrancy;
        let vibrancy_intensity = self.config.vibrancy_intensity;
        let dark = self.is_dark_theme();
        let cancel_autoplay = self.main_window_id == Some(id);
        window::run(id, move |native_window| {
            if let (Ok(window_handle), Ok(display_handle)) = (
                native_window.window_handle(),
                native_window.display_handle(),
            ) {
                let _ = crate::platform::apply_window_corners(
                    &window_handle,
                    &display_handle,
                    width,
                    height,
                    radius,
                );
                if cancel_autoplay {
                    #[cfg(target_os = "windows")]
                    let _ = crate::platform::install_main_window_hooks(&window_handle);
                    let _ = crate::platform::prepare_storage_change_notifications(&window_handle);
                }
            }
            if apply_vibrancy {
                let active = crate::platform::apply_window_vibrancy(
                    native_window,
                    vibrancy,
                    vibrancy_intensity,
                    dark,
                    width,
                    height,
                    radius,
                )
                .unwrap_or_else(|error| {
                    crate::utils::log::info(format!(
                        "Native window effect could not be applied; using opaque fallback: {error}"
                    ));
                    false
                });
                return Message::VibrancyApplied(active);
            }
            Message::Noop
        })
    }

    fn subscription(&self) -> Subscription<Message> {
        let input_events = event::listen_with(input_event_message);

        let pointer_events = if self.pointer_tracking_active() {
            event::listen_with(|event, _status, _window| match event {
                Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    Some(Message::PointerMoved(position))
                }
                Event::Mouse(mouse::Event::CursorLeft) => Some(Message::PointerLeftWindow),
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    Some(Message::StopResize)
                }
                _ => None,
            })
        } else {
            Subscription::none()
        };
        let duplicate_column_resize_events = if self
            .duplicate_cleanup
            .as_ref()
            .is_some_and(|state| state.column_resize.is_some())
        {
            if self.duplicate_window_id.is_some() {
                event::listen_with(duplicate_column_resize_event_message)
            } else {
                Subscription::none()
            }
        } else {
            Subscription::none()
        };

        let transfer_tick = if self.transfer_active()
            || self.archive_active()
            || self.defender_active()
            || self.duplicate_cleanup.as_ref().is_some_and(|state| {
                matches!(
                    state.phase,
                    DuplicateCleanupPhase::Counting | DuplicateCleanupPhase::Scanning
                )
            })
            || self.main_window_detached_for_operations
            || self.pending_main_window_reactivation
        {
            Subscription::run(transfer_tick_stream)
        } else {
            Subscription::none()
        };
        let animation_frame = if self.startup.waiting_for_first_frame()
            || self.sidebar_animation_active()
            || self.preview_panel_animation_active()
            || self.popup_fade_animation_active()
            || self.file_drag_fade_animation_active()
        {
            window::frames().map(Message::AnimationFrame)
        } else {
            Subscription::none()
        };
        let scrollbar_tick = if self.scrollbar_animation_active() {
            Subscription::run(scrollbar_animation_tick_stream)
        } else {
            Subscription::none()
        };
        let async_progress_tick = if self.async_progress_animation_active() {
            Subscription::run(async_progress_tick_stream)
        } else {
            Subscription::none()
        };
        // Poll the custom Wayland source only while a BExplorer drag is being
        // prepared or remains active. Incoming drops arrive through the
        // blocking event-driven subscription below and need no idle timer.
        let external_drag_tick = if external_drag_polling_required(
            self.file_drag.is_some(),
            self.native_external_drag_active,
        ) {
            Subscription::run(external_drag_tick_stream)
        } else {
            Subscription::none()
        };
        let external_file_drops = if cfg!(all(unix, not(target_os = "macos"))) {
            Subscription::run(external_file_drop_stream)
        } else {
            Subscription::none()
        };
        let search_tick = if self.search_in_progress() {
            Subscription::run(search_tick_stream)
        } else {
            Subscription::none()
        };
        let system_theme_changes = if matches!(self.config.theme, ThemePreference::System) {
            iced::system::theme_changes().map(Message::SystemThemeChanged)
        } else {
            Subscription::none()
        };
        let storage_changes = if cfg!(any(target_os = "windows", target_os = "linux")) {
            Subscription::run(storage_change_stream)
        } else {
            Subscription::none()
        };
        let directory_changes = if cfg!(any(target_os = "windows", target_os = "linux")) {
            Subscription::run(directory_change_stream)
        } else {
            Subscription::none()
        };

        Subscription::batch([
            window::resize_events().map(|(id, size)| Message::WindowResized(id, size)),
            window::close_requests().map(Message::WindowCloseRequested),
            window::close_events().map(Message::WindowClosed),
            input_events,
            pointer_events,
            duplicate_column_resize_events,
            transfer_tick,
            animation_frame,
            scrollbar_tick,
            async_progress_tick,
            external_drag_tick,
            external_file_drops,
            search_tick,
            system_theme_changes,
            storage_changes,
            directory_changes,
        ])
    }
}

#[cfg(test)]
mod address_focus_tests {
    use super::*;

    #[test]
    fn every_mouse_press_requests_an_address_focus_check() {
        let window = window::Id::unique();
        let message = input_event_message(
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            event::Status::Captured,
            window,
        );

        assert!(matches!(
            message,
            Some(Message::CheckAddressFocus(id)) if id == window
        ));
    }
}

#[cfg(test)]
mod context_layout_tests {
    use super::*;

    #[test]
    fn submenu_offsets_align_with_their_parent_rows() {
        let font_size = 13.0;
        let open_with = context_submenu_parent_offset(true, 1, 1, font_size);
        let send_to = context_submenu_parent_offset(true, 2, 1, font_size);
        let archive = context_submenu_parent_offset(true, 2, 2, font_size);
        let extract = context_submenu_parent_offset(true, 3, 2, font_size);

        assert_eq!(
            send_to - open_with,
            context_menu_row_height(font_size) + 2.0
        );
        assert_eq!(archive - send_to, 3.0);
        assert_eq!(extract - archive, context_menu_row_height(font_size) + 2.0);
    }

    #[test]
    fn submenu_offsets_scale_with_context_rows() {
        let normal = context_submenu_parent_offset(true, 2, 1, 13.0);
        let enlarged = context_submenu_parent_offset(true, 2, 1, 19.0);
        assert!(enlarged > normal);
    }

    #[test]
    fn tab_titles_use_the_available_width_without_wrapping() {
        let width = 150.0;
        let title =
            ellipsize_tab_title_to_width("BStream-Music-1.2.1-Android-arm64-v8a.apk", width, 13.0);

        assert!(title.ends_with("..."));
        assert!(!title.contains('\n'));
        assert!(estimated_ui_text_width(&title, 13.0) <= width);
    }

    #[test]
    fn tabs_shrink_only_when_the_title_area_is_crowded() {
        let preferred = 212.0;
        let minimum = 72.0;
        assert_eq!(
            fitted_tab_width(900.0, 3, 32.0, 3.0, preferred, minimum),
            preferred
        );
        let crowded = fitted_tab_width(500.0, 5, 32.0, 3.0, preferred, minimum);
        assert!(crowded < preferred);
        assert!(crowded > minimum);
        assert_eq!(
            fitted_tab_width(100.0, 10, 32.0, 3.0, preferred, minimum),
            minimum
        );
    }
}
