use eframe::egui;

use crate::app::config::AppConfig;
use crate::app::state::BExplorerApp;
use crate::fs::defender::{DefenderProgress, DefenderScanState, DefenderSummary};
use crate::ui::i18n;
use crate::ui::theme;
use crate::ui::window_frame;

use super::transfer::{
    paint_transfer_compact_button, paint_transfer_progress_bar, paint_transfer_titlebar_button,
    styled_button,
};
use super::{TRANSFER_TITLEBAR_HEIGHT, TRANSFER_WINDOW_BUTTON};

const DEFENDER_CARD_HEIGHT: f32 = 134.0;
const DEFENDER_SUMMARY_COMPACT_HEIGHT: f32 = 204.0;
const DEFENDER_SUMMARY_DETAIL_HEIGHT: f32 = 238.0;
const DEFENDER_SUMMARY_THREATS_HEIGHT: f32 = 318.0;
const DEFENDER_FOOTER_HEIGHT: f32 = 42.0;

pub(super) fn show(app: &mut BExplorerApp, ctx: &egui::Context) {
    if !app.defender_visible() {
        app.defender_panel_spawned = false;
        return;
    }

    let title = i18n::tr(&app.config, "windows_defender").to_string();
    let panel_size = if app.defender_panel_minimized {
        egui::vec2(360.0, 104.0)
    } else if let Some(summary) = app.defender_summary.as_ref() {
        egui::vec2(500.0, defender_summary_panel_height(summary))
    } else {
        egui::vec2(
            462.0,
            DEFENDER_CARD_HEIGHT + TRANSFER_TITLEBAR_HEIGHT + 28.0,
        )
    };

    let parent_rect = ctx
        .input(|input| input.viewport().outer_rect.or(input.viewport().inner_rect))
        .unwrap_or_else(|| ctx.screen_rect());
    let default_pos = egui::pos2(
        parent_rect.center().x - panel_size.x * 0.5,
        parent_rect.center().y - panel_size.y * 0.5,
    );
    let mut builder = egui::ViewportBuilder::default()
        .with_title(&title)
        .with_inner_size(panel_size)
        .with_min_inner_size(egui::vec2(340.0, 96.0))
        .with_resizable(true)
        .with_decorations(false)
        .with_taskbar(true)
        .with_close_button(false)
        .with_minimize_button(false)
        .with_maximize_button(false);
    if !app.defender_panel_spawned {
        builder = builder.with_position(default_pos);
        app.defender_panel_spawned = true;
    }

    let mut cancel_scan = false;
    let mut close_panel = false;
    let mut remove_threats = false;
    let mut exclude_paths = false;
    let mut open_security = false;

    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("bexplorer_defender_window"),
        builder,
        |viewport_ctx, _class| {
            theme::apply(viewport_ctx, &app.config);
            viewport_ctx.request_repaint_after(std::time::Duration::from_millis(16));
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(theme::surface_elevated(&app.config)))
                .show(viewport_ctx, |ui| {
                    let rect = ui.max_rect();
                    theme::paint_canvas_gradient(ui.painter(), rect, &app.config);
                    paint_defender_titlebar(app, viewport_ctx, ui, &title, &mut close_panel);

                    egui::Frame::none()
                        .inner_margin(egui::Margin::same(12.0))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());

                            if app.defender_panel_minimized {
                                paint_defender_minimized(app, ui);
                                return;
                            }

                            if let Some(summary) = app.defender_summary.as_ref() {
                                paint_defender_summary(
                                    app,
                                    ui,
                                    summary,
                                    &mut remove_threats,
                                    &mut exclude_paths,
                                    &mut open_security,
                                    &mut close_panel,
                                );
                            } else if let Some(progress) = app.defender_progress.as_ref() {
                                paint_defender_progress(app, ui, progress, &mut cancel_scan);
                            }
                        });
                });
            window_frame::show_resize_handles(viewport_ctx);
        },
    );

    if cancel_scan {
        app.cancel_defender_scan();
    }
    if close_panel {
        app.close_defender_panel();
    }
    if remove_threats {
        app.remove_defender_threats();
    }
    if exclude_paths {
        app.exclude_defender_scan_paths();
    }
    if open_security {
        app.open_windows_security();
    }
}

fn paint_defender_titlebar(
    app: &mut BExplorerApp,
    ctx: &egui::Context,
    ui: &mut egui::Ui,
    title: &str,
    close_panel: &mut bool,
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
        egui::pos2(rect.right() - TRANSFER_WINDOW_BUTTON * 3.0, rect.top()),
        egui::vec2(TRANSFER_WINDOW_BUTTON, rect.height()),
    );
    let minimize_rect = egui::Rect::from_min_size(
        egui::pos2(rect.right() - TRANSFER_WINDOW_BUTTON * 2.0, rect.top()),
        egui::vec2(TRANSFER_WINDOW_BUTTON, rect.height()),
    );
    let close_rect = egui::Rect::from_min_size(
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
        title,
        theme::font(&app.config, 12.5),
        theme::text(&app.config),
    );

    let compact_response = ui
        .allocate_rect(compact_rect, egui::Sense::click())
        .on_hover_text(if app.defender_panel_minimized {
            i18n::tr(&app.config, "expand")
        } else {
            i18n::tr(&app.config, "collapse")
        });
    paint_transfer_compact_button(
        ui,
        &app.config,
        compact_rect,
        app.defender_panel_minimized,
        compact_response.hovered(),
    );
    if compact_response.clicked() {
        app.defender_panel_minimized = !app.defender_panel_minimized;
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

    let close_response = ui
        .allocate_rect(close_rect, egui::Sense::click())
        .on_hover_text(i18n::tr(&app.config, "close"));
    paint_transfer_titlebar_button(ui, &app.config, close_rect, "x", close_response.hovered());
    if close_response.clicked() {
        *close_panel = true;
    }
}

fn paint_defender_minimized(app: &BExplorerApp, ui: &mut egui::Ui) {
    let progress = app.defender_progress.as_ref();
    let text = if let Some(progress) = progress {
        format!(
            "{}    {} / {}",
            defender_state_label(&app.config, progress.state),
            progress.scanned,
            progress.total
        )
    } else {
        i18n::tr(&app.config, "windows_defender").to_string()
    };
    ui.label(
        egui::RichText::new(text)
            .size(app.config.font_size - 0.5)
            .color(theme::muted(&app.config)),
    );
    if let Some(progress) = progress {
        if progress.total > 1 {
            paint_transfer_progress_bar(ui, &app.config, progress_fraction(progress), 6.0);
        } else {
            paint_indeterminate_progress_bar(ui, &app.config, 6.0);
        }
    }
}

fn defender_summary_panel_height(summary: &DefenderSummary) -> f32 {
    if !summary.threats.is_empty() {
        DEFENDER_SUMMARY_THREATS_HEIGHT
    } else if summary.error.is_some() || summary.state != DefenderScanState::Finished {
        DEFENDER_SUMMARY_DETAIL_HEIGHT
    } else {
        DEFENDER_SUMMARY_COMPACT_HEIGHT
    }
}

fn paint_defender_progress(
    app: &BExplorerApp,
    ui: &mut egui::Ui,
    progress: &DefenderProgress,
    cancel_scan: &mut bool,
) {
    let (card_rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), DEFENDER_CARD_HEIGHT),
        egui::Sense::hover(),
    );
    paint_card_shell(ui, &app.config, card_rect);
    let inner_rect = card_rect.shrink(8.0);
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(inner_rect), |ui| {
        ui.set_width(inner_rect.width());
        ui.spacing_mut().item_spacing = egui::vec2(0.0, 4.0);

        let title_row = ui
            .allocate_exact_size(egui::vec2(ui.available_width(), 30.0), egui::Sense::hover())
            .0;
        let button_rect = egui::Rect::from_min_size(
            egui::pos2(title_row.right() - 88.0, title_row.top()),
            egui::vec2(88.0, title_row.height()),
        );
        paint_text_clipped(
            ui,
            &app.config,
            egui::Rect::from_min_max(title_row.left_top(), button_rect.left_bottom()),
            i18n::tr(&app.config, "defender_scanning"),
            app.config.font_size,
            theme::text(&app.config),
        );
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(button_rect), |ui| {
            if styled_button(ui, &app.config, i18n::tr(&app.config, "cancel"), false).clicked() {
                *cancel_scan = true;
            }
        });

        let current = progress
            .current_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        paint_text_row(
            ui,
            &app.config,
            &current,
            app.config.font_size - 0.5,
            theme::muted(&app.config),
            22.0,
        );

        if progress.total > 1 {
            paint_transfer_progress_bar(ui, &app.config, progress_fraction(progress), 7.0);
        } else {
            paint_indeterminate_progress_bar(ui, &app.config, 7.0);
        }

        let elapsed = progress.started.elapsed().as_secs();
        let detail = format!(
            "{} / {}    {}: {}    {}: {}",
            progress.scanned,
            progress.total,
            i18n::tr(&app.config, "threats"),
            progress.threats_found,
            i18n::tr(&app.config, "elapsed"),
            format_duration(elapsed)
        );
        paint_text_row(
            ui,
            &app.config,
            &detail,
            app.config.font_size - 1.0,
            theme::faint(&app.config),
            20.0,
        );
    });
}

fn paint_defender_summary(
    app: &BExplorerApp,
    ui: &mut egui::Ui,
    summary: &DefenderSummary,
    remove_threats: &mut bool,
    exclude_paths: &mut bool,
    open_security: &mut bool,
    close_panel: &mut bool,
) {
    let body_height = (ui.available_height() - DEFENDER_FOOTER_HEIGHT).max(72.0);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), body_height),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            let title = match summary.state {
                DefenderScanState::Finished if summary.threats.is_empty() => {
                    i18n::tr(&app.config, "defender_no_threats")
                }
                DefenderScanState::Finished => i18n::tr(&app.config, "defender_threats_found"),
                DefenderScanState::Cancelled => i18n::tr(&app.config, "cancelled"),
                DefenderScanState::Failed => i18n::tr(&app.config, "failed"),
                DefenderScanState::Running => i18n::tr(&app.config, "defender_scanning"),
            };
            ui.label(
                egui::RichText::new(title)
                    .size(app.config.font_size + 0.5)
                    .color(theme::text(&app.config)),
            );
            ui.add_space(8.0);
            let detail = format!(
                "{} / {}    {}: {}",
                summary.scanned,
                summary.total,
                i18n::tr(&app.config, "threats"),
                summary.threats.len()
            );
            ui.label(
                egui::RichText::new(detail)
                    .size(app.config.font_size - 0.5)
                    .color(theme::muted(&app.config)),
            );
            if let Some(error) = &summary.error {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(error)
                        .size(app.config.font_size - 0.5)
                        .color(theme::muted(&app.config)),
                );
            }
            if let Some(last_output) = summary.outputs.last() {
                ui.add_space(6.0);
                let output_line = last_output
                    .output
                    .lines()
                    .rev()
                    .map(str::trim)
                    .find(|line| !line.is_empty())
                    .unwrap_or("");
                let status = last_output
                    .exit_code
                    .map(|code| format!("exit {code}"))
                    .unwrap_or_else(|| "exit".into());
                let target = last_output
                    .target
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_else(|| last_output.target.to_str().unwrap_or(""));
                let line = if output_line.is_empty() {
                    format!("{target}    {status}")
                } else {
                    format!("{target}    {status}    {output_line}")
                };
                paint_text_row(
                    ui,
                    &app.config,
                    &line,
                    app.config.font_size - 1.0,
                    theme::faint(&app.config),
                    20.0,
                );
            }
            if !summary.threats.is_empty() {
                ui.add_space(10.0);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(118.0)
                    .show(ui, |ui| {
                        for threat in &summary.threats {
                            paint_threat_row(app, ui, threat);
                            ui.add_space(5.0);
                        }
                    });
            }
        },
    );
    paint_defender_footer(
        app,
        ui,
        summary,
        remove_threats,
        exclude_paths,
        open_security,
        close_panel,
    );
}

fn paint_threat_row(
    app: &BExplorerApp,
    ui: &mut egui::Ui,
    threat: &crate::fs::defender::DefenderThreat,
) {
    let height = 48.0;
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(rect, 5.0, theme::surface(&app.config));
    ui.painter().rect_stroke(
        rect,
        5.0,
        egui::Stroke::new(1.0, theme::subtle_stroke(&app.config)),
    );
    let inner = rect.shrink(7.0);
    let path = threat
        .path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| threat.status.clone());
    paint_text_clipped(
        ui,
        &app.config,
        egui::Rect::from_min_max(
            inner.left_top(),
            egui::pos2(inner.right(), inner.top() + 20.0),
        ),
        &threat.name,
        app.config.font_size - 0.3,
        theme::text(&app.config),
    );
    paint_text_clipped(
        ui,
        &app.config,
        egui::Rect::from_min_max(
            egui::pos2(inner.left(), inner.top() + 21.0),
            inner.right_bottom(),
        ),
        &path,
        app.config.font_size - 1.0,
        theme::muted(&app.config),
    );
}

fn paint_defender_footer(
    app: &BExplorerApp,
    ui: &mut egui::Ui,
    summary: &DefenderSummary,
    remove_threats: &mut bool,
    exclude_paths: &mut bool,
    open_security: &mut bool,
    close_panel: &mut bool,
) {
    ui.add_space(6.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if styled_button(ui, &app.config, i18n::tr(&app.config, "close"), false).clicked() {
            *close_panel = true;
        }
        ui.add_space(6.0);
        if styled_button(
            ui,
            &app.config,
            i18n::tr(&app.config, "open_windows_security"),
            false,
        )
        .clicked()
        {
            *open_security = true;
        }
        if !summary.threats.is_empty() {
            ui.add_space(6.0);
            if styled_button(
                ui,
                &app.config,
                i18n::tr(&app.config, "exclude_paths"),
                false,
            )
            .clicked()
            {
                *exclude_paths = true;
            }
            ui.add_space(6.0);
            if styled_button(
                ui,
                &app.config,
                i18n::tr(&app.config, "remove_threats"),
                true,
            )
            .clicked()
            {
                *remove_threats = true;
            }
        }
    });
}

fn paint_card_shell(ui: &mut egui::Ui, config: &AppConfig, rect: egui::Rect) {
    ui.painter().rect_filled(rect, 5.0, theme::surface(config));
    ui.painter().rect_stroke(
        rect,
        5.0,
        egui::Stroke::new(1.0, theme::subtle_stroke(config)),
    );
}

fn paint_text_row(
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
    paint_text_clipped(ui, config, rect, text, font_size, color);
}

fn paint_text_clipped(
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

fn paint_indeterminate_progress_bar(ui: &mut egui::Ui, config: &AppConfig, height: f32) {
    let (bar_rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(bar_rect, 4.0, theme::control(config));
    let time = ui.ctx().input(|input| input.time) as f32;
    let width = (bar_rect.width() * 0.32).clamp(56.0, 180.0);
    let travel = (bar_rect.width() + width).max(1.0);
    let left = bar_rect.left() - width + (time * 180.0).rem_euclid(travel);
    let fill = egui::Rect::from_min_max(
        egui::pos2(left, bar_rect.top()),
        egui::pos2((left + width).min(bar_rect.right()), bar_rect.bottom()),
    )
    .intersect(bar_rect);
    theme::paint_selection_gradient(ui.painter(), fill, config);
}

fn progress_fraction(progress: &DefenderProgress) -> f32 {
    if progress.total == 0 {
        0.0
    } else {
        (progress.scanned as f32 / progress.total as f32).clamp(0.0, 1.0)
    }
}

fn defender_state_label(config: &AppConfig, state: DefenderScanState) -> &'static str {
    match state {
        DefenderScanState::Running => i18n::tr(config, "defender_scanning"),
        DefenderScanState::Finished => i18n::tr(config, "finished"),
        DefenderScanState::Cancelled => i18n::tr(config, "cancelled"),
        DefenderScanState::Failed => i18n::tr(config, "failed"),
    }
}

fn format_duration(seconds: u64) -> String {
    let minutes = seconds / 60;
    let seconds = seconds % 60;
    if minutes == 0 {
        format!("{seconds}s")
    } else {
        format!("{minutes}m {seconds}s")
    }
}
