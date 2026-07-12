use super::*;
use iced::widget::{column, row};

impl BExplorerIced {
    pub(in crate::iced_ui) fn file_pane(
        &self,
        pane: PaneId,
        palette: Palette,
        round_bottom_left: bool,
        round_bottom_right: bool,
    ) -> Element<'_, Message> {
        let favorite_active = self
            .tab_for_pane(pane)
            .path
            .as_ref()
            .is_some_and(|path| self.config.favorites.contains(path));
        let toolbar = row![
            icon_button("back", Message::Back(pane), palette, false),
            icon_button("next", Message::Forward(pane), palette, false),
            icon_button("up", Message::Up(pane), palette, false),
            icon_button(
                "bookmark",
                Message::ToggleFavorite(pane),
                palette,
                favorite_active
            ),
            self.address_bar(pane, palette),
            icon_button("refresh", Message::Refresh(pane), palette, false),
        ]
        .height(42)
        .spacing(4)
        .align_y(Alignment::Center)
        .padding([4, 10]);

        let undo_action: Element<'_, Message> = if self.last_undo_action.is_some() {
            tool_button(
                self.localized("Deshacer", "Undo"),
                Message::UndoLastAction,
                palette,
                false,
                false,
            )
            .into()
        } else {
            Space::new().width(0).into()
        };
        let action_bar_content = row![
            tool_button(
                self.localized("Nuevo", "New"),
                Message::ToggleNewMenu(pane),
                palette,
                self.new_menu_open == Some(pane),
                false,
            ),
            undo_action,
            tool_button(
                self.localized("Pegar", "Paste"),
                Message::PasteIntoPane(pane),
                palette,
                false,
                false,
            ),
            tool_button(
                self.localized("Copiar", "Copy"),
                Message::CopySelection(pane),
                palette,
                false,
                false,
            ),
            tool_button(
                self.localized("Cortar", "Cut"),
                Message::CutSelection(pane),
                palette,
                false,
                false,
            ),
            tool_button(
                self.localized("Renombrar", "Rename"),
                Message::RenameSelected(pane),
                palette,
                false,
                false,
            ),
            tool_button(
                self.localized("Eliminar", "Delete"),
                Message::DeleteSelected(pane),
                palette,
                false,
                false,
            ),
            tool_button(
                self.localized("Comprimir", "Compress"),
                Message::OpenArchiveDialog(pane),
                palette,
                false,
                false,
            ),
            Space::new().width(Length::Fill),
            tool_button(
                self.localized("Agrupar", "Group"),
                Message::ToggleGroupMenu(pane),
                palette,
                self.group_menu_open == Some(pane),
                self.split.is_some(),
            ),
            tool_button(
                self.localized("Vista previa", "Preview"),
                Message::TogglePreviewPanel(pane),
                palette,
                self.preview_panel_visible(pane),
                self.split.is_some(),
            ),
        ]
        .height(46)
        .spacing(4)
        .padding([5, 12])
        .align_y(Alignment::Center)
        .width(Length::Fill);
        let action_bar: Element<'_, Message> = container(action_bar_content)
            .width(Length::Fill)
            .height(46)
            .clip(true)
            .into();
        let action_bar: Element<'_, Message> = if self.config.show_action_bar {
            action_bar
        } else {
            Space::new().height(0).into()
        };
        let bookmark_bar: Element<'_, Message> =
            if self.config.show_bookmark_bar || !self.sidebar_visible {
                self.bookmark_bar(pane, palette, !self.sidebar_visible)
            } else {
                Space::new().height(0).into()
            };

        let table = self.file_table(pane, palette);
        let file_content: Element<'_, Message> = if self.preview_panel_visible(pane) {
            row![
                container(table)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .clip(true),
                self.preview_resize_handle(pane, palette),
                self.preview_panel(pane, palette),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else {
            table
        };
        let action_focus_line = self.split_focus_line(pane, palette);
        let status_focus_line = self.split_focus_line(pane, palette);

        let search_mode_icon = match self.pane(pane).search_mode {
            SearchMode::Quick => "folder",
            SearchMode::Complete => "folder-stack",
        };
        let search_input = text_input(
            self.localized("Filtrar", "Filter"),
            &self.pane(pane).search_text,
        )
        .id(search_input_id(pane))
        .on_input(move |value| Message::SearchChanged(pane, value))
        .size(self.font_size())
        .padding(Padding::new(6.0).left(9.0).right(38.0))
        .width(Length::Fill)
        .style(move |_, status| {
            let border_color = if matches!(status, iced::widget::text_input::Status::Focused { .. })
            {
                palette.accent
            } else {
                palette.strong_border
            };
            iced::widget::text_input::Style {
                background: palette.input_bg.into(),
                border: border::rounded(7).color(border_color).width(1),
                icon: palette.muted_text,
                placeholder: palette.muted_text,
                value: palette.text,
                selection: translucent_color(palette.accent, 0.58),
            }
        });
        let search_mode_button = Button::new(
            container(inline_icon(
                search_mode_icon,
                if self.search_mode_menu_open == Some(pane) {
                    palette.accent_text
                } else {
                    palette.muted_text
                },
                17.0,
            ))
            .width(Length::Fill)
            .height(Length::Fill)
            .center(Length::Fill),
        )
        .width(28)
        .height(26)
        .padding(0)
        .on_press(Message::ToggleSearchModeMenu(pane))
        .style(move |_, status| {
            selected_button_style(palette, self.search_mode_menu_open == Some(pane), status)
        });
        let filter_width = if self.split.is_some() { 210 } else { 260 };
        let filter = stack(vec![
            search_input.into(),
            container(search_mode_button)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_right(Length::Fill)
                .center_y(Length::Fill)
                .padding([0, 3])
                .into(),
        ])
        .width(filter_width)
        .height(32);

        let (selected_count, selected_size) = self.selection_status_metrics(pane);
        let selected_label = if selected_count == 1 {
            format!("1 {}", self.localized("seleccionado", "selected"))
        } else {
            format!(
                "{selected_count} {}",
                self.localized("seleccionados", "selected")
            )
        };
        let selection_status = format!("· {selected_label} · {}", format_size(Some(selected_size)));

        let status_content = row![
            filter,
            text(self.pane(pane).status.as_str())
                .size(self.font_size())
                .color(palette.muted_text),
            text(selection_status)
                .size(self.font_size())
                .color(palette.muted_text),
            Space::new().width(Length::Fill),
            text(self.localized("Vista", "View"))
                .size(self.font_size())
                .color(palette.muted_text),
            self.view_selector_button(pane, palette),
        ]
        .height(36)
        .spacing(8)
        .padding([4, 14])
        .align_y(Alignment::Center);

        let transfer_active = self.transfer_in_progress_for(pane);
        let search_active = self.pane(pane).search_receiver.is_some();
        let progress_active = transfer_active || search_active;
        let progress = if transfer_active {
            self.transfer_progress_fraction_for(pane).unwrap_or(0.0)
        } else {
            self.pane(pane).search_progress_phase
        };
        let status: Element<'_, Message> = column![
            iced::widget::progress_bar(0.0..=1.0, progress)
                .girth(if progress_active { 2.0 } else { 0.0 })
                .style(move |_| iced::widget::progress_bar::Style {
                    background: translucent_color(palette.border, 0.72).into(),
                    bar: accent_gradient(palette).into(),
                    border: border::rounded(0),
                }),
            status_content,
        ]
        .spacing(0)
        .into();

        let pane_body = container(
            column![
                toolbar,
                action_focus_line,
                action_bar,
                bookmark_bar,
                file_content,
                status_focus_line,
                status
            ]
            .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_| {
            container::Style::default()
                .background(palette.page_bg)
                .border(border::rounded(bottom_radius(
                    round_bottom_left,
                    round_bottom_right,
                )))
        })
        .into();

        if self.new_menu_open == Some(pane) {
            stack(vec![pane_body, self.new_menu_overlay(pane, palette)])
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else if self.view_menu_open == Some(pane) {
            stack(vec![pane_body, self.view_menu_overlay(pane, palette)])
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else if self.group_menu_open == Some(pane) {
            stack(vec![pane_body, self.group_menu_overlay(pane, palette)])
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else if self.search_mode_menu_open == Some(pane) {
            stack(vec![
                pane_body,
                self.search_mode_menu_overlay(pane, palette),
            ])
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else {
            pane_body
        }
    }

    pub(in crate::iced_ui) fn split_focus_line(
        &self,
        pane: PaneId,
        palette: Palette,
    ) -> Element<'_, Message> {
        if self.split.is_none() {
            return Space::new().height(0).into();
        }

        let color = if self.is_split_focused_pane(pane) {
            translucent_color(palette.accent, 0.42)
        } else {
            Color::TRANSPARENT
        };

        container(Space::new())
            .width(Length::Fill)
            .height(1)
            .style(move |_| container::Style::default().background(color))
            .into()
    }

    pub(in crate::iced_ui) fn bookmark_bar(
        &self,
        pane: PaneId,
        palette: Palette,
        include_storage_shortcut: bool,
    ) -> Element<'_, Message> {
        let mut bookmarks = row![]
            .spacing(6)
            .align_y(Alignment::Center)
            .height(Length::Fill);
        let filesystem = filesystem_root_path();
        if include_storage_shortcut {
            let storage_active =
                self.tab_for_pane(pane).path.as_deref() == Some(filesystem.as_path());
            let storage_icon: Element<'_, Message> = self
                .sidebar_directory_icon_handle(&filesystem)
                .map(|handle| {
                    iced_image::Image::new(handle)
                        .width(18)
                        .height(18)
                        .content_fit(ContentFit::Contain)
                        .into()
                })
                .unwrap_or_else(|| inline_icon("storage", palette.accent, 18.0));
            bookmarks = bookmarks.push(
                Button::new(
                    row![
                        storage_icon,
                        text(self.localized("Sistema de archivos", "Filesystem"))
                            .size(self.font_size())
                            .color(if storage_active {
                                palette.accent_text
                            } else {
                                palette.text
                            }),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .padding([7, 10])
                .on_press(Message::Navigate(pane, Some(filesystem.clone())))
                .style(move |_, status| selected_button_style(palette, storage_active, status)),
            );
        }

        if self.config.favorites.is_empty() && !include_storage_shortcut {
            bookmarks = bookmarks.push(
                text(self.localized("Marcadores", "Bookmarks"))
                    .size(self.font_size())
                    .color(palette.muted_text),
            );
        }

        for path in self.config.favorites.iter().take(6) {
            if include_storage_shortcut && path == &filesystem {
                continue;
            }
            let label = path
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| path.to_string_lossy().into_owned());
            let target = path.clone();
            bookmarks = bookmarks.push(
                Button::new(
                    row![
                        inline_icon("folder", palette.folder, 18.0),
                        text(ellipsize_text(&label, 18))
                            .size(self.font_size())
                            .color(palette.text),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .padding([7, 10])
                .on_press(Message::Navigate(pane, Some(target)))
                .style(move |_, status| button_style(palette, false, status)),
            );
        }

        container(bookmarks)
            .width(Length::Fill)
            .height(46)
            .padding([5, 12])
            .clip(true)
            .style(move |_| {
                container::Style::default()
                    .background(palette.sidebar_bg)
                    .border(border::color(palette.border).width(1))
            })
            .into()
    }

    pub(in crate::iced_ui) fn preview_panel_visible(&self, pane: PaneId) -> bool {
        (self.uses_split_preview_panels() || self.preview_panel_pane == Some(pane))
            && self.preview_panel_progress > 0.001
    }

    pub(in crate::iced_ui) fn preview_resize_handle(
        &self,
        pane: PaneId,
        _palette: Palette,
    ) -> Element<'_, Message> {
        let width =
            (SIDEBAR_RESIZE_HANDLE_WIDTH * self.preview_panel_progress.clamp(0.0, 1.0)).max(1.0);
        mouse_area(
            container(Space::new())
                .width(width)
                .height(Length::Fill)
                .style(|_| container::Style::default().background(Color::TRANSPARENT)),
        )
        .on_press(Message::StartPreviewResize(pane))
        .interaction(mouse::Interaction::ResizingHorizontally)
        .into()
    }

    pub(in crate::iced_ui) fn preview_panel(
        &self,
        pane: PaneId,
        palette: Palette,
    ) -> Element<'_, Message> {
        let width = self.current_preview_panel_width().max(1.0);
        let selected_entry = self
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
        let full_size_document_preview = selected_entry.as_ref().is_some_and(|entry| {
            thumbnail_data::is_pdf_preview_candidate(entry)
                || thumbnail_data::is_text_preview_candidate(entry)
        });

        let body: Element<'_, Message> = if let Some(entry) = selected_entry {
            if thumbnail_data::is_pdf_preview_candidate(&entry) {
                self.pdf_preview_content(pane, &entry, width, palette)
            } else {
                let is_pdf_document = thumbnail_data::is_pdf_preview_candidate(&entry);
                let is_text_document = thumbnail_data::is_text_preview_candidate(&entry);
                let is_document_preview = thumbnail_data::hides_preview_metadata(&entry);
                let preview_height = if is_pdf_document || is_text_document {
                    Length::Fill
                } else {
                    Length::Fixed(300.0)
                };
                let preview: Element<'_, Message> = if is_text_document {
                    let text_preview = self
                        .pane(pane)
                        .text_preview
                        .as_ref()
                        .filter(|preview| preview.path == entry.path);
                    if let Some(text_preview) = text_preview {
                        let path = entry.path.clone();
                        text_editor::TextEditor::new(&text_preview.content)
                            .on_action(move |action| {
                                Message::TextPreviewAction(pane, path.clone(), action)
                            })
                            // Preserve the editor's complete key map so it
                            // captures Enter/Delete too; mutations themselves
                            // are discarded in `TextPreviewAction`.
                            .key_binding(text_editor::Binding::from_key_press)
                            .size(self.font_size())
                            .padding(8)
                            .wrapping(iced::widget::text::Wrapping::WordOrGlyph)
                            .height(Length::Fill)
                            .style(move |_, _| text_editor::Style {
                                background: Color::TRANSPARENT.into(),
                                border: border::rounded(0).color(Color::TRANSPARENT).width(0),
                                placeholder: palette.muted_text,
                                value: palette.text,
                                selection: hover_tint(palette),
                            })
                            .into()
                    } else {
                        container(
                            text(self.localized(
                                "Cargando vista previa de texto…",
                                "Loading text preview…",
                            ))
                            .size(self.font_size())
                            .color(palette.muted_text),
                        )
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .center(Length::Fill)
                        .into()
                    }
                } else if let Some(IcedImageState::Ready(handle)) =
                    self.preview_cache.get(&entry.path)
                {
                    iced_image::Image::new(handle.clone())
                        .width(Length::Fill)
                        .height(preview_height)
                        .content_fit(ContentFit::Contain)
                        .into()
                } else if let Some(handle) = self.entry_image_handle(&entry).cloned() {
                    iced_image::Image::new(handle)
                        .width(Length::Fill)
                        .height(preview_height)
                        .content_fit(ContentFit::Contain)
                        .into()
                } else {
                    container(self.detail_file_entry_icon(&entry, palette, false, 72.0))
                        .width(Length::Fill)
                        .height(preview_height)
                        .center(Length::Fill)
                        .into()
                };
                let preview_surface = container(preview)
                    .width(Length::Fill)
                    .height(if is_pdf_document || is_text_document {
                        Length::Fill
                    } else {
                        Length::Fixed(318.0)
                    })
                    .padding(if is_text_document { 0 } else { 4 })
                    .style(move |_| {
                        container::Style::default()
                            .background(palette.input_bg)
                            .border(border::rounded(7).color(palette.border).width(1))
                    });
                if is_pdf_document || is_text_document {
                    column![preview_surface].height(Length::Fill).into()
                } else if is_document_preview {
                    column![preview_surface].into()
                } else {
                    column![
                        preview_surface,
                        text(self.entry_display_name(&entry))
                            .size(self.font_size() + 1.0)
                            .color(palette.text)
                            .wrapping(iced::widget::text::Wrapping::Word),
                        text(self.localized_entry_type_label(&entry))
                            .size(self.font_size())
                            .color(palette.muted_text),
                        text(format_size(entry.size))
                            .size(self.font_size())
                            .color(palette.muted_text),
                    ]
                    .spacing(8)
                    .into()
                }
            }
        } else {
            container(
                column![
                    inline_icon("preview", palette.muted_text, 42.0),
                    text(self.localized(
                        "Selecciona un archivo para ver su vista previa",
                        "Select a file to preview it",
                    ))
                    .size(self.font_size())
                    .color(palette.muted_text)
                    .align_x(Horizontal::Center)
                    .wrapping(iced::widget::text::Wrapping::Word),
                ]
                .spacing(12)
                .align_x(Alignment::Center),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center(Length::Fill)
            .into()
        };

        container(
            column![
                container(
                    row![
                        text(self.localized("Vista previa", "Preview"))
                            .size(self.font_size() + 1.0)
                            .color(palette.text)
                            .width(Length::Fill),
                        icon_button("x", Message::TogglePreviewPanel(pane), palette, false),
                    ]
                    .align_y(Alignment::Center),
                )
                .padding([8, 10])
                .style(move |_| {
                    container::Style::default()
                        .background(palette.header_bg)
                        .border(border::rounded(6).color(palette.border).width(1))
                }),
                container(body)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .padding(if full_size_document_preview { 4 } else { 8 }),
            ]
            .height(Length::Fill),
        )
        .width(width)
        .height(Length::Fill)
        .clip(true)
        .style(move |_| {
            container::Style::default()
                .background(palette.menu_bg)
                .border(border::color(palette.strong_border).width(1))
        })
        .into()
    }

    pub(in crate::iced_ui) fn pdf_preview_content(
        &self,
        pane: PaneId,
        entry: &FileEntry,
        panel_width: f32,
        palette: Palette,
    ) -> Element<'_, Message> {
        let state = self
            .pdf_previews
            .get(&pane)
            .filter(|state| state.path == entry.path);
        let page_count = state.and_then(|state| state.page_count);
        let current_page = state.map(|state| state.current_page).unwrap_or(0);
        let loading = state.is_none_or(|state| state.loading);
        let mut pages = column![].spacing(14);

        if let Some(state) = state {
            for page in &state.pages {
                let page_height = pdf_preview_page_height(panel_width, page.aspect_ratio);
                pages = pages.push(
                    container(
                        iced_image::Image::new(page.handle.clone())
                            .width(Length::Fill)
                            .height(Length::Fixed(page_height))
                            .content_fit(ContentFit::Contain),
                    )
                    .width(Length::Fill)
                    .height(Length::Fixed(page_height + 16.0))
                    .padding(8)
                    .style(move |_| {
                        container::Style::default()
                            .background(palette.input_bg)
                            .border(border::rounded(7).color(palette.border).width(1))
                    }),
                );
            }
        }

        if loading {
            let label = page_count
                .map(|total| {
                    format!(
                        "{} {} {} {total}…",
                        self.localized("Cargando página", "Loading page"),
                        current_page + 2,
                        self.localized("de", "of"),
                    )
                })
                .unwrap_or_else(|| {
                    self.localized("Cargando vista previa del PDF…", "Loading PDF preview…")
                        .into()
                });
            pages = pages.push(
                container(text(label).size(self.font_size()).color(palette.muted_text))
                    .width(Length::Fill)
                    .padding(12)
                    .center_x(Length::Fill),
            );
        }

        if state.is_some_and(|state| state.pages.is_empty() && !state.loading) {
            pages = pages.push(
                container(
                    text(self.localized(
                        "No se pudo renderizar este PDF.",
                        "This PDF could not be rendered.",
                    ))
                    .size(self.font_size())
                    .color(palette.muted_text),
                )
                .width(Length::Fill)
                .padding(12)
                .center_x(Length::Fill),
            );
        }

        let path = entry.path.clone();
        let document = scrollable(pages)
            .width(Length::Fill)
            .height(Length::Fill)
            .on_scroll(move |viewport| {
                Message::PdfPreviewScrolled(pane, path.clone(), viewport.absolute_offset().y)
            });
        let page_label = page_count
            .map(|total| format!("{} - {total}", current_page.saturating_add(1).min(total)))
            .unwrap_or_else(|| "…".into());

        column![
            container(document).width(Length::Fill).height(Length::Fill),
            container(
                text(page_label)
                    .size(self.font_size())
                    .color(palette.muted_text)
            )
            .width(Length::Fill)
            .padding([8, 0])
            .center_x(Length::Fill),
        ]
        .height(Length::Fill)
        .spacing(4)
        .into()
    }

    pub(in crate::iced_ui) fn view_selector_button(
        &self,
        pane: PaneId,
        palette: Palette,
    ) -> Element<'_, Message> {
        let mode = self.effective_view_mode(pane);
        let selected = self.view_menu_open == Some(pane);
        let color = if selected {
            palette.accent_text
        } else {
            palette.text
        };
        let affordance: Element<'_, Message> = inline_icon("chev-down", color, 12.0);
        let button = Button::new(
            row![
                inline_icon(view_mode_icon(mode), color, 15.0),
                text(self.localized(view_mode_label(mode), view_mode_label_english(mode)))
                    .size(self.font_size())
                    .color(color),
                affordance,
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([6, 10])
        .style(move |_, status| selected_button_style(palette, selected, status));

        button.on_press(Message::ToggleViewMenu(pane)).into()
    }

    pub(in crate::iced_ui) fn view_menu_overlay(
        &self,
        pane: PaneId,
        palette: Palette,
    ) -> Element<'_, Message> {
        let menu = container(
            column(
                view_menu_modes()
                    .into_iter()
                    .map(|mode| self.view_menu_item(pane, mode, palette))
                    .collect::<Vec<_>>(),
            )
            .spacing(3)
            .padding(6),
        )
        .width(218)
        .style(move |_| {
            container::Style::default()
                .background(palette.menu_bg)
                .border(border::rounded(6).color(palette.border).width(1))
                .shadow(iced::Shadow {
                    color: Color::from_rgba8(0, 0, 0, 0.22),
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
            self.frosted_popup_surface(self.popup_backdrop.as_ref(), menu.into(), 218.0, 219.0);
        let floating_menu: Element<'_, Message> = float(opaque(menu))
            .translate(|_, _| Vector::new(-14.0, -38.0))
            .into();

        let menu_layer = container(floating_menu)
            .align_right(Length::Fill)
            .align_bottom(Length::Fill)
            .into();

        stack(vec![backdrop.into(), menu_layer])
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    pub(in crate::iced_ui) fn view_menu_item(
        &self,
        pane: PaneId,
        mode: ViewMode,
        palette: Palette,
    ) -> Element<'_, Message> {
        let active = self.effective_view_mode(pane) == mode;
        let color = if active {
            palette.accent_text
        } else {
            palette.text
        };
        Button::new(
            container(
                row![
                    inline_icon(view_mode_icon(mode), color, 16.0),
                    text(self.localized(view_mode_label(mode), view_mode_label_english(mode)))
                        .size(self.font_size())
                        .color(color)
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
        .on_press(Message::SetViewMode(pane, mode))
        .style(move |_, status| selected_button_style(palette, active, status))
        .into()
    }

    pub(in crate::iced_ui) fn new_menu_overlay(
        &self,
        pane: PaneId,
        palette: Palette,
    ) -> Element<'_, Message> {
        let option = |icon: &'static str, label: &'static str, message: Message| {
            Button::new(
                container(
                    row![
                        inline_icon(icon, palette.muted_text, 17.0),
                        text(label).size(self.font_size()).color(palette.text),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                )
                .height(Length::Fill)
                .center_y(Length::Fill),
            )
            .width(Length::Fill)
            .height(32)
            .padding([0, 9])
            .on_press(message)
            .style(move |_, status| button_style(palette, false, status))
        };
        let menu = container(
            column![
                option(
                    "folder",
                    self.localized("Nueva carpeta", "New folder"),
                    Message::NewFolder(pane),
                ),
                option(
                    "file",
                    self.localized("Documento de texto", "Text document"),
                    Message::NewTextDocument(pane),
                ),
            ]
            .spacing(2)
            .padding(6),
        )
        .width(196)
        .style(move |_| {
            container::Style::default()
                .background(palette.menu_bg)
                .border(border::rounded(6).color(palette.border).width(1))
                .shadow(iced::Shadow {
                    color: Color::from_rgba8(0, 0, 0, 0.22),
                    offset: iced::Vector::new(0.0, 6.0),
                    blur_radius: 14.0,
                })
        });
        let menu =
            self.frosted_popup_surface(self.popup_backdrop.as_ref(), menu.into(), 196.0, 78.0);
        let backdrop = mouse_area(
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_press(Message::CloseFloatingMenus);
        let menu: Element<'_, Message> = float(opaque(menu))
            .translate(|_, _| Vector::new(12.0, 88.0))
            .into();
        stack(vec![backdrop.into(), menu])
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    pub(in crate::iced_ui) fn search_mode_menu_overlay(
        &self,
        pane: PaneId,
        palette: Palette,
    ) -> Element<'_, Message> {
        let option = |label: &'static str, icon: &'static str, mode: SearchMode| {
            let active = self.pane(pane).search_mode == mode;
            Button::new(
                container(
                    row![
                        inline_icon(
                            icon,
                            if active {
                                palette.accent_text
                            } else {
                                palette.muted_text
                            },
                            16.0,
                        ),
                        text(label).size(self.font_size()).color(if active {
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
            .on_press(Message::SetSearchMode(pane, mode))
            .style(move |_, status| selected_button_style(palette, active, status))
        };
        let menu = container(
            column![
                option(
                    self.localized("Búsqueda rápida", "Quick search"),
                    "folder",
                    SearchMode::Quick
                ),
                option(
                    self.localized("Búsqueda completa", "Full search"),
                    "folder-stack",
                    SearchMode::Complete
                ),
            ]
            .spacing(3)
            .padding(6),
        )
        .width(if self.split.is_some() { 210 } else { 260 })
        .style(move |_| {
            container::Style::default()
                .background(palette.menu_bg)
                .border(border::rounded(6).color(palette.border).width(1))
                .shadow(iced::Shadow {
                    color: Color::from_rgba8(0, 0, 0, 0.22),
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
        let menu_width = if self.split.is_some() { 210.0 } else { 260.0 };
        let menu =
            self.frosted_popup_surface(self.popup_backdrop.as_ref(), menu.into(), menu_width, 79.0);
        let floating_menu: Element<'_, Message> = float(opaque(menu))
            .translate(|_, _| Vector::new(14.0, -42.0))
            .into();
        let menu_layer = container(floating_menu)
            .align_left(Length::Fill)
            .align_bottom(Length::Fill)
            .into();

        stack(vec![backdrop.into(), menu_layer])
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    pub(in crate::iced_ui) fn group_menu_overlay(
        &self,
        pane: PaneId,
        palette: Palette,
    ) -> Element<'_, Message> {
        let items = column![
            self.group_mode_item(
                pane,
                GroupMode::None,
                self.localized("Ninguno", "None"),
                palette
            ),
            self.group_mode_item(
                pane,
                GroupMode::Type,
                self.localized("Tipo", "Type"),
                palette
            ),
            self.group_mode_item(
                pane,
                GroupMode::Name,
                self.localized("Nombre", "Name"),
                palette
            ),
            self.group_mode_item(
                pane,
                GroupMode::TotalSize,
                self.localized("Tamaño", "Size"),
                palette
            ),
            context_separator(palette),
            self.group_direction_item(
                pane,
                true,
                self.localized("Ascendente", "Ascending"),
                palette
            ),
            self.group_direction_item(
                pane,
                false,
                self.localized("Descendente", "Descending"),
                palette
            ),
        ];
        let menu = container(items.spacing(3).padding(6))
            .width(220)
            .style(move |_| {
                container::Style::default()
                    .background(palette.menu_bg)
                    .border(border::rounded(6).color(palette.border).width(1))
                    .shadow(iced::Shadow {
                        color: Color::from_rgba8(0, 0, 0, 0.22),
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
            self.frosted_popup_surface(self.popup_backdrop.as_ref(), menu.into(), 220.0, 223.0);
        let floating_menu: Element<'_, Message> = float(opaque(menu))
            .translate(|_, _| Vector::new(-104.0, 82.0))
            .into();

        let menu_layer = container(floating_menu)
            .align_right(Length::Fill)
            .align_top(Length::Fill)
            .into();

        stack(vec![backdrop.into(), menu_layer])
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    pub(in crate::iced_ui) fn group_mode_item(
        &self,
        pane: PaneId,
        mode: GroupMode,
        label: &'static str,
        palette: Palette,
    ) -> Element<'_, Message> {
        let active = self.effective_group_mode(pane) == mode;
        menu_choice_button(
            label,
            active,
            Message::SetGroupMode(pane, mode),
            palette,
            self.font_size(),
        )
    }

    pub(in crate::iced_ui) fn group_direction_item(
        &self,
        pane: PaneId,
        ascending: bool,
        label: &'static str,
        palette: Palette,
    ) -> Element<'_, Message> {
        let active = self.effective_group_ascending(pane) == ascending;
        menu_choice_button(
            label,
            active,
            Message::SetGroupAscending(pane, ascending),
            palette,
            self.font_size(),
        )
    }

    pub(in crate::iced_ui) fn file_table(
        &self,
        pane: PaneId,
        palette: Palette,
    ) -> Element<'_, Message> {
        match self.effective_view_mode(pane) {
            ViewMode::Details | ViewMode::List => self.detail_file_table(pane, palette),
            ViewMode::Tiles
            | ViewMode::SmallIcons
            | ViewMode::MediumIcons
            | ViewMode::LargeIcons
            | ViewMode::ExtraLargeIcons => self.visual_file_table(pane, palette),
        }
    }

    pub(in crate::iced_ui) fn localized_entry_type_label(&self, entry: &FileEntry) -> String {
        if let Some(kind) = entry.drive_kind {
            let label = if self.is_spanish() {
                match kind {
                    DriveKind::Local => "Disco local",
                    DriveKind::External => "Unidad externa",
                    DriveKind::Usb => "Unidad USB",
                    DriveKind::Network => "Unidad de red",
                    DriveKind::NetworkComputer => "Equipo de red",
                    DriveKind::NetworkPrinter => "Impresora de red",
                    DriveKind::NetworkScanner => "Escáner de red",
                    DriveKind::NetworkMultifunction => "Dispositivo multifunción de red",
                    DriveKind::NetworkDevice => "Dispositivo de red",
                    DriveKind::Portable => "Dispositivo portátil",
                    DriveKind::Optical => "Unidad óptica",
                    DriveKind::RamDisk => "Disco RAM",
                    DriveKind::Unknown => "Unidad",
                }
            } else {
                kind.label()
            };
            return if entry.file_system.trim().is_empty() {
                label.to_owned()
            } else {
                format!("{label} · {}", entry.file_system)
            };
        }

        if !self.is_spanish() {
            return entry.type_label();
        }

        match entry.kind {
            EntryKind::Drive => "Unidad".into(),
            EntryKind::Folder => "Carpeta".into(),
            EntryKind::Symlink => "Enlace simbólico".into(),
            EntryKind::File | EntryKind::Other => {
                let category = match entry.category {
                    FileCategory::Application => "Aplicación",
                    FileCategory::Image => "Imagen",
                    FileCategory::Audio => "Audio",
                    FileCategory::Video => "Vídeo",
                    FileCategory::Archive => "Archivo",
                    FileCategory::Document => "Documento",
                    FileCategory::Spreadsheet => "Hoja de cálculo",
                    FileCategory::Presentation => "Presentación",
                    FileCategory::Code => "Código fuente",
                    FileCategory::Font => "Fuente",
                    FileCategory::System => "Archivo de sistema",
                    FileCategory::DiskImage => "Imagen de disco",
                    FileCategory::Other => "Archivo",
                };
                entry
                    .path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .filter(|extension| !extension.is_empty())
                    .map(|extension| format!("{category} {}", extension.to_uppercase()))
                    .unwrap_or_else(|| category.into())
            }
        }
    }

    pub(in crate::iced_ui) fn localized_entry_group_label(
        &self,
        entry: &FileEntry,
        mode: GroupMode,
    ) -> String {
        if mode == GroupMode::Type {
            self.localized_entry_type_label(entry)
        } else {
            entry_group_label(entry, mode)
        }
    }

    pub(in crate::iced_ui) fn localized_tile_metadata_label(&self, entry: &FileEntry) -> String {
        let type_label = self.localized_entry_type_label(entry);
        if matches!(&entry.kind, EntryKind::File | EntryKind::Other)
            && let Some(size) = entry.size
        {
            return format!("{type_label} · {}", format_size(Some(size)));
        }
        type_label
    }

    pub(in crate::iced_ui) fn localized_drive_capacity_label(&self, entry: &FileEntry) -> String {
        match (entry.size, entry.free_space) {
            (Some(total), Some(free)) => format!(
                "{} {} {}",
                format_size(Some(total.saturating_sub(free))),
                self.localized("de", "of"),
                format_size(Some(total)),
            ),
            _ => self.localized_entry_type_label(entry),
        }
    }

    pub(in crate::iced_ui) fn localized_transfer_title(
        &self,
        item: &TransferDisplayState,
    ) -> &'static str {
        if self.is_spanish() {
            return transfer_title(item);
        }
        match item.state {
            TransferState::Pending => "Queued",
            TransferState::Paused => "Paused",
            TransferState::Finished => "Transfer complete",
            TransferState::Cancelled => "Transfer cancelled",
            TransferState::Failed => "Transfer failed",
            TransferState::Copying => match item.kind {
                TransferKind::Copy => "Copying",
                TransferKind::Move => "Moving",
            },
        }
    }

    pub(in crate::iced_ui) fn localized_transfer_state(
        &self,
        item: &TransferDisplayState,
    ) -> &'static str {
        if self.is_spanish() {
            return transfer_state_text(item);
        }
        match item.state {
            TransferState::Pending => "Waiting",
            TransferState::Copying => "Copying files",
            TransferState::Paused => "Paused",
            TransferState::Finished => "Completed",
            TransferState::Cancelled => "Cancelled",
            TransferState::Failed => "Error",
        }
    }

    pub(in crate::iced_ui) fn detail_file_table(
        &self,
        pane: PaneId,
        palette: Palette,
    ) -> Element<'_, Message> {
        let table_font_size = (self.font_size() - 0.5).max(11.0);
        let widths = self.detail_column_widths(pane, table_font_size);
        let table_width = widths.total_width();
        let sort_column = self.pane(pane).sort_column;
        let sort_ascending = self.pane(pane).sort_ascending;
        let header = row![
            table_header(
                self.localized("Nombre", "Name"),
                widths.name,
                pane,
                TableColumn::Name,
                true,
                sort_column == TableColumn::Name,
                sort_ascending,
                palette,
                table_font_size
            ),
            table_header(
                self.localized("Tipo", "Type"),
                widths.type_label,
                pane,
                TableColumn::Type,
                true,
                sort_column == TableColumn::Type,
                sort_ascending,
                palette,
                table_font_size
            ),
            table_header(
                self.localized("Tamaño", "Size"),
                widths.size,
                pane,
                TableColumn::Size,
                true,
                sort_column == TableColumn::Size,
                sort_ascending,
                palette,
                table_font_size
            ),
            table_header(
                self.localized("Modificado", "Modified"),
                widths.modified,
                pane,
                TableColumn::Modified,
                true,
                sort_column == TableColumn::Modified,
                sort_ascending,
                palette,
                table_font_size
            ),
            table_header(
                self.localized("Creado", "Created"),
                DETAIL_DATE_MIN_WIDTH,
                pane,
                TableColumn::Created,
                false,
                sort_column == TableColumn::Created,
                sort_ascending,
                palette,
                table_font_size
            ),
        ]
        .height(DETAIL_HEADER_HEIGHT)
        .align_y(Alignment::Center)
        .width(Length::Fixed(table_width));

        let entries = self.filtered_entries(pane);
        let total = entries.len();
        let render_limit = self.pane(pane).render_limit.min(total);
        let mut rows = column![header].width(Length::Fixed(table_width));
        let group_mode = self.effective_group_mode(pane);
        let mut current_group: Option<String> = None;
        for index in entries.into_iter().take(render_limit) {
            let Some(entry) = self.pane(pane).entries.get(index) else {
                continue;
            };
            if group_mode != GroupMode::None {
                let group = self.localized_entry_group_label(entry, group_mode);
                if current_group.as_ref() != Some(&group) {
                    current_group = Some(group.clone());
                    rows = rows.push(file_group_header(group, palette, self.font_size()));
                }
            }
            rows = rows.push(self.file_row(pane, index, entry, palette, widths));
        }
        if render_limit < total {
            rows = rows.push(render_progress_footer(
                render_limit,
                total,
                palette,
                self.font_size(),
            ));
        }

        let content = scrollable(rows)
            .id(pane_scroll_id(pane))
            .direction(scrollable::Direction::Both {
                vertical: scrollable::Scrollbar::default(),
                horizontal: scrollable::Scrollbar::default(),
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .on_scroll(move |viewport| {
                Message::PaneScrolled(
                    pane,
                    viewport.relative_offset().y,
                    viewport.absolute_offset().y,
                    viewport.content_bounds().height > viewport.bounds().height,
                )
            })
            .style(move |theme, status| {
                explorer_scrollable_style(
                    palette,
                    theme,
                    status,
                    self.pane(pane).scrollbar_reveal_progress,
                )
            });
        let base: Element<'_, Message> = container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_| {
                container::Style::default()
                    .background(palette.table_bg)
                    .border(border::color(palette.border).width(1))
            })
            .into();

        self.contextual_file_surface(
            pane,
            palette,
            self.rubber_band_layer(pane, palette, self.scrollbar_hover_layer(pane, base)),
        )
    }

    pub(in crate::iced_ui) fn scrollbar_hover_layer<'a>(
        &self,
        pane: PaneId,
        base: Element<'a, Message>,
    ) -> Element<'a, Message> {
        let vertical_zone = container(
            mouse_area(
                Space::new()
                    .width(SCROLLBAR_REVEAL_ZONE)
                    .height(Length::Fill),
            )
            .on_enter(Message::ScrollbarHover(pane, ScrollbarAxis::Vertical, true))
            .on_exit(Message::ScrollbarHover(
                pane,
                ScrollbarAxis::Vertical,
                false,
            )),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .align_right(Length::Fill);
        let horizontal_zone = container(
            mouse_area(
                Space::new()
                    .width(Length::Fill)
                    .height(SCROLLBAR_REVEAL_ZONE),
            )
            .on_enter(Message::ScrollbarHover(
                pane,
                ScrollbarAxis::Horizontal,
                true,
            ))
            .on_exit(Message::ScrollbarHover(
                pane,
                ScrollbarAxis::Horizontal,
                false,
            )),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .align_bottom(Length::Fill);

        stack(vec![base, vertical_zone.into(), horizontal_zone.into()])
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    pub(in crate::iced_ui) fn visual_file_table(
        &self,
        pane: PaneId,
        palette: Palette,
    ) -> Element<'_, Message> {
        let mode = self.effective_view_mode(pane);
        let layout = self.visual_layout_for_pane(pane, mode);
        let metrics = layout.metrics;

        let entries = self.filtered_entries(pane);
        let total = entries.len();
        let render_limit = self.pane(pane).render_limit.min(total);
        let group_mode = self.effective_group_mode(pane);
        // Group labels belong to the file surface itself, not the icon grid.
        // Keep the grid inset while allowing every label to meet the top and
        // horizontal edges of its group.
        let mut grid = column![].spacing(metrics.spacing);
        if group_mode == GroupMode::None {
            grid = grid.padding(metrics.grid_padding);
        }
        let mut row_items = row![].spacing(metrics.spacing).align_y(Alignment::Start);
        let mut col = 0;
        let mut current_group: Option<String> = None;

        for index in entries.into_iter().take(render_limit) {
            let Some(entry) = self.pane(pane).entries.get(index) else {
                continue;
            };
            if group_mode != GroupMode::None {
                let group = self.localized_entry_group_label(entry, group_mode);
                if current_group.as_ref() != Some(&group) {
                    if col > 0 {
                        grid = if group_mode == GroupMode::None {
                            grid.push(row_items)
                        } else {
                            grid.push(container(row_items).padding([0.0, metrics.grid_padding]))
                        };
                        row_items = row![].spacing(metrics.spacing).align_y(Alignment::Start);
                        col = 0;
                    }
                    current_group = Some(group.clone());
                    grid = grid.push(file_group_header(group, palette, self.font_size()));
                }
            }
            row_items = row_items.push(self.visual_file_item(pane, index, entry, palette, metrics));
            col += 1;
            if col >= layout.columns {
                grid = if group_mode == GroupMode::None {
                    grid.push(row_items)
                } else {
                    grid.push(container(row_items).padding([0.0, metrics.grid_padding]))
                };
                row_items = row![].spacing(metrics.spacing).align_y(Alignment::Start);
                col = 0;
            }
        }
        if col > 0 {
            grid = if group_mode == GroupMode::None {
                grid.push(row_items)
            } else {
                grid.push(container(row_items).padding([0.0, metrics.grid_padding]))
            };
        }
        if render_limit < total {
            grid = grid.push(render_progress_footer(
                render_limit,
                total,
                palette,
                self.font_size(),
            ));
        }

        let content = scrollable(grid)
            .id(pane_scroll_id(pane))
            .on_scroll(move |viewport| {
                Message::PaneScrolled(
                    pane,
                    viewport.relative_offset().y,
                    viewport.absolute_offset().y,
                    viewport.content_bounds().height > viewport.bounds().height,
                )
            })
            .style(move |theme, status| {
                explorer_scrollable_style(
                    palette,
                    theme,
                    status,
                    self.pane(pane).scrollbar_reveal_progress,
                )
            });
        let base: Element<'_, Message> = container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_| {
                container::Style::default()
                    .background(palette.table_bg)
                    .border(border::color(palette.border).width(1))
            })
            .into();

        self.contextual_file_surface(
            pane,
            palette,
            self.rubber_band_layer(pane, palette, self.scrollbar_hover_layer(pane, base)),
        )
    }

    pub(in crate::iced_ui) fn visual_file_item(
        &self,
        pane: PaneId,
        index: usize,
        entry: &FileEntry,
        palette: Palette,
        metrics: VisualViewMetrics,
    ) -> Element<'_, Message> {
        let selected =
            self.pane(pane).selected.contains(&entry.path) || self.is_file_drag_target(pane, index);
        let presentation_opacity = self.entry_presentation_opacity(entry, selected);
        let color = if selected {
            palette.accent_text
        } else {
            translucent_color(palette.text, presentation_opacity)
        };
        let secondary = if selected {
            palette.accent_text
        } else {
            translucent_color(palette.muted_text, presentation_opacity)
        };
        let display_name = self.entry_display_name(entry);
        let font_size = (self.font_size() - 0.4).max(11.0);
        let editing = self
            .rename_dialog
            .as_ref()
            .filter(|dialog| dialog.pane == pane && dialog.path == entry.path);

        let content: Element<'_, Message> = if metrics.tile {
            let text_width = metrics.cell_width - metrics.icon_size - 36.0;
            let is_this_pc_drive = self.is_this_pc_root(pane) && entry.kind == EntryKind::Drive;
            let name_height = if is_this_pc_drive {
                // Drive labels are a single line above their capacity bar.
                // Do not retain the regular two-line filename reservation here.
                (font_size + 6.0).ceil()
            } else {
                visual_label_height(font_size)
            };
            let name_editor: Element<'_, Message> = if let Some(dialog) = editing {
                if is_this_pc_drive {
                    inline_rename_editor(
                        dialog.value.as_str(),
                        dialog.extension.as_deref(),
                        text_width,
                        font_size,
                        palette,
                    )
                } else {
                    wrapped_inline_rename_editor(
                        &dialog.editor,
                        dialog.extension.as_deref(),
                        text_width,
                        name_height,
                        font_size,
                        palette,
                    )
                }
            } else {
                container(
                    text(two_line_ellipsize_to_width(
                        &display_name,
                        text_width,
                        font_size,
                    ))
                    .size(font_size)
                    .color(color)
                    .wrapping(iced::widget::text::Wrapping::None),
                )
                .height(name_height)
                .width(Length::Fixed(text_width))
                .into()
            };
            let metadata: Element<'_, Message> = if is_this_pc_drive {
                column![
                    drive_capacity_bar(entry.percent_full.unwrap_or(0.0), palette),
                    text(ellipsize_to_width(
                        &self.localized_drive_capacity_label(entry),
                        text_width,
                        font_size
                    ))
                    .size(font_size - 0.5)
                    .color(secondary)
                    .wrapping(iced::widget::text::Wrapping::None),
                ]
                .spacing(4)
                .width(Length::Fixed(text_width))
                .into()
            } else {
                text(ellipsize_to_width(
                    &self.localized_tile_metadata_label(entry),
                    text_width,
                    font_size,
                ))
                .size(font_size - 0.5)
                .color(secondary)
                .wrapping(iced::widget::text::Wrapping::None)
                .into()
            };
            row![
                self.file_entry_icon(
                    entry,
                    palette,
                    selected,
                    metrics.icon_size,
                    metrics.icon_size,
                    metrics.icon_size
                ),
                column![name_editor, metadata,]
                    .spacing(3)
                    .width(Length::Fixed(text_width)),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into()
        } else {
            let preview_width = (metrics.cell_width - 18.0).max(metrics.icon_size);
            let label_width = metrics.cell_width - 18.0;
            let label_height = visual_label_height(font_size);
            let name_editor: Element<'_, Message> = if let Some(dialog) = editing {
                wrapped_inline_rename_editor(
                    &dialog.editor,
                    dialog.extension.as_deref(),
                    label_width,
                    label_height,
                    font_size,
                    palette,
                )
            } else {
                container(
                    text(two_line_ellipsize_to_width(
                        &display_name,
                        label_width,
                        font_size,
                    ))
                    .size(font_size)
                    .color(color)
                    .width(Length::Fill)
                    .align_x(Horizontal::Center)
                    .wrapping(iced::widget::text::Wrapping::None),
                )
                .width(Length::Fill)
                .height(label_height)
                .center_x(Length::Fill)
                .into()
            };
            column![
                container(self.file_entry_icon(
                    entry,
                    palette,
                    selected,
                    metrics.icon_size,
                    preview_width,
                    metrics.preview_height
                ))
                .width(Length::Fill)
                .height(metrics.preview_height)
                .center_x(Length::Fill)
                .center_y(Length::Fill),
                name_editor,
            ]
            .spacing(4)
            .align_x(Horizontal::Center)
            .into()
        };

        let body = container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_y(Length::Fill);

        let item: Element<'_, Message> = if editing.is_some() {
            container(body)
                .width(metrics.cell_width)
                .height(metrics.cell_height)
                .padding(if metrics.tile { 6 } else { 8 })
                .style(move |_| {
                    let style = if selected {
                        container::Style::default().background(accent_gradient(palette))
                    } else {
                        container::Style::default().background(Color::TRANSPARENT)
                    };
                    style.border(border::rounded(4))
                })
                .into()
        } else {
            Button::new(
                mouse_area(body)
                    .on_press(Message::StartFileDrag(pane, index))
                    .on_double_click(Message::OpenEntry(pane, index))
                    .on_release(Message::StopResize)
                    .interaction(mouse::Interaction::Pointer),
            )
            .width(metrics.cell_width)
            .height(metrics.cell_height)
            .padding(if metrics.tile { 6 } else { 8 })
            .on_press(Message::RowPressed(pane, index))
            .style(move |_, status| selected_button_style(palette, selected, status))
            .into()
        };

        self.entry_context_surface(pane, index, item)
    }

    pub(in crate::iced_ui) fn file_entry_icon(
        &self,
        entry: &FileEntry,
        palette: Palette,
        selected: bool,
        icon_size: f32,
        width: f32,
        height: f32,
    ) -> Element<'static, Message> {
        if let Some(handle) = self.entry_image_handle(entry).cloned() {
            let opacity = self.entry_presentation_opacity(entry, selected);
            return container(
                iced_image::Image::new(handle)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .content_fit(ContentFit::Contain)
                    .opacity(opacity),
            )
            .width(Length::Fixed(width))
            .height(Length::Fixed(height))
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into();
        }

        let color = if selected {
            palette.accent_text
        } else if matches!(&entry.kind, EntryKind::Folder | EntryKind::Drive) {
            palette.folder
        } else {
            palette.accent
        };

        container(inline_icon(
            fallback_icon_label(entry),
            translucent_color(color, self.entry_presentation_opacity(entry, selected)),
            icon_size,
        ))
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    }

    pub(in crate::iced_ui) fn detail_file_entry_icon(
        &self,
        entry: &FileEntry,
        palette: Palette,
        selected: bool,
        size: f32,
    ) -> Element<'static, Message> {
        if let Some(handle) = self.entry_image_handle(entry).cloned() {
            let opacity = self.entry_presentation_opacity(entry, selected);
            return iced_image::Image::new(handle)
                .width(Length::Fixed(size))
                .height(Length::Fixed(size))
                .content_fit(ContentFit::Contain)
                .opacity(opacity)
                .into();
        }

        let color = if selected {
            palette.accent_text
        } else if matches!(&entry.kind, EntryKind::Folder | EntryKind::Drive) {
            palette.folder
        } else {
            palette.accent
        };
        inline_icon(
            fallback_icon_label(entry),
            translucent_color(color, self.entry_presentation_opacity(entry, selected)),
            size,
        )
    }

    pub(in crate::iced_ui) fn entry_image_handle(
        &self,
        entry: &FileEntry,
    ) -> Option<&iced_image::Handle> {
        if thumbnail_data::is_thumbnail_candidate(entry) {
            if let Some(IcedImageState::Ready(handle)) = self.thumbnail_cache.get(&entry.path) {
                return Some(handle);
            }
        }

        let (cache_key, _, _) = native_icon_request_for_entry(entry)?;
        match self.native_icon_cache.get(&cache_key) {
            Some(IcedImageState::Ready(handle)) => Some(handle),
            _ => None,
        }
    }

    pub(in crate::iced_ui) fn rubber_band_layer<'a>(
        &self,
        pane: PaneId,
        palette: Palette,
        base: Element<'a, Message>,
    ) -> Element<'a, Message> {
        let Some(drag) = self.rubber_band.as_ref().filter(|drag| drag.pane == pane) else {
            return base;
        };

        let rect = normalized_rect(drag.start, drag.current);
        if rect.width < RUBBER_BAND_MIN_SIZE && rect.height < RUBBER_BAND_MIN_SIZE {
            return base;
        }

        let overlay = float(
            container(Space::new())
                .width(Length::Fixed(rect.width.max(1.0)))
                .height(Length::Fixed(rect.height.max(1.0)))
                .style(move |_| {
                    container::Style::default()
                        .background(translucent_accent_gradient(palette, 0.18))
                        .border(
                            border::rounded(2)
                                .color(translucent_color(palette.accent, 0.72))
                                .width(1),
                        )
                }),
        )
        .translate(move |_, _| Vector::new(rect.x, rect.y))
        .into();

        stack(vec![base, overlay])
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    pub(in crate::iced_ui) fn visual_layout_for_pane(
        &self,
        pane: PaneId,
        mode: ViewMode,
    ) -> VisualLayout {
        let mut metrics = visual_view_metrics(mode);
        if mode == ViewMode::Tiles && self.is_this_pc_root(pane) {
            // Drive tiles include a capacity bar, so retain a uniform taller row.
            metrics.cell_height = 90.0;
        }
        let surface_width = self.file_surface_width(pane);
        let usable_width = (surface_width - metrics.grid_padding * 2.0).max(1.0);

        if mode == ViewMode::Tiles {
            let columns = ((usable_width + metrics.spacing)
                / (metrics.cell_width + metrics.spacing))
                .floor()
                .max(1.0) as usize;
            if columns == 1 {
                metrics.cell_width = metrics.cell_width.min(usable_width);
            }
            return VisualLayout { metrics, columns };
        }

        let min_width = visual_min_cell_width(mode).min(usable_width);
        let columns = ((usable_width + metrics.spacing) / (min_width + metrics.spacing))
            .floor()
            .max(1.0) as usize;
        let cell_width =
            (usable_width - metrics.spacing * columns.saturating_sub(1) as f32) / columns as f32;
        metrics.cell_width = cell_width.max(min_width);

        if !metrics.tile {
            let font_size = (self.font_size() - 0.4).max(11.0);
            let label_height = visual_label_height(font_size);
            let target_preview_height = match mode {
                ViewMode::LargeIcons => (metrics.cell_width * 0.56).clamp(112.0, 162.0),
                ViewMode::ExtraLargeIcons => (metrics.cell_width * 0.58).clamp(184.0, 252.0),
                ViewMode::MediumIcons => (metrics.cell_width * 0.48).clamp(70.0, 92.0),
                ViewMode::SmallIcons | ViewMode::List => metrics.preview_height,
                ViewMode::Details | ViewMode::Tiles => metrics.preview_height,
            };
            metrics.preview_height = target_preview_height;
            metrics.cell_height =
                (metrics.preview_height + label_height + 24.0).max(metrics.cell_height);
        }

        VisualLayout { metrics, columns }
    }

    pub(in crate::iced_ui) fn detail_column_widths(
        &self,
        pane: PaneId,
        font_size: f32,
    ) -> DetailColumnWidths {
        let mut name_chars = "Nombre".chars().count();
        let mut type_chars = "Tipo".chars().count();
        let mut size_chars = "Tamano".chars().count();
        let mut modified_chars = "Modificado".chars().count();

        for index in self.filtered_entries(pane).into_iter().take(400) {
            let Some(entry) = self.pane(pane).entries.get(index) else {
                continue;
            };
            name_chars = name_chars.max(self.entry_display_name(entry).chars().count());
            type_chars = type_chars.max(self.localized_entry_type_label(entry).chars().count());
            size_chars = size_chars.max(format_size(entry.size).chars().count());
            modified_chars = modified_chars.max(
                entry
                    .modified
                    .as_deref()
                    .unwrap_or_default()
                    .chars()
                    .count(),
            );
        }

        let auto = DetailColumnWidths {
            name: estimated_column_width(
                name_chars,
                font_size,
                DETAIL_ICON_SIZE + 7.0,
                DETAIL_NAME_MIN_WIDTH,
                DETAIL_NAME_MAX_WIDTH,
            ),
            type_label: estimated_column_width(
                type_chars,
                font_size,
                1.0,
                DETAIL_TYPE_MIN_WIDTH,
                DETAIL_TYPE_MAX_WIDTH,
            ),
            size: estimated_column_width(
                size_chars,
                font_size,
                1.0,
                DETAIL_SIZE_MIN_WIDTH,
                DETAIL_SIZE_MAX_WIDTH,
            ),
            modified: estimated_column_width(
                modified_chars,
                font_size,
                1.0,
                DETAIL_DATE_MIN_WIDTH,
                DETAIL_DATE_MAX_WIDTH,
            ),
        };
        let overrides = self.pane(pane).column_widths;
        DetailColumnWidths {
            name: overrides
                .get(TableColumn::Name)
                .unwrap_or(auto.name)
                .clamp(DETAIL_NAME_MIN_WIDTH, DETAIL_NAME_MAX_WIDTH),
            type_label: overrides
                .get(TableColumn::Type)
                .unwrap_or(auto.type_label)
                .clamp(DETAIL_TYPE_MIN_WIDTH, DETAIL_TYPE_MAX_WIDTH),
            size: overrides
                .get(TableColumn::Size)
                .unwrap_or(auto.size)
                .clamp(DETAIL_SIZE_MIN_WIDTH, DETAIL_SIZE_MAX_WIDTH),
            modified: overrides
                .get(TableColumn::Modified)
                .unwrap_or(auto.modified)
                .clamp(DETAIL_DATE_MIN_WIDTH, DETAIL_DATE_MAX_WIDTH),
        }
    }

    pub(in crate::iced_ui) fn file_row(
        &self,
        pane: PaneId,
        index: usize,
        entry: &FileEntry,
        palette: Palette,
        widths: DetailColumnWidths,
    ) -> Element<'_, Message> {
        let selected =
            self.pane(pane).selected.contains(&entry.path) || self.is_file_drag_target(pane, index);
        let presentation_opacity = self.entry_presentation_opacity(entry, selected);
        let table_font_size = (self.font_size() - 0.5).max(11.0);
        let text_color = if selected {
            palette.accent_text
        } else {
            translucent_color(palette.text, presentation_opacity)
        };
        let meta_color = if selected {
            palette.accent_text
        } else {
            translucent_color(palette.muted_text, presentation_opacity)
        };
        let editing = self
            .rename_dialog
            .as_ref()
            .filter(|dialog| dialog.pane == pane && dialog.path == entry.path);
        let name_cell: Element<'_, Message> = if let Some(dialog) = editing {
            row![
                self.detail_file_entry_icon(entry, palette, selected, DETAIL_ICON_SIZE),
                inline_rename_editor(
                    dialog.value.as_str(),
                    dialog.extension.as_deref(),
                    widths.name - DETAIL_ICON_SIZE - 6.0,
                    table_font_size,
                    palette,
                ),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
            .width(Length::Fixed(widths.name))
            .into()
        } else {
            let name = ellipsize_to_width(
                &self.entry_display_name(entry),
                widths.name - DETAIL_ICON_SIZE - 1.0,
                table_font_size,
            );
            row![
                self.detail_file_entry_icon(entry, palette, selected, DETAIL_ICON_SIZE),
                text(name)
                    .size(table_font_size)
                    .color(text_color)
                    .width(Length::Fill)
                    .wrapping(iced::widget::text::Wrapping::None),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
            .width(Length::Fixed(widths.name))
            .into()
        };
        let type_label = ellipsize_to_width(
            &self.localized_entry_type_label(entry),
            widths.type_label - 1.0,
            table_font_size,
        );
        let modified = entry.modified.clone().unwrap_or_default();
        let created = entry.created.clone().unwrap_or_default();
        let row = row![
            name_cell,
            text(type_label)
                .size(table_font_size)
                .color(meta_color)
                .wrapping(iced::widget::text::Wrapping::None)
                .width(Length::Fixed(widths.type_label)),
            text(format_size(entry.size))
                .size(table_font_size)
                .color(meta_color)
                .wrapping(iced::widget::text::Wrapping::None)
                .width(Length::Fixed(widths.size)),
            text(ellipsize_to_width(
                &modified,
                widths.modified - 1.0,
                table_font_size
            ))
            .size(table_font_size)
            .color(meta_color)
            .wrapping(iced::widget::text::Wrapping::None)
            .width(Length::Fixed(widths.modified)),
            text(created)
                .size(table_font_size)
                .color(meta_color)
                .wrapping(iced::widget::text::Wrapping::None)
                .width(Length::Fixed(DETAIL_DATE_MIN_WIDTH)),
        ]
        .height(DETAIL_ROW_HEIGHT)
        .padding([3, 8])
        .align_y(Alignment::Center)
        .width(Length::Fixed(widths.total_width()));

        let row_content: Element<'_, Message> = if editing.is_some() {
            container(row)
                .width(Length::Fixed(widths.total_width()))
                .style(move |_| row_background_style(palette, selected))
                .into()
        } else {
            Button::new(
                mouse_area(row)
                    .on_press(Message::StartFileDrag(pane, index))
                    .on_double_click(Message::OpenEntry(pane, index))
                    .on_release(Message::StopResize)
                    .interaction(mouse::Interaction::Pointer),
            )
            .padding(0)
            .width(Length::Fixed(widths.total_width()))
            .on_press(Message::RowPressed(pane, index))
            .style(move |_, status| selected_button_style(palette, selected, status))
            .into()
        };

        self.entry_context_surface(pane, index, row_content)
    }

    pub(in crate::iced_ui) fn entry_context_surface<'a>(
        &self,
        pane: PaneId,
        index: usize,
        content: Element<'a, Message>,
    ) -> Element<'a, Message> {
        mouse_area(content)
            .on_right_press(Message::OpenEntryContext(pane, index))
            .on_enter(Message::FileDragTargetEnter(pane, index))
            .on_exit(Message::FileDragTargetExit(pane, index))
            .interaction(mouse::Interaction::Pointer)
            .into()
    }
}
