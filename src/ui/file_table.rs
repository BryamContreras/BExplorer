use std::path::Path;

use eframe::egui::{self, Align2, Color32, FontId, Id, Order, Pos2, Rect, Sense, Stroke, Vec2};

use crate::app::config::{AppConfig, ViewMode};
use crate::app::session::{SplitFocus, SplitSide};
use crate::app::state::{BExplorerApp, FileGroup, FileSort};
use crate::fs::archive::{ArchiveFormat, ExtractMode};
use crate::fs::explorer::{EntryKind, FileCategory, FileEntry};
use crate::ui::i18n;
use crate::ui::theme;

mod columns;
mod highlight;
mod path_bar;
mod preview;
mod text;

use columns::{
    ColumnKind, ColumnSpec, column_index, columns_for_view, compact_column_widths,
    compute_auto_fit_widths, localized_column_title, localized_type_label,
};
use highlight::{highlighted_text_layout_job, search_highlight_ranges};
use path_bar::char_to_byte_index;
pub use text::format_bytes;
use text::{draw_text, draw_text_clipped, draw_text_elided, format_bytes_opt, snap_pos, snap_rect};

pub(super) const TOOLBAR_HEIGHT: f32 = 36.0;
const HEADER_HEIGHT: f32 = 27.0;
const ROW_HEIGHT: f32 = 23.0;
const GROUP_HEADER_HEIGHT: f32 = 32.0;
const ICON_SIZE: f32 = 17.0;
pub(super) const CONTEXT_MENU_MIN_WIDTH: f32 = 206.0;
pub(super) const CONTEXT_MENU_MAX_WIDTH: f32 = 380.0;
const CONTEXT_SUBMENU_MAX_WIDTH: f32 = 430.0;
const CONTEXT_ROW_HEIGHT: f32 = 30.0;
const FILE_DRAG_OFFSET: Vec2 = Vec2::new(18.0, 18.0);
const FILE_DRAG_CARD_HEIGHT: f32 = 56.0;

pub(crate) enum TableAction {
    Open(FileEntry),
    OpenWith(FileEntry),
    ScanWithWindowsDefender(FileEntry),
    OpenLocation(FileEntry),
    Select(FileEntry, bool, bool),
    Copy(FileEntry),
    Cut(FileEntry),
    PasteInto(FileEntry),
    Eject(FileEntry),
    Rename(FileEntry),
    Delete(FileEntry, bool),
    Compress(FileEntry),
    CompressAs(FileEntry, ArchiveFormat),
    Extract(FileEntry, ExtractMode),
    Properties(FileEntry),
    PropertiesCurrent,
    OpenTerminalHere,
    CopySelected,
    CutSelected,
    PasteHere,
    Refresh,
    CreateFolder,
    CreateTextDocument,
}

#[derive(Clone, Copy)]
pub(super) enum MenuIcon {
    Open,
    OpenWith,
    Defender,
    Copy,
    Cut,
    Paste,
    Eject,
    New,
    Refresh,
    Folder,
    TextDocument,
    Rename,
    Delete,
    Properties,
    Terminal,
}

#[derive(Clone, Copy)]
struct ContextMenuAnimationState {
    started_at: f64,
}

#[derive(Clone, Copy)]
enum VisualLayout {
    List,
    SmallIcons,
    MediumIcons,
    LargeIcons,
    ExtraLargeIcons,
    Tiles,
}

struct SubmenuItem {
    icon: MenuIcon,
    label: String,
}

struct EntryGroup {
    label: String,
    entries: Vec<FileEntry>,
}

pub(super) struct MenuRowMeasure<'a> {
    pub(super) label: &'a str,
    pub(super) shortcut: Option<&'a str>,
    pub(super) submenu: bool,
}

pub fn show(app: &mut BExplorerApp, ui: &mut egui::Ui) {
    show_in_pane(app, ui, 0);
}

fn show_in_pane(app: &mut BExplorerApp, ui: &mut egui::Ui, pane_id: usize) {
    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
    path_bar::show_navigation_bar(app, ui);
    crate::ui::action_bar::show(app, ui, pane_id);
    crate::ui::bookmarks_bar::show(app, ui);
    if app.preview_panel_visible {
        preview::show_with_panel(app, ui, show_content_view);
    } else {
        show_content_view(app, ui);
    }
}

fn show_content_view(app: &mut BExplorerApp, ui: &mut egui::Ui) {
    match app.active_view_mode() {
        ViewMode::Details => show_table(app, ui),
        ViewMode::List => show_visual_view(app, ui, VisualLayout::List),
        ViewMode::SmallIcons => show_visual_view(app, ui, VisualLayout::SmallIcons),
        ViewMode::MediumIcons => show_visual_view(app, ui, VisualLayout::MediumIcons),
        ViewMode::LargeIcons => show_visual_view(app, ui, VisualLayout::LargeIcons),
        ViewMode::ExtraLargeIcons => show_visual_view(app, ui, VisualLayout::ExtraLargeIcons),
        ViewMode::Tiles => show_visual_view(app, ui, VisualLayout::Tiles),
    }
}

fn grouped_entry_sections(app: &BExplorerApp) -> Option<Vec<EntryGroup>> {
    let (group_by, ascending) = app.group_state();
    if group_by == FileGroup::None {
        return None;
    }

    let mut groups: Vec<EntryGroup> = Vec::new();
    for entry in app.filtered_entries_slice() {
        let label = group_label(app, entry, group_by);
        if let Some(group) = groups.iter_mut().find(|group| group.label == label) {
            group.entries.push(entry.clone());
        } else {
            groups.push(EntryGroup {
                label,
                entries: vec![entry.clone()],
            });
        }
    }

    groups.sort_by(|left, right| {
        let ordering = left.label.to_lowercase().cmp(&right.label.to_lowercase());
        if ascending {
            ordering
        } else {
            ordering.reverse()
        }
    });
    Some(groups)
}

fn group_label(app: &BExplorerApp, entry: &FileEntry, group_by: FileGroup) -> String {
    match group_by {
        FileGroup::None => i18n::tr(&app.config, "none_group").to_string(),
        FileGroup::Name => entry_name_group(&entry_display_name(&app.config, entry)),
        FileGroup::Type => localized_type_label(&app.config, &entry.type_label()),
        FileGroup::TotalSize => format_bytes_opt(entry.size)
            .trim()
            .is_empty()
            .then(|| i18n::tr(&app.config, "none_group").to_string())
            .unwrap_or_else(|| format_bytes_opt(entry.size)),
        FileGroup::FreeSpace => format_bytes_opt(entry.free_space)
            .trim()
            .is_empty()
            .then(|| i18n::tr(&app.config, "none_group").to_string())
            .unwrap_or_else(|| format_bytes_opt(entry.free_space)),
    }
}

fn entry_name_group(name: &str) -> String {
    let Some(character) = name.chars().find(|character| !character.is_whitespace()) else {
        return "#".into();
    };
    if character.is_alphanumeric() {
        character
            .to_uppercase()
            .next()
            .map(|character| character.to_string())
            .unwrap_or_else(|| "#".into())
    } else {
        "#".into()
    }
}

fn paint_group_header(ui: &mut egui::Ui, config: &AppConfig, width: f32, label: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, GROUP_HEADER_HEIGHT), Sense::hover());
    let text_pos = Pos2::new(rect.left() + 12.0, rect.center().y + 2.0);
    ui.painter().text(
        text_pos,
        Align2::LEFT_CENTER,
        label,
        theme::font(config, 12.8),
        theme::text(config),
    );
    ui.painter().line_segment(
        [
            Pos2::new(rect.left() + 10.0, rect.bottom() - 1.0),
            Pos2::new(rect.right() - 10.0, rect.bottom() - 1.0),
        ],
        Stroke::new(0.7, theme::subtle_stroke(config)),
    );
}

fn rect_range_visible(top: f32, bottom: f32, viewport_top: f32, viewport_bottom: f32) -> bool {
    bottom >= viewport_top && top <= viewport_bottom
}

const SPLIT_DIVIDER_WIDTH: f32 = 6.0;
const SPLIT_SNAP_AWAY: f32 = 0.06;

pub fn show_split(app: &mut BExplorerApp, ctx: &egui::Context) {
    let split = match app.split.clone() {
        Some(s) => s,
        None => return,
    };

    egui::CentralPanel::default()
        .frame(egui::Frame::none().fill(crate::ui::theme::canvas(&app.config)))
        .show(ctx, |ui| {
            let full = ui.max_rect();
            crate::ui::theme::paint_canvas_gradient(ui.painter(), full, &app.config);

            let available_width = full.width() - SPLIT_DIVIDER_WIDTH;
            let left_width = (available_width * split.ratio).round();
            let right_width = available_width - left_width;

            let left_rect = Rect::from_min_size(full.min, egui::vec2(left_width, full.height()));
            let divider_rect = Rect::from_min_size(
                Pos2::new(left_rect.right(), full.top()),
                egui::vec2(SPLIT_DIVIDER_WIDTH, full.height()),
            );
            let right_rect = Rect::from_min_size(
                Pos2::new(divider_rect.right(), full.top()),
                egui::vec2(right_width, full.height()),
            );

            let (left_pane, right_pane) = match split.side {
                SplitSide::Left => (SplitFocus::Secondary, SplitFocus::Primary),
                SplitSide::Right => (SplitFocus::Primary, SplitFocus::Secondary),
            };

            render_logical_pane(app, ui, left_rect, left_pane, 0, split.focused);
            render_logical_pane(app, ui, right_rect, right_pane, 1, split.focused);

            // Divider.
            let divider_color = if ctx
                .input(|i| i.pointer.hover_pos())
                .map(|p| divider_rect.expand(4.0).contains(p))
                .unwrap_or(false)
            {
                theme::accent(&app.config)
            } else {
                theme::stroke(&app.config)
            };
            ui.painter().rect_filled(divider_rect, 0.0, divider_color);
            let divider_resp = ui.allocate_rect(divider_rect, Sense::click_and_drag());
            if divider_resp.dragged() {
                if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
                    let ratio = (pos.x - full.left()) / full.width();
                    if ratio < SPLIT_SNAP_AWAY || ratio > 1.0 - SPLIT_SNAP_AWAY {
                        app.close_split();
                    } else {
                        app.set_split_ratio(ratio);
                    }
                }
            }
            if divider_resp.double_clicked() {
                app.set_split_ratio(0.5);
            }

            let unfocused_rect = if split.focused == left_pane {
                right_rect
            } else {
                left_rect
            };

            // Close (X) button on the unfocused panel.
            let close_rect = Rect::from_center_size(
                Pos2::new(unfocused_rect.right() - 18.0, unfocused_rect.top() + 18.0),
                Vec2::splat(22.0),
            );
            let close_resp = ui.allocate_rect(close_rect, Sense::click());
            if close_resp.hovered() {
                ui.painter()
                    .rect_filled(close_rect, 4.0, Color32::from_rgb(196, 43, 43));
            }
            ui.painter().text(
                close_rect.center(),
                Align2::CENTER_CENTER,
                "x",
                theme::font(&app.config, 13.0),
                theme::text(&app.config),
            );
            if close_resp.clicked() {
                app.close_split();
            }

            if ctx.input(|i| i.pointer.primary_released()) {
                if let Some(pointer) = ctx.input(|i| i.pointer.hover_pos()) {
                    let requested_focus = if left_rect.contains(pointer) {
                        Some(left_pane)
                    } else if right_rect.contains(pointer) {
                        Some(right_pane)
                    } else {
                        None
                    };
                    if requested_focus.is_some_and(|pane| pane != split.focused) {
                        app.swap_split_focus();
                    }
                }
            }

            if app.sidebar_visible && app.config.show_split_pane_menus {
                crate::ui::sidebar::finish_drag_or_preview(app, ctx);
            }
        });
}

fn render_logical_pane(
    app: &mut BExplorerApp,
    ui: &mut egui::Ui,
    pane_rect: Rect,
    pane: SplitFocus,
    pane_id: usize,
    focused: SplitFocus,
) {
    if pane == focused {
        render_pane(app, ui, pane_rect, pane_id);
    } else if app.other_pane.is_some() {
        crate::app::state::with_other_pane(app, |app| {
            render_pane(app, ui, pane_rect, pane_id);
        });
    }
}

pub fn paint_file_drag_overlay(app: &mut BExplorerApp, ctx: &egui::Context) {
    let Some(feedback) = app.file_drag_feedback() else {
        return;
    };
    let Some(pointer) = ctx.input(|input| input.pointer.hover_pos()) else {
        return;
    };

    let primary = app.file_drag.as_ref().and_then(|drag| drag.primary.clone());
    let single_icon = if feedback.item_count == 1 {
        primary
            .as_ref()
            .and_then(|info| app.native_path_icon_texture_id(ctx, &info.path, info.is_directory))
    } else {
        None
    };

    let action_label = if feedback.copy {
        i18n::tr(&app.config, "copy_to")
    } else {
        i18n::tr(&app.config, "move_to")
    };
    let target_text = feedback
        .target_name
        .as_ref()
        .map(|target| format!("{action_label} {target}"));
    let title = target_text
        .as_deref()
        .unwrap_or(feedback.item_name.as_str());
    let subtitle = if feedback.item_count == 1 {
        feedback.item_name.as_str()
    } else {
        i18n::tr(&app.config, "drag_multiple_items")
    };
    let width = (title.chars().count() as f32 * 7.2 + 76.0).clamp(180.0, 360.0);
    let pos = file_drag_overlay_pos(
        ctx,
        pointer,
        egui::vec2(width + 20.0, FILE_DRAG_CARD_HEIGHT),
    );

    egui::Area::new(Id::new("file_drag_overlay"))
        .order(Order::Tooltip)
        .fixed_pos(pos)
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::none()
                .fill(Color32::from_rgba_unmultiplied(
                    theme::popup_surface(&app.config).r(),
                    theme::popup_surface(&app.config).g(),
                    theme::popup_surface(&app.config).b(),
                    218,
                ))
                .stroke(Stroke::new(1.0, theme::popup_stroke(&app.config)))
                .shadow(theme::popup_shadow(&app.config))
                .rounding(8.0)
                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                .show(ui, |ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 40.0), Sense::hover());
                    let icon_rect = Rect::from_center_size(
                        Pos2::new(rect.left() + 18.0, rect.center().y),
                        Vec2::splat(22.0),
                    );
                    if let Some(texture_id) = single_icon {
                        let uv = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0));
                        ui.painter()
                            .image(texture_id, icon_rect, uv, Color32::WHITE);
                    } else {
                        let badge = if feedback.item_count > 1 {
                            Some(feedback.item_count)
                        } else {
                            None
                        };
                        draw_drag_file_icon(ui.painter(), icon_rect, &app.config, badge);
                    }
                    draw_text_clipped(
                        ui,
                        Rect::from_min_max(
                            Pos2::new(rect.left() + 38.0, rect.top()),
                            rect.right_top() + egui::vec2(-8.0, 22.0),
                        ),
                        Pos2::new(rect.left() + 40.0, rect.top() + 13.0),
                        title,
                        theme::font(&app.config, 12.5),
                        theme::text(&app.config),
                        Align2::LEFT_CENTER,
                    );
                    draw_text_clipped(
                        ui,
                        Rect::from_min_max(
                            Pos2::new(rect.left() + 38.0, rect.top() + 18.0),
                            rect.right_bottom() - egui::vec2(8.0, 0.0),
                        ),
                        Pos2::new(rect.left() + 40.0, rect.top() + 29.0),
                        subtitle,
                        theme::font(&app.config, 11.2),
                        theme::muted(&app.config),
                        Align2::LEFT_CENTER,
                    );
                });
        });
}

fn file_drag_overlay_pos(ctx: &egui::Context, pointer: Pos2, size: Vec2) -> Pos2 {
    let screen = ctx.screen_rect().shrink(8.0);
    let max_x = (screen.right() - size.x).max(screen.left());
    let max_y = (screen.bottom() - size.y).max(screen.top());
    Pos2::new(
        (pointer.x + FILE_DRAG_OFFSET.x).clamp(screen.left(), max_x),
        (pointer.y + FILE_DRAG_OFFSET.y).clamp(screen.top(), max_y),
    )
}

fn file_drag_drop_hovered(
    app: &BExplorerApp,
    ui: &egui::Ui,
    entry: &FileEntry,
    rect: Rect,
) -> bool {
    if !entry.kind.is_container() || !app.file_drag_active() {
        return false;
    }
    let Some(pointer) = ui.ctx().input(|input| input.pointer.hover_pos()) else {
        return false;
    };
    rect.contains(pointer) && app.can_drop_file_drag_to(&entry.path)
}

fn paint_file_drag_drop_hover(
    painter: &egui::Painter,
    rect: Rect,
    rounding: f32,
    config: &AppConfig,
) {
    theme::paint_row_hover_gradient(painter, rect, rounding, config);
    painter.rect_stroke(
        rect.shrink(0.5),
        rounding,
        Stroke::new(1.2, theme::accent(config)),
    );
}

/// Helper: render a single panel (file table + status bar) into the given rect.
fn render_pane(app: &mut BExplorerApp, ui: &mut egui::Ui, pane_rect: Rect, pane_id: usize) {
    let status_h = 38.0;
    let sidebar_width = if app.sidebar_visible && app.config.show_split_pane_menus {
        crate::ui::sidebar::inline_width(&app.config, pane_rect.width())
    } else {
        0.0
    };
    let sidebar_rect =
        Rect::from_min_size(pane_rect.min, egui::vec2(sidebar_width, pane_rect.height()));
    let content_rect = Rect::from_min_max(
        Pos2::new(pane_rect.left() + sidebar_width, pane_rect.top()),
        pane_rect.right_bottom(),
    );
    let body_rect = Rect::from_min_size(
        content_rect.min,
        egui::vec2(
            content_rect.width(),
            (content_rect.height() - status_h).max(60.0),
        ),
    );
    let status_rect = Rect::from_min_size(
        Pos2::new(content_rect.left(), body_rect.bottom()),
        egui::vec2(content_rect.width(), status_h),
    );

    ui.painter()
        .rect_filled(pane_rect, 0.0, split_pane_isolation_fill(&app.config));

    ui.push_id(("split-pane", pane_id), |ui| {
        if sidebar_width > 0.0 {
            crate::ui::sidebar::show_inline(app, ui, sidebar_rect, pane_id);
        }

        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(body_rect), |ui| {
            ui.set_clip_rect(body_rect);
            show_in_pane(app, ui, pane_id);
        });

        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(status_rect), |ui| {
            ui.set_clip_rect(status_rect);
            crate::ui::status_bar::paint_in_rect(app, ui, status_rect);
        });
    });
}

fn split_pane_isolation_fill(config: &AppConfig) -> Color32 {
    theme::canvas(config)
}

fn show_table(app: &mut BExplorerApp, ui: &mut egui::Ui) {
    if !ui.input(|input| input.pointer.primary_down()) {
        app.drag_selection = None;
    } else if let Some(pos) = ui.input(|input| input.pointer.interact_pos()) {
        app.update_drag_selection(pos);
    }

    let entry_count = app.filtered_entry_count();
    let width = ui.available_width();
    let height = ui.available_height();

    // Auto-fit column widths on path change
    let current_path = app.active_path();
    let width_changed = app
        .last_auto_fit_width
        .map(|last_width| (last_width - width).abs() > 8.0)
        .unwrap_or(true);
    if (app.last_auto_fit_path != current_path || width_changed) && entry_count > 0 {
        let visible_kinds: Vec<ColumnKind> = if app.is_storage_view() {
            vec![
                ColumnKind::Name,
                ColumnKind::Type,
                ColumnKind::FileSystem,
                ColumnKind::FreeSpace,
                ColumnKind::Size,
                ColumnKind::PercentFull,
                ColumnKind::Modified,
            ]
        } else {
            vec![
                ColumnKind::Name,
                ColumnKind::Type,
                ColumnKind::Size,
                ColumnKind::Modified,
            ]
        };
        let mut visible_kinds = visible_kinds;
        if app.showing_complete_search_results() && !app.is_storage_view() {
            visible_kinds.push(ColumnKind::Location);
        }
        let widths = if app.split.is_some() {
            compact_column_widths(&visible_kinds)
        } else {
            compute_auto_fit_widths(
                app.filtered_entries_slice(),
                &visible_kinds,
                width,
                ui.ctx(),
                &app.config,
            )
        };
        app.column_widths = widths;
        app.last_auto_fit_path = current_path;
        app.last_auto_fit_width = Some(width);
    }

    let columns = columns_for_view(app, width);
    let mut action = None;
    let mut sort_action = None;

    let (header_rect, _) = ui.allocate_exact_size(egui::vec2(width, HEADER_HEIGHT), Sense::hover());
    paint_header(app, ui, header_rect, &columns, &mut sort_action);

    let body_height = (height - HEADER_HEIGHT).max(120.0);
    let body_rect = Rect::from_min_size(ui.cursor().min, egui::vec2(width, body_height));
    let mut wheel_direction: i32 = 0;
    ui.input(|input| {
        if input.modifiers.ctrl || input.modifiers.command {
            for event in &input.events {
                if let egui::Event::MouseWheel { delta, .. } = event {
                    if delta.y > 0.0 {
                        wheel_direction = 1;
                    } else if delta.y < 0.0 {
                        wheel_direction = -1;
                    }
                }
            }
        }
    });
    if wheel_direction != 0 {
        app.cycle_view_mode(wheel_direction);
        ui.ctx().input_mut(|i| {
            i.events
                .retain(|e| !matches!(e, egui::Event::MouseWheel { .. }));
        });
        ui.ctx().request_repaint();
    }
    let body_response = ui.interact(
        body_rect,
        ui.make_persistent_id("file_table_body_drag"),
        Sense::click_and_drag(),
    );
    if body_response.drag_started() {
        ui.memory_mut(|memory| memory.request_focus(body_response.id));
        app.clear_text_input_active();
        if let Some(start) = ui.input(|input| input.pointer.press_origin()) {
            let additive = ui.input(|input| input.modifiers.ctrl || input.modifiers.command);
            app.begin_drag_selection(start, additive);
        }
    }
    if body_response.clicked() {
        ui.memory_mut(|memory| memory.request_focus(body_response.id));
        app.clear_text_input_active();
        app.clear_selection();
    }
    if let Some(destination) = app.active_path() {
        app.register_file_drag_folder_rect(destination, body_response.rect);
    }

    // Context menu
    let has_selection = !app.selected.is_empty();
    let can_paste = app.can_paste(ui.ctx());
    body_response.context_menu(|ui| {
        background_context_menu(ui, &app.config, has_selection, can_paste, &mut action);
    });

    if app.drag_selection.is_some() {
        app.prepare_drag_selection_frame();
    }

    if let Some(groups) = grouped_entry_sections(app) {
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(body_rect), |ui| {
            egui::ScrollArea::vertical()
                .id_salt("file_table_grouped_body")
                .auto_shrink([false, false])
                .show_viewport(ui, |ui, viewport| {
                    let mut row_index = 0;
                    let mut cursor_y = 0.0;
                    for group in groups {
                        let header_visible = rect_range_visible(
                            cursor_y,
                            cursor_y + GROUP_HEADER_HEIGHT,
                            viewport.top(),
                            viewport.bottom(),
                        );
                        if header_visible {
                            paint_group_header(ui, &app.config, width, &group.label);
                        } else {
                            ui.add_space(GROUP_HEADER_HEIGHT);
                        }
                        cursor_y += GROUP_HEADER_HEIGHT;
                        for entry in group.entries {
                            let row_visible = rect_range_visible(
                                cursor_y,
                                cursor_y + ROW_HEIGHT,
                                viewport.top(),
                                viewport.bottom(),
                            );
                            if row_visible {
                                paint_row(app, ui, &columns, &entry, row_index, &mut action);
                            } else {
                                ui.add_space(ROW_HEIGHT);
                            }
                            cursor_y += ROW_HEIGHT;
                            row_index += 1;
                        }
                    }
                });
        });
    } else {
        let pending_scroll =
            pending_scroll_offset(app, ROW_HEIGHT, body_rect.height(), entry_count);
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(body_rect), |ui| {
            let mut scroll_area = egui::ScrollArea::vertical()
                .id_salt("file_table_body")
                .auto_shrink([false, false]);
            if let Some(offset) = pending_scroll {
                scroll_area = scroll_area.vertical_scroll_offset(offset);
            }
            scroll_area.show_rows(ui, ROW_HEIGHT, entry_count, |ui, range| {
                for row_index in range {
                    if let Some(entry) = app.filtered_entry_at(row_index) {
                        paint_row(app, ui, &columns, &entry, row_index, &mut action);
                    }
                }
            });
        });
    }

    if let Some(selection_rect) = app.drag_selection_rect() {
        let visible = selection_rect.intersect(body_rect);
        if visible.width() > 0.0 && visible.height() > 0.0 {
            let painter = ui.painter().with_clip_rect(body_rect);
            painter.rect_filled(visible, 2.0, theme::selection_rect_fill(&app.config));
            painter.rect_stroke(
                visible,
                2.0,
                Stroke::new(0.5, theme::selection_rect_stroke(&app.config)),
            );
        }
    }

    if let Some(sort) = sort_action {
        app.set_sort(sort);
    }

    if let Some(action) = action {
        run_action(app, action);
    }
}

fn show_visual_view(app: &mut BExplorerApp, ui: &mut egui::Ui, layout: VisualLayout) {
    if !ui.input(|input| input.pointer.primary_down()) {
        app.drag_selection = None;
    }

    let entry_count = app.filtered_entry_count();
    let width = ui.available_width();
    let height = ui.available_height();
    let top_padding = 8.0;
    let body_rect = Rect::from_min_size(
        ui.cursor().min + egui::vec2(0.0, top_padding),
        egui::vec2(width, (height - top_padding).max(120.0)),
    );
    let mut wheel_direction: i32 = 0;
    ui.input(|input| {
        if input.modifiers.ctrl || input.modifiers.command {
            for event in &input.events {
                if let egui::Event::MouseWheel { delta, .. } = event {
                    if delta.y > 0.0 {
                        wheel_direction = 1;
                    } else if delta.y < 0.0 {
                        wheel_direction = -1;
                    }
                }
            }
        }
    });
    if wheel_direction != 0 {
        app.cycle_view_mode(wheel_direction);
        ui.ctx().input_mut(|i| {
            i.events
                .retain(|e| !matches!(e, egui::Event::MouseWheel { .. }));
        });
        ui.ctx().request_repaint();
    }
    let mut action = None;
    let body_response = ui.interact(
        body_rect,
        ui.make_persistent_id("file_visual_body_drag"),
        Sense::click_and_drag(),
    );
    if body_response.drag_started() {
        ui.memory_mut(|memory| memory.request_focus(body_response.id));
        app.clear_text_input_active();
        if let Some(start) = ui.input(|input| input.pointer.press_origin()) {
            let additive = ui.input(|input| input.modifiers.ctrl || input.modifiers.command);
            app.begin_drag_selection(start, additive);
        }
    }
    if body_response.clicked() {
        ui.memory_mut(|memory| memory.request_focus(body_response.id));
        app.clear_text_input_active();
        app.clear_selection();
    }
    if let Some(destination) = app.active_path() {
        app.register_file_drag_folder_rect(destination, body_response.rect);
    }
    let has_selection = !app.selected.is_empty();
    let can_paste = app.can_paste(ui.ctx());
    body_response.context_menu(|ui| {
        background_context_menu(ui, &app.config, has_selection, can_paste, &mut action);
    });
    if let Some(pos) = ui.input(|input| input.pointer.interact_pos()) {
        app.update_drag_selection(pos);
    }
    if app.drag_selection.is_some() {
        app.prepare_drag_selection_frame();
    }

    if let Some(groups) = grouped_entry_sections(app) {
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(body_rect), |ui| {
            egui::ScrollArea::vertical()
                .id_salt(match layout {
                    VisualLayout::List => "file_list_grouped_view",
                    VisualLayout::SmallIcons => "file_small_icon_grouped_view",
                    VisualLayout::MediumIcons => "file_medium_icon_grouped_view",
                    VisualLayout::LargeIcons => "file_large_icon_grouped_view",
                    VisualLayout::ExtraLargeIcons => "file_extra_large_icon_grouped_view",
                    VisualLayout::Tiles => "file_tile_grouped_view",
                })
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut row_index = 0;
                    for group in groups {
                        paint_group_header(ui, &app.config, width, &group.label);
                        if matches!(layout, VisualLayout::List) {
                            for entry in group.entries {
                                paint_visual_item(
                                    app,
                                    ui,
                                    layout,
                                    &entry,
                                    row_index,
                                    width,
                                    &mut action,
                                );
                                row_index += 1;
                            }
                        } else {
                            row_index = paint_visual_grid_entries(
                                app,
                                ui,
                                layout,
                                &group.entries,
                                width,
                                row_index,
                                &mut action,
                            );
                        }
                    }
                });
        });

        if let Some(selection_rect) = app.drag_selection_rect() {
            let visible = selection_rect.intersect(body_rect);
            if visible.width() > 0.0 && visible.height() > 0.0 {
                let painter = ui.painter().with_clip_rect(body_rect);
                painter.rect_filled(visible, 2.0, theme::selection_rect_fill(&app.config));
                painter.rect_stroke(
                    visible,
                    2.0,
                    Stroke::new(0.5, theme::selection_rect_stroke(&app.config)),
                );
            }
        }

        if let Some(action) = action {
            run_action(app, action);
        }
        return;
    }

    let row_height = visual_scroll_row_height(layout, width);
    let row_count = visual_scroll_row_count(layout, width, entry_count);
    let pending_scroll = pending_visual_scroll_offset(
        app,
        layout,
        width,
        body_rect.height(),
        entry_count,
        row_height,
        row_count,
    );
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(body_rect), |ui| {
        let mut scroll_area = egui::ScrollArea::vertical()
            .id_salt(match layout {
                VisualLayout::List => "file_list_view",
                VisualLayout::SmallIcons => "file_small_icon_view",
                VisualLayout::MediumIcons => "file_medium_icon_view",
                VisualLayout::LargeIcons => "file_large_icon_view",
                VisualLayout::ExtraLargeIcons => "file_extra_large_icon_view",
                VisualLayout::Tiles => "file_tile_view",
            })
            .auto_shrink([false, false]);
        if let Some(offset) = pending_scroll {
            scroll_area = scroll_area.vertical_scroll_offset(offset);
        }
        scroll_area.show_rows(ui, row_height, row_count, |ui, range| {
            ui.set_width(width);
            match layout {
                VisualLayout::List => {
                    for row_index in range {
                        if let Some(entry) = app.filtered_entry_at(row_index) {
                            paint_visual_item(
                                app,
                                ui,
                                layout,
                                &entry,
                                row_index,
                                width,
                                &mut action,
                            );
                        }
                    }
                }
                VisualLayout::SmallIcons
                | VisualLayout::MediumIcons
                | VisualLayout::LargeIcons
                | VisualLayout::ExtraLargeIcons
                | VisualLayout::Tiles => {
                    paint_visual_grid_rows(app, ui, layout, entry_count, width, range, &mut action);
                }
            }
        });
    });

    if let Some(selection_rect) = app.drag_selection_rect() {
        let visible = selection_rect.intersect(body_rect);
        if visible.width() > 0.0 && visible.height() > 0.0 {
            let painter = ui.painter().with_clip_rect(body_rect);
            painter.rect_filled(visible, 2.0, theme::selection_rect_fill(&app.config));
            painter.rect_stroke(
                visible,
                2.0,
                Stroke::new(0.5, theme::selection_rect_stroke(&app.config)),
            );
        }
    }

    if let Some(action) = action {
        run_action(app, action);
    }
}

fn visual_scroll_row_height(layout: VisualLayout, _width: f32) -> f32 {
    let (_, row_height) = visual_cell_metrics(layout);
    match layout {
        VisualLayout::List => row_height,
        _ => row_height + 6.0,
    }
}

fn pending_scroll_offset(
    app: &mut BExplorerApp,
    row_height: f32,
    viewport_height: f32,
    row_count: usize,
) -> Option<f32> {
    let path = app.take_pending_scroll_path()?;
    let index = app
        .filtered_entries_slice()
        .iter()
        .position(|entry| entry.path == path)?;
    Some(centered_scroll_offset(
        index,
        row_height,
        viewport_height,
        row_count,
    ))
}

fn pending_visual_scroll_offset(
    app: &mut BExplorerApp,
    layout: VisualLayout,
    width: f32,
    viewport_height: f32,
    entry_count: usize,
    row_height: f32,
    row_count: usize,
) -> Option<f32> {
    let path = app.take_pending_scroll_path()?;
    let entry_index = app
        .filtered_entries_slice()
        .iter()
        .position(|entry| entry.path == path)?;
    let row_index = if matches!(layout, VisualLayout::List) {
        entry_index
    } else {
        let (cell_width, _) = visual_cell_metrics(layout);
        let gap = 6.0;
        let columns = ((width - 16.0) / (cell_width + gap)).floor().max(1.0) as usize;
        entry_index.min(entry_count.saturating_sub(1)) / columns.max(1)
    };
    Some(centered_scroll_offset(
        row_index,
        row_height,
        viewport_height,
        row_count,
    ))
}

fn centered_scroll_offset(
    row_index: usize,
    row_height: f32,
    viewport_height: f32,
    row_count: usize,
) -> f32 {
    let target_top = row_index as f32 * row_height;
    let max_offset = (row_count as f32 * row_height - viewport_height).max(0.0);
    (target_top - viewport_height * 0.42).clamp(0.0, max_offset)
}

fn visual_scroll_row_count(layout: VisualLayout, width: f32, entry_count: usize) -> usize {
    if entry_count == 0 {
        return 0;
    }
    if matches!(layout, VisualLayout::List) {
        return entry_count;
    }
    let (cell_width, _) = visual_cell_metrics(layout);
    let gap = 6.0;
    let columns = ((width - 16.0) / (cell_width + gap)).floor().max(1.0) as usize;
    entry_count.div_ceil(columns)
}

fn paint_visual_grid_rows(
    app: &mut BExplorerApp,
    ui: &mut egui::Ui,
    layout: VisualLayout,
    entry_count: usize,
    width: f32,
    row_range: std::ops::Range<usize>,
    action: &mut Option<TableAction>,
) {
    let (cell_width, row_height) = visual_cell_metrics(layout);
    let gap = 6.0;
    let columns = ((width - 16.0) / (cell_width + gap)).floor().max(1.0) as usize;

    for row_index in row_range {
        let (row_rect, _) =
            ui.allocate_exact_size(egui::vec2(width, row_height + gap), Sense::hover());
        for column in 0..columns {
            let entry_index = row_index * columns + column;
            if entry_index >= entry_count {
                break;
            }
            let Some(entry) = app.filtered_entry_at(entry_index) else {
                break;
            };
            let left = row_rect.left() + 8.0 + column as f32 * (cell_width + gap);
            let rect = snap_rect(Rect::from_min_size(
                Pos2::new(left, row_rect.top() + 3.0),
                egui::vec2(cell_width, row_height),
            ));
            let response = ui.allocate_rect(rect, Sense::click_and_drag());
            paint_visual_item_at(app, ui, layout, &entry, entry_index, rect, response, action);
        }
    }
}

fn paint_visual_grid_entries(
    app: &mut BExplorerApp,
    ui: &mut egui::Ui,
    layout: VisualLayout,
    entries: &[FileEntry],
    width: f32,
    start_index: usize,
    action: &mut Option<TableAction>,
) -> usize {
    let (cell_width, row_height) = visual_cell_metrics(layout);
    let gap = 6.0;
    let columns = ((width - 16.0) / (cell_width + gap)).floor().max(1.0) as usize;
    let mut painted = 0_usize;

    for row in entries.chunks(columns) {
        let (row_rect, _) =
            ui.allocate_exact_size(egui::vec2(width, row_height + gap), Sense::hover());
        for (column, entry) in row.iter().enumerate() {
            let left = row_rect.left() + 8.0 + column as f32 * (cell_width + gap);
            let rect = snap_rect(Rect::from_min_size(
                Pos2::new(left, row_rect.top() + 3.0),
                egui::vec2(cell_width, row_height),
            ));
            let response = ui.allocate_rect(rect, Sense::click_and_drag());
            paint_visual_item_at(
                app,
                ui,
                layout,
                entry,
                start_index + painted,
                rect,
                response,
                action,
            );
            painted += 1;
        }
    }

    start_index + painted
}

fn paint_visual_item(
    app: &mut BExplorerApp,
    ui: &mut egui::Ui,
    layout: VisualLayout,
    entry: &FileEntry,
    row_index: usize,
    width: f32,
    action: &mut Option<TableAction>,
) {
    let height = match layout {
        VisualLayout::List => 28.0,
        VisualLayout::SmallIcons => 90.0,
        VisualLayout::MediumIcons => 120.0,
        VisualLayout::LargeIcons => 150.0,
        VisualLayout::ExtraLargeIcons => 192.0,
        VisualLayout::Tiles => 66.0,
    };
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, height), Sense::click_and_drag());
    paint_visual_item_at(
        app,
        ui,
        layout,
        entry,
        row_index,
        snap_rect(rect),
        response,
        action,
    );
}

fn paint_visual_item_at(
    app: &mut BExplorerApp,
    ui: &mut egui::Ui,
    layout: VisualLayout,
    entry: &FileEntry,
    row_index: usize,
    rect: Rect,
    response: egui::Response,
    action: &mut Option<TableAction>,
) {
    if response.drag_started() {
        ui.memory_mut(|memory| memory.request_focus(response.id));
        app.clear_text_input_active();
        app.begin_file_drag(entry.path.clone());
    }

    let selected_by_drag = app
        .drag_selection_rect()
        .map(|selection_rect| selection_rect.intersects(rect))
        .unwrap_or(false);
    if selected_by_drag {
        app.add_drag_selected(entry.path.clone());
    }
    let selected = app.selected.contains(&entry.path);
    let hovered = response.hovered();
    let drop_hovered = file_drag_drop_hovered(app, ui, entry, rect);
    if entry.kind.is_container() {
        app.register_file_drag_folder_rect(entry.path.clone(), rect);
    }

    if selected {
        theme::paint_selection_gradient(ui.painter(), rect, &app.config);
        if drop_hovered {
            paint_file_drag_drop_hover(ui.painter(), rect.shrink(1.0), 4.0, &app.config);
        }
    } else if drop_hovered {
        paint_file_drag_drop_hover(ui.painter(), rect.shrink(1.0), 4.0, &app.config);
    } else if hovered {
        theme::paint_row_hover_gradient(ui.painter(), rect.shrink(1.0), 4.0, &app.config);
    } else if matches!(layout, VisualLayout::List) && row_index % 2 == 1 {
        ui.painter()
            .rect_filled(rect, 0.0, theme::row_alt(&app.config));
    }

    if matches!(layout, VisualLayout::List) {
        ui.painter().line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            Stroke::new(1.0, theme::subtle_stroke(&app.config)),
        );
    }

    if response.double_clicked() {
        ui.memory_mut(|memory| memory.request_focus(response.id));
        app.clear_text_input_active();
        *action = Some(TableAction::Open(entry.clone()));
    } else if response.clicked() {
        ui.memory_mut(|memory| memory.request_focus(response.id));
        app.clear_text_input_active();
        let (additive, range) = ui.input(|input| {
            (
                input.modifiers.ctrl || input.modifiers.command,
                input.modifiers.shift,
            )
        });
        *action = Some(TableAction::Select(entry.clone(), additive, range));
    }
    if response.secondary_clicked() {
        ui.memory_mut(|memory| memory.request_focus(response.id));
        app.clear_text_input_active();
        app.ensure_selected(entry.path.clone());
    }
    let can_paste = app.can_paste(ui.ctx());
    let show_open_location = app.showing_complete_search_results();
    response.context_menu(|ui| {
        context_menu(
            ui,
            &app.config,
            entry,
            can_paste,
            show_open_location,
            action,
        );
    });

    let display_name = entry_display_name(&app.config, entry);
    let cut = app.is_cut_path(&entry.path);

    match layout {
        VisualLayout::List => {
            let icon_rect = Rect::from_center_size(
                Pos2::new(rect.left() + 18.0, rect.center().y),
                Vec2::splat(16.0),
            );
            paint_entry_icon(app, ui, icon_rect, entry, selected, cut);
            let name_rect = Rect::from_min_max(
                Pos2::new(rect.left() + 34.0, rect.top()),
                Pos2::new(rect.right() - 8.0, rect.bottom()),
            );
            if !paint_inline_rename_editor(app, ui, name_rect.shrink2(egui::vec2(0.0, 2.0)), entry)
            {
                draw_entry_name_text_clipped(
                    app,
                    ui,
                    name_rect,
                    snap_pos(Pos2::new(rect.left() + 36.0, rect.center().y)),
                    &display_name,
                    theme::font(&app.config, 12.6),
                    entry_text_color(
                        &app.config,
                        selected,
                        entry.is_hidden,
                        cut,
                        theme::text(&app.config),
                    ),
                    Align2::LEFT_CENTER,
                    selected,
                );
            }
        }
        VisualLayout::SmallIcons => {
            let icon_rect = Rect::from_center_size(
                Pos2::new(rect.center().x, rect.top() + 28.0),
                Vec2::splat(45.0),
            );
            paint_entry_icon(app, ui, icon_rect, entry, selected, cut);
            let name_rect = Rect::from_min_max(
                Pos2::new(rect.left() + 7.0, rect.top() + 52.0),
                Pos2::new(rect.right() - 7.0, rect.bottom() - 6.0),
            );
            if !paint_inline_rename_editor(app, ui, name_rect.shrink2(egui::vec2(0.0, 3.0)), entry)
            {
                draw_entry_name_text_clipped(
                    app,
                    ui,
                    name_rect,
                    snap_pos(Pos2::new(rect.center().x, rect.top() + 70.0)),
                    &display_name,
                    theme::font(&app.config, 13.0),
                    entry_text_color(
                        &app.config,
                        selected,
                        entry.is_hidden,
                        cut,
                        theme::text(&app.config),
                    ),
                    Align2::CENTER_CENTER,
                    selected,
                );
            }
        }
        VisualLayout::MediumIcons => {
            let icon_rect = Rect::from_center_size(
                Pos2::new(rect.center().x, rect.top() + 38.0),
                Vec2::splat(63.0),
            );
            paint_entry_icon(app, ui, icon_rect, entry, selected, cut);
            let name_rect = Rect::from_min_max(
                Pos2::new(rect.left() + 7.0, rect.top() + 70.0),
                Pos2::new(rect.right() - 7.0, rect.bottom() - 6.0),
            );
            if !paint_inline_rename_editor(app, ui, name_rect.shrink2(egui::vec2(0.0, 3.0)), entry)
            {
                draw_entry_name_text_clipped(
                    app,
                    ui,
                    name_rect,
                    snap_pos(Pos2::new(rect.center().x, rect.top() + 92.0)),
                    &display_name,
                    theme::font(&app.config, 13.3),
                    entry_text_color(
                        &app.config,
                        selected,
                        entry.is_hidden,
                        cut,
                        theme::text(&app.config),
                    ),
                    Align2::CENTER_CENTER,
                    selected,
                );
            }
        }
        VisualLayout::LargeIcons => {
            let icon_rect = Rect::from_center_size(
                Pos2::new(rect.center().x, rect.top() + 52.0),
                Vec2::splat(90.0),
            );
            paint_entry_icon(app, ui, icon_rect, entry, selected, cut);
            let name_rect = Rect::from_min_max(
                Pos2::new(rect.left() + 7.0, rect.top() + 97.0),
                Pos2::new(rect.right() - 7.0, rect.bottom() - 6.0),
            );
            if !paint_inline_rename_editor(app, ui, name_rect.shrink2(egui::vec2(0.0, 3.0)), entry)
            {
                draw_entry_name_text_clipped(
                    app,
                    ui,
                    name_rect,
                    snap_pos(Pos2::new(rect.center().x, rect.top() + 118.0)),
                    &display_name,
                    theme::font(&app.config, 13.7),
                    entry_text_color(
                        &app.config,
                        selected,
                        entry.is_hidden,
                        cut,
                        theme::text(&app.config),
                    ),
                    Align2::CENTER_CENTER,
                    selected,
                );
            }
        }
        VisualLayout::ExtraLargeIcons => {
            let icon_rect = Rect::from_center_size(
                Pos2::new(rect.center().x, rect.top() + 70.0),
                Vec2::splat(126.0),
            );
            paint_entry_icon(app, ui, icon_rect, entry, selected, cut);
            let name_rect = Rect::from_min_max(
                Pos2::new(rect.left() + 7.0, rect.top() + 133.0),
                Pos2::new(rect.right() - 7.0, rect.bottom() - 6.0),
            );
            if !paint_inline_rename_editor(app, ui, name_rect.shrink2(egui::vec2(0.0, 3.0)), entry)
            {
                draw_entry_name_text_clipped(
                    app,
                    ui,
                    name_rect,
                    snap_pos(Pos2::new(rect.center().x, rect.top() + 152.0)),
                    &display_name,
                    theme::font(&app.config, 14.0),
                    entry_text_color(
                        &app.config,
                        selected,
                        entry.is_hidden,
                        cut,
                        theme::text(&app.config),
                    ),
                    Align2::CENTER_CENTER,
                    selected,
                );
            }
        }
        VisualLayout::Tiles => {
            let icon_rect = Rect::from_center_size(
                Pos2::new(rect.left() + 25.0, rect.center().y),
                Vec2::splat(32.0),
            );
            paint_entry_icon(app, ui, icon_rect, entry, selected, cut);
            let name_rect = Rect::from_min_max(
                Pos2::new(rect.left() + 48.0, rect.top() + 7.0),
                Pos2::new(rect.right() - 8.0, rect.top() + 29.0),
            );
            if !paint_inline_rename_editor(app, ui, name_rect.shrink2(egui::vec2(0.0, 2.0)), entry)
            {
                draw_entry_name_text_clipped(
                    app,
                    ui,
                    name_rect,
                    snap_pos(Pos2::new(rect.left() + 50.0, rect.top() + 18.0)),
                    &display_name,
                    theme::font(&app.config, 12.5),
                    entry_text_color(
                        &app.config,
                        selected,
                        entry.is_hidden,
                        cut,
                        theme::text(&app.config),
                    ),
                    Align2::LEFT_CENTER,
                    selected,
                );
            }
            if let Some(percent) = entry.percent_full {
                paint_tile_usage_bar(
                    ui,
                    &app.config,
                    Rect::from_min_max(
                        Pos2::new(rect.left() + 50.0, rect.top() + 31.0),
                        Pos2::new(rect.right() - 13.0, rect.top() + 39.0),
                    ),
                    percent,
                );
            }
            let meta = format!(
                "{}  {}",
                localized_type_label(&app.config, &entry.type_label()),
                format_bytes_opt(entry.size)
            );
            let meta_top = if entry.percent_full.is_some() {
                rect.top() + 40.0
            } else {
                rect.top() + 29.0
            };
            draw_text_clipped(
                ui,
                Rect::from_min_max(
                    Pos2::new(rect.left() + 50.0, meta_top),
                    Pos2::new(rect.right() - 8.0, rect.bottom() - 5.0),
                ),
                snap_pos(Pos2::new(rect.left() + 50.0, meta_top + 8.0)),
                &meta,
                theme::font(&app.config, 11.4),
                entry_text_color(
                    &app.config,
                    selected,
                    entry.is_hidden,
                    cut,
                    theme::muted(&app.config),
                ),
                Align2::LEFT_CENTER,
            );
        }
    }
}

fn paint_tile_usage_bar(ui: &mut egui::Ui, config: &AppConfig, rect: Rect, percent: f32) {
    let track = snap_rect(rect);
    let critical = percent >= 0.90;
    let rounding = 2.0;
    ui.painter()
        .rect_filled(track, rounding, theme::control(config));
    let stroke = if critical {
        Color32::from_rgb(155, 55, 50)
    } else {
        theme::stroke(config)
    };
    ui.painter()
        .rect_stroke(track, rounding, Stroke::new(0.75, stroke));
    let fill_width = track.width() * percent.clamp(0.0, 1.0);
    if fill_width > 0.5 {
        let fill = Rect::from_min_max(
            track.left_top(),
            Pos2::new(track.left() + fill_width, track.bottom()),
        );
        let color = if critical {
            Color32::from_rgb(218, 61, 54)
        } else {
            theme::percent_fill(config)
        };
        if critical {
            let left = blend_bar_color(color, Color32::BLACK, 0.14);
            let right = blend_bar_color(color, Color32::WHITE, 0.24);
            paint_rounded_bar_gradient(ui, fill, rounding, left, right);
        } else {
            theme::paint_selection_gradient_rounded(ui.painter(), fill, rounding, config);
        }
    }
}

fn paint_rounded_bar_gradient(
    ui: &mut egui::Ui,
    rect: Rect,
    rounding: f32,
    left: Color32,
    right: Color32,
) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }

    ui.painter().rect_filled(rect, rounding, left);
    let steps = ((rect.width() / 5.0).round() as usize).clamp(6, 36);

    for step in 0..steps {
        let t0 = step as f32 / steps as f32;
        let t1 = (step + 1) as f32 / steps as f32;
        let strip = Rect::from_min_max(
            Pos2::new(rect.left() + rect.width() * t0, rect.top()),
            Pos2::new(rect.left() + rect.width() * t1 + 0.75, rect.bottom()),
        );
        let color = blend_bar_color(left, right, (t0 + t1) * 0.5);
        let strip_rounding = if step == 0 || step + 1 == steps {
            rounding
        } else {
            0.0
        };
        ui.painter().rect_filled(strip, strip_rounding, color);
    }
}

fn blend_bar_color(base: Color32, tint: Color32, amount: f32) -> Color32 {
    let mix = |left: u8, right: u8| -> u8 {
        ((left as f32 * (1.0 - amount)) + (right as f32 * amount))
            .round()
            .clamp(0.0, 255.0) as u8
    };

    Color32::from_rgb(
        mix(base.r(), tint.r()),
        mix(base.g(), tint.g()),
        mix(base.b(), tint.b()),
    )
}

fn visual_cell_metrics(layout: VisualLayout) -> (f32, f32) {
    match layout {
        VisualLayout::List => (0.0, 28.0),
        VisualLayout::SmallIcons => (144.0, 90.0),
        VisualLayout::MediumIcons => (174.0, 120.0),
        VisualLayout::LargeIcons => (210.0, 150.0),
        VisualLayout::ExtraLargeIcons => (264.0, 192.0),
        VisualLayout::Tiles => (248.0, 66.0),
    }
}

fn paint_header(
    app: &mut BExplorerApp,
    ui: &mut egui::Ui,
    rect: Rect,
    columns: &[ColumnSpec],
    sort_action: &mut Option<FileSort>,
) {
    ui.painter()
        .rect_filled(rect, 0.0, theme::canvas(&app.config));
    ui.painter().line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        Stroke::new(1.0, theme::subtle_stroke(&app.config)),
    );

    let (active_sort, ascending) = app.sort_state();
    let mut x = rect.left();
    for (index, column) in columns.iter().enumerate() {
        let cell = Rect::from_min_size(
            Pos2::new(x, rect.top()),
            egui::vec2(column.width, rect.height()),
        );
        let response = if column.sort.is_some() {
            ui.allocate_rect(cell, Sense::click())
        } else {
            ui.allocate_rect(cell, Sense::hover())
        };

        if response.hovered() && column.sort.is_some() {
            theme::paint_row_hover_gradient(
                ui.painter(),
                cell.shrink2(egui::vec2(2.0, 4.0)),
                3.0,
                &app.config,
            );
        }

        if response.clicked() {
            *sort_action = column.sort;
        }

        let title = localized_column_title(app, column.title);
        let font = theme::font(&app.config, 12.0);
        let color = theme::muted(&app.config);
        let galley = ui.painter().layout_no_wrap(title.to_string(), font, color);
        let text_pos = if column.right_align {
            Pos2::new(cell.right() - 10.0, cell.center().y)
        } else {
            Pos2::new(cell.left() + 10.0, cell.center().y)
        };
        let galley_size = galley.size();
        let galley_pos = if column.right_align {
            Pos2::new(text_pos.x - galley_size.x, text_pos.y - galley_size.y / 2.0)
        } else {
            Pos2::new(text_pos.x, text_pos.y - galley_size.y / 2.0)
        };
        ui.painter().galley(galley_pos, galley, color);

        if column.sort == Some(active_sort) {
            let tri_size = egui::vec2(6.0, 6.0);
            let tri_x = if column.right_align {
                text_pos.x - galley_size.x - 4.0 - tri_size.x
            } else {
                text_pos.x + galley_size.x + 4.0
            };
            let tri_y = cell.center().y;
            let tri_color = theme::accent(&app.config);
            let tri_rect =
                Rect::from_center_size(Pos2::new(tri_x + tri_size.x / 2.0, tri_y), tri_size);
            let shape = if ascending {
                egui::Shape::convex_polygon(
                    vec![
                        Pos2::new(tri_rect.left(), tri_rect.bottom()),
                        Pos2::new(tri_rect.right(), tri_rect.bottom()),
                        Pos2::new(tri_rect.center().x, tri_rect.top()),
                    ],
                    tri_color,
                    Stroke::NONE,
                )
            } else {
                egui::Shape::convex_polygon(
                    vec![
                        Pos2::new(tri_rect.left(), tri_rect.top()),
                        Pos2::new(tri_rect.right(), tri_rect.top()),
                        Pos2::new(tri_rect.center().x, tri_rect.bottom()),
                    ],
                    tri_color,
                    Stroke::NONE,
                )
            };
            ui.painter().add(shape);
        }

        // Grip for column resize (all columns except the last)
        if index < columns.len() - 1 {
            let grip_rect = Rect::from_min_size(
                Pos2::new(cell.right() - 3.0, rect.top()),
                egui::vec2(6.0, rect.height()),
            );
            let grip_response = ui.allocate_rect(grip_rect, Sense::drag());
            if grip_response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeColumn);
            }
            if grip_response.dragged() {
                let delta = grip_response.drag_delta().x;
                let width_index = column_index(column.kind);
                app.set_column_width(width_index, app.column_widths[width_index] + delta);
            }
        }

        x += column.width;
    }
}

fn paint_row(
    app: &mut BExplorerApp,
    ui: &mut egui::Ui,
    columns: &[ColumnSpec],
    entry: &FileEntry,
    row_index: usize,
    action: &mut Option<TableAction>,
) {
    let width: f32 = columns.iter().map(|column| column.width).sum();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, ROW_HEIGHT), Sense::click_and_drag());
    let hovered = response.hovered();
    if response.drag_started() {
        ui.memory_mut(|memory| memory.request_focus(response.id));
        app.clear_text_input_active();
        app.begin_file_drag(entry.path.clone());
    }

    let selected_by_drag = app
        .drag_selection_rect()
        .map(|selection_rect| selection_rect.intersects(rect))
        .unwrap_or(false);
    if selected_by_drag {
        app.add_drag_selected(entry.path.clone());
    }
    let row_selected = app.selected.contains(&entry.path);
    let cut = app.is_cut_path(&entry.path);
    let drop_hovered = file_drag_drop_hovered(app, ui, entry, rect);
    if entry.kind.is_container() {
        app.register_file_drag_folder_rect(entry.path.clone(), rect);
    }
    let painter = ui.painter();

    if row_selected {
        theme::paint_selection_gradient(painter, rect, &app.config);
        if drop_hovered {
            paint_file_drag_drop_hover(painter, rect, 0.0, &app.config);
        }
    } else if drop_hovered {
        paint_file_drag_drop_hover(painter, rect, 0.0, &app.config);
    } else if hovered {
        theme::paint_row_hover_gradient(painter, rect, 0.0, &app.config);
    } else {
        let fill = if row_index % 2 == 0 {
            theme::surface(&app.config)
        } else {
            theme::row_alt(&app.config)
        };
        painter.rect_filled(rect, 0.0, fill);
    }

    painter.line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        Stroke::new(1.0, theme::subtle_stroke(&app.config)),
    );

    if response.double_clicked() {
        ui.memory_mut(|memory| memory.request_focus(response.id));
        app.clear_text_input_active();
        *action = Some(TableAction::Open(entry.clone()));
    } else if response.clicked() {
        ui.memory_mut(|memory| memory.request_focus(response.id));
        app.clear_text_input_active();
        let (additive, range) = ui.input(|input| {
            (
                input.modifiers.ctrl || input.modifiers.command,
                input.modifiers.shift,
            )
        });
        *action = Some(TableAction::Select(entry.clone(), additive, range));
    }
    if response.secondary_clicked() {
        ui.memory_mut(|memory| memory.request_focus(response.id));
        app.clear_text_input_active();
        app.ensure_selected(entry.path.clone());
    }

    let can_paste = app.can_paste(ui.ctx());
    let show_open_location = app.showing_complete_search_results();
    response.context_menu(|ui| {
        context_menu(
            ui,
            &app.config,
            entry,
            can_paste,
            show_open_location,
            action,
        );
    });

    let mut x = rect.left();
    for column in columns {
        let cell = Rect::from_min_size(
            Pos2::new(x, rect.top()),
            egui::vec2(column.width, ROW_HEIGHT),
        );
        match column.kind {
            ColumnKind::Name => paint_name_cell(app, ui, cell, entry, row_selected),
            ColumnKind::Type => paint_text_cell(
                ui,
                &app.config,
                cell,
                &localized_type_label(&app.config, &entry.type_label()),
                false,
                entry_text_color(
                    &app.config,
                    row_selected,
                    entry.is_hidden,
                    cut,
                    theme::muted(&app.config),
                ),
            ),
            ColumnKind::FileSystem => paint_text_cell(
                ui,
                &app.config,
                cell,
                &entry.file_system,
                false,
                entry_text_color(
                    &app.config,
                    row_selected,
                    entry.is_hidden,
                    cut,
                    theme::faint(&app.config),
                ),
            ),
            ColumnKind::FreeSpace => paint_text_cell(
                ui,
                &app.config,
                cell,
                &format_bytes_opt(entry.free_space),
                false,
                entry_text_color(
                    &app.config,
                    row_selected,
                    entry.is_hidden,
                    cut,
                    theme::faint(&app.config),
                ),
            ),
            ColumnKind::Size => paint_text_cell(
                ui,
                &app.config,
                cell,
                &format_bytes_opt(entry.size),
                true,
                entry_text_color(
                    &app.config,
                    row_selected,
                    entry.is_hidden,
                    cut,
                    theme::muted(&app.config),
                ),
            ),
            ColumnKind::PercentFull => {
                paint_percent_cell(ui, &app.config, cell, entry.percent_full)
            }
            ColumnKind::Modified => paint_text_cell(
                ui,
                &app.config,
                cell,
                entry.modified.as_deref().unwrap_or(""),
                false,
                entry_text_color(
                    &app.config,
                    row_selected,
                    entry.is_hidden,
                    cut,
                    theme::muted(&app.config),
                ),
            ),
            ColumnKind::Location => paint_text_cell(
                ui,
                &app.config,
                cell,
                &entry_location_text(entry),
                false,
                entry_text_color(
                    &app.config,
                    row_selected,
                    entry.is_hidden,
                    cut,
                    theme::muted(&app.config),
                ),
            ),
        }
        x += column.width;
    }
}

fn paint_name_cell(
    app: &mut BExplorerApp,
    ui: &mut egui::Ui,
    cell: Rect,
    entry: &FileEntry,
    selected: bool,
) {
    let icon_rect = Rect::from_center_size(
        Pos2::new(cell.left() + 18.0, cell.center().y),
        Vec2::splat(ICON_SIZE),
    );
    let cut = app.is_cut_path(&entry.path);

    paint_entry_icon(app, ui, icon_rect, entry, selected, cut);

    let name_rect = Rect::from_min_max(
        Pos2::new(cell.left() + 32.0, cell.top() + 2.0),
        Pos2::new(cell.right() - 6.0, cell.bottom() - 2.0),
    );
    if paint_inline_rename_editor(app, ui, name_rect, entry) {
        return;
    }

    draw_entry_name_text_clipped(
        app,
        ui,
        Rect::from_min_max(
            Pos2::new(cell.left() + 32.0, cell.top()),
            cell.right_bottom(),
        ),
        Pos2::new(cell.left() + 34.0, cell.center().y),
        &entry_display_name(&app.config, entry),
        theme::font(&app.config, 12.6),
        entry_text_color(
            &app.config,
            selected,
            entry.is_hidden,
            cut,
            theme::text(&app.config),
        ),
        Align2::LEFT_CENTER,
        selected,
    );
}

fn paint_entry_icon(
    app: &mut BExplorerApp,
    ui: &mut egui::Ui,
    icon_rect: Rect,
    entry: &FileEntry,
    selected: bool,
    cut: bool,
) {
    let icon_rect = snap_rect(icon_rect);
    let tint = if cut {
        theme::cut_tint(&app.config, Color32::WHITE)
    } else if entry.is_hidden && !selected {
        theme::hidden_icon_tint(&app.config, Color32::WHITE)
    } else {
        Color32::WHITE
    };

    if let Some(texture_id) = app.thumbnail_texture_id(ui.ctx(), entry) {
        ui.painter()
            .rect_filled(icon_rect, 2.0, theme::control(&app.config));
        ui.painter().image(
            texture_id,
            icon_rect.shrink(1.0),
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            tint,
        );
        if app.config.show_icon_borders {
            paint_icon_border(ui.painter(), icon_rect, &app.config, selected);
        }
    } else if let Some(texture_id) = app.native_icon_texture_id(ui.ctx(), entry) {
        ui.painter().image(
            texture_id,
            icon_rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            tint,
        );
        if app.config.show_icon_borders {
            paint_icon_border(ui.painter(), icon_rect, &app.config, selected);
        }
    } else {
        crate::ui::icons::draw_entry_icon(ui.painter(), icon_rect, entry);
        if cut {
            ui.painter().rect_filled(
                icon_rect,
                2.0,
                theme::cut_tint(&app.config, theme::canvas(&app.config)),
            );
        } else if entry.is_hidden && !selected {
            ui.painter().rect_filled(
                icon_rect,
                2.0,
                theme::hidden_icon_tint(&app.config, theme::canvas(&app.config)),
            );
        }
        if app.config.show_icon_borders {
            paint_icon_border(ui.painter(), icon_rect, &app.config, selected);
        }
    }
}

fn paint_icon_border(painter: &egui::Painter, rect: Rect, config: &AppConfig, selected: bool) {
    let accent = theme::accent(config);
    let outer_alpha = if selected { 110 } else { 70 };
    let inner_alpha = if selected { 235 } else { 165 };
    let outer = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), outer_alpha);
    let inner = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), inner_alpha);
    painter.rect_stroke(rect.expand(1.0), 3.0, Stroke::new(0.75, outer));
    painter.rect_stroke(rect.expand(0.25), 2.5, Stroke::new(0.55, inner));
}

fn draw_drag_file_icon(
    painter: &egui::Painter,
    rect: Rect,
    config: &AppConfig,
    badge: Option<usize>,
) {
    let shadow = rect.translate(egui::vec2(2.0, 2.0));
    painter.rect_filled(shadow, 3.0, Color32::from_black_alpha(70));
    let body = rect.shrink2(egui::vec2(3.0, 2.0));
    painter.rect_filled(body, 3.0, theme::surface_elevated(config));
    painter.rect_stroke(body, 3.0, Stroke::new(1.0, theme::accent(config)));
    painter.line_segment(
        [
            Pos2::new(body.left() + 4.0, body.top() + 7.0),
            Pos2::new(body.right() - 4.0, body.top() + 7.0),
        ],
        Stroke::new(1.1, theme::muted(config)),
    );
    painter.line_segment(
        [
            Pos2::new(body.left() + 4.0, body.top() + 12.0),
            Pos2::new(body.right() - 7.0, body.top() + 12.0),
        ],
        Stroke::new(1.1, theme::muted(config)),
    );

    if let Some(count) = badge
        && count > 1
    {
        let badge_radius = 7.0;
        let badge_center = Pos2::new(rect.right() - 2.0, rect.bottom() - 2.0);
        painter.circle_filled(badge_center, badge_radius, theme::accent(config));
        let label = if count > 99 {
            "99+".to_string()
        } else {
            count.to_string()
        };
        painter.text(
            badge_center,
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(8.5),
            Color32::WHITE,
        );
    }
}

fn entry_text_color(
    config: &AppConfig,
    selected: bool,
    hidden: bool,
    cut: bool,
    fallback: Color32,
) -> Color32 {
    let color = if selected {
        match config.theme {
            crate::app::config::ThemePreference::Dark => Color32::from_rgb(238, 248, 250),
            crate::app::config::ThemePreference::Light
            | crate::app::config::ThemePreference::Gray => Color32::from_rgb(255, 255, 255),
        }
    } else if hidden {
        theme::hidden_tint(config, fallback)
    } else {
        fallback
    };

    if cut {
        theme::cut_tint(config, color)
    } else {
        color
    }
}

pub(super) fn entry_display_name(config: &AppConfig, entry: &FileEntry) -> String {
    if config.show_extensions || entry.kind.is_container() {
        return entry.name.clone();
    }

    let name_path = Path::new(&entry.name);
    name_path
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(&entry.name)
        .to_string()
}

pub(super) fn entry_location_text(entry: &FileEntry) -> String {
    entry
        .path
        .parent()
        .unwrap_or(entry.path.as_path())
        .display()
        .to_string()
}

fn draw_entry_name_text_clipped(
    app: &BExplorerApp,
    ui: &mut egui::Ui,
    clip_rect: Rect,
    pos: Pos2,
    text: &str,
    font: FontId,
    color: Color32,
    align: Align2,
    selected: bool,
) {
    let ranges = search_highlight_ranges(text, &app.filter);
    if ranges.is_empty() {
        draw_text_clipped(ui, clip_rect, pos, text, font, color, align);
        return;
    }

    let job = highlighted_text_layout_job(text, font, color, &app.config, selected, &ranges);
    let galley = ui.fonts(|fonts| fonts.layout_job(job));
    let galley_pos = align.anchor_size(snap_pos(pos), galley.size()).min;
    ui.painter()
        .with_clip_rect(clip_rect)
        .galley(galley_pos, galley, color);
}

fn paint_inline_rename_editor(
    app: &mut BExplorerApp,
    ui: &mut egui::Ui,
    rect: Rect,
    entry: &FileEntry,
) -> bool {
    let active = app
        .rename_dialog
        .as_ref()
        .is_some_and(|dialog| dialog.path == entry.path);
    if !active {
        return false;
    }

    let clicked_outside = ui.input(|input| {
        input
            .pointer
            .press_origin()
            .is_some_and(|pos| !rect.expand(3.0).contains(pos))
    });
    if clicked_outside {
        app.apply_rename();
        return false;
    }

    let mut apply = false;
    let mut cancel = false;
    let editor_id = ui.make_persistent_id(("inline_rename", entry.path.clone()));

    ui.painter()
        .rect_filled(rect, 1.0, theme::surface_elevated(&app.config));
    ui.painter()
        .rect_stroke(rect, 0.0, Stroke::new(1.0, theme::accent(&app.config)));

    ui.allocate_new_ui(
        egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(3.0, 1.0))),
        |ui| {
            let pending_select_range = app
                .rename_dialog
                .as_ref()
                .and_then(|dialog| dialog.select_range);
            let selected_range = rename_editor_selection_range(ui, editor_id, pending_select_range);
            let text_color = theme::text(&app.config);
            let mut layouter = move |ui: &egui::Ui, text: &str, _wrap_width: f32| {
                let font_id = egui::FontSelection::Default.resolve(ui.style());
                let layout_job =
                    rename_editor_layout_job(text, font_id, text_color, selected_range.clone());
                ui.fonts(|fonts| fonts.layout_job(layout_job))
            };

            if let Some(dialog) = app.rename_dialog.as_mut() {
                let mut output = egui::TextEdit::singleline(&mut dialog.value)
                    .id(editor_id)
                    .frame(false)
                    .desired_width(rect.width() - 6.0)
                    .layouter(&mut layouter)
                    .show(ui);
                output.response.request_focus();
                if let Some((start, end)) = dialog.select_range.take() {
                    output
                        .state
                        .cursor
                        .set_char_range(Some(egui::text::CCursorRange::two(
                            egui::text::CCursor::new(start),
                            egui::text::CCursor::new(end),
                        )));
                    output.state.clone().store(ui.ctx(), output.response.id);
                }
                apply = ui.input(|input| input.key_pressed(egui::Key::Enter));
                cancel = ui.input(|input| input.key_pressed(egui::Key::Escape));
                if output.response.lost_focus() && !cancel {
                    apply = true;
                }
            }
        },
    );

    if cancel {
        app.rename_dialog = None;
    } else if apply {
        app.apply_rename();
    }

    true
}

fn rename_editor_selection_range(
    ui: &egui::Ui,
    editor_id: Id,
    pending_select_range: Option<(usize, usize)>,
) -> Option<std::ops::Range<usize>> {
    pending_select_range
        .map(|(start, end)| start.min(end)..start.max(end))
        .filter(|range| range.start < range.end)
        .or_else(|| {
            egui::TextEdit::load_state(ui.ctx(), editor_id)
                .and_then(|state| state.cursor.char_range())
                .map(|range| {
                    let [start, end] = range.sorted();
                    start.index.min(end.index)..start.index.max(end.index)
                })
                .filter(|range| range.start < range.end)
        })
}

fn rename_editor_layout_job(
    text: &str,
    font_id: FontId,
    text_color: Color32,
    selected_range: Option<std::ops::Range<usize>>,
) -> egui::text::LayoutJob {
    let Some(selected_range) = selected_range else {
        return egui::text::LayoutJob::simple_singleline(text.to_owned(), font_id, text_color);
    };

    let char_count = text.chars().count();
    let start = selected_range.start.min(char_count);
    let end = selected_range.end.min(char_count);
    if start >= end {
        return egui::text::LayoutJob::simple_singleline(text.to_owned(), font_id, text_color);
    }

    let start_byte = char_to_byte_index(text, start);
    let end_byte = char_to_byte_index(text, end);
    let normal = egui::TextFormat::simple(font_id.clone(), text_color);
    let selected = egui::TextFormat::simple(font_id, Color32::WHITE);
    let mut job = egui::text::LayoutJob {
        break_on_newline: false,
        ..Default::default()
    };
    if start_byte > 0 {
        job.append(&text[..start_byte], 0.0, normal.clone());
    }
    job.append(&text[start_byte..end_byte], 0.0, selected);
    if end_byte < text.len() {
        job.append(&text[end_byte..], 0.0, normal);
    }
    job
}

fn paint_text_cell(
    ui: &mut egui::Ui,
    config: &AppConfig,
    cell: Rect,
    text: &str,
    right_align: bool,
    color: Color32,
) {
    let pos = if right_align {
        Pos2::new(cell.right() - 10.0, cell.center().y)
    } else {
        Pos2::new(cell.left() + 10.0, cell.center().y)
    };
    draw_text_clipped(
        ui,
        cell.shrink2(egui::vec2(8.0, 0.0)),
        pos,
        text,
        theme::font(config, 12.2),
        color,
        if right_align {
            Align2::RIGHT_CENTER
        } else {
            Align2::LEFT_CENTER
        },
    );
}

fn paint_percent_cell(ui: &mut egui::Ui, config: &AppConfig, cell: Rect, percent: Option<f32>) {
    let Some(percent) = percent else {
        return;
    };

    let track = Rect::from_min_max(
        Pos2::new(cell.left() + 10.0, cell.center().y - 8.0),
        Pos2::new(cell.right() - 10.0, cell.center().y + 8.0),
    );
    ui.painter().rect_filled(track, 3.0, theme::control(config));
    ui.painter()
        .rect_stroke(track, 3.0, Stroke::new(1.0, theme::stroke(config)));
    let fill = Rect::from_min_max(
        track.left_top(),
        Pos2::new(
            track.left() + track.width() * percent.clamp(0.0, 1.0),
            track.bottom(),
        ),
    );
    theme::paint_percent_gradient(ui.painter(), fill, config);
    draw_text(
        ui,
        track.center(),
        &format!("{:.1}%", percent * 100.0),
        theme::font(config, 11.0),
        theme::muted(config),
        Align2::CENTER_CENTER,
    );
}

pub(crate) fn context_menu(
    ui: &mut egui::Ui,
    config: &AppConfig,
    entry: &FileEntry,
    can_paste: bool,
    show_open_location: bool,
    action: &mut Option<TableAction>,
) {
    begin_context_menu_animation(ui, config, "entry-context-menu");
    let stem = entry
        .path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&entry.name)
        .to_string();
    let extract_menu = entry.category == FileCategory::Archive
        || crate::fs::archive_listing::has_extractable_archive_extension(&entry.path);
    let menu_width =
        context_menu_width_for_entry(ui, config, entry, extract_menu, show_open_location);
    apply_context_menu_width(ui, menu_width);
    ui.spacing_mut().item_spacing = egui::vec2(0.0, 4.0);

    ui.horizontal(|ui| {
        ui.add_space(2.0);
        if context_command_button(ui, config, MenuIcon::Copy, i18n::tr(config, "copy"), true)
            .clicked()
        {
            *action = Some(TableAction::Copy(entry.clone()));
            ui.close_menu();
        }
        if context_command_button(ui, config, MenuIcon::Cut, i18n::tr(config, "cut"), true)
            .clicked()
        {
            *action = Some(TableAction::Cut(entry.clone()));
            ui.close_menu();
        }
        if context_command_button(
            ui,
            config,
            MenuIcon::Paste,
            i18n::tr(config, "paste"),
            can_paste,
        )
        .clicked()
        {
            *action = if entry.kind.is_container() {
                Some(TableAction::PasteInto(entry.clone()))
            } else {
                Some(TableAction::PasteHere)
            };
            ui.close_menu();
        }
    });

    ui.separator();

    if context_menu_row(
        ui,
        config,
        MenuIcon::Open,
        i18n::tr(config, "open"),
        None,
        true,
        false,
    )
    .clicked()
    {
        *action = Some(TableAction::Open(entry.clone()));
        ui.close_menu();
    }
    if context_menu_row(
        ui,
        config,
        MenuIcon::OpenWith,
        i18n::tr(config, "open_with"),
        None,
        true,
        false,
    )
    .clicked()
    {
        *action = Some(TableAction::OpenWith(entry.clone()));
        ui.close_menu();
    }
    #[cfg(target_os = "windows")]
    if can_scan_with_windows_defender(entry)
        && context_menu_row(
            ui,
            config,
            MenuIcon::Defender,
            i18n::tr(config, "scan_with_windows_defender"),
            None,
            true,
            false,
        )
        .clicked()
    {
        *action = Some(TableAction::ScanWithWindowsDefender(entry.clone()));
        ui.close_menu();
    }
    if show_open_location
        && context_menu_row(
            ui,
            config,
            MenuIcon::Folder,
            i18n::tr(config, "open_file_location"),
            None,
            true,
            false,
        )
        .clicked()
    {
        *action = Some(TableAction::OpenLocation(entry.clone()));
        ui.close_menu();
    }
    if is_ejectable_drive(entry)
        && context_menu_row(
            ui,
            config,
            MenuIcon::Eject,
            i18n::tr(config, "eject"),
            None,
            true,
            false,
        )
        .clicked()
    {
        *action = Some(TableAction::Eject(entry.clone()));
        ui.close_menu();
    }

    ui.separator();

    if matches!(entry.kind, EntryKind::File | EntryKind::Folder) {
        let compress_items = [
            SubmenuItem {
                icon: MenuIcon::Folder,
                label: i18n::tr(config, "compress").to_string(),
            },
            SubmenuItem {
                icon: MenuIcon::Folder,
                label: format!("{} {}.7z", i18n::tr(config, "compress"), stem),
            },
            SubmenuItem {
                icon: MenuIcon::Folder,
                label: format!("{} {}.zip", i18n::tr(config, "compress"), stem),
            },
        ];
        {
            let compress_entry = entry.clone();
            let mut on_compress = |index: usize| {
                *action = match index {
                    1 => Some(TableAction::CompressAs(
                        compress_entry.clone(),
                        ArchiveFormat::SevenZip,
                    )),
                    2 => Some(TableAction::CompressAs(
                        compress_entry.clone(),
                        ArchiveFormat::Zip,
                    )),
                    _ => Some(TableAction::Compress(compress_entry.clone())),
                };
            };
            show_delayed_submenu(
                ui,
                config,
                "compress-submenu",
                "compress",
                MenuIcon::Folder,
                &compress_items,
                &mut on_compress,
            );
        }

        if extract_menu {
            let extract_items = [
                SubmenuItem {
                    icon: MenuIcon::Folder,
                    label: i18n::tr(config, "extract_here").to_string(),
                },
                SubmenuItem {
                    icon: MenuIcon::Folder,
                    label: format!("{} {}/", i18n::tr(config, "extract_to"), stem),
                },
            ];
            {
                let extract_entry = entry.clone();
                let mut on_extract = |index: usize| {
                    let mode = match index {
                        1 => ExtractMode::ToNamedFolder,
                        _ => ExtractMode::Here,
                    };
                    *action = Some(TableAction::Extract(extract_entry.clone(), mode));
                };
                show_delayed_submenu(
                    ui,
                    config,
                    "extract-submenu",
                    "extract",
                    MenuIcon::Folder,
                    &extract_items,
                    &mut on_extract,
                );
            }
        }

        ui.separator();
    }

    if context_menu_row(
        ui,
        config,
        MenuIcon::Rename,
        i18n::tr(config, "rename"),
        None,
        true,
        false,
    )
    .clicked()
    {
        *action = Some(TableAction::Rename(entry.clone()));
        ui.close_menu();
    }
    if context_menu_row(
        ui,
        config,
        MenuIcon::Delete,
        i18n::tr(config, "delete"),
        None,
        true,
        false,
    )
    .clicked()
    {
        *action = Some(TableAction::Delete(entry.clone(), false));
        ui.close_menu();
    }
    if context_menu_row(
        ui,
        config,
        MenuIcon::Delete,
        i18n::tr(config, "delete_permanently"),
        None,
        true,
        false,
    )
    .clicked()
    {
        *action = Some(TableAction::Delete(entry.clone(), true));
        ui.close_menu();
    }

    ui.separator();

    if context_menu_row(
        ui,
        config,
        MenuIcon::Properties,
        i18n::tr(config, "properties"),
        Some("Alt+Enter"),
        true,
        false,
    )
    .clicked()
    {
        *action = Some(TableAction::Properties(entry.clone()));
        ui.close_menu();
    }
}

pub(crate) fn background_context_menu(
    ui: &mut egui::Ui,
    config: &AppConfig,
    has_selection: bool,
    can_paste: bool,
    action: &mut Option<TableAction>,
) {
    begin_context_menu_animation(ui, config, "background-context-menu");
    let menu_width = context_menu_width_for_background(ui, config);
    apply_context_menu_width(ui, menu_width);
    ui.spacing_mut().item_spacing = egui::vec2(0.0, 4.0);

    ui.horizontal(|ui| {
        ui.add_space(2.0);
        if context_command_button(
            ui,
            config,
            MenuIcon::Paste,
            i18n::tr(config, "paste"),
            can_paste,
        )
        .clicked()
        {
            *action = Some(TableAction::PasteHere);
            ui.close_menu();
        }
        if context_command_button(
            ui,
            config,
            MenuIcon::Copy,
            i18n::tr(config, "copy"),
            has_selection,
        )
        .clicked()
        {
            *action = Some(TableAction::CopySelected);
            ui.close_menu();
        }
        if context_command_button(
            ui,
            config,
            MenuIcon::Cut,
            i18n::tr(config, "cut"),
            has_selection,
        )
        .clicked()
        {
            *action = Some(TableAction::CutSelected);
            ui.close_menu();
        }
    });

    ui.separator();

    if context_menu_row(
        ui,
        config,
        MenuIcon::Refresh,
        i18n::tr(config, "refresh"),
        None,
        true,
        false,
    )
    .clicked()
    {
        *action = Some(TableAction::Refresh);
        ui.close_menu();
    }

    let new_items = [
        SubmenuItem {
            icon: MenuIcon::Folder,
            label: i18n::tr(config, "new_folder").to_string(),
        },
        SubmenuItem {
            icon: MenuIcon::TextDocument,
            label: i18n::tr(config, "text_document").to_string(),
        },
    ];
    let mut on_new_click = |idx: usize| match idx {
        0 => *action = Some(TableAction::CreateFolder),
        1 => *action = Some(TableAction::CreateTextDocument),
        _ => {}
    };
    show_delayed_submenu(
        ui,
        config,
        "new-submenu",
        "new",
        MenuIcon::New,
        &new_items,
        &mut on_new_click,
    );

    ui.separator();

    if context_menu_row(
        ui,
        config,
        MenuIcon::Terminal,
        i18n::tr(config, "open_terminal"),
        None,
        true,
        false,
    )
    .clicked()
    {
        *action = Some(TableAction::OpenTerminalHere);
        ui.close_menu();
    }
    if context_menu_row(
        ui,
        config,
        MenuIcon::Properties,
        i18n::tr(config, "properties"),
        Some("Alt+Enter"),
        true,
        false,
    )
    .clicked()
    {
        *action = Some(TableAction::PropertiesCurrent);
        ui.close_menu();
    }
}

#[allow(clippy::too_many_arguments)]
fn show_delayed_submenu(
    ui: &mut egui::Ui,
    config: &AppConfig,
    id_source: &'static str,
    parent_label: &str,
    parent_icon: MenuIcon,
    items: &[SubmenuItem],
    on_click: &mut dyn FnMut(usize),
) {
    ui.scope(|ui| {
        ui.visuals_mut().widgets.hovered.weak_bg_fill = theme::hover(config);
        ui.visuals_mut().widgets.open.weak_bg_fill = theme::hover(config);
        ui.visuals_mut().widgets.active.weak_bg_fill = theme::hover(config);
        ui.spacing_mut().button_padding = egui::vec2(8.0, 4.0);
        ui.spacing_mut().interact_size.y = 30.0;

        ui.visuals_mut().widgets.inactive.fg_stroke.color = Color32::TRANSPARENT;
        ui.visuals_mut().widgets.hovered.fg_stroke.color = Color32::TRANSPARENT;
        ui.visuals_mut().widgets.active.fg_stroke.color = Color32::TRANSPARENT;
        ui.visuals_mut().widgets.open.fg_stroke.color = Color32::TRANSPARENT;
        ui.style_mut().visuals.button_frame = false;

        let parent_width = context_menu_row_width(ui);
        ui.set_min_width(parent_width);
        ui.set_max_width(parent_width);

        let scope_id = ui.id();
        let btn_rect_id = scope_id.with((id_source, "btn-rect"));
        let sub_rect_id = scope_id.with((id_source, "sub-popup-rect"));

        let pointer = ui.ctx().input(|i| i.pointer.hover_pos());
        let prev_btn_rect: Option<Rect> = ui.ctx().data(|d| d.get_temp::<Rect>(btn_rect_id));
        let prev_sub_rect: Option<Rect> = ui
            .ctx()
            .data(|d| d.get_temp::<Rect>(sub_rect_id))
            .filter(|r| *r != Rect::NOTHING && r.is_finite());

        let hover_btn = match (prev_btn_rect, pointer) {
            (Some(r), Some(p)) if r.is_finite() => r.contains(p),
            _ => false,
        };
        let hover_sub = match (prev_sub_rect, pointer) {
            (Some(r), Some(p)) => r.expand(8.0).contains(p),
            _ => false,
        };
        let show_submenu = hover_btn || hover_sub;

        let parent_label_text = i18n::tr(config, parent_label);
        let (btn_rect, btn_hovered, submenu_open) = if show_submenu {
            let menu_resp = ui.menu_button(parent_label_text, |ui| {
                begin_context_menu_animation(ui, config, id_source);
                let submenu_width = context_submenu_width(ui, config, items);
                apply_context_menu_width(ui, submenu_width);
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 4.0);

                for (idx, item) in items.iter().enumerate() {
                    if context_menu_row(ui, config, item.icon, &item.label, None, true, false)
                        .clicked()
                    {
                        on_click(idx);
                        ui.close_menu();
                    }
                }

                let submenu_rect = ui.min_rect();
                ui.ctx()
                    .data_mut(|d| d.insert_temp(sub_rect_id, submenu_rect));
            });

            (
                menu_resp.response.rect,
                menu_resp.response.hovered(),
                menu_resp.inner.is_some(),
            )
        } else {
            ui.ctx()
                .data_mut(|d| d.insert_temp(sub_rect_id, Rect::NOTHING));

            let size = prev_btn_rect
                .filter(|r| r.is_finite())
                .map(|r| r.size())
                .unwrap_or(egui::vec2(parent_width, CONTEXT_ROW_HEIGHT));
            let (rect, response) = ui.allocate_exact_size(size, Sense::click());
            (rect, response.hovered(), false)
        };

        ui.ctx().data_mut(|d| d.insert_temp(btn_rect_id, btn_rect));

        if btn_hovered || submenu_open {
            theme::paint_hover_gradient(ui.painter(), btn_rect, 4.0, config);
        }
        draw_menu_icon(
            ui.painter(),
            Rect::from_center_size(
                Pos2::new(btn_rect.left() + 18.0, btn_rect.center().y),
                Vec2::splat(16.0),
            ),
            parent_icon,
            theme::muted(config),
        );
        draw_text_elided(
            ui,
            Rect::from_min_max(
                Pos2::new(btn_rect.left() + 36.0, btn_rect.top()),
                Pos2::new(btn_rect.right() - 28.0, btn_rect.bottom()),
            ),
            Pos2::new(btn_rect.left() + 36.0, btn_rect.center().y),
            parent_label_text,
            theme::font(config, 12.3),
            theme::text(config),
            Align2::LEFT_CENTER,
        );
        draw_submenu_chevron(
            ui.painter(),
            Rect::from_center_size(
                Pos2::new(btn_rect.right() - 12.0, btn_rect.center().y),
                Vec2::splat(12.0),
            ),
            theme::muted(config),
        );
    });
}

pub(super) fn begin_context_menu_animation(
    ui: &mut egui::Ui,
    config: &AppConfig,
    id_source: &'static str,
) {
    let frosted = frosted_menu_fill(config);
    ui.visuals_mut().window_fill = frosted;
    ui.visuals_mut().panel_fill = frosted;
    ui.visuals_mut().widgets.noninteractive.bg_fill = frosted;
    ui.visuals_mut().window_stroke = Stroke::new(1.0, theme::popup_stroke(config));
    ui.visuals_mut().popup_shadow = theme::popup_shadow(config);

    let id = ui.id().with(("context-menu-animation", id_source));
    let now = ui.ctx().input(|input| input.time);
    let state = ui
        .ctx()
        .data(|data| data.get_temp::<ContextMenuAnimationState>(id))
        .unwrap_or(ContextMenuAnimationState { started_at: now });
    ui.ctx().data_mut(|data| data.insert_temp(id, state));

    let progress = ((now - state.started_at) / 0.30).clamp(0.0, 1.0) as f32;
    let eased = 1.0 - (1.0 - progress).powi(3);
    if progress < 1.0 {
        ui.ctx().request_repaint();
    }

    ui.multiply_opacity(0.18 + eased * 0.82);
    ui.add_space((1.0 - eased) * 14.0);
}

fn context_command_button(
    ui: &mut egui::Ui,
    config: &AppConfig,
    icon: MenuIcon,
    label: &str,
    enabled: bool,
) -> egui::Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(64.0, 48.0), sense);
    if response.hovered() && enabled {
        theme::paint_hover_gradient(ui.painter(), rect, 5.0, config);
    }
    let color = if enabled {
        theme::muted(config)
    } else {
        theme::faint(config)
    };
    draw_menu_icon(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(rect.center().x, rect.top() + 16.0),
            Vec2::splat(16.0),
        ),
        icon,
        color,
    );
    draw_text(
        ui,
        Pos2::new(rect.center().x, rect.bottom() - 10.0),
        label,
        theme::font(config, 11.2),
        color,
        Align2::CENTER_CENTER,
    );
    response
}

pub(super) fn context_menu_row(
    ui: &mut egui::Ui,
    config: &AppConfig,
    icon: MenuIcon,
    label: &str,
    shortcut: Option<&str>,
    enabled: bool,
    submenu: bool,
) -> egui::Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let row_width = context_menu_row_width(ui);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(row_width, CONTEXT_ROW_HEIGHT), sense);
    if response.hovered() && enabled {
        theme::paint_hover_gradient(ui.painter(), rect, 4.0, config);
    }
    let color = if enabled {
        theme::text(config)
    } else {
        theme::faint(config)
    };
    let icon_color = if enabled {
        theme::muted(config)
    } else {
        theme::faint(config)
    };
    draw_menu_icon(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(rect.left() + 18.0, rect.center().y),
            Vec2::splat(16.0),
        ),
        icon,
        icon_color,
    );
    let label_right = if shortcut.is_some() {
        rect.right() - 86.0
    } else if submenu {
        rect.right() - 28.0
    } else {
        rect.right() - 12.0
    };
    draw_text_elided(
        ui,
        Rect::from_min_max(
            Pos2::new(rect.left() + 36.0, rect.top()),
            Pos2::new(label_right, rect.bottom()),
        ),
        Pos2::new(rect.left() + 36.0, rect.center().y),
        label,
        theme::font(config, 12.3),
        color,
        Align2::LEFT_CENTER,
    );
    if let Some(shortcut) = shortcut {
        draw_text(
            ui,
            Pos2::new(rect.right() - 10.0, rect.center().y),
            shortcut,
            theme::font(config, 11.3),
            theme::muted(config),
            Align2::RIGHT_CENTER,
        );
    }
    if submenu {
        draw_submenu_chevron(
            ui.painter(),
            Rect::from_center_size(
                Pos2::new(rect.right() - 12.0, rect.center().y),
                Vec2::splat(12.0),
            ),
            theme::muted(config),
        );
    }
    response
}

pub(super) fn context_menu_row_width(ui: &egui::Ui) -> f32 {
    let available = ui.available_width();
    if available.is_finite() && available >= CONTEXT_MENU_MIN_WIDTH {
        available.min(CONTEXT_SUBMENU_MAX_WIDTH)
    } else {
        CONTEXT_MENU_MIN_WIDTH
    }
}

pub(super) fn apply_context_menu_width(ui: &mut egui::Ui, width: f32) {
    ui.set_min_width(width);
    ui.set_max_width(width);
}

fn context_menu_width_for_entry(
    ui: &egui::Ui,
    config: &AppConfig,
    entry: &FileEntry,
    extract_menu: bool,
    show_open_location: bool,
) -> f32 {
    let mut rows = vec![
        MenuRowMeasure {
            label: i18n::tr(config, "open"),
            shortcut: None,
            submenu: false,
        },
        MenuRowMeasure {
            label: i18n::tr(config, "open_with"),
            shortcut: None,
            submenu: false,
        },
    ];
    #[cfg(target_os = "windows")]
    if can_scan_with_windows_defender(entry) {
        rows.push(MenuRowMeasure {
            label: i18n::tr(config, "scan_with_windows_defender"),
            shortcut: None,
            submenu: false,
        });
    }
    if show_open_location {
        rows.push(MenuRowMeasure {
            label: i18n::tr(config, "open_file_location"),
            shortcut: None,
            submenu: false,
        });
    }
    if is_ejectable_drive(entry) {
        rows.push(MenuRowMeasure {
            label: i18n::tr(config, "eject"),
            shortcut: None,
            submenu: false,
        });
    }
    if matches!(entry.kind, EntryKind::File | EntryKind::Folder) {
        rows.push(MenuRowMeasure {
            label: i18n::tr(config, "compress"),
            shortcut: None,
            submenu: true,
        });
        if extract_menu {
            rows.push(MenuRowMeasure {
                label: i18n::tr(config, "extract"),
                shortcut: None,
                submenu: true,
            });
        }
    }
    rows.extend([
        MenuRowMeasure {
            label: i18n::tr(config, "rename"),
            shortcut: None,
            submenu: false,
        },
        MenuRowMeasure {
            label: i18n::tr(config, "delete"),
            shortcut: None,
            submenu: false,
        },
        MenuRowMeasure {
            label: i18n::tr(config, "delete_permanently"),
            shortcut: None,
            submenu: false,
        },
        MenuRowMeasure {
            label: i18n::tr(config, "properties"),
            shortcut: Some("Alt+Enter"),
            submenu: false,
        },
    ]);

    context_menu_width(
        ui,
        config,
        &rows,
        CONTEXT_MENU_MIN_WIDTH,
        CONTEXT_MENU_MAX_WIDTH,
    )
}

fn context_menu_width_for_background(ui: &egui::Ui, config: &AppConfig) -> f32 {
    let rows = [
        MenuRowMeasure {
            label: i18n::tr(config, "refresh"),
            shortcut: None,
            submenu: false,
        },
        MenuRowMeasure {
            label: i18n::tr(config, "new"),
            shortcut: None,
            submenu: true,
        },
        MenuRowMeasure {
            label: i18n::tr(config, "open_terminal"),
            shortcut: None,
            submenu: false,
        },
        MenuRowMeasure {
            label: i18n::tr(config, "properties"),
            shortcut: Some("Alt+Enter"),
            submenu: false,
        },
    ];
    context_menu_width(
        ui,
        config,
        &rows,
        CONTEXT_MENU_MIN_WIDTH,
        CONTEXT_MENU_MAX_WIDTH,
    )
}

fn context_submenu_width(ui: &egui::Ui, config: &AppConfig, items: &[SubmenuItem]) -> f32 {
    let rows = items
        .iter()
        .map(|item| MenuRowMeasure {
            label: item.label.as_str(),
            shortcut: None,
            submenu: false,
        })
        .collect::<Vec<_>>();
    context_menu_width(
        ui,
        config,
        &rows,
        CONTEXT_MENU_MIN_WIDTH,
        CONTEXT_SUBMENU_MAX_WIDTH,
    )
}

pub(super) fn context_menu_width(
    ui: &egui::Ui,
    config: &AppConfig,
    rows: &[MenuRowMeasure<'_>],
    min_width: f32,
    max_width: f32,
) -> f32 {
    let label_font = theme::font(config, 12.3);
    let shortcut_font = theme::font(config, 11.3);
    let color = theme::text(config);
    let shortcut_color = theme::muted(config);
    let mut width = CONTEXT_MENU_MIN_WIDTH;

    for row in rows {
        let label_width = menu_text_width(ui, row.label, label_font.clone(), color);
        let right_padding = if let Some(shortcut) = row.shortcut {
            menu_text_width(ui, shortcut, shortcut_font.clone(), shortcut_color) + 34.0
        } else if row.submenu {
            30.0
        } else {
            14.0
        };
        width = width.max(36.0 + label_width + right_padding);
    }

    width.ceil().clamp(min_width, max_width)
}

pub(super) fn menu_text_width(ui: &egui::Ui, text: &str, font: FontId, color: Color32) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_string(), font, color)
        .size()
        .x
}

fn frosted_menu_fill(config: &AppConfig) -> Color32 {
    theme::popup_surface(config)
}

fn draw_menu_icon(painter: &egui::Painter, rect: Rect, icon: MenuIcon, color: Color32) {
    match icon {
        MenuIcon::Open => {
            let body = rect.shrink2(egui::vec2(3.0, 4.0));
            painter.rect_stroke(body, 1.5, Stroke::new(1.15, color));
            painter.line_segment(
                [
                    Pos2::new(body.left() + 4.0, body.center().y),
                    Pos2::new(body.right() - 3.0, body.center().y),
                ],
                Stroke::new(1.2, color),
            );
            painter.line_segment(
                [
                    Pos2::new(body.right() - 6.0, body.center().y - 3.0),
                    Pos2::new(body.right() - 3.0, body.center().y),
                ],
                Stroke::new(1.2, color),
            );
            painter.line_segment(
                [
                    Pos2::new(body.right() - 3.0, body.center().y),
                    Pos2::new(body.right() - 6.0, body.center().y + 3.0),
                ],
                Stroke::new(1.2, color),
            );
        }
        MenuIcon::OpenWith => {
            for (x, y) in [(3.0, 3.0), (9.0, 3.0), (3.0, 9.0), (9.0, 9.0)] {
                painter.rect_stroke(
                    Rect::from_min_size(rect.min + egui::vec2(x, y), egui::vec2(4.0, 4.0)),
                    1.0,
                    Stroke::new(1.1, color),
                );
            }
        }
        MenuIcon::Defender => {
            let top = Pos2::new(rect.center().x, rect.top() + 2.0);
            let left_shoulder = Pos2::new(rect.left() + 3.0, rect.top() + 4.5);
            let right_shoulder = Pos2::new(rect.right() - 3.0, rect.top() + 4.5);
            let left_mid = Pos2::new(rect.left() + 4.0, rect.center().y + 1.0);
            let right_mid = Pos2::new(rect.right() - 4.0, rect.center().y + 1.0);
            let bottom = Pos2::new(rect.center().x, rect.bottom() - 2.0);
            let shield_points = vec![
                top,
                right_shoulder,
                right_mid,
                Pos2::new(rect.center().x + 2.0, rect.bottom() - 4.0),
                bottom,
                Pos2::new(rect.center().x - 2.0, rect.bottom() - 4.0),
                left_mid,
                left_shoulder,
            ];
            painter.add(egui::Shape::convex_polygon(
                shield_points.clone(),
                color.gamma_multiply(0.12),
                Stroke::NONE,
            ));
            painter.add(egui::Shape::closed_line(
                shield_points,
                Stroke::new(1.35, color),
            ));
            painter.line_segment(
                [top, Pos2::new(rect.center().x, rect.bottom() - 4.0)],
                Stroke::new(1.05, color.gamma_multiply(0.85)),
            );
            painter.line_segment(
                [
                    Pos2::new(rect.left() + 4.6, rect.center().y),
                    Pos2::new(rect.right() - 4.6, rect.center().y),
                ],
                Stroke::new(1.05, color.gamma_multiply(0.85)),
            );
        }
        MenuIcon::Copy => {
            painter.rect_stroke(
                rect.translate(egui::vec2(2.0, -2.0)).shrink(3.0),
                1.5,
                Stroke::new(1.2, color),
            );
            painter.rect_stroke(
                rect.translate(egui::vec2(-2.0, 2.0)).shrink(3.0),
                1.5,
                Stroke::new(1.2, color),
            );
        }
        MenuIcon::Cut => {
            painter.circle_stroke(
                Pos2::new(rect.left() + 5.0, rect.bottom() - 4.0),
                2.2,
                Stroke::new(1.1, color),
            );
            painter.circle_stroke(
                Pos2::new(rect.right() - 5.0, rect.bottom() - 4.0),
                2.2,
                Stroke::new(1.1, color),
            );
            painter.line_segment(
                [
                    Pos2::new(rect.left() + 6.0, rect.top() + 4.0),
                    Pos2::new(rect.right() - 6.0, rect.bottom() - 6.0),
                ],
                Stroke::new(1.1, color),
            );
            painter.line_segment(
                [
                    Pos2::new(rect.right() - 6.0, rect.top() + 4.0),
                    Pos2::new(rect.left() + 6.0, rect.bottom() - 6.0),
                ],
                Stroke::new(1.1, color),
            );
        }
        MenuIcon::Paste => {
            let board = Rect::from_min_max(
                rect.left_top() + egui::vec2(3.0, 4.0),
                rect.right_bottom() - egui::vec2(3.0, 2.0),
            );
            painter.rect_stroke(board, 1.5, Stroke::new(1.2, color));
            painter.line_segment(
                [
                    Pos2::new(rect.left() + 6.0, rect.top() + 3.0),
                    Pos2::new(rect.right() - 6.0, rect.top() + 3.0),
                ],
                Stroke::new(1.2, color),
            );
        }
        MenuIcon::Eject => {
            let triangle = vec![
                Pos2::new(rect.center().x, rect.top() + 4.0),
                Pos2::new(rect.left() + 4.0, rect.center().y + 2.0),
                Pos2::new(rect.right() - 4.0, rect.center().y + 2.0),
            ];
            painter.add(egui::Shape::closed_line(triangle, Stroke::new(1.2, color)));
            painter.line_segment(
                [
                    Pos2::new(rect.left() + 4.0, rect.bottom() - 4.0),
                    Pos2::new(rect.right() - 4.0, rect.bottom() - 4.0),
                ],
                Stroke::new(1.3, color),
            );
        }
        MenuIcon::New => {
            draw_new_symbol(painter, rect, color);
        }
        MenuIcon::Refresh => {
            let center = rect.center();
            let radius = 5.2;
            let mut points = Vec::with_capacity(24);
            for step in 0..=22 {
                let angle = (40.0 + step as f32 * 11.7).to_radians();
                points.push(Pos2::new(
                    center.x + angle.cos() * radius,
                    center.y + angle.sin() * radius,
                ));
            }
            painter.add(egui::Shape::line(points, Stroke::new(1.25, color)));

            let tip_angle = 297.0_f32.to_radians();
            let tip = Pos2::new(
                center.x + tip_angle.cos() * radius,
                center.y + tip_angle.sin() * radius,
            );
            painter.line_segment(
                [tip, Pos2::new(tip.x + 3.1, tip.y - 0.4)],
                Stroke::new(1.25, color),
            );
            painter.line_segment(
                [tip, Pos2::new(tip.x + 0.9, tip.y + 3.0)],
                Stroke::new(1.25, color),
            );
        }
        MenuIcon::Folder => {
            let tab = Rect::from_min_max(
                rect.left_top() + egui::vec2(2.0, 4.0),
                Pos2::new(rect.left() + 8.0, rect.top() + 7.0),
            );
            let body = Rect::from_min_max(
                rect.left_top() + egui::vec2(2.0, 7.0),
                rect.right_bottom() - egui::vec2(2.0, 3.0),
            );
            painter.rect_filled(tab, 1.5, color);
            painter.rect_stroke(body, 1.5, Stroke::new(1.2, color));
        }
        MenuIcon::TextDocument => {
            let body = rect.shrink2(egui::vec2(4.0, 2.0));
            painter.rect_stroke(body, 1.5, Stroke::new(1.2, color));
            painter.line_segment(
                [
                    Pos2::new(body.left() + 3.0, body.top() + 5.0),
                    Pos2::new(body.right() - 3.0, body.top() + 5.0),
                ],
                Stroke::new(1.0, color),
            );
            painter.line_segment(
                [
                    Pos2::new(body.left() + 3.0, body.top() + 9.0),
                    Pos2::new(body.right() - 5.0, body.top() + 9.0),
                ],
                Stroke::new(1.0, color),
            );
        }
        MenuIcon::Rename => {
            painter.line_segment(
                [
                    Pos2::new(rect.left() + 4.0, rect.bottom() - 4.0),
                    Pos2::new(rect.right() - 4.0, rect.top() + 4.0),
                ],
                Stroke::new(1.25, color),
            );
            painter.line_segment(
                [
                    Pos2::new(rect.right() - 6.0, rect.top() + 4.0),
                    Pos2::new(rect.right() - 3.0, rect.top() + 7.0),
                ],
                Stroke::new(1.25, color),
            );
            painter.line_segment(
                [
                    Pos2::new(rect.left() + 3.0, rect.bottom() - 3.0),
                    Pos2::new(rect.left() + 7.0, rect.bottom() - 2.0),
                ],
                Stroke::new(1.15, color),
            );
        }
        MenuIcon::Delete => {
            let body = Rect::from_min_max(
                rect.left_top() + egui::vec2(5.0, 6.0),
                rect.right_bottom() - egui::vec2(5.0, 3.0),
            );
            painter.rect_stroke(body, 1.4, Stroke::new(1.1, color));
            painter.line_segment(
                [
                    Pos2::new(body.left() - 1.0, body.top() - 2.0),
                    Pos2::new(body.right() + 1.0, body.top() - 2.0),
                ],
                Stroke::new(1.1, color),
            );
            painter.line_segment(
                [
                    Pos2::new(rect.center().x - 2.5, body.top() - 4.0),
                    Pos2::new(rect.center().x + 2.5, body.top() - 4.0),
                ],
                Stroke::new(1.1, color),
            );
        }
        MenuIcon::Properties => {
            painter.circle_stroke(rect.center(), 5.5, Stroke::new(1.2, color));
            painter.circle_filled(rect.center(), 1.1, color);
            painter.line_segment(
                [
                    Pos2::new(rect.center().x, rect.top() + 4.0),
                    Pos2::new(rect.center().x, rect.top() + 6.0),
                ],
                Stroke::new(1.2, color),
            );
        }
        MenuIcon::Terminal => {
            let body = rect.shrink2(egui::vec2(2.0, 4.0));
            painter.rect_stroke(body, 1.5, Stroke::new(1.1, color));
            painter.line_segment(
                [
                    Pos2::new(body.left() + 3.0, body.center().y - 2.5),
                    Pos2::new(body.left() + 6.0, body.center().y),
                ],
                Stroke::new(1.2, color),
            );
            painter.line_segment(
                [
                    Pos2::new(body.left() + 6.0, body.center().y),
                    Pos2::new(body.left() + 3.0, body.center().y + 2.5),
                ],
                Stroke::new(1.2, color),
            );
            painter.line_segment(
                [
                    Pos2::new(body.left() + 8.0, body.center().y + 3.0),
                    Pos2::new(body.right() - 3.0, body.center().y + 3.0),
                ],
                Stroke::new(1.1, color),
            );
        }
    }
}

fn draw_submenu_chevron(painter: &egui::Painter, rect: Rect, color: Color32) {
    painter.line_segment(
        [
            Pos2::new(rect.left() + 4.0, rect.top() + 3.0),
            rect.center(),
        ],
        Stroke::new(1.3, color),
    );
    painter.line_segment(
        [
            rect.center(),
            Pos2::new(rect.left() + 4.0, rect.bottom() - 3.0),
        ],
        Stroke::new(1.3, color),
    );
}

pub(crate) fn run_action(app: &mut BExplorerApp, action: TableAction) {
    match action {
        TableAction::Open(entry) => app.open_entry(&entry),
        TableAction::OpenWith(entry) => app.open_with(&entry.path),
        TableAction::ScanWithWindowsDefender(entry) => app.scan_with_windows_defender(entry.path),
        TableAction::OpenLocation(entry) => app.open_location_for(&entry.path),
        TableAction::Select(entry, additive, range) => {
            app.select_entry(entry.path, additive, range)
        }
        TableAction::Copy(entry) => {
            app.ensure_selected(entry.path);
            app.copy_selection(false);
        }
        TableAction::Cut(entry) => {
            app.ensure_selected(entry.path);
            app.copy_selection(true);
        }
        TableAction::PasteInto(entry) => app.paste_into(entry.path),
        TableAction::Eject(entry) => app.eject_drive(entry.path),
        TableAction::Rename(entry) => app.begin_rename_entry(entry),
        TableAction::Delete(entry, permanent) => {
            app.ensure_selected(entry.path);
            app.request_delete_selected(permanent);
        }
        TableAction::Compress(entry) => {
            app.ensure_selected(entry.path);
            app.open_compress_dialog();
        }
        TableAction::CompressAs(entry, format) => {
            app.ensure_selected(entry.path);
            app.compress_selected_as(format);
        }
        TableAction::Extract(entry, mode) => app.extract_archive(entry.path, mode),
        TableAction::Properties(entry) => app.show_properties(&entry.path),
        TableAction::PropertiesCurrent => app.show_selected_or_current_properties(),
        TableAction::OpenTerminalHere => {
            if let Some(path) = app.active_path() {
                app.open_terminal_at(&path);
            }
        }
        TableAction::CopySelected => app.copy_selection(false),
        TableAction::CutSelected => app.copy_selection(true),
        TableAction::PasteHere => app.paste_into_active(),
        TableAction::Refresh => app.refresh_active_tab(),
        TableAction::CreateFolder => app.create_folder(),
        TableAction::CreateTextDocument => app.create_text_document(),
    }
}

pub(crate) fn is_ejectable_drive(entry: &FileEntry) -> bool {
    entry
        .drive_kind
        .is_some_and(crate::fs::explorer::DriveKind::is_ejectable)
}

#[cfg(target_os = "windows")]
fn can_scan_with_windows_defender(entry: &FileEntry) -> bool {
    matches!(
        entry.kind,
        EntryKind::File | EntryKind::Folder | EntryKind::Drive
    ) && !crate::fs::explorer::is_virtual_path(&entry.path)
        && !crate::fs::archive_listing::is_inside_archive(&entry.path)
}

fn draw_new_symbol(painter: &egui::Painter, rect: Rect, color: Color32) {
    draw_new_plus(painter, rect.center(), color);
}

fn draw_new_plus(painter: &egui::Painter, center: Pos2, color: Color32) {
    painter.line_segment(
        [
            Pos2::new(center.x - 5.0, center.y),
            Pos2::new(center.x + 5.0, center.y),
        ],
        Stroke::new(1.4, color),
    );
    painter.line_segment(
        [
            Pos2::new(center.x, center.y - 5.0),
            Pos2::new(center.x, center.y + 5.0),
        ],
        Stroke::new(1.4, color),
    );
}

pub(crate) fn show_main_new_submenu(
    app: &mut BExplorerApp,
    ctx: &egui::Context,
    parent_response: &egui::Response,
    pane_id: usize,
) {
    let submenu_rect_id = Id::new(("main_menu_new_submenu_rect", pane_id, parent_response.id));
    let submenu_area_id = Id::new(("main_menu_new_submenu", pane_id, parent_response.id));
    if parent_response.clicked() {
        app.action_bar_new_menu_open = !app.action_bar_new_menu_open;
        if app.action_bar_new_menu_open {
            app.options_menu_open = false;
            if let Some(other) = app.other_pane.as_mut() {
                other.action_bar_new_menu_open = false;
            }
        }
    }

    if !app.action_bar_new_menu_open {
        ctx.data_mut(|data| data.insert_temp(submenu_rect_id, Rect::NOTHING));
        return;
    }

    if should_close_action_new_submenu(ctx, parent_response.rect, submenu_rect_id) {
        app.action_bar_new_menu_open = false;
        ctx.data_mut(|data| data.insert_temp(submenu_rect_id, Rect::NOTHING));
        return;
    }

    let config = app.config.clone();
    let popup_pos = Pos2::new(
        parent_response.rect.left(),
        parent_response.rect.bottom() + 5.0,
    );
    let mut submenu_rect = Rect::NOTHING;
    let mut action = None;

    egui::Area::new(submenu_area_id)
        .order(Order::Foreground)
        .fixed_pos(popup_pos)
        .show(ctx, |ui| {
            egui::Frame::none()
                .fill(theme::popup_surface(&config))
                .stroke(Stroke::new(1.0, theme::popup_stroke(&config)))
                .rounding(egui::Rounding::same(6.0))
                .inner_margin(egui::Margin::same(6.0))
                .show(ui, |ui| {
                    let rows = [
                        MenuRowMeasure {
                            label: i18n::tr(&config, "new_folder"),
                            shortcut: None,
                            submenu: false,
                        },
                        MenuRowMeasure {
                            label: i18n::tr(&config, "text_document"),
                            shortcut: None,
                            submenu: false,
                        },
                    ];
                    let menu_width = context_menu_width(
                        ui,
                        &config,
                        &rows,
                        CONTEXT_MENU_MIN_WIDTH,
                        CONTEXT_MENU_MAX_WIDTH,
                    );
                    apply_context_menu_width(ui, menu_width);
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 3.0);

                    if context_menu_row(
                        ui,
                        &config,
                        MenuIcon::Folder,
                        i18n::tr(&config, "new_folder"),
                        None,
                        true,
                        false,
                    )
                    .clicked()
                    {
                        action = Some(0);
                    }
                    if context_menu_row(
                        ui,
                        &config,
                        MenuIcon::TextDocument,
                        i18n::tr(&config, "text_document"),
                        None,
                        true,
                        false,
                    )
                    .clicked()
                    {
                        action = Some(1);
                    }

                    submenu_rect = ui.min_rect();
                });
        });

    ctx.data_mut(|data| data.insert_temp(submenu_rect_id, submenu_rect));

    match action {
        Some(0) => {
            app.create_folder();
            app.action_bar_new_menu_open = false;
        }
        Some(1) => {
            app.create_text_document();
            app.action_bar_new_menu_open = false;
        }
        _ => {}
    }
}

fn should_close_action_new_submenu(
    ctx: &egui::Context,
    parent_rect: Rect,
    submenu_rect_id: Id,
) -> bool {
    if !ctx.input(|input| input.pointer.any_click()) {
        return false;
    }
    let Some(pointer) = ctx.input(|input| input.pointer.interact_pos()) else {
        return false;
    };
    if parent_rect.expand(3.0).contains(pointer) {
        return false;
    }
    ctx.data(|data| data.get_temp::<Rect>(submenu_rect_id))
        .filter(|rect| *rect != Rect::NOTHING && rect.is_finite())
        .is_none_or(|rect| !rect.expand(6.0).contains(pointer))
}
