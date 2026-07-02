#![allow(dead_code)]

use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum WatchEvent {
    Changed(PathBuf),
    Removed(PathBuf),
    Created(PathBuf),
}
