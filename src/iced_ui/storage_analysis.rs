use super::*;

impl BExplorerIced {
    pub(in crate::iced_ui) fn start_storage_analysis(
        &mut self,
        pane: PaneId,
        target: ContextTarget,
    ) -> Task<Message> {
        let Some(entry) = self.context_entry(pane, target) else {
            return Task::none();
        };
        if !storage_analysis_available_for_entry(&entry) {
            return self.report_error(
                pane,
                self.localized(
                    "El análisis de almacenamiento solo está disponible para carpetas y unidades montadas.",
                    "Storage analysis is only available for folders and mounted drives.",
                ),
            );
        }

        let (window_size, window_maximized, category_column_widths) = self
            .storage_analysis
            .as_ref()
            .map(|state| {
                (
                    state.window_size,
                    state.window_maximized,
                    state.category_column_widths,
                )
            })
            .unwrap_or_else(|| {
                (
                    storage_analysis_window_size(self.font_size()),
                    false,
                    STORAGE_CATEGORY_TABLE_COLUMN_WIDTHS,
                )
            });
        if let Some(state) = self.storage_analysis.take() {
            state.cancel_workers();
        }
        let root = entry.path;
        let (receiver, cancel) = storage_analysis_worker(root.clone());
        self.storage_analysis = Some(StorageAnalysisState {
            pane,
            root,
            summary: StorageAnalysisSummary::default(),
            files: StorageFiles::default(),
            category_colors: random_storage_category_colors(),
            donut_pointer: None,
            duplicate_donut_pointer: None,
            overview_selected_category: None,
            overview_extensions: Vec::new(),
            overview_selected_extension: None,
            overview_extension_scroll_offset_y: 0.0,
            overview_extension_viewport_height: 0.0,
            overview_extension_scroll_velocity_y: 0.0,
            overview_extension_scroll_sampled_at: None,
            selected_category: None,
            category_highlighted: None,
            category_pointer: Point::ORIGIN,
            category_context_path: None,
            category_context_position: Point::ORIGIN,
            category_column_widths,
            category_column_resize: None,
            category_sort_column: StorageCategorySortColumn::Name,
            category_sort_ascending: true,
            category_filter: String::new(),
            category_filter_matches: None,
            category_scroll_offset_y: 0.0,
            category_viewport_height: 0.0,
            category_scroll_velocity_y: 0.0,
            category_scroll_sampled_at: None,
            window_size,
            window_maximized,
            scrollbar_vertical_hovered: false,
            scrollbar_reveal_progress: 0.0,
            scrollbar_reveal_until: None,
            scanned: 0,
            skipped: 0,
            current_path: None,
            phase: StorageAnalysisPhase::Scanning,
            error: None,
            receiver,
            cancel,
            duplicate_estimate: StorageDuplicateEstimate::default(),
        });

        if let Some(id) = self.storage_analysis_window_id {
            Task::batch([window::minimize(id, false), window::gain_focus(id)])
        } else {
            let (id, open) = window::open(storage_analysis_window_settings(self.font_size()));
            self.storage_analysis_window_id = Some(id);
            open.map(Message::StorageAnalysisWindowOpened)
        }
    }

    pub(in crate::iced_ui) fn poll_storage_analysis_messages(&mut self) {
        let spanish = self.is_spanish();
        let Some(state) = self.storage_analysis.as_mut() else {
            return;
        };
        loop {
            match state.receiver.try_recv() {
                Ok(StorageAnalysisEvent::Scanning {
                    scanned,
                    current_path,
                    summary,
                    skipped,
                }) => {
                    state.phase = StorageAnalysisPhase::Scanning;
                    state.scanned = scanned;
                    state.current_path = current_path;
                    state.summary = summary;
                    state.skipped = skipped;
                }
                Ok(StorageAnalysisEvent::Finished {
                    summary,
                    files,
                    skipped,
                }) => {
                    state.phase = StorageAnalysisPhase::Complete;
                    state.current_path = None;
                    state.scanned = files.total_files();
                    state.summary = summary;
                    state.files = files;
                    state.skipped = skipped;
                    start_storage_duplicate_estimate(state);
                }
                Ok(StorageAnalysisEvent::Cancelled {
                    summary,
                    files,
                    skipped,
                }) => {
                    state.phase = StorageAnalysisPhase::Cancelled;
                    state.current_path = None;
                    state.scanned = files.total_files();
                    state.summary = summary;
                    state.files = files;
                    state.skipped = skipped;
                }
                Ok(StorageAnalysisEvent::Failed(error)) => {
                    state.phase = StorageAnalysisPhase::Failed;
                    state.current_path = None;
                    state.error = Some(error);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if state.phase == StorageAnalysisPhase::Scanning {
                        state.phase = StorageAnalysisPhase::Failed;
                        state.current_path = None;
                        state.error = Some(if spanish {
                            "El proceso de análisis terminó inesperadamente.".to_owned()
                        } else {
                            "The analysis process ended unexpectedly.".to_owned()
                        });
                    }
                    break;
                }
            }
        }

        let mut duplicate_terminal = false;
        if let Some(receiver) = state.duplicate_estimate.receiver.as_ref() {
            loop {
                match receiver.try_recv() {
                    Ok(DuplicateScanEvent::Counting { files_found }) => {
                        state.duplicate_estimate.phase = StorageDuplicateEstimatePhase::Counting;
                        state.duplicate_estimate.total = files_found;
                    }
                    Ok(DuplicateScanEvent::Progress {
                        scanned,
                        total,
                        skipped: _,
                        current_path: _,
                        duplicates: _,
                    }) => {
                        state.duplicate_estimate.phase = StorageDuplicateEstimatePhase::Scanning;
                        state.duplicate_estimate.scanned = scanned;
                        state.duplicate_estimate.total = total;
                    }
                    Ok(DuplicateScanEvent::Finished {
                        scanned,
                        total,
                        duplicates,
                        skipped: _,
                    }) => {
                        let summary = duplicate_storage_summary(&duplicates);
                        state.duplicate_estimate.phase = StorageDuplicateEstimatePhase::Complete;
                        state.duplicate_estimate.scanned = scanned;
                        state.duplicate_estimate.total = total;
                        state.duplicate_estimate.summary = summary;
                        duplicate_terminal = true;
                    }
                    Ok(DuplicateScanEvent::Cancelled) => {
                        state.duplicate_estimate.phase = StorageDuplicateEstimatePhase::Cancelled;
                        duplicate_terminal = true;
                    }
                    Ok(DuplicateScanEvent::Failed(_)) => {
                        state.duplicate_estimate.phase = StorageDuplicateEstimatePhase::Failed;
                        duplicate_terminal = true;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        if matches!(
                            state.duplicate_estimate.phase,
                            StorageDuplicateEstimatePhase::Counting
                                | StorageDuplicateEstimatePhase::Scanning
                        ) {
                            state.duplicate_estimate.phase = StorageDuplicateEstimatePhase::Failed;
                        }
                        duplicate_terminal = true;
                        break;
                    }
                }
            }
        }
        if duplicate_terminal {
            state.duplicate_estimate.receiver = None;
            state.duplicate_estimate.cancel = None;
        }
    }

    pub(in crate::iced_ui) fn open_storage_category(
        &mut self,
        category: StorageCategory,
    ) -> Task<Message> {
        let Some(state) = self.storage_analysis.as_mut() else {
            return Task::none();
        };
        if state.phase == StorageAnalysisPhase::Scanning
            || state.summary.usage(category).files == 0
            || state.files.get(category).is_empty()
        {
            return Task::none();
        }

        sort_storage_category_files(
            state.files.get_mut(category),
            state.category_sort_column,
            state.category_sort_ascending,
        );
        state.selected_category = Some(category);
        reset_storage_category_detail(state);
        self.queue_storage_category_images()
    }

    pub(in crate::iced_ui) fn select_storage_overview_category(
        &mut self,
        category: StorageCategory,
    ) -> Task<Message> {
        let Some(state) = self.storage_analysis.as_mut() else {
            return Task::none();
        };
        if state.phase == StorageAnalysisPhase::Scanning
            || state.summary.usage(category).files == 0
            || state.files.get(category).is_empty()
        {
            return Task::none();
        }

        state.overview_extensions = storage_extension_usage(state.files.get(category));
        state.overview_selected_category = Some(category);
        state.overview_selected_extension = None;
        state.overview_extension_scroll_offset_y = 0.0;
        state.overview_extension_viewport_height = 0.0;
        state.overview_extension_scroll_velocity_y = 0.0;
        state.overview_extension_scroll_sampled_at = None;
        iced::widget::operation::scroll_to(
            storage_overview_extension_scroll_id(),
            iced::widget::operation::AbsoluteOffset {
                x: None,
                y: Some(0.0),
            },
        )
    }

    pub(in crate::iced_ui) fn back_to_storage_overview(&mut self) -> Task<Message> {
        let Some(state) = self.storage_analysis.as_mut() else {
            return Task::none();
        };
        state.selected_category = None;
        reset_storage_category_detail(state);
        Task::none()
    }

    pub(in crate::iced_ui) fn sort_storage_category_column(
        &mut self,
        column: StorageCategorySortColumn,
    ) -> Task<Message> {
        {
            let Some(state) = self.storage_analysis.as_mut() else {
                return Task::none();
            };
            let Some(category) = state.selected_category else {
                return Task::none();
            };
            let (sort_column, ascending) = next_storage_category_sort(
                state.category_sort_column,
                state.category_sort_ascending,
                column,
            );
            state.category_sort_column = sort_column;
            state.category_sort_ascending = ascending;
            sort_storage_category_files(state.files.get_mut(category), sort_column, ascending);
            refresh_storage_category_filter(state);
            state.category_context_path = None;
            state.category_context_position = Point::ORIGIN;
            state.category_column_resize = None;
            reset_storage_table_scroll(state);
        }

        Task::batch([
            iced::widget::operation::scroll_to(
                storage_category_table_scroll_id(),
                iced::widget::operation::AbsoluteOffset {
                    x: None,
                    y: Some(0.0),
                },
            ),
            self.queue_storage_category_images(),
        ])
    }

    pub(in crate::iced_ui) fn update_storage_category_filter(
        &mut self,
        filter: String,
    ) -> Task<Message> {
        {
            let Some(state) = self.storage_analysis.as_mut() else {
                return Task::none();
            };
            if state.selected_category.is_none() {
                return Task::none();
            }
            state.category_filter = filter;
            refresh_storage_category_filter(state);
            state.category_highlighted = None;
            state.category_context_path = None;
            state.category_context_position = Point::ORIGIN;
            reset_storage_table_scroll(state);
        }

        Task::batch([
            iced::widget::operation::scroll_to(
                storage_category_table_scroll_id(),
                iced::widget::operation::AbsoluteOffset {
                    x: None,
                    y: Some(0.0),
                },
            ),
            self.queue_storage_category_images(),
        ])
    }

    pub(in crate::iced_ui) fn select_storage_category_file(
        &mut self,
        path: PathBuf,
    ) -> Task<Message> {
        let entry = {
            let Some(state) = self.storage_analysis.as_mut() else {
                return Task::none();
            };
            let Some(category) = state.selected_category else {
                return Task::none();
            };
            let Some(file) = state
                .files
                .get(category)
                .iter()
                .find(|file| file.path == path)
            else {
                return Task::none();
            };
            state.category_highlighted = Some(path);
            state.category_context_path = None;
            state.category_context_position = Point::ORIGIN;
            storage_file_entry(file)
        };
        Task::batch(self.queue_entry_images_for_variant(&entry, IcedImageVariant::Small))
    }

    pub(in crate::iced_ui) fn queue_storage_category_images(&mut self) -> Task<Message> {
        let entries = self
            .storage_analysis
            .as_ref()
            .and_then(|state| {
                let category = state.selected_category?;
                let files = state.files.get(category);
                let total = state
                    .category_filter_matches
                    .as_ref()
                    .map_or(files.len(), Vec::len);
                let range = virtual_table_range(
                    total,
                    self.ui_metric(DUPLICATE_TABLE_ROW_HEIGHT),
                    state.category_scroll_offset_y,
                    state.category_viewport_height,
                    state.category_scroll_velocity_y,
                );
                Some(
                    if let Some(matches) = state.category_filter_matches.as_deref() {
                        matches[range.start..range.end]
                            .iter()
                            .filter_map(|index| files.get(*index))
                            .map(storage_file_entry)
                            .collect::<Vec<_>>()
                    } else {
                        files[range.start..range.end]
                            .iter()
                            .map(storage_file_entry)
                            .collect::<Vec<_>>()
                    },
                )
            })
            .unwrap_or_default();
        Task::batch(
            entries.iter().flat_map(|entry| {
                self.queue_entry_images_for_variant(entry, IcedImageVariant::Small)
            }),
        )
    }

    pub(in crate::iced_ui) fn open_storage_category_file_task(
        &mut self,
        path: PathBuf,
    ) -> Task<Message> {
        let Some((stored_pane, entry)) = self.storage_analysis.as_mut().and_then(|state| {
            let category = state.selected_category?;
            let entry = state
                .files
                .get(category)
                .iter()
                .find(|file| file.path == path)
                .map(storage_file_entry)?;
            state.category_highlighted = Some(path);
            state.category_context_path = None;
            state.category_context_position = Point::ORIGIN;
            Some((state.pane, entry))
        }) else {
            return Task::none();
        };
        let pane = if stored_pane == PaneId::Secondary && self.split.is_none() {
            PaneId::Primary
        } else {
            stored_pane
        };
        let opens_inside_explorer = is_mountable_disk_image_entry(&entry)
            || crate::fs::archive_listing::is_browsable_archive(&entry.path)
            || entry.kind.is_container()
            || explorer::is_virtual_path(&entry.path);
        let open = self.open_file_entry(pane, entry);
        if !opens_inside_explorer {
            return open;
        }
        let ensure_main = self.ensure_main_window_for_attention_task();
        let focus = self
            .main_window_id
            .map(|id| Task::batch([window::minimize(id, false), window::gain_focus(id)]))
            .unwrap_or_else(Task::none);
        Task::batch([ensure_main, open, focus])
    }

    pub(in crate::iced_ui) fn open_storage_category_file_location_task(&mut self) -> Task<Message> {
        let Some((stored_pane, path)) = self.storage_analysis.as_mut().and_then(|state| {
            state
                .category_context_path
                .take()
                .or_else(|| state.category_highlighted.clone())
                .map(|path| (state.pane, path))
        }) else {
            return Task::none();
        };
        let pane = if stored_pane == PaneId::Secondary && self.split.is_none() {
            PaneId::Primary
        } else {
            stored_pane
        };
        let Some(location) = path.parent().map(Path::to_path_buf) else {
            let error = self
                .localized(
                    "La ubicación del archivo no está disponible.",
                    "The file location is not available.",
                )
                .to_owned();
            if let Some(state) = self.storage_analysis.as_mut() {
                state.error = Some(error);
            }
            return Task::none();
        };

        self.pending_reveal_in_new_tab = Some((pane, location.clone(), path));
        let ensure_main = self.ensure_main_window_for_attention_task();
        let navigation = self.open_path_in_new_tab(pane, Some(location));
        let focus = self
            .main_window_id
            .map(|id| Task::batch([window::minimize(id, false), window::gain_focus(id)]))
            .unwrap_or_else(Task::none);
        Task::batch([ensure_main, navigation, focus])
    }

    pub(in crate::iced_ui) fn close_storage_analysis_task(&mut self) -> Task<Message> {
        if let Some(state) = self.storage_analysis.as_ref() {
            state.cancel_workers();
        }
        if let Some(id) = self.storage_analysis_window_id {
            self.close_window_task(id)
        } else {
            self.storage_analysis = None;
            Task::none()
        }
    }

    pub(in crate::iced_ui) fn sync_storage_analysis_window_constraints_task(
        &self,
    ) -> Task<Message> {
        self.storage_analysis_window_id
            .map_or_else(Task::none, |id| {
                sync_storage_analysis_window_constraints_task(id, self.font_size())
            })
    }
}

fn reset_storage_category_detail(state: &mut StorageAnalysisState) {
    state.donut_pointer = None;
    state.duplicate_donut_pointer = None;
    state.category_highlighted = None;
    state.category_pointer = Point::ORIGIN;
    state.category_context_path = None;
    state.category_context_position = Point::ORIGIN;
    state.category_column_resize = None;
    state.category_filter.clear();
    state.category_filter_matches = None;
    reset_storage_table_scroll(state);
}

fn reset_storage_table_scroll(state: &mut StorageAnalysisState) {
    state.category_scroll_offset_y = 0.0;
    state.category_viewport_height = 0.0;
    state.category_scroll_velocity_y = 0.0;
    state.category_scroll_sampled_at = None;
}

fn refresh_storage_category_filter(state: &mut StorageAnalysisState) {
    let Some(category) = state.selected_category else {
        state.category_filter_matches = None;
        return;
    };
    state.category_filter_matches =
        storage_category_filter_matches(state.files.get(category), state.category_filter.as_str());
}

fn storage_category_filter_matches(files: &[StorageFile], query: &str) -> Option<Vec<usize>> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return None;
    }
    Some(
        files
            .iter()
            .enumerate()
            .filter_map(|(index, file)| {
                file.path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().to_lowercase().contains(&query))
                    .then_some(index)
            })
            .collect(),
    )
}

fn storage_extension_usage(files: &[StorageFile]) -> Vec<StorageExtensionUsage> {
    let mut extensions: HashMap<Option<String>, (u64, u64)> = HashMap::new();
    for file in files {
        let extension = file
            .path
            .extension()
            .filter(|extension| !extension.is_empty())
            .map(|extension| extension.to_string_lossy().to_ascii_lowercase());
        let usage = extensions.entry(extension).or_default();
        usage.0 = usage.0.saturating_add(file.size);
        usage.1 = usage.1.saturating_add(1);
    }

    let mut usage = extensions
        .into_iter()
        .map(|(extension, (bytes, files))| StorageExtensionUsage {
            extension,
            bytes,
            files,
        })
        .collect::<Vec<_>>();
    usage.sort_by(|left, right| {
        right
            .bytes
            .cmp(&left.bytes)
            .then_with(|| right.files.cmp(&left.files))
            .then_with(|| left.extension.cmp(&right.extension))
    });
    usage
}

fn next_storage_category_sort(
    current: StorageCategorySortColumn,
    ascending: bool,
    requested: StorageCategorySortColumn,
) -> (StorageCategorySortColumn, bool) {
    if current == requested {
        (current, !ascending)
    } else {
        (requested, true)
    }
}

fn sort_storage_category_files(
    files: &mut [StorageFile],
    column: StorageCategorySortColumn,
    ascending: bool,
) {
    if column == StorageCategorySortColumn::Type {
        if ascending {
            files.sort_by_cached_key(storage_file_type_sort_key);
        } else {
            files.sort_by_cached_key(|file| std::cmp::Reverse(storage_file_type_sort_key(file)));
        }
        return;
    }

    files.sort_by(|left, right| {
        let ordering = match column {
            StorageCategorySortColumn::Name => compare_storage_text(
                storage_file_name(left).as_ref(),
                storage_file_name(right).as_ref(),
                ascending,
            ),
            StorageCategorySortColumn::Type => unreachable!("type sorting is handled above"),
            StorageCategorySortColumn::Size => {
                ordered_comparison(left.size.cmp(&right.size), ascending)
            }
            StorageCategorySortColumn::Created => {
                compare_optional_storage_time(left.created, right.created, ascending)
            }
            StorageCategorySortColumn::Modified => {
                compare_optional_storage_time(left.modified, right.modified, ascending)
            }
            StorageCategorySortColumn::Location => compare_storage_text(
                storage_file_location(left).as_ref(),
                storage_file_location(right).as_ref(),
                ascending,
            ),
        };
        ordering
            .then_with(|| {
                explorer::compare_names_case_insensitive(
                    storage_file_name(left).as_ref(),
                    storage_file_name(right).as_ref(),
                )
            })
            .then_with(|| left.path.cmp(&right.path))
    });
}

fn storage_file_type_sort_key(file: &StorageFile) -> String {
    storage_file_entry(file).type_label().to_lowercase()
}

fn storage_file_name(file: &StorageFile) -> std::borrow::Cow<'_, str> {
    file.path.file_name().map_or_else(
        || file.path.to_string_lossy(),
        |name| name.to_string_lossy(),
    )
}

fn storage_file_location(file: &StorageFile) -> std::borrow::Cow<'_, str> {
    file.path
        .parent()
        .map_or_else(|| std::borrow::Cow::Borrowed(""), Path::to_string_lossy)
}

fn compare_storage_text(left: &str, right: &str, ascending: bool) -> std::cmp::Ordering {
    ordered_comparison(
        explorer::compare_names_case_insensitive(left, right),
        ascending,
    )
}

fn ordered_comparison(ordering: std::cmp::Ordering, ascending: bool) -> std::cmp::Ordering {
    if ascending {
        ordering
    } else {
        ordering.reverse()
    }
}

fn compare_optional_storage_time(
    left: Option<std::time::SystemTime>,
    right: Option<std::time::SystemTime>,
    ascending: bool,
) -> std::cmp::Ordering {
    match (left, right) {
        (Some(left), Some(right)) => ordered_comparison(left.cmp(&right), ascending),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

fn random_storage_category_colors() -> StorageCategoryColors {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let seed = (now as u64)
        ^ ((now >> u64::BITS) as u64).rotate_left(17)
        ^ sequence.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    storage_category_colors_from_seed(seed)
}

pub(in crate::iced_ui) fn storage_category_colors_from_seed(seed: u64) -> StorageCategoryColors {
    let mut random = seed;
    // Keep a visually balanced set, but shuffle the assignment every time an
    // analysis starts. Categories therefore have no permanent semantic color
    // while every card remains distinct and legible in light and dark themes.
    let mut colors = [
        Color::from_rgb8(64, 132, 246),
        Color::from_rgb8(225, 87, 151),
        Color::from_rgb8(139, 92, 246),
        Color::from_rgb8(20, 184, 166),
        Color::from_rgb8(234, 179, 8),
        Color::from_rgb8(249, 115, 22),
        Color::from_rgb8(161, 98, 7),
        Color::from_rgb8(79, 70, 229),
        Color::from_rgb8(6, 182, 212),
        Color::from_rgb8(100, 116, 139),
        Color::from_rgb8(148, 163, 184),
        Color::from_rgb8(34, 197, 94),
        Color::from_rgb8(244, 63, 94),
        Color::from_rgb8(14, 165, 233),
        Color::from_rgb8(132, 204, 22),
    ];

    for index in (1..colors.len()).rev() {
        let swap_with = (splitmix64(&mut random) % (index as u64 + 1)) as usize;
        colors.swap(index, swap_with);
    }
    colors
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut value = *state;
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

pub(in crate::iced_ui) fn storage_file_entry(file: &StorageFile) -> FileEntry {
    let name = file
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.path.display().to_string());
    FileEntry {
        name: name.clone(),
        path: file.path.clone(),
        kind: EntryKind::File,
        category: explorer::classify_file_category(&file.path),
        drive_kind: None,
        file_system: String::new(),
        free_space: None,
        size: Some(file.size),
        percent_full: None,
        modified: None,
        created: None,
        is_hidden: name.starts_with('.'),
    }
}

pub(in crate::iced_ui) fn storage_analysis_available_for_entry(entry: &FileEntry) -> bool {
    entry.kind.is_container()
        && !explorer::is_virtual_path(&entry.path)
        && !crate::fs::archive_listing::is_inside_archive(&entry.path)
        && entry.path.is_dir()
}

fn storage_analysis_worker(root: PathBuf) -> (Receiver<StorageAnalysisEvent>, Arc<AtomicBool>) {
    let (sender, receiver) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancel);
    thread::spawn(move || {
        crate::fs::storage_analysis::scan_folder(root, sender, worker_cancel);
    });
    (receiver, cancel)
}

fn start_storage_duplicate_estimate(state: &mut StorageAnalysisState) {
    if state.duplicate_estimate.phase != StorageDuplicateEstimatePhase::Waiting {
        return;
    }
    let (receiver, cancel) =
        storage_duplicate_estimate_worker(state.root.clone(), state.files.total_files());
    state.duplicate_estimate = StorageDuplicateEstimate {
        phase: StorageDuplicateEstimatePhase::Counting,
        receiver: Some(receiver),
        cancel: Some(cancel),
        ..StorageDuplicateEstimate::default()
    };
}

fn storage_duplicate_estimate_worker(
    root: PathBuf,
    known_total: usize,
) -> (Receiver<DuplicateScanEvent>, Arc<AtomicBool>) {
    let (sender, receiver) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancel);
    thread::spawn(move || {
        crate::fs::duplicates::scan_folder_for_estimate(root, known_total, sender, worker_cancel);
    });
    (receiver, cancel)
}

fn duplicate_storage_summary(entries: &[DuplicateFile]) -> StorageAnalysisSummary {
    let mut summary = StorageAnalysisSummary::default();
    for entry in entries
        .iter()
        .filter(|entry| entry.kind != crate::fs::duplicates::DuplicateKind::Original)
    {
        summary.add_file(
            crate::fs::storage_analysis::classify_storage_category(&entry.path),
            entry.size,
        );
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn color_key(color: Color) -> (u8, u8, u8) {
        (
            (color.r * 255.0).round() as u8,
            (color.g * 255.0).round() as u8,
            (color.b * 255.0).round() as u8,
        )
    }

    #[test]
    fn seeded_category_colors_are_distinct_and_repeatable() {
        let first = storage_category_colors_from_seed(42);
        let second = storage_category_colors_from_seed(42);
        assert_eq!(first, second);
        assert_eq!(
            first
                .into_iter()
                .map(color_key)
                .collect::<HashSet<_>>()
                .len(),
            STORAGE_CATEGORY_COLOR_COUNT
        );
    }

    #[test]
    fn different_seeds_change_category_assignments() {
        assert_ne!(
            storage_category_colors_from_seed(1),
            storage_category_colors_from_seed(2)
        );
    }

    fn storage_file(path: &str, size: u64, timestamp: Option<u64>) -> StorageFile {
        StorageFile {
            path: PathBuf::from(path),
            size,
            created: timestamp.map(|seconds| UNIX_EPOCH + Duration::from_secs(seconds)),
            modified: timestamp.map(|seconds| UNIX_EPOCH + Duration::from_secs(seconds)),
        }
    }

    #[test]
    fn storage_sort_toggles_or_starts_a_new_column_ascending() {
        assert_eq!(
            next_storage_category_sort(
                StorageCategorySortColumn::Name,
                true,
                StorageCategorySortColumn::Name,
            ),
            (StorageCategorySortColumn::Name, false)
        );
        assert_eq!(
            next_storage_category_sort(
                StorageCategorySortColumn::Name,
                false,
                StorageCategorySortColumn::Size,
            ),
            (StorageCategorySortColumn::Size, true)
        );
    }

    #[test]
    fn storage_files_sort_by_numeric_size_and_keep_missing_dates_last() {
        let mut files = vec![
            storage_file("C:/z.bin", 10, None),
            storage_file("C:/a.bin", 200, Some(20)),
            storage_file("C:/b.bin", 30, Some(10)),
        ];
        sort_storage_category_files(&mut files, StorageCategorySortColumn::Size, true);
        assert_eq!(
            files.iter().map(|file| file.size).collect::<Vec<_>>(),
            [10, 30, 200]
        );

        sort_storage_category_files(&mut files, StorageCategorySortColumn::Created, false);
        assert_eq!(
            files.iter().map(|file| file.created).collect::<Vec<_>>(),
            [
                Some(UNIX_EPOCH + Duration::from_secs(20)),
                Some(UNIX_EPOCH + Duration::from_secs(10)),
                None,
            ]
        );

        sort_storage_category_files(&mut files, StorageCategorySortColumn::Modified, true);
        assert_eq!(
            files.iter().map(|file| file.modified).collect::<Vec<_>>(),
            [
                Some(UNIX_EPOCH + Duration::from_secs(10)),
                Some(UNIX_EPOCH + Duration::from_secs(20)),
                None,
            ]
        );
    }

    #[test]
    fn storage_files_sort_by_display_type_in_both_directions() {
        let mut files = vec![
            storage_file("root/photo.png", 0, None),
            storage_file("root/report.txt", 0, None),
            storage_file("root/archive.zip", 0, None),
        ];

        sort_storage_category_files(&mut files, StorageCategorySortColumn::Type, true);
        assert_eq!(
            files
                .iter()
                .map(|file| storage_file_name(file).into_owned())
                .collect::<Vec<_>>(),
            ["archive.zip", "report.txt", "photo.png"]
        );

        sort_storage_category_files(&mut files, StorageCategorySortColumn::Type, false);
        assert_eq!(
            files
                .iter()
                .map(|file| storage_file_name(file).into_owned())
                .collect::<Vec<_>>(),
            ["photo.png", "report.txt", "archive.zip"]
        );
    }

    #[test]
    fn storage_files_sort_names_and_locations_without_case_sensitivity() {
        let mut files = vec![
            storage_file("root/Zulu/b.txt", 0, None),
            storage_file("root/alpha/C.txt", 0, None),
            storage_file("root/alpha/a.txt", 0, None),
        ];
        sort_storage_category_files(&mut files, StorageCategorySortColumn::Name, true);
        assert_eq!(
            files
                .iter()
                .map(|file| storage_file_name(file).into_owned())
                .collect::<Vec<_>>(),
            ["a.txt", "b.txt", "C.txt"]
        );

        sort_storage_category_files(&mut files, StorageCategorySortColumn::Location, true);
        let alpha = PathBuf::from("root/alpha").to_string_lossy().into_owned();
        let zulu = PathBuf::from("root/Zulu").to_string_lossy().into_owned();
        assert_eq!(
            files
                .iter()
                .map(|file| storage_file_location(file).into_owned())
                .collect::<Vec<_>>(),
            [alpha.clone(), alpha, zulu]
        );
    }

    #[test]
    fn storage_extensions_are_grouped_case_insensitively_and_ranked_by_size() {
        let files = vec![
            storage_file("root/first.ZIP", 180, None),
            storage_file("root/second.zip", 120, None),
            storage_file("root/archive.7z", 250, None),
            storage_file("root/README", 40, None),
        ];

        let usage = storage_extension_usage(&files);

        assert_eq!(
            usage,
            vec![
                StorageExtensionUsage {
                    extension: Some("zip".to_owned()),
                    bytes: 300,
                    files: 2,
                },
                StorageExtensionUsage {
                    extension: Some("7z".to_owned()),
                    bytes: 250,
                    files: 1,
                },
                StorageExtensionUsage {
                    extension: None,
                    bytes: 40,
                    files: 1,
                },
            ]
        );
    }

    #[test]
    fn storage_name_filter_searches_beyond_the_initial_render_batch() {
        const FORMER_BATCH_BOUNDARY: usize = 500;
        let mut files = (0..3_001)
            .map(|index| storage_file(&format!("root/file-{index:04}.bin"), 0, None))
            .collect::<Vec<_>>();
        files[2_750].path = PathBuf::from("root/Needle-Result.BIN");

        let matches = storage_category_filter_matches(&files, "needle").unwrap();

        assert_eq!(matches, vec![2_750]);
        assert!(matches[0] >= FORMER_BATCH_BOUNDARY);
        assert_eq!(storage_category_filter_matches(&files, "   "), None);
    }

    #[test]
    fn storage_sort_reorders_the_complete_collection_before_virtualizing() {
        const LARGE_COLLECTION_SIZE: usize = 500;
        let mut files = (0..LARGE_COLLECTION_SIZE)
            .map(|index| storage_file(&format!("root/z-{index:04}.bin"), 0, None))
            .collect::<Vec<_>>();
        files.push(storage_file("root/a-first.bin", 0, None));

        sort_storage_category_files(&mut files, StorageCategorySortColumn::Name, true);

        assert_eq!(
            storage_file_name(&files[0]).as_ref(),
            "a-first.bin",
            "sorting must run before the view selects its virtual window"
        );
    }

    #[test]
    fn duplicate_estimate_excludes_each_preserved_original() {
        let duplicate = |name: &str, size: u64, kind| DuplicateFile {
            path: PathBuf::from(name),
            name: name.to_owned(),
            extension: "bin".to_owned(),
            size,
            created: None,
            modified: None,
            kind,
        };
        let entries = vec![
            duplicate(
                "original.png",
                200,
                crate::fs::duplicates::DuplicateKind::Original,
            ),
            duplicate(
                "exact.png",
                200,
                crate::fs::duplicates::DuplicateKind::Exact,
            ),
            duplicate(
                "possible.mp4",
                75,
                crate::fs::duplicates::DuplicateKind::Possible,
            ),
        ];

        let summary = duplicate_storage_summary(&entries);
        assert_eq!(summary.total_files, 2);
        assert_eq!(summary.total_bytes, 275);
        assert_eq!(summary.usage(StorageCategory::Images).bytes, 200);
        assert_eq!(summary.usage(StorageCategory::Videos).bytes, 75);
    }
}
