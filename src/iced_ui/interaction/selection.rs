use super::*;

impl BExplorerIced {
    pub(in crate::iced_ui) fn displayed_entry_indices(&self, pane: PaneId) -> Vec<usize> {
        self.filtered_entries(pane)
    }

    pub(in crate::iced_ui) fn select_single(&mut self, pane: PaneId, index: usize) {
        let Some(path) = self
            .pane(pane)
            .entries
            .get(index)
            .map(|entry| entry.path.clone())
        else {
            return;
        };
        let state = self.pane_mut(pane);
        state.selected.clear();
        state.selected.insert(path);
        state.selection_anchor = Some(index);
        state.keyboard_selection_cursor = Some(index);
    }

    pub(in crate::iced_ui) fn reveal_entry_in_pane(
        &mut self,
        pane: PaneId,
        index: usize,
    ) -> Task<Message> {
        let displayed = self.filtered_entries(pane);
        let Some(position) = displayed
            .iter()
            .position(|entry_index| *entry_index == index)
        else {
            return scroll_pane_to_top_task(pane);
        };
        let target_y = self.displayed_entry_scroll_offset(pane, &displayed, position);
        self.select_single(pane, index);
        self.last_entry_click = None;

        let entry = self.pane(pane).entries.get(index).cloned();
        let mut tasks = vec![
            self.queue_selected_preview(pane),
            iced::widget::operation::scroll_to(
                pane_scroll_id(pane),
                iced::widget::operation::AbsoluteOffset {
                    x: Some(0.0),
                    y: Some(target_y.max(0.0)),
                },
            ),
        ];
        if let Some(entry) = entry {
            let variant = if uses_small_entry_images(self.effective_view_mode(pane)) {
                IcedImageVariant::Small
            } else {
                IcedImageVariant::Standard
            };
            tasks.extend(self.queue_entry_images_for_variant(&entry, variant));
        }
        Task::batch(tasks)
    }

    fn displayed_entry_scroll_offset(
        &self,
        pane: PaneId,
        displayed: &[usize],
        target_position: usize,
    ) -> f32 {
        match self.effective_view_mode(pane) {
            ViewMode::Details | ViewMode::List => {
                let group_mode = self.effective_group_mode(pane);
                let mut current_group: Option<String> = None;
                let mut y = self.detail_header_height();
                for (position, index) in displayed.iter().copied().enumerate() {
                    let Some(entry) = self.pane(pane).entries.get(index) else {
                        continue;
                    };
                    if group_mode != GroupMode::None {
                        let group = self.localized_entry_group_label(entry, group_mode);
                        if current_group.as_ref() != Some(&group) {
                            current_group = Some(group);
                            y += self.detail_group_height();
                        }
                    }
                    if position == target_position {
                        return y;
                    }
                    y += self.detail_row_height();
                }
                y
            }
            ViewMode::Tiles
            | ViewMode::SmallIcons
            | ViewMode::MediumIcons
            | ViewMode::LargeIcons
            | ViewMode::ExtraLargeIcons => {
                let mode = self.effective_view_mode(pane);
                let group_mode = self.effective_group_mode(pane);
                let layout = self.visual_layout_for_pane(pane, mode);
                let metrics = layout.metrics;
                let mut y = if group_mode == GroupMode::None {
                    metrics.grid_padding
                } else {
                    0.0
                };
                let mut column = 0_usize;
                let mut current_group: Option<String> = None;
                for (position, index) in displayed.iter().copied().enumerate() {
                    let Some(entry) = self.pane(pane).entries.get(index) else {
                        continue;
                    };
                    if group_mode != GroupMode::None {
                        let group = self.localized_entry_group_label(entry, group_mode);
                        if current_group.as_ref() != Some(&group) {
                            if column > 0 {
                                y += metrics.cell_height + metrics.spacing;
                                column = 0;
                            }
                            current_group = Some(group);
                            y += self.detail_group_height() + metrics.spacing;
                        }
                    }
                    if position == target_position {
                        return y;
                    }
                    column += 1;
                    if column >= layout.columns {
                        column = 0;
                        y += metrics.cell_height + metrics.spacing;
                    }
                }
                y
            }
        }
    }

    pub(in crate::iced_ui) fn select_range_to(&mut self, pane: PaneId, index: usize) {
        let anchor = self.pane(pane).selection_anchor.unwrap_or(index);
        let displayed = self.displayed_entry_indices(pane);
        let Some(anchor_pos) = displayed
            .iter()
            .position(|entry_index| *entry_index == anchor)
        else {
            self.select_single(pane, index);
            return;
        };
        let Some(target_pos) = displayed
            .iter()
            .position(|entry_index| *entry_index == index)
        else {
            self.select_single(pane, index);
            return;
        };
        let start = anchor_pos.min(target_pos);
        let end = anchor_pos.max(target_pos);
        let paths = displayed[start..=end]
            .iter()
            .filter_map(|entry_index| {
                self.pane(pane)
                    .entries
                    .get(*entry_index)
                    .map(|entry| entry.path.clone())
            })
            .collect::<HashSet<_>>();

        let state = self.pane_mut(pane);
        state.selected = paths;
        state.selection_anchor = Some(anchor);
        state.keyboard_selection_cursor = Some(index);
    }

    pub(in crate::iced_ui) fn select_all(&mut self, pane: PaneId) {
        let displayed = self.displayed_entry_indices(pane);
        let paths = displayed
            .iter()
            .filter_map(|index| self.pane(pane).entries.get(*index))
            .map(|entry| entry.path.clone())
            .collect::<HashSet<_>>();
        let anchor = displayed.first().copied();
        let count = paths.len();
        let state = self.pane_mut(pane);
        state.selected = paths;
        state.selection_anchor = anchor;
        state.keyboard_selection_cursor = anchor;
        state.status = format!("Selected {count} item(s)");
    }

    pub(in crate::iced_ui) fn move_file_selection_vertical(
        &mut self,
        pane: PaneId,
        forward: bool,
        extend: bool,
    ) -> Task<Message> {
        let step = match self.effective_view_mode(pane) {
            ViewMode::Details | ViewMode::List => 1,
            ViewMode::Tiles
            | ViewMode::SmallIcons
            | ViewMode::MediumIcons
            | ViewMode::LargeIcons
            | ViewMode::ExtraLargeIcons => self
                .visual_layout_for_pane(pane, self.effective_view_mode(pane))
                .columns
                .max(1),
        };
        self.move_file_selection_by_step(pane, forward, extend, step)
    }

    pub(in crate::iced_ui) fn move_file_selection_horizontal(
        &mut self,
        pane: PaneId,
        forward: bool,
        extend: bool,
    ) -> Task<Message> {
        if matches!(
            self.effective_view_mode(pane),
            ViewMode::Details | ViewMode::List
        ) {
            return Task::none();
        }
        self.move_file_selection_by_step(pane, forward, extend, 1)
    }

    fn move_file_selection_by_step(
        &mut self,
        pane: PaneId,
        forward: bool,
        extend: bool,
        step: usize,
    ) -> Task<Message> {
        if self.settings_open
            || self.shortcuts_open
            || self.about_open
            || self.rename_dialog.is_some()
            || self.archive_dialog.is_some()
            || self.format_dialog.is_some()
            || self.error_dialog.is_some()
            || self.permanent_delete_dialog.is_some()
            || self.elevated_transfer_dialog.is_some()
            || self.elevated_delete_dialog.is_some()
            || self.elevated_file_action_dialog.is_some()
            || self.context_menu.is_some()
        {
            return Task::none();
        }
        let displayed = self.displayed_entry_indices(pane);
        if displayed.is_empty() {
            return Task::none();
        }

        let current_index = self
            .pane(pane)
            .keyboard_selection_cursor
            .filter(|index| displayed.contains(index))
            .or_else(|| {
                self.pane(pane).selection_anchor.filter(|index| {
                    displayed.contains(index)
                        && self
                            .pane(pane)
                            .entries
                            .get(*index)
                            .is_some_and(|entry| self.pane(pane).selected.contains(&entry.path))
                })
            })
            .or_else(|| {
                displayed.iter().copied().find(|index| {
                    self.pane(pane)
                        .entries
                        .get(*index)
                        .is_some_and(|entry| self.pane(pane).selected.contains(&entry.path))
                })
            });
        let current_position = current_index
            .and_then(|index| displayed.iter().position(|candidate| *candidate == index));
        let Some(target_position) =
            keyboard_selection_target_position(current_position, displayed.len(), step, forward)
        else {
            return Task::none();
        };
        let target_index = displayed[target_position];

        if extend && current_index.is_some() {
            self.select_range_to(pane, target_index);
        } else {
            self.select_single(pane, target_index);
        }
        self.last_entry_click = None;
        Task::batch([
            self.queue_selected_preview(pane),
            self.reveal_keyboard_entry(pane, &displayed, target_position, target_index),
        ])
    }

    fn reveal_keyboard_entry(
        &mut self,
        pane: PaneId,
        displayed: &[usize],
        position: usize,
        index: usize,
    ) -> Task<Message> {
        let target_y = self.displayed_entry_scroll_offset(pane, displayed, position);
        let item_height = match self.effective_view_mode(pane) {
            ViewMode::Details | ViewMode::List => self.detail_row_height(),
            ViewMode::Tiles
            | ViewMode::SmallIcons
            | ViewMode::MediumIcons
            | ViewMode::LargeIcons
            | ViewMode::ExtraLargeIcons => {
                self.visual_layout_for_pane(pane, self.effective_view_mode(pane))
                    .metrics
                    .cell_height
            }
        };
        let action_height = self
            .config
            .show_action_bar
            .then(|| self.action_bar_height());
        let bookmark_height = (self.config.show_bookmark_bar || !self.sidebar_visible)
            .then(|| self.bookmark_bar_height());
        let viewport_height = (self.window_size.height
            - TITLE_HEIGHT
            - self.toolbar_height()
            - action_height.unwrap_or_default()
            - bookmark_height.unwrap_or_default()
            - self.status_bar_height()
            - self.ui_metric(10.0))
        .max(self.ui_metric(80.0));
        let current_y = self.pane(pane).scroll_offset_y;
        let margin = self.ui_metric(6.0);
        let target_bottom = target_y + item_height;
        let next_y = if target_y < current_y + margin {
            Some((target_y - margin).max(0.0))
        } else if target_bottom > current_y + viewport_height - margin {
            Some((target_bottom - viewport_height + margin).max(0.0))
        } else {
            None
        };

        let state = self.pane_mut(pane);
        if let Some(next_y) = next_y {
            state.scroll_offset_y = next_y;
            state.scroll_velocity_y = 0.0;
            state.scroll_sampled_at = None;
        }

        let entry = self.pane(pane).entries.get(index).cloned();
        let mut tasks = Vec::new();
        if let Some(next_y) = next_y {
            tasks.push(iced::widget::operation::scroll_to(
                pane_scroll_id(pane),
                iced::widget::operation::AbsoluteOffset {
                    x: None,
                    y: Some(next_y),
                },
            ));
        }
        if let Some(entry) = entry {
            let variant = if uses_small_entry_images(self.effective_view_mode(pane)) {
                IcedImageVariant::Small
            } else {
                IcedImageVariant::Standard
            };
            tasks.extend(self.queue_entry_images_for_variant(&entry, variant));
        }
        Task::batch(tasks)
    }

    pub(in crate::iced_ui) fn select_entry_starting_with(
        &mut self,
        pane: PaneId,
        character: &str,
    ) -> Task<Message> {
        if character.chars().count() != 1
            || !character.chars().all(char::is_alphanumeric)
            || self.settings_open
            || self.shortcuts_open
            || self.address_edit.is_some()
            || self.rename_dialog.is_some()
            || self.archive_dialog.is_some()
            || self.format_dialog.is_some()
            || self.error_dialog.is_some()
            || self.permanent_delete_dialog.is_some()
            || self.elevated_transfer_dialog.is_some()
            || self.elevated_delete_dialog.is_some()
            || self.elevated_file_action_dialog.is_some()
            || self.context_menu.is_some()
        {
            return Task::none();
        }

        let displayed = self.displayed_entry_indices(pane);
        let names = displayed
            .iter()
            .filter_map(|index| self.pane(pane).entries.get(*index))
            .map(|entry| self.entry_display_name(entry))
            .collect::<Vec<_>>();
        let selected_position = self
            .pane(pane)
            .selection_anchor
            .and_then(|anchor| displayed.iter().position(|index| *index == anchor));
        let Some(position) = next_matching_name_position(&names, selected_position, character)
        else {
            return Task::none();
        };
        let Some(index) = displayed.get(position).copied() else {
            return Task::none();
        };

        self.select_single(pane, index);
        self.last_entry_click = None;
        let relative_offset = if names.len() > 1 {
            position as f32 / (names.len() - 1) as f32
        } else {
            0.0
        };
        Task::batch([
            self.queue_selected_preview(pane),
            iced::widget::operation::snap_to(
                pane_scroll_id(pane),
                iced::widget::operation::RelativeOffset {
                    x: 0.0,
                    y: relative_offset,
                },
            ),
        ])
    }

    pub(in crate::iced_ui) fn rename_selected(&mut self, pane: PaneId) -> Task<Message> {
        let selected: Vec<_> = self.pane(pane).selected.iter().cloned().collect();
        if selected.is_empty() {
            self.pane_mut(pane).status = "No selected items".into();
            return Task::none();
        }
        if selected.len() > 1 {
            self.pane_mut(pane).status = "Select one item to rename".into();
            return Task::none();
        }
        let path = &selected[0];
        let Some(index) = self
            .pane(pane)
            .entries
            .iter()
            .position(|entry| entry.path == *path)
        else {
            self.pane_mut(pane).status = "Selected item is no longer available".into();
            return Task::none();
        };
        self.context_begin_rename(pane, ContextTarget::Entry(index))
    }

    pub(in crate::iced_ui) fn handle_keyboard_shortcut(
        &mut self,
        shortcut: KeyboardShortcut,
    ) -> Task<Message> {
        if self.permanent_delete_dialog.is_some() {
            return if shortcut == KeyboardShortcut::Open {
                self.confirm_permanent_delete()
            } else {
                Task::none()
            };
        }
        if shortcut == KeyboardShortcut::Open {
            if self
                .suppress_open_after_rename_until
                .is_some_and(|until| Instant::now() < until)
            {
                return Task::none();
            }
            self.suppress_open_after_rename_until = None;
        }
        let pane = self.focused_pane();
        // The text input submits the rename and clears `rename_dialog` before
        // the same Enter can be observed by the global shortcut listener.
        // Keep shortcuts inert until that filesystem operation finishes so
        // Enter cannot immediately try to open the old, now-renamed path.
        if self.pending_file_operations.contains(&pane) {
            return Task::none();
        }
        if self.settings_open
            || self.shortcuts_open
            || self.about_open
            || self.rename_dialog.is_some()
            || self.archive_dialog.is_some()
            || self.format_dialog.is_some()
            || self.error_dialog.is_some()
            || self.elevated_transfer_dialog.is_some()
            || self.elevated_delete_dialog.is_some()
            || self.elevated_file_action_dialog.is_some()
        {
            return Task::none();
        }

        match shortcut {
            KeyboardShortcut::Copy => self.context_copy(pane, ContextTarget::Background, false),
            KeyboardShortcut::Paste => self.context_paste(pane, ContextTarget::Background),
            KeyboardShortcut::Cut => self.context_copy(pane, ContextTarget::Background, true),
            KeyboardShortcut::Undo => self.undo_last_action(),
            KeyboardShortcut::Refresh => self.start_load(pane),
            KeyboardShortcut::Delete => self.delete_selection(pane, false),
            KeyboardShortcut::PermanentDelete => self.delete_selection(pane, true),
            KeyboardShortcut::SelectAll => {
                self.select_all(pane);
                Task::none()
            }
            KeyboardShortcut::Rename => self.rename_selected(pane),
            KeyboardShortcut::EditAddress => self.update(Message::BeginAddressEdit(pane)),
            KeyboardShortcut::Properties => self.selection_properties(pane),
            KeyboardShortcut::GoUp => self.update(Message::Up(pane)),
            KeyboardShortcut::GoBack => self.update(Message::Back(pane)),
            KeyboardShortcut::GoForward => self.update(Message::Forward(pane)),
            KeyboardShortcut::Open => self.open_selected(pane),
            KeyboardShortcut::SwitchPaneFocus => {
                if self.split.is_none() {
                    return Task::none();
                }
                let target = match pane {
                    PaneId::Primary => PaneId::Secondary,
                    PaneId::Secondary => PaneId::Primary,
                };
                self.address_edit = None;
                self.last_entry_click = None;
                self.focus_pane(target);
                // Focusing a logical, non-text file-surface target makes the
                // focus operation release every rendered text input. File
                // navigation itself is tracked by `focused_pane`.
                iced::widget::operation::focus(Id::new("file-surface-keyboard-focus"))
            }
            KeyboardShortcut::FocusSearch => {
                self.address_edit = None;
                self.focus_pane(pane);
                focus_search_input_task(pane)
            }
        }
    }

    pub(in crate::iced_ui) fn handle_rename_clipboard_shortcut(
        &mut self,
        shortcut: RenameClipboardShortcut,
    ) -> Task<Message> {
        let Some(dialog) = &mut self.rename_dialog else {
            return Task::none();
        };

        match shortcut {
            RenameClipboardShortcut::Copy => {
                let Some(selection) = dialog
                    .editor
                    .selection()
                    .filter(|selection| !selection.is_empty())
                else {
                    return Task::none();
                };
                self.file_clipboard = None;
                iced::clipboard::write(selection)
            }
            RenameClipboardShortcut::Cut => {
                let Some(selection) = dialog
                    .editor
                    .selection()
                    .filter(|selection| !selection.is_empty())
                else {
                    return Task::none();
                };
                dialog
                    .editor
                    .perform(text_editor::Action::Edit(text_editor::Edit::Delete));
                dialog.value = dialog.editor.text();
                self.file_clipboard = None;
                Task::batch([
                    iced::clipboard::write(selection),
                    iced::widget::operation::focus(inline_rename_input_id()),
                ])
            }
            RenameClipboardShortcut::Paste => {
                iced::clipboard::read().map(Message::RenameClipboardPaste)
            }
            RenameClipboardShortcut::SelectAll => {
                dialog.editor.perform(text_editor::Action::SelectAll);
                iced::widget::operation::focus(inline_rename_input_id())
            }
        }
    }

    pub(in crate::iced_ui) fn open_selected(&mut self, pane: PaneId) -> Task<Message> {
        self.focus_pane(pane);
        let selected_index = self
            .pane(pane)
            .selection_anchor
            .filter(|index| {
                self.pane(pane)
                    .entries
                    .get(*index)
                    .is_some_and(|entry| self.pane(pane).selected.contains(&entry.path))
            })
            .or_else(|| {
                self.pane(pane)
                    .entries
                    .iter()
                    .position(|entry| self.pane(pane).selected.contains(&entry.path))
            });
        let Some(index) = selected_index else {
            self.pane_mut(pane).status = "No hay ningún elemento seleccionado".into();
            return Task::none();
        };
        self.context_open(pane, ContextTarget::Entry(index))
    }

    pub(in crate::iced_ui) fn delete_selection(
        &mut self,
        pane: PaneId,
        permanent: bool,
    ) -> Task<Message> {
        self.focus_pane(pane);
        if self.is_trash_pane(pane) {
            return self.request_trash_selection_purge(pane);
        }
        self.context_delete(pane, ContextTarget::Background, permanent)
    }
}

fn keyboard_selection_target_position(
    current: Option<usize>,
    total: usize,
    step: usize,
    forward: bool,
) -> Option<usize> {
    if total == 0 {
        return None;
    }
    let step = step.max(1);
    Some(match current {
        None if forward => 0,
        None => total - 1,
        Some(position) if forward => position.saturating_add(step).min(total - 1),
        Some(position) => position.saturating_sub(step),
    })
}

fn next_matching_name_position(
    names: &[String],
    selected_position: Option<usize>,
    character: &str,
) -> Option<usize> {
    let prefix = character.to_lowercase();
    let last_position = selected_position
        .filter(|position| *position < names.len())
        .unwrap_or_else(|| names.len().saturating_sub(1));
    (1..=names.len())
        .map(|offset| (last_position + offset) % names.len())
        .find(|position| names[*position].to_lowercase().starts_with(&prefix))
}

#[cfg(test)]
mod tests {
    use super::{keyboard_selection_target_position, next_matching_name_position};

    #[test]
    fn keyboard_selection_starts_at_the_nearest_edge() {
        assert_eq!(
            keyboard_selection_target_position(None, 5, 1, true),
            Some(0)
        );
        assert_eq!(
            keyboard_selection_target_position(None, 5, 1, false),
            Some(4)
        );
        assert_eq!(keyboard_selection_target_position(None, 0, 1, true), None);
    }

    #[test]
    fn keyboard_selection_moves_by_rows_and_stays_in_bounds() {
        assert_eq!(
            keyboard_selection_target_position(Some(2), 10, 1, true),
            Some(3)
        );
        assert_eq!(
            keyboard_selection_target_position(Some(2), 10, 1, false),
            Some(1)
        );
        assert_eq!(
            keyboard_selection_target_position(Some(1), 10, 4, true),
            Some(5)
        );
        assert_eq!(
            keyboard_selection_target_position(Some(5), 10, 4, false),
            Some(1)
        );
        assert_eq!(
            keyboard_selection_target_position(Some(8), 10, 4, true),
            Some(9)
        );
        assert_eq!(
            keyboard_selection_target_position(Some(1), 10, 4, false),
            Some(0)
        );
    }

    #[test]
    fn name_navigation_cycles_through_matching_entries() {
        let names = vec![
            "Archivo.txt".into(),
            "Borrador.txt".into(),
            "Biblioteca".into(),
            "Documento.txt".into(),
        ];

        assert_eq!(next_matching_name_position(&names, Some(1), "b"), Some(2));
        assert_eq!(next_matching_name_position(&names, Some(2), "B"), Some(1));
        assert_eq!(next_matching_name_position(&names, None, "d"), Some(3));
        assert_eq!(next_matching_name_position(&names, Some(3), "z"), None);
    }
}
