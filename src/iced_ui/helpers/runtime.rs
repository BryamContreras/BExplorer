use super::*;
pub(in crate::iced_ui) fn main_window_settings(size: Size) -> window::Settings {
    window::Settings {
        size: Size::new(size.width.max(920.0), size.height.max(560.0)),
        min_size: Some(Size::new(920.0, 560.0)),
        decorations: false,
        resizable: true,
        transparent: true,
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

pub(in crate::iced_ui) fn progress_window_size_for_item_count(item_count: usize) -> Size {
    Size::new(
        TRANSFER_WINDOW_WIDTH,
        (TRANSFER_WINDOW_CHROME_HEIGHT + progress_visible_card_list_height(item_count))
            .clamp(TRANSFER_WINDOW_MIN_HEIGHT, TRANSFER_WINDOW_MAX_HEIGHT),
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
        #[cfg(target_os = "linux")]
        platform_specific: window::settings::PlatformSpecific {
            application_id: crate::platform::LINUX_APPLICATION_ID.into(),
            ..window::settings::PlatformSpecific::default()
        },
        ..window::Settings::default()
    }
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

pub(in crate::iced_ui) fn sidebar_animation_tick_stream()
-> impl iced::futures::Stream<Item = Message> {
    periodic_message_stream(Duration::from_millis(32), Message::SidebarAnimationTick)
}

pub(in crate::iced_ui) fn preview_panel_animation_tick_stream()
-> impl iced::futures::Stream<Item = Message> {
    periodic_message_stream(
        Duration::from_millis(16),
        Message::PreviewPanelAnimationTick,
    )
}

pub(in crate::iced_ui) fn popup_fade_animation_tick_stream()
-> impl iced::futures::Stream<Item = Message> {
    periodic_message_stream(Duration::from_millis(16), Message::PopupFadeAnimationTick)
}

pub(in crate::iced_ui) fn scrollbar_animation_tick_stream()
-> impl iced::futures::Stream<Item = Message> {
    periodic_message_stream(Duration::from_millis(16), Message::ScrollbarAnimationTick)
}

pub(in crate::iced_ui) fn external_drag_tick_stream(
    active: &bool,
) -> impl iced::futures::Stream<Item = Message> + use<> {
    let interval = if *active {
        Duration::from_millis(16)
    } else {
        Duration::from_millis(100)
    };
    periodic_message_stream(interval, Message::PollExternalFileDrag)
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
