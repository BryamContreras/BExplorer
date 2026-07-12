use super::*;

impl BExplorerIced {
    pub(in crate::iced_ui) fn apply_accent_plane_point(&mut self, point: Point) {
        let (hue, _, _) = accent_hsv_from_color(self.config.accent_color);
        let saturation = (point.x / COLOR_PICKER_PLANE_WIDTH).clamp(0.0, 1.0);
        let value = (1.0 - point.y / COLOR_PICKER_PLANE_HEIGHT).clamp(0.0, 1.0);
        self.set_accent_color(accent_color_from_hsv(hue, saturation, value));
    }

    pub(in crate::iced_ui) fn apply_accent_hue_point(&mut self, point: Point) {
        let (_, saturation, value) = accent_hsv_from_color(self.config.accent_color);
        let hue = (point.x / COLOR_PICKER_HUE_WIDTH).clamp(0.0, 1.0) * 360.0;
        self.set_accent_color(accent_color_from_hsv(hue, saturation, value));
    }

    pub(in crate::iced_ui) fn set_accent_color(&mut self, color: [u8; 3]) {
        self.config.accent_color = color;
        self.sync_color_inputs();
    }

    pub(in crate::iced_ui) fn sync_color_inputs(&mut self) {
        self.color_rgb_inputs = accent_rgb_strings(self.config.accent_color);
    }

    pub(in crate::iced_ui) fn start_load(&mut self, pane: PaneId) -> Task<Message> {
        self.cancel_recursive_search(pane);
        self.sync_pane_view_mode_from_tab(pane);
        let path = self.tab_for_pane(pane).path.clone();
        let show_hidden = self.config.show_hidden;
        let request_id = {
            let state = self.pane_mut(pane);
            state.request_id += 1;
            state.loading = true;
            state.status = String::from("Loading...");
            state.request_id
        };
        Task::perform(
            load_entries(pane, request_id, path, show_hidden),
            |result| Message::Loaded(result.pane, result.request_id, result.entries),
        )
    }

    pub(in crate::iced_ui) fn refresh_panes_for_directories(
        &mut self,
        fallback: PaneId,
        directories: &[PathBuf],
    ) -> Task<Message> {
        let mut panes = Vec::new();
        for pane in [PaneId::Primary, PaneId::Secondary] {
            if pane == PaneId::Secondary && self.split.is_none() {
                continue;
            }
            let matches_directory = self
                .tab_for_pane(pane)
                .path
                .as_ref()
                .is_some_and(|current| directories.iter().any(|directory| directory == current));
            if matches_directory {
                panes.push(pane);
            }
        }
        if panes.is_empty() {
            panes.push(fallback);
        }
        Task::batch(panes.into_iter().map(|pane| self.start_load(pane)))
    }

    pub(in crate::iced_ui) fn queue_visible_images(&mut self, pane: PaneId) -> Task<Message> {
        let limit = self.pane(pane).render_limit;

        let entries: Vec<_> = self
            .filtered_entries(pane)
            .into_iter()
            .take(limit)
            .filter_map(|index| self.pane(pane).entries.get(index).cloned())
            .collect();
        let mut tasks = Vec::new();

        for entry in entries {
            tasks.extend(self.queue_entry_images(&entry));
        }

        Task::batch(tasks)
    }

    pub(in crate::iced_ui) fn queue_selected_preview(&mut self, pane: PaneId) -> Task<Message> {
        if !self.preview_enabled_for_pane(pane) {
            self.pdf_previews.remove(&pane);
            self.pane_mut(pane).text_preview = None;
            return Task::none();
        }

        let selected = self
            .pane(pane)
            .selected
            .iter()
            .find_map(|path| {
                self.pane(pane)
                    .entries
                    .iter()
                    .find(|entry| entry.path == *path)
            })
            .cloned();
        let Some(entry) = selected else {
            self.pdf_previews.remove(&pane);
            self.pane_mut(pane).text_preview = None;
            return Task::none();
        };
        if thumbnail_data::is_pdf_preview_candidate(&entry) {
            if self
                .pdf_previews
                .get(&pane)
                .is_some_and(|state| state.path == entry.path)
            {
                return Task::none();
            }
            self.pdf_previews.insert(
                pane,
                PdfPreviewState {
                    path: entry.path.clone(),
                    page_count: None,
                    pages: Vec::new(),
                    current_page: 0,
                    loading: true,
                },
            );
            return load_pdf_preview_page_task(pane, entry.path, 0);
        }

        self.pdf_previews.remove(&pane);
        if thumbnail_data::is_text_preview_candidate(&entry) {
            let already_loaded = self
                .pane(pane)
                .text_preview
                .as_ref()
                .is_some_and(|preview| preview.path == entry.path);
            if !already_loaded {
                let content =
                    thumbnail_data::read_text_preview(&entry.path, self.config.preview_limit_bytes)
                        .unwrap_or_else(|| {
                            "No se pudo cargar una vista previa de este documento.".into()
                        });
                self.pane_mut(pane).text_preview = Some(TextPreviewState {
                    path: entry.path.clone(),
                    content: text_editor::Content::with_text(&content),
                });
            }
            return Task::none();
        }
        self.pane_mut(pane).text_preview = None;
        if !thumbnail_data::is_visual_preview_candidate(&entry)
            || self.preview_cache.contains_key(&entry.path)
        {
            return Task::none();
        }

        self.preview_cache
            .insert(entry.path.clone(), IcedImageState::Loading);
        load_iced_image_task(IcedImageJob::Preview { path: entry.path })
    }

    pub(in crate::iced_ui) fn queue_entry_images(
        &mut self,
        entry: &FileEntry,
    ) -> Vec<Task<Message>> {
        let mut tasks = Vec::new();

        if thumbnail_data::is_thumbnail_candidate(entry) {
            if explorer::is_portable_path(&entry.path) {
                if !self.thumbnail_cache.contains_key(&entry.path) {
                    self.thumbnail_cache
                        .insert(entry.path.clone(), IcedImageState::Loading);
                    let max_bytes = self.config.preview_limit_bytes.max(512 * 1024);
                    let allow_default_resource = entry
                        .size
                        .is_some_and(|size| size <= self.config.preview_limit_bytes as u64);
                    tasks.push(load_iced_image_task(IcedImageJob::Thumbnail {
                        path: entry.path.clone(),
                        max_bytes,
                        allow_default_resource,
                    }));
                }
            } else if thumbnail_data::is_pdf_preview_candidate(entry)
                || entry
                    .size
                    .is_some_and(|size| size <= self.config.preview_limit_bytes as u64)
            {
                if !self.thumbnail_cache.contains_key(&entry.path) {
                    self.thumbnail_cache
                        .insert(entry.path.clone(), IcedImageState::Loading);
                    tasks.push(load_iced_image_task(IcedImageJob::Thumbnail {
                        path: entry.path.clone(),
                        max_bytes: self.config.preview_limit_bytes,
                        allow_default_resource: false,
                    }));
                }
            } else {
                self.thumbnail_cache
                    .insert(entry.path.clone(), IcedImageState::Missing);
            }
        }

        if let Some((cache_key, path, is_directory)) = native_icon_request_for_entry(entry) {
            if !self.native_icon_cache.contains_key(&cache_key) {
                self.native_icon_cache
                    .insert(cache_key.clone(), IcedImageState::Loading);
                tasks.push(load_iced_image_task(IcedImageJob::NativeIcon {
                    cache_key,
                    path,
                    is_directory,
                    size: thumbnail_data::NATIVE_ICON_SIZE,
                }));
            }
        }

        tasks
    }

    pub(in crate::iced_ui) fn queue_sidebar_icons(&mut self) -> Task<Message> {
        let mut icon_paths = paths::common_places()
            .into_iter()
            .map(|place| place.path)
            .collect::<Vec<_>>();
        icon_paths.extend(self.config.favorites.iter().cloned());
        icon_paths.extend(self.config.recent_paths.iter().cloned());
        icon_paths.push(filesystem_root_path());

        let mut seen = HashSet::new();
        let tasks = icon_paths
            .into_iter()
            .filter(|path| seen.insert(path.clone()))
            .map(|path| self.queue_sidebar_path_icon(&path))
            .collect::<Vec<_>>();
        Task::batch(tasks)
    }

    pub(in crate::iced_ui) fn queue_sidebar_path_icon(&mut self, path: &Path) -> Task<Message> {
        if explorer::is_virtual_path(path) {
            return Task::none();
        }
        let cache_key = thumbnail_data::native_path_icon_cache_key(
            path,
            true,
            thumbnail_data::NATIVE_ICON_SIZE,
        );
        if self.native_icon_cache.contains_key(&cache_key) {
            return Task::none();
        }
        self.native_icon_cache
            .insert(cache_key.clone(), IcedImageState::Loading);
        load_iced_image_task(IcedImageJob::NativeIcon {
            cache_key,
            path: path.to_path_buf(),
            is_directory: true,
            size: thumbnail_data::NATIVE_ICON_SIZE,
        })
    }

    pub(in crate::iced_ui) fn sync_pane_view_mode_from_tab(&mut self, pane: PaneId) {
        if self.uses_fixed_root_presentation(pane) {
            return;
        }
        let (group_mode, group_ascending) = {
            let tab = self.tab_for_pane(pane);
            (tab.group_mode, tab.group_ascending)
        };
        let state = self.pane_mut(pane);
        state.group_mode = group_mode;
        state.group_ascending = group_ascending;
    }

    /// Add a tab to the pane that initiated the action and immediately load
    /// `path`. This is used by the regular new-tab button and by virtual
    /// archive folders, which should never replace their source directory.
    pub(in crate::iced_ui) fn open_path_in_new_tab(
        &mut self,
        pane: PaneId,
        path: Option<PathBuf>,
    ) -> Task<Message> {
        self.tab_drag = None;
        self.address_edit = None;
        self.focus_pane(pane);
        self.tabs
            .push(TabState::with_view_mode(path, self.config.default_view));
        let index = self.tabs.len() - 1;
        if let Some(split) = &mut self.split {
            match pane {
                PaneId::Primary => {
                    split.primary_tabs.push(index);
                    self.active_tab = index;
                }
                PaneId::Secondary => {
                    split.secondary_tabs.push(index);
                    split.secondary_tab = index;
                }
            }
        } else {
            self.active_tab = index;
        }
        self.save_session();
        self.start_load(pane)
    }

    pub(in crate::iced_ui) fn set_view_mode_for_pane(&mut self, pane: PaneId, mode: ViewMode) {
        if self.uses_fixed_root_presentation(pane) {
            self.pane_mut(pane).fixed_root_view_override = Some(mode);
        } else {
            let tab_index = self.tab_index_for_pane(pane);
            if let Some(tab) = self.tabs.get_mut(tab_index) {
                tab.view_mode = mode;
            }
        }
    }

    pub(in crate::iced_ui) fn effective_view_mode(&self, pane: PaneId) -> ViewMode {
        if self.uses_fixed_root_presentation(pane) {
            self.pane(pane)
                .fixed_root_view_override
                .unwrap_or(ViewMode::Tiles)
        } else {
            self.tab_for_pane(pane).view_mode
        }
    }

    pub(in crate::iced_ui) fn effective_group_mode(&self, pane: PaneId) -> GroupMode {
        if self.uses_fixed_root_presentation(pane) {
            self.pane(pane)
                .fixed_root_group_override
                .unwrap_or(GroupMode::Type)
        } else {
            self.pane(pane).group_mode
        }
    }

    pub(in crate::iced_ui) fn effective_group_ascending(&self, pane: PaneId) -> bool {
        if self.uses_fixed_root_presentation(pane) {
            self.pane(pane)
                .fixed_root_group_ascending_override
                .unwrap_or(true)
        } else {
            self.pane(pane).group_ascending
        }
    }

    pub(in crate::iced_ui) fn is_this_pc_root(&self, pane: PaneId) -> bool {
        self.tab_for_pane(pane).path.is_none()
    }

    pub(in crate::iced_ui) fn uses_fixed_root_presentation(&self, pane: PaneId) -> bool {
        uses_fixed_root_presentation(self.tab_for_pane(pane).path.as_deref())
    }

    pub(in crate::iced_ui) fn reset_fixed_root_presentation(&mut self, pane: PaneId) {
        if !self.uses_fixed_root_presentation(pane) {
            return;
        }
        let state = self.pane_mut(pane);
        state.fixed_root_view_override = None;
        state.fixed_root_group_override = None;
        state.fixed_root_group_ascending_override = None;
        state.render_limit = INITIAL_RENDER_LIMIT;
        state.scroll_offset_y = 0.0;
    }

    pub(in crate::iced_ui) fn contextual_file_surface<'a>(
        &self,
        pane: PaneId,
        _palette: Palette,
        base: Element<'a, Message>,
    ) -> Element<'a, Message> {
        mouse_area(base)
            .on_press(Message::StartRubberBand(pane))
            .on_right_press(Message::OpenBackgroundContext(pane))
            .on_move(move |point| Message::PanePointerMoved(pane, point))
            .on_exit(Message::PanePointerExited(pane))
            .on_scroll(move |delta| Message::PaneMouseWheel(pane, delta))
            .into()
    }

    pub(in crate::iced_ui) fn tab_for_pane(&self, pane: PaneId) -> &TabState {
        &self.tabs[self
            .tab_index_for_pane(pane)
            .min(self.tabs.len().saturating_sub(1))]
    }

    pub(in crate::iced_ui) fn entry_display_name(&self, entry: &FileEntry) -> String {
        if self.config.show_extensions || !matches!(entry.kind, EntryKind::File | EntryKind::Other)
        {
            return entry.name.clone();
        }

        entry
            .path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| entry.name.clone())
    }

    pub(in crate::iced_ui) fn entry_presentation_opacity(
        &self,
        entry: &FileEntry,
        selected: bool,
    ) -> f32 {
        if entry.is_hidden && !selected {
            0.68
        } else {
            1.0
        }
    }

    pub(in crate::iced_ui) fn tab_index_for_pane(&self, pane: PaneId) -> usize {
        match (pane, &self.split) {
            (PaneId::Secondary, Some(split)) => split.secondary_tab,
            _ => self.active_tab,
        }
        .min(self.tabs.len().saturating_sub(1))
    }

    pub(in crate::iced_ui) fn tab_indices_for_pane(&self, pane: PaneId) -> Vec<usize> {
        match (pane, &self.split) {
            (PaneId::Primary, Some(split)) => split.primary_tabs.clone(),
            (PaneId::Secondary, Some(split)) => split.secondary_tabs.clone(),
            _ => (0..self.tabs.len()).collect(),
        }
    }

    pub(in crate::iced_ui) fn tab_index_at(&self, pane: PaneId, slot: usize) -> Option<usize> {
        self.tab_indices_for_pane(pane).get(slot).copied()
    }

    pub(in crate::iced_ui) fn set_active_tab_for_pane(&mut self, pane: PaneId, tab_index: usize) {
        match (pane, &mut self.split) {
            (PaneId::Secondary, Some(split)) => split.secondary_tab = tab_index,
            _ => self.active_tab = tab_index,
        }
    }

    pub(in crate::iced_ui) fn close_tab(&mut self, pane: PaneId, slot: usize) {
        if self.tabs.len() <= 1 {
            return;
        }
        let pane_tabs = self.tab_indices_for_pane(pane);
        let Some(&removed) = pane_tabs.get(slot) else {
            return;
        };
        let pane_active = self.tab_index_for_pane(pane);
        let replacement = if pane_active == removed {
            pane_tabs
                .get(slot + 1)
                .or_else(|| slot.checked_sub(1).and_then(|index| pane_tabs.get(index)))
                .copied()
        } else {
            Some(pane_active)
        };
        let old_primary_active = self.active_tab;
        let old_secondary_active = self.split.as_ref().map(|split| split.secondary_tab);

        self.tabs.remove(removed);
        let last = self.tabs.len().saturating_sub(1);

        if let Some(split) = &mut self.split {
            split.primary_tabs = rebase_tab_indices(&split.primary_tabs, removed);
            split.secondary_tabs = rebase_tab_indices(&split.secondary_tabs, removed);

            self.active_tab = if pane == PaneId::Primary && pane_active == removed {
                replacement.and_then(|index| rebase_tab_index(index, removed))
            } else {
                rebase_tab_index(old_primary_active, removed)
            }
            .unwrap_or_else(|| split.primary_tabs.first().copied().unwrap_or(0))
            .min(last);

            split.secondary_tab = if pane == PaneId::Secondary && pane_active == removed {
                replacement.and_then(|index| rebase_tab_index(index, removed))
            } else {
                old_secondary_active.and_then(|index| rebase_tab_index(index, removed))
            }
            .unwrap_or_else(|| {
                split
                    .secondary_tabs
                    .first()
                    .copied()
                    .unwrap_or(self.active_tab)
            })
            .min(last);

            let collapse_to = if split.primary_tabs.is_empty() {
                Some(split.secondary_tab)
            } else if split.secondary_tabs.is_empty() {
                Some(self.active_tab)
            } else {
                None
            };
            if let Some(active) = collapse_to {
                self.active_tab = active.min(last);
                self.split = None;
            }
        } else {
            self.active_tab = replacement
                .and_then(|index| rebase_tab_index(index, removed))
                .unwrap_or(0)
                .min(last);
        }
    }

    pub(in crate::iced_ui) fn reorder_dragged_tab(
        &mut self,
        pane: PaneId,
        tab_index: usize,
        insertion_slot: usize,
    ) -> Option<usize> {
        if let Some(split) = &mut self.split {
            let tab_order = match pane {
                PaneId::Primary => &mut split.primary_tabs,
                PaneId::Secondary => &mut split.secondary_tabs,
            };
            let old_slot = tab_order.iter().position(|index| *index == tab_index)?;
            let mut new_slot = insertion_slot.min(tab_order.len());
            if new_slot > old_slot {
                new_slot -= 1;
            }
            if new_slot == old_slot {
                return Some(old_slot);
            }
            let moved = tab_order.remove(old_slot);
            tab_order.insert(new_slot, moved);
            return Some(new_slot);
        }

        if self.tabs.is_empty() || tab_index >= self.tabs.len() {
            return None;
        }
        let old_slot = tab_index;
        let mut new_slot = insertion_slot.min(self.tabs.len());
        if new_slot > old_slot {
            new_slot -= 1;
        }
        if new_slot == old_slot {
            return Some(old_slot);
        }
        let moved = self.tabs.remove(old_slot);
        self.tabs.insert(new_slot, moved);
        self.active_tab = new_slot;
        Some(new_slot)
    }

    pub(in crate::iced_ui) fn save_session(&self) {
        let split = self.split.as_ref().map(|split| SplitSession {
            tab_a: self.active_tab.min(self.tabs.len().saturating_sub(1)),
            tab_b: split.secondary_tab.min(self.tabs.len().saturating_sub(1)),
            primary_tabs: split.primary_tabs.clone(),
            secondary_tabs: split.secondary_tabs.clone(),
            focused: split.focused,
            ratio: split.ratio,
            side: SplitSide::Right,
        });
        let session = AppSession {
            tabs: self.tabs.clone(),
            active_tab: self.active_tab.min(self.tabs.len().saturating_sub(1)),
            split,
        };
        if let Err(error) = session.save() {
            crate::utils::log::error(format!("Session save failed: {error}"));
        }
    }

    pub(in crate::iced_ui) fn focus_pane(&mut self, pane: PaneId) {
        let mut changed = false;
        if let Some(split) = &mut self.split {
            let focused = match pane {
                PaneId::Primary => SplitFocus::Primary,
                PaneId::Secondary => SplitFocus::Secondary,
            };
            changed = split.focused != focused;
            split.focused = focused;
        }
        if self.config.show_preview_panel
            && self.split.is_some()
            && !self.uses_split_preview_panels()
        {
            if self.preview_panel_pane == Some(pane) {
                self.preview_panel_target_pane = None;
            } else if self.preview_panel_progress <= 0.001 {
                self.preview_panel_pane = Some(pane);
                self.preview_panel_target_pane = None;
            } else {
                self.preview_panel_target_pane = Some(pane);
            }
        }
        if changed {
            self.save_session();
        }
    }

    pub(in crate::iced_ui) fn focused_pane(&self) -> PaneId {
        match self.split.as_ref().map(|split| split.focused) {
            Some(SplitFocus::Secondary) => PaneId::Secondary,
            _ => PaneId::Primary,
        }
    }

    pub(in crate::iced_ui) fn is_split_focused_pane(&self, pane: PaneId) -> bool {
        self.split.is_some() && self.focused_pane() == pane
    }

    pub(in crate::iced_ui) fn pane(&self, pane: PaneId) -> &PaneState {
        match pane {
            PaneId::Primary => &self.primary,
            PaneId::Secondary => &self.secondary,
        }
    }

    pub(in crate::iced_ui) fn pane_mut(&mut self, pane: PaneId) -> &mut PaneState {
        match pane {
            PaneId::Primary => &mut self.primary,
            PaneId::Secondary => &mut self.secondary,
        }
    }

    pub(in crate::iced_ui) fn resolve_address_path(
        &self,
        pane: PaneId,
        value: &str,
    ) -> Result<Option<PathBuf>, String> {
        let value = value.trim().trim_matches('"');
        if value.is_empty() {
            return Err("Escribe una ruta para navegar".into());
        }
        if value.eq_ignore_ascii_case(THIS_PC_LABEL)
            || value.eq_ignore_ascii_case(self.localized("Este equipo", "This PC"))
        {
            return Ok(None);
        }
        if value.eq_ignore_ascii_case("Red") {
            return Ok(Some(explorer::network_root_path()));
        }

        let path = if value == "~" {
            paths::home_dir().unwrap_or_default()
        } else if let Some(relative) = value
            .strip_prefix("~/")
            .or_else(|| value.strip_prefix("~\\"))
        {
            paths::home_dir()
                .map(|home| home.join(relative))
                .unwrap_or_else(|| PathBuf::from(value))
        } else {
            PathBuf::from(value)
        };

        let path = if explorer::is_virtual_path(&path)
            || explorer::is_unc_path(&path)
            || path.is_absolute()
        {
            path
        } else {
            self.tab_for_pane(pane)
                .path
                .as_ref()
                .cloned()
                .unwrap_or_else(|| paths::home_dir().unwrap_or_default())
                .join(path)
        };

        if explorer::is_virtual_path(&path) || explorer::is_unc_path(&path) {
            return Ok(Some(path));
        }
        if !path.is_dir() {
            return Err(format!("No existe una carpeta en {}", path.display()));
        }

        Ok(Some(path.canonicalize().unwrap_or(path)))
    }
}
