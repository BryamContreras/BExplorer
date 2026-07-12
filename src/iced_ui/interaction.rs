use super::*;

impl BExplorerIced {
    pub(in crate::iced_ui) fn request_context_menu(
        &mut self,
        pane: PaneId,
        target: ContextTarget,
    ) -> Task<Message> {
        self.focus_pane(pane);
        self.title_menu_open = false;
        self.view_menu_open = None;
        self.group_menu_open = None;
        self.new_menu_open = None;
        self.context_archive_submenu = false;
        self.context_extract_submenu = false;
        self.context_new_submenu = false;
        self.context_archive_parent_hovered = false;
        self.context_archive_submenu_hovered = false;
        self.context_new_parent_hovered = false;
        self.context_new_submenu_hovered = false;
        let position = if matches!(target, ContextTarget::SidebarDrive(_)) {
            self.cursor_position
        } else {
            self.pane_pointer
                .filter(|(pointer_pane, _)| *pointer_pane == pane)
                .map(|(_, point)| point)
                .unwrap_or(Point::new(18.0, 92.0))
        };
        self.context_menu = None;
        self.context_menu_request_id = self.context_menu_request_id.saturating_add(1);
        let menu = ContextMenuState {
            request_id: self.context_menu_request_id,
            pane,
            target,
            position,
            backdrop_origin: Point::ORIGIN,
            backdrop: None,
            source_screenshot: None,
            submenu_backdrop: None,
            submenu_backdrop_kind: None,
            paste_available: false,
        };
        let (x, y) = self.context_menu_window_position(&menu);
        let menu = ContextMenuState {
            backdrop_origin: Point::new(x, y),
            ..menu
        };
        if matches!(target, ContextTarget::SidebarDrive(_)) {
            return self.capture_context_menu_backdrop(menu);
        }
        let local_paste_available = self
            .file_clipboard
            .as_ref()
            .is_some_and(|clipboard| !clipboard.paths.is_empty());
        Task::perform(
            async move {
                run_blocking_file_operation(move || {
                    let native_paste_available =
                        shell::read_files().is_ok_and(|clipboard| !clipboard.paths.is_empty());
                    Ok::<bool, BExplorerError>(local_paste_available || native_paste_available)
                })
                .await
                .unwrap_or(local_paste_available)
            },
            move |available| Message::ContextPasteAvailabilityResolved(menu.clone(), available),
        )
    }

    pub(in crate::iced_ui) fn capture_context_menu_backdrop(
        &mut self,
        menu: ContextMenuState,
    ) -> Task<Message> {
        let Some(id) = self.main_window_id else {
            self.popup_fade_progress = 0.0;
            self.context_menu = Some(menu);
            return Task::none();
        };
        window::screenshot(id)
            .map(move |screenshot| Message::ContextBackdropCaptured(menu.clone(), screenshot))
    }

    pub(in crate::iced_ui) fn context_submenu_geometry(
        &self,
        menu: &ContextMenuState,
        kind: ContextSubmenuKind,
    ) -> (Point, Size) {
        let labels = match kind {
            ContextSubmenuKind::Archive => {
                let archive_name = self
                    .default_archive_name(menu.pane, &self.context_paths(menu.pane, menu.target));
                vec![
                    self.localized("Comprimir", "Compress").to_owned(),
                    view::context_archive_option_label(
                        self.localized("Comprimir", "Compress"),
                        &archive_name,
                        "7z",
                    ),
                    view::context_archive_option_label(
                        self.localized("Comprimir", "Compress"),
                        &archive_name,
                        "zip",
                    ),
                ]
            }
            ContextSubmenuKind::Extract => {
                let extract_to_label = self
                    .context_entry(menu.pane, menu.target)
                    .and_then(|entry| {
                        archive::planned_extract_destination(
                            &entry.path,
                            ExtractMode::ToNamedFolder,
                        )
                        .ok()
                    })
                    .and_then(|path| {
                        path.file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                    })
                    .map(|folder| {
                        format!(
                            "{} {}",
                            self.localized("Extraer en", "Extract to"),
                            ellipsize_text(&folder, 25),
                        )
                    })
                    .unwrap_or_else(|| {
                        self.localized("Extraer en carpeta", "Extract to folder")
                            .to_owned()
                    });
                vec![
                    self.localized("Extraer aquí", "Extract here").to_owned(),
                    extract_to_label,
                ]
            }
            ContextSubmenuKind::New => vec![
                self.localized("Nueva carpeta", "New folder").to_owned(),
                self.localized("Documento de texto", "Text document")
                    .to_owned(),
            ],
        };
        let width = view::context_submenu_width(&labels);
        let height = match kind {
            ContextSubmenuKind::Archive => 114.0,
            ContextSubmenuKind::Extract | ContextSubmenuKind::New => 78.0,
        };
        let (x, y) = self.context_menu_window_position(menu);
        let submenu_x = if x + 258.0 + width <= self.window_size.width - 8.0 {
            x + 252.0
        } else {
            (x - width + 6.0).max(8.0)
        };
        let offset_y = match kind {
            ContextSubmenuKind::Archive => 112.0,
            ContextSubmenuKind::Extract => 146.0,
            ContextSubmenuKind::New => 98.0,
        };
        let submenu_y =
            (y + offset_y).clamp(8.0, (self.window_size.height - height - 8.0).max(8.0));
        (Point::new(submenu_x, submenu_y), Size::new(width, height))
    }

    pub(in crate::iced_ui) fn request_context_submenu_backdrop(
        &mut self,
        kind: ContextSubmenuKind,
    ) -> Task<Message> {
        let Some(menu) = self.context_menu.as_ref() else {
            return Task::none();
        };
        let request_id = menu.request_id;
        if let Some(screenshot) = menu.source_screenshot.clone() {
            let (origin, size) = self.context_submenu_geometry(menu, kind);
            return Task::perform(
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
                move |backdrop| Message::ContextSubmenuBackdropPrepared(request_id, kind, backdrop),
            );
        }
        let Some(id) = self.main_window_id else {
            return Task::none();
        };
        window::screenshot(id).map(move |screenshot| {
            Message::ContextSubmenuBackdropCaptured(request_id, kind, screenshot)
        })
    }

    /// Captures the content below a popup before making that popup visible.
    /// The blur itself runs on a worker in `PopupBackdropCaptured`.
    pub(in crate::iced_ui) fn request_popup_backdrop(
        &mut self,
        target: PopupBackdropTarget,
    ) -> Task<Message> {
        self.title_submenu_backdrop = None;
        if matches!(target, PopupBackdropTarget::ColorPicker) {
            self.color_picker_backdrop = None;
            self.color_picker_fade_progress = 0.0;
        } else {
            self.popup_backdrop = None;
            self.popup_fade_progress = 0.0;
        }
        let Some(id) = self.main_window_id else {
            return self.show_popup_with_backdrop(target);
        };
        window::screenshot(id)
            .map(move |screenshot| Message::PopupBackdropCaptured(target.clone(), screenshot))
    }

    pub(in crate::iced_ui) fn show_popup_with_backdrop(
        &mut self,
        target: PopupBackdropTarget,
    ) -> Task<Message> {
        match target {
            PopupBackdropTarget::TitleMenu => {
                self.title_menu_open = true;
            }
            PopupBackdropTarget::NewMenu(pane) => {
                self.new_menu_open = Some(pane);
            }
            PopupBackdropTarget::SearchModeMenu(pane) => {
                self.search_mode_menu_open = Some(pane);
            }
            PopupBackdropTarget::ViewMenu(pane) => {
                self.view_menu_open = Some(pane);
            }
            PopupBackdropTarget::GroupMenu(pane) => {
                self.group_menu_open = Some(pane);
            }
            PopupBackdropTarget::Settings => {
                self.settings_open = true;
            }
            PopupBackdropTarget::Shortcuts => {
                self.shortcuts_open = true;
            }
            PopupBackdropTarget::ColorPicker => {
                self.color_picker_open = true;
            }
            PopupBackdropTarget::Rename(dialog) => {
                let select_end = dialog.select_end;
                self.rename_dialog = Some(dialog);
                return focus_inline_rename_task(select_end);
            }
            PopupBackdropTarget::PermanentDelete(pending) => {
                self.permanent_delete_dialog = Some(pending);
            }
            PopupBackdropTarget::Archive(dialog) => {
                self.archive_dialog = Some(dialog);
            }
            PopupBackdropTarget::TransferConflict(dialog) => {
                self.transfer_conflict_dialog = Some(dialog);
            }
        }
        Task::none()
    }

    pub(in crate::iced_ui) fn context_menu_window_position(
        &self,
        menu: &ContextMenuState,
    ) -> (f32, f32) {
        const MENU_WIDTH: f32 = 258.0;
        let menu_height = self.context_menu_height(menu);

        if matches!(menu.target, ContextTarget::SidebarDrive(_)) {
            return (
                (menu.position.x + 2.0)
                    .clamp(8.0, (self.window_size.width - MENU_WIDTH - 8.0).max(8.0)),
                (menu.position.y + 2.0)
                    .clamp(8.0, (self.window_size.height - menu_height - 8.0).max(8.0)),
            );
        }

        let point = menu.position;
        let pane_x = self.pane_global_x(menu.pane);
        // `PanePointerMoved` is relative to the file-table surface. Convert
        // it using the bars that are actually visible; the old fixed 46 px
        // action-bar offset pushed a context menu down whenever that bar was
        // disabled.
        let table_y = TITLE_HEIGHT
            + 42.0
            + if self.split.is_some() { 1.0 } else { 0.0 }
            + if self.config.show_action_bar {
                46.0
            } else {
                0.0
            }
            + if self.config.show_bookmark_bar || !self.sidebar_visible {
                46.0
            } else {
                0.0
            };
        let x = (pane_x + point.x + 2.0)
            .clamp(8.0, (self.window_size.width - MENU_WIDTH - 8.0).max(8.0));
        let y = (table_y + point.y + 2.0)
            .clamp(8.0, (self.window_size.height - menu_height - 8.0).max(8.0));
        (x, y)
    }

    pub(in crate::iced_ui) fn context_menu_height(&self, menu: &ContextMenuState) -> f32 {
        match menu.target {
            ContextTarget::Background => 218.0,
            ContextTarget::SidebarDrive(_) => 48.0,
            ContextTarget::Entry(_) => {
                let has_extract_action =
                    self.context_entry(menu.pane, menu.target)
                        .is_some_and(|entry| {
                            crate::fs::archive_listing::has_extractable_archive_extension(
                                &entry.path,
                            )
                        });
                let terminal_available =
                    self.context_entry(menu.pane, menu.target)
                        .is_some_and(|entry| {
                            entry.kind.is_container() && !explorer::is_virtual_path(&entry.path)
                        });
                let base_height = if has_extract_action { 404.0 } else { 368.0 };
                let advanced_rows = self
                    .context_entry(menu.pane, menu.target)
                    .map(|entry| {
                        usize::from(is_mountable_disk_image_entry(&entry))
                            + usize::from(entry.drive_kind.is_some_and(DriveKind::is_ejectable))
                            + usize::from(
                                cfg!(target_os = "windows")
                                    && !explorer::is_virtual_path(&entry.path),
                            )
                    })
                    .unwrap_or(0);
                let base_height = base_height + advanced_rows as f32 * 36.0;
                if terminal_available {
                    base_height
                } else {
                    base_height - 36.0
                }
            }
        }
    }

    pub(in crate::iced_ui) fn pane_global_x(&self, pane: PaneId) -> f32 {
        let sidebar_width = self.current_sidebar_width();
        if let Some(split) = &self.split {
            let global_sidebar_width = if self.uses_split_sidebars() {
                0.0
            } else {
                sidebar_width
            };
            let content_width = (self.window_size.width - global_sidebar_width).max(1.0);
            let available = (content_width - SPLIT_DIVIDER_WIDTH).max(1.0);
            let pane_sidebar_width = self
                .uses_split_sidebars()
                .then_some(sidebar_width)
                .unwrap_or(0.0);
            match pane {
                PaneId::Primary => global_sidebar_width + pane_sidebar_width,
                PaneId::Secondary => {
                    global_sidebar_width
                        + available * split.ratio
                        + SPLIT_DIVIDER_WIDTH
                        + pane_sidebar_width
                }
            }
        } else {
            sidebar_width
        }
    }

    pub(in crate::iced_ui) fn run_context_command(
        &mut self,
        command: ContextCommand,
    ) -> Task<Message> {
        let Some(menu) = self.context_menu.clone() else {
            return Task::none();
        };
        if command == ContextCommand::CompressMenu {
            self.context_archive_submenu = true;
            self.context_extract_submenu = false;
            return self.request_context_submenu_backdrop(ContextSubmenuKind::Archive);
        }
        if command == ContextCommand::ExtractMenu {
            self.context_archive_submenu = true;
            self.context_extract_submenu = true;
            return self.request_context_submenu_backdrop(ContextSubmenuKind::Extract);
        }
        if command == ContextCommand::NewMenu {
            self.context_new_submenu = true;
            return self.request_context_submenu_backdrop(ContextSubmenuKind::New);
        }
        self.context_menu = None;
        self.context_archive_submenu = false;
        self.context_extract_submenu = false;
        self.context_new_submenu = false;
        self.context_archive_parent_hovered = false;
        self.context_archive_submenu_hovered = false;
        self.context_new_parent_hovered = false;
        self.context_new_submenu_hovered = false;
        match command {
            ContextCommand::Paste => self.context_paste(menu.pane, menu.target),
            ContextCommand::Copy => self.context_copy(menu.pane, menu.target, false),
            ContextCommand::Cut => self.context_copy(menu.pane, menu.target, true),
            ContextCommand::Refresh => self.start_load(menu.pane),
            ContextCommand::NewMenu => Task::none(),
            ContextCommand::NewFolder => self.update(Message::NewFolder(menu.pane)),
            ContextCommand::NewTextDocument => self.update(Message::NewTextDocument(menu.pane)),
            ContextCommand::OpenTerminal => {
                self.context_open_terminal(menu.pane, menu.target);
                Task::none()
            }
            ContextCommand::Properties => {
                self.context_properties(menu.pane, menu.target);
                Task::none()
            }
            ContextCommand::Open => self.context_open(menu.pane, menu.target),
            ContextCommand::OpenWith => {
                self.context_open_with(menu.pane, menu.target);
                Task::none()
            }
            ContextCommand::CompressMenu => Task::none(),
            ContextCommand::ExtractMenu => Task::none(),
            ContextCommand::CompressDialog => {
                self.open_archive_dialog_for_context(menu.pane, menu.target)
            }
            ContextCommand::CompressDefault(format) => {
                self.start_context_archive_default(menu.pane, menu.target, format)
            }
            ContextCommand::Extract(mode) => {
                self.start_context_extract(menu.pane, menu.target, mode)
            }
            ContextCommand::Rename => self.context_begin_rename(menu.pane, menu.target),
            ContextCommand::Delete => self.context_delete(menu.pane, menu.target, false),
            ContextCommand::DeletePermanent => self.context_delete(menu.pane, menu.target, true),
            ContextCommand::MountDiskImage => {
                let Some(entry) = self.context_entry(menu.pane, menu.target) else {
                    return Task::none();
                };
                self.mount_disk_image(menu.pane, entry.path)
            }
            ContextCommand::EjectDrive => {
                let Some(entry) = self.context_entry(menu.pane, menu.target) else {
                    return Task::none();
                };
                self.eject_drive(menu.pane, entry.path)
            }
            ContextCommand::ScanWithDefender => {
                let paths = self.context_paths(menu.pane, menu.target);
                self.start_defender_scan(menu.pane, paths)
            }
        }
    }

    pub(in crate::iced_ui) fn context_entry(
        &self,
        pane: PaneId,
        target: ContextTarget,
    ) -> Option<FileEntry> {
        match target {
            ContextTarget::Entry(index) => self.pane(pane).entries.get(index).cloned(),
            ContextTarget::SidebarDrive(index) => self.sidebar_storage_entries.get(index).cloned(),
            ContextTarget::Background => None,
        }
    }

    pub(in crate::iced_ui) fn context_paths(
        &self,
        pane: PaneId,
        target: ContextTarget,
    ) -> Vec<PathBuf> {
        if let Some(entry) = self.context_entry(pane, target) {
            if self.pane(pane).selected.contains(&entry.path) {
                return self.pane(pane).selected.iter().cloned().collect::<Vec<_>>();
            }
            vec![entry.path]
        } else {
            self.pane(pane).selected.iter().cloned().collect::<Vec<_>>()
        }
    }

    pub(in crate::iced_ui) fn context_destination(
        &self,
        pane: PaneId,
        target: ContextTarget,
    ) -> Option<PathBuf> {
        if let Some(entry) = self.context_entry(pane, target)
            && entry.kind.is_container()
        {
            return Some(entry.path);
        }
        self.tab_for_pane(pane).path.clone()
    }

    pub(in crate::iced_ui) fn displayed_entry_indices(&self, pane: PaneId) -> Vec<usize> {
        self.filtered_entries(pane)
    }

    pub(in crate::iced_ui) fn select_single(&mut self, pane: PaneId, index: usize) {
        let Some(path) = self
            .pane(pane)
            .entries
            .get(index)
            .map(|entry| entry.path.clone())
        else {
            return;
        };
        let state = self.pane_mut(pane);
        state.selected.clear();
        state.selected.insert(path);
        state.selection_anchor = Some(index);
    }

    pub(in crate::iced_ui) fn select_range_to(&mut self, pane: PaneId, index: usize) {
        let anchor = self.pane(pane).selection_anchor.unwrap_or(index);
        let displayed = self.displayed_entry_indices(pane);
        let Some(anchor_pos) = displayed
            .iter()
            .position(|entry_index| *entry_index == anchor)
        else {
            self.select_single(pane, index);
            return;
        };
        let Some(target_pos) = displayed
            .iter()
            .position(|entry_index| *entry_index == index)
        else {
            self.select_single(pane, index);
            return;
        };
        let start = anchor_pos.min(target_pos);
        let end = anchor_pos.max(target_pos);
        let paths = displayed[start..=end]
            .iter()
            .filter_map(|entry_index| {
                self.pane(pane)
                    .entries
                    .get(*entry_index)
                    .map(|entry| entry.path.clone())
            })
            .collect::<HashSet<_>>();

        let state = self.pane_mut(pane);
        state.selected = paths;
        state.selection_anchor = Some(anchor);
    }

    pub(in crate::iced_ui) fn select_all(&mut self, pane: PaneId) {
        let displayed = self.displayed_entry_indices(pane);
        let paths = displayed
            .iter()
            .filter_map(|index| self.pane(pane).entries.get(*index))
            .map(|entry| entry.path.clone())
            .collect::<HashSet<_>>();
        let anchor = displayed.first().copied();
        let count = paths.len();
        let state = self.pane_mut(pane);
        state.selected = paths;
        state.selection_anchor = anchor;
        state.status = format!("Selected {count} item(s)");
    }

    pub(in crate::iced_ui) fn rename_selected(&mut self, pane: PaneId) -> Task<Message> {
        let selected: Vec<_> = self.pane(pane).selected.iter().cloned().collect();
        if selected.is_empty() {
            self.pane_mut(pane).status = "No selected items".into();
            return Task::none();
        }
        if selected.len() > 1 {
            self.pane_mut(pane).status = "Select one item to rename".into();
            return Task::none();
        }
        let path = &selected[0];
        let Some(index) = self
            .pane(pane)
            .entries
            .iter()
            .position(|entry| entry.path == *path)
        else {
            self.pane_mut(pane).status = "Selected item is no longer available".into();
            return Task::none();
        };
        self.context_begin_rename(pane, ContextTarget::Entry(index))
    }

    pub(in crate::iced_ui) fn handle_keyboard_shortcut(
        &mut self,
        shortcut: KeyboardShortcut,
    ) -> Task<Message> {
        if self.permanent_delete_dialog.is_some() {
            return if shortcut == KeyboardShortcut::Open {
                self.confirm_permanent_delete()
            } else {
                Task::none()
            };
        }
        if shortcut == KeyboardShortcut::Open {
            if self
                .suppress_open_after_rename_until
                .is_some_and(|until| Instant::now() < until)
            {
                return Task::none();
            }
            self.suppress_open_after_rename_until = None;
        }
        let pane = self.focused_pane();
        // The text input submits the rename and clears `rename_dialog` before
        // the same Enter can be observed by the global shortcut listener.
        // Keep shortcuts inert until that filesystem operation finishes so
        // Enter cannot immediately try to open the old, now-renamed path.
        if self.pending_file_operations.contains(&pane) {
            return Task::none();
        }
        if self.settings_open
            || self.shortcuts_open
            || self.rename_dialog.is_some()
            || self.archive_dialog.is_some()
        {
            return Task::none();
        }

        match shortcut {
            KeyboardShortcut::Copy => self.context_copy(pane, ContextTarget::Background, false),
            KeyboardShortcut::Paste => self.context_paste(pane, ContextTarget::Background),
            KeyboardShortcut::Cut => self.context_copy(pane, ContextTarget::Background, true),
            KeyboardShortcut::Undo => self.undo_last_action(),
            KeyboardShortcut::Refresh => self.start_load(pane),
            KeyboardShortcut::Delete => self.delete_selection(pane, false),
            KeyboardShortcut::PermanentDelete => self.delete_selection(pane, true),
            KeyboardShortcut::SelectAll => {
                self.select_all(pane);
                Task::none()
            }
            KeyboardShortcut::Rename => self.rename_selected(pane),
            KeyboardShortcut::EditAddress => self.update(Message::BeginAddressEdit(pane)),
            KeyboardShortcut::Properties => {
                self.context_properties(pane, ContextTarget::Background);
                Task::none()
            }
            KeyboardShortcut::GoUp => self.update(Message::Up(pane)),
            KeyboardShortcut::GoBack => self.update(Message::Back(pane)),
            KeyboardShortcut::GoForward => self.update(Message::Forward(pane)),
            KeyboardShortcut::Open => self.open_selected(pane),
        }
    }

    pub(in crate::iced_ui) fn open_selected(&mut self, pane: PaneId) -> Task<Message> {
        self.focus_pane(pane);
        let selected_index = self
            .pane(pane)
            .selection_anchor
            .filter(|index| {
                self.pane(pane)
                    .entries
                    .get(*index)
                    .is_some_and(|entry| self.pane(pane).selected.contains(&entry.path))
            })
            .or_else(|| {
                self.pane(pane)
                    .entries
                    .iter()
                    .position(|entry| self.pane(pane).selected.contains(&entry.path))
            });
        let Some(index) = selected_index else {
            self.pane_mut(pane).status = "No hay ningún elemento seleccionado".into();
            return Task::none();
        };
        self.context_open(pane, ContextTarget::Entry(index))
    }

    pub(in crate::iced_ui) fn delete_selection(
        &mut self,
        pane: PaneId,
        permanent: bool,
    ) -> Task<Message> {
        self.focus_pane(pane);
        self.context_delete(pane, ContextTarget::Background, permanent)
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
        let Some(path) = self
            .pane(pane)
            .entries
            .get(index)
            .map(|entry| entry.path.clone())
        else {
            return Task::none();
        };

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
            self.last_entry_click = None;
            return Task::batch([commit_task, self.queue_selected_preview(pane)]);
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
        let mut tasks = vec![commit_task, self.queue_selected_preview(pane)];
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
        let Some(drag) = self.file_drag.as_mut() else {
            return;
        };
        if drag.dragging {
            return;
        }
        if pane != drag.source_pane {
            drag.dragging = true;
            return;
        }
        let Some(start) = drag.start_pane_point else {
            drag.start_pane_point = Some(point);
            return;
        };
        if pointer_moved_beyond(start, point, FILE_DRAG_START_THRESHOLD) {
            drag.dragging = true;
        }
    }

    pub(in crate::iced_ui) fn update_file_drag_cursor_position(&mut self, position: Point) {
        let Some(drag) = self.file_drag.as_mut() else {
            return;
        };
        let Some(start) = drag.start_cursor else {
            drag.start_cursor = Some(position);
            return;
        };
        if !drag.dragging && pointer_moved_beyond(start, position, FILE_DRAG_START_THRESHOLD) {
            drag.dragging = true;
        }
    }

    pub(in crate::iced_ui) fn set_file_drag_target(&mut self, pane: PaneId, index: usize) {
        let is_container = self
            .pane(pane)
            .entries
            .get(index)
            .is_some_and(|entry| entry.kind.is_container());
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
        let mut y = DETAIL_HEADER_HEIGHT - self.pane(pane).scroll_offset_y;
        let width = self.file_surface_width(pane);
        let mut selected = Vec::new();
        let render_limit = self.pane(pane).render_limit;
        for index in self.filtered_entries(pane).into_iter().take(render_limit) {
            let Some(entry) = self.pane(pane).entries.get(index) else {
                continue;
            };
            if group_mode != GroupMode::None {
                let group = entry_group_label(entry, group_mode);
                if current_group.as_ref() != Some(&group) {
                    current_group = Some(group);
                    y += DETAIL_GROUP_HEIGHT;
                }
            }
            let row_rect = Rectangle {
                x: 0.0,
                y,
                width,
                height: DETAIL_ROW_HEIGHT,
            };
            if rects_intersect(rect, row_rect) {
                selected.push(index);
            }
            y += DETAIL_ROW_HEIGHT;
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
        let mut y = if group_mode == GroupMode::None {
            metrics.grid_padding
        } else {
            0.0
        } - self.pane(pane).scroll_offset_y;
        let mut col = 0_usize;
        let mut current_group: Option<String> = None;

        let render_limit = self.pane(pane).render_limit;
        for index in self.filtered_entries(pane).into_iter().take(render_limit) {
            let Some(entry) = self.pane(pane).entries.get(index) else {
                continue;
            };
            if group_mode != GroupMode::None {
                let group = entry_group_label(entry, group_mode);
                if current_group.as_ref() != Some(&group) {
                    if col > 0 {
                        y += metrics.cell_height + metrics.spacing;
                        col = 0;
                    }
                    current_group = Some(group);
                    y += DETAIL_GROUP_HEIGHT + metrics.spacing;
                }
            }

            let x = metrics.grid_padding + col as f32 * (metrics.cell_width + metrics.spacing);
            let item_rect = Rectangle {
                x,
                y,
                width: metrics.cell_width,
                height: metrics.cell_height,
            };
            if rects_intersect(rect, item_rect) {
                selected.push(index);
            }

            col += 1;
            if col >= layout.columns {
                col = 0;
                y += metrics.cell_height + metrics.spacing;
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
        let pane_sidebar_width = self
            .uses_split_sidebars()
            .then_some(sidebar_width)
            .unwrap_or(0.0);
        let preview_width = self
            .preview_panel_visible(pane)
            .then(|| {
                self.current_preview_panel_width()
                    + SIDEBAR_RESIZE_HANDLE_WIDTH * self.preview_panel_progress.clamp(0.0, 1.0)
            })
            .unwrap_or(0.0);
        (pane_width - pane_sidebar_width - preview_width).max(1.0)
    }

    pub(in crate::iced_ui) fn current_sidebar_width(&self) -> f32 {
        let configured_width = self
            .config
            .sidebar_width
            .clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH);
        configured_width * self.sidebar_progress.clamp(0.0, 1.0)
    }

    pub(in crate::iced_ui) fn uses_split_sidebars(&self) -> bool {
        self.split.is_some() && self.config.show_split_pane_menus
    }

    pub(in crate::iced_ui) fn uses_split_preview_panels(&self) -> bool {
        self.split.is_some() && self.config.show_split_preview_panels
    }

    pub(in crate::iced_ui) fn preview_enabled_for_pane(&self, pane: PaneId) -> bool {
        self.config.show_preview_panel
            && (self.uses_split_preview_panels() || self.preview_panel_pane == Some(pane))
    }

    pub(in crate::iced_ui) fn sidebar_animation_active(&self) -> bool {
        let target = if self.sidebar_visible { 1.0 } else { 0.0 };
        (self.sidebar_progress - target).abs() > f32::EPSILON
    }

    pub(in crate::iced_ui) fn sidebar_is_rendered(&self) -> bool {
        self.sidebar_progress > 0.001
    }

    pub(in crate::iced_ui) fn preview_panel_animation_active(&self) -> bool {
        let target = self.preview_panel_animation_target();
        (self.preview_panel_progress - target).abs() > f32::EPSILON
    }

    pub(in crate::iced_ui) fn popup_fade_animation_active(&self) -> bool {
        self.popup_fade_progress < 0.999 || self.color_picker_fade_progress < 0.999
    }

    pub(in crate::iced_ui) fn scrollbar_animation_active(&self) -> bool {
        let now = Instant::now();
        [PaneId::Primary, PaneId::Secondary]
            .into_iter()
            .any(|pane| {
                let state = self.pane(pane);
                let wheel_reveal = state
                    .scrollbar_reveal_until
                    .is_some_and(|until| until > now);
                let target = f32::from(
                    state.scrollbar_horizontal_hovered
                        || state.scrollbar_vertical_hovered
                        || wheel_reveal,
                );
                wheel_reveal || (state.scrollbar_reveal_progress - target).abs() > f32::EPSILON
            })
    }

    pub(in crate::iced_ui) fn preview_panel_animation_target(&self) -> f32 {
        if self
            .preview_panel_target_pane
            .is_some_and(|pane| self.preview_panel_pane != Some(pane))
        {
            0.0
        } else if self.config.show_preview_panel {
            1.0
        } else {
            0.0
        }
    }

    pub(in crate::iced_ui) fn current_preview_panel_width(&self) -> f32 {
        self.config.preview_panel_width.clamp(220.0, 560.0)
            * self.preview_panel_progress.clamp(0.0, 1.0)
    }

    pub(in crate::iced_ui) fn pointer_tracking_active(&self) -> bool {
        self.sidebar_pointer_inside
            || self.resize_drag.is_some()
            || self.tab_drag.is_some()
            || self.sidebar_section_drag.is_some()
            || self.rubber_band.is_some()
            || self.file_drag.is_some()
    }

    pub(in crate::iced_ui) fn sidebar_section_expanded(&self, section: SidebarSection) -> bool {
        !self.config.sidebar_collapsed.contains(&section)
    }

    pub(in crate::iced_ui) fn toggle_sidebar_section(&mut self, section: SidebarSection) {
        if let Some(index) = self
            .config
            .sidebar_collapsed
            .iter()
            .position(|candidate| *candidate == section)
        {
            self.config.sidebar_collapsed.remove(index);
        } else {
            self.config.sidebar_collapsed.push(section);
        }
        save_config(&self.config);
    }

    pub(in crate::iced_ui) fn reorder_sidebar_section(
        &mut self,
        dragged: SidebarSection,
        target: SidebarSection,
        after: bool,
    ) -> Option<(usize, f32)> {
        let order = self.config.normalized_sidebar_order();
        let old_top = self.sidebar_section_top(&order, dragged);
        let new_order = sidebar_order_with_reorder(order.clone(), dragged, target, after);
        if new_order == order {
            return None;
        }
        let new_top = self.sidebar_section_top(&new_order, dragged);
        let new_slot = new_order
            .iter()
            .position(|section| *section == dragged)
            .unwrap_or(0);
        self.config.sidebar_order = new_order;
        Some((new_slot, new_top - old_top))
    }

    pub(in crate::iced_ui) fn sidebar_section_top(
        &self,
        order: &[SidebarSection],
        section: SidebarSection,
    ) -> f32 {
        let mut top = 0.0;
        for candidate in order {
            if *candidate == section {
                break;
            }
            top += self.sidebar_section_layout_height(*candidate) + 1.0;
        }
        top
    }

    pub(in crate::iced_ui) fn sidebar_section_layout_height(&self, section: SidebarSection) -> f32 {
        let item_count = if self.sidebar_section_expanded(section) {
            sidebar_items_for_section(
                &self.config,
                &self.sidebar_storage_entries,
                section,
                self.is_spanish(),
            )
            .len() as f32
        } else {
            0.0
        };
        SIDEBAR_SECTION_HEIGHT + item_count * SIDEBAR_ITEM_HEIGHT + item_count.max(0.0)
    }

    pub(in crate::iced_ui) fn handle_pointer_moved(&mut self, position: Point) {
        self.cursor_position = position;
        self.update_tab_drag_position(position);
        self.update_sidebar_section_drag_position(position);
        self.update_file_drag_cursor_position(position);

        let Some(drag) = self.resize_drag else {
            return;
        };

        match drag {
            ResizeDrag::Sidebar {
                start_x,
                start_width,
            } => {
                if !start_x.is_finite() {
                    self.resize_drag = Some(ResizeDrag::Sidebar {
                        start_x: position.x,
                        start_width,
                    });
                    return;
                }
                let width = (start_width + position.x - start_x)
                    .clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH);
                self.config.sidebar_width = width;
                self.config.sidebar_visible = true;
                self.sidebar_visible = true;
                self.sidebar_progress = 1.0;
            }
            ResizeDrag::Split {
                start_x,
                start_ratio,
            } => {
                if !start_x.is_finite() {
                    self.resize_drag = Some(ResizeDrag::Split {
                        start_x: position.x,
                        start_ratio,
                    });
                    return;
                }
                let sidebar_width = self.current_sidebar_width();
                let content_width = (self.window_size.width - sidebar_width).max(1.0);
                let available = (content_width - SPLIT_DIVIDER_WIDTH).max(1.0);
                let ratio = (start_ratio + (position.x - start_x) / available)
                    .clamp(SPLIT_MIN_RATIO, SPLIT_MAX_RATIO);
                if let Some(split) = &mut self.split {
                    split.ratio = ratio;
                }
            }
            ResizeDrag::Column {
                pane,
                column,
                start_x,
                start_width,
            } => {
                if !start_x.is_finite() {
                    self.resize_drag = Some(ResizeDrag::Column {
                        pane,
                        column,
                        start_x: position.x,
                        start_width,
                    });
                    return;
                }
                let width = clamp_detail_column_width(column, start_width + position.x - start_x);
                self.pane_mut(pane).column_widths.set(column, width);
            }
            ResizeDrag::Preview {
                pane,
                start_x,
                start_width,
            } => {
                if !start_x.is_finite() {
                    self.resize_drag = Some(ResizeDrag::Preview {
                        pane,
                        start_x: position.x,
                        start_width,
                    });
                    return;
                }
                self.config.preview_panel_width =
                    (start_width - (position.x - start_x)).clamp(220.0, 560.0);
            }
        }
    }

    pub(in crate::iced_ui) fn update_tab_drag_position(&mut self, position: Point) {
        let Some(mut drag) = self.tab_drag else {
            return;
        };
        if !drag.start_cursor_x.is_finite() {
            drag.start_cursor_x = position.x;
            drag.start_cursor_y = position.y;
            self.tab_drag = Some(drag);
            return;
        }

        let delta_x = position.x - drag.start_cursor_x;
        let delta_y = position.y - drag.start_cursor_y;
        drag.offset_x = delta_x.clamp(-TAB_DRAG_MAX_OFFSET, TAB_DRAG_MAX_OFFSET);
        if delta_x.abs().max(delta_y.abs()) >= TAB_DRAG_START_THRESHOLD {
            drag.dragging = true;
        }
        let dragging = drag.dragging;
        self.tab_drag = Some(drag);

        if dragging && !self.is_tab_split_drop_position(position.y) {
            self.reorder_tab_drag_at_position(position.x);
        }
    }

    pub(in crate::iced_ui) fn is_tab_split_drop_position(&self, y: f32) -> bool {
        y >= TAB_SPLIT_DROP_TRIGGER_Y
    }

    pub(in crate::iced_ui) fn tab_split_drop_side(&self, position: Point) -> Option<SplitSide> {
        self.is_tab_split_drop_position(position.y).then_some(
            if position.x < self.window_size.width * 0.5 {
                SplitSide::Left
            } else {
                SplitSide::Right
            },
        )
    }

    pub(in crate::iced_ui) fn place_tab_in_split(
        &mut self,
        tab_index: usize,
        side: SplitSide,
    ) -> Task<Message> {
        if self.split.is_some() || self.tabs.len() < 2 || tab_index >= self.tabs.len() {
            return Task::none();
        }

        let remaining_tabs = (0..self.tabs.len())
            .filter(|index| *index != tab_index)
            .collect::<Vec<_>>();
        let fallback = remaining_tabs.first().copied().unwrap_or(0);
        let prior_active = self.active_tab;
        let primary_active = if prior_active == tab_index {
            fallback
        } else {
            prior_active
        };

        let (primary_tabs, secondary_tabs, active_tab, secondary_tab, focused) = match side {
            SplitSide::Left => (
                vec![tab_index],
                remaining_tabs,
                tab_index,
                primary_active,
                SplitFocus::Primary,
            ),
            SplitSide::Right => (
                remaining_tabs,
                vec![tab_index],
                primary_active,
                tab_index,
                SplitFocus::Secondary,
            ),
        };

        self.active_tab = active_tab;
        self.split = Some(SplitRuntime {
            primary_tabs,
            secondary_tabs,
            secondary_tab,
            focused,
            ratio: 0.5,
        });
        self.focus_pane(match side {
            SplitSide::Left => PaneId::Primary,
            SplitSide::Right => PaneId::Secondary,
        });
        self.save_session();

        Task::batch([
            self.start_load(PaneId::Primary),
            self.start_load(PaneId::Secondary),
        ])
    }

    pub(in crate::iced_ui) fn reorder_tab_drag_at_position(&mut self, x: f32) {
        let Some(drag) = self.tab_drag else {
            return;
        };
        let Some(insertion_slot) = self.tab_insertion_slot_at(drag.pane, x) else {
            return;
        };
        let old_slot = drag.slot;
        let split_none = self.split.is_none();
        if let Some(new_slot) = self.reorder_dragged_tab(drag.pane, drag.tab_index, insertion_slot)
        {
            let slot_delta = new_slot as f32 - old_slot as f32;
            if let Some(tab_drag) = &mut self.tab_drag {
                tab_drag.slot = new_slot;
                if split_none {
                    tab_drag.tab_index = new_slot;
                }
                if slot_delta != 0.0 {
                    tab_drag.start_cursor_x += slot_delta * TAB_DRAG_STRIDE;
                    tab_drag.offset_x = (self.cursor_position.x - tab_drag.start_cursor_x)
                        .clamp(-TAB_DRAG_MAX_OFFSET, TAB_DRAG_MAX_OFFSET);
                    tab_drag.dirty = true;
                }
            }
        }
    }

    pub(in crate::iced_ui) fn tab_insertion_slot_at(&self, pane: PaneId, x: f32) -> Option<usize> {
        let (start, width) = self.title_pane_bounds(pane);
        let count = self.tab_indices_for_pane(pane).len();
        if count <= 1 || width <= 0.0 || x < start - TAB_WIDTH || x > start + width + TAB_WIDTH {
            return None;
        }

        let local_x = (x - start).max(0.0);
        let slot = (local_x / TAB_DRAG_STRIDE).floor().max(0.0) as usize;
        let within_slot = local_x - slot as f32 * TAB_DRAG_STRIDE;
        let insertion_slot = slot + usize::from(within_slot > TAB_WIDTH * 0.5);
        Some(insertion_slot.min(count))
    }

    pub(in crate::iced_ui) fn title_pane_bounds(&self, pane: PaneId) -> (f32, f32) {
        let start = self.title_tabs_start_x();
        let area_width = (self.window_size.width - start).max(1.0);
        if let Some(split) = &self.split {
            let available = (area_width - SPLIT_DIVIDER_WIDTH).max(1.0);
            let primary_width = available * split.ratio;
            match pane {
                PaneId::Primary => (start, primary_width),
                PaneId::Secondary => (
                    start + primary_width + SPLIT_DIVIDER_WIDTH,
                    available - primary_width,
                ),
            }
        } else {
            (start, area_width)
        }
    }

    pub(in crate::iced_ui) fn title_tabs_start_x(&self) -> f32 {
        let left_buttons = TITLE_BUTTON_WIDTH * 2.0 + TITLE_BUTTON_GAP;
        if self.sidebar_is_rendered() {
            self.current_sidebar_width().max(left_buttons)
        } else {
            left_buttons + TITLE_TAB_START_PADDING
        }
    }

    pub(in crate::iced_ui) fn update_sidebar_section_drag_position(&mut self, position: Point) {
        let Some(mut drag) = self.sidebar_section_drag else {
            return;
        };
        if !drag.start_cursor_y.is_finite() {
            drag.start_cursor_y = position.y;
            self.sidebar_section_drag = Some(drag);
            return;
        }

        let delta = position.y - drag.start_cursor_y;
        drag.offset_y = delta;
        if delta.abs() >= SIDEBAR_SECTION_DRAG_START_THRESHOLD {
            drag.dragging = true;
        }
        let dragging = drag.dragging;
        self.sidebar_section_drag = Some(drag);

        if dragging {
            self.reorder_sidebar_section_at_position(position.y);
        }
    }

    pub(in crate::iced_ui) fn reorder_sidebar_section_at_position(&mut self, y: f32) {
        let Some(drag) = self.sidebar_section_drag else {
            return;
        };
        let Some((target, after)) = self.sidebar_section_target_at_y(y) else {
            return;
        };
        if drag.section == target {
            return;
        }

        if let Some((new_slot, top_delta)) =
            self.reorder_sidebar_section(drag.section, target, after)
        {
            if let Some(section_drag) = &mut self.sidebar_section_drag {
                section_drag.slot = new_slot;
                if top_delta != 0.0 {
                    section_drag.start_cursor_y += top_delta;
                    section_drag.offset_y = self.cursor_position.y - section_drag.start_cursor_y;
                    section_drag.dirty = true;
                }
            }
        }
    }

    pub(in crate::iced_ui) fn sidebar_section_target_at_y(
        &self,
        y: f32,
    ) -> Option<(SidebarSection, bool)> {
        if !self.sidebar_is_rendered() {
            return None;
        }

        let mut top = TITLE_HEIGHT + 8.0;
        for section in self.config.normalized_sidebar_order() {
            let height = self.sidebar_section_layout_height(section);
            if y >= top && y <= top + height {
                return Some((section, y > top + height * 0.5));
            }
            top += height + 1.0;
        }
        None
    }
}
