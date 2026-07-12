use std::path::PathBuf;

use directories::{BaseDirs, ProjectDirs, UserDirs};

use crate::utils::errors::{BExplorerError, Result};

const QUALIFIER: &str = "dev";
const ORGANIZATION: &str = "BExplorer";
const APPLICATION: &str = "BExplorer";

pub fn config_dir() -> Result<PathBuf> {
    let dirs = ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
        .ok_or_else(|| BExplorerError::Operation("Could not resolve config directory".into()))?;
    let dir = dirs.config_dir().to_path_buf();
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn config_file() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.json"))
}

pub fn session_file() -> Result<PathBuf> {
    Ok(config_dir()?.join("session.json"))
}

pub fn log_file() -> Result<PathBuf> {
    Ok(config_dir()?.join("bexplorer.log"))
}

pub fn home_dir() -> Option<PathBuf> {
    BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf())
}

#[derive(Clone, Debug)]
pub struct CommonPlace {
    pub label: String,
    pub path: PathBuf,
}

pub fn common_places() -> Vec<CommonPlace> {
    let mut places = Vec::new();

    if let Some(home) = home_dir() {
        places.push(CommonPlace {
            label: "Home".into(),
            path: home,
        });
    }

    if let Some(user_dirs) = UserDirs::new() {
        push_place(&mut places, "Desktop", user_dirs.desktop_dir());
        push_place(&mut places, "Downloads", user_dirs.download_dir());
        push_place(&mut places, "Documents", user_dirs.document_dir());
        push_place(&mut places, "Music", user_dirs.audio_dir());
        push_place(&mut places, "Pictures", user_dirs.picture_dir());
        push_place(&mut places, "Videos", user_dirs.video_dir());
    }

    dedupe_places(places)
}

fn push_place(places: &mut Vec<CommonPlace>, label: &str, path: Option<&std::path::Path>) {
    if let Some(path) = path
        && path.exists()
    {
        places.push(CommonPlace {
            // XDG folders may be named Documents, Documentos, Dokumente,
            // etc. Their filesystem name is the only reliable display
            // name; the old static English labels ignored the user's
            // actual desktop layout.
            label: path
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .unwrap_or(label)
                .to_owned(),
            path: path.to_path_buf(),
        });
    }
}

fn dedupe_places(places: Vec<CommonPlace>) -> Vec<CommonPlace> {
    let mut out = Vec::new();
    for place in places {
        if !out.iter().any(|item: &CommonPlace| item.path == place.path) {
            out.push(place);
        }
    }
    out
}
