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
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    if let Some(exit_code) = fs::transfer_queue::try_run_elevated_transfer_helper_from_args() {
        std::process::exit(exit_code);
    }
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    if let Some(exit_code) = fs::operations::try_run_elevated_delete_helper_from_args() {
        std::process::exit(exit_code);
    }
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    if let Some(exit_code) = fs::operations::try_run_elevated_file_action_helper_from_args() {
        std::process::exit(exit_code);
    }
    iced_ui::run(command_line_path())?;
    Ok(())
}

fn command_line_path() -> Option<std::path::PathBuf> {
    std::env::args_os()
        .skip(1)
        .find(|argument| argument != "--")
        .map(std::path::PathBuf::from)
}
