use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;

use crate::app::config::ViewMode;
use crate::fs::explorer;
use crate::fs::portable;
use crate::fs::transfer_queue::TransferKind;

use super::BExplorerApp;
use super::entry_utils::{entry_starts_with, normalized_type_select_char, visible_entry_index};
use super::navigation::{archive_item_path, display_path_name, normalize_existing_path};
use super::types::{DragPrimary, DragSelection, FileDrag, FileDragFeedback, TypeSelectState};

impl BExplorerApp {
    pub fn clear_selection(&mut self) {
        self.rename_dialog = None;
        self.selected.clear();
        self.selection_anchor = None;
        self.selection_focus = None;
        self.type_select = None;
        self.pending_scroll_path = None;
    }

    pub fn active_view_mode(&self) -> ViewMode {
        self.tabs
            .get(self.active_tab)
            .map(|tab| tab.view_mode)
            .unwrap_or(self.config.default_view)
    }

    pub fn set_active_view_mode(&mut self, view_mode: ViewMode) {
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            if tab.view_mode == view_mode {
                return;
            }
            tab.view_mode = view_mode;
        } else {
            return;
        }
        self.remember_special_view_preference(view_mode);
        self.clear_selection();
        self.persist_session();
    }

    pub fn cycle_view_mode(&mut self, direction: i32) {
        let all = ViewMode::ALL;
        let len = all.len() as i32;
        let current_index = all
            .iter()
            .position(|v| *v == self.active_view_mode())
            .unwrap_or(0) as i32;
        let new_index = (current_index + direction).rem_euclid(len) as usize;
        self.set_active_view_mode(all[new_index]);
    }

    pub fn select_entry(&mut self, path: PathBuf, additive: bool, range: bool) {
        if self
            .rename_dialog
            .as_ref()
            .is_some_and(|dialog| dialog.path != path)
        {
            self.rename_dialog = None;
        }
        if range {
            self.select_entry_range(path, additive);
            return;
        }
        if additive {
            if !self.selected.remove(&path) {
                self.selected.insert(path.clone());
            }
        } else {
            self.selected.clear();
            self.selected.insert(path.clone());
        }
        self.selection_anchor = Some(path.clone());
        self.selection_focus = Some(path);
    }

    fn select_entry_range(&mut self, path: PathBuf, additive: bool) {
        let entries = self.filtered_entries();
        let target_index = visible_entry_index(&entries, &path);
        let anchor = self
            .selection_anchor
            .clone()
            .filter(|anchor| visible_entry_index(&entries, anchor).is_some())
            .or_else(|| {
                entries
                    .iter()
                    .find(|entry| self.selected.contains(&entry.path))
                    .map(|entry| entry.path.clone())
            })
            .unwrap_or_else(|| path.clone());
        let anchor_index = visible_entry_index(&entries, &anchor);

        let (Some(anchor_index), Some(target_index)) = (anchor_index, target_index) else {
            if !additive {
                self.selected.clear();
            }
            self.selected.insert(path.clone());
            self.selection_anchor = Some(path.clone());
            self.selection_focus = Some(path);
            return;
        };

        if !additive {
            self.selected.clear();
        }
        let start = anchor_index.min(target_index);
        let end = anchor_index.max(target_index);
        for entry in &entries[start..=end] {
            self.selected.insert(entry.path.clone());
        }
        self.selection_anchor = Some(anchor);
        self.selection_focus = Some(path);
    }

    pub fn select_all(&mut self) {
        let entries = self.filtered_entries();
        self.selection_anchor = entries.first().map(|entry| entry.path.clone());
        self.selection_focus = entries.last().map(|entry| entry.path.clone());
        self.selected = entries.into_iter().map(|entry| entry.path).collect();
    }

    pub fn move_selection(&mut self, direction: isize, range: bool) {
        let entries = self.filtered_entries();
        if entries.is_empty() {
            self.selected.clear();
            self.selection_anchor = None;
            self.selection_focus = None;
            return;
        }

        let selected_indexes: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| self.selected.contains(&entry.path).then_some(index))
            .collect();
        let focus_index = self
            .selection_focus
            .as_ref()
            .and_then(|focus| visible_entry_index(&entries, focus));
        let current_index = if range {
            let focus_index = focus_index.unwrap_or_else(|| {
                if direction < 0 {
                    selected_indexes
                        .first()
                        .copied()
                        .unwrap_or(entries.len() - 1)
                } else {
                    selected_indexes.last().copied().unwrap_or(0)
                }
            });
            (focus_index as isize + direction).clamp(0, entries.len() as isize - 1) as usize
        } else if selected_indexes.is_empty() {
            if direction < 0 { entries.len() - 1 } else { 0 }
        } else if direction < 0 {
            selected_indexes[0].saturating_sub(1)
        } else {
            (selected_indexes[selected_indexes.len() - 1] + 1).min(entries.len() - 1)
        };

        let path = entries[current_index].path.clone();
        if range {
            if self.selection_anchor.is_none() {
                self.selection_anchor = selected_indexes
                    .first()
                    .and_then(|index| entries.get(*index))
                    .map(|entry| entry.path.clone())
                    .or_else(|| Some(path.clone()));
            }
            self.select_entry_range(path, false);
        } else {
            self.selected.clear();
            self.selected.insert(path.clone());
            self.selection_anchor = Some(path.clone());
            self.selection_focus = Some(path);
            self.pending_scroll_path = self.selection_focus.clone();
        }
    }

    pub fn select_next_entry_starting_with(&mut self, character: char) {
        let Some(character) = normalized_type_select_char(character) else {
            return;
        };
        let entries = self.filtered_entries();
        if entries.is_empty() {
            return;
        }

        let now = Instant::now();
        let repeated = self.type_select.as_ref().is_some_and(|state| {
            state.character == character && state.updated_at.elapsed() < Duration::from_secs(2)
        });
        let focus_index = self
            .selection_focus
            .as_ref()
            .and_then(|focus| visible_entry_index(&entries, focus));
        let start = if repeated {
            focus_index.map(|index| index + 1).unwrap_or(0)
        } else {
            focus_index.unwrap_or(0)
        };

        let target = (0..entries.len())
            .map(|offset| (start + offset) % entries.len())
            .find(|index| entry_starts_with(&entries[*index], character));

        if let Some(index) = target {
            let path = entries[index].path.clone();
            self.selected.clear();
            self.selected.insert(path.clone());
            self.selection_anchor = Some(path.clone());
            self.selection_focus = Some(path.clone());
            self.pending_scroll_path = Some(path);
            self.type_select = Some(TypeSelectState {
                character,
                updated_at: now,
            });
        }
    }

    pub fn take_pending_scroll_path(&mut self) -> Option<PathBuf> {
        self.pending_scroll_path.take()
    }

    pub fn begin_drag_selection(&mut self, start: egui::Pos2, additive: bool) {
        let base_selected = if additive {
            self.selected.clone()
        } else {
            BTreeSet::new()
        };
        if !additive {
            self.selection_anchor = None;
            self.selection_focus = None;
        }
        self.selected = base_selected.clone();
        self.drag_selection = Some(DragSelection {
            start,
            current: start,
            base_selected,
        });
    }

    pub fn update_drag_selection(&mut self, current: egui::Pos2) {
        if let Some(selection) = &mut self.drag_selection {
            selection.current = current;
        }
    }

    pub fn prepare_drag_selection_frame(&mut self) {
        if let Some(selection) = &self.drag_selection {
            self.selected = selection.base_selected.clone();
        }
    }

    pub fn add_drag_selected(&mut self, path: PathBuf) {
        self.selected.insert(path);
    }

    pub fn drag_selection_rect(&self) -> Option<egui::Rect> {
        self.drag_selection.as_ref().map(|selection| {
            egui::Rect::from_two_pos(selection.start, selection.current).expand(0.5)
        })
    }

    pub fn ensure_selected(&mut self, path: PathBuf) {
        if !self.selected.contains(&path) {
            self.selected.clear();
            self.selected.insert(path.clone());
            self.selection_anchor = Some(path.clone());
            self.selection_focus = Some(path);
            self.pending_scroll_path = self.selection_focus.clone();
        }
    }

    pub fn begin_file_drag(&mut self, path: PathBuf) {
        if !self.selected.contains(&path) {
            self.selected.clear();
            self.selected.insert(path.clone());
            self.selection_anchor = Some(path.clone());
            self.selection_focus = Some(path);
        }
        let paths = self.selected_paths();
        if paths.is_empty() {
            return;
        }

        self.drag_selection = None;
        let primary = paths.first().map(|primary_path| DragPrimary {
            path: primary_path.clone(),
            is_directory: if explorer::is_portable_path(primary_path) {
                portable::path_is_folder(primary_path)
            } else {
                primary_path.is_dir()
            },
        });
        self.file_drag = Some(FileDrag {
            paths,
            target: None,
            primary,
        });
    }

    pub fn prepare_file_drag_frame(&mut self) {
        self.file_drag_folder_rects.clear();
        if let Some(drag) = self.file_drag.as_mut() {
            drag.target = None;
        }
    }

    pub fn register_file_drag_folder_rect(&mut self, path: PathBuf, rect: egui::Rect) {
        self.file_drag_folder_rects.push((path, rect));
    }

    pub fn resolve_file_drag_target(&mut self, ctx: &egui::Context) {
        if !self.file_drag_active() {
            return;
        }
        let Some(pointer) = ctx.input(|input| input.pointer.hover_pos()) else {
            return;
        };
        for (path, rect) in self.file_drag_folder_rects.iter().rev() {
            if rect.contains(pointer) {
                self.set_file_drag_target(path.clone());
                return;
            }
        }
    }

    pub fn file_drag_active(&self) -> bool {
        self.file_drag.is_some()
    }

    pub fn file_drag_feedback(&self) -> Option<FileDragFeedback> {
        let drag = self.file_drag.as_ref()?;
        let item_count = drag.paths.len();
        let item_name = if item_count == 1 {
            drag.paths
                .first()
                .map(|path| display_path_name(path))
                .unwrap_or_else(|| "Item".into())
        } else {
            format!("{item_count} items")
        };
        let target_name = drag.target.as_ref().map(|target| display_path_name(target));
        let copy = drag.paths.iter().all(|source| archive_item_path(source))
            || drag
                .paths
                .iter()
                .any(|source| explorer::is_portable_path(source))
            || drag
                .target
                .as_ref()
                .is_some_and(|target| explorer::is_portable_path(target));

        Some(FileDragFeedback {
            item_name,
            item_count,
            target_name,
            copy,
        })
    }

    pub fn can_drop_file_drag_to(&self, target: &Path) -> bool {
        let Some(drag) = self.file_drag.as_ref() else {
            return false;
        };
        let target_is_portable = explorer::is_portable_path(target);
        if !target.is_dir() && !target_is_portable {
            return false;
        }
        if drag.paths.iter().all(|source| archive_item_path(source)) {
            return target.is_dir()
                && !crate::fs::archive_listing::is_archive_navigation_path(target);
        }
        if drag.paths.iter().any(|source| archive_item_path(source)) {
            return false;
        }
        let portable_sources = drag
            .paths
            .iter()
            .filter(|source| explorer::is_portable_path(source))
            .count();
        if target_is_portable {
            return portable_sources == 0
                && drag.paths.iter().all(|source| source.exists())
                && !drag.paths.is_empty();
        }
        if portable_sources > 0 {
            return portable_sources == drag.paths.len() && target.is_dir();
        }

        let Some(target_abs) = normalize_existing_path(target) else {
            return false;
        };
        let mut useful_destination = false;
        for source in &drag.paths {
            let Some(source_abs) = normalize_existing_path(source) else {
                continue;
            };
            if source_abs == target_abs {
                return false;
            }
            if source_abs.is_dir() && target_abs.starts_with(&source_abs) {
                return false;
            }
            if source_abs.parent() != Some(target_abs.as_path()) {
                useful_destination = true;
            }
        }
        useful_destination
    }

    pub fn set_file_drag_target(&mut self, target: PathBuf) {
        if self.can_drop_file_drag_to(&target) {
            if let Some(drag) = self.file_drag.as_mut() {
                drag.target = Some(target);
            }
        }
    }

    pub(super) fn finish_file_drag_if_released(&mut self, ctx: &egui::Context) {
        if !ctx.input(|input| input.pointer.primary_released()) {
            return;
        }

        let Some(drag) = self.file_drag.take() else {
            return;
        };
        if let Some(target) = drag.target {
            if drag.paths.iter().all(|source| archive_item_path(source)) {
                self.extract_archive_items_to(drag.paths, target);
                return;
            }
            self.queue_transfer(drag.paths, target, TransferKind::Move);
        }
    }

    pub fn mark_text_input_active(&mut self) {
        self.text_input_active = true;
    }

    pub fn clear_text_input_active(&mut self) {
        self.text_input_active = false;
    }

    pub fn set_preview_text_selection(
        &mut self,
        path: &Path,
        text: String,
        range: egui::text::CCursorRange,
    ) {
        if text.is_empty() {
            self.clear_preview_text_selection(path);
        } else {
            self.preview_text_selection = Some((path.to_path_buf(), text, range));
        }
    }

    pub fn clear_preview_text_selection(&mut self, path: &Path) {
        if self
            .preview_text_selection
            .as_ref()
            .is_some_and(|(selection_path, _, _)| selection_path == path)
        {
            self.preview_text_selection = None;
        }
    }

    pub fn preview_text_selection(&self, path: &Path) -> Option<&str> {
        self.preview_text_selection
            .as_ref()
            .filter(|(selection_path, _, _)| selection_path == path)
            .map(|(_, text, _)| text.as_str())
    }

    pub fn preview_text_selection_range(&self, path: &Path) -> Option<egui::text::CCursorRange> {
        self.preview_text_selection
            .as_ref()
            .filter(|(selection_path, _, _)| selection_path == path)
            .map(|(_, _, range)| *range)
    }
}
