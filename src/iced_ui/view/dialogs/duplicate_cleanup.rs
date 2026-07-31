use super::*;
use iced::widget::{column, row};

impl BExplorerIced {
    pub(in crate::iced_ui) fn duplicate_cleanup_window_view(
        &self,
        palette: Palette,
    ) -> Element<'_, Message> {
        let (window_bg, window_title_bg) = palette.native_utility_backgrounds();
        let Some(state) = self.duplicate_cleanup.as_ref() else {
            return container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(move |_| container::Style::default().background(window_bg))
                .into();
        };
        let font_size = self.font_size();
        let title_height = scaled_ui_metric(TRANSFER_WINDOW_TITLE_HEIGHT, font_size);
        let window_size = state.window_size;
        let window_radius = if state.window_maximized {
            1.0
        } else {
            WINDOW_RADIUS
        };
        let inner_height = (window_size.height - WINDOW_BORDER_WIDTH * 2.0).max(0.0);
        let body_height = (inner_height - title_height).max(0.0);
        let active = matches!(
            state.phase,
            DuplicateCleanupPhase::Counting | DuplicateCleanupPhase::Scanning
        );
        let fraction = if state.phase == DuplicateCleanupPhase::Complete {
            1.0
        } else if state.total == 0 {
            0.0
        } else {
            (state.scanned as f32 / state.total as f32).clamp(0.0, 1.0)
        };

        let title_drag_area = mouse_area(
            container(
                text(self.localized("Limpieza de archivos duplicados", "Duplicate file cleanup"))
                    .size(font_size)
                    .color(palette.text)
                    .align_x(Horizontal::Center)
                    .width(Length::Fill),
            )
            .height(title_height)
            .width(Length::Fill)
            .center_y(Length::Fill),
        )
        .on_press(Message::DuplicateWindowDrag);
        let close_message = (!state.deleting).then_some(Message::CloseDuplicateCleanup);
        let title_bar = container(
            row![
                title_drag_area,
                native_window_minimize_button(
                    Message::DuplicateWindowMinimize,
                    palette,
                    title_height,
                    font_size,
                ),
                native_window_maximize_button(
                    Message::DuplicateWindowMaximize,
                    state.window_maximized,
                    palette,
                    title_height,
                    font_size,
                ),
                native_window_close_button_maybe(close_message, palette, title_height, font_size,),
            ]
            .align_y(Alignment::Center),
        )
        .height(title_height)
        .width(Length::Fill)
        .style(move |_| {
            container::Style::default()
                .background(window_title_bg)
                .border(border::rounded(border::top((window_radius - 1.0).max(0.0))))
        });

        let heading = match state.phase {
            DuplicateCleanupPhase::Counting | DuplicateCleanupPhase::Scanning => self.localized(
                "Buscando archivos duplicados",
                "Searching for duplicate files",
            ),
            DuplicateCleanupPhase::Complete => {
                self.localized("Búsqueda completada", "Search complete")
            }
            DuplicateCleanupPhase::Cancelled => {
                self.localized("Búsqueda cancelada", "Search cancelled")
            }
            DuplicateCleanupPhase::Failed => self.localized(
                "No se pudo completar la búsqueda",
                "The search could not finish",
            ),
        };
        let detail = match state.phase {
            DuplicateCleanupPhase::Counting => {
                if self.is_spanish() {
                    format!("Contando archivos… {} encontrados", state.files_found)
                } else {
                    format!("Counting files… {} found", state.files_found)
                }
            }
            DuplicateCleanupPhase::Scanning => {
                let current = state
                    .current_path
                    .as_ref()
                    .map(|path| ellipsize_text(&path.display().to_string(), 90))
                    .unwrap_or_default();
                if self.is_spanish() {
                    format!(
                        "{} de {} analizados · {} candidatos{}",
                        state.scanned,
                        state.total,
                        state.entries.len(),
                        if current.is_empty() {
                            String::new()
                        } else {
                            format!(" · {current}")
                        }
                    )
                } else {
                    format!(
                        "{} of {} scanned · {} candidates{}",
                        state.scanned,
                        state.total,
                        state.entries.len(),
                        if current.is_empty() {
                            String::new()
                        } else {
                            format!(" · {current}")
                        }
                    )
                }
            }
            _ => {
                if self.is_spanish() {
                    format!(
                        "{} archivos analizados · {} candidatos · {} omitidos",
                        state.scanned,
                        state.entries.len(),
                        state.skipped
                    )
                } else {
                    format!(
                        "{} files scanned · {} candidates · {} skipped",
                        state.scanned,
                        state.entries.len(),
                        state.skipped
                    )
                }
            }
        };
        let progress: Element<'_, Message> = if state.phase == DuplicateCleanupPhase::Counting {
            indeterminate_progress_bar(
                self.transfer_progress_phase,
                palette,
                TRANSFER_PROGRESS_BAR_HEIGHT,
            )
        } else {
            transfer_progress_bar(fraction, palette, TRANSFER_PROGRESS_BAR_HEIGHT)
        };
        let scan_header = column![
            row![
                column![
                    text(heading).size(font_size + 1.0).color(palette.text),
                    text(ellipsize_text(&state.root.display().to_string(), 105))
                        .size(font_size - 1.0)
                        .color(palette.muted_text),
                ]
                .spacing(2)
                .width(Length::Fill),
                text(if active && state.total > 0 {
                    format!("{:.0}%", fraction * 100.0)
                } else {
                    String::new()
                })
                .size(font_size)
                .color(palette.muted_text),
            ]
            .align_y(Alignment::Center),
            progress,
            text(detail)
                .size(font_size - 1.0)
                .color(palette.muted_text)
                .wrapping(iced::widget::text::Wrapping::None),
        ]
        .spacing(7);

        let header_checkbox = iced::widget::checkbox(state.all_candidates_selected)
            .size(self.ui_metric(16.0))
            .on_toggle(Message::SelectAllDuplicateCandidates)
            .style(move |_, status| duplicate_checkbox_style(palette, status));
        let mut column_widths = std::array::from_fn::<_, 7, _>(|index| {
            state.column_widths[index].max(duplicate_table_column_min_width(
                index,
                font_size,
                self.is_spanish(),
            ))
        });
        let available_table_width = (window_size.width - self.ui_metric(30.0)).max(0.0);
        let current_table_width = column_widths.iter().sum::<f32>();
        column_widths[6] += (available_table_width - current_table_width).max(0.0);
        let [
            name_column_width,
            type_column_width,
            size_column_width,
            created_column_width,
            modified_column_width,
            match_column_width,
            location_column_width,
        ] = column_widths;
        let table_content_width = column_widths.iter().sum::<f32>();
        let name_header: Element<'_, Message> = row![
            header_checkbox,
            text(self.localized("Nombre", "Name"))
                .size(font_size)
                .color(palette.text)
                .width(Length::Fill)
                .align_x(Horizontal::Left)
                .wrapping(iced::widget::text::Wrapping::None),
        ]
        .spacing(self.ui_metric(8.0))
        .align_y(Alignment::Center)
        .into();
        let table_header = row![
            table_header_content_cell(name_header, name_column_width, 0, palette, font_size),
            table_header_cell(
                self.localized("Tipo", "Type"),
                type_column_width,
                1,
                palette,
                font_size
            ),
            table_header_cell(
                self.localized("Tamaño", "Size"),
                size_column_width,
                2,
                palette,
                font_size
            ),
            table_header_cell(
                self.localized("Fecha de creación", "Creation date"),
                created_column_width,
                3,
                palette,
                font_size,
            ),
            table_header_cell(
                self.localized("Fecha de modificación", "Modification date"),
                modified_column_width,
                4,
                palette,
                font_size,
            ),
            table_header_cell(
                self.localized("Coincidencia", "Match"),
                match_column_width,
                5,
                palette,
                font_size
            ),
            table_header_cell(
                self.localized("Ubicación", "Location"),
                location_column_width,
                6,
                palette,
                font_size
            ),
        ]
        .width(Length::Fixed(table_content_width))
        .height(self.ui_metric(DUPLICATE_TABLE_HEADER_HEIGHT))
        .align_y(Alignment::Center);

        let total_entries = state.entries.len();
        enum VisibleDuplicateItem {
            Group(usize),
            Entry(usize),
        }
        let row_height = self.ui_metric(DUPLICATE_TABLE_ROW_HEIGHT);
        let group_height = self.ui_metric(36.0);
        let group_count = state.extension_group_starts.len();
        let total_height = total_entries as f32 * row_height + group_count as f32 * group_height;
        let (window_start, window_end) = virtual_table_pixel_window(
            state.table_scroll_offset_y,
            state.table_viewport_height,
            state.table_scroll_velocity_y,
            row_height,
        );
        let mut visible_items = Vec::new();
        let mut first_visible_y = None;
        let mut last_visible_y = 0.0_f32;
        let mut low = 0_usize;
        let mut high = total_entries;
        while low < high {
            let middle = low + (high - low) / 2;
            let groups_through_entry = state
                .extension_group_starts
                .partition_point(|start| *start <= middle);
            let entry_bottom =
                (middle + 1) as f32 * row_height + groups_through_entry as f32 * group_height;
            if entry_bottom < window_start {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        for index in low..total_entries {
            let groups_before = state
                .extension_group_starts
                .partition_point(|start| *start < index);
            let is_group_start = state.extension_group_starts.get(groups_before) == Some(&index);
            let mut item_start = index as f32 * row_height + groups_before as f32 * group_height;
            if is_group_start {
                let header_end = item_start + group_height;
                if header_end >= window_start && item_start <= window_end {
                    first_visible_y.get_or_insert(item_start);
                    visible_items.push(VisibleDuplicateItem::Group(index));
                    last_visible_y = header_end;
                }
                item_start = header_end;
            }
            let item_end = item_start + row_height;
            if item_end >= window_start && item_start <= window_end {
                first_visible_y.get_or_insert(item_start);
                visible_items.push(VisibleDuplicateItem::Entry(index));
                last_visible_y = item_end;
            } else if item_start > window_end && first_visible_y.is_some() {
                break;
            }
        }
        let mut rows = column![].width(Length::Fill);
        if let Some(before) = first_visible_y.filter(|height| *height > 0.0) {
            rows = rows.push(
                Space::new()
                    .width(Length::Fixed(table_content_width))
                    .height(Length::Fixed(before)),
            );
        }
        for item in visible_items {
            let index = match item {
                VisibleDuplicateItem::Group(index) => {
                    let entry = &state.entries[index];
                    let count = state
                        .extension_counts
                        .get(&entry.extension)
                        .copied()
                        .unwrap_or_default();
                    let extension = if entry.extension.is_empty() {
                        self.localized("SIN EXTENSIÓN", "NO EXTENSION").to_owned()
                    } else {
                        format!(".{}", entry.extension.to_uppercase())
                    };
                    let group_label = if self.is_spanish() {
                        format!("{extension} · {count} archivo(s)")
                    } else {
                        format!("{extension} · {count} file(s)")
                    };
                    rows = rows.push(
                        container(
                            text(group_label)
                                .size(font_size)
                                .color(palette.text)
                                .wrapping(iced::widget::text::Wrapping::None),
                        )
                        .width(Length::Fixed(table_content_width))
                        .height(Length::Fixed(self.ui_metric(36.0)))
                        .padding([0.0, self.ui_metric(8.0)])
                        .align_y(Alignment::Center)
                        .style(move |_| {
                            container::Style::default()
                                .background(mix_color(palette.header_bg, palette.table_bg, 0.32))
                                .border(border::rounded(0).color(palette.strong_border).width(1))
                        }),
                    );
                    continue;
                }
                VisibleDuplicateItem::Entry(index) => index,
            };
            let entry = &state.entries[index];
            let path = entry.path.clone();
            let checked = state.selected.contains(&entry.path);
            let checkbox = iced::widget::checkbox(checked)
                .size(self.ui_metric(16.0))
                .on_toggle(move |value| Message::ToggleDuplicateSelection(path.clone(), value))
                .style(move |_, status| duplicate_checkbox_style(palette, status));
            let (kind, kind_color) = match entry.kind {
                crate::fs::duplicates::DuplicateKind::Original => {
                    (self.localized("Original", "Original"), palette.accent)
                }
                crate::fs::duplicates::DuplicateKind::Exact => (
                    self.localized("Coincidencia exacta", "Exact match"),
                    Color::from_rgb8(77, 170, 112),
                ),
                crate::fs::duplicates::DuplicateKind::Possible => (
                    self.localized("Posible coincidencia", "Possible match"),
                    Color::from_rgb8(218, 157, 67),
                ),
            };
            let location = entry
                .path
                .parent()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "—".into());
            let type_label = self.localized_entry_type_label(
                &crate::iced_ui::duplicate_cleanup::duplicate_file_entry(entry),
            );
            let row_content = row![
                container(
                    row![
                        checkbox,
                        text(&entry.name)
                            .size(font_size)
                            .color(palette.text)
                            .wrapping(iced::widget::text::Wrapping::None),
                    ]
                    .spacing(self.ui_metric(8.0))
                    .align_y(Alignment::Center),
                )
                .width(Length::Fixed(name_column_width))
                .padding([0.0, self.ui_metric(8.0)])
                .clip(true),
                table_value_cell(type_label, type_column_width, palette, font_size),
                table_value_cell(
                    format_size(Some(entry.size)),
                    size_column_width,
                    palette,
                    font_size,
                ),
                table_value_cell(
                    format_duplicate_time(entry.created, self.is_spanish()),
                    created_column_width,
                    palette,
                    font_size,
                ),
                table_value_cell(
                    format_duplicate_time(entry.modified, self.is_spanish()),
                    modified_column_width,
                    palette,
                    font_size,
                ),
                container(text(kind).size(font_size).color(kind_color))
                    .width(Length::Fixed(match_column_width))
                    .padding([0.0, self.ui_metric(8.0)])
                    .clip(true),
                table_value_cell(location, location_column_width, palette, font_size),
            ]
            .width(Length::Fixed(table_content_width))
            .height(self.ui_metric(DUPLICATE_TABLE_ROW_HEIGHT))
            .align_y(Alignment::Center);
            let highlighted = state.highlighted.as_ref() == Some(&entry.path);
            let selected_path = entry.path.clone();
            let context_path = entry.path.clone();
            rows = rows.push(
                mouse_area(
                    container(row_content)
                        .width(Length::Fixed(table_content_width))
                        .style(move |_| {
                            container::Style::default()
                                .background(if highlighted {
                                    mix_color(palette.table_bg, palette.accent, 0.2)
                                } else {
                                    palette.table_bg
                                })
                                .border(border::rounded(0).color(palette.border).width(1))
                        }),
                )
                .on_press(Message::DuplicateRowSelected(selected_path))
                .on_right_press(Message::OpenDuplicateRowContext(context_path))
                .interaction(mouse::Interaction::Pointer),
            );
        }
        if first_visible_y.is_none() && total_height > 0.0 {
            rows = rows.push(
                Space::new()
                    .width(Length::Fixed(table_content_width))
                    .height(Length::Fixed(total_height)),
            );
        } else if last_visible_y < total_height {
            rows = rows.push(
                Space::new()
                    .width(Length::Fixed(table_content_width))
                    .height(Length::Fixed(total_height - last_visible_y)),
            );
        }
        if state.entries.is_empty() {
            let empty = if active {
                self.localized(
                    "Los candidatos aparecerán aquí durante el análisis.",
                    "Candidates will appear here during the scan.",
                )
            } else {
                self.localized(
                    "No se encontraron archivos duplicados.",
                    "No duplicate files were found.",
                )
            };
            rows = rows.push(
                container(text(empty).size(font_size).color(palette.muted_text))
                    .height(Length::Fixed(self.ui_metric(90.0)))
                    .center(Length::Fill),
            );
        }
        let header = scrollable(
            container(table_header)
                .width(Length::Fixed(table_content_width))
                .style(move |_| {
                    container::Style::default()
                        .background(palette.header_bg)
                        .border(border::rounded(0).color(palette.strong_border).width(1))
                }),
        )
        .id(duplicate_table_header_scroll_id())
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::hidden(),
        ))
        .width(Length::Fill)
        .height(Length::Fixed(self.ui_metric(DUPLICATE_TABLE_HEADER_HEIGHT)));
        let table_rows = scrollable(rows.width(Length::Fixed(table_content_width)))
            .direction(scrollable::Direction::Both {
                vertical: explorer_scrollbar(1.0),
                horizontal: explorer_scrollbar(1.0),
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .on_scroll(|viewport| Message::DuplicateTableScrolled {
                offset_x: viewport.absolute_offset().x,
                offset_y: viewport.absolute_offset().y,
                viewport_height: viewport.bounds().height,
            })
            .style(move |theme, status| explorer_scrollable_style(palette, theme, status, 1.0));
        let table = container(column![header, table_rows].height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_| {
                container::Style::default()
                    .background(palette.table_bg)
                    .border(border::rounded(5).color(palette.border).width(1))
            });

        let action_row: Element<'_, Message> = if state.confirm_delete {
            row![
                text(self.localized(
                    "Los elementos seleccionados se moverán a la papelera.",
                    "Selected items will be moved to the Recycle Bin.",
                ))
                .size(font_size)
                .color(Color::from_rgb8(218, 157, 67)),
                Space::new().width(Length::Fill),
                dialog_action_button(
                    self.localized("Cancelar", "Cancel"),
                    Some(Message::CancelDuplicateDelete),
                    false,
                    palette,
                    font_size,
                ),
                dialog_action_button(
                    self.localized("Mover a la papelera", "Move to Recycle Bin"),
                    Some(Message::ConfirmDuplicateDelete),
                    true,
                    palette,
                    font_size,
                ),
            ]
            .spacing(self.ui_metric(8.0))
            .align_y(Alignment::Center)
            .into()
        } else {
            let selected_label = if self.is_spanish() {
                format!("{} seleccionados", state.selected.len())
            } else {
                format!("{} selected", state.selected.len())
            };
            let delete_message = (!active && !state.deleting && !state.selected.is_empty())
                .then_some(Message::RequestDuplicateDelete);
            let cancel_or_close = if active {
                (
                    self.localized("Cancelar búsqueda", "Cancel search"),
                    Some(Message::CancelDuplicateScan),
                )
            } else {
                (
                    self.localized("Cerrar", "Close"),
                    Some(Message::CloseDuplicateCleanup),
                )
            };
            row![
                text(selected_label)
                    .size(font_size)
                    .color(palette.muted_text),
                if let Some(error) = state.error.as_deref() {
                    text(ellipsize_text(error, 65))
                        .size(font_size - 1.0)
                        .color(Color::from_rgb8(210, 72, 72))
                } else {
                    text("")
                },
                Space::new().width(Length::Fill),
                dialog_action_button(
                    self.localized("Eliminar seleccionados", "Delete selected"),
                    delete_message,
                    true,
                    palette,
                    font_size,
                ),
                dialog_action_button(
                    cancel_or_close.0,
                    cancel_or_close.1,
                    false,
                    palette,
                    font_size,
                ),
            ]
            .spacing(self.ui_metric(8.0))
            .align_y(Alignment::Center)
            .into()
        };

        let body = container(
            column![scan_header, table, action_row]
                .spacing(self.ui_metric(10.0))
                .padding(self.ui_metric(14.0))
                .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fixed(body_height))
        .style(move |_| container::Style::default().background(window_bg));
        let inner_panel = container(column![title_bar, body].height(Length::Fixed(inner_height)))
            .width(Length::Fill)
            .height(Length::Fixed(inner_height))
            .clip(true)
            .style(move |_| {
                container::Style::default()
                    .background(window_bg)
                    .border(border::rounded(
                        (window_radius - WINDOW_BORDER_WIDTH).max(0.0),
                    ))
            });
        let panel: Element<'_, Message> = mouse_area(
            container(inner_panel)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(WINDOW_BORDER_WIDTH)
                .style(move |_| {
                    container::Style::default()
                        .background(Color::TRANSPARENT)
                        .border(
                            border::rounded(window_radius)
                                .color(window_border_color(palette))
                                .width(WINDOW_BORDER_WIDTH),
                        )
                }),
        )
        .on_move(Message::DuplicatePointerMoved)
        .on_release(Message::StopDuplicateColumnResize)
        .into();

        let mut layers = vec![panel];
        if state.context_path.is_some() {
            let menu_width = self.ui_metric(270.0);
            let menu_height = context_menu_row_height(font_size) + self.ui_metric(8.0);
            let x = state.context_position.x.clamp(
                self.ui_metric(8.0),
                window_size.width - menu_width - self.ui_metric(8.0),
            );
            let y = state.context_position.y.clamp(
                self.ui_metric(8.0),
                window_size.height - menu_height - self.ui_metric(8.0),
            );
            let menu_content = row![
                inline_icon(
                    "folder",
                    palette.muted_text,
                    scaled_ui_metric(18.0, font_size),
                ),
                text(self.localized("Abrir ubicación del archivo", "Open file location"))
                    .size(font_size)
                    .color(palette.text)
                    .wrapping(iced::widget::text::Wrapping::None)
                    .width(Length::Fill),
            ]
            .spacing(scaled_ui_metric(12.0, font_size))
            .align_y(Alignment::Center)
            .height(Length::Fill);
            let location_button = Button::new(
                container(menu_content)
                    .height(Length::Fill)
                    .center_y(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fixed(context_menu_row_height(font_size)))
            .padding([0.0, self.ui_metric(10.0)])
            .on_press(Message::OpenDuplicateFileLocation)
            .style(move |_, status| selected_button_style(palette, false, status));
            let context_menu = container(location_button)
                .width(Length::Fixed(menu_width))
                .padding(self.ui_metric(4.0))
                .style(move |_| {
                    container::Style::default()
                        .background(palette.menu_bg)
                        .border(border::rounded(7).color(palette.strong_border).width(1))
                        .shadow(iced::Shadow {
                            color: Color::from_rgba8(0, 0, 0, 0.28),
                            offset: Vector::new(0.0, 7.0),
                            blur_radius: 18.0,
                        })
                });
            let backdrop = mouse_area(
                container(Space::new())
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .on_press(Message::CloseDuplicateRowContext);
            let floating_menu: Element<'_, Message> = float(opaque(context_menu))
                .translate(move |_, _| Vector::new(x, y))
                .into();
            layers.push(backdrop.into());
            layers.push(floating_menu);
        }
        layers.push(duplicate_window_resize_handles(state.window_maximized));

        stack(layers)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

fn table_header_cell<'a>(
    label: &'a str,
    width: f32,
    column: usize,
    palette: Palette,
    font_size: f32,
) -> Element<'a, Message> {
    table_header_content_cell(
        text(label)
            .size(font_size)
            .color(palette.text)
            .width(Length::Fill)
            .align_x(Horizontal::Left)
            .wrapping(iced::widget::text::Wrapping::None)
            .into(),
        width,
        column,
        palette,
        font_size,
    )
}

fn table_header_content_cell<'a>(
    content: Element<'a, Message>,
    width: f32,
    column: usize,
    palette: Palette,
    font_size: f32,
) -> Element<'a, Message> {
    let handle_width = scaled_ui_metric(DUPLICATE_TABLE_COLUMN_HANDLE_WIDTH, font_size);
    let handle = mouse_area(
        container(
            container(Space::new())
                .width(1.0)
                .height(Length::Fill)
                .style(move |_| container::Style::default().background(palette.border)),
        )
        .width(handle_width)
        .height(Length::Fill)
        // Preserve the fixed handle width. `center_x(Length::Fill)` would
        // replace it with `Fill`, taking roughly half of the header cell and
        // clipping labels even when the column has ample visible space.
        .align_x(Horizontal::Center),
    )
    .on_press(Message::StartDuplicateColumnResize(column))
    .interaction(mouse::Interaction::ResizingColumn);

    container(
        row![
            container(content)
                .width(Length::Fill)
                .padding([0.0, scaled_ui_metric(8.0, font_size)])
                .clip(true),
            handle,
        ]
        .height(Length::Fill)
        .align_y(Alignment::Center),
    )
    .width(Length::Fixed(width))
    .clip(true)
    .into()
}

pub(super) fn table_value_cell(
    value: String,
    width: f32,
    palette: Palette,
    font_size: f32,
) -> Element<'static, Message> {
    container(
        text(value)
            .size(font_size)
            .color(palette.muted_text)
            .wrapping(iced::widget::text::Wrapping::None),
    )
    .width(Length::Fixed(width))
    .padding([0.0, scaled_ui_metric(8.0, font_size)])
    .clip(true)
    .into()
}

fn duplicate_window_resize_handles(maximized: bool) -> Element<'static, Message> {
    if maximized {
        return Space::new().width(Length::Fill).height(Length::Fill).into();
    }
    let edge = WINDOW_RESIZE_HANDLE_WIDTH;
    let corner = WINDOW_RESIZE_HANDLE_WIDTH * 1.8;
    column![
        row![
            duplicate_window_resize_handle(corner, edge, window::Direction::NorthWest),
            duplicate_window_resize_handle(Length::Fill, edge, window::Direction::North),
            duplicate_window_resize_handle(corner, edge, window::Direction::NorthEast),
        ]
        .height(edge),
        row![
            duplicate_window_resize_handle(edge, Length::Fill, window::Direction::West),
            Space::new().width(Length::Fill).height(Length::Fill),
            duplicate_window_resize_handle(edge, Length::Fill, window::Direction::East),
        ]
        .height(Length::Fill),
        row![
            duplicate_window_resize_handle(corner, edge, window::Direction::SouthWest),
            duplicate_window_resize_handle(Length::Fill, edge, window::Direction::South),
            duplicate_window_resize_handle(corner, edge, window::Direction::SouthEast),
        ]
        .height(edge),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn duplicate_window_resize_handle<W, H>(
    width: W,
    height: H,
    direction: window::Direction,
) -> Element<'static, Message>
where
    W: Into<Length>,
    H: Into<Length>,
{
    let interaction = match direction {
        window::Direction::East | window::Direction::West => {
            mouse::Interaction::ResizingHorizontally
        }
        window::Direction::North | window::Direction::South => {
            mouse::Interaction::ResizingVertically
        }
        window::Direction::NorthEast | window::Direction::SouthWest => {
            mouse::Interaction::ResizingDiagonallyUp
        }
        window::Direction::NorthWest | window::Direction::SouthEast => {
            mouse::Interaction::ResizingDiagonallyDown
        }
    };
    mouse_area(
        container(Space::new())
            .width(width)
            .height(height)
            .style(|_| container::Style::default().background(Color::TRANSPARENT)),
    )
    .on_press(Message::DuplicateWindowResize(direction))
    .interaction(interaction)
    .into()
}

pub(super) fn dialog_action_button<'a>(
    label: &'a str,
    message: Option<Message>,
    primary: bool,
    palette: Palette,
    font_size: f32,
) -> Element<'a, Message> {
    let content = container(text(label).size(font_size))
        .height(Length::Fill)
        .center_y(Length::Fill);
    let button = Button::new(content)
        .height(scaled_ui_metric(34.0, font_size))
        .padding([0.0, scaled_ui_metric(12.0, font_size)])
        .style(move |_, status| dialog_button_style(palette, primary, status));
    if let Some(message) = message {
        button.on_press(message).into()
    } else {
        button.into()
    }
}

pub(super) fn format_duplicate_time(time: Option<std::time::SystemTime>, spanish: bool) -> String {
    let Some(time) = time else {
        return "—".into();
    };
    let date: chrono::DateTime<chrono::Local> = time.into();
    if spanish {
        date.format("%d/%m/%Y %H:%M").to_string()
    } else {
        date.format("%Y-%m-%d %H:%M").to_string()
    }
}

fn duplicate_checkbox_style(
    palette: Palette,
    status: iced::widget::checkbox::Status,
) -> iced::widget::checkbox::Style {
    let (is_checked, hovered, disabled) = match status {
        iced::widget::checkbox::Status::Active { is_checked } => (is_checked, false, false),
        iced::widget::checkbox::Status::Hovered { is_checked } => (is_checked, true, false),
        iced::widget::checkbox::Status::Disabled { is_checked } => (is_checked, false, true),
    };
    let background: Background = if is_checked && !disabled {
        accent_gradient(palette).into()
    } else if is_checked {
        mix_color(palette.input_bg, palette.accent, 0.45).into()
    } else if hovered {
        mix_color(palette.input_bg, hover_tint(palette), 0.45).into()
    } else {
        palette.input_bg.into()
    };
    let border_color = if is_checked {
        if disabled {
            mix_color(palette.strong_border, palette.accent, 0.42)
        } else if hovered {
            mix_color(palette.accent, Color::WHITE, 0.16)
        } else {
            palette.accent
        }
    } else if hovered {
        mix_color(palette.strong_border, palette.accent, 0.45)
    } else {
        palette.strong_border
    };
    iced::widget::checkbox::Style {
        background,
        icon_color: if disabled {
            mix_color(palette.muted_text, palette.accent_text, 0.35)
        } else {
            palette.accent_text
        },
        border: border::rounded(3).color(border_color).width(1),
        text_color: Some(if disabled {
            palette.muted_text
        } else {
            palette.text
        }),
    }
}
