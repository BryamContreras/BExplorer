use std::io::Write;

pub fn init_log_dir() {
    let _ = crate::utils::paths::config_dir();
}

pub fn error(message: impl AsRef<str>) {
    write_line("ERROR", message.as_ref());
}

#[allow(dead_code)]
pub fn info(message: impl AsRef<str>) {
    write_line("INFO", message.as_ref());
}

fn write_line(level: &str, message: &str) {
    let Ok(path) = crate::utils::paths::log_file() else {
        return;
    };

    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };

    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let _ = writeln!(file, "[{timestamp}] {level}: {message}");
}
