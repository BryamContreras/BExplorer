#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod fs;
mod iced_ui;
mod platform;
mod utils;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    utils::log::init_log_dir();

    if let Some(exit_code) = fs::archive::try_run_archive_helper_from_args() {
        std::process::exit(exit_code);
    }
    #[cfg(target_os = "windows")]
    if let Some(exit_code) = fs::defender::try_run_elevated_defender_helper_from_args() {
        std::process::exit(exit_code);
    }

    iced_ui::run()?;
    Ok(())
}
