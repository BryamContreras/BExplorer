use super::*;

fn file_drag_source_allowed(is_this_pc_root: bool, is_trash_item: bool, kind: &EntryKind) -> bool {
    !(is_trash_item || (is_this_pc_root && *kind == EntryKind::Drive))
}

impl BExplorerIced {
    pub(in crate::iced_ui) fn refresh_file_drag_fade_snapshot(&mut self) {
        let Some(drag) = self.file_drag.as_ref().filter(|drag| drag.dragging) else {
            return;
        };
        self.file_drag_fade_snapshot = Some(drag.clone());
        self.file_drag_fade_target = 1.0;
    }

    pub(in crate::iced_ui) fn fade_out_file_drag_overlay(&mut self, drag: &FileDragState) {
        if drag.dragging {
            self.file_drag_fade_snapshot = Some(drag.clone());
            self.file_drag_fade_target = 0.0;
        }
    }

    pub(in crate::iced_ui) fn start_rubber_band_selection(&mut self, pane: PaneId) {
        if self.file_drag.is_some() {
            return;
        }
        let Some((pointer_pane, start)) = self.pane_pointer else {
            return;
        };
        if pointer_pane != pane {
            return;
        }

        self.focus_pane(pane);
        self.context_menu = None;
        let keep_existing = self.current_modifiers.command() || self.current_modifiers.shift();
        let base_selected = if keep_existing {
            self.pane(pane).selected.clone()
        } else {
            self.pane_mut(pane).selected.clear();
            HashSet::new()
        };
        self.rubber_band = Some(RubberBandSelection {
            pane,
            start,
            current: start,
            base_selected,
        });
    }

    pub(in crate::iced_ui) fn start_file_drag(
        &mut self,
        pane: PaneId,
        index: usize,
    ) -> Task<Message> {
        self.focus_pane(pane);
        let Some(entry) = self.pane(pane).entries.get(index) else {
            return Task::none();
        };
        let drag_allowed = file_drag_source_allowed(
            self.is_this_pc_root(pane),
            explorer::is_trash_item_path(&entry.path),
            &entry.kind,
        );
        let entry_kind = entry.kind.clone();
        let path = entry.path.clone();

        if self
            .rename_dialog
            .as_ref()
            .is_some_and(|dialog| dialog.pane == pane && dialog.path == path)
        {
            return Task::none();
        }

        let commit_task = self.commit_pending_rename_if_not(pane, Some(&path));
        if self.current_modifiers.shift() {
            self.select_range_to(pane, index);
            self.last_entry_click = None;
            return Task::batch([commit_task, self.queue_selected_preview(pane)]);
        }
        if self.current_modifiers.command() {
            let state = self.pane_mut(pane);
            if !state.selected.remove(&path) {
                state.selected.insert(path);
            }
            state.selection_anchor = Some(index);
            state.keyboard_selection_cursor = Some(index);
            self.last_entry_click = None;
            return Task::batch([commit_task, self.queue_selected_preview(pane)]);
        }

        let clicked_at = Instant::now();
        let already_selected =
            self.pane(pane).selected.len() == 1 && self.pane(pane).selected.contains(&path);
        let request_rename = entry_click_requests_rename(
            self.last_entry_click.as_ref(),
            pane,
            &path,
            already_selected,
        ) && entry_kind != EntryKind::Drive
            && !explorer::is_virtual_path(&path)
            && !crate::fs::archive_listing::is_inside_archive(&path);
        self.last_entry_click = Some(EntryClickState {
            pane,
            path: path.clone(),
            at: clicked_at,
        });
        let rename_task = if request_rename {
            let rename_path = path.clone();
            Task::perform(delay(ENTRY_CLICK_RENAME_DELAY), move |_| {
                Message::BeginClickRename(pane, rename_path, clicked_at)
            })
        } else {
            Task::none()
        };

        if !drag_allowed {
            self.select_single(pane, index);
            self.rubber_band = None;
            return Task::batch([commit_task, self.queue_selected_preview(pane), rename_task]);
        }
        let collapse_selection_on_click =
            self.pane(pane).selected.len() > 1 && self.pane(pane).selected.contains(&path);
        if !self.pane(pane).selected.contains(&path) {
            self.select_single(pane, index);
        }
        let sources = self.pane(pane).selected.iter().cloned().collect::<Vec<_>>();
        let source_entries = self
            .pane(pane)
            .entries
            .iter()
            .filter(|entry| self.pane(pane).selected.contains(&entry.path))
            .cloned()
            .collect::<Vec<_>>();
        if sources.is_empty() {
            return commit_task;
        }

        self.rubber_band = None;
        self.file_drag = Some(FileDragState {
            source_pane: pane,
            source_index: index,
            sources,
            collapse_selection_on_click,
            start_pane_point: self
                .pane_pointer
                .filter(|(pointer_pane, _)| *pointer_pane == pane)
                .map(|(_, point)| point),
            start_cursor: None,
            drop_target: None,
            sidebar_destination: None,
            dragging: false,
        });
        let mut tasks = vec![commit_task, self.queue_selected_preview(pane), rename_task];
        for entry in &source_entries {
            tasks.extend(self.queue_entry_images(entry));
        }
        Task::batch(tasks)
    }

    /// Hands a drag that has left the BExplorer surface to the operating
    /// system. Internal moves are still completed by `finish_file_drag`; this
    /// path is only taken after the pointer exits the native window.
    pub(in crate::iced_ui) fn start_external_file_drag(&mut self) -> Task<Message> {
        let Some(drag) = self.file_drag.take() else {
            return Task::none();
        };
        self.last_entry_click = None;
        self.fade_out_file_drag_overlay(&drag);

        let pane = drag.source_pane;
        let count = drag.sources.len();
        if drag.sources.is_empty()
            || drag
                .sources
                .iter()
                .any(|path| crate::fs::archive_listing::is_inside_archive(path))
        {
            self.pane_mut(pane).status =
                "Los elementos virtuales de un comprimido se extraen dentro de BExplorer".into();
            return Task::none();
        }

        let Some(id) = self.main_window_id else {
            self.pane_mut(pane).status = "La ventana principal no está disponible".into();
            return Task::none();
        };

        self.rubber_band = None;
        self.file_drag_suppressed_click = Some((drag.source_pane, drag.source_index));
        let paths = drag.sources;
        crate::utils::log::info(format!(
            "Starting native external file drag for {} path(s)",
            paths.len()
        ));
        Task::batch([
            window::run(id, move |native_window| {
                let result = (|| {
                    let display_handle = native_window.display_handle().map_err(|error| {
                        format!("No se pudo acceder a la pantalla nativa: {error}")
                    })?;
                    let window_handle = native_window.window_handle().map_err(|error| {
                        format!("No se pudo acceder a la ventana nativa: {error}")
                    })?;
                    crate::platform::start_external_file_drag(
                        paths,
                        display_handle.as_raw(),
                        window_handle.as_raw(),
                    )
                    .map_err(|error| error.to_string())
                })();
                Message::ExternalFileDragFinished(pane, count, result)
            }),
            Task::perform(delay(Duration::from_millis(180)), |_| {
                Message::ClearFileDragClickSuppression
            }),
        ])
    }

    pub(in crate::iced_ui) fn file_drag_is_ready_for_external_handoff(
        &self,
        position: Point,
    ) -> bool {
        self.file_drag.as_ref().is_some_and(|drag| {
            drag.dragging
                && (position.x <= EXTERNAL_DRAG_EDGE_TRIGGER
                    || position.y <= EXTERNAL_DRAG_EDGE_TRIGGER
                    || position.x >= self.window_size.width - EXTERNAL_DRAG_EDGE_TRIGGER
                    || position.y >= self.window_size.height - EXTERNAL_DRAG_EDGE_TRIGGER)
        })
    }

    pub(in crate::iced_ui) fn update_file_drag_pane_position(
        &mut self,
        pane: PaneId,
        point: Point,
    ) {
        if let Some(drag) = self.file_drag.as_mut()
            && !drag.dragging
        {
            if pane != drag.source_pane {
                drag.dragging = true;
            } else if let Some(start) = drag.start_pane_point {
                if pointer_moved_beyond(start, point, FILE_DRAG_START_THRESHOLD) {
                    drag.dragging = true;
                }
            } else {
                drag.start_pane_point = Some(point);
            }
        }
        self.refresh_file_drag_fade_snapshot();
    }

    pub(in crate::iced_ui) fn update_file_drag_cursor_position(&mut self, position: Point) {
        if let Some(drag) = self.file_drag.as_mut() {
            if let Some(start) = drag.start_cursor {
                if !drag.dragging
                    && pointer_moved_beyond(start, position, FILE_DRAG_START_THRESHOLD)
                {
                    drag.dragging = true;
                }
            } else {
                drag.start_cursor = Some(position);
            }
        }
        self.refresh_file_drag_fade_snapshot();
    }

    pub(in crate::iced_ui) fn set_file_drag_target(&mut self, pane: PaneId, index: usize) {
        let is_container = self.pane(pane).entries.get(index).is_some_and(|entry| {
            entry.kind.is_container() && !explorer::is_trash_item_path(&entry.path)
        });
        let Some(drag) = self.file_drag.as_mut() else {
            return;
        };
        if pane != drag.source_pane {
            drag.dragging = true;
        }
        if drag.dragging && is_container {
            drag.sidebar_destination = None;
            drag.drop_target = Some((pane, index));
        }
        self.refresh_file_drag_fade_snapshot();
    }

    pub(in crate::iced_ui) fn set_file_drag_sidebar_target(
        &mut self,
        pane: PaneId,
        destination: PathBuf,
    ) {
        if explorer::is_virtual_path(&destination) || !destination.is_dir() {
            return;
        }
        let Some(drag) = self.file_drag.as_mut() else {
            return;
        };
        if drag.dragging {
            drag.drop_target = None;
            drag.sidebar_destination = Some((pane, destination));
        }
        self.refresh_file_drag_fade_snapshot();
    }

    pub(in crate::iced_ui) fn is_file_drag_target(&self, pane: PaneId, index: usize) -> bool {
        self.file_drag
            .as_ref()
            .is_some_and(|drag| drag.dragging && drag.drop_target == Some((pane, index)))
    }

    pub(in crate::iced_ui) fn is_file_drag_sidebar_target(
        &self,
        pane: PaneId,
        path: &Path,
    ) -> bool {
        self.file_drag.as_ref().is_some_and(|drag| {
            drag.dragging
                && drag
                    .sidebar_destination
                    .as_ref()
                    .is_some_and(|(target_pane, destination)| {
                        *target_pane == pane && destination == path
                    })
        })
    }

    pub(in crate::iced_ui) fn finish_file_drag(&mut self, drag: FileDragState) -> Task<Message> {
        let (target_pane, destination) =
            if let Some((pane, destination)) = &drag.sidebar_destination {
                (*pane, destination.clone())
            } else {
                let target = drag.drop_target.or_else(|| {
                    self.pane_pointer.and_then(|(pane, point)| {
                        self.entry_at_pane_point(pane, point)
                            .map(|index| (pane, index))
                    })
                });
                let target_pane = target
                    .map(|(pane, _)| pane)
                    .or_else(|| self.pane_pointer.map(|(pane, _)| pane));
                let Some(target_pane) = target_pane else {
                    self.pane_mut(drag.source_pane).status = "Movimiento cancelado".into();
                    return Task::none();
                };
                let destination = match target {
                    Some((_, index)) => {
                        let Some(entry) = self.pane(target_pane).entries.get(index) else {
                            return Task::none();
                        };
                        if !entry.kind.is_container() {
                            self.pane_mut(drag.source_pane).status =
                                "Suelta los archivos sobre una carpeta".into();
                            return Task::none();
                        }
                        entry.path.clone()
                    }
                    None => match self.tab_for_pane(target_pane).path.clone() {
                        Some(path) => path,
                        None => {
                            self.pane_mut(drag.source_pane).status =
                                "Suelta los archivos dentro de una carpeta".into();
                            return Task::none();
                        }
                    },
                };
                (target_pane, destination)
            };

        if explorer::is_trash_path(&destination) {
            self.pane_mut(drag.source_pane).status = self
                .localized(
                    "No se pueden soltar archivos dentro de la papelera.",
                    "Files cannot be dropped inside the Recycle Bin.",
                )
                .into();
            return Task::none();
        }

        if drag
            .sources
            .iter()
            .all(|source| source.parent().is_some_and(|parent| parent == destination))
        {
            self.pane_mut(drag.source_pane).status = "Los archivos ya están en esa carpeta".into();
            return Task::none();
        }

        let archive_sources = drag
            .sources
            .iter()
            .any(|source| crate::fs::archive_listing::is_inside_archive(source));
        if archive_sources {
            return self.queue_archive_entry_extraction(
                drag.source_pane,
                target_pane,
                drag.sources,
                destination,
            );
        }
        if drag
            .sources
            .iter()
            .any(|source| destination == *source || destination.starts_with(source))
        {
            self.pane_mut(drag.source_pane).status =
                "No se puede mover una carpeta dentro de sí misma".into();
            return Task::none();
        }

        self.focus_pane(target_pane);
        self.request_transfer(
            drag.source_pane,
            drag.sources,
            destination,
            TransferKind::Move,
            false,
        )
    }

    pub(in crate::iced_ui) fn entry_at_pane_point(
        &self,
        pane: PaneId,
        point: Point,
    ) -> Option<usize> {
        self.rubber_band_intersecting_indices(
            pane,
            Rectangle {
                x: point.x,
                y: point.y,
                width: 1.0,
                height: 1.0,
            },
        )
        .into_iter()
        .next()
    }

    pub(in crate::iced_ui) fn update_rubber_band_selection(
        &mut self,
        pane: PaneId,
        current: Point,
    ) {
        let Some((start, base_selected)) = self
            .rubber_band
            .as_ref()
            .filter(|drag| drag.pane == pane)
            .map(|drag| (drag.start, drag.base_selected.clone()))
        else {
            return;
        };
        if let Some(drag) = &mut self.rubber_band {
            drag.current = current;
        }

        let rect = normalized_rect(start, current);
        let mut selected = base_selected;
        if rect.width >= RUBBER_BAND_MIN_SIZE || rect.height >= RUBBER_BAND_MIN_SIZE {
            for index in self.rubber_band_intersecting_indices(pane, rect) {
                if let Some(entry) = self.pane(pane).entries.get(index) {
                    selected.insert(entry.path.clone());
                }
            }
        }
        let anchor = self
            .displayed_entry_indices(pane)
            .into_iter()
            .find(|index| {
                self.pane(pane)
                    .entries
                    .get(*index)
                    .is_some_and(|entry| selected.contains(&entry.path))
            });
        let state = self.pane_mut(pane);
        state.selected = selected;
        state.selection_anchor = anchor;
        state.keyboard_selection_cursor = anchor;
    }

    pub(in crate::iced_ui) fn rubber_band_intersecting_indices(
        &self,
        pane: PaneId,
        rect: Rectangle,
    ) -> Vec<usize> {
        match self.effective_view_mode(pane) {
            ViewMode::Details | ViewMode::List => self.detail_rubber_band_intersections(pane, rect),
            ViewMode::Tiles
            | ViewMode::SmallIcons
            | ViewMode::MediumIcons
            | ViewMode::LargeIcons
            | ViewMode::ExtraLargeIcons => self.visual_rubber_band_intersections(pane, rect),
        }
    }

    pub(in crate::iced_ui) fn detail_rubber_band_intersections(
        &self,
        pane: PaneId,
        rect: Rectangle,
    ) -> Vec<usize> {
        let group_mode = self.effective_group_mode(pane);
        let mut current_group: Option<String> = None;
        let mut y = self.detail_header_height() - self.pane(pane).scroll_offset_y;
        // Rows end at the last visible table column. The remaining surface is
        // empty space, so a rubber-band drawn there must not select any row.
        let width = self
            .detail_column_widths(pane, (self.font_size() - 0.5).max(11.0))
            .total_width();
        let mut selected = Vec::new();
        let entries = self.filtered_entries_ref(pane);
        let state = self.pane(pane);
        let (start, end) = if group_mode == GroupMode::None {
            let range = virtual_table_range(
                entries.len(),
                self.detail_row_height(),
                (state.scroll_offset_y - self.detail_header_height()).max(0.0),
                state.scroll_viewport_height,
                state.scroll_velocity_y,
            );
            y += range.before;
            (range.start, range.end)
        } else {
            (0, entries.len())
        };
        for &index in &entries[start..end] {
            let Some(entry) = self.pane(pane).entries.get(index) else {
                continue;
            };
            if group_mode != GroupMode::None {
                let group = entry_group_label(entry, group_mode);
                if current_group.as_ref() != Some(&group) {
                    current_group = Some(group);
                    y += self.detail_group_height();
                }
            }
            let row_rect = Rectangle {
                x: 0.0,
                y,
                width,
                height: self.detail_row_height(),
            };
            if rects_intersect(rect, row_rect) {
                selected.push(index);
            }
            y += self.detail_row_height();
            if group_mode != GroupMode::None && y > rect.y + rect.height {
                break;
            }
        }
        selected
    }

    pub(in crate::iced_ui) fn visual_rubber_band_intersections(
        &self,
        pane: PaneId,
        rect: Rectangle,
    ) -> Vec<usize> {
        let mode = self.effective_view_mode(pane);
        let group_mode = self.effective_group_mode(pane);
        let layout = self.visual_layout_for_pane(pane, mode);
        let metrics = layout.metrics;
        let mut selected = Vec::new();
        let group_starts = if group_mode == GroupMode::None {
            Vec::new()
        } else {
            self.filtered_entry_group_starts(pane)
        };
        let entries = self.filtered_entries_ref(pane);
        let total = entries.len();
        let state = self.pane(pane);
        let scroll_offset = state.scroll_offset_y;
        let row_extent = metrics.cell_height + metrics.spacing;
        let test_grid_row = |start: usize, end: usize, row_y: f32, selected: &mut Vec<usize>| {
            for (column, position) in (start..end).enumerate() {
                let x =
                    metrics.grid_padding + column as f32 * (metrics.cell_width + metrics.spacing);
                let item_rect = Rectangle {
                    x,
                    y: row_y - scroll_offset,
                    width: metrics.cell_width,
                    height: metrics.cell_height,
                };
                if rects_intersect(rect, item_rect)
                    && let Some(index) = entries.get(position).copied()
                {
                    selected.push(index);
                }
            }
        };

        if group_mode == GroupMode::None {
            let row_count = total.div_ceil(layout.columns);
            let content_start = (rect.y + scroll_offset - metrics.grid_padding).max(0.0);
            let content_end =
                (rect.y + rect.height + scroll_offset - metrics.grid_padding).max(0.0);
            let first_row = ((content_start / row_extent).floor() as usize).min(row_count);
            let last_row = ((content_end / row_extent).ceil() as usize)
                .saturating_add(1)
                .min(row_count);
            for row_index in first_row..last_row {
                let start = row_index * layout.columns;
                let end = (start + layout.columns).min(total);
                test_grid_row(
                    start,
                    end,
                    metrics.grid_padding + row_index as f32 * row_extent,
                    &mut selected,
                );
            }
        } else {
            let group_extent = self.detail_group_height() + metrics.spacing;
            let content_start = (rect.y + scroll_offset).max(0.0);
            let content_end = (rect.y + rect.height + scroll_offset).max(0.0);
            let mut y = 0.0_f32;
            for (group_index, &group_start) in group_starts.iter().enumerate() {
                let group_end = group_starts.get(group_index + 1).copied().unwrap_or(total);
                let group_rows = (group_end - group_start).div_ceil(layout.columns);
                let rows_start = y + group_extent;
                let block_height = group_extent + group_rows as f32 * row_extent;
                if y + block_height < content_start {
                    y += block_height;
                    continue;
                }
                if y > content_end {
                    break;
                }
                let first_row = (((content_start - rows_start).max(0.0) / row_extent).floor()
                    as usize)
                    .min(group_rows);
                let last_row = (((content_end - rows_start).max(0.0) / row_extent).ceil() as usize)
                    .saturating_add(1)
                    .min(group_rows);
                for row_index in first_row..last_row {
                    let start = group_start + row_index * layout.columns;
                    let end = (start + layout.columns).min(group_end);
                    test_grid_row(
                        start,
                        end,
                        rows_start + row_index as f32 * row_extent,
                        &mut selected,
                    );
                }
                y += block_height;
            }
        }

        selected
    }

    pub(in crate::iced_ui) fn file_surface_width(&self, pane: PaneId) -> f32 {
        let sidebar_width = self.current_sidebar_width();
        let global_sidebar_width = if self.uses_split_sidebars() {
            0.0
        } else {
            sidebar_width
        };
        let content_width = (self.window_size.width - global_sidebar_width).max(1.0);
        let pane_width = if let Some(split) = &self.split {
            let available = (content_width - SPLIT_DIVIDER_WIDTH).max(1.0);
            match pane {
                PaneId::Primary => available * split.ratio,
                PaneId::Secondary => available * (1.0 - split.ratio),
            }
        } else {
            content_width
        };
        let pane_sidebar_width = if self.uses_split_sidebars() {
            sidebar_width
        } else {
            0.0
        };
        let preview_width = if self.preview_panel_visible(pane) {
            {
                self.current_preview_panel_width()
                    + SIDEBAR_RESIZE_HANDLE_WIDTH * self.preview_panel_progress.clamp(0.0, 1.0)
            }
        } else {
            0.0
        };
        (pane_width - pane_sidebar_width - preview_width).max(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_pc_drives_cannot_start_a_file_drag() {
        assert!(!file_drag_source_allowed(true, false, &EntryKind::Drive));
        assert!(file_drag_source_allowed(true, false, &EntryKind::Folder));
        assert!(file_drag_source_allowed(false, false, &EntryKind::Drive));
        assert!(!file_drag_source_allowed(false, true, &EntryKind::File));
    }
}
