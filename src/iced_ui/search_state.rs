use super::*;

impl BExplorerIced {
    pub(in crate::iced_ui) fn refresh_search(&mut self, pane: PaneId) -> Task<Message> {
        if !self.pane(pane).search_text.trim().is_empty() {
            return self.start_recursive_search(pane);
        }

        let mut tasks = Vec::new();
        self.clear_recursive_search(pane);
        self.pane_mut(pane).render_limit = INITIAL_RENDER_LIMIT;
        self.pane_mut(pane).scroll_offset_y = 0.0;
        tasks.push(self.queue_visible_images(pane));
        tasks.push(scroll_pane_to_top_task(pane));
        Task::batch(tasks)
    }

    pub(in crate::iced_ui) fn start_recursive_search(&mut self, pane: PaneId) -> Task<Message> {
        self.cancel_recursive_search(pane);
        let query = self.pane(pane).search_text.trim().to_string();
        let root = self.tab_for_pane(pane).path.clone();
        let Some(root) = root else {
            self.pane_mut(pane).status = "Abre una carpeta antes de buscar".into();
            return Task::none();
        };
        if explorer::is_virtual_path(&root) && !explorer::is_portable_path(&root) {
            self.pane_mut(pane).status = "La búsqueda no está disponible en esta ubicación".into();
            return Task::none();
        }

        let cancelled = Arc::new(AtomicBool::new(false));
        let include_archives = self.pane(pane).search_mode == SearchMode::Complete;
        let (sender, receiver) = mpsc::channel();
        {
            let state = self.pane_mut(pane);
            if state.folder_entries.is_none() {
                state.folder_entries = Some(std::mem::take(&mut state.entries));
            } else {
                state.entries.clear();
            }
            state.mark_entries_changed();
            state.selected.clear();
            state.selection_anchor = None;
            state.search_cancel = Some(cancelled.clone());
            state.search_receiver = Some(receiver);
            state.recursive_search_active = true;
            state.search_progress_phase = 0.0;
            state.render_limit = INITIAL_RENDER_LIMIT;
            state.scroll_offset_y = 0.0;
            state.status = if include_archives {
                format!("Búsqueda completa de \"{query}\"…")
            } else {
                format!("Buscando \"{query}\"…")
            };
        }

        let show_hidden = self.config.show_hidden;
        thread::spawn(move || {
            let batch_sender = sender.clone();
            let batch_cancelled = cancelled.clone();
            let output = crate::fs::search::search_files_streaming(
                crate::fs::search::SearchOptions {
                    root,
                    query,
                    show_hidden,
                    include_archives,
                },
                &cancelled,
                move |entries| {
                    !batch_cancelled.load(AtomicOrdering::Relaxed)
                        && batch_sender
                            .send(crate::fs::search::SearchEvent::Batch(entries))
                            .is_ok()
                },
            );
            if !cancelled.load(AtomicOrdering::Relaxed) {
                let _ = sender.send(crate::fs::search::SearchEvent::Finished {
                    truncated: output.truncated,
                });
            }
        });

        Task::none()
    }

    pub(in crate::iced_ui) fn clear_recursive_search(&mut self, pane: PaneId) {
        let state = self.pane_mut(pane);
        if let Some(cancelled) = state.search_cancel.take() {
            cancelled.store(true, AtomicOrdering::Relaxed);
        }
        state.search_receiver = None;
        state.recursive_search_active = false;
        state.search_progress_phase = 0.0;
        if let Some(folder_entries) = state.folder_entries.take() {
            state.entries = folder_entries;
            state.mark_entries_changed();
            state.status = format!("{} elementos", state.entries.len());
        }
    }

    pub(in crate::iced_ui) fn cancel_recursive_search(&mut self, pane: PaneId) {
        let state = self.pane_mut(pane);
        if let Some(cancelled) = state.search_cancel.take() {
            cancelled.store(true, AtomicOrdering::Relaxed);
        }
        state.search_receiver = None;
        state.recursive_search_active = false;
        state.search_progress_phase = 0.0;
    }

    pub(in crate::iced_ui) fn search_in_progress(&self) -> bool {
        self.primary.search_receiver.is_some() || self.secondary.search_receiver.is_some()
    }

    pub(in crate::iced_ui) fn poll_searches(&mut self) -> Task<Message> {
        let mut changed_panes = Vec::new();
        for pane in [PaneId::Primary, PaneId::Secondary] {
            let complete_search = self.pane(pane).search_mode == SearchMode::Complete;
            let (events, disconnected) = {
                let Some(receiver) = self.pane(pane).search_receiver.as_ref() else {
                    continue;
                };
                let mut events = Vec::new();
                let mut disconnected = false;
                while events.len() < MAX_SEARCH_EVENTS_PER_TICK {
                    match receiver.try_recv() {
                        Ok(event) => events.push(event),
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            disconnected = true;
                            break;
                        }
                    }
                }
                (events, disconnected)
            };

            let state = self.pane_mut(pane);
            state.search_progress_phase = (state.search_progress_phase + 0.045).rem_euclid(1.0);

            if events.is_empty() && !disconnected {
                continue;
            }

            let mut finished = disconnected;
            let mut truncated = false;
            let mut received_batch = false;
            {
                let state = self.pane_mut(pane);
                for event in events {
                    match event {
                        crate::fs::search::SearchEvent::Batch(entries) => {
                            state.entries.extend(entries);
                            received_batch = true;
                        }
                        crate::fs::search::SearchEvent::Finished {
                            truncated: event_truncated,
                        } => {
                            finished = true;
                            truncated = event_truncated;
                        }
                    }
                }

                if received_batch {
                    state.mark_entries_changed();
                }

                let count = state.entries.len();
                if finished {
                    state.search_cancel = None;
                    state.search_receiver = None;
                    state.status = if truncated {
                        format!("{count} resultados (límite alcanzado)")
                    } else {
                        format!("{count} resultados")
                    };
                } else if received_batch {
                    state.status = if complete_search {
                        format!("Búsqueda completa: {count} resultados…")
                    } else {
                        format!("Búsqueda rápida: {count} resultados…")
                    };
                }
            }

            if received_batch || finished {
                changed_panes.push(pane);
            }
        }

        Task::batch(
            changed_panes
                .into_iter()
                .map(|pane| self.queue_visible_images(pane))
                .collect::<Vec<_>>(),
        )
    }

    pub(in crate::iced_ui) fn filtered_entries(&self, pane: PaneId) -> Vec<usize> {
        let state = self.pane(pane);
        let signature = DisplayOrderSignature {
            entries_epoch: state.entries_epoch,
            group_mode: self.effective_group_mode(pane),
            group_ascending: self.effective_group_ascending(pane),
            sort_column: state.sort_column,
            sort_ascending: state.sort_ascending,
        };
        {
            let cache = state.display_order.borrow();
            if cache.signature == Some(signature) {
                return cache.indices.clone();
            }
        }

        let mut indices = (0..state.entries.len()).collect::<Vec<_>>();

        indices.sort_by(|left, right| {
            compare_entries_for_view(
                &state.entries[*left],
                &state.entries[*right],
                signature.group_mode,
                signature.group_ascending,
                state.sort_column,
                state.sort_ascending,
            )
        });
        let mut cache = state.display_order.borrow_mut();
        cache.signature = Some(signature);
        cache.indices = indices.clone();
        indices
    }

    pub(in crate::iced_ui) fn selection_status_metrics(&self, pane: PaneId) -> (usize, u64) {
        let state = self.pane(pane);
        if state.selected.is_empty() {
            return (0, 0);
        }
        let selected_size = state
            .entries
            .iter()
            .filter(|entry| state.selected.contains(&entry.path))
            .filter_map(|entry| entry.size)
            .sum();
        (state.selected.len(), selected_size)
    }

    pub(in crate::iced_ui) fn font_size(&self) -> f32 {
        self.config.font_size.round().clamp(10.0, 18.0)
    }

    pub(in crate::iced_ui) fn begin_file_operation(&mut self, pane: PaneId, status: &str) -> bool {
        if !self.pending_file_operations.insert(pane) {
            self.pane_mut(pane).status = "Another file operation is still running".into();
            return false;
        }
        self.last_undo_action = None;
        self.pane_mut(pane).status = status.into();
        true
    }
}
