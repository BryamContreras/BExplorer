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
use crate::app::session::{AppSession, SplitFocus, SplitSession, SplitSide, TabState};
use crate::app::thumbnail_data;
use crate::fs::archive::{
    ArchiveCompressionMethod, ArchiveFormat, ArchiveJob, ArchiveJobKind, ArchiveProgress,
    ArchiveProgressMsg, ArchiveState, ExtractMode,
};
#[cfg(target_os = "windows")]
use crate::fs::defender::{self, DefenderJob};
use crate::fs::defender::{DefenderMessage, DefenderProgress, DefenderScanState, DefenderSummary};
use crate::fs::explorer::{self, DriveKind, EntryKind, FileCategory, FileEntry};
use crate::fs::transfer_queue::{
    self, ConflictPolicy, ElevatedTransferResult, TransferCompletedRoot, TransferControl,
    TransferJob, TransferKind, TransferMessage, TransferProgress, TransferState,
};
use crate::fs::{archive, operations, portable};
use crate::platform::shell;
use crate::utils::errors::BExplorerError;
use crate::utils::paths;

mod advanced;
mod file_actions;
mod helpers;
mod interaction;
mod navigation;
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
const TAB_WIDTH: f32 = 196.0;
const TAB_DRAG_START_THRESHOLD: f32 = 5.0;
const FILE_DRAG_START_THRESHOLD: f32 = 5.0;
const TAB_DRAG_STRIDE: f32 = TAB_WIDTH + 3.0;
const TAB_DRAG_MAX_OFFSET: f32 = TAB_DRAG_STRIDE;
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
const DETAIL_DATE_MAX_WIDTH: f32 = 172.0;
const DETAIL_COLUMN_HANDLE_WIDTH: f32 = 6.0;
const INITIAL_RENDER_LIMIT: usize = 500;
const RENDER_BATCH_SIZE: usize = 500;
const MAX_SEARCH_EVENTS_PER_TICK: usize = 2;
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
const TRANSFER_WINDOW_VISIBLE_CARD_LIMIT: f32 = 3.0;
const TRANSFER_WINDOW_CARD_ONLY_CHROME_HEIGHT: f32 = WINDOW_BORDER_WIDTH * 2.0
    + TRANSFER_WINDOW_TITLE_HEIGHT
    + TRANSFER_WINDOW_CARD_TOP_GAP
    + TRANSFER_WINDOW_CARD_BOTTOM_PADDING;
const TRANSFER_WINDOW_CARD_ONLY_MIN_HEIGHT: f32 =
    TRANSFER_WINDOW_CARD_ONLY_CHROME_HEIGHT + TRANSFER_CARD_HEIGHT;
const TRANSFER_WINDOW_CARD_ONLY_MAX_HEIGHT: f32 = TRANSFER_WINDOW_CARD_ONLY_CHROME_HEIGHT
    + TRANSFER_CARD_HEIGHT * TRANSFER_WINDOW_VISIBLE_CARD_LIMIT
    + TRANSFER_CARD_GAP * (TRANSFER_WINDOW_VISIBLE_CARD_LIMIT - 1.0);
const COLOR_PICKER_WIDTH: f32 = 290.0;
const COLOR_PICKER_PLANE_WIDTH: f32 = 260.0;
const COLOR_PICKER_PLANE_HEIGHT: f32 = 210.0;
const COLOR_PICKER_HUE_WIDTH: f32 = COLOR_PICKER_WIDTH - 24.0;
const COLOR_PICKER_HUE_HEIGHT: f32 = 20.0;

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
    search_mode_menu_open: Option<PaneId>,
    new_menu_open: Option<PaneId>,
    title_menu_open: bool,
    show_menu_open: bool,
    show_menu_parent_hovered: bool,
    show_menu_submenu_hovered: bool,
    view_menu_open: Option<PaneId>,
    group_menu_open: Option<PaneId>,
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
    preview_cache: HashMap<PathBuf, IcedImageState>,
    pdf_previews: HashMap<PaneId, PdfPreviewState>,
    native_icon_cache: HashMap<PathBuf, IcedImageState>,
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
    main_window_id: Option<window::Id>,
    closing_windows: HashSet<window::Id>,
    transfer_window_id: Option<window::Id>,
    transfer_window_item_count: usize,
    archive_window_id: Option<window::Id>,
    archive_window_item_count: usize,
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

fn keyboard_event_message(
    event: Event,
    status: event::Status,
    _window: window::Id,
) -> Option<Message> {
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
            Some(Message::KeyPressed(key, physical_key, modifiers))
        }
        Event::Window(window::Event::FileDropped(path)) => Some(Message::ExternalFileDropped(path)),
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
) -> Rectangle {
    let scale = scale_factor.max(1.0);
    let window_width = physical_size.width as f32 / scale;
    let window_height = physical_size.height as f32 / scale;
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
            width: 220.0_f32.min(window_width),
            height: 151.0_f32.min((window_height - TITLE_HEIGHT).max(0.0)),
        },
        PopupBackdropTarget::NewMenu(pane) => Rectangle {
            x: if matches!(pane, PaneId::Secondary) {
                window_width * 0.5 + 14.0
            } else {
                14.0
            },
            y: TITLE_HEIGHT + 88.0,
            width: 196.0_f32.min(window_width),
            height: 78.0_f32.min((window_height - TITLE_HEIGHT).max(0.0)),
        },
        PopupBackdropTarget::SearchModeMenu(pane) => Rectangle {
            x: if matches!(pane, PaneId::Secondary) {
                window_width * 0.5
            } else {
                0.0
            },
            y: (window_height - 124.0).max(0.0),
            width: (if window_width < 700.0 {
                210.0_f32
            } else {
                260.0_f32
            })
            .min(window_width),
            height: 79.0_f32.min(window_height),
        },
        PopupBackdropTarget::ViewMenu(_) => Rectangle {
            x: (window_width - 232.0).max(0.0),
            y: (window_height - 260.0).max(0.0),
            width: 218.0_f32.min(window_width),
            height: 219.0_f32.min(window_height),
        },
        PopupBackdropTarget::GroupMenu(_) => Rectangle {
            x: (window_width - 324.0).max(0.0),
            y: (TITLE_HEIGHT + 82.0).min(window_height),
            width: 220.0_f32.min(window_width),
            height: 223.0_f32.min(window_height),
        },
        PopupBackdropTarget::Settings => centered(470.0, 570.0),
        PopupBackdropTarget::Shortcuts => centered(740.0, 470.0),
        PopupBackdropTarget::About => centered(390.0, 245.0),
        PopupBackdropTarget::ColorPicker => {
            let mut region = centered(COLOR_PICKER_WIDTH, 400.0);
            region.x = ((window_width - 470.0) * 0.5 + 136.0)
                .min((window_width - region.width).max(0.0))
                .max(0.0);
            region.y = ((window_height - 310.0) * 0.5 + 158.0)
                .min((window_height - 330.0).max(0.0))
                .max(0.0);
            region
        }
        PopupBackdropTarget::Rename(_) => centered(380.0, 164.0),
        PopupBackdropTarget::PermanentDelete(_) => centered(420.0, 176.0),
        PopupBackdropTarget::Archive(_) => centered(470.0, 382.0),
        PopupBackdropTarget::Format(_) => centered(480.0, 560.0),
        PopupBackdropTarget::Error(_) => centered(500.0, 270.0),
        PopupBackdropTarget::TransferConflict(_) => centered(460.0, 238.0),
    }
}

impl BExplorerIced {
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
            PopupBackdropTarget::NewMenu(pane) => (*pane, 12.0, 88.0, 196.0, 78.0),
            PopupBackdropTarget::SearchModeMenu(pane) => (
                *pane,
                14.0,
                window_height - TITLE_HEIGHT - 121.0,
                if self.split.is_some() { 210.0 } else { 260.0 },
                79.0,
            ),
            PopupBackdropTarget::ViewMenu(pane) => {
                let bounds = self.file_pane_bounds_for_screenshot(*pane, window_width);
                return Some(Rectangle {
                    x: (bounds.x + bounds.width - 232.0).max(bounds.x),
                    y: (window_height - 257.0).max(TITLE_HEIGHT),
                    width: 218.0_f32.min(bounds.width),
                    height: 219.0_f32.min((window_height - TITLE_HEIGHT).max(0.0)),
                });
            }
            PopupBackdropTarget::GroupMenu(pane) => {
                let bounds = self.file_pane_bounds_for_screenshot(*pane, window_width);
                return Some(Rectangle {
                    x: (bounds.x + bounds.width - 324.0).max(bounds.x),
                    y: TITLE_HEIGHT + 82.0,
                    width: 220.0_f32.min(bounds.width),
                    height: 223.0_f32.min((window_height - TITLE_HEIGHT - 82.0).max(0.0)),
                });
            }
            _ => return None,
        };
        let bounds = self.file_pane_bounds_for_screenshot(pane, window_width);
        Some(Rectangle {
            x: (bounds.x + offset_x).min((window_width - width).max(0.0)),
            y: (TITLE_HEIGHT + offset_y).min((window_height - height).max(0.0)),
            width: width.min(bounds.width),
            height: height.min((window_height - TITLE_HEIGHT).max(0.0)),
        })
    }

    fn new(initial_path: Option<PathBuf>) -> (Self, Task<Message>) {
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
            search_mode_menu_open: None,
            new_menu_open: None,
            title_menu_open: false,
            show_menu_open: false,
            show_menu_parent_hovered: false,
            show_menu_submenu_hovered: false,
            view_menu_open: None,
            group_menu_open: None,
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
            preview_cache: HashMap::new(),
            pdf_previews: HashMap::new(),
            native_icon_cache: HashMap::new(),
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
            main_window_id: None,
            closing_windows: HashSet::new(),
            transfer_window_id: None,
            transfer_window_item_count: 0,
            archive_window_id: None,
            archive_window_item_count: 0,
        };

        // Paint the last known storage state immediately. The root load then
        // refreshes this data asynchronously without blocking the first frame.
        app.sidebar_storage_entries = explorer::load_storage_cache();

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
        let mut tasks = vec![
            open_main_window.map(Message::MainWindowOpened),
            app.start_load(PaneId::Primary),
            sidebar_icons,
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
            tasks.push(app.start_load(PaneId::Secondary));
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
        ]
        .into_iter()
        .flatten()
        {
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
                    let _ = crate::platform::install_autoplay_cancel(&window_handle);
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
        let keyboard_events = event::listen_with(keyboard_event_message);

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

        let transfer_tick =
            if self.transfer_active() || self.archive_active() || self.defender_active() {
                Subscription::run(transfer_tick_stream)
            } else {
                Subscription::none()
            };
        let animation_frame = if self.sidebar_animation_active()
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

        Subscription::batch([
            window::resize_events().map(|(id, size)| Message::WindowResized(id, size)),
            window::close_requests().map(Message::WindowCloseRequested),
            window::close_events().map(Message::WindowClosed),
            keyboard_events,
            pointer_events,
            transfer_tick,
            animation_frame,
            scrollbar_tick,
            async_progress_tick,
            external_drag_tick,
            external_file_drops,
            search_tick,
            system_theme_changes,
            storage_changes,
        ])
    }
}
