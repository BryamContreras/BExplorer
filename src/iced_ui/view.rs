use super::*;
mod dialogs;
mod files;
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
        } else {
            self.view()
        }
    }

    pub(super) fn window_title(&self, id: window::Id) -> String {
        if self.transfer_window_id == Some(id) {
            self.localized("Transferencias", "Transfers").to_owned()
        } else if self.archive_window_id == Some(id) {
            self.localized("Compresiones", "Compressions").to_owned()
        } else {
            "BExplorer".into()
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let palette = Palette::from_config(&self.config, self.is_dark_theme());
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

        let mut layers = vec![base.into(), self.window_resize_handles().into()];
        if self.title_menu_open {
            layers.push(self.title_menu_overlay(palette));
        }
        if self.context_menu.is_some() {
            layers.push(self.context_menu_overlay(palette));
        }
        if let Some(split_placement) = self.tab_split_placement_overlay(palette) {
            layers.push(split_placement);
        }
        if let Some(drag_overlay) = self.file_drag_overlay(palette) {
            layers.push(drag_overlay);
        }
        let mut modal_layers = vec![stack(layers).into()];
        if self.settings_open {
            modal_layers.push(opaque(self.settings_modal(palette)));
        }
        if self.shortcuts_open {
            modal_layers.push(opaque(self.shortcuts_modal(palette)));
        }
        if self.permanent_delete_dialog.is_some() {
            modal_layers.push(opaque(self.permanent_delete_modal(palette)));
        }
        if self.transfer_conflict_dialog.is_some() {
            modal_layers.push(opaque(self.transfer_conflict_modal(palette)));
        }
        if self.archive_dialog.is_some() {
            modal_layers.push(opaque(self.archive_dialog_modal(palette)));
        }
        if self.defender_visible() {
            modal_layers.push(opaque(self.defender_modal(palette)));
        }
        stack(modal_layers).into()
    }

    fn tab_split_placement_overlay(&self, palette: Palette) -> Option<Element<'_, Message>> {
        let dragged_tab = self.tab_drag.as_ref().and_then(|drag| {
            (self.split.is_none()
                && self.tabs.len() > 1
                && drag.dragging
                && self.is_tab_split_drop_position(self.cursor_position.y))
            .then_some(drag.tab_index)
        });
        let tab_index = dragged_tab?;
        let title = self
            .tabs
            .get(tab_index)
            .map(|tab| {
                if tab.path.is_none() {
                    self.localized("Este equipo", "This PC").to_owned()
                } else {
                    ellipsize_text(&tab.title, 22)
                }
            })
            .unwrap_or_else(|| self.localized("esta pestaña", "this tab").into());
        let suggested_side = self.tab_split_drop_side(self.cursor_position)?;

        let chooser = container(
            column![
                text(self.localized("Elegir ubicación", "Choose placement"))
                    .size((self.font_size() - 1.0).max(11.0))
                    .color(palette.muted_text),
                text(format!(
                    "{} {title}",
                    self.localized("Suelta para ubicar", "Drop to place")
                ))
                .size(self.font_size())
                .color(palette.text),
                row![
                    self.tab_split_placement_option(
                        SplitSide::Left,
                        suggested_side == SplitSide::Left,
                        palette,
                    ),
                    self.tab_split_placement_option(
                        SplitSide::Right,
                        suggested_side == SplitSide::Right,
                        palette,
                    ),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            ]
            .spacing(7)
            .align_x(Alignment::Center),
        )
        .width(284)
        .padding([10, 12])
        .style(move |_| elevated_panel_style(palette));
        let x = ((self.window_size.width - 284.0) * 0.5).max(8.0);
        let y = TITLE_HEIGHT + 8.0;
        let chooser: Element<'_, Message> = float(chooser)
            .translate(move |_, _| Vector::new(x, y))
            .into();
        Some(chooser)
    }

    fn tab_split_placement_option(
        &self,
        side: SplitSide,
        suggested: bool,
        palette: Palette,
    ) -> Element<'_, Message> {
        let active_left = side == SplitSide::Left;
        let pane = |active: bool| {
            container(Space::new())
                .width(48)
                .height(42)
                .style(move |_| {
                    let background: Background = if active {
                        translucent_accent_gradient(palette, 0.86).into()
                    } else {
                        palette.table_bg.into()
                    };
                    container::Style::default()
                        .background(background)
                        .border(border::rounded(2).color(palette.border).width(1))
                })
        };
        let preview = container(
            row![pane(active_left), pane(!active_left)]
                .spacing(2)
                .align_y(Alignment::Center),
        )
        .padding(5)
        .style(move |_| {
            container::Style::default()
                .background(palette.header_bg)
                .border(border::rounded(4).color(palette.border).width(1))
        });
        let label = match side {
            SplitSide::Left => self.localized("Izquierda", "Left"),
            SplitSide::Right => self.localized("Derecha", "Right"),
        };
        container(
            column![
                preview,
                text(label)
                    .size((self.font_size() - 1.0).max(11.0))
                    .color(if suggested {
                        palette.accent_text
                    } else {
                        palette.muted_text
                    }),
            ]
            .spacing(4)
            .align_x(Alignment::Center),
        )
        .width(122)
        .padding(5)
        .style(move |_| {
            let background: Background = if suggested {
                translucent_accent_gradient(palette, 0.9).into()
            } else {
                Color::TRANSPARENT.into()
            };
            container::Style::default().background(background).border(
                border::rounded(5)
                    .color(if suggested {
                        palette.accent
                    } else {
                        palette.border
                    })
                    .width(1),
            )
        })
        .into()
    }

    fn file_drag_overlay(&self, palette: Palette) -> Option<Element<'_, Message>> {
        let drag = self.file_drag.as_ref().filter(|drag| drag.dragging)?;
        let extracts_archive_entries = drag
            .sources
            .iter()
            .any(|path| crate::fs::archive_listing::is_inside_archive(path));
        let source_label = if drag.sources.len() == 1 {
            drag.sources
                .first()
                .and_then(|path| path.file_name())
                .and_then(|name| name.to_str())
                .map(|name| ellipsize_text(name, 26))
                .unwrap_or_else(|| self.localized("archivo", "file").into())
        } else {
            format!(
                "{} {}",
                drag.sources.len(),
                self.localized("elementos", "items")
            )
        };
        let destination = self.file_drag_destination_label(drag);
        let (message, hint) = if let Some(destination) = destination {
            if extracts_archive_entries {
                if drag.sources.len() == 1 {
                    (
                        format!(
                            "{} {destination}",
                            self.localized("Extraer a", "Extract to")
                        ),
                        format!(
                            "{source_label} · {}",
                            self.localized("Suelta para extraer", "Drop to extract")
                        ),
                    )
                } else {
                    (
                        format!(
                            "{} {source_label} {} {destination}",
                            self.localized("Extraer", "Extract"),
                            self.localized("a", "to")
                        ),
                        self.localized("Suelta para extraer", "Drop to extract")
                            .into(),
                    )
                }
            } else if drag.sources.len() == 1 {
                (
                    format!("{} {destination}", self.localized("Mover a", "Move to")),
                    format!(
                        "{source_label} · {}",
                        self.localized("Suelta para mover", "Drop to move")
                    ),
                )
            } else {
                (
                    format!(
                        "{} {source_label} {} {destination}",
                        self.localized("Mover", "Move"),
                        self.localized("a", "to")
                    ),
                    self.localized("Suelta para mover", "Drop to move").into(),
                )
            }
        } else if extracts_archive_entries {
            (
                format!("{} {source_label}", self.localized("Extraer", "Extract")),
                self.localized(
                    "Suelta en una carpeta para extraer",
                    "Drop on a folder to extract",
                )
                .into(),
            )
        } else {
            (
                format!("{} {source_label}", self.localized("Mover", "Move")),
                self.localized("Suelta sobre una carpeta", "Drop on a folder")
                    .into(),
            )
        };
        let message = ellipsize_to_width(&message, 236.0, self.font_size());
        let hint = ellipsize_to_width(&hint, 236.0, (self.font_size() - 1.0).max(10.0));
        let preview_stack = self.file_drag_preview_stack(drag, palette);
        let card = column![
            container(Space::new())
                .height(2)
                .width(Length::Fill)
                .style(move |_| container::Style::default().background(accent_gradient(palette))),
            container(
                row![
                    preview_stack,
                    column![
                        text(message)
                            .size(self.font_size())
                            .color(palette.text)
                            .wrapping(iced::widget::text::Wrapping::None),
                        text(hint)
                            .size((self.font_size() - 1.0).max(10.0))
                            .color(palette.muted_text),
                    ]
                    .spacing(2)
                    .width(Length::Fill),
                ]
                .spacing(9)
                .align_y(Alignment::Center)
                .width(Length::Fill),
            )
            .padding([8, 10])
            .clip(true),
        ]
        .spacing(0)
        .width(296);
        let card = container(card).clip(true).style(move |_| {
            container::Style::default()
                .background(palette.menu_bg)
                .border(
                    border::rounded(6)
                        .color(translucent_color(palette.accent, 0.78))
                        .width(1),
                )
                .shadow(iced::Shadow {
                    color: Color::from_rgba8(0, 0, 0, 0.34),
                    offset: Vector::new(0.0, 5.0),
                    blur_radius: 12.0,
                })
        });
        let x =
            (self.cursor_position.x + 18.0).clamp(8.0, (self.window_size.width - 306.0).max(8.0));
        let y =
            (self.cursor_position.y + 20.0).clamp(8.0, (self.window_size.height - 62.0).max(8.0));

        Some(float(card).translate(move |_, _| Vector::new(x, y)).into())
    }

    fn file_drag_preview_stack(
        &self,
        drag: &FileDragState,
        palette: Palette,
    ) -> Element<'_, Message> {
        const SINGLE_PREVIEW_SIZE: f32 = 34.0;
        const GROUP_PREVIEW_SIZE: f32 = 30.0;
        const PREVIEW_OFFSET: f32 = 4.0;
        const MAX_PREVIEWS: usize = 5;

        let entries = drag
            .sources
            .iter()
            .filter_map(|path| {
                self.pane(drag.source_pane)
                    .entries
                    .iter()
                    .find(|entry| entry.path == *path)
            })
            .take(MAX_PREVIEWS)
            .collect::<Vec<_>>();
        let count = entries.len().max(1);
        let preview_size = if count == 1 {
            SINGLE_PREVIEW_SIZE
        } else {
            GROUP_PREVIEW_SIZE
        };
        let stack_size = preview_size + PREVIEW_OFFSET * (count.saturating_sub(1)) as f32;
        let mut layers = Vec::with_capacity(count);

        for (index, entry) in entries.into_iter().enumerate() {
            let visual: Element<'_, Message> = if let Some(handle) = self.entry_image_handle(entry)
            {
                iced_image::Image::new(handle.clone())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .content_fit(ContentFit::Cover)
                    .into()
            } else {
                container(inline_icon(
                    fallback_icon_label(entry),
                    icon_color(fallback_icon_label(entry), palette, false),
                    20.0,
                ))
                .width(Length::Fill)
                .height(Length::Fill)
                .center(Length::Fill)
                .into()
            };
            let offset = (count - index - 1) as f32 * PREVIEW_OFFSET;
            let preview = container(visual)
                .width(preview_size)
                .height(preview_size)
                .clip(true)
                .style(move |_| {
                    container::Style::default()
                        .background(palette.table_bg)
                        .border(
                            border::rounded(4)
                                .color(translucent_color(palette.accent_text, 0.88))
                                .width(1.5),
                        )
                        .shadow(iced::Shadow {
                            color: Color::from_rgba8(0, 0, 0, 0.38),
                            offset: Vector::new(1.0, 1.0),
                            blur_radius: 3.0,
                        })
                });
            layers.push(
                container(preview)
                    .width(stack_size)
                    .height(stack_size)
                    .padding(Padding::new(0.0).top(offset).left(offset))
                    .into(),
            );
        }

        if layers.is_empty() {
            layers.push(
                container(inline_icon("file", palette.accent, 20.0))
                    .width(preview_size)
                    .height(preview_size)
                    .center(Length::Fill)
                    .into(),
            );
        }

        container(stack(layers).width(stack_size).height(stack_size))
            .width(stack_size)
            .height(stack_size)
            .into()
    }

    fn file_drag_destination_label(&self, drag: &FileDragState) -> Option<String> {
        if let Some((pane, index)) = drag.drop_target {
            return self
                .pane(pane)
                .entries
                .get(index)
                .map(|entry| ellipsize_text(&self.entry_display_name(entry), 24));
        }

        let (pane, point) = self.pane_pointer?;
        if self.entry_at_pane_point(pane, point).is_some() {
            return None;
        }
        self.tab_for_pane(pane).path.as_ref().map(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| ellipsize_text(name, 24))
                .unwrap_or_else(|| path.display().to_string())
        })
    }

    fn title_bar(&self, palette: Palette) -> Element<'_, Message> {
        let menu = icon_button("menu", Message::ToggleMenu, palette, self.title_menu_open);
        let sidebar = icon_button(
            "side",
            Message::ToggleSidebar,
            palette,
            self.sidebar_visible,
        );

        let tabs = self.title_tabs_area(palette);
        let pane_alignment_space = if self.sidebar_is_rendered() && !self.uses_split_sidebars() {
            (self.current_sidebar_width() - TITLE_BUTTON_WIDTH * 2.0 - TITLE_BUTTON_GAP).max(0.0)
        } else {
            TITLE_TAB_START_PADDING
        };

        let tab_band = row![
            menu,
            Space::new().width(TITLE_BUTTON_GAP),
            sidebar,
            Space::new().width(pane_alignment_space),
            tabs
        ]
        .align_y(Alignment::Center)
        .height(TITLE_HEIGHT)
        .width(Length::Fill);

        let controls = row![
            icon_button("split", Message::ToggleSplit, palette, self.split.is_some()),
            icon_button("min", Message::WindowMinimize, palette, false),
            icon_button(
                if self.window_maximized {
                    "restore"
                } else {
                    "max"
                },
                Message::WindowMaximize,
                palette,
                false,
            ),
            window_close_button(palette),
        ]
        .spacing(TITLE_BUTTON_GAP)
        .align_y(Alignment::Center);

        let controls_overlay = row![Space::new().width(Length::Fill), controls]
            .height(TITLE_HEIGHT)
            .width(Length::Fill)
            .align_y(Alignment::Center);

        let bar = stack(vec![tab_band.into(), controls_overlay.into()])
            .height(TITLE_HEIGHT)
            .width(Length::Fill);

        let title_radius = self.main_window_corner_radius();
        let base = container(bar)
            .height(TITLE_HEIGHT)
            .width(Length::Fill)
            .style(move |_| {
                container::Style::default()
                    .background(palette.title_bg)
                    .border(
                        border::rounded(border::top(title_radius))
                            .color(palette.border)
                            .width(1),
                    )
            });

        base.into()
    }

    fn title_menu_overlay(&self, palette: Palette) -> Element<'_, Message> {
        let show_menu_color = if self.show_menu_open {
            palette.accent_text
        } else {
            palette.text
        };
        let show_menu_icon_color = if self.show_menu_open {
            palette.accent_text
        } else {
            palette.muted_text
        };
        let show_menu_entry = mouse_area(
            Button::new(
                container(
                    row![
                        inline_icon("eye", show_menu_icon_color, 16.0),
                        text(self.localized("Mostrar", "Show"))
                            .size(self.font_size())
                            .color(show_menu_color)
                            .width(Length::Fill),
                        inline_icon("chev-right", show_menu_icon_color, 14.0),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                )
                .height(Length::Fill)
                .center_y(Length::Fill),
            )
            .width(Length::Fill)
            .height(32)
            .padding([0, 8])
            .on_press(Message::OpenShowMenu)
            .style(move |_, status| button_style(palette, self.show_menu_open, status)),
        )
        .on_enter(Message::ShowMenuParentEnter)
        .on_exit(Message::ShowMenuParentExit);
        let menu = container(
            column![
                Button::new(
                    container(
                        row![
                            inline_icon("keyboard", palette.muted_text, 16.0),
                            text(self.localized("Atajos", "Shortcuts"))
                                .size(self.font_size())
                                .color(palette.text)
                                .width(Length::Fill),
                        ]
                        .spacing(10)
                        .align_y(Alignment::Center),
                    )
                    .height(Length::Fill)
                    .center_y(Length::Fill),
                )
                .width(Length::Fill)
                .height(32)
                .padding([0, 8])
                .on_press(Message::OpenShortcuts)
                .style(move |_, status| button_style(palette, false, status)),
                show_menu_entry,
                Button::new(
                    container(
                        row![
                            inline_icon("settings", palette.muted_text, 16.0),
                            text(self.localized("Configuracion", "Settings"))
                                .size(self.font_size())
                                .color(palette.text)
                                .width(Length::Fill),
                        ]
                        .spacing(8)
                        .align_y(Alignment::Center),
                    )
                    .height(Length::Fill)
                    .center_y(Length::Fill),
                )
                .width(Length::Fill)
                .height(32)
                .padding([0, 8])
                .on_press(Message::ToggleSettings)
                .style(move |_, status| button_style(palette, false, status)),
            ]
            .spacing(3),
        )
        .padding(7)
        .width(220)
        .style(move |_| {
            container::Style::default()
                .background(palette.menu_bg)
                .border(border::rounded(4).color(palette.border).width(1))
                .shadow(iced::Shadow {
                    color: Color::from_rgba8(0, 0, 0, 0.34),
                    offset: iced::Vector::new(0.0, 6.0),
                    blur_radius: 14.0,
                })
        });
        let backdrop = mouse_area(
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_press(Message::CloseFloatingMenus);
        let menu =
            self.frosted_popup_surface(self.popup_backdrop.as_ref(), menu.into(), 220.0, 116.0);
        let floating_menu: Element<'_, Message> = float(opaque(menu))
            .translate(|_, _| Vector::new(0.0, TITLE_HEIGHT))
            .into();

        let mut layers = vec![backdrop.into(), floating_menu];
        if self.show_menu_open {
            let submenu = container(
                column![
                    self.show_menu_option(
                        self.localized("Barra de acciones", "Action bar"),
                        self.config.show_action_bar,
                        Message::ToggleActionBar,
                        palette,
                    ),
                    self.show_menu_option(
                        self.localized("Barra de marcadores", "Bookmarks bar"),
                        self.config.show_bookmark_bar,
                        Message::ToggleBookmarkBar,
                        palette,
                    ),
                    self.show_menu_option(
                        self.localized(
                            "Menu lateral en pantalla dividida",
                            "Sidebar in split view"
                        ),
                        self.config.show_split_pane_menus,
                        Message::ToggleSplitPaneMenus,
                        palette,
                    ),
                    self.show_menu_option(
                        self.localized(
                            "Panel de vista previa en pantalla dividida",
                            "Preview panel in split view",
                        ),
                        self.config.show_split_preview_panels,
                        Message::ToggleSplitPreviewPanels,
                        palette,
                    ),
                ]
                .spacing(3),
            )
            .padding(7)
            .width(286)
            .style(move |_| {
                container::Style::default()
                    .background(palette.menu_bg)
                    .border(border::rounded(4).color(palette.border).width(1))
                    .shadow(iced::Shadow {
                        color: Color::from_rgba8(0, 0, 0, 0.34),
                        offset: iced::Vector::new(0.0, 6.0),
                        blur_radius: 14.0,
                    })
            });
            let submenu = self.frosted_popup_surface(
                self.title_submenu_backdrop.as_ref(),
                submenu.into(),
                286.0,
                151.0,
            );
            let submenu = mouse_area(submenu)
                .on_enter(Message::ShowMenuSubmenuEnter)
                .on_exit(Message::ShowMenuSubmenuExit);
            layers.push(
                float(opaque(submenu))
                    .translate(|_, _| Vector::new(218.0, TITLE_HEIGHT + 41.0))
                    .into(),
            );
        }

        stack(layers)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn show_menu_option(
        &self,
        label: &'static str,
        enabled: bool,
        message: Message,
        palette: Palette,
    ) -> Element<'_, Message> {
        Button::new(
            container(
                row![
                    text(if enabled { "✓" } else { "" })
                        .size(self.font_size())
                        .color(palette.accent_text)
                        .width(18),
                    text(label).size(self.font_size()).color(if enabled {
                        palette.accent_text
                    } else {
                        palette.text
                    }),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .height(Length::Fill)
            .center_y(Length::Fill),
        )
        .width(Length::Fill)
        .height(32)
        .padding([0, 8])
        .on_press(message)
        .style(move |_, status| button_style(palette, enabled, status))
        .into()
    }

    fn context_menu_overlay(&self, palette: Palette) -> Element<'_, Message> {
        let Some(menu_state) = &self.context_menu else {
            return Space::new().into();
        };
        let (x, y) = self.context_menu_window_position(menu_state);
        let menu_height = self.context_menu_height(menu_state);
        let is_entry = matches!(menu_state.target, ContextTarget::Entry(_));
        let is_sidebar_drive = matches!(menu_state.target, ContextTarget::SidebarDrive(_));
        let extractable_archive = self
            .context_entry(menu_state.pane, menu_state.target)
            .is_some_and(|entry| {
                crate::fs::archive_listing::has_extractable_archive_extension(&entry.path)
            });
        let terminal_available = !is_sidebar_drive
            && (!is_entry
                || self
                    .context_entry(menu_state.pane, menu_state.target)
                    .is_some_and(|entry| {
                        entry.kind.is_container() && !explorer::is_virtual_path(&entry.path)
                    }));
        let context_entry = self.context_entry(menu_state.pane, menu_state.target);
        let mountable_disk_image = context_entry
            .as_ref()
            .is_some_and(is_mountable_disk_image_entry);
        let ejectable_drive = context_entry
            .as_ref()
            .and_then(|entry| entry.drive_kind)
            .is_some_and(DriveKind::is_ejectable);
        let defender_available = cfg!(target_os = "windows")
            && context_entry
                .as_ref()
                .is_some_and(|entry| !explorer::is_virtual_path(&entry.path));

        // On empty space, copying or cutting has no meaningful target. Keep
        // those familiar actions visible but disabled, and lead with Paste so
        // the useful action is immediately available.
        let quick_actions = if is_entry {
            row![
                context_quick_button(
                    "copy",
                    self.localized("Copiar", "Copy"),
                    ContextCommand::Copy,
                    palette,
                    true
                ),
                context_quick_button(
                    "cut",
                    self.localized("Cortar", "Cut"),
                    ContextCommand::Cut,
                    palette,
                    true
                ),
                context_quick_button(
                    "paste",
                    self.localized("Pegar", "Paste"),
                    ContextCommand::Paste,
                    palette,
                    menu_state.paste_available,
                ),
            ]
        } else {
            row![
                context_quick_button(
                    "paste",
                    self.localized("Pegar", "Paste"),
                    ContextCommand::Paste,
                    palette,
                    menu_state.paste_available,
                ),
                context_quick_button(
                    "copy",
                    self.localized("Copiar", "Copy"),
                    ContextCommand::Copy,
                    palette,
                    false
                ),
                context_quick_button(
                    "cut",
                    self.localized("Cortar", "Cut"),
                    ContextCommand::Cut,
                    palette,
                    false
                ),
            ]
        }
        .spacing(2)
        .padding([6, 0])
        .align_y(Alignment::Center)
        .width(Length::Fill);

        let mut items = if is_sidebar_drive {
            column![context_menu_row(
                "storage",
                self.localized("Expulsar", "Eject"),
                None,
                ContextCommand::EjectDrive,
                palette,
            )]
        } else {
            column![quick_actions, context_separator(palette)]
        }
        .spacing(2)
        .width(Length::Fill);

        if is_sidebar_drive {
            // The sidebar menu intentionally contains only actions that are
            // safe for the mounted volume itself.
        } else if is_entry {
            items = items
                .push(context_menu_row(
                    "open",
                    self.localized("Abrir", "Open"),
                    None,
                    ContextCommand::Open,
                    palette,
                ))
                .push(context_menu_row(
                    "open-with",
                    self.localized("Abrir con", "Open with"),
                    None,
                    ContextCommand::OpenWith,
                    palette,
                ))
                .push(context_separator(palette))
                .push(
                    mouse_area(context_menu_row(
                        "archive",
                        self.localized("Comprimir", "Compress"),
                        Some(ContextMenuTrailing::Icon("chev-right")),
                        ContextCommand::CompressMenu,
                        palette,
                    ))
                    .on_enter(Message::ContextArchiveParentEnter)
                    .on_exit(Message::ContextArchiveParentExit),
                );
            if extractable_archive {
                items = items.push(
                    mouse_area(context_menu_row(
                        "archive",
                        self.localized("Extraer", "Extract"),
                        Some(ContextMenuTrailing::Icon("chev-right")),
                        ContextCommand::ExtractMenu,
                        palette,
                    ))
                    .on_enter(Message::ContextExtractParentEnter)
                    .on_exit(Message::ContextArchiveParentExit),
                );
            }
            if mountable_disk_image {
                items = items.push(context_menu_row(
                    "storage",
                    self.localized("Montar imagen", "Mount image"),
                    None,
                    ContextCommand::MountDiskImage,
                    palette,
                ));
            }
            if ejectable_drive {
                items = items.push(context_menu_row(
                    "storage",
                    self.localized("Expulsar", "Eject"),
                    None,
                    ContextCommand::EjectDrive,
                    palette,
                ));
            }
            if defender_available {
                items = items.push(context_menu_row(
                    "properties",
                    self.localized(
                        "Analizar con Microsoft Defender",
                        "Scan with Microsoft Defender",
                    ),
                    None,
                    ContextCommand::ScanWithDefender,
                    palette,
                ));
            }
            items = items
                .push(context_separator(palette))
                .push(context_menu_row(
                    "rename",
                    self.localized("Renombrar", "Rename"),
                    None,
                    ContextCommand::Rename,
                    palette,
                ))
                .push(context_menu_row(
                    "trash",
                    self.localized("Eliminar", "Delete"),
                    None,
                    ContextCommand::Delete,
                    palette,
                ))
                .push(context_menu_row(
                    "delete-forever",
                    self.localized("Eliminar permanentemente", "Delete permanently"),
                    None,
                    ContextCommand::DeletePermanent,
                    palette,
                ))
                .push(context_separator(palette));
        } else {
            items = items
                .push(context_menu_row(
                    "refresh",
                    self.localized("Actualizar", "Refresh"),
                    None,
                    ContextCommand::Refresh,
                    palette,
                ))
                .push(
                    mouse_area(context_menu_row(
                        "add",
                        self.localized("Nuevo", "New"),
                        Some(ContextMenuTrailing::Icon("chev-right")),
                        ContextCommand::NewMenu,
                        palette,
                    ))
                    .on_enter(Message::ContextNewParentEnter)
                    .on_exit(Message::ContextNewParentExit),
                )
                .push(context_separator(palette));
        }

        if terminal_available {
            items = items.push(context_menu_row(
                "terminal",
                self.localized("Abrir en Terminal", "Open in Terminal"),
                None,
                ContextCommand::OpenTerminal,
                palette,
            ));
        }
        if !is_sidebar_drive {
            items = items.push(context_menu_row(
                "properties",
                self.localized("Propiedades", "Properties"),
                Some(ContextMenuTrailing::Text("Alt+Enter")),
                ContextCommand::Properties,
                palette,
            ));
        }

        let menu_content = container(items.padding([4, 6])).width(258).style(move |_| {
            container::Style::default()
                .background(palette.menu_bg)
                .border(border::rounded(7).color(palette.strong_border).width(1))
                .shadow(iced::Shadow {
                    color: Color::from_rgba8(0, 0, 0, 0.28),
                    offset: iced::Vector::new(0.0, 7.0),
                    blur_radius: 18.0,
                })
        });
        let menu = self.frosted_popup_surface(
            menu_state.backdrop.as_ref(),
            menu_content.into(),
            258.0,
            menu_height,
        );

        let backdrop = mouse_area(
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_press(Message::CloseContextMenu);

        let floating_menu: Element<'_, Message> = float(opaque(menu))
            .translate(move |_, _| Vector::new(x, y))
            .into();

        let mut overlay_layers = vec![backdrop.into(), floating_menu];
        if self.context_archive_submenu && is_entry {
            let (submenu_rows, submenu_labels): (Element<'_, Message>, Vec<String>) =
                if self.context_extract_submenu {
                    let extract_to_label = self
                        .context_entry(menu_state.pane, menu_state.target)
                        .and_then(|entry| {
                            archive::planned_extract_destination(
                                &entry.path,
                                ExtractMode::ToNamedFolder,
                            )
                            .ok()
                        })
                        .and_then(|path| {
                            path.file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                        })
                        // This submenu has enough room to retain a meaningful part of
                        // the destination name.  Keep the ellipsis only as the final
                        // fallback for unusually long archive names.
                        .map(|folder| {
                            format!(
                                "{} {}",
                                self.localized("Extraer en", "Extract to"),
                                ellipsize_text(&folder, 25),
                            )
                        })
                        .unwrap_or_else(|| {
                            self.localized("Extraer en carpeta", "Extract to folder")
                                .into()
                        });
                    let rows = column![
                        context_menu_row(
                            "archive",
                            self.localized("Extraer aquí", "Extract here"),
                            None,
                            ContextCommand::Extract(ExtractMode::Here),
                            palette,
                        ),
                        context_menu_dynamic_row(
                            "archive",
                            extract_to_label.clone(),
                            None,
                            ContextCommand::Extract(ExtractMode::ToNamedFolder),
                            palette,
                        ),
                    ]
                    .spacing(2)
                    .width(Length::Fill)
                    .padding([4, 6]);
                    (
                        rows.into(),
                        vec![
                            self.localized("Extraer aquí", "Extract here").into(),
                            extract_to_label,
                        ],
                    )
                } else {
                    let archive_name = self.default_archive_name(
                        menu_state.pane,
                        &self.context_paths(menu_state.pane, menu_state.target),
                    );
                    let seven_zip_label = context_archive_option_label(
                        self.localized("Comprimir", "Compress"),
                        &archive_name,
                        "7z",
                    );
                    let zip_label = context_archive_option_label(
                        self.localized("Comprimir", "Compress"),
                        &archive_name,
                        "zip",
                    );
                    let rows = column![
                        context_menu_row(
                            "archive",
                            self.localized("Comprimir", "Compress"),
                            None,
                            ContextCommand::CompressDialog,
                            palette,
                        ),
                        context_menu_dynamic_row(
                            "archive",
                            seven_zip_label.clone(),
                            None,
                            ContextCommand::CompressDefault(ArchiveFormat::SevenZip),
                            palette,
                        ),
                        context_menu_dynamic_row(
                            "archive",
                            zip_label.clone(),
                            None,
                            ContextCommand::CompressDefault(ArchiveFormat::Zip),
                            palette,
                        ),
                    ]
                    .spacing(2)
                    .width(Length::Fill)
                    .padding([4, 6]);
                    (
                        rows.into(),
                        vec![
                            self.localized("Comprimir", "Compress").into(),
                            seven_zip_label,
                            zip_label,
                        ],
                    )
                };
            let submenu_width = context_submenu_width(&submenu_labels);
            let submenu_height = if self.context_extract_submenu {
                78.0
            } else {
                114.0
            };
            let submenu_content = container(submenu_rows)
                .width(submenu_width)
                .style(move |_| {
                    container::Style::default()
                        .background(palette.menu_bg)
                        .border(border::rounded(7).color(palette.strong_border).width(1))
                        .shadow(iced::Shadow {
                            color: Color::from_rgba8(0, 0, 0, 0.28),
                            offset: iced::Vector::new(0.0, 7.0),
                            blur_radius: 18.0,
                        })
                });
            let submenu_kind = if self.context_extract_submenu {
                ContextSubmenuKind::Extract
            } else {
                ContextSubmenuKind::Archive
            };
            let submenu_backdrop = (menu_state.submenu_backdrop_kind == Some(submenu_kind))
                .then_some(menu_state.submenu_backdrop.as_ref())
                .flatten();
            let submenu = self.frosted_popup_surface(
                submenu_backdrop,
                submenu_content.into(),
                submenu_width,
                submenu_height,
            );
            let submenu_x = if x + 258.0 + submenu_width <= self.window_size.width - 8.0 {
                x + 252.0
            } else {
                (x - submenu_width + 6.0).max(8.0)
            };
            let submenu_offset_y = if self.context_extract_submenu {
                146.0
            } else {
                112.0
            };
            let submenu_y =
                (y + submenu_offset_y).clamp(8.0, (self.window_size.height - 120.0).max(8.0));
            let submenu = mouse_area(submenu)
                .on_enter(Message::ContextArchiveSubmenuEnter)
                .on_exit(Message::ContextArchiveSubmenuExit);
            overlay_layers.push(
                float(opaque(submenu))
                    .translate(move |_, _| Vector::new(submenu_x, submenu_y))
                    .into(),
            );
        } else if self.context_new_submenu && !is_entry {
            let labels = vec![
                self.localized("Nueva carpeta", "New folder").to_owned(),
                self.localized("Documento de texto", "Text document")
                    .to_owned(),
            ];
            let submenu_width = context_submenu_width(&labels);
            let submenu_height = 78.0;
            let submenu_content = container(
                column![
                    context_menu_row(
                        "folder",
                        self.localized("Nueva carpeta", "New folder"),
                        None,
                        ContextCommand::NewFolder,
                        palette,
                    ),
                    context_menu_row(
                        "file",
                        self.localized("Documento de texto", "Text document"),
                        None,
                        ContextCommand::NewTextDocument,
                        palette,
                    ),
                ]
                .spacing(2)
                .width(Length::Fill)
                .padding([4, 6]),
            )
            .width(submenu_width)
            .style(move |_| {
                container::Style::default()
                    .background(palette.menu_bg)
                    .border(border::rounded(7).color(palette.strong_border).width(1))
                    .shadow(iced::Shadow {
                        color: Color::from_rgba8(0, 0, 0, 0.28),
                        offset: iced::Vector::new(0.0, 7.0),
                        blur_radius: 18.0,
                    })
            });
            let submenu_x = if x + 258.0 + submenu_width <= self.window_size.width - 8.0 {
                x + 252.0
            } else {
                (x - submenu_width + 6.0).max(8.0)
            };
            let submenu_y = (y + 98.0).clamp(8.0, (self.window_size.height - 86.0).max(8.0));
            let submenu_backdrop = (menu_state.submenu_backdrop_kind
                == Some(ContextSubmenuKind::New))
            .then_some(menu_state.submenu_backdrop.as_ref())
            .flatten();
            let submenu = self.frosted_popup_surface(
                submenu_backdrop,
                submenu_content.into(),
                submenu_width,
                submenu_height,
            );
            let submenu = mouse_area(submenu)
                .on_enter(Message::ContextNewSubmenuEnter)
                .on_exit(Message::ContextNewSubmenuExit);
            overlay_layers.push(
                float(opaque(submenu))
                    .translate(move |_, _| Vector::new(submenu_x, submenu_y))
                    .into(),
            );
        }

        stack(overlay_layers)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn shortcuts_modal(&self, palette: Palette) -> Element<'_, Message> {
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
            .style(|_| container::Style::default().background(Color::from_rgba8(0, 0, 0, 0.24)))
            .into()
    }

    fn shortcut_editor_row(
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

    fn shortcut_action_label(&self, action: ShortcutAction) -> &'static str {
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

    fn shortcut_binding_label(&self, binding: &ShortcutBinding) -> String {
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

    fn frosted_popup_surface<'a>(
        &self,
        backdrop: Option<&iced_image::Handle>,
        foreground: Element<'a, Message>,
        width: f32,
        _height: f32,
    ) -> Element<'a, Message> {
        let Some(backdrop) = backdrop else {
            return foreground;
        };

        let fade = if self.color_picker_backdrop.as_ref() == Some(backdrop) {
            self.color_picker_fade_progress
        } else {
            self.popup_fade_progress
        }
        .clamp(0.0, 1.0);

        let backdrop = iced_image::Image::new(backdrop.clone())
            .width(Length::Fill)
            .height(Length::Fill)
            .border_radius(border::radius(7.0))
            .content_fit(ContentFit::Fill)
            .opacity(fade);
        // Iced has no general-purpose opacity wrapper for arbitrary widgets.
        // This neutral veil fades out over a few frames, revealing the whole
        // surface (including its text and controls) without relaying out it.
        let fade_veil = container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_| {
                container::Style::default().background(Color::from_rgba8(
                    15,
                    19,
                    21,
                    ((1.0 - fade) * 0.74).clamp(0.0, 0.74),
                ))
            });
        // Let the actual foreground surface determine the height. The former
        // fixed-height backdrop could outlive a shorter dialog/menu and show
        // as a blurred strip below it.
        stack(vec![foreground])
            .push_under(backdrop)
            .push(fade_veil)
            .width(Length::Fixed(width))
            .height(Length::Shrink)
            .into()
    }

    fn title_tabs_area(&self, palette: Palette) -> Element<'_, Message> {
        if let Some(split) = &self.split {
            let primary = ((split.ratio * 1000.0).round() as u16).clamp(1, 999);
            let secondary = 1000_u16.saturating_sub(primary).max(1);
            return row![
                container(row![
                    self.title_tabs(PaneId::Primary, palette),
                    self.title_drag_gap()
                ])
                .width(Length::FillPortion(primary))
                .height(TITLE_HEIGHT),
                Space::new().width(SPLIT_DIVIDER_WIDTH).height(TITLE_HEIGHT),
                container(row![
                    self.title_tabs(PaneId::Secondary, palette),
                    self.title_drag_gap()
                ])
                .width(Length::FillPortion(secondary))
                .height(TITLE_HEIGHT),
            ]
            .height(TITLE_HEIGHT)
            .width(Length::Fill)
            .into();
        }

        row![
            self.title_tabs(PaneId::Primary, palette),
            self.title_drag_gap()
        ]
        .height(TITLE_HEIGHT)
        .width(Length::Fill)
        .into()
    }

    fn title_drag_gap(&self) -> Element<'_, Message> {
        mouse_area(
            container(Space::new())
                .height(TITLE_HEIGHT)
                .width(Length::Fill),
        )
        .on_press(Message::WindowDrag)
        .into()
    }

    fn title_tabs(&self, pane: PaneId, palette: Palette) -> Element<'_, Message> {
        let mut tabs = row![].spacing(3).align_y(Alignment::Center);
        for (slot, tab_index) in self.tab_indices_for_pane(pane).into_iter().enumerate() {
            let Some(tab) = self.tabs.get(tab_index) else {
                continue;
            };
            let active = self.tab_index_for_pane(pane) == tab_index;
            // In split mode, the accent belongs to the tab in the pane that
            // currently owns focus. The other pane keeps its selected tab as
            // a quiet, neutral selection rather than looking active too.
            let focused_active =
                active && (self.split.is_none() || self.is_split_focused_pane(pane));
            let drag_offset = self
                .tab_drag
                .filter(|drag| drag.pane == pane && drag.tab_index == tab_index && drag.dragging)
                .map(|drag| drag.offset_x)
                .unwrap_or(0.0);
            let dragging = drag_offset.abs() > 0.1;
            let title = if tab.path.is_none() {
                self.localized("Este equipo", "This PC").to_owned()
            } else {
                tab.title.clone()
            };
            tabs = tabs.push(self.tab_button(
                pane,
                slot,
                title,
                active,
                focused_active,
                dragging,
                drag_offset,
                palette,
            ));
        }
        tabs = tabs.push(icon_button("add", Message::NewTab(pane), palette, false));
        container(
            column![Space::new().height(Length::Fill), tabs]
                .spacing(0)
                .height(TITLE_HEIGHT)
                .width(Length::Shrink),
        )
        .height(TITLE_HEIGHT)
        .width(Length::Shrink)
        .into()
    }

    fn tab_button(
        &self,
        pane: PaneId,
        slot: usize,
        title: String,
        active: bool,
        focused_active: bool,
        dragging: bool,
        drag_offset: f32,
        palette: Palette,
    ) -> Element<'_, Message> {
        let title_text = text(ellipsize_text(&title, 26))
            .size(self.font_size())
            .color(if active {
                palette.text
            } else {
                palette.muted_text
            });

        let leading = row![
            inline_icon("folder", palette.folder, TAB_ICON_SIZE),
            title_text,
        ]
        .spacing(5)
        .align_y(Alignment::Center)
        .width(Length::Shrink);

        let label = row![
            container(leading)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Horizontal::Left)
                .center_y(Length::Fill),
            Button::new(
                container(inline_icon("x", palette.muted_text, TAB_CLOSE_ICON_SIZE))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
            )
            .width(24)
            .height(TAB_HEIGHT - TAB_UNDERLINE_HEIGHT)
            .padding(0)
            .on_press(Message::CloseTab(pane, slot))
            .style(move |_, status| button_style(palette, false, status)),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .padding([0, 8])
        .spacing(0)
        .align_y(Alignment::Center);

        let tab_body = container(label)
            .padding(0)
            .width(Length::Fill)
            .height(TAB_HEIGHT - TAB_UNDERLINE_HEIGHT)
            .align_x(Horizontal::Left)
            .center_y(Length::Fill)
            .style(move |_| tab_body_style(palette, active, focused_active, dragging));

        let tab_press = mouse_area(tab_body).on_press(Message::StartTabDrag(pane, slot));

        let underline = container(Space::new())
            .width(Length::Fill)
            .height(TAB_UNDERLINE_HEIGHT)
            .style(move |_| {
                let style = if focused_active {
                    container::Style::default().background(accent_gradient(palette))
                } else {
                    container::Style::default().background(Color::TRANSPARENT)
                };
                style.border(border::rounded(1))
            });

        let tab = container(
            column![tab_press, underline]
                .spacing(0)
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .width(TAB_WIDTH)
        .height(TAB_HEIGHT)
        .clip(true);

        if dragging {
            float(tab)
                .translate(move |_, _| Vector::new(drag_offset, 0.0))
                .into()
        } else {
            tab.into()
        }
    }

    fn sidebar(
        &self,
        pane: PaneId,
        palette: Palette,
        _round_bottom_left: bool,
    ) -> Element<'_, Message> {
        if !self.sidebar_is_rendered() {
            return container(Space::new().height(Length::Fill)).width(0).into();
        }
        let sidebar_width = self.current_sidebar_width();

        let mut content = column![].padding([8, 8]).spacing(1);
        for section in self.config.normalized_sidebar_order() {
            content = content.push(self.sidebar_section(pane, section, palette));
        }

        let panel = container(scrollable(content))
            .width(sidebar_width)
            .height(Length::Fill)
            .clip(true)
            .style(move |_| container::Style::default().background(palette.sidebar_bg));

        mouse_area(
            stack(vec![
                panel.into(),
                row![
                    Space::new().width(Length::Fill),
                    container(Space::new())
                        .width(1)
                        .height(Length::Fill)
                        .style(move |_| container::Style::default().background(palette.border)),
                ]
                .height(Length::Fill)
                .width(Length::Fill)
                .into(),
                row![
                    Space::new().width(Length::Fill),
                    self.sidebar_resize_handle(palette)
                ]
                .height(Length::Fill)
                .width(Length::Fill)
                .into(),
            ])
            .width(sidebar_width)
            .height(Length::Fill),
        )
        .on_enter(Message::SidebarPointerEntered)
        .on_exit(Message::SidebarPointerExited)
        .into()
    }

    fn sidebar_section(
        &self,
        pane: PaneId,
        section: SidebarSection,
        palette: Palette,
    ) -> Element<'_, Message> {
        let expanded = self.sidebar_section_expanded(section);
        let dragging = self
            .sidebar_section_drag
            .is_some_and(|drag| drag.section == section && drag.dragging);
        let drag_offset = self
            .sidebar_section_drag
            .filter(|drag| drag.section == section)
            .map(|drag| drag.offset_y)
            .unwrap_or(0.0);

        if dragging {
            let mut section_content = column![Space::new().height(SIDEBAR_SECTION_HEIGHT)]
                .spacing(1)
                .width(Length::Fill);
            if expanded {
                for item in sidebar_items_for_section(
                    &self.config,
                    &self.sidebar_storage_entries,
                    section,
                    self.is_spanish(),
                ) {
                    section_content = section_content.push(self.sidebar_item(pane, item, palette));
                }
            }
            let header = sidebar_section_header(
                section,
                self.is_spanish(),
                expanded,
                true,
                drag_offset,
                palette,
                self.font_size(),
            );
            stack(vec![
                container(section_content).width(Length::Fill).into(),
                header,
            ])
            .width(Length::Fill)
            .into()
        } else {
            let mut section_content = column![sidebar_section_header(
                section,
                self.is_spanish(),
                expanded,
                false,
                0.0,
                palette,
                self.font_size(),
            )]
            .spacing(1)
            .width(Length::Fill);
            if expanded {
                for item in sidebar_items_for_section(
                    &self.config,
                    &self.sidebar_storage_entries,
                    section,
                    self.is_spanish(),
                ) {
                    section_content = section_content.push(self.sidebar_item(pane, item, palette));
                }
            }
            container(section_content).width(Length::Fill).into()
        }
    }

    fn sidebar_item(
        &self,
        pane: PaneId,
        item: SidebarItem,
        palette: Palette,
    ) -> Element<'_, Message> {
        let context_drive_index = item.context_drive_index;
        let active = match &item.target {
            SidebarTarget::Navigate(path) => self.tab_for_pane(pane).path == *path,
            SidebarTarget::Disabled => false,
        };
        let drop_target = matches!(
            &item.target,
            SidebarTarget::Navigate(Some(path)) if self.is_file_drag_sidebar_target(pane, path)
        );
        let highlighted = active || drop_target;
        let color = if highlighted {
            palette.accent_text
        } else if matches!(item.target, SidebarTarget::Disabled) {
            palette.muted_text
        } else {
            palette.text
        };
        let icon: Element<'static, Message> = match &item.target {
            SidebarTarget::Navigate(Some(path)) if !explorer::is_virtual_path(path) => self
                .sidebar_directory_icon_handle(path)
                .map(|handle| {
                    iced_image::Image::new(handle)
                        .width(18)
                        .height(18)
                        .content_fit(ContentFit::Contain)
                        .into()
                })
                .unwrap_or_else(|| inline_icon(item.icon, palette.accent, 18.0)),
            _ => inline_icon(item.icon, palette.accent, 18.0),
        };
        let label = if matches!(&item.target, SidebarTarget::Navigate(None)) {
            self.localized("Este equipo", "This PC").to_owned()
        } else {
            item.label.clone()
        };
        let row = row![
            icon,
            text(label)
                .size((self.font_size() - 0.5).max(11.0))
                .color(color)
                .wrapping(iced::widget::text::Wrapping::None)
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let button = Button::new(row)
            .padding([6, 14])
            .width(Length::Fill)
            .height(SIDEBAR_ITEM_HEIGHT)
            .style(move |_, status| selected_button_style(palette, highlighted, status));

        match item.target {
            SidebarTarget::Navigate(Some(path)) if !explorer::is_virtual_path(&path) => {
                let area = mouse_area(button.on_press(Message::Navigate(pane, Some(path.clone()))))
                    .on_enter(Message::FileDragSidebarTargetEnter(pane, path.clone()))
                    .on_exit(Message::FileDragSidebarTargetExit(path));
                if let Some(index) = context_drive_index {
                    area.on_right_press(Message::OpenSidebarDriveContext(pane, index))
                        .into()
                } else {
                    area.into()
                }
            }
            SidebarTarget::Navigate(path) => button.on_press(Message::Navigate(pane, path)).into(),
            SidebarTarget::Disabled => button.into(),
        }
    }

    fn sidebar_directory_icon_handle(&self, path: &Path) -> Option<iced_image::Handle> {
        let cache_key = thumbnail_data::native_path_icon_cache_key(
            path,
            true,
            thumbnail_data::NATIVE_ICON_SIZE,
        );
        match self.native_icon_cache.get(&cache_key) {
            Some(IcedImageState::Ready(handle)) => Some(handle.clone()),
            _ => None,
        }
    }

    fn sidebar_resize_handle(&self, _palette: Palette) -> Element<'_, Message> {
        mouse_area(
            container(Space::new())
                .width(SIDEBAR_RESIZE_HANDLE_WIDTH)
                .height(Length::Fill)
                .style(|_| container::Style::default().background(Color::TRANSPARENT)),
        )
        .on_press(Message::StartSidebarResize)
        .interaction(mouse::Interaction::ResizingHorizontally)
        .into()
    }

    fn window_resize_handles(&self) -> Element<'_, Message> {
        if self.window_maximized {
            return Space::new().width(Length::Fill).height(Length::Fill).into();
        }
        let edge = WINDOW_RESIZE_HANDLE_WIDTH;
        let corner = WINDOW_RESIZE_HANDLE_WIDTH * 1.8;
        column![
            row![
                self.window_resize_handle(corner, edge, window::Direction::NorthWest),
                self.window_resize_handle(Length::Fill, edge, window::Direction::North),
                self.window_resize_handle(corner, edge, window::Direction::NorthEast),
            ]
            .height(edge),
            row![
                self.window_resize_handle(edge, Length::Fill, window::Direction::West),
                Space::new().width(Length::Fill).height(Length::Fill),
                self.window_resize_handle(edge, Length::Fill, window::Direction::East),
            ]
            .height(Length::Fill),
            row![
                self.window_resize_handle(corner, edge, window::Direction::SouthWest),
                self.window_resize_handle(Length::Fill, edge, window::Direction::South),
                self.window_resize_handle(corner, edge, window::Direction::SouthEast),
            ]
            .height(edge),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn window_resize_handle<W, H>(
        &self,
        width: W,
        height: H,
        direction: window::Direction,
    ) -> Element<'_, Message>
    where
        W: Into<Length>,
        H: Into<Length>,
    {
        let interaction = match direction {
            window::Direction::East | window::Direction::West => {
                mouse::Interaction::ResizingHorizontally
            }
            window::Direction::North | window::Direction::South => {
                mouse::Interaction::ResizingVertically
            }
            window::Direction::NorthEast | window::Direction::SouthWest => {
                mouse::Interaction::ResizingDiagonallyUp
            }
            window::Direction::NorthWest | window::Direction::SouthEast => {
                mouse::Interaction::ResizingDiagonallyDown
            }
        };

        mouse_area(
            container(Space::new())
                .width(width)
                .height(height)
                .style(|_| container::Style::default().background(Color::TRANSPARENT)),
        )
        .on_press(Message::WindowResize(direction))
        .interaction(interaction)
        .into()
    }

    fn content_area(&self, palette: Palette) -> Element<'_, Message> {
        if let Some(split) = &self.split {
            let primary = ((split.ratio * 1000.0).round() as u16).clamp(1, 999);
            let secondary = 1000_u16.saturating_sub(primary).max(1);
            let primary_content: Element<'_, Message> = if self.uses_split_sidebars() {
                row![
                    self.sidebar(PaneId::Primary, palette, true),
                    self.file_pane(PaneId::Primary, palette, false, false),
                ]
                .height(Length::Fill)
                .into()
            } else {
                self.file_pane(PaneId::Primary, palette, true, false)
            };
            let secondary_content: Element<'_, Message> = if self.uses_split_sidebars() {
                row![
                    self.sidebar(PaneId::Secondary, palette, false),
                    self.file_pane(PaneId::Secondary, palette, false, true),
                ]
                .height(Length::Fill)
                .into()
            } else {
                self.file_pane(PaneId::Secondary, palette, false, true)
            };
            row![
                container(primary_content).width(Length::FillPortion(primary)),
                self.split_resize_handle(palette),
                container(secondary_content).width(Length::FillPortion(secondary)),
            ]
            .height(Length::Fill)
            .width(Length::Fill)
            .into()
        } else {
            self.file_pane(PaneId::Primary, palette, !self.sidebar_is_rendered(), true)
        }
    }

    fn split_resize_handle(&self, palette: Palette) -> Element<'_, Message> {
        mouse_area(
            container(Space::new())
                .width(SPLIT_DIVIDER_WIDTH)
                .height(Length::Fill)
                .style(move |_| container::Style::default().background(palette.border)),
        )
        .on_press(Message::StartSplitResize)
        .interaction(mouse::Interaction::ResizingHorizontally)
        .into()
    }

    fn address_bar(&self, pane: PaneId, palette: Palette) -> Element<'_, Message> {
        if let Some(address_edit) = self
            .address_edit
            .as_ref()
            .filter(|address_edit| address_edit.pane == pane)
        {
            let input = text_input("Escribe una ruta", &address_edit.value)
                .id(address_input_id(pane))
                .on_input(Message::AddressChanged)
                .on_submit(Message::SubmitAddress(pane))
                .size(self.font_size())
                .padding([5, 10])
                .width(Length::Fill)
                .style(move |_, status| {
                    let border_color =
                        if matches!(status, iced::widget::text_input::Status::Focused { .. }) {
                            palette.accent
                        } else {
                            palette.strong_border
                        };
                    iced::widget::text_input::Style {
                        background: chrome_glass_background(palette, palette.input_bg).into(),
                        border: border::rounded(5).color(border_color).width(1),
                        icon: palette.muted_text,
                        placeholder: palette.muted_text,
                        value: palette.text,
                        selection: translucent_color(palette.accent, 0.58),
                    }
                });
            return container(input)
                .width(Length::Fill)
                .height(30)
                .center_y(Length::Fill)
                .into();
        }

        let mut breadcrumbs = address_breadcrumbs(self.tab_for_pane(pane).path.as_ref());
        if let Some((label, _)) = breadcrumbs.first_mut() {
            *label = self.localized("Este equipo", "This PC").to_owned();
        }
        let navigation_secondary =
            if palette.table_bg.r + palette.table_bg.g + palette.table_bg.b > 2.1 {
                // Light backgrounds need a darker neutral so the intermediate
                // path segments do not wash out against the glass surface.
                mix_color(palette.muted_text, palette.text, 0.46)
            } else {
                // In dark mode retain the hierarchy while lifting breadcrumbs
                // enough that they do not look faded beside the active path.
                mix_color(palette.muted_text, Color::WHITE, 0.42)
            };
        let mut trail = row![].spacing(1).align_y(Alignment::Center);
        let last = breadcrumbs.len().saturating_sub(1);
        for (index, (label, target)) in breadcrumbs.into_iter().enumerate() {
            let active = index == last;
            let crumb = Button::new(
                text(label)
                    .size(self.font_size())
                    .color(if active {
                        palette.accent_text
                    } else {
                        navigation_secondary
                    })
                    .wrapping(iced::widget::text::Wrapping::None),
            )
            .padding([4, 6])
            .on_press(Message::Navigate(pane, target))
            .style(move |_, status| selected_button_style(palette, active, status));
            trail = trail.push(crumb);
            if index != last {
                trail = trail.push(inline_icon("chev-right", navigation_secondary, 13.0));
            }
        }

        mouse_area(
            container(trail)
                .width(Length::Fill)
                .height(30)
                .padding([0, 4])
                .center_y(Length::Fill)
                .clip(true)
                .style(move |_| {
                    container::Style::default()
                        .background(chrome_glass_background(palette, palette.input_bg))
                        .border(border::rounded(5).color(palette.strong_border).width(1))
                }),
        )
        .on_press(Message::BeginAddressEdit(pane))
        .into()
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
