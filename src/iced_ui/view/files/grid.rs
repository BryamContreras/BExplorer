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

        let group_mode = self.effective_group_mode(pane);
        let group_starts = if group_mode == GroupMode::None {
            Vec::new()
        } else {
            self.filtered_entry_group_starts(pane)
        };
        let entries = self.filtered_entries_ref(pane);
        let total = entries.len();
        let state = self.pane(pane);
        let row_extent = metrics.cell_height + metrics.spacing;
        let mut grid = column![];
        if group_mode == GroupMode::None {
            let row_count = total.div_ceil(layout.columns);
            let range = virtual_table_range(
                row_count,
                row_extent,
                (state.scroll_offset_y - metrics.grid_padding).max(0.0),
                state.scroll_viewport_height,
                state.scroll_velocity_y,
            );
            let before = metrics.grid_padding + range.before;
            if before > 0.0 {
                grid = grid.push(Space::new().height(Length::Fixed(before)));
            }
            for row_index in range.start..range.end {
                let start = row_index * layout.columns;
                let end = (start + layout.columns).min(total);
                grid = grid.push(self.visual_file_grid_row(
                    pane,
                    &entries[start..end],
                    palette,
                    metrics,
                    row_extent,
                ));
            }
            let after = range.after + metrics.grid_padding;
            if after > 0.0 {
                grid = grid.push(Space::new().height(Length::Fixed(after)));
            }
        } else {
            let group_extent = self.detail_group_height() + metrics.spacing;
            let (window_start, window_end) = virtual_table_pixel_window(
                state.scroll_offset_y,
                state.scroll_viewport_height,
                state.scroll_velocity_y,
                row_extent,
            );
            let mut y = 0.0_f32;
            let mut rendered_start = None;
            let mut rendered_end = 0.0_f32;
            for (group_index, &group_start) in group_starts.iter().enumerate() {
                let group_end = group_starts.get(group_index + 1).copied().unwrap_or(total);
                let group_rows = (group_end - group_start).div_ceil(layout.columns);
                let block_height = group_extent + group_rows as f32 * row_extent;
                if y + block_height < window_start {
                    y += block_height;
                    continue;
                }
                if y > window_end {
                    break;
                }

                let header_start = y;
                let header_end = header_start + group_extent;
                if header_end >= window_start && header_start <= window_end {
                    if rendered_start.is_none() {
                        rendered_start = Some(header_start);
                        if header_start > 0.0 {
                            grid = grid.push(Space::new().height(Length::Fixed(header_start)));
                        }
                    }
                    let entry_index = entries[group_start];
                    if let Some(entry) = self.pane(pane).entries.get(entry_index) {
                        grid = grid.push(
                            container(file_group_header(
                                self.localized_entry_group_label(entry, group_mode),
                                palette,
                                self.font_size(),
                                self.detail_group_height(),
                            ))
                            .height(Length::Fixed(group_extent))
                            .align_y(Alignment::Start),
                        );
                        rendered_end = header_end;
                    }
                }

                let rows_start = header_end;
                let first_row = (((window_start - rows_start).max(0.0) / row_extent).floor()
                    as usize)
                    .min(group_rows);
                let last_row = (((window_end - rows_start).max(0.0) / row_extent).ceil() as usize)
                    .saturating_add(1)
                    .min(group_rows);
                for row_index in first_row..last_row {
                    let item_start = rows_start + row_index as f32 * row_extent;
                    if rendered_start.is_none() {
                        rendered_start = Some(item_start);
                        if item_start > 0.0 {
                            grid = grid.push(Space::new().height(Length::Fixed(item_start)));
                        }
                    }
                    let start = group_start + row_index * layout.columns;
                    let end = (start + layout.columns).min(group_end);
                    grid = grid.push(self.visual_file_grid_row(
                        pane,
                        &entries[start..end],
                        palette,
                        metrics,
                        row_extent,
                    ));
                    rendered_end = item_start + row_extent;
                }
                y += block_height;
            }
            let total_height = group_starts
                .iter()
                .enumerate()
                .map(|(group_index, group_start)| {
                    let group_end = group_starts.get(group_index + 1).copied().unwrap_or(total);
                    group_extent
                        + (group_end - *group_start).div_ceil(layout.columns) as f32 * row_extent
                })
                .sum::<f32>();
            if rendered_start.is_none() && total_height > 0.0 {
                grid = grid.push(Space::new().height(Length::Fixed(total_height)));
            } else if rendered_end < total_height {
                grid = grid.push(Space::new().height(Length::Fixed(total_height - rendered_end)));
            }
        }

        // Keep the Scrollable mounted when Ctrl changes. Replacing it with a
        // container reset its internal offset and made a lone Ctrl press jump
        // to the top. A stable overlay captures only Ctrl+wheel for view zoom.
        let pane_state = self.pane(pane);
        let scrollbar_reveal_progress = pane_state.scrollbar_reveal_progress;
        let vertical_scrollbar_expansion = f32::from(pane_state.scrollbar_vertical_hovered);
        let scroller: Element<'_, Message> = scrollable(grid)
            .id(pane_scroll_id(pane))
            .direction(scrollable::Direction::Vertical(explorer_scrollbar(
                vertical_scrollbar_expansion,
            )))
            .on_scroll(move |viewport| {
                Message::PaneScrolled(
                    pane,
                    viewport.absolute_offset().y,
                    viewport.bounds().height,
                    viewport.content_bounds().height > viewport.bounds().height,
                )
            })
            .style(move |theme, status| {
                explorer_scrollable_style(palette, theme, status, scrollbar_reveal_progress)
            })
            .into();
        let content: Element<'_, Message> = stack(vec![
            scroller,
            pane_ctrl_wheel_overlay(pane, self.current_modifiers.control()),
        ])
        .width(Length::Fill)
        .height(Length::Fill)
        .into();
        let content_background = self.file_content_background(palette);
        let base: Element<'_, Message> = container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_| {
                container::Style::default()
                    .background(content_background)
                    .border(border::color(palette.border).width(1))
            })
            .into();

        self.contextual_file_surface(
            pane,
            palette,
            self.rubber_band_layer(pane, palette, self.scrollbar_hover_layer(pane, base)),
        )
    }

    fn visual_file_grid_row<'a>(
        &'a self,
        pane: PaneId,
        indices: &[usize],
        palette: Palette,
        metrics: VisualViewMetrics,
        row_extent: f32,
    ) -> Element<'a, Message> {
        let mut items = row![].spacing(metrics.spacing).align_y(Alignment::Start);
        for &index in indices {
            if let Some(entry) = self.pane(pane).entries.get(index) {
                items = items.push(self.visual_file_item(pane, index, entry, palette, metrics));
            }
        }
        container(items)
            .padding([0.0, metrics.grid_padding])
            .width(Length::Fill)
            .height(Length::Fixed(row_extent))
            .align_y(Alignment::Start)
            .into()
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
            translucent_color(
                selected_item_text_color(palette, false),
                presentation_opacity,
            )
        } else {
            translucent_color(palette.text, presentation_opacity)
        };
        let secondary = if selected {
            translucent_color(
                selected_item_text_color(palette, true),
                presentation_opacity,
            )
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
            let is_portable_drive = self.is_this_pc_root(pane)
                && entry.kind == EntryKind::Drive
                && entry.drive_kind == Some(DriveKind::Portable);
            // Portable devices do not have a capacity bar, so their label can
            // use the spare horizontal room normally reserved for one.
            let text_width = metrics.cell_width
                - metrics.icon_size
                - self.ui_metric(if is_portable_drive { 24.0 } else { 36.0 });
            let is_this_pc_drive =
                self.is_this_pc_root(pane) && entry.kind == EntryKind::Drive && !is_portable_drive;
            let name_height = if is_this_pc_drive {
                // Drive labels are a single line above their capacity bar.
                // Do not retain the regular two-line filename reservation here.
                (font_size + 6.0).ceil()
            } else {
                visual_label_height(font_size)
            };
            let name_editor: Element<'_, Message> = if let Some(dialog) = editing {
                if is_this_pc_drive {
                    inline_rename_editor(&dialog.editor, text_width, font_size, palette)
                } else {
                    wrapped_inline_rename_editor(
                        &dialog.editor,
                        text_width,
                        name_height,
                        font_size,
                        palette,
                        false,
                    )
                }
            } else {
                container(
                    highlighted_search_text(
                        self.pane(pane).search_text.as_str(),
                        &two_line_ellipsize_to_width(&display_name, text_width, font_size),
                        color,
                    )
                    .size(font_size)
                    .width(Length::Fill)
                    .wrapping(iced::widget::text::Wrapping::None),
                )
                .height(name_height)
                .width(Length::Fixed(text_width))
                .into()
            };
            let metadata: Element<'_, Message> = if is_this_pc_drive {
                let formatting_drive = self.pane(pane).formatting
                    && self.pane(pane).formatting_path.as_deref() == Some(entry.path.as_path());
                let capacity_indicator: Element<'_, Message> = if formatting_drive {
                    drive_formatting_bar(
                        self.pane(pane).search_progress_phase,
                        palette,
                        selected,
                        self.font_size(),
                    )
                } else {
                    drive_capacity_bar(
                        entry.percent_full.unwrap_or(0.0),
                        palette,
                        selected,
                        self.font_size(),
                    )
                };
                let capacity_label = if formatting_drive {
                    self.localized("Formateando...", "Formatting...").to_owned()
                } else {
                    self.localized_drive_capacity_label(entry)
                };
                column![
                    capacity_indicator,
                    text(ellipsize_to_width(&capacity_label, text_width, font_size))
                        .size(font_size - 0.5)
                        .color(secondary)
                        .wrapping(iced::widget::text::Wrapping::None),
                ]
                .spacing(self.ui_metric(4.0))
                .width(Length::Fixed(text_width))
                .into()
            } else {
                // A portable tile needs a readable device type rather than
                // its protocol name. It deliberately has enough room for the
                // complete localized label.
                let metadata_label = if is_portable_drive {
                    self.localized("Dispositivo portátil", "Portable device")
                        .to_owned()
                } else {
                    self.localized_tile_metadata_label(entry)
                };
                let metadata_label = if is_portable_drive {
                    metadata_label
                } else {
                    ellipsize_to_width(&metadata_label, text_width, font_size)
                };
                text(metadata_label)
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
                    .spacing(self.ui_metric(3.0))
                    .width(Length::Fixed(text_width)),
            ]
            .spacing(self.ui_metric(8.0))
            .align_y(Alignment::Center)
            .into()
        } else {
            let content_inset = self.ui_metric(18.0);
            let preview_width = (metrics.cell_width - content_inset).max(metrics.icon_size);
            let label_width = metrics.cell_width - content_inset;
            let label_height = visual_label_height(font_size);
            let name_editor: Element<'_, Message> = if let Some(dialog) = editing {
                wrapped_inline_rename_editor(
                    &dialog.editor,
                    label_width,
                    label_height,
                    font_size,
                    palette,
                    true,
                )
            } else {
                container(centered_highlighted_search_text(
                    self.pane(pane).search_text.as_str(),
                    &two_line_ellipsize_to_width(&display_name, label_width, font_size),
                    color,
                    font_size,
                ))
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
            .spacing(self.ui_metric(4.0))
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
                .padding(self.ui_vertical_padding(if metrics.tile { 6.0 } else { 8.0 }))
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
            .padding(self.ui_vertical_padding(if metrics.tile { 6.0 } else { 8.0 }))
            .on_press(Message::Noop)
            .style(move |_, status| file_item_button_style(palette, selected, status))
            .into()
        };

        self.entry_context_surface(pane, index, item)
    }
}
