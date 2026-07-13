use super::*;
use iced::widget::{column, row};

impl BExplorerIced {
    pub(in crate::iced_ui) fn archive_dialog_modal(
        &self,
        palette: Palette,
    ) -> Element<'_, Message> {
        let Some(dialog) = &self.archive_dialog else {
            return Space::new().into();
        };
        let font_size = self.font_size();
        let extension = dialog.format.extension();
        let password_mismatch = dialog.use_password
            && (!dialog.password.is_empty() || !dialog.password_confirmation.is_empty())
            && dialog.password != dialog.password_confirmation;

        let format_choice = |label: &'static str, format: ArchiveFormat| {
            Button::new(
                text(label)
                    .size(font_size)
                    .color(if dialog.format == format {
                        palette.accent_text
                    } else {
                        palette.text
                    }),
            )
            .width(Length::Fixed(88.0))
            .padding([7, 12])
            .on_press(Message::SetArchiveFormat(format))
            .style(move |_, status| dialog_button_style(palette, dialog.format == format, status))
        };
        let method_choice = |label: &'static str, method: ArchiveCompressionMethod| {
            Button::new(
                text(label)
                    .size(font_size)
                    .color(if dialog.method == method {
                        palette.accent_text
                    } else {
                        palette.text
                    }),
            )
            .width(Length::Fill)
            .padding([7, 10])
            .on_press(Message::SetArchiveCompressionMethod(method))
            .style(move |_, status| dialog_button_style(palette, dialog.method == method, status))
        };

        let password_visibility_button = |shown: bool, message_down, message_up| {
            mouse_area(
                container(inline_icon(
                    if shown { "eye-off" } else { "eye" },
                    palette.muted_text,
                    17.0,
                ))
                .width(34)
                .height(30)
                .center_x(34)
                .center_y(30),
            )
            .on_press(message_down)
            .on_release(message_up)
            .interaction(mouse::Interaction::Pointer)
        };

        let password_fields: Element<'_, Message> = if dialog.use_password {
            let password_input = row![
                text_input(self.localized("Contraseña", "Password"), &dialog.password)
                    .secure(!dialog.show_password)
                    .on_input(Message::ArchivePasswordChanged)
                    .size(font_size)
                    .padding([6, 8])
                    .width(Length::Fill),
                password_visibility_button(
                    dialog.show_password,
                    Message::ShowArchivePassword(true),
                    Message::ShowArchivePassword(false),
                ),
            ]
            .spacing(4)
            .align_y(Alignment::Center);
            let confirmation_input = row![
                text_input(
                    self.localized("Repetir contraseña", "Repeat password"),
                    &dialog.password_confirmation,
                )
                .secure(!dialog.show_password_confirmation)
                .on_input(Message::ArchivePasswordConfirmationChanged)
                .size(font_size)
                .padding([6, 8])
                .width(Length::Fill),
                password_visibility_button(
                    dialog.show_password_confirmation,
                    Message::ShowArchivePasswordConfirmation(true),
                    Message::ShowArchivePasswordConfirmation(false),
                ),
            ]
            .spacing(4)
            .align_y(Alignment::Center);
            let mut fields = column![password_input, confirmation_input].spacing(7);
            if password_mismatch {
                fields = fields.push(
                    text(self.localized("Las contraseñas no coinciden", "Passwords do not match"))
                        .size(font_size - 1.0)
                        .color(Color::from_rgb8(227, 107, 114)),
                );
            }
            fields.into()
        } else {
            Space::new().into()
        };

        let password_enabled = dialog.use_password;
        let password_toggle = Button::new(
            row![
                container(
                    text(if password_enabled { "✓" } else { "" })
                        .size(font_size)
                        .color(palette.accent_text)
                )
                .width(18)
                .height(18)
                .center(18)
                .style(move |_| {
                    let style = container::Style::default()
                        .border(border::rounded(3).color(palette.strong_border).width(1));
                    if password_enabled {
                        style.background(accent_gradient(palette))
                    } else {
                        style
                    }
                }),
                text(self.localized("Agregar contraseña", "Add password"))
                    .size(font_size)
                    .color(palette.text),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding([4, 0])
        .on_press(Message::ToggleArchivePassword)
        .style(move |_, status| dialog_button_style(palette, false, status));

        let panel = column![
            row![
                text(self.localized("Comprimir", "Compress"))
                    .size(font_size + 2.0)
                    .color(palette.text)
                    .width(Length::Fill),
                icon_button("x", Message::CancelArchiveDialog, palette, false),
            ]
            .align_y(Alignment::Center),
            text(if self.is_spanish() {
                format!("{} elemento(s) seleccionado(s)", dialog.sources.len())
            } else {
                format!("{} item(s) selected", dialog.sources.len())
            })
            .size(font_size - 1.0)
            .color(palette.muted_text),
            column![
                text(self.localized("Nombre", "Name"))
                    .size(font_size - 1.0)
                    .color(palette.muted_text),
                row![
                    text_input(
                        self.localized("Nombre del archivo", "Archive name"),
                        &dialog.name
                    )
                    .on_input(Message::ArchiveNameChanged)
                    .on_submit(Message::ConfirmArchiveDialog)
                    .size(font_size)
                    .padding([6, 8])
                    .width(Length::Fill),
                    text(format!(".{extension}"))
                        .size(font_size)
                        .color(palette.muted_text),
                ]
                .spacing(7)
                .align_y(Alignment::Center),
            ]
            .spacing(5),
            column![
                text(self.localized("Formato", "Format"))
                    .size(font_size - 1.0)
                    .color(palette.muted_text),
                row![
                    format_choice("ZIP", ArchiveFormat::Zip),
                    format_choice("7z", ArchiveFormat::SevenZip),
                ]
                .spacing(7),
            ]
            .spacing(5),
            column![
                text(self.localized("Nivel de compresión", "Compression level"))
                    .size(font_size - 1.0)
                    .color(palette.muted_text),
                row![
                    method_choice(
                        self.localized("Rápida", "Fast"),
                        ArchiveCompressionMethod::Fast
                    ),
                    method_choice(
                        self.localized("Normal", "Normal"),
                        ArchiveCompressionMethod::Normal
                    ),
                    method_choice(
                        self.localized("Alta", "High"),
                        ArchiveCompressionMethod::Maximum
                    ),
                ]
                .spacing(7),
            ]
            .spacing(5),
            column![password_toggle, password_fields].spacing(7),
            row![
                Space::new().width(Length::Fill),
                Button::new(
                    text(self.localized("Cancelar", "Cancel"))
                        .size(font_size)
                        .color(palette.text)
                )
                .padding([7, 14])
                .on_press(Message::CancelArchiveDialog)
                .style(move |_, status| dialog_button_style(palette, false, status)),
                Button::new(
                    text(self.localized("Comprimir", "Compress"))
                        .size(font_size)
                        .color(palette.accent_text)
                )
                .padding([7, 14])
                .on_press(Message::ConfirmArchiveDialog)
                .style(move |_, status| dialog_button_style(palette, true, status)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ]
        .spacing(14)
        .padding(18);

        let surface = container(panel).width(470).style(move |_| {
            container::Style::default()
                .background(palette.menu_bg)
                .border(border::rounded(8).color(palette.strong_border).width(1))
                .shadow(iced::Shadow {
                    color: Color::from_rgba8(0, 0, 0, 0.22),
                    offset: iced::Vector::new(0.0, 10.0),
                    blur_radius: 24.0,
                })
        });
        let surface =
            self.frosted_popup_surface(self.popup_backdrop.as_ref(), surface.into(), 470.0, 382.0);
        container(surface)
            .width(Length::Fill)
            .height(Length::Fill)
            .center(Length::Fill)
            .style(move |_| {
                container::Style::default().background(Color::from_rgba8(
                    0,
                    0,
                    0,
                    0.24 * palette.text.a,
                ))
            })
            .into()
    }

    pub(in crate::iced_ui) fn archive_item_card(
        &self,
        item: ArchiveDisplayState,
        palette: Palette,
    ) -> Element<'_, Message> {
        let progress = if item.progress.total == 0 {
            0.0
        } else {
            (item.progress.completed as f32 / item.progress.total as f32).clamp(0.0, 1.0)
        };
        let title = match (item.state, self.is_spanish()) {
            (ArchiveState::Running, true) => "Comprimiendo",
            (ArchiveState::Finished, true) => "Compresión completada",
            (ArchiveState::Cancelled, true) => "Compresión cancelada",
            (ArchiveState::Failed, true) => "Compresión fallida",
            (ArchiveState::Running, false) => "Compressing",
            (ArchiveState::Finished, false) => "Compression complete",
            (ArchiveState::Cancelled, false) => "Compression cancelled",
            (ArchiveState::Failed, false) => "Compression failed",
        };
        let state = match (item.state, self.is_spanish()) {
            (ArchiveState::Running, true) => "Comprimiendo archivos",
            (ArchiveState::Finished, true) => "Completado",
            (ArchiveState::Cancelled, true) => "Cancelado",
            (ArchiveState::Failed, true) => "Error",
            (ArchiveState::Running, false) => "Compressing files",
            (ArchiveState::Finished, false) => "Completed",
            (ArchiveState::Cancelled, false) => "Cancelled",
            (ArchiveState::Failed, false) => "Error",
        };
        let current_name = if item.progress.file_name.is_empty() {
            item.destination
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(self.localized("Archivo", "File"))
                .to_string()
        } else {
            item.progress.file_name.clone()
        };
        let details = if item.progress.total == 0 {
            format!(
                "{} · {}",
                self.localized("Preparando", "Preparing"),
                item.format.extension().to_uppercase()
            )
        } else {
            if self.is_spanish() {
                format!(
                    "{} archivos  -  {} de {}",
                    item.progress.files,
                    format_size(Some(item.progress.completed)),
                    format_size(Some(item.progress.total))
                )
            } else {
                format!(
                    "{} files  -  {} of {}",
                    item.progress.files,
                    format_size(Some(item.progress.completed)),
                    format_size(Some(item.progress.total))
                )
            }
        };
        let id = item.id;
        let controls: Element<'_, Message> = if item.state == ArchiveState::Running {
            transfer_control_button(
                self.localized("Cancelar", "Cancel"),
                Message::CancelArchive(id),
                palette,
                self.font_size(),
            )
            .into()
        } else {
            Space::new().into()
        };
        let card_width = TRANSFER_WINDOW_WIDTH
            - WINDOW_BORDER_WIDTH * 2.0
            - TRANSFER_WINDOW_CARD_PADDING_X * 2.0;
        let current_name = ellipsize_to_width(&current_name, card_width - 230.0, self.font_size());
        let details = ellipsize_to_width(&details, card_width - 170.0, self.font_size() - 1.0);
        let state = ellipsize_to_width(state, card_width * 0.42, self.font_size() - 1.0);

        let body = column![
            row![
                column![
                    text(title).size(self.font_size()).color(palette.text),
                    text(current_name)
                        .size(self.font_size())
                        .color(palette.text)
                        .width(Length::Fill)
                        .wrapping(iced::widget::text::Wrapping::None),
                ]
                .spacing(3)
                .width(Length::Fill),
                text(format!("{:.0}%", progress * 100.0))
                    .size(self.font_size())
                    .color(palette.muted_text),
                controls,
            ]
            .spacing(9)
            .align_y(Alignment::Center),
            transfer_progress_bar(progress, palette, TRANSFER_PROGRESS_BAR_HEIGHT),
            row![
                text(state)
                    .size(self.font_size() - 1.0)
                    .color(palette.muted_text)
                    .wrapping(iced::widget::text::Wrapping::None),
                Space::new().width(Length::Fill),
                text(details)
                    .size(self.font_size() - 1.0)
                    .color(palette.muted_text)
                    .wrapping(iced::widget::text::Wrapping::None),
            ]
            .align_y(Alignment::Center)
            .spacing(8),
        ]
        .spacing(7)
        .padding([7, 10]);

        container(body)
            .width(Length::Fill)
            .center_y(Length::Fixed(TRANSFER_CARD_HEIGHT))
            .clip(true)
            .style(move |_| {
                container::Style::default()
                    .background(palette.input_bg)
                    .border(border::rounded(6).color(palette.border).width(1))
            })
            .into()
    }

    pub(in crate::iced_ui) fn archive_window_view(&self, palette: Palette) -> Element<'_, Message> {
        let items = self.archive_items();
        let panel_height = transfer_window_size_for_item_count(items.len()).height;
        let inner_height = (panel_height - WINDOW_BORDER_WIDTH * 2.0).max(0.0);
        let body_height = (inner_height - TRANSFER_WINDOW_TITLE_HEIGHT).max(0.0);
        let title_drag_area = mouse_area(
            container(
                text(self.localized("Compresiones", "Compressions"))
                    .size(self.font_size())
                    .color(palette.text)
                    .align_x(Horizontal::Center)
                    .width(Length::Fill),
            )
            .height(TRANSFER_WINDOW_TITLE_HEIGHT)
            .width(Length::Fill)
            .center_y(Length::Fill),
        )
        .on_press(Message::ArchiveWindowDrag);
        let title_bar = container(
            row![
                title_drag_area,
                native_window_minimize_button(Message::ArchiveWindowMinimize, palette),
            ]
            .align_y(Alignment::Center),
        )
        .height(TRANSFER_WINDOW_TITLE_HEIGHT)
        .width(Length::Fill)
        .style(move |_| {
            container::Style::default()
                .background(palette.overlay_title_bg)
                .border(border::rounded(border::top(WINDOW_RADIUS - 1.0)))
        });
        let content: Element<'_, Message> = if items.is_empty() {
            container(
                text(self.localized("No hay compresiones", "No compressions"))
                    .size(self.font_size())
                    .color(palette.muted_text),
            )
            .center(Length::Fill)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else {
            let item_count = items.len();
            let cards_height = progress_card_list_height(item_count);
            let visible_height = progress_visible_card_list_height(item_count);
            let mut list = column![].spacing(TRANSFER_CARD_GAP);
            for item in items {
                list = list.push(self.archive_item_card(item, palette));
            }
            let cards: Element<'_, Message> = scrollable(
                container(list)
                    .width(Length::Fill)
                    .height(Length::Fixed(cards_height)),
            )
            .width(Length::Fill)
            .height(Length::Fixed(visible_height))
            .into();
            column![
                Space::new().height(TRANSFER_WINDOW_CARD_TOP_GAP),
                container(cards)
                    .width(Length::Fill)
                    .padding([0.0, TRANSFER_WINDOW_CARD_PADDING_X])
                    .height(Length::Fixed(visible_height)),
                Space::new().height(TRANSFER_WINDOW_CARD_BOTTOM_PADDING),
            ]
            .spacing(0)
            .height(Length::Fixed(body_height))
            .into()
        };
        let body = container(content)
            .width(Length::Fill)
            .height(Length::Fixed(body_height))
            .style(move |_| container::Style::default().background(palette.overlay_bg));
        let inner_panel = container(
            column![title_bar, body]
                .width(Length::Fill)
                .height(Length::Fixed(inner_height)),
        )
        .width(Length::Fill)
        .height(Length::Fixed(inner_height))
        .clip(true)
        .style(move |_| {
            container::Style::default()
                .background(palette.overlay_bg)
                .border(border::rounded(WINDOW_RADIUS - WINDOW_BORDER_WIDTH))
        });
        let panel = container(inner_panel)
            .width(Length::Fill)
            .height(Length::Fixed(panel_height))
            .padding(WINDOW_BORDER_WIDTH)
            .clip(true)
            .style(move |_| {
                container::Style::default()
                    .background(Color::TRANSPARENT)
                    .border(
                        border::rounded(WINDOW_RADIUS)
                            .color(window_border_color(palette))
                            .width(WINDOW_BORDER_WIDTH),
                    )
            });
        container(column![panel, Space::new().height(Length::Fill)])
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style::default().background(Color::TRANSPARENT))
            .into()
    }
}
