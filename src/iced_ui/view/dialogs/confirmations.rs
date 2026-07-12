use super::*;
use iced::widget::{column, row};

impl BExplorerIced {
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
}
