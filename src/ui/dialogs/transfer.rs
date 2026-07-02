use eframe::egui;

use crate::app::config::AppConfig;
use crate::app::state::BExplorerApp;
use crate::ui::i18n;
use crate::ui::theme;
use crate::ui::window_frame;

use super::{TRANSFER_TITLEBAR_HEIGHT, TRANSFER_WINDOW_BUTTON};

const TRANSFER_CARD_HEIGHT: f32 = 104.0;
const TRANSFER_CARD_GAP: f32 = 6.0;
const TRANSFER_FOOTER_HEIGHT: f32 = 40.0;

pub(super) fn show(app: &mut BExplorerApp, ctx: &egui::Context) {
    let panel_anim_id = egui::Id::new("transfer_panel_visible_anim");
    let items = app.transfer_items();
    if items.is_empty() {
        ctx.animate_bool_with_time(panel_anim_id, false, 0.18);
        app.transfer_panel_spawned = false;
        return;
    }

    let panel_t = ctx.animate_bool_with_time(panel_anim_id, true, 0.18);
    let mut pause_or_resume = Vec::new();
    let mut cancel_jobs = Vec::new();
    let mut cancel_active = false;
    let mut clear_pending = false;
    let panel_size = if app.transfer_panel_minimized {
        egui::vec2(340.0, 104.0)
    } else {
        let content_height = TRANSFER_TITLEBAR_HEIGHT
            + 24.0
            + TRANSFER_FOOTER_HEIGHT
            + items.len() as f32 * (TRANSFER_CARD_HEIGHT + TRANSFER_CARD_GAP);
        egui::vec2(462.0, content_height.clamp(178.0, 520.0))
    };
    let parent_rect = ctx
        .input(|input| input.viewport().outer_rect.or(input.viewport().inner_rect))
        .unwrap_or_else(|| ctx.screen_rect());
    let default_pos = egui::pos2(
        parent_rect.center().x - panel_size.x * 0.5,
        parent_rect.center().y - panel_size.y * 0.5 + (1.0 - panel_t) * 12.0,
    );
    let mut builder = egui::ViewportBuilder::default()
        .with_title(i18n::tr(&app.config, "transfers"))
        .with_inner_size(panel_size)
        .with_min_inner_size(egui::vec2(320.0, 96.0))
        .with_resizable(true)
        .with_decorations(false)
        .with_taskbar(true)
        .with_close_button(false)
        .with_minimize_button(false)
        .with_maximize_button(false);
    if !app.transfer_panel_spawned {
        builder = builder.with_position(default_pos);
        app.transfer_panel_spawned = true;
    }

    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("bexplorer_transfer_window"),
        builder,
        |viewport_ctx, _class| {
            theme::apply(viewport_ctx, &app.config);
            viewport_ctx.request_repaint_after(std::time::Duration::from_millis(16));
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(theme::surface_elevated(&app.config)))
                .show(viewport_ctx, |ui| {
                    let rect = ui.max_rect();
                    theme::paint_canvas_gradient(ui.painter(), rect, &app.config);
                    paint_transfer_titlebar(app, viewport_ctx, ui);

                    egui::Frame::none()
                        .inner_margin(egui::Margin::same(12.0))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());

                            if app.transfer_panel_minimized {
                                paint_transfer_minimized(app, ui);
                                return;
                            }

                            let footer_height = TRANSFER_FOOTER_HEIGHT;
                            let scroll_height = (ui.available_height() - footer_height).max(92.0);
                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(), scroll_height),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    egui::ScrollArea::vertical()
                                        .auto_shrink([false, false])
                                        .show(ui, |ui| {
                                            for item in &items {
                                                let fraction = if item.total_bytes == 0 {
                                                    0.0
                                                } else {
                                                    (item.copied_bytes as f32
                                                        / item.total_bytes as f32)
                                                        .clamp(0.0, 1.0)
                                                };
                                                let active = matches!(
                                                    item.state,
                                                    crate::fs::transfer_queue::TransferState::Copying
                                                        | crate::fs::transfer_queue::TransferState::Paused
                                                );
                                                paint_transfer_card(
                                                    app,
                                                    ui,
                                                    item,
                                                    fraction,
                                                    active,
                                                    &mut pause_or_resume,
                                                    &mut cancel_jobs,
                                                );
                                                ui.add_space(TRANSFER_CARD_GAP);
                                            }
                                        });
                                },
                            );
                            paint_transfer_footer(
                                app,
                                ui,
                                &mut cancel_active,
                                &mut clear_pending,
                            );
                        });
                });
            window_frame::show_resize_handles(viewport_ctx);
        },
    );

    if cancel_active {
        app.cancel_all_active_transfers();
    }
    if clear_pending {
        app.clear_pending_transfers();
    }
    for job_id in pause_or_resume {
        app.toggle_transfer_pause(job_id);
    }
    for job_id in cancel_jobs {
        app.cancel_transfer(job_id);
    }
}

fn paint_transfer_titlebar(app: &mut BExplorerApp, ctx: &egui::Context, ui: &mut egui::Ui) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width, TRANSFER_TITLEBAR_HEIGHT),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(rect, 0.0, theme::titlebar(&app.config));
    theme::paint_titlebar_gradient(ui.painter(), rect, &app.config);
    ui.painter().line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        egui::Stroke::new(1.0, theme::stroke(&app.config)),
    );

    let compact_rect = egui::Rect::from_min_size(
        egui::pos2(rect.right() - TRANSFER_WINDOW_BUTTON * 2.0, rect.top()),
        egui::vec2(TRANSFER_WINDOW_BUTTON, rect.height()),
    );
    let minimize_rect = egui::Rect::from_min_size(
        egui::pos2(rect.right() - TRANSFER_WINDOW_BUTTON, rect.top()),
        egui::vec2(TRANSFER_WINDOW_BUTTON, rect.height()),
    );
    let drag_rect = egui::Rect::from_min_max(
        rect.left_top(),
        egui::pos2(compact_rect.left(), rect.bottom()),
    );
    let drag_response = ui.allocate_rect(drag_rect, egui::Sense::click_and_drag());
    if drag_response.hovered()
        && ui.input(|input| input.pointer.primary_pressed())
        && !window_frame::pointer_in_resize_edge(ctx)
    {
        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }

    ui.painter().text(
        egui::pos2(rect.left() + 12.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        i18n::tr(&app.config, "transfers"),
        theme::font(&app.config, 12.5),
        theme::text(&app.config),
    );

    let compact_response = ui
        .allocate_rect(compact_rect, egui::Sense::click())
        .on_hover_text(if app.transfer_panel_minimized {
            i18n::tr(&app.config, "expand")
        } else {
            i18n::tr(&app.config, "collapse")
        });
    paint_transfer_compact_button(
        ui,
        &app.config,
        compact_rect,
        app.transfer_panel_minimized,
        compact_response.hovered(),
    );
    if compact_response.clicked() {
        app.transfer_panel_minimized = !app.transfer_panel_minimized;
    }

    let minimize_response = ui
        .allocate_rect(minimize_rect, egui::Sense::click())
        .on_hover_text(i18n::tr(&app.config, "minimize"));
    paint_transfer_titlebar_button(
        ui,
        &app.config,
        minimize_rect,
        "_",
        minimize_response.hovered(),
    );
    if minimize_response.clicked() {
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
    }
}

pub(super) fn paint_transfer_compact_button(
    ui: &mut egui::Ui,
    config: &AppConfig,
    rect: egui::Rect,
    panel_minimized: bool,
    hovered: bool,
) {
    if hovered {
        theme::paint_hover_gradient(ui.painter(), rect, 0.0, config);
    }
    let color = theme::text(config);
    let center = rect.center();
    let y_offset = if panel_minimized { -2.0 } else { 2.0 };
    let mid = egui::pos2(center.x, center.y + y_offset);
    let left = egui::pos2(center.x - 5.0, center.y - y_offset);
    let right = egui::pos2(center.x + 5.0, center.y - y_offset);
    ui.painter()
        .line_segment([left, mid], egui::Stroke::new(1.45, color));
    ui.painter()
        .line_segment([mid, right], egui::Stroke::new(1.45, color));
}

pub(super) fn paint_transfer_titlebar_button(
    ui: &mut egui::Ui,
    config: &AppConfig,
    rect: egui::Rect,
    label: &str,
    hovered: bool,
) {
    if hovered {
        theme::paint_hover_gradient(ui.painter(), rect, 0.0, config);
    }
    let color = theme::text(config);
    let center = rect.center();
    let stroke = egui::Stroke::new(1.35, color);
    match label {
        "_" => {
            ui.painter().line_segment(
                [
                    egui::pos2(center.x - 5.5, center.y),
                    egui::pos2(center.x + 5.5, center.y),
                ],
                stroke,
            );
        }
        "x" | "X" => {
            ui.painter().line_segment(
                [
                    egui::pos2(center.x - 4.5, center.y - 4.5),
                    egui::pos2(center.x + 4.5, center.y + 4.5),
                ],
                stroke,
            );
            ui.painter().line_segment(
                [
                    egui::pos2(center.x + 4.5, center.y - 4.5),
                    egui::pos2(center.x - 4.5, center.y + 4.5),
                ],
                stroke,
            );
        }
        _ => {
            ui.painter().text(
                center,
                egui::Align2::CENTER_CENTER,
                label,
                theme::font(config, 13.0),
                color,
            );
        }
    }
}

fn paint_transfer_footer(
    app: &BExplorerApp,
    ui: &mut egui::Ui,
    cancel_active: &mut bool,
    clear_pending: &mut bool,
) {
    ui.add_space(6.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if app.transfer_active()
            && styled_button(
                ui,
                &app.config,
                i18n::tr(&app.config, "cancel_active"),
                false,
            )
            .clicked()
        {
            *cancel_active = true;
        }
        if app.transfer_queue_len() > 0
            && styled_button(
                ui,
                &app.config,
                i18n::tr(&app.config, "clear_pending"),
                false,
            )
            .clicked()
        {
            *clear_pending = true;
        }
    });
}

fn paint_transfer_minimized(app: &BExplorerApp, ui: &mut egui::Ui) {
    let progress = app
        .transfer_progress_fraction()
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    let text = format!(
        "{}    {} {}",
        i18n::tr(&app.config, "transfers"),
        app.transfer_queue_len(),
        i18n::tr(&app.config, "pending")
    );
    ui.label(
        egui::RichText::new(text)
            .size(app.config.font_size - 0.5)
            .color(theme::muted(&app.config)),
    );
    paint_transfer_progress_bar(ui, &app.config, progress, 6.0);
}

fn paint_transfer_card(
    app: &BExplorerApp,
    ui: &mut egui::Ui,
    item: &crate::app::state::TransferDisplayItem,
    fraction: f32,
    active: bool,
    pause_or_resume: &mut Vec<u64>,
    cancel_jobs: &mut Vec<u64>,
) {
    let (card_rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), TRANSFER_CARD_HEIGHT),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(card_rect, 5.0, theme::surface(&app.config));
    ui.painter().rect_stroke(
        card_rect,
        5.0,
        egui::Stroke::new(1.0, theme::subtle_stroke(&app.config)),
    );

    let inner_rect = card_rect.shrink(7.0);
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(inner_rect), |ui| {
        ui.set_clip_rect(inner_rect);
        ui.set_width(inner_rect.width());
        ui.spacing_mut().item_spacing = egui::vec2(0.0, 2.0);

        let state = transfer_state_label(&app.config, item.state);
        let kind = transfer_kind_label(&app.config, item.kind);
        let title = if let Some(index) = item.queued_index {
            format!("{kind} #{index} - {state}")
        } else {
            format!("{kind} - {state}")
        };
        let title_row = ui
            .allocate_exact_size(egui::vec2(ui.available_width(), 30.0), egui::Sense::hover())
            .0;
        let buttons_width = if active { 184.0 } else { 0.0 };
        let title_clip = if active {
            egui::Rect::from_min_max(
                title_row.left_top(),
                egui::pos2(title_row.right() - buttons_width - 8.0, title_row.bottom()),
            )
        } else {
            title_row
        };
        paint_card_text_clipped(
            ui,
            &app.config,
            title_clip,
            &title,
            app.config.font_size,
            theme::text(&app.config),
        );
        if active {
            let buttons_rect = egui::Rect::from_min_size(
                egui::pos2(title_row.right() - buttons_width, title_row.top()),
                egui::vec2(buttons_width, title_row.height()),
            );
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(buttons_rect), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if styled_button(ui, &app.config, i18n::tr(&app.config, "cancel"), false)
                        .clicked()
                    {
                        cancel_jobs.push(item.id);
                    }
                    let label = if item.state == crate::fs::transfer_queue::TransferState::Paused {
                        i18n::tr(&app.config, "resume")
                    } else {
                        i18n::tr(&app.config, "pause")
                    };
                    if styled_button(ui, &app.config, label, false).clicked() {
                        pause_or_resume.push(item.id);
                    }
                });
            });
        }

        paint_card_text_row(
            ui,
            &app.config,
            &item.current_name,
            app.config.font_size - 0.5,
            theme::muted(&app.config),
            20.0,
        );
        paint_transfer_progress_bar(ui, &app.config, fraction, 7.0);

        let copied = crate::ui::file_table::format_bytes(item.copied_bytes);
        let total = crate::ui::file_table::format_bytes(item.total_bytes);
        let speed = crate::ui::file_table::format_bytes(item.bytes_per_second as u64);
        let detail = format!(
            "{copied} / {total}    {} / {} {}    {speed}/s",
            item.files_done,
            item.total_files,
            i18n::tr(&app.config, "files_count")
        );
        paint_card_text_row(
            ui,
            &app.config,
            &detail,
            app.config.font_size - 1.0,
            theme::faint(&app.config),
            19.0,
        );
    });
}

fn paint_card_text_row(
    ui: &mut egui::Ui,
    config: &AppConfig,
    text: &str,
    font_size: f32,
    color: egui::Color32,
    height: f32,
) {
    let rect = ui
        .allocate_exact_size(
            egui::vec2(ui.available_width(), height),
            egui::Sense::hover(),
        )
        .0;
    paint_card_text_clipped(ui, config, rect, text, font_size, color);
}

fn paint_card_text_clipped(
    ui: &mut egui::Ui,
    config: &AppConfig,
    rect: egui::Rect,
    text: &str,
    font_size: f32,
    color: egui::Color32,
) {
    let painter = ui.painter().with_clip_rect(rect);
    painter.text(
        egui::pos2(rect.left(), rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        theme::font(config, font_size),
        color,
    );
}

pub(super) fn paint_transfer_progress_bar(
    ui: &mut egui::Ui,
    config: &AppConfig,
    fraction: f32,
    height: f32,
) {
    let (bar_rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let rounding = (height * 0.5).max(2.0);
    ui.painter()
        .rect_filled(bar_rect, rounding, theme::control(config));
    let fill = egui::Rect::from_min_max(
        bar_rect.left_top(),
        egui::Pos2::new(
            bar_rect.left() + bar_rect.width() * fraction.clamp(0.0, 1.0),
            bar_rect.bottom(),
        ),
    );
    theme::paint_selection_gradient_rounded(ui.painter(), fill, rounding, config);
}

pub(super) fn styled_button(
    ui: &mut egui::Ui,
    config: &AppConfig,
    label: &str,
    primary: bool,
) -> egui::Response {
    let width = (label.chars().count() as f32 * 7.2 + 24.0).clamp(34.0, 210.0);
    let height = 30.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    let fill = if primary {
        theme::accent(config)
    } else if response.hovered() {
        theme::hover(config)
    } else {
        theme::control(config)
    };
    ui.painter().rect_filled(rect, 4.0, fill);
    ui.painter()
        .rect_stroke(rect, 4.0, egui::Stroke::new(1.0, theme::stroke(config)));
    let text_color = if primary {
        egui::Color32::WHITE
    } else {
        theme::text(config)
    };
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        theme::font(config, 12.0),
        text_color,
    );
    response
}

fn transfer_kind_label(
    config: &AppConfig,
    kind: crate::fs::transfer_queue::TransferKind,
) -> &'static str {
    match kind {
        crate::fs::transfer_queue::TransferKind::Copy => i18n::tr(config, "copying_kind"),
        crate::fs::transfer_queue::TransferKind::Move => i18n::tr(config, "moving_kind"),
    }
}

fn transfer_state_label(
    config: &AppConfig,
    state: crate::fs::transfer_queue::TransferState,
) -> &'static str {
    match state {
        crate::fs::transfer_queue::TransferState::Pending => i18n::tr(config, "pending"),
        crate::fs::transfer_queue::TransferState::Copying => i18n::tr(config, "copying"),
        crate::fs::transfer_queue::TransferState::Paused => i18n::tr(config, "paused"),
        crate::fs::transfer_queue::TransferState::Cancelled => i18n::tr(config, "cancelled"),
        crate::fs::transfer_queue::TransferState::Finished => i18n::tr(config, "finished"),
        crate::fs::transfer_queue::TransferState::Failed => i18n::tr(config, "failed"),
    }
}
