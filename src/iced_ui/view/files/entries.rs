use super::*;
use iced::widget::row;

impl BExplorerIced {
    pub(in crate::iced_ui) fn file_entry_icon(
        &self,
        entry: &FileEntry,
        palette: Palette,
        selected: bool,
        icon_size: f32,
        width: f32,
        height: f32,
    ) -> Element<'static, Message> {
        if let Some(handle) = self.entry_image_handle_for_size(entry, icon_size).cloned() {
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
            selected_item_text_color(palette, false)
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
        if let Some(handle) = self.entry_image_handle_for_size(entry, size).cloned() {
            let opacity = self.entry_presentation_opacity(entry, selected);
            return iced_image::Image::new(handle)
                .width(Length::Fixed(size))
                .height(Length::Fixed(size))
                .content_fit(ContentFit::Contain)
                .opacity(opacity)
                .into();
        }

        let color = if selected {
            selected_item_text_color(palette, false)
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
        self.entry_image_handle_for_variant(entry, IcedImageVariant::Standard)
    }

    fn entry_image_handle_for_size(
        &self,
        entry: &FileEntry,
        size: f32,
    ) -> Option<&iced_image::Handle> {
        let variant = if size <= 32.0 {
            IcedImageVariant::Small
        } else {
            IcedImageVariant::Standard
        };
        self.entry_image_handle_for_variant(entry, variant)
    }

    fn entry_image_handle_for_variant(
        &self,
        entry: &FileEntry,
        variant: IcedImageVariant,
    ) -> Option<&iced_image::Handle> {
        let thumbnail_cache = match variant {
            IcedImageVariant::Standard => &self.thumbnail_cache,
            IcedImageVariant::Small => &self.small_thumbnail_cache,
        };
        if thumbnail_data::is_thumbnail_candidate(entry)
            && let Some(IcedImageState::Ready(handle)) = thumbnail_cache.get(&entry.path)
        {
            return Some(handle);
        }

        let source_size = match variant {
            IcedImageVariant::Standard => thumbnail_data::NATIVE_ICON_SIZE,
            IcedImageVariant::Small => thumbnail_data::SMALL_ENTRY_IMAGE_SIZE,
        };
        let (cache_key, _, _) = native_icon_request_for_entry(entry, source_size)?;
        let native_cache = match variant {
            IcedImageVariant::Standard => &self.native_icon_cache,
            IcedImageVariant::Small => &self.small_native_icon_cache,
        };
        match native_cache.get(&cache_key) {
            Some(IcedImageState::Ready(handle)) => Some(handle),
            _ if variant == IcedImageVariant::Small => {
                self.entry_image_handle_for_variant(entry, IcedImageVariant::Standard)
            }
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
            translucent_color(
                selected_item_text_color(palette, false),
                presentation_opacity,
            )
        } else {
            translucent_color(palette.text, presentation_opacity)
        };
        let meta_color = if selected {
            translucent_color(
                selected_item_text_color(palette, true),
                presentation_opacity,
            )
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
                highlighted_search_text(self.pane(pane).search_text.as_str(), &name, text_color,)
                    .size(table_font_size)
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
            .style(move |_, status| file_item_button_style(palette, selected, status))
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
