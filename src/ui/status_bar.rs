use eframe::egui::{self, Align2, Pos2, Rect, Sense, Stroke, Vec2};

use crate::app::config::ViewMode;
use crate::app::state::{BExplorerApp, SearchMode};
use crate::fs::explorer;
use crate::ui::i18n;
use crate::ui::theme;

const STATUS_HEIGHT: f32 = 38.0;

pub fn show(app: &mut BExplorerApp, ctx: &egui::Context) {
    egui::TopBottomPanel::bottom("status_bar")
        .exact_height(STATUS_HEIGHT)
        .frame(egui::Frame::none().fill(theme::canvas(&app.config)))
        .show(ctx, |ui| {
            paint_in_rect(app, ui, ui.max_rect());
        });
}

/// Paint the status bar content into a given rect (no panel wrapper).
/// Used per-panel in split mode.
pub fn paint_in_rect(app: &mut BExplorerApp, ui: &mut egui::Ui, rect: Rect) {
    theme::paint_status_gradient(ui.painter(), rect, &app.config);
    paint_activity_progress(app, ui, rect);
    ui.painter().line_segment(
        [rect.left_top(), rect.right_top()],
        Stroke::new(1.0, theme::stroke(&app.config)),
    );

    let filter_right = paint_bottom_filter(app, ui, rect);
    paint_right_status(app, ui, rect);
    paint_left_metrics(app, ui, rect, filter_right + 12.0);
}

fn paint_activity_progress(app: &BExplorerApp, ui: &mut egui::Ui, rect: Rect) {
    let network_loading = app.loading
        && app
            .active_path()
            .as_deref()
            .is_some_and(explorer::is_network_root_path);
    let operation_active = app.operation_active();
    let defender_active = app.defender_active();
    if !app.searching
        && !network_loading
        && !app.transfer_active()
        && !operation_active
        && !defender_active
    {
        return;
    }

    let track = Rect::from_min_max(
        Pos2::new(rect.left(), rect.top()),
        Pos2::new(rect.right(), rect.top() + 2.0),
    );
    ui.painter()
        .rect_filled(track, 0.0, theme::subtle_stroke(&app.config));

    if app.searching || network_loading || operation_active || defender_active {
        let time = ui.ctx().input(|input| input.time) as f32;
        let width = (track.width() * 0.28).clamp(96.0, 360.0);
        let travel = (track.width() + width).max(1.0);
        let left = track.left() - width + (time * 260.0).rem_euclid(travel);
        let fill = Rect::from_min_max(
            Pos2::new(left, track.top()),
            Pos2::new((left + width).min(track.right()), track.bottom()),
        )
        .intersect(track);
        theme::paint_selection_gradient(ui.painter(), fill, &app.config);
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(16));
    } else if let Some(progress) = app.transfer_progress_fraction() {
        let fill = Rect::from_min_max(
            track.left_top(),
            Pos2::new(
                track.left() + track.width() * progress.clamp(0.0, 1.0),
                track.bottom(),
            ),
        );
        theme::paint_selection_gradient(ui.painter(), fill, &app.config);
    } else {
        theme::paint_selection_gradient(ui.painter(), track, &app.config);
    }
}

fn paint_bottom_filter(app: &mut BExplorerApp, ui: &mut egui::Ui, rect: Rect) -> f32 {
    let is_split = app.split.is_some();
    let filter_width = if is_split {
        (rect.width() * 0.34)
            .clamp(136.0, 190.0)
            .min((rect.width() - 24.0).max(96.0))
    } else {
        260.0_f32.min((rect.width() - 24.0).max(96.0))
    };
    let filter_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 12.0, rect.top() + 7.0),
        Vec2::new(filter_width, 25.0),
    );
    ui.painter()
        .rect_filled(filter_rect, 8.0, theme::surface_elevated(&app.config));
    ui.painter().rect_stroke(
        filter_rect,
        8.0,
        Stroke::new(1.0, theme::stroke(&app.config)),
    );

    let icon_center = Pos2::new(filter_rect.left() + 16.0, filter_rect.center().y);
    ui.painter().circle_stroke(
        icon_center,
        4.0,
        Stroke::new(1.2, theme::muted(&app.config)),
    );
    ui.painter().line_segment(
        [
            icon_center + egui::vec2(3.0, 3.0),
            icon_center + egui::vec2(7.0, 7.0),
        ],
        Stroke::new(1.2, theme::muted(&app.config)),
    );

    let edit_rect = Rect::from_min_max(
        Pos2::new(filter_rect.left() + 30.0, filter_rect.top() + 3.0),
        Pos2::new(filter_rect.right() - 33.0, filter_rect.bottom() - 3.0),
    );
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(edit_rect), |ui| {
        ui.visuals_mut().override_text_color = Some(theme::text(&app.config));
        let edit = egui::TextEdit::singleline(&mut app.filter)
            .hint_text(format!(
                "{} {}...",
                i18n::tr(&app.config, "filter").trim_end_matches("..."),
                app.entries.len()
            ))
            .frame(false)
            .desired_width(edit_rect.width());
        let response = ui.add(edit);
        if response.has_focus() {
            app.mark_text_input_active();
        }
        if response.changed() {
            app.on_filter_changed();
        }
    });

    let mode_rect = Rect::from_center_size(
        Pos2::new(filter_rect.right() - 16.0, filter_rect.center().y),
        Vec2::splat(18.0),
    );
    let mode_response = ui.allocate_rect(mode_rect, Sense::click());
    if mode_response.hovered() {
        theme::paint_hover_gradient(ui.painter(), mode_rect, 4.0, &app.config);
    }
    paint_search_mode_icon(ui, &app.config, mode_rect, app.search_mode);
    let popup_id = ui.make_persistent_id("filter_search_mode_menu");
    if mode_response.clicked() {
        ui.memory_mut(|memory| memory.toggle_popup(popup_id));
    }
    show_search_mode_popup(app, ui, popup_id, &mode_response);
    filter_rect.right()
}

fn show_search_mode_popup(
    app: &mut BExplorerApp,
    ui: &mut egui::Ui,
    popup_id: egui::Id,
    response: &egui::Response,
) {
    egui::popup::popup_above_or_below_widget(
        ui,
        popup_id,
        response,
        egui::AboveOrBelow::Above,
        egui::popup::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(190.0);
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 3.0);
            if paint_search_mode_row(ui, app, SearchMode::Quick).clicked() {
                app.set_search_mode(SearchMode::Quick);
                ui.memory_mut(|memory| memory.close_popup());
            }
            if paint_search_mode_row(ui, app, SearchMode::Complete).clicked() {
                app.set_search_mode(SearchMode::Complete);
                ui.memory_mut(|memory| memory.close_popup());
            }
        },
    );
}

fn paint_search_mode_row(
    ui: &mut egui::Ui,
    app: &BExplorerApp,
    mode: SearchMode,
) -> egui::Response {
    let selected = app.search_mode == mode;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(190.0, 28.0), Sense::click());
    if selected {
        theme::paint_sidebar_row_gradient(ui.painter(), rect, 4.0, &app.config);
    } else if response.hovered() {
        theme::paint_hover_gradient(ui.painter(), rect, 4.0, &app.config);
    }

    paint_search_mode_icon(
        ui,
        &app.config,
        Rect::from_center_size(
            Pos2::new(rect.left() + 17.0, rect.center().y),
            Vec2::splat(16.0),
        ),
        mode,
    );
    ui.painter().text(
        Pos2::new(rect.left() + 36.0, rect.center().y),
        Align2::LEFT_CENTER,
        search_mode_label(&app.config, mode),
        theme::font(&app.config, 12.1),
        if selected {
            theme::text(&app.config)
        } else {
            theme::muted(&app.config)
        },
    );
    response
}

fn search_mode_label(config: &crate::app::config::AppConfig, mode: SearchMode) -> &'static str {
    match mode {
        SearchMode::Quick => i18n::tr(config, "quick_search"),
        SearchMode::Complete => i18n::tr(config, "full_search"),
    }
}

fn paint_search_mode_icon(
    ui: &mut egui::Ui,
    config: &crate::app::config::AppConfig,
    rect: Rect,
    mode: SearchMode,
) {
    match mode {
        SearchMode::Quick => paint_folder_icon(ui, config, rect),
        SearchMode::Complete => {
            let small = Vec2::new(8.0, 6.0);
            let centers = [
                Pos2::new(rect.center().x, rect.top() + 5.0),
                Pos2::new(rect.left() + 5.0, rect.bottom() - 4.0),
                Pos2::new(rect.right() - 5.0, rect.bottom() - 4.0),
            ];
            for center in centers {
                paint_folder_icon(ui, config, Rect::from_center_size(center, small));
            }
        }
    }
}

fn paint_folder_icon(ui: &mut egui::Ui, config: &crate::app::config::AppConfig, rect: Rect) {
    let color = theme::muted(config);
    let tab = Rect::from_min_max(
        rect.left_top() + egui::vec2(1.0, 3.0),
        Pos2::new(rect.left() + rect.width() * 0.45, rect.top() + 6.0),
    );
    let body = Rect::from_min_max(
        rect.left_top() + egui::vec2(1.0, 5.5),
        rect.right_bottom() - egui::vec2(1.0, 2.0),
    );
    ui.painter().rect_filled(tab, 1.2, color);
    ui.painter().rect_stroke(body, 1.3, Stroke::new(1.1, color));
}

fn paint_left_metrics(app: &BExplorerApp, ui: &mut egui::Ui, rect: Rect, start_x: f32) {
    if rect.width() < 360.0 && app.split.is_some() {
        return;
    }
    let text = format!(
        "{} {}    {} {}    {}: {}",
        app.filtered_entry_count(),
        i18n::tr(&app.config, "items"),
        app.selected.len(),
        i18n::tr(&app.config, "selected"),
        i18n::tr(&app.config, "selected_size"),
        crate::ui::file_table::format_bytes(app.selected_size())
    );
    let clip = Rect::from_min_max(
        Pos2::new(start_x, rect.top()),
        Pos2::new(rect.right() - 112.0, rect.bottom()),
    );
    if clip.width() < 70.0 {
        return;
    }
    ui.painter().with_clip_rect(clip).text(
        Pos2::new(start_x, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        theme::font(&app.config, 12.0),
        theme::muted(&app.config),
    );
}

fn paint_right_status(app: &mut BExplorerApp, ui: &mut egui::Ui, rect: Rect) {
    let is_split = app.split.is_some();
    let is_maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
    let show_right = if is_split {
        is_maximized || rect.width() >= 260.0
    } else {
        true
    };
    if !show_right {
        return;
    }

    let mut right = rect.right() - 10.0;
    let active_view_mode = app.active_view_mode();
    let label = i18n::view_mode_label(&app.config, &active_view_mode);
    let view_width = (label.chars().count() as f32 * 6.8 + 43.0).clamp(92.0, 150.0);
    let view_rect = Rect::from_min_size(
        Pos2::new(right - view_width, rect.top() + 8.0),
        Vec2::new(view_width, 23.0),
    );
    let response = ui.allocate_rect(view_rect, Sense::click());
    let popup_id = ui.make_persistent_id("status_view_mode_menu");
    if response.clicked() {
        ui.memory_mut(|memory| memory.toggle_popup(popup_id));
    }
    ui.painter()
        .rect_filled(view_rect, 3.0, theme::surface_elevated(&app.config));
    if response.hovered() {
        theme::paint_hover_gradient(ui.painter(), view_rect, 3.0, &app.config);
    }
    paint_view_icon(
        ui,
        &app.config,
        Rect::from_center_size(
            Pos2::new(view_rect.left() + 15.0, view_rect.center().y),
            Vec2::splat(14.0),
        ),
        active_view_mode,
    );
    let text_clip = Rect::from_min_max(
        Pos2::new(view_rect.left() + 30.0, view_rect.top()),
        Pos2::new(view_rect.right() - 8.0, view_rect.bottom()),
    );
    ui.painter().with_clip_rect(text_clip).text(
        Pos2::new(view_rect.left() + 30.0, view_rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::font(&app.config, 12.0),
        theme::text(&app.config),
    );
    show_view_popup(app, ui, popup_id, &response);
    right -= view_width + 10.0;

    if !is_split && !app.status_message.is_empty() {
        ui.painter().text(
            Pos2::new(right - 8.0, rect.center().y),
            Align2::RIGHT_CENTER,
            status_label(app),
            theme::font(&app.config, 12.0),
            theme::muted(&app.config),
        );
    }
}

fn show_view_popup(
    app: &mut BExplorerApp,
    ui: &mut egui::Ui,
    popup_id: egui::Id,
    response: &egui::Response,
) {
    let current_view_mode = app.active_view_mode();
    egui::popup::popup_above_or_below_widget(
        ui,
        popup_id,
        response,
        egui::AboveOrBelow::Above,
        egui::popup::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(176.0);
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 2.0);
            for view_mode in ViewMode::ALL {
                if paint_view_menu_row(ui, &app.config, current_view_mode, view_mode).clicked() {
                    app.set_active_view_mode(view_mode);
                    ui.memory_mut(|memory| memory.close_popup());
                }
            }
        },
    );
}

fn paint_view_menu_row(
    ui: &mut egui::Ui,
    config: &crate::app::config::AppConfig,
    current_view_mode: ViewMode,
    view_mode: ViewMode,
) -> egui::Response {
    let selected = current_view_mode == view_mode;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(176.0, 25.0), Sense::click());
    if selected {
        theme::paint_sidebar_row_gradient(ui.painter(), rect, 4.0, config);
    } else if response.hovered() {
        theme::paint_hover_gradient(ui.painter(), rect, 4.0, config);
    }
    paint_view_icon(
        ui,
        config,
        Rect::from_center_size(
            Pos2::new(rect.left() + 15.0, rect.center().y),
            Vec2::splat(14.0),
        ),
        view_mode,
    );
    let text_clip = Rect::from_min_max(
        Pos2::new(rect.left() + 31.0, rect.top()),
        Pos2::new(rect.right() - 8.0, rect.bottom()),
    );
    ui.painter().with_clip_rect(text_clip).text(
        Pos2::new(rect.left() + 31.0, rect.center().y),
        Align2::LEFT_CENTER,
        i18n::view_mode_label(config, &view_mode),
        theme::font(config, 12.0),
        if selected {
            theme::text(config)
        } else {
            theme::muted(config)
        },
    );
    response
}

fn status_label(app: &BExplorerApp) -> &str {
    if app.status_message == "Ready" {
        i18n::tr(&app.config, "ready")
    } else {
        &app.status_message
    }
}

fn paint_list_icon(ui: &mut egui::Ui, config: &crate::app::config::AppConfig, rect: Rect) {
    for row in 0..3 {
        let y = rect.top() + 3.0 + row as f32 * 4.0;
        ui.painter()
            .circle_filled(Pos2::new(rect.left() + 2.0, y), 1.0, theme::muted(config));
        ui.painter().line_segment(
            [Pos2::new(rect.left() + 6.0, y), Pos2::new(rect.right(), y)],
            Stroke::new(1.2, theme::muted(config)),
        );
    }
}

fn paint_view_icon(
    ui: &mut egui::Ui,
    config: &crate::app::config::AppConfig,
    rect: Rect,
    view_mode: ViewMode,
) {
    match view_mode {
        ViewMode::Details | ViewMode::List => paint_list_icon(ui, config, rect),
        ViewMode::SmallIcons => {
            for row in 0..2 {
                for column in 0..2 {
                    let min = Pos2::new(
                        rect.left() + column as f32 * 7.0,
                        rect.top() + row as f32 * 7.0,
                    );
                    ui.painter().rect_filled(
                        Rect::from_min_size(min, Vec2::splat(4.0)),
                        1.0,
                        theme::muted(config),
                    );
                }
            }
        }
        ViewMode::MediumIcons => {
            for row in 0..2 {
                for column in 0..2 {
                    let min = Pos2::new(
                        rect.left() + column as f32 * 7.0,
                        rect.top() + row as f32 * 7.0,
                    );
                    ui.painter().rect_filled(
                        Rect::from_min_size(min, Vec2::splat(5.0)),
                        1.0,
                        theme::muted(config),
                    );
                }
            }
        }
        ViewMode::LargeIcons => {
            for row in 0..2 {
                for column in 0..2 {
                    let min = Pos2::new(
                        rect.left() + column as f32 * 7.0,
                        rect.top() + row as f32 * 7.0,
                    );
                    ui.painter().rect_filled(
                        Rect::from_min_size(min, Vec2::splat(6.0)),
                        1.0,
                        theme::muted(config),
                    );
                }
            }
        }
        ViewMode::ExtraLargeIcons => {
            for row in 0..2 {
                for column in 0..2 {
                    let min = Pos2::new(
                        rect.left() + column as f32 * 7.0,
                        rect.top() + row as f32 * 7.0,
                    );
                    ui.painter().rect_filled(
                        Rect::from_min_size(min, Vec2::splat(7.0)),
                        1.0,
                        theme::muted(config),
                    );
                }
            }
        }
        ViewMode::Tiles => {
            for row in 0..2 {
                let tile = Rect::from_min_size(
                    Pos2::new(rect.left(), rect.top() + row as f32 * 7.0),
                    Vec2::new(14.0, 5.0),
                );
                ui.painter().rect_filled(tile, 1.5, theme::muted(config));
            }
        }
    }
}
