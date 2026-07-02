use eframe::egui::{self, Align2, Color32, Id, Order, Pos2, Rect, Sense, Stroke, Vec2};

use crate::app::config::AppConfig;
use crate::app::session::{SplitFocus, SplitSide};
use crate::app::state::BExplorerApp;
use crate::ui::i18n;
use crate::ui::theme;
use crate::ui::window_frame;

const BAR_HEIGHT: f32 = 40.0;
const TAB_HEIGHT: f32 = 32.0;
const TAB_WIDTH: f32 = 214.0;
const TAB_GAP: f32 = 2.0;
const SPLIT_DIVIDER_WIDTH: f32 = 6.0;
const LEFT_TOOL_WIDTH_EXPANDED: f32 = 174.0;
const LEFT_TOOL_WIDTH_COLLAPSED: f32 = 86.0;
const LAYOUT_TOOL_X_EXPANDED: f32 = 150.0;
const LAYOUT_TOOL_X_COLLAPSED: f32 = 58.0;
const MAIN_MENU_WIDTH: f32 = 176.0;
const MAIN_MENU_ROW_HEIGHT: f32 = 28.0;
const WINDOW_BUTTON: f32 = 45.0;
const MAIN_MENU_POPUP_RECT_ID: &str = "main_menu_popup_rect";
const MAIN_MENU_BUTTON_RECT_ID: &str = "main_menu_button_rect";
const MAIN_MENU_DISPLAY_SUBMENU_RECT_ID: &str = "main_menu_display_submenu_rect";

#[derive(Clone, Copy, PartialEq, Eq)]
enum WindowControl {
    Minimize,
    Maximize,
    Close,
}

#[derive(Clone, Copy)]
enum MainMenuIcon {
    Show,
    Shortcuts,
    Options,
    ActionBar,
    BookmarkBar,
    SplitMenus,
}

pub fn show(app: &mut BExplorerApp, ctx: &egui::Context) {
    let titlebar = theme::titlebar(&app.config);
    egui::TopBottomPanel::top("tabs_bar")
        .exact_height(BAR_HEIGHT)
        .frame(egui::Frame::none().fill(titlebar))
        .show(ctx, |ui| {
            let full = ui.max_rect();
            theme::paint_titlebar_gradient(ui.painter(), full, &app.config);
            ui.painter().line_segment(
                [full.left_bottom(), full.right_bottom()],
                Stroke::new(1.0, theme::stroke(&app.config)),
            );

            let left_tools_t = ctx.animate_bool_with_time(
                Id::new("tabs_left_tools_sidebar_visible"),
                app.sidebar_visible,
                0.18,
            );
            let left_tool_width = lerp_f32(
                LEFT_TOOL_WIDTH_COLLAPSED,
                LEFT_TOOL_WIDTH_EXPANDED,
                left_tools_t,
            );
            let drag_gaps = paint_left_tools(app, ui, left_tools_t, left_tool_width);

            let mut switch_to = None;
            let mut close_tab = None;
            let mut focus_split_tab = None;
            let tab_top = full.bottom() - TAB_HEIGHT;
            let max_tab_right = full.right() - WINDOW_BUTTON * 3.0 - 38.0;
            let mut split_plus_rects: Vec<(SplitFocus, Rect)> = Vec::new();
            let mut split_drag_rects: Vec<Rect> = Vec::new();

            let (tab_infos, tab_rects, tab_panes, visible_count): (Vec<_>, Vec<_>, Vec<_>, usize) =
                if let Some(split) = app.split.as_ref() {
                    let split_inline_sidebar = app.config.show_split_pane_menus;
                    let global_sidebar_width = if split_inline_sidebar {
                        0.0
                    } else {
                        let sidebar_t = crate::ui::sidebar::visibility_t(ctx, app.sidebar_visible);
                        crate::ui::sidebar::global_width(&app.config, sidebar_t)
                    };
                    let content_left = full.left() + global_sidebar_width;
                    let content_right = full.right();
                    let tabs_left = (full.left() + left_tool_width + 8.0).max(content_left + 8.0);
                    let tabs_right = max_tab_right.max(tabs_left + 160.0);
                    let content_width =
                        (content_right - content_left - SPLIT_DIVIDER_WIDTH).max(1.0);
                    let divider_x = if content_width > 180.0 {
                        (content_left + content_width * split.ratio)
                            .clamp(content_left + 86.0, content_right - 86.0)
                    } else {
                        content_left + content_width * 0.5
                    };

                    ui.painter().line_segment(
                        [
                            Pos2::new(divider_x, full.top() + 5.0),
                            Pos2::new(divider_x, full.bottom()),
                        ],
                        Stroke::new(1.0, theme::subtle_stroke(&app.config)),
                    );

                    let left_lane = Rect::from_min_max(
                        Pos2::new(tabs_left, tab_top),
                        Pos2::new(divider_x - 8.0, full.bottom()),
                    );
                    let right_lane = Rect::from_min_max(
                        Pos2::new(divider_x + SPLIT_DIVIDER_WIDTH + 8.0, tab_top),
                        Pos2::new(tabs_right, full.bottom()),
                    );
                    let lanes = match split.side {
                        SplitSide::Left => [
                            (SplitFocus::Secondary, left_lane),
                            (SplitFocus::Primary, right_lane),
                        ],
                        SplitSide::Right => [
                            (SplitFocus::Primary, left_lane),
                            (SplitFocus::Secondary, right_lane),
                        ],
                    };

                    let mut infos = Vec::with_capacity(2);
                    let mut rects = Vec::with_capacity(2);
                    let mut panes = Vec::with_capacity(2);
                    for (pane, lane) in lanes {
                        if lane.width() < 58.0 {
                            continue;
                        }
                        let pane_tabs = match pane {
                            SplitFocus::Primary => &split.primary_tabs,
                            SplitFocus::Secondary => &split.secondary_tabs,
                        };
                        let mut x = lane.left();
                        let tab_limit = lane.right() - 32.0;
                        for index in pane_tabs {
                            if x + 60.0 > tab_limit {
                                break;
                            }
                            let width = TAB_WIDTH.min((tab_limit - x).max(60.0));
                            let rect = Rect::from_min_size(
                                Pos2::new(x, tab_top),
                                Vec2::new(width, TAB_HEIGHT),
                            );
                            if let Some(tab) = app.tabs.get(*index) {
                                infos.push((*index, tab.title.clone(), tab.path.clone()));
                                rects.push(rect);
                                panes.push(Some(pane));
                            }
                            x += width + TAB_GAP;
                        }

                        let plus_left = x.min(lane.right() - 28.0).max(lane.left());
                        let plus_rect = Rect::from_center_size(
                            Pos2::new(plus_left + 14.0, full.center().y + 1.0),
                            Vec2::splat(26.0),
                        );
                        split_plus_rects.push((pane, plus_rect));
                        x = plus_rect.right() + 4.0;
                        if x < lane.right() {
                            split_drag_rects.push(Rect::from_min_max(
                                Pos2::new(x, full.top() + window_frame::RESIZE_EDGE),
                                Pos2::new(lane.right(), full.bottom()),
                            ));
                        }
                    }
                    let count = rects.len();
                    (infos, rects, panes, count)
                } else {
                    // Normal mode: show all tabs (existing logic).
                    let infos: Vec<_> = app
                        .tabs
                        .iter()
                        .enumerate()
                        .map(|(index, tab)| (index, tab.title.clone(), tab.path.clone()))
                        .collect();
                    let mut rects: Vec<Rect> = Vec::with_capacity(infos.len());
                    let mut x = full.left() + left_tool_width + 8.0;
                    for (_, _, _) in &infos {
                        if x + 60.0 >= max_tab_right {
                            break;
                        }
                        let width = TAB_WIDTH.min(max_tab_right - x);
                        rects.push(Rect::from_min_size(
                            Pos2::new(x, tab_top),
                            Vec2::new(width, TAB_HEIGHT),
                        ));
                        x += width + TAB_GAP;
                    }
                    let count = rects.len();
                    let panes = vec![None; count];
                    (infos, rects, panes, count)
                };

            let is_split = app.split.is_some();

            // Resolve the active tab drag (if any) before drawing so the
            // visuals reflect this frame's target. We take the drag out of
            // `app` to avoid borrow conflicts while updating it, then put it
            // back. The actual reorder is deferred until after the draw loop so
            // this frame can render the drop position using the old order.
            let mut pending_drop: Option<(usize, usize)> = None;
            let mut pending_split_drop: Option<(SplitFocus, usize, usize)> = None;
            let mut pending_snap: Option<SplitSide> = None;
            let mut snap_zone = false;
            let mut drag_state = app.tab_drag.take();
            let mut abort_drag = false;
            // Snap zone rect (used by both drag resolution and popup).
            let screen = ctx.screen_rect();
            let snap_rect = Rect::from_min_max(
                Pos2::new(screen.left() + 12.0, full.bottom() + 4.0),
                Pos2::new(screen.right() - 12.0, screen.bottom() - 8.0),
            );
            if let Some(drag) = drag_state.as_mut() {
                if is_split {
                    if drag.offsets.len() != visible_count {
                        abort_drag = true;
                    } else if let Some(pointer) = ctx.input(|i| i.pointer.hover_pos()) {
                        let from_slot = drag.from_index.min(visible_count.saturating_sub(1));
                        let from_pane = tab_panes.get(from_slot).and_then(|pane| *pane);
                        if let Some(from_pane) = from_pane {
                            let pane_slots: Vec<usize> = tab_panes
                                .iter()
                                .enumerate()
                                .filter_map(|(slot, pane)| {
                                    (*pane == Some(from_pane)).then_some(slot)
                                })
                                .collect();
                            let from_pos = pane_slots
                                .iter()
                                .position(|slot| *slot == from_slot)
                                .unwrap_or(0);

                            if ctx.input(|i| i.pointer.primary_down()) {
                                let mut drop_slot = pane_slots.len();
                                for (pos, slot) in pane_slots.iter().enumerate() {
                                    if pointer.x < tab_rects[*slot].center().x {
                                        drop_slot = pos;
                                        break;
                                    }
                                }
                                let target_pos = if drop_slot <= from_pos {
                                    drop_slot
                                } else {
                                    drop_slot - 1
                                }
                                .min(pane_slots.len().saturating_sub(1));
                                drag.target_index = pane_slots[target_pos];

                                let shift = drag.tab_width + TAB_GAP;
                                let mut needs_repaint = false;
                                for (pos, slot) in pane_slots.iter().enumerate() {
                                    if *slot == from_slot {
                                        continue;
                                    }
                                    let target_offset = if from_pos < target_pos
                                        && pos > from_pos
                                        && pos <= target_pos
                                    {
                                        -shift
                                    } else if from_pos > target_pos
                                        && pos >= target_pos
                                        && pos < from_pos
                                    {
                                        shift
                                    } else {
                                        0.0
                                    };
                                    let prev = drag.offsets[*slot];
                                    drag.offsets[*slot] = prev + (target_offset - prev) * 0.3;
                                    if (drag.offsets[*slot] - target_offset).abs() > 0.5 {
                                        needs_repaint = true;
                                    }
                                }
                                if needs_repaint {
                                    ctx.request_repaint();
                                }
                            } else if ctx.input(|i| i.pointer.primary_released()) {
                                let from_tab = tab_infos
                                    .get(from_slot)
                                    .map(|(index, _, _)| *index)
                                    .unwrap_or(0);
                                let target_tab = tab_infos
                                    .get(drag.target_index)
                                    .map(|(index, _, _)| *index)
                                    .unwrap_or(from_tab);
                                pending_split_drop = Some((from_pane, from_tab, target_tab));
                            }
                        } else {
                            abort_drag = true;
                        }
                    }
                } else {
                    if drag.offsets.len() != visible_count {
                        // Tab set changed mid-drag (e.g. closed); abort cleanly.
                        abort_drag = true;
                    } else if let Some(pointer) = ctx.input(|i| i.pointer.hover_pos()) {
                        snap_zone = snap_rect.contains(pointer) && app.tabs.len() > 1;
                        let in_window = screen.expand(40.0).contains(pointer);
                        let in_bar =
                            pointer.y >= full.top() - 40.0 && pointer.y <= full.bottom() + 40.0;

                        if !in_window {
                            // Pointer left the window entirely: cancel the drag.
                            abort_drag = true;
                        } else if ctx.input(|i| i.pointer.primary_down()) {
                            if snap_zone || !in_bar {
                                // Snap zone OR in-transit (between bar and center):
                                // don't reorder; ease offsets back to 0 so the tabs
                                // return home visually while the cursor travels.
                                let mut needs_repaint = false;
                                for i in 0..visible_count {
                                    if i == drag.from_index {
                                        continue;
                                    }
                                    let prev = drag.offsets[i];
                                    drag.offsets[i] = prev * 0.7;
                                    if drag.offsets[i].abs() > 0.5 {
                                        needs_repaint = true;
                                    }
                                }
                                if needs_repaint {
                                    ctx.request_repaint();
                                }
                            } else {
                                // In the tab bar: compute the drop slot (0..=
                                // visible_count) the pointer is hovering over, then
                                // convert to a final index.
                                let mut drop_slot = visible_count;
                                for g in 0..visible_count {
                                    if pointer.x < tab_rects[g].center().x {
                                        drop_slot = g;
                                        break;
                                    }
                                }
                                drag.target_index = if drop_slot <= drag.from_index {
                                    drop_slot
                                } else {
                                    drop_slot - 1
                                };
                                drag.target_index =
                                    drag.target_index.min(visible_count.saturating_sub(1));

                                // Target offset for each non-dragged tab.
                                let shift = drag.tab_width + TAB_GAP;
                                let mut needs_repaint = false;
                                for i in 0..visible_count {
                                    if i == drag.from_index {
                                        continue;
                                    }
                                    let target_offset = if drag.from_index < drag.target_index
                                        && i > drag.from_index
                                        && i <= drag.target_index
                                    {
                                        -shift
                                    } else if drag.from_index > drag.target_index
                                        && i >= drag.target_index
                                        && i < drag.from_index
                                    {
                                        shift
                                    } else {
                                        0.0
                                    };
                                    let prev = drag.offsets[i];
                                    drag.offsets[i] = prev + (target_offset - prev) * 0.3;
                                    if (drag.offsets[i] - target_offset).abs() > 0.5 {
                                        needs_repaint = true;
                                    }
                                }
                                if needs_repaint {
                                    ctx.request_repaint();
                                }
                            }
                        } else if ctx.input(|i| i.pointer.primary_released()) {
                            if snap_zone {
                                // Drop in the snap zone: choose left/right based on
                                // which half of the snap rect the pointer is in.
                                let side = if pointer.x < snap_rect.center().x {
                                    SplitSide::Left
                                } else {
                                    SplitSide::Right
                                };
                                pending_snap = Some(side);
                            } else if in_bar {
                                // Drop in the tab bar: reorder.
                                pending_drop = Some((drag.from_index, drag.target_index));
                            } else {
                                // Released in no-man's-land: end the drag without
                                // reordering or snapping.
                                abort_drag = true;
                            }
                        }
                    }
                }
            }
            if abort_drag {
                drag_state = None;
                snap_zone = false;
            }
            app.tab_drag = drag_state;
            let drag_ref = app.tab_drag.clone();
            for (slot, (index, title, path)) in tab_infos.iter().enumerate() {
                if slot >= visible_count {
                    break;
                }
                let index = *index;
                let base_rect = tab_rects[slot];
                let is_dragged = drag_ref
                    .as_ref()
                    .map(|d| d.from_index == slot)
                    .unwrap_or(false);
                let offset = if let Some(d) = drag_ref.as_ref() {
                    d.offsets.get(slot).copied().unwrap_or(0.0)
                } else {
                    0.0
                };
                // Interaction stays on the original rect so click/hover behave
                // normally; only the paint position is shifted during a drag.
                let response = ui.allocate_rect(base_rect, Sense::click_and_drag());

                if is_split && response.clicked() && drag_ref.is_none() {
                    focus_split_tab = Some(index);
                } else if !is_split && response.clicked() && drag_ref.is_none() {
                    switch_to = Some(index);
                }
                if response.middle_clicked() {
                    close_tab = Some(index);
                }

                let icon_texture = path
                    .as_ref()
                    .and_then(|path| app.native_path_icon_texture_id(ctx, path, path.is_dir()));
                let display_title = if title == "This PC" {
                    i18n::tr(&app.config, "this_pc")
                } else {
                    title.as_str()
                };

                if app.file_drag_active() {
                    if let Some(path) = path.as_ref() {
                        app.register_file_drag_folder_rect(path.clone(), response.rect);
                    }
                }

                let pane = tab_panes.get(slot).and_then(|pane| *pane);
                let (selected, focused_tab) = if let Some(split) = app.split.as_ref() {
                    let selected = index == split.tab_a || index == split.tab_b;
                    let focused = pane.is_some_and(|pane| pane == split.focused);
                    (selected, focused)
                } else {
                    (index == app.active_tab, true)
                };

                // Initiate a tab reorder drag when the user starts dragging a
                // tab (and is not pressing its close button or a resize edge).
                if drag_ref.is_none()
                    && response.drag_started()
                    && !window_frame::pointer_in_resize_edge(ctx)
                {
                    let drag_started_on_close = ui
                        .input(|input| input.pointer.press_origin())
                        .map(|pos| {
                            Rect::from_center_size(
                                Pos2::new(base_rect.right() - 20.0, base_rect.center().y),
                                Vec2::splat(18.0),
                            )
                            .contains(pos)
                        })
                        .unwrap_or(false);
                    if !drag_started_on_close && app.tabs.len() > 1 {
                        let press_origin_x = ctx
                            .input(|i| i.pointer.press_origin())
                            .map(|p| p.x)
                            .unwrap_or(base_rect.left());
                        app.tab_drag = Some(crate::app::state::TabDrag {
                            from_index: slot,
                            target_index: slot,
                            press_origin_x,
                            tab_width: base_rect.width(),
                            offsets: vec![0.0; visible_count],
                        });
                    }
                }

                // Draw non-dragged tabs at their shifted position. The dragged
                // tab is drawn last (on top) so it follows the cursor cleanly.
                if !is_dragged {
                    paint_tab(
                        ui,
                        base_rect.translate(egui::vec2(offset, 0.0)),
                        &app.config,
                        display_title,
                        selected,
                        focused_tab,
                        response.hovered() && drag_ref.is_none(),
                        icon_texture,
                    );

                    let close_rect = Rect::from_center_size(
                        Pos2::new(base_rect.right() - 20.0 + offset, base_rect.center().y),
                        Vec2::splat(18.0),
                    );
                    let close_response = ui
                        .allocate_rect(close_rect, Sense::click())
                        .on_hover_text(i18n::tr(&app.config, "close_tab"));
                    paint_close(ui, &app.config, close_rect, close_response.hovered());
                    if close_response.clicked() {
                        close_tab = Some(index);
                    }
                }

                response.context_menu(|ui| {
                    ui.set_min_width(150.0);
                    if ui.button(i18n::tr(&app.config, "close_tab")).clicked() {
                        close_tab = Some(index);
                        ui.close_menu();
                    }
                });
            }

            // Draw the dragged tab on top, following the pointer along x.
            if let Some(d) = drag_ref.as_ref() {
                if d.from_index < visible_count {
                    let pointer_x = ctx
                        .input(|i| i.pointer.hover_pos())
                        .map(|p| p.x)
                        .unwrap_or(d.press_origin_x);
                    let drag_dx = pointer_x - d.press_origin_x;
                    let dragged_rect = tab_rects[d.from_index].translate(egui::vec2(drag_dx, 0.0));
                    let rounding = egui::Rounding {
                        nw: 8.0,
                        ne: 8.0,
                        sw: 0.0,
                        se: 0.0,
                    };
                    // Subtle shadow for the "lifted" feel.
                    ui.painter().rect_filled(
                        dragged_rect.translate(egui::vec2(2.0, 3.0)),
                        rounding,
                        Color32::from_black_alpha(90),
                    );
                    let (_, title, path) = &tab_infos[d.from_index];
                    let icon_texture = path
                        .as_ref()
                        .and_then(|path| app.native_path_icon_texture_id(ctx, path, path.is_dir()));
                    let display_title = if title == "This PC" {
                        i18n::tr(&app.config, "this_pc")
                    } else {
                        title.as_str()
                    };
                    let dragged_tab_index = tab_infos
                        .get(d.from_index)
                        .map(|(index, _, _)| *index)
                        .unwrap_or(app.active_tab);
                    let (selected, focused_tab) = if let Some(split) = app.split.as_ref() {
                        let pane = tab_panes.get(d.from_index).and_then(|pane| *pane);
                        (
                            dragged_tab_index == split.tab_a || dragged_tab_index == split.tab_b,
                            pane.is_some_and(|pane| pane == split.focused),
                        )
                    } else {
                        (app.active_tab == dragged_tab_index, true)
                    };
                    paint_tab(
                        ui,
                        dragged_rect,
                        &app.config,
                        display_title,
                        selected,
                        focused_tab,
                        true,
                        icon_texture,
                    );
                }
            }

            let next_x = tab_rects
                .last()
                .map(|r| r.right() + TAB_GAP)
                .unwrap_or(full.left() + left_tool_width + 8.0);
            if is_split {
                let mut add_split_tab = None;
                for (pane, plus_rect) in &split_plus_rects {
                    let plus_response = ui
                        .allocate_rect(*plus_rect, Sense::click())
                        .on_hover_text(i18n::tr(&app.config, "new_tab"));
                    paint_plus(ui, &app.config, *plus_rect, plus_response.hovered());
                    if plus_response.clicked() {
                        add_split_tab = Some(*pane);
                    }
                }
                if let Some(pane) = add_split_tab {
                    app.new_split_tab(pane, None);
                }
            } else {
                let plus_rect = Rect::from_center_size(
                    Pos2::new(next_x + 19.0, full.center().y + 1.0),
                    Vec2::splat(28.0),
                );
                let plus_response = ui
                    .allocate_rect(plus_rect, Sense::click())
                    .on_hover_text(i18n::tr(&app.config, "new_tab"));
                paint_plus(ui, &app.config, plus_rect, plus_response.hovered());
                if plus_response.clicked() {
                    app.new_tab(None);
                }
            }

            let split_new_center_x = full.right() - WINDOW_BUTTON * 3.0 - 6.0 - 13.0;
            let split_new = Rect::from_center_size(
                Pos2::new(split_new_center_x, full.center().y + 1.0),
                Vec2::splat(26.0),
            );
            let split_new_response = ui
                .allocate_rect(split_new, Sense::click())
                .on_hover_text(i18n::tr(&app.config, "new_split_tab"));
            if split_new_response.clicked() {
                app.open_new_tab_in_split();
            }
            paint_toolbar_square(ui, &app.config, split_new, split_new_response.hovered());
            paint_split_tab_icon(ui, split_new, &app.config);

            paint_window_controls(ui, ctx, &app.config, full);
            for rect in drag_gaps {
                handle_drag_rect(ui, ctx, rect);
            }
            let drag_top = full.top() + window_frame::RESIZE_EDGE;
            let top_strip = Rect::from_min_max(
                Pos2::new(full.left(), drag_top),
                Pos2::new(full.right() - WINDOW_BUTTON * 3.0, tab_top),
            );
            handle_drag_rect(ui, ctx, top_strip);

            if !is_split {
                let after_tabs_left = next_x + 38.0;
                if after_tabs_left < max_tab_right {
                    let after_tabs = Rect::from_min_max(
                        Pos2::new(after_tabs_left, drag_top),
                        Pos2::new(max_tab_right, full.bottom()),
                    );
                    handle_drag_rect(ui, ctx, after_tabs);
                }
            } else {
                for rect in split_drag_rects {
                    handle_drag_rect(ui, ctx, rect);
                }
            }

            if let Some(index) = focus_split_tab {
                app.activate_tab(index);
            }
            if let Some(index) = switch_to {
                app.activate_tab(index);
            }
            if let Some(index) = close_tab {
                app.close_tab(index);
            }
            // Apply the tab reorder after drawing and after click/close handling
            // so their (old) indices remain valid this frame.
            if let Some((from, to)) = pending_drop {
                app.tab_drag = None;
                app.move_tab(from, to);
            }
            if let Some((pane, from_tab, target_tab)) = pending_split_drop {
                app.tab_drag = None;
                app.move_split_tab_within_pane(pane, from_tab, target_tab);
            }
            // Apply a snap: the dragged tab becomes the secondary panel.
            if let Some(side) = pending_snap {
                app.tab_drag = None;
                app.snap_tab_to_split(drag_ref.as_ref().map(|d| d.from_index).unwrap_or(0), side);
            }

            // Snap-layout popup: shown while a tab drag hovers the snap zone.
            // Offers left/right split placements as styled window mockups.
            if snap_zone && !is_split {
                let config = &app.config;
                let popup_center = Pos2::new(snap_rect.center().x, full.bottom() + 56.0);
                let popup_rect = Rect::from_center_size(popup_center, egui::vec2(244.0, 116.0));
                let pointer = ctx.input(|i| i.pointer.hover_pos());
                let hover_left = pointer.map(|p| p.x < snap_rect.center().x).unwrap_or(false);

                egui::Area::new(Id::new("snap_layout_popup"))
                    .order(Order::Foreground)
                    .fixed_pos(popup_rect.min)
                    .show(ctx, |ui| {
                        egui::Frame::none()
                            .fill(theme::popup_surface(config))
                            .stroke(Stroke::new(1.0, theme::popup_stroke(config)))
                            .shadow(theme::popup_shadow(config))
                            .rounding(10.0)
                            .inner_margin(egui::Margin::same(14.0))
                            .show(ui, |ui| {
                                ui.set_min_size(popup_rect.size());
                                ui.vertical(|ui| {
                                    ui.add_space(2.0);
                                    // Title.
                                    ui.painter().text(
                                        Pos2::new(popup_rect.center().x, popup_rect.top() + 18.0),
                                        Align2::CENTER_CENTER,
                                        i18n::tr(config, "choose_layout"),
                                        theme::font(config, 11.5),
                                        theme::muted(config),
                                    );
                                    ui.add_space(10.0);
                                    // Tiles row.
                                    ui.horizontal(|ui| {
                                        ui.add_space(8.0);
                                        for (side, is_left) in
                                            [(SplitSide::Left, true), (SplitSide::Right, false)]
                                        {
                                            let _ = side;
                                            let hover = hover_left == is_left;
                                            paint_snap_tile(ui, config, is_left, hover);
                                            ui.add_space(10.0);
                                        }
                                    });
                                });
                            });
                    });
            }
        });

    show_menu_popup(app, ctx);
}

/// Paint one snap-layout tile: a mini window mockup split in two panes, with
/// the target pane (left or right) highlighted. Shown below is a text label.
fn paint_snap_tile(ui: &mut egui::Ui, config: &AppConfig, is_left: bool, hover: bool) {
    let tile_w = 108.0;
    let tile_h = 78.0;
    let (tile_rect, _) = ui.allocate_exact_size(egui::vec2(tile_w, tile_h), Sense::hover());

    // Tile background.
    let tile_bg = if hover {
        theme::selection_rect_fill(config)
    } else {
        Color32::TRANSPARENT
    };
    if tile_bg != Color32::TRANSPARENT {
        ui.painter().rect_filled(tile_rect, 7.0, tile_bg);
    }
    // Tile border.
    let tile_stroke = if hover {
        Stroke::new(2.5, theme::accent(config))
    } else {
        Stroke::new(1.0, theme::subtle_stroke(config))
    };
    ui.painter().rect_stroke(tile_rect, 7.0, tile_stroke);

    // Window mockup inside the tile (inset).
    let mock = tile_rect.shrink2(egui::vec2(10.0, 10.0));
    // Title bar of the mockup.
    let titlebar_h = 10.0;
    let titlebar_rect = Rect::from_min_size(mock.min, egui::vec2(mock.width(), titlebar_h));
    ui.painter().rect_filled(
        titlebar_rect,
        egui::Rounding {
            nw: 3.0,
            ne: 3.0,
            sw: 0.0,
            se: 0.0,
        },
        theme::stroke(config),
    );
    // Two dots as window controls in the mockup title bar.
    for (dx, color) in [
        (4.0, Color32::from_rgb(196, 43, 43)),
        (10.0, theme::accent(config)),
    ] {
        ui.painter().circle_filled(
            Pos2::new(titlebar_rect.left() + dx, titlebar_rect.center().y),
            1.6,
            color,
        );
    }

    // Body of the mockup: two panes split by a vertical divider.
    let body_rect = Rect::from_min_max(
        Pos2::new(mock.left(), titlebar_rect.bottom() + 1.0),
        mock.right_bottom(),
    );
    let pane_w = body_rect.width() * 0.5 - 0.5;
    let left_pane = Rect::from_min_size(body_rect.min, egui::vec2(pane_w, body_rect.height()));
    let right_pane = Rect::from_min_size(
        Pos2::new(body_rect.center().x + 0.5, body_rect.top()),
        egui::vec2(pane_w, body_rect.height()),
    );

    // Pane fill: highlight the target pane (left or right) with the accent
    // color; the other pane stays with a subtle border only.
    let active_alpha = if hover { 140 } else { 70 };
    let active_fill = Color32::from_rgba_unmultiplied(
        theme::accent(config).r(),
        theme::accent(config).g(),
        theme::accent(config).b(),
        active_alpha,
    );
    let target_pane = if is_left { left_pane } else { right_pane };
    ui.painter().rect_filled(target_pane, 2.0, active_fill);
    // Inactive pane outline.
    let inactive_pane = if is_left { right_pane } else { left_pane };
    ui.painter().rect_stroke(
        inactive_pane,
        2.0,
        Stroke::new(1.0, theme::subtle_stroke(config)),
    );
    // Divider line between panes.
    ui.painter().line_segment(
        [
            Pos2::new(body_rect.center().x, body_rect.top() + 1.0),
            Pos2::new(body_rect.center().x, body_rect.bottom() - 1.0),
        ],
        Stroke::new(1.0, theme::text(config)),
    );

    // Label below the tile.
    let label = if is_left {
        i18n::tr(config, "split_left")
    } else {
        i18n::tr(config, "split_right")
    };
    let label_color = if hover {
        theme::accent_text(config)
    } else {
        theme::muted(config)
    };
    let label_rect = Rect::from_min_size(
        Pos2::new(tile_rect.left(), tile_rect.bottom() + 4.0),
        egui::vec2(tile_w, 14.0),
    );
    ui.painter().text(
        label_rect.center(),
        Align2::CENTER_CENTER,
        label,
        theme::font(config, 11.0),
        label_color,
    );
}

fn lerp_f32(start: f32, end: f32, t: f32) -> f32 {
    start + (end - start) * t.clamp(0.0, 1.0)
}

fn paint_left_tools(
    app: &mut BExplorerApp,
    ui: &mut egui::Ui,
    left_tools_t: f32,
    left_tool_width: f32,
) -> Vec<Rect> {
    let rect = ui.max_rect();
    let drag_top = rect.top() + window_frame::RESIZE_EDGE;
    let menu = Rect::from_center_size(
        Pos2::new(rect.left() + 22.0, rect.center().y),
        Vec2::splat(26.0),
    );
    let layout_x = lerp_f32(
        LAYOUT_TOOL_X_COLLAPSED,
        LAYOUT_TOOL_X_EXPANDED,
        left_tools_t,
    );
    let layout = Rect::from_center_size(
        Pos2::new(rect.left() + layout_x, rect.center().y),
        Vec2::splat(26.0),
    );
    let menu_response = ui
        .allocate_rect(menu, Sense::click())
        .on_hover_text(i18n::tr(&app.config, "menu"));
    let layout_response = ui
        .allocate_rect(layout, Sense::click())
        .on_hover_text(i18n::tr(&app.config, "toggle_layout"));

    if menu_response.clicked() {
        app.options_menu_open = !app.options_menu_open;
        if app.options_menu_open {
            app.action_bar_new_menu_open = false;
            if let Some(other) = app.other_pane.as_mut() {
                other.action_bar_new_menu_open = false;
            }
        }
    }
    ui.ctx()
        .data_mut(|data| data.insert_temp(Id::new(MAIN_MENU_BUTTON_RECT_ID), menu));
    if layout_response.clicked() {
        app.toggle_sidebar_visible();
    }

    paint_toolbar_square(ui, &app.config, menu, menu_response.hovered());
    for offset in [-5.0, 0.0, 5.0] {
        ui.painter().line_segment(
            [
                Pos2::new(menu.left() + 8.0, menu.center().y + offset),
                Pos2::new(menu.right() - 8.0, menu.center().y + offset),
            ],
            Stroke::new(1.4, theme::muted(&app.config)),
        );
    }

    paint_toolbar_square(ui, &app.config, layout, layout_response.hovered());
    paint_layout_toggle_icon(ui, layout, &app.config, app.sidebar_visible);

    vec![
        Rect::from_min_max(
            Pos2::new(rect.left(), drag_top),
            Pos2::new(menu.left() - 2.0, rect.bottom()),
        ),
        Rect::from_min_max(
            Pos2::new(menu.right() + 2.0, drag_top),
            Pos2::new(layout.left() - 2.0, rect.bottom()),
        ),
        Rect::from_min_max(
            Pos2::new(layout.right() + 2.0, drag_top),
            Pos2::new(rect.left() + left_tool_width, rect.bottom()),
        ),
    ]
}

fn paint_layout_toggle_icon(
    ui: &mut egui::Ui,
    rect: Rect,
    config: &AppConfig,
    sidebar_visible: bool,
) {
    let color = if sidebar_visible {
        theme::muted(config)
    } else {
        theme::faint(config)
    };
    let inner = rect.shrink(7.0);
    ui.painter()
        .rect_stroke(inner, 1.0, Stroke::new(1.1, color));
    ui.painter().line_segment(
        [
            Pos2::new(inner.center().x, inner.top()),
            Pos2::new(inner.center().x, inner.bottom()),
        ],
        Stroke::new(1.1, color),
    );
}

fn paint_split_tab_icon(ui: &mut egui::Ui, rect: Rect, config: &AppConfig) {
    let color = theme::muted(config);
    let inner = rect.shrink(7.0);
    ui.painter()
        .rect_stroke(inner, 1.0, Stroke::new(1.1, color));
    ui.painter().line_segment(
        [
            Pos2::new(inner.center().x, inner.top()),
            Pos2::new(inner.center().x, inner.bottom()),
        ],
        Stroke::new(1.1, color),
    );

    let plus_center = Pos2::new(inner.right() - 4.0, inner.center().y);
    ui.painter().line_segment(
        [
            Pos2::new(plus_center.x - 3.0, plus_center.y),
            Pos2::new(plus_center.x + 3.0, plus_center.y),
        ],
        Stroke::new(1.15, color),
    );
    ui.painter().line_segment(
        [
            Pos2::new(plus_center.x, plus_center.y - 3.0),
            Pos2::new(plus_center.x, plus_center.y + 3.0),
        ],
        Stroke::new(1.15, color),
    );
}

fn paint_tab(
    ui: &mut egui::Ui,
    rect: Rect,
    config: &AppConfig,
    title: &str,
    selected: bool,
    focused: bool,
    hovered: bool,
    icon_texture: Option<egui::TextureId>,
) {
    let fill = if selected {
        if focused {
            theme::tab_active(config)
        } else {
            blend_for_tab(theme::tab_active(config), theme::tab_inactive(config), 0.28)
        }
    } else if hovered {
        theme::tab_hover(config)
    } else {
        theme::tab_inactive(config)
    };
    let rounding = egui::Rounding {
        nw: 8.0,
        ne: 8.0,
        sw: 0.0,
        se: 0.0,
    };
    ui.painter().rect_filled(rect, rounding, fill);
    let stroke = if selected && focused {
        Stroke::new(1.25, theme::accent(config))
    } else if selected {
        Stroke::new(1.0, theme::popup_stroke(config))
    } else {
        Stroke::new(1.0, theme::stroke(config))
    };
    ui.painter().rect_stroke(rect, rounding, stroke);
    if selected {
        let underline = Rect::from_min_max(
            Pos2::new(rect.left() + 12.0, rect.bottom() - 2.0),
            Pos2::new(rect.right() - 12.0, rect.bottom()),
        );
        let color = if focused {
            theme::accent(config)
        } else {
            theme::muted(config)
        };
        ui.painter().rect_filled(underline, 1.0, color);
    }

    let icon_rect = Rect::from_center_size(
        Pos2::new(rect.left() + 23.0, rect.center().y),
        Vec2::splat(16.0),
    );
    if let Some(texture_id) = icon_texture {
        ui.painter().image(
            texture_id,
            icon_rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    } else {
        paint_computer_icon(ui, icon_rect, config);
    }

    ui.painter()
        .with_clip_rect(Rect::from_min_max(
            Pos2::new(rect.left() + 43.0, rect.top()),
            Pos2::new(rect.right() - 34.0, rect.bottom()),
        ))
        .text(
            Pos2::new(rect.left() + 44.0, rect.center().y),
            Align2::LEFT_CENTER,
            title,
            theme::font(config, 12.8),
            if selected || hovered {
                theme::text(config)
            } else {
                theme::muted(config)
            },
        );
}

fn blend_for_tab(a: Color32, b: Color32, amount: f32) -> Color32 {
    let t = amount.clamp(0.0, 1.0);
    Color32::from_rgb(
        (a.r() as f32 * (1.0 - t) + b.r() as f32 * t).round() as u8,
        (a.g() as f32 * (1.0 - t) + b.g() as f32 * t).round() as u8,
        (a.b() as f32 * (1.0 - t) + b.b() as f32 * t).round() as u8,
    )
}

fn paint_computer_icon(ui: &mut egui::Ui, rect: Rect, config: &AppConfig) {
    let screen = Rect::from_min_max(
        rect.left_top() + egui::vec2(1.0, 2.0),
        rect.right_bottom() - egui::vec2(1.0, 5.0),
    );
    ui.painter()
        .rect_filled(screen, 1.5, Color32::from_rgb(45, 169, 213));
    ui.painter().line_segment(
        [
            Pos2::new(rect.center().x, screen.bottom()),
            Pos2::new(rect.center().x, rect.bottom() - 1.0),
        ],
        Stroke::new(1.0, theme::muted(config)),
    );
    ui.painter().line_segment(
        [
            Pos2::new(rect.left() + 4.0, rect.bottom() - 1.0),
            Pos2::new(rect.right() - 4.0, rect.bottom() - 1.0),
        ],
        Stroke::new(1.0, theme::muted(config)),
    );
}

fn paint_plus(ui: &mut egui::Ui, config: &AppConfig, rect: Rect, hovered: bool) {
    paint_toolbar_square(ui, config, rect, hovered);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        "+",
        theme::font(config, 18.0),
        theme::muted(config),
    );
}

fn paint_close(ui: &mut egui::Ui, config: &AppConfig, rect: Rect, hovered: bool) {
    if hovered {
        theme::paint_hover_gradient(ui.painter(), rect, 4.0, config);
    }
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        "x",
        theme::font(config, 12.0),
        theme::muted(config),
    );
}

fn paint_toolbar_square(ui: &mut egui::Ui, config: &AppConfig, rect: Rect, hovered: bool) {
    if hovered {
        theme::paint_hover_gradient(ui.painter(), rect, 4.0, config);
    }
}

fn paint_window_controls(ui: &mut egui::Ui, ctx: &egui::Context, config: &AppConfig, full: Rect) {
    let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
    let controls = [
        WindowControl::Minimize,
        WindowControl::Maximize,
        WindowControl::Close,
    ];
    for (control, rect) in controls.iter().zip(window_control_rects(full)) {
        let tooltip = match control {
            WindowControl::Minimize => i18n::tr(config, "minimize"),
            WindowControl::Maximize if maximized => i18n::tr(config, "restore"),
            WindowControl::Maximize => i18n::tr(config, "maximize"),
            WindowControl::Close => i18n::tr(config, "close"),
        };
        let response = ui
            .allocate_rect(rect, Sense::click())
            .on_hover_text(tooltip);

        paint_window_control_icon(ui, config, rect, *control, maximized, response.hovered());

        if response.clicked() {
            match control {
                WindowControl::Minimize => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
                WindowControl::Maximize => toggle_maximized(ctx),
                WindowControl::Close => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }
}

fn paint_window_control_icon(
    ui: &mut egui::Ui,
    config: &AppConfig,
    rect: Rect,
    control: WindowControl,
    maximized: bool,
    hovered: bool,
) {
    if hovered {
        if control == WindowControl::Close {
            ui.painter()
                .rect_filled(rect, 0.0, Color32::from_rgb(196, 43, 43));
        } else {
            theme::paint_hover_gradient(ui.painter(), rect, 0.0, config);
        }
    }

    let color = if control == WindowControl::Close && hovered {
        Color32::WHITE
    } else {
        theme::text(config)
    };
    let stroke = Stroke::new(1.25, color);
    let center = rect.center();

    match control {
        WindowControl::Minimize => {
            let y = center.y;
            ui.painter().line_segment(
                [Pos2::new(center.x - 4.8, y), Pos2::new(center.x + 4.8, y)],
                stroke,
            );
        }
        WindowControl::Maximize if maximized => {
            let back = Rect::from_center_size(center + Vec2::new(1.8, -1.8), Vec2::new(7.8, 7.0));
            let front = Rect::from_center_size(center + Vec2::new(-1.3, 1.3), Vec2::new(7.8, 7.0));
            ui.painter()
                .line_segment([back.left_top(), back.right_top()], stroke);
            ui.painter()
                .line_segment([back.right_top(), back.right_bottom()], stroke);
            ui.painter().line_segment(
                [
                    back.left_top(),
                    Pos2::new(back.left(), front.top().min(back.bottom())),
                ],
                stroke,
            );
            ui.painter().rect_stroke(front, 1.0, stroke);
        }
        WindowControl::Maximize => {
            let box_rect = Rect::from_center_size(center, Vec2::new(8.6, 7.8));
            ui.painter().rect_stroke(box_rect, 1.0, stroke);
        }
        WindowControl::Close => {
            ui.painter().line_segment(
                [
                    Pos2::new(center.x - 3.9, center.y - 3.9),
                    Pos2::new(center.x + 3.9, center.y + 3.9),
                ],
                stroke,
            );
            ui.painter().line_segment(
                [
                    Pos2::new(center.x + 3.9, center.y - 3.9),
                    Pos2::new(center.x - 3.9, center.y + 3.9),
                ],
                stroke,
            );
        }
    }
}

fn show_menu_popup(app: &mut BExplorerApp, ctx: &egui::Context) {
    if !app.options_menu_open {
        ctx.data_mut(|data| {
            data.insert_temp(Id::new(MAIN_MENU_POPUP_RECT_ID), Rect::NOTHING);
            data.insert_temp(Id::new(MAIN_MENU_DISPLAY_SUBMENU_RECT_ID), Rect::NOTHING);
        });
        return;
    }

    if should_close_main_menu_popup(ctx) {
        app.options_menu_open = false;
        ctx.data_mut(|data| {
            data.insert_temp(Id::new(MAIN_MENU_POPUP_RECT_ID), Rect::NOTHING);
            data.insert_temp(Id::new(MAIN_MENU_DISPLAY_SUBMENU_RECT_ID), Rect::NOTHING);
        });
        return;
    }

    let mut popup_rect = Rect::NOTHING;
    egui::Area::new(egui::Id::new("main_menu_popup"))
        .order(egui::Order::Foreground)
        .fixed_pos(Pos2::new(8.0, BAR_HEIGHT - 2.0))
        .show(ctx, |ui| {
            egui::Frame::none()
                .fill(theme::popup_surface(&app.config))
                .stroke(Stroke::new(1.0, theme::stroke(&app.config)))
                .rounding(egui::Rounding::same(6.0))
                .inner_margin(egui::Margin::same(6.0))
                .show(ui, |ui| {
                    let show_response = paint_main_menu_row(
                        ui,
                        &app.config,
                        MainMenuIcon::Show,
                        i18n::tr(&app.config, "show"),
                        true,
                    );
                    show_display_submenu(app, ctx, &show_response);

                    ui.separator();

                    let shortcuts_response = paint_main_menu_row(
                        ui,
                        &app.config,
                        MainMenuIcon::Shortcuts,
                        i18n::tr(&app.config, "custom_shortcuts"),
                        false,
                    );
                    if shortcuts_response.clicked() {
                        app.shortcuts_open = true;
                        app.options_menu_open = false;
                    }

                    let response = paint_main_menu_row(
                        ui,
                        &app.config,
                        MainMenuIcon::Options,
                        i18n::tr(&app.config, "options"),
                        false,
                    );
                    if response.clicked() {
                        app.options_open = true;
                        app.options_menu_open = false;
                    }

                    popup_rect = ui.min_rect();
                });
        });
    ctx.data_mut(|data| data.insert_temp(Id::new(MAIN_MENU_POPUP_RECT_ID), popup_rect));
}

fn should_close_main_menu_popup(ctx: &egui::Context) -> bool {
    let clicked = ctx.input(|input| input.pointer.any_click());
    if !clicked {
        return false;
    }
    let Some(pointer) = ctx.input(|input| input.pointer.interact_pos()) else {
        return false;
    };

    let contains_temp_rect = |id: &'static str, expand: f32| {
        ctx.data(|data| data.get_temp::<Rect>(Id::new(id)))
            .filter(|rect| *rect != Rect::NOTHING && rect.is_finite())
            .is_some_and(|rect| rect.expand(expand).contains(pointer))
    };

    !contains_temp_rect(MAIN_MENU_BUTTON_RECT_ID, 2.0)
        && !contains_temp_rect(MAIN_MENU_POPUP_RECT_ID, 6.0)
        && !contains_temp_rect(MAIN_MENU_DISPLAY_SUBMENU_RECT_ID, 6.0)
}

fn paint_main_menu_row(
    ui: &mut egui::Ui,
    config: &AppConfig,
    icon: MainMenuIcon,
    label: &str,
    submenu: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(MAIN_MENU_WIDTH, MAIN_MENU_ROW_HEIGHT),
        Sense::click(),
    );
    ui.painter()
        .rect_filled(rect, 4.0, theme::popup_surface(config));
    if response.hovered() {
        theme::paint_hover_gradient(ui.painter(), rect, 4.0, config);
    }
    paint_main_menu_icon(
        ui,
        Rect::from_center_size(
            Pos2::new(rect.left() + 15.0, rect.center().y),
            Vec2::splat(16.0),
        ),
        config,
        icon,
        response.hovered(),
    );
    ui.painter().text(
        Pos2::new(rect.left() + 34.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::font(config, 12.2),
        theme::text(config),
    );
    if submenu {
        paint_submenu_chevron(
            ui,
            Rect::from_center_size(
                Pos2::new(rect.right() - 12.0, rect.center().y),
                Vec2::splat(12.0),
            ),
            theme::muted(config),
        );
    }
    response
}

fn show_display_submenu(
    app: &mut BExplorerApp,
    ctx: &egui::Context,
    parent_response: &egui::Response,
) {
    let submenu_rect_id = Id::new(MAIN_MENU_DISPLAY_SUBMENU_RECT_ID);
    let pointer = ctx.input(|input| input.pointer.hover_pos());
    let previous_submenu_rect = ctx
        .data(|data| data.get_temp::<Rect>(submenu_rect_id))
        .filter(|rect| *rect != Rect::NOTHING && rect.is_finite());
    let hover_submenu = match (previous_submenu_rect, pointer) {
        (Some(rect), Some(pointer)) => rect.expand(8.0).contains(pointer),
        _ => false,
    };

    if !parent_response.hovered() && !hover_submenu && !parent_response.clicked() {
        ctx.data_mut(|data| data.insert_temp(submenu_rect_id, Rect::NOTHING));
        return;
    }

    ctx.request_repaint();

    let popup_pos = Pos2::new(
        parent_response.rect.right() + 6.0,
        parent_response.rect.top(),
    );
    let config = app.config.clone();
    let mut submenu_rect = Rect::NOTHING;
    let mut toggle_action_bar = false;
    let mut toggle_bookmarks = false;
    let mut toggle_split_menus = false;

    egui::Area::new(Id::new("main_menu_display_submenu"))
        .order(Order::Foreground)
        .fixed_pos(popup_pos)
        .show(ctx, |ui| {
            egui::Frame::none()
                .fill(theme::popup_surface(&config))
                .stroke(Stroke::new(1.0, theme::popup_stroke(&config)))
                .rounding(egui::Rounding::same(6.0))
                .inner_margin(egui::Margin::same(6.0))
                .show(ui, |ui| {
                    let action_bar_response = paint_display_submenu_row(
                        ui,
                        &config,
                        MainMenuIcon::ActionBar,
                        i18n::tr(&config, "show_action_bar"),
                        config.show_action_bar,
                    );
                    if action_bar_response.clicked() {
                        toggle_action_bar = true;
                    }

                    let bookmark_response = paint_display_submenu_row(
                        ui,
                        &config,
                        MainMenuIcon::BookmarkBar,
                        i18n::tr(&config, "show_bookmark_bar"),
                        config.show_bookmark_bar,
                    );
                    if bookmark_response.clicked() {
                        toggle_bookmarks = true;
                    }

                    let split_response = paint_display_submenu_row(
                        ui,
                        &config,
                        MainMenuIcon::SplitMenus,
                        i18n::tr(&config, "show_split_pane_menus"),
                        config.show_split_pane_menus,
                    );
                    if split_response.clicked() {
                        toggle_split_menus = true;
                    }

                    submenu_rect = ui.min_rect();
                });
        });

    ctx.data_mut(|data| data.insert_temp(submenu_rect_id, submenu_rect));

    if toggle_action_bar {
        app.config.show_action_bar = !app.config.show_action_bar;
        app.save_config();
        app.options_menu_open = false;
    }
    if toggle_bookmarks {
        app.config.show_bookmark_bar = !app.config.show_bookmark_bar;
        app.save_config();
        app.options_menu_open = false;
    }
    if toggle_split_menus {
        app.config.show_split_pane_menus = !app.config.show_split_pane_menus;
        app.save_config();
        app.options_menu_open = false;
    }
}

fn paint_display_submenu_row(
    ui: &mut egui::Ui,
    config: &AppConfig,
    icon: MainMenuIcon,
    label: &str,
    checked: bool,
) -> egui::Response {
    let width = 268.0;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, MAIN_MENU_ROW_HEIGHT), Sense::click());
    ui.painter()
        .rect_filled(rect, 4.0, theme::popup_surface(config));
    if response.hovered() {
        theme::paint_hover_gradient(ui.painter(), rect, 4.0, config);
    }
    if checked {
        paint_menu_check(
            ui,
            Rect::from_center_size(
                Pos2::new(rect.left() + 15.0, rect.center().y),
                Vec2::splat(14.0),
            ),
            theme::accent(config),
        );
    }
    paint_main_menu_icon(
        ui,
        Rect::from_center_size(
            Pos2::new(rect.left() + 36.0, rect.center().y),
            Vec2::splat(16.0),
        ),
        config,
        icon,
        response.hovered(),
    );
    let text_clip = Rect::from_min_max(
        Pos2::new(rect.left() + 56.0, rect.top()),
        Pos2::new(rect.right() - 8.0, rect.bottom()),
    );
    ui.painter().with_clip_rect(text_clip).text(
        Pos2::new(text_clip.left(), rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::font(config, 12.2),
        theme::text(config),
    );
    response
}

fn paint_main_menu_icon(
    ui: &egui::Ui,
    rect: Rect,
    config: &AppConfig,
    icon: MainMenuIcon,
    hovered: bool,
) {
    let color = if hovered {
        theme::text(config)
    } else {
        theme::muted(config)
    };
    let stroke = Stroke::new(1.25, color);
    let painter = ui.painter();
    match icon {
        MainMenuIcon::Show => {
            let body = Rect::from_min_size(
                Pos2::new(rect.left() + 2.0, rect.top() + 3.0),
                Vec2::new(12.0, 10.0),
            );
            painter.rect_stroke(body, 1.5, stroke);
            painter.line_segment(
                [
                    Pos2::new(body.left() + 3.0, body.top() + 3.0),
                    Pos2::new(body.right() - 3.0, body.top() + 3.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(body.left() + 3.0, body.center().y + 1.0),
                    Pos2::new(body.right() - 3.0, body.center().y + 1.0),
                ],
                stroke,
            );
        }
        MainMenuIcon::Options => {
            painter.circle_stroke(rect.center(), 4.1, stroke);
            painter.circle_filled(rect.center(), 1.25, color);
            for (x, y) in [(0.0, -6.5), (0.0, 6.5), (-6.5, 0.0), (6.5, 0.0)] {
                let a = Pos2::new(rect.center().x + x * 0.62, rect.center().y + y * 0.62);
                let b = Pos2::new(rect.center().x + x, rect.center().y + y);
                painter.line_segment([a, b], stroke);
            }
        }
        MainMenuIcon::Shortcuts => {
            let body = Rect::from_min_max(
                Pos2::new(rect.left() + 2.0, rect.top() + 4.0),
                Pos2::new(rect.right() - 2.0, rect.bottom() - 3.0),
            );
            painter.rect_stroke(body, 2.0, stroke);
            for x in [body.left() + 3.0, body.left() + 7.0, body.left() + 11.0] {
                painter.line_segment(
                    [
                        Pos2::new(x, body.top() + 3.0),
                        Pos2::new(x, body.bottom() - 3.0),
                    ],
                    Stroke::new(1.0, color),
                );
            }
            painter.line_segment(
                [
                    Pos2::new(body.left() + 2.5, body.center().y),
                    Pos2::new(body.right() - 2.5, body.center().y),
                ],
                Stroke::new(1.0, color),
            );
        }
        MainMenuIcon::ActionBar => {
            let rail = Rect::from_min_max(
                Pos2::new(rect.left() + 2.0, rect.top() + 4.0),
                Pos2::new(rect.right() - 2.0, rect.bottom() - 4.0),
            );
            painter.rect_stroke(rail, 2.0, stroke);
            for x in [rail.left() + 3.0, rail.left() + 6.5, rail.left() + 10.0] {
                painter.line_segment(
                    [
                        Pos2::new(x, rail.top() + 3.0),
                        Pos2::new(x, rail.bottom() - 3.0),
                    ],
                    Stroke::new(1.1, color),
                );
            }
        }
        MainMenuIcon::BookmarkBar => {
            let mark = Rect::from_min_max(
                Pos2::new(rect.left() + 4.0, rect.top() + 2.0),
                Pos2::new(rect.right() - 4.0, rect.bottom() - 2.0),
            );
            painter.line_segment([mark.left_top(), mark.left_bottom()], stroke);
            painter.line_segment([mark.left_top(), mark.right_top()], stroke);
            painter.line_segment([mark.right_top(), mark.right_bottom()], stroke);
            painter.line_segment(
                [
                    mark.left_bottom(),
                    Pos2::new(mark.center().x, mark.bottom() - 3.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(mark.center().x, mark.bottom() - 3.0),
                    mark.right_bottom(),
                ],
                stroke,
            );
        }
        MainMenuIcon::SplitMenus => {
            let left = Rect::from_min_size(
                Pos2::new(rect.left() + 2.0, rect.top() + 3.0),
                Vec2::new(5.0, 10.0),
            );
            let right = Rect::from_min_size(
                Pos2::new(rect.left() + 9.0, rect.top() + 3.0),
                Vec2::new(5.0, 10.0),
            );
            painter.rect_stroke(left, 1.0, stroke);
            painter.rect_stroke(right, 1.0, stroke);
        }
    }
}

fn paint_submenu_chevron(ui: &egui::Ui, rect: Rect, color: Color32) {
    ui.painter().line_segment(
        [
            Pos2::new(rect.left() + 4.0, rect.top() + 3.0),
            rect.center(),
        ],
        Stroke::new(1.3, color),
    );
    ui.painter().line_segment(
        [
            rect.center(),
            Pos2::new(rect.left() + 4.0, rect.bottom() - 3.0),
        ],
        Stroke::new(1.3, color),
    );
}

fn paint_menu_check(ui: &egui::Ui, rect: Rect, color: Color32) {
    ui.painter().line_segment(
        [
            Pos2::new(rect.left() + 2.0, rect.center().y),
            Pos2::new(rect.center().x - 1.0, rect.bottom() - 3.0),
        ],
        Stroke::new(1.5, color),
    );
    ui.painter().line_segment(
        [
            Pos2::new(rect.center().x - 1.0, rect.bottom() - 3.0),
            Pos2::new(rect.right() - 2.0, rect.top() + 3.0),
        ],
        Stroke::new(1.5, color),
    );
}

fn toggle_maximized(ctx: &egui::Context) {
    let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    ctx.request_repaint();
}

fn window_control_rects(full: Rect) -> Vec<Rect> {
    (0..3)
        .map(|slot| {
            Rect::from_min_size(
                Pos2::new(full.right() - WINDOW_BUTTON * (3 - slot) as f32, full.top()),
                Vec2::new(WINDOW_BUTTON, full.height()),
            )
        })
        .collect()
}

fn handle_drag_rect(ui: &mut egui::Ui, ctx: &egui::Context, rect: Rect) {
    if rect.width() <= 2.0 || rect.height() <= 2.0 {
        return;
    }
    if window_frame::pointer_in_resize_edge(ctx) {
        return;
    }

    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    if response.double_clicked() {
        toggle_maximized(ctx);
    } else if response.hovered() && ui.input(|input| input.pointer.primary_pressed()) {
        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
}
