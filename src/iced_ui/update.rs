use super::*;

impl BExplorerIced {
    pub(super) fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::NetworkDiscoveryEntries(pane, request_id, entries) => {
                // Keep the process-wide cache current even if the user has
                // already navigated away. Returning to Red can then paint the
                // partial results immediately instead of starting from empty.
                explorer::merge_network_root_cache(&entries);

                let is_current_network_root = self
                    .tab_for_pane(pane)
                    .path
                    .as_deref()
                    .is_some_and(explorer::is_network_root_path)
                    && self.pane(pane).request_id == request_id;
                if !is_current_network_root {
                    return Task::none();
                }

                let searching = self
                    .localized(
                        "Buscando dispositivos de red…",
                        "Searching network devices…",
                    )
                    .to_owned();
                let elements = self.localized("elementos", "items");
                let results = self.localized("resultados", "results");
                let completed_entries = {
                    let state = self.pane_mut(pane);
                    explorer::merge_network_entries(&mut state.entries, entries);
                    explorer::sort_entries_by_name(&mut state.entries);
                    state.network_discovery_pending =
                        state.network_discovery_pending.saturating_sub(1);
                    state.loading = state.network_discovery_pending != 0;
                    state.status = if state.loading {
                        format!("{} {results} · {searching}", state.entries.len())
                    } else {
                        format!("{} {elements}", state.entries.len())
                    };
                    state.folder_entries = None;
                    state.has_vertical_overflow = false;
                    state.mark_entries_changed();
                    (!state.loading).then(|| state.entries.clone())
                };
                if let Some(entries) = completed_entries {
                    let _ = explorer::save_network_cache(&entries);
                    self.startup.complete_pane_load(pane, request_id);
                }
                self.queue_visible_images(pane)
            }
            Message::NetworkDiscoveryAddresses(pane, request_id, addresses) => {
                let is_current_network_root = self
                    .tab_for_pane(pane)
                    .path
                    .as_deref()
                    .is_some_and(explorer::is_network_root_path)
                    && self.pane(pane).request_id == request_id;
                let elements = self.localized("elementos", "items");
                let results = self.localized("resultados", "results");
                let searching = self
                    .localized(
                        "Buscando dispositivos de red…",
                        "Searching network devices…",
                    )
                    .to_owned();
                let completed_entries = if is_current_network_root {
                    let state = self.pane_mut(pane);
                    state.network_discovery_pending = state
                        .network_discovery_pending
                        .saturating_sub(1)
                        .saturating_add(addresses.len());
                    state.loading = state.network_discovery_pending != 0;
                    state.status = if state.loading {
                        format!("{} {results} · {searching}", state.entries.len())
                    } else {
                        format!("{} {elements}", state.entries.len())
                    };
                    (!state.loading).then(|| state.entries.clone())
                } else {
                    None
                };
                if let Some(entries) = completed_entries {
                    let _ = explorer::save_network_cache(&entries);
                    self.startup.complete_pane_load(pane, request_id);
                }

                let tasks = addresses.into_iter().map(|address| {
                    Task::perform(
                        run_blocking_file_operation(move || {
                            Ok::<_, BExplorerError>(
                                explorer::network_computer_entry_netbios_address_timed(address),
                            )
                        }),
                        move |result| {
                            Message::NetworkDiscoveryEntries(
                                pane,
                                request_id,
                                result.ok().flatten().into_iter().collect::<Vec<_>>(),
                            )
                        },
                    )
                });
                Task::batch(tasks)
            }
            Message::Loaded(pane, request_id, result) => {
                let is_storage_root = self.tab_for_pane(pane).path.is_none();
                let pending_new_folder = self
                    .pending_new_folder_rename
                    .as_ref()
                    .filter(|(pending_pane, _)| *pending_pane == pane)
                    .cloned();
                let pending_reveal = self
                    .pending_reveal_in_new_tab
                    .as_ref()
                    .filter(|(pending_pane, location, _)| {
                        *pending_pane == pane
                            && self.tab_for_pane(pane).path.as_ref() == Some(location)
                    })
                    .cloned();
                if self.pane(pane).request_id != request_id {
                    return Task::none();
                }
                self.startup.complete_pane_load(pane, request_id);
                let state = self.pane_mut(pane);
                state.loading = false;
                state.search_progress_phase = 0.0;
                let storage_entries = match &result {
                    Ok(entries) if is_storage_root => Some(entries.clone()),
                    _ => None,
                };
                let load_error = result.as_ref().err().cloned();
                match result {
                    Ok(entries) => {
                        state.status = format!("{} elements", entries.len());
                        state.folder_entries = None;
                        state.entries = entries;
                        state.selected.clear();
                        state.selection_anchor = None;
                        state.render_limit = INITIAL_RENDER_LIMIT;
                        state.scroll_offset_y = 0.0;
                    }
                    Err(error) => {
                        state.status = error;
                        state.entries.clear();
                        state.folder_entries = None;
                        state.selected.clear();
                        state.selection_anchor = None;
                        state.render_limit = INITIAL_RENDER_LIMIT;
                        state.scroll_offset_y = 0.0;
                    }
                }
                state.mark_entries_changed();
                state.has_vertical_overflow = false;
                if pending_new_folder.is_some() {
                    self.pending_new_folder_rename = None;
                }
                if pending_reveal.is_some() {
                    self.pending_reveal_in_new_tab = None;
                }
                let mut tasks = vec![
                    self.queue_visible_images(pane),
                    scroll_pane_to_top_task(pane),
                ];
                if let Some(entries) = storage_entries {
                    let available_paths = entries
                        .iter()
                        .map(|entry| entry.path.clone())
                        .collect::<HashSet<_>>();
                    self.sidebar_storage_entries = entries.clone();
                    let _ = explorer::save_storage_cache(&entries);
                    for other_pane in [PaneId::Primary, PaneId::Secondary] {
                        if other_pane == pane
                            || (other_pane == PaneId::Secondary && self.split.is_none())
                        {
                            continue;
                        }
                        if self.tab_for_pane(other_pane).path.is_some() {
                            continue;
                        }
                        let state = self.pane_mut(other_pane);
                        state.loading = false;
                        state.folder_entries = None;
                        state.entries = entries.clone();
                        state.selected.retain(|path| available_paths.contains(path));
                        state.selection_anchor = None;
                        state.status = format!("{} elements", entries.len());
                        state.render_limit = INITIAL_RENDER_LIMIT;
                        state.scroll_offset_y = 0.0;
                        state.has_vertical_overflow = false;
                        state.mark_entries_changed();
                        tasks.push(self.queue_visible_images(other_pane));
                    }
                    tasks.extend(
                        entries
                            .iter()
                            .map(|entry| self.queue_sidebar_path_icon(&entry.path)),
                    );
                }
                if let Some((_, path)) = pending_new_folder
                    && let Some(index) = self
                        .pane(pane)
                        .entries
                        .iter()
                        .position(|entry| entry.path == path)
                {
                    tasks.push(self.context_begin_rename(pane, ContextTarget::Entry(index)));
                }
                if let Some((_, _, path)) = pending_reveal
                    && let Some(index) = self
                        .pane(pane)
                        .entries
                        .iter()
                        .position(|entry| entry.path == path)
                {
                    self.select_single(pane, index);
                }
                if !self.pane(pane).search_text.trim().is_empty() {
                    tasks.push(self.start_recursive_search(pane));
                }
                if let Some(error) = load_error {
                    tasks.push(
                        self.show_error_dialog(
                            self.localized(
                                "No se pudo cargar la ubicación",
                                "Could not load location",
                            )
                            .to_owned(),
                            error,
                        ),
                    );
                }
                Task::batch(tasks)
            }
            Message::SidebarStorageLoaded(result) => {
                let entries = match result {
                    Ok(entries) => entries,
                    Err(error) => {
                        self.startup.complete_storage_root_load();
                        let mut waiting_for_storage = false;
                        for pane in [PaneId::Primary, PaneId::Secondary] {
                            if pane == PaneId::Secondary && self.split.is_none() {
                                continue;
                            }
                            if self.tab_for_pane(pane).path.is_none() && self.pane(pane).loading {
                                waiting_for_storage = true;
                                let state = self.pane_mut(pane);
                                state.loading = false;
                                state.search_progress_phase = 0.0;
                                state.status = error.clone();
                            }
                        }
                        return if waiting_for_storage {
                            self.show_error_dialog(
                                self.localized(
                                    "No se pudieron cargar las unidades",
                                    "Could not load drives",
                                )
                                .to_owned(),
                                error,
                            )
                        } else {
                            Task::none()
                        };
                    }
                };
                self.startup.complete_storage_root_load();
                let paths = entries
                    .iter()
                    .map(|entry| entry.path.clone())
                    .collect::<Vec<_>>();
                self.sidebar_storage_entries = entries.clone();
                let _ = explorer::save_storage_cache(&entries);
                let available_paths = entries
                    .iter()
                    .map(|entry| entry.path.clone())
                    .collect::<HashSet<_>>();
                let mut tasks = paths
                    .iter()
                    .map(|path| self.queue_sidebar_path_icon(path))
                    .collect::<Vec<_>>();
                for pane in [PaneId::Primary, PaneId::Secondary] {
                    if pane == PaneId::Secondary && self.split.is_none() {
                        continue;
                    }
                    if self.tab_for_pane(pane).path.is_some() {
                        continue;
                    }
                    let state = self.pane_mut(pane);
                    state.loading = false;
                    state.search_progress_phase = 0.0;
                    state.status = format!("{} elements", entries.len());
                    state.folder_entries = None;
                    state.entries = entries.clone();
                    state.selected.retain(|path| available_paths.contains(path));
                    state.selection_anchor = None;
                    state.has_vertical_overflow = false;
                    state.mark_entries_changed();
                    tasks.push(self.queue_visible_images(pane));
                }
                Task::batch(tasks)
            }
            Message::StorageDevicesChanged => {
                if self.storage_refresh_scheduled {
                    return Task::none();
                }
                self.storage_refresh_scheduled = true;
                Task::perform(delay(Duration::from_millis(650)), |_| {
                    Message::RefreshStorageAfterDeviceChange
                })
            }
            Message::RefreshStorageAfterDeviceChange => {
                self.storage_refresh_scheduled = false;
                self.refresh_sidebar_storage()
            }
            Message::CloseTab(pane, slot) => {
                self.tab_drag = None;
                if self.tabs.len() == 1 && self.tab_index_at(pane, slot).is_some() {
                    return self.update(Message::WindowClose);
                }
                self.close_tab(pane, slot);
                self.sync_pane_search_from_tab(pane);
                self.save_session();
                self.start_navigation_load(pane)
            }
            Message::NewTab(pane) => self.open_path_in_new_tab(pane, paths::home_dir()),
            Message::StartTabDrag(pane, slot) => {
                self.focus_pane(pane);
                let Some(tab_index) = self.tab_index_at(pane, slot) else {
                    return Task::none();
                };
                let was_active = self.tab_index_for_pane(pane) == tab_index;
                self.set_active_tab_for_pane(pane, tab_index);
                if !was_active {
                    self.sync_pane_search_from_tab(pane);
                    self.save_session();
                }
                self.tab_drag = Some(TabDragState {
                    pane,
                    tab_index,
                    slot,
                    start_cursor_x: f32::NAN,
                    start_cursor_y: f32::NAN,
                    offset_x: 0.0,
                    dragging: false,
                    dirty: false,
                });
                if was_active {
                    Task::none()
                } else {
                    self.start_navigation_load(pane)
                }
            }
            Message::StartSidebarSectionDrag(section) => {
                let order = self.config.normalized_sidebar_order();
                let slot = order
                    .iter()
                    .position(|candidate| *candidate == section)
                    .unwrap_or(0);
                self.sidebar_section_drag = Some(SidebarSectionDragState {
                    section,
                    slot,
                    start_cursor_y: f32::NAN,
                    offset_y: 0.0,
                    dragging: false,
                    dirty: false,
                });
                Task::none()
            }
            Message::ToggleMenu => {
                if self.title_menu_open {
                    return self.request_popup_close(PendingPopupClose::FloatingMenus);
                }
                self.show_menu_open = false;
                self.show_menu_parent_hovered = false;
                self.show_menu_submenu_hovered = false;
                self.view_menu_open = None;
                self.group_menu_open = None;
                self.search_mode_menu_open = None;
                self.new_menu_open = None;
                self.context_menu = None;
                self.context_archive_submenu = false;
                self.context_extract_submenu = false;
                self.context_new_submenu = false;
                self.context_archive_parent_hovered = false;
                self.context_archive_submenu_hovered = false;
                self.context_new_parent_hovered = false;
                self.context_new_submenu_hovered = false;
                self.request_popup_backdrop(PopupBackdropTarget::TitleMenu)
            }
            Message::OpenShortcuts => {
                self.title_menu_open = false;
                self.show_menu_open = false;
                self.show_menu_parent_hovered = false;
                self.show_menu_submenu_hovered = false;
                self.request_popup_backdrop(PopupBackdropTarget::Shortcuts)
            }
            Message::CloseShortcuts => self.request_popup_close(PendingPopupClose::Shortcuts),
            Message::OpenAbout => {
                self.title_menu_open = false;
                self.show_menu_open = false;
                self.show_menu_parent_hovered = false;
                self.show_menu_submenu_hovered = false;
                self.settings_open = false;
                self.color_picker_open = false;
                self.color_picker_backdrop = None;
                self.accent_plane_dragging = false;
                self.accent_hue_dragging = false;
                self.request_popup_backdrop(PopupBackdropTarget::About)
            }
            Message::CloseAbout => self.request_popup_close(PendingPopupClose::About),
            Message::OpenRepository => match shell::open_url(env!("CARGO_PKG_REPOSITORY")) {
                Ok(()) => Task::none(),
                Err(error) => {
                    self.about_open = false;
                    self.show_error_dialog(
                        self.localized(
                            "No se pudo abrir el repositorio",
                            "Could not open repository",
                        )
                        .to_owned(),
                        error.to_string(),
                    )
                }
            },
            Message::BeginShortcutCapture(action) => {
                self.shortcut_capture = Some(action);
                Task::none()
            }
            Message::ShortcutBindingCaptured(binding) => {
                let Some(action) = self.shortcut_capture.take() else {
                    return Task::none();
                };
                self.config.shortcuts.set_binding(action, binding);
                save_config(&self.config);
                Task::none()
            }
            Message::ResetShortcut(action) => {
                let default = ShortcutConfig::default().binding(action).clone();
                self.config.shortcuts.set_binding(action, default);
                self.shortcut_capture = None;
                save_config(&self.config);
                Task::none()
            }
            Message::CloseFloatingMenus => {
                self.request_popup_close(PendingPopupClose::FloatingMenus)
            }
            Message::OpenShowMenu => {
                self.show_menu_open = true;
                Task::none()
            }
            Message::ShowMenuParentEnter => {
                self.show_menu_open = true;
                self.show_menu_parent_hovered = true;
                Task::none()
            }
            Message::ShowMenuParentExit => {
                self.show_menu_parent_hovered = false;
                Task::perform(delay(Duration::from_millis(140)), |_| {
                    Message::CloseShowMenuIfUnhovered
                })
            }
            Message::ShowMenuSubmenuEnter => {
                self.show_menu_open = true;
                self.show_menu_submenu_hovered = true;
                Task::none()
            }
            Message::ShowMenuSubmenuExit => {
                self.show_menu_submenu_hovered = false;
                Task::perform(delay(Duration::from_millis(140)), |_| {
                    Message::CloseShowMenuIfUnhovered
                })
            }
            Message::CloseShowMenuIfUnhovered => {
                if !self.show_menu_parent_hovered && !self.show_menu_submenu_hovered {
                    self.show_menu_open = false;
                }
                Task::none()
            }
            Message::ToggleActionBar => {
                self.config.show_action_bar = !self.config.show_action_bar;
                save_config(&self.config);
                Task::none()
            }
            Message::ToggleBookmarkBar => {
                self.config.show_bookmark_bar = !self.config.show_bookmark_bar;
                save_config(&self.config);
                Task::none()
            }
            Message::ToggleSplitPaneMenus => {
                self.config.show_split_pane_menus = !self.config.show_split_pane_menus;
                save_config(&self.config);
                Task::none()
            }
            Message::ToggleSplitPreviewPanels => {
                self.config.show_split_preview_panels = !self.config.show_split_preview_panels;
                self.preview_panel_target_pane = None;
                save_config(&self.config);
                if self.uses_split_preview_panels() && self.config.show_preview_panel {
                    return Task::batch([
                        self.queue_selected_preview(PaneId::Primary),
                        self.queue_selected_preview(PaneId::Secondary),
                    ]);
                }
                Task::none()
            }
            Message::ToggleSidebar => {
                self.new_menu_open = None;
                self.popup_backdrop = None;
                self.sidebar_visible = !self.sidebar_visible;
                self.last_animation_frame = None;
                self.config.sidebar_visible = self.sidebar_visible;
                save_config(&self.config);
                Task::none()
            }
            Message::SidebarPointerEntered => {
                self.sidebar_pointer_inside = true;
                Task::none()
            }
            Message::SidebarPointerExited => {
                self.sidebar_pointer_inside = false;
                Task::none()
            }
            Message::StartupBusyThresholdReached => {
                self.startup.mark_busy_threshold_reached();
                Task::none()
            }
            Message::AnimationFrame(now) => {
                self.startup.mark_first_frame_presented();
                let elapsed = self
                    .last_animation_frame
                    .replace(now)
                    .map(|previous| now.saturating_duration_since(previous))
                    .unwrap_or(Duration::from_secs_f32(1.0 / 60.0));
                let sidebar_target = if self.sidebar_visible { 1.0 } else { 0.0 };
                self.sidebar_progress =
                    advance_layout_animation(self.sidebar_progress, sidebar_target, elapsed);
                let preview_target = self.preview_panel_animation_target();
                self.preview_panel_progress =
                    advance_layout_animation(self.preview_panel_progress, preview_target, elapsed);
                let mut preview_to_queue = None;
                if self.preview_panel_progress <= 0.001 {
                    if let Some(pane) = self.preview_panel_target_pane.take() {
                        self.preview_panel_pane = Some(pane);
                        preview_to_queue = Some(pane);
                    }
                    if !self.config.show_preview_panel {
                        self.preview_panel_pane = None;
                        self.pdf_previews.clear();
                    }
                }
                self.popup_fade_progress = advance_popup_animation(
                    self.popup_fade_progress,
                    self.popup_fade_target,
                    elapsed,
                );
                self.color_picker_fade_progress = advance_popup_animation(
                    self.color_picker_fade_progress,
                    self.color_picker_fade_target,
                    elapsed,
                );
                self.file_drag_fade_progress = advance_popup_animation(
                    self.file_drag_fade_progress,
                    self.file_drag_fade_target,
                    elapsed,
                );
                if self.file_drag_fade_progress <= 0.0 && self.file_drag_fade_target <= 0.0 {
                    self.file_drag_fade_snapshot = None;
                }
                let close_finished =
                    self.pending_popup_close
                        .is_some_and(|pending| match pending {
                            PendingPopupClose::ColorPicker => {
                                self.color_picker_fade_progress <= 0.0
                            }
                            _ => self.popup_fade_progress <= 0.0,
                        });
                if close_finished {
                    self.finish_pending_popup_close();
                }
                if !self.sidebar_animation_active()
                    && !self.preview_panel_animation_active()
                    && !self.popup_fade_animation_active()
                    && !self.file_drag_fade_animation_active()
                {
                    self.last_animation_frame = None;
                }
                preview_to_queue
                    .map(|pane| self.queue_selected_preview(pane))
                    .unwrap_or_else(Task::none)
            }
            Message::ScrollbarHover(pane, axis, hovered) => {
                let state = self.pane_mut(pane);
                match axis {
                    ScrollbarAxis::Horizontal => state.scrollbar_horizontal_hovered = hovered,
                    ScrollbarAxis::Vertical => state.scrollbar_vertical_hovered = hovered,
                }
                Task::none()
            }
            Message::ScrollbarAnimationTick => {
                let now = Instant::now();
                for pane in [PaneId::Primary, PaneId::Secondary] {
                    let state = self.pane_mut(pane);
                    let wheel_reveal = state
                        .scrollbar_reveal_until
                        .is_some_and(|until| until > now);
                    if !wheel_reveal {
                        state.scrollbar_reveal_until = None;
                    }
                    let target = f32::from(
                        state.scrollbar_horizontal_hovered
                            || state.scrollbar_vertical_hovered
                            || wheel_reveal,
                    );
                    state.scrollbar_reveal_progress = if target > state.scrollbar_reveal_progress {
                        (state.scrollbar_reveal_progress + SCROLLBAR_FADE_STEP).min(target)
                    } else {
                        (state.scrollbar_reveal_progress - SCROLLBAR_FADE_STEP).max(target)
                    };
                }
                Task::none()
            }
            Message::AsyncProgressTick => {
                for pane in [PaneId::Primary, PaneId::Secondary] {
                    let state = self.pane_mut(pane);
                    if state.loading || state.mounting_disk_image || state.search_receiver.is_some()
                    {
                        state.search_progress_phase =
                            (state.progress_animation_started.elapsed().as_secs_f32() / 1.25)
                                .rem_euclid(1.0);
                    }
                }
                Task::none()
            }
            Message::ToggleSplit => {
                self.new_menu_open = None;
                self.popup_backdrop = None;
                if self.split.is_some() {
                    self.collapse_transfer_ownership_to_primary();
                    self.split = None;
                    if self.preview_panel_pane == Some(PaneId::Secondary) {
                        self.preview_panel_pane = Some(PaneId::Primary);
                    }
                    self.preview_panel_target_pane = None;
                    self.save_session();
                    return Task::none();
                }

                // A focused address input from the only pane must not retain
                // keyboard focus after we create the second pane.
                self.address_edit = None;
                self.tabs.push(TabState::with_view_mode(
                    paths::home_dir(),
                    self.config.default_view,
                ));
                let secondary = self.tabs.len() - 1;
                self.split = Some(SplitRuntime {
                    primary_tabs: (0..secondary).collect(),
                    secondary_tabs: vec![secondary],
                    secondary_tab: secondary,
                    focused: SplitFocus::Secondary,
                    ratio: 0.5,
                });
                // The new pane is the destination of the split. Set its focus
                // explicitly after the layout state exists so keyboard actions,
                // tab highlighting, and preview ownership all move with it.
                self.focus_pane(PaneId::Secondary);
                self.sync_pane_search_from_tab(PaneId::Secondary);
                self.save_session();
                self.start_navigation_load(PaneId::Secondary)
            }
            Message::Navigate(pane, path) => {
                self.new_menu_open = None;
                self.popup_backdrop = None;
                self.address_edit = None;
                self.focus_pane(pane);
                let reset_root_presentation = uses_fixed_root_presentation(path.as_deref());
                let tab_index = self.tab_index_for_pane(pane);
                if let Some(tab) = self.tabs.get_mut(tab_index) {
                    tab.navigate_to(path.clone());
                }
                if reset_root_presentation {
                    self.reset_fixed_root_presentation(pane);
                }
                let sidebar_icon_task = path
                    .as_ref()
                    .map(|path| self.queue_sidebar_path_icon(path))
                    .unwrap_or_else(Task::none);
                if let Some(path) = path {
                    self.config.remember_recent(path);
                    save_config(&self.config);
                }
                self.save_session();
                Task::batch([self.start_navigation_load(pane), sidebar_icon_task])
            }
            Message::BeginAddressEdit(pane) => {
                self.focus_pane(pane);
                let value = self
                    .tab_for_pane(pane)
                    .path
                    .as_ref()
                    .map(|path| path_label(Some(path)))
                    .unwrap_or_else(|| self.localized("Este equipo", "This PC").to_owned());
                let select_end = value.chars().count();
                self.address_edit = Some(AddressEditState { pane, value });
                focus_address_input_task(pane, select_end)
            }
            Message::AddressChanged(value) => {
                if let Some(address_edit) = &mut self.address_edit {
                    address_edit.value = value;
                }
                Task::none()
            }
            Message::SubmitAddress(pane) => {
                let Some(address_edit) = self
                    .address_edit
                    .as_ref()
                    .filter(|address_edit| address_edit.pane == pane)
                else {
                    return Task::none();
                };
                let value = address_edit.value.clone();
                match self.resolve_address_path(pane, &value) {
                    Ok(path) => self.update(Message::Navigate(pane, path)),
                    Err(error) => self.report_error(pane, error),
                }
            }
            Message::RowPressed(pane, index) => {
                if self
                    .file_drag
                    .as_ref()
                    .is_some_and(|drag| drag.source_pane == pane && drag.dragging)
                {
                    return Task::none();
                }
                if self.file_drag_suppressed_click == Some((pane, index)) {
                    self.file_drag_suppressed_click = None;
                    self.last_entry_click = None;
                    return Task::none();
                }
                self.focus_pane(pane);
                let Some(entry) = self.pane(pane).entries.get(index).cloned() else {
                    return Task::none();
                };
                if self
                    .rename_dialog
                    .as_ref()
                    .is_some_and(|dialog| dialog.pane == pane && dialog.path == entry.path)
                {
                    return Task::none();
                }
                let commit_task = self.commit_pending_rename_if_not(pane, Some(&entry.path));
                let modifiers = self.current_modifiers;
                let is_double_click = !modifiers.shift()
                    && !modifiers.command()
                    && self.last_entry_click.as_ref().is_some_and(|click| {
                        click.pane == pane
                            && click.path == entry.path
                            && click.at.elapsed() <= Duration::from_millis(450)
                    });
                if modifiers.shift() {
                    self.select_range_to(pane, index);
                } else if modifiers.command() {
                    let state = self.pane_mut(pane);
                    if !state.selected.remove(&entry.path) {
                        state.selected.insert(entry.path.clone());
                    }
                    state.selection_anchor = Some(index);
                } else {
                    self.select_single(pane, index);
                }
                self.last_entry_click = if modifiers.shift() || modifiers.command() {
                    None
                } else {
                    Some(EntryClickState {
                        pane,
                        path: entry.path.clone(),
                        at: Instant::now(),
                    })
                };
                let preview_task = self.queue_selected_preview(pane);
                if is_double_click {
                    self.last_entry_click = None;
                    return Task::batch([
                        commit_task,
                        preview_task,
                        self.context_open(pane, ContextTarget::Entry(index)),
                    ]);
                }
                Task::batch([commit_task, preview_task])
            }
            Message::Back(pane) => {
                self.address_edit = None;
                let tab_index = self.tab_index_for_pane(pane);
                if self.tabs.get_mut(tab_index).is_some_and(TabState::go_back) {
                    if self.uses_fixed_root_presentation(pane) {
                        self.reset_fixed_root_presentation(pane);
                    }
                    self.save_session();
                    self.start_navigation_load(pane)
                } else {
                    Task::none()
                }
            }
            Message::Forward(pane) => {
                self.address_edit = None;
                let tab_index = self.tab_index_for_pane(pane);
                if self
                    .tabs
                    .get_mut(tab_index)
                    .is_some_and(TabState::go_forward)
                {
                    if self.uses_fixed_root_presentation(pane) {
                        self.reset_fixed_root_presentation(pane);
                    }
                    self.save_session();
                    self.start_navigation_load(pane)
                } else {
                    Task::none()
                }
            }
            Message::Up(pane) => {
                let current = self.tab_for_pane(pane).path.clone();
                let Some(parent) = current.and_then(|path| path.parent().map(Path::to_path_buf))
                else {
                    return Task::none();
                };
                self.update(Message::Navigate(pane, Some(parent)))
            }
            Message::ToggleFavorite(pane) => {
                self.focus_pane(pane);
                let Some(path) = self.tab_for_pane(pane).path.clone() else {
                    return Task::none();
                };

                let added = if let Some(index) = self
                    .config
                    .favorites
                    .iter()
                    .position(|favorite| favorite == &path)
                {
                    self.config.favorites.remove(index);
                    false
                } else {
                    self.config.favorites.push(path.clone());
                    true
                };
                save_config(&self.config);
                if added {
                    self.queue_sidebar_path_icon(&path)
                } else {
                    Task::none()
                }
            }
            Message::Refresh(pane) => {
                let pane_is_storage_root = self.tab_for_pane(pane).path.is_none();
                if pane_is_storage_root {
                    // Loading This PC already refreshes the shared storage
                    // result and the sidebar. Starting a second enumeration
                    // here makes refresh unnecessarily slow, especially when
                    // Windows is probing portable devices.
                    self.start_load(pane)
                } else {
                    Task::batch([self.start_load(pane), self.refresh_sidebar_storage()])
                }
            }
            Message::ToggleNewMenu(pane) => {
                self.focus_pane(pane);
                if self.new_menu_open == Some(pane) {
                    return self.request_popup_close(PendingPopupClose::FloatingMenus);
                }
                self.title_menu_open = false;
                self.show_menu_open = false;
                self.view_menu_open = None;
                self.group_menu_open = None;
                self.search_mode_menu_open = None;
                self.new_menu_open = None;
                self.context_menu = None;
                self.request_popup_backdrop(PopupBackdropTarget::NewMenu(pane))
            }
            Message::NewFolder(pane) => {
                self.new_menu_open = None;
                self.popup_backdrop = None;
                let Some(path) = self.tab_for_pane(pane).path.clone() else {
                    return Task::none();
                };
                if !self.begin_file_operation(pane, "Creating folder...") {
                    return Task::none();
                }
                let name = self.localized("Nueva carpeta", "New folder").to_owned();
                Task::perform(
                    run_blocking_file_operation(move || {
                        operations::create_folder_named(&path, &name)
                    }),
                    move |result| Message::NewFolderFinished(pane, result),
                )
            }
            Message::NewFolderFinished(pane, result) => {
                self.pending_file_operations.remove(&pane);
                match result {
                    Ok(created) => {
                        self.pane_mut(pane).status = format!("Created {}", created.display());
                        self.pending_new_folder_rename = Some((pane, created));
                        self.start_load(pane)
                    }
                    Err(error) => {
                        if operations::error_message_is_permission_denied(&error)
                            && cfg!(any(target_os = "windows", target_os = "linux"))
                            && let Some(path) = self.tab_for_pane(pane).path.clone()
                        {
                            self.pane_mut(pane).status = if cfg!(target_os = "linux") {
                                "Crear la carpeta requiere permisos de root".into()
                            } else {
                                "Crear la carpeta requiere permisos de administrador".into()
                            };
                            self.elevated_file_action_dialog = Some(PendingElevatedFileAction {
                                pane,
                                action: operations::ElevatedFileAction::CreateFolder {
                                    parent: path,
                                    name: self.localized("Nueva carpeta", "New folder").into(),
                                },
                                error,
                            });
                            return Task::none();
                        }
                        self.report_error(pane, error)
                    }
                }
            }
            Message::NewTextDocument(pane) => {
                self.new_menu_open = None;
                self.popup_backdrop = None;
                let Some(path) = self.tab_for_pane(pane).path.clone() else {
                    return Task::none();
                };
                if !self.begin_file_operation(pane, "Creating text document...") {
                    return Task::none();
                }
                let name = self
                    .localized("Nuevo documento de texto.txt", "New text document.txt")
                    .to_owned();
                Task::perform(
                    run_blocking_file_operation(move || {
                        operations::create_empty_file_named(&path, &name)
                    }),
                    move |result| Message::NewTextDocumentFinished(pane, result),
                )
            }
            Message::NewTextDocumentFinished(pane, result) => {
                self.pending_file_operations.remove(&pane);
                match result {
                    Ok(created) => {
                        self.pane_mut(pane).status = format!("Created {}", created.display());
                        self.pending_new_folder_rename = Some((pane, created));
                        self.start_load(pane)
                    }
                    Err(error) => {
                        if operations::error_message_is_permission_denied(&error)
                            && cfg!(any(target_os = "windows", target_os = "linux"))
                            && let Some(path) = self.tab_for_pane(pane).path.clone()
                        {
                            self.pane_mut(pane).status = if cfg!(target_os = "linux") {
                                "Crear el archivo requiere permisos de root".into()
                            } else {
                                "Crear el archivo requiere permisos de administrador".into()
                            };
                            self.elevated_file_action_dialog = Some(PendingElevatedFileAction {
                                pane,
                                action: operations::ElevatedFileAction::CreateFile {
                                    parent: path,
                                    name: self
                                        .localized(
                                            "Nuevo documento de texto.txt",
                                            "New text document.txt",
                                        )
                                        .into(),
                                },
                                error,
                            });
                            return Task::none();
                        }
                        self.report_error(pane, error)
                    }
                }
            }
            Message::PasteIntoPane(pane) => self.context_paste(pane, ContextTarget::Background),
            Message::CopySelection(pane) => {
                self.context_copy(pane, ContextTarget::Background, false)
            }
            Message::CutSelection(pane) => self.context_copy(pane, ContextTarget::Background, true),
            Message::DeleteSelected(pane) => self.delete_selection(pane, false),
            Message::OpenArchiveDialog(pane) => self.open_archive_dialog(pane),
            Message::ArchiveNameChanged(value) => {
                if let Some(dialog) = &mut self.archive_dialog {
                    dialog.name = value;
                }
                Task::none()
            }
            Message::SetArchiveFormat(format) => {
                if let Some(dialog) = &mut self.archive_dialog {
                    dialog.format = format;
                }
                Task::none()
            }
            Message::SetArchiveCompressionMethod(method) => {
                if let Some(dialog) = &mut self.archive_dialog {
                    dialog.method = method;
                }
                Task::none()
            }
            Message::ToggleArchivePassword => {
                if let Some(dialog) = &mut self.archive_dialog {
                    dialog.use_password = !dialog.use_password;
                    if !dialog.use_password {
                        dialog.password.clear();
                        dialog.password_confirmation.clear();
                        dialog.show_password = false;
                        dialog.show_password_confirmation = false;
                    }
                }
                Task::none()
            }
            Message::ArchivePasswordChanged(value) => {
                if let Some(dialog) = &mut self.archive_dialog {
                    dialog.password = value;
                }
                Task::none()
            }
            Message::ArchivePasswordConfirmationChanged(value) => {
                if let Some(dialog) = &mut self.archive_dialog {
                    dialog.password_confirmation = value;
                }
                Task::none()
            }
            Message::ShowArchivePassword(show) => {
                if let Some(dialog) = &mut self.archive_dialog {
                    dialog.show_password = show;
                }
                Task::none()
            }
            Message::ShowArchivePasswordConfirmation(show) => {
                if let Some(dialog) = &mut self.archive_dialog {
                    dialog.show_password_confirmation = show;
                }
                Task::none()
            }
            Message::ConfirmArchiveDialog => self.confirm_archive_dialog(),
            Message::CancelArchiveDialog => {
                self.request_popup_close(PendingPopupClose::ArchiveDialog)
            }
            Message::FormatVolumeLabelChanged(value) => {
                if let Some(dialog) = &mut self.format_dialog {
                    dialog.volume_label = value;
                }
                Task::none()
            }
            Message::SetFormatFileSystem(filesystem) => {
                if let Some(dialog) = &mut self.format_dialog
                    && dialog.file_systems.iter().any(|item| item == &filesystem)
                {
                    dialog.file_system = filesystem;
                }
                Task::none()
            }
            Message::SetFormatAllocationUnitSize(size) => {
                if let Some(dialog) = &mut self.format_dialog {
                    dialog.allocation_unit_size = size;
                }
                Task::none()
            }
            Message::ToggleFormatQuick => {
                if let Some(dialog) = &mut self.format_dialog {
                    dialog.quick_format = !dialog.quick_format;
                }
                Task::none()
            }
            Message::ToggleFormatEraseConfirmation => {
                if let Some(dialog) = &mut self.format_dialog {
                    dialog.confirm_erase = !dialog.confirm_erase;
                }
                Task::none()
            }
            Message::ConfirmFormatDialog => self.confirm_format_dialog(),
            Message::CancelFormatDialog => self.cancel_format_dialog(),
            Message::DismissErrorDialog => self.request_popup_close(PendingPopupClose::ErrorDialog),
            Message::FormatFinished(pane, path, result) => match result {
                Ok(outcome) => {
                    let status = format!(
                        "{} {}",
                        self.localized("Unidad formateada:", "Drive formatted:"),
                        path.display()
                    );
                    let state = self.pane_mut(pane);
                    state.loading = false;
                    state.formatting = false;
                    state.formatting_path = None;
                    state.status = status;

                    let affected_tabs = self
                        .tabs
                        .iter()
                        .enumerate()
                        .filter_map(|(index, tab)| {
                            tab.path
                                .as_ref()
                                .is_some_and(|tab_path| tab_path.starts_with(&path))
                                .then_some(index)
                        })
                        .collect::<HashSet<_>>();
                    if !affected_tabs.is_empty() {
                        for index in &affected_tabs {
                            if let Some(tab) = self.tabs.get_mut(*index) {
                                tab.navigate_to(outcome.mount_path.clone());
                            }
                        }
                        self.save_session();
                    }

                    let mut tasks = vec![self.refresh_sidebar_storage()];
                    for candidate in [PaneId::Primary, PaneId::Secondary] {
                        if candidate == PaneId::Secondary && self.split.is_none() {
                            continue;
                        }
                        if affected_tabs.contains(&self.tab_index_for_pane(candidate)) {
                            tasks.push(self.start_navigation_load(candidate));
                        }
                    }
                    if let Some(warning) = outcome.warning {
                        tasks.push(
                            self.show_error_dialog(
                                self.localized(
                                    "Formato completado con advertencia",
                                    "Formatting completed with a warning",
                                )
                                .to_owned(),
                                warning,
                            ),
                        );
                    }
                    Task::batch(tasks)
                }
                Err(error) => {
                    let state = self.pane_mut(pane);
                    state.loading = false;
                    state.formatting = false;
                    state.formatting_path = None;
                    state.status = error.clone();
                    self.show_error_dialog(
                        self.localized("Error al formatear", "Formatting error")
                            .to_owned(),
                        format!(
                            "{}\n\n{}\n{}",
                            self.localized(
                                "No se pudo formatear la unidad:",
                                "The drive could not be formatted:",
                            ),
                            path.display(),
                            error,
                        ),
                    )
                }
            },
            Message::CancelArchive(id) => {
                self.cancel_archive(id);
                Task::none()
            }
            Message::TrashFinished(pane, paths, result) => {
                self.pending_file_operations.remove(&pane);
                let transfer_id = self
                    .active_deletes
                    .iter()
                    .find(|(_, deletion)| deletion.pane == pane && deletion.paths == paths)
                    .map(|(id, _)| *id)
                    .unwrap_or_default();
                self.active_deletes.remove(&transfer_id);
                let completion_task = match result {
                    Ok(outcome) => {
                        if !outcome.undo_records.is_empty() {
                            self.last_undo_action = Some(UndoAction::Trash {
                                pane,
                                records: outcome.undo_records,
                            });
                        }
                        self.pane_mut(pane).status = format!("Deleted {} item(s)", outcome.count);
                        self.start_load(pane)
                    }
                    Err(error) => {
                        if operations::error_message_is_permission_denied(&error)
                            && cfg!(any(target_os = "windows", target_os = "linux"))
                        {
                            self.pane_mut(pane).status = if cfg!(target_os = "linux") {
                                "Enviar a la papelera requiere permisos de root".into()
                            } else {
                                "Enviar a la papelera requiere permisos de administrador".into()
                            };
                            self.elevated_delete_dialog = Some(PendingElevatedDelete {
                                pane,
                                paths,
                                permanent: false,
                                transfer_id,
                                error,
                            });
                            Task::none()
                        } else {
                            let error_task = self.report_error(pane, error);
                            Task::batch([error_task, self.start_load(pane)])
                        }
                    }
                };
                Task::batch([completion_task, self.close_transfer_window_if_idle_task()])
            }
            Message::UndoLastAction => self.undo_last_action(),
            Message::UndoFinished(action, result) => {
                let pane = action.pane();
                match result {
                    Ok(count) => {
                        self.pane_mut(pane).status = format!("Se deshicieron {count} elemento(s)");
                        let directories = action.refresh_directories();
                        self.refresh_panes_for_directories(pane, &directories)
                    }
                    Err(error) => {
                        let error_task = self.report_error(pane, error);
                        // Keep the single undo available if no data was changed
                        // (for example, an original move destination is busy).
                        self.last_undo_action = Some(action);
                        error_task
                    }
                }
            }
            Message::VirtualArchiveExtractFinished(
                source_pane,
                target_pane,
                destination,
                result,
            ) => match result {
                Ok(count) => {
                    let status = format!("Extraídos {count} elemento(s)");
                    self.pane_mut(target_pane).status = status.clone();
                    if source_pane != target_pane {
                        self.pane_mut(source_pane).status = status;
                    }
                    self.focus_pane(target_pane);
                    self.refresh_panes_for_directories(target_pane, &[destination])
                }
                Err(error) => self.report_error(source_pane, error),
            },
            Message::SearchChanged(pane, value) => {
                self.focus_pane(pane);
                self.pane_mut(pane).search_text = value.clone();
                let tab_index = self.tab_index_for_pane(pane);
                if let Some(tab) = self.tabs.get_mut(tab_index) {
                    tab.search_text = value;
                }
                Task::batch([self.refresh_search(pane), focus_search_input_task(pane)])
            }
            Message::ToggleSearchModeMenu(pane) => {
                self.focus_pane(pane);
                if self.search_mode_menu_open == Some(pane) {
                    return self.request_popup_close(PendingPopupClose::FloatingMenus);
                }
                self.title_menu_open = false;
                self.view_menu_open = None;
                self.group_menu_open = None;
                self.new_menu_open = None;
                self.context_menu = None;
                self.request_popup_backdrop(PopupBackdropTarget::SearchModeMenu(pane))
            }
            Message::SetSearchMode(pane, mode) => {
                self.focus_pane(pane);
                self.pane_mut(pane).search_mode = mode;
                self.search_mode_menu_open = None;
                self.refresh_search(pane)
            }
            Message::PollSearches => self.poll_searches(),
            Message::PaneScrolled(pane, relative_y, absolute_y, has_vertical_overflow) => {
                let state = self.pane_mut(pane);
                state.scroll_offset_y = absolute_y.max(0.0);
                state.has_vertical_overflow = has_vertical_overflow;
                state.scrollbar_reveal_until = Some(Instant::now() + Duration::from_millis(850));
                if let Some(current) = self
                    .rubber_band
                    .as_ref()
                    .filter(|drag| drag.pane == pane)
                    .map(|drag| drag.current)
                {
                    self.update_rubber_band_selection(pane, current);
                }
                if relative_y < 0.9 {
                    return Task::none();
                }
                let total = self.filtered_entries(pane).len();
                let state = self.pane_mut(pane);
                if state.render_limit >= total {
                    return Task::none();
                }
                state.render_limit = expanded_render_limit(state.render_limit, total);
                self.queue_visible_images(pane)
            }
            Message::PaneMouseWheel(pane, delta) => {
                if self.current_modifiers.control() {
                    let vertical_delta = match delta {
                        mouse::ScrollDelta::Lines { x, y } => {
                            if y.abs() > f32::EPSILON {
                                y * 40.0
                            } else {
                                x * 40.0
                            }
                        }
                        mouse::ScrollDelta::Pixels { x, y } => {
                            if y.abs() > f32::EPSILON {
                                y
                            } else {
                                x
                            }
                        }
                    };
                    self.view_scroll_accumulator += vertical_delta;
                    if self.view_scroll_accumulator.abs() < 36.0 {
                        return Task::none();
                    }
                    let larger = self.view_scroll_accumulator > 0.0;
                    self.view_scroll_accumulator = 0.0;
                    self.focus_pane(pane);
                    let current_mode = self.effective_view_mode(pane);
                    let mode = adjacent_view_mode(current_mode, larger);
                    if mode == current_mode {
                        return Task::none();
                    }
                    self.set_view_mode_for_pane(pane, mode);
                    self.view_menu_open = None;
                    self.save_session();
                    return self.queue_visible_images(pane);
                }
                self.view_scroll_accumulator = 0.0;
                let state = self.pane_mut(pane);
                state.scrollbar_reveal_until = Some(Instant::now() + Duration::from_millis(850));
                if state.has_vertical_overflow {
                    return Task::none();
                }

                let horizontal_delta = match delta {
                    mouse::ScrollDelta::Lines { x, y } => {
                        let delta = if y.abs() > f32::EPSILON { y } else { x };
                        -delta * 60.0
                    }
                    mouse::ScrollDelta::Pixels { x, y } => {
                        let delta = if y.abs() > f32::EPSILON { y } else { x };
                        -delta
                    }
                };
                if horizontal_delta.abs() <= f32::EPSILON {
                    return Task::none();
                }

                iced::widget::operation::scroll_by(
                    pane_scroll_id(pane),
                    iced::widget::operation::AbsoluteOffset {
                        x: horizontal_delta,
                        y: 0.0,
                    },
                )
            }
            Message::ToggleViewMenu(pane) => {
                self.focus_pane(pane);
                if self.view_menu_open == Some(pane) {
                    return self.request_popup_close(PendingPopupClose::FloatingMenus);
                }
                self.title_menu_open = false;
                self.group_menu_open = None;
                self.search_mode_menu_open = None;
                self.new_menu_open = None;
                self.context_menu = None;
                self.request_popup_backdrop(PopupBackdropTarget::ViewMenu(pane))
            }
            Message::SetViewMode(pane, mode) => {
                self.focus_pane(pane);
                self.set_view_mode_for_pane(pane, mode);
                let state = self.pane_mut(pane);
                state.render_limit = INITIAL_RENDER_LIMIT;
                state.scroll_offset_y = 0.0;
                self.view_menu_open = None;
                self.save_session();
                Task::batch([
                    self.queue_visible_images(pane),
                    scroll_pane_to_top_task(pane),
                ])
            }
            Message::ToggleGroupMenu(pane) => {
                self.focus_pane(pane);
                if self.group_menu_open == Some(pane) {
                    return self.request_popup_close(PendingPopupClose::FloatingMenus);
                }
                self.title_menu_open = false;
                self.view_menu_open = None;
                self.search_mode_menu_open = None;
                self.new_menu_open = None;
                self.context_menu = None;
                self.request_popup_backdrop(PopupBackdropTarget::GroupMenu(pane))
            }
            Message::SetGroupMode(pane, mode) => {
                self.focus_pane(pane);
                let fixed_root = self.uses_fixed_root_presentation(pane);
                if fixed_root {
                    self.pane_mut(pane).fixed_root_group_override = Some(mode);
                } else {
                    self.pane_mut(pane).group_mode = mode;
                    let tab_index = self.tab_index_for_pane(pane);
                    if let Some(tab) = self.tabs.get_mut(tab_index) {
                        tab.group_mode = mode;
                    }
                }
                let state = self.pane_mut(pane);
                state.render_limit = INITIAL_RENDER_LIMIT;
                state.scroll_offset_y = 0.0;
                self.group_menu_open = None;
                self.save_session();
                Task::batch([
                    self.queue_visible_images(pane),
                    scroll_pane_to_top_task(pane),
                ])
            }
            Message::SetGroupAscending(pane, ascending) => {
                self.focus_pane(pane);
                let fixed_root = self.uses_fixed_root_presentation(pane);
                if fixed_root {
                    self.pane_mut(pane).fixed_root_group_ascending_override = Some(ascending);
                } else {
                    self.pane_mut(pane).group_ascending = ascending;
                    let tab_index = self.tab_index_for_pane(pane);
                    if let Some(tab) = self.tabs.get_mut(tab_index) {
                        tab.group_ascending = ascending;
                    }
                }
                let state = self.pane_mut(pane);
                state.render_limit = INITIAL_RENDER_LIMIT;
                state.scroll_offset_y = 0.0;
                self.group_menu_open = None;
                self.save_session();
                Task::batch([
                    self.queue_visible_images(pane),
                    scroll_pane_to_top_task(pane),
                ])
            }
            Message::SortColumn(pane, column) => {
                self.focus_pane(pane);
                let state = self.pane_mut(pane);
                if state.sort_column == column {
                    state.sort_ascending = !state.sort_ascending;
                } else {
                    state.sort_column = column;
                    state.sort_ascending = true;
                }
                state.render_limit = INITIAL_RENDER_LIMIT;
                state.scroll_offset_y = 0.0;
                Task::batch([
                    self.queue_visible_images(pane),
                    scroll_pane_to_top_task(pane),
                ])
            }
            Message::ImageLoaded(result) => {
                let state = result
                    .image
                    .map(|image| {
                        IcedImageState::Ready(iced_image::Handle::from_rgba(
                            image.width,
                            image.height,
                            image.rgba,
                        ))
                    })
                    .unwrap_or(IcedImageState::Missing);

                match result.key {
                    IcedImageKey::Thumbnail(path) => {
                        self.thumbnail_cache.insert(path, state);
                    }
                    IcedImageKey::Preview(path) => {
                        self.preview_cache.insert(path, state);
                    }
                    IcedImageKey::NativeIcon(path) => {
                        self.native_icon_cache.insert(path, state);
                    }
                }
                Task::none()
            }
            Message::PdfPreviewPageLoaded(result) => {
                let mut first_page_handle = None;
                let mut next_page = None;

                if let Some(state) = self
                    .pdf_previews
                    .get_mut(&result.pane)
                    .filter(|state| state.path == result.path)
                {
                    state.loading = false;
                    if let Some(page_count) = result.page_count {
                        state.page_count = Some(page_count);
                    }

                    if let Some(image) = result.image {
                        let aspect_ratio = image.width as f32 / image.height.max(1) as f32;
                        let handle =
                            iced_image::Handle::from_rgba(image.width, image.height, image.rgba);
                        if result.page_index == 0 {
                            first_page_handle = Some(handle.clone());
                        }
                        if !state
                            .pages
                            .iter()
                            .any(|page| page.index == result.page_index)
                        {
                            state.pages.push(PdfPreviewPage {
                                index: result.page_index,
                                handle,
                                aspect_ratio,
                            });
                        }
                    }

                    if let Some(page_count) = state.page_count {
                        let candidate = result.page_index.saturating_add(1);
                        if candidate < page_count {
                            state.loading = true;
                            next_page = Some(candidate);
                        }
                    }
                }

                if let Some(handle) = first_page_handle {
                    self.preview_cache
                        .insert(result.path.clone(), IcedImageState::Ready(handle));
                }
                next_page.map_or_else(Task::none, |page_index| {
                    load_pdf_preview_page_task(result.pane, result.path, page_index)
                })
            }
            Message::PdfPreviewScrolled(pane, path, scroll_y) => {
                let page_width = self.config.preview_panel_width.clamp(220.0, 560.0);
                if let Some(state) = self
                    .pdf_previews
                    .get_mut(&pane)
                    .filter(|state| state.path == path)
                {
                    let mut offset_y = 0.0;
                    let mut current_page = state.current_page;
                    for page in &state.pages {
                        let page_height = pdf_preview_page_height(page_width, page.aspect_ratio);
                        if scroll_y < offset_y + page_height {
                            current_page = page.index;
                            break;
                        }
                        offset_y += page_height + 14.0;
                        current_page = page.index;
                    }
                    state.current_page = current_page;
                }
                Task::none()
            }
            Message::TextPreviewAction(pane, path, action) => {
                let Some(preview) = self
                    .pane_mut(pane)
                    .text_preview
                    .as_mut()
                    .filter(|preview| preview.path == path)
                else {
                    return Task::none();
                };
                // Selection, cursor movement and scrolling are state changes
                // needed by the editor. Ignore only mutations, making this a
                // true read-only preview while preserving Ctrl+C.
                if !action.is_edit() {
                    preview.content.perform(action);
                }
                Task::none()
            }
            Message::PanePointerMoved(pane, point) => {
                self.pane_pointer = Some((pane, point));
                self.update_file_drag_pane_position(pane, point);
                if self
                    .rubber_band
                    .as_ref()
                    .is_some_and(|drag| drag.pane == pane)
                {
                    self.update_rubber_band_selection(pane, point);
                }
                Task::none()
            }
            Message::PanePointerExited(pane) => {
                if self
                    .pane_pointer
                    .is_some_and(|(pointer_pane, _)| pointer_pane == pane)
                {
                    self.pane_pointer = None;
                }
                Task::none()
            }
            Message::StartRubberBand(pane) => {
                let commit_task = self.commit_pending_rename_if_not(pane, None);
                self.start_rubber_band_selection(pane);
                commit_task
            }
            Message::StartFileDrag(pane, index) => self.start_file_drag(pane, index),
            Message::OpenEntry(pane, index) => {
                let path = self
                    .pane(pane)
                    .entries
                    .get(index)
                    .map(|entry| entry.path.clone());
                let Some(path) = path else {
                    return Task::none();
                };
                if let Some(drag) = self.file_drag.take() {
                    self.fade_out_file_drag_overlay(&drag);
                }
                self.file_drag_suppressed_click = None;
                self.last_entry_click = None;
                self.focus_pane(pane);
                let commit_task = self.commit_pending_rename_if_not(pane, Some(&path));
                Task::batch([
                    commit_task,
                    self.context_open(pane, ContextTarget::Entry(index)),
                ])
            }
            Message::FileDragTargetEnter(pane, index) => {
                self.set_file_drag_target(pane, index);
                Task::none()
            }
            Message::FileDragTargetExit(pane, index) => {
                if self
                    .file_drag
                    .as_ref()
                    .is_some_and(|drag| drag.drop_target == Some((pane, index)))
                {
                    self.file_drag.as_mut().expect("checked above").drop_target = None;
                    self.refresh_file_drag_fade_snapshot();
                }
                Task::none()
            }
            Message::FileDragSidebarTargetEnter(pane, path) => {
                self.set_file_drag_sidebar_target(pane, path);
                Task::none()
            }
            Message::FileDragSidebarTargetExit(path) => {
                if self.file_drag.as_ref().is_some_and(|drag| {
                    drag.sidebar_destination
                        .as_ref()
                        .is_some_and(|(_, destination)| destination == &path)
                }) {
                    self.file_drag
                        .as_mut()
                        .expect("checked above")
                        .sidebar_destination = None;
                    self.refresh_file_drag_fade_snapshot();
                }
                Task::none()
            }
            Message::OpenBackgroundContext(pane) => {
                let commit_task = self.commit_pending_rename_if_not(pane, None);
                Task::batch([
                    commit_task,
                    self.request_context_menu(pane, ContextTarget::Background),
                ])
            }
            Message::OpenEntryContext(pane, index) => {
                let path = self
                    .pane(pane)
                    .entries
                    .get(index)
                    .map(|entry| entry.path.clone());
                let commit_task = self.commit_pending_rename_if_not(pane, path.as_deref());
                let should_select = path
                    .as_ref()
                    .is_some_and(|path| !self.pane(pane).selected.contains(path));
                if should_select {
                    self.select_single(pane, index);
                }
                let menu_task = self.request_context_menu(pane, ContextTarget::Entry(index));
                if should_select {
                    Task::batch([commit_task, self.queue_selected_preview(pane), menu_task])
                } else {
                    Task::batch([commit_task, menu_task])
                }
            }
            Message::OpenSidebarDriveContext(pane, index) => {
                let Some(entry) = self.sidebar_storage_entries.get(index) else {
                    return Task::none();
                };
                if !entry.drive_kind.is_some_and(DriveKind::is_ejectable) {
                    return Task::none();
                }
                self.request_context_menu(pane, ContextTarget::SidebarDrive(index))
            }
            Message::ContextPasteAvailabilityResolved(menu, paste_available) => {
                if menu.request_id != self.context_menu_request_id {
                    return Task::none();
                }
                self.capture_context_menu_backdrop(ContextMenuState {
                    paste_available,
                    ..menu
                })
            }
            Message::ContextBackdropCaptured(menu, screenshot) => {
                if menu.request_id != self.context_menu_request_id {
                    return Task::none();
                }
                let screenshot_for_submenus = screenshot.clone();
                let mut menu_for_callback = menu.clone();
                Task::perform(
                    {
                        let backdrop_height = self.context_menu_height(&menu);
                        async move {
                            run_blocking_file_operation(move || {
                                Ok(blurred_screenshot_region(
                                    screenshot,
                                    Rectangle::new(
                                        menu.backdrop_origin,
                                        Size::new(258.0, backdrop_height),
                                    ),
                                ))
                            })
                            .await
                            .ok()
                            .flatten()
                        }
                    },
                    move |backdrop| {
                        menu_for_callback.source_screenshot = Some(screenshot_for_submenus);
                        Message::ContextBackdropPrepared(menu_for_callback, backdrop)
                    },
                )
            }
            Message::ContextBackdropPrepared(mut menu, backdrop) => {
                if menu.request_id != self.context_menu_request_id {
                    return Task::none();
                }
                menu.backdrop = backdrop;
                self.popup_fade_progress = 0.0;
                self.context_menu = Some(menu);
                Task::none()
            }
            Message::ContextSubmenuBackdropCaptured(request_id, kind, screenshot) => {
                let Some(menu) = self
                    .context_menu
                    .as_ref()
                    .filter(|menu| menu.request_id == request_id)
                    .cloned()
                else {
                    return Task::none();
                };
                let (origin, size) = self.context_submenu_geometry(&menu, kind);
                Task::perform(
                    async move {
                        run_blocking_file_operation(move || {
                            Ok(blurred_screenshot_region(
                                screenshot,
                                Rectangle::new(origin, size),
                            ))
                        })
                        .await
                        .ok()
                        .flatten()
                    },
                    move |backdrop| {
                        Message::ContextSubmenuBackdropPrepared(request_id, kind, backdrop)
                    },
                )
            }
            Message::ContextSubmenuBackdropPrepared(request_id, kind, backdrop) => {
                if let Some(menu) = self
                    .context_menu
                    .as_mut()
                    .filter(|menu| menu.request_id == request_id)
                {
                    menu.submenu_backdrop = backdrop;
                    menu.submenu_backdrop_kind = Some(kind);
                }
                Task::none()
            }
            Message::PopupBackdropCaptured(target, screenshot) => {
                if let Some(region) = self.pane_popup_backdrop_region(
                    &target,
                    screenshot.size,
                    screenshot.scale_factor,
                ) {
                    return Task::perform(
                        async move {
                            run_blocking_file_operation(move || {
                                Ok(blurred_screenshot_region(screenshot, region))
                            })
                            .await
                            .ok()
                            .flatten()
                        },
                        move |backdrop| Message::PopupBackdropPrepared(target, backdrop),
                    );
                }
                if matches!(target, PopupBackdropTarget::TitleMenu) {
                    let menu_screenshot = screenshot.clone();
                    return Task::perform(
                        async move {
                            run_blocking_file_operation(move || {
                                let menu = blurred_screenshot_region(
                                    menu_screenshot,
                                    Rectangle::new(
                                        Point::new(0.0, TITLE_HEIGHT),
                                        Size::new(220.0, 116.0),
                                    ),
                                );
                                let submenu = blurred_screenshot_region(
                                    screenshot,
                                    Rectangle::new(
                                        Point::new(218.0, TITLE_HEIGHT + 41.0),
                                        Size::new(286.0, 151.0),
                                    ),
                                );
                                Ok::<_, BExplorerError>((menu, submenu))
                            })
                            .await
                            .ok()
                            .unwrap_or((None, None))
                        },
                        |(menu, submenu)| Message::TitleMenuBackdropsPrepared(menu, submenu),
                    );
                }
                let region_target = target.clone();
                Task::perform(
                    async move {
                        let region = popup_backdrop_region_for_screenshot(
                            &region_target,
                            screenshot.size,
                            screenshot.scale_factor,
                        );
                        run_blocking_file_operation(move || {
                            Ok(blurred_screenshot_region(screenshot, region))
                        })
                        .await
                        .ok()
                        .flatten()
                    },
                    move |backdrop| Message::PopupBackdropPrepared(target, backdrop),
                )
            }
            Message::PopupBackdropPrepared(target, backdrop) => {
                if matches!(target, PopupBackdropTarget::ColorPicker) {
                    self.color_picker_backdrop = backdrop;
                    self.color_picker_fade_progress = 0.0;
                } else {
                    self.popup_backdrop = backdrop;
                    self.popup_fade_progress = 0.0;
                }
                self.show_popup_with_backdrop(target)
            }
            Message::TitleMenuBackdropsPrepared(menu, submenu) => {
                self.popup_backdrop = menu;
                self.title_submenu_backdrop = submenu;
                self.popup_fade_progress = 0.0;
                self.show_popup_with_backdrop(PopupBackdropTarget::TitleMenu)
            }
            Message::CloseContextMenu => {
                self.dismiss_context_menu();
                Task::none()
            }
            Message::ContextArchiveParentEnter => {
                self.context_archive_submenu = true;
                self.context_open_with_submenu = false;
                self.context_extract_submenu = false;
                self.context_archive_parent_hovered = true;
                self.context_new_submenu = false;
                self.request_context_submenu_backdrop(ContextSubmenuKind::Archive)
            }
            Message::ContextExtractParentEnter => {
                self.context_archive_submenu = true;
                self.context_open_with_submenu = false;
                self.context_extract_submenu = true;
                self.context_archive_parent_hovered = true;
                self.context_new_submenu = false;
                self.request_context_submenu_backdrop(ContextSubmenuKind::Extract)
            }
            Message::ContextNewParentEnter => {
                self.context_new_submenu = true;
                self.context_open_with_submenu = false;
                self.context_new_parent_hovered = true;
                self.context_archive_submenu = false;
                self.context_extract_submenu = false;
                self.request_context_submenu_backdrop(ContextSubmenuKind::New)
            }
            Message::ContextOpenWithParentEnter => {
                let Some(menu) = self.context_menu.as_ref() else {
                    return Task::none();
                };
                let pane = menu.pane;
                let target = menu.target;
                self.context_open_with_submenu = true;
                self.context_open_with_parent_hovered = true;
                self.context_archive_submenu = false;
                self.context_extract_submenu = false;
                self.context_new_submenu = false;
                let backdrop = self.request_context_submenu_backdrop(ContextSubmenuKind::OpenWith);
                let icons = self.queue_open_with_application_icons(pane, target);
                Task::batch([backdrop, icons])
            }
            Message::ContextOpenWithParentExit => {
                self.context_open_with_parent_hovered = false;
                Task::perform(delay(Duration::from_millis(140)), |_| {
                    Message::CloseContextOpenWithSubmenuIfUnhovered
                })
            }
            Message::ContextOpenWithSubmenuEnter => {
                self.context_open_with_submenu_hovered = true;
                Task::none()
            }
            Message::ContextOpenWithSubmenuExit => {
                self.context_open_with_submenu_hovered = false;
                Task::perform(delay(Duration::from_millis(140)), |_| {
                    Message::CloseContextOpenWithSubmenuIfUnhovered
                })
            }
            Message::CloseContextOpenWithSubmenuIfUnhovered => {
                if !self.context_open_with_parent_hovered && !self.context_open_with_submenu_hovered
                {
                    self.context_open_with_submenu = false;
                }
                Task::none()
            }
            Message::ContextArchiveParentExit => {
                self.context_archive_parent_hovered = false;
                Task::perform(delay(Duration::from_millis(140)), |_| {
                    Message::CloseContextArchiveSubmenuIfUnhovered
                })
            }
            Message::ContextNewParentExit => {
                self.context_new_parent_hovered = false;
                Task::perform(delay(Duration::from_millis(140)), |_| {
                    Message::CloseContextNewSubmenuIfUnhovered
                })
            }
            Message::ContextArchiveSubmenuEnter => {
                self.context_archive_submenu = true;
                self.context_archive_submenu_hovered = true;
                Task::none()
            }
            Message::ContextArchiveSubmenuExit => {
                self.context_archive_submenu_hovered = false;
                Task::perform(delay(Duration::from_millis(140)), |_| {
                    Message::CloseContextArchiveSubmenuIfUnhovered
                })
            }
            Message::ContextNewSubmenuEnter => {
                self.context_new_submenu = true;
                self.context_new_submenu_hovered = true;
                Task::none()
            }
            Message::ContextNewSubmenuExit => {
                self.context_new_submenu_hovered = false;
                Task::perform(delay(Duration::from_millis(140)), |_| {
                    Message::CloseContextNewSubmenuIfUnhovered
                })
            }
            Message::CloseContextArchiveSubmenuIfUnhovered => {
                if !self.context_archive_parent_hovered && !self.context_archive_submenu_hovered {
                    self.context_archive_submenu = false;
                    self.context_extract_submenu = false;
                }
                Task::none()
            }
            Message::CloseContextNewSubmenuIfUnhovered => {
                if !self.context_new_parent_hovered && !self.context_new_submenu_hovered {
                    self.context_new_submenu = false;
                }
                Task::none()
            }
            Message::RunContextCommand(command) => self.run_context_command(command),
            Message::KeyboardModifiersChanged(modifiers) => {
                self.current_modifiers = modifiers;
                Task::none()
            }
            Message::KeyPressed(key, physical_key, modifiers) => {
                self.current_modifiers = modifiers;
                let is_enter = matches!(
                    key.as_ref(),
                    keyboard::Key::Named(keyboard::key::Named::Enter)
                );
                let is_escape = matches!(
                    key.as_ref(),
                    keyboard::Key::Named(keyboard::key::Named::Escape)
                );
                if is_enter {
                    if self.elevated_transfer_dialog.is_some() {
                        return self.update(Message::ConfirmElevatedTransfer);
                    }
                    if self.elevated_delete_dialog.is_some() {
                        return self.update(Message::ConfirmElevatedDelete);
                    }
                    if self.elevated_file_action_dialog.is_some() {
                        return self.update(Message::ConfirmElevatedFileAction);
                    }
                    if self.format_dialog.is_some() {
                        return self.update(Message::ConfirmFormatDialog);
                    }
                    if self.error_dialog.is_some() {
                        return self.update(Message::DismissErrorDialog);
                    }
                }
                if is_escape {
                    if self.elevated_transfer_dialog.is_some() {
                        return self.update(Message::CancelElevatedTransfer);
                    }
                    if self.elevated_delete_dialog.is_some() {
                        return self.update(Message::CancelElevatedDelete);
                    }
                    if self.elevated_file_action_dialog.is_some() {
                        return self.update(Message::CancelElevatedFileAction);
                    }
                    if self.format_dialog.is_some() {
                        return self.update(Message::CancelFormatDialog);
                    }
                    if self.error_dialog.is_some() {
                        return self.update(Message::DismissErrorDialog);
                    }
                }
                if self.rename_dialog.is_some() && is_escape {
                    return self.update(Message::CancelRename);
                }
                if self.shortcuts_open
                    && matches!(
                        key.as_ref(),
                        keyboard::Key::Named(keyboard::key::Named::Escape)
                    )
                {
                    return self.update(Message::CloseShortcuts);
                }
                if self.about_open && is_escape {
                    return self.update(Message::CloseAbout);
                }
                if self.shortcut_capture.is_some() {
                    return shortcut_binding_from_key(&key, physical_key, modifiers)
                        .map(|binding| self.update(Message::ShortcutBindingCaptured(binding)))
                        .unwrap_or_else(Task::none);
                }
                if let Some(shortcut) = keyboard_shortcut_from_key(
                    &key,
                    physical_key,
                    modifiers,
                    &self.config.shortcuts,
                ) {
                    return self.handle_keyboard_shortcut(shortcut);
                }

                if !modifiers.command()
                    && !modifiers.alt()
                    && !modifiers.logo()
                    && let keyboard::Key::Character(character) = key.as_ref()
                {
                    return self.select_entry_starting_with(self.focused_pane(), character);
                }

                Task::none()
            }
            Message::RenameChanged(value) => {
                if let Some(dialog) = &mut self.rename_dialog {
                    dialog.value = value.clone();
                    dialog.editor = text_editor::Content::with_text(&value);
                }
                Task::none()
            }
            Message::RenameEdited(action) => {
                if let Some(dialog) = &mut self.rename_dialog {
                    dialog.editor.perform(action);
                    dialog.value = dialog.editor.text();
                }
                Task::none()
            }
            Message::RenameSelected(pane) => self.rename_selected(pane),
            Message::ConfirmRename => self.commit_pending_rename(),
            Message::RenameFinished(mut dialog, result) => {
                self.pending_file_operations.remove(&dialog.pane);
                match result {
                    Ok(target) => {
                        let state = self.pane_mut(dialog.pane);
                        if state.selected.remove(&dialog.path) {
                            state.selected.insert(target.clone());
                        }
                        state.status = format!("Renamed to {}", target.display());
                        self.start_load(dialog.pane)
                    }
                    Err(error) => {
                        if operations::error_message_is_permission_denied(&error)
                            && cfg!(any(target_os = "windows", target_os = "linux"))
                        {
                            self.pane_mut(dialog.pane).status = if cfg!(target_os = "linux") {
                                "Renombrar requiere permisos de root".into()
                            } else {
                                "Renombrar requiere permisos de administrador".into()
                            };
                            self.elevated_file_action_dialog = Some(PendingElevatedFileAction {
                                pane: dialog.pane,
                                action: operations::ElevatedFileAction::Rename {
                                    path: dialog.path.clone(),
                                    name: rename_target_name(
                                        &dialog.value,
                                        dialog.extension.as_deref(),
                                    ),
                                },
                                error,
                            });
                            return Task::none();
                        }
                        dialog.editor.perform(text_editor::Action::SelectAll);
                        self.rename_dialog = Some(dialog.clone());
                        Task::batch([
                            self.report_error(dialog.pane, error),
                            focus_inline_rename_task(dialog.select_end),
                        ])
                    }
                }
            }
            Message::CancelRename => {
                self.rename_dialog = None;
                self.popup_backdrop = None;
                Task::none()
            }
            Message::ConfirmPermanentDelete => self.confirm_permanent_delete(),
            Message::PermanentDeleteFinished(pane, paths, result) => {
                self.pending_file_operations.remove(&pane);
                let transfer_id = self
                    .active_deletes
                    .iter()
                    .find(|(_, deletion)| deletion.pane == pane && deletion.paths == paths)
                    .map(|(id, _)| *id)
                    .unwrap_or_default();
                self.active_deletes.remove(&transfer_id);
                let completion_task = match result {
                    Ok(count) => {
                        self.pane_mut(pane).status = format!("Deleted {count} item(s)");
                        self.start_load(pane)
                    }
                    Err(error) => {
                        if operations::error_message_is_permission_denied(&error)
                            && cfg!(any(target_os = "windows", target_os = "linux"))
                        {
                            self.pane_mut(pane).status = if cfg!(target_os = "linux") {
                                "Eliminar permanentemente requiere permisos de root".into()
                            } else {
                                "Eliminar permanentemente requiere permisos de administrador".into()
                            };
                            self.elevated_delete_dialog = Some(PendingElevatedDelete {
                                pane,
                                paths,
                                permanent: true,
                                transfer_id,
                                error,
                            });
                            Task::none()
                        } else {
                            Task::batch([self.report_error(pane, error), self.start_load(pane)])
                        }
                    }
                };
                Task::batch([completion_task, self.close_transfer_window_if_idle_task()])
            }
            Message::CancelPermanentDelete => {
                self.request_popup_close(PendingPopupClose::PermanentDelete)
            }
            Message::DiskImageMounted(pane, source, result) => {
                self.mounting_disk_images.remove(&source);
                self.pane_mut(pane).mounting_disk_image = false;
                match result {
                    Ok(root) => {
                        self.pane_mut(pane).status =
                            format!("Imagen montada en {}", root.display());
                        Task::batch([
                            self.open_path_in_new_tab(pane, Some(root)),
                            self.refresh_sidebar_storage(),
                        ])
                    }
                    Err(error) => self.report_error(
                        pane,
                        format!("No se pudo montar {}: {error}", source.display()),
                    ),
                }
            }
            Message::DriveEjected(pane, path, result) => match result {
                Ok(()) => {
                    self.pane_mut(pane).status = "Unidad expulsada".into();
                    let pane_task = if self
                        .tab_for_pane(pane)
                        .path
                        .as_ref()
                        .is_some_and(|current| current.starts_with(&path))
                    {
                        self.update(Message::Navigate(pane, None))
                    } else {
                        self.start_load(pane)
                    };
                    Task::batch([pane_task, self.refresh_sidebar_storage()])
                }
                Err(error) => self.report_error(pane, error),
            },
            Message::CancelDefenderScan => {
                self.cancel_defender_scan();
                Task::none()
            }
            Message::CloseDefenderPanel => self.close_defender_panel(),
            Message::RemediateDefenderThreats => self.remediate_defender_threats_task(),
            #[cfg(target_os = "windows")]
            Message::DefenderThreatRemediationFinished(result) => {
                self.defender_threat_remediation_pending = false;
                let (message, failed) = match result {
                    Ok(count) => {
                        if let Some(summary) = self.defender_summary.as_mut() {
                            for threat in &mut summary.threats {
                                threat.status = "Remediated".into();
                            }
                        }
                        (
                            if self.is_spanish() {
                                format!("Defender procesó {count} amenaza(s).")
                            } else {
                                format!("Defender processed {count} threat(s).")
                            },
                            false,
                        )
                    }
                    Err(error) => (error, true),
                };
                self.pane_mut(self.focused_pane()).status = message.clone();
                self.defender_threat_remediation_message = Some((message, failed));
                Task::none()
            }
            Message::OpenWindowsSecurity => {
                let pane = self.focused_pane();
                match shell::open_windows_security() {
                    Ok(()) => {
                        self.pane_mut(pane).status = "Seguridad de Windows abierta".into();
                        Task::none()
                    }
                    Err(error) => self.report_error(pane, error.to_string()),
                }
            }
            Message::PortableClipboardPrepared(pane, result) => {
                match result {
                    Ok(paths) => {
                        self.file_clipboard = Some(FileClipboardState {
                            paths: paths.clone(),
                            cut: false,
                        });
                        let _ = shell::copy_files(&paths, false);
                        self.pane_mut(pane).status =
                            format!("{} elemento(s) MTP preparados", paths.len());
                    }
                    Err(error) => return self.report_error(pane, error),
                }
                Task::none()
            }
            Message::PortableOpenPrepared(pane, result) => {
                match result {
                    Ok(path) => match operations::open_path(&path) {
                        Ok(()) => self.pane_mut(pane).status = "Archivo MTP abierto".into(),
                        Err(error) => return self.report_error(pane, error.to_string()),
                    },
                    Err(error) => return self.report_error(pane, error),
                }
                Task::none()
            }
            Message::PortableDeleteFinished(pane, result) => match result {
                Ok(count) => {
                    self.pane_mut(pane).status = format!("{count} elemento(s) MTP eliminados");
                    self.start_load(pane)
                }
                Err(error) => self.report_error(pane, error),
            },
            Message::PortableTransferFinished(
                pane,
                refresh_directories,
                clear_clipboard,
                result,
            ) => match result {
                Ok(count) => {
                    if clear_clipboard {
                        self.file_clipboard = None;
                    }
                    self.pane_mut(pane).status =
                        format!("Transferencia MTP completada: {count} archivo(s)");
                    self.refresh_panes_for_directories(pane, &refresh_directories)
                }
                Err(error) => self.report_error(pane, error),
            },
            Message::ResolveTransferConflict(policy) => self.resolve_transfer_conflict(policy),
            Message::CancelTransferConflict => {
                self.request_popup_close(PendingPopupClose::TransferConflict)
            }
            Message::ConfirmElevatedTransfer => {
                let Some(pending) = self.elevated_transfer_dialog.take() else {
                    return Task::none();
                };
                let pane = pending.pane;
                let job = pending.job;
                let worker_job = job.clone();
                let mut progress = TransferProgress::pending(&job);
                progress.state = TransferState::Copying;
                progress.current_name = if cfg!(target_os = "linux") {
                    "Esperando permisos de root…".into()
                } else {
                    "Esperando permisos de administrador…".into()
                };
                self.transfer_progress.insert(job.id, progress);
                self.pane_mut(pane).status = "Esperando autorización del sistema…".into();
                let elevated_task = Task::perform(
                    run_blocking_file_operation(move || {
                        transfer_queue::run_elevated_transfer(&worker_job)
                    }),
                    move |result| Message::ElevatedTransferFinished(pane, job, result),
                );
                Task::batch([self.ensure_transfer_window_task(), elevated_task])
            }
            Message::CancelElevatedTransfer => {
                if let Some(pending) = self.elevated_transfer_dialog.take() {
                    self.pane_mut(pending.pane).status = pending.error;
                }
                Task::none()
            }
            Message::ElevatedTransferFinished(pane, job, result) => match result {
                Ok(result) => {
                    if let Some(mut progress) = self.transfer_progress.remove(&job.id) {
                        progress.state = TransferState::Finished;
                        progress.files_done = result.completed_files;
                        self.transfer_history.push_back(TransferHistoryState {
                            progress,
                            finished_at: Instant::now(),
                        });
                    }
                    if job.conflict_policy == ConflictPolicy::KeepBoth
                        && !result.completed_roots.is_empty()
                    {
                        self.last_undo_action = Some(match job.kind {
                            TransferKind::Copy => UndoAction::Copy {
                                pane,
                                targets: result
                                    .completed_roots
                                    .iter()
                                    .map(|item| item.target.clone())
                                    .collect(),
                            },
                            TransferKind::Move => UndoAction::Move {
                                pane,
                                items: result.completed_roots.clone(),
                            },
                        });
                    }
                    self.pane_mut(pane).status = match job.kind {
                        TransferKind::Copy => {
                            format!(
                                "Copiados {} elemento(s) con permisos elevados",
                                result.completed_files
                            )
                        }
                        TransferKind::Move => {
                            format!(
                                "Movidos {} elemento(s) con permisos elevados",
                                result.completed_files
                            )
                        }
                    };
                    self.refresh_panes_for_directories(
                        pane,
                        &crate::iced_ui::file_actions::transfer_refresh_directories(&job),
                    )
                }
                Err(error) => {
                    if let Some(mut progress) = self.transfer_progress.remove(&job.id) {
                        progress.state = TransferState::Failed;
                        self.transfer_history.push_back(TransferHistoryState {
                            progress,
                            finished_at: Instant::now(),
                        });
                    }
                    let error_task = self.report_error(pane, error);
                    Task::batch([
                        error_task,
                        self.refresh_panes_for_directories(
                            pane,
                            &crate::iced_ui::file_actions::transfer_refresh_directories(&job),
                        ),
                    ])
                }
            },
            Message::ConfirmElevatedDelete => {
                let Some(pending) = self.elevated_delete_dialog.take() else {
                    return Task::none();
                };
                let pane = pending.pane;
                let permanent = pending.permanent;
                let transfer_id = pending.transfer_id;
                self.active_deletes.insert(
                    transfer_id,
                    ActiveDeleteState {
                        id: transfer_id,
                        pane,
                        paths: pending.paths.clone(),
                        permanent,
                    },
                );
                let kind = if permanent {
                    operations::ElevatedDeleteKind::Permanent
                } else {
                    operations::ElevatedDeleteKind::Trash
                };
                self.pane_mut(pane).status = "Esperando autorización del sistema…".into();
                let delete_task = Task::perform(
                    run_blocking_file_operation(move || {
                        operations::run_elevated_delete(&pending.paths, kind)
                    }),
                    move |result| {
                        Message::ElevatedDeleteFinished(pane, permanent, transfer_id, result)
                    },
                );
                Task::batch([self.ensure_transfer_window_task(), delete_task])
            }
            Message::CancelElevatedDelete => {
                if let Some(pending) = self.elevated_delete_dialog.take() {
                    self.pane_mut(pending.pane).status = pending.error;
                }
                Task::none()
            }
            Message::ElevatedDeleteFinished(pane, permanent, transfer_id, result) => {
                self.active_deletes.remove(&transfer_id);
                match result {
                    Ok(count) => {
                        self.pane_mut(pane).status = if permanent {
                            format!("Eliminados permanentemente {count} elemento(s)")
                        } else {
                            format!("Enviados a la papelera {count} elemento(s)")
                        };
                    }
                    Err(error) => {
                        let error_task = self.report_error(pane, error);
                        return Task::batch([
                            error_task,
                            self.start_load(pane),
                            self.close_transfer_window_if_idle_task(),
                        ]);
                    }
                }
                Task::batch([
                    self.start_load(pane),
                    self.close_transfer_window_if_idle_task(),
                ])
            }
            Message::ConfirmElevatedFileAction => {
                let Some(pending) = self.elevated_file_action_dialog.take() else {
                    return Task::none();
                };
                let pane = pending.pane;
                let action = pending.action;
                let worker_action = action.clone();
                self.pane_mut(pane).status = "Esperando autorización del sistema…".into();
                Task::perform(
                    run_blocking_file_operation(move || {
                        operations::run_elevated_file_action(&worker_action)
                    }),
                    move |result| Message::ElevatedFileActionFinished(pane, action, result),
                )
            }
            Message::CancelElevatedFileAction => {
                if let Some(pending) = self.elevated_file_action_dialog.take() {
                    self.pane_mut(pending.pane).status = pending.error;
                }
                Task::none()
            }
            Message::ElevatedFileActionFinished(pane, action, result) => match result {
                Ok(created_or_renamed) => {
                    match &action {
                        operations::ElevatedFileAction::Rename { path, .. } => {
                            let state = self.pane_mut(pane);
                            if state.selected.remove(path) {
                                state.selected.insert(created_or_renamed.clone());
                            }
                            state.status = format!(
                                "Renombrado a {} con permisos elevados",
                                created_or_renamed.display()
                            );
                        }
                        _ => {
                            self.pane_mut(pane).status = format!(
                                "Creado {} con permisos elevados",
                                created_or_renamed.display()
                            );
                            self.pending_new_folder_rename =
                                Some((pane, created_or_renamed.clone()));
                        }
                    }
                    self.start_load(pane)
                }
                Err(error) => self.report_error(pane, error),
            },
            Message::ToggleSettings => {
                if self.settings_open {
                    return self.request_popup_close(PendingPopupClose::Settings);
                }
                self.title_menu_open = false;
                self.show_menu_open = false;
                self.show_menu_parent_hovered = false;
                self.show_menu_submenu_hovered = false;
                self.new_menu_open = None;
                self.request_popup_backdrop(PopupBackdropTarget::Settings)
            }
            Message::TogglePreviewPanel(pane) => {
                self.focus_pane(pane);
                if self.uses_split_preview_panels() {
                    self.config.show_preview_panel = !self.config.show_preview_panel;
                    self.preview_panel_target_pane = None;
                    if self.config.show_preview_panel {
                        self.preview_panel_pane = Some(pane);
                    }
                    save_config(&self.config);
                    return if self.config.show_preview_panel {
                        Task::batch([
                            self.queue_selected_preview(PaneId::Primary),
                            self.queue_selected_preview(PaneId::Secondary),
                        ])
                    } else {
                        Task::none()
                    };
                }
                let is_current_panel = self.preview_panel_pane == Some(pane)
                    && self.preview_panel_target_pane.is_none();
                if self.config.show_preview_panel && is_current_panel {
                    self.config.show_preview_panel = false;
                    self.preview_panel_target_pane = None;
                } else {
                    self.config.show_preview_panel = true;
                    if self.preview_panel_pane.is_none() {
                        self.preview_panel_pane = Some(pane);
                    }
                }
                save_config(&self.config);
                if self.config.show_preview_panel {
                    self.queue_selected_preview(pane)
                } else {
                    Task::none()
                }
            }
            Message::ToggleColorPicker => {
                if self.color_picker_open {
                    return self.request_popup_close(PendingPopupClose::ColorPicker);
                }
                self.request_popup_backdrop(PopupBackdropTarget::ColorPicker)
            }
            Message::FontDown => {
                self.config.font_size = (self.config.font_size.round() - 1.0).clamp(10.0, 18.0);
                save_config(&self.config);
                Task::none()
            }
            Message::FontUp => {
                self.config.font_size = (self.config.font_size.round() + 1.0).clamp(10.0, 18.0);
                save_config(&self.config);
                Task::none()
            }
            Message::AccentRgbChanged(channel, value) => {
                if channel < self.color_rgb_inputs.len() {
                    self.color_rgb_inputs[channel] = value;
                    if let (Ok(r), Ok(g), Ok(b)) = (
                        self.color_rgb_inputs[0].parse::<u8>(),
                        self.color_rgb_inputs[1].parse::<u8>(),
                        self.color_rgb_inputs[2].parse::<u8>(),
                    ) {
                        self.set_accent_color([r, g, b]);
                        save_config(&self.config);
                    }
                }
                Task::none()
            }
            Message::StartAccentPlaneDrag => {
                self.accent_plane_dragging = true;
                if let Some(point) = self.accent_plane_pointer {
                    self.apply_accent_plane_point(point);
                }
                Task::none()
            }
            Message::AccentPlaneHover(point) => {
                self.accent_plane_pointer = Some(point);
                if self.accent_plane_dragging {
                    self.apply_accent_plane_point(point);
                }
                Task::none()
            }
            Message::StartAccentHueDrag => {
                self.accent_hue_dragging = true;
                if let Some(point) = self.accent_hue_pointer {
                    self.apply_accent_hue_point(point);
                }
                Task::none()
            }
            Message::AccentHueHover(point) => {
                self.accent_hue_pointer = Some(point);
                if self.accent_hue_dragging {
                    self.apply_accent_hue_point(point);
                }
                Task::none()
            }
            Message::FinishColorDrag => {
                self.accent_plane_dragging = false;
                self.accent_hue_dragging = false;
                save_config(&self.config);
                Task::none()
            }
            Message::SelectLanguage(selection) => {
                self.config.language = if selection == "Español" || selection == "Spanish" {
                    "es".into()
                } else {
                    "en".into()
                };
                save_config(&self.config);
                Task::none()
            }
            Message::SelectTheme(selection) => {
                self.config.theme = match selection.as_str() {
                    "Sistema" | "System" => ThemePreference::System,
                    "Claro" | "Light" => ThemePreference::Light,
                    _ => ThemePreference::Dark,
                };
                save_config(&self.config);
                let appearance_task = self.apply_window_corners_task();
                if matches!(self.config.theme, ThemePreference::System) {
                    Task::batch([
                        appearance_task,
                        iced::system::theme().map(Message::SystemThemeChanged),
                    ])
                } else {
                    appearance_task
                }
            }
            Message::SystemThemeChanged(mode) => {
                self.system_theme_mode = mode;
                self.apply_window_corners_task()
            }
            Message::SelectVibrancy(selection) => {
                self.config.vibrancy = vibrancy_mode_from_label(&selection, self.is_spanish());
                self.config.vibrancy_active = self.config.vibrancy != VibrancyMode::None;
                save_config(&self.config);
                self.apply_window_corners_task()
            }
            Message::SetVibrancyIntensity(intensity) => {
                self.config.vibrancy_intensity = intensity.min(100);
                Task::none()
            }
            Message::VibrancyIntensityReleased => {
                save_config(&self.config);
                self.apply_window_corners_task()
            }
            Message::VibrancyApplied(active) => {
                self.config.vibrancy_active = active;
                Task::none()
            }
            Message::ToggleShowExtensions => {
                self.config.show_extensions = !self.config.show_extensions;
                save_config(&self.config);
                Task::none()
            }
            Message::ToggleShowHidden => {
                self.config.show_hidden = !self.config.show_hidden;
                save_config(&self.config);
                let mut tasks = vec![self.start_load(PaneId::Primary)];
                if self.split.is_some() {
                    tasks.push(self.start_load(PaneId::Secondary));
                }
                Task::batch(tasks)
            }
            Message::StartSidebarResize => {
                self.resize_drag = Some(ResizeDrag::Sidebar {
                    start_x: f32::NAN,
                    start_width: self.config.sidebar_width,
                });
                Task::none()
            }
            Message::StartPreviewResize(pane) => {
                self.resize_drag = Some(ResizeDrag::Preview {
                    pane,
                    start_x: f32::NAN,
                    start_width: self.config.preview_panel_width,
                });
                Task::none()
            }
            Message::StartSplitResize => {
                if let Some(split) = &self.split {
                    self.resize_drag = Some(ResizeDrag::Split {
                        start_x: f32::NAN,
                        start_ratio: split.ratio,
                    });
                }
                Task::none()
            }
            Message::StartColumnResize(pane, column) => {
                let widths = self.detail_column_widths(pane, self.font_size());
                self.resize_drag = Some(ResizeDrag::Column {
                    pane,
                    column,
                    start_x: f32::NAN,
                    start_width: widths.get(column),
                });
                Task::none()
            }
            Message::PointerMoved(position) => {
                self.handle_pointer_moved(position);
                if self.file_drag_is_ready_for_external_handoff(position) {
                    self.start_external_file_drag()
                } else {
                    Task::none()
                }
            }
            Message::PointerLeftWindow => {
                self.sidebar_pointer_inside = false;
                self.start_external_file_drag()
            }
            Message::StopResize => {
                let tab_drag_dirty = self.tab_drag.is_some_and(|drag| drag.dirty);
                let split_placement = self.tab_drag.and_then(|drag| {
                    (drag.dragging && self.split.is_none() && self.tabs.len() > 1)
                        .then(|| self.tab_split_drop_side(self.cursor_position))
                        .flatten()
                        .map(|side| (drag.tab_index, side))
                });
                let split_resize_dirty = matches!(self.resize_drag, Some(ResizeDrag::Split { .. }));
                let sidebar_section_drag = self.sidebar_section_drag.take();
                let file_drag = self.file_drag.take();
                if let Some(drag) = &file_drag {
                    self.fade_out_file_drag_overlay(drag);
                }
                if self.resize_drag.is_some() {
                    save_config(&self.config);
                }
                self.resize_drag = None;
                self.tab_drag = None;
                self.rubber_band = None;
                if tab_drag_dirty || split_resize_dirty {
                    self.save_session();
                }
                if let Some((tab_index, side)) = split_placement {
                    return self.place_tab_in_split(tab_index, side);
                }
                if let Some(drag) = sidebar_section_drag {
                    if drag.dragging {
                        if drag.dirty {
                            save_config(&self.config);
                        }
                    } else {
                        self.toggle_sidebar_section(drag.section);
                    }
                }
                if let Some(drag) = file_drag {
                    if drag.dragging {
                        self.file_drag_suppressed_click =
                            Some((drag.source_pane, drag.source_index));
                        return Task::batch([
                            self.finish_file_drag(drag),
                            Task::perform(delay(Duration::from_millis(180)), |_| {
                                Message::ClearFileDragClickSuppression
                            }),
                        ]);
                    }
                    if drag.collapse_selection_on_click {
                        self.select_single(drag.source_pane, drag.source_index);
                        return self.queue_selected_preview(drag.source_pane);
                    }
                }
                Task::none()
            }
            Message::ClearFileDragClickSuppression => {
                self.file_drag_suppressed_click = None;
                Task::none()
            }
            Message::ExternalFileDragFinished(pane, count, result) => match result {
                Ok(()) => {
                    self.native_external_drag_active = true;
                    self.pane_mut(pane).status =
                        format!("Arrastrando {count} elemento(s) fuera de BExplorer");
                    Task::none()
                }
                Err(error) => {
                    self.native_external_drag_active = false;
                    self.report_error(
                        pane,
                        format!("No se pudo iniciar el arrastre externo: {error}"),
                    )
                }
            },
            Message::PollExternalFileDrag => self.poll_external_file_drag(),
            Message::ExternalFileDragPolled(result) => match result {
                Ok((active, drops)) => {
                    let was_active = self.native_external_drag_active;
                    self.native_external_drag_active = active;
                    if was_active && !active {
                        // A completed outbound drag must not leave a stale
                        // status in the footer. Restore the normal summary
                        // without skipping any incoming drops from this poll.
                        let pane = self.focused_pane();
                        let count = self.pane(pane).entries.len();
                        self.pane_mut(pane).status = format!("{count} elements");
                    }
                    let tasks = drops
                        .into_iter()
                        .map(|paths| self.copy_external_files_into_focused_pane(paths))
                        .collect::<Vec<_>>();
                    if tasks.is_empty() {
                        Task::none()
                    } else {
                        Task::batch(tasks)
                    }
                }
                Err(error) => {
                    self.native_external_drag_active = false;
                    self.pane_mut(self.focused_pane()).status =
                        format!("El arrastre externo se interrumpió: {error}");
                    Task::none()
                }
            },
            Message::ExternalFileDropped(path) => {
                if !path.exists() || self.pending_external_file_drops.contains(&path) {
                    return Task::none();
                }
                self.pending_external_file_drops.push(path);
                if self.external_file_drop_flush_queued {
                    Task::none()
                } else {
                    self.external_file_drop_flush_queued = true;
                    Task::perform(delay(Duration::from_millis(45)), |_| {
                        Message::FlushExternalFileDrops
                    })
                }
            }
            Message::FlushExternalFileDrops => {
                self.external_file_drop_flush_queued = false;
                let paths = std::mem::take(&mut self.pending_external_file_drops);
                self.copy_external_files_into_focused_pane(paths)
            }
            Message::MainWindowOpened(id) => {
                self.main_window_id = Some(id);
                Task::batch([
                    self.apply_window_corners_task_for(id),
                    self.prepare_native_file_drag_task_for(id),
                    self.sync_main_window_maximized_task(id),
                ])
            }
            Message::TransferWindowOpened(id) => {
                self.transfer_window_id = Some(id);
                self.transfer_window_item_count = self.transfer_items().len();
                Task::batch([
                    self.apply_window_corners_task_for(id),
                    self.sync_transfer_window_size_task(),
                    window::minimize(id, false),
                    window::gain_focus(id),
                ])
            }
            Message::ArchiveWindowOpened(id) => {
                self.archive_window_id = Some(id);
                self.archive_window_item_count = self.archive_items().len();
                Task::batch([
                    self.apply_window_corners_task_for(id),
                    self.sync_archive_window_size_task(),
                    window::minimize(id, false),
                    window::gain_focus(id),
                ])
            }
            #[cfg(target_os = "windows")]
            Message::DefenderWindowOpened(id) => {
                self.defender_window_id = Some(id);
                Task::batch([
                    self.apply_window_corners_task_for(id),
                    self.sync_defender_window_size_task(),
                    window::minimize(id, false),
                    window::gain_focus(id),
                ])
            }
            Message::DefenderThreatsWindowOpened(id) => {
                self.defender_threats_window_id = Some(id);
                let threat_count = self
                    .defender_summary
                    .as_ref()
                    .map(|summary| summary.threats.len())
                    .unwrap_or_default();
                Task::batch([
                    self.apply_window_corners_task_for(id),
                    sync_fixed_progress_window_size_task(
                        id,
                        defender_threats_window_size(threat_count),
                    ),
                    window::minimize(id, false),
                    window::gain_focus(id),
                ])
            }
            Message::ReopenTransferWindow(old_id, position) => {
                if self.transfer_window_id == Some(old_id) {
                    self.reopen_transfer_window_task(old_id, self.transfer_items().len(), position)
                } else {
                    Task::none()
                }
            }
            Message::ReopenArchiveWindow(old_id, position) => {
                if self.archive_window_id == Some(old_id) {
                    self.reopen_archive_window_task(old_id, self.archive_items().len(), position)
                } else {
                    Task::none()
                }
            }
            Message::WindowCloseRequested(id) => {
                if self.main_window_id == Some(id) {
                    self.update(Message::WindowClose)
                } else if self.transfer_window_id == Some(id)
                    || self.archive_window_id == Some(id)
                    || self.defender_window_id == Some(id)
                    || self.defender_threats_window_id == Some(id)
                {
                    self.close_window_task(id)
                } else {
                    Task::none()
                }
            }
            Message::WindowClosed(id) => {
                self.closing_windows.remove(&id);
                if self.main_window_id == Some(id) {
                    self.save_session();
                    save_config(&self.config);
                    self.main_window_id = None;
                    iced::exit()
                } else {
                    if self.transfer_window_id == Some(id) {
                        self.transfer_window_id = None;
                        self.transfer_window_item_count = 0;
                    }
                    if self.archive_window_id == Some(id) {
                        self.archive_window_id = None;
                        self.archive_window_item_count = 0;
                    }
                    if self.defender_window_id == Some(id) {
                        self.defender_window_id = None;
                        self.cancel_defender_scan();
                        self.defender_progress = None;
                        self.defender_summary = None;
                        self.defender_rx = None;
                        self.defender_cancel = None;
                    }
                    if self.defender_threats_window_id == Some(id) {
                        self.defender_threats_window_id = None;
                    }
                    Task::none()
                }
            }
            Message::PollTransfers => {
                self.transfer_progress_phase = (self.transfer_progress_phase + 0.025) % 1.0;
                let defender_task = self.poll_defender_messages();
                let mut tasks = vec![
                    defender_task,
                    self.poll_transfer_messages(),
                    self.poll_archive_messages(),
                    self.sync_defender_window_size_task(),
                ];
                if !self.transfer_active()
                    && let Some(id) = self.transfer_window_id.take()
                {
                    self.transfer_window_item_count = 0;
                    tasks.push(self.close_window_task(id));
                } else {
                    tasks.push(self.sync_transfer_window_size_task());
                }
                if !self.archive_active()
                    && let Some(id) = self.archive_window_id.take()
                {
                    self.archive_window_item_count = 0;
                    tasks.push(self.close_window_task(id));
                } else {
                    tasks.push(self.sync_archive_window_size_task());
                }
                Task::batch(tasks)
            }
            Message::TransferWindowDrag => {
                if let Some(id) = self.transfer_window_id {
                    window::drag(id)
                } else {
                    Task::none()
                }
            }
            Message::TransferWindowMinimize => {
                if let Some(id) = self.transfer_window_id {
                    window::minimize(id, true)
                } else {
                    Task::none()
                }
            }
            Message::ArchiveWindowDrag => {
                if let Some(id) = self.archive_window_id {
                    window::drag(id)
                } else {
                    Task::none()
                }
            }
            Message::ArchiveWindowMinimize => {
                if let Some(id) = self.archive_window_id {
                    window::minimize(id, true)
                } else {
                    Task::none()
                }
            }
            Message::DefenderWindowDrag => {
                if let Some(id) = self.defender_window_id {
                    window::drag(id)
                } else {
                    Task::none()
                }
            }
            Message::DefenderWindowMinimize => {
                if let Some(id) = self.defender_window_id {
                    window::minimize(id, true)
                } else {
                    Task::none()
                }
            }
            Message::DefenderThreatsWindowDrag => {
                if let Some(id) = self.defender_threats_window_id {
                    window::drag(id)
                } else {
                    Task::none()
                }
            }
            Message::DefenderThreatsWindowMinimize => {
                if let Some(id) = self.defender_threats_window_id {
                    window::minimize(id, true)
                } else {
                    Task::none()
                }
            }
            Message::ToggleTransferPause(id) => {
                if let Some(active) = self.active_transfers.get(&id) {
                    let paused = !active
                        .control
                        .pause
                        .load(std::sync::atomic::Ordering::Relaxed);
                    active
                        .control
                        .pause
                        .store(paused, std::sync::atomic::Ordering::Relaxed);
                    if let Some(progress) = self.transfer_progress.get_mut(&id) {
                        progress.state = if paused {
                            TransferState::Paused
                        } else {
                            TransferState::Copying
                        };
                    }
                }
                Task::none()
            }
            Message::CancelTransfer(id) => {
                if let Some(active) = self.active_transfers.get(&id) {
                    active
                        .control
                        .cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                } else if let Some(index) = self
                    .transfer_queue
                    .iter()
                    .position(|queued| queued.job.id == id)
                    && let Some(queued) = self.transfer_queue.remove(index)
                {
                    let mut progress = self
                        .transfer_progress
                        .remove(&id)
                        .unwrap_or_else(|| TransferProgress::pending(&queued.job));
                    progress.state = TransferState::Cancelled;
                    self.transfer_history.push_back(TransferHistoryState {
                        progress,
                        finished_at: Instant::now(),
                    });
                }
                self.sync_transfer_window_size_task()
            }
            Message::WindowResized(id, size) => {
                if self.main_window_id == Some(id) {
                    self.window_size = size;
                    self.config.window_size = [size.width, size.height];
                    Task::batch([
                        self.apply_window_corners_only_task_for(id),
                        self.sync_main_window_maximized_task(id),
                    ])
                } else if self.transfer_window_id == Some(id)
                    && progress_window_needs_resize(size, self.transfer_window_size())
                {
                    // On Wayland a newly created secondary window may report its
                    // pre-scale physical size first. Retry after that configure
                    // event, when Iced knows the monitor scale factor.
                    self.sync_transfer_window_size_task()
                } else if self.archive_window_id == Some(id)
                    && progress_window_needs_resize(size, self.archive_window_size())
                {
                    self.sync_archive_window_size_task()
                } else if self.defender_window_id == Some(id)
                    && progress_window_needs_resize(size, self.defender_window_size())
                {
                    self.sync_defender_window_size_task()
                } else if self.defender_threats_window_id == Some(id) {
                    let threat_count = self
                        .defender_summary
                        .as_ref()
                        .map(|summary| summary.threats.len())
                        .unwrap_or_default();
                    let expected = defender_threats_window_size(threat_count);
                    if progress_window_needs_resize(size, expected) {
                        sync_fixed_progress_window_size_task(id, expected)
                    } else {
                        self.apply_window_corners_only_task_for(id)
                    }
                } else if self.transfer_window_id == Some(id)
                    || self.archive_window_id == Some(id)
                    || self.defender_window_id == Some(id)
                {
                    self.apply_window_corners_only_task_for(id)
                } else {
                    Task::none()
                }
            }
            Message::WindowDrag => {
                if let Some(id) = self.main_window_id {
                    window::drag(id)
                } else {
                    Task::none()
                }
            }
            Message::WindowResize(direction) => {
                if !self.window_maximized
                    && let Some(id) = self.main_window_id
                {
                    window::drag_resize(id, direction)
                } else {
                    Task::none()
                }
            }
            Message::WindowMinimize => {
                if let Some(id) = self.main_window_id {
                    window::minimize(id, true)
                } else {
                    Task::none()
                }
            }
            Message::WindowMaximize => {
                if let Some(id) = self.main_window_id {
                    // Ask the native window for the explicit next state. A later
                    // native-state query remains authoritative when the window
                    // is restored through the compositor or system shortcuts.
                    self.window_maximized = !self.window_maximized;
                    self.config.window_maximized = self.window_maximized;
                    Task::batch([
                        window::maximize(id, self.window_maximized),
                        self.apply_window_corners_task_for(id),
                    ])
                } else {
                    Task::none()
                }
            }
            Message::WindowMaximizedState(id, maximized) => {
                if self.main_window_id != Some(id) {
                    return Task::none();
                }
                let changed = self.window_maximized != maximized;
                self.window_maximized = maximized;
                self.config.window_maximized = maximized;
                if maximized {
                    self.resize_drag = None;
                }
                if changed {
                    self.apply_window_corners_task_for(id)
                } else {
                    Task::none()
                }
            }
            Message::WindowClose => {
                if self
                    .main_window_id
                    .is_some_and(|id| self.closing_windows.contains(&id))
                {
                    return Task::none();
                }
                self.save_session();
                save_config(&self.config);
                if let Some(id) = self.main_window_id {
                    self.close_application_task(id)
                } else {
                    iced::exit()
                }
            }
            #[cfg(debug_assertions)]
            Message::DebugAddArchive(index) => {
                self.insert_debug_archive(index);
                self.ensure_archive_window_task()
            }
            Message::Noop => Task::none(),
        }
    }
}
