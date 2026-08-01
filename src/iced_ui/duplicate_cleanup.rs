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
            sort_column: DuplicateSortColumn::Type,
            sort_ascending: true,
            scrollbar_horizontal_hovered: false,
            scrollbar_vertical_hovered: false,
            scrollbar_reveal_progress: 0.0,
            scrollbar_reveal_until: None,
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

    pub(in crate::iced_ui) fn sort_duplicate_column(
        &mut self,
        column: DuplicateSortColumn,
    ) -> Task<Message> {
        let Some(state) = self.duplicate_cleanup.as_mut() else {
            return Task::none();
        };
        let (sort_column, ascending) =
            next_duplicate_sort(state.sort_column, state.sort_ascending, column);
        state.sort_column = sort_column;
        state.sort_ascending = ascending;
        sort_duplicate_entries(&mut state.entries, sort_column, ascending);
        refresh_duplicate_group_metadata(state);
        state.context_path = None;
        state.context_position = Point::ORIGIN;
        state.column_resize = None;
        state.table_scroll_offset_y = 0.0;
        state.table_scroll_velocity_y = 0.0;
        state.table_scroll_sampled_at = None;

        iced::widget::operation::scroll_to(
            duplicate_table_scroll_id(),
            iced::widget::operation::AbsoluteOffset {
                x: None,
                y: Some(0.0),
            },
        )
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

fn replace_duplicate_entries(state: &mut DuplicateCleanupState, mut entries: Vec<DuplicateFile>) {
    sort_duplicate_entries(&mut entries, state.sort_column, state.sort_ascending);
    let paths = entries
        .iter()
        .map(|entry| &entry.path)
        .collect::<HashSet<_>>();
    state.selected.retain(|path| paths.contains(path));
    state.entries = entries;
    refresh_duplicate_group_metadata(state);
    refresh_all_duplicate_candidates_selected(state);
    retain_duplicate_row_state(state);
}

fn refresh_duplicate_group_metadata(state: &mut DuplicateCleanupState) {
    state.extension_counts.clear();
    state.extension_group_starts.clear();
    let mut previous_extension: Option<&str> = None;
    for (index, entry) in state.entries.iter().enumerate() {
        *state
            .extension_counts
            .entry(entry.extension.clone())
            .or_default() += 1;
        if state.sort_column == DuplicateSortColumn::Type
            && previous_extension != Some(entry.extension.as_str())
        {
            state.extension_group_starts.push(index);
            previous_extension = Some(entry.extension.as_str());
        }
    }
}

fn next_duplicate_sort(
    current: DuplicateSortColumn,
    ascending: bool,
    requested: DuplicateSortColumn,
) -> (DuplicateSortColumn, bool) {
    if current == requested {
        (current, !ascending)
    } else {
        (requested, true)
    }
}

fn sort_duplicate_entries(
    entries: &mut [DuplicateFile],
    column: DuplicateSortColumn,
    ascending: bool,
) {
    entries.sort_by(|left, right| {
        let ordering = match column {
            DuplicateSortColumn::Name => compare_duplicate_text(&left.name, &right.name, ascending),
            DuplicateSortColumn::Type => {
                compare_duplicate_text(&left.extension, &right.extension, ascending)
            }
            DuplicateSortColumn::Size => {
                ordered_duplicate_comparison(left.size.cmp(&right.size), ascending)
            }
            DuplicateSortColumn::Created => {
                compare_optional_duplicate_time(left.created, right.created, ascending)
            }
            DuplicateSortColumn::Modified => {
                compare_optional_duplicate_time(left.modified, right.modified, ascending)
            }
            DuplicateSortColumn::Match => ordered_duplicate_comparison(
                duplicate_kind_rank(left.kind).cmp(&duplicate_kind_rank(right.kind)),
                ascending,
            ),
            DuplicateSortColumn::Location => compare_duplicate_text(
                duplicate_location(left).as_ref(),
                duplicate_location(right).as_ref(),
                ascending,
            ),
        };
        ordering
            .then_with(|| explorer::compare_names_case_insensitive(&left.name, &right.name))
            .then_with(|| left.path.cmp(&right.path))
    });
}

fn compare_duplicate_text(left: &str, right: &str, ascending: bool) -> std::cmp::Ordering {
    ordered_duplicate_comparison(
        explorer::compare_names_case_insensitive(left, right),
        ascending,
    )
}

fn compare_optional_duplicate_time(
    left: Option<std::time::SystemTime>,
    right: Option<std::time::SystemTime>,
    ascending: bool,
) -> std::cmp::Ordering {
    match (left, right) {
        (Some(left), Some(right)) => ordered_duplicate_comparison(left.cmp(&right), ascending),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

fn ordered_duplicate_comparison(
    ordering: std::cmp::Ordering,
    ascending: bool,
) -> std::cmp::Ordering {
    if ascending {
        ordering
    } else {
        ordering.reverse()
    }
}

fn duplicate_kind_rank(kind: crate::fs::duplicates::DuplicateKind) -> u8 {
    match kind {
        crate::fs::duplicates::DuplicateKind::Original => 0,
        crate::fs::duplicates::DuplicateKind::Exact => 1,
        crate::fs::duplicates::DuplicateKind::Possible => 2,
    }
}

fn duplicate_location(file: &DuplicateFile) -> std::borrow::Cow<'_, str> {
    file.path
        .parent()
        .map_or_else(|| std::borrow::Cow::Borrowed(""), Path::to_string_lossy)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn duplicate(
        path: &str,
        size: u64,
        created: Option<std::time::SystemTime>,
        kind: crate::fs::duplicates::DuplicateKind,
    ) -> DuplicateFile {
        let path = PathBuf::from(path);
        DuplicateFile {
            name: path
                .file_name()
                .expect("test file name")
                .to_string_lossy()
                .into_owned(),
            extension: path
                .extension()
                .map(|extension| extension.to_string_lossy().to_lowercase())
                .unwrap_or_default(),
            path,
            size,
            created,
            modified: created,
            kind,
        }
    }

    #[test]
    fn duplicate_sort_toggles_or_starts_a_new_column_ascending() {
        assert_eq!(
            next_duplicate_sort(DuplicateSortColumn::Type, true, DuplicateSortColumn::Type,),
            (DuplicateSortColumn::Type, false)
        );
        assert_eq!(
            next_duplicate_sort(DuplicateSortColumn::Type, false, DuplicateSortColumn::Size,),
            (DuplicateSortColumn::Size, true)
        );
    }

    #[test]
    fn duplicate_sort_uses_numeric_sizes_and_keeps_missing_dates_last() {
        let early = UNIX_EPOCH + Duration::from_secs(10);
        let late = UNIX_EPOCH + Duration::from_secs(20);
        let mut entries = vec![
            duplicate(
                "/z/large.png",
                100,
                None,
                crate::fs::duplicates::DuplicateKind::Possible,
            ),
            duplicate(
                "/a/small.txt",
                2,
                Some(late),
                crate::fs::duplicates::DuplicateKind::Exact,
            ),
            duplicate(
                "/m/medium.jpg",
                30,
                Some(early),
                crate::fs::duplicates::DuplicateKind::Original,
            ),
        ];

        sort_duplicate_entries(&mut entries, DuplicateSortColumn::Size, true);
        assert_eq!(
            entries.iter().map(|entry| entry.size).collect::<Vec<_>>(),
            [2, 30, 100]
        );

        sort_duplicate_entries(&mut entries, DuplicateSortColumn::Created, false);
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            ["small.txt", "medium.jpg", "large.png"]
        );
    }

    #[test]
    fn duplicate_sort_supports_match_location_and_type_columns() {
        let mut entries = vec![
            duplicate(
                "/zeta/report.png",
                1,
                None,
                crate::fs::duplicates::DuplicateKind::Possible,
            ),
            duplicate(
                "/alpha/report.txt",
                1,
                None,
                crate::fs::duplicates::DuplicateKind::Original,
            ),
            duplicate(
                "/middle/report.jpg",
                1,
                None,
                crate::fs::duplicates::DuplicateKind::Exact,
            ),
        ];

        sort_duplicate_entries(&mut entries, DuplicateSortColumn::Match, true);
        assert_eq!(
            entries.iter().map(|entry| entry.kind).collect::<Vec<_>>(),
            [
                crate::fs::duplicates::DuplicateKind::Original,
                crate::fs::duplicates::DuplicateKind::Exact,
                crate::fs::duplicates::DuplicateKind::Possible,
            ]
        );

        sort_duplicate_entries(&mut entries, DuplicateSortColumn::Location, true);
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.path.parent().expect("test parent"))
                .collect::<Vec<_>>(),
            [
                Path::new("/alpha"),
                Path::new("/middle"),
                Path::new("/zeta")
            ]
        );

        sort_duplicate_entries(&mut entries, DuplicateSortColumn::Type, false);
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.extension.as_str())
                .collect::<Vec<_>>(),
            ["txt", "png", "jpg"]
        );
    }
}
