use super::*;

impl BExplorerIced {
    pub(in crate::iced_ui) fn queue_open_with_application_icons(
        &mut self,
        pane: PaneId,
        target: ContextTarget,
    ) -> Task<Message> {
        let Some(entry) = self.context_entry(pane, target) else {
            return Task::none();
        };
        let applications = shell::open_with_applications(&entry.path).unwrap_or_default();
        let mut tasks = Vec::new();
        for application in applications {
            let Some(icon_path) = application.icon_path else {
                continue;
            };
            let key = thumbnail_data::native_path_icon_cache_key(
                &icon_path,
                false,
                thumbnail_data::NATIVE_ICON_SIZE,
            );
            if self.native_icon_cache.contains_key(&key) {
                continue;
            }
            self.native_icon_cache
                .insert(key.clone(), IcedImageState::Loading);
            tasks.push(load_iced_image_task(IcedImageJob::NativeIcon {
                cache_key: key,
                path: icon_path,
                is_directory: false,
                size: thumbnail_data::NATIVE_ICON_SIZE,
            }));
        }
        Task::batch(tasks)
    }

    pub(in crate::iced_ui) fn request_context_menu(
        &mut self,
        pane: PaneId,
        target: ContextTarget,
    ) -> Task<Message> {
        self.begin_popup_animation(false);
        self.focus_pane(pane);
        self.title_menu_open = false;
        self.view_menu_open = None;
        self.group_menu_open = None;
        self.new_menu_open = None;
        self.context_archive_submenu = false;
        self.context_open_with_submenu = false;
        self.context_open_with_parent_hovered = false;
        self.context_open_with_submenu_hovered = false;
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
            ContextSubmenuKind::OpenWith => {
                let mut labels = self
                    .context_entry(menu.pane, menu.target)
                    .and_then(|entry| shell::open_with_applications(&entry.path).ok())
                    .unwrap_or_default()
                    .into_iter()
                    .map(|application| application.name)
                    .collect::<Vec<_>>();
                labels.push(
                    self.localized("Elegir otra aplicación…", "Choose another app…")
                        .into(),
                );
                labels
            }
        };
        let width = view::context_submenu_width(&labels);
        let height = match kind {
            ContextSubmenuKind::Archive => 114.0,
            ContextSubmenuKind::Extract | ContextSubmenuKind::New => 78.0,
            ContextSubmenuKind::OpenWith => (labels.len() as f32 * 36.0 + 46.0).min(320.0),
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
            ContextSubmenuKind::OpenWith => 42.0,
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
            self.begin_popup_animation(true);
            self.color_picker_backdrop = None;
        } else {
            self.begin_popup_animation(false);
            self.popup_backdrop = None;
        }
        let Some(id) = self.main_window_id else {
            return self.show_popup_with_backdrop(target);
        };
        window::screenshot(id)
            .map(move |screenshot| Message::PopupBackdropCaptured(target.clone(), screenshot))
    }

    pub(in crate::iced_ui) fn begin_popup_animation(&mut self, color_picker: bool) {
        self.pending_popup_close = None;
        self.last_animation_frame = None;
        if color_picker {
            self.color_picker_fade_progress = 0.0;
            self.color_picker_fade_target = 1.0;
        } else {
            self.popup_fade_progress = 0.0;
            self.popup_fade_target = 1.0;
        }
    }

    pub(in crate::iced_ui) fn request_popup_close(
        &mut self,
        pending: PendingPopupClose,
    ) -> Task<Message> {
        self.pending_popup_close = Some(pending);
        self.last_animation_frame = None;
        let already_hidden = if pending == PendingPopupClose::ColorPicker {
            self.color_picker_fade_target = 0.0;
            self.color_picker_fade_progress <= 0.002
        } else {
            self.popup_fade_target = 0.0;
            self.popup_fade_progress <= 0.002
        };
        if already_hidden {
            self.finish_pending_popup_close();
        }
        Task::none()
    }

    pub(in crate::iced_ui) fn finish_pending_popup_close(&mut self) {
        let Some(pending) = self.pending_popup_close.take() else {
            return;
        };
        match pending {
            PendingPopupClose::FloatingMenus => {
                self.title_menu_open = false;
                self.show_menu_open = false;
                self.show_menu_parent_hovered = false;
                self.show_menu_submenu_hovered = false;
                self.view_menu_open = None;
                self.group_menu_open = None;
                self.search_mode_menu_open = None;
                self.new_menu_open = None;
                self.title_submenu_backdrop = None;
                self.popup_backdrop = None;
            }
            PendingPopupClose::Shortcuts => {
                self.shortcuts_open = false;
                self.shortcut_capture = None;
                self.popup_backdrop = None;
            }
            PendingPopupClose::Settings => {
                self.settings_open = false;
                self.popup_backdrop = None;
                self.color_picker_backdrop = None;
                self.color_picker_open = false;
                self.accent_plane_dragging = false;
                self.accent_hue_dragging = false;
            }
            PendingPopupClose::ColorPicker => {
                self.color_picker_open = false;
                self.color_picker_backdrop = None;
                self.accent_plane_dragging = false;
                self.accent_hue_dragging = false;
            }
            PendingPopupClose::ArchiveDialog => {
                self.archive_dialog = None;
                self.popup_backdrop = None;
            }
            PendingPopupClose::FormatDialog => {
                self.format_dialog = None;
                self.popup_backdrop = None;
            }
            PendingPopupClose::ErrorDialog => {
                self.error_dialog = None;
                self.popup_backdrop = None;
            }
            PendingPopupClose::PermanentDelete => {
                self.permanent_delete_dialog = None;
                self.popup_backdrop = None;
            }
            PendingPopupClose::TransferConflict => {
                self.transfer_conflict_dialog = None;
                self.popup_backdrop = None;
            }
        }
    }

    pub(in crate::iced_ui) fn dismiss_context_menu(&mut self) {
        self.context_menu = None;
        self.context_archive_submenu = false;
        self.context_open_with_submenu = false;
        self.context_open_with_parent_hovered = false;
        self.context_open_with_submenu_hovered = false;
        self.context_extract_submenu = false;
        self.context_new_submenu = false;
        self.context_archive_parent_hovered = false;
        self.context_archive_submenu_hovered = false;
        self.context_new_parent_hovered = false;
        self.context_new_submenu_hovered = false;
        self.popup_fade_progress = 0.0;
        self.popup_fade_target = 0.0;
        self.pending_popup_close = None;
        self.last_animation_frame = None;
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
            PopupBackdropTarget::Format(dialog) => {
                self.format_dialog = Some(dialog);
            }
            PopupBackdropTarget::Error(dialog) => {
                self.error_dialog = Some(dialog);
            }
            PopupBackdropTarget::TransferConflict(dialog) => {
                self.transfer_conflict_dialog = Some(dialog);
            }
        }
        Task::none()
    }

    pub(in crate::iced_ui) fn show_error_dialog(
        &mut self,
        title: String,
        message: String,
    ) -> Task<Message> {
        self.request_popup_backdrop(PopupBackdropTarget::Error(ErrorDialogState {
            title,
            message,
        }))
    }

    pub(in crate::iced_ui) fn report_error(
        &mut self,
        pane: PaneId,
        message: impl Into<String>,
    ) -> Task<Message> {
        let message = message.into();
        self.pane_mut(pane).status = message.clone();
        self.show_error_dialog(
            self.localized("Se produjo un error", "An error occurred")
                .to_owned(),
            message,
        )
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
            ContextTarget::SidebarDrive(_) => {
                let formatable = self
                    .context_entry(menu.pane, menu.target)
                    .is_some_and(|entry| {
                        entry.kind == EntryKind::Drive
                            && entry.drive_kind.is_some_and(DriveKind::is_formatable)
                    });
                if formatable { 84.0 } else { 48.0 }
            }
            ContextTarget::Entry(_) => {
                let drive_entry = self
                    .context_entry(menu.pane, menu.target)
                    .is_some_and(|entry| entry.kind == EntryKind::Drive);
                if drive_entry {
                    let action_rows = self
                        .context_entry(menu.pane, menu.target)
                        .map(|entry| {
                            usize::from(entry.drive_kind.is_some_and(DriveKind::is_ejectable))
                                + usize::from(
                                    entry.kind == EntryKind::Drive
                                        && entry.drive_kind.is_some_and(DriveKind::is_formatable),
                                )
                        })
                        .unwrap_or(0);
                    return 128.0 + action_rows as f32 * 36.0;
                }
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
                                entry.kind == EntryKind::Drive
                                    && entry.drive_kind.is_some_and(DriveKind::is_formatable),
                            )
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
            let pane_sidebar_width = if self.uses_split_sidebars() {
                sidebar_width
            } else {
                0.0
            };
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
        let target_is_drive = self
            .context_entry(menu.pane, menu.target)
            .is_some_and(|entry| entry.kind == EntryKind::Drive);
        if target_is_drive
            && matches!(
                command,
                ContextCommand::CompressMenu
                    | ContextCommand::CompressDialog
                    | ContextCommand::CompressDefault(_)
                    | ContextCommand::Copy
                    | ContextCommand::Cut
                    | ContextCommand::Delete
                    | ContextCommand::DeletePermanent
            )
        {
            return Task::none();
        }
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
        if command == ContextCommand::OpenWithMenu {
            self.context_open_with_submenu = true;
            self.context_archive_submenu = false;
            return self.request_context_submenu_backdrop(ContextSubmenuKind::OpenWith);
        }
        if command == ContextCommand::NewMenu {
            self.context_new_submenu = true;
            return self.request_context_submenu_backdrop(ContextSubmenuKind::New);
        }
        self.dismiss_context_menu();
        match command {
            ContextCommand::Paste => self.context_paste(menu.pane, menu.target),
            ContextCommand::Copy => self.context_copy(menu.pane, menu.target, false),
            ContextCommand::Cut => self.context_copy(menu.pane, menu.target, true),
            ContextCommand::Refresh => self.start_load(menu.pane),
            ContextCommand::NewMenu => Task::none(),
            ContextCommand::NewFolder => self.update(Message::NewFolder(menu.pane)),
            ContextCommand::NewTextDocument => self.update(Message::NewTextDocument(menu.pane)),
            ContextCommand::OpenTerminal => self.context_open_terminal(menu.pane, menu.target),
            ContextCommand::Properties => self.context_properties(menu.pane, menu.target),
            ContextCommand::Open => self.context_open(menu.pane, menu.target),
            ContextCommand::OpenWith => self.context_open_with(menu.pane, menu.target),
            ContextCommand::OpenWithMenu => Task::none(),
            ContextCommand::OpenWithApplication(index) => {
                let Some(entry) = self.context_entry(menu.pane, menu.target) else {
                    return Task::none();
                };
                let path = entry.path.clone();
                match shell::open_with_application(&path, index) {
                    Ok(()) => self.pane_mut(menu.pane).status = "Aplicación abierta".into(),
                    Err(error) => return self.report_error(menu.pane, error.to_string()),
                }
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
            ContextCommand::FormatDrive => self.context_format_drive(menu.pane, menu.target),
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
}
