use eframe::egui;

use crate::app::commands::AppCommand;
use crate::app::state::BExplorerApp;
use crate::ui::i18n;

pub fn show(app: &mut BExplorerApp, ctx: &egui::Context) {
    if !app.command_palette_open {
        return;
    }

    let mut close = false;
    let mut command_to_run = None;

    egui::Window::new(i18n::tr(&app.config, "command_palette"))
        .collapsible(false)
        .resizable(false)
        .default_width(460.0)
        .anchor(egui::Align2::CENTER_TOP, [0.0, 90.0])
        .show(ctx, |ui| {
            let response = ui.text_edit_singleline(&mut app.command_query);
            response.request_focus();

            ui.separator();
            let query = app.command_query.to_lowercase();
            for command in AppCommand::all() {
                let label = command_label(app, *command);
                if query.is_empty() || fuzzy_match(&label.to_lowercase(), &query) {
                    if ui.button(label).clicked() {
                        command_to_run = Some(*command);
                        close = true;
                    }
                }
            }

            if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                close = true;
            }
        });

    if let Some(command) = command_to_run {
        app.run_command(command);
    }

    if close {
        app.command_palette_open = false;
        app.command_query.clear();
    }
}

fn command_label(app: &BExplorerApp, command: AppCommand) -> &'static str {
    if app.config.language == "es" {
        match command {
            AppCommand::NewTab => "Nueva pestana",
            AppCommand::CloseTab => "Cerrar pestana",
            AppCommand::CopyPath => "Copiar ruta actual",
            AppCommand::ToggleHidden => "Mostrar/ocultar archivos ocultos",
            AppCommand::ToggleTheme => "Cambiar tema",
            AppCommand::Refresh => "Refrescar",
            AppCommand::GoUp => "Subir carpeta",
            AppCommand::Rename => "Renombrar seleccionado",
        }
    } else {
        command.label()
    }
}

fn fuzzy_match(candidate: &str, query: &str) -> bool {
    let mut chars = candidate.chars();
    query.chars().all(|needle| chars.any(|item| item == needle))
}
