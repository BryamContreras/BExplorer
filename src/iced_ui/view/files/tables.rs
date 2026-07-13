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
                TransferDisplayKind::Copy => "Copying",
                TransferDisplayKind::Move => "Moving",
                TransferDisplayKind::Trash => "Moving to recycle bin",
                TransferDisplayKind::PermanentDelete => "Deleting permanently",
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
                table_font_size
            ),
            table_header(
                self.localized("Tipo", "Type"),
                widths.type_label,
                header_config(TableColumn::Type, true),
                palette,
                table_font_size
            ),
            table_header(
                self.localized("Tamaño", "Size"),
                widths.size,
                header_config(TableColumn::Size, true),
                palette,
                table_font_size
            ),
            table_header(
                self.localized("Modificado", "Modified"),
                widths.modified,
                header_config(TableColumn::Modified, true),
                palette,
                table_font_size
            ),
            table_header(
                self.localized("Creado", "Created"),
                DETAIL_DATE_MIN_WIDTH,
                header_config(TableColumn::Created, false),
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

        let scroll_content: Element<'_, Message> = if self.current_modifiers.control() {
            mouse_area(rows)
                .on_scroll(move |delta| Message::PaneMouseWheel(pane, delta))
                .into()
        } else {
            rows.into()
        };
        let content: Element<'_, Message> = if self.current_modifiers.control() {
            container(scroll_content)
                .width(Length::Fill)
                .height(Length::Fill)
                .clip(true)
                .into()
        } else {
            scrollable(scroll_content)
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
                })
                .into()
        };
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
}
