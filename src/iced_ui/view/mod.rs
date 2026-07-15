use super::*;
mod chrome;
mod dialogs;
mod drag_overlays;
mod files;
mod menus;
mod shortcuts;
use iced::widget::{column, pick_list, row};

fn explorer_scrollable_style(
    palette: Palette,
    theme: &Theme,
    status: scrollable::Status,
    reveal_progress: f32,
) -> scrollable::Style {
    let mut style = scrollable::default(theme, status);
    let (horizontal_dragged, vertical_dragged) = match status {
        scrollable::Status::Dragged {
            is_horizontal_scrollbar_dragged,
            is_vertical_scrollbar_dragged,
            ..
        } => (
            is_horizontal_scrollbar_dragged,
            is_vertical_scrollbar_dragged,
        ),
        _ => (false, false),
    };
    let rail = |opacity: f32| scrollable::Rail {
        background: Some(
            translucent_color(
                mix_color(palette.table_bg, palette.sidebar_bg, 0.66),
                0.82 * opacity,
            )
            .into(),
        ),
        border: border::rounded(2)
            .color(translucent_color(palette.border, 0.72 * opacity))
            .width(1),
        scroller: scrollable::Scroller {
            background: translucent_accent_gradient(palette, 0.92 * opacity).into(),
            border: border::rounded(2)
                .color(translucent_color(palette.accent_text, 0.28 * opacity))
                .width(1),
        },
    };
    let reveal_progress = reveal_progress.clamp(0.0, 1.0);
    style.vertical_rail = rail(if vertical_dragged {
        1.0
    } else {
        reveal_progress
    });
    style.horizontal_rail = rail(if horizontal_dragged {
        1.0
    } else {
        reveal_progress
    });
    style.gap = Some(translucent_color(palette.table_bg, 0.9 * reveal_progress).into());
    style
}

fn settings_pick_list_style(
    palette: Palette,
    status: iced::widget::pick_list::Status,
) -> iced::widget::pick_list::Style {
    let hovered = matches!(
        status,
        iced::widget::pick_list::Status::Hovered
            | iced::widget::pick_list::Status::Opened { is_hovered: true }
    );
    iced::widget::pick_list::Style {
        text_color: palette.text,
        placeholder_color: palette.muted_text,
        handle_color: palette.muted_text,
        background: mix_color(
            palette.input_bg,
            if hovered {
                palette.hover
            } else {
                palette.menu_bg
            },
            if hovered { 0.28 } else { 0.14 },
        )
        .into(),
        border: border::rounded(4)
            .color(if hovered {
                palette.strong_border
            } else {
                palette.border
            })
            .width(1),
    }
}

fn settings_pick_list_menu_style(palette: Palette) -> iced::widget::overlay::menu::Style {
    iced::widget::overlay::menu::Style {
        background: palette.menu_bg.into(),
        border: border::rounded(5).color(palette.strong_border).width(1),
        text_color: palette.text,
        selected_text_color: palette.accent_text,
        selected_background: accent_gradient(palette).into(),
        shadow: iced::Shadow {
            color: Color::from_rgba8(0, 0, 0, 0.22),
            offset: iced::Vector::new(0.0, 6.0),
            blur_radius: 16.0,
        },
    }
}

impl BExplorerIced {
    pub(super) fn view_window(&self, id: window::Id) -> Element<'_, Message> {
        if self.transfer_window_id == Some(id) {
            self.transfer_window_view(Palette::from_config(&self.config, self.is_dark_theme()))
        } else if self.archive_window_id == Some(id) {
            self.archive_window_view(Palette::from_config(&self.config, self.is_dark_theme()))
        } else if self.defender_window_id == Some(id) {
            self.defender_window_view(Palette::from_config(&self.config, self.is_dark_theme()))
        } else if self.defender_threats_window_id == Some(id) {
            self.defender_threats_window_view(Palette::from_config(
                &self.config,
                self.is_dark_theme(),
            ))
        } else if cfg!(target_os = "linux") && self.is_properties_window(id) {
            #[cfg(target_os = "linux")]
            return self
                .properties_window_view(Palette::from_config(&self.config, self.is_dark_theme()));
            #[cfg(not(target_os = "linux"))]
            unreachable!();
        } else {
            self.view()
        }
    }

    pub(super) fn window_title(&self, id: window::Id) -> String {
        if self.transfer_window_id == Some(id) {
            self.localized("Transferencias", "Transfers").to_owned()
        } else if self.archive_window_id == Some(id) {
            self.localized("Compresiones", "Compressions").to_owned()
        } else if self.defender_window_id == Some(id) {
            self.localized("Microsoft Defender", "Microsoft Defender")
                .to_owned()
        } else if self.defender_threats_window_id == Some(id) {
            self.localized(
                "Amenazas de Microsoft Defender",
                "Microsoft Defender threats",
            )
            .to_owned()
        } else if cfg!(target_os = "linux") && self.is_properties_window(id) {
            #[cfg(target_os = "linux")]
            return self.properties_window_title();
            #[cfg(not(target_os = "linux"))]
            unreachable!();
        } else {
            "BExplorer".into()
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let palette = Palette::from_config(&self.config, self.is_dark_theme());
        let popup_palette = palette.with_opacity(self.popup_fade_progress);
        let window_radius = self.main_window_corner_radius();
        // With one shared sidebar in split mode, it controls whichever pane
        // currently owns focus. Once per-pane sidebars are enabled, each one
        // keeps its own explicit target below in `content_area`.
        let shared_sidebar_pane = if self.split.is_some() {
            self.focused_pane()
        } else {
            PaneId::Primary
        };
        let global_sidebar: Element<'_, Message> = if self.uses_split_sidebars() {
            Space::new().width(0).into()
        } else {
            self.sidebar(shared_sidebar_pane, palette, true)
        };
        let app = column![
            self.title_bar(palette),
            row![global_sidebar, self.content_area(palette),].height(Length::Fill)
        ]
        .width(Length::Fill)
        .height(Length::Fill);

        let framed_app = container(app)
            .width(Length::Fill)
            .height(Length::Fill)
            .clip(true)
            .style(move |_| {
                container::Style::default()
                    .background(palette.page_bg)
                    .border(border::rounded(
                        (window_radius - WINDOW_BORDER_WIDTH).max(0.0),
                    ))
            });

        let base = container(framed_app)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(WINDOW_BORDER_WIDTH)
            .clip(true)
            .style(move |_| {
                container::Style::default()
                    .background(Color::TRANSPARENT)
                    .border(
                        border::rounded(window_radius)
                            .color(window_border_color(palette))
                            .width(WINDOW_BORDER_WIDTH),
                    )
            });

        let mut layers = vec![base.into(), self.window_resize_handles()];
        if self.title_menu_open {
            layers.push(self.title_menu_overlay(popup_palette));
        }
        if self.context_menu.is_some() {
            layers.push(self.context_menu_overlay(popup_palette));
        }
        if let Some(split_placement) = self.tab_split_placement_overlay(palette) {
            layers.push(split_placement);
        }
        if let Some(drag_overlay) = self.file_drag_overlay(palette) {
            layers.push(drag_overlay);
        }
        let mut modal_layers = vec![stack(layers).into()];
        if self.settings_open {
            modal_layers.push(opaque(self.settings_modal(popup_palette)));
        }
        if self.shortcuts_open {
            modal_layers.push(opaque(self.shortcuts_modal(popup_palette)));
        }
        if self.about_open {
            modal_layers.push(opaque(self.about_modal(popup_palette)));
        }
        if self.permanent_delete_dialog.is_some() {
            modal_layers.push(opaque(self.permanent_delete_modal(popup_palette)));
        }
        if self.transfer_conflict_dialog.is_some() {
            modal_layers.push(opaque(self.transfer_conflict_modal(popup_palette)));
        }
        if self.elevated_transfer_dialog.is_some() {
            modal_layers.push(opaque(self.elevated_transfer_modal(palette)));
        }
        if self.elevated_delete_dialog.is_some() {
            modal_layers.push(opaque(self.elevated_delete_modal(palette)));
        }
        if self.elevated_file_action_dialog.is_some() {
            modal_layers.push(opaque(self.elevated_file_action_modal(palette)));
        }
        if self.archive_dialog.is_some() {
            modal_layers.push(opaque(self.archive_dialog_modal(popup_palette)));
        }
        if self.format_dialog.is_some() {
            modal_layers.push(opaque(self.format_dialog_modal(popup_palette)));
        }
        if self.error_dialog.is_some() {
            modal_layers.push(opaque(self.error_dialog_modal(popup_palette)));
        }
        let app: Element<'_, Message> = stack(modal_layers).into();
        if self.startup.show_busy_cursor() {
            let cursor_layer = mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
                .interaction(mouse::Interaction::Progress);
            stack(vec![app, cursor_layer.into()]).into()
        } else {
            app
        }
    }
}

pub(super) fn context_archive_option_label(action: &str, name: &str, extension: &str) -> String {
    const MAX_NAME_CHARS: usize = 23;
    let truncated = ellipsize_text(name, MAX_NAME_CHARS);
    format!("{action} {truncated}.{extension}")
}

pub(super) fn context_submenu_width(labels: &[String]) -> f32 {
    // Rows use a 20 px icon, 12 px gap and 10 px padding on each side. A
    // uniform character width made labels with many `i`, dots and hyphens
    // needlessly wide, so approximate the proportional UI font instead.
    let text_width = labels
        .iter()
        .map(|label| {
            label
                .chars()
                .map(|character| match character {
                    'i' | 'l' | 'I' | 'j' | '.' | ',' | ':' | ';' | '\'' | '|' => 0.28,
                    ' ' => 0.31,
                    'r' | 't' | 'f' | '-' | '_' => 0.40,
                    'm' | 'w' | 'M' | 'W' => 0.82,
                    'A'..='Z' => 0.65,
                    '0'..='9' => 0.52,
                    _ => 0.55,
                } * 13.0)
                .sum::<f32>()
        })
        .fold(0.0_f32, f32::max);
    (text_width + 68.0).ceil().clamp(204.0, 318.0)
}
