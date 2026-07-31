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
        let toolbar_height = self.toolbar_height();
        let action_bar_height = self.action_bar_height();
        let status_bar_height = self.status_bar_height();
        let filter_height = self.filter_control_height();
        let trash_view = self.is_trash_pane(pane);
        let toolbar = row![
            delayed_title_tooltip(
                icon_button(
                    "back",
                    Message::Back(pane),
                    palette,
                    false,
                    self.font_size(),
                ),
                self.localized("Atrás", "Back"),
                palette,
                self.font_size(),
            ),
            delayed_title_tooltip(
                icon_button(
                    "next",
                    Message::Forward(pane),
                    palette,
                    false,
                    self.font_size(),
                ),
                self.localized("Adelante", "Forward"),
                palette,
                self.font_size(),
            ),
            delayed_title_tooltip(
                icon_button("up", Message::Up(pane), palette, false, self.font_size(),),
                self.localized("Subir una carpeta", "Go up"),
                palette,
                self.font_size(),
            ),
            delayed_title_tooltip(
                icon_button(
                    "bookmark",
                    Message::ToggleFavorite(pane),
                    palette,
                    favorite_active,
                    self.font_size(),
                ),
                if favorite_active {
                    self.localized("Quitar de marcadores", "Remove bookmark")
                } else {
                    self.localized("Añadir a marcadores", "Add bookmark")
                },
                palette,
                self.font_size(),
            ),
            self.address_bar(pane, palette),
            delayed_title_tooltip(
                icon_button(
                    "refresh",
                    Message::Refresh(pane),
                    palette,
                    false,
                    self.font_size(),
                ),
                self.localized("Actualizar", "Refresh"),
                palette,
                self.font_size(),
            ),
        ]
        .height(toolbar_height)
        .spacing(self.ui_metric(4.0))
        .align_y(Alignment::Center)
        .padding([4.0, self.ui_metric(10.0)]);

        let undo_action: Element<'_, Message> = if self.last_undo_action.is_some() {
            tool_button(
                self.localized("Deshacer", "Undo"),
                Message::UndoLastAction,
                palette,
                false,
                false,
                self.font_size(),
            )
            .into()
        } else {
            Space::new().width(0).into()
        };
        let action_bar_content: Element<'_, Message> = if trash_view {
            row![
                tool_button(
                    self.localized("Restaurar", "Restore"),
                    Message::RestoreTrashSelected(pane),
                    palette,
                    false,
                    false,
                    self.font_size(),
                ),
                tool_button(
                    self.localized("Eliminar", "Delete"),
                    Message::DeleteTrashSelected(pane),
                    palette,
                    false,
                    false,
                    self.font_size(),
                ),
                tool_button(
                    self.localized("Vaciar papelera", "Empty Recycle Bin"),
                    Message::EmptyTrash(pane),
                    palette,
                    false,
                    false,
                    self.font_size(),
                ),
                Space::new().width(Length::Fill),
                tool_button(
                    self.localized("Agrupar", "Group"),
                    Message::ToggleGroupMenu(pane),
                    palette,
                    self.group_menu_open == Some(pane),
                    self.split.is_some(),
                    self.font_size(),
                ),
                tool_button(
                    self.localized("Vista previa", "Preview"),
                    Message::TogglePreviewPanel(pane),
                    palette,
                    self.preview_panel_visible(pane),
                    self.split.is_some(),
                    self.font_size(),
                ),
            ]
            .height(action_bar_height)
            .spacing(self.ui_metric(4.0))
            .padding([5.0, self.ui_metric(12.0)])
            .align_y(Alignment::Center)
            .width(Length::Fill)
            .into()
        } else {
            row![
                tool_button(
                    self.localized("Nuevo", "New"),
                    Message::ToggleNewMenu(pane),
                    palette,
                    self.new_menu_open == Some(pane),
                    false,
                    self.font_size(),
                ),
                undo_action,
                tool_button(
                    self.localized("Pegar", "Paste"),
                    Message::PasteIntoPane(pane),
                    palette,
                    false,
                    false,
                    self.font_size(),
                ),
                tool_button(
                    self.localized("Copiar", "Copy"),
                    Message::CopySelection(pane),
                    palette,
                    false,
                    false,
                    self.font_size(),
                ),
                tool_button(
                    self.localized("Cortar", "Cut"),
                    Message::CutSelection(pane),
                    palette,
                    false,
                    false,
                    self.font_size(),
                ),
                tool_button(
                    self.localized("Renombrar", "Rename"),
                    Message::RenameSelected(pane),
                    palette,
                    false,
                    false,
                    self.font_size(),
                ),
                tool_button(
                    self.localized("Eliminar", "Delete"),
                    Message::DeleteSelected(pane),
                    palette,
                    false,
                    false,
                    self.font_size(),
                ),
                tool_button(
                    self.localized("Comprimir", "Compress"),
                    Message::OpenArchiveDialog(pane),
                    palette,
                    false,
                    false,
                    self.font_size(),
                ),
                Space::new().width(Length::Fill),
                tool_button(
                    self.localized("Agrupar", "Group"),
                    Message::ToggleGroupMenu(pane),
                    palette,
                    self.group_menu_open == Some(pane),
                    self.split.is_some(),
                    self.font_size(),
                ),
                tool_button(
                    self.localized("Vista previa", "Preview"),
                    Message::TogglePreviewPanel(pane),
                    palette,
                    self.preview_panel_visible(pane),
                    self.split.is_some(),
                    self.font_size(),
                ),
            ]
            .height(action_bar_height)
            .spacing(self.ui_metric(4.0))
            .padding([5.0, self.ui_metric(12.0)])
            .align_y(Alignment::Center)
            .width(Length::Fill)
            .into()
        };
        let action_bar: Element<'_, Message> = container(action_bar_content)
            .width(Length::Fill)
            .height(action_bar_height)
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
        .padding(
            Padding::new(6.0)
                .left(self.ui_metric(9.0))
                .right(self.ui_metric(38.0)),
        )
        .width(Length::Fill)
        .style(move |_, status| {
            let border_color = if matches!(status, iced::widget::text_input::Status::Focused { .. })
            {
                palette.accent
            } else {
                palette.strong_border
            };
            iced::widget::text_input::Style {
                background: chrome_glass_background(palette, palette.input_bg).into(),
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
                self.ui_metric(17.0),
            ))
            .width(Length::Fill)
            .height(Length::Fill)
            .center(Length::Fill),
        )
        .width(self.ui_metric(28.0))
        .height(self.ui_metric(26.0))
        .padding(0)
        .on_press(Message::ToggleSearchModeMenu(pane))
        .style(move |_, status| {
            selected_button_style(palette, self.search_mode_menu_open == Some(pane), status)
        });
        let filter_width = if self.split.is_some() { 210.0 } else { 260.0 };
        let filter = stack(vec![
            search_input.into(),
            container(search_mode_button)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_right(Length::Fill)
                .center_y(Length::Fill)
                .padding([0.0, self.ui_metric(3.0)])
                .into(),
        ])
        .width(self.ui_metric(filter_width))
        .height(filter_height);

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
        let pane_status = {
            let state = self.pane(pane);
            if is_entry_count_status(&state.status, state.entries.len()) {
                localized_entry_count(state.entries.len(), self.is_spanish())
            } else {
                state.status.clone()
            }
        };

        let status_content = row![
            filter,
            text(pane_status)
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
        .height(status_bar_height)
        .spacing(self.ui_metric(8.0))
        .padding([2.0, self.ui_metric(14.0)])
        .align_y(Alignment::Center);

        let transfer_active = self.transfer_in_progress_for(pane);
        let formatting = self.pane(pane).formatting;
        let search_active = self.pane(pane).search_receiver.is_some();
        let loading_active = self.pane(pane).loading || self.pane(pane).mounting_disk_image;
        let progress_bar: Element<'_, Message> = if transfer_active {
            let progress = self.transfer_progress_fraction_for(pane).unwrap_or(0.0);
            let progress_height = self.ui_metric(2.0);
            row![
                iced::widget::progress_bar(0.0..=1.0, progress)
                    .girth(progress_height)
                    .style(move |_| iced::widget::progress_bar::Style {
                        background: translucent_color(palette.border, 0.72).into(),
                        bar: accent_gradient(palette).into(),
                        border: border::rounded(0),
                    }),
            ]
            .width(Length::Fill)
            .height(Length::Fixed(progress_height))
            .into()
        } else if !formatting && (search_active || loading_active) {
            let progress_height = self.ui_metric(2.0);
            row![indeterminate_progress_bar(
                self.pane(pane).search_progress_phase,
                palette,
                progress_height
            )]
            .width(Length::Fill)
            .height(Length::Fixed(progress_height))
            .into()
        } else {
            row![].width(Length::Fill).height(Length::Fixed(0.0)).into()
        };
        // Keep the filter row keyed separately from the progress indicator.
        // Search batches change the indicator's widget type, but that must not
        // recreate the text input and drop its keyboard focus.
        let status: Element<'_, Message> = iced::widget::keyed::Column::with_children(vec![
            (0_u8, progress_bar),
            (1_u8, status_content.into()),
        ])
        .spacing(0)
        .width(Length::Fill)
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
        let popup_palette = palette.with_opacity(self.popup_fade_progress);

        if self.new_menu_open == Some(pane) {
            stack(vec![pane_body, self.new_menu_overlay(pane, popup_palette)])
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else if self.view_menu_open == Some(pane) {
            stack(vec![pane_body, self.view_menu_overlay(pane, popup_palette)])
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else if self.group_menu_open == Some(pane) {
            stack(vec![
                pane_body,
                self.group_menu_overlay(pane, popup_palette),
            ])
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else if self.search_mode_menu_open == Some(pane) {
            stack(vec![
                pane_body,
                self.search_mode_menu_overlay(pane, popup_palette),
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
        include_storage_shortcuts: bool,
    ) -> Element<'_, Message> {
        let icon_size = self.ui_metric(18.0);
        let button_height = bookmark_button_height(self.font_size());
        let mut bookmarks = row![]
            .spacing(self.ui_metric(6.0))
            .align_y(Alignment::Center)
            .height(Length::Fill);
        if include_storage_shortcuts {
            if self.sidebar_storage_entries.is_empty() {
                let filesystem = filesystem_root_path();
                bookmarks = bookmarks.push(self.bookmark_storage_button(
                    pane,
                    palette,
                    filesystem,
                    filesystem_root_label(),
                    "storage",
                ));
            } else {
                for entry in &self.sidebar_storage_entries {
                    bookmarks = bookmarks.push(self.bookmark_storage_button(
                        pane,
                        palette,
                        entry.path.clone(),
                        entry.name.clone(),
                        fallback_icon_label(entry),
                    ));
                }
            }
        }

        if self.config.favorites.is_empty() && !include_storage_shortcuts {
            bookmarks = bookmarks.push(
                text(self.localized("Marcadores", "Bookmarks"))
                    .size(self.font_size())
                    .color(palette.muted_text),
            );
        }

        for path in self.config.favorites.iter().take(6) {
            if include_storage_shortcuts
                && self
                    .sidebar_storage_entries
                    .iter()
                    .any(|entry| entry.path == *path)
            {
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
                    container(
                        row![
                            inline_icon("folder", palette.folder, icon_size),
                            text(ellipsize_text(&label, 18))
                                .size(self.font_size())
                                .color(palette.text)
                                .wrapping(iced::widget::text::Wrapping::None),
                        ]
                        .spacing(self.ui_metric(6.0))
                        .align_y(Alignment::Center),
                    )
                    .center_y(Length::Fill),
                )
                .height(button_height)
                .padding([0.0, self.ui_metric(10.0)])
                .on_press(Message::Navigate(pane, Some(target)))
                .style(move |_, status| button_style(palette, false, status)),
            );
        }

        let bookmarks = scrollable(bookmarks)
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::default(),
            ))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |theme, status| explorer_scrollable_style(palette, theme, status, 0.0));

        container(bookmarks)
            .width(Length::Fill)
            .height(self.bookmark_bar_height())
            .padding([5.0, self.ui_metric(12.0)])
            .clip(true)
            .style(move |_| {
                container::Style::default()
                    .background(palette.sidebar_bg)
                    .border(border::color(palette.border).width(1))
            })
            .into()
    }

    fn bookmark_storage_button(
        &self,
        pane: PaneId,
        palette: Palette,
        path: PathBuf,
        label: String,
        fallback_icon: &'static str,
    ) -> Element<'_, Message> {
        let active = self.tab_for_pane(pane).path.as_ref() == Some(&path);
        let icon_size = self.ui_metric(18.0);
        let icon_layout_size = rendered_inline_icon_size(icon_size);
        let storage_icon: Element<'_, Message> = self
            .sidebar_directory_icon_handle(&path)
            .map(|handle| {
                iced_image::Image::new(handle)
                    .width(icon_layout_size)
                    .height(icon_layout_size)
                    .content_fit(ContentFit::Contain)
                    .into()
            })
            .unwrap_or_else(|| inline_icon(fallback_icon, palette.accent, icon_size));

        Button::new(
            container(
                row![
                    storage_icon,
                    text(ellipsize_text(&label, 22))
                        .size(self.font_size())
                        .color(if active {
                            palette.accent_text
                        } else {
                            palette.text
                        })
                        .wrapping(iced::widget::text::Wrapping::None),
                ]
                .spacing(self.ui_metric(6.0))
                .align_y(Alignment::Center),
            )
            .center_y(Length::Fill),
        )
        .height(bookmark_button_height(self.font_size()))
        .padding([0.0, self.ui_metric(10.0)])
        .on_press(Message::Navigate(pane, Some(path)))
        .style(move |_, status| selected_button_style(palette, active, status))
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
                        let action_path = entry.path.clone();
                        let copy_path = entry.path.clone();
                        text_editor::TextEditor::new(&text_preview.content)
                            .on_action(move |action| {
                                Message::TextPreviewAction(pane, action_path.clone(), action)
                            })
                            // Preserve the editor's complete key map so it
                            // captures Enter/Delete too; mutations themselves
                            // are discarded in `TextPreviewAction`.
                            .key_binding(move |key_press| {
                                if matches!(
                                    rename_clipboard_shortcut_from_key(
                                        &key_press.key,
                                        key_press.physical_key,
                                        key_press.modifiers,
                                    ),
                                    Some(
                                        RenameClipboardShortcut::Copy
                                            | RenameClipboardShortcut::Cut
                                    )
                                ) {
                                    Some(text_editor::Binding::Custom(Message::TextPreviewCopy(
                                        pane,
                                        copy_path.clone(),
                                    )))
                                } else {
                                    text_editor::Binding::from_key_press(key_press)
                                }
                            })
                            .size(self.font_size())
                            .padding(self.ui_vertical_padding(8.0))
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
                    container(self.detail_file_entry_icon(
                        &entry,
                        palette,
                        false,
                        self.ui_metric(72.0),
                    ))
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
                        Length::Fixed(self.ui_metric(318.0))
                    })
                    .padding(if is_text_document {
                        0.0
                    } else {
                        self.ui_vertical_padding(4.0)
                    })
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
                    .spacing(self.ui_metric(8.0))
                    .into()
                }
            }
        } else {
            container(
                column![
                    inline_icon("preview", palette.muted_text, self.ui_metric(42.0)),
                    text(self.localized(
                        "Selecciona un archivo para ver su vista previa",
                        "Select a file to preview it",
                    ))
                    .size(self.font_size())
                    .color(palette.muted_text)
                    .align_x(Horizontal::Center)
                    .wrapping(iced::widget::text::Wrapping::Word),
                ]
                .spacing(self.ui_metric(12.0))
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
                        icon_button(
                            "x",
                            Message::TogglePreviewPanel(pane),
                            palette,
                            false,
                            self.font_size(),
                        ),
                    ]
                    .align_y(Alignment::Center),
                )
                .padding([self.ui_vertical_padding(8.0), self.ui_metric(10.0),])
                .style(move |_| {
                    container::Style::default()
                        .background(palette.header_bg)
                        .border(border::rounded(6).color(palette.border).width(1))
                }),
                container(body)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .padding(self.ui_vertical_padding(if full_size_document_preview {
                        4.0
                    } else {
                        8.0
                    })),
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
        let mut pages = column![].spacing(self.ui_metric(14.0));

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
                    .height(Length::Fixed(page_height + self.ui_metric(16.0)))
                    .padding(self.ui_vertical_padding(8.0))
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
                    .padding(self.ui_vertical_padding(12.0))
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
                .padding(self.ui_vertical_padding(12.0))
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
            .padding([self.ui_vertical_padding(8.0), 0.0])
            .center_x(Length::Fill),
        ]
        .height(Length::Fill)
        .spacing(self.ui_metric(4.0))
        .into()
    }
}
