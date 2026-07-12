use super::*;
use iced::widget::{column, row};
pub(in crate::iced_ui) fn icon_button<'a>(
    label: &'static str,
    message: Message,
    palette: Palette,
    selected: bool,
) -> Button<'a, Message> {
    title_icon_button(label, message, palette, selected)
}

pub(in crate::iced_ui) fn title_icon_button<'a>(
    label: &'static str,
    message: Message,
    palette: Palette,
    selected: bool,
) -> Button<'a, Message> {
    Button::new(title_bar_icon(label, icon_color(label, palette, selected)))
        .width(TITLE_BUTTON_WIDTH)
        .height(TITLE_BUTTON_HEIGHT)
        .padding(0)
        .on_press(message)
        .style(move |_, status| selected_button_style(palette, selected, status))
}

pub(in crate::iced_ui) fn window_close_button<'a>(palette: Palette) -> Button<'a, Message> {
    Button::new(title_bar_icon("x", palette.text))
        .width(TITLE_BUTTON_WIDTH)
        .height(TITLE_BUTTON_HEIGHT)
        .padding(0)
        .on_press(Message::WindowClose)
        .style(move |_, status| {
            let danger = matches!(status, button::Status::Hovered | button::Status::Pressed);
            let background = danger.then(|| Color::from_rgb8(227, 107, 114).into());
            button::Style {
                background,
                text_color: if danger { Color::WHITE } else { palette.text },
                border: border::rounded(border::top_right(WINDOW_RADIUS - WINDOW_BORDER_WIDTH)),
                ..button::Style::default()
            }
        })
}

pub(in crate::iced_ui) fn transfer_window_minimize_button<'a>(
    palette: Palette,
) -> Button<'a, Message> {
    native_window_minimize_button(Message::TransferWindowMinimize, palette)
}

pub(in crate::iced_ui) fn native_window_minimize_button<'a>(
    message: Message,
    palette: Palette,
) -> Button<'a, Message> {
    Button::new(title_bar_icon("min", palette.text))
        .width(34)
        .height(TRANSFER_WINDOW_TITLE_HEIGHT)
        .padding(0)
        .on_press(message)
        .style(move |_, status| button_style(palette, false, status))
}

pub(in crate::iced_ui) fn title_bar_icon<'a>(
    label: &'static str,
    color: Color,
) -> Element<'a, Message> {
    container(inline_icon(label, color, TITLE_ICON_SIZE))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

pub(in crate::iced_ui) fn tool_button<'a>(
    label: &'static str,
    message: Message,
    palette: Palette,
    selected: bool,
    compact: bool,
) -> Button<'a, Message> {
    let color = if selected {
        palette.accent_text
    } else {
        palette.text
    };
    let button = if compact {
        Button::new(
            container(inline_icon(tool_icon(label), color, TOOL_ICON_SIZE))
                .width(36)
                .height(36)
                .center(Length::Fill),
        )
        .width(40)
        .height(36)
        .padding(0)
    } else {
        Button::new(
            row![
                inline_icon(tool_icon(label), color, TOOL_ICON_SIZE),
                text(label).size(13).color(color)
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([8, 10])
    };
    button
        .on_press(message)
        .style(move |_, status| selected_button_style(palette, selected, status))
}

pub(in crate::iced_ui) fn context_quick_button<'a>(
    icon: &'static str,
    label: &'static str,
    command: ContextCommand,
    palette: Palette,
    enabled: bool,
) -> Element<'a, Message> {
    let color = if enabled {
        mix_color(palette.text, palette.muted_text, 0.28)
    } else {
        translucent_color(palette.muted_text, 0.44)
    };
    let content = column![
        inline_icon(icon, color, 20.0),
        text(label)
            .size(11.0)
            .color(color)
            .align_x(Horizontal::Center)
    ]
    .spacing(2)
    .align_x(Alignment::Center);

    let button = Button::new(
        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .center(Length::Fill),
    )
    .width(Length::Fill)
    .height(48)
    .padding(0);
    let button = if enabled {
        button.on_press(Message::RunContextCommand(command))
    } else {
        button
    };
    button
        .style(move |_, status| context_button_style(palette, status))
        .into()
}

pub(in crate::iced_ui) fn context_menu_row<'a>(
    icon: &'static str,
    label: &'static str,
    trailing: Option<ContextMenuTrailing>,
    command: ContextCommand,
    palette: Palette,
) -> Element<'a, Message> {
    context_menu_dynamic_row(icon, label.to_string(), trailing, command, palette)
}

pub(in crate::iced_ui) fn context_menu_dynamic_row<'a>(
    icon: &'static str,
    label: String,
    trailing: Option<ContextMenuTrailing>,
    command: ContextCommand,
    palette: Palette,
) -> Element<'a, Message> {
    let trailing: Element<'a, Message> = match trailing {
        Some(ContextMenuTrailing::Text(label)) => {
            text(label).size(12.0).color(palette.muted_text).into()
        }
        Some(ContextMenuTrailing::Icon(icon)) => inline_icon(icon, palette.muted_text, 13.0),
        None => Space::new().width(0).into(),
    };
    let content = row![
        inline_icon(icon, palette.muted_text, 18.0),
        text(label)
            .size(13.0)
            .color(palette.text)
            .wrapping(iced::widget::text::Wrapping::None)
            .width(Length::Fill),
        trailing,
    ]
    .spacing(12)
    .align_y(Alignment::Center)
    .height(Length::Fill);

    Button::new(
        container(content)
            .height(Length::Fill)
            .center_y(Length::Fill),
    )
    .width(Length::Fill)
    .height(34)
    .padding([0, 10])
    .on_press(Message::RunContextCommand(command))
    .style(move |_, status| context_button_style(palette, status))
    .into()
}

pub(in crate::iced_ui) fn context_separator<'a>(palette: Palette) -> Element<'a, Message> {
    container(Space::new())
        .height(1)
        .width(Length::Fill)
        .style(move |_| {
            container::Style::default().background(translucent_color(palette.border, 0.62))
        })
        .into()
}

pub(in crate::iced_ui) fn menu_choice_button(
    label: &'static str,
    active: bool,
    message: Message,
    palette: Palette,
    font_size: f32,
) -> Element<'static, Message> {
    let color = if active {
        palette.accent_text
    } else {
        palette.text
    };
    Button::new(
        container(
            row![
                text(if active { "✓" } else { "" })
                    .size(font_size)
                    .color(color)
                    .width(18),
                text(label).size(font_size).color(color).width(Length::Fill),
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
    .style(move |_, status| selected_button_style(palette, active, status))
    .into()
}

pub(in crate::iced_ui) fn tool_icon(label: &str) -> &'static str {
    match label {
        "Nuevo" | "New" => "add",
        "Pegar" | "Paste" => "paste",
        "Copiar" | "Copy" => "copy",
        "Cortar" | "Cut" => "cut",
        "Deshacer" | "Undo" => "undo",
        "Renombrar" | "Rename" => "rename",
        "Eliminar" | "Delete" => "trash",
        "Comprimir" | "Compress" => "archive",
        "Agrupar" | "Group" => "group",
        "Vista previa" | "Preview" => "preview",
        "Claro" | "Oscuro" | "Light" | "Dark" | "Sistema" | "System" => "theme",
        _ => "dot",
    }
}

pub(in crate::iced_ui) fn inline_icon<'a>(
    label: &'static str,
    color: Color,
    size: f32,
) -> Element<'a, Message> {
    // Every app glyph comes from the same 24×24 SVG family. Render it on an
    // integral, slightly larger pixel grid: this avoids fractional sampling
    // (the source of the soft-looking strokes) while keeping the original
    // lightweight line weight intact.
    const ICON_OPTICAL_SCALE: f32 = 1.12;
    let size = (size * ICON_OPTICAL_SCALE).round().max(1.0);
    let preserves_native_colors = matches!(label, "pc" | "printer");
    let icon = svg::Svg::new(svg::Handle::from_memory(icon_svg(label)))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |_, _| svg::Style {
            color: (!preserves_native_colors).then_some(color),
        });

    container(icon)
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .into()
}

pub(in crate::iced_ui) fn load_iced_image_task(job: IcedImageJob) -> Task<Message> {
    Task::perform(async move { load_iced_image(job) }, Message::ImageLoaded)
}

pub(in crate::iced_ui) fn load_pdf_preview_page_task(
    pane: PaneId,
    path: PathBuf,
    page_index: usize,
) -> Task<Message> {
    Task::perform(
        async move {
            let rendered = thumbnail_data::render_pdf_preview_page(&path, page_index)
                .map(|(page_count, image)| (page_count, iced_rgba_from_native(image)));
            PdfPreviewLoadResult {
                pane,
                path,
                page_index,
                page_count: rendered.as_ref().map(|(page_count, _)| *page_count),
                image: rendered.and_then(|(_, image)| image),
            }
        },
        Message::PdfPreviewPageLoaded,
    )
}

pub(in crate::iced_ui) fn load_iced_image(job: IcedImageJob) -> IcedImageLoadResult {
    match job {
        IcedImageJob::Thumbnail {
            path,
            max_bytes,
            allow_default_resource,
        } => {
            let image = if explorer::is_portable_path(&path) {
                thumbnail_data::load_portable_thumbnail_image(
                    &path,
                    max_bytes,
                    allow_default_resource,
                )
            } else {
                thumbnail_data::load_thumbnail_image_with_fallback(&path)
            };
            IcedImageLoadResult {
                key: IcedImageKey::Thumbnail(path),
                image: image.and_then(iced_rgba_from_native),
            }
        }
        IcedImageJob::Preview { path } => IcedImageLoadResult {
            key: IcedImageKey::Preview(path.clone()),
            image: thumbnail_data::load_preview_image(&path).and_then(iced_rgba_from_native),
        },
        IcedImageJob::NativeIcon {
            cache_key,
            path,
            is_directory,
            size,
        } => {
            let image = thumbnail_data::load_native_icon_image(&path, is_directory, size);
            IcedImageLoadResult {
                key: IcedImageKey::NativeIcon(cache_key),
                image: image.and_then(iced_rgba_from_native),
            }
        }
    }
}

pub(in crate::iced_ui) fn iced_rgba_from_native(
    image: crate::platform::NativeIconImage,
) -> Option<IcedRgbaImage> {
    let width = u32::try_from(image.width).ok()?;
    let height = u32::try_from(image.height).ok()?;
    Some(IcedRgbaImage {
        width,
        height,
        rgba: image.rgba,
    })
}

pub(in crate::iced_ui) fn native_icon_request_for_entry(
    entry: &FileEntry,
) -> Option<(PathBuf, PathBuf, bool)> {
    if explorer::is_virtual_path(&entry.path) {
        return thumbnail_data::virtual_native_icon_request(entry);
    }

    Some((
        thumbnail_data::native_entry_icon_cache_key(entry),
        entry.path.clone(),
        matches!(entry.kind, EntryKind::Folder | EntryKind::Drive),
    ))
}

pub(in crate::iced_ui) fn fallback_icon_label(entry: &FileEntry) -> &'static str {
    match &entry.kind {
        EntryKind::Drive if entry.drive_kind == Some(DriveKind::NetworkPrinter) => "printer",
        EntryKind::Drive => "pc",
        EntryKind::Folder => "folder",
        EntryKind::Symlink => "lnk",
        EntryKind::File | EntryKind::Other => "file",
    }
}

pub(in crate::iced_ui) fn icon_color(
    label: &'static str,
    palette: Palette,
    selected: bool,
) -> Color {
    if matches!(label, "folder" | "dir") {
        palette.folder
    } else if selected {
        palette.accent_text
    } else {
        palette.text
    }
}

pub(in crate::iced_ui) fn icon_svg(label: &'static str) -> &'static [u8] {
    match label {
        "menu" => ICON_MENU,
        "side" => ICON_SIDEBAR,
        "split" => ICON_SPLIT,
        "min" => ICON_MIN,
        "max" => ICON_MAX,
        "restore" => ICON_RESTORE,
        "add" => ICON_ADD,
        "back" => ICON_BACK,
        "next" => ICON_NEXT,
        "up" => ICON_UP,
        "refresh" => ICON_REFRESH,
        "folder" => ICON_FOLDER,
        "dir" => ICON_FOLDER,
        "folder-stack" => ICON_FOLDER_STACK,
        "places" => ICON_PLACES,
        "bookmark" => ICON_BOOKMARK,
        "storage" => ICON_STORAGE,
        "file" => ICON_FILE,
        "pc" => ICON_PC,
        "printer" => ICON_PRINTER,
        "lnk" => ICON_LINK,
        "rec" => ICON_RECENT,
        "net" => ICON_NETWORK,
        "copy" => ICON_COPY,
        "paste" => ICON_PASTE,
        "cut" => ICON_CUT,
        "undo" => ICON_UNDO,
        "rename" => ICON_RENAME,
        "trash" => ICON_TRASH,
        "delete-forever" => ICON_DELETE_FOREVER,
        "archive" => ICON_ARCHIVE,
        "group" => ICON_GROUP,
        "preview" => ICON_PREVIEW,
        "open" => ICON_OPEN,
        "open-with" => ICON_OPEN_WITH,
        "terminal" => ICON_TERMINAL,
        "properties" => ICON_PROPERTIES,
        "settings" => ICON_SETTINGS,
        "keyboard" => ICON_KEYBOARD,
        "theme" => ICON_THEME,
        "view-details" => ICON_VIEW_DETAILS,
        "view-list" => ICON_VIEW_LIST,
        "view-tiles" => ICON_VIEW_TILES,
        "view-grid-small" => ICON_VIEW_GRID_SMALL,
        "view-grid-large" => ICON_VIEW_GRID_LARGE,
        "chev-right" => ICON_CHEVRON_RIGHT,
        "chev-down" => ICON_CHEVRON_DOWN,
        "eye" => ICON_EYE,
        "eye-off" => ICON_EYE_OFF,
        "x" => ICON_X,
        _ => ICON_DOT,
    }
}

const ICON_MENU: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M5 7h14M5 12h14M5 17h14" fill="none" stroke="#000" stroke-width="1.7" stroke-linecap="round"/></svg>"##;
const ICON_SIDEBAR: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><rect x="5" y="6" width="14" height="12" rx="2" fill="none" stroke="#000" stroke-width="1.55"/><path d="M10 6v12" fill="none" stroke="#000" stroke-width="1.55"/></svg>"##;
const ICON_SPLIT: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><rect x="5" y="6" width="14" height="12" rx="2" fill="none" stroke="#000" stroke-width="1.55"/><path d="M12 6v12" fill="none" stroke="#000" stroke-width="1.55"/></svg>"##;
const ICON_MIN: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M8 12h8" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round"/></svg>"##;
const ICON_MAX: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><rect x="8" y="8" width="8" height="8" rx="1" fill="none" stroke="#000" stroke-width="1.55"/></svg>"##;
const ICON_RESTORE: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><rect x="8" y="8" width="9" height="9" rx="1" fill="none" stroke="#000" stroke-width="1.6"/><path d="M6 14V6h8" fill="none" stroke="#000" stroke-width="1.6" stroke-linejoin="round"/></svg>"##;
const ICON_ADD: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M12 6v12M6 12h12" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round"/></svg>"##;
const ICON_EYE: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M3.5 12s3-5 8.5-5 8.5 5 8.5 5-3 5-8.5 5-8.5-5-8.5-5Z" fill="none" stroke="#000" stroke-width="1.65" stroke-linejoin="round"/><circle cx="12" cy="12" r="2.25" fill="none" stroke="#000" stroke-width="1.65"/></svg>"##;
const ICON_EYE_OFF: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M4 4l16 16M9.9 7.25A9.8 9.8 0 0 1 12 7c5.5 0 8.5 5 8.5 5a14.6 14.6 0 0 1-3.1 3.45M6.1 8.05C4.35 9.5 3.5 12 3.5 12s3 5 8.5 5c.7 0 1.36-.08 1.98-.23" fill="none" stroke="#000" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round"/><path d="M9.9 9.9a3 3 0 0 0 4.2 4.2" fill="none" stroke="#000" stroke-width="1.65" stroke-linecap="round"/></svg>"##;
const ICON_BACK: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M19 12H6m5-5-5 5 5 5" fill="none" stroke="#000" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;
const ICON_NEXT: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M5 12h13m-5-5 5 5-5 5" fill="none" stroke="#000" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;
const ICON_UP: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M12 19V6m-5 5 5-5 5 5" fill="none" stroke="#000" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;
const ICON_REFRESH: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M18 9a6 6 0 0 0-10.5-3.7L6 7m0 0V3m0 4h4M6 15a6 6 0 0 0 10.5 3.7L18 17m0 0v4m0-4h-4" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;
const ICON_FOLDER: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M3 7.5c0-1.1.9-2 2-2h5l2 2h7c1.1 0 2 .9 2 2v7c0 1.1-.9 2-2 2H5c-1.1 0-2-.9-2-2z" fill="#000"/><path d="M3 10h18" fill="none" stroke="#000" stroke-width="1.2" opacity=".35"/></svg>"##;
const ICON_FOLDER_STACK: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M8 3.4c0-.65.52-1.18 1.18-1.18h2.72l1.32 1.32h2.94c.65 0 1.18.53 1.18 1.18v3.68c0 .65-.53 1.18-1.18 1.18H9.18C8.52 9.6 8 9.07 8 8.42z" fill="#000" opacity=".52"/><path d="M2.25 12.15c0-.65.53-1.18 1.18-1.18h2.72l1.32 1.32h2.94c.65 0 1.18.53 1.18 1.18v5.08c0 .65-.53 1.18-1.18 1.18H3.43c-.65 0-1.18-.53-1.18-1.18z" fill="#000"/><path d="M12.35 12.15c0-.65.53-1.18 1.18-1.18h2.72l1.32 1.32h2.94c.65 0 1.18.53 1.18 1.18v5.08c0 .65-.53 1.18-1.18 1.18h-6.98c-.65 0-1.18-.53-1.18-1.18z" fill="#000"/></svg>"##;
const ICON_PLACES: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M3.5 8.2c0-1.1.9-2 2-2h4.7l1.7 1.8h6.6c1.1 0 2 .9 2 2v6.8c0 1.1-.9 2-2 2h-13c-1.1 0-2-.9-2-2z" fill="#000"/></svg>"##;
const ICON_BOOKMARK: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M7 4.5h10v15l-5-3-5 3z" fill="none" stroke="#000" stroke-width="1.8" stroke-linejoin="round"/></svg>"##;
const ICON_STORAGE: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><ellipse cx="12" cy="6" rx="7" ry="2.8" fill="none" stroke="#000" stroke-width="1.7"/><path d="M5 6v9.5c0 1.5 3.1 2.8 7 2.8s7-1.3 7-2.8V6M5 11c0 1.5 3.1 2.8 7 2.8s7-1.3 7-2.8" fill="none" stroke="#000" stroke-width="1.7"/></svg>"##;
const ICON_FILE: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M7 3.5h7l4 4V20a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1V4.5a1 1 0 0 1 1-1z" fill="none" stroke="#000" stroke-width="1.6" stroke-linejoin="round"/><path d="M14 3.5V8h4" fill="none" stroke="#000" stroke-width="1.6" stroke-linejoin="round"/></svg>"##;
const ICON_PC: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><rect x="1.1" y="2.15" width="21.8" height="15.35" rx="1.55" fill="#294b55" stroke="#f7fcff" stroke-width=".42" stroke-opacity=".72"/><rect x="1.5" y="2.55" width="21" height="14.55" rx="1.2" fill="none" stroke="#132f38" stroke-width=".72"/><rect x="2.85" y="3.65" width="18.3" height="11.75" rx=".38" fill="#08bfe8"/><path d="M2.85 3.65h18.3V15.4z" fill="#087fa7" opacity=".2"/><path d="M10.45 17.4h3.1v2.3h3.4c.6 0 1.1.48 1.1 1.08v.47H5.95v-.47c0-.6.5-1.08 1.1-1.08h3.4z" fill="#294b55" stroke="#f7fcff" stroke-width=".35" stroke-opacity=".6"/><path d="M5.95 21.25h12.1" fill="none" stroke="#132f38" stroke-width=".75" stroke-linecap="round"/></svg>"##;
const ICON_PRINTER: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M7 3.2h10v5.1H7z" fill="#d9eef5" stroke="#031923" stroke-width="1.15" stroke-linejoin="round"/><rect x="3" y="7.2" width="18" height="9.4" rx="2" fill="#16779c" stroke="#031923" stroke-width="1.2"/><circle cx="18.1" cy="10.3" r=".75" fill="#8ce1f5"/><path d="M6.4 13.2h11.2v7.6H6.4z" fill="#eaf7fa" stroke="#031923" stroke-width="1.15" stroke-linejoin="round"/><path d="M8.4 16h7.2M8.4 18.2h5.2" fill="none" stroke="#52717d" stroke-width="1" stroke-linecap="round"/></svg>"##;
const ICON_LINK: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M10 8.5 11.5 7a4 4 0 0 1 5.7 5.7l-2 2a4 4 0 0 1-5.7 0" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round"/><path d="m14 15.5-1.5 1.5a4 4 0 0 1-5.7-5.7l2-2a4 4 0 0 1 5.7 0" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round"/></svg>"##;
const ICON_RECENT: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><circle cx="12" cy="12" r="7.5" fill="none" stroke="#000" stroke-width="1.7"/><path d="M12 7.5V12l3 2" fill="none" stroke="#000" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;
const ICON_NETWORK: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><circle cx="12" cy="5.5" r="2" fill="none" stroke="#000" stroke-width="1.7"/><circle cx="6" cy="18.5" r="2" fill="none" stroke="#000" stroke-width="1.7"/><circle cx="18" cy="18.5" r="2" fill="none" stroke="#000" stroke-width="1.7"/><path d="M12 7.5v4.5M12 12 7.5 17M12 12l4.5 5" fill="none" stroke="#000" stroke-width="1.7" stroke-linecap="round"/></svg>"##;
const ICON_COPY: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><rect x="8" y="8" width="10" height="11" rx="1.5" fill="none" stroke="#000" stroke-width="1.7"/><rect x="5" y="5" width="10" height="11" rx="1.5" fill="none" stroke="#000" stroke-width="1.7" opacity=".85"/></svg>"##;
const ICON_PASTE: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M9 5h6l1 2h2v13H6V7h2z" fill="none" stroke="#000" stroke-width="1.7" stroke-linejoin="round"/><path d="M9 5.5h6M9 11h6M9 15h5" fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round"/></svg>"##;
const ICON_CUT: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><circle cx="6" cy="7" r="2.2" fill="none" stroke="#000" stroke-width="1.7"/><circle cx="6" cy="17" r="2.2" fill="none" stroke="#000" stroke-width="1.7"/><path d="M8 8.5 18 18M8 15.5 18 6" fill="none" stroke="#000" stroke-width="1.7" stroke-linecap="round"/></svg>"##;
const ICON_UNDO: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M9 8 5 12l4 4M5.5 12H14a5 5 0 0 1 5 5" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;
const ICON_RENAME: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="m5 16 1 3 3-1 8.5-8.5-3-3z" fill="none" stroke="#000" stroke-width="1.7" stroke-linejoin="round"/><path d="M13.5 7.5 16.5 10.5M5 20h14" fill="none" stroke="#000" stroke-width="1.7" stroke-linecap="round"/></svg>"##;
const ICON_TRASH: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M7 8h10l-.8 11H7.8zM9 8V5h6v3M5 8h14" fill="none" stroke="#000" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;
const ICON_DELETE_FOREVER: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M7 8h10l-.8 11H7.8zM9 8V5h6v3M5 8h14" fill="none" stroke="#000" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/><path d="m9 12 6 6M15 12l-6 6" fill="none" stroke="#000" stroke-width="1.4" stroke-linecap="round"/></svg>"##;
const ICON_ARCHIVE: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><rect x="5" y="5" width="14" height="14" rx="2" fill="none" stroke="#000" stroke-width="1.7"/><path d="M9 5v14M9 8h3M9 11h3M9 14h3" fill="none" stroke="#000" stroke-width="1.5" stroke-linecap="round"/></svg>"##;
const ICON_GROUP: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M6 7h12M6 12h12M6 17h12" fill="none" stroke="#000" stroke-width="1.7" stroke-linecap="round"/><path d="M3.5 7h.1M3.5 12h.1M3.5 17h.1" fill="none" stroke="#000" stroke-width="2.6" stroke-linecap="round"/></svg>"##;
const ICON_PREVIEW: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><rect x="4" y="6" width="16" height="12" rx="2" fill="none" stroke="#000" stroke-width="1.7"/><path d="M12 6v12" fill="none" stroke="#000" stroke-width="1.5"/><path d="M7 10h2M7 13h2" fill="none" stroke="#000" stroke-width="1.5" stroke-linecap="round"/></svg>"##;
const ICON_OPEN: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M5 7.5h5l2 2h7v7.5a2 2 0 0 1-2 2H6.5a2 2 0 0 1-2-2V8.5a1 1 0 0 1 1-1z" fill="none" stroke="#000" stroke-width="1.7" stroke-linejoin="round"/><path d="m10 14 3-3 3 3M13 11v6" fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;
const ICON_OPEN_WITH: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><rect x="5" y="5" width="5" height="5" rx="1" fill="none" stroke="#000" stroke-width="1.6"/><rect x="14" y="5" width="5" height="5" rx="1" fill="none" stroke="#000" stroke-width="1.6"/><rect x="5" y="14" width="5" height="5" rx="1" fill="none" stroke="#000" stroke-width="1.6"/><path d="M15 15h4M17 13v4" fill="none" stroke="#000" stroke-width="1.7" stroke-linecap="round"/></svg>"##;
const ICON_TERMINAL: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><rect x="4" y="6" width="16" height="12" rx="2" fill="none" stroke="#000" stroke-width="1.7"/><path d="m7 10 3 2-3 2M12 15h5" fill="none" stroke="#000" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;
const ICON_PROPERTIES: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><circle cx="12" cy="12" r="8" fill="none" stroke="#000" stroke-width="1.7"/><path d="M12 10.5v5M12 7.7h.1" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round"/></svg>"##;
const ICON_SETTINGS: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><circle cx="12" cy="12" r="3" fill="none" stroke="#000" stroke-width="1.7"/><path d="M12 4.5v2M12 17.5v2M19.5 12h-2M6.5 12h-2M17.3 6.7l-1.4 1.4M8.1 15.9l-1.4 1.4M17.3 17.3l-1.4-1.4M8.1 8.1 6.7 6.7" fill="none" stroke="#000" stroke-width="1.7" stroke-linecap="round"/></svg>"##;
const ICON_KEYBOARD: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><rect x="3.5" y="6" width="17" height="12" rx="2" fill="none" stroke="#000" stroke-width="1.55"/><path d="M6.5 9.5h.1M9.5 9.5h.1M12.5 9.5h.1M15.5 9.5h.1M6.5 12.5h.1M9.5 12.5h.1M12.5 12.5h.1M15.5 12.5h.1M8 15.5h8" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round"/></svg>"##;
const ICON_THEME: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><circle cx="12" cy="12" r="7" fill="none" stroke="#000" stroke-width="1.7"/><path d="M12 5a7 7 0 0 1 0 14z" fill="#000"/></svg>"##;
const ICON_VIEW_DETAILS: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M8 7h11M8 12h11M8 17h11" fill="none" stroke="#000" stroke-width="1.7" stroke-linecap="round"/><path d="M4.5 7h.1M4.5 12h.1M4.5 17h.1" fill="none" stroke="#000" stroke-width="2.5" stroke-linecap="round"/></svg>"##;
const ICON_VIEW_LIST: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M7 8h12M7 12h12M7 16h12" fill="none" stroke="#000" stroke-width="1.7" stroke-linecap="round"/><path d="M4 8h.1M4 12h.1M4 16h.1" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round"/></svg>"##;
const ICON_VIEW_TILES: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><rect x="4" y="5" width="6" height="6" rx="1" fill="#000"/><rect x="4" y="13" width="6" height="6" rx="1" fill="#000"/><path d="M13 7h7M13 11h5M13 15h7M13 19h5" fill="none" stroke="#000" stroke-width="1.5" stroke-linecap="round"/></svg>"##;
const ICON_VIEW_GRID_SMALL: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><rect x="5" y="5" width="4" height="4" rx=".8" fill="#000"/><rect x="10" y="5" width="4" height="4" rx=".8" fill="#000"/><rect x="15" y="5" width="4" height="4" rx=".8" fill="#000"/><rect x="5" y="10" width="4" height="4" rx=".8" fill="#000"/><rect x="10" y="10" width="4" height="4" rx=".8" fill="#000"/><rect x="15" y="10" width="4" height="4" rx=".8" fill="#000"/><rect x="5" y="15" width="4" height="4" rx=".8" fill="#000"/><rect x="10" y="15" width="4" height="4" rx=".8" fill="#000"/><rect x="15" y="15" width="4" height="4" rx=".8" fill="#000"/></svg>"##;
const ICON_VIEW_GRID_LARGE: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><rect x="5" y="5" width="6" height="6" rx="1" fill="#000"/><rect x="13" y="5" width="6" height="6" rx="1" fill="#000"/><rect x="5" y="13" width="6" height="6" rx="1" fill="#000"/><rect x="13" y="13" width="6" height="6" rx="1" fill="#000"/></svg>"##;
const ICON_CHEVRON_RIGHT: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="m9 6 6 6-6 6" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;
const ICON_CHEVRON_DOWN: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="m6 9 6 6 6-6" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;
const ICON_X: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="m7 7 10 10M17 7 7 17" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round"/></svg>"##;
const ICON_DOT: &[u8] = br##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><circle cx="12" cy="12" r="2" fill="#000"/></svg>"##;

pub(in crate::iced_ui) fn button_style(
    palette: Palette,
    selected: bool,
    status: button::Status,
) -> button::Style {
    selected_button_style(palette, selected, status)
}

pub(in crate::iced_ui) fn context_button_style(
    palette: Palette,
    status: button::Status,
) -> button::Style {
    let background = if matches!(status, button::Status::Hovered | button::Status::Pressed) {
        Some(hover_tint(palette).into())
    } else {
        None
    };
    button::Style {
        background,
        text_color: palette.text,
        border: border::rounded(4),
        ..button::Style::default()
    }
}

pub(in crate::iced_ui) fn selected_button_style(
    palette: Palette,
    selected: bool,
    status: button::Status,
) -> button::Style {
    let background = if selected {
        Some(accent_gradient(palette).into())
    } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
        Some(hover_tint(palette).into())
    } else {
        Some(Color::TRANSPARENT.into())
    };
    button::Style {
        background,
        text_color: if selected {
            palette.accent_text
        } else {
            palette.text
        },
        border: border::rounded(4),
        ..button::Style::default()
    }
}

pub(in crate::iced_ui) fn row_background_style(
    palette: Palette,
    selected: bool,
) -> container::Style {
    let style = container::Style::default().border(border::rounded(4));
    if selected {
        style.background(accent_gradient(palette))
    } else {
        style.background(Color::TRANSPARENT)
    }
}

pub(in crate::iced_ui) fn tab_body_style(
    palette: Palette,
    active: bool,
    focused_active: bool,
    dragging: bool,
) -> container::Style {
    let background = if dragging {
        Some(translucent_accent_gradient(palette, 0.18).into())
    } else if focused_active {
        Some(palette.page_bg.into())
    } else if active {
        Some(mix_color(palette.title_bg, palette.page_bg, 0.32).into())
    } else {
        Some(Color::TRANSPARENT.into())
    };
    container::Style {
        background,
        border: border::rounded(4)
            .color(if dragging {
                palette.accent
            } else if focused_active {
                palette.strong_border
            } else {
                translucent_color(palette.border, 0.72)
            })
            .width(1),
        ..container::Style::default()
    }
}

#[derive(Clone, Copy)]
pub(in crate::iced_ui) struct Palette {
    pub(in crate::iced_ui) page_bg: Color,
    pub(in crate::iced_ui) table_bg: Color,
    pub(in crate::iced_ui) title_bg: Color,
    pub(in crate::iced_ui) sidebar_bg: Color,
    pub(in crate::iced_ui) menu_bg: Color,
    pub(in crate::iced_ui) overlay_bg: Color,
    pub(in crate::iced_ui) overlay_title_bg: Color,
    pub(in crate::iced_ui) input_bg: Color,
    pub(in crate::iced_ui) header_bg: Color,
    pub(in crate::iced_ui) hover: Color,
    pub(in crate::iced_ui) border: Color,
    pub(in crate::iced_ui) strong_border: Color,
    pub(in crate::iced_ui) text: Color,
    pub(in crate::iced_ui) muted_text: Color,
    pub(in crate::iced_ui) accent: Color,
    pub(in crate::iced_ui) accent_text: Color,
    pub(in crate::iced_ui) folder: Color,
}

impl Palette {
    pub(in crate::iced_ui) fn with_opacity(mut self, opacity: f32) -> Self {
        let opacity = opacity.clamp(0.0, 1.0);
        for color in [
            &mut self.page_bg,
            &mut self.table_bg,
            &mut self.title_bg,
            &mut self.sidebar_bg,
            &mut self.menu_bg,
            &mut self.overlay_bg,
            &mut self.overlay_title_bg,
            &mut self.input_bg,
            &mut self.header_bg,
            &mut self.hover,
            &mut self.border,
            &mut self.strong_border,
            &mut self.text,
            &mut self.muted_text,
            &mut self.accent,
            &mut self.accent_text,
            &mut self.folder,
        ] {
            color.a *= opacity;
        }
        self
    }

    pub(in crate::iced_ui) fn from_config(config: &AppConfig, dark_theme: bool) -> Self {
        let accent = Color::from_rgb8(
            config.accent_color[0],
            config.accent_color[1],
            config.accent_color[2],
        );
        let mut palette = if dark_theme {
            Self {
                page_bg: Color::from_rgb8(23, 28, 30),
                table_bg: Color::from_rgb8(18, 22, 24),
                title_bg: Color::from_rgb8(31, 37, 39),
                sidebar_bg: Color::from_rgb8(22, 27, 29),
                menu_bg: Color::from_rgb8(22, 27, 29),
                overlay_bg: Color::from_rgb8(22, 27, 29),
                overlay_title_bg: Color::from_rgb8(31, 37, 39),
                input_bg: Color::from_rgb8(25, 30, 32),
                header_bg: Color::from_rgb8(34, 40, 43),
                hover: Color::from_rgb8(53, 63, 67),
                border: Color::from_rgb8(59, 70, 74),
                strong_border: Color::from_rgb8(80, 94, 99),
                text: Color::from_rgb8(222, 229, 232),
                muted_text: Color::from_rgb8(150, 164, 169),
                accent,
                accent_text: Color::WHITE,
                folder: Color::from_rgb8(244, 196, 60),
            }
        } else {
            Self {
                page_bg: Color::from_rgb8(250, 250, 252),
                table_bg: Color::from_rgb8(255, 255, 255),
                title_bg: Color::from_rgb8(246, 246, 248),
                // Cool neutral gray for the light sidebar. Keeping red and
                // green balanced avoids the warm/yellow cast against white
                // content, while the tiny blue lift matches GNOME's calm
                // Nautilus surface.
                sidebar_bg: Color::from_rgb8(246, 246, 248),
                menu_bg: Color::from_rgb8(253, 255, 255),
                overlay_bg: Color::from_rgb8(253, 255, 255),
                overlay_title_bg: Color::from_rgb8(246, 246, 248),
                input_bg: Color::from_rgb8(255, 255, 255),
                header_bg: Color::from_rgb8(244, 244, 246),
                hover: Color::from_rgb8(220, 231, 233),
                border: Color::from_rgb8(208, 220, 223),
                strong_border: Color::from_rgb8(187, 204, 209),
                text: Color::from_rgb8(34, 47, 52),
                muted_text: Color::from_rgb8(93, 108, 114),
                accent,
                accent_text: Color::WHITE,
                folder: Color::from_rgb8(245, 198, 55),
            }
        };
        if config.vibrancy_active {
            let intensity = config.vibrancy_intensity.clamp(15, 90) as f32 / 100.0;
            // Intensity controls how much of the native/compositor backdrop is
            // allowed through. The previous scale left the window mostly
            // opaque even at 90%, making KWin's blur impossible to perceive.
            // Keep a readable floor while making the high end visibly glassy.
            let alpha = (1.0 - intensity * 0.68).clamp(0.32, 0.9);
            for surface in [
                &mut palette.page_bg,
                &mut palette.table_bg,
                &mut palette.title_bg,
                &mut palette.sidebar_bg,
                &mut palette.header_bg,
            ] {
                surface.a = alpha;
            }
            // Cached in-window backdrops need a little more transparency than
            // a native window surface; otherwise their blurred texture would
            // be imperceptible. Dialogs remain slightly denser for legibility.
            let panel_alpha = (alpha + 0.34).clamp(0.74, 0.88);
            let dialog_alpha = (alpha + 0.42).clamp(0.80, 0.92);
            palette.menu_bg.a = panel_alpha;
            palette.input_bg.a = panel_alpha;
            palette.overlay_bg.a = dialog_alpha;
            palette.overlay_title_bg.a = dialog_alpha;
        }
        palette
    }
}
