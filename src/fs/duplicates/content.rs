use crate::fs::transfer_queue::{FileIdentity, opened_file_identity, path_identity};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

#[derive(Clone, Copy, Debug)]
pub(super) struct ContentStamp {
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub identity: FileIdentity,
    pub digest: [u8; 16],
}

fn cancelled(cancel: &AtomicBool) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Duplicate verification cancelled",
        ))
    } else {
        Ok(())
    }
}

fn open_regular(path: &Path) -> io::Result<File> {
    if !fs::symlink_metadata(path)?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "A duplicate candidate is no longer a regular file",
        ));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows::Win32::Storage::FileSystem::{FILE_SHARE_DELETE, FILE_SHARE_READ};
        fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_DELETE.0)
            .open(path)
    }
    #[cfg(not(windows))]
    {
        File::open(path)
    }
}

fn stable(path: &Path, file: &File, before: &fs::Metadata) -> io::Result<()> {
    let after = file.metadata()?;
    if !fs::symlink_metadata(path)?.is_file()
        || path_identity(path)? != opened_file_identity(file)?
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err(io::Error::other(
            "A duplicate candidate changed while it was being verified",
        ));
    }
    Ok(())
}

pub(super) fn fingerprint(path: &Path, cancel: &AtomicBool) -> io::Result<ContentStamp> {
    cancelled(cancel)?;
    let mut file = open_regular(path)?;
    let before = file.metadata()?;
    let identity = opened_file_identity(&file)?;
    let mut digest = md5::Context::new();
    let mut buffer = vec![0; 128 * 1024];
    let mut count = 0_u64;
    loop {
        cancelled(cancel)?;
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        count = count.saturating_add(read as u64);
        digest.consume(&buffer[..read]);
    }
    stable(path, &file, &before)?;
    if count != before.len() {
        return Err(io::Error::other(
            "File size changed during duplicate hashing",
        ));
    }
    Ok(ContentStamp {
        size: count,
        modified: before.modified().ok(),
        identity,
        digest: digest.compute().0,
    })
}

pub(super) fn equal(left: &Path, right: &Path, cancel: &AtomicBool) -> io::Result<bool> {
    cancelled(cancel)?;
    let mut first = open_regular(left)?;
    let mut second = open_regular(right)?;
    let before_first = first.metadata()?;
    let before_second = second.metadata()?;
    if before_first.len() != before_second.len() {
        return Ok(false);
    }
    let mut first_buffer = vec![0; 128 * 1024];
    let mut second_buffer = vec![0; 128 * 1024];
    loop {
        cancelled(cancel)?;
        let read = first.read(&mut first_buffer)?;
        if read == 0 {
            if second.read(&mut second_buffer[..1])? != 0 {
                return Ok(false);
            }
            break;
        }
        if let Err(error) = second.read_exact(&mut second_buffer[..read]) {
            if error.kind() == io::ErrorKind::UnexpectedEof {
                return Ok(false);
            }
            return Err(error);
        }
        if first_buffer[..read] != second_buffer[..read] {
            return Ok(false);
        }
    }
    stable(left, &first, &before_first)?;
    stable(right, &second, &before_second)?;
    Ok(true)
}
