#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppCommand {
    NewTab,
    CloseTab,
    CopyPath,
    ToggleHidden,
    ToggleTheme,
    Refresh,
    GoUp,
    Rename,
}

impl AppCommand {
    pub fn label(self) -> &'static str {
        match self {
            Self::NewTab => "New tab",
            Self::CloseTab => "Close tab",
            Self::CopyPath => "Copy current path",
            Self::ToggleHidden => "Toggle hidden files",
            Self::ToggleTheme => "Toggle theme",
            Self::Refresh => "Refresh",
            Self::GoUp => "Go up",
            Self::Rename => "Rename selected item",
        }
    }

    pub fn all() -> &'static [AppCommand] {
        &[
            AppCommand::NewTab,
            AppCommand::CloseTab,
            AppCommand::CopyPath,
            AppCommand::ToggleHidden,
            AppCommand::ToggleTheme,
            AppCommand::Refresh,
            AppCommand::GoUp,
            AppCommand::Rename,
        ]
    }
}
