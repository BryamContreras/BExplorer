use super::*;

use crate::fs::properties::{PropertyApplication, PropertyIdentity, PropertyKind};
use crate::iced_ui::properties::{
    PropertiesIdentityMenu, PropertiesMessage, PropertiesSelectorMenu, PropertiesTab,
    PropertiesWindowState, properties_selector_scroll_id,
};
use iced::alignment::Vertical;
use iced::widget::{column, pin, row};

const PROPERTIES_ICON_SIZE: f32 = 44.0;
const PROPERTIES_TAB_HEIGHT: f32 = 34.0;
const PROPERTIES_TAB_TOP_MARGIN: f32 = 7.0;
const PROPERTIES_TAB_REGION_HEIGHT: f32 = PROPERTIES_TAB_HEIGHT + PROPERTIES_TAB_TOP_MARGIN;
const PROPERTIES_FOOTER_HEIGHT: f32 = 56.0;
const PROPERTY_LABEL_WIDTH: f32 = 112.0;
const PROPERTY_TEXT_SPACING: f32 = 12.0;
const PERMISSION_COLUMN_WIDTH: f32 = 100.0;
const APPLICATION_SELECTOR_WIDTH: f32 = 216.0;
const APPLICATION_SELECTOR_HEIGHT: f32 = 28.0;
const IDENTITY_SELECTOR_WIDTH: f32 = 308.0;
const IDENTITY_SELECTOR_HEIGHT: f32 = 30.0;
const PROPERTIES_OUTER_VERTICAL_PADDING: f32 = 7.0;
const PROPERTIES_OUTER_HORIZONTAL_PADDING: f32 = 10.0;
const PROPERTIES_CARD_VERTICAL_PADDING: f32 = 10.0;
const PROPERTIES_CARD_HORIZONTAL_PADDING: f32 = 12.0;
const PROPERTIES_SECTION_SPACING: f32 = 10.0;
const PROPERTIES_ITEM_SPACING: f32 = 10.0;
const GENERAL_TYPE_ROW_HEIGHT: f32 = 18.0;
const APPLICATION_MENU_GAP: f32 = 2.0;
const IDENTITY_MENU_X: f32 = APPLICATION_MENU_X;
const OWNER_MENU_Y: f32 = PROPERTIES_OUTER_VERTICAL_PADDING
    + PROPERTIES_CARD_VERTICAL_PADDING
    + IDENTITY_SELECTOR_HEIGHT
    + APPLICATION_MENU_GAP;
const GROUP_MENU_Y: f32 = OWNER_MENU_Y + IDENTITY_SELECTOR_HEIGHT + PROPERTIES_ITEM_SPACING;
const APPLICATION_MENU_X: f32 = PROPERTIES_OUTER_HORIZONTAL_PADDING
    + PROPERTIES_CARD_HORIZONTAL_PADDING
    + PROPERTY_LABEL_WIDTH
    + PROPERTY_TEXT_SPACING;
const APPLICATION_MENU_Y: f32 = PROPERTIES_OUTER_VERTICAL_PADDING
    + PROPERTIES_CARD_VERTICAL_PADDING
    + PROPERTIES_ICON_SIZE
    + PROPERTIES_SECTION_SPACING
    + 1.0
    + PROPERTIES_SECTION_SPACING
    + GENERAL_TYPE_ROW_HEIGHT
    + PROPERTIES_ITEM_SPACING
    + APPLICATION_SELECTOR_HEIGHT
    + APPLICATION_MENU_GAP;

impl BExplorerIced {
    pub(in crate::iced_ui) fn properties_window_view(
        &self,
        palette: Palette,
    ) -> Element<'_, Message> {
        let (window_bg, window_title_bg) = palette.native_utility_backgrounds();
        let card_bg = palette.native_utility_card_background(self.config.vibrancy_active);
        let font_size = self.font_size();
        let Some(state) = &self.properties_window else {
            return container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(move |_| container::Style::default().background(window_bg))
                .into();
        };

        let title = self.properties_window_title();
        let title_drag_area = mouse_area(
            container(
                text(title)
                    .size(font_size)
                    .color(palette.text)
                    .align_x(Horizontal::Center)
                    .width(Length::Fill),
            )
            .height(TRANSFER_WINDOW_TITLE_HEIGHT)
            .width(Length::Fill)
            .center_y(Length::Fill),
        )
        .on_press(Message::Properties(PropertiesMessage::Drag));
        let title_bar = container(
            row![
                title_drag_area,
                native_window_close_button_maybe(
                    (!state.applying).then_some(Message::Properties(PropertiesMessage::Close)),
                    palette,
                ),
            ]
            .align_y(Alignment::Center),
        )
        .height(TRANSFER_WINDOW_TITLE_HEIGHT)
        .width(Length::Fill)
        .style(move |_| {
            container::Style::default()
                .background(window_title_bg)
                .border(border::rounded(border::top(WINDOW_RADIUS - 1.0)))
        });

        let tab_card = container(
            row![
                properties_tab_button(
                    self.localized("General", "General"),
                    PropertiesTab::General,
                    state.tab,
                    palette,
                    font_size,
                ),
                properties_tab_button(
                    self.localized("Permisos", "Permissions"),
                    PropertiesTab::Permissions,
                    state.tab,
                    palette,
                    font_size,
                ),
                properties_tab_button(
                    self.localized("Detalles", "Details"),
                    PropertiesTab::Details,
                    state.tab,
                    palette,
                    font_size,
                ),
            ]
            .spacing(2)
            .height(Length::Fill)
            .align_y(Alignment::Center),
        )
        .height(PROPERTIES_TAB_HEIGHT)
        .width(Length::Fill)
        .padding([3, 6])
        .style(move |_| {
            container::Style::default()
                .background(window_title_bg)
                .border(border::color(palette.border).width(1))
        });
        let tabs = container(tab_card)
            .height(PROPERTIES_TAB_REGION_HEIGHT)
            .width(Length::Fill)
            .padding(Padding {
                top: PROPERTIES_TAB_TOP_MARGIN,
                right: 0.0,
                bottom: 0.0,
                left: 0.0,
            })
            .style(move |_| container::Style::default().background(window_bg));

        let content: Element<'_, Message> = if state.loading {
            container(
                column![
                    inline_icon("refresh", palette.accent, 28.0),
                    text(self.localized("Cargando propiedades...", "Loading properties..."))
                        .size(font_size)
                        .color(palette.muted_text),
                ]
                .spacing(12)
                .align_x(Alignment::Center),
            )
            .center(Length::Fill)
            .into()
        } else if state.snapshot.is_none() {
            container(
                text(
                    state
                        .notice
                        .as_ref()
                        .map(|notice| notice.0.as_str())
                        .unwrap_or(self.localized(
                            "No se pudieron cargar las propiedades.",
                            "The properties could not be loaded.",
                        )),
                )
                .size(font_size)
                .color(Color::from_rgb8(227, 107, 114))
                .width(Length::Fill),
            )
            .padding(18)
            .width(Length::Fill)
            .into()
        } else {
            match state.tab {
                PropertiesTab::General => self.properties_general_tab(state, palette, card_bg),
                PropertiesTab::Permissions => {
                    self.properties_permissions_tab(state, palette, card_bg)
                }
                PropertiesTab::Details => self.properties_details_tab(state, palette, card_bg),
            }
        };

        let dirty = state.is_dirty();
        let apply_enabled = dirty && !state.applying && !state.loading;
        let notice = state
            .notice
            .as_ref()
            .map(|(message, is_error)| {
                text(ellipsize_text(message, 42))
                    .size(font_size - 1.0)
                    .color(if *is_error {
                        Color::from_rgb8(227, 107, 114)
                    } else {
                        palette.muted_text
                    })
                    .wrapping(iced::widget::text::Wrapping::None)
            })
            .unwrap_or_else(|| text("").size(font_size - 1.0));
        let footer = container(
            row![
                notice.width(Length::Fill),
                Button::new(text(self.localized("Cancelar", "Cancel")).size(font_size))
                    .padding([7, 13])
                    .on_press_maybe(
                        (!state.applying).then_some(Message::Properties(PropertiesMessage::Close)),
                    )
                    .style(move |_, status| dialog_button_style(palette, false, status)),
                Button::new(
                    text(self.localized("Aplicar", "Apply"))
                        .size(font_size)
                        .color(if apply_enabled {
                            palette.text
                        } else {
                            palette.muted_text
                        }),
                )
                .padding([7, 13])
                .on_press_maybe(
                    apply_enabled.then_some(Message::Properties(PropertiesMessage::Apply)),
                )
                .style(move |_, status| dialog_button_style(palette, false, status)),
                Button::new(text(self.localized("Aceptar", "OK")).size(font_size))
                    .padding([7, 15])
                    .on_press_maybe(
                        (!state.applying)
                            .then_some(Message::Properties(PropertiesMessage::Accept,))
                    )
                    .style(move |_, status| dialog_button_style(palette, true, status)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .height(PROPERTIES_FOOTER_HEIGHT)
        .padding([8, 10])
        .align_y(Vertical::Center)
        .width(Length::Fill)
        .style(move |_| {
            container::Style::default()
                .background(window_title_bg)
                .border(border::color(palette.border).width(1))
        });

        let inner_height = PROPERTIES_WINDOW_HEIGHT - WINDOW_BORDER_WIDTH * 2.0;
        let body = container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_| container::Style::default().background(window_bg));
        let inner = container(
            column![title_bar, tabs, body, footer]
                .width(Length::Fill)
                .height(Length::Fixed(inner_height)),
        )
        .width(Length::Fill)
        .height(Length::Fixed(inner_height))
        .clip(true)
        .style(move |_| {
            container::Style::default()
                .background(window_bg)
                .border(border::rounded(WINDOW_RADIUS - WINDOW_BORDER_WIDTH))
        });
        let panel = container(inner)
            .width(Length::Fill)
            .height(Length::Fixed(PROPERTIES_WINDOW_HEIGHT))
            .padding(WINDOW_BORDER_WIDTH)
            .clip(true)
            .style(move |_| {
                container::Style::default()
                    .background(Color::TRANSPARENT)
                    .border(
                        border::rounded(WINDOW_RADIUS)
                            .color(window_border_color(palette))
                            .width(WINDOW_BORDER_WIDTH),
                    )
            });

        container(column![panel, Space::new().height(Length::Fill)])
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style::default().background(Color::TRANSPARENT))
            .into()
    }

    fn properties_general_tab<'a>(
        &'a self,
        state: &'a PropertiesWindowState,
        palette: Palette,
        card_bg: Color,
    ) -> Element<'a, Message> {
        let snapshot = state.snapshot.as_ref().expect("properties snapshot");
        let font_size = self.font_size();
        let icon: Element<'_, Message> = if let Some(handle) = &state.icon {
            iced_image::Image::new(handle.clone())
                .width(PROPERTIES_ICON_SIZE)
                .height(PROPERTIES_ICON_SIZE)
                .content_fit(ContentFit::Contain)
                .into()
        } else {
            let label = match snapshot.kind {
                PropertyKind::Directory => "folder",
                PropertyKind::SymlinkFile
                | PropertyKind::SymlinkDirectory
                | PropertyKind::BrokenSymlink => "lnk",
                PropertyKind::Multiple => "copy",
                PropertyKind::File | PropertyKind::Other => "file",
            };
            container(inline_icon(label, palette.accent, 34.0))
                .width(PROPERTIES_ICON_SIZE)
                .height(PROPERTIES_ICON_SIZE)
                .center(Length::Fill)
                .into()
        };
        let name: Element<'_, Message> = if state.can_rename() {
            text_input("", &state.name)
                .on_input(|value| Message::Properties(PropertiesMessage::NameChanged(value)))
                .size(font_size + 1.0)
                .padding([4, 7])
                .width(Length::Fill)
                .into()
        } else {
            text(&state.name)
                .size(font_size + 2.0)
                .color(palette.text)
                .width(Length::Fill)
                .into()
        };
        let heading = row![icon, name].spacing(9).align_y(Alignment::Center);

        let location = snapshot
            .location
            .as_ref()
            .or_else(|| {
                snapshot
                    .paths
                    .first()
                    .filter(|path| path.parent().is_none())
            })
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "—".into());
        let type_row = container(property_value_row(
            self.localized("Tipo", "Type"),
            localized_property_kind(snapshot.kind, self.is_spanish()),
            palette,
            font_size,
        ))
        .height(GENERAL_TYPE_ROW_HEIGHT)
        .align_y(Vertical::Center);
        let mut basic = column![type_row].spacing(PROPERTIES_ITEM_SPACING);
        if !snapshot.applications.is_empty() {
            basic = basic.push(
                row![
                    container(
                        text(self.localized("Abrir con", "Open with"))
                            .size(font_size - 1.0)
                            .color(palette.muted_text),
                    )
                    .width(PROPERTY_LABEL_WIDTH)
                    .height(APPLICATION_SELECTOR_HEIGHT)
                    .center_y(Length::Fill),
                    application_selector(
                        state,
                        self.localized("Sin predeterminado", "No default"),
                        palette,
                        font_size,
                    ),
                ]
                .spacing(PROPERTY_TEXT_SPACING)
                .align_y(Alignment::Start),
            );
        }
        basic = basic.push(property_value_row(
            self.localized("Ubicación", "Location"),
            location,
            palette,
            font_size,
        ));
        if let Some(symlink) = &snapshot.symlink {
            let target = if symlink.broken {
                format!(
                    "{} ({})",
                    symlink.raw_target.display(),
                    self.localized("destino roto", "broken target")
                )
            } else {
                symlink.raw_target.display().to_string()
            };
            basic = basic.push(property_value_row(
                self.localized("Destino", "Target"),
                target,
                palette,
                font_size,
            ));
        }

        let size = state.size.as_ref();
        let logical_size = size.map_or(snapshot.logical_size, |size| size.bytes);
        let allocated_size = size.map_or(snapshot.allocated_size, |size| size.allocated);
        let item_summary = size.map(|size| {
            let mut summary = if self.is_spanish() {
                format!(
                    "{} archivos, {} carpetas, {} enlaces",
                    size.files, size.directories, size.links
                )
            } else {
                format!(
                    "{} files, {} folders, {} links",
                    size.files, size.directories, size.links
                )
            };
            if size.unreadable > 0 {
                summary.push_str(&if self.is_spanish() {
                    format!(", {} sin acceso", size.unreadable)
                } else {
                    format!(", {} inaccessible", size.unreadable)
                });
            }
            summary
        });
        let size_buttons: Element<'_, Message> = if snapshot.contains_directory {
            if state.size_loading {
                Button::new(text(self.localized("Detener", "Stop")).size(font_size - 1.0))
                    .padding([4, 7])
                    .on_press(Message::Properties(PropertiesMessage::StopSize))
                    .style(move |_, status| dialog_button_style(palette, false, status))
                    .into()
            } else {
                Button::new(text(self.localized("Actualizar", "Refresh")).size(font_size - 1.0))
                    .padding([4, 7])
                    .on_press_maybe(
                        (!state.applying)
                            .then_some(Message::Properties(PropertiesMessage::RefreshSize)),
                    )
                    .style(move |_, status| dialog_button_style(palette, false, status))
                    .into()
            }
        } else {
            Space::new().into()
        };
        let size_text = if state.size_loading {
            self.localized("Calculando...", "Calculating...").to_owned()
        } else {
            format_size(Some(logical_size))
        };
        let mut size_column = column![
            property_value_row(
                self.localized("Tamaño", "Size"),
                size_text,
                palette,
                font_size,
            ),
            property_value_row(
                self.localized("Tamaño en disco", "Size on disk"),
                format_size(Some(allocated_size)),
                palette,
                font_size,
            ),
        ]
        .spacing(PROPERTIES_ITEM_SPACING);
        match (item_summary, snapshot.contains_directory) {
            (Some(summary), true) => {
                size_column = size_column.push(
                    row![
                        text(summary)
                            .size(font_size - 1.0)
                            .color(palette.muted_text)
                            .width(Length::Fill),
                        size_buttons,
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                );
            }
            (None, true) => {
                size_column = size_column.push(
                    row![Space::new().width(Length::Fill), size_buttons].align_y(Alignment::Center),
                );
            }
            (Some(summary), false) => {
                size_column = size_column.push(
                    text(summary)
                        .size(font_size - 1.0)
                        .color(palette.muted_text),
                );
            }
            (None, false) => {}
        }
        let dates = column![
            property_value_row(
                self.localized("Creado", "Created"),
                format_property_time(snapshot.created, self.is_spanish()),
                palette,
                font_size,
            ),
            property_value_row(
                self.localized("Modificado", "Modified"),
                format_property_time(snapshot.modified, self.is_spanish()),
                palette,
                font_size,
            ),
            property_value_row(
                self.localized("Accedido", "Accessed"),
                format_property_time(snapshot.accessed, self.is_spanish()),
                palette,
                font_size,
            ),
        ]
        .spacing(PROPERTIES_ITEM_SPACING);

        let mut content = column![
            heading,
            properties_divider(palette),
            basic,
            properties_divider(palette),
            size_column,
            properties_divider(palette),
            dates,
        ]
        .spacing(PROPERTIES_SECTION_SPACING);

        if let Some(mount) = &snapshot.mount {
            let used_fraction = mount
                .total
                .zip(mount.free)
                .filter(|(total, _)| *total > 0)
                .map(|(total, free)| total.saturating_sub(free) as f32 / total as f32)
                .unwrap_or_default();
            let capacity = mount
                .total
                .zip(mount.available.or(mount.free))
                .map(|(total, available)| {
                    format!(
                        "{} {} {}",
                        format_size(Some(available)),
                        self.localized("libres de", "free of"),
                        format_size(Some(total))
                    )
                })
                .unwrap_or_default();
            let mount_content = column![
                property_value_row(
                    self.localized("Sistema", "File system"),
                    mount.file_system.clone(),
                    palette,
                    font_size,
                ),
                property_value_row(
                    self.localized("Montado en", "Mounted at"),
                    mount.mount_point.display().to_string(),
                    palette,
                    font_size,
                ),
                property_value_row(
                    self.localized("Dispositivo", "Device"),
                    mount.source.clone(),
                    palette,
                    font_size,
                ),
                row![
                    text(self.localized("Espacio", "Space"))
                        .size(font_size - 1.0)
                        .color(palette.muted_text)
                        .width(PROPERTY_LABEL_WIDTH),
                    column![
                        transfer_progress_bar(used_fraction, palette, 7.0),
                        text(capacity)
                            .size(font_size - 1.0)
                            .color(palette.muted_text),
                    ]
                    .spacing(4)
                    .width(Length::Fill),
                ]
                .spacing(PROPERTY_TEXT_SPACING)
                .align_y(Alignment::Center),
            ]
            .spacing(PROPERTIES_ITEM_SPACING);
            content = content
                .push(properties_divider(palette))
                .push(mount_content);
        }

        let card = container(content)
            .padding([
                PROPERTIES_CARD_VERTICAL_PADDING,
                PROPERTIES_CARD_HORIZONTAL_PADDING,
            ])
            .width(Length::Fill)
            .style(move |_| properties_card_style(palette, card_bg));
        let body: Element<'a, Message> = scrollable(container(card).padding([
            PROPERTIES_OUTER_VERTICAL_PADDING,
            PROPERTIES_OUTER_HORIZONTAL_PADDING,
        ]))
        .style(move |theme, status| explorer_scrollable_style(palette, theme, status, 1.0))
        .width(Length::Fill)
        .height(Length::Fill)
        .into();
        if state.application_menu_open {
            let backdrop: Element<'a, Message> = mouse_area(
                container(Space::new())
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .on_press(Message::Properties(PropertiesMessage::CloseApplicationMenu))
            .into();
            let menu = pin(opaque(application_selector_menu(
                state,
                &snapshot.applications,
                palette,
                font_size,
            )))
            .x(APPLICATION_MENU_X)
            .y(APPLICATION_MENU_Y);
            stack(vec![body, backdrop, menu.into()])
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            body
        }
    }

    fn properties_permissions_tab<'a>(
        &'a self,
        state: &'a PropertiesWindowState,
        palette: Palette,
        card_bg: Color,
    ) -> Element<'a, Message> {
        let snapshot = state.snapshot.as_ref().expect("properties snapshot");
        let font_size = self.font_size();
        let editable = state.permission_editable();
        let identity_editable = state.identity_editable();
        let mode = state.mode.unwrap_or_default();

        let identity: Option<Element<'_, Message>> = if identity_editable {
            let owner = identity_selector(
                state,
                PropertiesIdentityMenu::Owner,
                &state.users,
                state.owner.as_ref(),
                self.localized("Sin propietario", "No owner"),
                palette,
                font_size,
            );
            let group = identity_selector(
                state,
                PropertiesIdentityMenu::Group,
                &state.groups,
                state.group.as_ref(),
                self.localized("Sin grupo", "No group"),
                palette,
                font_size,
            );
            Some(
                column![
                    row![
                        text(self.localized("Propietario", "Owner"))
                            .size(font_size - 1.0)
                            .color(palette.muted_text)
                            .width(PROPERTY_LABEL_WIDTH),
                        owner,
                    ]
                    .spacing(PROPERTY_TEXT_SPACING)
                    .align_y(Alignment::Center),
                    row![
                        text(self.localized("Grupo", "Group"))
                            .size(font_size - 1.0)
                            .color(palette.muted_text)
                            .width(PROPERTY_LABEL_WIDTH),
                        group,
                    ]
                    .spacing(PROPERTY_TEXT_SPACING)
                    .align_y(Alignment::Center),
                ]
                .spacing(PROPERTIES_ITEM_SPACING)
                .into(),
            )
        } else if state.owner.is_some()
            || state.group.is_some()
            || snapshot.owner.is_some()
            || snapshot.group.is_some()
        {
            Some(
                column![
                    property_value_row(
                        self.localized("Propietario", "Owner"),
                        state
                            .owner
                            .as_ref()
                            .map(|identity| identity.name.clone())
                            .or_else(|| snapshot.owner.clone())
                            .unwrap_or_else(|| "—".into()),
                        palette,
                        font_size,
                    ),
                    property_value_row(
                        self.localized("Grupo", "Group"),
                        state
                            .group
                            .as_ref()
                            .map(|identity| identity.name.clone())
                            .or_else(|| snapshot.group.clone())
                            .unwrap_or_else(|| "—".into()),
                        palette,
                        font_size,
                    ),
                ]
                .spacing(PROPERTIES_ITEM_SPACING)
                .into(),
            )
        } else {
            None
        };

        let permissions = column![
            permission_heading(self, palette, font_size),
            permission_row(
                self.localized("Propietario", "Owner"),
                mode,
                [0o400, 0o200, 0o100],
                editable,
                palette,
                font_size,
            ),
            permission_row(
                self.localized("Grupo", "Group"),
                mode,
                [0o040, 0o020, 0o010],
                editable,
                palette,
                font_size,
            ),
            permission_row(
                self.localized("Otros", "Others"),
                mode,
                [0o004, 0o002, 0o001],
                editable,
                palette,
                font_size,
            ),
            property_value_row(
                self.localized("Modo", "Mode"),
                format!("{:04o}", mode & 0o7777),
                palette,
                font_size,
            ),
        ]
        .spacing(PROPERTIES_ITEM_SPACING);

        let advanced = row![
            container(
                text(self.localized("Permisos avanzados", "Advanced permissions"))
                    .size(font_size - 1.0)
                    .color(palette.text),
            )
            .width(PROPERTY_LABEL_WIDTH + PROPERTY_TEXT_SPACING),
            permission_option_cell(
                self.localized("Setuid", "Setuid"),
                mode,
                0o4000,
                editable,
                palette,
                font_size,
            ),
            permission_option_cell(
                self.localized("Setgid", "Setgid"),
                mode,
                0o2000,
                editable,
                palette,
                font_size,
            ),
            permission_option_cell(
                self.localized("Sticky", "Sticky"),
                mode,
                0o1000,
                editable,
                palette,
                font_size,
            ),
        ]
        .align_y(Alignment::Center);

        let recursive: Option<Element<'_, Message>> =
            if snapshot.contains_directory && identity_editable {
                Some(
                    iced::widget::checkbox(state.recursive)
                        .label(self.localized(
                            "Aplicar los cambios también al contenido",
                            "Also apply changes to enclosed items",
                        ))
                        .on_toggle(|value| {
                            Message::Properties(PropertiesMessage::RecursiveChanged(value))
                        })
                        .text_size(font_size - 1.0)
                        .style(move |_, status| properties_checkbox_style(palette, status))
                        .into(),
                )
            } else {
                None
            };

        let warning: Option<Element<'_, Message>> = if matches!(
            snapshot.kind,
            PropertyKind::SymlinkFile
                | PropertyKind::SymlinkDirectory
                | PropertyKind::BrokenSymlink
        ) {
            Some(
                text(self.localized(
                    "Linux no permite cambiar los permisos del enlace simbólico. BExplorer no modificará silenciosamente su destino.",
                    "Linux does not support changing symbolic-link permissions. BExplorer will not silently modify its target.",
                ))
                .size(font_size - 1.0)
                .color(Color::from_rgb8(221, 154, 87))
                .width(Length::Fill)
                .into(),
            )
        } else if snapshot.mode.is_none() {
            Some(
                text(self.localized(
                    "Los elementos seleccionados tienen permisos diferentes. Ábrelos individualmente para modificarlos.",
                    "The selected items have different permissions. Open them individually to edit them.",
                ))
                .size(font_size - 1.0)
                .color(Color::from_rgb8(221, 154, 87))
                .width(Length::Fill)
                .into(),
            )
        } else {
            None
        };

        let mut content = column![].spacing(PROPERTIES_SECTION_SPACING);
        if let Some(identity) = identity {
            content = content.push(identity).push(properties_divider(palette));
        }
        content = content
            .push(permissions)
            .push(properties_divider(palette))
            .push(advanced);
        if let Some(recursive) = recursive {
            content = content.push(properties_divider(palette)).push(recursive);
        }
        if let Some(warning) = warning {
            content = content.push(properties_divider(palette)).push(warning);
        }
        let card = container(content)
            .padding([
                PROPERTIES_CARD_VERTICAL_PADDING,
                PROPERTIES_CARD_HORIZONTAL_PADDING,
            ])
            .width(Length::Fill)
            .style(move |_| properties_card_style(palette, card_bg));
        let body: Element<'a, Message> = scrollable(container(card).padding([
            PROPERTIES_OUTER_VERTICAL_PADDING,
            PROPERTIES_OUTER_HORIZONTAL_PADDING,
        ]))
        .style(move |theme, status| explorer_scrollable_style(palette, theme, status, 1.0))
        .width(Length::Fill)
        .height(Length::Fill)
        .into();
        if let Some(menu) = state.identity_menu_open {
            let (identities, y) = match menu {
                PropertiesIdentityMenu::Owner => (&state.users, OWNER_MENU_Y),
                PropertiesIdentityMenu::Group => (&state.groups, GROUP_MENU_Y),
            };
            let backdrop: Element<'a, Message> = mouse_area(
                container(Space::new())
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .on_press(Message::Properties(PropertiesMessage::CloseIdentityMenu))
            .into();
            let menu = pin(opaque(identity_selector_menu(
                state, menu, identities, palette, font_size,
            )))
            .x(IDENTITY_MENU_X)
            .y(y);
            stack(vec![body, backdrop, menu.into()])
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            body
        }
    }

    fn properties_details_tab<'a>(
        &'a self,
        state: &'a PropertiesWindowState,
        palette: Palette,
        card_bg: Color,
    ) -> Element<'a, Message> {
        let snapshot = state.snapshot.as_ref().expect("properties snapshot");
        let font_size = self.font_size();
        let paths = snapshot
            .paths
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let mut rows = column![
            property_value_row(
                self.localized("Elementos", "Items"),
                snapshot.items.len().to_string(),
                palette,
                font_size,
            ),
            property_value_row(self.localized("Ruta", "Path"), paths, palette, font_size,),
            property_value_row(
                "MIME",
                snapshot.mime_type.clone().unwrap_or_else(|| "—".into()),
                palette,
                font_size,
            ),
            property_value_row(
                self.localized("Tamaño exacto", "Exact size"),
                format!("{} bytes", snapshot.logical_size),
                palette,
                font_size,
            ),
            property_value_row(
                self.localized("Espacio asignado", "Allocated space"),
                format!("{} bytes", snapshot.allocated_size),
                palette,
                font_size,
            ),
            property_value_row("Inode", optional_number(snapshot.inode), palette, font_size,),
            property_value_row(
                self.localized("Dispositivo", "Device"),
                optional_number(snapshot.device),
                palette,
                font_size,
            ),
            property_value_row(
                self.localized("Enlaces físicos", "Hard links"),
                optional_number(snapshot.hard_links),
                palette,
                font_size,
            ),
            property_value_row(
                "UID",
                snapshot
                    .uid
                    .map_or_else(|| "—".into(), |value| value.to_string()),
                palette,
                font_size,
            ),
            property_value_row(
                "GID",
                snapshot
                    .gid
                    .map_or_else(|| "—".into(), |value| value.to_string()),
                palette,
                font_size,
            ),
            property_value_row(
                self.localized("Modo", "Mode"),
                snapshot
                    .mode
                    .map_or_else(|| "—".into(), |mode| format!("{:04o}", mode & 0o7777)),
                palette,
                font_size,
            ),
            property_value_row(
                self.localized("Creado", "Created"),
                format_property_time(snapshot.created, self.is_spanish()),
                palette,
                font_size,
            ),
            property_value_row(
                self.localized("Modificado", "Modified"),
                format_property_time(snapshot.modified, self.is_spanish()),
                palette,
                font_size,
            ),
            property_value_row(
                self.localized("Accedido", "Accessed"),
                format_property_time(snapshot.accessed, self.is_spanish()),
                palette,
                font_size,
            ),
            property_value_row(
                self.localized("Cambio de metadatos", "Metadata changed"),
                format_property_time(snapshot.changed, self.is_spanish()),
                palette,
                font_size,
            ),
        ]
        .spacing(PROPERTIES_ITEM_SPACING);
        if let Some(symlink) = &snapshot.symlink {
            rows = rows
                .push(property_value_row(
                    self.localized("Enlace", "Link target"),
                    symlink.raw_target.display().to_string(),
                    palette,
                    font_size,
                ))
                .push(property_value_row(
                    self.localized("Destino resuelto", "Resolved target"),
                    symlink.resolved_target.display().to_string(),
                    palette,
                    font_size,
                ));
        }
        if let Some(mount) = &snapshot.mount {
            rows = rows
                .push(property_value_row(
                    self.localized("Sistema de archivos", "File system"),
                    mount.file_system.clone(),
                    palette,
                    font_size,
                ))
                .push(property_value_row(
                    self.localized("Sólo lectura", "Read only"),
                    self.localized(
                        if mount.read_only { "Sí" } else { "No" },
                        if mount.read_only { "Yes" } else { "No" },
                    ),
                    palette,
                    font_size,
                ));
        }
        let card = container(rows)
            .padding([
                PROPERTIES_CARD_VERTICAL_PADDING,
                PROPERTIES_CARD_HORIZONTAL_PADDING,
            ])
            .width(Length::Fill)
            .style(move |_| properties_card_style(palette, card_bg));
        scrollable(container(card).padding([
            PROPERTIES_OUTER_VERTICAL_PADDING,
            PROPERTIES_OUTER_HORIZONTAL_PADDING,
        ]))
        .style(move |theme, status| explorer_scrollable_style(palette, theme, status, 1.0))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}

fn properties_tab_button<'a>(
    label: &'a str,
    tab: PropertiesTab,
    selected: PropertiesTab,
    palette: Palette,
    font_size: f32,
) -> Button<'a, Message> {
    Button::new(
        container(
            text(label)
                .size(font_size)
                .align_x(Horizontal::Center)
                .width(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_y(Length::Fill),
    )
    .height(Length::Fill)
    .width(Length::Fill)
    .padding(0)
    .on_press(Message::Properties(PropertiesMessage::SelectTab(tab)))
    .style(move |_, status| selected_button_style(palette, tab == selected, status))
}

fn identity_selector<'a>(
    state: &'a PropertiesWindowState,
    menu: PropertiesIdentityMenu,
    identities: &'a [PropertyIdentity],
    selected: Option<&'a PropertyIdentity>,
    placeholder: &'a str,
    palette: Palette,
    font_size: f32,
) -> Element<'a, Message> {
    let open = state.identity_menu_open == Some(menu);
    Button::new(
        container(
            row![
                text(
                    selected
                        .map(|identity| identity.name.as_str())
                        .unwrap_or(placeholder),
                )
                .size(font_size)
                .color(palette.text)
                .wrapping(iced::widget::text::Wrapping::None)
                .width(Length::Fill),
                inline_icon("chev-down", palette.muted_text, 11.0),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
        )
        .height(Length::Fill)
        .center_y(Length::Fill),
    )
    .width(Length::Fill)
    .height(IDENTITY_SELECTOR_HEIGHT)
    .padding([0, 8])
    .on_press_maybe((!identities.is_empty()).then_some(Message::Properties(
        PropertiesMessage::ToggleIdentityMenu(menu),
    )))
    .style(move |_, status| properties_selector_button_style(palette, open, status))
    .into()
}

fn identity_selector_menu<'a>(
    state: &'a PropertiesWindowState,
    menu: PropertiesIdentityMenu,
    identities: &'a [PropertyIdentity],
    palette: Palette,
    font_size: f32,
) -> Element<'a, Message> {
    let options = identities
        .iter()
        .enumerate()
        .map(|(index, identity)| {
            let highlighted = state.identity_menu_index == index;
            Button::new(
                container(
                    text(&identity.name)
                        .size(font_size)
                        .color(if highlighted {
                            palette.accent_text
                        } else {
                            palette.text
                        })
                        .wrapping(iced::widget::text::Wrapping::None)
                        .width(Length::Fill),
                )
                .height(Length::Fill)
                .center_y(Length::Fill),
            )
            .width(Length::Fill)
            .height(IDENTITY_SELECTOR_HEIGHT)
            .padding([0, 8])
            .on_press(Message::Properties(PropertiesMessage::IdentitySelected(
                menu,
                identity.clone(),
            )))
            .style(move |_, status| selected_button_style(palette, highlighted, status))
            .into()
        })
        .collect::<Vec<Element<'a, Message>>>();
    let menu_kind = match menu {
        PropertiesIdentityMenu::Owner => PropertiesSelectorMenu::Owner,
        PropertiesIdentityMenu::Group => PropertiesSelectorMenu::Group,
    };
    let menu_height = (identities.len().min(7) as f32 * 32.0 + 8.0).max(40.0);
    container(
        scrollable(column(options).spacing(2).padding(4))
            .id(properties_selector_scroll_id(menu_kind))
            .height(menu_height)
            .style(move |theme, status| explorer_scrollable_style(palette, theme, status, 1.0)),
    )
    .width(IDENTITY_SELECTOR_WIDTH)
    .height(menu_height)
    .style(move |_| properties_selector_menu_style(palette))
    .into()
}

fn application_selector<'a>(
    state: &'a PropertiesWindowState,
    placeholder: &'a str,
    palette: Palette,
    font_size: f32,
) -> Element<'a, Message> {
    let current = state.application.as_ref();
    let current_icon = current
        .and_then(|application| state.application_icons.get(application.desktop_id.as_str()));
    let current_name = current
        .map(|application| application.name.as_str())
        .unwrap_or(placeholder);
    let button = Button::new(
        container(
            row![
                application_icon(current_icon, palette),
                text(ellipsize_text(current_name, 24))
                    .size(font_size)
                    .color(palette.text)
                    .wrapping(iced::widget::text::Wrapping::None)
                    .width(Length::Fill),
                inline_icon("chev-down", palette.muted_text, 11.0),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
        )
        .height(Length::Fill)
        .center_y(Length::Fill),
    )
    .width(APPLICATION_SELECTOR_WIDTH)
    .height(APPLICATION_SELECTOR_HEIGHT)
    .padding([0, 8])
    .on_press(Message::Properties(
        PropertiesMessage::ToggleApplicationMenu,
    ))
    .style(move |_, status| {
        properties_selector_button_style(palette, state.application_menu_open, status)
    });
    button.into()
}

fn application_selector_menu<'a>(
    state: &'a PropertiesWindowState,
    applications: &'a [PropertyApplication],
    palette: Palette,
    font_size: f32,
) -> Element<'a, Message> {
    let options = applications
        .iter()
        .enumerate()
        .map(|(index, application)| {
            let highlighted = state.application_menu_index == index;
            let color = if highlighted {
                palette.accent_text
            } else {
                palette.text
            };
            let icon = state.application_icons.get(application.desktop_id.as_str());
            Button::new(
                container(
                    row![
                        application_icon(icon, palette),
                        text(ellipsize_text(&application.name, 25))
                            .size(font_size)
                            .color(color)
                            .wrapping(iced::widget::text::Wrapping::None)
                            .width(Length::Fill),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .height(Length::Fill)
                .center_y(Length::Fill),
            )
            .width(Length::Fill)
            .height(APPLICATION_SELECTOR_HEIGHT)
            .padding([0, 7])
            .on_press(Message::Properties(PropertiesMessage::ApplicationSelected(
                application.clone(),
            )))
            .style(move |_, status| selected_button_style(palette, highlighted, status))
            .into()
        })
        .collect::<Vec<Element<'a, Message>>>();
    let menu_height = (applications.len().min(5) as f32 * 30.0 + 8.0).max(38.0);
    let menu = container(
        scrollable(column(options).spacing(2).padding(4))
            .id(properties_selector_scroll_id(
                PropertiesSelectorMenu::Application,
            ))
            .height(menu_height)
            .style(move |theme, status| explorer_scrollable_style(palette, theme, status, 1.0)),
    )
    .width(APPLICATION_SELECTOR_WIDTH)
    .height(menu_height)
    .style(move |_| properties_selector_menu_style(palette));
    menu.into()
}

fn application_icon<'a>(
    handle: Option<&iced_image::Handle>,
    palette: Palette,
) -> Element<'a, Message> {
    handle.map_or_else(
        || inline_icon("open", palette.muted_text, 17.0),
        |handle| {
            iced_image::Image::new(handle.clone())
                .width(18)
                .height(18)
                .content_fit(ContentFit::Contain)
                .into()
        },
    )
}

fn properties_selector_button_style(
    palette: Palette,
    open: bool,
    status: button::Status,
) -> button::Style {
    let background = if matches!(status, button::Status::Hovered | button::Status::Pressed) {
        mix_color(palette.input_bg, hover_tint(palette), 0.46)
    } else {
        palette.input_bg
    };
    button::Style {
        background: Some(background.into()),
        text_color: palette.text,
        border: border::rounded(4)
            .color(if open {
                palette.accent
            } else {
                palette.strong_border
            })
            .width(1),
        ..button::Style::default()
    }
}

fn properties_selector_menu_style(palette: Palette) -> container::Style {
    container::Style::default()
        .background(palette.menu_bg)
        .border(border::rounded(5).color(palette.strong_border).width(1))
        .shadow(iced::Shadow {
            color: Color::from_rgba8(0, 0, 0, 0.25),
            offset: Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        })
}

fn properties_divider<'a>(palette: Palette) -> Element<'a, Message> {
    container(Space::new())
        .width(Length::Fill)
        .height(1)
        .style(move |_| {
            container::Style::default().background(translucent_color(palette.border, 0.72))
        })
        .into()
}

fn property_value_row<'a>(
    label: &'a str,
    value: impl Into<String>,
    palette: Palette,
    font_size: f32,
) -> Element<'a, Message> {
    row![
        text(label)
            .size(font_size - 1.0)
            .color(palette.muted_text)
            .width(PROPERTY_LABEL_WIDTH),
        text(value.into())
            .size(font_size)
            .color(palette.text)
            .width(Length::Fill)
            .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
    ]
    .spacing(PROPERTY_TEXT_SPACING)
    .align_y(Alignment::Start)
    .into()
}

fn properties_card_style(palette: Palette, background: Color) -> container::Style {
    container::Style::default()
        .background(background)
        .border(border::rounded(6).color(palette.border).width(1))
}

fn localized_property_kind(kind: PropertyKind, spanish: bool) -> &'static str {
    match (kind, spanish) {
        (PropertyKind::File, true) => "Archivo",
        (PropertyKind::File, false) => "File",
        (PropertyKind::Directory, true) => "Carpeta",
        (PropertyKind::Directory, false) => "Folder",
        (PropertyKind::SymlinkFile, true) => "Enlace simbólico a archivo",
        (PropertyKind::SymlinkFile, false) => "Symbolic link to file",
        (PropertyKind::SymlinkDirectory, true) => "Enlace simbólico a carpeta",
        (PropertyKind::SymlinkDirectory, false) => "Symbolic link to folder",
        (PropertyKind::BrokenSymlink, true) => "Enlace simbólico roto",
        (PropertyKind::BrokenSymlink, false) => "Broken symbolic link",
        (PropertyKind::Other, true) => "Otro",
        (PropertyKind::Other, false) => "Other",
        (PropertyKind::Multiple, true) => "Varios elementos",
        (PropertyKind::Multiple, false) => "Multiple items",
    }
}

fn permission_heading(
    app: &BExplorerIced,
    palette: Palette,
    font_size: f32,
) -> Element<'static, Message> {
    row![
        container(
            text(app.localized("Permisos de acceso", "Access permissions"))
                .size(font_size - 1.0)
                .color(palette.text),
        )
        .width(PROPERTY_LABEL_WIDTH + PROPERTY_TEXT_SPACING),
        permission_heading_cell(app.localized("Leer", "Read"), palette, font_size),
        permission_heading_cell(app.localized("Escribir", "Write"), palette, font_size),
        permission_heading_cell(app.localized("Ejecutar", "Execute"), palette, font_size),
    ]
    .align_y(Alignment::Center)
    .into()
}

fn permission_heading_cell(
    label: &'static str,
    palette: Palette,
    font_size: f32,
) -> Element<'static, Message> {
    container(text(label).size(font_size - 1.0).color(palette.muted_text))
        .width(PERMISSION_COLUMN_WIDTH)
        .align_x(Horizontal::Center)
        .into()
}

fn permission_row<'a>(
    label: &'a str,
    mode: u32,
    bits: [u32; 3],
    editable: bool,
    palette: Palette,
    font_size: f32,
) -> Element<'a, Message> {
    row![
        text(label)
            .size(font_size - 1.0)
            .color(palette.muted_text)
            .width(PROPERTY_LABEL_WIDTH + PROPERTY_TEXT_SPACING),
        permission_bit_cell(mode, bits[0], editable, palette, font_size),
        permission_bit_cell(mode, bits[1], editable, palette, font_size),
        permission_bit_cell(mode, bits[2], editable, palette, font_size),
    ]
    .align_y(Alignment::Center)
    .into()
}

fn permission_bit_cell(
    mode: u32,
    bit: u32,
    editable: bool,
    palette: Palette,
    font_size: f32,
) -> Element<'static, Message> {
    container(permission_checkbox(
        "", mode, bit, editable, palette, font_size,
    ))
    .width(PERMISSION_COLUMN_WIDTH)
    .align_x(Horizontal::Center)
    .into()
}

fn permission_option_cell(
    label: &'static str,
    mode: u32,
    bit: u32,
    editable: bool,
    palette: Palette,
    font_size: f32,
) -> Element<'static, Message> {
    container(permission_checkbox(
        label, mode, bit, editable, palette, font_size,
    ))
    .width(PERMISSION_COLUMN_WIDTH)
    .align_x(Horizontal::Center)
    .into()
}

fn permission_checkbox<'a>(
    label: &'a str,
    mode: u32,
    bit: u32,
    editable: bool,
    palette: Palette,
    font_size: f32,
) -> iced::widget::Checkbox<'a, Message> {
    let checkbox = iced::widget::checkbox(mode & bit != 0).size(16);
    let checkbox = if label.is_empty() {
        checkbox
    } else {
        checkbox.label(label)
    };
    let checkbox = checkbox
        .text_size(font_size - 1.0)
        .spacing(5)
        .style(move |_, status| properties_checkbox_style(palette, status));
    if editable {
        checkbox.on_toggle(move |_| Message::Properties(PropertiesMessage::PermissionToggled(bit)))
    } else {
        checkbox
    }
}

fn properties_checkbox_style(
    palette: Palette,
    status: iced::widget::checkbox::Status,
) -> iced::widget::checkbox::Style {
    let (is_checked, hovered, disabled) = match status {
        iced::widget::checkbox::Status::Active { is_checked } => (is_checked, false, false),
        iced::widget::checkbox::Status::Hovered { is_checked } => (is_checked, true, false),
        iced::widget::checkbox::Status::Disabled { is_checked } => (is_checked, false, true),
    };
    let background: Background = if is_checked && !disabled {
        accent_gradient(palette).into()
    } else if is_checked {
        mix_color(palette.input_bg, palette.accent, 0.45).into()
    } else if hovered {
        mix_color(palette.input_bg, hover_tint(palette), 0.45).into()
    } else {
        palette.input_bg.into()
    };
    let border_color = if is_checked {
        if disabled {
            mix_color(palette.strong_border, palette.accent, 0.42)
        } else if hovered {
            mix_color(palette.accent, Color::WHITE, 0.16)
        } else {
            palette.accent
        }
    } else if hovered {
        mix_color(palette.strong_border, palette.accent, 0.45)
    } else {
        palette.strong_border
    };
    iced::widget::checkbox::Style {
        background,
        icon_color: if disabled {
            mix_color(palette.muted_text, palette.accent_text, 0.35)
        } else {
            palette.accent_text
        },
        border: border::rounded(3).color(border_color).width(1),
        text_color: Some(if disabled {
            palette.muted_text
        } else {
            palette.text
        }),
    }
}

fn format_property_time(time: Option<std::time::SystemTime>, spanish: bool) -> String {
    let Some(time) = time else {
        return "—".into();
    };
    let date: chrono::DateTime<chrono::Local> = time.into();
    if spanish {
        date.format("%d/%m/%Y %H:%M:%S").to_string()
    } else {
        date.format("%Y-%m-%d %H:%M:%S").to_string()
    }
}

fn optional_number(value: Option<u64>) -> String {
    value.map_or_else(|| "—".into(), |value| value.to_string())
}
