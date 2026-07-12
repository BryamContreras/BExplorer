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
        .style(move |_, status| button_style(palette, color_picker_open, status));

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
            .style(|_| container::Style::default().background(Color::from_rgba8(0, 0, 0, 0.42)));

        if !color_picker_open {
            return modal.into();
        }

        let picker_x = ((self.window_size.width - 470.0) * 0.5 + 136.0).max(8.0);
        let picker_y = ((self.window_size.height - 310.0) * 0.5 + 158.0)
            .min((self.window_size.height - 330.0).max(8.0))
            .max(8.0);
        let picker = float(opaque(self.color_picker_panel(palette)))
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

    pub(in crate::iced_ui) fn permanent_delete_modal(
        &self,
        palette: Palette,
    ) -> Element<'_, Message> {
        let count = self
            .permanent_delete_dialog
            .as_ref()
            .map(|pending| pending.paths.len())
            .unwrap_or(0);

        let panel = column![
            row![
                text(self.localized("Eliminar permanentemente", "Delete permanently"))
                    .size(self.font_size() + 2.0)
                    .color(palette.text)
                    .width(Length::Fill),
                icon_button("x", Message::CancelPermanentDelete, palette, false),
            ]
            .align_y(Alignment::Center),
            text(if self.is_spanish() {
                format!("¿Quieres eliminar permanentemente {count} elemento(s)?")
            } else {
                format!("Do you want to permanently delete {count} item(s)?")
            })
            .size(self.font_size())
            .color(palette.text),
            text(self.localized(
                "Esta acción no se puede deshacer.",
                "This action cannot be undone.",
            ))
            .size(self.font_size() - 1.0)
            .color(palette.muted_text),
            row![
                Space::new().width(Length::Fill),
                Button::new(
                    text(self.localized("Cancelar", "Cancel"))
                        .size(self.font_size())
                        .color(palette.text)
                )
                .padding([7, 14])
                .on_press(Message::CancelPermanentDelete)
                .style(move |_, status| button_style(palette, false, status)),
                Button::new(
                    text(self.localized("Eliminar", "Delete"))
                        .size(self.font_size())
                        .color(palette.accent_text)
                )
                .padding([7, 14])
                .on_press(Message::ConfirmPermanentDelete)
                .style(move |_, status| selected_button_style(palette, true, status)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ]
        .spacing(16)
        .padding(18);

        let surface = container(panel).width(420).style(move |_| {
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
            self.frosted_popup_surface(self.popup_backdrop.as_ref(), surface.into(), 420.0, 176.0);
        container(surface)
            .width(Length::Fill)
            .height(Length::Fill)
            .center(Length::Fill)
            .style(|_| container::Style::default().background(Color::from_rgba8(0, 0, 0, 0.24)))
            .into()
    }

    pub(in crate::iced_ui) fn transfer_conflict_modal(
        &self,
        palette: Palette,
    ) -> Element<'_, Message> {
        let Some(dialog) = &self.transfer_conflict_dialog else {
            return Space::new().into();
        };
        let count = dialog.conflicts.len();
        let first_name = dialog
            .conflicts
            .first()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| {
                self.localized("el elemento seleccionado", "the selected item")
                    .to_owned()
            });
        let conflict_message = if count == 1 && self.is_spanish() {
            format!("Ya existe \"{first_name}\" en esta ubicación.")
        } else if count == 1 {
            format!("\"{first_name}\" already exists in this location.")
        } else if self.is_spanish() {
            format!("{count} elementos ya existen en esta ubicación.")
        } else {
            format!("{count} items already exist in this location.")
        };

        let panel = column![
            row![
                text(self.localized("Conflicto de archivos", "File conflict"))
                    .size(self.font_size() + 2.0)
                    .color(palette.text)
                    .width(Length::Fill),
                icon_button("x", Message::CancelTransferConflict, palette, false),
            ]
            .align_y(Alignment::Center),
            text(conflict_message)
                .size(self.font_size())
                .color(palette.text),
            text(self.localized(
                "Elige cómo continuar con los elementos que tienen el mismo nombre.",
                "Choose how to continue with items that have the same name.",
            ))
            .size(self.font_size() - 1.0)
            .color(palette.muted_text),
            row![
                Space::new().width(Length::Fill),
                Button::new(
                    text(self.localized("Cancelar", "Cancel"))
                        .size(self.font_size())
                        .color(palette.text)
                )
                .padding([7, 12])
                .on_press(Message::CancelTransferConflict)
                .style(move |_, status| button_style(palette, false, status)),
                Button::new(
                    text(self.localized("Omitir", "Skip"))
                        .size(self.font_size())
                        .color(palette.text)
                )
                .padding([7, 12])
                .on_press(Message::ResolveTransferConflict(ConflictPolicy::Skip))
                .style(move |_, status| button_style(palette, false, status)),
                Button::new(
                    text(self.localized("Reemplazar", "Replace"))
                        .size(self.font_size())
                        .color(palette.text)
                )
                .padding([7, 12])
                .on_press(Message::ResolveTransferConflict(ConflictPolicy::Replace))
                .style(move |_, status| button_style(palette, false, status)),
                Button::new(
                    text(self.localized("Conservar ambos", "Keep both"))
                        .size(self.font_size())
                        .color(palette.accent_text)
                )
                .padding([7, 12])
                .on_press(Message::ResolveTransferConflict(ConflictPolicy::KeepBoth))
                .style(move |_, status| selected_button_style(palette, true, status)),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
        ]
        .spacing(16)
        .padding(18);

        let surface = container(panel).width(460).style(move |_| {
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
            self.frosted_popup_surface(self.popup_backdrop.as_ref(), surface.into(), 460.0, 238.0);
        container(surface)
            .width(Length::Fill)
            .height(Length::Fill)
            .center(Length::Fill)
            .style(|_| container::Style::default().background(Color::from_rgba8(0, 0, 0, 0.24)))
            .into()
    }

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
            .width(Length::Fill)
            .padding([7, 12])
            .on_press(Message::SetArchiveFormat(format))
            .style(move |_, status| selected_button_style(palette, dialog.format == format, status))
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
            .style(move |_, status| selected_button_style(palette, dialog.method == method, status))
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
        .style(move |_, status| button_style(palette, false, status));

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
                .style(move |_, status| button_style(palette, false, status)),
                Button::new(
                    text(self.localized("Comprimir", "Compress"))
                        .size(font_size)
                        .color(palette.accent_text)
                )
                .padding([7, 14])
                .on_press(Message::ConfirmArchiveDialog)
                .style(move |_, status| selected_button_style(palette, true, status)),
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
            .style(|_| container::Style::default().background(Color::from_rgba8(0, 0, 0, 0.24)))
            .into()
    }

    pub(in crate::iced_ui) fn transfer_item_card(
        &self,
        item: TransferDisplayState,
        palette: Palette,
    ) -> Element<'_, Message> {
        let progress = if item.total_bytes == 0 {
            0.0
        } else {
            (item.copied_bytes as f32 / item.total_bytes as f32).clamp(0.0, 1.0)
        };
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
        let details = if speed.is_empty() {
            format!("{files}  -  {size}")
        } else {
            format!("{files}  -  {size}  -  {speed}")
        };
        let state = self.localized_transfer_state(&item);
        let title = self.localized_transfer_title(&item);
        let id = item.id;
        let controls: Element<'_, Message> =
            if matches!(item.state, TransferState::Copying | TransferState::Paused) {
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

    pub(in crate::iced_ui) fn transfer_window_view(
        &self,
        palette: Palette,
    ) -> Element<'_, Message> {
        let items = self.transfer_items();
        let panel_height = progress_window_size_for_item_count(items.len()).height;
        let inner_height = (panel_height - WINDOW_BORDER_WIDTH * 2.0).max(0.0);
        let body_height = (inner_height - TRANSFER_WINDOW_TITLE_HEIGHT).max(0.0);
        let active_count = self.active_transfers.len();
        let queued_count = self.transfer_queue.len();
        let summary = if active_count == 1 && queued_count == 0 {
            self.localized("1 transferencia", "1 transfer").to_owned()
        } else if active_count == 0 && queued_count == 0 {
            self.localized("Sin transferencias activas", "No active transfers")
                .to_owned()
        } else {
            if self.is_spanish() {
                format!("{active_count} activas, {queued_count} en cola")
            } else {
                format!("{active_count} active, {queued_count} queued")
            }
        };
        let overall = self.transfer_progress_fraction().unwrap_or(0.0);

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
            row![title_drag_area, transfer_window_minimize_button(palette),]
                .align_y(Alignment::Center),
        )
        .height(TRANSFER_WINDOW_TITLE_HEIGHT)
        .width(Length::Fill)
        .style(move |_| {
            container::Style::default()
                .background(palette.overlay_title_bg)
                .border(border::rounded(border::top(WINDOW_RADIUS - 1.0)))
        });

        let header = row![
            text(self.localized("Transferencias", "Transfers"))
                .size(self.font_size() + 2.0)
                .color(palette.text)
                .width(Length::Fill),
            text(summary)
                .size(self.font_size())
                .color(palette.muted_text),
        ]
        .spacing(8)
        .height(TRANSFER_WINDOW_HEADER_HEIGHT)
        .align_y(Alignment::Center);

        let content: Element<'_, Message> = if items.is_empty() {
            column![
                container(header).width(Length::Fill).padding([
                    TRANSFER_WINDOW_HEADER_PADDING_Y,
                    TRANSFER_WINDOW_HEADER_PADDING_X,
                ]),
                container(
                    text(self.localized("No hay transferencias", "No transfers"))
                        .size(self.font_size())
                        .color(palette.muted_text)
                )
                .center(Length::Fill)
                .width(Length::Fill)
                .height(Length::Fill),
            ]
            .spacing(12)
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
                container(
                    column![
                        header,
                        transfer_progress_bar(overall, palette, TRANSFER_WINDOW_OVERALL_BAR_HEIGHT),
                    ]
                    .spacing(TRANSFER_WINDOW_CONTENT_GAP)
                )
                .width(Length::Fill)
                .padding([
                    TRANSFER_WINDOW_HEADER_PADDING_Y,
                    TRANSFER_WINDOW_HEADER_PADDING_X,
                ]),
                container(cards)
                    .width(Length::Fill)
                    .padding([0.0, TRANSFER_WINDOW_CARD_PADDING_X])
                    .height(Length::Fixed(visible_height)),
                Space::new().height(TRANSFER_WINDOW_CARD_BOTTOM_PADDING),
            ]
            .spacing(TRANSFER_WINDOW_CARD_TOP_GAP)
            .into()
        };

        let body = container(content)
            .width(Length::Fill)
            .height(Length::Fixed(body_height))
            .style(move |_| container::Style::default().background(palette.overlay_bg));

        let framed = column![title_bar, body]
            .width(Length::Fill)
            .height(Length::Fixed(inner_height));

        let inner_panel = container(framed)
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
        let panel_height = progress_window_size_for_item_count(items.len()).height;
        let inner_height = (panel_height - WINDOW_BORDER_WIDTH * 2.0).max(0.0);
        let body_height = (inner_height - TRANSFER_WINDOW_TITLE_HEIGHT).max(0.0);
        let active_count = self.active_archives.len();
        let summary = if active_count == 1 {
            self.localized("1 compresión activa", "1 active compression")
                .to_owned()
        } else if active_count == 0 {
            self.localized("Sin compresiones activas", "No active compressions")
                .to_owned()
        } else {
            if self.is_spanish() {
                format!("{active_count} compresiones activas")
            } else {
                format!("{active_count} active compressions")
            }
        };
        let overall = self.archive_progress_fraction().unwrap_or(0.0);

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
        let header = row![
            text(self.localized("Compresiones", "Compressions"))
                .size(self.font_size() + 2.0)
                .color(palette.text)
                .width(Length::Fill),
            text(summary)
                .size(self.font_size())
                .color(palette.muted_text),
        ]
        .spacing(8)
        .height(TRANSFER_WINDOW_HEADER_HEIGHT)
        .align_y(Alignment::Center);
        let content: Element<'_, Message> = if items.is_empty() {
            column![
                container(header).width(Length::Fill).padding([
                    TRANSFER_WINDOW_HEADER_PADDING_Y,
                    TRANSFER_WINDOW_HEADER_PADDING_X,
                ]),
                container(
                    text(self.localized("No hay compresiones", "No compressions"))
                        .size(self.font_size())
                        .color(palette.muted_text)
                )
                .center(Length::Fill)
                .width(Length::Fill)
                .height(Length::Fill),
            ]
            .spacing(12)
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
                container(
                    column![
                        header,
                        transfer_progress_bar(overall, palette, TRANSFER_WINDOW_OVERALL_BAR_HEIGHT),
                    ]
                    .spacing(TRANSFER_WINDOW_CONTENT_GAP)
                )
                .width(Length::Fill)
                .padding([
                    TRANSFER_WINDOW_HEADER_PADDING_Y,
                    TRANSFER_WINDOW_HEADER_PADDING_X,
                ]),
                container(cards)
                    .width(Length::Fill)
                    .padding([0.0, TRANSFER_WINDOW_CARD_PADDING_X])
                    .height(Length::Fixed(visible_height)),
                Space::new().height(TRANSFER_WINDOW_CARD_BOTTOM_PADDING),
            ]
            .spacing(TRANSFER_WINDOW_CARD_TOP_GAP)
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
                    .on_press(Message::CancelDefenderScan)
                    .style(move |_, status| button_style(palette, false, status)),
            ]
        } else {
            let remove = Button::new(text(self.localized("Eliminar amenazas", "Remove threats")))
                .on_press_maybe((threats > 0).then_some(Message::RemoveDefenderThreats))
                .style(move |_, status| button_style(palette, false, status));
            let exclude = Button::new(text(self.localized("Añadir exclusión", "Add exclusion")))
                .on_press_maybe(
                    summary
                        .is_some_and(|item| !item.paths.is_empty())
                        .then_some(Message::ExcludeDefenderPaths),
                )
                .style(move |_, status| button_style(palette, false, status));
            let security = Button::new(text(
                self.localized("Seguridad de Windows", "Windows Security"),
            ))
            .on_press(Message::OpenWindowsSecurity)
            .style(move |_, status| button_style(palette, false, status));
            row![
                remove,
                exclude,
                security,
                Space::new().width(Length::Fill),
                Button::new(text(self.localized("Cerrar", "Close")))
                    .on_press(Message::CloseDefenderPanel)
                    .style(move |_, status| selected_button_style(palette, true, status)),
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
