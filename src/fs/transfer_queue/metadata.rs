use super::*;

pub(super) fn create_file(path: &Path) -> std::io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

pub(super) fn create_directory(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(path)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir(path)
    }
}

#[cfg(unix)]
pub(super) fn allow_directory_cleanup(path: &Path, identity: FileIdentity) {
    use std::os::unix::fs::PermissionsExt;

    if let Ok(metadata) = fs::symlink_metadata(path)
        && metadata.is_dir()
        && metadata.permissions().mode() & 0o700 != 0o700
        && let Ok(directory) = fs::File::open(path)
        && opened_file_identity(&directory).is_ok_and(|opened| opened == identity)
    {
        let mut permissions = metadata.permissions();
        permissions.set_mode(permissions.mode() | 0o700);
        let _ = directory.set_permissions(permissions);
    }
}

pub(super) fn finish_file(
    input: &fs::File,
    output: &fs::File,
    target: &Path,
    metadata: &fs::Metadata,
    durable: bool,
) -> Result<()> {
    #[cfg(target_os = "linux")]
    linux::copy_attributes(input, output, target, false)?;
    #[cfg(not(target_os = "linux"))]
    let _ = input;
    set_metadata(output, target, metadata, false)?;
    if durable {
        output.sync_all()?;
    }
    Ok(())
}

pub(super) fn finish_directory(
    source: &Path,
    target: &Path,
    metadata: &fs::Metadata,
    durable: bool,
) -> Result<()> {
    #[cfg(unix)]
    let output = fs::File::open(target)?;
    #[cfg(windows)]
    let output = windows::open_for_metadata(target)?;
    #[cfg(target_os = "linux")]
    linux::copy_attributes(&fs::File::open(source)?, &output, target, true)?;
    set_metadata(&output, target, metadata, true)?;
    #[cfg(unix)]
    if durable {
        output.sync_all()?;
    }
    let _ = (source, durable);
    Ok(())
}

fn set_metadata(
    file: &fs::File,
    target: &Path,
    metadata: &fs::Metadata,
    directory: bool,
) -> Result<()> {
    let mut times = fs::FileTimes::new();
    if let Ok(accessed) = metadata.accessed() {
        times = times.set_accessed(accessed);
    }
    if let Ok(modified) = metadata.modified() {
        times = times.set_modified(modified);
    }
    #[cfg(windows)]
    if let Ok(created) = metadata.created() {
        use std::os::windows::fs::FileTimesExt;
        times = times.set_created(created);
    }
    file.set_times(times)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        // Grant execute/read/write access as on the source. Copy-created entries
        // retain their destination ownership, so do not propagate setuid/setgid.
        let mask = if directory { 0o1777 } else { 0o777 };
        let mode = metadata.permissions().mode() & mask;
        let changed = file.set_permissions(fs::Permissions::from_mode(mode));
        if changed.is_err() || file.metadata()?.permissions().mode() & mask != mode {
            #[cfg(target_os = "linux")]
            if linux::has_synthetic_permissions(file)? {
                return Ok(());
            }
            changed?;
            return Err(BExplorerError::Operation(format!(
                "Could not preserve permissions at {}",
                target.display()
            )));
        }
    }
    #[cfg(windows)]
    {
        windows::copy_dos_attributes(file, metadata)?;
        let _ = (target, directory);
    }
    Ok(())
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use rustix::fs::{XattrFlags, fgetxattr, flistxattr, fremovexattr, fsetxattr, fstatfs};
    use rustix::io::Errno;

    pub(super) fn has_synthetic_permissions(file: &fs::File) -> Result<bool> {
        // FAT/exFAT expose mode bits synthesized from mount options, and
        // offer no per-file chmod. A transfer inherits the mount's policy.
        Ok(matches!(
            fstatfs(file).map_err(std::io::Error::from)?.f_type,
            0x4d44 | 0x2011bab0
        ))
    }

    pub(super) fn copy_attributes(
        input: &fs::File,
        output: &fs::File,
        target: &Path,
        directory: bool,
    ) -> Result<()> {
        use std::os::unix::ffi::OsStrExt;

        const ACCESS_ACL: &[u8] = b"system.posix_acl_access";
        const DEFAULT_ACL: &[u8] = b"system.posix_acl_default";
        const XATTR_LIMIT: usize = 64 * 1024;

        // The Linux VFS limits an attribute value and the list of names to
        // 64 KiB each. Avoid allocating those buffers for common small inputs.
        let mut small_names = [0_u8; 256];
        let mut large_names = Vec::new();
        let names = match flistxattr(input, &mut small_names[..]) {
            Ok(size) => &small_names[..size],
            Err(Errno::RANGE) => {
                large_names.resize(XATTR_LIMIT, 0);
                let size = flistxattr(input, &mut large_names[..]).map_err(std::io::Error::from)?;
                &large_names[..size]
            }
            Err(Errno::NOTSUP) => &[],
            Err(error) => return Err(std::io::Error::from(error).into()),
        };

        let mut access = false;
        let mut default = false;
        let mut small_value = [0_u8; 256];
        let mut large_value = Vec::new();
        let names = names.split(|byte| *byte == 0);
        let user_attributes = names.clone().filter(|name| name.starts_with(b"user."));
        let acl_attributes = names.filter(|name| *name == ACCESS_ACL || *name == DEFAULT_ACL);
        // Applying the ACL can remove the write permission required to set
        // user attributes. The source filesystem's name order is arbitrary.
        for name in user_attributes.chain(acl_attributes) {
            access |= name == ACCESS_ACL;
            default |= name == DEFAULT_ACL;
            let name = std::ffi::OsStr::from_bytes(name);
            let value = match fgetxattr(input, name, &mut small_value[..]) {
                Ok(size) => &small_value[..size],
                Err(Errno::RANGE) => {
                    large_value.resize(XATTR_LIMIT, 0);
                    let size = fgetxattr(input, name, &mut large_value[..])
                        .map_err(std::io::Error::from)?;
                    &large_value[..size]
                }
                Err(error) => return Err(std::io::Error::from(error).into()),
            };
            fsetxattr(output, name, value, XattrFlags::empty()).map_err(|error| {
                BExplorerError::Operation(format!(
                    "Could not preserve attribute {} at {}: {error}",
                    name.to_string_lossy(),
                    target.display()
                ))
            })?;
        }
        if !access {
            remove_inherited_acl(output, ACCESS_ACL)?;
        }
        if directory && !default {
            remove_inherited_acl(output, DEFAULT_ACL)?;
        }
        Ok(())
    }

    fn remove_inherited_acl(output: &fs::File, name: &[u8]) -> Result<()> {
        use std::os::unix::ffi::OsStrExt;
        match fremovexattr(output, std::ffi::OsStr::from_bytes(name)) {
            Ok(()) | Err(Errno::NODATA | Errno::NOTSUP) => Ok(()),
            Err(error) => Err(std::io::Error::from(error).into()),
        }
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use ::windows::Win32::{
        Foundation::{
            ERROR_HANDLE_EOF, ERROR_INVALID_FUNCTION, ERROR_INVALID_NAME, ERROR_INVALID_PARAMETER,
            ERROR_NOT_SUPPORTED, HANDLE,
        },
        Storage::FileSystem::{
            FILE_ATTRIBUTE_ARCHIVE, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_NORMAL,
            FILE_ATTRIBUTE_NOT_CONTENT_INDEXED, FILE_ATTRIBUTE_READONLY, FILE_ATTRIBUTE_SYSTEM,
            FILE_BASIC_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_READ_ATTRIBUTES, FILE_WRITE_ATTRIBUTES, FileBasicInfo, FindClose,
            FindFirstStreamW, FindNextStreamW, FindStreamInfoStandard, SetFileInformationByHandle,
            WIN32_FIND_STREAM_DATA,
        },
    };
    use ::windows::core::PCWSTR;
    use std::ffi::OsString;
    use std::os::windows::{
        ffi::{OsStrExt, OsStringExt},
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    };

    struct StreamSearch(HANDLE);

    impl Drop for StreamSearch {
        fn drop(&mut self) {
            let _ = unsafe { FindClose(self.0) };
        }
    }

    pub(in crate::fs::transfer_queue) fn copy_streams(
        source: &Path,
        target: &Path,
        durable: bool,
        kind: TransferKind,
        mut checkpoint: impl FnMut() -> Result<()>,
    ) -> Result<()> {
        let source = std::path::absolute(source)?;
        let source_wide = extended_stream_search_path(&source);
        let mut data = WIN32_FIND_STREAM_DATA::default();
        let data_ptr = (&mut data as *mut WIN32_FIND_STREAM_DATA).cast();
        let search = match unsafe {
            FindFirstStreamW(
                PCWSTR(source_wide.as_ptr()),
                FindStreamInfoStandard,
                data_ptr,
                0,
            )
        } {
            Ok(handle) => StreamSearch(handle),
            Err(error) if no_stream_support(&error) => return Ok(()),
            Err(error) if error.code() == ERROR_HANDLE_EOF.to_hresult() => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        let mut buffer = Vec::new();
        loop {
            checkpoint()?;
            let end = data
                .cStreamName
                .iter()
                .position(|&unit| unit == 0)
                .ok_or_else(|| {
                    BExplorerError::Operation("Windows returned an invalid stream name".into())
                })?;
            let name = &data.cStreamName[..end];
            if !name.iter().copied().eq("::$DATA".encode_utf16()) {
                let name = OsString::from_wide(name);
                let source_stream = stream_path(&source, &name);
                let target_stream = stream_path(target, &name);
                let mut input = fs::File::open(&source_stream)?;
                let output = create_file(&target_stream);
                if let Err(error) = &output
                    && kind == TransferKind::Copy
                    && is_unsupported_stream_destination(error)
                {
                    crate::utils::log::info(format!(
                        "The destination does not support named data streams: {}",
                        target.display()
                    ));
                    return Ok(());
                }
                let mut output = output.map_err(|error| {
                    BExplorerError::Operation(format!(
                        "Could not preserve named stream {}: {error}",
                        target_stream.display()
                    ))
                })?;
                buffer.resize(COPY_BUFFER_SIZE, 0);
                let mut bytes = 0_u64;
                loop {
                    checkpoint()?;
                    let read = input.read(&mut buffer)?;
                    if read == 0 {
                        break;
                    }
                    output.write_all(&buffer[..read])?;
                    bytes += read as u64;
                }
                if i64::try_from(bytes).ok() != Some(data.StreamSize) {
                    return Err(BExplorerError::Operation(format!(
                        "The named stream changed while copying: {}",
                        source_stream.display()
                    )));
                }
                if durable {
                    output.sync_all()?;
                }
            }
            match unsafe { FindNextStreamW(search.0, data_ptr) } {
                Ok(()) => {}
                Err(error) if error.code() == ERROR_HANDLE_EOF.to_hresult() => return Ok(()),
                Err(error) => return Err(error.into()),
            }
        }
    }

    fn no_stream_support(error: &::windows::core::Error) -> bool {
        [
            ERROR_INVALID_FUNCTION,
            ERROR_INVALID_PARAMETER,
            ERROR_NOT_SUPPORTED,
        ]
        .iter()
        .any(|code| error.code() == code.to_hresult())
    }

    fn is_unsupported_stream_destination(error: &std::io::Error) -> bool {
        [
            ERROR_INVALID_FUNCTION,
            ERROR_INVALID_NAME,
            ERROR_INVALID_PARAMETER,
            ERROR_NOT_SUPPORTED,
        ]
        .iter()
        .any(|code| error.raw_os_error() == Some(code.0 as i32))
    }

    fn stream_path(path: &Path, name: &std::ffi::OsStr) -> PathBuf {
        let mut path = path.as_os_str().to_os_string();
        path.push(name);
        PathBuf::from(path)
    }

    fn extended_stream_search_path(path: &Path) -> Vec<u16> {
        use std::path::{Component, Prefix};

        let mut wide: Vec<_> = path.as_os_str().encode_wide().collect();
        if wide.len() >= 260
            && let Some(Component::Prefix(prefix)) = path.components().next()
        {
            let prefix = match prefix.kind() {
                Prefix::Disk(_) => Some("\\\\?\\"),
                Prefix::UNC(_, _) => {
                    wide.drain(..2);
                    Some("\\\\?\\UNC\\")
                }
                _ => None,
            };
            if let Some(prefix) = prefix {
                wide.splice(..0, prefix.encode_utf16());
            }
        }
        wide.push(0);
        wide
    }
    pub(super) fn open_for_metadata(path: &Path) -> std::io::Result<fs::File> {
        fs::OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES.0 | FILE_WRITE_ATTRIBUTES.0)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(path)
    }

    pub(super) fn copy_dos_attributes(file: &fs::File, source: &fs::Metadata) -> Result<()> {
        const FLAGS: u32 = FILE_ATTRIBUTE_ARCHIVE.0
            | FILE_ATTRIBUTE_HIDDEN.0
            | FILE_ATTRIBUTE_NOT_CONTENT_INDEXED.0
            | FILE_ATTRIBUTE_READONLY.0
            | FILE_ATTRIBUTE_SYSTEM.0;

        let attributes = source.file_attributes() & FLAGS;
        let info = FILE_BASIC_INFO {
            FileAttributes: if attributes == 0 {
                FILE_ATTRIBUTE_NORMAL.0
            } else {
                attributes
            },
            ..Default::default()
        };
        unsafe {
            SetFileInformationByHandle(
                HANDLE(file.as_raw_handle()),
                FileBasicInfo,
                (&info as *const FILE_BASIC_INFO).cast(),
                std::mem::size_of_val(&info) as u32,
            )?;
        }
        Ok(())
    }
}

#[cfg(windows)]
pub(super) use windows::copy_streams;
