use eframe::egui;

use crate::app::config::{
    AppConfig, ShortcutAction, ShortcutConfig, ThemePreference, VibrancyMode, ViewMode,
};
use crate::app::state::{BExplorerApp, shortcut_binding_from_input, shortcut_binding_label};
use crate::fs::archive::{ArchiveCompressionMethod, ArchiveFormat};
use crate::fs::operations::ElevatedFileOperation;
use crate::fs::transfer_queue::{ConflictPolicy, TransferKind};
use crate::ui::i18n;
use crate::ui::theme;
use crate::ui::window_frame;

mod archive_progress;
mod defender_progress;
mod transfer;

const TRANSFER_TITLEBAR_HEIGHT: f32 = 36.0;
const TRANSFER_WINDOW_BUTTON: f32 = 42.0;
const FLOATING_DIALOG_RADIUS: f32 = 6.0;
const ERROR_DIALOG_WIDTH: f32 = 430.0;
const CONFLICT_DIALOG_WIDTH: f32 = 470.0;
const COMPRESS_DIALOG_WIDTH: f32 = 480.0;
const ARCHIVE_PASSWORD_DIALOG_WIDTH: f32 = 430.0;
const OPTIONS_DIALOG_WIDTH: f32 = 398.0;
const SHORTCUTS_DIALOG_WIDTH: f32 = 430.0;
const CUSTOMIZABLE_SHORTCUT_ACTIONS: [ShortcutAction; 13] = [
    ShortcutAction::CommandPalette,
    ShortcutAction::Copy,
    ShortcutAction::Cut,
    ShortcutAction::Paste,
    ShortcutAction::SelectAll,
    ShortcutAction::Refresh,
    ShortcutAction::Rename,
    ShortcutAction::Delete,
    ShortcutAction::PermanentDelete,
    ShortcutAction::Properties,
    ShortcutAction::GoUp,
    ShortcutAction::GoBack,
    ShortcutAction::GoForward,
];

enum DialogTitleAlign {
    Left,
    Center,
}

pub fn show(app: &mut BExplorerApp, ctx: &egui::Context) {
    show_error(app, ctx);
    show_delete_confirmation(app, ctx);
    show_transfer_conflict_dialog(app, ctx);
    show_elevated_transfer_dialog(app, ctx);
    show_archive_password_dialog(app, ctx);
    show_compress_dialog(app, ctx);
    transfer::show(app, ctx);
    archive_progress::show(app, ctx);
    defender_progress::show(app, ctx);
    show_options(app, ctx);
    show_shortcuts(app, ctx);
}

fn show_elevated_transfer_dialog(app: &mut BExplorerApp, ctx: &egui::Context) {
    let transfer_job = app.pending_elevated_transfer.clone();
    let file_operation = app.pending_elevated_operation.clone();
    if transfer_job.is_none() && file_operation.is_none() {
        return;
    };
    let message = if let Some(job) = transfer_job.as_ref() {
        elevated_transfer_message(app, job)
    } else {
        elevated_operation_message(app, file_operation.as_ref().expect("operation exists"))
    };

    let mut confirm = false;
    let mut cancel = false;
    if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        cancel = true;
    }

    egui::Area::new(egui::Id::new("elevated_transfer_dialog"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .movable(true)
        .show(ctx, |ui| {
            floating_dialog_frame(&app.config).show(ui, |ui| {
                set_floating_dialog_width(ui, CONFLICT_DIALOG_WIDTH);
                paint_floating_dialog_header(
                    app,
                    ui,
                    CONFLICT_DIALOG_WIDTH,
                    i18n::tr(&app.config, "admin_permission"),
                    DialogTitleAlign::Center,
                    &mut cancel,
                );
                floating_dialog_body(ui, CONFLICT_DIALOG_WIDTH, 14.0, 12.0, |ui| {
                    ui.add_sized(
                        egui::vec2(ui.available_width(), 0.0),
                        egui::Label::new(
                            egui::RichText::new(message.clone())
                                .size(app.config.font_size)
                                .color(theme::muted(&app.config)),
                        )
                        .wrap(),
                    );
                    ui.add_space(12.0);
                    floating_dialog_button_row(ui, |ui| {
                        if transfer::styled_button(
                            ui,
                            &app.config,
                            i18n::tr(&app.config, "cancel"),
                            false,
                        )
                        .clicked()
                        {
                            cancel = true;
                        }
                        ui.add_space(6.0);
                        if transfer::styled_button(
                            ui,
                            &app.config,
                            i18n::tr(&app.config, "grant_access"),
                            true,
                        )
                        .clicked()
                        {
                            confirm = true;
                        }
                    });
                });
            });
        });

    if confirm {
        if transfer_job.is_some() {
            app.confirm_elevated_transfer();
        } else {
            app.confirm_elevated_file_operation();
        }
    } else if cancel {
        if transfer_job.is_some() {
            app.cancel_elevated_transfer();
        } else {
            app.cancel_elevated_file_operation();
        }
    }
}

fn elevated_transfer_message(
    app: &BExplorerApp,
    job: &crate::fs::transfer_queue::TransferJob,
) -> String {
    let destination = job.destination.display();
    if app.config.language == "es" {
        format!(
            "No tienes permisos para completar esta transferencia en \"{destination}\". Puedes conceder acceso con permisos de administrador."
        )
    } else {
        format!(
            "You do not have permission to complete this transfer in \"{destination}\". You can grant access with administrator permission."
        )
    }
}

fn elevated_operation_message(app: &BExplorerApp, operation: &ElevatedFileOperation) -> String {
    let target = match operation {
        ElevatedFileOperation::Rename { path, .. } | ElevatedFileOperation::Duplicate { path } => {
            path.display().to_string()
        }
        ElevatedFileOperation::Delete { paths, .. } => paths
            .first()
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
        ElevatedFileOperation::CreateFolder { parent, .. }
        | ElevatedFileOperation::CreateFile { parent, .. } => parent.display().to_string(),
    };

    if app.config.language == "es" {
        format!(
            "No tienes permisos para completar esta operacion en \"{target}\". Puedes conceder acceso con permisos de administrador."
        )
    } else {
        format!(
            "You do not have permission to complete this operation in \"{target}\". You can grant access with administrator permission."
        )
    }
}

fn show_transfer_conflict_dialog(app: &mut BExplorerApp, ctx: &egui::Context) {
    let Some(conflict) = app.pending_transfer_conflict.clone() else {
        return;
    };

    let mut selected_policy = None;
    let mut cancel = false;
    if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        cancel = true;
    }

    egui::Area::new(egui::Id::new("transfer_conflict_dialog"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .movable(true)
        .show(ctx, |ui| {
            floating_dialog_frame(&app.config).show(ui, |ui| {
                set_floating_dialog_width(ui, CONFLICT_DIALOG_WIDTH);
                paint_floating_dialog_header(
                    app,
                    ui,
                    CONFLICT_DIALOG_WIDTH,
                    i18n::tr(&app.config, "file_conflict"),
                    DialogTitleAlign::Center,
                    &mut cancel,
                );
                floating_dialog_body(ui, CONFLICT_DIALOG_WIDTH, 14.0, 12.0, |ui| {
                    ui.add_sized(
                        egui::vec2(ui.available_width(), 0.0),
                        egui::Label::new(
                            egui::RichText::new(transfer_conflict_message(app, &conflict))
                                .size(app.config.font_size)
                                .color(theme::muted(&app.config)),
                        )
                        .wrap(),
                    );
                    ui.add_space(12.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), 32.0),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if transfer::styled_button(
                                ui,
                                &app.config,
                                i18n::tr(&app.config, "cancel"),
                                false,
                            )
                            .clicked()
                            {
                                cancel = true;
                            }
                            ui.add_space(6.0);
                            if transfer::styled_button(
                                ui,
                                &app.config,
                                i18n::tr(&app.config, "skip"),
                                false,
                            )
                            .clicked()
                            {
                                selected_policy = Some(ConflictPolicy::Skip);
                            }
                            ui.add_space(6.0);
                            if transfer::styled_button(
                                ui,
                                &app.config,
                                i18n::tr(&app.config, "replace"),
                                false,
                            )
                            .clicked()
                            {
                                selected_policy = Some(ConflictPolicy::Replace);
                            }
                            ui.add_space(6.0);
                            if transfer::styled_button(
                                ui,
                                &app.config,
                                i18n::tr(&app.config, "keep_both"),
                                true,
                            )
                            .clicked()
                            {
                                selected_policy = Some(ConflictPolicy::KeepBoth);
                            }
                        },
                    );
                });
            });
        });

    if let Some(policy) = selected_policy {
        app.confirm_transfer_conflict(policy);
    } else if cancel {
        app.cancel_transfer_conflict();
    }
}

fn transfer_conflict_message(
    app: &BExplorerApp,
    conflict: &crate::app::state::PendingTransferConflict,
) -> String {
    let action = match conflict.kind {
        TransferKind::Copy => i18n::tr(&app.config, "copy_action"),
        TransferKind::Move => i18n::tr(&app.config, "move_action"),
    };

    if app.config.language == "es" {
        if conflict.conflict_count == 1 {
            format!(
                "Ya existe \"{}\" en el destino. Elige como quieres continuar al {}.",
                conflict.first_conflict_name, action
            )
        } else {
            format!(
                "Ya existen {} elementos en el destino. Elige como quieres continuar al {}.",
                conflict.conflict_count, action
            )
        }
    } else if conflict.conflict_count == 1 {
        format!(
            "\"{}\" already exists in the destination. Choose how to continue.",
            conflict.first_conflict_name
        )
    } else {
        format!(
            "{} items already exist in the destination. Choose how to continue.",
            conflict.conflict_count
        )
    }
}

fn floating_dialog_frame(config: &AppConfig) -> egui::Frame {
    egui::Frame::none()
        .fill(theme::popup_surface(config))
        .stroke(egui::Stroke::new(1.0, theme::popup_stroke(config)))
        .rounding(egui::Rounding::same(FLOATING_DIALOG_RADIUS))
        .shadow(theme::popup_shadow(config))
}

fn set_floating_dialog_width(ui: &mut egui::Ui, width: f32) {
    ui.set_width(width);
    ui.set_min_width(width);
    ui.set_max_width(width);
}

fn floating_dialog_body(
    ui: &mut egui::Ui,
    width: f32,
    margin_x: f32,
    margin_y: f32,
    contents: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::none()
        .inner_margin(egui::Margin::symmetric(margin_x, margin_y))
        .show(ui, |ui| {
            let content_width = (width - margin_x * 2.0).max(0.0);
            ui.set_width(content_width);
            ui.set_min_width(content_width);
            ui.set_max_width(content_width);
            contents(ui);
        });
}

fn floating_dialog_button_row(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), 32.0),
        egui::Layout::right_to_left(egui::Align::Center),
        contents,
    );
}

fn paint_floating_dialog_header(
    app: &BExplorerApp,
    ui: &mut egui::Ui,
    width: f32,
    title: &str,
    align: DialogTitleAlign,
    close: &mut bool,
) {
    let (header, _) = ui.allocate_exact_size(egui::vec2(width, 38.0), egui::Sense::hover());
    let rounding = egui::Rounding {
        nw: FLOATING_DIALOG_RADIUS,
        ne: FLOATING_DIALOG_RADIUS,
        sw: 0.0,
        se: 0.0,
    };
    ui.painter()
        .rect_filled(header, rounding, theme::titlebar(&app.config));
    theme::paint_titlebar_gradient(ui.painter(), header, &app.config);
    ui.painter().line_segment(
        [header.left_bottom(), header.right_bottom()],
        egui::Stroke::new(1.0, theme::stroke(&app.config)),
    );

    let (text_pos, text_align) = match align {
        DialogTitleAlign::Left => (
            egui::pos2(header.left() + 14.0, header.center().y),
            egui::Align2::LEFT_CENTER,
        ),
        DialogTitleAlign::Center => (header.center(), egui::Align2::CENTER_CENTER),
    };
    ui.painter().text(
        text_pos,
        text_align,
        title,
        theme::font(&app.config, 13.0),
        theme::text(&app.config),
    );

    let close_rect = egui::Rect::from_min_size(
        egui::pos2(header.right() - TRANSFER_WINDOW_BUTTON, header.top()),
        egui::vec2(TRANSFER_WINDOW_BUTTON, header.height()),
    );
    let close_response = ui.allocate_rect(close_rect, egui::Sense::click());
    if close_response.hovered() {
        theme::paint_hover_gradient(ui.painter(), close_rect, 0.0, &app.config);
    }
    ui.painter().text(
        close_rect.center(),
        egui::Align2::CENTER_CENTER,
        "x",
        theme::font(&app.config, 13.0),
        theme::muted(&app.config),
    );
    if close_response.clicked() {
        *close = true;
    }
}

fn show_error(app: &mut BExplorerApp, ctx: &egui::Context) {
    let Some(message) = app.error_message.clone() else {
        return;
    };

    let mut close = false;
    if ctx
        .input(|input| input.key_pressed(egui::Key::Enter) || input.key_pressed(egui::Key::Escape))
    {
        close = true;
    }
    let message = friendly_error_message(&app.config, &message);

    egui::Area::new(egui::Id::new("error_panel"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .movable(true)
        .show(ctx, |ui| {
            floating_dialog_frame(&app.config).show(ui, |ui| {
                set_floating_dialog_width(ui, ERROR_DIALOG_WIDTH);
                paint_error_header(app, ui, &mut close);
                floating_dialog_body(ui, ERROR_DIALOG_WIDTH, 14.0, 12.0, |ui| {
                    ui.add_sized(
                        egui::vec2(ui.available_width(), 0.0),
                        egui::Label::new(
                            egui::RichText::new(message.clone())
                                .size(app.config.font_size)
                                .color(theme::muted(&app.config)),
                        )
                        .wrap(),
                    );
                    ui.add_space(12.0);
                    floating_dialog_button_row(ui, |ui| {
                        if transfer::styled_button(
                            ui,
                            &app.config,
                            i18n::tr(&app.config, "ok"),
                            true,
                        )
                        .clicked()
                        {
                            close = true;
                        }
                    });
                });
            });
        });

    if close {
        app.error_message = None;
    }
}

fn paint_error_header(app: &BExplorerApp, ui: &mut egui::Ui, close: &mut bool) {
    paint_floating_dialog_header(
        app,
        ui,
        ERROR_DIALOG_WIDTH,
        i18n::tr(&app.config, "error"),
        DialogTitleAlign::Center,
        close,
    );
}

fn friendly_error_message(config: &AppConfig, message: &str) -> String {
    let cleaned = strip_error_prefix(message);
    let lower = cleaned.to_ascii_lowercase();
    if lower.contains("7z listing crashed")
        || lower.contains("7z listing helper failed")
        || lower.contains("0xc0000005")
    {
        if config.language == "es" {
            return "No se pudo abrir este comprimido. El motor 7z se cerro inesperadamente al leer su contenido; la app principal sigue estable.".to_string();
        }
        return "Could not open this archive. The 7z engine closed unexpectedly while reading its contents; the main app is still stable.".to_string();
    }
    cleaned.to_string()
}

fn strip_error_prefix(message: &str) -> &str {
    for prefix in [
        "Operation error: ",
        "Shell error: ",
        "I/O error: ",
        "JSON error: ",
        "Clipboard error: ",
    ] {
        if let Some(rest) = message.strip_prefix(prefix) {
            return rest;
        }
    }
    message
}

fn show_delete_confirmation(app: &mut BExplorerApp, ctx: &egui::Context) {
    let Some(paths) = app.confirm_permanent_delete.clone() else {
        app.delete_panel_spawned = false;
        return;
    };

    let mut confirm = false;
    let mut cancel = false;
    if ctx.input(|input| input.key_pressed(egui::Key::Enter)) {
        confirm = true;
    }
    if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        cancel = true;
    }
    let panel_size = egui::vec2(480.0, 168.0);
    let parent_rect = ctx
        .input(|input| input.viewport().outer_rect.or(input.viewport().inner_rect))
        .unwrap_or_else(|| ctx.screen_rect());
    let default_pos = egui::pos2(
        parent_rect.center().x - panel_size.x * 0.5,
        parent_rect.center().y - panel_size.y * 0.5,
    );
    let mut builder = egui::ViewportBuilder::default()
        .with_title(i18n::tr(&app.config, "confirm_delete"))
        .with_inner_size(panel_size)
        .with_min_inner_size(egui::vec2(360.0, 132.0))
        .with_resizable(true)
        .with_decorations(false)
        .with_always_on_top()
        .with_taskbar(true)
        .with_close_button(false)
        .with_minimize_button(false)
        .with_maximize_button(false);
    if !app.delete_panel_spawned {
        builder = builder.with_position(default_pos);
        app.delete_panel_spawned = true;
    }

    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("bexplorer_delete_confirm_window"),
        builder,
        |viewport_ctx, _class| {
            theme::apply(viewport_ctx, &app.config);
            if viewport_ctx.input(|input| input.key_pressed(egui::Key::Enter)) {
                confirm = true;
            }
            if viewport_ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
                cancel = true;
            }

            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(theme::surface_elevated(&app.config)))
                .show(viewport_ctx, |ui| {
                    let rect = ui.max_rect();
                    theme::paint_canvas_gradient(ui.painter(), rect, &app.config);
                    paint_delete_titlebar(app, viewport_ctx, ui, &mut cancel);

                    egui::Frame::none()
                        .inner_margin(egui::Margin::same(12.0))
                        .show(ui, |ui| {
                            let footer_height = 42.0;
                            let scroll_height = (ui.available_height() - footer_height).max(42.0);
                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(), scroll_height),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    egui::ScrollArea::vertical()
                                        .auto_shrink([false, false])
                                        .show(ui, |ui| {
                                            ui.label(
                                                egui::RichText::new(permanent_delete_message(
                                                    app, &paths,
                                                ))
                                                .size(app.config.font_size)
                                                .color(theme::muted(&app.config)),
                                            );
                                        });
                                },
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if transfer::styled_button(
                                        ui,
                                        &app.config,
                                        i18n::tr(&app.config, "cancel"),
                                        false,
                                    )
                                    .clicked()
                                    {
                                        cancel = true;
                                    }
                                    ui.add_space(6.0);
                                    if transfer::styled_button(
                                        ui,
                                        &app.config,
                                        i18n::tr(&app.config, "delete_permanently"),
                                        true,
                                    )
                                    .clicked()
                                    {
                                        confirm = true;
                                    }
                                },
                            );
                        });
                });
            window_frame::show_resize_handles(viewport_ctx);
        },
    );

    if confirm {
        app.confirm_permanent_delete = None;
        app.delete_panel_spawned = false;
        app.delete_paths(paths, true);
    } else if cancel {
        app.confirm_permanent_delete = None;
        app.delete_panel_spawned = false;
    }
}

fn paint_delete_titlebar(
    app: &BExplorerApp,
    ctx: &egui::Context,
    ui: &mut egui::Ui,
    close: &mut bool,
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

    let close_rect = egui::Rect::from_min_size(
        egui::pos2(rect.right() - TRANSFER_WINDOW_BUTTON, rect.top()),
        egui::vec2(TRANSFER_WINDOW_BUTTON, rect.height()),
    );
    let drag_rect = egui::Rect::from_min_max(rect.left_top(), close_rect.left_bottom());
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
        i18n::tr(&app.config, "confirm_delete"),
        theme::font(&app.config, 12.5),
        theme::text(&app.config),
    );

    let close_response = ui
        .allocate_rect(close_rect, egui::Sense::click())
        .on_hover_text(i18n::tr(&app.config, "cancel"));
    transfer::paint_transfer_titlebar_button(
        ui,
        &app.config,
        close_rect,
        "x",
        close_response.hovered(),
    );
    if close_response.clicked() {
        *close = true;
    }
}

fn permanent_delete_message(app: &BExplorerApp, paths: &[std::path::PathBuf]) -> String {
    let item = paths
        .first()
        .and_then(|path| path.file_name())
        .and_then(|value| value.to_str())
        .unwrap_or("archivo");
    if app.config.language == "es" {
        if paths.len() == 1 {
            format!(
                "Seguro que quieres eliminar permanentemente \"{item}\"? Esta accion no se puede deshacer."
            )
        } else {
            format!(
                "Seguro que quieres eliminar permanentemente {} elementos? Esta accion no se puede deshacer.",
                paths.len()
            )
        }
    } else if paths.len() == 1 {
        format!("Permanently delete \"{item}\"? This cannot be undone.")
    } else {
        format!(
            "Permanently delete {} item(s)? This cannot be undone.",
            paths.len()
        )
    }
}

fn show_archive_password_dialog(app: &mut BExplorerApp, ctx: &egui::Context) {
    if app.archive_password_dialog.is_none() {
        return;
    }

    let mut confirm = false;
    let mut cancel = false;
    let mut text_active = false;
    if ctx.input(|input| input.key_pressed(egui::Key::Enter)) {
        confirm = true;
    }
    if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        cancel = true;
    }

    egui::Area::new(egui::Id::new("archive_password_dialog"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .movable(true)
        .show(ctx, |ui| {
            floating_dialog_frame(&app.config).show(ui, |ui| {
                set_floating_dialog_width(ui, ARCHIVE_PASSWORD_DIALOG_WIDTH);
                paint_floating_dialog_header(
                    app,
                    ui,
                    ARCHIVE_PASSWORD_DIALOG_WIDTH,
                    i18n::tr(&app.config, "archive_password_required"),
                    DialogTitleAlign::Center,
                    &mut cancel,
                );

                let config = app.config.clone();
                if let Some(dialog) = app.archive_password_dialog.as_mut() {
                    floating_dialog_body(ui, ARCHIVE_PASSWORD_DIALOG_WIDTH, 14.0, 8.0, |ui| {
                        let message = if dialog
                            .error
                            .as_deref()
                            .is_some_and(|value| value == "Password is required")
                        {
                            i18n::tr(&config, "password_empty")
                        } else {
                            i18n::tr(&config, "archive_password_message")
                        };
                        ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                            ui.add_sized(
                                egui::vec2(ui.available_width().min(340.0), 0.0),
                                egui::Label::new(
                                    egui::RichText::new(message)
                                        .size(config.font_size)
                                        .color(theme::muted(&config)),
                                )
                                .wrap(),
                            );
                        });
                        ui.add_space(8.0);
                        let response = archive_password_input_row(
                            ui,
                            &config,
                            i18n::tr(&config, "password"),
                            &mut dialog.password,
                        );
                        text_active |= response.has_focus();
                        ui.add_space(8.0);
                        archive_password_button_row(&config, ui, &mut confirm, &mut cancel);
                    });
                }
            });
        });

    if text_active {
        app.mark_text_input_active();
    }
    if confirm {
        app.confirm_archive_password_dialog();
    } else if cancel {
        app.cancel_archive_password_dialog();
    }
}

fn archive_password_input_row(
    ui: &mut egui::Ui,
    config: &AppConfig,
    label: &str,
    value: &mut String,
) -> egui::Response {
    let label_width = 88.0;
    let spacing = 12.0;
    let row_width = ui.available_width().min(318.0);
    let field_width = (row_width - label_width - spacing).max(210.0);
    let indent = ((ui.available_width() - row_width) * 0.5).max(0.0);
    let mut output = None;

    ui.horizontal(|ui| {
        ui.add_space(indent);
        ui.add_sized(
            egui::vec2(label_width, 28.0),
            egui::Label::new(
                egui::RichText::new(label)
                    .color(theme::muted(config))
                    .size(config.font_size),
            ),
        );
        ui.add_space(spacing);
        ui.allocate_ui_with_layout(
            egui::vec2(field_width, 28.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_size(egui::vec2(field_width, 28.0));
                output = Some(compress_password_field(ui, config, value));
            },
        );
    });

    output.expect("password field was painted")
}

fn archive_password_button_row(
    config: &AppConfig,
    ui: &mut egui::Ui,
    confirm: &mut bool,
    cancel: &mut bool,
) {
    let extract_width = button_width_for_label(i18n::tr(config, "extract"));
    let cancel_width = button_width_for_label(i18n::tr(config, "cancel"));
    let group_width = extract_width + 6.0 + cancel_width;
    let indent = (ui.available_width() - group_width).max(0.0);
    ui.horizontal(|ui| {
        ui.add_space(indent);
        if transfer::styled_button(ui, config, i18n::tr(config, "extract"), true).clicked() {
            *confirm = true;
        }
        ui.add_space(6.0);
        if transfer::styled_button(ui, config, i18n::tr(config, "cancel"), false).clicked() {
            *cancel = true;
        }
    });
}

fn button_width_for_label(label: &str) -> f32 {
    (label.chars().count() as f32 * 7.2 + 24.0).clamp(34.0, 210.0)
}

fn show_compress_dialog(app: &mut BExplorerApp, ctx: &egui::Context) {
    if app.compress_dialog.is_none() {
        return;
    }

    let mut confirm = false;
    let mut cancel = false;
    let mut text_active = false;
    if ctx.input(|input| input.key_pressed(egui::Key::Enter)) {
        confirm = true;
    }
    if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        cancel = true;
    }

    egui::Area::new(egui::Id::new("compress_dialog"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .movable(true)
        .show(ctx, |ui| {
            floating_dialog_frame(&app.config).show(ui, |ui| {
                set_floating_dialog_width(ui, COMPRESS_DIALOG_WIDTH);
                paint_compress_header(app, ui, &mut cancel);

                if let Some(dialog) = app.compress_dialog.as_mut() {
                    floating_dialog_body(ui, COMPRESS_DIALOG_WIDTH, 14.0, 12.0, |ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(8.0, 9.0);
                        paint_compress_summary(ui, &app.config, dialog.sources.len());

                        compress_option_row(
                            ui,
                            &app.config,
                            i18n::tr(&app.config, "archive_name"),
                            |ui| {
                                let response =
                                    compress_text_field(ui, &app.config, &mut dialog.name);
                                text_active |= response.has_focus();
                            },
                        );

                        compress_option_row(
                            ui,
                            &app.config,
                            i18n::tr(&app.config, "format"),
                            |ui| {
                                compress_format_picker(ui, &app.config, &mut dialog.format);
                            },
                        );

                        compress_option_row(
                            ui,
                            &app.config,
                            i18n::tr(&app.config, "compression_method"),
                            |ui| {
                                compress_method_picker(ui, &app.config, &mut dialog.method);
                            },
                        );

                        compress_option_row(
                            ui,
                            &app.config,
                            i18n::tr(&app.config, "password"),
                            |ui| {
                                let response =
                                    compress_password_field(ui, &app.config, &mut dialog.password);
                                text_active |= response.has_focus();
                            },
                        );

                        compress_option_row(
                            ui,
                            &app.config,
                            i18n::tr(&app.config, "repeat_password"),
                            |ui| {
                                let response = compress_password_field(
                                    ui,
                                    &app.config,
                                    &mut dialog.confirm_password,
                                );
                                text_active |= response.has_focus();
                            },
                        );

                        if dialog.password == dialog.confirm_password {
                            dialog.password_mismatch = false;
                        }
                        let show_password_mismatch = dialog.password != dialog.confirm_password
                            && (dialog.password_mismatch || !dialog.confirm_password.is_empty());
                        if show_password_mismatch {
                            paint_compress_validation_label(
                                ui,
                                &app.config,
                                i18n::tr(&app.config, "passwords_do_not_match"),
                            );
                        }

                        ui.add_space(2.0);
                    });

                    ui.add_space(2.0);
                    floating_dialog_body(ui, COMPRESS_DIALOG_WIDTH, 14.0, 0.0, |ui| {
                        floating_dialog_button_row(ui, |ui| {
                            if transfer::styled_button(
                                ui,
                                &app.config,
                                i18n::tr(&app.config, "cancel"),
                                false,
                            )
                            .clicked()
                            {
                                cancel = true;
                            }
                            ui.add_space(6.0);
                            if transfer::styled_button(
                                ui,
                                &app.config,
                                i18n::tr(&app.config, "compress"),
                                true,
                            )
                            .clicked()
                            {
                                confirm = true;
                            }
                        });
                    });
                    ui.add_space(14.0);
                }
            });
        });

    if text_active {
        app.mark_text_input_active();
    }
    if confirm {
        app.confirm_compress_dialog();
    } else if cancel {
        app.cancel_compress_dialog();
    }
}

fn paint_compress_header(app: &BExplorerApp, ui: &mut egui::Ui, close: &mut bool) {
    paint_floating_dialog_header(
        app,
        ui,
        COMPRESS_DIALOG_WIDTH,
        i18n::tr(&app.config, "compress"),
        DialogTitleAlign::Left,
        close,
    );
}

fn compress_option_row(
    ui: &mut egui::Ui,
    config: &AppConfig,
    label: &str,
    contents: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        let available = ui.available_width().max(0.0);
        let spacing = ui.spacing().item_spacing.x;
        let field_width = (available - 124.0 - spacing).clamp(220.0, 252.0);
        let label_width = (available - field_width - spacing).clamp(112.0, 144.0);
        ui.add_sized(
            egui::vec2(label_width, 28.0),
            egui::Label::new(
                egui::RichText::new(label)
                    .color(theme::muted(config))
                    .size(config.font_size),
            ),
        );
        ui.allocate_ui_with_layout(
            egui::vec2(field_width, 28.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_size(egui::vec2(field_width, 28.0));
                contents(ui);
            },
        );
    });
}

fn paint_compress_field_frame(ui: &mut egui::Ui, config: &AppConfig, rect: egui::Rect) {
    ui.painter()
        .rect_filled(rect, 6.0, theme::surface_elevated(config));
    ui.painter()
        .rect_stroke(rect, 6.0, egui::Stroke::new(1.0, theme::stroke(config)));
    ui.painter().line_segment(
        [
            egui::pos2(rect.left() + 6.0, rect.bottom() - 0.5),
            egui::pos2(rect.right() - 6.0, rect.bottom() - 0.5),
        ],
        egui::Stroke::new(1.0, theme::subtle_stroke(config)),
    );
}

fn paint_compress_segmented_background(ui: &mut egui::Ui, config: &AppConfig, rect: egui::Rect) {
    paint_compress_field_frame(ui, config, rect);
    ui.painter().rect_stroke(
        rect.shrink(0.5),
        6.0,
        egui::Stroke::new(1.0, theme::subtle_stroke(config)),
    );
}

fn compress_segment_rect(container: egui::Rect, index: usize, count: usize) -> egui::Rect {
    let width = container.width() / count as f32;
    egui::Rect::from_min_max(
        egui::pos2(container.left() + width * index as f32, container.top()),
        egui::pos2(
            container.left() + width * (index + 1) as f32,
            container.bottom(),
        ),
    )
}

fn paint_compress_segment(
    ui: &mut egui::Ui,
    config: &AppConfig,
    rect: egui::Rect,
    label: &str,
    hover: &str,
    selected: bool,
    on_click: impl FnOnce(),
) {
    let response = ui.allocate_rect(rect, egui::Sense::click());
    if selected {
        theme::paint_selection_gradient(ui.painter(), rect.shrink(2.0), config);
    } else if response.hovered() {
        theme::paint_hover_gradient(ui.painter(), rect.shrink(2.0), 4.0, config);
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        theme::font(config, 11.0),
        if selected {
            egui::Color32::WHITE
        } else {
            theme::text(config)
        },
    );
    if response.on_hover_text(hover).clicked() {
        on_click();
    }
}

fn paint_compress_segment_separator(ui: &mut egui::Ui, config: &AppConfig, rect: egui::Rect) {
    ui.painter().line_segment(
        [
            egui::pos2(rect.right(), rect.top() + 5.0),
            egui::pos2(rect.right(), rect.bottom() - 5.0),
        ],
        egui::Stroke::new(1.0, theme::subtle_stroke(config)),
    );
}

fn paint_compress_summary(ui: &mut egui::Ui, config: &AppConfig, count: usize) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), egui::Sense::hover());
    paint_compress_field_frame(ui, config, rect);
    let accent = egui::Rect::from_min_max(
        rect.left_top(),
        egui::pos2(rect.left() + 3.0, rect.bottom()),
    );
    theme::paint_selection_gradient(ui.painter(), accent, config);
    ui.painter().text(
        egui::pos2(rect.left() + 12.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        compress_selection_summary(config, count),
        theme::font(config, 11.5),
        theme::muted(config),
    );
}

fn paint_compress_validation_label(ui: &mut egui::Ui, config: &AppConfig, text: &str) {
    let available = ui.available_width().max(0.0);
    let label_width = 144.0;
    let field_width = 252.0;
    let spacing = ui.spacing().item_spacing.x;
    let row_width = (label_width + spacing + field_width).min(available);
    let indent = (available - row_width).max(0.0);
    ui.horizontal(|ui| {
        ui.add_space(indent + label_width + spacing);
        ui.add_sized(
            egui::vec2(field_width.min(ui.available_width()), 18.0),
            egui::Label::new(
                egui::RichText::new(text)
                    .color(theme::accent(config))
                    .size((config.font_size - 0.5).max(10.0)),
            ),
        );
    });
}

fn compress_text_field(
    ui: &mut egui::Ui,
    config: &AppConfig,
    value: &mut String,
) -> egui::Response {
    compress_text_field_inner(ui, config, value, false)
}

fn compress_password_field(
    ui: &mut egui::Ui,
    config: &AppConfig,
    value: &mut String,
) -> egui::Response {
    compress_text_field_inner(ui, config, value, true)
}

fn compress_text_field_inner(
    ui: &mut egui::Ui,
    config: &AppConfig,
    value: &mut String,
    password: bool,
) -> egui::Response {
    let width = ui.available_width().clamp(220.0, 252.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 28.0), egui::Sense::click());
    paint_compress_field_frame(ui, config, rect);
    if response.hovered() {
        theme::paint_hover_gradient(ui.painter(), rect.shrink(1.0), 6.0, config);
    }
    let eye_rect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 15.0, rect.center().y),
        egui::vec2(20.0, 20.0),
    );
    let reveal_password = password
        && ui.input(|input| {
            input.pointer.primary_down()
                && input
                    .pointer
                    .interact_pos()
                    .is_some_and(|pos| eye_rect.contains(pos))
        });
    let edit_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 9.0, rect.top() + 2.0),
        egui::pos2(
            rect.right() - if password { 33.0 } else { 9.0 },
            rect.bottom() - 2.0,
        ),
    );
    let text_response = ui
        .allocate_new_ui(egui::UiBuilder::new().max_rect(edit_rect), |ui| {
            ui.set_clip_rect(edit_rect);
            ui.add_sized(
                egui::vec2(edit_rect.width().max(0.0), 24.0),
                egui::TextEdit::singleline(value)
                    .password(password && !reveal_password)
                    .font(theme::font(config, config.font_size))
                    .text_color(theme::text(config))
                    .frame(false),
            )
        })
        .inner;
    if password {
        let eye_response = ui
            .allocate_rect(eye_rect, egui::Sense::click())
            .on_hover_text(i18n::tr(config, "hold_to_show_password"));
        if reveal_password {
            ui.ctx().request_repaint();
        }
        paint_password_eye_icon(
            ui.painter(),
            eye_rect,
            if reveal_password || eye_response.hovered() {
                theme::accent(config)
            } else {
                theme::muted(config)
            },
        );
    }
    text_response
}

fn paint_password_eye_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let center = rect.center();
    let stroke = egui::Stroke::new(1.25, color);
    let left = egui::pos2(rect.left() + 3.5, center.y);
    let right = egui::pos2(rect.right() - 3.5, center.y);
    let top = egui::pos2(center.x, rect.top() + 5.0);
    let bottom = egui::pos2(center.x, rect.bottom() - 5.0);
    painter.line_segment([left, top], stroke);
    painter.line_segment([top, right], stroke);
    painter.line_segment([right, bottom], stroke);
    painter.line_segment([bottom, left], stroke);
    painter.circle_filled(center, 2.4, color);
}

fn compress_format_picker(ui: &mut egui::Ui, config: &AppConfig, value: &mut ArchiveFormat) {
    let width = ui.available_width().clamp(220.0, 252.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 28.0), egui::Sense::hover());
    paint_compress_segmented_background(ui, config, rect);
    let zip_rect = compress_segment_rect(rect, 0, 2);
    let seven_zip_rect = compress_segment_rect(rect, 1, 2);
    paint_compress_segment(
        ui,
        config,
        zip_rect,
        "ZIP",
        "ZIP (.zip)",
        *value == ArchiveFormat::Zip,
        || *value = ArchiveFormat::Zip,
    );
    paint_compress_segment_separator(ui, config, zip_rect);
    paint_compress_segment(
        ui,
        config,
        seven_zip_rect,
        "7z",
        "7z (.7z)",
        *value == ArchiveFormat::SevenZip,
        || *value = ArchiveFormat::SevenZip,
    );
}

fn compress_method_picker(
    ui: &mut egui::Ui,
    config: &AppConfig,
    value: &mut ArchiveCompressionMethod,
) {
    if *value == ArchiveCompressionMethod::Store {
        *value = ArchiveCompressionMethod::Normal;
    }
    let methods = [
        ArchiveCompressionMethod::Fast,
        ArchiveCompressionMethod::Normal,
        ArchiveCompressionMethod::Maximum,
    ];
    let width = ui.available_width().clamp(220.0, 252.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 28.0), egui::Sense::hover());
    paint_compress_segmented_background(ui, config, rect);
    for (index, method) in methods.into_iter().enumerate() {
        let segment_rect = compress_segment_rect(rect, index, 3);
        paint_compress_segment(
            ui,
            config,
            segment_rect,
            compression_method_short_label(config, method),
            compression_method_label(config, method),
            *value == method,
            || *value = method,
        );
        if index + 1 < methods.len() {
            paint_compress_segment_separator(ui, config, segment_rect);
        }
    }
}

fn compress_selection_summary(config: &AppConfig, count: usize) -> String {
    if config.language == "es" {
        if count == 1 {
            "1 elemento seleccionado".to_string()
        } else {
            format!("{count} elementos seleccionados")
        }
    } else if count == 1 {
        "1 selected item".to_string()
    } else {
        format!("{count} selected items")
    }
}

fn compression_method_label(config: &AppConfig, method: ArchiveCompressionMethod) -> &'static str {
    match method {
        ArchiveCompressionMethod::Store => i18n::tr(config, "compression_store"),
        ArchiveCompressionMethod::Fast => i18n::tr(config, "compression_fast"),
        ArchiveCompressionMethod::Normal => i18n::tr(config, "compression_normal"),
        ArchiveCompressionMethod::Maximum => i18n::tr(config, "compression_maximum"),
    }
}

fn compression_method_short_label(
    config: &AppConfig,
    method: ArchiveCompressionMethod,
) -> &'static str {
    let spanish = config.language == "es";
    match (spanish, method) {
        (true, ArchiveCompressionMethod::Store) => "Sin",
        (true, ArchiveCompressionMethod::Fast) => "Rapida",
        (true, ArchiveCompressionMethod::Normal) => "Normal",
        (true, ArchiveCompressionMethod::Maximum) => "Maxima",
        (false, ArchiveCompressionMethod::Store) => "Store",
        (false, ArchiveCompressionMethod::Fast) => "Fast",
        (false, ArchiveCompressionMethod::Normal) => "Normal",
        (false, ArchiveCompressionMethod::Maximum) => "Max",
    }
}

fn show_options(app: &mut BExplorerApp, ctx: &egui::Context) {
    if !app.options_open {
        return;
    }

    let mut changed = false;
    let mut refresh_needed = false;
    let mut close = false;

    egui::Area::new(egui::Id::new("options_panel"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .movable(true)
        .show(ctx, |ui| {
            floating_dialog_frame(&app.config).show(ui, |ui| {
                set_floating_dialog_width(ui, OPTIONS_DIALOG_WIDTH);
                paint_options_header(app, ui, &mut close);
                ui.add_space(8.0);

                floating_dialog_body(ui, OPTIONS_DIALOG_WIDTH, 12.0, 0.0, |ui| {
                    let max_height = (ctx.screen_rect().height() - 150.0).clamp(280.0, 680.0);
                    egui::ScrollArea::vertical()
                        .id_salt("options_scroll")
                        .max_height(max_height)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);

                            let language_config = app.config.clone();
                            let language_row_label = i18n::tr(&language_config, "language");
                            let english = i18n::tr(&language_config, "english");
                            let spanish = i18n::tr(&language_config, "spanish");
                            option_row(ui, &language_config, language_row_label, |ui| {
                                egui::ComboBox::from_id_salt("options_language")
                                    .selected_text(language_label(&language_config))
                                    .width(170.0)
                                    .show_ui(ui, |ui| {
                                        changed |= ui
                                            .selectable_value(
                                                &mut app.config.language,
                                                "en".to_string(),
                                                english,
                                            )
                                            .changed();
                                        changed |= ui
                                            .selectable_value(
                                                &mut app.config.language,
                                                "es".to_string(),
                                                spanish,
                                            )
                                            .changed();
                                    });
                            });

                            ui.separator();
                            ui.label(
                                egui::RichText::new(i18n::tr(&app.config, "personalization"))
                                    .size(app.config.font_size + 2.0)
                                    .color(theme::text(&app.config)),
                            );

                            let theme_config = app.config.clone();
                            let theme_row_label = i18n::tr(&theme_config, "theme");
                            let dark = i18n::tr(&theme_config, "dark");
                            let light = i18n::tr(&theme_config, "light");
                            option_row(ui, &theme_config, theme_row_label, |ui| {
                                egui::ComboBox::from_id_salt("options_theme")
                                    .selected_text(theme_label(&theme_config))
                                    .width(170.0)
                                    .show_ui(ui, |ui| {
                                        changed |= ui
                                            .selectable_value(
                                                &mut app.config.theme,
                                                ThemePreference::Dark,
                                                dark,
                                            )
                                            .changed();
                                        changed |= ui
                                            .selectable_value(
                                                &mut app.config.theme,
                                                ThemePreference::Light,
                                                light,
                                            )
                                            .changed();
                                    });
                            });

                            let font_config = app.config.clone();
                            let font_label = i18n::tr(&font_config, "font_size");
                            option_row(ui, &font_config, font_label, |ui| {
                                if ui.button("+").clicked() {
                                    app.config.font_size =
                                        (app.config.font_size + 0.5).clamp(10.0, 18.0);
                                    changed = true;
                                }
                                ui.label(format!("{:.1}", app.config.font_size));
                                if ui.button("-").clicked() {
                                    app.config.font_size =
                                        (app.config.font_size - 0.5).clamp(10.0, 18.0);
                                    changed = true;
                                }
                            });

                            let color_config = app.config.clone();
                            let color_label = i18n::tr(&color_config, "selection_color");
                            option_row(ui, &color_config, color_label, |ui| {
                                changed |= ui
                                    .color_edit_button_srgb(&mut app.config.accent_color)
                                    .changed();
                            });

                            let icon_borders_config = app.config.clone();
                            let icon_borders_label = i18n::tr(&icon_borders_config, "icon_borders");
                            option_row(ui, &icon_borders_config, icon_borders_label, |ui| {
                                if ui.checkbox(&mut app.config.show_icon_borders, "").changed() {
                                    changed = true;
                                }
                            });

                            let vibrancy_config = app.config.clone();
                            let vibrancy_label = i18n::tr(&vibrancy_config, "vibrancy");
                            let prev_vibrancy = app.config.vibrancy;
                            option_row(ui, &vibrancy_config, vibrancy_label, |ui| {
                                let current_mode = match app.config.vibrancy {
                                    VibrancyMode::None => i18n::tr(&vibrancy_config, "none"),
                                    VibrancyMode::Mica => i18n::tr(&vibrancy_config, "mica"),
                                    VibrancyMode::Acrylic => i18n::tr(&vibrancy_config, "acrylic"),
                                    VibrancyMode::Blur => i18n::tr(&vibrancy_config, "blur"),
                                };
                                egui::ComboBox::from_id_salt("options_vibrancy")
                                    .selected_text(current_mode)
                                    .width(170.0)
                                    .show_ui(ui, |ui| {
                                        changed |= ui
                                            .selectable_value(
                                                &mut app.config.vibrancy,
                                                VibrancyMode::None,
                                                i18n::tr(&vibrancy_config, "none"),
                                            )
                                            .changed();
                                        changed |= ui
                                            .selectable_value(
                                                &mut app.config.vibrancy,
                                                VibrancyMode::Mica,
                                                i18n::tr(&vibrancy_config, "mica"),
                                            )
                                            .changed();
                                        changed |= ui
                                            .selectable_value(
                                                &mut app.config.vibrancy,
                                                VibrancyMode::Acrylic,
                                                i18n::tr(&vibrancy_config, "acrylic"),
                                            )
                                            .changed();
                                    });
                                if prev_vibrancy != app.config.vibrancy {
                                    app.vibrancy_dirty = true;
                                }
                            });

                            let intensity_config = app.config.clone();
                            let intensity_label = i18n::tr(&intensity_config, "vibrancy_intensity");
                            option_row(ui, &intensity_config, intensity_label, |ui| {
                                changed |= ui
                                    .add(
                                        egui::Slider::new(
                                            &mut app.config.vibrancy_intensity,
                                            0..=100,
                                        )
                                        .text("%")
                                        .show_value(true),
                                    )
                                    .changed();
                            });

                            ui.separator();
                            ui.label(
                                egui::RichText::new(i18n::tr(&app.config, "files"))
                                    .size(app.config.font_size + 2.0)
                                    .color(theme::text(&app.config)),
                            );

                            let view_config = app.config.clone();
                            let view_label = i18n::tr(&view_config, "default_view");
                            option_row(ui, &view_config, view_label, |ui| {
                                egui::ComboBox::from_id_salt("options_default_view")
                                    .selected_text(i18n::view_mode_label(
                                        &view_config,
                                        &view_config.default_view,
                                    ))
                                    .width(170.0)
                                    .show_ui(ui, |ui| {
                                        for view_mode in ViewMode::ALL {
                                            changed |= ui
                                                .selectable_value(
                                                    &mut app.config.default_view,
                                                    view_mode,
                                                    i18n::view_mode_label(&view_config, &view_mode),
                                                )
                                                .changed();
                                        }
                                    });
                            });

                            let hidden_config = app.config.clone();
                            let hidden_label = i18n::tr(&hidden_config, "show_hidden");
                            option_row(ui, &hidden_config, hidden_label, |ui| {
                                let response = ui.checkbox(&mut app.config.show_hidden, "");
                                if response.changed() {
                                    changed = true;
                                    refresh_needed = true;
                                }
                            });

                            let extensions_config = app.config.clone();
                            let extensions_label = i18n::tr(&extensions_config, "show_extensions");
                            option_row(ui, &extensions_config, extensions_label, |ui| {
                                if ui.checkbox(&mut app.config.show_extensions, "").changed() {
                                    changed = true;
                                }
                            });
                        });
                });

                ui.add_space(12.0);
            });
        });

    if close {
        app.options_open = false;
    }

    if changed {
        app.config.font_size = app.config.font_size.clamp(10.0, 18.0);
        theme::apply(ctx, &app.config);
        app.save_config();
        if refresh_needed {
            app.refresh_active_tab();
        }
        ctx.request_repaint();
    }
}

fn show_shortcuts(app: &mut BExplorerApp, ctx: &egui::Context) {
    if !app.shortcuts_open {
        return;
    }

    let mut changed = false;
    let mut close = false;
    if app.shortcut_capture.is_some() {
        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            app.shortcut_capture = None;
        } else if let Some(binding) = shortcut_binding_from_input(ctx) {
            if let Some(action) = app.shortcut_capture.take() {
                app.config.shortcuts.set_binding(action, binding);
                changed = true;
            }
        }
    } else if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        close = true;
    }

    egui::Area::new(egui::Id::new("shortcuts_panel"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .movable(true)
        .show(ctx, |ui| {
            floating_dialog_frame(&app.config).show(ui, |ui| {
                set_floating_dialog_width(ui, SHORTCUTS_DIALOG_WIDTH);
                paint_floating_dialog_header(
                    app,
                    ui,
                    SHORTCUTS_DIALOG_WIDTH,
                    i18n::tr(&app.config, "custom_shortcuts"),
                    DialogTitleAlign::Center,
                    &mut close,
                );
                ui.add_space(8.0);

                floating_dialog_body(ui, SHORTCUTS_DIALOG_WIDTH, 14.0, 0.0, |ui| {
                    let max_height = (ctx.screen_rect().height() - 150.0).clamp(260.0, 620.0);
                    egui::ScrollArea::vertical()
                        .id_salt("shortcuts_scroll")
                        .max_height(max_height)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                            for action in CUSTOMIZABLE_SHORTCUT_ACTIONS {
                                shortcut_option_row(ui, app, action);
                            }

                            ui.separator();
                            let restore_config = app.config.clone();
                            option_row(
                                ui,
                                &restore_config,
                                i18n::tr(&restore_config, "restore_defaults"),
                                |ui| {
                                    if shortcut_button(
                                        ui,
                                        &restore_config,
                                        i18n::tr(&restore_config, "restore"),
                                        false,
                                    )
                                    .clicked()
                                    {
                                        app.config.shortcuts = ShortcutConfig::default();
                                        app.shortcut_capture = None;
                                        changed = true;
                                    }
                                },
                            );
                        });
                });

                ui.add_space(12.0);
            });
        });

    if close {
        app.shortcuts_open = false;
        app.shortcut_capture = None;
    }

    if changed {
        app.save_config();
        ctx.request_repaint();
    }
}

fn paint_options_header(app: &BExplorerApp, ui: &mut egui::Ui, close: &mut bool) {
    paint_floating_dialog_header(
        app,
        ui,
        OPTIONS_DIALOG_WIDTH,
        i18n::tr(&app.config, "options"),
        DialogTitleAlign::Center,
        close,
    );
}

fn option_row(
    ui: &mut egui::Ui,
    config: &AppConfig,
    label: &str,
    contents: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        ui.set_min_width(364.0);
        ui.label(
            egui::RichText::new(label)
                .color(theme::muted(config))
                .size(config.font_size),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            contents(ui);
        });
    });
}

fn shortcut_option_row(ui: &mut egui::Ui, app: &mut BExplorerApp, action: ShortcutAction) {
    let row_config = app.config.clone();
    let label = shortcut_action_label(&row_config, action);
    option_row(ui, &row_config, label, |ui| {
        let capturing = app.shortcut_capture == Some(action);
        let text = if capturing {
            i18n::tr(&row_config, "press_shortcut").to_string()
        } else {
            shortcut_binding_label(app.config.shortcuts.binding(action))
        };
        if shortcut_button(ui, &row_config, &text, capturing).clicked() {
            app.shortcut_capture = Some(action);
        }
    });
}

fn shortcut_button(
    ui: &mut egui::Ui,
    config: &AppConfig,
    label: &str,
    capturing: bool,
) -> egui::Response {
    let width = 170.0_f32.min(ui.available_width().max(90.0));
    let height = 28.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    let fill = if capturing {
        theme::accent(config)
    } else if response.hovered() {
        theme::hover(config)
    } else {
        theme::surface_elevated(config)
    };
    ui.painter().rect_filled(rect, 5.0, fill);
    ui.painter().rect_stroke(
        rect.shrink(0.5),
        5.0,
        egui::Stroke::new(1.0, theme::stroke(config)),
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        theme::font(config, 11.4),
        if capturing {
            egui::Color32::WHITE
        } else {
            theme::text(config)
        },
    );
    response.on_hover_text(label)
}

fn shortcut_action_label(config: &AppConfig, action: ShortcutAction) -> &'static str {
    match action {
        ShortcutAction::CommandPalette => i18n::tr(config, "shortcut_command_palette"),
        ShortcutAction::Copy => i18n::tr(config, "copy"),
        ShortcutAction::Cut => i18n::tr(config, "cut"),
        ShortcutAction::Paste => i18n::tr(config, "paste"),
        ShortcutAction::SelectAll => i18n::tr(config, "shortcut_select_all"),
        ShortcutAction::Refresh => i18n::tr(config, "refresh"),
        ShortcutAction::Rename => i18n::tr(config, "rename"),
        ShortcutAction::Delete => i18n::tr(config, "delete"),
        ShortcutAction::PermanentDelete => i18n::tr(config, "shortcut_permanent_delete"),
        ShortcutAction::Properties => i18n::tr(config, "properties"),
        ShortcutAction::GoUp => i18n::tr(config, "shortcut_go_up"),
        ShortcutAction::GoBack => i18n::tr(config, "shortcut_go_back"),
        ShortcutAction::GoForward => i18n::tr(config, "shortcut_go_forward"),
        ShortcutAction::Open => i18n::tr(config, "open"),
        ShortcutAction::MoveUp => i18n::tr(config, "shortcut_move_up"),
        ShortcutAction::MoveDown => i18n::tr(config, "shortcut_move_down"),
    }
}

fn language_label(config: &AppConfig) -> &'static str {
    match config.language.as_str() {
        "es" => i18n::tr(config, "spanish"),
        _ => i18n::tr(config, "english"),
    }
}

fn theme_label(config: &AppConfig) -> &'static str {
    match config.theme {
        ThemePreference::Dark => i18n::tr(config, "dark"),
        ThemePreference::Light | ThemePreference::Gray => i18n::tr(config, "light"),
    }
}
