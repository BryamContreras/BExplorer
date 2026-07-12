use super::*;
use iced::widget::{column, row};

impl BExplorerIced {
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
}
