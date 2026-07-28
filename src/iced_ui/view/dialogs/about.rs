use super::*;
use iced::widget::{column, row};

const ABOUT_WIDTH: f32 = 390.0;
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const REPOSITORY_LABEL: &str = "github.com/BryamContreras/BExplorer";

impl BExplorerIced {
    pub(in crate::iced_ui) fn about_modal(&self, palette: Palette) -> Element<'_, Message> {
        let about_width = self.modal_text_surface_width(ABOUT_WIDTH);
        let about_height = about_panel_height(self.font_size());
        let app_icon_size = self.ui_metric(96.0);
        let header = row![
            text(self.localized("Acerca de BExplorer", "About BExplorer"))
                .size(self.font_size() + 3.0)
                .color(palette.text)
                .width(Length::Fill),
            icon_button("x", Message::CloseAbout, palette, false, self.font_size(),),
        ]
        .align_y(Alignment::Center);

        let app_icon = iced_image::Image::new(app_icon_image_handle())
            .width(app_icon_size)
            .height(app_icon_size)
            .content_fit(ContentFit::Contain)
            .border_radius(border::radius(19.0));

        let app_tile = container(
            row![
                app_icon,
                column![
                    text("BExplorer")
                        .size(self.font_size() + 9.0)
                        .color(palette.text),
                    text(format!(
                        "{} {APP_VERSION}",
                        self.localized("Versión", "Version")
                    ))
                    .size(self.font_size())
                    .color(palette.muted_text),
                    text(self.localized(
                        "Explorador de archivos nativo para Windows y Linux.",
                        "Native file explorer for Windows and Linux.",
                    ))
                    .size(self.font_size())
                    .color(palette.muted_text),
                ]
                .spacing(5)
                .width(Length::Fill),
            ]
            .spacing(14)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .padding([4, 8]);

        let repository = Button::new(
            row![
                inline_icon("github", palette.text, self.ui_metric(24.0)),
                column![
                    text(self.localized("Repositorio", "Repository"))
                        .size(self.font_size() - 1.0)
                        .color(palette.muted_text),
                    text(REPOSITORY_LABEL)
                        .size(self.font_size())
                        .color(palette.accent),
                ]
                .spacing(2)
                .width(Length::Fill),
                inline_icon("open", palette.accent, self.ui_metric(17.0)),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .padding([8, 10])
        .on_press(Message::OpenRepository)
        .style(move |_, status| dialog_button_style(palette, false, status));

        let content = column![header, app_tile, repository,]
            .spacing(self.ui_metric(10.0))
            .padding(14.0 + self.ui_density())
            .height(about_height);

        let surface = container(content)
            .width(about_width)
            .height(about_height)
            .style(move |_| {
                container::Style::default()
                    .background(palette.menu_bg)
                    .border(border::rounded(10).color(palette.strong_border).width(1))
                    .shadow(iced::Shadow {
                        color: Color::from_rgba8(0, 0, 0, 0.42),
                        offset: iced::Vector::new(0.0, 14.0),
                        blur_radius: 30.0,
                    })
            });
        let surface = self.frosted_popup_surface(
            self.popup_backdrop.as_ref(),
            surface.into(),
            about_width,
            about_height,
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
                    0.42 * palette.text.a,
                ))
            })
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn about_metadata_comes_from_the_package() {
        let repository_url = env!("CARGO_PKG_REPOSITORY");
        assert_eq!(APP_VERSION, env!("CARGO_PKG_VERSION"));
        assert!(repository_url.starts_with("https://"));
        assert!(repository_url.contains("BExplorer"));
    }
}
