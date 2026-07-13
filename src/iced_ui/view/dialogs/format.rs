use super::*;
use iced::widget::{column, pick_list, row};

impl BExplorerIced {
    pub(in crate::iced_ui) fn format_dialog_modal(&self, palette: Palette) -> Element<'_, Message> {
        let Some(dialog) = &self.format_dialog else {
            return Space::new().into();
        };
        let font_size = self.font_size();
        let can_start = dialog.confirm_erase && !dialog.file_system.trim().is_empty();
        let allocation_options = vec![
            self.localized("Predeterminado", "Default").to_owned(),
            "512 bytes".to_owned(),
            "1024 bytes".to_owned(),
            "2048 bytes".to_owned(),
            "4096 bytes".to_owned(),
            "8192 bytes".to_owned(),
            "16 KB".to_owned(),
            "32 KB".to_owned(),
            "64 KB".to_owned(),
        ];
        let title = self.localized("Formatear unidad", "Format drive");
        let filesystem_options = dialog.file_systems.clone();
        let selected_filesystem = Some(dialog.file_system.clone());
        let selected_allocation = Some(dialog.allocation_unit_size.clone());
        let allocation_section: Element<'_, Message> = if cfg!(target_os = "windows") {
            column![
                text(self.localized("Tamaño de unidad de asignación", "Allocation unit size"))
                    .size(font_size - 1.0)
                    .color(palette.muted_text),
                pick_list(
                    allocation_options,
                    selected_allocation,
                    Message::SetFormatAllocationUnitSize,
                )
                .text_size(font_size)
                .padding([6, 8])
                .style(move |_, status| settings_pick_list_style(palette, status))
                .menu_style(move |_| settings_pick_list_menu_style(palette))
                .width(Length::Fill),
            ]
            .spacing(5)
            .into()
        } else {
            Space::new().into()
        };

        let checkbox = |checked: bool, message: Message, label: String| {
            Button::new(
                row![
                    container(text(if checked { "✓" } else { "" }).size(font_size))
                        .width(20)
                        .height(20)
                        .center(20)
                        .style(move |_| {
                            let mut style = container::Style::default()
                                .border(border::rounded(3).color(palette.strong_border).width(1));
                            if checked {
                                style = style.background(accent_gradient(palette));
                            }
                            style
                        }),
                    text(label).size(font_size).color(palette.text),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .padding([2, 0])
            .on_press(message)
            .style(move |_, _| button::Style {
                background: None,
                text_color: palette.text,
                border: border::Border::default(),
                ..button::Style::default()
            })
        };

        let panel = column![
            row![
                column![
                    text(title).size(font_size + 2.0).color(palette.text),
                    text(dialog.display_name.as_str())
                        .size(font_size - 1.0)
                        .color(palette.muted_text),
                ]
                .spacing(3)
                .width(Length::Fill),
                icon_button("x", Message::CancelFormatDialog, palette, false),
            ]
            .align_y(Alignment::Center),
            column![
                text(self.localized("Capacidad", "Capacity"))
                    .size(font_size - 1.0)
                    .color(palette.muted_text),
                text(format_size(dialog.capacity,))
                    .size(font_size)
                    .color(palette.text),
            ]
            .spacing(4),
            column![
                text(self.localized("Sistema de archivos", "File system"))
                    .size(font_size - 1.0)
                    .color(palette.muted_text),
                pick_list(
                    filesystem_options,
                    selected_filesystem,
                    Message::SetFormatFileSystem,
                )
                .text_size(font_size)
                .padding([6, 8])
                .style(move |_, status| settings_pick_list_style(palette, status))
                .menu_style(move |_| settings_pick_list_menu_style(palette))
                .width(Length::Fill),
            ]
            .spacing(5),
            allocation_section,
            column![
                text(self.localized("Etiqueta de volumen", "Volume label"))
                    .size(font_size - 1.0)
                    .color(palette.muted_text),
                text_input(
                    self.localized("Escribe una etiqueta", "Type a volume label"),
                    &dialog.volume_label,
                )
                .on_input(Message::FormatVolumeLabelChanged)
                .size(font_size)
                .padding([6, 8])
                .width(Length::Fill),
            ]
            .spacing(5),
            checkbox(
                dialog.quick_format,
                Message::ToggleFormatQuick,
                self.localized("Formato rápido", "Quick format").to_owned(),
            ),
            column![
                text(self.localized(
                    "Advertencia: todos los datos de esta unidad se eliminarán.",
                    "Warning: all data on this drive will be erased.",
                ))
                .size(font_size - 1.0)
                .color(Color::from_rgb8(221, 125, 87)),
                checkbox(
                    dialog.confirm_erase,
                    Message::ToggleFormatEraseConfirmation,
                    self.localized(
                        "Confirmo que quiero borrar todos los datos.",
                        "I understand that all data will be erased.",
                    )
                    .to_owned(),
                ),
            ]
            .spacing(4),
            row![
                Space::new().width(Length::Fill),
                Button::new(text(self.localized("Cancelar", "Cancel")).size(font_size))
                    .padding([7, 14])
                    .on_press(Message::CancelFormatDialog)
                    .style(move |_, status| dialog_button_style(palette, false, status)),
                Button::new(
                    text(self.localized("Iniciar", "Start"))
                        .size(font_size)
                        .color(if can_start {
                            palette.accent_text
                        } else {
                            palette.muted_text
                        }),
                )
                .padding([7, 14])
                .on_press_maybe(can_start.then_some(Message::ConfirmFormatDialog))
                .style(move |_, status| dialog_button_style(palette, can_start, status)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ]
        .spacing(13)
        .padding(18);

        let surface = container(panel).width(480).style(move |_| {
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
            self.frosted_popup_surface(self.popup_backdrop.as_ref(), surface.into(), 480.0, 560.0);
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
