use super::*;

impl BExplorerIced {
    pub(in crate::iced_ui) fn current_sidebar_width(&self) -> f32 {
        let configured_width = self
            .config
            .sidebar_width
            .clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH);
        configured_width * self.sidebar_progress.clamp(0.0, 1.0)
    }

    pub(in crate::iced_ui) fn uses_split_sidebars(&self) -> bool {
        self.split.is_some() && self.config.show_split_pane_menus
    }

    pub(in crate::iced_ui) fn uses_split_preview_panels(&self) -> bool {
        self.split.is_some() && self.config.show_split_preview_panels
    }

    pub(in crate::iced_ui) fn preview_enabled_for_pane(&self, pane: PaneId) -> bool {
        self.config.show_preview_panel
            && (self.uses_split_preview_panels() || self.preview_panel_pane == Some(pane))
    }

    pub(in crate::iced_ui) fn sidebar_animation_active(&self) -> bool {
        let target = if self.sidebar_visible { 1.0 } else { 0.0 };
        (self.sidebar_progress - target).abs() > f32::EPSILON
    }

    pub(in crate::iced_ui) fn sidebar_is_rendered(&self) -> bool {
        self.sidebar_progress > 0.001
    }

    pub(in crate::iced_ui) fn preview_panel_animation_active(&self) -> bool {
        let target = self.preview_panel_animation_target();
        (self.preview_panel_progress - target).abs() > f32::EPSILON
    }

    pub(in crate::iced_ui) fn popup_fade_animation_active(&self) -> bool {
        (self.popup_fade_progress - self.popup_fade_target).abs() > f32::EPSILON
            || (self.color_picker_fade_progress - self.color_picker_fade_target).abs()
                > f32::EPSILON
    }

    pub(in crate::iced_ui) fn file_drag_fade_animation_active(&self) -> bool {
        (self.file_drag_fade_progress - self.file_drag_fade_target).abs() > f32::EPSILON
    }

    pub(in crate::iced_ui) fn scrollbar_animation_active(&self) -> bool {
        let now = Instant::now();
        [PaneId::Primary, PaneId::Secondary]
            .into_iter()
            .any(|pane| {
                let state = self.pane(pane);
                let wheel_reveal = state
                    .scrollbar_reveal_until
                    .is_some_and(|until| until > now);
                let target = f32::from(
                    state.scrollbar_horizontal_hovered
                        || state.scrollbar_vertical_hovered
                        || wheel_reveal,
                );
                wheel_reveal || (state.scrollbar_reveal_progress - target).abs() > f32::EPSILON
            })
    }

    pub(in crate::iced_ui) fn async_progress_animation_active(&self) -> bool {
        [PaneId::Primary, PaneId::Secondary]
            .into_iter()
            .any(|pane| {
                let state = self.pane(pane);
                state.loading || state.mounting_disk_image || state.search_receiver.is_some()
            })
    }

    pub(in crate::iced_ui) fn preview_panel_animation_target(&self) -> f32 {
        if self
            .preview_panel_target_pane
            .is_some_and(|pane| self.preview_panel_pane != Some(pane))
        {
            0.0
        } else if self.config.show_preview_panel {
            1.0
        } else {
            0.0
        }
    }

    pub(in crate::iced_ui) fn current_preview_panel_width(&self) -> f32 {
        self.config.preview_panel_width.clamp(220.0, 560.0)
            * self.preview_panel_progress.clamp(0.0, 1.0)
    }

    pub(in crate::iced_ui) fn pointer_tracking_active(&self) -> bool {
        self.sidebar_pointer_inside
            || self.resize_drag.is_some()
            || self.tab_drag.is_some()
            || self.sidebar_section_drag.is_some()
            || self.rubber_band.is_some()
            || self.file_drag.is_some()
    }

    pub(in crate::iced_ui) fn sidebar_section_expanded(&self, section: SidebarSection) -> bool {
        !self.config.sidebar_collapsed.contains(&section)
    }

    pub(in crate::iced_ui) fn sidebar_section_visible(&self, section: SidebarSection) -> bool {
        section != SidebarSection::Portable
            || self
                .sidebar_storage_entries
                .iter()
                .any(|entry| entry.drive_kind == Some(DriveKind::Portable))
    }

    pub(in crate::iced_ui) fn toggle_sidebar_section(&mut self, section: SidebarSection) {
        if let Some(index) = self
            .config
            .sidebar_collapsed
            .iter()
            .position(|candidate| *candidate == section)
        {
            self.config.sidebar_collapsed.remove(index);
        } else {
            self.config.sidebar_collapsed.push(section);
        }
        save_config(&self.config);
    }

    pub(in crate::iced_ui) fn reorder_sidebar_section(
        &mut self,
        dragged: SidebarSection,
        target: SidebarSection,
        after: bool,
    ) -> Option<(usize, f32)> {
        let order = self.config.normalized_sidebar_order();
        let old_top = self.sidebar_section_top(&order, dragged);
        let new_order = sidebar_order_with_reorder(order.clone(), dragged, target, after);
        if new_order == order {
            return None;
        }
        let new_top = self.sidebar_section_top(&new_order, dragged);
        let new_slot = new_order
            .iter()
            .position(|section| *section == dragged)
            .unwrap_or(0);
        self.config.sidebar_order = new_order;
        Some((new_slot, new_top - old_top))
    }

    pub(in crate::iced_ui) fn sidebar_section_top(
        &self,
        order: &[SidebarSection],
        section: SidebarSection,
    ) -> f32 {
        let mut top = 0.0;
        for candidate in order {
            if *candidate == section {
                break;
            }
            if self.sidebar_section_visible(*candidate) {
                top += self.sidebar_section_layout_height(*candidate) + 1.0;
            }
        }
        top
    }

    pub(in crate::iced_ui) fn sidebar_section_layout_height(&self, section: SidebarSection) -> f32 {
        if !self.sidebar_section_visible(section) {
            return 0.0;
        }
        let item_count = if self.sidebar_section_expanded(section) {
            sidebar_items_for_section(
                &self.config,
                &self.sidebar_storage_entries,
                section,
                self.is_spanish(),
            )
            .len() as f32
        } else {
            0.0
        };
        SIDEBAR_SECTION_HEIGHT + item_count * SIDEBAR_ITEM_HEIGHT + item_count.max(0.0)
    }
}
