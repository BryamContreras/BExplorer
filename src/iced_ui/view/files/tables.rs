use super::*;
use iced::widget::{column, row};

impl BExplorerIced {
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
                    DriveKind::System => "Unidad del sistema",
                    DriveKind::Local => "Disco local",
                    DriveKind::External => "Unidad externa",
                    DriveKind::Usb => "Unidad USB",
                    DriveKind::DiskImage => "Imagen de disco montada",
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
            EntryKind::SymlinkFolder => "Enlace simbólico a carpeta".into(),
            EntryKind::SymlinkFile => "Enlace simbólico a archivo".into(),
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
                let type_path = if explorer::is_virtual_path(&entry.path) {
                    Path::new(&entry.name)
                } else {
                    &entry.path
                };
                type_path
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
            // Windows can discover the same class of printer through several
            // providers, each with a different descriptive comment. Keep all
            // printer entries under one predictable group instead of making
            // that provider text part of the group name.
            if entry.drive_kind == Some(DriveKind::NetworkPrinter) {
                return self
                    .localized("Impresoras de red", "Network printers")
                    .into();
            }
            self.localized_entry_type_label(entry)
        } else {
            entry_group_label(entry, mode)
        }
    }

    pub(in crate::iced_ui) fn localized_tile_metadata_label(&self, entry: &FileEntry) -> String {
        let type_label = self.localized_entry_type_label(entry);
        if matches!(
            &entry.kind,
            EntryKind::File | EntryKind::SymlinkFile | EntryKind::Other
        ) && let Some(size) = entry.size
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
                TransferDisplayKind::Copy => "Copying",
                TransferDisplayKind::Move => "Moving",
                TransferDisplayKind::Trash => "Moving to recycle bin",
                TransferDisplayKind::PermanentDelete => "Deleting permanently",
                TransferDisplayKind::RestoreTrash => "Restoring from recycle bin",
                TransferDisplayKind::PurgeTrash => "Deleting from recycle bin",
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
            TransferState::Copying => match item.kind {
                TransferDisplayKind::Trash => "Moving items to recycle bin",
                TransferDisplayKind::PermanentDelete => "Deleting items",
                TransferDisplayKind::RestoreTrash => "Restoring items",
                TransferDisplayKind::PurgeTrash => "Deleting items",
                _ => "Copying files",
            },
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
        let trash_view = self.is_trash_pane(pane);
        let modified_label = if trash_view {
            self.localized("Ubicación original", "Original location")
        } else {
            self.localized("Modificado", "Modified")
        };
        let created_label = if trash_view {
            self.localized("Fecha de eliminación", "Date deleted")
        } else {
            self.localized("Creado", "Created")
        };
        let header_config = |column, resizable| TableHeaderConfig {
            pane,
            column,
            resizable,
            sort_active: sort_column == column,
            sort_ascending,
        };
        let header = row![
            table_header(
                self.localized("Nombre", "Name"),
                widths.name,
                header_config(TableColumn::Name, true),
                palette,
                table_font_size,
                self.detail_header_height(),
            ),
            table_header(
                self.localized("Tipo", "Type"),
                widths.type_label,
                header_config(TableColumn::Type, true),
                palette,
                table_font_size,
                self.detail_header_height(),
            ),
            table_header(
                self.localized("Tamaño", "Size"),
                widths.size,
                header_config(TableColumn::Size, true),
                palette,
                table_font_size,
                self.detail_header_height(),
            ),
            table_header(
                modified_label,
                widths.modified,
                header_config(TableColumn::Modified, true),
                palette,
                table_font_size,
                self.detail_header_height(),
            ),
            table_header(
                created_label,
                widths.created,
                header_config(TableColumn::Created, false),
                palette,
                table_font_size,
                self.detail_header_height(),
            ),
        ]
        .height(self.detail_header_height())
        .align_y(Alignment::Center)
        .width(Length::Fixed(table_width));

        let group_mode = self.effective_group_mode(pane);
        let group_starts = if group_mode == GroupMode::None {
            Vec::new()
        } else {
            self.filtered_entry_group_starts(pane)
        };
        let entries = self.filtered_entries_ref(pane);
        let total = entries.len();
        let mut rows = column![header].width(Length::Fixed(table_width));
        let state = self.pane(pane);
        let row_offset = (state.scroll_offset_y - self.detail_header_height()).max(0.0);
        let viewport_height = state.scroll_viewport_height;
        let velocity_y = state.scroll_velocity_y;
        if group_mode == GroupMode::None {
            let range = virtual_table_range(
                total,
                self.detail_row_height(),
                row_offset,
                viewport_height,
                velocity_y,
            );
            if range.before > 0.0 {
                rows = rows.push(
                    Space::new()
                        .width(Length::Fixed(table_width))
                        .height(Length::Fixed(range.before)),
                );
            }
            for &index in &entries[range.start..range.end] {
                if let Some(entry) = self.pane(pane).entries.get(index) {
                    rows = rows.push(self.file_row(pane, index, entry, palette, widths));
                }
            }
            if range.after > 0.0 {
                rows = rows.push(
                    Space::new()
                        .width(Length::Fixed(table_width))
                        .height(Length::Fixed(range.after)),
                );
            }
        } else {
            let row_height = self.detail_row_height();
            let group_height = self.detail_group_height();
            let total_height = total as f32 * row_height + group_starts.len() as f32 * group_height;
            let (window_start, window_end) =
                virtual_table_pixel_window(row_offset, viewport_height, velocity_y, row_height);
            let mut rendered_start = None;
            let mut rendered_end = 0.0_f32;
            let mut low = 0_usize;
            let mut high = total;
            while low < high {
                let middle = low + (high - low) / 2;
                let groups_through_entry = group_starts.partition_point(|start| *start <= middle);
                let entry_bottom =
                    (middle + 1) as f32 * row_height + groups_through_entry as f32 * group_height;
                if entry_bottom < window_start {
                    low = middle + 1;
                } else {
                    high = middle;
                }
            }
            for position in low..total {
                let index = entries[position];
                let Some(entry) = self.pane(pane).entries.get(index) else {
                    continue;
                };
                let groups_before = group_starts.partition_point(|start| *start < position);
                let is_group_start = group_starts.get(groups_before) == Some(&position);
                let mut item_start =
                    position as f32 * row_height + groups_before as f32 * group_height;
                if is_group_start {
                    let header_end = item_start + group_height;
                    if header_end >= window_start && item_start <= window_end {
                        if rendered_start.is_none() {
                            rendered_start = Some(item_start);
                            if item_start > 0.0 {
                                rows = rows.push(
                                    Space::new()
                                        .width(Length::Fixed(table_width))
                                        .height(Length::Fixed(item_start)),
                                );
                            }
                        }
                        rows = rows.push(file_group_header(
                            self.localized_entry_group_label(entry, group_mode),
                            palette,
                            self.font_size(),
                            group_height,
                        ));
                        rendered_end = header_end;
                    }
                    item_start = header_end;
                }
                let item_end = item_start + row_height;
                if item_end >= window_start && item_start <= window_end {
                    if rendered_start.is_none() {
                        rendered_start = Some(item_start);
                        if item_start > 0.0 {
                            rows = rows.push(
                                Space::new()
                                    .width(Length::Fixed(table_width))
                                    .height(Length::Fixed(item_start)),
                            );
                        }
                    }
                    rows = rows.push(self.file_row(pane, index, entry, palette, widths));
                    rendered_end = item_end;
                } else if item_start > window_end && rendered_start.is_some() {
                    break;
                }
            }
            if rendered_start.is_none() && total_height > 0.0 {
                rows = rows.push(
                    Space::new()
                        .width(Length::Fixed(table_width))
                        .height(Length::Fixed(total_height)),
                );
            } else if rendered_end < total_height {
                rows = rows.push(
                    Space::new()
                        .width(Length::Fixed(table_width))
                        .height(Length::Fixed(total_height - rendered_end)),
                );
            }
        }

        // The Scrollable must remain in the widget tree while Ctrl is held so
        // its offset survives modifier-only updates. The overlay intercepts
        // Ctrl+wheel without changing the tree that owns the scroll state.
        let pane_state = self.pane(pane);
        let scrollbar_reveal_progress = pane_state.scrollbar_reveal_progress;
        let vertical_scrollbar_expansion = f32::from(pane_state.scrollbar_vertical_hovered);
        let horizontal_scrollbar_expansion = f32::from(pane_state.scrollbar_horizontal_hovered);
        let scroller: Element<'_, Message> = scrollable(rows)
            .id(pane_scroll_id(pane))
            .direction(scrollable::Direction::Both {
                vertical: explorer_scrollbar(vertical_scrollbar_expansion),
                horizontal: explorer_scrollbar(horizontal_scrollbar_expansion),
            })
            .width(Length::Fill)
            .height(Length::Fill)
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

    pub(in crate::iced_ui) fn scrollbar_hover_layer<'a>(
        &self,
        pane: PaneId,
        base: Element<'a, Message>,
    ) -> Element<'a, Message> {
        scrollbar_proximity_layer(
            base,
            Some((
                Message::ScrollbarHover(pane, ScrollbarAxis::Vertical, true),
                Message::ScrollbarHover(pane, ScrollbarAxis::Vertical, false),
            )),
            Some((
                Message::ScrollbarHover(pane, ScrollbarAxis::Horizontal, true),
                Message::ScrollbarHover(pane, ScrollbarAxis::Horizontal, false),
            )),
        )
    }
}
