use super::*;
use iced::widget::{column, row};

impl BExplorerIced {
    pub(in crate::iced_ui) fn defender_modal(&self, palette: Palette) -> Element<'_, Message> {
        let progress = self.defender_progress.as_ref();
        let summary = self.defender_summary.as_ref();
        let scanned = progress.map(|item| item.scanned).unwrap_or_default();
        let total = progress.map(|item| item.total).unwrap_or_default();
        let threats = summary
            .map(|item| item.threats.len())
            .or_else(|| progress.map(|item| item.threats_found))
            .unwrap_or_default();
        let fraction = if total == 0 {
            0.0
        } else {
            scanned as f32 / total as f32
        };
        let elapsed = progress
            .map(|item| item.started.elapsed().as_secs())
            .unwrap_or_default();
        let title = if self.defender_active() {
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

        let mut body = column![
            row![
                text(title)
                    .size(self.font_size() + 2.0)
                    .color(palette.text)
                    .width(Length::Fill),
                icon_button("x", Message::CloseDefenderPanel, palette, false),
            ]
            .align_y(Alignment::Center),
            text(current)
                .size(self.font_size() - 1.0)
                .color(palette.muted_text),
            transfer_progress_bar(fraction, palette, 9.0),
            text(if self.is_spanish() {
                format!("{scanned} de {total} elementos · {threats} amenaza(s) · {elapsed} s")
            } else {
                format!("{scanned} of {total} items · {threats} threat(s) · {elapsed} s")
            })
            .size(self.font_size())
            .color(if threats > 0 {
                Color::from_rgb8(210, 72, 72)
            } else {
                palette.text
            }),
        ]
        .spacing(12);

        if let Some(summary) = summary {
            let mut threat_list = column![].spacing(5);
            for threat in summary.threats.iter().take(6) {
                let label = threat
                    .path
                    .as_ref()
                    .map(|path| format!("{} — {} — {}", threat.name, threat.status, path.display()))
                    .unwrap_or_else(|| format!("{} — {}", threat.name, threat.status));
                threat_list = threat_list.push(
                    text(ellipsize_text(&label, 72))
                        .size(self.font_size() - 1.0)
                        .color(palette.muted_text),
                );
            }
            if let Some(error) = summary.error.as_deref() {
                body = body.push(
                    text(ellipsize_text(error, 90))
                        .size(self.font_size() - 1.0)
                        .color(Color::from_rgb8(210, 72, 72)),
                );
            } else if let Some(output) = summary.outputs.last()
                && output.exit_code.is_some_and(|code| code != 0)
            {
                let detail = output
                    .output
                    .lines()
                    .rev()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or("Microsoft Defender requiere atención");
                body = body.push(
                    text(ellipsize_text(
                        &format!("{}: {detail}", output.target.display()),
                        90,
                    ))
                    .size(self.font_size() - 1.0)
                    .color(Color::from_rgb8(210, 72, 72)),
                );
            }
            body = body.push(threat_list);
        }

        let actions = if self.defender_active() {
            row![
                Space::new().width(Length::Fill),
                Button::new(text(self.localized("Cancelar", "Cancel")))
                    .padding([6, 10])
                    .on_press(Message::CancelDefenderScan)
                    .style(move |_, status| dialog_button_style(palette, false, status)),
            ]
        } else {
            let remove = Button::new(text(self.localized("Eliminar amenazas", "Remove threats")))
                .padding([6, 10])
                .on_press_maybe((threats > 0).then_some(Message::RemoveDefenderThreats))
                .style(move |_, status| dialog_button_style(palette, false, status));
            let exclude = Button::new(text(self.localized("Añadir exclusión", "Add exclusion")))
                .padding([6, 10])
                .on_press_maybe(
                    summary
                        .is_some_and(|item| !item.paths.is_empty())
                        .then_some(Message::ExcludeDefenderPaths),
                )
                .style(move |_, status| dialog_button_style(palette, false, status));
            let security = Button::new(text(
                self.localized("Seguridad de Windows", "Windows Security"),
            ))
            .padding([6, 10])
            .on_press(Message::OpenWindowsSecurity)
            .style(move |_, status| dialog_button_style(palette, false, status));
            row![
                remove,
                exclude,
                security,
                Space::new().width(Length::Fill),
                Button::new(text(self.localized("Cerrar", "Close")))
                    .padding([6, 10])
                    .on_press(Message::CloseDefenderPanel)
                    .style(move |_, status| dialog_button_style(palette, true, status)),
            ]
            .spacing(8)
        };
        body = body.push(actions.align_y(Alignment::Center));

        let panel = container(body.padding(18))
            .width(560)
            .style(move |_| elevated_panel_style(palette));
        container(panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .center(Length::Fill)
            .style(|_| container::Style::default().background(Color::from_rgba8(0, 0, 0, 0.24)))
            .into()
    }
}
