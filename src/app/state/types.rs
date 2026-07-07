use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::mpsc::Receiver;
use std::time::Instant;

use eframe::egui;
use eframe::egui::{ColorImage, TextureHandle};

use crate::app::session::{SplitFocus, SplitSide};
use crate::fs::archive::{
    ArchiveCompressionMethod, ArchiveFormat, ArchiveJob, ArchiveJobKind, ArchiveProgress,
    ArchiveProgressMsg, ArchiveState,
};
use crate::fs::explorer::FileEntry;
use crate::fs::search;
use crate::fs::transfer_queue::{
    TransferControl, TransferJob, TransferKind, TransferProgress, TransferState,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileSort {
    Name,
    Type,
    Size,
    Modified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileGroup {
    None,
    Name,
    Type,
    TotalSize,
    FreeSpace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchMode {
    Quick,
    Complete,
}

#[derive(Clone, Debug)]
pub(super) struct FileClipboard {
    pub(super) paths: Vec<PathBuf>,
    pub(super) cut: bool,
}

#[derive(Clone)]
pub(super) struct ActiveTransfer {
    pub(super) job: TransferJob,
    pub(super) control: TransferControl,
}

#[derive(Clone)]
pub(super) struct TransferHistoryItem {
    pub(super) progress: TransferProgress,
    pub(super) finished_at: Instant,
}

#[derive(Clone, Debug)]
pub struct TransferDisplayItem {
    pub id: u64,
    pub kind: TransferKind,
    pub state: TransferState,
    pub current_name: String,
    pub copied_bytes: u64,
    pub total_bytes: u64,
    pub files_done: usize,
    pub total_files: usize,
    pub bytes_per_second: f64,
    pub queued_index: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct PendingTransferConflict {
    pub sources: Vec<PathBuf>,
    pub destination: PathBuf,
    pub kind: TransferKind,
    pub conflict_count: usize,
    pub first_conflict_name: String,
    pub clear_clipboard_on_confirm: bool,
}

impl TransferDisplayItem {
    pub(super) fn from_progress(progress: TransferProgress, queued_index: Option<usize>) -> Self {
        let kind = progress.kind;
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
            kind,
            state: progress.state,
            current_name,
            copied_bytes: progress.copied_bytes,
            total_bytes: progress.total_bytes,
            files_done: progress.files_done,
            total_files: progress.total_files,
            bytes_per_second: progress.bytes_per_second,
            queued_index,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RenameDialog {
    pub path: PathBuf,
    pub value: String,
    pub select_range: Option<(usize, usize)>,
}

#[derive(Clone, Debug)]
pub struct CompressDialog {
    pub sources: Vec<PathBuf>,
    pub destination_dir: PathBuf,
    pub name: String,
    pub format: ArchiveFormat,
    pub method: ArchiveCompressionMethod,
    pub password: String,
    pub confirm_password: String,
    pub password_mismatch: bool,
}

#[derive(Clone, Debug)]
pub struct ArchivePasswordDialog {
    pub job: ArchiveJob,
    pub password: String,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct DragSelection {
    pub start: egui::Pos2,
    pub current: egui::Pos2,
    pub base_selected: BTreeSet<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct TabDrag {
    pub from_index: usize,
    pub target_index: usize,
    pub press_origin_x: f32,
    pub tab_width: f32,
    pub offsets: Vec<f32>,
}

#[derive(Clone, Debug)]
pub struct SplitState {
    pub tab_a: usize,
    pub tab_b: usize,
    pub primary_tabs: Vec<usize>,
    pub secondary_tabs: Vec<usize>,
    pub focused: SplitFocus,
    pub ratio: f32,
    pub side: SplitSide,
}

#[derive(Clone, Debug)]
pub struct FileDrag {
    pub paths: Vec<PathBuf>,
    pub target: Option<PathBuf>,
    pub primary: Option<DragPrimary>,
}

#[derive(Clone, Debug)]
pub struct DragPrimary {
    pub path: PathBuf,
    pub is_directory: bool,
}

#[derive(Clone, Debug)]
pub struct FileDragFeedback {
    pub item_name: String,
    pub item_count: usize,
    pub target_name: Option<String>,
    pub copy: bool,
}

#[derive(Clone, Debug)]
pub(super) struct TypeSelectState {
    pub(super) character: char,
    pub(super) updated_at: Instant,
}

pub struct PaneState {
    pub active_tab: usize,
    pub entries: Vec<FileEntry>,
    pub selected: BTreeSet<PathBuf>,
    pub selection_anchor: Option<PathBuf>,
    pub selection_focus: Option<PathBuf>,
    pub filter: String,
    pub search_mode: SearchMode,
    pub search_results: Vec<FileEntry>,
    pub searching: bool,
    pub loading: bool,
    pub status_message: String,
    pub rename_dialog: Option<RenameDialog>,
    pub pending_rename: Option<PathBuf>,
    pub action_bar_new_menu_open: bool,
    pub drag_selection: Option<DragSelection>,
    pub pending_select: Option<PathBuf>,
    pub path_bar_text_visible: bool,
    pub path_bar_edit_text: String,
    pub path_bar_focus_pending: bool,
    pub path_bar_selection_range: Option<(usize, usize)>,
    pub preview_active_path: Option<PathBuf>,
    pub preview_text_selection: Option<(PathBuf, String, egui::text::CCursorRange)>,
    pub preview_panel_visible: bool,
    pub sort: FileSort,
    pub sort_ascending: bool,
    pub group_by: FileGroup,
    pub group_ascending: bool,
    pub column_widths: [f32; 8],
    pub last_auto_fit_path: Option<PathBuf>,
    pub last_auto_fit_width: Option<f32>,
    pub(super) next_request_id: u64,
    pub(super) next_search_request_id: u64,
    pub(super) load_rx: Option<Receiver<LoadMessage>>,
    pub(super) search_rx: Option<Receiver<SearchMessage>>,
    pub(super) search_cancel: Option<Arc<AtomicBool>>,
    pub(super) type_select: Option<TypeSelectState>,
    pub pending_scroll_path: Option<PathBuf>,
}

pub(super) struct TabSearchState {
    pub(super) filter: String,
    pub(super) search_mode: SearchMode,
    pub(super) search_results: Vec<FileEntry>,
    pub(super) searching: bool,
    pub(super) next_search_request_id: u64,
    pub(super) search_rx: Option<Receiver<SearchMessage>>,
    pub(super) search_cancel: Option<Arc<AtomicBool>>,
}

impl Default for TabSearchState {
    fn default() -> Self {
        Self {
            filter: String::new(),
            search_mode: SearchMode::Quick,
            search_results: Vec::new(),
            searching: false,
            next_search_request_id: 0,
            search_rx: None,
            search_cancel: None,
        }
    }
}

pub(super) struct LoadMessage {
    pub(super) request_id: u64,
    pub(super) finished: bool,
    pub(super) append: bool,
    pub(super) result: std::result::Result<Vec<FileEntry>, String>,
}

pub(super) struct OperationMessage {
    pub(super) refresh: bool,
    pub(super) result: std::result::Result<String, String>,
    pub(super) elevated_operation: Option<crate::fs::operations::ElevatedFileOperation>,
}

pub(super) struct SearchMessage {
    pub(super) request_id: u64,
    pub(super) query: String,
    pub(super) root: PathBuf,
    pub(super) event: search::SearchEvent,
}

pub(super) struct ThumbnailMessage {
    pub(super) path: PathBuf,
    pub(super) image: Option<ColorImage>,
}

pub(super) struct ThumbnailJob {
    pub(super) path: PathBuf,
}

pub(super) struct PortableThumbnailJob {
    pub(super) path: PathBuf,
    pub(super) max_bytes: usize,
    pub(super) allow_default_resource: bool,
}

pub(super) struct NativeIconJob {
    pub(super) cache_key: PathBuf,
    pub(super) path: PathBuf,
    pub(super) is_directory: bool,
    pub(super) size: u32,
}

pub(super) struct NativeIconMessage {
    pub(super) cache_key: PathBuf,
    pub(super) image: Option<ColorImage>,
}

pub(super) struct PreviewJob {
    pub(super) entry: FileEntry,
    pub(super) max_bytes: usize,
    pub(super) generation: u64,
}

pub(super) struct PreviewMessage {
    pub(super) path: PathBuf,
    pub(super) generation: u64,
    pub(super) content: crate::preview::PreviewContent,
}

#[derive(Clone, Debug)]
pub struct ArchiveProgressState {
    pub name: String,
    pub completed: u64,
    pub total: u64,
    pub command: String,
    pub file_name: String,
    pub started: Instant,
    pub last_completed: u64,
    pub bytes_per_second: f64,
}

impl ArchiveProgressState {
    pub fn new(progress: &ArchiveProgress, _kind: ArchiveJobKind, name: &str) -> Self {
        let now = Instant::now();
        Self {
            name: name.to_string(),
            completed: progress.completed,
            total: progress.total,
            command: progress.command.clone(),
            file_name: progress.file_name.clone(),
            started: now,
            last_completed: progress.completed,
            bytes_per_second: 0.0,
        }
    }

    pub fn new_for_job(_kind: ArchiveJobKind, name: &str) -> Self {
        let now = Instant::now();
        Self {
            name: name.to_string(),
            completed: 0,
            total: 0,
            command: String::new(),
            file_name: String::new(),
            started: now,
            last_completed: 0,
            bytes_per_second: 0.0,
        }
    }

    pub fn update(&mut self, progress: &ArchiveProgress) {
        let now = Instant::now();
        let elapsed = now
            .saturating_duration_since(self.started)
            .as_secs_f64()
            .max(0.001);
        let next_total = if progress.total == 0 || progress.total == u64::MAX {
            self.total
        } else {
            progress.total
        };
        self.total = next_total;
        self.completed = if self.total > 0 {
            progress.completed.min(self.total)
        } else {
            progress.completed
        };
        self.bytes_per_second = self.completed as f64 / elapsed;
        self.command = progress.command.clone();
        self.file_name = progress.file_name.clone();
        self.last_completed = self.completed;
    }
}

#[derive(Clone, Debug)]
pub struct ArchiveDisplayItem {
    pub id: u64,
    pub kind: ArchiveJobKind,
    pub state: ArchiveState,
    pub current_name: String,
    pub file_name: String,
    pub completed: u64,
    pub total: u64,
    pub bytes_per_second: f64,
    pub queued_index: Option<usize>,
}

pub(super) struct ActiveArchive {
    pub(super) job: ArchiveJob,
    pub(super) rx: std::sync::mpsc::Receiver<ArchiveProgressMsg>,
    pub(super) cancel_flag: Arc<AtomicU32>,
}

pub(super) struct ArchiveHistoryItem {
    pub(super) id: u64,
    pub(super) kind: ArchiveJobKind,
    pub(super) state: ArchiveState,
    pub(super) name: String,
    pub(super) completed: u64,
    pub(super) total: u64,
    pub(super) finished_at: Instant,
}

#[derive(Clone, Debug)]
pub struct SidebarOpen {
    pub recents: bool,
    pub favorites: bool,
    pub storage: bool,
    pub network: bool,
    pub places: bool,
}

impl Default for SidebarOpen {
    fn default() -> Self {
        Self {
            recents: true,
            favorites: true,
            storage: true,
            network: true,
            places: true,
        }
    }
}

pub(super) enum ThumbnailState {
    Ready(TextureHandle),
    Loading,
    Missing,
}

pub(super) enum NativeIconState {
    Ready(TextureHandle),
    Loading,
    Missing,
}

pub enum PreviewCacheState {
    Images {
        textures: Vec<TextureHandle>,
        generation: u64,
        loading: bool,
        page_count: Option<usize>,
    },
    Text(String),
    Loading(u64),
    Missing,
}
