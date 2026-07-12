use super::*;
use iced::widget::{column, row};

impl BExplorerIced {
    pub(super) fn shortcuts_modal(&self, palette: Palette) -> Element<'_, Message> {
        const ACTIONS: [ShortcutAction; 15] = [
            ShortcutAction::Copy,
            ShortcutAction::Cut,
            ShortcutAction::Paste,
            ShortcutAction::Undo,
            ShortcutAction::SelectAll,
            ShortcutAction::Refresh,
            ShortcutAction::Rename,
            ShortcutAction::Delete,
            ShortcutAction::PermanentDelete,
            ShortcutAction::Properties,
            ShortcutAction::GoUp,
            ShortcutAction::GoBack,
            ShortcutAction::GoForward,
            ShortcutAction::EditAddress,
            ShortcutAction::Open,
        ];

        let font_size = self.font_size();
        let split_at = ACTIONS.len().div_ceil(2);
        let mut left = column![].spacing(4).width(Length::Fill);
        let mut right = column![].spacing(4).width(Length::Fill);
        for (index, action) in ACTIONS.into_iter().enumerate() {
            let row = self.shortcut_editor_row(action, palette, font_size);
            if index < split_at {
                left = left.push(row);
            } else {
                right = right.push(row);
            }
        }
        let shortcuts = row![left, right]
            .spacing(8)
            .width(Length::Fill)
            .align_y(Alignment::Start);

        let panel = column![
            row![
                column![
                    text(self.localized("Atajos", "Shortcuts"))
                        .size(font_size + 3.0)
                        .color(palette.text),
                    text(self.localized(
                        "Haz clic en un atajo y pulsa la nueva combinación",
                        "Click a shortcut and press a new key combination",
                    ))
                    .size(font_size - 1.0)
                    .color(palette.muted_text),
                ]
                .spacing(3)
                .width(Length::Fill),
                icon_button("x", Message::CloseShortcuts, palette, false),
            ]
            .align_y(Alignment::Center),
            shortcuts,
            text(self.localized(
                "Usa el botón de flecha para restaurar el valor predeterminado.",
                "Use the arrow button to restore a default shortcut.",
            ))
            .size(font_size - 1.0)
            .color(palette.muted_text),
        ]
        .spacing(14)
        .padding(18);

        let surface = container(panel).width(740).style(move |_| {
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
            self.frosted_popup_surface(self.popup_backdrop.as_ref(), surface.into(), 740.0, 470.0);
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

    pub(super) fn shortcut_editor_row(
        &self,
        action: ShortcutAction,
        palette: Palette,
        font_size: f32,
    ) -> Element<'_, Message> {
        let capturing = self.shortcut_capture == Some(action);
        let binding = if capturing {
            self.localized("Pulsa una combinación", "Press a key combination")
                .to_owned()
        } else {
            self.shortcut_binding_label(self.config.shortcuts.binding(action))
        };
        let binding_color = if capturing {
            palette.accent_text
        } else {
            palette.text
        };
        let binding_button = Button::new(
            container(text(binding).size(font_size - 0.5).color(binding_color))
                .width(Length::Fill)
                .height(28)
                .center(Length::Fill),
        )
        .width(130)
        .height(30)
        .padding([0, 8])
        .on_press(Message::BeginShortcutCapture(action))
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(if capturing {
                    accent_gradient(palette).into()
                } else if hovered {
                    mix_color(palette.input_bg, palette.hover, 0.35).into()
                } else {
                    mix_color(palette.input_bg, palette.menu_bg, 0.18).into()
                }),
                text_color: binding_color,
                border: border::rounded(4)
                    .color(if capturing {
                        palette.accent
                    } else {
                        palette.strong_border
                    })
                    .width(1),
                ..button::Style::default()
            }
        });
        let reset = Button::new(
            container(inline_icon("undo", palette.muted_text, 14.0))
                .width(Length::Fill)
                .height(Length::Fill)
                .center(Length::Fill),
        )
        .width(30)
        .height(30)
        .padding(0)
        .on_press(Message::ResetShortcut(action))
        .style(move |_, status| button_style(palette, false, status));
        container(
            row![
                container(
                    text(self.shortcut_action_label(action))
                        .size(font_size)
                        .color(palette.text)
                        .wrapping(iced::widget::text::Wrapping::None),
                )
                .width(Length::Fill)
                .height(30)
                .center_y(30),
                container(binding_button)
                    .width(130)
                    .height(Length::Fill)
                    .center_y(Length::Fill),
                container(reset)
                    .width(30)
                    .height(Length::Fill)
                    .center_y(Length::Fill),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
        )
        .height(40)
        .padding([0, 9])
        .clip(true)
        .style(move |_| {
            container::Style::default()
                .background(mix_color(palette.menu_bg, palette.title_bg, 0.22))
                .border(border::rounded(5).color(palette.border).width(1))
        })
        .into()
    }

    pub(super) fn shortcut_action_label(&self, action: ShortcutAction) -> &'static str {
        match action {
            ShortcutAction::Copy => self.localized("Copiar", "Copy"),
            ShortcutAction::Cut => self.localized("Cortar", "Cut"),
            ShortcutAction::Paste => self.localized("Pegar", "Paste"),
            ShortcutAction::Undo => self.localized("Deshacer", "Undo"),
            ShortcutAction::SelectAll => self.localized("Seleccionar todo", "Select all"),
            ShortcutAction::Refresh => self.localized("Actualizar", "Refresh"),
            ShortcutAction::Rename => self.localized("Renombrar", "Rename"),
            ShortcutAction::Delete => self.localized("Enviar a la papelera", "Move to trash"),
            ShortcutAction::PermanentDelete => {
                self.localized("Eliminado Permanente", "Permanent deletion")
            }
            ShortcutAction::Properties => self.localized("Propiedades", "Properties"),
            ShortcutAction::GoUp => self.localized("Subir una carpeta", "Go up"),
            ShortcutAction::GoBack => self.localized("Atrás", "Back"),
            ShortcutAction::GoForward => self.localized("Adelante", "Forward"),
            ShortcutAction::EditAddress => self.localized("Editar dirección", "Edit address"),
            ShortcutAction::Open => self.localized("Abrir", "Open"),
            ShortcutAction::CommandPalette | ShortcutAction::MoveUp | ShortcutAction::MoveDown => {
                ""
            }
        }
    }

    pub(super) fn shortcut_binding_label(&self, binding: &ShortcutBinding) -> String {
        let mut keys = Vec::new();
        if binding.ctrl {
            keys.push("Ctrl".to_owned());
        }
        if binding.alt {
            keys.push("Alt".to_owned());
        }
        if binding.shift {
            keys.push("Shift".to_owned());
        }
        if !binding.key.is_empty() {
            keys.push(binding.key.clone());
        }
        keys.join(" + ")
    }

    pub(super) fn frosted_popup_surface<'a>(
        &self,
        backdrop: Option<&iced_image::Handle>,
        foreground: Element<'a, Message>,
        width: f32,
        _height: f32,
    ) -> Element<'a, Message> {
        let Some(backdrop) = backdrop else {
            return foreground;
        };

        let (fade, target) = if self.color_picker_backdrop.as_ref() == Some(backdrop) {
            (
                self.color_picker_fade_progress,
                self.color_picker_fade_target,
            )
        } else {
            (self.popup_fade_progress, self.popup_fade_target)
        };
        let fade = fade.clamp(0.0, 1.0);
        let backdrop_opacity = popup_backdrop_opacity(fade, target);

        let backdrop = iced_image::Image::new(backdrop.clone())
            .width(Length::Fill)
            .height(Length::Fill)
            .border_radius(border::radius(7.0))
            .content_fit(ContentFit::Fill)
            .opacity(backdrop_opacity);
        // Let the actual foreground surface determine the height. The former
        // fixed-height backdrop could outlive a shorter dialog/menu and show
        // as a blurred strip below it.
        stack(vec![foreground])
            .push_under(backdrop)
            .width(Length::Fixed(width))
            .height(Length::Shrink)
            .into()
    }
}
