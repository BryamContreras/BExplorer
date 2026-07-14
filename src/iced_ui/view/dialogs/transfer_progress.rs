use super::*;
use iced::widget::{column, row};

impl BExplorerIced {
    pub(in crate::iced_ui) fn transfer_item_card(
        &self,
        item: TransferDisplayState,
        palette: Palette,
    ) -> Element<'_, Message> {
        let card_bg = palette.native_utility_card_background(self.config.vibrancy_active);
        let progress = if item.total_bytes == 0 {
            0.0
        } else {
            (item.copied_bytes as f32 / item.total_bytes as f32).clamp(0.0, 1.0)
        };
        let is_delete = matches!(
            item.kind,
            TransferDisplayKind::Trash | TransferDisplayKind::PermanentDelete
        );
        let files = if item.total_files == 0 {
            self.localized("Preparando archivos", "Preparing files")
                .to_owned()
        } else {
            if self.is_spanish() {
                format!("{} de {} elementos", item.files_done, item.total_files)
            } else {
                format!("{} of {} items", item.files_done, item.total_files)
            }
        };
        let size = if item.total_bytes == 0 {
            self.localized("Calculando tamaño", "Calculating size")
                .to_owned()
        } else {
            format!(
                "{} {} {}",
                format_size(Some(item.copied_bytes)),
                self.localized("de", "of"),
                format_size(Some(item.total_bytes))
            )
        };
        let speed = if item.bytes_per_second > 0.0
            && matches!(item.state, TransferState::Copying | TransferState::Paused)
        {
            format!("{} / s", format_size(Some(item.bytes_per_second as u64)))
        } else {
            String::new()
        };
        let details = if is_delete {
            if self.is_spanish() {
                format!("{} elemento(s)", item.total_files)
            } else {
                format!("{} item(s)", item.total_files)
            }
        } else if speed.is_empty() {
            format!("{files}  -  {size}")
        } else {
            format!("{files}  -  {size}  -  {speed}")
        };
        let state = self.localized_transfer_state(&item);
        let title = self.localized_transfer_title(&item);
        let id = item.id;
        let controls: Element<'_, Message> =
            if matches!(item.state, TransferState::Copying | TransferState::Paused)
                && matches!(
                    item.kind,
                    TransferDisplayKind::Copy | TransferDisplayKind::Move
                )
            {
                let pause_label = if item.state == TransferState::Paused {
                    self.localized("Reanudar", "Resume")
                } else {
                    self.localized("Pausar", "Pause")
                };
                row![
                    transfer_control_button(
                        pause_label,
                        Message::ToggleTransferPause(id),
                        palette,
                        self.font_size(),
                    ),
                    transfer_control_button(
                        self.localized("Cancelar", "Cancel"),
                        Message::CancelTransfer(id),
                        palette,
                        self.font_size(),
                    ),
                ]
                .spacing(6)
                .align_y(Alignment::Center)
                .into()
            } else if item.state == TransferState::Pending {
                transfer_control_button(
                    self.localized("Cancelar", "Cancel"),
                    Message::CancelTransfer(id),
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
        let current_name =
            ellipsize_to_width(&item.current_name, card_width - 230.0, self.font_size());
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
                text(if is_delete {
                    String::new()
                } else {
                    format!("{:.0}%", progress * 100.0)
                })
                .size(self.font_size())
                .color(palette.muted_text),
                controls,
            ]
            .spacing(9)
            .align_y(Alignment::Center),
            if is_delete {
                indeterminate_progress_bar(
                    self.transfer_progress_phase,
                    palette,
                    TRANSFER_PROGRESS_BAR_HEIGHT,
                )
            } else {
                transfer_progress_bar(progress, palette, TRANSFER_PROGRESS_BAR_HEIGHT)
            },
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
                    .background(card_bg)
                    .border(border::rounded(6).color(palette.border).width(1))
            })
            .into()
    }

    pub(in crate::iced_ui) fn transfer_window_view(
        &self,
        palette: Palette,
    ) -> Element<'_, Message> {
        let (window_bg, window_title_bg) = palette.native_utility_backgrounds();
        let items = self.transfer_items();
        let panel_height = transfer_window_size_for_item_count(items.len()).height;
        let inner_height = (panel_height - WINDOW_BORDER_WIDTH * 2.0).max(0.0);
        let body_height = (inner_height - TRANSFER_WINDOW_TITLE_HEIGHT).max(0.0);
        let title_drag_area = mouse_area(
            container(
                text(self.localized("Transferencias", "Transfers"))
                    .size(self.font_size())
                    .color(palette.text)
                    .align_x(Horizontal::Center)
                    .width(Length::Fill),
            )
            .height(TRANSFER_WINDOW_TITLE_HEIGHT)
            .width(Length::Fill)
            .center_y(Length::Fill),
        )
        .on_press(Message::TransferWindowDrag);
        let title_bar = container(
            row![
                title_drag_area,
                native_window_minimize_button(Message::TransferWindowMinimize, palette),
            ]
            .align_y(Alignment::Center),
        )
        .height(TRANSFER_WINDOW_TITLE_HEIGHT)
        .width(Length::Fill)
        .style(move |_| {
            container::Style::default()
                .background(window_title_bg)
                .border(border::rounded(border::top(WINDOW_RADIUS - 1.0)))
        });

        let content: Element<'_, Message> = if items.is_empty() {
            container(
                text(self.localized("No hay transferencias", "No transfers"))
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
                list = list.push(self.transfer_item_card(item, palette));
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
            .style(move |_| container::Style::default().background(window_bg));

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
                .background(window_bg)
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
