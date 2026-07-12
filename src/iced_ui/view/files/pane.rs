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
}
