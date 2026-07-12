use super::*;
use iced::widget::{column, row};

impl BExplorerIced {
    pub(in crate::iced_ui) fn settings_modal(&self, palette: Palette) -> Element<'_, Message> {
        let font_size = self.config.font_size.round() as i32;
        let dark = self.is_dark_theme();
        let spanish = self.is_spanish();
        let color_picker_open = self.color_picker_open;
        let color_swatch = Button::new(container(Space::new().width(38).height(28)).style(
            move |_| {
                container::Style::default()
                    .background(accent_gradient(palette))
                    .border(border::rounded(5).color(palette.strong_border).width(1))
            },
        ))
        .padding(2)
        .on_press(Message::ToggleColorPicker)
        .style(move |_, status| dialog_button_style(palette, color_picker_open, status));

        let header = container(
            row![
                column![
                    text(self.localized("Configuración", "Settings"))
                        .size(self.font_size() + 3.0)
                        .color(palette.text),
                    text(self.localized(
                        "Aspecto y preferencias de la aplicación",
                        "Application appearance and preferences",
                    ))
                    .size(self.font_size() - 1.0)
                    .color(palette.muted_text),
                ]
                .spacing(3)
                .width(Length::Fill),
                icon_button("x", Message::ToggleSettings, palette, false),
            ]
            .align_y(Alignment::Center),
        )
        .padding([14, 16])
        .style(move |_| {
            container::Style::default()
                .background(mix_color(palette.title_bg, palette.menu_bg, 0.34))
                .border(border::rounded(8).color(palette.strong_border).width(1))
        });

        let language_options = vec!["Español".to_owned(), "English".to_owned()];
        let selected_language = if spanish { "Español" } else { "English" }.to_owned();
        let language_row = container(
            row![
                column![
                    text(self.localized("Idioma", "Language"))
                        .size(self.font_size())
                        .color(palette.text),
                    text(self.localized("Idioma de la interfaz", "Interface language"))
                        .size(self.font_size() - 1.0)
                        .color(palette.muted_text),
                ]
                .spacing(3)
                .width(Length::Fill),
                pick_list(
                    language_options,
                    Some(selected_language),
                    Message::SelectLanguage,
                )
                .text_size(self.font_size())
                .padding([5, 8])
                .style(move |_, status| settings_pick_list_style(palette, status))
                .menu_style(move |_| settings_pick_list_menu_style(palette))
                .width(Length::Fixed(142.0)),
            ]
            .align_y(Alignment::Center)
            .spacing(10),
        )
        .height(44)
        .padding([5, 9])
        .style(move |_| {
            container::Style::default()
                .background(mix_color(palette.input_bg, palette.header_bg, 0.42))
                .border(border::rounded(7).color(palette.border).width(1))
        });

        let (theme_options, selected_theme) = if spanish {
            (
                vec![
                    "Sistema".to_owned(),
                    "Claro".to_owned(),
                    "Oscuro".to_owned(),
                ],
                match self.config.theme {
                    ThemePreference::System => "Sistema",
                    ThemePreference::Light | ThemePreference::Gray => "Claro",
                    ThemePreference::Dark => "Oscuro",
                }
                .to_owned(),
            )
        } else {
            (
                vec!["System".to_owned(), "Light".to_owned(), "Dark".to_owned()],
                match self.config.theme {
                    ThemePreference::System => "System",
                    ThemePreference::Light | ThemePreference::Gray => "Light",
                    ThemePreference::Dark => "Dark",
                }
                .to_owned(),
            )
        };

        let font_row = container(
            row![
                text(self.localized("Tamaño de letra", "Font size"))
                    .size(self.font_size())
                    .color(palette.text)
                    .width(Length::Fill),
                icon_button("min", Message::FontDown, palette, false),
                container(
                    text(format!("{font_size} px"))
                        .size(self.font_size())
                        .color(palette.text)
                        .width(64)
                        .align_x(Horizontal::Center),
                )
                .padding([5, 4])
                .style(move |_| {
                    container::Style::default()
                        .background(palette.page_bg)
                        .border(border::rounded(4).color(palette.border).width(1))
                }),
                icon_button("add", Message::FontUp, palette, false),
            ]
            .align_y(Alignment::Center)
            .spacing(8),
        )
        .height(44)
        .padding([5, 9])
        .style(move |_| {
            container::Style::default()
                .background(mix_color(palette.input_bg, palette.header_bg, 0.42))
                .border(border::rounded(7).color(palette.border).width(1))
        });

        let accent_row = container(
            row![
                column![
                    text(self.localized("Color de resaltado", "Accent color"))
                        .size(self.font_size())
                        .color(palette.text),
                    text(if color_picker_open {
                        self.localized("Selector de color abierto", "Color picker open")
                    } else {
                        self.localized(
                            "Elige un color para toda la interfaz",
                            "Choose a color for the whole interface",
                        )
                    })
                    .size(self.font_size() - 1.0)
                    .color(palette.muted_text),
                ]
                .spacing(3)
                .width(Length::Fill),
                color_swatch,
            ]
            .align_y(Alignment::Center)
            .spacing(10),
        )
        .height(44)
        .padding([5, 9])
        .style(move |_| {
            container::Style::default()
                .background(if color_picker_open {
                    Background::Gradient(translucent_accent_gradient(palette, 0.14).into())
                } else {
                    Background::Color(mix_color(palette.input_bg, palette.header_bg, 0.42))
                })
                .border(
                    border::rounded(7)
                        .color(if color_picker_open {
                            mix_color(palette.strong_border, palette.accent, 0.52)
                        } else {
                            palette.border
                        })
                        .width(1),
                )
        });

        let theme_row = container(
            row![
                column![
                    text(self.localized("Tema", "Theme"))
                        .size(self.font_size())
                        .color(palette.text),
                    text(match self.config.theme {
                        ThemePreference::System => self.localized(
                            "Sigue la apariencia del sistema",
                            "Follows the system appearance",
                        ),
                        _ if dark => self.localized("Contraste oscuro", "Dark contrast"),
                        _ => self.localized("Contraste claro", "Light contrast"),
                    })
                    .size(self.font_size() - 1.0)
                    .color(palette.muted_text),
                ]
                .spacing(3)
                .width(Length::Fill),
                pick_list(theme_options, Some(selected_theme), Message::SelectTheme)
                    .text_size(self.font_size())
                    .padding([5, 8])
                    .style(move |_, status| settings_pick_list_style(palette, status))
                    .menu_style(move |_| settings_pick_list_menu_style(palette))
                    .width(Length::Fixed(142.0)),
            ]
            .align_y(Alignment::Center),
        )
        .height(44)
        .padding([5, 9])
        .style(move |_| {
            container::Style::default()
                .background(mix_color(palette.input_bg, palette.header_bg, 0.42))
                .border(border::rounded(7).color(palette.border).width(1))
        });

        let vibrancy_options = available_vibrancy_modes()
            .iter()
            .map(|mode| vibrancy_mode_label(*mode, spanish).to_owned())
            .collect::<Vec<_>>();
        let selected_vibrancy = vibrancy_mode_label(self.config.vibrancy, spanish).to_owned();
        let vibrancy_description = if self.config.vibrancy != VibrancyMode::None
            && !self.config.vibrancy_active
        {
            #[cfg(target_os = "linux")]
            if crate::platform::linux::is_gnome_wayland() {
                self.localized(
                    "GNOME requiere que la extensión Blur My Shell esté instalada y habilitada",
                    "GNOME requires the Blur My Shell extension to be installed and enabled",
                )
            } else {
                self.localized(
                    "El compositor no ofrece difuminado; se usa un fondo opaco",
                    "The compositor does not provide blur; an opaque background is used",
                )
            }
            #[cfg(not(target_os = "linux"))]
            self.localized(
                "El compositor no ofrece difuminado; se usa un fondo opaco",
                "The compositor does not provide blur; an opaque background is used",
            )
        } else {
            match self.config.vibrancy {
                VibrancyMode::None => self.localized(
                    "Usa superficies opacas normales",
                    "Uses regular opaque surfaces",
                ),
                #[cfg(target_os = "windows")]
                VibrancyMode::Mica => self.localized(
                    "Material nativo de Windows 11",
                    "Native Windows 11 material",
                ),
                #[cfg(target_os = "windows")]
                VibrancyMode::Acrylic => {
                    self.localized("Difuminado acrílico nativo", "Native acrylic blur")
                }
                #[cfg(target_os = "macos")]
                VibrancyMode::Blur => {
                    self.localized("Vibrancy nativa de macOS", "Native macOS vibrancy")
                }
                #[cfg(not(any(target_os = "windows", target_os = "macos")))]
                VibrancyMode::Blur => {
                    if crate::platform::linux::is_gnome_wayland() {
                        self.localized(
                            "Integración de aplicaciones con Blur My Shell",
                            "Application integration with Blur My Shell",
                        )
                    } else {
                        self.localized(
                        "Solicita difuminado al compositor; si no está disponible, usa un fondo opaco",
                        "Requests compositor blur; uses an opaque fallback when unavailable",
                    )
                    }
                }
                #[cfg(target_os = "windows")]
                VibrancyMode::Blur => {
                    self.localized("Difuminado nativo de ventana", "Native window blur")
                }
                #[cfg(target_os = "macos")]
                VibrancyMode::Mica | VibrancyMode::Acrylic => {
                    self.localized("Vibrancy nativa de macOS", "Native macOS vibrancy")
                }
                #[cfg(not(any(target_os = "windows", target_os = "macos")))]
                VibrancyMode::Mica | VibrancyMode::Acrylic => self.localized(
                    "Solicita difuminado al compositor; si no está disponible, usa un fondo opaco",
                    "Requests compositor blur; uses an opaque fallback when unavailable",
                ),
            }
        };
        let vibrancy_row = container(
            row![
                column![
                    text(self.localized("Efecto de ventana", "Window effect"))
                        .size(self.font_size())
                        .color(palette.text),
                    text(vibrancy_description)
                        .size(self.font_size() - 1.0)
                        .color(palette.muted_text),
                ]
                .spacing(3)
                .width(Length::Fill),
                pick_list(
                    vibrancy_options,
                    Some(selected_vibrancy),
                    Message::SelectVibrancy,
                )
                .text_size(self.font_size())
                .padding([5, 8])
                .style(move |_, status| settings_pick_list_style(palette, status))
                .menu_style(move |_| settings_pick_list_menu_style(palette))
                .width(Length::Fixed(142.0)),
            ]
            .align_y(Alignment::Center)
            .spacing(10),
        )
        .height(44)
        .padding([5, 9])
        .style(move |_| {
            container::Style::default()
                .background(mix_color(palette.input_bg, palette.header_bg, 0.42))
                .border(border::rounded(7).color(palette.border).width(1))
        });

        let vibrancy_intensity: Element<'_, Message> = if self.config.vibrancy != VibrancyMode::None
        {
            let intensity = self.config.vibrancy_intensity.clamp(15, 90);
            container(
                row![
                    text(self.localized("Intensidad", "Intensity"))
                        .size(self.font_size())
                        .color(palette.text)
                        .width(Length::Fixed(84.0)),
                    slider(15..=90, intensity, Message::SetVibrancyIntensity)
                        .step(1)
                        .on_release(Message::VibrancyIntensityReleased)
                        .width(Length::Fill),
                    text(format!("{intensity}%"))
                        .size(self.font_size() - 1.0)
                        .color(palette.muted_text)
                        .width(Length::Fixed(38.0))
                        .align_x(Horizontal::Right),
                ]
                .align_y(Alignment::Center)
                .spacing(9),
            )
            .height(40)
            .padding([4, 9])
            .style(move |_| {
                container::Style::default()
                    .background(mix_color(palette.input_bg, palette.header_bg, 0.42))
                    .border(border::rounded(7).color(palette.border).width(1))
            })
            .into()
        } else {
            Space::new().height(0).into()
        };

        let files_section = column![
            text(self.localized("ARCHIVOS", "FILES"))
                .size(self.font_size() - 1.0)
                .color(palette.muted_text),
            self.settings_file_option(
                self.localized("Mostrar extensiones de archivos", "Show file extensions"),
                self.localized(
                    "Muestra el sufijo, por ejemplo .pdf o .jpg",
                    "Shows the suffix, for example .pdf or .jpg",
                ),
                self.config.show_extensions,
                Message::ToggleShowExtensions,
                palette,
            ),
            self.settings_file_option(
                self.localized("Mostrar archivos ocultos", "Show hidden files"),
                self.localized(
                    "Los elementos ocultos se muestran con menor opacidad",
                    "Hidden items are shown with lower opacity",
                ),
                self.config.show_hidden,
                Message::ToggleShowHidden,
                palette,
            ),
        ]
        .spacing(6);

        let panel = container(
            column![
                header,
                column![
                    text(self.localized("GENERAL", "GENERAL"))
                        .size(self.font_size() - 1.0)
                        .color(palette.muted_text),
                    language_row,
                    theme_row,
                    text(self.localized("PERSONALIZACIÓN", "PERSONALIZATION"))
                        .size(self.font_size() - 1.0)
                        .color(palette.muted_text),
                    font_row,
                    accent_row,
                    vibrancy_row,
                    vibrancy_intensity,
                    files_section,
                ]
                .spacing(9)
                .padding([14, 16]),
            ]
            .spacing(0),
        )
        .width(470)
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

        let panel =
            self.frosted_popup_surface(self.popup_backdrop.as_ref(), panel.into(), 470.0, 570.0);
        let modal = container(panel)
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
            });

        if !color_picker_open {
            return modal.into();
        }

        let picker_x = ((self.window_size.width - 470.0) * 0.5 + 136.0).max(8.0);
        let picker_y = ((self.window_size.height - 310.0) * 0.5 + 158.0)
            .min((self.window_size.height - 330.0).max(8.0))
            .max(8.0);
        let picker_palette = palette.with_opacity(self.color_picker_fade_progress);
        let picker = float(opaque(self.color_picker_panel(picker_palette)))
            .translate(move |_, _| Vector::new(picker_x, picker_y));

        stack(vec![modal.into(), picker.into()])
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    pub(in crate::iced_ui) fn settings_file_option(
        &self,
        label: &'static str,
        description: &'static str,
        enabled: bool,
        message: Message,
        palette: Palette,
    ) -> Element<'_, Message> {
        let check = container(
            text(if enabled { "✓" } else { "" })
                .size(self.font_size())
                .color(if enabled {
                    palette.accent_text
                } else {
                    palette.muted_text
                }),
        )
        .width(Length::Fixed(19.0))
        .height(Length::Fixed(19.0))
        .center(Length::Fixed(19.0))
        .style(move |_| {
            let background: Background = if enabled {
                accent_gradient(palette).into()
            } else {
                palette.input_bg.into()
            };
            container::Style::default().background(background).border(
                border::rounded(4)
                    .color(if enabled {
                        palette.accent
                    } else {
                        palette.strong_border
                    })
                    .width(1),
            )
        });
        let content = row![
            check,
            column![
                text(label).size(self.font_size()).color(palette.text),
                text(description)
                    .size(self.font_size() - 1.0)
                    .color(palette.muted_text),
            ]
            .spacing(2)
            .width(Length::Fill),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        container(
            Button::new(content)
                .padding([7, 9])
                .width(Length::Fill)
                .on_press(message)
                .style(move |_, status| button_style(palette, false, status)),
        )
        .width(Length::Fill)
        .height(44)
        .style(move |_| {
            container::Style::default()
                .background(mix_color(palette.input_bg, palette.header_bg, 0.42))
                .border(border::rounded(7).color(palette.border).width(1))
        })
        .into()
    }
}
