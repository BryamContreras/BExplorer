use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::utils::errors::Result;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum VibrancyMode {
    None,
    Mica,
    Acrylic,
    Blur,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ThemePreference {
    System,
    Dark,
    Light,
    Gray,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ViewMode {
    Details,
    List,
    SmallIcons,
    MediumIcons,
    LargeIcons,
    ExtraLargeIcons,
    Tiles,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum GroupMode {
    None,
    Name,
    Type,
    TotalSize,
    FreeSpace,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ShortcutAction {
    CommandPalette,
    Copy,
    Cut,
    Paste,
    Undo,
    SelectAll,
    Refresh,
    Rename,
    Delete,
    PermanentDelete,
    Properties,
    GoUp,
    GoBack,
    GoForward,
    EditAddress,
    Open,
    MoveUp,
    MoveDown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ShortcutBinding {
    pub key: String,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl ShortcutBinding {
    pub fn new(key: &str, ctrl: bool, alt: bool, shift: bool) -> Self {
        Self {
            key: key.into(),
            ctrl,
            alt,
            shift,
        }
    }
}

impl Default for ShortcutBinding {
    fn default() -> Self {
        Self::new("", false, false, false)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ShortcutConfig {
    pub command_palette: ShortcutBinding,
    pub copy: ShortcutBinding,
    pub cut: ShortcutBinding,
    pub paste: ShortcutBinding,
    #[serde(default = "default_undo_shortcut")]
    pub undo: ShortcutBinding,
    pub select_all: ShortcutBinding,
    pub refresh: ShortcutBinding,
    pub rename: ShortcutBinding,
    pub delete: ShortcutBinding,
    pub permanent_delete: ShortcutBinding,
    pub properties: ShortcutBinding,
    pub go_up: ShortcutBinding,
    pub go_back: ShortcutBinding,
    pub go_forward: ShortcutBinding,
    #[serde(default = "default_edit_address_shortcut")]
    pub edit_address: ShortcutBinding,
    pub open: ShortcutBinding,
    pub move_up: ShortcutBinding,
    pub move_down: ShortcutBinding,
}

impl ShortcutConfig {
    pub fn binding(&self, action: ShortcutAction) -> &ShortcutBinding {
        match action {
            ShortcutAction::CommandPalette => &self.command_palette,
            ShortcutAction::Copy => &self.copy,
            ShortcutAction::Cut => &self.cut,
            ShortcutAction::Paste => &self.paste,
            ShortcutAction::Undo => &self.undo,
            ShortcutAction::SelectAll => &self.select_all,
            ShortcutAction::Refresh => &self.refresh,
            ShortcutAction::Rename => &self.rename,
            ShortcutAction::Delete => &self.delete,
            ShortcutAction::PermanentDelete => &self.permanent_delete,
            ShortcutAction::Properties => &self.properties,
            ShortcutAction::GoUp => &self.go_up,
            ShortcutAction::GoBack => &self.go_back,
            ShortcutAction::GoForward => &self.go_forward,
            ShortcutAction::EditAddress => &self.edit_address,
            ShortcutAction::Open => &self.open,
            ShortcutAction::MoveUp => &self.move_up,
            ShortcutAction::MoveDown => &self.move_down,
        }
    }

    pub fn set_binding(&mut self, action: ShortcutAction, binding: ShortcutBinding) {
        match action {
            ShortcutAction::CommandPalette => self.command_palette = binding,
            ShortcutAction::Copy => self.copy = binding,
            ShortcutAction::Cut => self.cut = binding,
            ShortcutAction::Paste => self.paste = binding,
            ShortcutAction::Undo => self.undo = binding,
            ShortcutAction::SelectAll => self.select_all = binding,
            ShortcutAction::Refresh => self.refresh = binding,
            ShortcutAction::Rename => self.rename = binding,
            ShortcutAction::Delete => self.delete = binding,
            ShortcutAction::PermanentDelete => self.permanent_delete = binding,
            ShortcutAction::Properties => self.properties = binding,
            ShortcutAction::GoUp => self.go_up = binding,
            ShortcutAction::GoBack => self.go_back = binding,
            ShortcutAction::GoForward => self.go_forward = binding,
            ShortcutAction::EditAddress => self.edit_address = binding,
            ShortcutAction::Open => self.open = binding,
            ShortcutAction::MoveUp => self.move_up = binding,
            ShortcutAction::MoveDown => self.move_down = binding,
        }
    }
}

impl Default for ShortcutConfig {
    fn default() -> Self {
        Self {
            command_palette: ShortcutBinding::new("P", true, false, false),
            copy: ShortcutBinding::new("C", true, false, false),
            cut: ShortcutBinding::new("X", true, false, false),
            paste: ShortcutBinding::new("V", true, false, false),
            undo: ShortcutBinding::new("Z", true, false, false),
            select_all: ShortcutBinding::new("A", true, false, false),
            refresh: ShortcutBinding::new("F5", false, false, false),
            rename: ShortcutBinding::new("F2", false, false, false),
            delete: ShortcutBinding::new("Delete", false, false, false),
            permanent_delete: ShortcutBinding::new("Delete", false, false, true),
            properties: ShortcutBinding::new("Enter", false, true, false),
            go_up: ShortcutBinding::new("Backspace", false, false, false),
            go_back: ShortcutBinding::new("ArrowLeft", false, true, false),
            go_forward: ShortcutBinding::new("ArrowRight", false, true, false),
            edit_address: ShortcutBinding::new("L", true, false, false),
            open: ShortcutBinding::new("Enter", false, false, false),
            move_up: ShortcutBinding::new("ArrowUp", false, false, false),
            move_down: ShortcutBinding::new("ArrowDown", false, false, false),
        }
    }
}

fn default_undo_shortcut() -> ShortcutBinding {
    ShortcutBinding::new("Z", true, false, false)
}

fn default_edit_address_shortcut() -> ShortcutBinding {
    ShortcutBinding::new("L", true, false, false)
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum SidebarSection {
    Recents,
    Favorites,
    Storage,
    Portable,
    Network,
    Places,
}

impl SidebarSection {
    pub const ALL: [Self; 6] = [
        Self::Favorites,
        Self::Places,
        Self::Storage,
        Self::Portable,
        Self::Network,
        Self::Recents,
    ];
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub theme: ThemePreference,
    pub language: String,
    pub font_size: f32,
    pub accent_color: [u8; 3],
    pub window_size: [f32; 2],
    pub window_maximized: bool,
    pub favorites: Vec<PathBuf>,
    pub recent_paths: Vec<PathBuf>,
    pub show_hidden: bool,
    pub show_extensions: bool,
    pub show_icon_borders: bool,
    pub show_action_bar: bool,
    pub show_bookmark_bar: bool,
    pub show_split_pane_menus: bool,
    pub show_split_preview_panels: bool,
    pub show_preview_panel: bool,
    pub sidebar_visible: bool,
    pub default_view: ViewMode,
    pub storage_view: ViewMode,
    pub network_view: ViewMode,
    pub storage_group: GroupMode,
    pub network_group: GroupMode,
    pub storage_group_ascending: bool,
    pub network_group_ascending: bool,
    pub shortcuts: ShortcutConfig,
    pub sidebar_width: f32,
    pub sidebar_order: Vec<SidebarSection>,
    pub sidebar_collapsed: Vec<SidebarSection>,
    pub preview_panel_width: f32,
    pub preview_limit_bytes: usize,
    pub vibrancy: VibrancyMode,
    pub vibrancy_intensity: u8,
    #[serde(skip)]
    pub vibrancy_active: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            theme: ThemePreference::System,
            language: "en".into(),
            font_size: 12.5,
            accent_color: [3, 117, 172],
            window_size: [1280.0, 760.0],
            window_maximized: false,
            favorites: Vec::new(),
            recent_paths: Vec::new(),
            show_hidden: true,
            show_extensions: true,
            show_icon_borders: true,
            show_action_bar: true,
            show_bookmark_bar: false,
            show_split_pane_menus: false,
            show_split_preview_panels: false,
            show_preview_panel: false,
            sidebar_visible: true,
            default_view: ViewMode::Details,
            storage_view: ViewMode::Tiles,
            network_view: ViewMode::Tiles,
            storage_group: GroupMode::Type,
            network_group: GroupMode::Type,
            storage_group_ascending: true,
            network_group_ascending: true,
            shortcuts: ShortcutConfig::default(),
            sidebar_width: 220.0,
            sidebar_order: SidebarSection::ALL.to_vec(),
            sidebar_collapsed: Vec::new(),
            preview_panel_width: 320.0,
            preview_limit_bytes: 2 * 1024 * 1024,
            vibrancy: VibrancyMode::None,
            vibrancy_intensity: 50,
            vibrancy_active: false,
        }
    }
}

impl AppConfig {
    pub fn load() -> Self {
        match try_load() {
            Ok(mut config) => {
                config.normalize_sidebar_order();
                config.normalize_sidebar_collapsed();
                config.normalize_preview_panel_width();
                config
            }
            Err(error) => {
                crate::utils::log::error(format!("Config load failed: {error}"));
                let config = Self::default();
                let _ = config.save();
                config
            }
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = crate::utils::paths::config_file()?;
        let text = serde_json::to_string_pretty(self)?;
        crate::utils::atomic_file::write(&path, text.as_bytes())
    }

    pub fn remember_recent(&mut self, path: PathBuf) {
        self.recent_paths.retain(|item| item != &path);
        self.recent_paths.insert(0, path);
        self.recent_paths.truncate(5);
    }

    pub fn normalized_sidebar_order(&self) -> Vec<SidebarSection> {
        let mut order = Vec::with_capacity(SidebarSection::ALL.len());
        for section in self.sidebar_order.iter().copied() {
            if !order.contains(&section) {
                order.push(section);
            }
        }
        for section in SidebarSection::ALL {
            if !order.contains(&section) {
                order.push(section);
            }
        }
        order
    }

    pub fn normalize_sidebar_order(&mut self) {
        let legacy_default = vec![
            SidebarSection::Recents,
            SidebarSection::Favorites,
            SidebarSection::Storage,
            SidebarSection::Network,
            SidebarSection::Places,
        ];
        let previous_default = vec![
            SidebarSection::Favorites,
            SidebarSection::Places,
            SidebarSection::Storage,
            SidebarSection::Network,
            SidebarSection::Recents,
        ];
        if self.sidebar_order == legacy_default || self.sidebar_order == previous_default {
            self.sidebar_order = SidebarSection::ALL.to_vec();
            return;
        }
        self.sidebar_order = self.normalized_sidebar_order();
    }

    pub fn normalize_sidebar_collapsed(&mut self) {
        let mut collapsed = Vec::new();
        for section in self.sidebar_collapsed.iter().copied() {
            if SidebarSection::ALL.contains(&section) && !collapsed.contains(&section) {
                collapsed.push(section);
            }
        }
        self.sidebar_collapsed = collapsed;
    }

    pub fn normalize_preview_panel_width(&mut self) {
        self.preview_panel_width = self.preview_panel_width.clamp(220.0, 560.0);
    }
}

fn try_load() -> Result<AppConfig> {
    let path = crate::utils::paths::config_file()?;
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    let text = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&text)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_install_defaults_follow_system_and_show_all_file_names() {
        let config = AppConfig::default();
        assert_eq!(config.theme, ThemePreference::System);
        assert_eq!(config.accent_color, [3, 117, 172]);
        assert_eq!(config.vibrancy, VibrancyMode::None);
        assert!(!config.window_maximized);
        assert!(config.show_extensions);
        assert!(config.show_hidden);
    }
}
