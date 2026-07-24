use super::*;

pub(in crate::iced_ui) fn transfer_refresh_directories(job: &TransferJob) -> Vec<PathBuf> {
    let mut directories = vec![job.destination.clone()];
    for source in &job.sources {
        if let Some(parent) = source.parent().map(Path::to_path_buf)
            && !directories.contains(&parent)
        {
            directories.push(parent);
        }
    }
    directories
}

fn archive_refresh_directories(job: &ArchiveJob) -> Vec<PathBuf> {
    job.destination
        .parent()
        .map(Path::to_path_buf)
        .into_iter()
        .collect()
}

impl BExplorerIced {
    pub(super) fn context_copy(
        &mut self,
        pane: PaneId,
        target: ContextTarget,
        cut: bool,
    ) -> Task<Message> {
        let paths = self.context_paths(pane, target);
        if paths.is_empty() {
            self.pane_mut(pane).status = "No selected items".into();
            return Task::none();
        }
        let includes_drive = self
            .context_entry(pane, target)
            .is_some_and(|entry| entry.kind == EntryKind::Drive)
            || paths.iter().any(|path| {
                self.pane(pane)
                    .entries
                    .iter()
                    .any(|entry| entry.path == *path && entry.kind == EntryKind::Drive)
            });
        if includes_drive {
            self.pane_mut(pane).status = self
                .localized(
                    "Las unidades no se pueden copiar ni cortar.",
                    "Drives cannot be copied or cut.",
                )
                .into();
            return Task::none();
        }
        let portable_count = paths
            .iter()
            .filter(|path| explorer::is_portable_path(path))
            .count();
        if portable_count > 0 {
            if portable_count != paths.len() {
                self.pane_mut(pane).status =
                    "No se pueden mezclar elementos MTP y archivos locales".into();
                return Task::none();
            }
            if cut {
                self.pane_mut(pane).status = "MTP no admite cortar; se preparará una copia".into();
            }
            return self.prepare_portable_clipboard(pane, paths);
        }
        let contains_archive_entries = paths
            .iter()
            .any(|path| crate::fs::archive_listing::is_inside_archive(path));
        self.file_clipboard = Some(FileClipboardState {
            paths: paths.clone(),
            // Archive entries are virtual. A cut cannot remove an item from an
            // archive, so pasting these entries always means extraction.
            cut: cut && !contains_archive_entries,
        });
        if contains_archive_entries {
            self.pane_mut(pane).status = format!(
                "{} elemento(s) del comprimido listos para extraer",
                paths.len()
            );
            return Task::none();
        }
        let system_sync_failed = shell::copy_files(&paths, cut).err();
        self.pane_mut(pane).status = match (cut, system_sync_failed) {
            (true, Some(_)) => format!("Cut {} item(s) in BExplorer", paths.len()),
            (false, Some(_)) => format!("Copied {} item(s) in BExplorer", paths.len()),
            (true, None) => format!("Cut {} item(s)", paths.len()),
            (false, None) => format!("Copied {} item(s)", paths.len()),
        };
        Task::none()
    }

    pub(super) fn context_paste(&mut self, pane: PaneId, target: ContextTarget) -> Task<Message> {
        let Some(destination) = self.context_destination(pane, target) else {
            self.pane_mut(pane).status = "No paste destination".into();
            return Task::none();
        };
        // The desktop clipboard is authoritative: another file manager may
        // have copied new paths after BExplorer's last copy. Keep the local
        // copy only as a fallback for platforms where native file clipboard
        // access is unavailable.
        // A virtual archive item cannot be represented by the operating
        // system clipboard. Prefer our local clipboard in that one case;
        // ordinary files still use the native clipboard as the source of truth.
        let local_archive_clipboard = self.file_clipboard.clone().filter(|clipboard| {
            clipboard
                .paths
                .iter()
                .any(|path| crate::fs::archive_listing::is_inside_archive(path))
        });
        let clipboard = match local_archive_clipboard {
            Some(clipboard) => Ok(clipboard),
            None => match shell::read_files() {
                Ok(system_clipboard) => {
                    let clipboard = FileClipboardState {
                        paths: system_clipboard.paths,
                        cut: system_clipboard.cut,
                    };
                    self.file_clipboard = Some(clipboard.clone());
                    Ok(clipboard)
                }
                Err(system_error) => self.file_clipboard.clone().ok_or(system_error),
            },
        };

        let clipboard = match clipboard {
            Ok(clipboard) => clipboard,
            Err(error) => {
                return self.report_error(pane, error.to_string());
            }
        };
        if clipboard
            .paths
            .iter()
            .any(|path| crate::fs::archive_listing::is_inside_archive(path))
        {
            return self.queue_archive_entry_extraction(pane, pane, clipboard.paths, destination);
        }
        let kind = if clipboard.cut {
            TransferKind::Move
        } else {
            TransferKind::Copy
        };
        self.request_transfer(pane, clipboard.paths, destination, kind, clipboard.cut)
    }

    /// Validates a transfer and asks before a destination would overwrite an
    /// existing top-level entry. The worker still resolves each destination
    /// with the chosen policy, which protects against races and collisions
    /// found while copying nested directory contents.
    pub(super) fn request_transfer(
        &mut self,
        pane: PaneId,
        sources: Vec<PathBuf>,
        destination: PathBuf,
        kind: TransferKind,
        clear_clipboard: bool,
    ) -> Task<Message> {
        let portable_sources = sources
            .iter()
            .filter(|source| explorer::is_portable_path(source))
            .count();
        if explorer::is_portable_path(&destination) || portable_sources > 0 {
            return self.transfer_with_portable(pane, sources, destination, kind, clear_clipboard);
        }

        if let Err(error) = self.validate_transfer(&sources, &destination) {
            return self.report_error(pane, error.to_string());
        }

        let conflicts = self.transfer_conflicts(&sources, &destination);
        if !conflicts.is_empty() {
            return self.request_popup_backdrop(PopupBackdropTarget::TransferConflict(
                PendingTransferConflict {
                    pane,
                    sources,
                    destination,
                    kind,
                    clear_clipboard,
                    conflicts,
                },
            ));
        }

        match self.queue_transfer_with_policy(
            pane,
            sources,
            destination,
            kind,
            clear_clipboard,
            ConflictPolicy::KeepBoth,
        ) {
            Ok(()) => self.ensure_transfer_window_task(),
            Err(error) => self.report_error(pane, error.to_string()),
        }
    }

    pub(super) fn resolve_transfer_conflict(&mut self, policy: ConflictPolicy) -> Task<Message> {
        let Some(pending) = self.transfer_conflict_dialog.take() else {
            return Task::none();
        };
        self.popup_backdrop = None;
        match self.queue_transfer_with_policy(
            pending.pane,
            pending.sources,
            pending.destination,
            pending.kind,
            pending.clear_clipboard,
            policy,
        ) {
            Ok(()) => self.ensure_transfer_window_task(),
            Err(error) => self.report_error(pending.pane, error.to_string()),
        }
    }

    fn transfer_conflicts(&self, sources: &[PathBuf], destination: &Path) -> Vec<PathBuf> {
        sources
            .iter()
            .filter_map(|source| source.file_name().map(|name| destination.join(name)))
            .filter(|target| target.exists())
            .collect()
    }

    fn validate_transfer(
        &self,
        sources: &[PathBuf],
        destination: &Path,
    ) -> crate::utils::errors::Result<()> {
        if sources.is_empty() {
            return Err(BExplorerError::Clipboard("Clipboard is empty".into()));
        }
        if explorer::is_virtual_path(destination) {
            return Err(BExplorerError::Operation(
                "Paste is not available in this virtual location".into(),
            ));
        }
        if sources.iter().any(|path| explorer::is_virtual_path(path)) {
            return Err(BExplorerError::Operation(
                "Paste is not available from this virtual location".into(),
            ));
        }
        if sources
            .iter()
            .any(|path| crate::fs::archive_listing::is_inside_archive(path))
        {
            return Err(BExplorerError::Operation(
                "Los elementos de un comprimido se extraen al pegarlos".into(),
            ));
        }
        Ok(())
    }

    fn queue_transfer_with_policy(
        &mut self,
        pane: PaneId,
        sources: Vec<PathBuf>,
        destination: PathBuf,
        kind: TransferKind,
        clear_clipboard: bool,
        conflict_policy: ConflictPolicy,
    ) -> crate::utils::errors::Result<()> {
        self.validate_transfer(&sources, &destination)?;

        if self.active_transfers.is_empty() && self.transfer_queue.is_empty() {
            self.transfer_batch_totals.clear();
        } else if !self.transfer_in_progress_for(pane) {
            self.transfer_batch_totals.remove(&pane);
        }

        self.next_transfer_id = self.next_transfer_id.saturating_add(1);
        // Only one completed mutating operation can be undone. A new transfer
        // supersedes a previously available undo immediately.
        self.last_undo_action = None;
        let job = TransferJob {
            id: self.next_transfer_id,
            sources,
            destination,
            kind,
            conflict_policy,
        };

        self.transfer_progress
            .insert(job.id, TransferProgress::pending(&job));
        self.transfer_queue
            .push_back(QueuedTransferState { job, pane });

        if clear_clipboard {
            self.file_clipboard = None;
            let _ = shell::clear_clipboard();
        }

        self.pane_mut(pane).status = match kind {
            TransferKind::Copy => "Copy queued".into(),
            TransferKind::Move => "Move queued".into(),
        };
        self.start_next_transfers();
        Ok(())
    }

    pub(super) fn queue_archive_entry_extraction(
        &mut self,
        source_pane: PaneId,
        target_pane: PaneId,
        sources: Vec<PathBuf>,
        destination: PathBuf,
    ) -> Task<Message> {
        if sources.is_empty()
            || !sources
                .iter()
                .all(|path| crate::fs::archive_listing::is_inside_archive(path))
        {
            self.pane_mut(source_pane).status =
                "Selecciona elementos que estén dentro de un comprimido".into();
            return Task::none();
        }
        if explorer::is_virtual_path(&destination)
            || crate::fs::archive_listing::is_archive_navigation_path(&destination)
        {
            self.pane_mut(source_pane).status =
                "No se puede extraer dentro de una ubicación virtual o comprimida".into();
            return Task::none();
        }

        self.focus_pane(target_pane);
        self.last_undo_action = None;
        self.pane_mut(source_pane).status = "Extrayendo elementos...".into();
        let operation_sources = sources;
        let operation_destination = destination.clone();
        Task::perform(
            run_blocking_file_operation(move || {
                archive::extract_virtual_paths_to_destination(
                    &operation_sources,
                    &operation_destination,
                )
            }),
            move |result| {
                Message::VirtualArchiveExtractFinished(
                    source_pane,
                    target_pane,
                    destination,
                    result,
                )
            },
        )
    }

    pub(super) fn ensure_transfer_window_task(&mut self) -> Task<Message> {
        let item_count = self.transfer_items().len();
        if let Some(id) = self.transfer_window_id {
            if self.transfer_window_item_count != item_count {
                return window::position(id)
                    .map(move |position| Message::ReopenTransferWindow(id, position));
            }
            return Task::batch([
                self.sync_transfer_window_size_task(),
                window::minimize(id, false),
                window::gain_focus(id),
            ]);
        }

        let (id, task) = window::open(transfer_window_settings(self.transfer_window_size()));
        self.transfer_window_id = Some(id);
        self.transfer_window_item_count = item_count;
        task.map(Message::TransferWindowOpened)
    }

    pub(super) fn close_transfer_window_if_idle_task(&mut self) -> Task<Message> {
        if self.transfer_active() {
            return self.sync_transfer_window_size_task();
        }
        let Some(id) = self.transfer_window_id.take() else {
            return Task::none();
        };
        self.transfer_window_item_count = 0;
        self.close_window_task(id)
    }

    pub(super) fn reopen_transfer_window_task(
        &mut self,
        old_id: window::Id,
        item_count: usize,
        position: Option<Point>,
    ) -> Task<Message> {
        let (new_id, open_task) = window::open(transfer_window_settings_at(
            self.transfer_window_size(),
            position,
        ));
        self.transfer_window_id = Some(new_id);
        self.transfer_window_item_count = item_count;
        self.close_window_task(old_id)
            .chain(open_task.map(Message::TransferWindowOpened))
    }

    pub(super) fn sync_transfer_window_size_task(&mut self) -> Task<Message> {
        let item_count = self.transfer_items().len();
        if let Some(id) = self.transfer_window_id {
            if self.transfer_window_item_count != item_count {
                return window::position(id)
                    .map(move |position| Message::ReopenTransferWindow(id, position));
            }
            sync_fixed_progress_window_size_task(id, self.transfer_window_size())
        } else {
            Task::none()
        }
    }

    pub(super) fn start_next_transfers(&mut self) {
        while self.active_transfers.len() < TRANSFER_MAX_PARALLEL {
            let Some(queued) = self.transfer_queue.pop_front() else {
                break;
            };

            let control = TransferControl::new();
            let worker_control = control.clone();
            let worker_job = queued.job.clone();
            let tx = self.transfer_tx.clone();
            self.transfer_progress
                .insert(queued.job.id, TransferProgress::pending(&queued.job));
            self.pane_mut(queued.pane).status = match queued.job.kind {
                TransferKind::Copy => "Copying...".into(),
                TransferKind::Move => "Moving...".into(),
            };
            self.active_transfers.insert(
                queued.job.id,
                ActiveTransferState {
                    job: queued.job,
                    pane: queued.pane,
                    control,
                },
            );

            thread::spawn(move || {
                transfer_queue::run_transfer(worker_job, tx, worker_control);
            });
        }
    }

    pub(super) fn poll_transfer_messages(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        while let Ok(message) = self.transfer_rx.try_recv() {
            match message {
                TransferMessage::Progress(progress) => {
                    self.transfer_progress.insert(progress.job_id, progress);
                }
                TransferMessage::Finished {
                    job_id,
                    kind,
                    completed_files,
                    completed_roots,
                } => {
                    let active = self.active_transfers.remove(&job_id);
                    let owner_pane = active.as_ref().map(|active| active.pane);
                    let mut progress = self.transfer_progress.remove(&job_id).or_else(|| {
                        active
                            .as_ref()
                            .map(|active| TransferProgress::pending(&active.job))
                    });
                    if let Some(progress) = &mut progress {
                        progress.state = TransferState::Finished;
                        progress.files_done = completed_files;
                        let completed_bytes = progress.total_bytes.max(progress.copied_bytes);
                        if let Some(owner_pane) = owner_pane {
                            let totals = self.transfer_batch_totals.entry(owner_pane).or_default();
                            totals.0 = totals.0.saturating_add(completed_bytes);
                            totals.1 = totals.1.saturating_add(completed_bytes);
                        }
                        self.transfer_history.push_back(TransferHistoryState {
                            progress: progress.clone(),
                            finished_at: Instant::now(),
                        });
                    }
                    if let Some(active) = active {
                        if active.job.conflict_policy == ConflictPolicy::KeepBoth
                            && !completed_roots.is_empty()
                        {
                            self.last_undo_action = Some(match kind {
                                TransferKind::Copy => UndoAction::Copy {
                                    pane: active.pane,
                                    targets: completed_roots
                                        .iter()
                                        .map(|item| item.target.clone())
                                        .collect(),
                                },
                                TransferKind::Move => UndoAction::Move {
                                    pane: active.pane,
                                    items: completed_roots,
                                },
                            });
                        }
                        self.pane_mut(active.pane).status = match kind {
                            TransferKind::Copy => format!("Copied {completed_files} item(s)"),
                            TransferKind::Move => format!("Moved {completed_files} item(s)"),
                        };
                        tasks.push(self.refresh_panes_for_directories(
                            active.pane,
                            &transfer_refresh_directories(&active.job),
                        ));
                    }
                    self.start_next_transfers();
                }
                TransferMessage::Failed {
                    job_id,
                    error,
                    permission_denied,
                } => {
                    let active = self.active_transfers.remove(&job_id);
                    let can_elevate =
                        permission_denied && cfg!(any(target_os = "windows", target_os = "linux"));
                    let progress = self.transfer_progress.remove(&job_id);
                    if !can_elevate && let Some(mut progress) = progress {
                        progress.state = TransferState::Failed;
                        self.transfer_history.push_back(TransferHistoryState {
                            progress,
                            finished_at: Instant::now(),
                        });
                    }
                    if let Some(active) = active {
                        if can_elevate {
                            self.pane_mut(active.pane).status = if cfg!(target_os = "linux") {
                                "Esta acción requiere permisos de root".into()
                            } else {
                                "Esta acción requiere permisos de administrador".into()
                            };
                            self.elevated_transfer_dialog = Some(PendingElevatedTransfer {
                                pane: active.pane,
                                job: active.job.clone(),
                                error,
                            });
                        } else {
                            tasks.push(self.report_error(active.pane, error));
                        }
                        tasks.push(self.refresh_panes_for_directories(
                            active.pane,
                            &transfer_refresh_directories(&active.job),
                        ));
                    }
                    self.start_next_transfers();
                }
                TransferMessage::Cancelled { job_id } => {
                    let active = self.active_transfers.remove(&job_id);
                    if let Some(mut progress) = self.transfer_progress.remove(&job_id) {
                        progress.state = TransferState::Cancelled;
                        self.transfer_history.push_back(TransferHistoryState {
                            progress,
                            finished_at: Instant::now(),
                        });
                    }
                    if let Some(active) = active {
                        self.pane_mut(active.pane).status = "Transfer cancelled".into();
                        tasks.push(self.refresh_panes_for_directories(
                            active.pane,
                            &transfer_refresh_directories(&active.job),
                        ));
                    }
                    self.start_next_transfers();
                }
            }
        }

        self.prune_transfer_history();
        Task::batch(tasks)
    }

    pub(super) fn prune_transfer_history(&mut self) {
        while self
            .transfer_history
            .front()
            .is_some_and(|item| item.finished_at.elapsed() > Duration::from_secs(1))
        {
            self.transfer_history.pop_front();
        }
    }

    pub(super) fn transfer_items(&self) -> Vec<TransferDisplayState> {
        let mut items = Vec::new();
        for active in self.active_transfers.values() {
            let progress = self
                .transfer_progress
                .get(&active.job.id)
                .cloned()
                .unwrap_or_else(|| TransferProgress::pending(&active.job));
            items.push(TransferDisplayState::from_progress(progress));
        }
        for queued in &self.transfer_queue {
            items.push(TransferDisplayState::from_progress(
                TransferProgress::pending(&queued.job),
            ));
        }
        for history in self.transfer_history.iter().rev().take(3) {
            items.push(TransferDisplayState::from_progress(
                history.progress.clone(),
            ));
        }
        for deletion in self.active_deletes.values() {
            let total_files = deletion.paths.len();
            let current_name = if total_files == 1 {
                deletion.paths[0]
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_owned()
            } else {
                format!("{total_files} elementos")
            };
            items.push(TransferDisplayState {
                id: deletion.id,
                kind: if deletion.permanent {
                    TransferDisplayKind::PermanentDelete
                } else {
                    TransferDisplayKind::Trash
                },
                state: TransferState::Copying,
                current_name,
                copied_bytes: 0,
                total_bytes: 0,
                files_done: 0,
                total_files,
                bytes_per_second: 0.0,
            });
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

    pub(super) fn transfer_active(&self) -> bool {
        !self.active_transfers.is_empty()
            || !self.transfer_queue.is_empty()
            || !self.transfer_history.is_empty()
            || !self.active_deletes.is_empty()
    }

    pub(super) fn transfer_window_size(&self) -> Size {
        transfer_window_size_for_item_count(self.transfer_items().len())
    }

    pub(super) fn transfer_in_progress_for(&self, pane: PaneId) -> bool {
        self.active_transfers
            .values()
            .any(|active| active.pane == pane)
            || self.transfer_queue.iter().any(|queued| queued.pane == pane)
    }

    pub(super) fn transfer_progress_fraction_for(&self, pane: PaneId) -> Option<f32> {
        let (mut copied, mut total) = self
            .transfer_batch_totals
            .get(&pane)
            .copied()
            .unwrap_or_default();
        for (job_id, progress) in &self.transfer_progress {
            let owner = self
                .active_transfers
                .get(job_id)
                .map(|active| active.pane)
                .or_else(|| {
                    self.transfer_queue
                        .iter()
                        .find(|queued| queued.job.id == *job_id)
                        .map(|queued| queued.pane)
                });
            if owner == Some(pane) {
                copied = copied.saturating_add(progress.copied_bytes);
                total = total.saturating_add(progress.total_bytes);
            }
        }
        if total == 0 {
            None
        } else {
            Some((copied as f32 / total as f32).clamp(0.0, 1.0))
        }
    }

    pub(super) fn collapse_transfer_ownership_to_primary(&mut self) {
        for active in self.active_transfers.values_mut() {
            active.pane = PaneId::Primary;
        }
        for queued in &mut self.transfer_queue {
            queued.pane = PaneId::Primary;
        }
        if let Some((secondary_copied, secondary_total)) =
            self.transfer_batch_totals.remove(&PaneId::Secondary)
        {
            let primary = self
                .transfer_batch_totals
                .entry(PaneId::Primary)
                .or_default();
            primary.0 = primary.0.saturating_add(secondary_copied);
            primary.1 = primary.1.saturating_add(secondary_total);
        }
    }

    pub(super) fn undo_last_action(&mut self) -> Task<Message> {
        let Some(action) = self.last_undo_action.take() else {
            self.pane_mut(self.focused_pane()).status = "No hay una acción para deshacer".into();
            return Task::none();
        };
        let pane = action.pane();
        self.pane_mut(pane).status = "Deshaciendo la última acción...".into();
        let worker_action = action.clone();
        Task::perform(
            run_blocking_file_operation(move || match worker_action {
                UndoAction::Copy { targets, .. } => operations::delete_permanently(&targets),
                UndoAction::Move { items, .. } => {
                    let paths = items
                        .iter()
                        .map(|item| (item.target.clone(), item.source.clone()))
                        .collect::<Vec<_>>();
                    operations::move_paths_back(&paths)
                }
                UndoAction::Trash { records, .. } => operations::restore_from_trash(&records),
            }),
            move |result| Message::UndoFinished(action, result),
        )
    }

    pub(super) fn context_open(&mut self, pane: PaneId, target: ContextTarget) -> Task<Message> {
        let Some(entry) = self.context_entry(pane, target) else {
            return Task::none();
        };
        if entry.kind == EntryKind::Symlink {
            return self.report_error(
                pane,
                self.localized(
                    "El enlace simbólico está roto o su destino no está disponible",
                    "The symbolic link is broken or its target is unavailable",
                ),
            );
        }
        if is_mountable_disk_image_entry(&entry) {
            return self.mount_disk_image(pane, entry.path);
        }
        // A browsable archive is a virtual folder. Open its root in a fresh
        // tab so the original directory remains available, matching normal
        // archive browsing in desktop file managers.
        if crate::fs::archive_listing::is_browsable_archive(&entry.path) {
            return self.open_path_in_new_tab(pane, Some(entry.path));
        }
        if entry.kind.is_container() || explorer::is_virtual_path(&entry.path) {
            if explorer::is_portable_path(&entry.path) && !entry.kind.is_container() {
                return self.prepare_portable_open(pane, entry.path);
            }
            return self.update(Message::Navigate(pane, Some(entry.path)));
        }
        match operations::open_path(&entry.path) {
            Ok(()) => self.pane_mut(pane).status = "Opened".into(),
            Err(error) => return self.report_error(pane, error.to_string()),
        }
        Task::none()
    }

    pub(super) fn context_open_with(
        &mut self,
        pane: PaneId,
        target: ContextTarget,
    ) -> Task<Message> {
        let Some(entry) = self.context_entry(pane, target) else {
            return Task::none();
        };
        if entry.kind == EntryKind::Symlink {
            return self.report_error(
                pane,
                self.localized(
                    "El enlace simbólico está roto o su destino no está disponible",
                    "The symbolic link is broken or its target is unavailable",
                ),
            );
        }
        if explorer::is_virtual_path(&entry.path) {
            return self.report_error(pane, "Open with is not available for virtual locations");
        }
        let path = entry.path;
        let status = self
            .localized(
                "Abriendo selector de aplicaciones...",
                "Opening application chooser...",
            )
            .to_owned();
        self.pane_mut(pane).status = status;
        Task::perform(
            run_blocking_file_operation(move || shell::open_with(&path)),
            move |result| Message::OpenWithChooserFinished(pane, result),
        )
    }

    pub(super) fn context_open_file_location(
        &mut self,
        pane: PaneId,
        target: ContextTarget,
    ) -> Task<Message> {
        let Some(entry) = self.context_entry(pane, target) else {
            return Task::none();
        };
        let Some(location) = containing_location(&entry.path) else {
            return self.report_error(pane, "The file location is not available");
        };
        self.pending_reveal_in_new_tab = Some((pane, location.clone(), entry.path.clone()));
        self.open_path_in_new_tab(pane, Some(location))
    }

    pub(super) fn context_open_terminal(
        &mut self,
        pane: PaneId,
        target: ContextTarget,
    ) -> Task<Message> {
        let path = self
            .context_entry(pane, target)
            .map(|entry| entry.path)
            .or_else(|| self.tab_for_pane(pane).path.clone());
        let Some(path) = path else {
            return self.report_error(pane, "Terminal is not available here");
        };
        if explorer::is_virtual_path(&path) {
            return self.report_error(pane, "Terminal is not available for virtual locations");
        }
        match shell::open_terminal_at(&path) {
            Ok(()) => self.pane_mut(pane).status = "Terminal opened".into(),
            Err(error) => return self.report_error(pane, error.to_string()),
        }
        Task::none()
    }

    pub(super) fn context_properties(
        &mut self,
        pane: PaneId,
        target: ContextTarget,
    ) -> Task<Message> {
        #[cfg(not(target_os = "linux"))]
        {
            let path = self
                .context_entry(pane, target)
                .map(|entry| entry.path)
                .or_else(|| self.tab_for_pane(pane).path.clone());
            self.open_properties_paths(pane, path.into_iter().collect())
        }

        #[cfg(target_os = "linux")]
        let paths = if matches!(target, ContextTarget::Background) {
            self.tab_for_pane(pane).path.clone().into_iter().collect()
        } else {
            self.context_paths(pane, target)
        };
        #[cfg(target_os = "linux")]
        self.open_properties_paths(pane, paths)
    }

    pub(super) fn selection_properties(&mut self, pane: PaneId) -> Task<Message> {
        #[cfg(not(target_os = "linux"))]
        return self.context_properties(pane, ContextTarget::Background);

        #[cfg(target_os = "linux")]
        let mut paths = self.pane(pane).selected.iter().cloned().collect::<Vec<_>>();
        #[cfg(target_os = "linux")]
        paths.sort();
        #[cfg(target_os = "linux")]
        if paths.is_empty()
            && let Some(path) = self.tab_for_pane(pane).path.clone()
        {
            paths.push(path);
        }
        #[cfg(target_os = "linux")]
        self.open_properties_paths(pane, paths)
    }

    fn open_properties_paths(&mut self, pane: PaneId, paths: Vec<PathBuf>) -> Task<Message> {
        if paths.is_empty() {
            return self.report_error(pane, "No properties target");
        }
        if paths.iter().any(|path| explorer::is_virtual_path(path)) {
            return self.report_error(pane, "Properties are not available for virtual locations");
        }

        #[cfg(target_os = "linux")]
        return self.open_properties_window(pane, paths);

        #[cfg(not(target_os = "linux"))]
        let path = &paths[0];
        #[cfg(not(target_os = "linux"))]
        match shell::show_properties(path) {
            Ok(()) => self.pane_mut(pane).status = "Properties opened".into(),
            Err(error) => return self.report_error(pane, error.to_string()),
        }
        #[cfg(not(target_os = "linux"))]
        Task::none()
    }

    pub(super) fn context_format_drive(
        &mut self,
        pane: PaneId,
        target: ContextTarget,
    ) -> Task<Message> {
        let Some(entry) = self.context_entry(pane, target) else {
            return Task::none();
        };
        if entry.kind != EntryKind::Drive || !entry.drive_kind.is_some_and(DriveKind::is_formatable)
        {
            return self.report_error(
                pane,
                self.localized(
                    "Esta unidad no se puede formatear desde BExplorer",
                    "This drive cannot be formatted from BExplorer",
                ),
            );
        }
        let file_systems = operations::available_format_filesystems(&entry.path);
        let Some(default_file_system) = file_systems
            .iter()
            .find(|filesystem| filesystem.eq_ignore_ascii_case(&entry.file_system))
            .cloned()
            .or_else(|| file_systems.first().cloned())
        else {
            return self.report_error(
                pane,
                self.localized(
                    "No hay formatos disponibles en este sistema",
                    "No formats are available on this system",
                ),
            );
        };
        let drive_identity = match operations::format_drive_identity(&entry.path) {
            Ok(identity) => identity,
            Err(error) => return self.report_error(pane, error.to_string()),
        };
        let display_name = entry.name.clone();
        // Linux automounters commonly use the filesystem UUID as the mount
        // directory when a volume has no label. Reusing that directory name
        // would exceed the label limit of filesystems such as ext4 or XFS and
        // make an otherwise valid format fail. Start empty and let the user
        // choose an intentional label there.
        #[cfg(all(unix, not(target_os = "macos")))]
        let volume_label = String::new();
        #[cfg(not(all(unix, not(target_os = "macos"))))]
        let volume_label = entry
            .name
            .rsplit_once(" (")
            .map(|(label, _)| label.to_owned())
            .unwrap_or(entry.name);
        let dialog = FormatDialogState {
            pane,
            path: entry.path,
            display_name,
            capacity: entry.size,
            file_systems,
            file_system: default_file_system,
            drive_identity,
            volume_label,
            allocation_unit_size: self.localized("Predeterminado", "Default").to_owned(),
            quick_format: true,
            confirm_erase: false,
        };
        self.request_popup_backdrop(PopupBackdropTarget::Format(dialog))
    }

    pub(super) fn confirm_format_dialog(&mut self) -> Task<Message> {
        let Some(dialog) = self.format_dialog.clone() else {
            return Task::none();
        };
        if !dialog.confirm_erase || dialog.file_system.trim().is_empty() {
            return Task::none();
        }
        let allocation_unit_size = parse_allocation_unit_size(&dialog.allocation_unit_size);
        let pane = dialog.pane;
        let path = dialog.path.clone();
        let worker_path = path.clone();
        let filesystem = dialog.file_system.clone();
        let label = dialog.volume_label.trim().to_owned();
        let quick = dialog.quick_format;
        let drive_identity = dialog.drive_identity;
        self.format_dialog = None;
        self.popup_backdrop = None;
        let state = self.pane_mut(pane);
        // Ignore a stale directory load that may still be completing while
        // the selected drive is being formatted.
        state.request_id = state.request_id.wrapping_add(1);
        state.loading = true;
        state.formatting = true;
        state.formatting_path = Some(path.clone());
        state.status.clear();
        Task::perform(
            run_blocking_file_operation(move || {
                operations::format_drive(
                    &worker_path,
                    &filesystem,
                    &label,
                    quick,
                    allocation_unit_size,
                    drive_identity.as_ref(),
                )
            }),
            move |result| Message::FormatFinished(pane, path, result),
        )
    }

    pub(super) fn cancel_format_dialog(&mut self) -> Task<Message> {
        self.request_popup_close(PendingPopupClose::FormatDialog)
    }

    pub(super) fn commit_pending_rename_if_not(
        &mut self,
        pane: PaneId,
        path: Option<&Path>,
    ) -> Task<Message> {
        let editing_same_path = self.rename_dialog.as_ref().is_some_and(|dialog| {
            dialog.pane == pane && path.is_some_and(|path| dialog.path.as_path() == path)
        });
        if editing_same_path {
            Task::none()
        } else {
            self.commit_pending_rename()
        }
    }

    pub(super) fn commit_pending_rename(&mut self) -> Task<Message> {
        let Some(dialog) = self.rename_dialog.clone() else {
            return Task::none();
        };

        // `TextInput::on_submit` and the application-wide keyboard listener
        // can receive this physical Enter in separate update passes. Block
        // only that brief propagation window; later Enter presses still open
        // the selected item normally.
        self.suppress_open_after_rename_until =
            Some(std::time::Instant::now() + std::time::Duration::from_millis(350));

        let target_name = rename_target_name(&dialog.value);
        if target_name.is_empty() {
            self.pane_mut(dialog.pane).status = "Name cannot be empty".into();
            return Task::none();
        }

        let current_name = dialog
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if target_name == current_name {
            self.rename_dialog = None;
            return Task::none();
        }

        if !self.begin_file_operation(dialog.pane, "Renaming...") {
            return Task::none();
        }
        self.rename_dialog = None;
        let operation_path = dialog.path.clone();
        Task::perform(
            run_blocking_file_operation(move || {
                operations::rename_path(&operation_path, &target_name)
            }),
            move |result| Message::RenameFinished(dialog, result),
        )
    }

    pub(super) fn confirm_permanent_delete(&mut self) -> Task<Message> {
        let Some(pending) = self.permanent_delete_dialog.take() else {
            return Task::none();
        };

        if !self.begin_file_operation(pending.pane, "Deleting permanently...") {
            self.permanent_delete_dialog = Some(pending);
            return Task::none();
        }
        let pane = pending.pane;
        let paths = pending.paths;
        self.next_transfer_id = self.next_transfer_id.saturating_add(1);
        let transfer_id = self.next_transfer_id;
        self.active_deletes.insert(
            transfer_id,
            ActiveDeleteState {
                id: transfer_id,
                pane,
                paths: paths.clone(),
                permanent: true,
            },
        );
        let worker_paths = paths.clone();
        let delete_task = Task::perform(
            run_blocking_file_operation(move || operations::delete_permanently(&worker_paths)),
            move |result| Message::PermanentDeleteFinished(pane, paths, result),
        );
        Task::batch([self.ensure_transfer_window_task(), delete_task])
    }

    pub(super) fn context_begin_rename(
        &mut self,
        pane: PaneId,
        target: ContextTarget,
    ) -> Task<Message> {
        let Some(entry) = self.context_entry(pane, target) else {
            return Task::none();
        };
        if explorer::is_virtual_path(&entry.path) {
            self.pane_mut(pane).status = "Rename is not available for virtual locations yet".into();
            return Task::none();
        }
        let edit_value = rename_edit_value(&entry);
        let select_end = rename_selection_end(&entry, &edit_value);
        let mut editor = text_editor::Content::with_text(&edit_value);
        select_rename_editor_prefix(&mut editor, select_end);
        let dialog = RenameState {
            pane,
            path: entry.path.clone(),
            value: edit_value,
            editor,
            select_end,
        };
        if let Some(index) = self
            .pane(pane)
            .entries
            .iter()
            .position(|candidate| candidate.path == entry.path)
        {
            self.select_single(pane, index);
        }
        self.request_popup_backdrop(PopupBackdropTarget::Rename(dialog))
    }

    pub(super) fn context_delete(
        &mut self,
        pane: PaneId,
        target: ContextTarget,
        permanent: bool,
    ) -> Task<Message> {
        let paths = self.context_paths(pane, target);
        if paths.is_empty() {
            self.pane_mut(pane).status = "No selected items".into();
            return Task::none();
        }
        if paths.iter().all(|path| explorer::is_portable_path(path)) {
            return self.delete_portable_paths(pane, paths);
        }
        if paths.iter().any(|path| explorer::is_portable_path(path)) {
            self.pane_mut(pane).status =
                "No se pueden eliminar juntos elementos MTP y archivos locales".into();
            return Task::none();
        }
        if permanent {
            self.last_undo_action = None;
            return self.request_popup_backdrop(PopupBackdropTarget::PermanentDelete(
                PendingPermanentDelete { pane, paths },
            ));
        }
        if !self.begin_file_operation(pane, "Moving to trash...") {
            return Task::none();
        }
        self.last_undo_action = None;
        self.next_transfer_id = self.next_transfer_id.saturating_add(1);
        let transfer_id = self.next_transfer_id;
        self.active_deletes.insert(
            transfer_id,
            ActiveDeleteState {
                id: transfer_id,
                pane,
                paths: paths.clone(),
                permanent: false,
            },
        );
        let worker_paths = paths.clone();
        let delete_task = Task::perform(
            run_blocking_file_operation(move || {
                operations::delete_to_trash_with_undo(&worker_paths)
            }),
            move |result| Message::TrashFinished(pane, paths, result),
        );
        Task::batch([self.ensure_transfer_window_task(), delete_task])
    }

    pub(super) fn open_archive_dialog(&mut self, pane: PaneId) -> Task<Message> {
        let paths = self.pane(pane).selected.iter().cloned().collect();
        self.open_archive_dialog_for_paths(pane, paths)
    }

    pub(super) fn open_archive_dialog_for_context(
        &mut self,
        pane: PaneId,
        target: ContextTarget,
    ) -> Task<Message> {
        self.open_archive_dialog_for_paths(pane, self.context_paths(pane, target))
    }

    fn open_archive_dialog_for_paths(
        &mut self,
        pane: PaneId,
        sources: Vec<PathBuf>,
    ) -> Task<Message> {
        if !self.archive_sources_are_valid(pane, &sources) {
            return Task::none();
        }
        let dialog = ArchiveDialogState {
            pane,
            name: self.default_archive_name(pane, &sources),
            sources,
            format: ArchiveFormat::Zip,
            method: ArchiveCompressionMethod::Normal,
            use_password: false,
            password: String::new(),
            password_confirmation: String::new(),
            show_password: false,
            show_password_confirmation: false,
        };
        self.request_popup_backdrop(PopupBackdropTarget::Archive(dialog))
    }

    pub(super) fn start_context_archive_default(
        &mut self,
        pane: PaneId,
        target: ContextTarget,
        format: ArchiveFormat,
    ) -> Task<Message> {
        let sources = self.context_paths(pane, target);
        if !self.archive_sources_are_valid(pane, &sources) {
            return Task::none();
        }
        let name = self.default_archive_name(pane, &sources);
        match self.start_archive_job(
            pane,
            sources,
            name,
            format,
            ArchiveCompressionMethod::Normal,
            None,
        ) {
            Ok(task) => task,
            Err(error) => self.report_error(pane, error),
        }
    }

    pub(super) fn start_context_extract(
        &mut self,
        pane: PaneId,
        target: ContextTarget,
        mode: ExtractMode,
    ) -> Task<Message> {
        let Some(entry) = self.context_entry(pane, target) else {
            return Task::none();
        };
        if !crate::fs::archive_listing::has_extractable_archive_extension(&entry.path) {
            self.pane_mut(pane).status = "El archivo no se puede extraer".into();
            return Task::none();
        }

        match self.start_extract_job(pane, entry.path, mode) {
            Ok(task) => task,
            Err(error) => self.report_error(pane, error),
        }
    }

    pub(super) fn confirm_archive_dialog(&mut self) -> Task<Message> {
        let Some(dialog) = self.archive_dialog.clone() else {
            return Task::none();
        };
        if dialog.name.trim().is_empty() {
            self.pane_mut(dialog.pane).status = "El nombre del archivo no puede estar vacio".into();
            return Task::none();
        }
        if dialog.use_password && dialog.password.is_empty() {
            self.pane_mut(dialog.pane).status = "Escribe una contrasena para el archivo".into();
            return Task::none();
        }
        if dialog.use_password && dialog.password != dialog.password_confirmation {
            return Task::none();
        }

        let password = dialog.use_password.then_some(dialog.password.as_str());
        match self.start_archive_job(
            dialog.pane,
            dialog.sources,
            dialog.name,
            dialog.format,
            dialog.method,
            password,
        ) {
            Ok(task) => {
                self.archive_dialog = None;
                task
            }
            Err(error) => self.report_error(dialog.pane, error),
        }
    }

    fn archive_sources_are_valid(&mut self, pane: PaneId, sources: &[PathBuf]) -> bool {
        if sources.is_empty() {
            self.pane_mut(pane).status = "No hay elementos seleccionados para comprimir".into();
            return false;
        }
        if sources.iter().any(|path| explorer::is_virtual_path(path)) {
            self.pane_mut(pane).status =
                "La compresion no esta disponible en ubicaciones virtuales".into();
            return false;
        }
        true
    }

    pub(super) fn default_archive_name(&self, pane: PaneId, sources: &[PathBuf]) -> String {
        if sources.len() == 1 {
            return sources[0]
                .file_stem()
                .or_else(|| sources[0].file_name())
                .and_then(|name| name.to_str())
                .filter(|name| !name.trim().is_empty())
                .unwrap_or("Archive")
                .to_string();
        }
        self.tab_for_pane(pane)
            .path
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or("Archive")
            .to_string()
    }

    fn start_archive_job(
        &mut self,
        pane: PaneId,
        sources: Vec<PathBuf>,
        name: String,
        format: ArchiveFormat,
        method: ArchiveCompressionMethod,
        password: Option<&str>,
    ) -> Result<Task<Message>, String> {
        let destination = self.archive_destination(pane, &sources, &name, format)?;
        self.last_undo_action = None;
        self.next_archive_id = self.next_archive_id.saturating_add(1);
        let job = ArchiveJob {
            id: self.next_archive_id,
            kind: ArchiveJobKind::Compress,
            format,
            method,
            password: password
                .filter(|value| !value.is_empty())
                .map(str::to_string),
            sources,
            destination: destination.clone(),
            archive_path: destination,
            extract_mode: ExtractMode::Here,
        };
        let progress = ArchiveProgress {
            completed: 0,
            total: 0,
            files: 0,
            command: "Compress".into(),
            file_name: job
                .sources
                .first()
                .and_then(|path| path.file_name())
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_string(),
        };
        let (sender, receiver) = mpsc::channel();
        let cancel = Arc::new(AtomicU32::new(0));
        let worker_job = job.clone();
        let worker_cancel = cancel.clone();
        thread::spawn(move || archive::run_archive_job(worker_job, sender, worker_cancel));
        self.active_archives.insert(
            job.id,
            ActiveArchiveState {
                job,
                pane,
                receiver,
                cancel,
                progress,
            },
        );
        self.pane_mut(pane).status = "Comprimiendo...".into();
        Ok(self.ensure_archive_window_task())
    }

    fn start_extract_job(
        &mut self,
        pane: PaneId,
        archive_path: PathBuf,
        mode: ExtractMode,
    ) -> Result<Task<Message>, String> {
        let destination = archive::planned_extract_destination(&archive_path, mode)
            .map_err(|error| error.to_string())?;
        self.last_undo_action = None;
        let format = if archive_path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
        {
            ArchiveFormat::Zip
        } else {
            ArchiveFormat::SevenZip
        };
        self.next_archive_id = self.next_archive_id.saturating_add(1);
        let job = ArchiveJob {
            id: self.next_archive_id,
            kind: ArchiveJobKind::Extract,
            format,
            method: ArchiveCompressionMethod::Normal,
            password: None,
            sources: vec![archive_path.clone()],
            destination,
            archive_path: archive_path.clone(),
            extract_mode: mode,
        };
        let progress = ArchiveProgress {
            completed: 0,
            total: 0,
            files: 0,
            command: "Extract".into(),
            file_name: archive_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_string(),
        };
        let (sender, receiver) = mpsc::channel();
        let cancel = Arc::new(AtomicU32::new(0));
        let worker_job = job.clone();
        let worker_cancel = cancel.clone();
        thread::spawn(move || archive::run_archive_job(worker_job, sender, worker_cancel));
        self.active_archives.insert(
            job.id,
            ActiveArchiveState {
                job,
                pane,
                receiver,
                cancel,
                progress,
            },
        );
        self.pane_mut(pane).status = "Extrayendo...".into();
        Ok(self.ensure_archive_window_task())
    }

    fn archive_destination(
        &self,
        pane: PaneId,
        sources: &[PathBuf],
        name: &str,
        format: ArchiveFormat,
    ) -> Result<PathBuf, String> {
        let directory = self
            .tab_for_pane(pane)
            .path
            .as_ref()
            .filter(|path| path.is_dir())
            .cloned()
            .or_else(|| {
                sources
                    .first()
                    .and_then(|path| path.parent().map(Path::to_path_buf))
            })
            .ok_or_else(|| "No se pudo determinar la carpeta de destino".to_string())?;
        let name = Path::new(name.trim())
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.trim().is_empty())
            .ok_or_else(|| "El nombre del archivo no es valido".to_string())?;
        let base = name
            .strip_suffix(".zip")
            .or_else(|| name.strip_suffix(".7z"))
            .unwrap_or(name)
            .trim_end_matches('.')
            .trim();
        if base.is_empty() {
            return Err("El nombre del archivo no es valido".into());
        }
        let extension = format.extension();
        let mut candidate = directory.join(format!("{base}.{extension}"));
        let mut suffix = 2_u32;
        while candidate.exists()
            || self
                .active_archives
                .values()
                .any(|active| active.job.destination == candidate)
        {
            candidate = directory.join(format!("{base} ({suffix}).{extension}"));
            suffix = suffix.saturating_add(1);
        }
        Ok(candidate)
    }

    pub(super) fn ensure_archive_window_task(&mut self) -> Task<Message> {
        let item_count = self.archive_items().len();
        if let Some(id) = self.archive_window_id {
            if self.archive_window_item_count != item_count {
                return window::position(id)
                    .map(move |position| Message::ReopenArchiveWindow(id, position));
            }
            return Task::batch([
                self.sync_archive_window_size_task(),
                window::minimize(id, false),
                window::gain_focus(id),
            ]);
        }

        let (id, task) = window::open(archive_window_settings(self.archive_window_size()));
        self.archive_window_id = Some(id);
        self.archive_window_item_count = item_count;
        task.map(Message::ArchiveWindowOpened)
    }

    pub(super) fn reopen_archive_window_task(
        &mut self,
        old_id: window::Id,
        item_count: usize,
        position: Option<Point>,
    ) -> Task<Message> {
        let (new_id, open_task) = window::open(archive_window_settings_at(
            self.archive_window_size(),
            position,
        ));
        self.archive_window_id = Some(new_id);
        self.archive_window_item_count = item_count;
        self.close_window_task(old_id)
            .chain(open_task.map(Message::ArchiveWindowOpened))
    }

    pub(super) fn sync_archive_window_size_task(&mut self) -> Task<Message> {
        let item_count = self.archive_items().len();
        if let Some(id) = self.archive_window_id {
            if self.archive_window_item_count != item_count {
                return window::position(id)
                    .map(move |position| Message::ReopenArchiveWindow(id, position));
            }
            sync_fixed_progress_window_size_task(id, self.archive_window_size())
        } else {
            Task::none()
        }
    }

    pub(super) fn poll_archive_messages(&mut self) -> Task<Message> {
        let mut messages = Vec::new();
        for id in self.active_archives.keys().copied().collect::<Vec<_>>() {
            if let Some(active) = self.active_archives.get(&id) {
                while let Ok(message) = active.receiver.try_recv() {
                    messages.push((id, message));
                }
            }
        }

        let mut tasks = Vec::new();
        for (id, message) in messages {
            match message {
                ArchiveProgressMsg::Progress(progress) => {
                    if let Some(active) = self.active_archives.get_mut(&id) {
                        active.progress = progress;
                    }
                }
                ArchiveProgressMsg::Finished(result) => {
                    let Some(active) = self.active_archives.remove(&id) else {
                        continue;
                    };
                    let state = if result.is_ok() {
                        ArchiveState::Finished
                    } else {
                        ArchiveState::Failed
                    };
                    let mut progress = active.progress.clone();
                    if result.is_ok() && progress.total > 0 {
                        progress.completed = progress.total;
                    }
                    let refresh_directories = archive_refresh_directories(&active.job);
                    self.archive_history.push_back(ArchiveHistoryState {
                        job: active.job,
                        progress,
                        state,
                        finished_at: Instant::now(),
                    });
                    match result {
                        Ok(path) => {
                            self.pane_mut(active.pane).status =
                                format!("Archivo creado: {}", path.display());
                            tasks.push(
                                self.refresh_panes_for_directories(
                                    active.pane,
                                    &refresh_directories,
                                ),
                            );
                        }
                        Err(error) => {
                            tasks.push(self.report_error(active.pane, error));
                            tasks.push(
                                self.refresh_panes_for_directories(
                                    active.pane,
                                    &refresh_directories,
                                ),
                            );
                        }
                    }
                }
                ArchiveProgressMsg::Cancelled => {
                    let Some(active) = self.active_archives.remove(&id) else {
                        continue;
                    };
                    let refresh_directories = archive_refresh_directories(&active.job);
                    self.archive_history.push_back(ArchiveHistoryState {
                        job: active.job,
                        progress: active.progress,
                        state: ArchiveState::Cancelled,
                        finished_at: Instant::now(),
                    });
                    self.pane_mut(active.pane).status = "Compresion cancelada".into();
                    tasks.push(
                        self.refresh_panes_for_directories(active.pane, &refresh_directories),
                    );
                }
            }
        }
        self.prune_archive_history();
        Task::batch(tasks)
    }

    pub(super) fn archive_items(&self) -> Vec<ArchiveDisplayState> {
        let mut items = self
            .active_archives
            .values()
            .map(|active| {
                ArchiveDisplayState::new(
                    &active.job,
                    ArchiveState::Running,
                    active.progress.clone(),
                )
            })
            .collect::<Vec<_>>();
        items.extend(self.archive_history.iter().rev().take(3).map(|history| {
            ArchiveDisplayState::new(&history.job, history.state, history.progress.clone())
        }));
        items.sort_by_key(|item| match item.state {
            ArchiveState::Running => (0, item.id),
            ArchiveState::Finished | ArchiveState::Cancelled | ArchiveState::Failed => (1, item.id),
        });
        items
    }

    pub(super) fn archive_active(&self) -> bool {
        !self.active_archives.is_empty() || !self.archive_history.is_empty()
    }

    pub(super) fn archive_window_size(&self) -> Size {
        transfer_window_size_for_item_count(self.archive_items().len())
    }

    pub(super) fn cancel_archive(&mut self, id: u64) {
        if let Some(active) = self.active_archives.get(&id) {
            active.cancel.store(1, AtomicOrdering::Relaxed);
        }
    }

    fn prune_archive_history(&mut self) {
        while self
            .archive_history
            .front()
            .is_some_and(|item| item.finished_at.elapsed() > Duration::from_secs(3))
        {
            self.archive_history.pop_front();
        }
    }
}

fn parse_allocation_unit_size(value: &str) -> Option<u64> {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() || value == "default" || value == "predeterminado" {
        return None;
    }
    let (number, multiplier) = if let Some(number) = value.strip_suffix("kb") {
        (number.trim(), 1024_u64)
    } else if let Some(number) = value.strip_suffix("bytes") {
        (number.trim(), 1_u64)
    } else {
        return None;
    };
    number
        .parse::<u64>()
        .ok()
        .and_then(|number| number.checked_mul(multiplier))
}

fn containing_location(path: &Path) -> Option<PathBuf> {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod file_location_tests {
    use super::containing_location;
    use std::path::{Path, PathBuf};

    #[test]
    fn search_result_location_is_its_containing_directory() {
        assert_eq!(
            containing_location(Path::new("folder/nested/report.txt")),
            Some(PathBuf::from("folder/nested"))
        );
        assert_eq!(containing_location(Path::new("report.txt")), None);
    }
}
