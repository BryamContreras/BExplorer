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
use crate::fs::defender::{
    DefenderMessage, DefenderProgress, DefenderScanState, DefenderSummary, ElevatedDefenderAction,
};
use crate::fs::explorer::{self, DriveKind, EntryKind, FileCategory, FileEntry};
use crate::fs::transfer_queue::{
    self, ConflictPolicy, TransferCompletedRoot, TransferControl, TransferJob, TransferKind,
    TransferMessage, TransferProgress, TransferState,
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
const SIDEBAR_SLIDE_STEP: f32 = 0.16;
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
const TRANSFER_WINDOW_HEADER_HEIGHT: f32 = 26.0;
const TRANSFER_WINDOW_OVERALL_BAR_HEIGHT: f32 = 9.0;
const TRANSFER_PROGRESS_BAR_HEIGHT: f32 = 9.0;
const TRANSFER_WINDOW_CONTENT_GAP: f32 = 12.0;
const TRANSFER_WINDOW_HEADER_PADDING_X: f32 = 12.0;
const TRANSFER_WINDOW_HEADER_PADDING_Y: f32 = 10.0;
const TRANSFER_WINDOW_CARD_PADDING_X: f32 = 4.0;
const TRANSFER_WINDOW_CARD_TOP_GAP: f32 = 2.0;
const TRANSFER_WINDOW_CARD_BOTTOM_PADDING: f32 = 8.0;
const TRANSFER_WINDOW_VISIBLE_CARD_LIMIT: f32 = 3.0;
const TRANSFER_WINDOW_CHROME_HEIGHT: f32 = TRANSFER_WINDOW_TITLE_HEIGHT
    + TRANSFER_WINDOW_HEADER_PADDING_Y * 2.0
    + TRANSFER_WINDOW_HEADER_HEIGHT
    + TRANSFER_WINDOW_CONTENT_GAP
    + TRANSFER_WINDOW_OVERALL_BAR_HEIGHT
    + TRANSFER_WINDOW_CARD_TOP_GAP * 2.0
    + TRANSFER_WINDOW_CARD_BOTTOM_PADDING
    + 2.0;
const TRANSFER_WINDOW_MIN_HEIGHT: f32 = TRANSFER_WINDOW_CHROME_HEIGHT + TRANSFER_CARD_HEIGHT;
const TRANSFER_WINDOW_MAX_HEIGHT: f32 = TRANSFER_WINDOW_CHROME_HEIGHT
    + TRANSFER_CARD_HEIGHT * TRANSFER_WINDOW_VISIBLE_CARD_LIMIT
    + TRANSFER_CARD_GAP * (TRANSFER_WINDOW_VISIBLE_CARD_LIMIT - 1.0);
const COLOR_PICKER_WIDTH: f32 = 290.0;
const COLOR_PICKER_PLANE_WIDTH: f32 = 260.0;
const COLOR_PICKER_PLANE_HEIGHT: f32 = 210.0;
const COLOR_PICKER_HUE_WIDTH: f32 = COLOR_PICKER_WIDTH - 24.0;
const COLOR_PICKER_HUE_HEIGHT: f32 = 20.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum PaneId {
    Primary,
    Secondary,
}

#[derive(Clone, Copy, Debug)]
enum ScrollbarAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Debug)]
enum Message {
    Loaded(PaneId, u64, Result<Vec<FileEntry>, String>),
    CloseTab(PaneId, usize),
    NewTab(PaneId),
    StartTabDrag(PaneId, usize),
    StartSidebarSectionDrag(SidebarSection),
    ToggleMenu,
    OpenShortcuts,
    CloseShortcuts,
    BeginShortcutCapture(ShortcutAction),
    ShortcutBindingCaptured(ShortcutBinding),
    ResetShortcut(ShortcutAction),
    OpenShowMenu,
    ShowMenuParentEnter,
    ShowMenuParentExit,
    ShowMenuSubmenuEnter,
    ShowMenuSubmenuExit,
    CloseShowMenuIfUnhovered,
    ToggleActionBar,
    ToggleBookmarkBar,
    ToggleSplitPaneMenus,
    ToggleSplitPreviewPanels,
    ToggleSidebar,
    SidebarAnimationTick,
    PreviewPanelAnimationTick,
    PopupFadeAnimationTick,
    ScrollbarHover(PaneId, ScrollbarAxis, bool),
    ScrollbarAnimationTick,
    ToggleSplit,
    Navigate(PaneId, Option<PathBuf>),
    BeginAddressEdit(PaneId),
    AddressChanged(String),
    SubmitAddress(PaneId),
    RowPressed(PaneId, usize),
    Back(PaneId),
    Forward(PaneId),
    Up(PaneId),
    ToggleFavorite(PaneId),
    Refresh(PaneId),
    ToggleNewMenu(PaneId),
    NewFolder(PaneId),
    NewFolderFinished(PaneId, Result<PathBuf, String>),
    NewTextDocument(PaneId),
    NewTextDocumentFinished(PaneId, Result<PathBuf, String>),
    PasteIntoPane(PaneId),
    CopySelection(PaneId),
    CutSelection(PaneId),
    DeleteSelected(PaneId),
    OpenArchiveDialog(PaneId),
    ArchiveNameChanged(String),
    SetArchiveFormat(ArchiveFormat),
    SetArchiveCompressionMethod(ArchiveCompressionMethod),
    ToggleArchivePassword,
    ArchivePasswordChanged(String),
    ArchivePasswordConfirmationChanged(String),
    ShowArchivePassword(bool),
    ShowArchivePasswordConfirmation(bool),
    ConfirmArchiveDialog,
    CancelArchiveDialog,
    CancelArchive(u64),
    TrashFinished(PaneId, Result<operations::TrashDeleteOutcome, String>),
    UndoLastAction,
    UndoFinished(UndoAction, Result<usize, String>),
    VirtualArchiveExtractFinished(PaneId, PaneId, PathBuf, Result<usize, String>),
    SearchChanged(PaneId, String),
    ToggleSearchModeMenu(PaneId),
    SetSearchMode(PaneId, SearchMode),
    PollSearches,
    PaneScrolled(PaneId, f32, f32, bool),
    PaneMouseWheel(PaneId, mouse::ScrollDelta),
    ToggleViewMenu(PaneId),
    CloseFloatingMenus,
    SetViewMode(PaneId, ViewMode),
    ToggleGroupMenu(PaneId),
    SetGroupMode(PaneId, GroupMode),
    SetGroupAscending(PaneId, bool),
    SortColumn(PaneId, TableColumn),
    ImageLoaded(IcedImageLoadResult),
    PdfPreviewPageLoaded(PdfPreviewLoadResult),
    PdfPreviewScrolled(PaneId, PathBuf, f32),
    TextPreviewAction(PaneId, PathBuf, text_editor::Action),
    PanePointerMoved(PaneId, Point),
    PanePointerExited(PaneId),
    StartRubberBand(PaneId),
    StartFileDrag(PaneId, usize),
    OpenEntry(PaneId, usize),
    FileDragTargetEnter(PaneId, usize),
    FileDragTargetExit(PaneId, usize),
    FileDragSidebarTargetEnter(PaneId, PathBuf),
    FileDragSidebarTargetExit(PathBuf),
    OpenBackgroundContext(PaneId),
    OpenEntryContext(PaneId, usize),
    ContextPasteAvailabilityResolved(ContextMenuState, bool),
    ContextBackdropCaptured(ContextMenuState, window::Screenshot),
    ContextBackdropPrepared(ContextMenuState, Option<iced_image::Handle>),
    ContextSubmenuBackdropCaptured(u64, ContextSubmenuKind, window::Screenshot),
    ContextSubmenuBackdropPrepared(u64, ContextSubmenuKind, Option<iced_image::Handle>),
    PopupBackdropCaptured(PopupBackdropTarget, window::Screenshot),
    PopupBackdropPrepared(PopupBackdropTarget, Option<iced_image::Handle>),
    TitleMenuBackdropsPrepared(Option<iced_image::Handle>, Option<iced_image::Handle>),
    CloseContextMenu,
    ContextArchiveParentEnter,
    ContextExtractParentEnter,
    ContextNewParentEnter,
    ContextArchiveParentExit,
    ContextNewParentExit,
    ContextArchiveSubmenuEnter,
    ContextNewSubmenuEnter,
    ContextArchiveSubmenuExit,
    ContextNewSubmenuExit,
    CloseContextArchiveSubmenuIfUnhovered,
    CloseContextNewSubmenuIfUnhovered,
    RunContextCommand(ContextCommand),
    KeyPressed(keyboard::Key, keyboard::key::Physical, keyboard::Modifiers),
    KeyboardModifiersChanged(keyboard::Modifiers),
    RenameChanged(String),
    RenameEdited(text_editor::Action),
    RenameSelected(PaneId),
    ConfirmRename,
    RenameFinished(RenameState, Result<PathBuf, String>),
    CancelRename,
    ConfirmPermanentDelete,
    PermanentDeleteFinished(PaneId, Result<usize, String>),
    CancelPermanentDelete,
    DiskImageMounted(PaneId, PathBuf, Result<PathBuf, String>),
    DriveEjected(PaneId, Result<(), String>),
    CancelDefenderScan,
    CloseDefenderPanel,
    RemoveDefenderThreats,
    ExcludeDefenderPaths,
    OpenWindowsSecurity,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    DefenderActionFinished(Result<String, String>),
    PortableClipboardPrepared(PaneId, Result<Vec<PathBuf>, String>),
    PortableOpenPrepared(PaneId, Result<PathBuf, String>),
    PortableDeleteFinished(PaneId, Result<usize, String>),
    PortableTransferFinished(PaneId, Vec<PathBuf>, bool, Result<usize, String>),
    ResolveTransferConflict(ConflictPolicy),
    CancelTransferConflict,
    MainWindowOpened(window::Id),
    TransferWindowOpened(window::Id),
    ArchiveWindowOpened(window::Id),
    ReopenTransferWindow(window::Id, Option<Point>),
    ReopenArchiveWindow(window::Id, Option<Point>),
    WindowClosed(window::Id),
    PollTransfers,
    TransferWindowDrag,
    TransferWindowMinimize,
    ArchiveWindowDrag,
    ArchiveWindowMinimize,
    ToggleTransferPause(u64),
    CancelTransfer(u64),
    ToggleSettings,
    TogglePreviewPanel(PaneId),
    ToggleColorPicker,
    FontDown,
    FontUp,
    AccentRgbChanged(usize, String),
    StartAccentPlaneDrag,
    AccentPlaneHover(Point),
    StartAccentHueDrag,
    AccentHueHover(Point),
    FinishColorDrag,
    SelectLanguage(String),
    SelectTheme(String),
    SystemThemeChanged(iced::theme::Mode),
    SelectVibrancy(String),
    SetVibrancyIntensity(u8),
    VibrancyIntensityReleased,
    VibrancyApplied(bool),
    ToggleShowExtensions,
    ToggleShowHidden,
    WindowDrag,
    WindowResize(window::Direction),
    WindowMinimize,
    WindowMaximize,
    WindowMaximizedState(window::Id, bool),
    WindowClose,
    StartSidebarResize,
    StartPreviewResize(PaneId),
    StartSplitResize,
    StartColumnResize(PaneId, TableColumn),
    PointerMoved(Point),
    PointerLeftWindow,
    StopResize,
    ExternalFileDragFinished(PaneId, usize, Result<(), String>),
    PollExternalFileDrag,
    ExternalFileDragPolled(Result<(bool, Vec<Vec<PathBuf>>), String>),
    ExternalFileDropped(PathBuf),
    FlushExternalFileDrops,
    ClearFileDragClickSuppression,
    WindowResized(window::Id, Size),
    #[cfg(debug_assertions)]
    DebugAddArchive(usize),
    Noop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ContextTarget {
    Background,
    Entry(usize),
}

/// A floating in-window surface that needs a cached blurred backdrop.
///
/// The data-bearing variants intentionally hold the dialog state until the
/// screenshot has been taken. This keeps the dialog out of its own backdrop.
#[derive(Clone, Debug)]
enum PopupBackdropTarget {
    TitleMenu,
    NewMenu(PaneId),
    SearchModeMenu(PaneId),
    ViewMenu(PaneId),
    GroupMenu(PaneId),
    Settings,
    Shortcuts,
    ColorPicker,
    Rename(RenameState),
    PermanentDelete(PendingPermanentDelete),
    Archive(ArchiveDialogState),
    TransferConflict(PendingTransferConflict),
}

#[derive(Clone, Debug)]
struct ContextMenuState {
    request_id: u64,
    pane: PaneId,
    target: ContextTarget,
    position: Point,
    backdrop_origin: Point,
    backdrop: Option<iced_image::Handle>,
    source_screenshot: Option<window::Screenshot>,
    submenu_backdrop: Option<iced_image::Handle>,
    submenu_backdrop_kind: Option<ContextSubmenuKind>,
    paste_available: bool,
}

#[derive(Clone, Debug)]
struct AddressEditState {
    pane: PaneId,
    value: String,
}

#[derive(Clone, Copy, Debug)]
struct TabDragState {
    pane: PaneId,
    tab_index: usize,
    slot: usize,
    start_cursor_x: f32,
    start_cursor_y: f32,
    offset_x: f32,
    dragging: bool,
    dirty: bool,
}

#[derive(Clone, Copy, Debug)]
struct SidebarSectionDragState {
    section: SidebarSection,
    slot: usize,
    start_cursor_y: f32,
    offset_y: f32,
    dragging: bool,
    dirty: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ContextCommand {
    Paste,
    Copy,
    Cut,
    Refresh,
    NewMenu,
    NewFolder,
    NewTextDocument,
    OpenTerminal,
    Properties,
    Open,
    OpenWith,
    CompressMenu,
    ExtractMenu,
    CompressDialog,
    CompressDefault(ArchiveFormat),
    Extract(ExtractMode),
    Rename,
    Delete,
    DeletePermanent,
    MountDiskImage,
    EjectDrive,
    ScanWithDefender,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ContextMenuTrailing {
    Text(&'static str),
    Icon(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ContextSubmenuKind {
    Archive,
    Extract,
    New,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyboardShortcut {
    Copy,
    Paste,
    Cut,
    Undo,
    Refresh,
    Delete,
    PermanentDelete,
    SelectAll,
    Rename,
    EditAddress,
    Properties,
    GoUp,
    GoBack,
    GoForward,
    Open,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SearchMode {
    Quick,
    Complete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TableColumn {
    Name,
    Type,
    Size,
    Modified,
    Created,
}

#[derive(Clone, Copy, Debug)]
struct VisualViewMetrics {
    cell_width: f32,
    cell_height: f32,
    icon_size: f32,
    preview_height: f32,
    spacing: f32,
    grid_padding: f32,
    tile: bool,
}

#[derive(Clone, Copy, Debug)]
struct VisualLayout {
    metrics: VisualViewMetrics,
    columns: usize,
}

#[derive(Clone, Copy, Debug)]
struct DetailColumnWidths {
    name: f32,
    type_label: f32,
    size: f32,
    modified: f32,
}

impl DetailColumnWidths {
    fn get(self, column: TableColumn) -> f32 {
        match column {
            TableColumn::Name => self.name,
            TableColumn::Type => self.type_label,
            TableColumn::Size => self.size,
            TableColumn::Modified => self.modified,
            TableColumn::Created => DETAIL_DATE_MIN_WIDTH,
        }
    }

    fn total_width(self) -> f32 {
        self.name + self.type_label + self.size + self.modified + DETAIL_DATE_MIN_WIDTH
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct ColumnWidthOverrides {
    name: Option<f32>,
    type_label: Option<f32>,
    size: Option<f32>,
    modified: Option<f32>,
}

impl ColumnWidthOverrides {
    fn get(self, column: TableColumn) -> Option<f32> {
        match column {
            TableColumn::Name => self.name,
            TableColumn::Type => self.type_label,
            TableColumn::Size => self.size,
            TableColumn::Modified => self.modified,
            TableColumn::Created => None,
        }
    }

    fn set(&mut self, column: TableColumn, width: f32) {
        match column {
            TableColumn::Name => self.name = Some(width),
            TableColumn::Type => self.type_label = Some(width),
            TableColumn::Size => self.size = Some(width),
            TableColumn::Modified => self.modified = Some(width),
            TableColumn::Created => {}
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum ResizeDrag {
    Sidebar {
        start_x: f32,
        start_width: f32,
    },
    Split {
        start_x: f32,
        start_ratio: f32,
    },
    Column {
        pane: PaneId,
        column: TableColumn,
        start_x: f32,
        start_width: f32,
    },
    Preview {
        pane: PaneId,
        start_x: f32,
        start_width: f32,
    },
}

#[derive(Clone, Debug)]
struct RenameState {
    pane: PaneId,
    path: PathBuf,
    value: String,
    editor: text_editor::Content,
    extension: Option<String>,
    select_end: usize,
}

#[derive(Clone, Debug)]
struct PendingPermanentDelete {
    pane: PaneId,
    paths: Vec<PathBuf>,
}

/// A transfer waits here until the user chooses how all detected top-level
/// name collisions should be handled. The transfer queue already applies the
/// selected policy atomically while it reserves each destination.
#[derive(Clone, Debug)]
struct PendingTransferConflict {
    pane: PaneId,
    sources: Vec<PathBuf>,
    destination: PathBuf,
    kind: TransferKind,
    clear_clipboard: bool,
    conflicts: Vec<PathBuf>,
}

#[derive(Clone, Debug)]
struct FileClipboardState {
    paths: Vec<PathBuf>,
    cut: bool,
}

#[derive(Clone, Debug)]
enum UndoAction {
    Copy {
        pane: PaneId,
        targets: Vec<PathBuf>,
    },
    Move {
        pane: PaneId,
        items: Vec<TransferCompletedRoot>,
    },
    Trash {
        pane: PaneId,
        records: Vec<operations::TrashUndoRecord>,
    },
}

impl UndoAction {
    fn pane(&self) -> PaneId {
        match self {
            Self::Copy { pane, .. } | Self::Move { pane, .. } | Self::Trash { pane, .. } => *pane,
        }
    }

    fn refresh_directories(&self) -> Vec<PathBuf> {
        let mut directories = Vec::new();
        let mut push_parent = |path: &Path| {
            if let Some(parent) = path.parent().map(Path::to_path_buf)
                && !directories.contains(&parent)
            {
                directories.push(parent);
            }
        };
        match self {
            Self::Copy { targets, .. } => {
                for target in targets {
                    push_parent(target);
                }
            }
            Self::Move { items, .. } => {
                for item in items {
                    push_parent(&item.target);
                    push_parent(&item.source);
                }
            }
            Self::Trash { records, .. } => {
                for record in records {
                    push_parent(&record.original_path);
                }
            }
        }
        directories
    }
}

#[derive(Clone, Debug)]
struct EntryClickState {
    pane: PaneId,
    path: PathBuf,
    at: Instant,
}

enum IcedImageState {
    Loading,
    Ready(iced_image::Handle),
    Missing,
}

#[derive(Clone, Debug)]
struct IcedRgbaImage {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

#[derive(Clone, Debug)]
enum IcedImageKey {
    Thumbnail(PathBuf),
    Preview(PathBuf),
    NativeIcon(PathBuf),
}

#[derive(Clone, Debug)]
enum IcedImageJob {
    Thumbnail {
        path: PathBuf,
        max_bytes: usize,
        allow_default_resource: bool,
    },
    Preview {
        path: PathBuf,
    },
    NativeIcon {
        cache_key: PathBuf,
        path: PathBuf,
        is_directory: bool,
        size: u32,
    },
}

#[derive(Clone, Debug)]
struct IcedImageLoadResult {
    key: IcedImageKey,
    image: Option<IcedRgbaImage>,
}

struct PdfPreviewPage {
    index: usize,
    handle: iced_image::Handle,
    aspect_ratio: f32,
}

struct PdfPreviewState {
    path: PathBuf,
    page_count: Option<usize>,
    pages: Vec<PdfPreviewPage>,
    current_page: usize,
    loading: bool,
}

/// Stateful, read-only text preview. `TextEditor::Content` retains the cursor,
/// selection, and scroll position so its native copy behaviour remains
/// available without allowing the source file to be edited.
struct TextPreviewState {
    path: PathBuf,
    content: text_editor::Content,
}

impl std::fmt::Debug for TextPreviewState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TextPreviewState")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug)]
struct PdfPreviewLoadResult {
    pane: PaneId,
    path: PathBuf,
    page_index: usize,
    page_count: Option<usize>,
    image: Option<IcedRgbaImage>,
}

fn pdf_preview_page_height(panel_width: f32, aspect_ratio: f32) -> f32 {
    let image_width = (panel_width - 40.0).max(180.0);
    (image_width / aspect_ratio.max(0.1)).clamp(260.0, 920.0)
}

struct QueuedTransferState {
    job: TransferJob,
    pane: PaneId,
}

struct ActiveTransferState {
    job: TransferJob,
    pane: PaneId,
    control: TransferControl,
}

struct TransferHistoryState {
    progress: TransferProgress,
    finished_at: Instant,
}

#[derive(Clone, Debug)]
struct ArchiveDialogState {
    pane: PaneId,
    sources: Vec<PathBuf>,
    name: String,
    format: ArchiveFormat,
    method: ArchiveCompressionMethod,
    use_password: bool,
    password: String,
    password_confirmation: String,
    show_password: bool,
    show_password_confirmation: bool,
}

struct ActiveArchiveState {
    job: ArchiveJob,
    pane: PaneId,
    receiver: Receiver<ArchiveProgressMsg>,
    cancel: Arc<AtomicU32>,
    progress: ArchiveProgress,
}

struct ArchiveHistoryState {
    job: ArchiveJob,
    progress: ArchiveProgress,
    state: ArchiveState,
    finished_at: Instant,
}

#[derive(Clone, Debug)]
struct ArchiveDisplayState {
    id: u64,
    destination: PathBuf,
    format: ArchiveFormat,
    state: ArchiveState,
    progress: ArchiveProgress,
}

impl ArchiveDisplayState {
    fn new(job: &ArchiveJob, state: ArchiveState, progress: ArchiveProgress) -> Self {
        Self {
            id: job.id,
            destination: job.destination.clone(),
            format: job.format,
            state,
            progress,
        }
    }
}

#[derive(Clone, Debug)]
struct RubberBandSelection {
    pane: PaneId,
    start: Point,
    current: Point,
    base_selected: HashSet<PathBuf>,
}

#[derive(Clone, Debug)]
struct FileDragState {
    source_pane: PaneId,
    source_index: usize,
    sources: Vec<PathBuf>,
    collapse_selection_on_click: bool,
    start_pane_point: Option<Point>,
    start_cursor: Option<Point>,
    drop_target: Option<(PaneId, usize)>,
    sidebar_destination: Option<(PaneId, PathBuf)>,
    dragging: bool,
}

#[derive(Clone, Debug)]
struct TransferDisplayState {
    id: u64,
    kind: TransferKind,
    state: TransferState,
    current_name: String,
    copied_bytes: u64,
    total_bytes: u64,
    files_done: usize,
    total_files: usize,
    bytes_per_second: f64,
}

impl TransferDisplayState {
    fn from_progress(progress: TransferProgress) -> Self {
        let target = progress
            .destination
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_else(|| progress.destination.to_str().unwrap_or(""));
        let current_name = if progress.current_name.is_empty() {
            target.to_string()
        } else {
            progress.current_name.clone()
        };

        Self {
            id: progress.job_id,
            kind: progress.kind,
            state: progress.state,
            current_name,
            copied_bytes: progress.copied_bytes,
            total_bytes: progress.total_bytes,
            files_done: progress.files_done,
            total_files: progress.total_files,
            bytes_per_second: progress.bytes_per_second,
        }
    }
}

#[derive(Debug)]
struct PaneState {
    entries: Vec<FileEntry>,
    folder_entries: Option<Vec<FileEntry>>,
    entries_epoch: u64,
    display_order: RefCell<DisplayOrderCache>,
    search_text: String,
    search_mode: SearchMode,
    selected: HashSet<PathBuf>,
    selection_anchor: Option<usize>,
    // These overrides exist only while displaying This PC or the network
    // root. They keep that special presentation from overwriting the view and
    // grouping selected for ordinary folders in the same tab.
    fixed_root_view_override: Option<ViewMode>,
    fixed_root_group_override: Option<GroupMode>,
    fixed_root_group_ascending_override: Option<bool>,
    group_mode: GroupMode,
    group_ascending: bool,
    sort_column: TableColumn,
    sort_ascending: bool,
    loading: bool,
    status: String,
    request_id: u64,
    search_cancel: Option<Arc<AtomicBool>>,
    search_receiver: Option<Receiver<crate::fs::search::SearchEvent>>,
    recursive_search_active: bool,
    search_progress_phase: f32,
    scrollbar_horizontal_hovered: bool,
    scrollbar_vertical_hovered: bool,
    scrollbar_reveal_progress: f32,
    scrollbar_reveal_until: Option<Instant>,
    has_vertical_overflow: bool,
    render_limit: usize,
    scroll_offset_y: f32,
    column_widths: ColumnWidthOverrides,
    text_preview: Option<TextPreviewState>,
}

impl Default for PaneState {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            folder_entries: None,
            entries_epoch: 0,
            display_order: RefCell::new(DisplayOrderCache::default()),
            search_text: String::new(),
            search_mode: SearchMode::Quick,
            selected: HashSet::new(),
            selection_anchor: None,
            fixed_root_view_override: None,
            fixed_root_group_override: None,
            fixed_root_group_ascending_override: None,
            group_mode: GroupMode::None,
            group_ascending: true,
            sort_column: TableColumn::Name,
            sort_ascending: true,
            loading: false,
            status: String::from("0 elements"),
            request_id: 0,
            search_cancel: None,
            search_receiver: None,
            recursive_search_active: false,
            search_progress_phase: 0.0,
            scrollbar_horizontal_hovered: false,
            scrollbar_vertical_hovered: false,
            scrollbar_reveal_progress: 0.0,
            scrollbar_reveal_until: None,
            has_vertical_overflow: false,
            render_limit: INITIAL_RENDER_LIMIT,
            scroll_offset_y: 0.0,
            column_widths: ColumnWidthOverrides::default(),
            text_preview: None,
        }
    }
}

impl PaneState {
    fn mark_entries_changed(&mut self) {
        self.entries_epoch = self.entries_epoch.wrapping_add(1);
        self.display_order.borrow_mut().signature = None;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DisplayOrderSignature {
    entries_epoch: u64,
    group_mode: GroupMode,
    group_ascending: bool,
    sort_column: TableColumn,
    sort_ascending: bool,
}

#[derive(Debug, Default)]
struct DisplayOrderCache {
    signature: Option<DisplayOrderSignature>,
    indices: Vec<usize>,
}

#[derive(Clone, Debug)]
struct SplitRuntime {
    primary_tabs: Vec<usize>,
    secondary_tabs: Vec<usize>,
    secondary_tab: usize,
    focused: SplitFocus,
    ratio: f32,
}

#[derive(Clone, Debug)]
struct SidebarItem {
    label: String,
    target: SidebarTarget,
    icon: &'static str,
}

#[derive(Clone, Debug)]
enum SidebarTarget {
    Navigate(Option<PathBuf>),
    Disabled,
}

struct BExplorerIced {
    config: AppConfig,
    tabs: Vec<TabState>,
    active_tab: usize,
    split: Option<SplitRuntime>,
    primary: PaneState,
    secondary: PaneState,
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
    color_picker_fade_progress: f32,
    context_archive_submenu: bool,
    context_extract_submenu: bool,
    context_new_submenu: bool,
    context_archive_parent_hovered: bool,
    context_archive_submenu_hovered: bool,
    context_new_parent_hovered: bool,
    context_new_submenu_hovered: bool,
    pane_pointer: Option<(PaneId, Point)>,
    current_modifiers: keyboard::Modifiers,
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
    transfer_history: VecDeque<TransferHistoryState>,
    active_archives: HashMap<u64, ActiveArchiveState>,
    archive_history: VecDeque<ArchiveHistoryState>,
    defender_rx: Option<Receiver<DefenderMessage>>,
    defender_cancel: Option<Arc<AtomicBool>>,
    defender_progress: Option<DefenderProgress>,
    defender_summary: Option<DefenderSummary>,
    rename_dialog: Option<RenameState>,
    archive_dialog: Option<ArchiveDialogState>,
    permanent_delete_dialog: Option<PendingPermanentDelete>,
    transfer_conflict_dialog: Option<PendingTransferConflict>,
    pending_new_folder_rename: Option<(PaneId, PathBuf)>,
    pending_file_operations: HashSet<PaneId>,
    // A text-input submit and the global key listener can observe the same
    // Enter in either order. Keep Enter from opening the just-renamed item
    // while that event finishes propagating.
    suppress_open_after_rename_until: Option<Instant>,
    rubber_band: Option<RubberBandSelection>,
    file_drag: Option<FileDragState>,
    file_drag_suppressed_click: Option<(PaneId, usize)>,
    native_external_drag_active: bool,
    pending_external_file_drops: Vec<PathBuf>,
    external_file_drop_flush_queued: bool,
    tab_drag: Option<TabDragState>,
    sidebar_section_drag: Option<SidebarSectionDragState>,
    color_rgb_inputs: [String; 3],
    sidebar_progress: f32,
    preview_panel_progress: f32,
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
    shortcut_capture: Option<ShortcutAction>,
    sidebar_visible: bool,
    window_maximized: bool,
    main_window_id: Option<window::Id>,
    transfer_window_id: Option<window::Id>,
    transfer_window_item_count: usize,
    archive_window_id: Option<window::Id>,
    archive_window_item_count: usize,
}

pub fn run() -> iced::Result {
    iced::daemon(
        BExplorerIced::new,
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
            height: 116.0_f32.min((window_height - TITLE_HEIGHT).max(0.0)),
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

    fn new() -> (Self, Task<Message>) {
        let mut config = AppConfig::load();
        if !available_vibrancy_modes().contains(&config.vibrancy) {
            config.vibrancy = VibrancyMode::None;
        }
        config.vibrancy_active = config.vibrancy != VibrancyMode::None;
        let session = AppSession::load();
        let mut tabs = session.tabs;
        if tabs.is_empty() {
            tabs.push(TabState::new(None));
        }
        let active_tab = session.active_tab.min(tabs.len().saturating_sub(1));
        let split = session.split.and_then(|split| {
            if split.tab_a < tabs.len() && split.tab_b < tabs.len() && split.tab_a != split.tab_b {
                Some(SplitRuntime {
                    primary_tabs: normalize_tabs(split.primary_tabs, split.tab_a, tabs.len()),
                    secondary_tabs: normalize_tabs(split.secondary_tabs, split.tab_b, tabs.len()),
                    secondary_tab: split.tab_b,
                    focused: split.focused,
                    ratio: split.ratio.clamp(SPLIT_MIN_RATIO, SPLIT_MAX_RATIO),
                })
            } else {
                None
            }
        });

        let (transfer_tx, transfer_rx) = mpsc::channel();
        let initial_size = Size::new(config.window_size[0], config.window_size[1]);
        let color_rgb_inputs = accent_rgb_strings(config.accent_color);
        let preview_panel_pane = config.show_preview_panel.then_some(PaneId::Primary);
        let preview_panel_progress = if config.show_preview_panel { 1.0 } else { 0.0 };
        let mut app = Self {
            sidebar_visible: config.sidebar_visible,
            sidebar_progress: if config.sidebar_visible { 1.0 } else { 0.0 },
            preview_panel_progress,
            window_size: initial_size,
            cursor_position: Point::new(0.0, 0.0),
            resize_drag: None,
            config,
            tabs,
            active_tab,
            split,
            primary: PaneState::default(),
            secondary: PaneState::default(),
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
            popup_fade_progress: 1.0,
            color_picker_fade_progress: 1.0,
            context_archive_submenu: false,
            context_extract_submenu: false,
            context_new_submenu: false,
            context_archive_parent_hovered: false,
            context_archive_submenu_hovered: false,
            context_new_parent_hovered: false,
            context_new_submenu_hovered: false,
            pane_pointer: None,
            current_modifiers: keyboard::Modifiers::default(),
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
            transfer_history: VecDeque::new(),
            active_archives: HashMap::new(),
            archive_history: VecDeque::new(),
            defender_rx: None,
            defender_cancel: None,
            defender_progress: None,
            defender_summary: None,
            rename_dialog: None,
            archive_dialog: None,
            permanent_delete_dialog: None,
            transfer_conflict_dialog: None,
            pending_new_folder_rename: None,
            pending_file_operations: HashSet::new(),
            suppress_open_after_rename_until: None,
            rubber_band: None,
            file_drag: None,
            file_drag_suppressed_click: None,
            native_external_drag_active: false,
            pending_external_file_drops: Vec::new(),
            external_file_drop_flush_queued: false,
            tab_drag: None,
            sidebar_section_drag: None,
            color_rgb_inputs,
            settings_open: false,
            shortcuts_open: false,
            shortcut_capture: None,
            color_picker_open: false,
            accent_plane_dragging: false,
            accent_plane_pointer: None,
            accent_hue_dragging: false,
            accent_hue_pointer: None,
            window_maximized: false,
            main_window_id: None,
            transfer_window_id: None,
            transfer_window_item_count: 0,
            archive_window_id: None,
            archive_window_item_count: 0,
        };

        app.reset_fixed_root_presentation(PaneId::Primary);
        if app.split.is_some() {
            app.reset_fixed_root_presentation(PaneId::Secondary);
        }
        let (main_window_id, open_main_window) = window::open(main_window_settings(initial_size));
        app.main_window_id = Some(main_window_id);

        let sidebar_icons = app.queue_sidebar_icons();
        let mut tasks = vec![
            open_main_window.map(Message::MainWindowOpened),
            app.start_load(PaneId::Primary),
            sidebar_icons,
        ];
        if matches!(app.config.theme, ThemePreference::System) {
            tasks.push(iced::system::theme().map(Message::SystemThemeChanged));
        }
        if app.split.is_some() {
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
        let vibrancy = self.config.vibrancy;
        let vibrancy_intensity = self.config.vibrancy_intensity;
        let dark = self.is_dark_theme();
        let cancel_autoplay = self.main_window_id == Some(id);
        window::run(id, move |native_window| {
            if let (Ok(window_handle), Ok(display_handle)) = (
                native_window.window_handle(),
                native_window.display_handle(),
            ) {
                let _ =
                    crate::platform::apply_window_corners(&window_handle, &display_handle, radius);
                if cancel_autoplay {
                    #[cfg(target_os = "windows")]
                    let _ = crate::platform::install_autoplay_cancel(&window_handle);
                }
            }
            if apply_vibrancy {
                let active = crate::platform::apply_window_vibrancy(
                    native_window,
                    vibrancy,
                    vibrancy_intensity,
                    dark,
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
        let sidebar_tick = if self.sidebar_animation_active() {
            Subscription::run(sidebar_animation_tick_stream)
        } else {
            Subscription::none()
        };
        let preview_panel_tick = if self.preview_panel_animation_active() {
            Subscription::run(preview_panel_animation_tick_stream)
        } else {
            Subscription::none()
        };
        let popup_fade_tick = if self.popup_fade_animation_active() {
            Subscription::run(popup_fade_animation_tick_stream)
        } else {
            Subscription::none()
        };
        let scrollbar_tick = if self.scrollbar_animation_active() {
            Subscription::run(scrollbar_animation_tick_stream)
        } else {
            Subscription::none()
        };
        // Wayland delivers file drops through the data-device queue instead
        // of Winit's `FileDropped` event. Pump it lightly while idle too.
        let external_drag_tick =
            Subscription::run_with(self.native_external_drag_active, external_drag_tick_stream);
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

        Subscription::batch([
            window::resize_events().map(|(id, size)| Message::WindowResized(id, size)),
            window::close_events().map(Message::WindowClosed),
            keyboard_events,
            pointer_events,
            transfer_tick,
            sidebar_tick,
            preview_panel_tick,
            popup_fade_tick,
            scrollbar_tick,
            external_drag_tick,
            search_tick,
            system_theme_changes,
        ])
    }
}
