use std::collections::{BTreeSet, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering as AtomicOrdering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;
use eframe::egui::TextureOptions;

use crate::app::commands::AppCommand;
use crate::app::config::{
    AppConfig, GroupMode, ShortcutAction, ShortcutBinding, ShortcutConfig, SidebarSection,
    ThemePreference, VibrancyMode, ViewMode,
};
use crate::app::session::{AppSession, SplitFocus, SplitSession, SplitSide, TabState};
use crate::fs::archive::{
    self, ArchiveCompressionMethod, ArchiveFormat, ArchiveJob, ArchiveJobKind, ArchiveProgressMsg,
    ArchiveState, ExtractMode,
};
use crate::fs::defender::{
    self, DefenderJob, DefenderMessage, DefenderProgress, DefenderScanState, DefenderSummary,
    ElevatedDefenderAction,
};
use crate::fs::explorer::{self, EntryKind, FileEntry};
use crate::fs::search::{self, SearchOptions};
use crate::fs::transfer_queue::{
    self, ConflictPolicy, TransferControl, TransferJob, TransferKind, TransferMessage,
    TransferProgress, TransferState,
};
use crate::fs::{operations, portable};
use crate::utils::errors::Result;

mod entry_utils;
mod navigation;
mod network;
mod selection;
mod thumbnail;
mod types;

const STORAGE_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

type StorageSnapshot = (Vec<FileEntry>, Vec<explorer::PortableDevice>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SpecialRoot {
    Storage,
    Network,
}

fn file_group_from_config(group: GroupMode) -> FileGroup {
    match group {
        GroupMode::None => FileGroup::None,
        GroupMode::Name => FileGroup::Name,
        GroupMode::Type => FileGroup::Type,
        GroupMode::TotalSize => FileGroup::TotalSize,
        GroupMode::FreeSpace => FileGroup::FreeSpace,
    }
}

fn config_group_from_file(group: FileGroup) -> GroupMode {
    match group {
        FileGroup::None => GroupMode::None,
        FileGroup::Name => GroupMode::Name,
        FileGroup::Type => GroupMode::Type,
        FileGroup::TotalSize => GroupMode::TotalSize,
        FileGroup::FreeSpace => GroupMode::FreeSpace,
    }
}

fn default_compress_dialog_name(
    sources: &[PathBuf],
    destination_dir: &Path,
    format: ArchiveFormat,
) -> String {
    let raw_name = if sources.len() == 1 {
        sources
            .first()
            .and_then(|path| path.file_stem().or_else(|| path.file_name()))
            .and_then(|name| name.to_str())
            .filter(|name| !name.trim().is_empty())
            .map(ToOwned::to_owned)
    } else {
        destination_dir
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.trim().is_empty())
            .map(ToOwned::to_owned)
    }
    .unwrap_or_else(|| "Archive".into());

    let file_name = archive_file_name_from_input(&raw_name, format);
    file_name
        .strip_suffix(format!(".{}", format.extension()).as_str())
        .unwrap_or(file_name.as_str())
        .to_string()
}

fn load_storage_snapshot() -> Result<StorageSnapshot> {
    let started_at = Instant::now();
    let entries = explorer::list_storage_entries()?;
    let portable_devices = explorer::list_portable_devices_for_storage(&entries);
    let elapsed = started_at.elapsed();
    if elapsed > Duration::from_millis(750) {
        crate::utils::log::info(format!("Storage snapshot took {elapsed:?}"));
    }
    Ok((entries, portable_devices))
}

pub enum PreviewContentRef {
    Images {
        images: Vec<(egui::TextureId, egui::Vec2)>,
        loading: bool,
        page_count: Option<usize>,
    },
    Text(String),
    Loading,
}

use types::{
    ActiveArchive, ActiveTransfer, ArchiveHistoryItem, FileClipboard, LoadMessage, NativeIconState,
    OperationMessage, PortableThumbnailJob, PreviewCacheState, PreviewJob, PreviewMessage,
    SearchMessage, TabSearchState, ThumbnailMessage, ThumbnailState, TransferHistoryItem,
};

use entry_utils::*;
use navigation::*;
use network::*;
use thumbnail::*;
use types::TypeSelectState;
#[allow(unused_imports)]
pub use types::{
    ArchiveDisplayItem, ArchivePasswordDialog, ArchiveProgressState, CompressDialog, DragPrimary,
    DragSelection, FileDrag, FileDragFeedback, FileGroup, FileSort, PaneState,
    PendingTransferConflict, RenameDialog, SearchMode, SidebarOpen, SplitState, TabDrag,
    TransferDisplayItem,
};

pub struct BExplorerApp {
    pub config: AppConfig,
    pub tabs: Vec<TabState>,
    tab_search: Vec<TabSearchState>,
    pub active_tab: usize,
    pub entries: Vec<FileEntry>,
    pub storage_entries: Vec<FileEntry>,
    pub portable_devices: Vec<explorer::PortableDevice>,
    pub selected: BTreeSet<PathBuf>,
    pub selection_anchor: Option<PathBuf>,
    pub selection_focus: Option<PathBuf>,
    pub filter: String,
    pub search_mode: SearchMode,
    pub search_results: Vec<FileEntry>,
    pub searching: bool,
    pub loading: bool,
    pub status_message: String,
    pub error_message: Option<String>,
    pub rename_dialog: Option<RenameDialog>,
    pub compress_dialog: Option<CompressDialog>,
    pub archive_password_dialog: Option<ArchivePasswordDialog>,
    pub pending_rename: Option<PathBuf>,
    pub confirm_permanent_delete: Option<Vec<PathBuf>>,
    pub pending_transfer_conflict: Option<PendingTransferConflict>,
    pub pending_elevated_transfer: Option<TransferJob>,
    pub pending_elevated_operation: Option<operations::ElevatedFileOperation>,
    pub delete_panel_spawned: bool,
    pub command_palette_open: bool,
    pub command_query: String,
    pub options_menu_open: bool,
    pub action_bar_new_menu_open: bool,
    pub options_open: bool,
    pub shortcuts_open: bool,
    pub transfer_panel_minimized: bool,
    pub transfer_panel_spawned: bool,
    pub sidebar_visible: bool,
    pub sidebar_open: SidebarOpen,
    pub sidebar_drag: Option<SidebarSection>,
    pub sidebar_drop_target: Option<(SidebarSection, bool)>,
    pub drag_selection: Option<DragSelection>,
    pub tab_drag: Option<TabDrag>,
    pub file_drag: Option<FileDrag>,
    pub file_drag_folder_rects: Vec<(PathBuf, egui::Rect)>,
    pub split: Option<SplitState>,
    pub other_pane: Option<PaneState>,
    pub focused_pane: usize,
    sort: FileSort,
    sort_ascending: bool,
    group_by: FileGroup,
    group_ascending: bool,
    pub column_widths: [f32; 8],
    next_request_id: u64,
    next_search_request_id: u64,
    next_transfer_id: u64,
    load_rx: Option<Receiver<LoadMessage>>,
    search_rx: Option<Receiver<SearchMessage>>,
    search_cancel: Option<Arc<AtomicBool>>,
    operation_rx: Option<Receiver<OperationMessage>>,
    storage_refresh_rx: Option<Receiver<Result<StorageSnapshot>>>,
    transfer_tx: Sender<TransferMessage>,
    transfer_rx: Receiver<TransferMessage>,
    transfer_queue: VecDeque<TransferJob>,
    active_transfers: HashMap<u64, ActiveTransfer>,
    pub transfer_progress: HashMap<u64, TransferProgress>,
    transfer_history: VecDeque<TransferHistoryItem>,
    max_parallel_transfers: usize,
    thumbnail_tx: Sender<ThumbnailMessage>,
    thumbnail_rx: Receiver<ThumbnailMessage>,
    portable_thumbnail_tx: Sender<PortableThumbnailJob>,
    preview_tx: Sender<PreviewJob>,
    preview_rx: Receiver<PreviewMessage>,
    preview_generation: Arc<AtomicU64>,
    preview_active_path: Option<PathBuf>,
    pub archive_queue: VecDeque<ArchiveJob>,
    active_archives: HashMap<u64, ActiveArchive>,
    pub archive_progress: HashMap<u64, ArchiveProgressState>,
    archive_history: VecDeque<ArchiveHistoryItem>,
    next_archive_id: u64,
    max_parallel_archives: usize,
    pub archive_panel_minimized: bool,
    pub archive_panel_spawned: bool,
    defender_rx: Option<Receiver<DefenderMessage>>,
    defender_cancel: Option<Arc<AtomicBool>>,
    pub defender_progress: Option<DefenderProgress>,
    pub defender_summary: Option<DefenderSummary>,
    pub defender_panel_minimized: bool,
    pub defender_panel_spawned: bool,
    clipboard: Option<FileClipboard>,
    thumbnail_cache: HashMap<PathBuf, ThumbnailState>,
    native_icon_cache: HashMap<PathBuf, NativeIconState>,
    preview_cache: HashMap<PathBuf, PreviewCacheState>,
    preview_text_selection: Option<(PathBuf, String, egui::text::CCursorRange)>,
    pending_select: Option<PathBuf>,
    type_select: Option<TypeSelectState>,
    pub pending_scroll_path: Option<PathBuf>,
    pub path_bar_text_visible: bool,
    pub path_bar_edit_text: String,
    pub path_bar_focus_pending: bool,
    pub path_bar_selection_range: Option<(usize, usize)>,
    pub preview_panel_visible: bool,
    text_input_active: bool,
    paste_shortcut_down: bool,
    pub shortcut_capture: Option<ShortcutAction>,
    pub last_auto_fit_path: Option<PathBuf>,
    pub last_auto_fit_width: Option<f32>,
    pub vibrancy_applied: bool,
    pub vibrancy_dirty: bool,
    last_storage_refresh: Instant,
}

impl PaneState {
    pub fn new(active_tab: usize) -> Self {
        Self {
            active_tab,
            entries: Vec::new(),
            selected: BTreeSet::new(),
            selection_anchor: None,
            selection_focus: None,
            filter: String::new(),
            search_mode: SearchMode::Quick,
            search_results: Vec::new(),
            searching: false,
            loading: false,
            status_message: String::new(),
            rename_dialog: None,
            pending_rename: None,
            action_bar_new_menu_open: false,
            drag_selection: None,
            pending_select: None,
            type_select: None,
            pending_scroll_path: None,
            path_bar_text_visible: false,
            path_bar_edit_text: String::new(),
            path_bar_focus_pending: false,
            path_bar_selection_range: None,
            preview_active_path: None,
            preview_text_selection: None,
            preview_panel_visible: false,
            sort: FileSort::Name,
            sort_ascending: true,
            group_by: FileGroup::None,
            group_ascending: true,
            column_widths: default_column_widths(),
            last_auto_fit_path: None,
            last_auto_fit_width: None,
            next_request_id: 0,
            next_search_request_id: 0,
            load_rx: None,
            search_rx: None,
            search_cancel: None,
        }
    }

    fn filtered_entries_slice(&self) -> &[FileEntry] {
        if self.filter.trim().is_empty() {
            &self.entries
        } else {
            &self.search_results
        }
    }

    fn visible_contains_path(&self, path: &Path) -> bool {
        self.filtered_entries_slice()
            .iter()
            .any(|entry| entry.path == path)
    }

    fn start_folder_load(&mut self, path: Option<PathBuf>, show_hidden: bool) {
        self.next_request_id = self.next_request_id.saturating_add(1);
        let request_id = self.next_request_id;
        let (tx, rx) = mpsc::channel();
        self.loading = true;
        self.status_message = "Loading folder...".into();
        self.load_rx = Some(rx);
        self.entries.clear();
        self.selected.clear();
        self.selection_anchor = None;
        self.selection_focus = None;
        if path.as_deref().is_some_and(explorer::is_network_root_path) {
            spawn_network_root_load(tx, request_id);
            return;
        }

        thread::spawn(move || {
            let result = explorer::list_entries(path.as_deref(), show_hidden)
                .map_err(|error| error.to_string());
            let _ = tx.send(LoadMessage {
                request_id,
                finished: true,
                append: false,
                result,
            });
        });
    }
}

impl BExplorerApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let config = AppConfig::load();
        crate::ui::theme::apply(&cc.egui_ctx, &config);

        let session = AppSession::load();
        let mut tabs = session.tabs;
        if tabs.is_empty() {
            tabs.push(TabState::with_view_mode(
                None,
                Self::view_for_path_from_config(&config, None),
            ));
        } else {
            for tab in &mut tabs {
                if Self::special_root_for_path(tab.path.as_deref()).is_some() {
                    tab.view_mode = Self::view_for_path_from_config(&config, tab.path.as_deref());
                }
            }
        }
        let tab_search = std::iter::repeat_with(TabSearchState::default)
            .take(tabs.len())
            .collect();
        let active_tab = session.active_tab.min(tabs.len().saturating_sub(1));
        let (storage_entries, portable_devices) = load_storage_snapshot().unwrap_or_else(|error| {
            crate::utils::log::error(format!("Storage scan failed: {error}"));
            (Vec::new(), Vec::new())
        });

        let (thumbnail_tx, thumbnail_rx) = mpsc::channel();
        let (portable_thumbnail_tx, portable_thumbnail_rx) =
            mpsc::channel::<PortableThumbnailJob>();
        let (preview_tx, preview_job_rx) = mpsc::channel::<PreviewJob>();
        let (preview_result_tx, preview_rx) = mpsc::channel::<PreviewMessage>();
        let preview_generation = Arc::new(AtomicU64::new(0));
        let sidebar_visible = config.sidebar_visible;
        let preview_panel_visible = config.show_preview_panel;
        let portable_thumbnail_result_tx = thumbnail_tx.clone();
        thread::spawn(move || {
            while let Ok(job) = portable_thumbnail_rx.recv() {
                let image = load_portable_thumbnail_image(
                    &job.path,
                    job.max_bytes,
                    job.allow_default_resource,
                );
                let _ = portable_thumbnail_result_tx.send(ThumbnailMessage {
                    path: job.path,
                    image,
                });
            }
        });
        thread::spawn(move || {
            while let Ok(job) = preview_job_rx.recv() {
                let path = job.entry.path.clone();
                let generation = job.generation;
                crate::preview::render_entry_streaming(&job.entry, job.max_bytes, |content| {
                    preview_result_tx
                        .send(PreviewMessage {
                            path: path.clone(),
                            generation,
                            content,
                        })
                        .is_ok()
                });
            }
        });
        let (transfer_tx, transfer_rx) = mpsc::channel();

        let mut app = Self {
            config,
            tabs,
            tab_search,
            active_tab,
            entries: Vec::new(),
            storage_entries,
            portable_devices,
            selected: BTreeSet::new(),
            selection_anchor: None,
            selection_focus: None,
            filter: String::new(),
            search_mode: SearchMode::Quick,
            search_results: Vec::new(),
            searching: false,
            loading: false,
            status_message: String::new(),
            error_message: None,
            rename_dialog: None,
            compress_dialog: None,
            archive_password_dialog: None,
            pending_rename: None,
            confirm_permanent_delete: None,
            pending_transfer_conflict: None,
            pending_elevated_transfer: None,
            pending_elevated_operation: None,
            delete_panel_spawned: false,
            command_palette_open: false,
            command_query: String::new(),
            options_menu_open: false,
            action_bar_new_menu_open: false,
            options_open: false,
            shortcuts_open: false,
            transfer_panel_minimized: false,
            transfer_panel_spawned: false,
            sidebar_visible,
            sidebar_open: SidebarOpen::default(),
            sidebar_drag: None,
            sidebar_drop_target: None,
            drag_selection: None,
            tab_drag: None,
            file_drag: None,
            file_drag_folder_rects: Vec::new(),
            split: None,
            other_pane: None,
            focused_pane: 0,
            sort: FileSort::Name,
            sort_ascending: true,
            group_by: FileGroup::None,
            group_ascending: true,
            column_widths: default_column_widths(),
            next_request_id: 0,
            next_search_request_id: 0,
            next_transfer_id: 0,
            load_rx: None,
            search_rx: None,
            search_cancel: None,
            operation_rx: None,
            storage_refresh_rx: None,
            transfer_tx,
            transfer_rx,
            transfer_queue: VecDeque::new(),
            active_transfers: HashMap::new(),
            transfer_progress: HashMap::new(),
            transfer_history: VecDeque::new(),
            max_parallel_transfers: 2,
            thumbnail_tx,
            thumbnail_rx,
            portable_thumbnail_tx,
            preview_tx,
            preview_rx,
            preview_generation,
            preview_active_path: None,
            archive_queue: VecDeque::new(),
            active_archives: HashMap::new(),
            archive_progress: HashMap::new(),
            archive_history: VecDeque::new(),
            next_archive_id: 0,
            max_parallel_archives: 3,
            archive_panel_minimized: false,
            archive_panel_spawned: false,
            defender_rx: None,
            defender_cancel: None,
            defender_progress: None,
            defender_summary: None,
            defender_panel_minimized: false,
            defender_panel_spawned: false,
            clipboard: None,
            thumbnail_cache: HashMap::new(),
            native_icon_cache: HashMap::new(),
            preview_cache: HashMap::new(),
            preview_text_selection: None,
            pending_select: None,
            type_select: None,
            pending_scroll_path: None,
            path_bar_text_visible: false,
            path_bar_edit_text: String::new(),
            path_bar_focus_pending: false,
            path_bar_selection_range: None,
            preview_panel_visible,
            text_input_active: false,
            paste_shortcut_down: false,
            shortcut_capture: None,
            last_auto_fit_path: None,
            last_auto_fit_width: None,
            vibrancy_applied: false,
            vibrancy_dirty: false,
            last_storage_refresh: Instant::now(),
        };
        let (group_by, group_ascending) = app.group_for_path(app.active_path().as_deref());
        app.group_by = group_by;
        app.group_ascending = group_ascending;
        // Restore a persisted split view. The active panel uses the app's
        // global state; the other panel gets its own PaneState.
        let mut restored_split = false;
        if let Some(sess) = session.split {
            if sess.tab_a < app.tabs.len() && sess.tab_b < app.tabs.len() {
                let mut primary_tabs =
                    normalize_split_tabs(sess.primary_tabs, sess.tab_a, app.tabs.len());
                let mut secondary_tabs =
                    normalize_split_tabs(sess.secondary_tabs, sess.tab_b, app.tabs.len());
                primary_tabs.retain(|index| !secondary_tabs.contains(index));
                if primary_tabs.is_empty() {
                    primary_tabs.push(sess.tab_a);
                }
                if secondary_tabs.is_empty() {
                    secondary_tabs.push(sess.tab_b);
                }
                let focused_tab = match sess.focused {
                    SplitFocus::Primary => sess.tab_a,
                    SplitFocus::Secondary => sess.tab_b,
                };
                let other_tab = match sess.focused {
                    SplitFocus::Primary => sess.tab_b,
                    SplitFocus::Secondary => sess.tab_a,
                };
                app.active_tab = focused_tab;
                app.focused_pane = 0;
                // Build a PaneState for the other panel; start folder load.
                let mut other = PaneState::new(other_tab);
                let (group_by, group_ascending) =
                    app.group_for_path(app.tabs.get(other_tab).and_then(|t| t.path.as_deref()));
                other.group_by = group_by;
                other.group_ascending = group_ascending;
                other.preview_panel_visible = app.preview_panel_visible;
                other.start_folder_load(
                    app.tabs.get(other_tab).and_then(|t| t.path.clone()),
                    app.config.show_hidden,
                );
                app.other_pane = Some(other);
                app.split = Some(SplitState {
                    tab_a: sess.tab_a,
                    tab_b: sess.tab_b,
                    primary_tabs,
                    secondary_tabs,
                    focused: sess.focused,
                    ratio: sess.ratio.clamp(0.1, 0.9),
                    side: sess.side,
                });
                app.refresh_active_tab();
                restored_split = true;
            }
        }
        if !restored_split {
            app.refresh_active_tab();
        }

        app
    }

    pub fn active_path(&self) -> Option<PathBuf> {
        self.tabs
            .get(self.active_tab)
            .and_then(|tab| tab.path.clone())
    }

    fn ensure_tab_search_len(&mut self) {
        self.tab_search
            .resize_with(self.tabs.len(), TabSearchState::default);
    }

    fn save_current_tab_search_state(&mut self) {
        self.ensure_tab_search_len();
        if let Some(state) = self.tab_search.get_mut(self.active_tab) {
            state.filter = self.filter.clone();
            state.search_mode = self.search_mode;
            state.search_results = self.search_results.clone();
            state.searching = self.searching;
            state.next_search_request_id = self.next_search_request_id;
            state.search_rx = self.search_rx.take();
            state.search_cancel = self.search_cancel.take();
        }
    }

    fn restore_current_tab_search_state(&mut self) {
        self.ensure_tab_search_len();
        let state = self.take_tab_search_state(self.active_tab);
        self.filter = state.filter;
        self.search_mode = state.search_mode;
        self.search_results = state.search_results;
        self.searching = state.searching;
        self.next_search_request_id = state.next_search_request_id;
        self.search_rx = state.search_rx;
        self.search_cancel = state.search_cancel;
        self.selected.clear();
        self.selection_anchor = None;
        self.selection_focus = None;
    }

    fn clear_tab_search_state(&mut self, index: usize) {
        self.ensure_tab_search_len();
        if let Some(state) = self.tab_search.get_mut(index) {
            if let Some(cancel) = state.search_cancel.take() {
                cancel.store(true, AtomicOrdering::Relaxed);
            }
            *state = TabSearchState::default();
        }
    }

    fn take_tab_search_state(&mut self, index: usize) -> TabSearchState {
        self.ensure_tab_search_len();
        self.tab_search
            .get_mut(index)
            .map(std::mem::take)
            .unwrap_or_default()
    }

    fn tab_search_snapshot_without_worker(&mut self, index: usize) -> TabSearchState {
        self.ensure_tab_search_len();
        self.tab_search
            .get(index)
            .map(|state| TabSearchState {
                filter: state.filter.clone(),
                search_mode: state.search_mode,
                search_results: state.search_results.clone(),
                searching: false,
                next_search_request_id: 0,
                search_rx: None,
                search_cancel: None,
            })
            .unwrap_or_default()
    }

    fn apply_tab_search_to_pane(pane: &mut PaneState, state: TabSearchState) {
        pane.filter = state.filter;
        pane.search_mode = state.search_mode;
        pane.search_results = state.search_results;
        pane.searching = state.searching;
        pane.next_search_request_id = state.next_search_request_id;
        pane.search_rx = state.search_rx;
        pane.search_cancel = state.search_cancel;
    }

    fn push_tab_search_state(&mut self) {
        self.ensure_tab_search_len();
        self.tab_search.push(TabSearchState::default());
    }

    fn remove_tab_search_state(&mut self, index: usize) {
        self.ensure_tab_search_len();
        if index < self.tab_search.len() {
            if let Some(cancel) = self.tab_search[index].search_cancel.take() {
                cancel.store(true, AtomicOrdering::Relaxed);
            }
            self.tab_search.remove(index);
        }
        self.ensure_tab_search_len();
    }

    fn move_tab_search_state(&mut self, from: usize, to: usize) {
        self.ensure_tab_search_len();
        if from >= self.tab_search.len() || to >= self.tab_search.len() {
            return;
        }
        let state = self.tab_search.remove(from);
        self.tab_search.insert(to, state);
    }

    fn start_folder_load_preserving_search(&mut self) {
        self.start_folder_load(self.active_path());
        if self.filter.trim().is_empty() {
            self.sync_complete_search();
        } else {
            self.status_message = format!("Search results: {} item(s)", self.search_results.len());
        }
    }

    pub fn is_storage_view(&self) -> bool {
        self.active_path().is_none()
    }

    fn special_root_for_path(path: Option<&Path>) -> Option<SpecialRoot> {
        match path {
            None => Some(SpecialRoot::Storage),
            Some(path) if explorer::is_network_root_path(path) => Some(SpecialRoot::Network),
            _ => None,
        }
    }

    fn view_for_path_from_config(config: &AppConfig, path: Option<&Path>) -> ViewMode {
        match Self::special_root_for_path(path) {
            Some(SpecialRoot::Storage) => config.storage_view,
            Some(SpecialRoot::Network) => config.network_view,
            None => config.default_view,
        }
    }

    fn group_for_path(&self, path: Option<&Path>) -> (FileGroup, bool) {
        match Self::special_root_for_path(path) {
            Some(SpecialRoot::Storage) => (
                file_group_from_config(self.config.storage_group),
                self.config.storage_group_ascending,
            ),
            Some(SpecialRoot::Network) => (
                file_group_from_config(self.config.network_group),
                self.config.network_group_ascending,
            ),
            None => (FileGroup::None, true),
        }
    }

    fn set_current_tab_view_mode(&mut self, view_mode: ViewMode) {
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.view_mode = view_mode;
        }
    }

    fn apply_view_for_path_transition(
        &mut self,
        previous_path: Option<&Path>,
        current_path: Option<&Path>,
    ) {
        match Self::special_root_for_path(current_path) {
            Some(_) => {
                let view_mode = Self::view_for_path_from_config(&self.config, current_path);
                self.set_current_tab_view_mode(view_mode);
            }
            None if Self::special_root_for_path(previous_path).is_some() => {
                self.set_current_tab_view_mode(self.config.default_view);
            }
            None => {}
        }
    }

    fn remember_special_view_preference(&mut self, view_mode: ViewMode) {
        let Some(root) = Self::special_root_for_path(self.active_path().as_deref()) else {
            return;
        };
        let changed = match root {
            SpecialRoot::Storage if self.config.storage_view != view_mode => {
                self.config.storage_view = view_mode;
                true
            }
            SpecialRoot::Network if self.config.network_view != view_mode => {
                self.config.network_view = view_mode;
                true
            }
            _ => false,
        };
        if changed {
            if let Err(error) = self.config.save() {
                crate::utils::log::error(format!("Config save failed: {error}"));
            }
        }
    }

    fn remember_special_group_preference(&mut self) {
        let Some(root) = Self::special_root_for_path(self.active_path().as_deref()) else {
            return;
        };
        let group = config_group_from_file(self.group_by);
        let changed = match root {
            SpecialRoot::Storage
                if self.config.storage_group != group
                    || self.config.storage_group_ascending != self.group_ascending =>
            {
                self.config.storage_group = group;
                self.config.storage_group_ascending = self.group_ascending;
                true
            }
            SpecialRoot::Network
                if self.config.network_group != group
                    || self.config.network_group_ascending != self.group_ascending =>
            {
                self.config.network_group = group;
                self.config.network_group_ascending = self.group_ascending;
                true
            }
            _ => false,
        };
        if changed {
            if let Err(error) = self.config.save() {
                crate::utils::log::error(format!("Config save failed: {error}"));
            }
        }
    }

    pub fn can_go_back(&self) -> bool {
        self.tabs
            .get(self.active_tab)
            .map(|tab| tab.history_index > 0)
            .unwrap_or(false)
    }

    pub fn can_go_forward(&self) -> bool {
        self.tabs
            .get(self.active_tab)
            .map(|tab| tab.history_index + 1 < tab.history.len())
            .unwrap_or(false)
    }

    pub fn can_go_up(&self) -> bool {
        let Some(path) = self.active_path() else {
            return false;
        };
        if explorer::virtual_parent(&path).is_some() || explorer::unc_share_parent(&path).is_some()
        {
            return true;
        }
        path.parent().is_some()
    }

    pub fn filtered_entries(&self) -> Vec<FileEntry> {
        self.filtered_entries_slice().to_vec()
    }

    pub fn filtered_entries_slice(&self) -> &[FileEntry] {
        if self.filter.trim().is_empty() {
            &self.entries
        } else {
            &self.search_results
        }
    }

    pub fn filtered_entry_count(&self) -> usize {
        self.filtered_entries_slice().len()
    }

    pub fn filtered_entry_at(&self, index: usize) -> Option<FileEntry> {
        self.filtered_entries_slice().get(index).cloned()
    }

    pub fn visible_contains_path(&self, path: &Path) -> bool {
        self.filtered_entries_slice()
            .iter()
            .any(|entry| entry.path == path)
    }

    pub fn selected_visible_entry(&self) -> Option<FileEntry> {
        self.filtered_entries_slice()
            .iter()
            .find(|entry| self.selected.contains(&entry.path))
            .cloned()
    }

    pub fn selected_size(&self) -> u64 {
        if self.selected.is_empty() {
            return 0;
        }
        self.filtered_entries_slice()
            .iter()
            .filter(|entry| self.selected.contains(&entry.path))
            .filter_map(|entry| entry.size)
            .sum()
    }

    pub fn open_entry(&mut self, entry: &FileEntry) {
        match entry.kind {
            EntryKind::Drive | EntryKind::Folder => self.navigate_to(Some(entry.path.clone())),
            _ if explorer::is_portable_path(&entry.path) => {
                let path = entry.path.clone();
                let name = entry.name.clone();
                self.spawn_operation(format!("Opening {name} from device..."), false, move || {
                    let local_path = portable::stage_file_for_open(&path)?;
                    operations::open_path(&local_path)?;
                    Ok("File opened".into())
                });
            }
            _ if crate::fs::archive_listing::is_browsable_archive(&entry.path) => {
                self.new_tab(Some(entry.path.clone()));
            }
            _ if is_iso_image(&entry.path) => match operations::mount_disk_image(&entry.path) {
                Ok(()) => {
                    self.refresh_storage();
                    match operations::mounted_disk_image_root(&entry.path) {
                        Ok(root) => {
                            if let Err(error) = operations::suppress_file_explorer_windows_at(&root)
                            {
                                crate::utils::log::error(format!(
                                    "Could not suppress external Explorer window: {error}"
                                ));
                            }
                            self.status_message = format!("ISO mounted at {}", root.display());
                            self.new_tab(Some(root));
                        }
                        Err(error) => {
                            crate::utils::log::error(format!(
                                "Mounted ISO volume lookup failed: {error}"
                            ));
                            self.status_message = "ISO mounted".into();
                        }
                    }
                }
                Err(error) => {
                    crate::utils::log::error(format!("ISO mount failed: {error}"));
                    match operations::mounted_disk_image_root(&entry.path) {
                        Ok(root) => {
                            self.refresh_storage();
                            if let Err(error) = operations::suppress_file_explorer_windows_at(&root)
                            {
                                crate::utils::log::error(format!(
                                    "Could not suppress external Explorer window: {error}"
                                ));
                            }
                            self.status_message =
                                format!("ISO already mounted at {}", root.display());
                            self.new_tab(Some(root));
                        }
                        Err(root_error) => {
                            crate::utils::log::error(format!(
                                "Mounted ISO volume lookup after mount failure failed: {root_error}"
                            ));
                            self.set_error(format!("Could not mount ISO image: {error}"));
                        }
                    }
                }
            },
            _ => {
                if let Err(error) = operations::open_path(&entry.path) {
                    self.set_error(error.to_string());
                }
            }
        }
    }

    pub fn open_with(&mut self, path: &Path) {
        if explorer::is_portable_path(path) {
            let path = path.to_path_buf();
            let name = portable::path_name(&path);
            self.spawn_operation(
                format!("Preparing {name} from device..."),
                false,
                move || {
                    let local_path = portable::stage_file_for_open(&path)?;
                    crate::platform::shell::open_with(&local_path)?;
                    Ok("Open with requested".into())
                },
            );
            return;
        }

        if explorer::is_virtual_path(path) {
            self.status_message = "Open with is not available for virtual locations".into();
            return;
        }
        match crate::platform::shell::open_with(path) {
            Ok(()) => self.status_message = "Open with requested".into(),
            Err(error) => self.set_error(error.to_string()),
        }
    }

    pub fn scan_with_windows_defender(&mut self, path: PathBuf) {
        if explorer::is_virtual_path(&path) || archive_item_path(&path) {
            self.status_message = "Windows Defender scan is not available for this location".into();
            return;
        }

        self.ensure_selected(path);
        let paths = self
            .selected_paths()
            .into_iter()
            .filter(|path| !explorer::is_virtual_path(path) && !archive_item_path(path))
            .collect::<Vec<_>>();
        if paths.is_empty() {
            self.status_message = "No scan target selected".into();
            return;
        }

        let job = DefenderJob { paths };
        let total = job.paths.len();
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let progress = DefenderProgress {
            state: DefenderScanState::Running,
            current_path: job.paths.first().cloned(),
            scanned: 0,
            total,
            threats_found: 0,
            started: Instant::now(),
        };

        self.defender_rx = Some(rx);
        self.defender_cancel = Some(cancel);
        self.defender_progress = Some(progress);
        self.defender_summary = None;
        self.defender_panel_minimized = false;
        self.defender_panel_spawned = false;
        self.status_message = "Scanning with Windows Defender...".into();
        thread::spawn(move || defender::run_scan(job, tx, worker_cancel));
    }

    pub fn show_properties(&mut self, path: &Path) {
        if explorer::is_virtual_path(path) {
            self.status_message = "Properties are not available for virtual locations".into();
            return;
        }
        match crate::platform::shell::show_properties(path) {
            Ok(()) => self.status_message = "Properties opened".into(),
            Err(error) => self.set_error(error.to_string()),
        }
    }

    pub fn show_selected_or_current_properties(&mut self) {
        if let Some(path) = self.selected.iter().next().cloned() {
            self.show_properties(&path);
        } else if let Some(path) = self.active_path() {
            self.show_properties(&path);
        } else {
            self.status_message = "No properties target".into();
        }
    }

    pub fn eject_drive(&mut self, path: PathBuf) {
        self.spawn_operation("Ejecting drive...".into(), true, move || {
            operations::eject_drive(&path)?;
            Ok("Drive ejected".into())
        });
    }

    pub fn open_selected(&mut self) {
        let Some(path) = self.selected.iter().next().cloned() else {
            return;
        };

        if let Some(entry) = self
            .filtered_entries()
            .into_iter()
            .find(|entry| entry.path == path)
        {
            self.open_entry(&entry);
        }
    }

    pub fn navigate_to(&mut self, path: Option<PathBuf>) {
        let previous_path = self.active_path();
        if let Some(path) = path.as_ref() {
            let is_inside_archive = crate::fs::archive_listing::is_inside_archive(path);
            let is_virtual = explorer::is_virtual_path(path);
            let is_unc = explorer::is_unc_path(path);
            if !is_virtual && !is_inside_archive && !is_unc && !path.exists() {
                self.set_error(format!("Path does not exist: {}", path.display()));
                return;
            }
        }

        if self.active_path() == path {
            if path
                .as_ref()
                .is_some_and(|path| explorer::is_virtual_path(path))
            {
                self.refresh_active_tab();
            }
            return;
        }

        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.navigate_to(path.clone());
        }

        if let Some(path) = path {
            self.config.remember_recent(path);
        }

        let current_path = self.active_path();
        self.apply_view_for_path_transition(previous_path.as_deref(), current_path.as_deref());
        self.clear_tab_search_state(self.active_tab);
        self.reset_per_tab_state();
        self.start_folder_load(self.active_path());
        self.sync_complete_search();
    }

    pub fn go_back(&mut self) {
        let previous_path = self.active_path();
        if self
            .tabs
            .get_mut(self.active_tab)
            .map(TabState::go_back)
            .unwrap_or(false)
        {
            let current_path = self.active_path();
            self.apply_view_for_path_transition(previous_path.as_deref(), current_path.as_deref());
            self.clear_tab_search_state(self.active_tab);
            self.reset_per_tab_state();
            self.persist_session();
            self.refresh_active_tab();
        }
    }

    pub fn go_forward(&mut self) {
        let previous_path = self.active_path();
        if self
            .tabs
            .get_mut(self.active_tab)
            .map(TabState::go_forward)
            .unwrap_or(false)
        {
            let current_path = self.active_path();
            self.apply_view_for_path_transition(previous_path.as_deref(), current_path.as_deref());
            self.clear_tab_search_state(self.active_tab);
            self.reset_per_tab_state();
            self.persist_session();
            self.refresh_active_tab();
        }
    }

    pub fn go_up(&mut self) {
        let Some(path) = self.active_path() else {
            return;
        };

        if let Some(parent) = explorer::virtual_parent(&path) {
            self.navigate_to(parent);
            return;
        }

        if let Some(parent) = explorer::unc_share_parent(&path) {
            self.navigate_to(Some(parent));
            return;
        }

        if let Some(parent) = path.parent() {
            self.navigate_to(Some(parent.to_path_buf()));
        } else {
            self.navigate_to(None);
        }
    }

    pub fn new_tab(&mut self, path: Option<PathBuf>) {
        if let Some(focused) = self.split.as_ref().map(|split| split.focused) {
            self.new_split_tab(focused, path);
            return;
        }

        self.save_current_tab_search_state();
        self.push_tab_search_state();
        let view_mode = Self::view_for_path_from_config(&self.config, path.as_deref());
        self.tabs.push(TabState::with_view_mode(path, view_mode));
        self.active_tab = self.tabs.len().saturating_sub(1);
        self.reset_per_tab_state();
        self.persist_session();
        self.refresh_active_tab();
    }

    pub fn new_split_tab(&mut self, pane: SplitFocus, path: Option<PathBuf>) {
        if self.split.is_none() {
            self.new_tab(path);
            return;
        }

        self.focus_split_pane(pane);
        let index = self.tabs.len();
        self.save_current_tab_search_state();
        self.push_tab_search_state();
        let view_mode = Self::view_for_path_from_config(&self.config, path.as_deref());
        self.tabs.push(TabState::with_view_mode(path, view_mode));
        self.active_tab = index;
        self.reset_per_tab_state();

        if let Some(split) = self.split.as_mut() {
            match pane {
                SplitFocus::Primary => {
                    split.tab_a = index;
                    split.primary_tabs.push(index);
                }
                SplitFocus::Secondary => {
                    split.tab_b = index;
                    split.secondary_tabs.push(index);
                }
            }
        }

        self.persist_session();
        self.refresh_active_tab();
    }

    pub fn toggle_sidebar_visible(&mut self) {
        self.sidebar_visible = !self.sidebar_visible;
        self.config.sidebar_visible = self.sidebar_visible;
        self.save_config();
    }

    pub fn open_new_tab_in_split(&mut self) {
        let path = self.active_path();

        if let Some(focused) = self.split.as_ref().map(|split| split.focused) {
            self.new_split_tab(opposite_split_focus(focused), path);
            return;
        }

        let primary = self.active_tab.min(self.tabs.len().saturating_sub(1));
        let secondary = self.tabs.len();
        self.save_current_tab_search_state();
        let primary_snapshot = self.tab_search_snapshot_without_worker(primary);
        let active_search_state = self.take_tab_search_state(primary);
        self.push_tab_search_state();
        let view_mode = Self::view_for_path_from_config(&self.config, path.as_deref());
        self.tabs.push(TabState::with_view_mode(path, view_mode));
        self.active_tab = secondary;
        if let Some(state) = self.tab_search.get_mut(primary) {
            *state = primary_snapshot;
        }
        if let Some(state) = self.tab_search.get_mut(secondary) {
            *state = active_search_state;
        }
        self.restore_current_tab_search_state();

        let mut other = PaneState::new(primary);
        let (group_by, group_ascending) =
            self.group_for_path(self.tabs.get(primary).and_then(|t| t.path.as_deref()));
        other.group_by = group_by;
        other.group_ascending = group_ascending;
        other.preview_panel_visible = self.preview_panel_visible;
        let other_search = self.tab_search_snapshot_without_worker(primary);
        Self::apply_tab_search_to_pane(&mut other, other_search);
        other.start_folder_load(
            self.tabs.get(primary).and_then(|tab| tab.path.clone()),
            self.config.show_hidden,
        );
        self.other_pane = Some(other);
        self.focused_pane = 0;
        self.split = Some(SplitState {
            tab_a: primary,
            tab_b: secondary,
            primary_tabs: (0..secondary).collect(),
            secondary_tabs: vec![secondary],
            focused: SplitFocus::Secondary,
            ratio: 0.5,
            side: SplitSide::Right,
        });
        self.start_folder_load_preserving_search();
        self.persist_session();
    }

    pub fn close_tab(&mut self, index: usize) {
        self.save_current_tab_search_state();
        if let Some(pane) = self.split_pane_for_tab(index) {
            self.close_split_tab(index, pane);
            return;
        }

        // If the closed tab participates in a split, collapse the split first.
        if let Some(split) = self.split.as_ref() {
            if split.tab_a == index || split.tab_b == index {
                let surviving = if split.tab_a == index {
                    split.tab_b
                } else {
                    split.tab_a
                };
                self.active_tab = surviving.min(self.tabs.len().saturating_sub(1));
                self.other_pane = None;
                self.split = None;
            }
        }

        if self.tabs.len() <= 1 {
            if let Some(tab) = self.tabs.get_mut(0) {
                *tab = TabState::with_view_mode(
                    None,
                    Self::view_for_path_from_config(&self.config, None),
                );
            }
            self.active_tab = 0;
            self.clear_tab_search_state(0);
        } else if index < self.tabs.len() {
            self.tabs.remove(index);
            self.remove_tab_search_state(index);
            self.active_tab = self.active_tab.min(self.tabs.len().saturating_sub(1));
            // Reindex other_pane's active_tab if it exists and split wasn't collapsed.
            if let Some(other) = self.other_pane.as_mut() {
                other.active_tab = reindex_on_close(other.active_tab, index);
            }
            self.reindex_split_on_close(index);
        }
        self.restore_current_tab_search_state();
        self.persist_session();
        self.start_folder_load_preserving_search();
    }

    pub fn activate_tab(&mut self, index: usize) {
        if self.split.is_some() {
            self.activate_split_tab(index);
            return;
        }

        if index < self.tabs.len() && index != self.active_tab {
            self.save_current_tab_search_state();
            self.cancel_complete_search();
            self.active_tab = index;
            let current_path = self.active_path();
            self.apply_view_for_path_transition(None, current_path.as_deref());
            self.restore_current_tab_search_state();
            self.persist_session();
            self.start_folder_load_preserving_search();
        }
    }

    pub fn focus_split_pane(&mut self, pane: SplitFocus) {
        let should_swap = self
            .split
            .as_ref()
            .is_some_and(|split| split.focused != pane);
        if should_swap {
            self.swap_split_focus();
        }
    }

    pub fn activate_split_tab(&mut self, index: usize) {
        let Some(pane) = self.split_pane_for_tab(index) else {
            return;
        };
        if index >= self.tabs.len() {
            return;
        }

        self.focus_split_pane(pane);
        if self.active_tab == index {
            return;
        }

        self.save_current_tab_search_state();
        self.cancel_complete_search();
        self.active_tab = index;
        if let Some(split) = self.split.as_mut() {
            match pane {
                SplitFocus::Primary => split.tab_a = index,
                SplitFocus::Secondary => split.tab_b = index,
            }
        }
        self.restore_current_tab_search_state();
        let current_path = self.active_path();
        self.apply_view_for_path_transition(None, current_path.as_deref());
        self.persist_session();
        self.start_folder_load_preserving_search();
    }

    pub fn move_tab(&mut self, from: usize, to: usize) {
        if from == to || from >= self.tabs.len() || to >= self.tabs.len() {
            return;
        }
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        self.move_tab_search_state(from, to);
        // Keep the active tab pointing to the same physical tab.
        self.active_tab = match self.active_tab {
            a if a == from => to,
            a if from < a && to >= a => a - 1,
            a if from > a && to <= a => a + 1,
            a => a,
        };
        // Reindex other_pane's active_tab.
        if let Some(other) = self.other_pane.as_mut() {
            other.active_tab = reindex_after_move(other.active_tab, from, to);
        }
        // Reindex split panel tabs the same way as active_tab.
        if let Some(split) = self.split.as_mut() {
            split.tab_a = reindex_after_move(split.tab_a, from, to);
            split.tab_b = reindex_after_move(split.tab_b, from, to);
            reindex_split_tab_list_after_move(&mut split.primary_tabs, from, to);
            reindex_split_tab_list_after_move(&mut split.secondary_tabs, from, to);
        }
        self.persist_session();
    }

    pub fn move_split_tab_within_pane(
        &mut self,
        pane: SplitFocus,
        from_tab: usize,
        target_tab: usize,
    ) {
        let Some(split) = self.split.as_mut() else {
            return;
        };
        let list = match pane {
            SplitFocus::Primary => &mut split.primary_tabs,
            SplitFocus::Secondary => &mut split.secondary_tabs,
        };
        let Some(from_pos) = list.iter().position(|index| *index == from_tab) else {
            return;
        };
        let Some(target_pos) = list.iter().position(|index| *index == target_tab) else {
            return;
        };
        if from_pos == target_pos {
            return;
        }

        let moved = list.remove(from_pos);
        list.insert(target_pos.min(list.len()), moved);
        self.persist_session();
    }

    pub fn snap_tab_to_split(&mut self, dragged_tab: usize, side: SplitSide) {
        if dragged_tab >= self.tabs.len() || self.tabs.len() < 2 {
            return;
        }
        // The dragged tab becomes the secondary panel; another tab stays as
        // the primary. If the dragged tab is the active one, pick the first
        // different tab as the primary. The dragged tab is focused afterwards.
        let primary = if self.active_tab == dragged_tab {
            (0..self.tabs.len())
                .find(|&i| i != dragged_tab)
                .unwrap_or(dragged_tab)
        } else {
            self.active_tab
        };
        let secondary = dragged_tab;
        if primary == secondary {
            return;
        }
        // The app's live fields become the focused secondary pane; the
        // unfocused primary pane lives in `other_pane`.
        let mut other = PaneState::new(primary);
        let (group_by, group_ascending) =
            self.group_for_path(self.tabs.get(primary).and_then(|t| t.path.as_deref()));
        other.group_by = group_by;
        other.group_ascending = group_ascending;
        other.preview_panel_visible = self.preview_panel_visible;
        other.start_folder_load(
            self.tabs.get(primary).and_then(|t| t.path.clone()),
            self.config.show_hidden,
        );
        // The secondary panel is the dragged tab; focus it.
        self.active_tab = secondary;
        self.reset_per_tab_state();
        self.other_pane = Some(other);
        self.focused_pane = 0;
        self.split = Some(SplitState {
            tab_a: primary,
            tab_b: secondary,
            primary_tabs: (0..self.tabs.len())
                .filter(|index| *index != secondary)
                .collect(),
            secondary_tabs: vec![secondary],
            focused: SplitFocus::Secondary,
            ratio: 0.5,
            side,
        });
        self.refresh_active_tab();
        self.persist_session();
    }

    pub fn split_pane_for_tab(&self, index: usize) -> Option<SplitFocus> {
        let split = self.split.as_ref()?;
        if split.primary_tabs.contains(&index) {
            Some(SplitFocus::Primary)
        } else if split.secondary_tabs.contains(&index) {
            Some(SplitFocus::Secondary)
        } else {
            None
        }
    }

    fn close_split_tab(&mut self, index: usize, pane: SplitFocus) {
        if index >= self.tabs.len() {
            return;
        }

        let (pane_tabs, other_tabs, pane_active) = match self.split.as_ref() {
            Some(split) => match pane {
                SplitFocus::Primary => (&split.primary_tabs, &split.secondary_tabs, split.tab_a),
                SplitFocus::Secondary => (&split.secondary_tabs, &split.primary_tabs, split.tab_b),
            },
            None => return,
        };
        let position = pane_tabs.iter().position(|tab| *tab == index).unwrap_or(0);
        let replacement = if pane_active != index {
            Some(pane_active)
        } else if pane_tabs.len() > 1 {
            pane_tabs
                .get(position + 1)
                .or_else(|| position.checked_sub(1).and_then(|pos| pane_tabs.get(pos)))
                .copied()
        } else {
            None
        };

        if replacement.is_none() {
            let Some(other_active) = other_tabs.first().copied() else {
                return;
            };
            self.focus_split_pane(opposite_split_focus(pane));
            self.active_tab = other_active;
            self.tabs.remove(index);
            self.remove_tab_search_state(index);
            self.active_tab = reindex_on_close(self.active_tab, index);
            self.other_pane = None;
            self.split = None;
            self.focused_pane = 0;
            self.restore_current_tab_search_state();
            self.persist_session();
            self.start_folder_load_preserving_search();
            return;
        }

        self.focus_split_pane(pane);
        self.active_tab = replacement.unwrap();
        if let Some(split) = self.split.as_mut() {
            match pane {
                SplitFocus::Primary => split.tab_a = self.active_tab,
                SplitFocus::Secondary => split.tab_b = self.active_tab,
            }
        }

        self.tabs.remove(index);
        self.remove_tab_search_state(index);
        self.active_tab = reindex_on_close(self.active_tab, index);
        if let Some(other) = self.other_pane.as_mut() {
            other.active_tab = reindex_on_close(other.active_tab, index);
        }
        self.reindex_split_on_close(index);
        self.restore_current_tab_search_state();
        self.persist_session();
        self.start_folder_load_preserving_search();
    }

    pub fn close_split(&mut self) {
        // Keep the currently focused tab as the single active tab.
        self.other_pane = None;
        if let Some(split) = self.split.take() {
            self.active_tab = match split.focused {
                SplitFocus::Primary => split.tab_a,
                SplitFocus::Secondary => split.tab_b,
            };
            self.active_tab = self.active_tab.min(self.tabs.len().saturating_sub(1));
        }
        self.focused_pane = 0;
        self.refresh_active_tab();
        self.persist_session();
    }

    pub fn swap_split_focus(&mut self) {
        // Swap the whole per-pane state. Both panes keep their live state,
        // so there's no snapshot to freeze or reload.
        if self.other_pane.is_none() {
            return;
        }
        self.swap_panes();
        if let Some(split) = self.split.as_mut() {
            split.focused = match split.focused {
                SplitFocus::Primary => SplitFocus::Secondary,
                SplitFocus::Secondary => SplitFocus::Primary,
            };
        }
        self.focused_pane = if self.focused_pane == 0 { 1 } else { 0 };
        self.persist_session();
    }

    pub fn set_split_ratio(&mut self, ratio: f32) {
        if let Some(split) = self.split.as_mut() {
            split.ratio = ratio.clamp(0.1, 0.9);
            self.persist_session();
        }
    }

    /// Adjust split tab indices after a tab is closed at `removed_index`.
    fn reindex_split_on_close(&mut self, removed_index: usize) {
        if let Some(split) = self.split.as_mut() {
            split.tab_a = reindex_on_close(split.tab_a, removed_index);
            split.tab_b = reindex_on_close(split.tab_b, removed_index);
            reindex_split_tab_list_on_close(&mut split.primary_tabs, removed_index);
            reindex_split_tab_list_on_close(&mut split.secondary_tabs, removed_index);
        }
    }

    pub fn refresh_active_tab(&mut self) {
        if self.active_path().is_none() {
            self.refresh_storage();
        }
        self.start_folder_load(self.active_path());
        self.sync_complete_search();
    }

    fn refresh_all_panes(&mut self) {
        self.refresh_storage();
        self.refresh_active_tab();
        if self.other_pane.is_some() {
            with_other_pane(self, |app| {
                app.refresh_active_tab();
            });
        }
    }

    /// Swap the current per-panel fields with the other pane's fields.
    /// Used before rendering/interacting with the other panel.
    pub fn swap_panes(&mut self) {
        let mut other = self
            .other_pane
            .take()
            .unwrap_or_else(|| panic!("swap_panes called but other_pane is None"));
        std::mem::swap(&mut self.active_tab, &mut other.active_tab);
        std::mem::swap(&mut self.entries, &mut other.entries);
        std::mem::swap(&mut self.selected, &mut other.selected);
        std::mem::swap(&mut self.selection_anchor, &mut other.selection_anchor);
        std::mem::swap(&mut self.selection_focus, &mut other.selection_focus);
        std::mem::swap(&mut self.filter, &mut other.filter);
        std::mem::swap(&mut self.search_mode, &mut other.search_mode);
        std::mem::swap(&mut self.search_results, &mut other.search_results);
        std::mem::swap(&mut self.searching, &mut other.searching);
        std::mem::swap(&mut self.loading, &mut other.loading);
        std::mem::swap(&mut self.status_message, &mut other.status_message);
        std::mem::swap(&mut self.rename_dialog, &mut other.rename_dialog);
        std::mem::swap(&mut self.pending_rename, &mut other.pending_rename);
        std::mem::swap(
            &mut self.action_bar_new_menu_open,
            &mut other.action_bar_new_menu_open,
        );
        std::mem::swap(&mut self.drag_selection, &mut other.drag_selection);
        std::mem::swap(&mut self.sort, &mut other.sort);
        std::mem::swap(&mut self.sort_ascending, &mut other.sort_ascending);
        std::mem::swap(&mut self.group_by, &mut other.group_by);
        std::mem::swap(&mut self.group_ascending, &mut other.group_ascending);
        std::mem::swap(&mut self.column_widths, &mut other.column_widths);
        std::mem::swap(&mut self.last_auto_fit_path, &mut other.last_auto_fit_path);
        std::mem::swap(
            &mut self.last_auto_fit_width,
            &mut other.last_auto_fit_width,
        );
        std::mem::swap(&mut self.pending_select, &mut other.pending_select);
        std::mem::swap(
            &mut self.path_bar_text_visible,
            &mut other.path_bar_text_visible,
        );
        std::mem::swap(&mut self.path_bar_edit_text, &mut other.path_bar_edit_text);
        std::mem::swap(
            &mut self.path_bar_focus_pending,
            &mut other.path_bar_focus_pending,
        );
        std::mem::swap(
            &mut self.path_bar_selection_range,
            &mut other.path_bar_selection_range,
        );
        std::mem::swap(
            &mut self.preview_active_path,
            &mut other.preview_active_path,
        );
        std::mem::swap(
            &mut self.preview_text_selection,
            &mut other.preview_text_selection,
        );
        std::mem::swap(
            &mut self.preview_panel_visible,
            &mut other.preview_panel_visible,
        );
        std::mem::swap(&mut self.next_request_id, &mut other.next_request_id);
        std::mem::swap(
            &mut self.next_search_request_id,
            &mut other.next_search_request_id,
        );
        std::mem::swap(&mut self.load_rx, &mut other.load_rx);
        std::mem::swap(&mut self.search_rx, &mut other.search_rx);
        std::mem::swap(&mut self.search_cancel, &mut other.search_cancel);
        std::mem::swap(&mut self.type_select, &mut other.type_select);
        std::mem::swap(
            &mut self.pending_scroll_path,
            &mut other.pending_scroll_path,
        );
        self.other_pane = Some(other);
    }

    /// Clear the per-tab view state (selection, filter, search) so the status
    /// bar and table reflect only the newly active tab/folder. Called whenever
    /// the active tab/folder changes.
    fn reset_per_tab_state(&mut self) {
        self.selected.clear();
        self.selection_anchor = None;
        self.selection_focus = None;
        self.type_select = None;
        self.pending_scroll_path = None;
        self.filter.clear();
        self.cancel_complete_search();
        self.searching = false;
        self.search_rx = None;
        self.search_results.clear();
        self.last_auto_fit_path = None;
        self.last_auto_fit_width = None;
        self.action_bar_new_menu_open = false;
        let (group_by, group_ascending) = self.group_for_path(self.active_path().as_deref());
        self.group_by = group_by;
        self.group_ascending = group_ascending;
        self.path_bar_text_visible = false;
        self.path_bar_edit_text.clear();
        self.path_bar_focus_pending = false;
        self.path_bar_selection_range = None;
        self.preview_active_path = None;
        self.preview_text_selection = None;
    }

    fn start_folder_load(&mut self, path: Option<PathBuf>) {
        self.next_request_id = self.next_request_id.saturating_add(1);
        let request_id = self.next_request_id;
        let show_hidden = self.config.show_hidden;
        let (tx, rx) = mpsc::channel();

        self.loading = true;
        self.status_message = "Loading folder...".into();
        self.load_rx = Some(rx);
        self.entries.clear();
        self.selected.clear();
        self.selection_anchor = None;
        self.selection_focus = None;

        if path.as_deref().is_some_and(explorer::is_network_root_path) {
            spawn_network_root_load(tx, request_id);
            return;
        }

        thread::spawn(move || {
            let result = explorer::list_entries(path.as_deref(), show_hidden)
                .map_err(|error| error.to_string());
            let _ = tx.send(LoadMessage {
                request_id,
                finished: true,
                append: false,
                result,
            });
        });
    }

    pub fn refresh_storage(&mut self) {
        self.storage_refresh_rx = None;
        match load_storage_snapshot() {
            Ok((entries, portable_devices)) => {
                self.apply_storage_snapshot(entries, portable_devices);
            }
            Err(error) => crate::utils::log::error(format!("Storage refresh failed: {error}")),
        }
        self.last_storage_refresh = Instant::now();
    }

    fn apply_storage_snapshot(
        &mut self,
        entries: Vec<FileEntry>,
        portable_devices: Vec<explorer::PortableDevice>,
    ) -> bool {
        if storage_signature(&entries) == storage_signature(&self.storage_entries)
            && portable_signature(&portable_devices) == portable_signature(&self.portable_devices)
        {
            return false;
        }

        self.storage_entries = entries.clone();
        self.portable_devices = portable_devices.clone();
        if self.active_path().is_none() {
            self.entries =
                explorer::combine_storage_and_portable_entries(&entries, &portable_devices);
            self.sort_entries();
            self.selected.clear();
            self.selection_anchor = None;
            self.selection_focus = None;
            self.retain_visible_thumbnails();
        }

        true
    }

    fn refresh_storage_if_needed(&mut self, ctx: &egui::Context) {
        if let Some(rx) = self.storage_refresh_rx.as_ref() {
            match rx.try_recv() {
                Ok(Ok((entries, portable_devices))) => {
                    self.storage_refresh_rx = None;
                    if self.apply_storage_snapshot(entries, portable_devices) {
                        ctx.request_repaint();
                    }
                }
                Ok(Err(error)) => {
                    self.storage_refresh_rx = None;
                    crate::utils::log::error(format!("Storage refresh failed: {error}"));
                }
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(250));
                    return;
                }
                Err(TryRecvError::Disconnected) => {
                    self.storage_refresh_rx = None;
                }
            }
        }

        let elapsed = self.last_storage_refresh.elapsed();
        if elapsed < STORAGE_REFRESH_INTERVAL {
            ctx.request_repaint_after(STORAGE_REFRESH_INTERVAL.saturating_sub(elapsed));
            return;
        }

        self.last_storage_refresh = Instant::now();
        let (tx, rx) = mpsc::channel();
        self.storage_refresh_rx = Some(rx);
        thread::spawn(move || {
            let _ = tx.send(load_storage_snapshot());
        });
        ctx.request_repaint_after(Duration::from_millis(250));
    }

    pub fn set_sort(&mut self, sort: FileSort) {
        if self.sort == sort {
            self.sort_ascending = !self.sort_ascending;
        } else {
            self.sort = sort;
            self.sort_ascending = true;
        }
        self.sort_entries();
        self.sort_search_results();
    }

    pub fn sort_state(&self) -> (FileSort, bool) {
        (self.sort, self.sort_ascending)
    }

    pub fn group_state(&self) -> (FileGroup, bool) {
        (self.group_by, self.group_ascending)
    }

    pub fn set_group(&mut self, group_by: FileGroup) {
        if self.group_by != group_by {
            self.group_by = group_by;
            self.group_ascending = true;
        }
        self.remember_special_group_preference();
        self.clear_selection();
    }

    pub fn set_group_ascending(&mut self, ascending: bool) {
        self.group_ascending = ascending;
        self.remember_special_group_preference();
    }

    pub fn set_column_width(&mut self, index: usize, width: f32) {
        if let Some(cell) = self.column_widths.get_mut(index) {
            let (min, max) = match index {
                0 => (240.0, 700.0),
                7 => (180.0, 900.0),
                _ => (92.0, 500.0),
            };
            *cell = width.clamp(min, max);
        }
    }

    pub fn copy_selection(&mut self, cut: bool) {
        let paths = self.selected_paths();
        if paths.is_empty() {
            self.status_message = "No selected items".into();
            return;
        }

        let archive_items = paths.iter().any(|path| archive_item_path(path));
        let portable_items = paths.iter().any(|path| explorer::is_portable_path(path));
        if portable_items {
            if !paths.iter().all(|path| explorer::is_portable_path(path)) {
                self.set_error(
                    "Cannot mix portable device items with other items in one copy".into(),
                );
                return;
            }

            let worker_paths = paths.clone();
            self.clipboard = Some(FileClipboard { paths, cut: false });
            if let Err(error) = crate::platform::shell::clear_clipboard() {
                crate::utils::log::error(format!("Clipboard clear failed: {error}"));
            }
            self.spawn_operation(
                "Preparing portable item(s) for clipboard...".into(),
                false,
                move || {
                    let staged = portable::stage_paths_for_clipboard(&worker_paths)?;
                    crate::platform::shell::copy_files(&staged, false)?;
                    Ok("Portable item(s) copied to clipboard".into())
                },
            );
            return;
        }
        let cut = cut && !archive_items;
        sync_file_clipboard(&paths, cut);
        self.clipboard = Some(FileClipboard { paths, cut });
        self.status_message = if cut {
            "Ready to move selected item(s)".into()
        } else if archive_items {
            "Ready to extract archive item(s)".into()
        } else {
            "Ready to copy selected item(s)".into()
        };
    }

    pub fn is_cut_path(&self, path: &Path) -> bool {
        self.clipboard.as_ref().is_some_and(|clipboard| {
            clipboard.cut && clipboard.paths.iter().any(|item| item == path)
        })
    }

    pub fn can_paste(&self) -> bool {
        self.clipboard
            .as_ref()
            .is_some_and(|clipboard| !clipboard.paths.is_empty())
            || crate::platform::shell::read_files().is_ok()
    }

    fn import_system_clipboard_paths(&mut self) -> bool {
        #[cfg(test)]
        {
            return self.clipboard.is_some();
        }

        #[cfg(not(test))]
        {
            if let Ok(files) = crate::platform::shell::read_files()
                && !files.paths.is_empty()
            {
                self.clipboard = Some(FileClipboard {
                    paths: files.paths,
                    cut: files.cut,
                });
                return true;
            }

            let Ok(text) = crate::platform::shell::read_text() else {
                return false;
            };
            let paths = paths_from_clipboard_text(&text);
            if paths.is_empty() {
                return false;
            }
            self.clipboard = Some(FileClipboard { paths, cut: false });
            true
        }
    }

    pub fn transfer_active(&self) -> bool {
        !self.active_transfers.is_empty()
    }

    pub fn operation_active(&self) -> bool {
        self.operation_rx.is_some()
    }

    pub fn defender_active(&self) -> bool {
        self.defender_rx.is_some()
            || self
                .defender_progress
                .as_ref()
                .is_some_and(|progress| progress.state == DefenderScanState::Running)
    }

    pub fn defender_visible(&self) -> bool {
        self.defender_active() || self.defender_summary.is_some()
    }

    pub fn cancel_defender_scan(&mut self) {
        if let Some(cancel) = &self.defender_cancel {
            cancel.store(true, AtomicOrdering::Relaxed);
            self.status_message = "Cancelling Windows Defender scan...".into();
        }
    }

    pub fn close_defender_panel(&mut self) {
        if self.defender_active() {
            self.cancel_defender_scan();
            return;
        }
        self.defender_progress = None;
        self.defender_summary = None;
        self.defender_panel_spawned = false;
    }

    pub fn remove_defender_threats(&mut self) {
        self.spawn_operation(
            "Removing Windows Defender threats...".into(),
            true,
            move || {
                defender::run_elevated_defender_action(&ElevatedDefenderAction::RemoveThreats)?;
                Ok("Windows Defender threats removed".into())
            },
        );
    }

    pub fn exclude_defender_scan_paths(&mut self) {
        let paths = self.defender_exclusion_paths();
        if paths.is_empty() {
            self.status_message = "No exclusion target selected".into();
            return;
        }
        self.spawn_operation(
            "Adding Windows Defender exclusion...".into(),
            false,
            move || {
                defender::run_elevated_defender_action(&ElevatedDefenderAction::ExcludePaths {
                    paths,
                })?;
                Ok("Windows Defender exclusion added".into())
            },
        );
    }

    pub fn open_windows_security(&mut self) {
        match crate::platform::shell::open_windows_security() {
            Ok(()) => self.status_message = "Windows Security opened".into(),
            Err(error) => self.set_error(error.to_string()),
        }
    }

    fn defender_exclusion_paths(&self) -> Vec<PathBuf> {
        let Some(summary) = self.defender_summary.as_ref() else {
            return Vec::new();
        };
        let mut paths = summary
            .threats
            .iter()
            .filter_map(|threat| threat.path.clone())
            .collect::<Vec<_>>();
        if paths.is_empty() {
            paths = summary.paths.clone();
        }
        paths.sort();
        paths.dedup();
        paths
    }

    pub fn transfer_queue_len(&self) -> usize {
        self.transfer_queue.len()
    }

    pub fn transfer_progress_fraction(&self) -> Option<f32> {
        if self.transfer_progress.is_empty() {
            return None;
        }

        let copied: u64 = self
            .transfer_progress
            .values()
            .map(|progress| progress.copied_bytes)
            .sum();
        let total: u64 = self
            .transfer_progress
            .values()
            .map(|progress| progress.total_bytes)
            .sum();
        Some(if total == 0 {
            0.0
        } else {
            copied as f32 / total as f32
        })
    }

    pub fn cancel_transfer(&mut self, job_id: u64) {
        if let Some(active) = self.active_transfers.get(&job_id) {
            active.control.cancel.store(true, AtomicOrdering::Relaxed);
            if let Some(progress) = self.transfer_progress.get_mut(&job_id) {
                progress.state = TransferState::Cancelled;
            }
            self.status_message = "Cancelling transfer...".into();
        }
    }

    pub fn pause_transfer(&mut self, job_id: u64) {
        if let Some(active) = self.active_transfers.get(&job_id) {
            active.control.pause.store(true, AtomicOrdering::Relaxed);
            if let Some(progress) = self.transfer_progress.get_mut(&job_id) {
                progress.state = TransferState::Paused;
            }
            self.status_message = "Transfer paused".into();
        }
    }

    pub fn resume_transfer(&mut self, job_id: u64) {
        if let Some(active) = self.active_transfers.get(&job_id) {
            active.control.pause.store(false, AtomicOrdering::Relaxed);
            if let Some(progress) = self.transfer_progress.get_mut(&job_id) {
                progress.state = TransferState::Copying;
            }
            self.status_message = "Transfer resumed".into();
        }
    }

    pub fn toggle_transfer_pause(&mut self, job_id: u64) {
        let Some(active) = self.active_transfers.get(&job_id) else {
            return;
        };
        if active.control.pause.load(AtomicOrdering::Relaxed) {
            self.resume_transfer(job_id);
        } else {
            self.pause_transfer(job_id);
        }
    }

    pub fn is_archive_active(&self) -> bool {
        self.active_archives
            .values()
            .any(|active| active.cancel_flag.load(AtomicOrdering::Relaxed) == 0)
    }

    pub fn cancel_archive(&mut self, id: u64) {
        if let Some(active) = self.active_archives.get(&id) {
            active.cancel_flag.store(1, AtomicOrdering::Relaxed);
            self.status_message = "Cancelling archive operation...".into();
        }
    }

    #[allow(dead_code)]
    pub fn cancel_all_active_archives(&mut self) {
        for active in self.active_archives.values() {
            active.cancel_flag.store(1, AtomicOrdering::Relaxed);
        }
        if !self.active_archives.is_empty() {
            self.status_message = "Cancelling archive operations...".into();
        }
    }

    pub fn archive_items(&self) -> Vec<ArchiveDisplayItem> {
        let mut items = Vec::new();

        for active in self.active_archives.values() {
            let display_state = if active.cancel_flag.load(AtomicOrdering::Relaxed) != 0 {
                ArchiveState::Cancelled
            } else {
                ArchiveState::Running
            };
            if let Some(state) = self.archive_progress.get(&active.job.id) {
                items.push(ArchiveDisplayItem {
                    id: active.job.id,
                    kind: active.job.kind,
                    state: display_state,
                    current_name: state.name.clone(),
                    file_name: state.file_name.clone(),
                    completed: state.completed,
                    total: state.total,
                    bytes_per_second: state.bytes_per_second,
                    queued_index: None,
                });
            } else {
                items.push(ArchiveDisplayItem {
                    id: active.job.id,
                    kind: active.job.kind,
                    state: display_state,
                    current_name: active.job.display_name(),
                    file_name: String::new(),
                    completed: 0,
                    total: 0,
                    bytes_per_second: 0.0,
                    queued_index: None,
                });
            }
        }

        for (index, job) in self.archive_queue.iter().enumerate() {
            items.push(ArchiveDisplayItem {
                id: job.id,
                kind: job.kind,
                state: ArchiveState::Pending,
                current_name: job.display_name(),
                file_name: String::new(),
                completed: 0,
                total: 0,
                bytes_per_second: 0.0,
                queued_index: Some(index + 1),
            });
        }

        for item in self
            .archive_history
            .iter()
            .rev()
            .filter(|h| h.finished_at.elapsed() < Duration::from_secs(1))
            .take(4)
        {
            items.push(ArchiveDisplayItem {
                id: item.id,
                kind: item.kind,
                state: item.state,
                current_name: item.name.clone(),
                file_name: String::new(),
                completed: item.completed,
                total: item.total,
                bytes_per_second: 0.0,
                queued_index: None,
            });
        }

        items.sort_by_key(|item| match item.state {
            ArchiveState::Running => (0, item.id),
            ArchiveState::Pending => (1, item.id),
            ArchiveState::Failed | ArchiveState::Cancelled | ArchiveState::Finished => (2, item.id),
        });
        items
    }

    fn start_next_archives(&mut self) {
        while self.active_archives.len() < self.max_parallel_archives {
            let Some(job) = self.archive_queue.pop_front() else {
                break;
            };
            let (tx, rx) = mpsc::channel();
            let job_name = job.display_name();
            let cancel_flag = Arc::new(AtomicU32::new(0));
            let worker_cancel = cancel_flag.clone();
            self.archive_progress.insert(
                job.id,
                ArchiveProgressState::new_for_job(job.kind, &job_name),
            );
            self.active_archives.insert(
                job.id,
                ActiveArchive {
                    job: job.clone(),
                    rx,
                    cancel_flag,
                },
            );
            thread::spawn(move || {
                archive::run_archive_job(job, tx, worker_cancel);
            });
        }
    }

    fn cleanup_partial_archive_destination(job: &ArchiveJob) {
        if job.kind != ArchiveJobKind::Compress || job.destination.as_os_str().is_empty() {
            return;
        }
        if let Err(error) = std::fs::remove_file(&job.destination)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            crate::utils::log::error(format!(
                "Could not remove partial archive {}: {error}",
                job.destination.display()
            ));
        }
    }

    pub fn clear_pending_archives(&mut self) {
        let count = self.archive_queue.len();
        if count > 0 {
            for job in self.archive_queue.drain(..) {
                self.archive_history.push_back(ArchiveHistoryItem {
                    id: job.id,
                    kind: job.kind,
                    state: ArchiveState::Cancelled,
                    name: job.display_name(),
                    completed: 0,
                    total: 0,
                    finished_at: Instant::now(),
                });
            }
            while self.archive_history.len() > 8 {
                self.archive_history.pop_front();
            }
            self.status_message = format!("Cleared {count} pending archive operation(s)");
        }
    }

    fn prune_archive_history(&mut self) {
        let visible_for = Duration::from_secs(1);
        while self
            .archive_history
            .front()
            .is_some_and(|item| item.finished_at.elapsed() >= visible_for)
        {
            self.archive_history.pop_front();
        }
    }

    pub fn cancel_all_active_transfers(&mut self) {
        let ids: Vec<u64> = self.active_transfers.keys().copied().collect();
        for job_id in ids {
            self.cancel_transfer(job_id);
        }
    }

    pub fn clear_pending_transfers(&mut self) {
        let cancelled: Vec<_> = self
            .transfer_queue
            .drain(..)
            .map(|job| TransferProgress {
                state: TransferState::Cancelled,
                ..TransferProgress::pending(&job)
            })
            .collect();
        for progress in cancelled {
            self.push_transfer_history(progress);
        }
        self.status_message = "Pending transfers cleared".into();
    }

    pub fn transfer_items(&self) -> Vec<TransferDisplayItem> {
        let mut items = Vec::new();

        for active in self.active_transfers.values() {
            let progress = self
                .transfer_progress
                .get(&active.job.id)
                .cloned()
                .unwrap_or_else(|| TransferProgress::pending(&active.job));
            items.push(TransferDisplayItem::from_progress(progress, None));
        }

        for (index, job) in self.transfer_queue.iter().enumerate() {
            items.push(TransferDisplayItem::from_progress(
                TransferProgress::pending(job),
                Some(index + 1),
            ));
        }

        for item in self
            .transfer_history
            .iter()
            .rev()
            .filter(|item| item.finished_at.elapsed() < Duration::from_secs(1))
            .take(4)
        {
            items.push(TransferDisplayItem::from_progress(
                item.progress.clone(),
                None,
            ));
        }

        items.sort_by_key(|item| match item.state {
            TransferState::Copying | TransferState::Paused => (0, item.id),
            TransferState::Pending => (1, item.id),
            TransferState::Failed | TransferState::Cancelled | TransferState::Finished => {
                (2, item.id)
            }
        });
        items
    }

    pub fn set_search_mode(&mut self, mode: SearchMode) {
        if self.search_mode == mode {
            return;
        }

        self.search_mode = mode;
        self.clear_selection();
        self.last_auto_fit_path = None;
        self.last_auto_fit_width = None;
        self.sync_complete_search();
    }

    pub fn on_filter_changed(&mut self) {
        self.clear_selection();
        self.last_auto_fit_path = None;
        self.last_auto_fit_width = None;
        self.sync_complete_search();
    }

    pub fn showing_complete_search_results(&self) -> bool {
        !self.filter.trim().is_empty()
    }

    pub fn open_location_for(&mut self, path: &Path) {
        let Some(parent) = path.parent() else {
            return;
        };

        self.filter.clear();
        self.cancel_complete_search();
        self.search_results.clear();
        self.pending_select = Some(path.to_path_buf());
        self.new_tab(Some(parent.to_path_buf()));
    }

    pub fn paste_into_active(&mut self) {
        let Some(destination) = self.active_path() else {
            self.status_message = "Open a folder before pasting".into();
            return;
        };
        self.paste_into(destination);
    }

    pub fn paste_into(&mut self, destination: PathBuf) {
        self.import_system_clipboard_paths();
        let Some(clipboard) = self.clipboard.clone() else {
            self.status_message = "Clipboard is empty".into();
            return;
        };
        let destination_is_portable = explorer::is_portable_path(&destination);
        if explorer::is_virtual_path(&destination) && !destination_is_portable {
            self.status_message = "Paste is not available in this virtual location".into();
            return;
        }
        if clipboard
            .paths
            .iter()
            .any(|path| explorer::is_virtual_path(path) && !explorer::is_portable_path(path))
        {
            self.status_message = "Paste is not available from this virtual location".into();
            return;
        }
        let archive_items = clipboard
            .paths
            .iter()
            .filter(|path| archive_item_path(path))
            .count();
        if destination_is_portable && archive_items > 0 {
            self.status_message =
                "Extract archive items to a normal folder before copying to a device".into();
            return;
        }
        if archive_items > 0 {
            if archive_items != clipboard.paths.len() {
                self.set_error("Cannot mix archive items and real files in one paste".into());
                return;
            }
            self.extract_archive_items_to(clipboard.paths.clone(), destination);
            return;
        }

        let portable_items = clipboard
            .paths
            .iter()
            .filter(|path| explorer::is_portable_path(path))
            .count();
        if destination_is_portable && portable_items > 0 {
            self.status_message =
                "Copying directly between portable device folders is not available yet".into();
            return;
        }
        if portable_items > 0 && portable_items != clipboard.paths.len() {
            self.set_error("Cannot mix portable device items with real files in one paste".into());
            return;
        }

        let kind = if clipboard.cut && portable_items == 0 {
            TransferKind::Move
        } else {
            TransferKind::Copy
        };
        self.queue_transfer_with_conflict_prompt(
            clipboard.paths.clone(),
            destination,
            kind,
            clipboard.cut && portable_items == 0,
        );
    }

    pub fn confirm_transfer_conflict(&mut self, policy: ConflictPolicy) {
        let Some(conflict) = self.pending_transfer_conflict.take() else {
            return;
        };
        self.enqueue_transfer_job(
            conflict.sources,
            conflict.destination,
            conflict.kind,
            policy,
            conflict.clear_clipboard_on_confirm,
        );
    }

    pub fn cancel_transfer_conflict(&mut self) {
        self.pending_transfer_conflict = None;
        self.status_message = "Transfer cancelled".into();
    }

    pub fn confirm_elevated_transfer(&mut self) {
        let Some(job) = self.pending_elevated_transfer.take() else {
            return;
        };
        self.spawn_operation(
            "Waiting for administrator permission...".into(),
            true,
            move || {
                transfer_queue::run_transfer_elevated(&job)?;
                Ok("Elevated transfer completed".into())
            },
        );
    }

    pub fn cancel_elevated_transfer(&mut self) {
        self.pending_elevated_transfer = None;
        self.status_message = "Elevated transfer cancelled".into();
    }

    pub fn confirm_elevated_file_operation(&mut self) {
        let Some(operation) = self.pending_elevated_operation.take() else {
            return;
        };
        self.spawn_operation(
            "Waiting for administrator permission...".into(),
            true,
            move || {
                operations::run_elevated_file_operation(&operation)?;
                Ok("Elevated operation completed".into())
            },
        );
    }

    pub fn cancel_elevated_file_operation(&mut self) {
        self.pending_elevated_operation = None;
        self.status_message = "Elevated operation cancelled".into();
    }

    fn request_elevated_file_operation(&mut self, operation: operations::ElevatedFileOperation) {
        self.pending_elevated_operation = Some(operation);
        self.status_message = "Administrator permission is needed to finish this operation".into();
    }

    fn handle_file_operation_error(
        &mut self,
        error: crate::utils::errors::BExplorerError,
        operation: operations::ElevatedFileOperation,
    ) {
        let message = error.to_string();
        if file_operation_error_needs_elevation(&message) {
            self.request_elevated_file_operation(operation);
        } else {
            self.set_error(message);
        }
    }

    fn queue_transfer_with_conflict_prompt(
        &mut self,
        sources: Vec<PathBuf>,
        destination: PathBuf,
        kind: TransferKind,
        clear_clipboard_on_confirm: bool,
    ) {
        if let Some(conflict) =
            pending_transfer_conflict(&sources, &destination, kind, clear_clipboard_on_confirm)
        {
            self.pending_transfer_conflict = Some(conflict);
            self.status_message = "File conflict needs a decision".into();
            return;
        }

        self.enqueue_transfer_job(
            sources,
            destination,
            kind,
            ConflictPolicy::KeepBoth,
            clear_clipboard_on_confirm,
        );
    }

    fn enqueue_transfer_job(
        &mut self,
        sources: Vec<PathBuf>,
        destination: PathBuf,
        kind: TransferKind,
        conflict_policy: ConflictPolicy,
        clear_clipboard_on_confirm: bool,
    ) {
        self.next_transfer_id = self.next_transfer_id.saturating_add(1);
        self.transfer_queue.push_back(TransferJob {
            id: self.next_transfer_id,
            sources,
            destination,
            kind,
            conflict_policy,
        });

        if clear_clipboard_on_confirm {
            self.clipboard = None;
            clear_system_clipboard_after_cut();
        }

        self.status_message = if kind == TransferKind::Move {
            "Move queued".into()
        } else {
            "Copy queued".into()
        };
        self.start_next_transfers();
    }

    fn queue_transfer(&mut self, sources: Vec<PathBuf>, destination: PathBuf, kind: TransferKind) {
        let destination_is_portable = explorer::is_portable_path(&destination);
        let portable_sources = sources
            .iter()
            .filter(|source| explorer::is_portable_path(source))
            .count();
        if explorer::is_virtual_path(&destination) && !destination_is_portable
            || sources.iter().any(|source| {
                explorer::is_virtual_path(source) && !explorer::is_portable_path(source)
            })
        {
            self.status_message = "Transfers with this virtual location are not available".into();
            return;
        }
        let archive_items = sources
            .iter()
            .filter(|source| archive_item_path(source))
            .count();

        if destination_is_portable {
            if portable_sources > 0 {
                self.status_message =
                    "Copying directly between portable device folders is not available yet".into();
                return;
            }
            if archive_items > 0 {
                self.status_message =
                    "Extract archive items to a normal folder before copying to a device".into();
                return;
            }
            let sources: Vec<PathBuf> = sources
                .into_iter()
                .filter(|source| source.exists())
                .collect();
            if sources.is_empty() {
                self.status_message = "Nothing to copy".into();
                return;
            }
            self.queue_transfer_with_conflict_prompt(
                sources,
                destination,
                TransferKind::Copy,
                false,
            );
            return;
        }

        if portable_sources > 0 {
            if portable_sources != sources.len() {
                self.set_error(
                    "Cannot mix portable device items with real files in one transfer".into(),
                );
                return;
            }
            if !destination.is_dir() {
                self.status_message = "Drop target is not a folder".into();
                return;
            }
            self.queue_transfer_with_conflict_prompt(
                sources,
                destination,
                TransferKind::Copy,
                false,
            );
            return;
        }

        if !destination.is_dir() {
            self.status_message = "Drop target is not a folder".into();
            return;
        }

        if archive_items > 0 {
            if archive_items != sources.len() {
                self.set_error("Cannot mix archive items and real files in one transfer".into());
                return;
            }
            self.extract_archive_items_to(sources, destination);
            return;
        }

        let sources: Vec<PathBuf> = sources
            .into_iter()
            .filter(|source| source.exists())
            .filter(|source| {
                normalize_existing_path(source)
                    .zip(normalize_existing_path(&destination))
                    .is_some_and(|(source_abs, dest_abs)| {
                        source_abs != dest_abs
                            && !(source_abs.is_dir() && dest_abs.starts_with(&source_abs))
                            && source_abs.parent() != Some(dest_abs.as_path())
                    })
            })
            .collect();
        if sources.is_empty() {
            self.status_message = "Nothing to move".into();
            return;
        }

        self.queue_transfer_with_conflict_prompt(sources, destination, kind, false);
    }

    fn extract_archive_items_to(&mut self, sources: Vec<PathBuf>, destination: PathBuf) {
        if !destination.is_dir() {
            self.status_message = "Extract target is not a folder".into();
            return;
        }
        if crate::fs::archive_listing::is_archive_navigation_path(&destination) {
            self.status_message = "Cannot extract into an archive".into();
            return;
        }
        if sources.is_empty() {
            self.status_message = "No archive items selected".into();
            return;
        }

        let count = sources.len();
        self.spawn_operation(
            format!("Extracting {count} archive item(s)..."),
            true,
            move || {
                let extracted =
                    archive::extract_virtual_paths_to_destination(&sources, &destination)?;
                Ok(format!("Extracted {extracted} archive item(s)"))
            },
        );
    }

    pub fn request_delete_selected(&mut self, permanent: bool) {
        let paths = self.selected_paths();
        if paths.is_empty() {
            self.status_message = "No selected items".into();
            return;
        }
        if paths
            .iter()
            .any(|path| explorer::is_virtual_path(path) && !explorer::is_portable_path(path))
        {
            self.status_message = "Delete is not available for this virtual location".into();
            return;
        }
        let portable_items = paths
            .iter()
            .filter(|path| explorer::is_portable_path(path))
            .count();
        if portable_items > 0 {
            if portable_items != paths.len() {
                self.set_error(
                    "Cannot mix portable device items with real files in one delete".into(),
                );
                return;
            }
            self.confirm_permanent_delete = Some(paths);
            self.delete_panel_spawned = false;
            return;
        }

        if permanent {
            self.confirm_permanent_delete = Some(paths);
            self.delete_panel_spawned = false;
        } else {
            self.delete_paths(paths, false);
        }
    }

    pub fn delete_paths(&mut self, paths: Vec<PathBuf>, permanent: bool) {
        if paths.iter().all(|path| explorer::is_portable_path(path)) {
            self.spawn_operation("Deleting portable item(s)...".into(), true, move || {
                let count = portable::delete_paths(&paths)?;
                Ok(format!("Portable delete completed: {count} item(s)"))
            });
            return;
        }

        if paths.iter().any(|path| explorer::is_virtual_path(path)) {
            self.status_message = "Delete is not available for this virtual location".into();
            return;
        }

        let label = if permanent {
            "Permanent delete"
        } else {
            "Move to trash"
        };

        let elevated_operation = operations::ElevatedFileOperation::Delete {
            paths: paths.clone(),
            permanent,
        };
        self.spawn_elevatable_operation(
            format!("{label} in progress..."),
            true,
            elevated_operation,
            move || {
                let count = if permanent {
                    operations::delete_permanently(&paths)?
                } else {
                    operations::delete_to_trash(&paths)?
                };
                Ok(format!("{label} completed: {count} item(s)"))
            },
        );
    }

    pub fn begin_rename(&mut self, path: PathBuf) {
        if let Some(entry) = self
            .filtered_entries()
            .into_iter()
            .find(|entry| entry.path == path)
        {
            self.begin_rename_entry(entry);
            return;
        }

        let value = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_string();
        self.begin_rename_with_value(path, value);
    }

    pub fn begin_rename_entry(&mut self, entry: FileEntry) {
        let path = entry.path.clone();
        let value = editable_entry_name(&entry);
        self.begin_rename_with_value(path, value);
    }

    fn begin_rename_with_value(&mut self, path: PathBuf, value: String) {
        let end = initial_rename_selection_end(&path, &value);
        self.rename_dialog = Some(RenameDialog {
            path,
            value,
            select_range: Some((0, end)),
        });
    }

    pub fn rename_selected(&mut self) {
        let Some(path) = self.selected.iter().next().cloned() else {
            self.status_message = "No selected item".into();
            return;
        };
        if explorer::is_virtual_path(&path) {
            self.status_message = "Rename is not available for virtual locations yet".into();
            return;
        }
        self.begin_rename(path);
    }

    pub fn apply_rename(&mut self) {
        let Some(dialog) = self.rename_dialog.take() else {
            return;
        };
        let renaming_drive = self
            .storage_entries
            .iter()
            .any(|entry| entry.path == dialog.path && entry.kind == EntryKind::Drive);

        let elevated_operation = operations::ElevatedFileOperation::Rename {
            path: dialog.path.clone(),
            new_name: dialog.value.clone(),
        };
        match operations::rename_path(&dialog.path, &dialog.value) {
            Ok(new_path) => {
                if renaming_drive {
                    self.refresh_storage();
                }
                self.selected.clear();
                self.selection_anchor = Some(new_path.clone());
                self.selection_focus = Some(new_path.clone());
                self.selected.insert(new_path);
                self.status_message = "Renamed".into();
                self.refresh_active_tab();
            }
            Err(error) => self.handle_file_operation_error(error, elevated_operation),
        }
    }

    pub fn create_folder(&mut self) {
        let Some(parent) = self.active_path() else {
            self.status_message = "Open a folder before creating items".into();
            return;
        };
        if explorer::is_virtual_path(&parent) {
            self.status_message =
                "Creating folders in virtual locations is not available yet".into();
            return;
        }
        let name = if self.config.language == "es" {
            "Nueva carpeta"
        } else {
            "New Folder"
        };
        let elevated_operation = operations::ElevatedFileOperation::CreateFolder {
            parent: parent.clone(),
            name: name.to_string(),
        };
        let result = operations::create_folder_named(&parent, name);
        match result {
            Ok(new_path) => {
                self.queue_rename_for_new_item(new_path);
                self.status_message = "Folder created".into();
            }
            Err(error) => self.handle_file_operation_error(error, elevated_operation),
        }
    }

    pub fn create_text_document(&mut self) {
        let Some(parent) = self.active_path() else {
            self.status_message = "Open a folder before creating items".into();
            return;
        };
        if explorer::is_virtual_path(&parent) {
            self.status_message = "Creating files in virtual locations is not available yet".into();
            return;
        }
        let name = if self.config.language == "es" {
            "Nuevo documento de texto.txt"
        } else {
            "New Text Document.txt"
        }
        .to_string();
        let elevated_operation = operations::ElevatedFileOperation::CreateFile {
            parent: parent.clone(),
            name: name.clone(),
        };
        let result = operations::create_empty_file_named(&parent, &name);
        match result {
            Ok(new_path) => {
                self.queue_rename_for_new_item(new_path);
                self.status_message = "Text document created".into();
            }
            Err(error) => self.handle_file_operation_error(error, elevated_operation),
        }
    }

    fn queue_rename_for_new_item(&mut self, path: PathBuf) {
        if !self.filter.is_empty() || self.showing_complete_search_results() {
            self.filter.clear();
            self.cancel_complete_search();
            self.searching = false;
            self.search_results.clear();
        }
        self.selected.clear();
        self.selection_anchor = Some(path.clone());
        self.selection_focus = Some(path.clone());
        self.selected.insert(path.clone());
        self.pending_rename = Some(path);
        self.refresh_active_tab();
    }

    #[allow(dead_code)]
    pub fn duplicate_path(&mut self, path: PathBuf) {
        let elevated_operation =
            operations::ElevatedFileOperation::Duplicate { path: path.clone() };
        self.spawn_elevatable_operation(
            "Duplicating item...".into(),
            true,
            elevated_operation,
            move || operations::duplicate_path(&path).map(|_| "Duplicate created".into()),
        );
    }

    fn unique_archive_destination_for_queue(&self, destination: PathBuf) -> PathBuf {
        if !self.archive_destination_reserved(&destination) {
            return destination;
        }

        let parent = destination.parent().unwrap_or_else(|| Path::new(""));
        let stem = destination
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("Archive");
        let extension = destination.extension().and_then(|value| value.to_str());

        for index in 1..10_000 {
            let name = if let Some(extension) = extension {
                format!("{stem} ({index}).{extension}")
            } else {
                format!("{stem} ({index})")
            };
            let candidate = parent.join(name);
            if !self.archive_destination_reserved(&candidate) {
                return candidate;
            }
        }

        destination
    }

    fn archive_destination_reserved(&self, destination: &Path) -> bool {
        if destination.exists() {
            return true;
        }

        self.active_archives.values().any(|active| {
            active.job.kind == ArchiveJobKind::Compress && active.job.destination == destination
        }) || self
            .archive_queue
            .iter()
            .any(|job| job.kind == ArchiveJobKind::Compress && job.destination == destination)
    }

    pub fn open_compress_dialog(&mut self) {
        let sources = self.selected_paths();
        if sources.is_empty() {
            self.status_message = "No selected items".into();
            return;
        }
        if sources.iter().any(|path| explorer::is_virtual_path(path)) {
            self.status_message = "Compression is not available for virtual locations yet".into();
            return;
        }

        let destination_dir = self
            .active_path()
            .filter(|path| path.is_dir())
            .or_else(|| {
                sources
                    .first()
                    .and_then(|path| path.parent())
                    .map(Path::to_path_buf)
            })
            .unwrap_or_else(|| PathBuf::from("."));
        let format = ArchiveFormat::Zip;
        let default_name = default_compress_dialog_name(&sources, &destination_dir, format);

        self.compress_dialog = Some(CompressDialog {
            sources,
            destination_dir,
            name: default_name,
            format,
            method: ArchiveCompressionMethod::default(),
            password: String::new(),
            confirm_password: String::new(),
            password_mismatch: false,
        });
    }

    pub fn confirm_compress_dialog(&mut self) {
        let Some(mut dialog) = self.compress_dialog.take() else {
            return;
        };
        if dialog.password != dialog.confirm_password {
            dialog.password_mismatch = true;
            self.compress_dialog = Some(dialog);
            return;
        }
        self.compress_sources(
            dialog.sources,
            dialog.destination_dir,
            dialog.name,
            dialog.format,
            dialog.method,
            password_from_input(dialog.password),
        );
    }

    pub fn cancel_compress_dialog(&mut self) {
        self.compress_dialog = None;
    }

    pub fn compress_selected_as(&mut self, format: ArchiveFormat) {
        let sources = self.selected_paths();
        if sources.is_empty() {
            self.status_message = "No selected items".into();
            return;
        }
        if sources.iter().any(|path| explorer::is_virtual_path(path)) {
            self.status_message = "Compression is not available for virtual locations yet".into();
            return;
        }

        let destination =
            match archive::default_archive_path(&sources, self.active_path().as_deref(), format) {
                Ok(path) => path,
                Err(error) => {
                    self.set_error(error.to_string());
                    return;
                }
            };
        let destination_dir = destination
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let name = destination
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("Archive")
            .to_string();
        self.compress_sources(
            sources,
            destination_dir,
            name,
            format,
            ArchiveCompressionMethod::Normal,
            None,
        );
    }

    fn compress_sources(
        &mut self,
        sources: Vec<PathBuf>,
        destination_dir: PathBuf,
        name: String,
        format: ArchiveFormat,
        method: ArchiveCompressionMethod,
        password: Option<String>,
    ) {
        if sources.is_empty() {
            self.status_message = "No selected items".into();
            return;
        }

        let file_name = archive_file_name_from_input(&name, format);
        let destination = destination_dir.join(file_name);
        let destination = self.unique_archive_destination_for_queue(destination);
        let label = match format {
            ArchiveFormat::Zip => "ZIP",
            ArchiveFormat::SevenZip => "7z",
        };

        self.next_archive_id = self.next_archive_id.saturating_add(1);
        let job = ArchiveJob {
            id: self.next_archive_id,
            kind: ArchiveJobKind::Compress,
            format,
            method,
            password,
            sources,
            destination,
            archive_path: PathBuf::new(),
            extract_mode: ExtractMode::Here,
        };
        self.archive_queue.push_back(job);
        self.status_message = format!("Compressing {label} archive queued...");
        self.start_next_archives();
    }

    pub fn extract_archive(&mut self, path: PathBuf, mode: ExtractMode) {
        if !path.is_file() {
            self.status_message = "Select an archive to extract".into();
            return;
        }

        let format = if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
        {
            ArchiveFormat::Zip
        } else {
            ArchiveFormat::SevenZip
        };

        let destination = match archive::planned_extract_destination(&path, mode) {
            Ok(destination) => destination,
            Err(error) => {
                self.set_error(error.to_string());
                return;
            }
        };

        self.next_archive_id = self.next_archive_id.saturating_add(1);
        let job = ArchiveJob {
            id: self.next_archive_id,
            kind: ArchiveJobKind::Extract,
            format,
            method: ArchiveCompressionMethod::default(),
            password: None,
            sources: Vec::new(),
            destination,
            archive_path: path,
            extract_mode: mode,
        };
        self.archive_queue.push_back(job);
        self.status_message = "Extracting archive queued...".into();
        self.start_next_archives();
    }

    pub fn confirm_archive_password_dialog(&mut self) {
        let Some(mut dialog) = self.archive_password_dialog.take() else {
            return;
        };
        if dialog.password.is_empty() {
            dialog.error = Some("Password is required".into());
            self.archive_password_dialog = Some(dialog);
            return;
        }
        dialog.job.password = Some(dialog.password);
        self.archive_queue.push_front(dialog.job);
        self.status_message = "Extracting protected archive queued...".into();
        self.start_next_archives();
    }

    pub fn cancel_archive_password_dialog(&mut self) {
        self.archive_password_dialog = None;
    }

    fn open_archive_password_dialog(&mut self, job: ArchiveJob, error: String) {
        self.archive_password_dialog = Some(ArchivePasswordDialog {
            job,
            password: String::new(),
            error: Some(error),
        });
        self.status_message = "Archive password required".into();
    }

    fn should_prompt_archive_password(job: &ArchiveJob, error: &str) -> bool {
        job.kind == ArchiveJobKind::Extract
            && !job.has_password()
            && archive::archive_error_may_need_password(error)
    }

    #[allow(dead_code)]
    pub fn add_favorite(&mut self, path: PathBuf) {
        if !path.is_dir() {
            self.status_message = "Only folders can be favorites".into();
            return;
        }

        if !self.config.favorites.contains(&path) {
            self.config.favorites.push(path);
            self.save_config();
        }
        self.status_message = "Favorite saved".into();
    }

    pub fn remove_favorite(&mut self, path: &Path) {
        self.config.favorites.retain(|item| item != path);
        self.save_config();
    }

    pub fn is_favorite(&self, path: &Path) -> bool {
        self.config.favorites.iter().any(|p| p == path)
    }

    pub fn toggle_favorite(&mut self, path: PathBuf) {
        if self.is_favorite(&path) {
            self.remove_favorite(&path);
            self.status_message = "Removed from favorites".into();
        } else {
            self.add_favorite(path);
        }
    }

    #[allow(dead_code)]
    pub fn copy_path_to_clipboard(&mut self, path: &Path) {
        match crate::platform::shell::copy_text(&path.display().to_string()) {
            Ok(()) => self.status_message = "Path copied".into(),
            Err(error) => self.set_error(error.to_string()),
        }
    }

    pub fn copy_current_path_to_clipboard(&mut self) {
        let text = self
            .active_path()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "This PC".into());
        match crate::platform::shell::copy_text(&text) {
            Ok(()) => self.status_message = "Current path copied".into(),
            Err(error) => self.set_error(error.to_string()),
        }
    }

    pub fn open_terminal_at(&mut self, path: &Path) {
        if explorer::is_virtual_path(path) {
            self.status_message = "Terminal is not available for virtual locations".into();
            return;
        }
        match crate::platform::shell::open_terminal_at(path) {
            Ok(()) => self.status_message = "Terminal opened".into(),
            Err(error) => self.set_error(error.to_string()),
        }
    }

    pub fn run_command(&mut self, command: AppCommand) {
        match command {
            AppCommand::NewTab => self.new_tab(None),
            AppCommand::CloseTab => self.close_tab(self.active_tab),
            AppCommand::CopyPath => self.copy_current_path_to_clipboard(),
            AppCommand::ToggleHidden => {
                self.config.show_hidden = !self.config.show_hidden;
                self.save_config();
                self.refresh_active_tab();
            }
            AppCommand::ToggleTheme => {
                self.config.theme = match self.config.theme {
                    ThemePreference::Dark => ThemePreference::Light,
                    ThemePreference::Light | ThemePreference::Gray => ThemePreference::Dark,
                };
                self.save_config();
            }
            AppCommand::Refresh => self.refresh_active_tab(),
            AppCommand::GoUp => self.go_up(),
            AppCommand::Rename => self.rename_selected(),
        }
    }

    fn selected_paths(&self) -> Vec<PathBuf> {
        self.selected.iter().cloned().collect()
    }

    /// Poll per-pane background receivers (load_rx, search_rx).
    /// Only operates on the pane currently in `self` fields.
    fn poll_pane_background(&mut self, ctx: &egui::Context) {
        while let Some(message) = self
            .load_rx
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok())
        {
            if message.request_id != self.next_request_id {
                continue;
            }

            self.loading = !message.finished;
            match message.result {
                Ok(entries) => {
                    let previous_entries = self.entries.clone();
                    if message.append {
                        merge_load_entries(&mut self.entries, entries);
                    } else {
                        self.entries = entries;
                        explorer::sort_entries_by_name(&mut self.entries);
                    }
                    let entries_changed = !file_entries_equal(&previous_entries, &self.entries);
                    if entries_changed {
                        self.selected.clear();
                        self.selection_anchor = None;
                        self.selection_focus = None;
                        self.type_select = None;
                        if self.pending_select.is_none() {
                            self.pending_scroll_path = None;
                        }
                    }
                    if let Some(path) = self.pending_select.take() {
                        if self.entries.iter().any(|entry| entry.path == path) {
                            self.selection_anchor = Some(path.clone());
                            self.selection_focus = Some(path.clone());
                            self.selected.insert(path.clone());
                            self.pending_scroll_path = Some(path);
                        }
                    }
                    if let Some(path) = self.pending_rename.take() {
                        if self.entries.iter().any(|entry| entry.path == path) {
                            self.selection_anchor = Some(path.clone());
                            self.selection_focus = Some(path.clone());
                            self.selected.insert(path.clone());
                            self.begin_rename(path);
                        }
                    }
                    if entries_changed {
                        self.retain_visible_thumbnails();
                    }
                    self.status_message = if self.loading {
                        format!("Detectando red... {} elemento(s)", self.entries.len())
                    } else if self.showing_complete_search_results() {
                        format!("Search results: {} item(s)", self.search_results.len())
                    } else {
                        "Ready".into()
                    };
                    if self.loading {
                        ctx.request_repaint_after(Duration::from_millis(16));
                    } else {
                        ctx.request_repaint();
                    }
                }
                Err(error) => {
                    self.entries.clear();
                    self.set_error(error);
                    self.status_message = "Could not load folder".into();
                    ctx.request_repaint();
                }
            }
        }

        let mut search_messages = 0;
        while search_messages < 4 {
            let Some(message) = self
                .search_rx
                .as_ref()
                .and_then(|receiver| receiver.try_recv().ok())
            else {
                break;
            };
            search_messages += 1;

            if message.request_id != self.next_search_request_id {
                continue;
            }
            if self.filter.trim() != message.query
                || self.active_path().as_ref() != Some(&message.root)
            {
                continue;
            }

            match message.event {
                search::SearchEvent::Batch(entries) => {
                    self.search_results.extend(entries);
                    self.status_message =
                        format!("Searching... {} item(s)", self.search_results.len());
                    ctx.request_repaint_after(Duration::from_millis(16));
                }
                search::SearchEvent::Finished { truncated } => {
                    self.searching = false;
                    self.search_cancel = None;
                    self.search_rx = None;
                    self.sort_search_results();
                    self.retain_visible_thumbnails();
                    self.status_message = if truncated {
                        format!("Search results: {}+ item(s)", self.search_results.len())
                    } else {
                        format!("Search results: {} item(s)", self.search_results.len())
                    };
                    self.save_current_tab_search_state();
                    ctx.request_repaint();
                    break;
                }
            }
        }

        if self.searching {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }

    /// Poll shared background receivers (operation_rx, transfer_rx, thumbnail_rx).
    fn poll_global_background(&mut self, ctx: &egui::Context) {
        let mut clear_operation_rx = false;
        loop {
            let message = match self
                .operation_rx
                .as_ref()
                .map(|receiver| receiver.try_recv())
            {
                Some(Ok(message)) => message,
                Some(Err(TryRecvError::Empty)) | None => break,
                Some(Err(TryRecvError::Disconnected)) => {
                    clear_operation_rx = true;
                    break;
                }
            };

            clear_operation_rx = true;
            if let Some(operation) = message.elevated_operation {
                self.request_elevated_file_operation(operation);
            } else {
                match message.result {
                    Ok(status) => self.status_message = status,
                    Err(error) => self.set_error(error),
                }
            }

            if message.refresh {
                self.refresh_all_panes();
            }
        }
        if clear_operation_rx {
            self.operation_rx = None;
        }

        let mut clear_defender_rx = false;
        loop {
            let message = match self
                .defender_rx
                .as_ref()
                .map(|receiver| receiver.try_recv())
            {
                Some(Ok(message)) => message,
                Some(Err(TryRecvError::Empty)) | None => break,
                Some(Err(TryRecvError::Disconnected)) => {
                    clear_defender_rx = true;
                    break;
                }
            };

            match message {
                DefenderMessage::Progress(progress) => {
                    self.defender_progress = Some(progress);
                    ctx.request_repaint_after(Duration::from_millis(16));
                }
                DefenderMessage::Finished(summary) => {
                    clear_defender_rx = true;
                    self.status_message = if summary.threats.is_empty() {
                        format!(
                            "Windows Defender scan completed: {} item(s)",
                            summary.scanned
                        )
                    } else {
                        format!("Windows Defender found {} threat(s)", summary.threats.len())
                    };
                    self.defender_progress = Some(DefenderProgress {
                        state: DefenderScanState::Finished,
                        current_path: None,
                        scanned: summary.scanned,
                        total: summary.total,
                        threats_found: summary.threats.len(),
                        started: Instant::now(),
                    });
                    self.defender_summary = Some(summary);
                    ctx.request_repaint();
                }
                DefenderMessage::Failed(summary) => {
                    clear_defender_rx = true;
                    self.set_error(
                        summary
                            .error
                            .clone()
                            .unwrap_or_else(|| "Windows Defender scan failed".into()),
                    );
                    self.defender_progress = Some(DefenderProgress {
                        state: DefenderScanState::Failed,
                        current_path: None,
                        scanned: summary.scanned,
                        total: summary.total,
                        threats_found: summary.threats.len(),
                        started: Instant::now(),
                    });
                    self.defender_summary = Some(summary);
                    ctx.request_repaint();
                }
                DefenderMessage::Cancelled(summary) => {
                    clear_defender_rx = true;
                    self.status_message = "Windows Defender scan cancelled".into();
                    self.defender_progress = Some(DefenderProgress {
                        state: DefenderScanState::Cancelled,
                        current_path: None,
                        scanned: summary.scanned,
                        total: summary.total,
                        threats_found: summary.threats.len(),
                        started: Instant::now(),
                    });
                    self.defender_summary = Some(summary);
                    ctx.request_repaint();
                }
            }
        }
        if clear_defender_rx {
            self.defender_rx = None;
            self.defender_cancel = None;
        }

        while let Ok(message) = self.transfer_rx.try_recv() {
            match message {
                TransferMessage::Progress(progress) => {
                    if self.active_transfers.contains_key(&progress.job_id) {
                        self.transfer_progress.insert(progress.job_id, progress);
                        ctx.request_repaint_after(Duration::from_millis(16));
                    }
                }
                TransferMessage::Finished {
                    job_id,
                    kind,
                    completed_files,
                } => {
                    if self.active_transfers.remove(&job_id).is_some() {
                        let mut progress =
                            self.transfer_progress.remove(&job_id).unwrap_or_else(|| {
                                TransferProgress {
                                    job_id,
                                    kind,
                                    state: TransferState::Finished,
                                    current_name: String::new(),
                                    destination: PathBuf::new(),
                                    copied_bytes: 0,
                                    total_bytes: 0,
                                    files_done: completed_files,
                                    total_files: completed_files,
                                    bytes_per_second: 0.0,
                                }
                            });
                        progress.state = TransferState::Finished;
                        progress.files_done = completed_files;
                        self.push_transfer_history(progress);
                        self.status_message = format!(
                            "{} completed: {} item(s)",
                            if kind == TransferKind::Move {
                                "Move"
                            } else {
                                "Copy"
                            },
                            completed_files
                        );
                        self.refresh_all_panes();
                        self.start_next_transfers();
                    }
                }
                TransferMessage::Failed { job_id, error } => {
                    if let Some(active) = self.active_transfers.remove(&job_id) {
                        let progress = self.transfer_progress.remove(&job_id);
                        if transfer_error_needs_elevation(&error, &active.job) {
                            self.pending_elevated_transfer = Some(active.job);
                            self.status_message =
                                "Administrator permission is needed to finish this transfer".into();
                        } else {
                            if let Some(mut progress) = progress {
                                progress.state = TransferState::Failed;
                                self.push_transfer_history(progress);
                            }
                            self.set_error(error);
                        }
                        self.start_next_transfers();
                    }
                }
                TransferMessage::Cancelled { job_id } => {
                    if self.active_transfers.remove(&job_id).is_some() {
                        if let Some(mut progress) = self.transfer_progress.remove(&job_id) {
                            progress.state = TransferState::Cancelled;
                            self.push_transfer_history(progress);
                        }
                        self.status_message = "Transfer cancelled".into();
                        self.refresh_all_panes();
                        self.start_next_transfers();
                    }
                }
            }
        }

        // Poll archive progress â€” iterate over all active archives
        let active_ids: Vec<u64> = self.active_archives.keys().copied().collect();
        for job_id in active_ids {
            let Some(active) = self.active_archives.remove(&job_id) else {
                continue;
            };
            let kind = active.job.kind;
            let job_name = active.job.display_name();
            let mut keep = true;
            while let Ok(message) = active.rx.try_recv() {
                match message {
                    ArchiveProgressMsg::Progress(p) => {
                        if let Some(state) = self.archive_progress.get_mut(&job_id) {
                            state.update(&p);
                        } else {
                            self.archive_progress
                                .insert(job_id, ArchiveProgressState::new(&p, kind, &job_name));
                        }
                        ctx.request_repaint();
                        ctx.request_repaint_after(Duration::from_millis(16));
                    }
                    ArchiveProgressMsg::Finished(result) => {
                        keep = false;
                        let (completed, total) = self
                            .archive_progress
                            .get(&job_id)
                            .map(|s| (s.completed, s.total))
                            .unwrap_or((0, 0));
                        if let Err(error) = &result
                            && Self::should_prompt_archive_password(&active.job, error)
                        {
                            self.archive_progress.remove(&job_id);
                            self.open_archive_password_dialog(active.job.clone(), error.clone());
                            ctx.request_repaint();
                            self.start_next_archives();
                            continue;
                        }
                        let hist_state = match &result {
                            Ok(_) => ArchiveState::Finished,
                            Err(_) => ArchiveState::Failed,
                        };
                        self.archive_progress.remove(&job_id);
                        self.archive_history.push_back(ArchiveHistoryItem {
                            id: job_id,
                            kind,
                            state: hist_state,
                            name: job_name.clone(),
                            completed,
                            total,
                            finished_at: Instant::now(),
                        });
                        while self.archive_history.len() > 8 {
                            self.archive_history.pop_front();
                        }
                        if result.is_err() {
                            Self::cleanup_partial_archive_destination(&active.job);
                        }
                        match result {
                            Ok(dest) => {
                                self.status_message =
                                    format!("Archive operation completed: {}", dest.display());
                            }
                            Err(error) => self.set_error(error),
                        }
                        self.refresh_storage();
                        self.refresh_active_tab();
                        self.start_next_archives();
                    }
                    ArchiveProgressMsg::Cancelled => {
                        keep = false;
                        let (completed, total) = self
                            .archive_progress
                            .get(&job_id)
                            .map(|s| (s.completed, s.total))
                            .unwrap_or((0, 0));
                        self.archive_progress.remove(&job_id);
                        self.archive_history.push_back(ArchiveHistoryItem {
                            id: job_id,
                            kind,
                            state: ArchiveState::Cancelled,
                            name: job_name.clone(),
                            completed,
                            total,
                            finished_at: Instant::now(),
                        });
                        while self.archive_history.len() > 8 {
                            self.archive_history.pop_front();
                        }
                        Self::cleanup_partial_archive_destination(&active.job);
                        self.status_message = "Archive operation cancelled".into();
                        self.refresh_storage();
                        self.refresh_active_tab();
                        self.start_next_archives();
                    }
                }
            }
            if keep {
                self.active_archives.insert(job_id, active);
            }
        }

        while let Ok(message) = self.thumbnail_rx.try_recv() {
            // Check both current pane and other pane visibility.
            let visible_current = self.visible_contains_path(&message.path);
            let visible_other = self
                .other_pane
                .as_ref()
                .map(|other| other.visible_contains_path(&message.path))
                .unwrap_or(false);
            if !visible_current && !visible_other {
                self.thumbnail_cache.remove(&message.path);
                continue;
            }

            match message.image {
                Some(image) => {
                    let texture = ctx.load_texture(
                        format!("thumb:{}", message.path.display()),
                        image,
                        TextureOptions::LINEAR,
                    );
                    self.thumbnail_cache
                        .insert(message.path, ThumbnailState::Ready(texture));
                    ctx.request_repaint();
                }
                None => {
                    self.thumbnail_cache
                        .insert(message.path, ThumbnailState::Missing);
                }
            }
        }

        while let Ok(message) = self.preview_rx.try_recv() {
            let cache_generation_matches = matches!(
                self.preview_cache.get(&message.path),
                Some(PreviewCacheState::Loading(generation)) if *generation == message.generation
            ) || matches!(
                self.preview_cache.get(&message.path),
                Some(PreviewCacheState::Images { generation, .. }) if *generation == message.generation
            );
            if !cache_generation_matches {
                continue;
            }
            match message.content {
                crate::preview::PreviewContent::Images {
                    images,
                    append,
                    finished,
                    page_count,
                } => {
                    if images.is_empty() {
                        if finished {
                            match self.preview_cache.get_mut(&message.path) {
                                Some(PreviewCacheState::Images { loading, .. }) => {
                                    *loading = false;
                                }
                                _ => {
                                    self.preview_cache
                                        .insert(message.path, PreviewCacheState::Missing);
                                }
                            }
                        }
                        ctx.request_repaint();
                    } else {
                        let textures = images
                            .into_iter()
                            .enumerate()
                            .map(|(index, image)| {
                                ctx.load_texture(
                                    format!("preview:{}:{index}", message.path.display()),
                                    image,
                                    TextureOptions::LINEAR,
                                )
                            })
                            .collect::<Vec<_>>();
                        match self.preview_cache.get_mut(&message.path) {
                            Some(PreviewCacheState::Images {
                                textures: existing,
                                loading,
                                page_count: existing_page_count,
                                ..
                            }) => {
                                if !append {
                                    existing.clear();
                                }
                                existing.extend(textures);
                                *loading = !finished;
                                if page_count.is_some() {
                                    *existing_page_count = page_count;
                                }
                            }
                            _ => {
                                self.preview_cache.insert(
                                    message.path,
                                    PreviewCacheState::Images {
                                        textures,
                                        generation: message.generation,
                                        loading: !finished,
                                        page_count,
                                    },
                                );
                            }
                        }
                        ctx.request_repaint();
                    }
                }
                crate::preview::PreviewContent::Text(text) => {
                    self.preview_cache
                        .insert(message.path, PreviewCacheState::Text(text));
                    ctx.request_repaint();
                }
                crate::preview::PreviewContent::Unsupported => {
                    self.preview_cache
                        .insert(message.path, PreviewCacheState::Missing);
                    ctx.request_repaint();
                }
            }
        }
    }

    fn read_shortcuts(ctx: &egui::Context, bindings: &ShortcutConfig) -> Shortcuts {
        ctx.input(|input| {
            let command = input.modifiers.ctrl || input.modifiers.command;
            let control_text = |value: char| {
                input.events.iter().any(
                    |event| matches!(event, egui::Event::Text(text) if text.chars().eq([value])),
                )
            };
            let shortcut = |action| shortcut_pressed(input, bindings.binding(action));
            let default_copy = shortcut_binding_is(
                bindings.binding(ShortcutAction::Copy),
                "C",
                true,
                false,
                false,
            );
            let default_cut = shortcut_binding_is(
                bindings.binding(ShortcutAction::Cut),
                "X",
                true,
                false,
                false,
            );
            let default_paste = shortcut_binding_is(
                bindings.binding(ShortcutAction::Paste),
                "V",
                true,
                false,
                false,
            );
            let native_cut = input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Cut));
            let native_paste = input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Paste(_)));
            Shortcuts {
                copy_event: command
                    && !input.modifiers.shift
                    && input
                        .events
                        .iter()
                        .any(|event| matches!(event, egui::Event::Copy))
                    && default_copy,
                cut_event: command && !input.modifiers.shift && native_cut && default_cut,
                paste_event: native_paste && default_paste,
                command_palette: shortcut(ShortcutAction::CommandPalette),
                copy: shortcut(ShortcutAction::Copy) || (default_copy && control_text('\u{3}')),
                cut: shortcut(ShortcutAction::Cut) || (default_cut && control_text('\u{18}')),
                paste: shortcut(ShortcutAction::Paste) || (default_paste && control_text('\u{16}')),
                select_all: shortcut(ShortcutAction::SelectAll),
                refresh: shortcut(ShortcutAction::Refresh),
                enter: shortcut(ShortcutAction::Open),
                properties: shortcut(ShortcutAction::Properties),
                rename: shortcut(ShortcutAction::Rename),
                delete: shortcut(ShortcutAction::Delete) || (!command && native_cut && default_cut),
                permanent_delete: shortcut(ShortcutAction::PermanentDelete),
                up: shortcut(ShortcutAction::GoUp),
                arrow_up: shortcut_pressed_allow_extend(
                    input,
                    bindings.binding(ShortcutAction::MoveUp),
                ),
                arrow_down: shortcut_pressed_allow_extend(
                    input,
                    bindings.binding(ShortcutAction::MoveDown),
                ),
                extend_selection: input.modifiers.shift,
                alt_left: shortcut(ShortcutAction::GoBack),
                alt_right: shortcut(ShortcutAction::GoForward),
                type_select: if !command && !input.modifiers.alt {
                    input.events.iter().find_map(|event| {
                        if let egui::Event::Text(text) = event {
                            let mut chars = text.chars();
                            let character = chars.next()?;
                            if chars.next().is_none() {
                                normalized_type_select_char(character)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                } else {
                    None
                },
            }
        })
    }

    fn handle_shortcuts(&mut self, shortcuts: Shortcuts) {
        let modal_open = self.modal_keyboard_capture_active();

        if shortcuts.command_palette && !modal_open {
            self.command_palette_open = true;
            return;
        }

        if self.command_palette_open || modal_open {
            return;
        }

        if shortcuts.alt_left {
            self.go_back();
            return;
        }
        if shortcuts.alt_right {
            self.go_forward();
            return;
        }

        if self.text_input_active {
            return;
        }

        if shortcuts.properties {
            self.show_selected_or_current_properties();
        } else if shortcuts.permanent_delete {
            self.request_delete_selected(true);
        } else if shortcuts.delete {
            self.request_delete_selected(false);
        } else if shortcuts.copy || shortcuts.copy_event {
            self.copy_selection(false);
        } else if shortcuts.cut || shortcuts.cut_event {
            self.copy_selection(true);
        } else if shortcuts.paste || shortcuts.paste_event {
            self.paste_into_active();
        } else if shortcuts.select_all {
            self.select_all();
        } else if shortcuts.refresh {
            self.refresh_active_tab();
        } else if shortcuts.enter {
            self.open_selected();
        } else if shortcuts.rename {
            self.rename_selected();
        } else if shortcuts.up {
            self.go_up();
        } else if shortcuts.arrow_up {
            self.move_selection(-1, shortcuts.extend_selection);
        } else if shortcuts.arrow_down {
            self.move_selection(1, shortcuts.extend_selection);
        } else if let Some(character) = shortcuts.type_select {
            self.select_next_entry_starting_with(character);
        }
    }

    fn apply_platform_paste_shortcut(&mut self, shortcuts: &mut Shortcuts, paste_down: bool) {
        if paste_down && !self.paste_shortcut_down {
            shortcuts.paste = true;
        }
        self.paste_shortcut_down = paste_down;
    }

    fn modal_keyboard_capture_active(&self) -> bool {
        self.options_menu_open
            || self.action_bar_new_menu_open
            || self.options_open
            || self.shortcuts_open
            || self.rename_dialog.is_some()
            || self.compress_dialog.is_some()
            || self.archive_password_dialog.is_some()
            || self.confirm_permanent_delete.is_some()
            || self.pending_transfer_conflict.is_some()
            || self.pending_elevated_transfer.is_some()
            || self.pending_elevated_operation.is_some()
            || self.error_message.is_some()
    }

    fn sort_entries(&mut self) {
        sort_file_entries(&mut self.entries, self.sort, self.sort_ascending);
    }

    fn sort_search_results(&mut self) {
        sort_file_entries(&mut self.search_results, self.sort, self.sort_ascending);
    }

    fn start_next_transfers(&mut self) {
        while self.active_transfers.len() < self.max_parallel_transfers {
            let Some(job) = self.transfer_queue.pop_front() else {
                break;
            };

            let control = TransferControl::new();
            let worker_control = control.clone();
            let worker_job = job.clone();
            let tx = self.transfer_tx.clone();

            self.transfer_progress
                .insert(job.id, TransferProgress::pending(&job));
            self.status_message = if job.kind == TransferKind::Move {
                "Moving...".into()
            } else {
                "Copying...".into()
            };
            self.active_transfers
                .insert(job.id, ActiveTransfer { job, control });

            thread::spawn(move || {
                transfer_queue::run_transfer(worker_job, tx, worker_control);
            });
        }
    }

    fn push_transfer_history(&mut self, progress: TransferProgress) {
        self.transfer_history.push_back(TransferHistoryItem {
            progress,
            finished_at: Instant::now(),
        });
        while self.transfer_history.len() > 8 {
            self.transfer_history.pop_front();
        }
    }

    fn prune_transfer_history(&mut self) {
        let visible_for = Duration::from_secs(1);
        while self
            .transfer_history
            .front()
            .is_some_and(|item| item.finished_at.elapsed() >= visible_for)
        {
            self.transfer_history.pop_front();
        }
    }

    fn sync_complete_search(&mut self) {
        if self.filter.trim().is_empty() {
            let was_searching = self.searching;
            self.cancel_complete_search();
            self.search_results.clear();
            self.searching = false;
            if was_searching || self.status_message == "Searching..." {
                self.status_message = "Ready".into();
            }
            return;
        }

        let Some(root) = self.active_path() else {
            self.cancel_complete_search();
            self.search_results.clear();
            self.searching = false;
            self.status_message = "Open a folder before search".into();
            return;
        };
        if explorer::is_virtual_path(&root) && !explorer::is_portable_path(&root) {
            self.cancel_complete_search();
            self.search_results.clear();
            self.searching = false;
            self.status_message = "Search is not available in virtual locations yet".into();
            return;
        }

        self.cancel_complete_search();
        self.search_results.clear();
        self.selected.clear();
        self.selection_anchor = None;
        self.selection_focus = None;
        self.next_search_request_id = self.next_search_request_id.saturating_add(1);
        let request_id = self.next_search_request_id;
        let query = self.filter.trim().to_string();
        let show_hidden = self.config.show_hidden;
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let worker_root = root.clone();
        let worker_query = query.clone();
        let include_archives = self.search_mode == SearchMode::Complete;

        self.search_rx = Some(rx);
        self.search_cancel = Some(cancel);
        self.searching = true;
        self.status_message = "Searching...".into();

        thread::spawn(move || {
            let batch_tx = tx.clone();
            let batch_query = worker_query.clone();
            let batch_root = worker_root.clone();
            let batch_cancel = Arc::clone(&worker_cancel);
            let result = search::search_files_streaming(
                SearchOptions {
                    root: worker_root.clone(),
                    query: worker_query.clone(),
                    show_hidden,
                    include_archives,
                },
                &worker_cancel,
                move |entries| {
                    if batch_cancel.load(AtomicOrdering::Relaxed) {
                        return false;
                    }
                    batch_tx
                        .send(SearchMessage {
                            request_id,
                            query: batch_query.clone(),
                            root: batch_root.clone(),
                            event: search::SearchEvent::Batch(entries),
                        })
                        .is_ok()
                },
            );
            if !worker_cancel.load(AtomicOrdering::Relaxed) {
                let _ = tx.send(SearchMessage {
                    request_id,
                    query: worker_query,
                    root: worker_root,
                    event: search::SearchEvent::Finished {
                        truncated: result.truncated,
                    },
                });
            }
        });
    }

    fn cancel_complete_search(&mut self) {
        if let Some(cancel) = self.search_cancel.take() {
            cancel.store(true, AtomicOrdering::Relaxed);
        }
    }

    fn spawn_operation<F>(&mut self, status: String, refresh: bool, operation: F)
    where
        F: FnOnce() -> Result<String> + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        self.operation_rx = Some(rx);
        self.status_message = status;

        thread::spawn(move || {
            let result = operation().map_err(|error| error.to_string());
            let _ = tx.send(OperationMessage {
                refresh,
                result,
                elevated_operation: None,
            });
        });
    }

    fn spawn_elevatable_operation<F>(
        &mut self,
        status: String,
        refresh: bool,
        elevated_operation: operations::ElevatedFileOperation,
        operation: F,
    ) where
        F: FnOnce() -> Result<String> + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        self.operation_rx = Some(rx);
        self.status_message = status;

        thread::spawn(move || {
            let result = operation().map_err(|error| error.to_string());
            let elevated_operation = result
                .as_ref()
                .err()
                .filter(|error| file_operation_error_needs_elevation(error))
                .map(|_| elevated_operation);
            let _ = tx.send(OperationMessage {
                refresh,
                result,
                elevated_operation,
            });
        });
    }

    #[allow(dead_code)]
    fn run_and_refresh<F>(&mut self, operation: F)
    where
        F: FnOnce() -> Result<String> + Send + 'static,
    {
        self.spawn_operation("Working...".into(), true, operation);
    }

    pub fn save_config(&self) {
        if let Err(error) = self.config.save() {
            crate::utils::log::error(format!("Config save failed: {error}"));
        }
    }

    fn handle_external_file_drop(&mut self, ctx: &egui::Context) {
        let dropped_files = ctx.input(|input| input.raw.dropped_files.clone());
        if dropped_files.is_empty() {
            return;
        }

        let sources: Vec<PathBuf> = dropped_files
            .iter()
            .filter_map(|file| file.path.clone())
            .collect();
        if sources.is_empty() {
            self.status_message =
                "This drop does not expose file paths BExplorer can copy yet".into();
            return;
        }

        let Some(destination) = self.external_drop_destination(ctx) else {
            self.status_message = "Open a folder before dropping files here".into();
            return;
        };

        self.queue_transfer(sources, destination, TransferKind::Copy);
    }

    fn try_start_native_file_drag_outside_window(
        &mut self,
        _ctx: &egui::Context,
        _frame: &eframe::Frame,
    ) {
        #[cfg(target_os = "windows")]
        {
            use raw_window_handle::HasWindowHandle;

            let Some(drag) = self.file_drag.as_ref() else {
                return;
            };
            if !_ctx.input(|input| input.pointer.primary_down()) {
                return;
            }
            let outside_window = _frame.window_handle().ok().is_some_and(|handle| {
                crate::platform::windows::cursor_is_outside_window(handle.as_raw(), 8)
            });
            if !outside_window {
                return;
            }

            let paths = drag.paths.clone();
            if paths.iter().any(|path| {
                explorer::is_virtual_path(path) || archive_item_path(path) || !path.exists()
            }) {
                self.file_drag = None;
                self.status_message =
                    "Dragging this item outside BExplorer is not available yet".into();
                return;
            }

            self.file_drag = None;
            crate::platform::windows::release_mouse_capture();
            match crate::platform::windows::start_file_drag(paths) {
                Ok(result) => {
                    let item_count = result.paths.len();
                    match result.effect {
                        crate::platform::windows::NativeDragEffect::Move => {
                            self.status_message =
                                format!("Moved {item_count} item(s) through Windows drag and drop");
                            self.refresh_all_panes();
                        }
                        crate::platform::windows::NativeDragEffect::Copy
                        | crate::platform::windows::NativeDragEffect::Link => {
                            self.status_message = format!(
                                "Copied {item_count} item(s) through Windows drag and drop"
                            );
                            self.refresh_all_panes();
                        }
                        crate::platform::windows::NativeDragEffect::None => {
                            self.status_message = "Drag cancelled".into();
                        }
                    }
                }
                Err(error) => {
                    self.set_error(error.to_string());
                }
            }
            _ctx.request_repaint();
        }
    }

    fn external_drop_destination(&self, ctx: &egui::Context) -> Option<PathBuf> {
        if let Some(pointer) = ctx.input(|input| input.pointer.hover_pos())
            && let Some(path) = self
                .file_drag_folder_rects
                .iter()
                .rev()
                .find_map(|(path, rect)| {
                    if rect.contains(pointer) {
                        Some(path.clone())
                    } else {
                        None
                    }
                })
        {
            return Some(path);
        }
        self.active_path()
    }

    fn persist_session(&self) {
        let split = self.split.as_ref().map(|s| SplitSession {
            tab_a: s.tab_a,
            tab_b: s.tab_b,
            primary_tabs: s.primary_tabs.clone(),
            secondary_tabs: s.secondary_tabs.clone(),
            focused: s.focused,
            ratio: s.ratio,
            side: s.side,
        });
        let session = AppSession {
            tabs: self.tabs.clone(),
            active_tab: self.active_tab,
            split,
        };

        if let Err(error) = session.save() {
            crate::utils::log::error(format!("Session save failed: {error}"));
        }
    }

    fn persist_all(&self) {
        self.save_config();
        self.persist_session();
    }

    fn set_error(&mut self, message: String) {
        crate::utils::log::error(&message);
        self.error_message = Some(message);
    }

    fn retain_visible_thumbnails(&mut self) {
        let mut visible_paths: BTreeSet<PathBuf> = self
            .filtered_entries_slice()
            .iter()
            .map(|entry| entry.path.clone())
            .collect();
        if let Some(path) = self.preview_active_path.as_ref() {
            visible_paths.insert(path.clone());
        }
        if let Some(other) = self.other_pane.as_ref() {
            visible_paths.extend(
                other
                    .filtered_entries_slice()
                    .iter()
                    .map(|entry| entry.path.clone()),
            );
            if let Some(path) = other.preview_active_path.as_ref() {
                visible_paths.insert(path.clone());
            }
        }
        self.thumbnail_cache
            .retain(|path, _| visible_paths.contains(path));
        self.preview_cache
            .retain(|path, _| visible_paths.contains(path));
    }
}

impl eframe::App for BExplorerApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        if self.config.vibrancy != VibrancyMode::None {
            [0.0, 0.0, 0.0, 0.0]
        } else {
            egui::Color32::from_rgba_unmultiplied(12, 12, 12, 180).to_normalized_gamma_f32()
        }
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        crate::ui::theme::apply(ctx, &self.config);

        #[cfg(target_os = "windows")]
        self.apply_vibrancy(frame);

        self.poll_global_background(ctx);
        self.refresh_storage_if_needed(ctx);
        // Poll current pane's loads.
        self.poll_pane_background(ctx);
        // Poll other pane's loads (if any) by swapping in/out.
        if self.other_pane.is_some() {
            with_other_pane(self, |app| {
                app.poll_pane_background(ctx);
            });
        }
        self.prune_transfer_history();
        self.prune_archive_history();
        self.text_input_active = false;
        self.prepare_file_drag_frame();
        let mut shortcuts = Self::read_shortcuts(ctx, &self.config.shortcuts);
        let platform_paste_down = shortcut_binding_is(
            self.config.shortcuts.binding(ShortcutAction::Paste),
            "V",
            true,
            false,
            false,
        ) && crate::platform::file_paste_shortcut_down();
        self.apply_platform_paste_shortcut(&mut shortcuts, platform_paste_down);

        crate::ui::tabs::show(self, ctx);
        let split_inline_sidebar = self.split.is_some() && self.config.show_split_pane_menus;
        let sidebar_t = crate::ui::sidebar::visibility_t(ctx, self.sidebar_visible);
        if !split_inline_sidebar && (self.sidebar_visible || sidebar_t > 0.01) {
            crate::ui::sidebar::show(self, ctx, sidebar_t);
        }

        if self.split.is_some() {
            crate::ui::file_table::show_split(self, ctx);
        } else {
            crate::ui::status_bar::show(self, ctx);
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(crate::ui::theme::canvas(&self.config)))
                .show(ctx, |ui| {
                    let rect = ui.max_rect();
                    crate::ui::theme::paint_canvas_gradient(ui.painter(), rect, &self.config);
                    crate::ui::file_table::show(self, ui);
                });
        }

        self.try_start_native_file_drag_outside_window(ctx, frame);
        self.handle_external_file_drop(ctx);
        self.resolve_file_drag_target(ctx);
        self.finish_file_drag_if_released(ctx);
        self.handle_shortcuts(shortcuts);

        crate::ui::dialogs::show(self, ctx);
        crate::ui::command_palette::show(self, ctx);
        crate::ui::file_table::paint_file_drag_overlay(self, ctx);
        crate::ui::window_frame::show_resize_handles(ctx);

        if self.loading
            || self.operation_rx.is_some()
            || !self.active_archives.is_empty()
            || !self.archive_queue.is_empty()
            || !self.archive_history.is_empty()
            || self.defender_visible()
            || self.drag_selection.is_some()
            || self.sidebar_drag.is_some()
            || self.file_drag.is_some()
            || !self.transfer_history.is_empty()
        {
            ctx.request_repaint_after(Duration::from_millis(120));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.persist_all();
    }
}

#[cfg(target_os = "windows")]
impl BExplorerApp {
    fn apply_vibrancy(&mut self, frame: &mut eframe::Frame) {
        use raw_window_handle::HasWindowHandle;

        let Ok(handle) = frame.window_handle() else {
            return;
        };
        let _ = crate::platform::install_autoplay_cancel(&handle);

        if self.vibrancy_dirty {
            let _ = window_vibrancy::clear_mica(&handle);
            let _ = window_vibrancy::clear_tabbed(&handle);
            let _ = window_vibrancy::clear_acrylic(&handle);
            let _ = window_vibrancy::clear_blur(&handle);
            self.vibrancy_applied = false;
            self.vibrancy_dirty = false;
        }

        if self.vibrancy_applied {
            return;
        }

        let _ = crate::platform::apply_small_window_corners(&handle);

        let dark = matches!(self.config.theme, ThemePreference::Dark);

        self.config.vibrancy_active = match self.config.vibrancy {
            VibrancyMode::Mica => window_vibrancy::apply_tabbed(&handle, Some(dark)).is_ok(),
            VibrancyMode::Acrylic => {
                if dark {
                    window_vibrancy::apply_acrylic(&handle, Some((18, 18, 18, 77))).is_ok()
                } else {
                    window_vibrancy::apply_acrylic(&handle, Some((245, 245, 245, 77))).is_ok()
                }
            }
            VibrancyMode::Blur => false,
            VibrancyMode::None => false,
        };
        self.vibrancy_applied = true;
    }
}

/// Temporarily swap the other pane into app, run a closure, then swap back.
/// Use when you need to access/modify the other pane's state.
pub(crate) fn with_other_pane<F>(app: &mut BExplorerApp, f: F)
where
    F: FnOnce(&mut BExplorerApp),
{
    app.swap_panes();
    f(app);
    app.swap_panes();
}

pub(crate) fn shortcut_binding_label(binding: &ShortcutBinding) -> String {
    if binding.key.trim().is_empty() {
        return "Sin asignar".into();
    }

    let mut parts = Vec::new();
    if binding.ctrl {
        parts.push("Ctrl".to_string());
    }
    if binding.alt {
        parts.push("Alt".to_string());
    }
    if binding.shift {
        parts.push("Shift".to_string());
    }
    parts.push(shortcut_key_display(&binding.key).to_string());
    parts.join("+")
}

pub(crate) fn shortcut_binding_from_input(ctx: &egui::Context) -> Option<ShortcutBinding> {
    ctx.input(|input| {
        input.events.iter().find_map(|event| {
            let egui::Event::Key {
                key,
                pressed: true,
                repeat: false,
                modifiers,
                ..
            } = event
            else {
                return None;
            };
            if *key == egui::Key::Escape {
                return None;
            }
            Some(ShortcutBinding {
                key: shortcut_key_name(*key)?.into(),
                ctrl: modifiers.ctrl || modifiers.command,
                alt: modifiers.alt,
                shift: modifiers.shift,
            })
        })
    })
}

fn shortcut_pressed(input: &egui::InputState, binding: &ShortcutBinding) -> bool {
    let Some(key) = shortcut_key_from_name(&binding.key) else {
        return false;
    };
    input.events.iter().any(|event| {
        matches!(
            event,
            egui::Event::Key {
                key: event_key,
                pressed: true,
                modifiers,
                ..
            } if *event_key == key && shortcut_modifiers_match(*modifiers, binding)
        )
    })
}

fn shortcut_pressed_allow_extend(input: &egui::InputState, binding: &ShortcutBinding) -> bool {
    let Some(key) = shortcut_key_from_name(&binding.key) else {
        return false;
    };
    input.events.iter().any(|event| {
        matches!(
            event,
            egui::Event::Key {
                key: event_key,
                pressed: true,
                modifiers,
                ..
            } if *event_key == key && shortcut_modifiers_match_allow_extra_shift(*modifiers, binding)
        )
    })
}

fn shortcut_modifiers_match(modifiers: egui::Modifiers, binding: &ShortcutBinding) -> bool {
    let ctrl = modifiers.ctrl || modifiers.command;
    ctrl == binding.ctrl && modifiers.alt == binding.alt && modifiers.shift == binding.shift
}

fn shortcut_modifiers_match_allow_extra_shift(
    modifiers: egui::Modifiers,
    binding: &ShortcutBinding,
) -> bool {
    let ctrl = modifiers.ctrl || modifiers.command;
    ctrl == binding.ctrl
        && modifiers.alt == binding.alt
        && (modifiers.shift == binding.shift || (!binding.shift && modifiers.shift))
}

fn shortcut_binding_is(
    binding: &ShortcutBinding,
    key: &str,
    ctrl: bool,
    alt: bool,
    shift: bool,
) -> bool {
    binding.key.eq_ignore_ascii_case(key)
        && binding.ctrl == ctrl
        && binding.alt == alt
        && binding.shift == shift
}

fn shortcut_key_name(key: egui::Key) -> Option<&'static str> {
    Some(match key {
        egui::Key::ArrowDown => "ArrowDown",
        egui::Key::ArrowLeft => "ArrowLeft",
        egui::Key::ArrowRight => "ArrowRight",
        egui::Key::ArrowUp => "ArrowUp",
        egui::Key::Escape => "Escape",
        egui::Key::Tab => "Tab",
        egui::Key::Backspace => "Backspace",
        egui::Key::Enter => "Enter",
        egui::Key::Space => "Space",
        egui::Key::Insert => "Insert",
        egui::Key::Delete => "Delete",
        egui::Key::Home => "Home",
        egui::Key::End => "End",
        egui::Key::PageUp => "PageUp",
        egui::Key::PageDown => "PageDown",
        egui::Key::A => "A",
        egui::Key::B => "B",
        egui::Key::C => "C",
        egui::Key::D => "D",
        egui::Key::E => "E",
        egui::Key::F => "F",
        egui::Key::G => "G",
        egui::Key::H => "H",
        egui::Key::I => "I",
        egui::Key::J => "J",
        egui::Key::K => "K",
        egui::Key::L => "L",
        egui::Key::M => "M",
        egui::Key::N => "N",
        egui::Key::O => "O",
        egui::Key::P => "P",
        egui::Key::Q => "Q",
        egui::Key::R => "R",
        egui::Key::S => "S",
        egui::Key::T => "T",
        egui::Key::U => "U",
        egui::Key::V => "V",
        egui::Key::W => "W",
        egui::Key::X => "X",
        egui::Key::Y => "Y",
        egui::Key::Z => "Z",
        egui::Key::Num0 => "0",
        egui::Key::Num1 => "1",
        egui::Key::Num2 => "2",
        egui::Key::Num3 => "3",
        egui::Key::Num4 => "4",
        egui::Key::Num5 => "5",
        egui::Key::Num6 => "6",
        egui::Key::Num7 => "7",
        egui::Key::Num8 => "8",
        egui::Key::Num9 => "9",
        egui::Key::F1 => "F1",
        egui::Key::F2 => "F2",
        egui::Key::F3 => "F3",
        egui::Key::F4 => "F4",
        egui::Key::F5 => "F5",
        egui::Key::F6 => "F6",
        egui::Key::F7 => "F7",
        egui::Key::F8 => "F8",
        egui::Key::F9 => "F9",
        egui::Key::F10 => "F10",
        egui::Key::F11 => "F11",
        egui::Key::F12 => "F12",
        _ => return None,
    })
}

fn shortcut_key_from_name(name: &str) -> Option<egui::Key> {
    Some(match name {
        "ArrowDown" => egui::Key::ArrowDown,
        "ArrowLeft" => egui::Key::ArrowLeft,
        "ArrowRight" => egui::Key::ArrowRight,
        "ArrowUp" => egui::Key::ArrowUp,
        "Escape" => egui::Key::Escape,
        "Tab" => egui::Key::Tab,
        "Backspace" => egui::Key::Backspace,
        "Enter" => egui::Key::Enter,
        "Space" => egui::Key::Space,
        "Insert" => egui::Key::Insert,
        "Delete" => egui::Key::Delete,
        "Home" => egui::Key::Home,
        "End" => egui::Key::End,
        "PageUp" => egui::Key::PageUp,
        "PageDown" => egui::Key::PageDown,
        "A" => egui::Key::A,
        "B" => egui::Key::B,
        "C" => egui::Key::C,
        "D" => egui::Key::D,
        "E" => egui::Key::E,
        "F" => egui::Key::F,
        "G" => egui::Key::G,
        "H" => egui::Key::H,
        "I" => egui::Key::I,
        "J" => egui::Key::J,
        "K" => egui::Key::K,
        "L" => egui::Key::L,
        "M" => egui::Key::M,
        "N" => egui::Key::N,
        "O" => egui::Key::O,
        "P" => egui::Key::P,
        "Q" => egui::Key::Q,
        "R" => egui::Key::R,
        "S" => egui::Key::S,
        "T" => egui::Key::T,
        "U" => egui::Key::U,
        "V" => egui::Key::V,
        "W" => egui::Key::W,
        "X" => egui::Key::X,
        "Y" => egui::Key::Y,
        "Z" => egui::Key::Z,
        "0" => egui::Key::Num0,
        "1" => egui::Key::Num1,
        "2" => egui::Key::Num2,
        "3" => egui::Key::Num3,
        "4" => egui::Key::Num4,
        "5" => egui::Key::Num5,
        "6" => egui::Key::Num6,
        "7" => egui::Key::Num7,
        "8" => egui::Key::Num8,
        "9" => egui::Key::Num9,
        "F1" => egui::Key::F1,
        "F2" => egui::Key::F2,
        "F3" => egui::Key::F3,
        "F4" => egui::Key::F4,
        "F5" => egui::Key::F5,
        "F6" => egui::Key::F6,
        "F7" => egui::Key::F7,
        "F8" => egui::Key::F8,
        "F9" => egui::Key::F9,
        "F10" => egui::Key::F10,
        "F11" => egui::Key::F11,
        "F12" => egui::Key::F12,
        _ => return None,
    })
}

fn shortcut_key_display(name: &str) -> &str {
    match name {
        "ArrowDown" => "Abajo",
        "ArrowLeft" => "Izquierda",
        "ArrowRight" => "Derecha",
        "ArrowUp" => "Arriba",
        "Backspace" => "Retroceso",
        "Delete" => "Supr",
        "PageUp" => "Re Pag",
        "PageDown" => "Av Pag",
        "Space" => "Espacio",
        other => other,
    }
}

#[derive(Default)]
struct Shortcuts {
    copy_event: bool,
    cut_event: bool,
    paste_event: bool,
    command_palette: bool,
    copy: bool,
    cut: bool,
    paste: bool,
    select_all: bool,
    refresh: bool,
    enter: bool,
    properties: bool,
    rename: bool,
    delete: bool,
    permanent_delete: bool,
    up: bool,
    arrow_up: bool,
    arrow_down: bool,
    extend_selection: bool,
    alt_left: bool,
    alt_right: bool,
    type_select: Option<char>,
}

#[cfg(test)]
mod shortcut_tests {
    use super::*;
    use crate::app::config::ViewMode;
    use crate::app::session::TabState;

    fn shortcuts_for(events: Vec<egui::Event>, modifiers: egui::Modifiers) -> Shortcuts {
        shortcuts_for_config(events, modifiers, &ShortcutConfig::default())
    }

    fn shortcuts_for_config(
        events: Vec<egui::Event>,
        modifiers: egui::Modifiers,
        config: &ShortcutConfig,
    ) -> Shortcuts {
        let ctx = egui::Context::default();
        ctx.begin_pass(egui::RawInput {
            events,
            modifiers,
            ..Default::default()
        });
        BExplorerApp::read_shortcuts(&ctx, config)
    }

    #[test]
    fn detects_file_copy_paste_shortcuts_from_key_events() {
        let ctrl = egui::Modifiers {
            ctrl: true,
            ..Default::default()
        };
        let copy = shortcuts_for(
            vec![egui::Event::Key {
                key: egui::Key::C,
                physical_key: Some(egui::Key::C),
                pressed: true,
                repeat: false,
                modifiers: ctrl,
            }],
            ctrl,
        );
        assert!(copy.copy);
        assert!(!copy.paste);

        let paste = shortcuts_for(
            vec![egui::Event::Key {
                key: egui::Key::V,
                physical_key: Some(egui::Key::V),
                pressed: true,
                repeat: false,
                modifiers: ctrl,
            }],
            ctrl,
        );
        assert!(paste.paste);
        assert!(!paste.copy);
    }

    #[test]
    fn custom_shortcut_replaces_default_key_binding() {
        let ctrl = egui::Modifiers {
            ctrl: true,
            ..Default::default()
        };
        let mut config = ShortcutConfig::default();
        config.set_binding(
            ShortcutAction::Paste,
            ShortcutBinding::new("F6", false, false, false),
        );

        let old_paste = shortcuts_for_config(
            vec![egui::Event::Key {
                key: egui::Key::V,
                physical_key: Some(egui::Key::V),
                pressed: true,
                repeat: false,
                modifiers: ctrl,
            }],
            ctrl,
            &config,
        );
        assert!(!old_paste.paste);

        let new_paste = shortcuts_for_config(
            vec![egui::Event::Key {
                key: egui::Key::F6,
                physical_key: Some(egui::Key::F6),
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
            Default::default(),
            &config,
        );
        assert!(new_paste.paste);
    }

    #[test]
    fn shift_arrow_still_extends_selection() {
        let shift = egui::Modifiers {
            shift: true,
            ..Default::default()
        };
        let shortcuts = shortcuts_for(
            vec![egui::Event::Key {
                key: egui::Key::ArrowDown,
                physical_key: Some(egui::Key::ArrowDown),
                pressed: true,
                repeat: false,
                modifiers: shift,
            }],
            shift,
        );

        assert!(shortcuts.arrow_down);
        assert!(shortcuts.extend_selection);
    }

    #[test]
    fn detects_file_paste_from_native_paste_event() {
        let shortcuts = shortcuts_for(vec![egui::Event::Paste(String::new())], Default::default());
        assert!(shortcuts.paste_event);
    }

    #[test]
    fn platform_paste_shortcut_fires_once_per_key_press() {
        let root = std::env::temp_dir().join(format!(
            "bexplorer-platform-paste-edge-{}",
            std::process::id()
        ));
        let mut app = test_app_at(root);

        let mut first = empty_shortcuts();
        app.apply_platform_paste_shortcut(&mut first, true);
        assert!(first.paste);

        let mut repeat = empty_shortcuts();
        app.apply_platform_paste_shortcut(&mut repeat, true);
        assert!(!repeat.paste);

        let mut released = empty_shortcuts();
        app.apply_platform_paste_shortcut(&mut released, false);
        assert!(!released.paste);

        let mut second = empty_shortcuts();
        app.apply_platform_paste_shortcut(&mut second, true);
        assert!(second.paste);
    }

    fn empty_shortcuts() -> Shortcuts {
        Shortcuts::default()
    }

    fn test_app_at(path: PathBuf) -> BExplorerApp {
        let (transfer_tx, transfer_rx) = mpsc::channel();
        let (thumbnail_tx, thumbnail_rx) = mpsc::channel();
        let (portable_thumbnail_tx, _portable_thumbnail_rx) = mpsc::channel();
        let (preview_tx, _preview_job_rx) = mpsc::channel();
        let (_preview_result_tx, preview_rx) = mpsc::channel();

        BExplorerApp {
            config: AppConfig {
                default_view: ViewMode::Details,
                ..AppConfig::default()
            },
            tabs: vec![TabState::with_view_mode(Some(path), ViewMode::Details)],
            tab_search: vec![TabSearchState::default()],
            active_tab: 0,
            entries: Vec::new(),
            storage_entries: Vec::new(),
            portable_devices: Vec::new(),
            selected: BTreeSet::new(),
            selection_anchor: None,
            selection_focus: None,
            filter: String::new(),
            search_mode: SearchMode::Quick,
            search_results: Vec::new(),
            searching: false,
            loading: false,
            status_message: String::new(),
            error_message: None,
            rename_dialog: None,
            compress_dialog: None,
            archive_password_dialog: None,
            pending_rename: None,
            confirm_permanent_delete: None,
            pending_transfer_conflict: None,
            pending_elevated_transfer: None,
            pending_elevated_operation: None,
            delete_panel_spawned: false,
            command_palette_open: false,
            command_query: String::new(),
            options_menu_open: false,
            action_bar_new_menu_open: false,
            options_open: false,
            shortcuts_open: false,
            transfer_panel_minimized: false,
            transfer_panel_spawned: false,
            sidebar_visible: true,
            sidebar_open: SidebarOpen::default(),
            sidebar_drag: None,
            sidebar_drop_target: None,
            drag_selection: None,
            tab_drag: None,
            file_drag: None,
            file_drag_folder_rects: Vec::new(),
            split: None,
            other_pane: None,
            focused_pane: 0,
            sort: FileSort::Name,
            sort_ascending: true,
            group_by: FileGroup::None,
            group_ascending: true,
            column_widths: default_column_widths(),
            next_request_id: 0,
            next_search_request_id: 0,
            next_transfer_id: 0,
            load_rx: None,
            search_rx: None,
            search_cancel: None,
            operation_rx: None,
            storage_refresh_rx: None,
            transfer_tx,
            transfer_rx,
            transfer_queue: VecDeque::new(),
            active_transfers: HashMap::new(),
            transfer_progress: HashMap::new(),
            transfer_history: VecDeque::new(),
            max_parallel_transfers: 2,
            thumbnail_tx,
            thumbnail_rx,
            portable_thumbnail_tx,
            preview_tx,
            preview_rx,
            preview_generation: Arc::new(AtomicU64::new(0)),
            preview_active_path: None,
            archive_queue: VecDeque::new(),
            active_archives: HashMap::new(),
            archive_progress: HashMap::new(),
            archive_history: VecDeque::new(),
            next_archive_id: 0,
            max_parallel_archives: 3,
            archive_panel_minimized: false,
            archive_panel_spawned: false,
            defender_rx: None,
            defender_cancel: None,
            defender_progress: None,
            defender_summary: None,
            defender_panel_minimized: false,
            defender_panel_spawned: false,
            clipboard: None,
            thumbnail_cache: HashMap::new(),
            native_icon_cache: HashMap::new(),
            preview_cache: HashMap::new(),
            preview_text_selection: None,
            pending_select: None,
            type_select: None,
            pending_scroll_path: None,
            path_bar_text_visible: false,
            path_bar_edit_text: String::new(),
            path_bar_focus_pending: false,
            path_bar_selection_range: None,
            preview_panel_visible: false,
            text_input_active: false,
            paste_shortcut_down: false,
            shortcut_capture: None,
            last_auto_fit_path: None,
            last_auto_fit_width: None,
            vibrancy_applied: false,
            vibrancy_dirty: false,
            last_storage_refresh: Instant::now(),
        }
    }

    #[test]
    fn ctrl_c_then_ctrl_v_same_folder_opens_conflict_prompt() {
        let root =
            std::env::temp_dir().join(format!("bexplorer-shortcut-paste-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create temp folder");
        let file = root.join("sample.txt");
        std::fs::write(&file, b"copy me").expect("write temp file");

        let mut app = test_app_at(root.clone());
        app.selected.insert(file.clone());

        let mut copy = empty_shortcuts();
        copy.copy = true;
        app.handle_shortcuts(copy);
        assert!(
            app.clipboard
                .as_ref()
                .is_some_and(|clipboard| clipboard.paths == vec![file.clone()] && !clipboard.cut),
            "Ctrl+C should populate the internal file clipboard"
        );

        let mut paste = empty_shortcuts();
        paste.paste = true;
        app.handle_shortcuts(paste);
        assert!(
            app.pending_transfer_conflict.is_some(),
            "Ctrl+V in the same folder should ask how to handle the existing file"
        );

        std::fs::remove_dir_all(root).expect("cleanup temp folder");
    }
}
