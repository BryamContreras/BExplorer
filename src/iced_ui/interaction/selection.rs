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
        state.status = format!("Selected {count} item(s)");
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
            || self.rename_dialog.is_some()
            || self.archive_dialog.is_some()
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
            KeyboardShortcut::Properties => {
                self.context_properties(pane, ContextTarget::Background);
                Task::none()
            }
            KeyboardShortcut::GoUp => self.update(Message::Up(pane)),
            KeyboardShortcut::GoBack => self.update(Message::Back(pane)),
            KeyboardShortcut::GoForward => self.update(Message::Forward(pane)),
            KeyboardShortcut::Open => self.open_selected(pane),
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
        self.context_delete(pane, ContextTarget::Background, permanent)
    }
}
