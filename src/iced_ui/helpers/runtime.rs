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

pub(in crate::iced_ui) fn progress_card_height(font_size: f32) -> f32 {
    let content_height = ui_text_line_height(font_size) * 2.0
        + 3.0
        + TRANSFER_PROGRESS_BAR_HEIGHT
        + ui_text_line_height(font_size - 1.0)
        + 14.0
        + 14.0;
    scaled_ui_metric(TRANSFER_CARD_HEIGHT, font_size).max(content_height)
}

pub(in crate::iced_ui) fn progress_card_gap(font_size: f32) -> f32 {
    scaled_ui_metric(TRANSFER_CARD_GAP, font_size)
}

pub(in crate::iced_ui) fn transfer_window_title_height(font_size: f32) -> f32 {
    scaled_ui_metric(TRANSFER_WINDOW_TITLE_HEIGHT, font_size)
        .max(ui_text_line_height(font_size) + 8.0)
}

pub(in crate::iced_ui) fn transfer_window_width(font_size: f32) -> f32 {
    adaptive_text_surface_width(TRANSFER_WINDOW_WIDTH, font_size)
}

pub(in crate::iced_ui) fn progress_card_list_height(item_count: usize, font_size: f32) -> f32 {
    let count = item_count.max(1) as f32;
    count * progress_card_height(font_size) + (count - 1.0) * progress_card_gap(font_size)
}

pub(in crate::iced_ui) fn progress_visible_card_list_height(
    item_count: usize,
    font_size: f32,
) -> f32 {
    progress_card_list_height(
        item_count.min(TRANSFER_WINDOW_VISIBLE_CARD_LIMIT),
        font_size,
    )
}

pub(in crate::iced_ui) fn transfer_window_size_for_item_count(
    item_count: usize,
    font_size: f32,
) -> Size {
    let chrome_height = WINDOW_BORDER_WIDTH * 2.0
        + transfer_window_title_height(font_size)
        + TRANSFER_WINDOW_CARD_TOP_GAP
        + TRANSFER_WINDOW_CARD_BOTTOM_PADDING;
    Size::new(
        transfer_window_width(font_size),
        chrome_height + progress_visible_card_list_height(item_count, font_size),
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

pub(in crate::iced_ui) fn duplicate_cleanup_window_size(font_size: f32) -> Size {
    Size::new(
        adaptive_text_surface_width(DUPLICATE_WINDOW_WIDTH, font_size),
        scaled_ui_metric(DUPLICATE_WINDOW_HEIGHT, font_size),
    )
}

pub(in crate::iced_ui) fn storage_analysis_window_size(font_size: f32) -> Size {
    Size::new(
        adaptive_text_surface_width(STORAGE_ANALYSIS_WINDOW_WIDTH, font_size),
        scaled_ui_metric(STORAGE_ANALYSIS_WINDOW_HEIGHT, font_size),
    )
}

pub(in crate::iced_ui) fn storage_analysis_window_settings(font_size: f32) -> window::Settings {
    window::Settings {
        size: storage_analysis_window_size(font_size),
        min_size: Some(storage_analysis_window_min_size(font_size)),
        closeable: false,
        decorations: false,
        resizable: true,
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

pub(in crate::iced_ui) fn storage_analysis_window_min_size(font_size: f32) -> Size {
    Size::new(
        adaptive_text_surface_width(1_000.0, font_size),
        scaled_ui_metric(520.0, font_size),
    )
}

pub(in crate::iced_ui) fn sync_storage_analysis_window_constraints_task(
    id: window::Id,
    font_size: f32,
) -> Task<Message> {
    window::set_min_size(id, Some(storage_analysis_window_min_size(font_size)))
        .chain(window::set_max_size(id, None))
        .chain(window::set_resizable(id, true))
}

pub(in crate::iced_ui) fn duplicate_cleanup_window_settings(font_size: f32) -> window::Settings {
    window::Settings {
        size: duplicate_cleanup_window_size(font_size),
        min_size: Some(duplicate_cleanup_window_min_size(font_size)),
        closeable: false,
        decorations: false,
        resizable: true,
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

pub(in crate::iced_ui) fn duplicate_cleanup_window_min_size(font_size: f32) -> Size {
    Size::new(
        adaptive_text_surface_width(920.0, font_size),
        scaled_ui_metric(520.0, font_size),
    )
}

pub(in crate::iced_ui) fn sync_duplicate_cleanup_window_constraints_task(
    id: window::Id,
    font_size: f32,
) -> Task<Message> {
    window::set_min_size(id, Some(duplicate_cleanup_window_min_size(font_size)))
        .chain(window::set_max_size(id, None))
        .chain(window::set_resizable(id, true))
}

#[cfg(target_os = "linux")]
pub(in crate::iced_ui) fn properties_window_size(font_size: f32) -> Size {
    Size::new(
        adaptive_text_surface_width(PROPERTIES_WINDOW_WIDTH, font_size),
        scaled_ui_metric(PROPERTIES_WINDOW_HEIGHT, font_size),
    )
}

#[cfg(target_os = "linux")]
pub(in crate::iced_ui) fn properties_window_settings(font_size: f32) -> window::Settings {
    fixed_progress_window_settings(properties_window_size(font_size), None)
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
    advance_popup_animation_with_response(current, target, elapsed, POPUP_ANIMATION_RESPONSE)
}

pub(in crate::iced_ui) fn advance_context_menu_animation(
    current: f32,
    target: f32,
    elapsed: Duration,
) -> f32 {
    advance_popup_animation_with_response(current, target, elapsed, CONTEXT_MENU_ANIMATION_RESPONSE)
}

fn advance_popup_animation_with_response(
    current: f32,
    target: f32,
    elapsed: Duration,
    response: f32,
) -> f32 {
    let elapsed = elapsed.as_secs_f32().clamp(0.0, 1.0 / 30.0);
    let blend = 1.0 - (-response * elapsed).exp();
    let next = current + (target - current) * blend;
    if (next - target).abs() <= 0.002 {
        target
    } else {
        next.clamp(0.0, 1.0)
    }
}

pub(in crate::iced_ui) fn context_menu_reveal_offset(
    progress: f32,
    opens_upward: bool,
    distance: f32,
) -> f32 {
    let remaining = 1.0 - progress.clamp(0.0, 1.0);
    let direction = if opens_upward { -1.0 } else { 1.0 };
    direction * distance.max(0.0) * remaining
}

pub(in crate::iced_ui) fn context_menu_reveal_scale(progress: f32) -> f32 {
    let progress = progress.clamp(0.0, 1.0);
    CONTEXT_MENU_INITIAL_SCALE + (1.0 - CONTEXT_MENU_INITIAL_SCALE) * progress
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

#[cfg(target_os = "windows")]
pub(in crate::iced_ui) fn windows_window_appearance_stream()
-> impl iced::futures::Stream<Item = Message> {
    use iced::futures::channel::mpsc;

    iced::stream::channel(1, move |output: mpsc::Sender<Message>| async move {
        thread::spawn(move || {
            let receiver = crate::platform::main_window_appearance_receiver();
            let mut output = output;
            while receiver.recv().is_ok() {
                let event = crate::platform::take_main_window_appearance_event();
                let message = Message::WindowsWindowAppearanceSettled(
                    event.generation,
                    event.revision,
                    event.maximized,
                );
                // This dedicated bridge thread honors backpressure so the
                // newest native settle cannot be dropped while Iced is busy.
                if iced::futures::executor::block_on(iced::futures::SinkExt::send(
                    &mut output,
                    message,
                ))
                .is_err()
                {
                    return;
                }
            }
        });
        iced::futures::future::pending::<()>().await;
    })
}

pub(in crate::iced_ui) fn directory_change_stream() -> impl iced::futures::Stream<Item = Message> {
    use iced::futures::channel::mpsc;

    iced::stream::channel(1, move |output: mpsc::Sender<Message>| async move {
        thread::spawn(move || {
            let receiver = crate::fs::watcher::directory_change_receiver();
            let mut output = output;
            while let Ok(change) = receiver.recv() {
                if let Err(error) = output.try_send(Message::DirectoryChanged(change))
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
        (
            ShortcutAction::SwitchPaneFocus,
            KeyboardShortcut::SwitchPaneFocus,
        ),
        (ShortcutAction::FocusSearch, KeyboardShortcut::FocusSearch),
    ]
    .into_iter()
    .find_map(|(action, shortcut)| {
        let configured = shortcuts.binding(action);
        let exact = configured == &binding;
        let reverse_pane_focus = modifiers.shift()
            && action == ShortcutAction::SwitchPaneFocus
            && !configured.shift
            && configured.key == binding.key
            && configured.ctrl == binding.ctrl
            && configured.alt == binding.alt;
        (exact || reverse_pane_focus).then_some(shortcut)
    })
}

pub(in crate::iced_ui) fn rename_clipboard_shortcut_from_key(
    key: &keyboard::Key,
    physical_key: keyboard::key::Physical,
    modifiers: keyboard::Modifiers,
) -> Option<RenameClipboardShortcut> {
    match key.to_latin(physical_key) {
        Some('c') if modifiers.command() => Some(RenameClipboardShortcut::Copy),
        Some('x') if modifiers.command() => Some(RenameClipboardShortcut::Cut),
        Some('v') if modifiers.command() && !modifiers.alt() => {
            Some(RenameClipboardShortcut::Paste)
        }
        Some('a') if modifiers.command() => Some(RenameClipboardShortcut::SelectAll),
        _ => None,
    }
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
        keyboard::Key::Named(Named::Tab) => "Tab".into(),
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

#[cfg(test)]
mod rename_clipboard_shortcut_tests {
    use super::*;
    use iced::keyboard::key::{Code, Physical};

    fn shortcut(character: &str, code: Code) -> Option<RenameClipboardShortcut> {
        rename_clipboard_shortcut_from_key(
            &keyboard::Key::Character(character.into()),
            Physical::Code(code),
            keyboard::Modifiers::CTRL,
        )
    }

    #[test]
    fn standard_text_shortcuts_take_priority_while_renaming() {
        assert_eq!(
            shortcut("c", Code::KeyC),
            Some(RenameClipboardShortcut::Copy)
        );
        assert_eq!(
            shortcut("x", Code::KeyX),
            Some(RenameClipboardShortcut::Cut)
        );
        assert_eq!(
            shortcut("v", Code::KeyV),
            Some(RenameClipboardShortcut::Paste)
        );
        assert_eq!(
            shortcut("a", Code::KeyA),
            Some(RenameClipboardShortcut::SelectAll)
        );
        assert_eq!(shortcut("z", Code::KeyZ), None);
    }

    #[test]
    fn pane_focus_and_search_shortcuts_use_their_defaults() {
        let shortcuts = ShortcutConfig::default();
        assert_eq!(
            keyboard_shortcut_from_key(
                &keyboard::Key::Named(keyboard::key::Named::Tab),
                Physical::Code(Code::Tab),
                keyboard::Modifiers::empty(),
                &shortcuts,
            ),
            Some(KeyboardShortcut::SwitchPaneFocus)
        );
        assert_eq!(
            keyboard_shortcut_from_key(
                &keyboard::Key::Named(keyboard::key::Named::Tab),
                Physical::Code(Code::Tab),
                keyboard::Modifiers::SHIFT,
                &shortcuts,
            ),
            Some(KeyboardShortcut::SwitchPaneFocus)
        );
        assert_eq!(
            keyboard_shortcut_from_key(
                &keyboard::Key::Character("b".into()),
                Physical::Code(Code::KeyB),
                keyboard::Modifiers::CTRL,
                &shortcuts,
            ),
            Some(KeyboardShortcut::FocusSearch)
        );
    }

    #[test]
    fn file_navigation_arrows_are_fixed_instead_of_configurable() {
        let shortcuts = ShortcutConfig::default();
        assert_eq!(
            keyboard_shortcut_from_key(
                &keyboard::Key::Named(keyboard::key::Named::ArrowUp),
                Physical::Code(Code::ArrowUp),
                keyboard::Modifiers::empty(),
                &shortcuts,
            ),
            None
        );
    }
}
