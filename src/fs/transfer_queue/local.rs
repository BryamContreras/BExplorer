use super::*;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct FileIdentity(u64, u64);

pub(crate) fn opened_file_identity(file: &fs::File) -> std::io::Result<FileIdentity> {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
        };
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }.map_err(
            |error| std::io::Error::from_raw_os_error((error.code().0 as u32 & 0xffff) as i32),
        )?;
        Ok(FileIdentity(
            info.dwVolumeSerialNumber as u64,
            (info.nFileIndexHigh as u64) << 32 | info.nFileIndexLow as u64,
        ))
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let info = file.metadata()?;
        Ok(FileIdentity(info.dev(), info.ino()))
    }
}

pub(crate) fn path_identity(path: &Path) -> std::io::Result<FileIdentity> {
    #[cfg(windows)]
    {
        let file = open_metadata_handle(path)?;
        opened_file_identity(&file)
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let info = fs::symlink_metadata(path)?;
        Ok(FileIdentity(info.dev(), info.ino()))
    }
}

#[cfg(windows)]
fn open_metadata_handle(path: &Path) -> std::io::Result<fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
    };
    fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES.0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)
}

pub(super) fn same_file(source: &Path, target: &Path) -> Result<bool> {
    match fs::symlink_metadata(target) {
        Ok(_) => Ok(path_identity(source)? == path_identity(target)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn is_directory_reparse_point(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & (0x400 | 0x10) == (0x400 | 0x10)
    }
    #[cfg(not(windows))]
    {
        let _ = metadata;
        false
    }
}

pub(super) fn ensure_supported(path: &Path, metadata: &fs::Metadata) -> Result<()> {
    #[cfg(windows)]
    let reparse_point = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(windows)]
    if reparse_point {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_TAG_INFO, FileAttributeTagInfo, GetFileInformationByHandleEx,
        };
        let file = open_metadata_handle(path)?;
        let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
        unsafe {
            GetFileInformationByHandleEx(
                HANDLE(file.as_raw_handle()),
                FileAttributeTagInfo,
                &mut info as *mut _ as *mut _,
                std::mem::size_of_val(&info) as u32,
            )
        }?;
        // CloudFiles and other nonredirecting tags are ordinary files/folders
        // for traversal. Symbolic links are copied as links. Other name
        // surrogates (including junctions) must never be followed implicitly.
        if supported_reparse_tag(info.ReparseTag) {
            return Ok(());
        }
        return Err(BExplorerError::Operation(format!(
            "Directory junctions and other redirecting reparse points are not supported for transfers: {}",
            path.display()
        )));
    }
    #[cfg(not(windows))]
    let _ = (path, metadata);
    Ok(())
}

#[cfg(any(windows, test))]
fn supported_reparse_tag(tag: u32) -> bool {
    const SYMLINK: u32 = 0xA000_000C;
    const MOUNT_POINT: u32 = 0xA000_0003;
    const NAME_SURROGATE: u32 = 0x2000_0000;
    match tag {
        SYMLINK => true,
        MOUNT_POINT => false,
        _ => tag & NAME_SURROGATE == 0,
    }
}

#[cfg(test)]
mod reparse_tag_tests {
    use super::supported_reparse_tag;

    #[test]
    fn cloud_files_do_not_get_rejected_as_directory_redirectors() {
        // CLOUD through CLOUD_F encode the placeholder state in bits 12..15.
        for state in 0..=15 {
            assert!(supported_reparse_tag(0x9000_001A | state << 12));
        }
        assert!(supported_reparse_tag(0x8000_0013)); // DEDUP
        assert!(supported_reparse_tag(0x8000_0015)); // FILE_PLACEHOLDER
    }

    #[test]
    fn only_supported_symbolic_links_are_allowed_among_name_surrogates() {
        assert!(supported_reparse_tag(0xA000_000C)); // preserved by copy_link
        assert!(!supported_reparse_tag(0xA000_0003)); // junction / mount point
        assert!(!supported_reparse_tag(0xA000_001D)); // Linux symlink reparse tag
        assert!(!supported_reparse_tag(0xA000_1234)); // unknown redirector
    }
}

pub(super) fn reservation_key(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let resolved = fs::canonicalize(parent)
        .map(|parent| parent.join(path.file_name().unwrap_or_default()))
        .unwrap_or_else(|_| path.to_path_buf());
    #[cfg(windows)]
    {
        PathBuf::from(resolved.as_os_str().to_string_lossy().to_lowercase())
    }
    #[cfg(not(windows))]
    {
        resolved
    }
}

pub(super) fn prepare(
    job: &TransferJob,
    tx: &Sender<TransferMessage>,
    control: &TransferControl,
    runtime: &mut TransferRuntime,
) -> Result<()> {
    check_cancelled(control)?;
    let portable_destination = explorer::is_portable_path(&job.destination);
    let destination = (!portable_destination)
        .then(|| fs::canonicalize(&job.destination))
        .transpose()?;
    for source in &job.sources {
        if explorer::is_portable_path(source) {
            continue;
        }
        check_cancelled(control)?;
        let metadata = match fs::symlink_metadata(source) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        ensure_supported(source, &metadata)?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            let source_id = path_identity(source)?;
            let mut ancestor = destination.as_deref();
            while let Some(path) = ancestor {
                check_cancelled(control)?;
                if path_identity(path)? == source_id {
                    return Err(BExplorerError::Operation(
                        "A folder cannot be copied or moved into itself or one of its descendants"
                            .into(),
                    ));
                }
                ancestor = path.parent();
            }
        }
        let mut pending = vec![source.to_path_buf()];
        let mut visited = HashSet::new();
        let mut bytes = 0_u64;
        let mut files = 0_usize;
        while let Some(path) = pending.pop() {
            check_cancelled(control)?;
            let metadata = fs::symlink_metadata(&path)?;
            ensure_supported(&path, &metadata)?;
            if metadata.file_type().is_symlink() {
                if portable_destination {
                    return Err(BExplorerError::Operation(format!(
                        "Symbolic links cannot be transferred to a portable device: {}",
                        path.display()
                    )));
                }
                files = files.saturating_add(1);
            } else if metadata.is_file() {
                bytes = bytes.saturating_add(metadata.len());
                files = files.saturating_add(1);
            } else if metadata.is_dir() {
                if !visited.insert(path_identity(&path)?) {
                    return Err(BExplorerError::Operation(format!(
                        "Directory cycle detected at {}",
                        path.display()
                    )));
                }
                for child in fs::read_dir(&path)? {
                    check_cancelled(control)?;
                    pending.push(child?.path());
                }
            } else {
                return Err(BExplorerError::Operation(format!(
                    "Unsupported file type: {}",
                    path.display()
                )));
            }
            if portable_destination && job.kind == TransferKind::Move {
                runtime.copied_sources.push(SourceSnapshot::capture(&path)?);
            }
            emit_progress_if_due(
                job,
                current_name(&path),
                TransferState::Pending,
                runtime,
                tx,
            );
        }
        runtime.total_bytes = runtime.total_bytes.saturating_add(bytes);
        runtime.total_files = runtime.total_files.saturating_add(files);
        runtime.source_totals.insert(source.clone(), (bytes, files));
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub(super) struct SourceSnapshot {
    path: PathBuf,
    identity: FileIdentity,
    size: u64,
    modified: Option<std::time::SystemTime>,
    directory: bool,
    link: Option<PathBuf>,
}

impl SourceSnapshot {
    pub(super) fn capture(path: &Path) -> Result<Self> {
        let metadata = fs::symlink_metadata(path)?;
        ensure_supported(path, &metadata)?;
        let link = metadata
            .file_type()
            .is_symlink()
            .then(|| fs::read_link(path))
            .transpose()?;
        Ok(Self {
            path: path.into(),
            identity: path_identity(path)?,
            size: metadata.len(),
            modified: metadata.modified().ok(),
            directory: metadata.is_dir() && link.is_none(),
            link,
        })
    }
    pub(super) fn unchanged(&self) -> bool {
        let Ok(current) = Self::capture(&self.path) else {
            return false;
        };
        self.identity == current.identity
            && self.directory == current.directory
            && self.link == current.link
            && (self.directory || self.size == current.size && self.modified == current.modified)
    }
    pub(super) fn size(&self) -> u64 {
        self.size
    }
    pub(super) fn modified(&self) -> Option<std::time::SystemTime> {
        self.modified
    }
    pub(super) fn matches_open_file(&self, file: &fs::File) -> Result<bool> {
        Ok(opened_file_identity(file)? == self.identity && file.metadata()?.len() == self.size)
    }
}

pub(super) fn remove_copied_source(
    root: &Path,
    snapshots: &[SourceSnapshot],
    control: &TransferControl,
) -> Result<()> {
    for snapshot in snapshots
        .iter()
        .rev()
        .filter(|snapshot| snapshot.path.starts_with(root))
    {
        check_cancelled(control)?;
        if !snapshot.unchanged() {
            return Err(BExplorerError::Operation(format!(
                "The source changed after copying; it was preserved at {}",
                snapshot.path.display()
            )));
        }
        if snapshot.directory {
            fs::remove_dir(&snapshot.path)?;
        } else if snapshot.link.is_some() {
            remove_link(&snapshot.path)?;
        } else {
            fs::remove_file(&snapshot.path)?;
        }
    }
    Ok(())
}

pub(super) fn copy_link(source: &Path, target: &Path) -> Result<()> {
    let link = fs::read_link(source)?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(link, target)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileTypeExt;
        let kind = fs::symlink_metadata(source)?.file_type();
        if kind.is_symlink_dir() {
            std::os::windows::fs::symlink_dir(link, target)?;
        } else {
            std::os::windows::fs::symlink_file(link, target)?;
        }
    }
    Ok(())
}

pub(super) fn remove_link(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileTypeExt;
        if fs::symlink_metadata(path)?.file_type().is_symlink_dir() {
            return fs::remove_dir(path);
        }
    }
    fs::remove_file(path)
}
