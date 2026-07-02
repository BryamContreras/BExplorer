use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::app::config::ViewMode;
use crate::utils::errors::Result;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TabState {
    pub path: Option<PathBuf>,
    pub title: String,
    pub history: Vec<Option<PathBuf>>,
    pub history_index: usize,
    #[serde(default = "default_view_mode")]
    pub view_mode: ViewMode,
}

impl TabState {
    pub fn new(path: Option<PathBuf>) -> Self {
        Self::with_view_mode(path, default_view_mode())
    }

    pub fn with_view_mode(path: Option<PathBuf>, view_mode: ViewMode) -> Self {
        let title = tab_title(path.as_ref());
        Self {
            path: path.clone(),
            title,
            history: vec![path],
            history_index: 0,
            view_mode,
        }
    }

    pub fn navigate_to(&mut self, path: Option<PathBuf>) {
        if self.path == path {
            return;
        }

        self.path = path.clone();
        self.title = tab_title(path.as_ref());

        if self.history_index + 1 < self.history.len() {
            self.history.truncate(self.history_index + 1);
        }

        self.history.push(path);
        self.history_index = self.history.len().saturating_sub(1);
    }

    pub fn go_back(&mut self) -> bool {
        if self.history_index == 0 {
            return false;
        }
        self.history_index -= 1;
        self.path = self.history.get(self.history_index).cloned().flatten();
        self.title = tab_title(self.path.as_ref());
        true
    }

    pub fn go_forward(&mut self) -> bool {
        if self.history_index + 1 >= self.history.len() {
            return false;
        }
        self.history_index += 1;
        self.path = self.history.get(self.history_index).cloned().flatten();
        self.title = tab_title(self.path.as_ref());
        true
    }
}

fn default_view_mode() -> ViewMode {
    ViewMode::Details
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SplitSide {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SplitFocus {
    Primary,
    Secondary,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SplitSession {
    pub tab_a: usize,
    pub tab_b: usize,
    #[serde(default)]
    pub primary_tabs: Vec<usize>,
    #[serde(default)]
    pub secondary_tabs: Vec<usize>,
    pub focused: SplitFocus,
    pub ratio: f32,
    pub side: SplitSide,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppSession {
    pub tabs: Vec<TabState>,
    pub active_tab: usize,
    #[serde(default)]
    pub split: Option<SplitSession>,
}

impl Default for AppSession {
    fn default() -> Self {
        Self {
            tabs: vec![TabState::new(None)],
            active_tab: 0,
            split: None,
        }
    }
}

impl AppSession {
    pub fn load() -> Self {
        match try_load() {
            Ok(session) if !session.tabs.is_empty() => session,
            Ok(_) => Self::default(),
            Err(error) => {
                crate::utils::log::error(format!("Session load failed: {error}"));
                Self::default()
            }
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = crate::utils::paths::session_file()?;
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(path, text)?;
        Ok(())
    }
}

fn try_load() -> Result<AppSession> {
    let path = crate::utils::paths::session_file()?;
    if !path.exists() {
        return Ok(AppSession::default());
    }
    let text = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&text)?)
}

pub fn tab_title(path: Option<&PathBuf>) -> String {
    let Some(path) = path else {
        return "This PC".into();
    };

    if let Some(title) = crate::fs::explorer::virtual_title(path) {
        return title;
    }

    if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
        if !name.is_empty() {
            return name.to_string();
        }
    }

    path.display().to_string()
}
