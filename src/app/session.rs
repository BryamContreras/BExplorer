use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::app::config::{GroupMode, ViewMode};
use crate::utils::errors::Result;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TabState {
    pub path: Option<PathBuf>,
    pub title: String,
    pub history: Vec<Option<PathBuf>>,
    pub history_index: usize,
    #[serde(default = "default_view_mode")]
    pub view_mode: ViewMode,
    #[serde(default = "default_group_mode")]
    pub group_mode: GroupMode,
    #[serde(default = "default_group_ascending")]
    pub group_ascending: bool,
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
            group_mode: default_group_mode(),
            group_ascending: default_group_ascending(),
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

fn default_group_mode() -> GroupMode {
    GroupMode::None
}

fn default_group_ascending() -> bool {
    true
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
    #[serde(default = "default_split_ratio")]
    pub ratio: f32,
    #[serde(default = "default_split_side")]
    pub side: SplitSide,
}

fn default_split_ratio() -> f32 {
    0.5
}

fn default_split_side() -> SplitSide {
    SplitSide::Right
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
            Ok(session) if !session.tabs.is_empty() => session.normalized(),
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
        crate::utils::atomic_file::write(&path, text.as_bytes())
    }

    fn normalized(mut self) -> Self {
        if self.tabs.is_empty() {
            return Self::default();
        }

        for tab in &mut self.tabs {
            if tab.history.is_empty() {
                tab.history.push(tab.path.clone());
            }
            tab.history_index = tab.history_index.min(tab.history.len().saturating_sub(1));
            tab.path = tab.history.get(tab.history_index).cloned().flatten();
            tab.title = tab_title(tab.path.as_ref());
        }
        self.active_tab = self.active_tab.min(self.tabs.len().saturating_sub(1));

        let tab_count = self.tabs.len();
        self.split = self.split.take().and_then(|mut split| {
            if tab_count < 2 {
                return None;
            }
            split.tab_a = split.tab_a.min(tab_count - 1);
            split.tab_b = split.tab_b.min(tab_count - 1);
            if split.tab_a == split.tab_b {
                split.tab_b = (0..tab_count).find(|index| *index != split.tab_a)?;
            }
            split.primary_tabs = normalized_tab_indices(split.primary_tabs, tab_count);
            split.secondary_tabs = normalized_tab_indices(split.secondary_tabs, tab_count);

            split.secondary_tabs.retain(|index| *index != split.tab_a);
            if !split.primary_tabs.contains(&split.tab_a) {
                split.primary_tabs.push(split.tab_a);
            }
            split.primary_tabs.retain(|index| *index != split.tab_b);
            if !split.secondary_tabs.contains(&split.tab_b) {
                split.secondary_tabs.push(split.tab_b);
            }
            for index in 0..tab_count {
                if !split.primary_tabs.contains(&index) && !split.secondary_tabs.contains(&index) {
                    split.primary_tabs.push(index);
                }
            }
            split.ratio = if split.ratio.is_finite() {
                split.ratio.clamp(0.24, 0.76)
            } else {
                default_split_ratio()
            };
            self.active_tab = split.tab_a;
            Some(split)
        });
        self
    }
}

fn normalized_tab_indices(indices: Vec<usize>, len: usize) -> Vec<usize> {
    let mut normalized = Vec::new();
    for index in indices {
        if index < len && !normalized.contains(&index) {
            normalized.push(index);
        }
    }
    normalized
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

    if let Some(name) = path.file_name().and_then(|name| name.to_str())
        && !name.is_empty()
    {
        return name.to_string();
    }

    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_roundtrip_preserves_split_tabs_and_history() {
        let mut first = TabState::new(Some(PathBuf::from("/one")));
        first.navigate_to(Some(PathBuf::from("/one/two")));
        let session = AppSession {
            tabs: vec![first, TabState::new(Some(PathBuf::from("/three")))],
            active_tab: 0,
            split: Some(SplitSession {
                tab_a: 0,
                tab_b: 1,
                primary_tabs: vec![0],
                secondary_tabs: vec![1],
                focused: SplitFocus::Secondary,
                ratio: 0.6,
                side: SplitSide::Right,
            }),
        };

        let json = serde_json::to_string(&session).expect("serialize session");
        let restored: AppSession = serde_json::from_str(&json).expect("deserialize session");
        let restored = restored.normalized();

        assert_eq!(restored.tabs[0].path, Some(PathBuf::from("/one/two")));
        assert_eq!(restored.tabs[0].history.len(), 2);
        let split = restored.split.expect("split session");
        assert_eq!(split.primary_tabs, vec![0]);
        assert_eq!(split.secondary_tabs, vec![1]);
        assert_eq!(split.focused, SplitFocus::Secondary);
    }

    #[test]
    fn invalid_split_indices_are_repaired_without_losing_tabs() {
        let session = AppSession {
            tabs: vec![
                TabState::new(None),
                TabState::new(None),
                TabState::new(None),
            ],
            active_tab: 99,
            split: Some(SplitSession {
                tab_a: 0,
                tab_b: 1,
                primary_tabs: vec![0, 0, 99],
                secondary_tabs: vec![0, 1, 1],
                focused: SplitFocus::Primary,
                ratio: f32::NAN,
                side: SplitSide::Right,
            }),
        }
        .normalized();

        let split = session.split.expect("normalized split");
        assert_eq!(split.primary_tabs, vec![0, 2]);
        assert_eq!(split.secondary_tabs, vec![1]);
        assert_eq!(split.ratio, 0.5);
    }
}
