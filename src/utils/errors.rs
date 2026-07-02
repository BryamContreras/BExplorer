use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum BExplorerError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Clipboard error: {0}")]
    Clipboard(String),

    #[error("Shell error: {0}")]
    Shell(String),

    #[error("Operation error: {0}")]
    Operation(String),

    #[error("Invalid path: {0}")]
    InvalidPath(PathBuf),
}

#[cfg(target_os = "windows")]
impl From<windows::core::Error> for BExplorerError {
    fn from(error: windows::core::Error) -> Self {
        Self::Operation(format!("Windows API error: {error}"))
    }
}

pub type Result<T> = std::result::Result<T, BExplorerError>;
