#![allow(dead_code)]

use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HashAlgorithm {
    Md5,
    Sha1,
    Sha256,
    Xxh3,
}

#[derive(Clone, Debug)]
pub struct HashRequest {
    pub path: PathBuf,
    pub algorithm: HashAlgorithm,
}
