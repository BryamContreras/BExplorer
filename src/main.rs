#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod fs;
mod platform;
mod preview;
mod ui;
mod utils;

use app::state::BExplorerApp;

fn main() -> eframe::Result<()> {
    utils::log::init_log_dir();

    if let Some(exit_code) = fs::archive::try_run_archive_helper_from_args() {
        std::process::exit(exit_code);
    }
    if let Some(exit_code) = fs::transfer_queue::try_run_elevated_transfer_helper_from_args() {
        std::process::exit(exit_code);
    }
    if let Some(exit_code) = fs::operations::try_run_elevated_operation_helper_from_args() {
        std::process::exit(exit_code);
    }
    if let Some(exit_code) = fs::defender::try_run_elevated_defender_helper_from_args() {
        std::process::exit(exit_code);
    }

    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size([1280.0, 760.0])
        .with_min_inner_size([900.0, 560.0])
        .with_resizable(true)
        .with_decorations(false)
        .with_transparent(true)
        .with_title("BExplorer");
    if let Some(icon) = app_icon() {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "BExplorer",
        options,
        Box::new(|cc| Ok(Box::new(BExplorerApp::new(cc)))),
    )
}

fn app_icon() -> Option<eframe::egui::IconData> {
    eframe::icon_data::from_png_bytes(include_bytes!("../assets/icons/appicon.png")).ok()
}
