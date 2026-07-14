use super::*;

const APP_ICON_PNG: &[u8] = include_bytes!("../../../assets/icons/appicon.png");

pub(in crate::iced_ui) fn app_window_icon() -> Option<window::Icon> {
    let icon = image::load_from_memory(APP_ICON_PNG)
        .ok()?
        .thumbnail(256, 256)
        .to_rgba8();
    let (width, height) = icon.dimensions();
    window::icon::from_rgba(icon.into_raw(), width, height).ok()
}

pub(in crate::iced_ui) fn app_icon_image_handle() -> iced_image::Handle {
    static HANDLE: std::sync::OnceLock<iced_image::Handle> = std::sync::OnceLock::new();
    HANDLE
        .get_or_init(|| {
            let icon = image::load_from_memory(APP_ICON_PNG)
                .expect("embedded application icon must be a valid PNG")
                .resize_exact(192, 192, image::imageops::FilterType::Lanczos3)
                .to_rgba8();
            iced_image::Handle::from_rgba(192, 192, icon.into_raw())
        })
        .clone()
}

pub(in crate::iced_ui) fn main_window_settings(size: Size, maximized: bool) -> window::Settings {
    window::Settings {
        size: Size::new(size.width.max(920.0), size.height.max(560.0)),
        maximized,
        min_size: Some(Size::new(920.0, 560.0)),
        decorations: false,
        resizable: true,
        transparent: true,
        // The application owns the shutdown sequence so borrowed native
        // resources can be released before winit destroys the window.
        exit_on_close_request: false,
        icon: app_window_icon(),
        #[cfg(target_os = "linux")]
        platform_specific: window::settings::PlatformSpecific {
            application_id: crate::platform::LINUX_APPLICATION_ID.into(),
            ..window::settings::PlatformSpecific::default()
        },
        ..window::Settings::default()
    }
}

pub(in crate::iced_ui) fn progress_card_list_height(item_count: usize) -> f32 {
    let count = item_count.max(1) as f32;
    count * TRANSFER_CARD_HEIGHT + (count - 1.0) * TRANSFER_CARD_GAP
}

pub(in crate::iced_ui) fn progress_visible_card_list_height(item_count: usize) -> f32 {
    progress_card_list_height(item_count.min(TRANSFER_WINDOW_VISIBLE_CARD_LIMIT as usize))
}

pub(in crate::iced_ui) fn transfer_window_size_for_item_count(item_count: usize) -> Size {
    Size::new(
        TRANSFER_WINDOW_WIDTH,
        (TRANSFER_WINDOW_CARD_ONLY_CHROME_HEIGHT + progress_visible_card_list_height(item_count))
            .clamp(
                TRANSFER_WINDOW_CARD_ONLY_MIN_HEIGHT,
                TRANSFER_WINDOW_CARD_ONLY_MAX_HEIGHT,
            ),
    )
}

pub(in crate::iced_ui) fn progress_window_needs_resize(actual: Size, expected: Size) -> bool {
    (actual.width - expected.width).abs() > 0.5 || (actual.height - expected.height).abs() > 0.5
}

fn fixed_progress_window_settings(size: Size, position: Option<Point>) -> window::Settings {
    window::Settings {
        size,
        position: position.map(window::Position::Specific).unwrap_or_default(),
        min_size: Some(size),
        max_size: Some(size),
        closeable: false,
        decorations: false,
        resizable: false,
        transparent: true,
        exit_on_close_request: false,
        icon: app_window_icon(),
        #[cfg(target_os = "linux")]
        platform_specific: window::settings::PlatformSpecific {
            application_id: crate::platform::LINUX_APPLICATION_ID.into(),
            ..window::settings::PlatformSpecific::default()
        },
        ..window::Settings::default()
    }
}

pub(in crate::iced_ui) fn close_window_after_native_cleanup(id: window::Id) -> Task<Message> {
    window::run(id, move |native_window| {
        if let (Ok(display_handle), Ok(window_handle)) = (
            native_window.display_handle(),
            native_window.window_handle(),
        ) {
            crate::platform::release_external_window_resources(
                display_handle.as_raw(),
                window_handle.as_raw(),
            );
        }
        Message::Noop
    })
    .chain(window::close(id))
}

pub(in crate::iced_ui) fn close_application_after_native_cleanup(id: window::Id) -> Task<Message> {
    window::run(id, move |native_window| {
        if let Ok(display_handle) = native_window.display_handle() {
            crate::platform::release_external_display_resources(display_handle.as_raw());
        }
        Message::Noop
    })
    .chain(window::close(id))
}

pub(in crate::iced_ui) fn transfer_window_settings(size: Size) -> window::Settings {
    fixed_progress_window_settings(size, None)
}

pub(in crate::iced_ui) fn transfer_window_settings_at(
    size: Size,
    position: Option<Point>,
) -> window::Settings {
    fixed_progress_window_settings(size, position)
}

pub(in crate::iced_ui) fn archive_window_settings(size: Size) -> window::Settings {
    fixed_progress_window_settings(size, None)
}

pub(in crate::iced_ui) fn archive_window_settings_at(
    size: Size,
    position: Option<Point>,
) -> window::Settings {
    fixed_progress_window_settings(size, position)
}

pub(in crate::iced_ui) fn defender_window_size_for_detail_lines(detail_lines: usize) -> Size {
    let height = (DEFENDER_WINDOW_BASE_HEIGHT
        + detail_lines as f32 * DEFENDER_WINDOW_DETAIL_LINE_HEIGHT)
        .min(DEFENDER_WINDOW_MAX_HEIGHT);
    Size::new(TRANSFER_WINDOW_WIDTH, height)
}

#[cfg(any(test, target_os = "windows"))]
pub(in crate::iced_ui) fn defender_window_settings(size: Size) -> window::Settings {
    fixed_progress_window_settings(size, None)
}

pub(in crate::iced_ui) fn defender_threats_window_size(threat_count: usize) -> Size {
    let visible_count = threat_count.clamp(1, DEFENDER_THREAT_WINDOW_VISIBLE_CARD_LIMIT);
    let height = DEFENDER_THREAT_WINDOW_BASE_HEIGHT
        + (visible_count.saturating_sub(1) as f32)
            * (DEFENDER_THREAT_CARD_HEIGHT + DEFENDER_THREAT_CARD_GAP);
    Size::new(DEFENDER_THREAT_WINDOW_WIDTH, height)
}

pub(in crate::iced_ui) fn defender_threats_window_settings(
    threat_count: usize,
) -> window::Settings {
    fixed_progress_window_settings(defender_threats_window_size(threat_count), None)
}

pub(in crate::iced_ui) fn sync_fixed_progress_window_size_task(
    id: window::Id,
    size: Size,
) -> Task<Message> {
    window::set_min_size(id, None)
        .chain(window::set_max_size(id, None))
        .chain(window::resize(id, size))
        .chain(window::set_min_size(id, Some(size)))
        .chain(window::set_max_size(id, Some(size)))
        .chain(window::set_resizable(id, false))
}

pub(in crate::iced_ui) fn transfer_tick_stream() -> impl iced::futures::Stream<Item = Message> {
    periodic_message_stream(Duration::from_millis(80), Message::PollTransfers)
}

pub(in crate::iced_ui) fn advance_layout_animation(
    current: f32,
    target: f32,
    elapsed: Duration,
) -> f32 {
    let elapsed = elapsed.as_secs_f32().clamp(0.0, 1.0 / 30.0);
    let blend = 1.0 - (-LAYOUT_ANIMATION_RESPONSE * elapsed).exp();
    let next = current + (target - current) * blend;
    if (next - target).abs() <= 0.0005 {
        target
    } else {
        next.clamp(0.0, 1.0)
    }
}

pub(in crate::iced_ui) fn advance_popup_animation(
    current: f32,
    target: f32,
    elapsed: Duration,
) -> f32 {
    let elapsed = elapsed.as_secs_f32().clamp(0.0, 1.0 / 30.0);
    let blend = 1.0 - (-POPUP_ANIMATION_RESPONSE * elapsed).exp();
    let next = current + (target - current) * blend;
    if (next - target).abs() <= 0.002 {
        target
    } else {
        next.clamp(0.0, 1.0)
    }
}

pub(in crate::iced_ui) fn popup_backdrop_opacity(progress: f32, target: f32) -> f32 {
    let progress = progress.clamp(0.0, 1.0);
    if target < progress {
        // While closing, the foreground becomes translucent and would expose
        // its cached blur underneath. Retire that texture faster so it never
        // survives visually after the menu surface.
        progress * progress
    } else {
        progress
    }
}

pub(in crate::iced_ui) fn scrollbar_animation_tick_stream()
-> impl iced::futures::Stream<Item = Message> {
    periodic_message_stream(Duration::from_millis(16), Message::ScrollbarAnimationTick)
}

pub(in crate::iced_ui) fn async_progress_tick_stream() -> impl iced::futures::Stream<Item = Message>
{
    periodic_message_stream(Duration::from_millis(33), Message::AsyncProgressTick)
}

pub(in crate::iced_ui) fn external_drag_tick_stream() -> impl iced::futures::Stream<Item = Message>
{
    periodic_message_stream(Duration::from_millis(16), Message::PollExternalFileDrag)
}

pub(in crate::iced_ui) fn external_drag_polling_required(
    preparing_drag: bool,
    native_drag_active: bool,
) -> bool {
    preparing_drag || native_drag_active
}

pub(in crate::iced_ui) fn external_file_drop_stream() -> impl iced::futures::Stream<Item = Message>
{
    use iced::futures::channel::mpsc;

    iced::stream::channel(1, move |output: mpsc::Sender<Message>| async move {
        thread::spawn(move || {
            let receiver = crate::platform::external_file_drop_receiver();
            let mut output = output;
            while receiver.recv().is_ok() {
                if let Err(error) = output.try_send(Message::PollExternalFileDrag)
                    && error.is_disconnected()
                {
                    break;
                }
            }
        });
        iced::futures::future::pending::<()>().await;
    })
}

pub(in crate::iced_ui) fn search_tick_stream() -> impl iced::futures::Stream<Item = Message> {
    periodic_message_stream(Duration::from_millis(32), Message::PollSearches)
}

pub(in crate::iced_ui) fn storage_change_stream() -> impl iced::futures::Stream<Item = Message> {
    use iced::futures::channel::mpsc;

    iced::stream::channel(1, move |output: mpsc::Sender<Message>| async move {
        thread::spawn(move || {
            let receiver = crate::platform::storage_change_receiver();
            let mut output = output;
            while receiver.recv().is_ok() {
                if let Err(error) = output.try_send(Message::StorageDevicesChanged)
                    && error.is_disconnected()
                {
                    break;
                }
            }
        });
        iced::futures::future::pending::<()>().await;
    })
}

fn periodic_message_stream(
    interval: Duration,
    message: Message,
) -> impl iced::futures::Stream<Item = Message> {
    use iced::futures::channel::mpsc;

    iced::stream::channel(1, move |output: mpsc::Sender<Message>| async move {
        thread::spawn(move || {
            let mut output = output;
            loop {
                thread::sleep(interval);
                if let Err(error) = output.try_send(message.clone())
                    && error.is_disconnected()
                {
                    break;
                }
            }
        });
        iced::futures::future::pending::<()>().await;
    })
}

pub(in crate::iced_ui) fn keyboard_shortcut_from_key(
    key: &keyboard::Key,
    physical_key: keyboard::key::Physical,
    modifiers: keyboard::Modifiers,
    shortcuts: &ShortcutConfig,
) -> Option<KeyboardShortcut> {
    let binding = shortcut_binding_from_key(key, physical_key, modifiers)?;
    [
        (ShortcutAction::Copy, KeyboardShortcut::Copy),
        (ShortcutAction::Paste, KeyboardShortcut::Paste),
        (ShortcutAction::Cut, KeyboardShortcut::Cut),
        (ShortcutAction::Undo, KeyboardShortcut::Undo),
        (ShortcutAction::Refresh, KeyboardShortcut::Refresh),
        (ShortcutAction::Delete, KeyboardShortcut::Delete),
        (
            ShortcutAction::PermanentDelete,
            KeyboardShortcut::PermanentDelete,
        ),
        (ShortcutAction::SelectAll, KeyboardShortcut::SelectAll),
        (ShortcutAction::Rename, KeyboardShortcut::Rename),
        (ShortcutAction::EditAddress, KeyboardShortcut::EditAddress),
        (ShortcutAction::Properties, KeyboardShortcut::Properties),
        (ShortcutAction::GoUp, KeyboardShortcut::GoUp),
        (ShortcutAction::GoBack, KeyboardShortcut::GoBack),
        (ShortcutAction::GoForward, KeyboardShortcut::GoForward),
        (ShortcutAction::Open, KeyboardShortcut::Open),
    ]
    .into_iter()
    .find_map(|(action, shortcut)| (shortcuts.binding(action) == &binding).then_some(shortcut))
}

pub(in crate::iced_ui) fn shortcut_binding_from_key(
    key: &keyboard::Key,
    physical_key: keyboard::key::Physical,
    modifiers: keyboard::Modifiers,
) -> Option<ShortcutBinding> {
    use keyboard::key::Named;

    let key = match key.as_ref() {
        keyboard::Key::Named(Named::Delete) => "Delete".into(),
        keyboard::Key::Named(Named::Backspace) => "Backspace".into(),
        keyboard::Key::Named(Named::Enter) => "Enter".into(),
        keyboard::Key::Named(Named::ArrowUp) => "ArrowUp".into(),
        keyboard::Key::Named(Named::ArrowDown) => "ArrowDown".into(),
        keyboard::Key::Named(Named::ArrowLeft) => "ArrowLeft".into(),
        keyboard::Key::Named(Named::ArrowRight) => "ArrowRight".into(),
        keyboard::Key::Named(Named::F2) => "F2".into(),
        keyboard::Key::Named(Named::F5) => "F5".into(),
        _ => key.to_latin(physical_key)?.to_ascii_uppercase().to_string(),
    };
    Some(ShortcutBinding::new(
        &key,
        modifiers.command(),
        modifiers.alt(),
        modifiers.shift(),
    ))
}

pub(in crate::iced_ui) fn save_config(config: &AppConfig) {
    if let Err(error) = config.save() {
        crate::utils::log::error(format!("Config save failed: {error}"));
    }
}
