use super::*;
use iced::widget::{column, row};

impl BExplorerIced {
    pub(in crate::iced_ui) fn error_dialog_modal(&self, palette: Palette) -> Element<'_, Message> {
        let Some(dialog) = &self.error_dialog else {
            return Space::new().into();
        };
        let font_size = self.font_size();
        let surface_width = self.modal_text_surface_width(500.0);
        let error_icon_size = self
            .ui_metric(32.0)
            .max(ui_text_line_height(font_size + 4.0));
        let error_color = Color::from_rgb8(210, 76, 76);
        let panel = column![
            row![
                container(text("!").size(font_size + 4.0).color(Color::WHITE),)
                    .width(error_icon_size)
                    .height(error_icon_size)
                    .center(error_icon_size)
                    .style(move |_| {
                        container::Style::default()
                            .background(error_color)
                            .border(border::rounded(16).color(error_color).width(1))
                    }),
                text(dialog.title.as_str())
                    .size(font_size + 2.0)
                    .color(palette.text)
                    .width(Length::Fill),
                icon_button(
                    "x",
                    Message::DismissErrorDialog,
                    palette,
                    false,
                    self.font_size(),
                ),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            text(dialog.message.as_str())
                .size(font_size)
                .color(palette.text)
                .width(Length::Fill),
            row![
                Space::new().width(Length::Fill),
                Button::new(text(self.localized("Aceptar", "OK")).size(font_size))
                    .padding([7, 16])
                    .on_press(Message::DismissErrorDialog)
                    .style(move |_, status| dialog_button_style(palette, true, status)),
            ],
        ]
        .spacing(18)
        .padding(18);
        let surface = container(panel).width(surface_width).style(move |_| {
            container::Style::default()
                .background(palette.menu_bg)
                .border(border::rounded(8).color(palette.strong_border).width(1))
                .shadow(iced::Shadow {
                    color: Color::from_rgba8(0, 0, 0, 0.24),
                    offset: iced::Vector::new(0.0, 10.0),
                    blur_radius: 24.0,
                })
        });
        let surface = self.frosted_popup_surface(
            self.popup_backdrop.as_ref(),
            surface.into(),
            surface_width,
            270.0,
        );
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
}
