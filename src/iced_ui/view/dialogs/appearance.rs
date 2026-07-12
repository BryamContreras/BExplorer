use super::*;
use iced::widget::{column, row};

impl BExplorerIced {
    pub(in crate::iced_ui) fn color_picker_panel(&self, palette: Palette) -> Element<'_, Message> {
        let color = self.config.accent_color;
        let (hue, saturation, value) = accent_hsv_from_color(color);
        let color_hex = format!("#{:02X}{:02X}{:02X}", color[0], color[1], color[2]);
        let marker_x = (saturation * COLOR_PICKER_PLANE_WIDTH - 8.0)
            .clamp(0.0, COLOR_PICKER_PLANE_WIDTH - 16.0);
        let marker_y = ((1.0 - value) * COLOR_PICKER_PLANE_HEIGHT - 8.0)
            .clamp(0.0, COLOR_PICKER_PLANE_HEIGHT - 16.0);

        let channel_row = row![
            color_channel_input("R", &self.color_rgb_inputs[0], 0, palette, self.font_size()),
            color_channel_input("G", &self.color_rgb_inputs[1], 1, palette, self.font_size()),
            color_channel_input("B", &self.color_rgb_inputs[2], 2, palette, self.font_size()),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        let hue_rgb = accent_color_from_hsv(hue, 1.0, 1.0);
        let plane_base = container(Space::new())
            .width(COLOR_PICKER_PLANE_WIDTH)
            .height(COLOR_PICKER_PLANE_HEIGHT)
            .style(move |_| {
                container::Style::default()
                    .background(Color::from_rgb8(hue_rgb[0], hue_rgb[1], hue_rgb[2]))
            });
        let white_overlay = container(Space::new())
            .width(COLOR_PICKER_PLANE_WIDTH)
            .height(COLOR_PICKER_PLANE_HEIGHT)
            .style(move |_| {
                container::Style::default().background(
                    gradient::Linear::new(std::f32::consts::FRAC_PI_2)
                        .add_stop(0.0, Color::WHITE)
                        .add_stop(1.0, Color::from_rgba8(255, 255, 255, 0.0)),
                )
            });
        let black_overlay = container(Space::new())
            .width(COLOR_PICKER_PLANE_WIDTH)
            .height(COLOR_PICKER_PLANE_HEIGHT)
            .style(|_| {
                container::Style::default().background(
                    gradient::Linear::new(0.0)
                        .add_stop(0.0, Color::BLACK)
                        .add_stop(1.0, Color::from_rgba8(0, 0, 0, 0.0)),
                )
            });
        let marker = column![
            Space::new().height(marker_y),
            row![
                Space::new().width(marker_x),
                container(Space::new())
                    .width(16)
                    .height(16)
                    .style(move |_| {
                        container::Style::default()
                            .background(accent_gradient(palette))
                            .border(border::rounded(8).color(Color::WHITE).width(1))
                    })
            ]
        ]
        .width(COLOR_PICKER_PLANE_WIDTH)
        .height(COLOR_PICKER_PLANE_HEIGHT);

        let plane_visual = stack(vec![
            plane_base.into(),
            white_overlay.into(),
            black_overlay.into(),
            marker.into(),
        ])
        .width(COLOR_PICKER_PLANE_WIDTH)
        .height(COLOR_PICKER_PLANE_HEIGHT);

        let plane = mouse_area(plane_visual)
            .on_press(Message::StartAccentPlaneDrag)
            .on_move(Message::AccentPlaneHover)
            .on_release(Message::FinishColorDrag)
            .interaction(mouse::Interaction::Crosshair);

        let hue_marker_x =
            ((hue / 360.0) * COLOR_PICKER_HUE_WIDTH - 1.5).clamp(0.0, COLOR_PICKER_HUE_WIDTH - 3.0);
        let hue_spectrum_base = container(Space::new())
            .width(COLOR_PICKER_HUE_WIDTH)
            .height(COLOR_PICKER_HUE_HEIGHT)
            .style(move |_| {
                container::Style::default()
                    .background(
                        gradient::Linear::new(std::f32::consts::FRAC_PI_2)
                            .add_stop(0.0, Color::from_rgb8(255, 0, 0))
                            .add_stop(0.17, Color::from_rgb8(255, 255, 0))
                            .add_stop(0.33, Color::from_rgb8(0, 255, 0))
                            .add_stop(0.50, Color::from_rgb8(0, 255, 255))
                            .add_stop(0.67, Color::from_rgb8(0, 0, 255))
                            .add_stop(0.83, Color::from_rgb8(255, 0, 255))
                            .add_stop(1.0, Color::from_rgb8(255, 0, 0)),
                    )
                    .border(border::rounded(4).color(palette.border).width(1))
            });
        let hue_marker = column![row![
            Space::new().width(hue_marker_x),
            container(Space::new())
                .width(3)
                .height(COLOR_PICKER_HUE_HEIGHT)
                .style(move |_| {
                    container::Style::default()
                        .background(Color::WHITE)
                        .border(border::rounded(1).color(Color::from_rgb8(0, 0, 0)).width(1))
                }),
        ]]
        .width(COLOR_PICKER_HUE_WIDTH)
        .height(COLOR_PICKER_HUE_HEIGHT);
        let hue_spectrum = mouse_area(
            stack(vec![hue_spectrum_base.into(), hue_marker.into()])
                .width(COLOR_PICKER_HUE_WIDTH)
                .height(COLOR_PICKER_HUE_HEIGHT),
        )
        .on_press(Message::StartAccentHueDrag)
        .on_move(Message::AccentHueHover)
        .on_release(Message::FinishColorDrag)
        .interaction(mouse::Interaction::Crosshair);

        let content = column![
            row![
                column![
                    text("Color de resaltado")
                        .size(self.font_size() + 1.0)
                        .color(palette.text),
                    text(color_hex)
                        .size(self.font_size() - 1.0)
                        .color(palette.muted_text),
                ]
                .spacing(2)
                .width(Length::Fill),
                icon_button("x", Message::ToggleColorPicker, palette, false),
                container(Space::new().width(30).height(30)).style(move |_| {
                    container::Style::default()
                        .background(accent_gradient(palette))
                        .border(border::rounded(5).color(palette.strong_border).width(1))
                }),
            ]
            .align_y(Alignment::Center),
            plane,
            text("Tono")
                .size(self.font_size() - 1.0)
                .color(palette.muted_text),
            hue_spectrum,
            channel_row,
            container(Space::new())
                .width(Length::Fill)
                .height(16)
                .style(move |_| {
                    container::Style::default()
                        .background(accent_gradient(palette))
                        .border(border::rounded(4).color(palette.border).width(1))
                }),
        ]
        .spacing(8)
        .padding(12)
        .width(COLOR_PICKER_WIDTH);

        let panel = container(content)
            .width(COLOR_PICKER_WIDTH)
            .style(move |_| {
                container::Style::default()
                    .background(mix_color(palette.menu_bg, palette.title_bg, 0.28))
                    .border(border::rounded(9).color(palette.strong_border).width(1))
                    .shadow(iced::Shadow {
                        color: Color::from_rgba8(0, 0, 0, 0.42),
                        offset: iced::Vector::new(0.0, 10.0),
                        blur_radius: 24.0,
                    })
            });
        self.frosted_popup_surface(
            self.color_picker_backdrop.as_ref(),
            panel.into(),
            COLOR_PICKER_WIDTH,
            400.0,
        )
    }
}
