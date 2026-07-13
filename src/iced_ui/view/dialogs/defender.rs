use super::*;
use iced::widget::{column, row};

impl BExplorerIced {
    /// Native Defender window using the same title bar and card treatment as
    /// transfer and compression progress windows.
    pub(in crate::iced_ui) fn defender_window_view(
        &self,
        palette: Palette,
    ) -> Element<'_, Message> {
        let progress = self.defender_progress.as_ref();
        let summary = self.defender_summary.as_ref();
        let active = self.defender_active();
        let scanned = progress.map(|item| item.scanned).unwrap_or_default();
        let total = progress.map(|item| item.total).unwrap_or_default();
        let threats = summary
            .map(|item| item.threats.len())
            .or_else(|| progress.map(|item| item.threats_found))
            .unwrap_or_default();
        let fraction = if total == 0 {
            0.0
        } else {
            (scanned as f32 / total as f32).clamp(0.0, 1.0)
        };
        let elapsed = if active {
            progress
                .map(|item| item.started.elapsed().as_secs())
                .unwrap_or_default()
        } else {
            summary
                .map(|item| item.elapsed.as_secs())
                .unwrap_or_default()
        };
        let card_title = if active {
            self.localized(
                "Analizando con Microsoft Defender",
                "Scanning with Microsoft Defender",
            )
        } else {
            self.localized(
                "Resultado de Microsoft Defender",
                "Microsoft Defender result",
            )
        };
        let current = progress
            .and_then(|item| item.current_path.as_ref())
            .map(|path| ellipsize_text(&path.display().to_string(), 62))
            .unwrap_or_else(|| {
                self.localized("Preparando análisis…", "Preparing scan…")
                    .into()
            });
        let cancelled = progress.is_some_and(|item| item.state == DefenderScanState::Cancelled)
            || summary.is_some_and(|item| item.state == DefenderScanState::Cancelled);
        let details = if self.is_spanish() {
            let details =
                format!("{scanned} de {total} elementos · {threats} amenaza(s) · {elapsed} s");
            if cancelled {
                format!("{details} · Cancelado")
            } else {
                details
            }
        } else {
            let details = format!("{scanned} of {total} items · {threats} threat(s) · {elapsed} s");
            if cancelled {
                format!("{details} · Cancelled")
            } else {
                details
            }
        };

        let actions: Element<'_, Message> = if active {
            row![
                Space::new().width(Length::Fill),
                Button::new(text(self.localized("Cancelar", "Cancel")))
                    .padding([6, 10])
                    .on_press(Message::CancelDefenderScan)
                    .style(move |_, status| dialog_button_style(palette, false, status)),
            ]
            .align_y(Alignment::Center)
            .into()
        } else {
            let close = Button::new(text(self.localized("Cerrar", "Close")))
                .padding([6, 10])
                .on_press(Message::CloseDefenderPanel)
                .style(move |_, status| dialog_button_style(palette, true, status));
            if threats == 0 {
                row![Space::new().width(Length::Fill), close]
                    .align_y(Alignment::Center)
                    .into()
            } else {
                let security = Button::new(text(
                    self.localized("Abrir Seguridad de Windows", "Open Windows Security"),
                ))
                .padding([6, 10])
                .on_press(Message::OpenWindowsSecurity)
                .style(move |_, status| dialog_button_style(palette, false, status));
                row![security, Space::new().width(Length::Fill), close]
                    .spacing(6)
                    .align_y(Alignment::Center)
                    .into()
            }
        };

        let mut card_body = column![
            row![
                column![
                    text(card_title).size(self.font_size()).color(palette.text),
                    text(current)
                        .size(self.font_size() - 1.0)
                        .color(palette.muted_text)
                        .width(Length::Fill)
                        .wrapping(iced::widget::text::Wrapping::None),
                ]
                .spacing(3)
                .width(Length::Fill),
                text(if active && total > 1 {
                    format!("{:.0}%", fraction * 100.0)
                } else {
                    String::new()
                })
                .size(self.font_size())
                .color(palette.muted_text),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            if active && total <= 1 {
                indeterminate_progress_bar(
                    self.transfer_progress_phase,
                    palette,
                    TRANSFER_PROGRESS_BAR_HEIGHT,
                )
            } else {
                transfer_progress_bar(fraction, palette, TRANSFER_PROGRESS_BAR_HEIGHT)
            },
            text(details)
                .size(self.font_size() - 1.0)
                .color(if threats > 0 || cancelled {
                    Color::from_rgb8(210, 72, 72)
                } else {
                    palette.muted_text
                })
                .wrapping(iced::widget::text::Wrapping::None),
        ]
        .spacing(7);

        if let Some(summary) = summary {
            if let Some(error) = summary.error.as_deref() {
                card_body = card_body.push(
                    text(ellipsize_text(error, 78))
                        .size(self.font_size() - 1.0)
                        .color(Color::from_rgb8(210, 72, 72)),
                );
            }
            for threat in summary.threats.iter().take(4) {
                let label = threat
                    .path
                    .as_ref()
                    .map(|path| format!("{} — {} — {}", threat.name, threat.status, path.display()))
                    .unwrap_or_else(|| format!("{} — {}", threat.name, threat.status));
                card_body = card_body.push(
                    text(ellipsize_text(&label, 76))
                        .size(self.font_size() - 1.0)
                        .color(Color::from_rgb8(210, 72, 72)),
                );
            }
        }
        card_body = card_body.push(actions);

        let panel_height = self.defender_window_size().height;
        let inner_height = (panel_height - WINDOW_BORDER_WIDTH * 2.0).max(0.0);
        let body_height = (inner_height - TRANSFER_WINDOW_TITLE_HEIGHT).max(0.0);
        let title_drag_area = mouse_area(
            container(
                text(self.localized("Microsoft Defender", "Microsoft Defender"))
                    .size(self.font_size())
                    .color(palette.text)
                    .align_x(Horizontal::Center)
                    .width(Length::Fill),
            )
            .height(TRANSFER_WINDOW_TITLE_HEIGHT)
            .width(Length::Fill)
            .center_y(Length::Fill),
        )
        .on_press(Message::DefenderWindowDrag);
        let title_bar = container(
            row![
                title_drag_area,
                native_window_minimize_button(Message::DefenderWindowMinimize, palette),
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

        let card_height = if summary.is_some_and(|summary| summary.error.is_some()) {
            DEFENDER_ERROR_CARD_HEIGHT
        } else {
            DEFENDER_CARD_HEIGHT
        };
        let card = container(card_body.padding([8, 10]))
            .width(Length::Fill)
            .center_y(Length::Fixed(card_height))
            .style(move |_| {
                container::Style::default()
                    .background(palette.input_bg)
                    .border(border::rounded(6).color(palette.border).width(1))
            });
        let content = scrollable(container(card).width(Length::Fill).padding([6, 4]))
            .width(Length::Fill)
            .height(Length::Fixed(body_height));
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

    /// Threats get their own window so the compact scan result never needs to
    /// stretch or clip its action row.
    pub(in crate::iced_ui) fn defender_threats_window_view(
        &self,
        palette: Palette,
    ) -> Element<'_, Message> {
        let threats = self
            .defender_summary
            .as_ref()
            .map(|summary| summary.threats.as_slice())
            .unwrap_or_default();
        let visible_count = threats
            .len()
            .clamp(1, DEFENDER_THREAT_WINDOW_VISIBLE_CARD_LIMIT);
        let cards_height = threats.len() as f32 * DEFENDER_THREAT_CARD_HEIGHT
            + threats.len().saturating_sub(1) as f32 * DEFENDER_THREAT_CARD_GAP;
        let visible_cards_height = visible_count as f32 * DEFENDER_THREAT_CARD_HEIGHT
            + visible_count.saturating_sub(1) as f32 * DEFENDER_THREAT_CARD_GAP;
        let panel_height = defender_threats_window_size(threats.len()).height;
        let inner_height = (panel_height - WINDOW_BORDER_WIDTH * 2.0).max(0.0);
        let body_height = (inner_height - TRANSFER_WINDOW_TITLE_HEIGHT).max(0.0);

        let title_drag_area = mouse_area(
            container(
                text(self.localized(
                    "Amenazas de Microsoft Defender",
                    "Microsoft Defender threats",
                ))
                .size(self.font_size())
                .color(palette.text)
                .align_x(Horizontal::Center)
                .width(Length::Fill),
            )
            .height(TRANSFER_WINDOW_TITLE_HEIGHT)
            .width(Length::Fill)
            .center_y(Length::Fill),
        )
        .on_press(Message::DefenderThreatsWindowDrag);
        let title_bar = container(
            row![
                title_drag_area,
                native_window_minimize_button(Message::DefenderThreatsWindowMinimize, palette),
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

        let mut threat_list = column![].spacing(DEFENDER_THREAT_CARD_GAP);
        for threat in threats {
            let status = match threat.status.as_str() {
                "Remediated" => self
                    .localized("Neutralizada por Defender", "Remediated")
                    .to_owned(),
                "Action required" => self
                    .localized("Requiere acción", "Action required")
                    .to_owned(),
                _ => threat.status.clone(),
            };
            let path = threat
                .path
                .as_ref()
                .map(|path| ellipsize_text(&path.display().to_string(), 82))
                .unwrap_or_else(|| {
                    self.localized("Ruta no disponible", "Path unavailable")
                        .into()
                });
            let details = column![
                text(&threat.name)
                    .size(self.font_size())
                    .color(Color::from_rgb8(220, 82, 82)),
                row![
                    text(status)
                        .size(self.font_size() - 1.0)
                        .color(palette.muted_text),
                    Space::new().width(Length::Fill),
                    text(path)
                        .size(self.font_size() - 1.0)
                        .color(palette.muted_text)
                        .wrapping(iced::widget::text::Wrapping::None),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            ]
            .spacing(4)
            .padding([6, 10]);
            threat_list = threat_list.push(
                container(details)
                    .width(Length::Fill)
                    .height(Length::Fixed(DEFENDER_THREAT_CARD_HEIGHT))
                    .clip(true)
                    .style(move |_| {
                        container::Style::default()
                            .background(palette.input_bg)
                            .border(
                                border::rounded(6)
                                    .color(Color::from_rgb8(170, 68, 68))
                                    .width(1),
                            )
                    }),
            );
        }
        if threats.is_empty() {
            threat_list = threat_list.push(
                container(text(self.localized("No hay amenazas", "No threats")))
                    .height(Length::Fixed(DEFENDER_THREAT_CARD_HEIGHT))
                    .center(Length::Fill),
            );
        }

        let remediation = Button::new(text(if self.defender_threat_remediation_pending {
            self.localized("Eliminando…", "Removing…")
        } else {
            self.localized("Eliminar amenazas", "Remove threats")
        }))
        .padding([6, 10])
        .on_press_maybe(
            (!self.defender_threat_remediation_pending)
                .then_some(Message::RemediateDefenderThreats),
        )
        .style(move |_, status| dialog_button_style(palette, true, status));
        let security = Button::new(text(
            self.localized("Abrir Seguridad de Windows", "Open Windows Security"),
        ))
        .padding([6, 10])
        .on_press(Message::OpenWindowsSecurity)
        .style(move |_, status| dialog_button_style(palette, false, status));
        let close = Button::new(text(self.localized("Cerrar", "Close")))
            .padding([6, 10])
            .on_press(Message::CloseDefenderPanel)
            .style(move |_, status| dialog_button_style(palette, true, status));
        let cards_content = row![
            container(threat_list).width(Length::Fill),
            Space::new().width(12),
        ]
        .width(Length::Fill)
        .height(Length::Fixed(cards_height.max(DEFENDER_THREAT_CARD_HEIGHT)));
        let cards = scrollable(
            cards_content
                .width(Length::Fill)
                .height(Length::Fixed(cards_height.max(DEFENDER_THREAT_CARD_HEIGHT))),
        )
        .width(Length::Fill)
        .height(Length::Fixed(visible_cards_height))
        .style(move |theme, status| explorer_scrollable_style(palette, theme, status, 1.0));
        let (summary_text, summary_color) = if self.defender_threat_remediation_pending {
            (
                self.localized(
                    "Defender está procesando las amenazas…",
                    "Defender is processing the threats…",
                )
                .to_owned(),
                palette.muted_text,
            )
        } else if let Some((message, failed)) = &self.defender_threat_remediation_message {
            (
                message.clone(),
                if *failed {
                    Color::from_rgb8(220, 82, 82)
                } else {
                    palette.muted_text
                },
            )
        } else if self.is_spanish() {
            (
                format!("{} amenaza(s) detectada(s)", threats.len()),
                Color::from_rgb8(220, 82, 82),
            )
        } else {
            (
                format!("{} threat(s) detected", threats.len()),
                Color::from_rgb8(220, 82, 82),
            )
        };
        let content = column![
            text(summary_text)
                .size(self.font_size())
                .color(summary_color),
            Space::new().height(DEFENDER_THREAT_SECTION_GAP),
            cards,
            Space::new().height(DEFENDER_THREAT_SECTION_GAP),
            row![
                remediation,
                security,
                Space::new().width(Length::Fill),
                close
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        ]
        .spacing(0)
        .padding([6, 10])
        .width(Length::Fill)
        .height(Length::Fixed(body_height));
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
