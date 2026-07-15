#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum PaneId {
    Primary,
    Secondary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StartupInitialLoad {
    Pane { pane: PaneId, request_id: u64 },
    StorageRoot,
}

#[derive(Debug, Default)]
struct StartupState {
    restoration_complete: bool,
    first_frame_presented: bool,
    busy_threshold_reached: bool,
    pending_initial_loads: Vec<StartupInitialLoad>,
}

impl StartupState {
    fn mark_restoration_complete(&mut self) {
        self.restoration_complete = true;
    }

    fn wait_for_initial_load(&mut self, pane: PaneId, request_id: u64, storage_root: bool) {
        let load = if storage_root {
            StartupInitialLoad::StorageRoot
        } else {
            StartupInitialLoad::Pane { pane, request_id }
        };
        if !self.pending_initial_loads.contains(&load) {
            self.pending_initial_loads.push(load);
        }
    }

    fn complete_pane_load(&mut self, pane: PaneId, request_id: u64) {
        self.pending_initial_loads.retain(|load| {
            *load
                != StartupInitialLoad::Pane {
                    pane,
                    request_id,
                }
        });
    }

    fn complete_storage_root_load(&mut self) {
        self.pending_initial_loads
            .retain(|load| *load != StartupInitialLoad::StorageRoot);
    }

    fn mark_first_frame_presented(&mut self) {
        self.first_frame_presented = true;
    }

    fn mark_busy_threshold_reached(&mut self) {
        self.busy_threshold_reached = true;
    }

    fn waiting_for_first_frame(&self) -> bool {
        !self.first_frame_presented
    }

    fn is_complete(&self) -> bool {
        self.restoration_complete
            && self.first_frame_presented
            && self.pending_initial_loads.is_empty()
    }

    fn show_busy_cursor(&self) -> bool {
        self.busy_threshold_reached && !self.is_complete()
    }
}

#[cfg(test)]
mod startup_state_tests {
    use super::*;

    #[test]
    fn busy_cursor_waits_for_threshold_and_every_startup_condition() {
        let mut startup = StartupState::default();
        startup.wait_for_initial_load(PaneId::Primary, 7, false);
        startup.mark_restoration_complete();
        startup.mark_first_frame_presented();

        assert!(!startup.show_busy_cursor());

        startup.mark_busy_threshold_reached();
        assert!(startup.show_busy_cursor());

        startup.complete_pane_load(PaneId::Primary, 6);
        assert!(startup.show_busy_cursor());

        startup.complete_pane_load(PaneId::Primary, 7);
        assert!(startup.is_complete());
        assert!(!startup.show_busy_cursor());
    }

    #[test]
    fn one_storage_enumeration_completes_the_shared_storage_root() {
        let mut startup = StartupState::default();
        startup.wait_for_initial_load(PaneId::Primary, 1, true);
        startup.wait_for_initial_load(PaneId::Secondary, 2, true);

        assert_eq!(startup.pending_initial_loads.len(), 1);

        startup.complete_storage_root_load();
        assert!(startup.pending_initial_loads.is_empty());
    }

    #[test]
    fn restoration_and_a_presented_frame_are_both_required() {
        let mut startup = StartupState::default();
        startup.mark_busy_threshold_reached();

        assert!(startup.show_busy_cursor());

        startup.mark_restoration_complete();
        assert!(startup.show_busy_cursor());

        startup.mark_first_frame_presented();
        assert!(startup.is_complete());
        assert!(!startup.show_busy_cursor());
    }
}

#[derive(Clone, Copy, Debug)]
enum ScrollbarAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Debug)]
enum Message {
    Loaded(PaneId, u64, Result<Vec<FileEntry>, String>),
    NetworkDiscoveryEntries(PaneId, u64, Vec<FileEntry>),
    NetworkDiscoveryAddresses(PaneId, u64, Vec<String>),
    SidebarStorageLoaded(Result<Vec<FileEntry>, String>),
    StorageDevicesChanged,
    RefreshStorageAfterDeviceChange,
    CloseTab(PaneId, usize),
    NewTab(PaneId),
    StartTabDrag(PaneId, usize),
    StartSidebarSectionDrag(SidebarSection),
    ToggleMenu,
    OpenShortcuts,
    CloseShortcuts,
    OpenAbout,
    CloseAbout,
    OpenRepository,
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
    SidebarPointerEntered,
    SidebarPointerExited,
    StartupBusyThresholdReached,
    AnimationFrame(Instant),
    ScrollbarHover(PaneId, ScrollbarAxis, bool),
    ScrollbarAnimationTick,
    AsyncProgressTick,
    ToggleSplit,
    Navigate(PaneId, Option<PathBuf>),
    BeginAddressEdit(PaneId),
    AddressEditReady(PaneId),
    CheckAddressFocus(window::Id),
    AddressFocusChecked(PaneId, u64, bool),
    AddressChanged(String),
    SubmitAddress(PaneId),
    RowPressed(PaneId, usize),
    OpenSidebarDriveContext(PaneId, usize),
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
    FormatVolumeLabelChanged(String),
    SetFormatFileSystem(String),
    SetFormatAllocationUnitSize(String),
    ToggleFormatQuick,
    ToggleFormatEraseConfirmation,
    ConfirmFormatDialog,
    CancelFormatDialog,
    FormatFinished(
        PaneId,
        PathBuf,
        Result<operations::FormatDriveOutcome, String>,
    ),
    DismissErrorDialog,
    CancelArchive(u64),
    TrashFinished(PaneId, Vec<PathBuf>, Result<operations::TrashDeleteOutcome, String>),
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
    ContextMenuDataResolved(ContextMenuState, bool, Vec<shell::OpenWithApplication>),
    ContextBackdropCaptured(ContextMenuState, window::Screenshot),
    ContextBackdropPrepared(ContextMenuState, Option<iced_image::Handle>),
    ContextSubmenuBackdropCaptured(u64, ContextSubmenuKind, window::Screenshot),
    ContextSubmenuBackdropPrepared(u64, ContextSubmenuKind, Option<iced_image::Handle>),
    PopupBackdropCaptured(PopupBackdropTarget, window::Screenshot),
    PopupBackdropPrepared(PopupBackdropTarget, Option<iced_image::Handle>),
    TitleMenuBackdropsPrepared(Option<iced_image::Handle>, Option<iced_image::Handle>),
    CloseContextMenu,
    ContextArchiveParentEnter,
    ContextOpenWithParentEnter,
    ContextOpenWithParentExit,
    ContextOpenWithSubmenuEnter,
    ContextOpenWithSubmenuExit,
    CloseContextOpenWithSubmenuIfUnhovered,
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
    KeyPressed(
        window::Id,
        keyboard::Key,
        keyboard::key::Physical,
        keyboard::Modifiers,
    ),
    KeyboardModifiersChanged(keyboard::Modifiers),
    RenameChanged(String),
    RenameEdited(text_editor::Action),
    RenameSelected(PaneId),
    ConfirmRename,
    RenameFinished(RenameState, Result<PathBuf, String>),
    CancelRename,
    ConfirmPermanentDelete,
    PermanentDeleteFinished(PaneId, Vec<PathBuf>, Result<usize, String>),
    CancelPermanentDelete,
    DiskImageMounted(PaneId, PathBuf, Result<PathBuf, String>),
    DriveEjected(PaneId, PathBuf, Result<(), String>),
    OpenWithChooserFinished(PaneId, Result<(), String>),
    CancelDefenderScan,
    CloseDefenderPanel,
    RemediateDefenderThreats,
    #[cfg(target_os = "windows")]
    DefenderThreatRemediationFinished(Result<usize, String>),
    OpenWindowsSecurity,
    PortableClipboardPrepared(PaneId, Result<Vec<PathBuf>, String>),
    PortableOpenPrepared(PaneId, Result<PathBuf, String>),
    PortableDeleteFinished(PaneId, Result<usize, String>),
    PortableTransferFinished(PaneId, Vec<PathBuf>, bool, Result<usize, String>),
    ResolveTransferConflict(ConflictPolicy),
    CancelTransferConflict,
    ConfirmElevatedTransfer,
    CancelElevatedTransfer,
    ElevatedTransferFinished(PaneId, TransferJob, Result<ElevatedTransferResult, String>),
    ConfirmElevatedDelete,
    CancelElevatedDelete,
    ElevatedDeleteFinished(PaneId, bool, u64, Result<usize, String>),
    ConfirmElevatedFileAction,
    CancelElevatedFileAction,
    ElevatedFileActionFinished(
        PaneId,
        operations::ElevatedFileAction,
        Result<PathBuf, String>,
    ),
    MainWindowOpened(window::Id),
    TransferWindowOpened(window::Id),
    ArchiveWindowOpened(window::Id),
    #[cfg(target_os = "windows")]
    DefenderWindowOpened(window::Id),
    DefenderThreatsWindowOpened(window::Id),
    #[cfg(target_os = "linux")]
    Properties(properties::PropertiesMessage),
    ReopenTransferWindow(window::Id, Option<Point>),
    ReopenArchiveWindow(window::Id, Option<Point>),
    WindowCloseRequested(window::Id),
    WindowClosed(window::Id),
    PollTransfers,
    TransferWindowDrag,
    TransferWindowMinimize,
    ArchiveWindowDrag,
    ArchiveWindowMinimize,
    DefenderWindowDrag,
    DefenderWindowMinimize,
    DefenderThreatsWindowDrag,
    DefenderThreatsWindowMinimize,
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
    SidebarDrive(usize),
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
    About,
    ColorPicker,
    Rename(RenameState),
    PermanentDelete(PendingPermanentDelete),
    Archive(ArchiveDialogState),
    Format(FormatDialogState),
    Error(ErrorDialogState),
    TransferConflict(PendingTransferConflict),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingPopupClose {
    FloatingMenus,
    Shortcuts,
    Settings,
    About,
    ColorPicker,
    ArchiveDialog,
    FormatDialog,
    ErrorDialog,
    PermanentDelete,
    TransferConflict,
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
    open_with_applications: Vec<shell::OpenWithApplication>,
}

#[derive(Clone, Debug)]
struct AddressEditState {
    pane: PaneId,
    value: String,
    focus_ready: bool,
    focus_check_id: u64,
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
    OpenWithMenu,
    OpenWithApplication(usize),
    OpenFileLocation,
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
    FormatDrive,
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
    OpenWith,
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
enum KeyboardMenu {
    Title,
    Show,
    View(PaneId),
    Group(PaneId),
    Search(PaneId),
    New(PaneId),
    Context,
    ContextOpenWith,
    ContextArchive,
    ContextExtract,
    ContextNew,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct KeyboardMenuSelection {
    menu: KeyboardMenu,
    index: usize,
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
struct PendingElevatedTransfer {
    pane: PaneId,
    job: TransferJob,
    error: String,
}

#[derive(Clone, Debug)]
struct PendingElevatedDelete {
    pane: PaneId,
    paths: Vec<PathBuf>,
    permanent: bool,
    transfer_id: u64,
    error: String,
}

#[derive(Clone, Debug)]
struct PendingElevatedFileAction {
    pane: PaneId,
    action: operations::ElevatedFileAction,
    error: String,
}

#[derive(Clone, Debug)]
struct ActiveDeleteState {
    id: u64,
    pane: PaneId,
    paths: Vec<PathBuf>,
    permanent: bool,
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
    ApplicationIcon {
        cache_key: PathBuf,
        application: shell::OpenWithApplication,
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

#[derive(Clone, Debug)]
struct FormatDialogState {
    pane: PaneId,
    path: PathBuf,
    display_name: String,
    capacity: Option<u64>,
    file_systems: Vec<String>,
    file_system: String,
    drive_identity: Option<operations::FormatDriveIdentity>,
    volume_label: String,
    allocation_unit_size: String,
    quick_format: bool,
    confirm_erase: bool,
}

#[derive(Clone, Debug)]
struct ErrorDialogState {
    title: String,
    message: String,
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
    kind: TransferDisplayKind,
    state: TransferState,
    current_name: String,
    copied_bytes: u64,
    total_bytes: u64,
    files_done: usize,
    total_files: usize,
    bytes_per_second: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TransferDisplayKind {
    Copy,
    Move,
    Trash,
    PermanentDelete,
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
            kind: match progress.kind {
                TransferKind::Copy => TransferDisplayKind::Copy,
                TransferKind::Move => TransferDisplayKind::Move,
            },
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
    formatting: bool,
    formatting_path: Option<PathBuf>,
    mounting_disk_image: bool,
    status: String,
    request_id: u64,
    network_discovery_pending: usize,
    search_cancel: Option<Arc<AtomicBool>>,
    search_receiver: Option<Receiver<crate::fs::search::SearchEvent>>,
    recursive_search_active: bool,
    search_progress_phase: f32,
    progress_animation_started: Instant,
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
            formatting: false,
            formatting_path: None,
            mounting_disk_image: false,
            status: String::from("0 elements"),
            request_id: 0,
            network_discovery_pending: 0,
            search_cancel: None,
            search_receiver: None,
            recursive_search_active: false,
            search_progress_phase: 0.0,
            progress_animation_started: Instant::now(),
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
    streaming_search: bool,
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
    context_drive_index: Option<usize>,
}

#[derive(Clone, Debug)]
enum SidebarTarget {
    Navigate(Option<PathBuf>),
    Disabled,
}
