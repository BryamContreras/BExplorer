use std::path::PathBuf;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum ArchiveJobKind {
    Compress,
    Extract,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArchiveState {
    Running,
    Finished,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ArchiveJob {
    pub id: u64,
    pub kind: ArchiveJobKind,
    pub format: ArchiveFormat,
    #[serde(default)]
    pub method: ArchiveCompressionMethod,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    pub sources: Vec<PathBuf>,
    pub destination: PathBuf,
    pub archive_path: PathBuf,
    pub extract_mode: ExtractMode,
}

impl ArchiveJob {
    pub fn has_password(&self) -> bool {
        self.password
            .as_deref()
            .is_some_and(|value| !value.is_empty())
    }
}

/// A single entry from listing an archive (ZIP or 7z).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ArchiveListEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: Option<u64>,
    #[allow(dead_code)]
    pub pack_size: Option<u64>,
    pub modified: Option<SystemTime>,
}

/// Progress snapshot from the 7-Zip engine.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ArchiveProgress {
    pub completed: u64,
    pub total: u64,
    #[allow(dead_code)]
    pub files: u64,
    pub command: String,
    pub file_name: String,
}

/// Message from the archive operation thread to the UI.
#[derive(Clone, Debug)]
pub enum ArchiveProgressMsg {
    Progress(ArchiveProgress),
    Finished(std::result::Result<PathBuf, String>),
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum ArchiveFormat {
    Zip,
    SevenZip,
}

impl ArchiveFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::SevenZip => "7z",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub enum ArchiveCompressionMethod {
    Store,
    Fast,
    #[default]
    Normal,
    Maximum,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum ExtractMode {
    Here,
    ToNamedFolder,
}
