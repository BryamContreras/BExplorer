use super::*;
use iced::widget::{column, row};

impl BExplorerIced {
    pub(in crate::iced_ui) fn settings_modal(&self, palette: Palette) -> Element<'_, Message> {
        let font_size = self.config.font_size.round() as i32;
        let density = self.ui_density();
        let stacked_content_height = ui_text_line_height(self.font_size())
            + ui_text_line_height(self.font_size() - 1.0)
            + 3.0;
        let stacked_row_padding =
            ((self.stacked_text_control_height(44.0) - stacked_content_height) * 0.5).max(0.0);
        let single_row_height = self.ui_metric(44.0);
        let single_row_padding =
            ((single_row_height - TITLE_BUTTON_HEIGHT) * 0.5).max(stacked_row_padding);
        let panel_width = self.modal_text_surface_width(SETTINGS_PANEL_WIDTH);
        let panel_height = settings_panel_height(
            self.font_size(),
            self.window_size.height,
            self.config.vibrancy != VibrancyMode::None,
        );
        let dark = self.is_dark_theme();
        let spanish = self.is_spanish();
        let color_picker_open = self.color_picker_open;
        // KWin Settings opens without a frozen backdrop so the real window can
        // be previewed underneath. Keep its modal veil light; otherwise it
        // masks the opacity difference the user is trying to tune.
        let live_vibrancy_preview = self.uses_linux_surface_blur() && self.popup_backdrop.is_none();
        let modal_veil_alpha = if live_vibrancy_preview { 0.04 } else { 0.42 };
        let color_swatch = Button::new(
            container(
                Space::new()
                    .width(self.ui_metric(38.0))
                    .height(self.ui_metric(28.0)),
            )
            .style(move |_| {
                container::Style::default()
                    .background(accent_gradient(palette))
                    .border(border::rounded(5).color(palette.strong_border).width(1))
            }),
        )
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
                icon_button(
                    "x",
                    Message::ToggleSettings,
                    palette,
                    false,
                    self.font_size(),
                ),
            ]
            .align_y(Alignment::Center),
        )
        .padding([self.ui_vertical_padding(10.0), 16.0 + density])
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
                .padding([self.ui_vertical_padding(5.0), 8.0 + density])
                .style(move |_, status| settings_pick_list_style(palette, status))
                .menu_style(move |_| settings_pick_list_menu_style(palette))
                .width(Length::Fixed(self.ui_metric(142.0))),
            ]
            .align_y(Alignment::Center)
            .spacing(10),
        )
        .padding([stacked_row_padding, 9.0 + density])
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
                icon_button("min", Message::FontDown, palette, false, self.font_size(),),
                container(
                    text(format!("{font_size} px"))
                        .size(self.font_size())
                        .color(palette.text)
                        .width(self.ui_metric(64.0))
                        .align_x(Horizontal::Center),
                )
                .padding([stacked_row_padding, 4.0 + density])
                .style(move |_| {
                    container::Style::default()
                        .background(palette.page_bg)
                        .border(border::rounded(4).color(palette.border).width(1))
                }),
                icon_button("add", Message::FontUp, palette, false, self.font_size(),),
            ]
            .align_y(Alignment::Center)
            .spacing(8),
        )
        .height(single_row_height)
        .padding([single_row_padding, 9.0 + density])
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
        .padding([stacked_row_padding, 9.0 + density])
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
                    .padding([self.ui_vertical_padding(5.0), 8.0 + density])
                    .style(move |_, status| settings_pick_list_style(palette, status))
                    .menu_style(move |_| settings_pick_list_menu_style(palette))
                    .width(Length::Fixed(self.ui_metric(142.0))),
            ]
            .align_y(Alignment::Center),
        )
        .padding([stacked_row_padding, 9.0 + density])
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
                .padding([self.ui_vertical_padding(5.0), 8.0 + density])
                .style(move |_, status| settings_pick_list_style(palette, status))
                .menu_style(move |_| settings_pick_list_menu_style(palette))
                .width(Length::Fixed(self.ui_metric(142.0))),
            ]
            .align_y(Alignment::Center)
            .spacing(10),
        )
        .padding([stacked_row_padding, 9.0 + density])
        .style(move |_| {
            container::Style::default()
                .background(mix_color(palette.input_bg, palette.header_bg, 0.42))
                .border(border::rounded(7).color(palette.border).width(1))
        });

        let vibrancy_intensity: Element<'_, Message> = if self.config.vibrancy != VibrancyMode::None
        {
            let intensity = self.config.vibrancy_intensity.min(100);
            let intensity_label = if self.uses_linux_surface_blur() {
                self.localized("Transparencia", "Transparency")
            } else {
                self.localized("Intensidad", "Intensity")
            };
            let intensity_label_width =
                adaptive_text_slot_width(intensity_label, self.font_size(), 84.0);
            container(
                row![
                    text(intensity_label)
                        .size(self.font_size())
                        .color(palette.text)
                        .width(Length::Fixed(intensity_label_width)),
                    slider(0..=100, intensity, Message::SetVibrancyIntensity)
                        .step(1)
                        .on_release(Message::VibrancyIntensityReleased)
                        .style(move |_, status| settings_slider_style(palette, status))
                        .width(Length::Fill),
                    text(format!("{intensity}%"))
                        .size(self.font_size() - 1.0)
                        .color(palette.muted_text)
                        .width(Length::Fixed(self.ui_metric(38.0)))
                        .align_x(Horizontal::Right),
                ]
                .align_y(Alignment::Center)
                .spacing(9),
            )
            .height(self.ui_metric(40.0))
            .padding([self.ui_vertical_padding(4.0), 9.0 + density])
            .style(move |_| {
                container::Style::default()
                    .background(mix_color(palette.input_bg, palette.header_bg, 0.42))
                    .border(border::rounded(7).color(palette.border).width(1))
            })
            .into()
        } else {
            Space::new().height(0).into()
        };

        let horizontal_padding = 16.0 + self.ui_density();
        let column_spacing = self.ui_metric(12.0);
        let flow_column_width =
            ((panel_width - horizontal_padding * 2.0 - column_spacing) * 0.5).max(1.0);
        // Fill the left column from top to bottom, then continue naturally on
        // the right. Pair every section heading with its first control so a
        // wrap can never leave an orphan heading at the bottom of a column.
        let general_start = column![
            text(self.localized("GENERAL", "GENERAL"))
                .size(self.font_size() - 1.0)
                .color(palette.muted_text),
            language_row,
        ]
        .spacing(9);
        let personalization_start = column![
            text(self.localized("PERSONALIZACIÓN", "PERSONALIZATION"))
                .size(self.font_size() - 1.0)
                .color(palette.muted_text),
            font_row,
        ]
        .spacing(9);
        let files_start = column![
            text(self.localized("ARCHIVOS", "FILES"))
                .size(self.font_size() - 1.0)
                .color(palette.muted_text),
            self.settings_check_option(
                self.localized("Mostrar extensiones de archivos", "Show file extensions"),
                self.localized(
                    "Muestra el sufijo, por ejemplo .pdf o .jpg",
                    "Shows the suffix, for example .pdf or .jpg",
                ),
                self.config.show_extensions,
                Message::ToggleShowExtensions,
                palette,
            ),
        ]
        .spacing(6);
        let sidebar_start = column![
            text(self.localized("MENÚ LATERAL", "SIDE MENU"))
                .size(self.font_size() - 1.0)
                .color(palette.muted_text),
            self.settings_check_option(
                self.localized("Mostrar Marcadores", "Show Bookmarks"),
                self.localized(
                    "Muestra las ubicaciones guardadas en el menú lateral",
                    "Shows saved locations in the side menu",
                ),
                self.config.show_sidebar_bookmarks,
                Message::ToggleShowSidebarBookmarks,
                palette,
            ),
        ]
        .spacing(6);

        let settings_flow = column![
            settings_flow_item(general_start, flow_column_width),
            settings_flow_item(theme_row, flow_column_width),
            settings_flow_item(personalization_start, flow_column_width),
            settings_flow_item(accent_row, flow_column_width),
            settings_flow_item(vibrancy_row, flow_column_width),
            settings_flow_item(vibrancy_intensity, flow_column_width),
            settings_flow_item(files_start, flow_column_width),
            settings_flow_item(
                self.settings_check_option(
                    self.localized("Mostrar archivos ocultos", "Show hidden files"),
                    self.localized(
                        "Los elementos ocultos se muestran con menor opacidad",
                        "Hidden items are shown with lower opacity",
                    ),
                    self.config.show_hidden,
                    Message::ToggleShowHidden,
                    palette,
                ),
                flow_column_width,
            ),
            settings_flow_item(
                self.settings_check_option(
                    self.localized(
                        "Mostrar unidades del sistema ocultas",
                        "Show hidden system drives",
                    ),
                    self.localized(
                        "Incluye particiones y montajes reservados para el sistema",
                        "Includes partitions and mounts reserved for the system",
                    ),
                    self.config.show_hidden_system_drives,
                    Message::ToggleShowHiddenSystemDrives,
                    palette,
                ),
                flow_column_width,
            ),
            settings_flow_item(sidebar_start, flow_column_width),
            settings_flow_item(
                self.settings_check_option(
                    self.localized("Mostrar Red", "Show Network"),
                    self.localized(
                        "Muestra el acceso a equipos y dispositivos de red",
                        "Shows access to network computers and devices",
                    ),
                    self.config.show_sidebar_network,
                    Message::ToggleShowSidebarNetwork,
                    palette,
                ),
                flow_column_width,
            ),
            settings_flow_item(
                self.settings_check_option(
                    self.localized("Mostrar Recientes", "Show Recent items"),
                    self.localized(
                        "Muestra las últimas ubicaciones visitadas",
                        "Shows recently visited locations",
                    ),
                    self.config.show_sidebar_recents,
                    Message::ToggleShowSidebarRecents,
                    palette,
                ),
                flow_column_width,
            ),
        ]
        .spacing(6)
        .padding([self.ui_vertical_padding(10.0), horizontal_padding])
        .height(Length::Fill)
        .wrap()
        .horizontal_spacing(column_spacing);

        let settings_body = scrollable(settings_flow)
            .horizontal()
            .height(Length::Fill)
            .style(move |theme, status| {
                explorer_scrollable_style(palette, theme, status, self.ui_density())
            });
        let panel = container(
            column![header, settings_body]
                .spacing(0)
                .height(Length::Fill),
        )
        .width(panel_width)
        .height(panel_height)
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

        let panel = self.frosted_popup_surface(
            self.popup_backdrop.as_ref(),
            panel.into(),
            panel_width,
            panel_height,
        );
        let modal = container(panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .center(Length::Fill)
            .style(move |_| {
                container::Style::default().background(Color::from_rgba8(
                    0,
                    0,
                    0,
                    modal_veil_alpha * palette.text.a,
                ))
            });

        if !color_picker_open {
            return modal.into();
        }

        let picker_x =
            ((self.window_size.width - panel_width) * 0.5 + self.ui_metric(136.0)).max(8.0);
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

    pub(in crate::iced_ui) fn settings_check_option(
        &self,
        label: &'static str,
        description: &'static str,
        enabled: bool,
        message: Message,
        palette: Palette,
    ) -> Element<'_, Message> {
        let check_size = self
            .ui_metric(19.0)
            .max(ui_text_line_height(self.font_size()));
        let check = container(
            text(if enabled { "✓" } else { "" })
                .size(self.font_size())
                .color(if enabled {
                    palette.accent_text
                } else {
                    palette.muted_text
                }),
        )
        .width(Length::Fixed(check_size))
        .height(Length::Fixed(check_size))
        .center(Length::Fixed(check_size))
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
                .padding([self.ui_vertical_padding(6.0), 9.0 + self.ui_density()])
                .width(Length::Fill)
                .on_press(message)
                .style(move |_, status| button_style(palette, false, status)),
        )
        .width(Length::Fill)
        .style(move |_| {
            container::Style::default()
                .background(mix_color(palette.input_bg, palette.header_bg, 0.42))
                .border(border::rounded(7).color(palette.border).width(1))
        })
        .into()
    }
}

fn settings_flow_item<'a>(
    content: impl Into<Element<'a, Message>>,
    width: f32,
) -> Element<'a, Message> {
    container(content).width(Length::Fixed(width)).into()
}
