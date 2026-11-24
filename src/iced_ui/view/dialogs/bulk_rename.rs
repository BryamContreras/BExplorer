use super::*;
use crate::fs::bulk_rename::Issue;
use crate::iced_ui::bulk_rename::{BulkRenameMessage, Mode, PREVIEW_PAGE_SIZE, input_id};
use iced::widget::{checkbox, column, row};

impl BExplorerIced {
    pub(in crate::iced_ui) fn bulk_rename_modal(&self, palette: Palette) -> Element<'_, Message> {
        let Some(dialog) = &self.bulk_rename_dialog else {
            return Space::new().into();
        };
        let size = self.font_size();
        let action_button = |label, message: Option<BulkRenameMessage>, selected| {
            Button::new(text(label).size(size))
                .padding([6, 10])
                .on_press_maybe(message.map(Message::BulkRename))
                .style(move |_, status| dialog_button_style(palette, selected, status))
        };
        let can_apply = dialog.preview.can_apply() && !dialog.applying;
        let body: Element<'_, Message> = match &dialog.selection {
            None => text(self.localized("Preparando vista previa…", "Preparing preview…"))
                .size(size)
                .color(palette.muted_text)
                .into(),
            Some(Err(error)) => column![
                text(self.localized(
                    "Selecciona archivos o carpetas de una misma ubicación.",
                    "Select files or folders in the same location.",
                ))
                .size(size)
                .color(palette.text),
                text(error.as_str())
                    .size(size - 1.0)
                    .color(palette.muted_text),
            ]
            .spacing(6)
            .into(),
            Some(Ok(_)) => {
                let first_label = match dialog.mode {
                    Mode::Replace => self.localized("Buscar texto", "Find text"),
                    Mode::Number => self.localized("Nombre base", "Base name"),
                };
                let second_label = self.localized("Reemplazar por", "Replace with");
                let first = column![
                    text(first_label).size(size - 1.0).color(palette.muted_text),
                    text_input(first_label, dialog.first_input())
                        .id(input_id(false))
                        .on_input_maybe((!dialog.applying).then_some(|value| {
                            Message::BulkRename(BulkRenameMessage::FirstInput(value))
                        }))
                        .on_submit_maybe(
                            can_apply.then_some(Message::BulkRename(BulkRenameMessage::Submit))
                        )
                        .padding([6, 8])
                        .size(size),
                ]
                .width(Length::Fill)
                .spacing(4);
                let mut inputs = row![first].spacing(9);
                if dialog.mode == Mode::Replace {
                    inputs = inputs.push(
                        column![
                            text(second_label)
                                .size(size - 1.0)
                                .color(palette.muted_text),
                            text_input(second_label, &dialog.replacement)
                                .id(input_id(true))
                                .on_input_maybe((!dialog.applying).then_some(|value| {
                                    Message::BulkRename(BulkRenameMessage::SecondInput(value))
                                }))
                                .on_submit_maybe(
                                    can_apply
                                        .then_some(Message::BulkRename(BulkRenameMessage::Submit))
                                )
                                .padding([6, 8])
                                .size(size),
                        ]
                        .width(Length::Fill)
                        .spacing(4),
                    );
                }
                let start = dialog.page * PREVIEW_PAGE_SIZE;
                let end = (start + PREVIEW_PAGE_SIZE).min(dialog.preview.rows.len());
                let preview = column(dialog.preview.rows[start..end].iter().map(|item| {
                    let color = if item.issue.is_some() {
                        Color::from_rgb8(194, 89, 65)
                    } else {
                        palette.text
                    };
                    let mut proposed = column![
                        text(&item.after)
                            .size(size)
                            .color(color)
                            .wrapping(text::Wrapping::Glyph),
                    ]
                    .spacing(2)
                    .width(Length::FillPortion(1));
                    if let Some(issue) = item.issue {
                        let label = match issue {
                            Issue::Invalid => self.localized("Nombre inválido", "Invalid name"),
                            Issue::Duplicate => self.localized("Nombre repetido", "Duplicate name"),
                            Issue::Occupied => self.localized("Ya existe", "Already exists"),
                        };
                        proposed = proposed.push(text(label).size(size - 1.0).color(color));
                    }
                    row![
                        text(&item.before)
                            .size(size)
                            .color(palette.text)
                            .width(Length::FillPortion(1))
                            .wrapping(text::Wrapping::Glyph),
                        proposed,
                    ]
                    .spacing(12)
                    .into()
                }))
                .spacing(8)
                .width(Length::Fill)
                .padding([4, 8]);
                let estimate = if self.is_spanish() {
                    format!(
                        "{} cambios · {} conflictos",
                        dialog.preview.changes, dialog.preview.issues
                    )
                } else {
                    format!(
                        "{} changes · {} conflicts",
                        dialog.preview.changes, dialog.preview.issues
                    )
                };
                let side_font = size - 1.0;
                let mut details = column![
                    row![
                        action_button(
                            self.localized("Reemplazar texto", "Replace text"),
                            (!dialog.applying).then_some(BulkRenameMessage::SetMode(Mode::Replace)),
                            dialog.mode == Mode::Replace,
                        ),
                        action_button(
                            self.localized("Nombre numerado", "Numbered name"),
                            (!dialog.applying).then_some(BulkRenameMessage::SetMode(Mode::Number)),
                            dialog.mode == Mode::Number,
                        ),
                    ]
                    .spacing(8),
                    inputs,
                    checkbox(dialog.preserve_extension)
                        .label(self.localized("Conservar extensiones", "Keep extensions"))
                        .text_size(size)
                        .on_toggle_maybe((!dialog.applying).then_some(|_| {
                            Message::BulkRename(BulkRenameMessage::ToggleExtension)
                        })),
                    row![
                        text(self.localized("Nombre actual", "Current name"))
                            .size(side_font)
                            .color(palette.muted_text)
                            .width(Length::FillPortion(1)),
                        text(self.localized("Nombre nuevo", "New name"))
                            .size(side_font)
                            .color(palette.muted_text)
                            .width(Length::FillPortion(1)),
                    ]
                    .padding([0, 8])
                    .spacing(12),
                    container(
                        scrollable(preview)
                            .height((self.window_size.height * 0.29).clamp(100.0, 260.0))
                    )
                    .width(Length::Fill)
                    .style(move |_| container::Style::default()
                        .border(border::rounded(6).color(palette.border).width(1))),
                    text(estimate).size(side_font).color(palette.muted_text),
                ]
                .spacing(8);
                if dialog.page_count() > 1 {
                    let page_label = format!("{} / {}", dialog.page + 1, dialog.page_count());
                    details = details.push(
                        row![
                            action_button(
                                self.localized("Anterior", "Previous"),
                                (dialog.page > 0 && !dialog.applying)
                                    .then_some(BulkRenameMessage::PreviousPage),
                                false,
                            ),
                            text(page_label).size(side_font).color(palette.muted_text),
                            action_button(
                                self.localized("Siguiente", "Next"),
                                (dialog.page + 1 < dialog.page_count() && !dialog.applying)
                                    .then_some(BulkRenameMessage::NextPage),
                                false,
                            ),
                        ]
                        .spacing(8)
                        .align_y(Alignment::Center),
                    );
                }
                let keyboard_help = self.localized(
                    "Tab Campos · F3 Modo · F4 Extensiones · Enter Aplicar · Esc Cerrar",
                    "Tab Fields · F3 Mode · F4 Extensions · Enter Apply · Esc Close",
                );
                details
                    .push(
                        text(keyboard_help)
                            .size(side_font)
                            .color(palette.muted_text),
                    )
                    .into()
            }
        };

        let actions = row![
            Space::new().width(Length::Fill),
            action_button(
                self.localized("Cancelar", "Cancel"),
                (!dialog.applying).then_some(BulkRenameMessage::Cancel),
                false,
            ),
            action_button(
                if dialog.applying {
                    self.localized("Renombrando…", "Renaming…")
                } else {
                    self.localized("Aplicar", "Apply")
                },
                can_apply.then_some(BulkRenameMessage::Submit),
                can_apply,
            ),
        ]
        .spacing(8);

        let panel = column![
            text(self.localized("Renombrar en lote", "Bulk rename"))
                .size(size + 3.0)
                .color(palette.text),
            body,
            actions,
        ]
        .padding(18)
        .spacing(12);
        let surface = container(scrollable(panel))
            .width(self.modal_text_surface_width(620.0))
            .max_height((self.window_size.height - 24.0).max(100.0))
            .style(move |_| {
                container::Style::default()
                    .background(palette.menu_bg)
                    .border(border::rounded(8).color(palette.strong_border).width(1))
                    .shadow(iced::Shadow {
                        color: Color::from_rgba8(0, 0, 0, 0.24),
                        offset: iced::Vector::new(0.0, 10.0),
                        blur_radius: 24.0,
                    })
            });
        container(surface)
            .center(Length::Fill)
            .style(move |_| {
                container::Style::default().background(Color::from_rgba8(0, 0, 0, 0.24))
            })
            .into()
    }
}
