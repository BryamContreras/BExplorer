use super::*;

impl BExplorerIced {
    pub(in crate::iced_ui) fn start_duplicate_cleanup(
        &mut self,
        pane: PaneId,
        target: ContextTarget,
    ) -> Task<Message> {
        let Some(entry) = self.context_entry(pane, target) else {
            return Task::none();
        };
        if !duplicate_cleanup_available_for_entry(&entry) {
            return self.report_error(
                pane,
                self.localized(
                    "La limpieza de archivos duplicados solo está disponible para carpetas y unidades montadas.",
                    "Duplicate file cleanup is only available for folders and mounted drives.",
                ),
            );
        }

        let (window_size, window_maximized, column_widths) = self
            .duplicate_cleanup
            .as_ref()
            .map(|state| {
                (
                    state.window_size,
                    state.window_maximized,
                    state.column_widths,
                )
            })
            .unwrap_or_else(|| {
                (
                    duplicate_cleanup_window_size(self.font_size()),
                    false,
                    DUPLICATE_TABLE_COLUMN_WIDTHS,
                )
            });
        if let Some(state) = self.duplicate_cleanup.take() {
            state.cancel.store(true, AtomicOrdering::Relaxed);
        }
        let root = entry.path;
        let (receiver, cancel) = duplicate_scan_worker(root.clone());
        self.duplicate_cleanup = Some(DuplicateCleanupState {
            pane,
            root,
            entries: Vec::new(),
            extension_counts: HashMap::new(),
            extension_group_starts: Vec::new(),
            table_scroll_offset_y: 0.0,
            table_viewport_height: 0.0,
            table_scroll_velocity_y: 0.0,
            table_scroll_sampled_at: None,
            selected: HashSet::new(),
            all_candidates_selected: false,
            highlighted: None,
            pointer: Point::ORIGIN,
            context_path: None,
            context_position: Point::ORIGIN,
            window_size,
            window_maximized,
            column_widths,
            column_resize: None,
            scanned: 0,
            total: 0,
            files_found: 0,
            skipped: 0,
            current_path: None,
            phase: DuplicateCleanupPhase::Counting,
            error: None,
            confirm_delete: false,
            deleting: false,
            receiver,
            cancel,
        });

        if let Some(id) = self.duplicate_window_id {
            Task::batch([window::minimize(id, false), window::gain_focus(id)])
        } else {
            let (id, open) = window::open(duplicate_cleanup_window_settings(self.font_size()));
            self.duplicate_window_id = Some(id);
            open.map(Message::DuplicateCleanupWindowOpened)
        }
    }

    pub(in crate::iced_ui) fn poll_duplicate_cleanup_messages(&mut self) {
        let Some(state) = self.duplicate_cleanup.as_mut() else {
            return;
        };
        loop {
            match state.receiver.try_recv() {
                Ok(DuplicateScanEvent::Counting { files_found }) => {
                    state.files_found = files_found;
                }
                Ok(DuplicateScanEvent::Progress {
                    scanned,
                    total,
                    current_path,
                    duplicates,
                    skipped,
                }) => {
                    state.phase = DuplicateCleanupPhase::Scanning;
                    state.scanned = scanned;
                    state.total = total;
                    state.current_path = current_path;
                    if let Some(duplicates) = duplicates {
                        replace_duplicate_entries(state, duplicates);
                    }
                    state.skipped = skipped;
                }
                Ok(DuplicateScanEvent::Finished {
                    scanned,
                    total,
                    duplicates,
                    skipped,
                }) => {
                    state.phase = DuplicateCleanupPhase::Complete;
                    state.scanned = scanned;
                    state.total = total;
                    state.current_path = None;
                    replace_duplicate_entries(state, duplicates);
                    state.skipped = skipped;
                }
                Ok(DuplicateScanEvent::Cancelled) => {
                    state.phase = DuplicateCleanupPhase::Cancelled;
                    state.current_path = None;
                }
                Ok(DuplicateScanEvent::Failed(error)) => {
                    state.phase = DuplicateCleanupPhase::Failed;
                    state.error = Some(error);
                    state.current_path = None;
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
    }

    pub(in crate::iced_ui) fn close_duplicate_cleanup_task(&mut self) -> Task<Message> {
        if let Some(state) = self.duplicate_cleanup.as_ref() {
            state.cancel.store(true, AtomicOrdering::Relaxed);
        }
        if let Some(id) = self.duplicate_window_id {
            self.close_window_task(id)
        } else {
            self.duplicate_cleanup = None;
            Task::none()
        }
    }

    pub(in crate::iced_ui) fn sync_duplicate_window_size_task(&self) -> Task<Message> {
        self.duplicate_window_id
            .map(|id| sync_duplicate_cleanup_window_constraints_task(id, self.font_size()))
            .unwrap_or_else(Task::none)
    }

    pub(in crate::iced_ui) fn confirm_duplicate_delete_task(&mut self) -> Task<Message> {
        let Some(state) = self.duplicate_cleanup.as_mut() else {
            return Task::none();
        };
        if state.deleting || state.selected.is_empty() {
            return Task::none();
        }
        state.confirm_delete = false;
        state.deleting = true;
        let paths = state.selected.iter().cloned().collect::<Vec<_>>();
        let worker_paths = paths.clone();
        Task::perform(
            run_blocking_file_operation(move || {
                operations::delete_to_trash_with_undo(&worker_paths)
            }),
            move |result| Message::DuplicateDeleteFinished(paths, result),
        )
    }

    pub(in crate::iced_ui) fn open_duplicate_file_location_task(&mut self) -> Task<Message> {
        let Some((stored_pane, path)) = self.duplicate_cleanup.as_mut().and_then(|state| {
            state
                .context_path
                .take()
                .or_else(|| state.highlighted.clone())
                .map(|path| (state.pane, path))
        }) else {
            return Task::none();
        };
        let pane = if stored_pane == PaneId::Secondary && self.split.is_none() {
            PaneId::Primary
        } else {
            stored_pane
        };
        let Some(location) = path.parent().map(Path::to_path_buf) else {
            let error = self
                .localized(
                    "La ubicación del archivo no está disponible.",
                    "The file location is not available.",
                )
                .to_owned();
            if let Some(state) = self.duplicate_cleanup.as_mut() {
                state.error = Some(error);
            }
            return Task::none();
        };
        self.pending_reveal_in_new_tab = Some((pane, location.clone(), path));
        let ensure_main = self.ensure_main_window_for_attention_task();
        let navigation = self.open_path_in_new_tab(pane, Some(location));
        let focus = self
            .main_window_id
            .map(|id| Task::batch([window::minimize(id, false), window::gain_focus(id)]))
            .unwrap_or_else(Task::none);
        Task::batch([ensure_main, navigation, focus])
    }

    pub(in crate::iced_ui) fn duplicate_delete_finished(
        &mut self,
        paths: Vec<PathBuf>,
        result: Result<operations::TrashDeleteOutcome, String>,
    ) -> Task<Message> {
        let Some(state) = self.duplicate_cleanup.as_mut() else {
            return Task::none();
        };
        state.deleting = false;
        match result {
            Ok(outcome) => {
                let pane = state.pane;
                if !outcome.undo_records.is_empty() {
                    self.last_undo_action = Some(UndoAction::Trash {
                        pane,
                        records: outcome.undo_records,
                    });
                }
                let mut directories = paths
                    .iter()
                    .filter_map(|path| path.parent().map(Path::to_path_buf))
                    .collect::<HashSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>();
                directories.push(explorer::trash_root_path());
                self.restart_duplicate_cleanup_scan();
                self.refresh_panes_for_directories(pane, &directories)
            }
            Err(error) => {
                state.error = Some(error);
                Task::none()
            }
        }
    }

    fn restart_duplicate_cleanup_scan(&mut self) {
        let Some(state) = self.duplicate_cleanup.as_mut() else {
            return;
        };
        state.cancel.store(true, AtomicOrdering::Relaxed);
        let (receiver, cancel) = duplicate_scan_worker(state.root.clone());
        state.entries.clear();
        state.extension_counts.clear();
        state.extension_group_starts.clear();
        state.table_scroll_offset_y = 0.0;
        state.table_viewport_height = 0.0;
        state.table_scroll_velocity_y = 0.0;
        state.table_scroll_sampled_at = None;
        state.selected.clear();
        state.all_candidates_selected = false;
        state.highlighted = None;
        state.context_path = None;
        state.context_position = Point::ORIGIN;
        state.column_resize = None;
        state.scanned = 0;
        state.total = 0;
        state.files_found = 0;
        state.skipped = 0;
        state.current_path = None;
        state.phase = DuplicateCleanupPhase::Counting;
        state.error = None;
        state.confirm_delete = false;
        state.receiver = receiver;
        state.cancel = cancel;
    }
}

pub(in crate::iced_ui) fn duplicate_cleanup_available_for_entry(entry: &FileEntry) -> bool {
    let is_system_drive =
        entry.kind == EntryKind::Drive && entry.drive_kind == Some(DriveKind::System);
    !is_system_drive
        && entry.kind.is_container()
        && !explorer::is_virtual_path(&entry.path)
        && !crate::fs::archive_listing::is_inside_archive(&entry.path)
        && entry.path.is_dir()
}

pub(in crate::iced_ui) fn duplicate_file_entry(file: &DuplicateFile) -> FileEntry {
    FileEntry {
        name: file.name.clone(),
        path: file.path.clone(),
        kind: EntryKind::File,
        category: explorer::classify_file_category(&file.path),
        drive_kind: None,
        file_system: String::new(),
        free_space: None,
        size: Some(file.size),
        percent_full: None,
        modified: None,
        created: None,
        is_hidden: file.name.starts_with('.'),
    }
}

fn replace_duplicate_entries(state: &mut DuplicateCleanupState, entries: Vec<DuplicateFile>) {
    state.extension_counts.clear();
    state.extension_group_starts.clear();
    let mut previous_extension: Option<&str> = None;
    for (index, entry) in entries.iter().enumerate() {
        *state
            .extension_counts
            .entry(entry.extension.clone())
            .or_default() += 1;
        if previous_extension != Some(entry.extension.as_str()) {
            state.extension_group_starts.push(index);
            previous_extension = Some(entry.extension.as_str());
        }
    }
    let paths = entries
        .iter()
        .map(|entry| &entry.path)
        .collect::<HashSet<_>>();
    state.selected.retain(|path| paths.contains(path));
    state.entries = entries;
    refresh_all_duplicate_candidates_selected(state);
    retain_duplicate_row_state(state);
}

pub(in crate::iced_ui) fn refresh_all_duplicate_candidates_selected(
    state: &mut DuplicateCleanupState,
) {
    let mut candidates = state
        .entries
        .iter()
        .filter(|entry| entry.kind != crate::fs::duplicates::DuplicateKind::Original);
    let Some(first) = candidates.next() else {
        state.all_candidates_selected = false;
        return;
    };
    state.all_candidates_selected = state.selected.contains(&first.path)
        && candidates.all(|entry| state.selected.contains(&entry.path));
}

fn retain_duplicate_row_state(state: &mut DuplicateCleanupState) {
    let exists = |path: &PathBuf| state.entries.iter().any(|entry| &entry.path == path);
    if state.highlighted.as_ref().is_some_and(|path| !exists(path)) {
        state.highlighted = None;
    }
    if state
        .context_path
        .as_ref()
        .is_some_and(|path| !exists(path))
    {
        state.context_path = None;
        state.context_position = Point::ORIGIN;
    }
}

fn duplicate_scan_worker(root: PathBuf) -> (Receiver<DuplicateScanEvent>, Arc<AtomicBool>) {
    let (sender, receiver) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancel);
    thread::spawn(move || {
        crate::fs::duplicates::scan_folder(root, sender, worker_cancel);
    });
    (receiver, cancel)
}
