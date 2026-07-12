use super::*;
use iced::gradient;
use iced::widget::text_editor;

pub(super) struct LoadResult {
    pub(super) pane: PaneId,
    pub(super) request_id: u64,
    pub(super) entries: Result<Vec<FileEntry>, String>,
}

pub(super) async fn load_entries(
    pane: PaneId,
    request_id: u64,
    path: Option<PathBuf>,
    show_hidden: bool,
) -> LoadResult {
    let entries =
        run_blocking_file_operation(move || explorer::list_entries(path.as_deref(), show_hidden))
            .await;
    LoadResult {
        pane,
        request_id,
        entries,
    }
}

pub(super) async fn run_blocking_file_operation<T, F>(operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> crate::utils::errors::Result<T> + Send + 'static,
{
    let (sender, receiver) = iced::futures::channel::oneshot::channel();
    thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation))
            .map_err(|_| String::from("File operation worker panicked"))
            .and_then(|result| result.map_err(|error| error.to_string()));
        let _ = sender.send(result);
    });

    receiver
        .await
        .unwrap_or_else(|_| Err(String::from("File operation worker stopped unexpectedly")))
}

pub(super) async fn delay(duration: Duration) {
    let (sender, receiver) = iced::futures::channel::oneshot::channel();
    thread::spawn(move || {
        thread::sleep(duration);
        let _ = sender.send(());
    });
    let _ = receiver.await;
}

pub(super) fn is_mountable_disk_image_entry(entry: &FileEntry) -> bool {
    entry.category == FileCategory::DiskImage
        && !explorer::is_virtual_path(&entry.path)
        && operations::can_mount_disk_image(&entry.path)
}

/// Builds a compact, cached backdrop for an in-window floating surface.
///
/// Iced overlays share the same native surface as the file table, so a
/// compositor cannot blur the table underneath them.  We take a screenshot
/// before opening the overlay, blur only its small region off the UI thread,
/// and reuse the resulting texture until the overlay closes.
pub(super) fn blurred_screenshot_region(
    screenshot: iced::window::Screenshot,
    logical_region: Rectangle,
) -> Option<iced_image::Handle> {
    if logical_region.width <= 0.0 || logical_region.height <= 0.0 {
        return None;
    }

    let scale = screenshot.scale_factor.max(1.0);
    let left = (logical_region.x.max(0.0) * scale).floor() as u32;
    let top = (logical_region.y.max(0.0) * scale).floor() as u32;
    let width = (logical_region.width * scale).ceil() as u32;
    let height = (logical_region.height * scale).ceil() as u32;
    let max_width = screenshot.size.width.saturating_sub(left);
    let max_height = screenshot.size.height.saturating_sub(top);
    let width = width.min(max_width);
    let height = height.min(max_height);
    if width < 2 || height < 2 {
        return None;
    }

    let crop = screenshot
        .crop(Rectangle {
            x: left,
            y: top,
            width,
            height,
        })
        .ok()?;
    let source = image::RgbaImage::from_raw(width, height, crop.rgba.to_vec())?;
    // Downsampling makes the blur both cheaper and smoother.  The work is
    // performed once per opening, never while the user moves the pointer.
    let small_width = (width / 4).max(1);
    let small_height = (height / 4).max(1);
    let reduced = image::imageops::resize(
        &source,
        small_width,
        small_height,
        image::imageops::FilterType::Triangle,
    );
    // The snapshot is captured before the compositor applies native blur, so
    // use a denser pass here to visually match the system glass effect.
    let blurred = image::imageops::blur(&reduced, 5.2);
    let restored = image::imageops::resize(
        &blurred,
        width,
        height,
        image::imageops::FilterType::CatmullRom,
    );

    Some(iced_image::Handle::from_rgba(
        width,
        height,
        restored.into_raw(),
    ))
}

mod colors;
mod controls;
mod ordering;
mod runtime;
mod sidebar;
mod widgets;

pub(super) use colors::*;
pub(super) use controls::*;
pub(super) use ordering::*;
pub(super) use runtime::*;
pub(super) use sidebar::*;
pub(super) use widgets::*;

#[cfg(test)]
mod tests {
    use super::*;

    fn test_entry(name: &str, kind: EntryKind, size: Option<u64>) -> FileEntry {
        let path = PathBuf::from(name);
        FileEntry {
            name: name.into(),
            category: explorer::classify_file_category(&path),
            path,
            kind,
            drive_kind: None,
            file_system: String::new(),
            free_space: None,
            size,
            percent_full: None,
            modified: None,
            created: None,
            is_hidden: false,
        }
    }

    #[test]
    fn accent_hue_roundtrip_stays_close() {
        for hue in [0.0, 45.0, 120.0, 200.0, 280.0, 340.0] {
            let color = accent_color_from_hue(hue);
            let actual = accent_hue_from_color(color);
            let diff = (actual - hue).abs().min(360.0 - (actual - hue).abs());
            assert!(diff <= 1.0, "hue {hue} became {actual}");
        }
    }

    #[test]
    fn vibrancy_uses_dense_glass_for_menus_inputs_and_overlay_windows() {
        let config = AppConfig {
            vibrancy: VibrancyMode::Blur,
            vibrancy_intensity: 90,
            vibrancy_active: true,
            ..AppConfig::default()
        };

        let palette = Palette::from_config(&config, true);
        assert!(palette.page_bg.a < 1.0);
        assert!(palette.menu_bg.a > palette.page_bg.a);
        assert!(palette.input_bg.a > palette.page_bg.a);
        assert!(palette.overlay_bg.a > palette.menu_bg.a);
        assert!(palette.overlay_title_bg.a > palette.menu_bg.a);
        assert!(palette.overlay_bg.a < 1.0);
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    #[test]
    fn linux_uses_one_blur_label_for_gnome_and_kde() {
        assert_eq!(vibrancy_mode_label(VibrancyMode::Blur, true), "Difuminado");
        assert_eq!(vibrancy_mode_label(VibrancyMode::Blur, false), "Blur");
    }

    #[test]
    fn backdrop_snapshot_blurs_a_requested_region() {
        let mut pixels = vec![0_u8; 12 * 8 * 4];
        for y in 0..8 {
            for x in 0..12 {
                let index = (y * 12 + x) * 4;
                pixels[index] = if x < 6 { 20 } else { 230 };
                pixels[index + 1] = 80;
                pixels[index + 2] = 160;
                pixels[index + 3] = 255;
            }
        }
        let screenshot = iced::window::Screenshot::new(pixels, Size::new(12, 8), 1.0);
        let backdrop = blurred_screenshot_region(
            screenshot,
            Rectangle::new(Point::new(2.0, 1.0), Size::new(7.0, 6.0)),
        );
        assert!(backdrop.is_some());
    }

    #[test]
    fn tile_metadata_includes_the_file_size() {
        let entry = test_entry("imagen.png", EntryKind::File, Some(2 * 1024 * 1024));
        assert_eq!(tile_metadata_label(&entry), "Image PNG · 2.0 MB");
    }

    #[test]
    fn drive_capacity_label_reports_used_and_total_space() {
        const GB: u64 = 1024 * 1024 * 1024;
        let mut entry = test_entry("disk", EntryKind::Drive, Some(512 * GB));
        entry.free_space = Some(256 * GB);
        assert_eq!(drive_capacity_label(&entry), "256.0 GB de 512.0 GB");
    }

    #[test]
    fn iso_entries_use_the_native_mount_action() {
        let entry = test_entry("installer.iso", EntryKind::File, Some(1024));
        assert!(is_mountable_disk_image_entry(&entry));

        let document = test_entry("notes.txt", EntryKind::File, Some(32));
        assert!(!is_mountable_disk_image_entry(&document));
    }

    #[test]
    fn sidebar_exposes_eject_only_for_removable_storage() {
        let mut system = test_entry("system", EntryKind::Drive, None);
        system.path = filesystem_root_path();
        system.drive_kind = Some(DriveKind::Local);

        let mut image = test_entry("installer", EntryKind::Drive, None);
        image.path = filesystem_root_path().join("media").join("installer");
        image.drive_kind = Some(DriveKind::Optical);

        let items = sidebar_storage_items(&[system, image], true);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].context_drive_index, None);
        assert_eq!(items[1].context_drive_index, Some(1));
    }

    #[test]
    fn address_breadcrumbs_include_this_pc_and_each_directory_level() {
        let path = filesystem_root_path().join("home").join("dev");
        let breadcrumbs = address_breadcrumbs(Some(&path));
        assert_eq!(
            breadcrumbs.first().map(|crumb| crumb.0.as_str()),
            Some("This PC")
        );
        assert_eq!(
            breadcrumbs.last().map(|crumb| crumb.0.as_str()),
            Some("dev")
        );
        assert_eq!(
            breadcrumbs.last().and_then(|crumb| crumb.1.as_ref()),
            Some(&path)
        );
    }

    #[test]
    fn virtual_address_breadcrumbs_use_their_display_labels() {
        let breadcrumbs = address_breadcrumbs(Some(&explorer::network_root_path()));
        assert_eq!(
            breadcrumbs
                .iter()
                .map(|crumb| crumb.0.as_str())
                .collect::<Vec<_>>(),
            vec!["This PC", "Red"]
        );
    }

    #[test]
    fn tab_indices_are_rebased_after_a_real_close() {
        assert_eq!(rebase_tab_indices(&[0, 1], 1), vec![0]);
        assert_eq!(rebase_tab_indices(&[2, 3], 1), vec![1, 2]);
        assert_eq!(rebase_tab_index(1, 1), None);
    }

    #[test]
    fn incremental_rendering_eventually_reaches_every_entry() {
        let total = 2_350;
        let mut visible = INITIAL_RENDER_LIMIT;
        while visible < total {
            visible = expanded_render_limit(visible, total);
        }
        assert_eq!(visible, total);
    }

    #[test]
    fn rubber_band_rectangle_is_normalized_in_every_drag_direction() {
        let expected = Rectangle {
            x: 20.0,
            y: 10.0,
            width: 80.0,
            height: 70.0,
        };
        for (start, current) in [
            (Point::new(20.0, 10.0), Point::new(100.0, 80.0)),
            (Point::new(100.0, 10.0), Point::new(20.0, 80.0)),
            (Point::new(20.0, 80.0), Point::new(100.0, 10.0)),
            (Point::new(100.0, 80.0), Point::new(20.0, 10.0)),
        ] {
            assert_eq!(normalized_rect(start, current), expected);
        }
    }

    #[test]
    fn rename_target_preserves_the_original_extension() {
        assert_eq!(
            rename_target_name("vacaciones", Some("jpeg")),
            "vacaciones.jpeg"
        );
        assert_eq!(
            rename_target_name("vacaciones.jpeg", Some("jpeg")),
            "vacaciones.jpeg"
        );
    }

    #[test]
    fn grouping_keeps_equal_type_labels_contiguous() {
        let mut entries = [
            test_entry("z.png", EntryKind::File, Some(20)),
            test_entry("notes.txt", EntryKind::File, Some(10)),
            test_entry("a.png", EntryKind::File, Some(5)),
        ];
        entries.sort_by(|left, right| {
            compare_entries_for_view(left, right, GroupMode::Type, true, TableColumn::Name, true)
        });

        let labels = entries
            .iter()
            .map(|entry| entry_group_label(entry, GroupMode::Type))
            .collect::<Vec<_>>();
        assert_eq!(labels, ["Document TXT", "Image PNG", "Image PNG"]);
        assert_eq!(entries[1].name, "a.png");
        assert_eq!(entries[2].name, "z.png");
    }

    #[test]
    fn detail_column_sort_keeps_directories_first_and_toggles_direction() {
        let folder = test_entry("carpeta", EntryKind::Folder, None);
        let small = test_entry("small.bin", EntryKind::File, Some(1));
        let large = test_entry("large.bin", EntryKind::File, Some(100));

        let mut ascending = [large.clone(), folder.clone(), small.clone()];
        ascending.sort_by(|left, right| {
            compare_entries_for_view(left, right, GroupMode::None, true, TableColumn::Size, true)
        });
        assert_eq!(ascending[0].name, folder.name);
        assert_eq!(ascending[1].name, small.name);
        assert_eq!(ascending[2].name, large.name);

        let mut descending = [small, folder, large];
        descending.sort_by(|left, right| {
            compare_entries_for_view(left, right, GroupMode::None, true, TableColumn::Size, false)
        });
        assert_eq!(descending[0].kind, EntryKind::Folder);
        assert_eq!(descending[1].size, Some(100));
        assert_eq!(descending[2].size, Some(1));
    }

    #[test]
    fn this_pc_and_network_root_use_the_fixed_presentation_only_at_the_root() {
        assert!(uses_fixed_root_presentation(None));

        let network_root = explorer::network_root_path();
        assert!(uses_fixed_root_presentation(Some(&network_root)));

        let network_host = explorer::network_host_path("nas");
        assert!(!uses_fixed_root_presentation(Some(&network_host)));
        assert!(!uses_fixed_root_presentation(Some(Path::new("/tmp"))));
    }

    #[test]
    fn progress_window_grows_for_three_cards_then_caps_and_scrolls() {
        assert_eq!(progress_card_list_height(1), TRANSFER_CARD_HEIGHT);
        assert_eq!(
            progress_card_list_height(2),
            TRANSFER_CARD_HEIGHT * 2.0 + TRANSFER_CARD_GAP
        );
        assert_eq!(
            progress_visible_card_list_height(4),
            TRANSFER_CARD_HEIGHT * 3.0 + TRANSFER_CARD_GAP * 2.0
        );

        assert_eq!(
            progress_window_size_for_item_count(1).height,
            TRANSFER_WINDOW_MIN_HEIGHT
        );
        assert_eq!(
            progress_window_size_for_item_count(2).height,
            TRANSFER_WINDOW_MIN_HEIGHT + TRANSFER_CARD_HEIGHT + TRANSFER_CARD_GAP
        );
        assert_eq!(
            progress_window_size_for_item_count(3).height,
            TRANSFER_WINDOW_MAX_HEIGHT
        );
        assert_eq!(
            progress_window_size_for_item_count(4).height,
            TRANSFER_WINDOW_MAX_HEIGHT
        );
    }

    #[test]
    fn native_progress_windows_are_fixed_size_and_open_at_the_requested_height() {
        let size = progress_window_size_for_item_count(2);
        let transfer = transfer_window_settings(size);
        let archive = archive_window_settings(size);

        assert_eq!(transfer.size, size);
        assert_eq!(archive.size, size);
        assert!(!transfer.resizable);
        assert!(!archive.resizable);
        assert_eq!(transfer.min_size, Some(size));
        assert_eq!(transfer.max_size, Some(size));
        assert_eq!(archive.min_size, Some(size));
        assert_eq!(archive.max_size, Some(size));

        #[cfg(target_os = "linux")]
        {
            assert_eq!(
                main_window_settings(Size::new(1280.0, 760.0))
                    .platform_specific
                    .application_id,
                crate::platform::LINUX_APPLICATION_ID
            );
            assert_eq!(
                transfer.platform_specific.application_id,
                crate::platform::LINUX_APPLICATION_ID
            );
        }
    }

    #[test]
    fn progress_window_retries_after_the_compositor_reports_a_scaled_initial_size() {
        let expected = progress_window_size_for_item_count(2);
        assert!(progress_window_needs_resize(
            Size::new(expected.width, TRANSFER_WINDOW_MIN_HEIGHT),
            expected
        ));
        assert!(!progress_window_needs_resize(expected, expected));
    }
}
