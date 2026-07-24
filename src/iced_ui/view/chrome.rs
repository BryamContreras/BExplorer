use super::*;
use iced::widget::{column, row};

#[derive(Clone, Copy)]
struct TabVisualState {
    active: bool,
    focused_active: bool,
    dragging: bool,
    drag_offset: f32,
}

impl BExplorerIced {
    pub(super) fn title_tabs_area(&self, palette: Palette) -> Element<'_, Message> {
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

    pub(super) fn title_drag_gap(&self) -> Element<'_, Message> {
        mouse_area(
            container(Space::new())
                .height(TITLE_HEIGHT)
                .width(Length::Fill),
        )
        .on_press(Message::WindowDrag)
        .into()
    }

    pub(super) fn title_tabs(&self, pane: PaneId, palette: Palette) -> Element<'_, Message> {
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
                TabVisualState {
                    active,
                    focused_active,
                    dragging,
                    drag_offset,
                },
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
        visual: TabVisualState,
        palette: Palette,
    ) -> Element<'_, Message> {
        let TabVisualState {
            active,
            focused_active,
            dragging,
            drag_offset,
        } = visual;
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

    pub(super) fn sidebar(
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
        for section in self
            .config
            .normalized_sidebar_order()
            .into_iter()
            .filter(|section| self.sidebar_section_visible(*section))
        {
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

    pub(super) fn sidebar_section(
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

    pub(super) fn sidebar_item(
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
                .unwrap_or_else(|| inline_icon(item.icon, palette.accent, 16.0)),
            _ => inline_icon(item.icon, palette.accent, 16.0),
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
            .padding(Padding {
                top: 6.0,
                right: 14.0,
                bottom: 6.0,
                left: 26.0,
            })
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

    pub(super) fn sidebar_directory_icon_handle(&self, path: &Path) -> Option<iced_image::Handle> {
        let cache_key = sidebar_native_icon_cache_key(
            path,
            &self.sidebar_storage_entries,
            thumbnail_data::SMALL_ENTRY_IMAGE_SIZE,
        );
        match self.small_native_icon_cache.get(&cache_key) {
            Some(IcedImageState::Ready(handle)) => Some(handle.clone()),
            _ => None,
        }
    }

    pub(super) fn sidebar_resize_handle(&self, _palette: Palette) -> Element<'_, Message> {
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

    pub(super) fn window_resize_handles(&self) -> Element<'_, Message> {
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

    pub(super) fn window_resize_handle<W, H>(
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

    pub(super) fn content_area(&self, palette: Palette) -> Element<'_, Message> {
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

    pub(super) fn split_resize_handle(&self, palette: Palette) -> Element<'_, Message> {
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

    pub(super) fn address_bar(&self, pane: PaneId, palette: Palette) -> Element<'_, Message> {
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
