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
        if let Some((handle, has_thumbnail)) = self
            .entry_image_source_for_size(entry, icon_size)
            .map(|(handle, has_thumbnail)| (handle.clone(), has_thumbnail))
        {
            let opacity = self.entry_presentation_opacity(entry, selected);
            return entry_image_visual(entry, handle, has_thumbnail, opacity, width, height);
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
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into()
    }

    pub(in crate::iced_ui) fn detail_file_entry_icon(
        &self,
        entry: &FileEntry,
        palette: Palette,
        selected: bool,
        size: f32,
    ) -> Element<'static, Message> {
        let layout_size = rendered_inline_icon_size(size);
        if let Some((handle, has_thumbnail)) = self
            .entry_image_source_for_size(entry, size)
            .map(|(handle, has_thumbnail)| (handle.clone(), has_thumbnail))
        {
            let opacity = self.entry_presentation_opacity(entry, selected);
            return entry_image_visual(
                entry,
                handle,
                has_thumbnail,
                opacity,
                layout_size,
                layout_size,
            );
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
            size,
        ))
        .width(layout_size)
        .height(layout_size)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into()
    }

    pub(in crate::iced_ui) fn entry_image_handle(
        &self,
        entry: &FileEntry,
    ) -> Option<&iced_image::Handle> {
        self.entry_image_source_for_variant(entry, IcedImageVariant::Standard)
            .map(|(handle, _)| handle)
    }

    fn entry_image_source_for_size(
        &self,
        entry: &FileEntry,
        size: f32,
    ) -> Option<(&iced_image::Handle, bool)> {
        let variant = if size <= 32.0 {
            IcedImageVariant::Small
        } else {
            IcedImageVariant::Standard
        };
        self.entry_image_source_for_variant(entry, variant)
    }

    fn entry_image_source_for_variant(
        &self,
        entry: &FileEntry,
        variant: IcedImageVariant,
    ) -> Option<(&iced_image::Handle, bool)> {
        let thumbnail_cache = match variant {
            IcedImageVariant::Standard => &self.thumbnail_cache,
            IcedImageVariant::Small => &self.small_thumbnail_cache,
        };
        if thumbnail_data::is_thumbnail_candidate(entry)
            && let Some(IcedImageState::Ready(handle)) = thumbnail_cache.get(&entry.path)
        {
            return Some((handle, true));
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
            Some(IcedImageState::Ready(handle)) => Some((handle, false)),
            _ if variant == IcedImageVariant::Small => {
                self.entry_image_source_for_variant(entry, IcedImageVariant::Standard)
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
        let density = self.ui_density();
        metrics.cell_width = visual_cell_width_for_font(mode, self.font_size());
        metrics.cell_height += density;
        metrics.icon_size += density;
        metrics.preview_height += density;
        if metrics.spacing > 0.0 {
            metrics.spacing += density;
        }
        if metrics.grid_padding > 0.0 {
            metrics.grid_padding += density;
        }
        if mode == ViewMode::Tiles && self.is_this_pc_root(pane) {
            // Drive tiles include a capacity bar, so retain a uniform taller row.
            metrics.cell_height = self.ui_metric(90.0);
        }
        let surface_width = self.file_surface_width(pane);
        let usable_width = (surface_width - metrics.grid_padding * 2.0).max(1.0);

        if mode == ViewMode::Tiles {
            let font_size = (self.font_size() - 0.4).max(11.0);
            let text_height =
                visual_label_height(font_size) + 3.0 + ui_text_line_height(font_size - 0.5);
            metrics.cell_height = metrics
                .cell_height
                .max(metrics.icon_size.max(text_height) + self.ui_metric(12.0));
            let columns = ((usable_width + metrics.spacing)
                / (metrics.cell_width + metrics.spacing))
                .floor()
                .max(1.0) as usize;
            if columns == 1 {
                metrics.cell_width = metrics.cell_width.min(usable_width);
            }
            return VisualLayout { metrics, columns };
        }

        let min_width = visual_min_cell_width_for_font(mode, self.font_size()).min(usable_width);
        let columns = ((usable_width + metrics.spacing) / (min_width + metrics.spacing))
            .floor()
            .max(1.0) as usize;
        let cell_width =
            (usable_width - metrics.spacing * columns.saturating_sub(1) as f32) / columns as f32;
        metrics.cell_width = cell_width.max(min_width);

        if !metrics.tile {
            let font_size = (self.font_size() - 0.4).max(11.0);
            let label_height = visual_label_height(font_size);
            let target_preview_height =
                match mode {
                    ViewMode::LargeIcons => (metrics.cell_width * 0.56)
                        .clamp(self.ui_metric(112.0), self.ui_metric(162.0)),
                    ViewMode::ExtraLargeIcons => (metrics.cell_width * 0.58)
                        .clamp(self.ui_metric(184.0), self.ui_metric(252.0)),
                    ViewMode::MediumIcons => (metrics.cell_width * 0.48)
                        .clamp(self.ui_metric(70.0), self.ui_metric(92.0)),
                    ViewMode::SmallIcons | ViewMode::List => metrics.preview_height,
                    ViewMode::Details | ViewMode::Tiles => metrics.preview_height,
                };
            metrics.preview_height = target_preview_height;
            metrics.cell_height = (metrics.preview_height + label_height + self.ui_metric(24.0))
                .max(metrics.cell_height);
        }

        VisualLayout { metrics, columns }
    }

    pub(in crate::iced_ui) fn detail_column_widths(
        &self,
        pane: PaneId,
        font_size: f32,
    ) -> DetailColumnWidths {
        let detail_icon_size = self.ui_metric(DETAIL_ICON_SIZE);
        let detail_icon_layout_size = rendered_inline_icon_size(detail_icon_size);
        let cell_padding = self.ui_metric(8.0);
        let cell_insets = cell_padding * 2.0;
        let name_min = self.ui_metric(DETAIL_NAME_MIN_WIDTH);
        let name_max = self.ui_metric(DETAIL_NAME_MAX_WIDTH);
        let type_min = self.ui_metric(DETAIL_TYPE_MIN_WIDTH);
        let type_max = self.ui_metric(DETAIL_TYPE_MAX_WIDTH);
        let size_min = self.ui_metric(DETAIL_SIZE_MIN_WIDTH);
        let size_max = self.ui_metric(DETAIL_SIZE_MAX_WIDTH);
        let date_min = self.ui_metric(DETAIL_DATE_MIN_WIDTH);
        let date_max = self.ui_metric(DETAIL_DATE_MAX_WIDTH);
        let mut name_chars = "Nombre".chars().count();
        let mut type_chars = "Tipo".chars().count();
        let mut size_chars = "Tamano".chars().count();
        let (modified_header, created_header) = if self.is_trash_pane(pane) {
            (
                self.localized("Ubicación original", "Original location"),
                self.localized("Fecha de eliminación", "Date deleted"),
            )
        } else {
            (
                self.localized("Modificado", "Modified"),
                self.localized("Creado", "Created"),
            )
        };
        let mut modified_chars = modified_header.chars().count();
        let mut created_chars = created_header.chars().count();

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
            created_chars =
                created_chars.max(entry.created.as_deref().unwrap_or_default().chars().count());
        }

        let auto = DetailColumnWidths {
            name: estimated_column_width(
                name_chars,
                font_size,
                detail_icon_layout_size + self.ui_metric(6.0) + cell_insets,
                name_min,
                name_max,
            ),
            type_label: estimated_column_width(
                type_chars,
                font_size,
                cell_insets,
                type_min,
                type_max,
            ),
            size: estimated_column_width(size_chars, font_size, cell_insets, size_min, size_max),
            modified: estimated_column_width(
                modified_chars,
                font_size,
                cell_insets,
                date_min,
                date_max,
            ),
            created: estimated_column_width(
                created_chars,
                font_size,
                cell_insets,
                date_min,
                date_max,
            ),
        };
        let overrides = self.pane(pane).column_widths;
        DetailColumnWidths {
            name: overrides
                .get(TableColumn::Name)
                .unwrap_or(auto.name)
                .clamp(name_min, name_max),
            type_label: overrides
                .get(TableColumn::Type)
                .unwrap_or(auto.type_label)
                .clamp(type_min, type_max),
            size: overrides
                .get(TableColumn::Size)
                .unwrap_or(auto.size)
                .clamp(size_min, size_max),
            modified: overrides
                .get(TableColumn::Modified)
                .unwrap_or(auto.modified)
                .clamp(date_min, date_max),
            created: auto.created,
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
        let detail_icon_size = self.ui_metric(DETAIL_ICON_SIZE);
        let detail_icon_layout_size = rendered_inline_icon_size(detail_icon_size);
        let name_spacing = self.ui_metric(6.0);
        let cell_padding = self.ui_metric(8.0);
        let name_text_width =
            (widths.name - cell_padding * 2.0 - detail_icon_layout_size - name_spacing).max(1.0);
        let name_content: Element<'_, Message> = if let Some(dialog) = editing {
            row![
                self.detail_file_entry_icon(entry, palette, selected, detail_icon_size),
                inline_rename_editor(&dialog.editor, name_text_width, table_font_size, palette,),
            ]
            .spacing(name_spacing)
            .align_y(Alignment::Center)
            .width(Length::Fill)
            .into()
        } else {
            let name = ellipsize_to_width(
                &self.entry_display_name(entry),
                name_text_width,
                table_font_size,
            );
            row![
                self.detail_file_entry_icon(entry, palette, selected, detail_icon_size),
                highlighted_search_text(self.pane(pane).search_text.as_str(), &name, text_color,)
                    .size(table_font_size)
                    .width(Length::Fill)
                    .wrapping(iced::widget::text::Wrapping::None),
            ]
            .spacing(name_spacing)
            .align_y(Alignment::Center)
            .width(Length::Fill)
            .into()
        };
        let name_cell = container(name_content)
            .width(Length::Fixed(widths.name))
            .height(Length::Fill)
            .padding([0.0, cell_padding])
            .center_y(Length::Fill)
            .clip(true);
        let type_content_width = (widths.type_label - cell_padding * 2.0).max(1.0);
        let size_content_width = (widths.size - cell_padding * 2.0).max(1.0);
        let modified_content_width = (widths.modified - cell_padding * 2.0).max(1.0);
        let created_content_width = (widths.created - cell_padding * 2.0).max(1.0);
        let type_label = ellipsize_to_width(
            &self.localized_entry_type_label(entry),
            type_content_width,
            table_font_size,
        );
        let modified = entry.modified.clone().unwrap_or_default();
        let created = entry.created.clone().unwrap_or_default();
        let row = row![
            name_cell,
            container(
                text(type_label)
                    .size(table_font_size)
                    .color(meta_color)
                    .wrapping(iced::widget::text::Wrapping::None)
                    .width(Length::Fill),
            )
            .width(Length::Fixed(widths.type_label))
            .height(Length::Fill)
            .padding([0.0, cell_padding])
            .center_y(Length::Fill)
            .clip(true),
            container(
                text(ellipsize_to_width(
                    &format_size(entry.size),
                    size_content_width,
                    table_font_size,
                ))
                .size(table_font_size)
                .color(meta_color)
                .wrapping(iced::widget::text::Wrapping::None)
                .width(Length::Fill),
            )
            .width(Length::Fixed(widths.size))
            .height(Length::Fill)
            .padding([0.0, cell_padding])
            .center_y(Length::Fill)
            .clip(true),
            container(
                text(ellipsize_to_width(
                    &modified,
                    modified_content_width,
                    table_font_size
                ))
                .size(table_font_size)
                .color(meta_color)
                .wrapping(iced::widget::text::Wrapping::None)
                .width(Length::Fill),
            )
            .width(Length::Fixed(widths.modified))
            .height(Length::Fill)
            .padding([0.0, cell_padding])
            .center_y(Length::Fill)
            .clip(true),
            container(
                text(ellipsize_to_width(
                    &created,
                    created_content_width,
                    table_font_size,
                ))
                .size(table_font_size)
                .color(meta_color)
                .wrapping(iced::widget::text::Wrapping::None)
                .width(Length::Fill),
            )
            .width(Length::Fixed(widths.created))
            .height(Length::Fill)
            .padding([0.0, cell_padding])
            .center_y(Length::Fill)
            .clip(true),
        ]
        .height(self.detail_row_height())
        .padding([3.0, 0.0])
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

fn entry_image_visual(
    entry: &FileEntry,
    handle: iced_image::Handle,
    has_thumbnail: bool,
    opacity: f32,
    width: f32,
    height: f32,
) -> Element<'static, Message> {
    let display_size = contained_image_size(&handle, width, height);
    let image: Element<'static, Message> = iced_image::Image::new(handle)
        .width(Length::Fill)
        .height(Length::Fill)
        .content_fit(ContentFit::Contain)
        .opacity(opacity)
        .into();
    let visual = if shows_video_thumbnail_decorations(entry.category, has_thumbnail) {
        let glyph_size = video_play_icon_size(display_size.width.min(display_size.height));
        let outline_size = glyph_size + (glyph_size * 0.1).clamp(2.0, 4.0);
        let mut layers = vec![image];
        if shows_video_filmstrip(display_size.width, display_size.height) {
            layers.push(video_filmstrip_overlay(opacity));
        }
        let outline: Element<'static, Message> = container(inline_icon(
            "play",
            Color::from_rgba8(0, 0, 0, 0.82 * opacity),
            outline_size,
        ))
        .width(Length::Fill)
        .height(Length::Fill)
        .center(Length::Fill)
        .into();
        let play: Element<'static, Message> = container(inline_icon(
            "play",
            translucent_color(Color::WHITE, opacity),
            glyph_size,
        ))
        .width(Length::Fill)
        .height(Length::Fill)
        .center(Length::Fill)
        .into();
        layers.extend([outline, play]);
        stack(layers)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    } else {
        image
    };

    let thumbnail = container(visual)
        .width(Length::Fixed(display_size.width))
        .height(Length::Fixed(display_size.height));
    container(thumbnail)
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into()
}

fn contained_image_size(handle: &iced_image::Handle, width: f32, height: f32) -> Size {
    let bounds = Size::new(width.max(1.0), height.max(1.0));
    let iced_image::Handle::Rgba {
        width: source_width,
        height: source_height,
        ..
    } = handle
    else {
        return bounds;
    };
    if *source_width == 0 || *source_height == 0 {
        return bounds;
    }
    ContentFit::Contain.fit(
        Size::new(*source_width as f32, *source_height as f32),
        bounds,
    )
}

fn shows_video_thumbnail_decorations(category: FileCategory, has_thumbnail: bool) -> bool {
    category == FileCategory::Video && has_thumbnail
}

fn video_play_icon_size(available: f32) -> f32 {
    (available * 0.38).clamp(17.0, 52.0)
}

fn shows_video_filmstrip(width: f32, height: f32) -> bool {
    width >= 80.0 && height >= 52.0
}

fn video_filmstrip_overlay(opacity: f32) -> Element<'static, Message> {
    let layer = |data: &'static [u8], color| -> Element<'static, Message> {
        svg::Svg::new(svg::Handle::from_memory(data))
            .width(Length::Fill)
            .height(Length::Fill)
            .content_fit(ContentFit::Fill)
            .style(move |_, _| svg::Style { color: Some(color) })
            .into()
    };
    let rails = layer(VIDEO_FILM_RAILS, Color::from_rgba8(0, 0, 0, 0.78 * opacity));
    let perforations = layer(
        VIDEO_FILM_PERFORATIONS,
        Color::from_rgba8(245, 247, 248, 0.72 * opacity),
    );
    stack(vec![rails, perforations])
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

const VIDEO_FILM_RAILS: &[u8] = br##"<svg viewBox="0 0 100 56" preserveAspectRatio="none" xmlns="http://www.w3.org/2000/svg"><path d="M0 0h8v56H0zM92 0h8v56h-8z" fill="#000"/></svg>"##;
const VIDEO_FILM_PERFORATIONS: &[u8] = br##"<svg viewBox="0 0 100 56" preserveAspectRatio="none" xmlns="http://www.w3.org/2000/svg"><g fill="#000"><rect x="2" y="2" width="4" height="3.5" rx=".65"/><rect x="94" y="2" width="4" height="3.5" rx=".65"/><rect x="2" y="9" width="4" height="3.5" rx=".65"/><rect x="94" y="9" width="4" height="3.5" rx=".65"/><rect x="2" y="16" width="4" height="3.5" rx=".65"/><rect x="94" y="16" width="4" height="3.5" rx=".65"/><rect x="2" y="23" width="4" height="3.5" rx=".65"/><rect x="94" y="23" width="4" height="3.5" rx=".65"/><rect x="2" y="30" width="4" height="3.5" rx=".65"/><rect x="94" y="30" width="4" height="3.5" rx=".65"/><rect x="2" y="37" width="4" height="3.5" rx=".65"/><rect x="94" y="37" width="4" height="3.5" rx=".65"/><rect x="2" y="44" width="4" height="3.5" rx=".65"/><rect x="94" y="44" width="4" height="3.5" rx=".65"/><rect x="2" y="51" width="4" height="3" rx=".65"/><rect x="94" y="51" width="4" height="3" rx=".65"/></g></svg>"##;

#[cfg(test)]
mod video_thumbnail_tests {
    use super::*;

    #[test]
    fn video_decorations_require_a_real_video_thumbnail() {
        assert!(shows_video_thumbnail_decorations(FileCategory::Video, true));
        assert!(!shows_video_thumbnail_decorations(
            FileCategory::Video,
            false
        ));
        assert!(!shows_video_thumbnail_decorations(
            FileCategory::Image,
            true
        ));
    }

    #[test]
    fn video_play_icon_stays_legible_across_view_sizes() {
        assert_eq!(video_play_icon_size(20.0), 17.0);
        assert!((video_play_icon_size(100.0) - 38.0).abs() < 0.001);
        assert_eq!(video_play_icon_size(240.0), 52.0);
    }

    #[test]
    fn filmstrip_is_reserved_for_previews_with_enough_room() {
        assert!(shows_video_filmstrip(194.0, 112.0));
        assert!(shows_video_filmstrip(152.0, 70.0));
        assert!(!shows_video_filmstrip(122.0, 38.0));
        assert!(!shows_video_filmstrip(18.0, 18.0));
    }

    #[test]
    fn filmstrip_can_follow_the_contained_video_frame() {
        let handle = iced_image::Handle::from_rgba(16, 9, vec![0; 16 * 9 * 4]);
        let size = contained_image_size(&handle, 212.0, 112.0);
        assert!((size.width - 199.111_11).abs() < 0.001);
        assert_eq!(size.height, 112.0);
    }
}
