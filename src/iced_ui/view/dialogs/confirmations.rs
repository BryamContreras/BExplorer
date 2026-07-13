use super::*;
use iced::widget::{column, row};

impl BExplorerIced {
    pub(in crate::iced_ui) fn elevated_file_action_modal(
        &self,
        palette: Palette,
    ) -> Element<'_, Message> {
        let Some(pending) = &self.elevated_file_action_dialog else {
            return Space::new().into();
        };
        let permission = if cfg!(target_os = "linux") {
            self.localized("permisos de root", "root permission")
        } else {
            self.localized("permisos de administrador", "administrator permission")
        };
        let action = match &pending.action {
            operations::ElevatedFileAction::CreateFolder { .. } => {
                self.localized("crear la carpeta", "create the folder")
            }
            operations::ElevatedFileAction::CreateFile { .. } => {
                self.localized("crear el archivo", "create the file")
            }
            operations::ElevatedFileAction::Rename { .. } => {
                self.localized("renombrar el elemento", "rename the item")
            }
        };
        let authentication = if cfg!(target_os = "linux") {
            self.localized(
                "Polkit mostrarÃ¡ el campo de contraseÃ±a seguro del sistema.",
                "Polkit will show the system's secure password prompt.",
            )
        } else {
            self.localized(
                "Windows mostrarÃ¡ la confirmaciÃ³n UAC.",
                "Windows will show the UAC confirmation.",
            )
        };
        let panel = column![
            text(self.localized("Se requieren permisos", "Permission required"))
                .size(self.font_size() + 2.0)
                .color(palette.text),
            text(format!(
                "{} {action}: {permission}.",
                self.localized("Esta acciÃ³n requiere", "This action requires")
            ))
            .size(self.font_size())
            .color(palette.text),
            text(authentication)
                .size(self.font_size() - 1.0)
                .color(palette.muted_text),
            row![
                Space::new().width(Length::Fill),
                Button::new(text(self.localized("Cancelar", "Cancel")).size(self.font_size()))
                    .padding([7, 14])
                    .on_press(Message::CancelElevatedFileAction)
                    .style(move |_, status| dialog_button_style(palette, false, status)),
                Button::new(
                    text(self.localized("Conceder permisos", "Grant permission"))
                        .size(self.font_size())
                        .color(palette.accent_text),
                )
                .padding([7, 14])
                .on_press(Message::ConfirmElevatedFileAction)
                .style(move |_, status| dialog_button_style(palette, true, status)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ]
        .spacing(16)
        .padding(18);
        let surface = container(panel)
            .width(470)
            .style(move |_| elevated_panel_style(palette));
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

    pub(in crate::iced_ui) fn elevated_delete_modal(
        &self,
        palette: Palette,
    ) -> Element<'_, Message> {
        let Some(pending) = &self.elevated_delete_dialog else {
            return Space::new().into();
        };
        let permission = if cfg!(target_os = "linux") {
            self.localized("permisos de root", "root permission")
        } else {
            self.localized("permisos de administrador", "administrator permission")
        };
        let action = if pending.permanent {
            self.localized("eliminar permanentemente", "delete permanently")
        } else {
            self.localized("enviar a la papelera", "move to trash")
        };
        let authentication = if cfg!(target_os = "linux") {
            self.localized(
                "Polkit mostrará el campo de contraseña seguro del sistema.",
                "Polkit will show the system's secure password prompt.",
            )
        } else {
            self.localized(
                "Windows mostrará la confirmación UAC.",
                "Windows will show the UAC confirmation.",
            )
        };
        let panel = column![
            text(self.localized("Se requieren permisos", "Permission required"))
                .size(self.font_size() + 2.0)
                .color(palette.text),
            text(format!(
                "{} {action}: {permission}.",
                self.localized("No se pudo", "Could not")
            ))
            .size(self.font_size())
            .color(palette.text),
            text(authentication)
                .size(self.font_size() - 1.0)
                .color(palette.muted_text),
            row![
                Space::new().width(Length::Fill),
                Button::new(text(self.localized("Cancelar", "Cancel")).size(self.font_size()))
                    .padding([7, 14])
                    .on_press(Message::CancelElevatedDelete)
                    .style(move |_, status| dialog_button_style(palette, false, status)),
                Button::new(
                    text(self.localized("Conceder permisos", "Grant permission"))
                        .size(self.font_size())
                        .color(palette.accent_text),
                )
                .padding([7, 14])
                .on_press(Message::ConfirmElevatedDelete)
                .style(move |_, status| dialog_button_style(palette, true, status)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ]
        .spacing(16)
        .padding(18);
        let surface = container(panel)
            .width(470)
            .style(move |_| elevated_panel_style(palette));
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

    pub(in crate::iced_ui) fn elevated_transfer_modal(
        &self,
        palette: Palette,
    ) -> Element<'_, Message> {
        let Some(pending) = &self.elevated_transfer_dialog else {
            return Space::new().into();
        };
        let action = match pending.job.kind {
            TransferKind::Copy => self.localized("copiar", "copy"),
            TransferKind::Move => self.localized("mover", "move"),
        };
        let title = if cfg!(target_os = "linux") {
            self.localized("Permisos de root", "Root permission")
        } else {
            self.localized("Permisos de administrador", "Administrator permission")
        };
        let explanation = if cfg!(target_os = "linux") {
            self.localized(
                "Esta acción requiere permisos de root. Polkit mostrará el campo de contraseña seguro del sistema.",
                "This action requires root permission. Polkit will show the system's secure password prompt.",
            )
        } else {
            self.localized(
                "Esta acción requiere permisos de administrador. Windows mostrará la confirmación UAC.",
                "This action requires administrator permission. Windows will show the UAC confirmation.",
            )
        };
        let panel = column![
            text(title).size(self.font_size() + 2.0).color(palette.text),
            text(format!(
                "{} {} {} {}",
                self.localized("No tienes permisos para", "You do not have permission to"),
                action,
                self.localized("en", "to"),
                pending.job.destination.display()
            ))
            .size(self.font_size())
            .color(palette.text),
            text(explanation)
                .size(self.font_size() - 1.0)
                .color(palette.muted_text),
            row![
                Space::new().width(Length::Fill),
                Button::new(text(self.localized("Cancelar", "Cancel")).size(self.font_size()))
                    .padding([7, 14])
                    .on_press(Message::CancelElevatedTransfer)
                    .style(move |_, status| dialog_button_style(palette, false, status)),
                Button::new(
                    text(self.localized("Conceder permisos", "Grant permission"))
                        .size(self.font_size())
                        .color(palette.accent_text),
                )
                .padding([7, 14])
                .on_press(Message::ConfirmElevatedTransfer)
                .style(move |_, status| dialog_button_style(palette, true, status)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ]
        .spacing(16)
        .padding(18);

        let surface = container(panel)
            .width(480)
            .style(move |_| elevated_panel_style(palette));
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
                .style(move |_, status| dialog_button_style(palette, false, status)),
                Button::new(
                    text(self.localized("Eliminar", "Delete"))
                        .size(self.font_size())
                        .color(palette.accent_text)
                )
                .padding([7, 14])
                .on_press(Message::ConfirmPermanentDelete)
                .style(move |_, status| dialog_button_style(palette, true, status)),
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
                .style(move |_, status| dialog_button_style(palette, false, status)),
                Button::new(
                    text(self.localized("Omitir", "Skip"))
                        .size(self.font_size())
                        .color(palette.text)
                )
                .padding([7, 12])
                .on_press(Message::ResolveTransferConflict(ConflictPolicy::Skip))
                .style(move |_, status| dialog_button_style(palette, false, status)),
                Button::new(
                    text(self.localized("Reemplazar", "Replace"))
                        .size(self.font_size())
                        .color(palette.text)
                )
                .padding([7, 12])
                .on_press(Message::ResolveTransferConflict(ConflictPolicy::Replace))
                .style(move |_, status| dialog_button_style(palette, false, status)),
                Button::new(
                    text(self.localized("Conservar ambos", "Keep both"))
                        .size(self.font_size())
                        .color(palette.accent_text)
                )
                .padding([7, 12])
                .on_press(Message::ResolveTransferConflict(ConflictPolicy::KeepBoth))
                .style(move |_, status| dialog_button_style(palette, true, status)),
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
