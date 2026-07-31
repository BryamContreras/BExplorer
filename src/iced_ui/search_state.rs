use super::*;

impl BExplorerIced {
    pub(in crate::iced_ui) fn refresh_search(&mut self, pane: PaneId) -> Task<Message> {
        if !self.pane(pane).search_text.trim().is_empty() {
            return self.start_recursive_search(pane);
        }

        let mut tasks = Vec::new();
        self.clear_recursive_search(pane);
        self.pane_mut(pane).reset_scroll_position();
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
            state.keyboard_selection_cursor = None;
            state.search_cancel = Some(cancelled.clone());
            state.search_receiver = Some(receiver);
            state.recursive_search_active = true;
            state.search_progress_phase = 0.0;
            state.reset_scroll_position();
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
        self.filtered_entries_ref(pane).clone()
    }

    pub(in crate::iced_ui) fn filtered_entries_ref(
        &self,
        pane: PaneId,
    ) -> std::cell::Ref<'_, Vec<usize>> {
        let state = self.pane(pane);
        let signature = DisplayOrderSignature {
            entries_epoch: state.entries_epoch,
            streaming_search: state.search_receiver.is_some(),
            group_mode: self.effective_group_mode(pane),
            group_ascending: self.effective_group_ascending(pane),
            sort_column: state.sort_column,
            sort_ascending: state.sort_ascending,
        };
        {
            let cache = state.display_order.borrow();
            if cache.signature == Some(signature) {
                return std::cell::Ref::map(cache, |cache| &cache.indices);
            }
        }

        let mut indices = (0..state.entries.len()).collect::<Vec<_>>();

        if !signature.streaming_search {
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
        }
        let mut group_starts = Vec::new();
        if signature.group_mode != GroupMode::None {
            let mut previous_group: Option<String> = None;
            for (position, index) in indices.iter().copied().enumerate() {
                let group = entry_group_label(&state.entries[index], signature.group_mode);
                if previous_group.as_ref() != Some(&group) {
                    previous_group = Some(group);
                    group_starts.push(position);
                }
            }
        }
        let mut cache = state.display_order.borrow_mut();
        cache.signature = Some(signature);
        cache.indices = indices;
        cache.group_starts = group_starts;
        drop(cache);
        std::cell::Ref::map(state.display_order.borrow(), |cache| &cache.indices)
    }

    pub(in crate::iced_ui) fn filtered_entry_group_starts(&self, pane: PaneId) -> Vec<usize> {
        drop(self.filtered_entries_ref(pane));
        self.pane(pane).display_order.borrow().group_starts.clone()
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

    pub(in crate::iced_ui) fn ui_density(&self) -> f32 {
        ui_density_level(self.font_size())
    }

    pub(in crate::iced_ui) fn ui_metric(&self, base: f32) -> f32 {
        scaled_ui_metric(base, self.font_size())
    }

    pub(in crate::iced_ui) fn modal_text_surface_width(&self, base: f32) -> f32 {
        modal_text_surface_width(base, self.font_size(), self.window_size.width)
    }

    pub(in crate::iced_ui) fn ui_vertical_padding(&self, base: f32) -> f32 {
        base + self.ui_density() * 0.5
    }

    pub(in crate::iced_ui) fn stacked_text_control_height(&self, base: f32) -> f32 {
        stacked_text_control_height(base, self.font_size())
    }

    pub(in crate::iced_ui) fn toolbar_height(&self) -> f32 {
        self.ui_metric(42.0)
    }

    pub(in crate::iced_ui) fn action_bar_height(&self) -> f32 {
        (action_button_height(self.font_size()) + 10.0).max(self.ui_metric(46.0))
    }

    pub(in crate::iced_ui) fn bookmark_bar_height(&self) -> f32 {
        (bookmark_button_height(self.font_size()) + 10.0).max(self.ui_metric(46.0))
    }

    pub(in crate::iced_ui) fn status_bar_height(&self) -> f32 {
        self.ui_metric(40.0)
    }

    pub(in crate::iced_ui) fn filter_control_height(&self) -> f32 {
        self.ui_metric(32.0)
    }

    pub(in crate::iced_ui) fn tab_width(&self) -> f32 {
        self.ui_metric(TAB_WIDTH)
    }

    pub(in crate::iced_ui) fn tab_width_for_pane(&self, pane: PaneId) -> f32 {
        let tab_count = self.tab_indices_for_pane(pane).len();
        let (_, area_width) = self.title_pane_bounds(pane);
        fitted_tab_width(
            area_width,
            tab_count,
            self.ui_metric(TITLE_BUTTON_WIDTH),
            self.tab_spacing(),
            self.tab_width(),
            self.ui_metric(TAB_MIN_WIDTH),
        )
    }

    pub(in crate::iced_ui) fn tab_spacing(&self) -> f32 {
        self.ui_metric(3.0)
    }

    pub(in crate::iced_ui) fn tab_drag_stride(&self, pane: PaneId) -> f32 {
        self.tab_width_for_pane(pane) + self.tab_spacing()
    }

    pub(in crate::iced_ui) fn title_controls_width(&self) -> f32 {
        self.ui_metric(TITLE_BUTTON_WIDTH)
            + self.ui_metric(WINDOW_CAPTION_BUTTON_WIDTH) * 3.0
            + self.ui_metric(TITLE_BUTTON_GAP) * 3.0
    }

    pub(in crate::iced_ui) fn sidebar_section_height(&self) -> f32 {
        sidebar_section_height(self.font_size())
    }

    pub(in crate::iced_ui) fn sidebar_item_height(&self) -> f32 {
        sidebar_item_height_for_font(self.font_size())
    }

    pub(in crate::iced_ui) fn detail_header_height(&self) -> f32 {
        self.ui_metric(DETAIL_HEADER_HEIGHT)
            .max(ui_text_line_height((self.font_size() - 0.5).max(11.0)) + 6.0)
    }

    pub(in crate::iced_ui) fn detail_row_height(&self) -> f32 {
        detail_row_height_for_font(self.font_size())
    }

    pub(in crate::iced_ui) fn detail_group_height(&self) -> f32 {
        self.ui_metric(DETAIL_GROUP_HEIGHT)
            .max(ui_text_line_height((self.font_size() - 0.4).max(11.0)) + 4.0)
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
