use super::*;
use iced::widget::{column, row};

impl BExplorerIced {
    pub(super) fn title_bar(&self, palette: Palette) -> Element<'_, Message> {
        let menu = icon_button("menu", Message::ToggleMenu, palette, self.title_menu_open);
        let sidebar = icon_button(
            "side",
            Message::ToggleSidebar,
            palette,
            self.sidebar_visible,
        );

        let tabs = self.title_tabs_area(palette);
        let pane_alignment_space = if self.sidebar_is_rendered() && !self.uses_split_sidebars() {
            (self.current_sidebar_width() - TITLE_BUTTON_WIDTH * 2.0 - TITLE_BUTTON_GAP).max(0.0)
        } else {
            TITLE_TAB_START_PADDING
        };

        let tab_band = row![
            menu,
            Space::new().width(TITLE_BUTTON_GAP),
            sidebar,
            Space::new().width(pane_alignment_space),
            tabs
        ]
        .align_y(Alignment::Center)
        .height(TITLE_HEIGHT)
        .width(Length::Fill);

        let controls = row![
            icon_button("split", Message::ToggleSplit, palette, self.split.is_some()),
            icon_button("min", Message::WindowMinimize, palette, false),
            icon_button(
                if self.window_maximized {
                    "restore"
                } else {
                    "max"
                },
                Message::WindowMaximize,
                palette,
                false,
            ),
            window_close_button(palette),
        ]
        .spacing(TITLE_BUTTON_GAP)
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
        let show_menu_color = if self.show_menu_open {
            palette.accent_text
        } else {
            palette.text
        };
        let show_menu_icon_color = if self.show_menu_open {
            palette.accent_text
        } else {
            palette.muted_text
        };
        let show_menu_entry = mouse_area(
            Button::new(
                container(
                    row![
                        inline_icon("eye", show_menu_icon_color, 16.0),
                        text(self.localized("Mostrar", "Show"))
                            .size(self.font_size())
                            .color(show_menu_color)
                            .width(Length::Fill),
                        inline_icon("chev-right", show_menu_icon_color, 14.0),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                )
                .height(Length::Fill)
                .center_y(Length::Fill),
            )
            .width(Length::Fill)
            .height(32)
            .padding([0, 8])
            .on_press(Message::OpenShowMenu)
            .style(move |_, status| button_style(palette, self.show_menu_open, status)),
        )
        .on_enter(Message::ShowMenuParentEnter)
        .on_exit(Message::ShowMenuParentExit);
        let menu = container(
            column![
                Button::new(
                    container(
                        row![
                            inline_icon("keyboard", palette.muted_text, 16.0),
                            text(self.localized("Atajos", "Shortcuts"))
                                .size(self.font_size())
                                .color(palette.text)
                                .width(Length::Fill),
                        ]
                        .spacing(10)
                        .align_y(Alignment::Center),
                    )
                    .height(Length::Fill)
                    .center_y(Length::Fill),
                )
                .width(Length::Fill)
                .height(32)
                .padding([0, 8])
                .on_press(Message::OpenShortcuts)
                .style(move |_, status| button_style(palette, false, status)),
                show_menu_entry,
                Button::new(
                    container(
                        row![
                            inline_icon("settings", palette.muted_text, 16.0),
                            text(self.localized("Configuracion", "Settings"))
                                .size(self.font_size())
                                .color(palette.text)
                                .width(Length::Fill),
                        ]
                        .spacing(8)
                        .align_y(Alignment::Center),
                    )
                    .height(Length::Fill)
                    .center_y(Length::Fill),
                )
                .width(Length::Fill)
                .height(32)
                .padding([0, 8])
                .on_press(Message::ToggleSettings)
                .style(move |_, status| button_style(palette, false, status)),
            ]
            .spacing(3),
        )
        .padding(7)
        .width(220)
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
        let menu =
            self.frosted_popup_surface(self.popup_backdrop.as_ref(), menu.into(), 220.0, 116.0);
        let floating_menu: Element<'_, Message> = float(opaque(menu))
            .translate(|_, _| Vector::new(0.0, TITLE_HEIGHT))
            .into();

        let mut layers = vec![backdrop.into(), floating_menu];
        if self.show_menu_open {
            let submenu = container(
                column![
                    self.show_menu_option(
                        self.localized("Barra de acciones", "Action bar"),
                        self.config.show_action_bar,
                        Message::ToggleActionBar,
                        palette,
                    ),
                    self.show_menu_option(
                        self.localized("Barra de marcadores", "Bookmarks bar"),
                        self.config.show_bookmark_bar,
                        Message::ToggleBookmarkBar,
                        palette,
                    ),
                    self.show_menu_option(
                        self.localized(
                            "Menu lateral en pantalla dividida",
                            "Sidebar in split view"
                        ),
                        self.config.show_split_pane_menus,
                        Message::ToggleSplitPaneMenus,
                        palette,
                    ),
                    self.show_menu_option(
                        self.localized(
                            "Panel de vista previa en pantalla dividida",
                            "Preview panel in split view",
                        ),
                        self.config.show_split_preview_panels,
                        Message::ToggleSplitPreviewPanels,
                        palette,
                    ),
                ]
                .spacing(3),
            )
            .padding(7)
            .width(286)
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
                286.0,
                151.0,
            );
            let submenu = mouse_area(submenu)
                .on_enter(Message::ShowMenuSubmenuEnter)
                .on_exit(Message::ShowMenuSubmenuExit);
            layers.push(
                float(opaque(submenu))
                    .translate(|_, _| Vector::new(218.0, TITLE_HEIGHT + 41.0))
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
        label: &'static str,
        enabled: bool,
        message: Message,
        palette: Palette,
    ) -> Element<'_, Message> {
        Button::new(
            container(
                row![
                    text(if enabled { "✓" } else { "" })
                        .size(self.font_size())
                        .color(palette.accent_text)
                        .width(18),
                    text(label).size(self.font_size()).color(if enabled {
                        palette.accent_text
                    } else {
                        palette.text
                    }),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .height(Length::Fill)
            .center_y(Length::Fill),
        )
        .width(Length::Fill)
        .height(32)
        .padding([0, 8])
        .on_press(message)
        .style(move |_, status| button_style(palette, enabled, status))
        .into()
    }

    pub(super) fn context_menu_overlay(&self, palette: Palette) -> Element<'_, Message> {
        let Some(menu_state) = &self.context_menu else {
            return Space::new().into();
        };
        let (x, y) = self.context_menu_window_position(menu_state);
        let menu_height = self.context_menu_height(menu_state);
        let is_entry = matches!(menu_state.target, ContextTarget::Entry(_));
        let is_sidebar_drive = matches!(menu_state.target, ContextTarget::SidebarDrive(_));
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
        let defender_available = cfg!(target_os = "windows")
            && context_entry
                .as_ref()
                .is_some_and(|entry| !explorer::is_virtual_path(&entry.path));

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
                    true
                ),
                context_quick_button(
                    "cut",
                    self.localized("Cortar", "Cut"),
                    ContextCommand::Cut,
                    palette,
                    true
                ),
                context_quick_button(
                    "paste",
                    self.localized("Pegar", "Paste"),
                    ContextCommand::Paste,
                    palette,
                    menu_state.paste_available,
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
                ),
                context_quick_button(
                    "copy",
                    self.localized("Copiar", "Copy"),
                    ContextCommand::Copy,
                    palette,
                    false
                ),
                context_quick_button(
                    "cut",
                    self.localized("Cortar", "Cut"),
                    ContextCommand::Cut,
                    palette,
                    false
                ),
            ]
        }
        .spacing(2)
        .padding([6, 0])
        .align_y(Alignment::Center)
        .width(Length::Fill);

        let mut items = if is_sidebar_drive {
            column![context_menu_row(
                "storage",
                self.localized("Expulsar", "Eject"),
                None,
                ContextCommand::EjectDrive,
                palette,
            )]
        } else {
            column![quick_actions, context_separator(palette)]
        }
        .spacing(2)
        .width(Length::Fill);

        if is_sidebar_drive {
            // The sidebar menu intentionally contains only actions that are
            // safe for the mounted volume itself.
        } else if is_entry {
            items = items
                .push(context_menu_row(
                    "open",
                    self.localized("Abrir", "Open"),
                    None,
                    ContextCommand::Open,
                    palette,
                ))
                .push(
                    mouse_area(context_menu_row(
                        "open-with",
                        self.localized("Abrir con", "Open with"),
                        Some(ContextMenuTrailing::Icon("chev-right")),
                        ContextCommand::OpenWithMenu,
                        palette,
                    ))
                    .on_enter(Message::ContextOpenWithParentEnter)
                    .on_exit(Message::ContextOpenWithParentExit),
                )
                .push(context_separator(palette))
                .push(
                    mouse_area(context_menu_row(
                        "archive",
                        self.localized("Comprimir", "Compress"),
                        Some(ContextMenuTrailing::Icon("chev-right")),
                        ContextCommand::CompressMenu,
                        palette,
                    ))
                    .on_enter(Message::ContextArchiveParentEnter)
                    .on_exit(Message::ContextArchiveParentExit),
                );
            if extractable_archive {
                items = items.push(
                    mouse_area(context_menu_row(
                        "archive",
                        self.localized("Extraer", "Extract"),
                        Some(ContextMenuTrailing::Icon("chev-right")),
                        ContextCommand::ExtractMenu,
                        palette,
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
                ));
            }
            if ejectable_drive {
                items = items.push(context_menu_row(
                    "storage",
                    self.localized("Expulsar", "Eject"),
                    None,
                    ContextCommand::EjectDrive,
                    palette,
                ));
            }
            if defender_available {
                items = items.push(context_menu_row(
                    "properties",
                    self.localized(
                        "Analizar con Microsoft Defender",
                        "Scan with Microsoft Defender",
                    ),
                    None,
                    ContextCommand::ScanWithDefender,
                    palette,
                ));
            }
            items = items
                .push(context_separator(palette))
                .push(context_menu_row(
                    "rename",
                    self.localized("Renombrar", "Rename"),
                    None,
                    ContextCommand::Rename,
                    palette,
                ))
                .push(context_menu_row(
                    "trash",
                    self.localized("Eliminar", "Delete"),
                    None,
                    ContextCommand::Delete,
                    palette,
                ))
                .push(context_menu_row(
                    "delete-forever",
                    self.localized("Eliminar permanentemente", "Delete permanently"),
                    None,
                    ContextCommand::DeletePermanent,
                    palette,
                ))
                .push(context_separator(palette));
        } else {
            items = items
                .push(context_menu_row(
                    "refresh",
                    self.localized("Actualizar", "Refresh"),
                    None,
                    ContextCommand::Refresh,
                    palette,
                ))
                .push(
                    mouse_area(context_menu_row(
                        "add",
                        self.localized("Nuevo", "New"),
                        Some(ContextMenuTrailing::Icon("chev-right")),
                        ContextCommand::NewMenu,
                        palette,
                    ))
                    .on_enter(Message::ContextNewParentEnter)
                    .on_exit(Message::ContextNewParentExit),
                )
                .push(context_separator(palette));
        }

        if terminal_available {
            items = items.push(context_menu_row(
                "terminal",
                self.localized("Abrir en Terminal", "Open in Terminal"),
                None,
                ContextCommand::OpenTerminal,
                palette,
            ));
        }
        if !is_sidebar_drive {
            items = items.push(context_menu_row(
                "properties",
                self.localized("Propiedades", "Properties"),
                Some(ContextMenuTrailing::Text("Alt+Enter")),
                ContextCommand::Properties,
                palette,
            ));
        }

        let menu_content = container(items.padding([4, 6])).width(258).style(move |_| {
            container::Style::default()
                .background(palette.menu_bg)
                .border(border::rounded(7).color(palette.strong_border).width(1))
                .shadow(iced::Shadow {
                    color: Color::from_rgba8(0, 0, 0, 0.28),
                    offset: iced::Vector::new(0.0, 7.0),
                    blur_radius: 18.0,
                })
        });
        let menu = self.frosted_popup_surface(
            menu_state.backdrop.as_ref(),
            menu_content.into(),
            258.0,
            menu_height,
        );

        let backdrop = mouse_area(
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_press(Message::CloseContextMenu);

        let floating_menu: Element<'_, Message> = float(opaque(menu))
            .translate(move |_, _| Vector::new(x, y))
            .into();

        let mut overlay_layers = vec![backdrop.into(), floating_menu];
        if self.context_open_with_submenu && is_entry {
            let applications = self
                .context_entry(menu_state.pane, menu_state.target)
                .and_then(|entry| shell::open_with_applications(&entry.path).ok())
                .unwrap_or_default();
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
                let icon = application.icon_path.as_ref().and_then(|path| {
                    let key = thumbnail_data::native_path_icon_cache_key(
                        path,
                        false,
                        thumbnail_data::NATIVE_ICON_SIZE,
                    );
                    match self.native_icon_cache.get(&key) {
                        Some(IcedImageState::Ready(handle)) => Some(handle.clone()),
                        _ => None,
                    }
                });
                rows = rows.push(context_menu_application_row(
                    application.name.clone(),
                    icon,
                    ContextCommand::OpenWithApplication(index),
                    palette,
                ));
            }
            rows = rows.push(context_menu_dynamic_row(
                "open-with",
                self.localized("Elegir otra aplicación…", "Choose another app…")
                    .into(),
                None,
                ContextCommand::OpenWith,
                palette,
            ));
            let submenu_width = context_submenu_width(&submenu_labels).max(220.0);
            let submenu_height = (applications.len() as f32 * 36.0 + 46.0).min(320.0);
            let submenu_content = container(rows).width(submenu_width).style(move |_| {
                container::Style::default()
                    .background(palette.menu_bg)
                    .border(border::rounded(7).color(palette.strong_border).width(1))
                    .shadow(iced::Shadow {
                        color: Color::from_rgba8(0, 0, 0, 0.28),
                        offset: iced::Vector::new(0.0, 7.0),
                        blur_radius: 18.0,
                    })
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
            let submenu_x = if x + 258.0 + submenu_width <= self.window_size.width - 8.0 {
                x + 252.0
            } else {
                (x - submenu_width + 6.0).max(8.0)
            };
            let submenu_y = (y + 42.0).clamp(
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
        if self.context_archive_submenu && is_entry {
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
                        ),
                        context_menu_dynamic_row(
                            "archive",
                            extract_to_label.clone(),
                            None,
                            ContextCommand::Extract(ExtractMode::ToNamedFolder),
                            palette,
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
                        ),
                        context_menu_dynamic_row(
                            "archive",
                            seven_zip_label.clone(),
                            None,
                            ContextCommand::CompressDefault(ArchiveFormat::SevenZip),
                            palette,
                        ),
                        context_menu_dynamic_row(
                            "archive",
                            zip_label.clone(),
                            None,
                            ContextCommand::CompressDefault(ArchiveFormat::Zip),
                            palette,
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
            let submenu_width = context_submenu_width(&submenu_labels);
            let submenu_height = if self.context_extract_submenu {
                78.0
            } else {
                114.0
            };
            let submenu_content = container(submenu_rows)
                .width(submenu_width)
                .style(move |_| {
                    container::Style::default()
                        .background(palette.menu_bg)
                        .border(border::rounded(7).color(palette.strong_border).width(1))
                        .shadow(iced::Shadow {
                            color: Color::from_rgba8(0, 0, 0, 0.28),
                            offset: iced::Vector::new(0.0, 7.0),
                            blur_radius: 18.0,
                        })
                });
            let submenu_kind = if self.context_extract_submenu {
                ContextSubmenuKind::Extract
            } else {
                ContextSubmenuKind::Archive
            };
            let submenu_backdrop = (menu_state.submenu_backdrop_kind == Some(submenu_kind))
                .then_some(menu_state.submenu_backdrop.as_ref())
                .flatten();
            let submenu = self.frosted_popup_surface(
                submenu_backdrop,
                submenu_content.into(),
                submenu_width,
                submenu_height,
            );
            let submenu_x = if x + 258.0 + submenu_width <= self.window_size.width - 8.0 {
                x + 252.0
            } else {
                (x - submenu_width + 6.0).max(8.0)
            };
            let submenu_offset_y = if self.context_extract_submenu {
                146.0
            } else {
                112.0
            };
            let submenu_y =
                (y + submenu_offset_y).clamp(8.0, (self.window_size.height - 120.0).max(8.0));
            let submenu = mouse_area(submenu)
                .on_enter(Message::ContextArchiveSubmenuEnter)
                .on_exit(Message::ContextArchiveSubmenuExit);
            overlay_layers.push(
                float(opaque(submenu))
                    .translate(move |_, _| Vector::new(submenu_x, submenu_y))
                    .into(),
            );
        } else if self.context_new_submenu && !is_entry {
            let labels = vec![
                self.localized("Nueva carpeta", "New folder").to_owned(),
                self.localized("Documento de texto", "Text document")
                    .to_owned(),
            ];
            let submenu_width = context_submenu_width(&labels);
            let submenu_height = 78.0;
            let submenu_content = container(
                column![
                    context_menu_row(
                        "folder",
                        self.localized("Nueva carpeta", "New folder"),
                        None,
                        ContextCommand::NewFolder,
                        palette,
                    ),
                    context_menu_row(
                        "file",
                        self.localized("Documento de texto", "Text document"),
                        None,
                        ContextCommand::NewTextDocument,
                        palette,
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
                    .shadow(iced::Shadow {
                        color: Color::from_rgba8(0, 0, 0, 0.28),
                        offset: iced::Vector::new(0.0, 7.0),
                        blur_radius: 18.0,
                    })
            });
            let submenu_x = if x + 258.0 + submenu_width <= self.window_size.width - 8.0 {
                x + 252.0
            } else {
                (x - submenu_width + 6.0).max(8.0)
            };
            let submenu_y = (y + 98.0).clamp(8.0, (self.window_size.height - 86.0).max(8.0));
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
