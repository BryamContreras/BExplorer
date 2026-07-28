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
        let affordance: Element<'_, Message> =
            inline_icon("chev-down", color, self.ui_metric(12.0));
        let button = Button::new(
            row![
                inline_icon(view_mode_icon(mode), color, self.ui_metric(15.0)),
                text(self.localized(view_mode_label(mode), view_mode_label_english(mode)))
                    .size(self.font_size())
                    .color(color),
                affordance,
            ]
            .spacing(self.ui_metric(6.0))
            .align_y(Alignment::Center),
        )
        .padding([self.ui_vertical_padding(6.0), self.ui_metric(10.0)])
        .style(move |_, status| selected_button_style(palette, selected, status));

        button.on_press(Message::ToggleViewMenu(pane)).into()
    }

    pub(in crate::iced_ui) fn view_menu_overlay(
        &self,
        pane: PaneId,
        palette: Palette,
    ) -> Element<'_, Message> {
        let menu_width = self.ui_metric(218.0);
        let item_spacing = self.ui_metric(3.0);
        let menu_padding = self.ui_metric(6.0);
        let menu_labels = view_menu_modes()
            .map(|mode| self.localized(view_mode_label(mode), view_mode_label_english(mode)));
        let menu_height = adaptive_menu_list_height(
            &menu_labels,
            self.font_size(),
            menu_width,
            item_spacing,
            menu_padding,
        );
        let menu = container(
            column(
                view_menu_modes()
                    .into_iter()
                    .map(|mode| self.view_menu_item(pane, mode, palette))
                    .collect::<Vec<_>>(),
            )
            .spacing(item_spacing)
            .padding(menu_padding),
        )
        .width(menu_width)
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
        let menu = self.frosted_popup_surface(
            self.popup_backdrop.as_ref(),
            menu.into(),
            menu_width,
            menu_height,
        );
        let menu_offset_y = -self.ui_metric(38.0);
        let menu_offset_x = -self.ui_metric(14.0);
        let floating_menu: Element<'_, Message> = float(opaque(menu))
            .translate(move |_, _| Vector::new(menu_offset_x, menu_offset_y))
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
        let label = self.localized(view_mode_label(mode), view_mode_label_english(mode));
        Button::new(
            container(
                row![
                    inline_icon(view_mode_icon(mode), color, self.ui_metric(16.0)),
                    text(label)
                        .size(self.font_size())
                        .color(color)
                        .width(Length::Fill),
                ]
                .spacing(self.ui_metric(8.0))
                .align_y(Alignment::Center),
            )
            .height(Length::Fill)
            .center_y(Length::Fill),
        )
        .width(Length::Fill)
        .height(adaptive_menu_item_height(
            label,
            self.font_size(),
            self.ui_metric(218.0),
        ))
        .padding([0.0, self.ui_metric(8.0)])
        .on_press(Message::SetViewMode(pane, mode))
        .style(move |_, status| selected_button_style(palette, selected, status))
        .into()
    }

    pub(in crate::iced_ui) fn new_menu_overlay(
        &self,
        pane: PaneId,
        palette: Palette,
    ) -> Element<'_, Message> {
        let menu_width = self.ui_metric(196.0);
        let item_spacing = self.ui_metric(2.0);
        let menu_padding = self.ui_metric(6.0);
        let folder_label = self.localized("Nueva carpeta", "New folder");
        let text_document_label = self.localized("Documento de texto", "Text document");
        let menu_height = adaptive_menu_list_height(
            &[folder_label, text_document_label],
            self.font_size(),
            menu_width,
            item_spacing,
            menu_padding,
        );
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
                            self.ui_metric(17.0),
                        ),
                        text(label)
                            .size(self.font_size())
                            .color(if selected {
                                palette.accent_text
                            } else {
                                palette.text
                            })
                            .width(Length::Fill),
                    ]
                    .spacing(self.ui_metric(10.0))
                    .align_y(Alignment::Center),
                )
                .height(Length::Fill)
                .center_y(Length::Fill),
            )
            .width(Length::Fill)
            .height(adaptive_menu_item_height(
                label,
                self.font_size(),
                menu_width,
            ))
            .padding([0.0, self.ui_metric(9.0)])
            .on_press(message)
            .style(move |_, status| selected_button_style(palette, selected, status))
        };
        let menu = container(
            column![
                option(0, "folder", folder_label, Message::NewFolder(pane),),
                option(
                    1,
                    "file",
                    text_document_label,
                    Message::NewTextDocument(pane),
                ),
            ]
            .spacing(item_spacing)
            .padding(menu_padding),
        )
        .width(menu_width)
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
        let menu = self.frosted_popup_surface(
            self.popup_backdrop.as_ref(),
            menu.into(),
            menu_width,
            menu_height,
        );
        let backdrop = mouse_area(
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_press(Message::CloseFloatingMenus);
        let menu_y = self.toolbar_height() + self.action_bar_height();
        let menu_x = self.ui_metric(12.0);
        let menu: Element<'_, Message> = float(opaque(menu))
            .translate(move |_, _| Vector::new(menu_x, menu_y))
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
        let menu_width = self.ui_metric(if self.split.is_some() { 210.0 } else { 260.0 });
        let item_spacing = self.ui_metric(3.0);
        let menu_padding = self.ui_metric(6.0);
        let quick_label = self.localized("Búsqueda rápida", "Quick search");
        let complete_label = self.localized("Búsqueda completa", "Full search");
        let menu_height = adaptive_menu_list_height(
            &[quick_label, complete_label],
            self.font_size(),
            menu_width,
            item_spacing,
            menu_padding,
        );
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
                            self.ui_metric(16.0),
                        ),
                        text(label)
                            .size(self.font_size())
                            .color(if selected {
                                palette.accent_text
                            } else {
                                palette.text
                            })
                            .width(Length::Fill),
                    ]
                    .spacing(self.ui_metric(6.0))
                    .align_y(Alignment::Center),
                )
                .height(Length::Fill)
                .center_y(Length::Fill),
            )
            .width(Length::Fill)
            .height(adaptive_menu_item_height(
                label,
                self.font_size(),
                menu_width,
            ))
            .padding([0.0, self.ui_metric(8.0)])
            .on_press(Message::SetSearchMode(pane, mode))
            .style(move |_, status| selected_button_style(palette, selected, status))
        };
        let menu = container(
            column![
                option(0, quick_label, "folder", SearchMode::Quick),
                option(1, complete_label, "folder-stack", SearchMode::Complete),
            ]
            .spacing(item_spacing)
            .padding(menu_padding),
        )
        .width(menu_width)
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
        let menu = self.frosted_popup_surface(
            self.popup_backdrop.as_ref(),
            menu.into(),
            menu_width,
            menu_height,
        );
        let menu_offset_y = -(self.status_bar_height() + self.ui_metric(6.0));
        let menu_offset_x = self.ui_metric(14.0);
        let floating_menu: Element<'_, Message> = float(opaque(menu))
            .translate(move |_, _| Vector::new(menu_offset_x, menu_offset_y))
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
        let none_label = self.localized("Ninguno", "None");
        let type_label = self.localized("Tipo", "Type");
        let name_label = self.localized("Nombre", "Name");
        let size_label = self.localized("Tamaño", "Size");
        let ascending_label = self.localized("Ascendente", "Ascending");
        let descending_label = self.localized("Descendente", "Descending");
        let items = column![
            self.group_mode_item(pane, 0, GroupMode::None, none_label, palette),
            self.group_mode_item(pane, 1, GroupMode::Type, type_label, palette),
            self.group_mode_item(pane, 2, GroupMode::Name, name_label, palette),
            self.group_mode_item(pane, 3, GroupMode::TotalSize, size_label, palette),
            context_separator(palette),
            self.group_direction_item(pane, 4, true, ascending_label, palette),
            self.group_direction_item(pane, 5, false, descending_label, palette),
        ];
        let menu_width = self.ui_metric(220.0);
        let item_spacing = self.ui_metric(3.0);
        let menu_padding = self.ui_metric(6.0);
        let menu_height = adaptive_menu_list_height(
            &[
                none_label,
                type_label,
                name_label,
                size_label,
                ascending_label,
                descending_label,
            ],
            self.font_size(),
            menu_width,
            item_spacing,
            menu_padding,
        ) + item_spacing
            + 1.0;
        let menu = container(items.spacing(item_spacing).padding(menu_padding))
            .width(menu_width)
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
        let menu = self.frosted_popup_surface(
            self.popup_backdrop.as_ref(),
            menu.into(),
            menu_width,
            menu_height,
        );
        let menu_y = self.ui_metric(82.0);
        let menu_x = -self.ui_metric(104.0);
        let floating_menu: Element<'_, Message> = float(opaque(menu))
            .translate(move |_, _| Vector::new(menu_x, menu_y))
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
