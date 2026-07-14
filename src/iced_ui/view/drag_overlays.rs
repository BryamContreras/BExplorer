use super::*;
use iced::widget::{column, row};

impl BExplorerIced {
    pub(super) fn tab_split_placement_overlay(
        &self,
        palette: Palette,
    ) -> Option<Element<'_, Message>> {
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

    pub(super) fn tab_split_placement_option(
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

    pub(super) fn file_drag_overlay(&self, palette: Palette) -> Option<Element<'_, Message>> {
        let fade = self.file_drag_fade_progress.clamp(0.0, 1.0);
        if fade <= 0.0 {
            return None;
        }
        let drag = self
            .file_drag
            .as_ref()
            .filter(|drag| drag.dragging)
            .or(self.file_drag_fade_snapshot.as_ref())?;
        let palette = palette.with_opacity(fade);
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
        let preview_stack = self.file_drag_preview_stack(drag, palette, fade);
        let card = container(
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
        .width(296)
        .padding([8, 10])
        .clip(true);
        let card = container(card).clip(true).style(move |_| {
            container::Style::default()
                .background(palette.menu_bg)
                .border(
                    border::rounded(6)
                        .color(translucent_color(palette.accent, 0.78 * fade))
                        .width(1),
                )
                .shadow(iced::Shadow {
                    color: Color::from_rgba8(0, 0, 0, 0.34 * fade),
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

    pub(super) fn file_drag_preview_stack(
        &self,
        drag: &FileDragState,
        palette: Palette,
        fade: f32,
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
                    .opacity(fade)
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
                                .color(translucent_color(palette.accent_text, 0.88 * fade))
                                .width(1.5),
                        )
                        .shadow(iced::Shadow {
                            color: Color::from_rgba8(0, 0, 0, 0.38 * fade),
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

    pub(super) fn file_drag_destination_label(&self, drag: &FileDragState) -> Option<String> {
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
}
