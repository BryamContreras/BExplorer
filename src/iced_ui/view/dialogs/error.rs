use super::*;
use iced::widget::{column, row};

impl BExplorerIced {
    pub(in crate::iced_ui) fn error_dialog_modal(&self, palette: Palette) -> Element<'_, Message> {
        let Some(dialog) = &self.error_dialog else {
            return Space::new().into();
        };
        let font_size = self.font_size();
        let error_color = Color::from_rgb8(210, 76, 76);
        let panel = column![
            row![
                container(text("!").size(font_size + 4.0).color(Color::WHITE),)
                    .width(32)
                    .height(32)
                    .center(32)
                    .style(move |_| {
                        container::Style::default()
                            .background(error_color)
                            .border(border::rounded(16).color(error_color).width(1))
                    }),
                text(dialog.title.as_str())
                    .size(font_size + 2.0)
                    .color(palette.text)
                    .width(Length::Fill),
                icon_button("x", Message::DismissErrorDialog, palette, false),
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
        let surface = container(panel).width(500).style(move |_| {
            container::Style::default()
                .background(palette.menu_bg)
                .border(border::rounded(8).color(palette.strong_border).width(1))
                .shadow(iced::Shadow {
                    color: Color::from_rgba8(0, 0, 0, 0.24),
                    offset: iced::Vector::new(0.0, 10.0),
                    blur_radius: 24.0,
                })
        });
        let surface =
            self.frosted_popup_surface(self.popup_backdrop.as_ref(), surface.into(), 500.0, 270.0);
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
