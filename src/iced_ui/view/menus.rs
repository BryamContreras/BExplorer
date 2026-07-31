use super::*;
use iced::widget::{column, row};

fn context_menu_shadow(opacity: f32) -> iced::Shadow {
    iced::Shadow {
        color: Color::from_rgba8(0, 0, 0, 0.28 * opacity.clamp(0.0, 1.0)),
        offset: iced::Vector::new(0.0, 7.0),
        blur_radius: 18.0,
    }
}

impl BExplorerIced {
    pub(super) fn title_bar(&self, palette: Palette) -> Element<'_, Message> {
        let menu = delayed_title_tooltip(
            icon_button(
                "menu",
                Message::ToggleMenu,
                palette,
                self.title_menu_open,
                self.font_size(),
            ),
            self.localized("Menú", "Menu"),
            palette,
            self.font_size(),
        );
        let sidebar = delayed_title_tooltip(
            icon_button(
                "side",
                Message::ToggleSidebar,
                palette,
                self.sidebar_visible,
                self.font_size(),
            ),
            if self.sidebar_visible {
                self.localized("Ocultar menú lateral", "Hide sidebar")
            } else {
                self.localized("Mostrar menú lateral", "Show sidebar")
            },
            palette,
            self.font_size(),
        );

        let tabs = self.title_tabs_area(palette);
        let title_button_width = self.ui_metric(TITLE_BUTTON_WIDTH);
        let window_caption_button_width = self.ui_metric(WINDOW_CAPTION_BUTTON_WIDTH);
        let title_button_gap = self.ui_metric(TITLE_BUTTON_GAP);
        let menu_sidebar_gap = self.ui_metric(8.0);
        let pane_alignment_space =
            (self.title_tabs_start_x() - title_button_width * 2.0 - menu_sidebar_gap).max(0.0);

        let tab_band = row![
            menu,
            Space::new().width(menu_sidebar_gap),
            sidebar,
            Space::new().width(pane_alignment_space),
            container(tabs)
                .width(Length::Fill)
                .height(TITLE_HEIGHT)
                .clip(true),
        ]
        .align_y(Alignment::Center)
        .height(TITLE_HEIGHT)
        .width(Length::Fill);

        let split = delayed_title_tooltip(
            icon_button(
                "split",
                Message::ToggleSplit,
                palette,
                self.split.is_some(),
                self.font_size(),
            ),
            if self.split.is_some() {
                self.localized("Cerrar vista dividida", "Close split view")
            } else {
                self.localized("Vista dividida", "Split view")
            },
            palette,
            self.font_size(),
        );
        let minimize = delayed_title_tooltip(
            icon_button(
                "min",
                Message::WindowMinimize,
                palette,
                false,
                self.font_size(),
            )
            .width(window_caption_button_width)
            .height(TITLE_HEIGHT),
            self.localized("Minimizar", "Minimize"),
            palette,
            self.font_size(),
        );
        let maximize = delayed_title_tooltip(
            icon_button(
                if self.window_maximized {
                    "restore"
                } else {
                    "max"
                },
                Message::WindowMaximize,
                palette,
                false,
                self.font_size(),
            )
            .width(window_caption_button_width)
            .height(TITLE_HEIGHT),
            if self.window_maximized {
                self.localized("Restaurar", "Restore")
            } else {
                self.localized("Maximizar", "Maximize")
            },
            palette,
            self.font_size(),
        );
        let close = delayed_title_tooltip(
            window_close_button(palette, self.font_size())
                .width(window_caption_button_width)
                .height(TITLE_HEIGHT),
            self.localized("Cerrar", "Close"),
            palette,
            self.font_size(),
        );

        let controls = row![split, minimize, maximize, close]
            .spacing(title_button_gap)
            .align_y(Alignment::Center);

        let controls_overlay = row![Space::new().width(Length::Fill), controls]
            .height(TITLE_HEIGHT)
            .width(Length::Fill)
            .align_y(Alignment::Center);

        let bar = stack(vec![tab_band.into(), controls_overlay.into()])
            .height(TITLE_HEIGHT)
            .width(Length::Fill);

        let title_radius = self.main_window_corner_radius();
        let base = container(bar)
            .height(TITLE_HEIGHT)
            .width(Length::Fill)
            .style(move |_| {
                container::Style::default()
                    .background(palette.title_bg)
                    .border(
                        border::rounded(border::top(title_radius))
                            .color(palette.border)
                            .width(1),
                    )
            });

        base.into()
    }

    pub(super) fn title_menu_overlay(&self, palette: Palette) -> Element<'_, Message> {
        let density = self.ui_density();
        let menu_item_height = self.ui_metric(32.0);
        let menu_width = self.ui_metric(220.0);
        let menu_height = 151.0 + density * 4.0;
        let shortcuts_selected = self.keyboard_menu_item_selected(KeyboardMenu::Title, 0);
        let show_keyboard_selected = self.keyboard_menu_item_selected(KeyboardMenu::Title, 1);
        let settings_selected = self.keyboard_menu_item_selected(KeyboardMenu::Title, 2);
        let about_selected = self.keyboard_menu_item_selected(KeyboardMenu::Title, 3);
        let show_selected = self.show_menu_open || show_keyboard_selected;
        let show_menu_color = if show_selected {
            palette.accent_text
        } else {
            palette.text
        };
        let show_menu_icon_color = if show_selected {
            palette.accent_text
        } else {
            palette.muted_text
        };
        let show_menu_entry = mouse_area(
            Button::new(
                container(
                    row![
                        inline_icon("view-tiles", show_menu_icon_color, self.ui_metric(16.0)),
                        text(self.localized("Vista", "View"))
                            .size(self.font_size())
                            .color(show_menu_color)
                            .width(Length::Fill),
                        inline_icon("chev-right", show_menu_icon_color, self.ui_metric(14.0)),
                    ]
                    .spacing(self.ui_metric(10.0))
                    .align_y(Alignment::Center),
                )
                .height(Length::Fill)
                .center_y(Length::Fill),
            )
            .width(Length::Fill)
            .height(menu_item_height)
            .padding([0.0, self.ui_metric(8.0)])
            .on_press(Message::OpenShowMenu)
            .style(move |_, status| selected_button_style(palette, show_selected, status)),
        )
        .on_enter(Message::ShowMenuParentEnter)
        .on_exit(Message::ShowMenuParentExit);
        let menu = container(
            column![
                Button::new(
                    container(
                        row![
                            inline_icon(
                                "keyboard",
                                if shortcuts_selected {
                                    palette.accent_text
                                } else {
                                    palette.muted_text
                                },
                                self.ui_metric(16.0),
                            ),
                            text(self.localized("Atajos", "Shortcuts"))
                                .size(self.font_size())
                                .color(if shortcuts_selected {
                                    palette.accent_text
                                } else {
                                    palette.text
                                })
                                .width(Length::Fill),
                        ]
                        .spacing(self.ui_metric(10.0))
                        .align_y(Alignment::Center),
                    )
                    .height(Length::Fill)
                    .center_y(Length::Fill),
                )
                .width(Length::Fill)
                .height(menu_item_height)
                .padding([0.0, self.ui_metric(8.0)])
                .on_press(Message::OpenShortcuts)
                .style(move |_, status| {
                    selected_button_style(palette, shortcuts_selected, status)
                }),
                show_menu_entry,
                Button::new(
                    container(
                        row![
                            inline_icon(
                                "settings",
                                if settings_selected {
                                    palette.accent_text
                                } else {
                                    palette.muted_text
                                },
                                self.ui_metric(16.0),
                            ),
                            text(self.localized("Configuracion", "Settings"))
                                .size(self.font_size())
                                .color(if settings_selected {
                                    palette.accent_text
                                } else {
                                    palette.text
                                })
                                .width(Length::Fill),
                        ]
                        .spacing(self.ui_metric(8.0))
                        .align_y(Alignment::Center),
                    )
                    .height(Length::Fill)
                    .center_y(Length::Fill),
                )
                .width(Length::Fill)
                .height(menu_item_height)
                .padding([0.0, self.ui_metric(8.0)])
                .on_press(Message::ToggleSettings)
                .style(move |_, status| {
                    selected_button_style(palette, settings_selected, status)
                }),
                Button::new(
                    container(
                        row![
                            inline_icon(
                                "properties",
                                if about_selected {
                                    palette.accent_text
                                } else {
                                    palette.muted_text
                                },
                                self.ui_metric(16.0),
                            ),
                            text(self.localized("Acerca de", "About"))
                                .size(self.font_size())
                                .color(if about_selected {
                                    palette.accent_text
                                } else {
                                    palette.text
                                })
                                .width(Length::Fill),
                        ]
                        .spacing(self.ui_metric(8.0))
                        .align_y(Alignment::Center),
                    )
                    .height(Length::Fill)
                    .center_y(Length::Fill),
                )
                .width(Length::Fill)
                .height(menu_item_height)
                .padding([0.0, self.ui_metric(8.0)])
                .on_press(Message::OpenAbout)
                .style(move |_, status| { selected_button_style(palette, about_selected, status) }),
            ]
            .spacing(3),
        )
        .padding(7)
        .width(menu_width)
        .style(move |_| {
            container::Style::default()
                .background(palette.menu_bg)
                .border(border::rounded(4).color(palette.border).width(1))
                .shadow(iced::Shadow {
                    color: Color::from_rgba8(0, 0, 0, 0.34),
                    offset: iced::Vector::new(0.0, 6.0),
                    blur_radius: 14.0,
                })
        });
        let backdrop = mouse_area(
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_press(Message::CloseFloatingMenus);
        let menu = self.frosted_popup_surface(
            self.popup_backdrop.as_ref(),
            menu.into(),
            menu_width,
            menu_height,
        );
        let floating_menu: Element<'_, Message> = float(opaque(menu))
            .translate(|_, _| Vector::new(0.0, TITLE_HEIGHT))
            .into();

        let mut layers = vec![backdrop.into(), floating_menu];
        if self.show_menu_open {
            let submenu_width = self.ui_metric(286.0);
            let action_bar_label = self.localized("Barra de acciones", "Action bar");
            let bookmark_bar_label = self.localized("Barra de marcadores", "Bookmarks bar");
            let split_sidebar_label =
                self.localized("Menu lateral en pantalla dividida", "Sidebar in split view");
            let split_preview_label = self.localized(
                "Panel de vista previa en pantalla dividida",
                "Preview panel in split view",
            );
            let submenu_labels = [
                action_bar_label,
                bookmark_bar_label,
                split_sidebar_label,
                split_preview_label,
            ];
            let submenu_height = adaptive_menu_list_height(
                &submenu_labels,
                self.font_size(),
                submenu_width,
                3.0,
                7.0,
            );
            let submenu = container(
                column![
                    self.show_menu_option(
                        0,
                        action_bar_label,
                        self.config.show_action_bar,
                        Message::ToggleActionBar,
                        palette,
                    ),
                    self.show_menu_option(
                        1,
                        bookmark_bar_label,
                        self.config.show_bookmark_bar,
                        Message::ToggleBookmarkBar,
                        palette,
                    ),
                    self.show_menu_option(
                        2,
                        split_sidebar_label,
                        self.config.show_split_pane_menus,
                        Message::ToggleSplitPaneMenus,
                        palette,
                    ),
                    self.show_menu_option(
                        3,
                        split_preview_label,
                        self.config.show_split_preview_panels,
                        Message::ToggleSplitPreviewPanels,
                        palette,
                    ),
                ]
                .spacing(3),
            )
            .padding(7)
            .width(submenu_width)
            .style(move |_| {
                container::Style::default()
                    .background(palette.menu_bg)
                    .border(border::rounded(4).color(palette.border).width(1))
                    .shadow(iced::Shadow {
                        color: Color::from_rgba8(0, 0, 0, 0.34),
                        offset: iced::Vector::new(0.0, 6.0),
                        blur_radius: 14.0,
                    })
            });
            let submenu = self.frosted_popup_surface(
                self.title_submenu_backdrop.as_ref(),
                submenu.into(),
                submenu_width,
                submenu_height,
            );
            let submenu = mouse_area(submenu)
                .on_enter(Message::ShowMenuSubmenuEnter)
                .on_exit(Message::ShowMenuSubmenuExit);
            let submenu_y = TITLE_HEIGHT + self.ui_metric(41.0);
            layers.push(
                float(opaque(submenu))
                    .translate(move |_, _| Vector::new(menu_width - 2.0, submenu_y))
                    .into(),
            );
        }

        stack(layers)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    pub(super) fn show_menu_option(
        &self,
        index: usize,
        label: &'static str,
        enabled: bool,
        message: Message,
        palette: Palette,
    ) -> Element<'_, Message> {
        let keyboard_selected = self.keyboard_menu_item_selected(KeyboardMenu::Show, index);
        let selected = if self.keyboard_menu_has_selection(KeyboardMenu::Show) {
            keyboard_selected
        } else {
            enabled
        };
        Button::new(
            container(
                row![
                    text(if enabled { "✓" } else { "" })
                        .size(self.font_size())
                        .color(if selected {
                            palette.accent_text
                        } else {
                            palette.muted_text
                        })
                        .width(self.ui_metric(18.0)),
                    text(label)
                        .size(self.font_size())
                        .color(if selected {
                            palette.accent_text
                        } else {
                            palette.text
                        })
                        .width(Length::Fill),
                ]
                .spacing(self.ui_metric(6.0))
                .align_y(Alignment::Center),
            )
            .height(Length::Fill)
            .center_y(Length::Fill),
        )
        .width(Length::Fill)
        .height(adaptive_menu_item_height(
            label,
            self.font_size(),
            self.ui_metric(286.0),
        ))
        .padding([0.0, self.ui_metric(8.0)])
        .on_press(message)
        .style(move |_, status| selected_button_style(palette, selected, status))
        .into()
    }

    pub(super) fn context_menu_overlay(&self, palette: Palette) -> Element<'_, Message> {
        let Some(menu_state) = &self.context_menu else {
            return Space::new().into();
        };
        let shadow_opacity = self.popup_fade_progress;
        let (x, y) = self.context_menu_window_position(menu_state);
        let menu_height = self.context_menu_height(menu_state);
        let menu_width = context_menu_width(self.font_size());
        let is_entry = matches!(menu_state.target, ContextTarget::Entry(_));
        let is_sidebar_drive = matches!(menu_state.target, ContextTarget::SidebarDrive(_));
        let trash_view = self.is_trash_pane(menu_state.pane) && !is_sidebar_drive;
        let is_search_result = is_entry && self.pane(menu_state.pane).folder_entries.is_some();
        let extractable_archive = self
            .context_entry(menu_state.pane, menu_state.target)
            .is_some_and(|entry| {
                crate::fs::archive_listing::has_extractable_archive_extension(&entry.path)
            });
        let terminal_available = !is_sidebar_drive
            && (!is_entry
                || self
                    .context_entry(menu_state.pane, menu_state.target)
                    .is_some_and(|entry| {
                        entry.kind.is_container() && !explorer::is_virtual_path(&entry.path)
                    }));
        let context_entry = self.context_entry(menu_state.pane, menu_state.target);
        let mountable_disk_image = context_entry
            .as_ref()
            .is_some_and(is_mountable_disk_image_entry);
        let ejectable_drive = context_entry
            .as_ref()
            .and_then(|entry| entry.drive_kind)
            .is_some_and(DriveKind::is_ejectable);
        let formatable_drive = context_entry.as_ref().is_some_and(|entry| {
            entry.kind == EntryKind::Drive && entry.drive_kind.is_some_and(DriveKind::is_formatable)
        });
        let drive_entry = context_entry
            .as_ref()
            .is_some_and(|entry| entry.kind == EntryKind::Drive);
        let duplicate_cleanup_available = context_entry
            .as_ref()
            .is_some_and(crate::iced_ui::duplicate_cleanup::duplicate_cleanup_available_for_entry);
        let storage_analysis_available = context_entry
            .as_ref()
            .is_some_and(crate::iced_ui::storage_analysis::storage_analysis_available_for_entry);
        let tools_available = storage_analysis_available || duplicate_cleanup_available;
        let tools_rows = usize::from(tools_available);
        let extra_archive_rows =
            usize::from(!menu_state.send_to_targets.is_empty()) + usize::from(is_search_result);
        let can_copy_or_cut = is_entry && !drive_entry;
        let defender_available = cfg!(target_os = "windows")
            && context_entry
                .as_ref()
                .is_some_and(|entry| !explorer::is_virtual_path(&entry.path));
        let context_font_size = self.font_size();
        let context_quick_button = |icon, label, command, palette, enabled, selected| {
            crate::iced_ui::helpers::context_quick_button(
                icon,
                label,
                command,
                palette,
                enabled,
                selected,
                context_font_size,
            )
        };
        let context_menu_row = |icon, label, trailing, command, palette, selected| {
            crate::iced_ui::helpers::context_menu_row(
                icon,
                label,
                trailing,
                command,
                palette,
                selected,
                context_font_size,
            )
        };
        let context_menu_dynamic_row = |icon, label, trailing, command, palette, selected| {
            crate::iced_ui::helpers::context_menu_dynamic_row(
                icon,
                label,
                trailing,
                command,
                palette,
                selected,
                context_font_size,
            )
        };
        let context_menu_application_row = |label, icon, command, palette, selected| {
            crate::iced_ui::helpers::context_menu_application_row(
                label,
                icon,
                command,
                palette,
                selected,
                context_font_size,
            )
        };

        // On empty space, copying or cutting has no meaningful target. Keep
        // those familiar actions visible but disabled, and lead with Paste so
        // the useful action is immediately available.
        let quick_actions = if is_entry {
            row![
                context_quick_button(
                    "copy",
                    self.localized("Copiar", "Copy"),
                    ContextCommand::Copy,
                    palette,
                    can_copy_or_cut,
                    self.context_command_keyboard_selected(ContextCommand::Copy),
                ),
                context_quick_button(
                    "cut",
                    self.localized("Cortar", "Cut"),
                    ContextCommand::Cut,
                    palette,
                    can_copy_or_cut,
                    self.context_command_keyboard_selected(ContextCommand::Cut),
                ),
                context_quick_button(
                    "paste",
                    self.localized("Pegar", "Paste"),
                    ContextCommand::Paste,
                    palette,
                    menu_state.paste_available,
                    self.context_command_keyboard_selected(ContextCommand::Paste),
                ),
            ]
        } else {
            row![
                context_quick_button(
                    "paste",
                    self.localized("Pegar", "Paste"),
                    ContextCommand::Paste,
                    palette,
                    menu_state.paste_available,
                    self.context_command_keyboard_selected(ContextCommand::Paste),
                ),
                context_quick_button(
                    "copy",
                    self.localized("Copiar", "Copy"),
                    ContextCommand::Copy,
                    palette,
                    false,
                    false,
                ),
                context_quick_button(
                    "cut",
                    self.localized("Cortar", "Cut"),
                    ContextCommand::Cut,
                    palette,
                    false,
                    false,
                ),
            ]
        }
        .spacing(2)
        .padding([6, 0])
        .align_y(Alignment::Center)
        .width(Length::Fill);

        let mut items = column![].spacing(2).width(Length::Fill);
        if trash_view {
            if is_entry {
                items = items
                    .push(context_menu_row(
                        "undo",
                        self.localized("Restaurar", "Restore"),
                        None,
                        ContextCommand::RestoreTrash,
                        palette,
                        self.context_command_keyboard_selected(ContextCommand::RestoreTrash),
                    ))
                    .push(context_menu_row(
                        "delete-forever",
                        self.localized("Eliminar", "Delete"),
                        None,
                        ContextCommand::DeleteTrash,
                        palette,
                        self.context_command_keyboard_selected(ContextCommand::DeleteTrash),
                    ));
            } else {
                items = items
                    .push(context_menu_row(
                        "refresh",
                        self.localized("Actualizar", "Refresh"),
                        None,
                        ContextCommand::Refresh,
                        palette,
                        self.context_command_keyboard_selected(ContextCommand::Refresh),
                    ))
                    .push(context_menu_row(
                        "delete-forever",
                        self.localized("Vaciar papelera", "Empty Recycle Bin"),
                        None,
                        ContextCommand::EmptyTrash,
                        palette,
                        self.context_command_keyboard_selected(ContextCommand::EmptyTrash),
                    ));
            }
        } else if is_sidebar_drive {
            if tools_available {
                items = items.push(
                    mouse_area(context_menu_row(
                        "settings",
                        self.localized("Herramientas", "Tools"),
                        Some(ContextMenuTrailing::Icon("chev-right")),
                        ContextCommand::ToolsMenu,
                        palette,
                        self.context_command_keyboard_selected(ContextCommand::ToolsMenu),
                    ))
                    .on_enter(Message::ContextToolsParentEnter)
                    .on_exit(Message::ContextToolsParentExit),
                );
            }
            if formatable_drive {
                items = items.push(context_menu_row(
                    "format",
                    self.localized("Formatear", "Format"),
                    None,
                    ContextCommand::FormatDrive,
                    palette,
                    self.context_command_keyboard_selected(ContextCommand::FormatDrive),
                ));
            }
            if ejectable_drive {
                items = items.push(context_menu_row(
                    "eject",
                    self.localized("Expulsar", "Eject"),
                    None,
                    ContextCommand::EjectDrive,
                    palette,
                    self.context_command_keyboard_selected(ContextCommand::EjectDrive),
                ));
            }
        } else {
            items = items.push(quick_actions).push(context_separator(palette));
        }

        if trash_view {
            // Trash has its own intentionally small action set above.
        } else if is_sidebar_drive {
            // The sidebar menu intentionally contains only actions that are
            // safe for the mounted volume itself.
        } else if is_entry {
            items = items.push(context_menu_row(
                "open",
                self.localized("Abrir", "Open"),
                None,
                ContextCommand::Open,
                palette,
                self.context_command_keyboard_selected(ContextCommand::Open),
            ));
            if drive_entry && tools_available {
                items = items.push(
                    mouse_area(context_menu_row(
                        "settings",
                        self.localized("Herramientas", "Tools"),
                        Some(ContextMenuTrailing::Icon("chev-right")),
                        ContextCommand::ToolsMenu,
                        palette,
                        self.context_command_keyboard_selected(ContextCommand::ToolsMenu),
                    ))
                    .on_enter(Message::ContextToolsParentEnter)
                    .on_exit(Message::ContextToolsParentExit),
                );
            }
            if !drive_entry {
                items = items.push(
                    mouse_area(context_menu_row(
                        "open-with",
                        self.localized("Abrir con", "Open with"),
                        Some(ContextMenuTrailing::Icon("chev-right")),
                        ContextCommand::OpenWithMenu,
                        palette,
                        self.context_command_keyboard_selected(ContextCommand::OpenWithMenu),
                    ))
                    .on_enter(Message::ContextOpenWithParentEnter)
                    .on_exit(Message::ContextOpenWithParentExit),
                );
                if !menu_state.send_to_targets.is_empty() {
                    items = items.push(
                        mouse_area(context_menu_row(
                            "send",
                            self.localized("Enviar a", "Send to"),
                            Some(ContextMenuTrailing::Icon("chev-right")),
                            ContextCommand::SendToMenu,
                            palette,
                            self.context_command_keyboard_selected(ContextCommand::SendToMenu),
                        ))
                        .on_enter(Message::ContextSendToParentEnter)
                        .on_exit(Message::ContextSendToParentExit),
                    );
                }
                if is_search_result {
                    items = items.push(context_menu_row(
                        "folder",
                        self.localized("Abrir ubicación del archivo", "Open file location"),
                        None,
                        ContextCommand::OpenFileLocation,
                        palette,
                        self.context_command_keyboard_selected(ContextCommand::OpenFileLocation),
                    ));
                }
                items = items.push(context_separator(palette)).push(
                    mouse_area(context_menu_row(
                        "archive",
                        self.localized("Comprimir", "Compress"),
                        Some(ContextMenuTrailing::Icon("chev-right")),
                        ContextCommand::CompressMenu,
                        palette,
                        self.context_command_keyboard_selected(ContextCommand::CompressMenu),
                    ))
                    .on_enter(Message::ContextArchiveParentEnter)
                    .on_exit(Message::ContextArchiveParentExit),
                );
                if tools_available {
                    items = items.push(
                        mouse_area(context_menu_row(
                            "settings",
                            self.localized("Herramientas", "Tools"),
                            Some(ContextMenuTrailing::Icon("chev-right")),
                            ContextCommand::ToolsMenu,
                            palette,
                            self.context_command_keyboard_selected(ContextCommand::ToolsMenu),
                        ))
                        .on_enter(Message::ContextToolsParentEnter)
                        .on_exit(Message::ContextToolsParentExit),
                    );
                }
                if extractable_archive {
                    items = items.push(
                        mouse_area(context_menu_row(
                            "archive",
                            self.localized("Extraer", "Extract"),
                            Some(ContextMenuTrailing::Icon("chev-right")),
                            ContextCommand::ExtractMenu,
                            palette,
                            self.context_command_keyboard_selected(ContextCommand::ExtractMenu),
                        ))
                        .on_enter(Message::ContextExtractParentEnter)
                        .on_exit(Message::ContextArchiveParentExit),
                    );
                }
                if mountable_disk_image {
                    items = items.push(context_menu_row(
                        "storage",
                        self.localized("Montar imagen", "Mount image"),
                        None,
                        ContextCommand::MountDiskImage,
                        palette,
                        self.context_command_keyboard_selected(ContextCommand::MountDiskImage),
                    ));
                }
            }
            if ejectable_drive {
                items = items.push(context_menu_row(
                    "eject",
                    self.localized("Expulsar", "Eject"),
                    None,
                    ContextCommand::EjectDrive,
                    palette,
                    self.context_command_keyboard_selected(ContextCommand::EjectDrive),
                ));
            }
            if formatable_drive {
                items = items.push(context_menu_row(
                    "format",
                    self.localized("Formatear", "Format"),
                    None,
                    ContextCommand::FormatDrive,
                    palette,
                    self.context_command_keyboard_selected(ContextCommand::FormatDrive),
                ));
            }
            if defender_available && !drive_entry {
                items = items.push(context_menu_row(
                    "properties",
                    self.localized(
                        "Analizar con Microsoft Defender",
                        "Scan with Microsoft Defender",
                    ),
                    None,
                    ContextCommand::ScanWithDefender,
                    palette,
                    self.context_command_keyboard_selected(ContextCommand::ScanWithDefender),
                ));
            }
            if !drive_entry {
                items = items
                    .push(context_separator(palette))
                    .push(context_menu_row(
                        "rename",
                        self.localized("Renombrar", "Rename"),
                        None,
                        ContextCommand::Rename,
                        palette,
                        self.context_command_keyboard_selected(ContextCommand::Rename),
                    ))
                    .push(context_menu_row(
                        "trash",
                        self.localized("Eliminar", "Delete"),
                        None,
                        ContextCommand::Delete,
                        palette,
                        self.context_command_keyboard_selected(ContextCommand::Delete),
                    ))
                    .push(context_menu_row(
                        "delete-forever",
                        self.localized("Eliminar permanentemente", "Delete permanently"),
                        None,
                        ContextCommand::DeletePermanent,
                        palette,
                        self.context_command_keyboard_selected(ContextCommand::DeletePermanent),
                    ))
                    .push(context_separator(palette));
            }
        } else {
            items = items
                .push(context_menu_row(
                    "refresh",
                    self.localized("Actualizar", "Refresh"),
                    None,
                    ContextCommand::Refresh,
                    palette,
                    self.context_command_keyboard_selected(ContextCommand::Refresh),
                ))
                .push(
                    mouse_area(context_menu_row(
                        "add",
                        self.localized("Nuevo", "New"),
                        Some(ContextMenuTrailing::Icon("chev-right")),
                        ContextCommand::NewMenu,
                        palette,
                        self.context_command_keyboard_selected(ContextCommand::NewMenu),
                    ))
                    .on_enter(Message::ContextNewParentEnter)
                    .on_exit(Message::ContextNewParentExit),
                )
                .push(context_separator(palette));
        }

        if terminal_available && !trash_view {
            items = items.push(context_menu_row(
                "terminal",
                self.localized("Abrir en Terminal", "Open in Terminal"),
                None,
                ContextCommand::OpenTerminal,
                palette,
                self.context_command_keyboard_selected(ContextCommand::OpenTerminal),
            ));
        }
        if !is_sidebar_drive && !trash_view {
            items = items.push(context_menu_row(
                "properties",
                self.localized("Propiedades", "Properties"),
                Some(ContextMenuTrailing::Text("Alt+Enter")),
                ContextCommand::Properties,
                palette,
                self.context_command_keyboard_selected(ContextCommand::Properties),
            ));
        }

        let menu_content = container(items.padding([4, 6]))
            .width(menu_width)
            .style(move |_| {
                container::Style::default()
                    .background(palette.menu_bg)
                    .border(border::rounded(7).color(palette.strong_border).width(1))
                    .shadow(context_menu_shadow(shadow_opacity))
            });
        let menu = self.frosted_popup_surface(
            menu_state.backdrop.as_ref(),
            menu_content.into(),
            menu_width,
            menu_height,
        );

        let backdrop = mouse_area(
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_press(Message::CloseContextMenu);

        let reveal_distance = self.ui_metric(4.0);
        let opens_upward =
            y + menu_height + reveal_distance > self.window_size.height - self.ui_metric(8.0);
        let reveal_y =
            context_menu_reveal_offset(self.popup_fade_progress, opens_upward, reveal_distance);
        let reveal_scale = context_menu_reveal_scale(self.popup_fade_progress);
        let floating_menu: Element<'_, Message> = float(opaque(menu))
            .scale(reveal_scale)
            .translate(move |bounds, _| {
                // `Float` scales around its center. Offset that transform so
                // regular menus grow from their top-left corner and menus near
                // the bottom edge grow from their bottom-left corner.
                let x_compensation = (1.0 - reveal_scale) * bounds.width / 2.0;
                let y_compensation = (1.0 - reveal_scale) * bounds.height / 2.0;
                Vector::new(
                    x - x_compensation,
                    y + reveal_y
                        + if opens_upward {
                            y_compensation
                        } else {
                            -y_compensation
                        },
                )
            })
            .into();

        let mut overlay_layers = vec![backdrop.into(), floating_menu];
        let submenu_backdrop_ready = |kind| menu_state.submenu_backdrop_kind == Some(kind);
        if self.context_tools_submenu
            && tools_available
            && !trash_view
            && submenu_backdrop_ready(ContextSubmenuKind::Tools)
        {
            let mut labels = Vec::new();
            let mut submenu_rows = column![].spacing(2).width(Length::Fill);
            for command in
                context_tools_commands(storage_analysis_available, duplicate_cleanup_available)
            {
                let (icon, label) = match command {
                    ContextCommand::StorageAnalysis => (
                        "storage",
                        self.localized("Análisis de almacenamiento", "Storage analysis"),
                    ),
                    ContextCommand::DuplicateCleanup => (
                        "folder-stack",
                        self.localized("Limpieza de archivos duplicados", "Duplicate file cleanup"),
                    ),
                    _ => unreachable!("tools submenu only contains tool commands"),
                };
                labels.push(label.to_owned());
                submenu_rows = submenu_rows.push(context_menu_row(
                    icon,
                    label,
                    None,
                    command,
                    palette,
                    self.context_command_keyboard_selected(command),
                ));
            }
            let submenu_width = context_submenu_width(&labels, self.font_size());
            let submenu_height = context_submenu_rows_height(labels.len(), self.font_size());
            let submenu_content = container(submenu_rows.padding([4, 6]))
                .width(submenu_width)
                .height(submenu_height)
                .style(move |_| {
                    container::Style::default()
                        .background(palette.menu_bg)
                        .border(border::rounded(7).color(palette.strong_border).width(1))
                        .shadow(context_menu_shadow(shadow_opacity))
                });
            let submenu_backdrop = (menu_state.submenu_backdrop_kind
                == Some(ContextSubmenuKind::Tools))
            .then_some(menu_state.submenu_backdrop.as_ref())
            .flatten();
            let submenu = self.frosted_popup_surface(
                submenu_backdrop,
                submenu_content.into(),
                submenu_width,
                submenu_height,
            );
            let submenu_x = if x + menu_width + submenu_width <= self.window_size.width - 8.0 {
                x + menu_width - 6.0
            } else {
                (x - submenu_width + 6.0).max(8.0)
            };
            let submenu_offset_y = context_tools_submenu_parent_offset(
                is_sidebar_drive,
                drive_entry,
                extra_archive_rows,
                self.font_size(),
            );
            let submenu_y = (y + submenu_offset_y).clamp(
                8.0,
                (self.window_size.height - submenu_height - 8.0).max(8.0),
            );
            let submenu = mouse_area(submenu)
                .on_enter(Message::ContextToolsSubmenuEnter)
                .on_exit(Message::ContextToolsSubmenuExit);
            overlay_layers.push(
                float(opaque(submenu))
                    .translate(move |_, _| Vector::new(submenu_x, submenu_y))
                    .into(),
            );
        }
        if self.context_open_with_submenu
            && is_entry
            && !trash_view
            && submenu_backdrop_ready(ContextSubmenuKind::OpenWith)
        {
            let applications = &menu_state.open_with_applications;
            let mut submenu_labels = applications
                .iter()
                .map(|application| application.name.clone())
                .collect::<Vec<_>>();
            submenu_labels.push(
                self.localized("Elegir otra aplicación…", "Choose another app…")
                    .into(),
            );
            let mut rows = column![].spacing(2).width(Length::Fill).padding([4, 6]);
            for (index, application) in applications.iter().enumerate() {
                let icon = open_with_application_icon_cache_key(
                    application,
                    thumbnail_data::SMALL_ENTRY_IMAGE_SIZE,
                )
                .and_then(|key| match self.native_icon_cache.get(&key) {
                    Some(IcedImageState::Ready(handle)) => Some(handle.clone()),
                    _ => None,
                });
                rows = rows.push(context_menu_application_row(
                    application.name.clone(),
                    icon,
                    ContextCommand::OpenWithApplication(index),
                    palette,
                    self.context_command_keyboard_selected(ContextCommand::OpenWithApplication(
                        index,
                    )),
                ));
            }
            rows = rows.push(context_menu_dynamic_row(
                "open-with",
                self.localized("Elegir otra aplicación…", "Choose another app…")
                    .into(),
                None,
                ContextCommand::OpenWith,
                palette,
                self.context_command_keyboard_selected(ContextCommand::OpenWith),
            ));
            let submenu_width =
                context_submenu_width(&submenu_labels, self.font_size()).max(self.ui_metric(220.0));
            let submenu_height =
                context_submenu_rows_height(applications.len() + 1, self.font_size())
                    .min(self.ui_metric(320.0));
            let submenu_content = container(
                scrollable(rows)
                    .id(Self::context_open_with_scroll_id())
                    .height(submenu_height)
                    .style(move |theme, status| {
                        explorer_scrollable_style(palette, theme, status, 1.0)
                    }),
            )
            .width(submenu_width)
            .height(submenu_height)
            .style(move |_| {
                container::Style::default()
                    .background(palette.menu_bg)
                    .border(border::rounded(7).color(palette.strong_border).width(1))
                    .shadow(context_menu_shadow(shadow_opacity))
            });
            let submenu_backdrop = (menu_state.submenu_backdrop_kind
                == Some(ContextSubmenuKind::OpenWith))
            .then_some(menu_state.submenu_backdrop.as_ref())
            .flatten();
            let submenu = self.frosted_popup_surface(
                submenu_backdrop,
                submenu_content.into(),
                submenu_width,
                submenu_height,
            );
            let submenu_x = if x + menu_width + submenu_width <= self.window_size.width - 8.0 {
                x + menu_width - 6.0
            } else {
                (x - submenu_width + 6.0).max(8.0)
            };
            let submenu_y = (y + context_submenu_parent_offset(true, 1, 1, self.font_size()))
                .clamp(
                    8.0,
                    (self.window_size.height - submenu_height - 8.0).max(8.0),
                );
            let submenu = mouse_area(submenu)
                .on_enter(Message::ContextOpenWithSubmenuEnter)
                .on_exit(Message::ContextOpenWithSubmenuExit);
            overlay_layers.push(
                float(opaque(submenu))
                    .translate(move |_, _| Vector::new(submenu_x, submenu_y))
                    .into(),
            );
        }
        if self.context_send_to_submenu
            && is_entry
            && !trash_view
            && !menu_state.send_to_targets.is_empty()
            && submenu_backdrop_ready(ContextSubmenuKind::SendTo)
        {
            let labels = menu_state
                .send_to_targets
                .iter()
                .map(|target| target.label().to_owned())
                .collect::<Vec<_>>();
            let mut rows = column![].spacing(2).width(Length::Fill).padding([4, 6]);
            for (index, target) in menu_state.send_to_targets.iter().enumerate() {
                rows = rows.push(context_menu_native_icon_row(
                    target.label().to_owned(),
                    self.context_send_to_icon_handle(target),
                    target.icon(),
                    ContextCommand::SendToTarget(index),
                    palette,
                    self.context_command_keyboard_selected(ContextCommand::SendToTarget(index)),
                    self.font_size(),
                ));
            }
            let submenu_width = context_submenu_width(&labels, self.font_size());
            let submenu_height = context_submenu_rows_height(labels.len(), self.font_size())
                .min(self.ui_metric(320.0));
            let submenu_content = container(
                scrollable(rows)
                    .id(Self::context_send_to_scroll_id())
                    .height(submenu_height)
                    .style(move |theme, status| {
                        explorer_scrollable_style(palette, theme, status, 1.0)
                    }),
            )
            .width(submenu_width)
            .height(submenu_height)
            .style(move |_| {
                container::Style::default()
                    .background(palette.menu_bg)
                    .border(border::rounded(7).color(palette.strong_border).width(1))
                    .shadow(context_menu_shadow(shadow_opacity))
            });
            let submenu_backdrop = (menu_state.submenu_backdrop_kind
                == Some(ContextSubmenuKind::SendTo))
            .then_some(menu_state.submenu_backdrop.as_ref())
            .flatten();
            let submenu = self.frosted_popup_surface(
                submenu_backdrop,
                submenu_content.into(),
                submenu_width,
                submenu_height,
            );
            let submenu_x = if x + menu_width + submenu_width <= self.window_size.width - 8.0 {
                x + menu_width - 6.0
            } else {
                (x - submenu_width + 6.0).max(8.0)
            };
            let submenu_y = (y + context_submenu_parent_offset(true, 2, 1, self.font_size()))
                .clamp(
                    8.0,
                    (self.window_size.height - submenu_height - 8.0).max(8.0),
                );
            let submenu = mouse_area(submenu)
                .on_enter(Message::ContextSendToSubmenuEnter)
                .on_exit(Message::ContextSendToSubmenuExit);
            overlay_layers.push(
                float(opaque(submenu))
                    .translate(move |_, _| Vector::new(submenu_x, submenu_y))
                    .into(),
            );
        }
        let archive_submenu_kind = if self.context_extract_submenu {
            ContextSubmenuKind::Extract
        } else {
            ContextSubmenuKind::Archive
        };
        if self.context_archive_submenu && is_entry && submenu_backdrop_ready(archive_submenu_kind)
        {
            let (submenu_rows, submenu_labels): (Element<'_, Message>, Vec<String>) =
                if self.context_extract_submenu {
                    let extract_to_label = self
                        .context_entry(menu_state.pane, menu_state.target)
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
                        // This submenu has enough room to retain a meaningful part of
                        // the destination name.  Keep the ellipsis only as the final
                        // fallback for unusually long archive names.
                        .map(|folder| {
                            format!(
                                "{} {}",
                                self.localized("Extraer en", "Extract to"),
                                ellipsize_text(&folder, 25),
                            )
                        })
                        .unwrap_or_else(|| {
                            self.localized("Extraer en carpeta", "Extract to folder")
                                .into()
                        });
                    let rows = column![
                        context_menu_row(
                            "archive",
                            self.localized("Extraer aquí", "Extract here"),
                            None,
                            ContextCommand::Extract(ExtractMode::Here),
                            palette,
                            self.context_command_keyboard_selected(ContextCommand::Extract(
                                ExtractMode::Here,
                            )),
                        ),
                        context_menu_dynamic_row(
                            "archive",
                            extract_to_label.clone(),
                            None,
                            ContextCommand::Extract(ExtractMode::ToNamedFolder),
                            palette,
                            self.context_command_keyboard_selected(ContextCommand::Extract(
                                ExtractMode::ToNamedFolder,
                            )),
                        ),
                    ]
                    .spacing(2)
                    .width(Length::Fill)
                    .padding([4, 6]);
                    (
                        rows.into(),
                        vec![
                            self.localized("Extraer aquí", "Extract here").into(),
                            extract_to_label,
                        ],
                    )
                } else {
                    let archive_name = self.default_archive_name(
                        menu_state.pane,
                        &self.context_paths(menu_state.pane, menu_state.target),
                    );
                    let seven_zip_label = context_archive_option_label(
                        self.localized("Comprimir", "Compress"),
                        &archive_name,
                        "7z",
                    );
                    let zip_label = context_archive_option_label(
                        self.localized("Comprimir", "Compress"),
                        &archive_name,
                        "zip",
                    );
                    let rows = column![
                        context_menu_row(
                            "archive",
                            self.localized("Comprimir", "Compress"),
                            None,
                            ContextCommand::CompressDialog,
                            palette,
                            self.context_command_keyboard_selected(ContextCommand::CompressDialog,),
                        ),
                        context_menu_dynamic_row(
                            "archive",
                            seven_zip_label.clone(),
                            None,
                            ContextCommand::CompressDefault(ArchiveFormat::SevenZip),
                            palette,
                            self.context_command_keyboard_selected(
                                ContextCommand::CompressDefault(ArchiveFormat::SevenZip,)
                            ),
                        ),
                        context_menu_dynamic_row(
                            "archive",
                            zip_label.clone(),
                            None,
                            ContextCommand::CompressDefault(ArchiveFormat::Zip),
                            palette,
                            self.context_command_keyboard_selected(
                                ContextCommand::CompressDefault(ArchiveFormat::Zip,)
                            ),
                        ),
                    ]
                    .spacing(2)
                    .width(Length::Fill)
                    .padding([4, 6]);
                    (
                        rows.into(),
                        vec![
                            self.localized("Comprimir", "Compress").into(),
                            seven_zip_label,
                            zip_label,
                        ],
                    )
                };
            let submenu_width = context_submenu_width(&submenu_labels, self.font_size());
            let submenu_height = if self.context_extract_submenu {
                context_submenu_rows_height(2, self.font_size())
            } else {
                context_submenu_rows_height(3, self.font_size())
            };
            let submenu_content = container(submenu_rows)
                .width(submenu_width)
                .style(move |_| {
                    container::Style::default()
                        .background(palette.menu_bg)
                        .border(border::rounded(7).color(palette.strong_border).width(1))
                        .shadow(context_menu_shadow(shadow_opacity))
                });
            let submenu_kind = archive_submenu_kind;
            let submenu_backdrop = (menu_state.submenu_backdrop_kind == Some(submenu_kind))
                .then_some(menu_state.submenu_backdrop.as_ref())
                .flatten();
            let submenu = self.frosted_popup_surface(
                submenu_backdrop,
                submenu_content.into(),
                submenu_width,
                submenu_height,
            );
            let submenu_x = if x + menu_width + submenu_width <= self.window_size.width - 8.0 {
                x + menu_width - 6.0
            } else {
                (x - submenu_width + 6.0).max(8.0)
            };
            let submenu_offset_y = if self.context_extract_submenu {
                context_submenu_parent_offset(
                    true,
                    3 + tools_rows + extra_archive_rows,
                    2,
                    self.font_size(),
                )
            } else {
                context_submenu_parent_offset(true, 2 + extra_archive_rows, 2, self.font_size())
            };
            let submenu_y = (y + submenu_offset_y).clamp(
                8.0,
                (self.window_size.height - submenu_height - 8.0).max(8.0),
            );
            let submenu = mouse_area(submenu)
                .on_enter(Message::ContextArchiveSubmenuEnter)
                .on_exit(Message::ContextArchiveSubmenuExit);
            overlay_layers.push(
                float(opaque(submenu))
                    .translate(move |_, _| Vector::new(submenu_x, submenu_y))
                    .into(),
            );
        } else if self.context_new_submenu
            && !is_entry
            && submenu_backdrop_ready(ContextSubmenuKind::New)
        {
            let labels = vec![
                self.localized("Nueva carpeta", "New folder").to_owned(),
                self.localized("Documento de texto", "Text document")
                    .to_owned(),
            ];
            let submenu_width = context_submenu_width(&labels, self.font_size());
            let submenu_height = context_submenu_rows_height(2, self.font_size());
            let submenu_content = container(
                column![
                    context_menu_row(
                        "folder",
                        self.localized("Nueva carpeta", "New folder"),
                        None,
                        ContextCommand::NewFolder,
                        palette,
                        self.context_command_keyboard_selected(ContextCommand::NewFolder),
                    ),
                    context_menu_row(
                        "file",
                        self.localized("Documento de texto", "Text document"),
                        None,
                        ContextCommand::NewTextDocument,
                        palette,
                        self.context_command_keyboard_selected(ContextCommand::NewTextDocument),
                    ),
                ]
                .spacing(2)
                .width(Length::Fill)
                .padding([4, 6]),
            )
            .width(submenu_width)
            .style(move |_| {
                container::Style::default()
                    .background(palette.menu_bg)
                    .border(border::rounded(7).color(palette.strong_border).width(1))
                    .shadow(context_menu_shadow(shadow_opacity))
            });
            let submenu_x = if x + menu_width + submenu_width <= self.window_size.width - 8.0 {
                x + menu_width - 6.0
            } else {
                (x - submenu_width + 6.0).max(8.0)
            };
            let submenu_y = (y + context_submenu_parent_offset(true, 1, 1, self.font_size()))
                .clamp(
                    8.0,
                    (self.window_size.height - submenu_height - 8.0).max(8.0),
                );
            let submenu_backdrop = (menu_state.submenu_backdrop_kind
                == Some(ContextSubmenuKind::New))
            .then_some(menu_state.submenu_backdrop.as_ref())
            .flatten();
            let submenu = self.frosted_popup_surface(
                submenu_backdrop,
                submenu_content.into(),
                submenu_width,
                submenu_height,
            );
            let submenu = mouse_area(submenu)
                .on_enter(Message::ContextNewSubmenuEnter)
                .on_exit(Message::ContextNewSubmenuExit);
            overlay_layers.push(
                float(opaque(submenu))
                    .translate(move |_, _| Vector::new(submenu_x, submenu_y))
                    .into(),
            );
        }

        stack(overlay_layers)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}
