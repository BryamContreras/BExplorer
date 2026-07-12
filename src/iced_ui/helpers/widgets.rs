use super::*;
use iced::widget::row;
pub(in crate::iced_ui) fn table_header(
    label: &'static str,
    width: f32,
    pane: PaneId,
    column: TableColumn,
    resizable: bool,
    sort_active: bool,
    sort_ascending: bool,
    palette: Palette,
    font_size: f32,
) -> Element<'static, Message> {
    let light_header = palette.header_bg.r + palette.header_bg.g + palette.header_bg.b > 2.1;
    let header_text = if light_header {
        mix_color(palette.text, Color::BLACK, 0.12)
    } else {
        palette.muted_text
    };
    let header_background = if light_header {
        mix_color(palette.header_bg, Color::WHITE, 0.46)
    } else {
        mix_color(palette.header_bg, Color::BLACK, 0.12)
    };
    let header_background = chrome_glass_background(palette, header_background);
    let indicator = if sort_active {
        if sort_ascending { "▲" } else { "▼" }
    } else {
        ""
    };
    let label_button = Button::new(
        row![
            text(label)
                .size(font_size)
                .color(header_text)
                .wrapping(iced::widget::text::Wrapping::None)
                .width(Length::Fill),
            text(indicator)
                .size((font_size - 1.0).max(10.0))
                .color(palette.accent),
        ]
        .align_y(Alignment::Center)
        .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(DETAIL_HEADER_HEIGHT)
    .padding([0, 8])
    .on_press(Message::SortColumn(pane, column))
    .style(move |_, status| button_style(palette, false, status));

    let content: Element<'static, Message> = if resizable {
        row![
            label_button,
            mouse_area(
                row![
                    Space::new().width(2),
                    container(Space::new())
                        .width(1)
                        .height(Length::Fill)
                        .style(move |_| container::Style::default().background(palette.border)),
                    Space::new().width(2),
                ]
                .height(Length::Fill)
                .width(DETAIL_COLUMN_HANDLE_WIDTH)
            )
            .on_press(Message::StartColumnResize(pane, column))
            .interaction(mouse::Interaction::ResizingColumn),
        ]
        .align_y(Alignment::Center)
        .height(DETAIL_HEADER_HEIGHT)
        .into()
    } else {
        row![label_button]
            .align_y(Alignment::Center)
            .height(DETAIL_HEADER_HEIGHT)
            .into()
    };

    let container = container(content)
        .height(DETAIL_HEADER_HEIGHT)
        .align_y(Alignment::Center)
        .style(move |_| container::Style::default().background(header_background));

    if width > 0.0 {
        container.width(width).into()
    } else {
        container.width(Length::Fill).into()
    }
}

pub(in crate::iced_ui) fn render_progress_footer(
    visible: usize,
    total: usize,
    palette: Palette,
    font_size: f32,
) -> Element<'static, Message> {
    container(
        text(format!(
            "Showing {visible} of {total}. Scroll to continue loading."
        ))
        .size(font_size)
        .color(palette.muted_text),
    )
    .width(Length::Fill)
    .height(36)
    .center_x(Length::Fill)
    .center_y(36)
    .into()
}

pub(in crate::iced_ui) fn file_group_header(
    label: String,
    palette: Palette,
    font_size: f32,
) -> Element<'static, Message> {
    container(
        text(label)
            .size((font_size - 0.4).max(11.0))
            .color(palette.muted_text),
    )
    .width(Length::Fill)
    .height(DETAIL_GROUP_HEIGHT)
    // Keep the group strip flush with the file surface while giving its label
    // enough breathing room from the horizontal edge.
    .padding([0, 10])
    .align_y(Alignment::Center)
    .style(move |_| {
        container::Style::default()
            .background(chrome_glass_background(
                palette,
                mix_color(palette.table_bg, palette.header_bg, 0.52),
            ))
            .border(border::color(translucent_color(palette.border, 0.62)).width(1))
    })
    .into()
}

pub(in crate::iced_ui) fn format_size(size: Option<u64>) -> String {
    let Some(size) = size else {
        return String::new();
    };
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let size = size as f64;
    if size >= GB {
        format!("{:.1} GB", size / GB)
    } else if size >= MB {
        format!("{:.1} MB", size / MB)
    } else if size >= KB {
        format!("{:.1} KB", size / KB)
    } else {
        format!("{size:.0} B")
    }
}

#[cfg(test)]
pub(in crate::iced_ui) fn tile_metadata_label(entry: &FileEntry) -> String {
    let type_label = entry.type_label();
    if matches!(&entry.kind, EntryKind::File | EntryKind::Other) {
        if let Some(size) = entry.size {
            return format!("{type_label} · {}", format_size(Some(size)));
        }
    }
    type_label
}

#[cfg(test)]
pub(in crate::iced_ui) fn drive_capacity_label(entry: &FileEntry) -> String {
    match (entry.size, entry.free_space) {
        (Some(total), Some(free)) => format!(
            "{} de {}",
            format_size(Some(total.saturating_sub(free))),
            format_size(Some(total)),
        ),
        _ => entry.type_label(),
    }
}

pub(in crate::iced_ui) fn transfer_progress_bar(
    progress: f32,
    palette: Palette,
    height: f32,
) -> Element<'static, Message> {
    let filled = ((progress.clamp(0.0, 1.0) * 1000.0).round() as u16).min(1000);
    let empty = 1000_u16.saturating_sub(filled);
    let mut segments = row![].spacing(0).height(height);

    if filled > 0 {
        segments = segments.push(
            container(Space::new())
                .height(height)
                .width(Length::FillPortion(filled))
                .style(move |_| {
                    container::Style::default()
                        .background(accent_gradient(palette))
                        .border(border::rounded(height / 2.0))
                }),
        );
    }

    if empty > 0 {
        segments = segments.push(
            Space::new()
                .height(height)
                .width(Length::FillPortion(empty)),
        );
    }

    container(segments)
        .height(height)
        .width(Length::Fill)
        .clip(true)
        .style(move |_| {
            container::Style::default()
                .background(palette.header_bg)
                .border(border::rounded(height / 2.0))
        })
        .into()
}

pub(in crate::iced_ui) fn drive_capacity_bar(
    progress: f32,
    palette: Palette,
) -> Element<'static, Message> {
    const HEIGHT: f32 = 10.0;
    const RADIUS: f32 = 2.0;

    let filled = ((progress.clamp(0.0, 1.0) * 1000.0).round() as u16).min(1000);
    let empty = 1000_u16.saturating_sub(filled);
    let mut segments = row![].spacing(0).height(HEIGHT);

    if filled > 0 {
        segments = segments.push(
            container(Space::new())
                .height(Length::Fill)
                .width(Length::FillPortion(filled))
                .style(move |_| container::Style::default().background(accent_gradient(palette))),
        );
    }

    if empty > 0 {
        segments = segments.push(
            Space::new()
                .height(Length::Fill)
                .width(Length::FillPortion(empty)),
        );
    }

    container(segments)
        .height(HEIGHT)
        .width(Length::Fill)
        .clip(true)
        .style(move |_| {
            container::Style::default()
                .background(mix_color(palette.input_bg, palette.border, 0.38))
                .border(border::rounded(RADIUS).color(palette.border).width(1))
        })
        .into()
}

pub(in crate::iced_ui) fn elevated_panel_style(palette: Palette) -> container::Style {
    container::Style::default()
        .background(palette.menu_bg)
        .border(border::rounded(8).color(palette.strong_border).width(1))
        .shadow(iced::Shadow {
            color: Color::from_rgba8(0, 0, 0, 0.38),
            offset: iced::Vector::new(0.0, 10.0),
            blur_radius: 24.0,
        })
}

pub(in crate::iced_ui) fn transfer_title(item: &TransferDisplayState) -> &'static str {
    match item.state {
        TransferState::Pending => "En cola",
        TransferState::Paused => "Pausado",
        TransferState::Finished => "Transferencia completada",
        TransferState::Cancelled => "Transferencia cancelada",
        TransferState::Failed => "Transferencia fallida",
        TransferState::Copying => match item.kind {
            TransferKind::Copy => "Copiando",
            TransferKind::Move => "Moviendo",
        },
    }
}

pub(in crate::iced_ui) fn transfer_state_text(item: &TransferDisplayState) -> &'static str {
    match item.state {
        TransferState::Pending => "Esperando",
        TransferState::Copying => "Copiando archivos",
        TransferState::Paused => "Pausado",
        TransferState::Finished => "Completado",
        TransferState::Cancelled => "Cancelado",
        TransferState::Failed => "Error",
    }
}

pub(in crate::iced_ui) fn transfer_control_button(
    label: &'static str,
    message: Message,
    palette: Palette,
    font_size: f32,
) -> Button<'static, Message> {
    Button::new(text(label).size(font_size).color(palette.text))
        .padding([4, 9])
        .style(move |_, status| button_style(palette, false, status))
        .on_press(message)
}

pub(in crate::iced_ui) fn color_channel_input(
    label: &'static str,
    value: &str,
    channel: usize,
    palette: Palette,
    font_size: f32,
) -> Element<'static, Message> {
    row![
        text(label).size(font_size - 1.0).color(palette.muted_text),
        text_input("", value)
            .size(font_size)
            .padding([4, 6])
            .width(54)
            .on_input(move |value| Message::AccentRgbChanged(channel, value)),
    ]
    .spacing(4)
    .align_y(Alignment::Center)
    .into()
}

pub(in crate::iced_ui) fn normalized_rect(start: Point, current: Point) -> Rectangle {
    let x = start.x.min(current.x);
    let y = start.y.min(current.y);
    Rectangle {
        x,
        y,
        width: (start.x - current.x).abs(),
        height: (start.y - current.y).abs(),
    }
}

pub(in crate::iced_ui) fn rects_intersect(left: Rectangle, right: Rectangle) -> bool {
    left.x < right.x + right.width
        && left.x + left.width > right.x
        && left.y < right.y + right.height
        && left.y + left.height > right.y
}

pub(in crate::iced_ui) fn inline_rename_input_id() -> Id {
    Id::new("inline-rename")
}

pub(in crate::iced_ui) fn address_input_id(pane: PaneId) -> Id {
    Id::new(match pane {
        PaneId::Primary => "address-primary",
        PaneId::Secondary => "address-secondary",
    })
}

pub(in crate::iced_ui) fn search_input_id(pane: PaneId) -> Id {
    Id::new(match pane {
        PaneId::Primary => "search-primary",
        PaneId::Secondary => "search-secondary",
    })
}

pub(in crate::iced_ui) fn focus_search_input_task(pane: PaneId) -> Task<Message> {
    iced::widget::operation::focus(search_input_id(pane))
}

pub(in crate::iced_ui) fn focus_address_input_task(
    pane: PaneId,
    select_end: usize,
) -> Task<Message> {
    let id = address_input_id(pane);
    iced::widget::operation::focus(id.clone())
        .chain(iced::widget::operation::select_range(id, select_end, 0))
}

pub(in crate::iced_ui) fn pane_scroll_id(pane: PaneId) -> Id {
    Id::new(match pane {
        PaneId::Primary => "file-scroll-primary",
        PaneId::Secondary => "file-scroll-secondary",
    })
}

pub(in crate::iced_ui) fn scroll_pane_to_top_task(pane: PaneId) -> Task<Message> {
    iced::widget::operation::snap_to(
        pane_scroll_id(pane),
        iced::widget::operation::RelativeOffset::START,
    )
}

pub(in crate::iced_ui) fn inline_rename_editor<'a>(
    value: &'a str,
    extension: Option<&'a str>,
    width: f32,
    font_size: f32,
    palette: Palette,
) -> Element<'a, Message> {
    // The editable control grows with its value, up to the available name
    // column. This keeps a preserved extension directly after a short name
    // instead of pinning it to the far edge of the column.
    let extension_width = extension
        .filter(|value| !value.is_empty())
        .map(|value| ((value.chars().count() + 1) as f32 * font_size * 0.58).ceil())
        .unwrap_or(0.0);
    let light_surface = palette.table_bg.r + palette.table_bg.g + palette.table_bg.b > 2.1;
    let maximum_input_width = (width - extension_width).max(28.0);
    let desired_input_width = (value.chars().count() as f32 * font_size * 0.58 + 14.0).ceil();
    let input_width =
        desired_input_width.clamp(44.0_f32.min(maximum_input_width), maximum_input_width);
    let rename_border = if light_surface {
        Color::from_rgb8(176, 181, 185)
    } else {
        Color::from_rgb8(94, 101, 105)
    };
    let rename_background = if light_surface {
        palette.input_bg
    } else {
        mix_color(palette.input_bg, palette.table_bg, 0.3)
    };
    let rename_value = if light_surface {
        palette.text
    } else {
        palette.accent_text
    };
    let input = text_input("", value)
        .id(inline_rename_input_id())
        .on_input(Message::RenameChanged)
        .on_submit(Message::ConfirmRename)
        .size(font_size)
        .padding([2, 6])
        .width(Length::Fixed(input_width))
        .style(move |_, _status| iced::widget::text_input::Style {
            background: rename_background.into(),
            border: border::rounded(3).color(rename_border).width(1),
            icon: palette.muted_text,
            placeholder: rename_value,
            value: rename_value,
            selection: hover_tint(palette),
        });

    let mut editor = row![input]
        .spacing(0)
        .align_y(Alignment::Center)
        .width(Length::Fixed(width.max(64.0)));

    if let Some(extension) = extension.filter(|value| !value.is_empty()) {
        editor = editor.push(
            text(format!(".{extension}"))
                .size(font_size)
                .color(palette.accent_text)
                .wrapping(iced::widget::text::Wrapping::None),
        );
    }

    container(editor)
        .width(Length::Fixed(width.max(64.0)))
        .height(Length::Fixed((font_size + 12.0).max(24.0)))
        .center_y(Length::Fill)
        .into()
}

pub(in crate::iced_ui) fn wrapped_inline_rename_editor<'a>(
    content: &'a text_editor::Content,
    extension: Option<&'a str>,
    width: f32,
    height: f32,
    font_size: f32,
    palette: Palette,
) -> Element<'a, Message> {
    let extension_width = extension
        .filter(|value| !value.is_empty())
        .map(|value| ((value.chars().count() + 1) as f32 * font_size * 0.58).ceil())
        .unwrap_or(0.0);
    let light_surface = palette.table_bg.r + palette.table_bg.g + palette.table_bg.b > 2.1;
    let input_width = (width - extension_width).max(28.0);
    let rename_border = if light_surface {
        Color::from_rgb8(176, 181, 185)
    } else {
        Color::from_rgb8(94, 101, 105)
    };
    let rename_background = if light_surface {
        palette.input_bg
    } else {
        mix_color(palette.input_bg, palette.table_bg, 0.3)
    };
    let rename_value = if light_surface {
        palette.text
    } else {
        palette.accent_text
    };
    let input = text_editor::TextEditor::new(content)
        .id(inline_rename_input_id())
        .on_action(Message::RenameEdited)
        .key_binding(|key_press| {
            if matches!(
                key_press.modified_key.as_ref(),
                keyboard::Key::Named(keyboard::key::Named::Enter)
            ) {
                Some(text_editor::Binding::Custom(Message::ConfirmRename))
            } else {
                text_editor::Binding::from_key_press(key_press)
            }
        })
        .size(font_size)
        .padding([2, 6])
        .wrapping(iced::widget::text::Wrapping::WordOrGlyph)
        .width(input_width)
        .height(Length::Fixed(height.max((font_size * 2.35).ceil())))
        .style(move |_, _status| text_editor::Style {
            background: rename_background.into(),
            border: border::rounded(3).color(rename_border).width(1),
            placeholder: rename_value,
            value: rename_value,
            selection: hover_tint(palette),
        });

    let mut editor = row![input]
        .spacing(0)
        .align_y(Alignment::Start)
        .width(Length::Fixed(width.max(64.0)));

    if let Some(extension) = extension.filter(|value| !value.is_empty()) {
        editor = editor.push(
            text(format!(".{extension}"))
                .size(font_size)
                .color(palette.accent_text)
                .wrapping(iced::widget::text::Wrapping::None),
        );
    }

    container(editor)
        .width(Length::Fixed(width.max(64.0)))
        .height(Length::Fixed(height.max((font_size * 2.35).ceil())))
        .into()
}

pub(in crate::iced_ui) fn rename_preserved_extension(entry: &FileEntry) -> Option<String> {
    if entry.kind.is_container() {
        return None;
    }
    entry
        .path
        .extension()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

pub(in crate::iced_ui) fn rename_edit_value(entry: &FileEntry, extension: Option<&str>) -> String {
    let file_name = entry
        .path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(entry.name.as_str());

    if let Some(extension) = extension.filter(|value| !value.is_empty()) {
        let suffix = format!(".{extension}");
        if let Some(stem) = file_name.strip_suffix(&suffix) {
            return stem.to_string();
        }
    }

    file_name.to_string()
}

pub(in crate::iced_ui) fn rename_target_name(value: &str, extension: Option<&str>) -> String {
    let base = value.trim();
    if base.is_empty() {
        return String::new();
    }

    if let Some(extension) = extension.filter(|value| !value.is_empty()) {
        let suffix = format!(".{extension}");
        if base.ends_with(&suffix) {
            return base.to_string();
        }
        return format!("{base}{suffix}");
    }

    base.to_string()
}

pub(in crate::iced_ui) fn focus_inline_rename_task(_select_end: usize) -> Task<Message> {
    let id = inline_rename_input_id();
    iced::widget::operation::focus(id.clone()).chain(
        // Ask the widget itself to select its complete editable value. Using
        // a calculated range left some UTF-8 names and clipped editors with
        // only a partial selection after focus was restored.
        iced::widget::operation::select_all(id),
    )
}
