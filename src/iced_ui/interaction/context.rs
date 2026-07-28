use super::*;

impl BExplorerIced {
    pub(in crate::iced_ui) fn queue_open_with_application_icons(
        &mut self,
        pane: PaneId,
        target: ContextTarget,
    ) -> Task<Message> {
        let applications = self
            .context_menu
            .as_ref()
            .filter(|menu| menu.pane == pane && menu.target == target)
            .map(|menu| menu.open_with_applications.clone())
            .unwrap_or_default();
        let mut tasks = Vec::new();
        for application in applications {
            let Some(key) = open_with_application_icon_cache_key(
                &application,
                thumbnail_data::NATIVE_ICON_SIZE,
            ) else {
                continue;
            };
            if self.native_icon_cache.contains_key(&key) {
                continue;
            }
            self.native_icon_cache
                .insert(key.clone(), IcedImageState::Loading);
            tasks.push(load_iced_image_task(IcedImageJob::ApplicationIcon {
                cache_key: key,
                application,
                size: thumbnail_data::NATIVE_ICON_SIZE,
            }));
        }
        Task::batch(tasks)
    }

    pub(in crate::iced_ui) fn queue_send_to_target_icons(
        &mut self,
        targets: &[ContextSendToTarget],
    ) -> Task<Message> {
        let mut tasks = Vec::new();
        for target in targets {
            match target {
                ContextSendToTarget::Storage { destination, .. } => {
                    tasks.push(self.queue_sidebar_path_icon(destination));
                }
                ContextSendToTarget::Native(target) => {
                    let Some((cache_key, path, is_directory)) =
                        native_send_to_icon_request(target, thumbnail_data::SMALL_ENTRY_IMAGE_SIZE)
                    else {
                        continue;
                    };
                    if self.small_native_icon_cache.contains_key(&cache_key) {
                        continue;
                    }
                    self.small_native_icon_cache
                        .insert(cache_key.clone(), IcedImageState::Loading);
                    tasks.push(load_iced_image_task(IcedImageJob::NativeIcon {
                        cache_key,
                        path,
                        is_directory,
                        size: thumbnail_data::SMALL_ENTRY_IMAGE_SIZE,
                        variant: IcedImageVariant::Small,
                    }));
                }
            }
        }
        Task::batch(tasks)
    }

    pub(in crate::iced_ui) fn queue_current_send_to_target_icons(&mut self) -> Task<Message> {
        let targets = self
            .context_menu
            .as_ref()
            .map(|menu| menu.send_to_targets.clone())
            .unwrap_or_default();
        self.queue_send_to_target_icons(&targets)
    }

    pub(in crate::iced_ui) fn context_send_to_icon_handle(
        &self,
        target: &ContextSendToTarget,
    ) -> Option<iced_image::Handle> {
        let cache_key = match target {
            ContextSendToTarget::Storage { destination, .. } => sidebar_native_icon_cache_key(
                destination,
                &self.sidebar_storage_entries,
                thumbnail_data::SMALL_ENTRY_IMAGE_SIZE,
            ),
            ContextSendToTarget::Native(target) => {
                native_send_to_icon_request(target, thumbnail_data::SMALL_ENTRY_IMAGE_SIZE)?.0
            }
        };
        match self.small_native_icon_cache.get(&cache_key) {
            Some(IcedImageState::Ready(handle)) => Some(handle.clone()),
            _ => None,
        }
    }

    pub(in crate::iced_ui) fn request_context_menu(
        &mut self,
        pane: PaneId,
        target: ContextTarget,
    ) -> Task<Message> {
        self.begin_popup_animation(false);
        self.keyboard_menu_selection = None;
        self.focus_pane(pane);
        self.title_menu_open = false;
        self.view_menu_open = None;
        self.group_menu_open = None;
        self.new_menu_open = None;
        self.context_archive_submenu = false;
        self.context_open_with_submenu = false;
        self.context_open_with_parent_hovered = false;
        self.context_open_with_submenu_hovered = false;
        self.context_send_to_submenu = false;
        self.context_send_to_parent_hovered = false;
        self.context_send_to_submenu_hovered = false;
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
        let send_paths = self
            .context_entry(pane, target)
            .filter(|entry| entry.kind != EntryKind::Drive)
            .map(|_| self.context_paths(pane, target))
            .unwrap_or_default();
        let send_to_targets = self.context_storage_send_to_targets(&send_paths);
        let send_to_icon_tasks = self.queue_send_to_target_icons(&send_to_targets);
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
            open_with_applications: Vec::new(),
            send_to_targets,
        };
        let (x, y) = self.context_menu_window_position(&menu);
        let menu = ContextMenuState {
            backdrop_origin: Point::new(x, y),
            ..menu
        };
        if matches!(target, ContextTarget::SidebarDrive(_)) || self.is_trash_pane(pane) {
            return Task::batch([self.capture_context_menu_backdrop(menu), send_to_icon_tasks]);
        }
        let local_paste_available = self
            .file_clipboard
            .as_ref()
            .is_some_and(|clipboard| !clipboard.paths.is_empty());
        let open_with_path = self
            .context_entry(pane, target)
            .filter(|entry| entry.kind != EntryKind::Drive)
            .map(|entry| entry.path);
        let native_send_paths = send_paths;
        let spanish = self.is_spanish();
        let menu_data = Task::perform(
            async move {
                run_blocking_file_operation(move || {
                    let native_paste_available =
                        shell::read_files().is_ok_and(|clipboard| !clipboard.paths.is_empty());
                    let applications = open_with_path
                        .as_deref()
                        .and_then(|path| shell::open_with_applications(path).ok())
                        .unwrap_or_default();
                    let native_send_to_targets =
                        shell::native_send_to_targets(&native_send_paths, spanish);
                    Ok::<_, BExplorerError>((
                        local_paste_available || native_paste_available,
                        applications,
                        native_send_to_targets,
                    ))
                })
                .await
                .unwrap_or_else(|_| (local_paste_available, Vec::new(), Vec::new()))
            },
            move |(available, applications, send_to_targets)| {
                Message::ContextMenuDataResolved(
                    menu.clone(),
                    available,
                    applications,
                    send_to_targets,
                )
            },
        );
        Task::batch([menu_data, send_to_icon_tasks])
    }

    fn context_storage_send_to_targets(&self, sources: &[PathBuf]) -> Vec<ContextSendToTarget> {
        if sources.is_empty() {
            return Vec::new();
        }
        let has_virtual_portable_source = sources
            .iter()
            .any(|source| explorer::is_portable_path(source));
        let spanish = self.is_spanish();
        self.sidebar_storage_entries
            .iter()
            .filter(|entry| {
                matches!(
                    entry.drive_kind,
                    Some(DriveKind::External | DriveKind::Usb | DriveKind::Portable)
                )
            })
            // The transfer backend deliberately rejects direct MTP-to-MTP
            // copies. Mounted GVfs devices are ordinary paths and do not hit
            // this condition.
            .filter(|entry| {
                !(has_virtual_portable_source && entry.drive_kind == Some(DriveKind::Portable))
            })
            .map(|entry| ContextSendToTarget::Storage {
                label: send_to_storage_label(&entry.name, &entry.path, entry.drive_kind, spanish),
                destination: entry.path.clone(),
                icon: send_to_storage_icon(entry.drive_kind),
            })
            .collect()
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
                let mut labels = menu
                    .open_with_applications
                    .iter()
                    .map(|application| application.name.clone())
                    .collect::<Vec<_>>();
                labels.push(
                    self.localized("Elegir otra aplicación…", "Choose another app…")
                        .into(),
                );
                labels
            }
            ContextSubmenuKind::SendTo => menu
                .send_to_targets
                .iter()
                .map(|target| target.label().to_owned())
                .collect(),
        };
        let font_size = self.font_size();
        let width = view::context_submenu_width(&labels, font_size);
        let height = match kind {
            ContextSubmenuKind::Archive => context_submenu_rows_height(3, font_size),
            ContextSubmenuKind::Extract | ContextSubmenuKind::New => {
                context_submenu_rows_height(2, font_size)
            }
            ContextSubmenuKind::OpenWith | ContextSubmenuKind::SendTo => {
                context_submenu_rows_height(labels.len(), font_size)
                    .min(scaled_ui_metric(320.0, font_size))
            }
        };
        let (x, y) = self.context_menu_window_position(menu);
        let menu_width = context_menu_width(font_size);
        let submenu_x = if x + menu_width + width <= self.window_size.width - 8.0 {
            x + menu_width - 6.0
        } else {
            (x - width + 6.0).max(8.0)
        };
        let extra_archive_rows = usize::from(!menu.send_to_targets.is_empty())
            + usize::from(self.pane(menu.pane).folder_entries.is_some());
        let offset_y = match kind {
            ContextSubmenuKind::Archive => {
                context_submenu_parent_offset(true, 2 + extra_archive_rows, 2, font_size)
            }
            ContextSubmenuKind::Extract => {
                context_submenu_parent_offset(true, 3 + extra_archive_rows, 2, font_size)
            }
            ContextSubmenuKind::New => context_submenu_parent_offset(true, 1, 1, font_size),
            ContextSubmenuKind::OpenWith => context_submenu_parent_offset(true, 1, 1, font_size),
            ContextSubmenuKind::SendTo => context_submenu_parent_offset(true, 2, 1, font_size),
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
        // KWin already supplies the live blurred surface. Capturing a second,
        // frozen backdrop for Settings would hide slider updates and removing
        // it on the first movement would cause a large artificial jump.
        if matches!(&target, PopupBackdropTarget::Settings) && self.requests_linux_surface_blur() {
            return self.show_popup_with_backdrop(target);
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
                self.keyboard_menu_selection = None;
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
            PendingPopupClose::About => {
                self.about_open = false;
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
        self.keyboard_menu_selection = None;
        self.context_menu = None;
        self.context_archive_submenu = false;
        self.context_open_with_submenu = false;
        self.context_open_with_parent_hovered = false;
        self.context_open_with_submenu_hovered = false;
        self.context_send_to_submenu = false;
        self.context_send_to_parent_hovered = false;
        self.context_send_to_submenu_hovered = false;
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
            PopupBackdropTarget::About => {
                self.about_open = true;
            }
            PopupBackdropTarget::ColorPicker => {
                self.color_picker_open = true;
            }
            PopupBackdropTarget::Rename(mut dialog) => {
                let select_end = dialog.select_end;
                // `text_editor::Content::clone` intentionally clones only
                // text, not its cursor or selection. Popup backdrop capture
                // clones the target before it is shown, so restore the
                // filename-only selection after that clone has completed.
                select_rename_editor_prefix(&mut dialog.editor, select_end);
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
        let menu_width = context_menu_width(self.font_size());
        let menu_height = self.context_menu_height(menu);

        if matches!(menu.target, ContextTarget::SidebarDrive(_)) {
            return (
                (menu.position.x + 2.0)
                    .clamp(8.0, (self.window_size.width - menu_width - 8.0).max(8.0)),
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
            + self.toolbar_height()
            + if self.split.is_some() { 1.0 } else { 0.0 }
            + if self.config.show_action_bar {
                self.action_bar_height()
            } else {
                0.0
            }
            + if self.config.show_bookmark_bar || !self.sidebar_visible {
                self.bookmark_bar_height()
            } else {
                0.0
            };
        let x = (pane_x + point.x + 2.0)
            .clamp(8.0, (self.window_size.width - menu_width - 8.0).max(8.0));
        let y = (table_y + point.y + 2.0)
            .clamp(8.0, (self.window_size.height - menu_height - 8.0).max(8.0));
        (x, y)
    }

    pub(in crate::iced_ui) fn context_menu_height(&self, menu: &ContextMenuState) -> f32 {
        let row_height = context_menu_row_height(self.font_size());
        let quick_height = context_quick_button_height(self.font_size()) + 12.0;
        let menu_height = |quick: bool, rows: usize, separators: usize| {
            let children = usize::from(quick) + rows + separators;
            8.0 + if quick { quick_height } else { 0.0 }
                + rows as f32 * row_height
                + separators as f32
                + children.saturating_sub(1) as f32 * 2.0
        };
        if self.is_trash_pane(menu.pane) && !matches!(menu.target, ContextTarget::SidebarDrive(_)) {
            return menu_height(false, 2, 0);
        }
        match menu.target {
            ContextTarget::Background => menu_height(true, 4, 2),
            ContextTarget::SidebarDrive(_) => {
                let context_entry = self.context_entry(menu.pane, menu.target);
                let formatable = context_entry.as_ref().is_some_and(|entry| {
                    entry.kind == EntryKind::Drive
                        && entry.drive_kind.is_some_and(DriveKind::is_formatable)
                });
                let ejectable = context_entry
                    .as_ref()
                    .and_then(|entry| entry.drive_kind)
                    .is_some_and(DriveKind::is_ejectable);
                let duplicate_cleanup_available = context_entry.as_ref().is_some_and(
                    crate::iced_ui::duplicate_cleanup::duplicate_cleanup_available_for_entry,
                );
                menu_height(
                    false,
                    usize::from(ejectable)
                        + usize::from(formatable)
                        + usize::from(duplicate_cleanup_available),
                    0,
                )
            }
            ContextTarget::Entry(_) => {
                let context_entry = self.context_entry(menu.pane, menu.target);
                let duplicate_cleanup_available = context_entry.as_ref().is_some_and(
                    crate::iced_ui::duplicate_cleanup::duplicate_cleanup_available_for_entry,
                );
                let drive_entry = context_entry
                    .as_ref()
                    .is_some_and(|entry| entry.kind == EntryKind::Drive);
                if drive_entry {
                    let action_rows = context_entry
                        .as_ref()
                        .map(|entry| {
                            usize::from(entry.drive_kind.is_some_and(DriveKind::is_ejectable))
                                + usize::from(
                                    entry.drive_kind.is_some_and(DriveKind::is_formatable),
                                )
                                + usize::from(duplicate_cleanup_available)
                        })
                        .unwrap_or(0);
                    return menu_height(true, 2 + action_rows, 1);
                }
                let has_extract_action = context_entry.as_ref().is_some_and(|entry| {
                    crate::fs::archive_listing::has_extractable_archive_extension(&entry.path)
                });
                let terminal_available = context_entry.as_ref().is_some_and(|entry| {
                    entry.kind.is_container() && !explorer::is_virtual_path(&entry.path)
                });
                let advanced_rows = context_entry
                    .as_ref()
                    .map(|entry| {
                        usize::from(is_mountable_disk_image_entry(entry))
                            + usize::from(entry.drive_kind.is_some_and(DriveKind::is_ejectable))
                            + usize::from(entry.drive_kind.is_some_and(DriveKind::is_formatable))
                            + usize::from(
                                cfg!(target_os = "windows")
                                    && !explorer::is_virtual_path(&entry.path),
                            )
                    })
                    .unwrap_or(0);
                let rows = 7
                    + usize::from(self.pane(menu.pane).folder_entries.is_some())
                    + usize::from(!menu.send_to_targets.is_empty())
                    + usize::from(has_extract_action)
                    + usize::from(terminal_available)
                    + usize::from(duplicate_cleanup_available)
                    + advanced_rows;
                menu_height(true, rows, 4)
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
                    | ContextCommand::SendToMenu
                    | ContextCommand::SendToTarget(_)
                    | ContextCommand::Delete
                    | ContextCommand::DeletePermanent
            )
        {
            return Task::none();
        }
        if command == ContextCommand::CompressMenu {
            self.keyboard_menu_selection = None;
            self.context_archive_submenu = true;
            self.context_open_with_submenu = false;
            self.context_send_to_submenu = false;
            self.context_extract_submenu = false;
            self.context_new_submenu = false;
            return self.request_context_submenu_backdrop(ContextSubmenuKind::Archive);
        }
        if command == ContextCommand::ExtractMenu {
            self.keyboard_menu_selection = None;
            self.context_archive_submenu = true;
            self.context_open_with_submenu = false;
            self.context_send_to_submenu = false;
            self.context_extract_submenu = true;
            self.context_new_submenu = false;
            return self.request_context_submenu_backdrop(ContextSubmenuKind::Extract);
        }
        if command == ContextCommand::OpenWithMenu {
            self.keyboard_menu_selection = None;
            self.context_open_with_submenu = true;
            self.context_send_to_submenu = false;
            self.context_archive_submenu = false;
            self.context_extract_submenu = false;
            self.context_new_submenu = false;
            return Task::batch([
                self.request_context_submenu_backdrop(ContextSubmenuKind::OpenWith),
                self.queue_open_with_application_icons(menu.pane, menu.target),
            ]);
        }
        if command == ContextCommand::SendToMenu {
            self.keyboard_menu_selection = None;
            self.context_send_to_submenu = true;
            self.context_open_with_submenu = false;
            self.context_archive_submenu = false;
            self.context_extract_submenu = false;
            self.context_new_submenu = false;
            return Task::batch([
                self.request_context_submenu_backdrop(ContextSubmenuKind::SendTo),
                self.queue_current_send_to_target_icons(),
            ]);
        }
        if command == ContextCommand::NewMenu {
            self.keyboard_menu_selection = None;
            self.context_new_submenu = true;
            self.context_open_with_submenu = false;
            self.context_send_to_submenu = false;
            self.context_archive_submenu = false;
            self.context_extract_submenu = false;
            return self.request_context_submenu_backdrop(ContextSubmenuKind::New);
        }
        self.dismiss_context_menu();
        match command {
            ContextCommand::RestoreTrash => {
                let paths = self.context_paths(menu.pane, menu.target);
                self.restore_trash_paths(menu.pane, paths)
            }
            ContextCommand::DeleteTrash => {
                let paths = self.context_paths(menu.pane, menu.target);
                self.request_trash_purge(menu.pane, paths, PermanentDeleteTarget::TrashItems)
            }
            ContextCommand::EmptyTrash => self.request_empty_trash(menu.pane),
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
            ContextCommand::SendToMenu => Task::none(),
            ContextCommand::SendToTarget(index) => {
                let Some(target) = menu.send_to_targets.get(index).cloned() else {
                    return self.report_error(
                        menu.pane,
                        self.localized(
                            "El destino seleccionado ya no está disponible",
                            "The selected destination is no longer available",
                        ),
                    );
                };
                let paths = self.context_paths(menu.pane, menu.target);
                match target {
                    ContextSendToTarget::Storage {
                        label, destination, ..
                    } => {
                        if paths
                            .iter()
                            .any(|path| crate::fs::archive_listing::is_inside_archive(path))
                        {
                            return self.queue_archive_entry_extraction(
                                menu.pane,
                                menu.pane,
                                paths,
                                destination,
                            );
                        }
                        self.pane_mut(menu.pane).status =
                            format!("{} {label}", self.localized("Copiando a", "Copying to"));
                        self.request_transfer(
                            menu.pane,
                            paths,
                            destination,
                            TransferKind::Copy,
                            false,
                        )
                    }
                    ContextSendToTarget::Native(target) => {
                        let label = target.label().to_owned();
                        self.pane_mut(menu.pane).status =
                            format!("{} {label}…", self.localized("Abriendo", "Opening"));
                        Task::perform(
                            run_blocking_file_operation(move || {
                                shell::invoke_native_send_to(&target, &paths)
                            }),
                            move |result| Message::SendToFinished(menu.pane, label.clone(), result),
                        )
                    }
                }
            }
            ContextCommand::OpenFileLocation => {
                self.context_open_file_location(menu.pane, menu.target)
            }
            ContextCommand::OpenWithApplication(index) => {
                let Some(entry) = self.context_entry(menu.pane, menu.target) else {
                    return Task::none();
                };
                let path = entry.path.clone();
                let Some(application) = menu.open_with_applications.get(index) else {
                    return self.report_error(
                        menu.pane,
                        self.localized(
                            "La aplicación seleccionada ya no está disponible",
                            "The selected application is no longer available",
                        ),
                    );
                };
                match shell::open_with_application(&path, application) {
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
            ContextCommand::DuplicateCleanup => {
                self.start_duplicate_cleanup(menu.pane, menu.target)
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
        if self.is_trash_pane(pane) {
            return None;
        }
        if let Some(entry) = self.context_entry(pane, target)
            && entry.kind.is_container()
            && !explorer::is_trash_item_path(&entry.path)
        {
            return Some(entry.path);
        }
        self.tab_for_pane(pane).path.clone()
    }
}

fn send_to_storage_icon(drive_kind: Option<DriveKind>) -> &'static str {
    match drive_kind {
        Some(DriveKind::Portable) => "portable",
        Some(DriveKind::Usb) => "usb",
        Some(DriveKind::External) => "external-drive",
        _ => "storage",
    }
}

fn send_to_storage_label(
    name: &str,
    path: &Path,
    drive_kind: Option<DriveKind>,
    spanish: bool,
) -> String {
    #[cfg(target_os = "windows")]
    {
        if !spanish {
            return name.to_owned();
        }
        let Some(drive_kind @ (DriveKind::Usb | DriveKind::External)) = drive_kind else {
            return name.to_owned();
        };
        let path_text = path.as_os_str().to_string_lossy();
        let mut characters = path_text.chars();
        let Some(letter) = characters.next().filter(char::is_ascii_alphabetic) else {
            return name.to_owned();
        };
        if characters.next() != Some(':') {
            return name.to_owned();
        }
        let generic_name = format!("{} ({letter}:)", drive_kind.label());
        if !name.eq_ignore_ascii_case(&generic_name) {
            return name.to_owned();
        }
        let localized_kind = match drive_kind {
            DriveKind::Usb => "Unidad USB",
            DriveKind::External => "Unidad externa",
            _ => unreachable!("only removable drive kinds are matched above"),
        };
        format!("{localized_kind} ({letter}:)")
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (path, drive_kind, spanish);
        name.to_owned()
    }
}

fn native_send_to_icon_request(
    target: &shell::NativeSendToTarget,
    size: u32,
) -> Option<(PathBuf, PathBuf, bool)> {
    #[cfg(target_os = "linux")]
    {
        linux_send_to_icon_request(target.icon(), size)
    }
    #[cfg(target_os = "windows")]
    {
        Some(windows_send_to_icon_request(target.icon_path(), size))
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        let _ = (target, size);
        None
    }
}

#[cfg(target_os = "linux")]
fn linux_send_to_icon_request(icon: &str, size: u32) -> Option<(PathBuf, PathBuf, bool)> {
    let path = match icon {
        "portable" => "bexplorer-portable-device",
        "bluetooth" => "bexplorer-bluetooth",
        "mail" => "bexplorer-mail",
        _ => return None,
    };
    Some((
        PathBuf::from(format!("__bexplorer_send_to_{icon}_icon_size_{size}")),
        PathBuf::from(path),
        false,
    ))
}

#[cfg(target_os = "windows")]
fn windows_send_to_icon_request(path: &Path, size: u32) -> (PathBuf, PathBuf, bool) {
    (
        thumbnail_data::native_path_icon_cache_key(path, false, size),
        path.to_path_buf(),
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_to_uses_an_icon_matching_each_removable_destination() {
        assert_eq!(send_to_storage_icon(Some(DriveKind::Portable)), "portable");
        assert_eq!(send_to_storage_icon(Some(DriveKind::Usb)), "usb");
        assert_eq!(
            send_to_storage_icon(Some(DriveKind::External)),
            "external-drive"
        );
        assert_eq!(send_to_storage_icon(Some(DriveKind::Local)), "storage");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn send_to_native_providers_request_desktop_theme_icons() {
        let (_, phone, _) =
            linux_send_to_icon_request("portable", 48).expect("portable native icon");
        let (_, bluetooth, _) =
            linux_send_to_icon_request("bluetooth", 48).expect("Bluetooth native icon");
        let (_, mail, _) = linux_send_to_icon_request("mail", 48).expect("mail native icon");

        assert_eq!(phone, Path::new("bexplorer-portable-device"));
        assert_eq!(bluetooth, Path::new("bexplorer-bluetooth"));
        assert_eq!(mail, Path::new("bexplorer-mail"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn send_to_windows_targets_request_their_shell_icons() {
        let target = Path::new(r"C:\Users\Test\AppData\Roaming\Microsoft\Windows\SendTo\App.lnk");
        let (cache_key, icon_path, is_directory) = windows_send_to_icon_request(target, 48);

        assert_eq!(
            cache_key,
            thumbnail_data::native_path_icon_cache_key(target, false, 48)
        );
        assert_eq!(icon_path, target);
        assert!(!is_directory);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn send_to_localizes_only_generic_windows_drive_names() {
        assert_eq!(
            send_to_storage_label(
                "USB Drive (E:)",
                Path::new(r"E:\"),
                Some(DriveKind::Usb),
                true,
            ),
            "Unidad USB (E:)"
        );
        assert_eq!(
            send_to_storage_label("BACKUP (E:)", Path::new(r"E:\"), Some(DriveKind::Usb), true,),
            "BACKUP (E:)"
        );
        assert_eq!(
            send_to_storage_label(
                "USB Drive (E:)",
                Path::new(r"E:\"),
                Some(DriveKind::Usb),
                false,
            ),
            "USB Drive (E:)"
        );
    }
}
