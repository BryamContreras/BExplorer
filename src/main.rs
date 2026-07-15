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
    #[cfg(target_os = "linux")]
    if let Some(exit_code) = fs::properties::try_run_elevated_helper_from_args() {
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
        .map(normalize_launch_path)
}

fn normalize_launch_path(path: std::path::PathBuf) -> std::path::PathBuf {
    if !path.is_file() {
        return path;
    }

    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(std::path::Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::normalize_launch_path;
    use std::path::PathBuf;

    #[test]
    fn launch_directory_is_preserved() {
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        assert_eq!(normalize_launch_path(directory.clone()), directory);
    }

    #[test]
    fn launch_file_navigates_to_its_parent() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        assert_eq!(normalize_launch_path(root.join("Cargo.toml")), root);
    }
}
