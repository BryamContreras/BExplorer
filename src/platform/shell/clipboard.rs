use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use crate::utils::errors::{BExplorerError, Result};

// X11 and Wayland keep clipboard contents in the process that owns them. Keep
// arboard alive for the application lifetime so other applications can request
// the data after the copy action has returned.
static SYSTEM_CLIPBOARD: OnceLock<Mutex<Option<arboard::Clipboard>>> = OnceLock::new();

fn with_clipboard<T>(
    operation: impl FnOnce(&mut arboard::Clipboard) -> std::result::Result<T, arboard::Error>,
) -> Result<T> {
    let mut clipboard = SYSTEM_CLIPBOARD
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| BExplorerError::Clipboard("System clipboard lock was poisoned".into()))?;

    if clipboard.is_none() {
        *clipboard = Some(
            arboard::Clipboard::new()
                .map_err(|error| BExplorerError::Clipboard(error.to_string()))?,
        );
    }

    operation(
        clipboard
            .as_mut()
            .ok_or_else(|| BExplorerError::Clipboard("Clipboard initialization failed".into()))?,
    )
    .map_err(|error| BExplorerError::Clipboard(error.to_string()))
}

pub(super) fn set_text(text: &str) -> Result<()> {
    with_clipboard(|clipboard| clipboard.set_text(text.to_owned()))
}

pub(super) fn text() -> Result<String> {
    with_clipboard(arboard::Clipboard::get_text)
}

pub(super) fn set_file_list(paths: &[PathBuf]) -> Result<()> {
    with_clipboard(|clipboard| clipboard.set().file_list(paths))
}

pub(super) fn file_list() -> Result<Vec<PathBuf>> {
    with_clipboard(|clipboard| clipboard.get().file_list())
}

pub(super) fn clear() -> Result<()> {
    with_clipboard(arboard::Clipboard::clear)
}
