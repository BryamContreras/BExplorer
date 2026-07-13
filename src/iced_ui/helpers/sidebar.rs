use super::*;
use iced::widget::row;
pub(in crate::iced_ui) fn sidebar_section_header(
    section: SidebarSection,
    spanish: bool,
    expanded: bool,
    dragging: bool,
    drag_offset: f32,
    palette: Palette,
    font_size: f32,
) -> Element<'static, Message> {
    let label = sidebar_section_label(section, spanish);
    let header = container(
        row![
            inline_icon(
                sidebar_section_icon(section),
                palette.muted_text,
                SIDEBAR_SECTION_ICON_SIZE
            ),
            text(label)
                .size((font_size + 0.8).max(12.4))
                .color(palette.muted_text)
                .width(Length::Fill)
                .wrapping(iced::widget::text::Wrapping::None),
            inline_icon(
                if expanded { "chev-down" } else { "chev-right" },
                palette.muted_text,
                13.0
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .height(SIDEBAR_SECTION_HEIGHT)
    .padding([5, 7])
    .width(Length::Fill)
    .style(move |_| {
        let background = if dragging {
            Some(mix_color(palette.sidebar_bg, palette.accent, 0.18).into())
        } else {
            None
        };
        container::Style {
            background,
            border: border::rounded(4),
            ..container::Style::default()
        }
    });

    // Buttons emit `on_press` only after release. The drag state must begin
    // on the physical press so the global pointer tracker can observe motion
    // while the mouse button is still held.
    let header = Button::new(
        mouse_area(header)
            .on_press(Message::StartSidebarSectionDrag(section))
            .on_release(Message::StopResize)
            .interaction(mouse::Interaction::Pointer),
    )
    .width(Length::Fill)
    .padding(0)
    .on_press(Message::Noop)
    .style(move |_, status| selected_button_style(palette, false, status));

    if dragging {
        float(header)
            .translate(move |_, _| Vector::new(0.0, drag_offset))
            .into()
    } else {
        header.into()
    }
}

pub(in crate::iced_ui) fn sidebar_items_for_section(
    config: &AppConfig,
    storage_entries: &[FileEntry],
    section: SidebarSection,
    spanish: bool,
) -> Vec<SidebarItem> {
    match section {
        SidebarSection::Favorites => sidebar_favorite_items(config, spanish),
        SidebarSection::Places => sidebar_place_items(spanish),
        SidebarSection::Storage => sidebar_storage_items(storage_entries, spanish),
        SidebarSection::Portable => sidebar_portable_items(storage_entries),
        SidebarSection::Network => vec![SidebarItem {
            label: if spanish {
                String::from("Abrir red")
            } else {
                String::from("Browse network")
            },
            target: SidebarTarget::Navigate(Some(explorer::network_root_path())),
            icon: "net",
            context_drive_index: None,
        }],
        SidebarSection::Recents => sidebar_recent_items(config, spanish),
    }
}

pub(in crate::iced_ui) fn sidebar_favorite_items(
    config: &AppConfig,
    spanish: bool,
) -> Vec<SidebarItem> {
    if config.favorites.is_empty() {
        return vec![SidebarItem {
            label: if spanish {
                String::from("Sin favoritos")
            } else {
                String::from("No favorites")
            },
            target: SidebarTarget::Disabled,
            icon: "bookmark",
            context_drive_index: None,
        }];
    }

    config
        .favorites
        .iter()
        .map(|path| SidebarItem {
            label: display_label(path),
            target: SidebarTarget::Navigate(Some(path.clone())),
            icon: "dir",
            context_drive_index: None,
        })
        .collect()
}

pub(in crate::iced_ui) fn sidebar_place_items(spanish: bool) -> Vec<SidebarItem> {
    let mut items = vec![SidebarItem {
        label: String::from(THIS_PC_LABEL),
        target: SidebarTarget::Navigate(None),
        icon: "pc",
        context_drive_index: None,
    }];
    for place in paths::common_places() {
        items.push(SidebarItem {
            label: localized_common_place_label(place.kind, spanish).to_owned(),
            target: SidebarTarget::Navigate(Some(place.path)),
            icon: "dir",
            context_drive_index: None,
        });
    }
    items
}

pub(in crate::iced_ui) fn localized_common_place_label(
    kind: paths::CommonPlaceKind,
    spanish: bool,
) -> &'static str {
    use paths::CommonPlaceKind;

    match (kind, spanish) {
        (CommonPlaceKind::Home, true) => "Inicio",
        (CommonPlaceKind::Desktop, true) => "Escritorio",
        (CommonPlaceKind::Downloads, true) => "Descargas",
        (CommonPlaceKind::Documents, true) => "Documentos",
        (CommonPlaceKind::Music, true) => "Música",
        (CommonPlaceKind::Pictures, true) => "Imágenes",
        (CommonPlaceKind::Videos, true) => "Videos",
        (CommonPlaceKind::Home, false) => "Home",
        (CommonPlaceKind::Desktop, false) => "Desktop",
        (CommonPlaceKind::Downloads, false) => "Downloads",
        (CommonPlaceKind::Documents, false) => "Documents",
        (CommonPlaceKind::Music, false) => "Music",
        (CommonPlaceKind::Pictures, false) => "Pictures",
        (CommonPlaceKind::Videos, false) => "Videos",
    }
}

pub(in crate::iced_ui) fn sidebar_storage_items(
    storage_entries: &[FileEntry],
    _spanish: bool,
) -> Vec<SidebarItem> {
    // Portable devices expose virtual paths and should live in their own
    // sidebar section instead of being mixed in with mounted storage.
    let mut entries = storage_entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.drive_kind != Some(DriveKind::Portable))
        .collect::<Vec<_>>();
    if entries.is_empty() {
        return vec![SidebarItem {
            label: filesystem_root_label(),
            target: SidebarTarget::Navigate(Some(filesystem_root_path())),
            icon: "storage",
            context_drive_index: None,
        }];
    }

    // Mirror This PC: drives are grouped by their type and each group is
    // ordered ascending by name. Keep the source index for contextual eject
    // actions, since `storage_entries` itself remains in its cached order.
    entries.sort_by(|(_, left), (_, right)| {
        left.type_label()
            .to_ascii_lowercase()
            .cmp(&right.type_label().to_ascii_lowercase())
            .then_with(|| explorer::compare_names_case_insensitive(&left.name, &right.name))
    });

    entries
        .into_iter()
        .map(|(index, entry)| SidebarItem {
            label: entry.name.clone(),
            target: SidebarTarget::Navigate(Some(entry.path.clone())),
            icon: "storage",
            context_drive_index: entry
                .drive_kind
                .is_some_and(DriveKind::is_ejectable)
                .then_some(index),
        })
        .collect()
}

pub(in crate::iced_ui) fn sidebar_portable_items(
    storage_entries: &[FileEntry],
) -> Vec<SidebarItem> {
    let mut entries = storage_entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.drive_kind == Some(DriveKind::Portable))
        .collect::<Vec<_>>();
    entries.sort_by(|(_, left), (_, right)| {
        explorer::compare_names_case_insensitive(&left.name, &right.name)
    });

    entries
        .into_iter()
        .map(|(index, entry)| SidebarItem {
            label: entry.name.clone(),
            target: SidebarTarget::Navigate(Some(entry.path.clone())),
            // Keep the sidebar deliberately quiet and theme-colored. The
            // richer device illustration is reserved for the This PC view.
            icon: "portable-sidebar",
            context_drive_index: entry
                .drive_kind
                .is_some_and(DriveKind::is_ejectable)
                .then_some(index),
        })
        .collect()
}

pub(in crate::iced_ui) fn sidebar_recent_items(
    config: &AppConfig,
    spanish: bool,
) -> Vec<SidebarItem> {
    if config.recent_paths.is_empty() {
        return vec![SidebarItem {
            label: if spanish {
                String::from("Sin recientes")
            } else {
                String::from("No recent items")
            },
            target: SidebarTarget::Disabled,
            icon: "rec",
            context_drive_index: None,
        }];
    }

    config
        .recent_paths
        .iter()
        .take(5)
        .map(|recent| SidebarItem {
            label: display_label(recent),
            target: SidebarTarget::Navigate(Some(recent.clone())),
            icon: "rec",
            context_drive_index: None,
        })
        .collect()
}

pub(in crate::iced_ui) fn sidebar_order_with_reorder(
    mut order: Vec<SidebarSection>,
    dragged: SidebarSection,
    target: SidebarSection,
    after: bool,
) -> Vec<SidebarSection> {
    if dragged == target {
        return order;
    }
    let Some(from) = order.iter().position(|section| *section == dragged) else {
        return order;
    };
    order.remove(from);
    let Some(target_index) = order.iter().position(|section| *section == target) else {
        return order;
    };
    let insert_at = if after {
        target_index + 1
    } else {
        target_index
    };
    order.insert(insert_at.min(order.len()), dragged);
    order
}

pub(in crate::iced_ui) fn sidebar_section_label(
    section: SidebarSection,
    spanish: bool,
) -> &'static str {
    match (section, spanish) {
        (SidebarSection::Favorites, true) => "Marcadores",
        (SidebarSection::Favorites, false) => "Bookmarks",
        (SidebarSection::Places, true) => "Lugares",
        (SidebarSection::Places, false) => "Places",
        (SidebarSection::Storage, true) => "Almacenamiento",
        (SidebarSection::Storage, false) => "Storage",
        (SidebarSection::Portable, true) => "Dispositivos portátiles",
        (SidebarSection::Portable, false) => "Portable devices",
        (SidebarSection::Network, true) => "Red",
        (SidebarSection::Network, false) => "Network",
        (SidebarSection::Recents, true) => "Recientes",
        (SidebarSection::Recents, false) => "Recent",
    }
}

pub(in crate::iced_ui) fn sidebar_section_icon(section: SidebarSection) -> &'static str {
    match section {
        SidebarSection::Favorites => "bookmark",
        SidebarSection::Places => "places",
        SidebarSection::Storage => "storage",
        SidebarSection::Portable => "portable-sidebar",
        SidebarSection::Network => "net",
        SidebarSection::Recents => "rec",
    }
}

pub(in crate::iced_ui) fn filesystem_root_path() -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from("C:\\")
    }

    #[cfg(not(windows))]
    {
        PathBuf::from("/")
    }
}

pub(in crate::iced_ui) fn filesystem_root_label() -> String {
    let path = filesystem_root_path();
    #[cfg(windows)]
    {
        let info = crate::platform::drive_info(&path);
        let drive = path
            .to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .to_owned();
        info.volume_label
            .filter(|label| !label.trim().is_empty())
            .map(|label| format!("{label} ({drive})"))
            .unwrap_or(drive)
    }
    #[cfg(not(windows))]
    {
        display_label(&path)
    }
}

pub(in crate::iced_ui) fn display_label(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| path.display().to_string())
}

pub(in crate::iced_ui) fn path_label(path: Option<&PathBuf>) -> String {
    path.map(|path| path.display().to_string())
        .unwrap_or_else(|| THIS_PC_LABEL.to_string())
}

pub(in crate::iced_ui) fn address_breadcrumbs(
    path: Option<&PathBuf>,
) -> Vec<(String, Option<PathBuf>)> {
    let Some(path) = path else {
        return vec![(THIS_PC_LABEL.to_string(), None)];
    };

    if let Some(breadcrumbs) = explorer::virtual_breadcrumbs(path) {
        return breadcrumbs;
    }
    if let Some(breadcrumbs) = explorer::unc_breadcrumbs(path) {
        return breadcrumbs;
    }

    let mut breadcrumbs = vec![(THIS_PC_LABEL.to_string(), None)];
    let mut ancestors = path.ancestors().map(Path::to_path_buf).collect::<Vec<_>>();
    ancestors.reverse();
    breadcrumbs.extend(
        ancestors
            .into_iter()
            .map(|ancestor| (display_label(&ancestor), Some(ancestor))),
    );
    breadcrumbs
}

pub(in crate::iced_ui) fn uses_fixed_root_presentation(path: Option<&Path>) -> bool {
    path.is_none() || path.is_some_and(explorer::is_network_root_path)
}

pub(in crate::iced_ui) fn available_vibrancy_modes() -> &'static [VibrancyMode] {
    #[cfg(target_os = "windows")]
    {
        &[VibrancyMode::None, VibrancyMode::Acrylic]
    }
    #[cfg(target_os = "macos")]
    {
        &[VibrancyMode::None, VibrancyMode::Blur]
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        // Wayland/X11 has no compositor-independent blur protocol. The
        // compositor-specific blur option falls back to an opaque surface.
        &[VibrancyMode::None, VibrancyMode::Blur]
    }
}

pub(in crate::iced_ui) fn vibrancy_mode_label(mode: VibrancyMode, spanish: bool) -> &'static str {
    match mode {
        VibrancyMode::None => {
            if spanish {
                "Desactivado"
            } else {
                "Disabled"
            }
        }
        VibrancyMode::Mica => "Mica",
        VibrancyMode::Acrylic => "Acrílico",
        VibrancyMode::Blur => {
            #[cfg(target_os = "macos")]
            {
                if spanish { "Vibrante" } else { "Vibrant" }
            }
            #[cfg(not(any(target_os = "windows", target_os = "macos")))]
            {
                if spanish { "Difuminado" } else { "Blur" }
            }
            #[cfg(target_os = "windows")]
            {
                if spanish { "Difuminado" } else { "Blur" }
            }
        }
    }
}

pub(in crate::iced_ui) fn vibrancy_mode_from_label(label: &str, spanish: bool) -> VibrancyMode {
    available_vibrancy_modes()
        .iter()
        .copied()
        .find(|mode| vibrancy_mode_label(*mode, spanish) == label)
        .unwrap_or(VibrancyMode::None)
}
