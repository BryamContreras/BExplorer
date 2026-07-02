use eframe::egui;

use crate::app::config::AppConfig;
use crate::app::state::{ArchiveDisplayItem, BExplorerApp};
use crate::fs::archive::{ArchiveJobKind, ArchiveState};
use crate::ui::i18n;
use crate::ui::theme;
use crate::ui::window_frame;

use super::transfer::{
    paint_transfer_compact_button, paint_transfer_progress_bar, paint_transfer_titlebar_button,
    styled_button,
};
use super::{TRANSFER_TITLEBAR_HEIGHT, TRANSFER_WINDOW_BUTTON};

const ARCHIVE_CARD_HEIGHT: f32 = 118.0;
const ARCHIVE_CARD_GAP: f32 = 6.0;
const ARCHIVE_FOOTER_HEIGHT: f32 = 40.0;

pub(super) fn show(app: &mut BExplorerApp, ctx: &egui::Context) {
    let items = app.archive_items();
    if items.is_empty() {
        app.archive_panel_spawned = false;
        return;
    }

    let window_title = compute_archive_window_title(app, &items);

    let panel_size = if app.archive_panel_minimized {
        egui::vec2(340.0, 104.0)
    } else {
        let content_height = TRANSFER_TITLEBAR_HEIGHT
            + 24.0
            + ARCHIVE_FOOTER_HEIGHT
            + items.len() as f32 * (ARCHIVE_CARD_HEIGHT + ARCHIVE_CARD_GAP);
        egui::vec2(462.0, content_height.clamp(190.0, 520.0))
    };

    let parent_rect = ctx
        .input(|i| i.viewport().outer_rect.or(i.viewport().inner_rect))
        .unwrap_or_else(|| ctx.screen_rect());
    let default_pos = egui::pos2(
        parent_rect.center().x - panel_size.x * 0.5,
        parent_rect.center().y - panel_size.y * 0.5,
    );
    let mut builder = egui::ViewportBuilder::default()
        .with_title(&window_title)
        .with_inner_size(panel_size)
        .with_min_inner_size(egui::vec2(320.0, 96.0))
        .with_resizable(true)
        .with_decorations(false)
        .with_taskbar(true)
        .with_close_button(false)
        .with_minimize_button(false)
        .with_maximize_button(false);
    if !app.archive_panel_spawned {
        builder = builder.with_position(default_pos);
        app.archive_panel_spawned = true;
    }

    let mut cancel_jobs = Vec::new();
    let mut clear_pending = false;
    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("bexplorer_archive_window"),
        builder,
        |viewport_ctx, _class| {
            theme::apply(viewport_ctx, &app.config);
            viewport_ctx.request_repaint_after(std::time::Duration::from_millis(16));
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(theme::surface_elevated(&app.config)))
                .show(viewport_ctx, |ui| {
                    let rect = ui.max_rect();
                    theme::paint_canvas_gradient(ui.painter(), rect, &app.config);
                    paint_archive_titlebar(app, viewport_ctx, ui, &window_title);

                    egui::Frame::none()
                        .inner_margin(egui::Margin::same(12.0))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());

                            if app.archive_panel_minimized {
                                paint_archive_minimized(app, ui, &items);
                                return;
                            }

                            let footer_height = ARCHIVE_FOOTER_HEIGHT;
                            let scroll_height = (ui.available_height() - footer_height).max(92.0);
                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(), scroll_height),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    egui::ScrollArea::vertical()
                                        .auto_shrink([false, false])
                                        .show(ui, |ui| {
                                            for item in &items {
                                                let fraction =
                                                    if item.total == 0 || item.total == u64::MAX {
                                                        0.0
                                                    } else {
                                                        (item.completed as f32 / item.total as f32)
                                                            .clamp(0.0, 1.0)
                                                    };
                                                let is_active = item.state == ArchiveState::Running;
                                                paint_archive_card(
                                                    app,
                                                    ui,
                                                    item,
                                                    fraction,
                                                    is_active,
                                                    &mut cancel_jobs,
                                                );
                                                ui.add_space(ARCHIVE_CARD_GAP);
                                            }
                                        });
                                },
                            );
                            paint_archive_footer(app, ui, &mut cancel_jobs, &mut clear_pending);
                        });
                });
            window_frame::show_resize_handles(viewport_ctx);
        },
    );

    for job_id in cancel_jobs {
        app.cancel_archive(job_id);
    }
    if clear_pending {
        app.clear_pending_archives();
    }
}

fn compute_archive_window_title(app: &BExplorerApp, items: &[ArchiveDisplayItem]) -> String {
    if let Some(running) = items.iter().find(|i| i.state == ArchiveState::Running) {
        let verb = match running.kind {
            ArchiveJobKind::Compress => i18n::tr(&app.config, "compressing"),
            ArchiveJobKind::Extract => i18n::tr(&app.config, "extracting"),
        };
        format!("{verb} {}", running.current_name)
    } else if let Some(first) = items.first() {
        archive_kind_label(&app.config, first.kind).to_string()
    } else {
        String::new()
    }
}

fn paint_archive_titlebar(
    app: &mut BExplorerApp,
    ctx: &egui::Context,
    ui: &mut egui::Ui,
    title: &str,
) {
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
        && ui.input(|i| i.pointer.primary_pressed())
        && !window_frame::pointer_in_resize_edge(ctx)
    {
        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }

    ui.painter().text(
        egui::pos2(rect.left() + 12.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        theme::font(&app.config, 12.5),
        theme::text(&app.config),
    );

    let compact_response = ui
        .allocate_rect(compact_rect, egui::Sense::click())
        .on_hover_text(if app.archive_panel_minimized {
            i18n::tr(&app.config, "expand")
        } else {
            i18n::tr(&app.config, "collapse")
        });
    paint_transfer_compact_button(
        ui,
        &app.config,
        compact_rect,
        app.archive_panel_minimized,
        compact_response.hovered(),
    );
    if compact_response.clicked() {
        app.archive_panel_minimized = !app.archive_panel_minimized;
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

fn paint_archive_minimized(app: &BExplorerApp, ui: &mut egui::Ui, items: &[ArchiveDisplayItem]) {
    let title = compute_archive_window_title(app, items);
    let running = items.iter().find(|i| i.state == ArchiveState::Running);
    let pending_count = items
        .iter()
        .filter(|i| i.state == ArchiveState::Pending)
        .count();

    let progress = running
        .filter(|r| r.total > 0 && r.total != u64::MAX)
        .map(|r| (r.completed as f32 / r.total as f32).clamp(0.0, 1.0))
        .unwrap_or(0.0);

    let suffix = if pending_count > 0 {
        format!(
            "    + {} {}",
            pending_count,
            i18n::tr(&app.config, "pending")
        )
    } else if running.is_some() && progress > 0.0 {
        format!("    {}%", (progress * 100.0) as u32)
    } else {
        String::new()
    };

    let text = format!("{title}{suffix}");
    ui.label(
        egui::RichText::new(text)
            .size(app.config.font_size - 0.5)
            .color(theme::muted(&app.config)),
    );
    paint_transfer_progress_bar(ui, &app.config, progress, 6.0);
}

fn paint_archive_card(
    app: &BExplorerApp,
    ui: &mut egui::Ui,
    item: &ArchiveDisplayItem,
    fraction: f32,
    is_active: bool,
    cancel_jobs: &mut Vec<u64>,
) {
    let (card_rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ARCHIVE_CARD_HEIGHT),
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

        let title = if item.state == ArchiveState::Running {
            let verb = match item.kind {
                ArchiveJobKind::Compress => i18n::tr(&app.config, "compressing"),
                ArchiveJobKind::Extract => i18n::tr(&app.config, "extracting"),
            };
            format!("{verb} {}", item.current_name)
        } else {
            let kind = archive_kind_label(&app.config, item.kind);
            let state = archive_state_label(&app.config, item.state);
            if let Some(index) = item.queued_index {
                format!("{kind} #{index} - {state}")
            } else {
                format!("{kind} - {state}")
            }
        };
        let title_row = ui
            .allocate_exact_size(egui::vec2(ui.available_width(), 30.0), egui::Sense::hover())
            .0;
        let button_width = if is_active { 84.0 } else { 0.0 };
        let title_clip = if is_active {
            egui::Rect::from_min_max(
                title_row.left_top(),
                egui::pos2(title_row.right() - button_width - 8.0, title_row.bottom()),
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
        if is_active {
            let button_rect = egui::Rect::from_min_size(
                egui::pos2(title_row.right() - button_width, title_row.top()),
                egui::vec2(button_width, title_row.height()),
            );
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(button_rect), |ui| {
                if styled_button(ui, &app.config, i18n::tr(&app.config, "cancel"), false).clicked()
                {
                    cancel_jobs.push(item.id);
                }
            });
        }

        let subtitle = if is_active && !item.file_name.is_empty() {
            item.file_name.as_str()
        } else {
            ""
        };
        paint_card_text_row(
            ui,
            &app.config,
            subtitle,
            app.config.font_size - 0.5,
            theme::muted(&app.config),
            20.0,
        );

        paint_transfer_progress_bar(ui, &app.config, fraction, 7.0);

        let show_indeterminate = item.total == 0 || item.total == u64::MAX;
        let percent_text = if show_indeterminate {
            String::new()
        } else {
            format!("{}%  ", (fraction * 100.0) as u32)
        };
        let copied = crate::ui::file_table::format_bytes(item.completed);
        let total_str = crate::ui::file_table::format_bytes(item.total);
        let speed = crate::ui::file_table::format_bytes(item.bytes_per_second as u64);

        let detail = if item.state == ArchiveState::Pending {
            String::new()
        } else if show_indeterminate || item.bytes_per_second <= 0.0 {
            format!("{copied} / {total_str}    {speed}/s")
        } else {
            let remaining = item.total.saturating_sub(item.completed) as f64;
            let eta_secs = (remaining / item.bytes_per_second) as u64;
            format!(
                "{percent_text}{copied} / {total_str}    {speed}/s    ETA: {}",
                format_duration(eta_secs)
            )
        };
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

fn paint_archive_footer(
    app: &BExplorerApp,
    ui: &mut egui::Ui,
    cancel_jobs: &mut Vec<u64>,
    clear_pending: &mut bool,
) {
    ui.add_space(6.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if app.is_archive_active()
            && styled_button(
                ui,
                &app.config,
                i18n::tr(&app.config, "cancel_active"),
                false,
            )
            .clicked()
        {
            for item in app.archive_items() {
                if item.state == ArchiveState::Running {
                    cancel_jobs.push(item.id);
                }
            }
        }
        if app.archive_queue.len() > 0
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

fn archive_kind_label(config: &AppConfig, kind: ArchiveJobKind) -> &'static str {
    match kind {
        ArchiveJobKind::Compress => i18n::tr(config, "compressing_kind"),
        ArchiveJobKind::Extract => i18n::tr(config, "extracting_kind"),
    }
}

fn archive_state_label(config: &AppConfig, state: ArchiveState) -> &'static str {
    match state {
        ArchiveState::Pending => i18n::tr(config, "pending"),
        ArchiveState::Running => "",
        ArchiveState::Finished => i18n::tr(config, "finished"),
        ArchiveState::Cancelled => i18n::tr(config, "cancelled"),
        ArchiveState::Failed => i18n::tr(config, "failed"),
    }
}

fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {:02}s", secs / 60, secs % 60)
    } else {
        format!(
            "{}h {:02}m {:02}s",
            secs / 3600,
            (secs % 3600) / 60,
            secs % 60
        )
    }
}
