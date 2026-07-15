use super::*;
use iced::widget::{column, row};

impl BExplorerIced {
    pub(in crate::iced_ui) fn view_selector_button(
        &self,
        pane: PaneId,
        palette: Palette,
    ) -> Element<'_, Message> {
        let mode = self.effective_view_mode(pane);
        let selected = self.view_menu_open == Some(pane);
        let color = if selected {
            palette.accent_text
        } else {
            palette.text
        };
        let affordance: Element<'_, Message> = inline_icon("chev-down", color, 12.0);
        let button = Button::new(
            row![
                inline_icon(view_mode_icon(mode), color, 15.0),
                text(self.localized(view_mode_label(mode), view_mode_label_english(mode)))
                    .size(self.font_size())
                    .color(color),
                affordance,
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([6, 10])
        .style(move |_, status| selected_button_style(palette, selected, status));

        button.on_press(Message::ToggleViewMenu(pane)).into()
    }

    pub(in crate::iced_ui) fn view_menu_overlay(
        &self,
        pane: PaneId,
        palette: Palette,
    ) -> Element<'_, Message> {
        let menu = container(
            column(
                view_menu_modes()
                    .into_iter()
                    .map(|mode| self.view_menu_item(pane, mode, palette))
                    .collect::<Vec<_>>(),
            )
            .spacing(3)
            .padding(6),
        )
        .width(218)
        .style(move |_| {
            container::Style::default()
                .background(palette.menu_bg)
                .border(border::rounded(6).color(palette.border).width(1))
                .shadow(iced::Shadow {
                    color: Color::from_rgba8(0, 0, 0, 0.22),
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
            self.frosted_popup_surface(self.popup_backdrop.as_ref(), menu.into(), 218.0, 219.0);
        let floating_menu: Element<'_, Message> = float(opaque(menu))
            .translate(|_, _| Vector::new(-14.0, -38.0))
            .into();

        let menu_layer = container(floating_menu)
            .align_right(Length::Fill)
            .align_bottom(Length::Fill)
            .into();

        stack(vec![backdrop.into(), menu_layer])
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    pub(in crate::iced_ui) fn view_menu_item(
        &self,
        pane: PaneId,
        mode: ViewMode,
        palette: Palette,
    ) -> Element<'_, Message> {
        let active = self.effective_view_mode(pane) == mode;
        let keyboard_selected = view_menu_modes()
            .iter()
            .position(|candidate| *candidate == mode)
            .is_some_and(|index| self.keyboard_menu_item_selected(KeyboardMenu::View(pane), index));
        let selected = if self.keyboard_menu_has_selection(KeyboardMenu::View(pane)) {
            keyboard_selected
        } else {
            active
        };
        let color = if selected {
            palette.accent_text
        } else {
            palette.text
        };
        Button::new(
            container(
                row![
                    inline_icon(view_mode_icon(mode), color, 16.0),
                    text(self.localized(view_mode_label(mode), view_mode_label_english(mode)))
                        .size(self.font_size())
                        .color(color)
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
        .on_press(Message::SetViewMode(pane, mode))
        .style(move |_, status| selected_button_style(palette, selected, status))
        .into()
    }

    pub(in crate::iced_ui) fn new_menu_overlay(
        &self,
        pane: PaneId,
        palette: Palette,
    ) -> Element<'_, Message> {
        let option = |index: usize, icon: &'static str, label: &'static str, message: Message| {
            let selected = self.keyboard_menu_item_selected(KeyboardMenu::New(pane), index);
            Button::new(
                container(
                    row![
                        inline_icon(
                            icon,
                            if selected {
                                palette.accent_text
                            } else {
                                palette.muted_text
                            },
                            17.0,
                        ),
                        text(label).size(self.font_size()).color(if selected {
                            palette.accent_text
                        } else {
                            palette.text
                        }),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                )
                .height(Length::Fill)
                .center_y(Length::Fill),
            )
            .width(Length::Fill)
            .height(32)
            .padding([0, 9])
            .on_press(message)
            .style(move |_, status| selected_button_style(palette, selected, status))
        };
        let menu = container(
            column![
                option(
                    0,
                    "folder",
                    self.localized("Nueva carpeta", "New folder"),
                    Message::NewFolder(pane),
                ),
                option(
                    1,
                    "file",
                    self.localized("Documento de texto", "Text document"),
                    Message::NewTextDocument(pane),
                ),
            ]
            .spacing(2)
            .padding(6),
        )
        .width(196)
        .style(move |_| {
            container::Style::default()
                .background(palette.menu_bg)
                .border(border::rounded(6).color(palette.border).width(1))
                .shadow(iced::Shadow {
                    color: Color::from_rgba8(0, 0, 0, 0.22),
                    offset: iced::Vector::new(0.0, 6.0),
                    blur_radius: 14.0,
                })
        });
        let menu =
            self.frosted_popup_surface(self.popup_backdrop.as_ref(), menu.into(), 196.0, 78.0);
        let backdrop = mouse_area(
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_press(Message::CloseFloatingMenus);
        let menu: Element<'_, Message> = float(opaque(menu))
            .translate(|_, _| Vector::new(12.0, 88.0))
            .into();
        stack(vec![backdrop.into(), menu])
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    pub(in crate::iced_ui) fn search_mode_menu_overlay(
        &self,
        pane: PaneId,
        palette: Palette,
    ) -> Element<'_, Message> {
        let option = |index: usize, label: &'static str, icon: &'static str, mode: SearchMode| {
            let active = self.pane(pane).search_mode == mode;
            let keyboard_selected =
                self.keyboard_menu_item_selected(KeyboardMenu::Search(pane), index);
            let selected = if self.keyboard_menu_has_selection(KeyboardMenu::Search(pane)) {
                keyboard_selected
            } else {
                active
            };
            Button::new(
                container(
                    row![
                        inline_icon(
                            icon,
                            if selected {
                                palette.accent_text
                            } else {
                                palette.muted_text
                            },
                            16.0,
                        ),
                        text(label).size(self.font_size()).color(if selected {
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
            .on_press(Message::SetSearchMode(pane, mode))
            .style(move |_, status| selected_button_style(palette, selected, status))
        };
        let menu = container(
            column![
                option(
                    0,
                    self.localized("Búsqueda rápida", "Quick search"),
                    "folder",
                    SearchMode::Quick
                ),
                option(
                    1,
                    self.localized("Búsqueda completa", "Full search"),
                    "folder-stack",
                    SearchMode::Complete
                ),
            ]
            .spacing(3)
            .padding(6),
        )
        .width(if self.split.is_some() { 210 } else { 260 })
        .style(move |_| {
            container::Style::default()
                .background(palette.menu_bg)
                .border(border::rounded(6).color(palette.border).width(1))
                .shadow(iced::Shadow {
                    color: Color::from_rgba8(0, 0, 0, 0.22),
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
        let menu_width = if self.split.is_some() { 210.0 } else { 260.0 };
        let menu =
            self.frosted_popup_surface(self.popup_backdrop.as_ref(), menu.into(), menu_width, 79.0);
        let floating_menu: Element<'_, Message> = float(opaque(menu))
            .translate(|_, _| Vector::new(14.0, -42.0))
            .into();
        let menu_layer = container(floating_menu)
            .align_left(Length::Fill)
            .align_bottom(Length::Fill)
            .into();

        stack(vec![backdrop.into(), menu_layer])
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    pub(in crate::iced_ui) fn group_menu_overlay(
        &self,
        pane: PaneId,
        palette: Palette,
    ) -> Element<'_, Message> {
        let items = column![
            self.group_mode_item(
                pane,
                0,
                GroupMode::None,
                self.localized("Ninguno", "None"),
                palette
            ),
            self.group_mode_item(
                pane,
                1,
                GroupMode::Type,
                self.localized("Tipo", "Type"),
                palette
            ),
            self.group_mode_item(
                pane,
                2,
                GroupMode::Name,
                self.localized("Nombre", "Name"),
                palette
            ),
            self.group_mode_item(
                pane,
                3,
                GroupMode::TotalSize,
                self.localized("Tamaño", "Size"),
                palette
            ),
            context_separator(palette),
            self.group_direction_item(
                pane,
                4,
                true,
                self.localized("Ascendente", "Ascending"),
                palette
            ),
            self.group_direction_item(
                pane,
                5,
                false,
                self.localized("Descendente", "Descending"),
                palette
            ),
        ];
        let menu = container(items.spacing(3).padding(6))
            .width(220)
            .style(move |_| {
                container::Style::default()
                    .background(palette.menu_bg)
                    .border(border::rounded(6).color(palette.border).width(1))
                    .shadow(iced::Shadow {
                        color: Color::from_rgba8(0, 0, 0, 0.22),
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
            self.frosted_popup_surface(self.popup_backdrop.as_ref(), menu.into(), 220.0, 223.0);
        let floating_menu: Element<'_, Message> = float(opaque(menu))
            .translate(|_, _| Vector::new(-104.0, 82.0))
            .into();

        let menu_layer = container(floating_menu)
            .align_right(Length::Fill)
            .align_top(Length::Fill)
            .into();

        stack(vec![backdrop.into(), menu_layer])
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    pub(in crate::iced_ui) fn group_mode_item(
        &self,
        pane: PaneId,
        index: usize,
        mode: GroupMode,
        label: &'static str,
        palette: Palette,
    ) -> Element<'_, Message> {
        let active = self.effective_group_mode(pane) == mode;
        let keyboard_selected = self.keyboard_menu_item_selected(KeyboardMenu::Group(pane), index);
        let selected = if self.keyboard_menu_has_selection(KeyboardMenu::Group(pane)) {
            keyboard_selected
        } else {
            active
        };
        menu_choice_button(
            label,
            active,
            selected,
            Message::SetGroupMode(pane, mode),
            palette,
            self.font_size(),
        )
    }

    pub(in crate::iced_ui) fn group_direction_item(
        &self,
        pane: PaneId,
        index: usize,
        ascending: bool,
        label: &'static str,
        palette: Palette,
    ) -> Element<'_, Message> {
        let active = self.effective_group_ascending(pane) == ascending;
        let keyboard_selected = self.keyboard_menu_item_selected(KeyboardMenu::Group(pane), index);
        let selected = if self.keyboard_menu_has_selection(KeyboardMenu::Group(pane)) {
            keyboard_selected
        } else {
            active
        };
        menu_choice_button(
            label,
            active,
            selected,
            Message::SetGroupAscending(pane, ascending),
            palette,
            self.font_size(),
        )
    }
}
