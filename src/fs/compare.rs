#![allow(dead_code)]

use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum CompareStatus {
    Equal,
    Different,
    MissingLeft,
    MissingRight,
}

#[derive(Clone, Debug)]
pub struct CompareItem {
    pub left: Option<PathBuf>,
    pub right: Option<PathBuf>,
    pub status: CompareStatus,
}
