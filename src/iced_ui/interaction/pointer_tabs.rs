use super::*;

impl BExplorerIced {
    pub(in crate::iced_ui) fn handle_pointer_moved(&mut self, position: Point) {
        self.cursor_position = position;
        self.update_tab_drag_position(position);
        self.update_sidebar_section_drag_position(position);
        self.update_file_drag_cursor_position(position);

        let Some(drag) = self.resize_drag else {
            return;
        };

        match drag {
            ResizeDrag::Sidebar {
                start_x,
                start_width,
            } => {
                if !start_x.is_finite() {
                    self.resize_drag = Some(ResizeDrag::Sidebar {
                        start_x: position.x,
                        start_width,
                    });
                    return;
                }
                let width = (start_width + position.x - start_x)
                    .clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH);
                self.config.sidebar_width = width;
                self.config.sidebar_visible = true;
                self.sidebar_visible = true;
                self.sidebar_progress = 1.0;
            }
            ResizeDrag::Split {
                start_x,
                start_ratio,
            } => {
                if !start_x.is_finite() {
                    self.resize_drag = Some(ResizeDrag::Split {
                        start_x: position.x,
                        start_ratio,
                    });
                    return;
                }
                let sidebar_width = self.current_sidebar_width();
                let content_width = (self.window_size.width - sidebar_width).max(1.0);
                let available = (content_width - SPLIT_DIVIDER_WIDTH).max(1.0);
                let ratio = (start_ratio + (position.x - start_x) / available)
                    .clamp(SPLIT_MIN_RATIO, SPLIT_MAX_RATIO);
                if let Some(split) = &mut self.split {
                    split.ratio = ratio;
                }
            }
            ResizeDrag::Column {
                pane,
                column,
                start_x,
                start_width,
            } => {
                if !start_x.is_finite() {
                    self.resize_drag = Some(ResizeDrag::Column {
                        pane,
                        column,
                        start_x: position.x,
                        start_width,
                    });
                    return;
                }
                let width = clamp_detail_column_width(column, start_width + position.x - start_x);
                self.pane_mut(pane).column_widths.set(column, width);
            }
            ResizeDrag::Preview {
                pane,
                start_x,
                start_width,
            } => {
                if !start_x.is_finite() {
                    self.resize_drag = Some(ResizeDrag::Preview {
                        pane,
                        start_x: position.x,
                        start_width,
                    });
                    return;
                }
                self.config.preview_panel_width =
                    (start_width - (position.x - start_x)).clamp(220.0, 560.0);
            }
        }
    }

    pub(in crate::iced_ui) fn update_tab_drag_position(&mut self, position: Point) {
        let Some(mut drag) = self.tab_drag else {
            return;
        };
        if !drag.start_cursor_x.is_finite() {
            drag.start_cursor_x = position.x;
            drag.start_cursor_y = position.y;
            self.tab_drag = Some(drag);
            return;
        }

        let delta_x = position.x - drag.start_cursor_x;
        let delta_y = position.y - drag.start_cursor_y;
        drag.offset_x = delta_x.clamp(-TAB_DRAG_MAX_OFFSET, TAB_DRAG_MAX_OFFSET);
        if delta_x.abs().max(delta_y.abs()) >= TAB_DRAG_START_THRESHOLD {
            drag.dragging = true;
        }
        let dragging = drag.dragging;
        self.tab_drag = Some(drag);

        if dragging && !self.is_tab_split_drop_position(position.y) {
            self.reorder_tab_drag_at_position(position.x);
        }
    }

    pub(in crate::iced_ui) fn is_tab_split_drop_position(&self, y: f32) -> bool {
        y >= TAB_SPLIT_DROP_TRIGGER_Y
    }

    pub(in crate::iced_ui) fn tab_split_drop_side(&self, position: Point) -> Option<SplitSide> {
        self.is_tab_split_drop_position(position.y).then_some(
            if position.x < self.window_size.width * 0.5 {
                SplitSide::Left
            } else {
                SplitSide::Right
            },
        )
    }

    pub(in crate::iced_ui) fn place_tab_in_split(
        &mut self,
        tab_index: usize,
        side: SplitSide,
    ) -> Task<Message> {
        if self.split.is_some() || self.tabs.len() < 2 || tab_index >= self.tabs.len() {
            return Task::none();
        }

        let remaining_tabs = (0..self.tabs.len())
            .filter(|index| *index != tab_index)
            .collect::<Vec<_>>();
        let fallback = remaining_tabs.first().copied().unwrap_or(0);
        let prior_active = self.active_tab;
        let primary_active = if prior_active == tab_index {
            fallback
        } else {
            prior_active
        };

        let (primary_tabs, secondary_tabs, active_tab, secondary_tab, focused) = match side {
            SplitSide::Left => (
                vec![tab_index],
                remaining_tabs,
                tab_index,
                primary_active,
                SplitFocus::Primary,
            ),
            SplitSide::Right => (
                remaining_tabs,
                vec![tab_index],
                primary_active,
                tab_index,
                SplitFocus::Secondary,
            ),
        };

        self.active_tab = active_tab;
        self.split = Some(SplitRuntime {
            primary_tabs,
            secondary_tabs,
            secondary_tab,
            focused,
            ratio: 0.5,
        });
        self.focus_pane(match side {
            SplitSide::Left => PaneId::Primary,
            SplitSide::Right => PaneId::Secondary,
        });
        self.save_session();

        Task::batch([
            self.start_navigation_load(PaneId::Primary),
            self.start_navigation_load(PaneId::Secondary),
        ])
    }

    pub(in crate::iced_ui) fn reorder_tab_drag_at_position(&mut self, x: f32) {
        let Some(drag) = self.tab_drag else {
            return;
        };
        let Some(insertion_slot) = self.tab_insertion_slot_at(drag.pane, x) else {
            return;
        };
        let old_slot = drag.slot;
        let split_none = self.split.is_none();
        if let Some(new_slot) = self.reorder_dragged_tab(drag.pane, drag.tab_index, insertion_slot)
        {
            let slot_delta = new_slot as f32 - old_slot as f32;
            if let Some(tab_drag) = &mut self.tab_drag {
                tab_drag.slot = new_slot;
                if split_none {
                    tab_drag.tab_index = new_slot;
                }
                if slot_delta != 0.0 {
                    tab_drag.start_cursor_x += slot_delta * TAB_DRAG_STRIDE;
                    tab_drag.offset_x = (self.cursor_position.x - tab_drag.start_cursor_x)
                        .clamp(-TAB_DRAG_MAX_OFFSET, TAB_DRAG_MAX_OFFSET);
                    tab_drag.dirty = true;
                }
            }
        }
    }

    pub(in crate::iced_ui) fn tab_insertion_slot_at(&self, pane: PaneId, x: f32) -> Option<usize> {
        let (start, width) = self.title_pane_bounds(pane);
        let count = self.tab_indices_for_pane(pane).len();
        if count <= 1 || width <= 0.0 || x < start - TAB_WIDTH || x > start + width + TAB_WIDTH {
            return None;
        }

        let local_x = (x - start).max(0.0);
        let slot = (local_x / TAB_DRAG_STRIDE).floor().max(0.0) as usize;
        let within_slot = local_x - slot as f32 * TAB_DRAG_STRIDE;
        let insertion_slot = slot + usize::from(within_slot > TAB_WIDTH * 0.5);
        Some(insertion_slot.min(count))
    }

    pub(in crate::iced_ui) fn title_pane_bounds(&self, pane: PaneId) -> (f32, f32) {
        let start = self.title_tabs_start_x();
        let area_width = (self.window_size.width - start).max(1.0);
        if let Some(split) = &self.split {
            let available = (area_width - SPLIT_DIVIDER_WIDTH).max(1.0);
            let primary_width = available * split.ratio;
            match pane {
                PaneId::Primary => (start, primary_width),
                PaneId::Secondary => (
                    start + primary_width + SPLIT_DIVIDER_WIDTH,
                    available - primary_width,
                ),
            }
        } else {
            (start, area_width)
        }
    }

    pub(in crate::iced_ui) fn title_tabs_start_x(&self) -> f32 {
        let left_buttons = TITLE_BUTTON_WIDTH * 2.0 + TITLE_BUTTON_GAP;
        if self.sidebar_is_rendered() {
            self.current_sidebar_width().max(left_buttons)
        } else {
            left_buttons + TITLE_TAB_START_PADDING
        }
    }

    pub(in crate::iced_ui) fn update_sidebar_section_drag_position(&mut self, position: Point) {
        let Some(mut drag) = self.sidebar_section_drag else {
            return;
        };
        if !drag.start_cursor_y.is_finite() {
            drag.start_cursor_y = position.y;
            self.sidebar_section_drag = Some(drag);
            return;
        }

        let delta = position.y - drag.start_cursor_y;
        drag.offset_y = delta;
        if delta.abs() >= SIDEBAR_SECTION_DRAG_START_THRESHOLD {
            drag.dragging = true;
        }
        let dragging = drag.dragging;
        self.sidebar_section_drag = Some(drag);

        if dragging {
            self.reorder_sidebar_section_at_position(position.y);
        }
    }

    pub(in crate::iced_ui) fn reorder_sidebar_section_at_position(&mut self, y: f32) {
        let Some(drag) = self.sidebar_section_drag else {
            return;
        };
        let Some((target, after)) = self.sidebar_section_target_at_y(y) else {
            return;
        };
        if drag.section == target {
            return;
        }

        if let Some((new_slot, top_delta)) =
            self.reorder_sidebar_section(drag.section, target, after)
            && let Some(section_drag) = &mut self.sidebar_section_drag
        {
            section_drag.slot = new_slot;
            if top_delta != 0.0 {
                section_drag.start_cursor_y += top_delta;
                section_drag.offset_y = self.cursor_position.y - section_drag.start_cursor_y;
                section_drag.dirty = true;
            }
        }
    }

    pub(in crate::iced_ui) fn sidebar_section_target_at_y(
        &self,
        y: f32,
    ) -> Option<(SidebarSection, bool)> {
        if !self.sidebar_is_rendered() {
            return None;
        }

        let mut top = TITLE_HEIGHT + 8.0;
        for section in self.config.normalized_sidebar_order() {
            if !self.sidebar_section_visible(section) {
                continue;
            }
            let height = self.sidebar_section_layout_height(section);
            if y >= top && y <= top + height {
                return Some((section, y > top + height * 0.5));
            }
            top += height + 1.0;
        }
        None
    }
}
