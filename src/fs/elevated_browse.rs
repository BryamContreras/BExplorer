//! One read-only enumeration of a local folder approved through UAC or Polkit.

use std::path::Path;

use crate::utils::errors::{BExplorerError, Result};

use super::explorer::FileEntry;

#[cfg(any(windows, target_os = "linux"))]
use {
    super::explorer::{self, EntryKind},
    serde::{Deserialize, Serialize, de::DeserializeOwned},
    std::ffi::{OsStr, OsString},
    std::io::{Read, Write},
    std::path::{Component, PathBuf},
    std::time::{Duration, UNIX_EPOCH},
};

#[cfg(windows)]
mod windows_ipc;

#[cfg(any(windows, target_os = "linux"))]
const HELPER_ARGUMENT: &str = "--bexplorer-elevated-browse-helper";
#[cfg(any(windows, target_os = "linux"))]
const MAX_REQUEST_BYTES: usize = 256 * 1024;
#[cfg(any(windows, target_os = "linux"))]
const MAX_RESPONSE_BYTES: usize = 64 * 1024 * 1024;

/// Call from the directory worker after a normal listing gets access denied.
pub(crate) fn can_elevate_directory(path: &Path) -> bool {
    #[cfg(any(windows, target_os = "linux"))]
    {
        path.is_absolute()
            && !native_units(path.as_os_str()).contains(&0)
            && !path.components().any(|part| part == Component::ParentDir)
            && !explorer::is_virtual_path(path)
            && local_storage(path)
            && !super::archive_listing::is_archive_navigation_path(path)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = path;
        false
    }
}

/// Returns `None` only when the system authorization dialog was canceled.
pub(crate) fn list_directory_elevated(
    path: &Path,
    show_hidden: bool,
) -> Result<Option<Vec<FileEntry>>> {
    #[cfg(any(windows, target_os = "linux"))]
    {
        if !can_elevate_directory(path) {
            return Err(BExplorerError::InvalidPath(path.to_path_buf()));
        }
        let request = BrowseRequest {
            directory: native_units(path.as_os_str()),
            show_hidden,
        };
        let bytes = serde_json::to_vec(&request)?;
        if bytes.len() > MAX_REQUEST_BYTES {
            return Err(BExplorerError::InvalidPath(path.to_path_buf()));
        }
        let Some(response) = launch_helper(&bytes)? else {
            return Ok(None);
        };
        response
            .into_iter()
            .map(|entry| entry.into_file_entry(path))
            .collect::<Result<Vec<_>>>()
            .map(Some)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (path, show_hidden);
        Err(BExplorerError::Operation(
            "Elevated folder browsing is available on Windows and Linux".into(),
        ))
    }
}

#[cfg(target_os = "linux")]
fn local_storage(path: &Path) -> bool {
    explorer::linux_elevated_browse_mount_is_local(path)
}

#[cfg(windows)]
fn local_storage(path: &Path) -> bool {
    use std::path::Prefix;
    use windows::Win32::{
        Storage::FileSystem::GetDriveTypeW,
        System::WindowsProgramming::{DRIVE_CDROM, DRIVE_FIXED, DRIVE_RAMDISK, DRIVE_REMOVABLE},
    };
    use windows::core::PCWSTR;

    let Some(Component::Prefix(prefix)) = path.components().next() else {
        return false;
    };
    let (Prefix::Disk(letter) | Prefix::VerbatimDisk(letter)) = prefix.kind() else {
        return false;
    };
    let root = [u16::from(letter), u16::from(b':'), u16::from(b'\\'), 0];
    matches!(
        unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) },
        DRIVE_FIXED | DRIVE_REMOVABLE | DRIVE_RAMDISK | DRIVE_CDROM
    )
}

#[cfg(target_os = "linux")]
fn launch_helper(request: &[u8]) -> Result<Option<Vec<BrowseEntry>>> {
    let output = crate::platform::shell::run_elevated_current_exe_with_graphical_agent(
        &[OsString::from(HELPER_ARGUMENT)],
        request,
    )?;
    if output.status.code() == Some(126) && output.stdout.is_empty() {
        return Ok(None);
    }
    if output.stdout.is_empty() && !output.status.success() {
        let details = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(BExplorerError::Shell(if details.is_empty() {
            "Polkit could not authorize the folder listing".into()
        } else {
            details
        }));
    }
    response_from_reader(output.stdout.as_slice()).map(Some)
}

#[cfg(windows)]
use windows_ipc::launch_helper;

#[cfg(any(windows, target_os = "linux"))]
/// Routes the narrow helper before the application creates config or UI state.
pub(crate) fn try_run_elevated_helper_from_args() -> Option<i32> {
    let mut args = std::env::args_os().skip(1);
    if args.next()?.as_os_str() != OsStr::new(HELPER_ARGUMENT) {
        return None;
    }
    #[cfg(target_os = "linux")]
    let code = if args.next().is_none() {
        run_helper(std::io::stdin().lock(), std::io::stdout().lock())
    } else {
        2
    };
    #[cfg(windows)]
    let code = windows_ipc::run_helper_with_args(args);
    Some(code)
}

#[cfg(any(windows, target_os = "linux"))]
fn run_helper(reader: impl Read, mut writer: impl Write) -> i32 {
    let result =
        read_json::<BrowseRequest>(reader, MAX_REQUEST_BYTES).and_then(list_requested_directory);
    let response: BrowseResponse = result.map_err(|error| error.to_string());
    let mut bytes = LimitedResponse(Vec::new());
    let (bytes, success) = if serde_json::to_writer(&mut bytes, &response).is_ok() {
        (bytes.0, response.is_ok())
    } else {
        (
            br#"{"Err":"The elevated folder listing is too large"}"#.to_vec(),
            false,
        )
    };
    if writer
        .write_all(&bytes)
        .and_then(|()| writer.flush())
        .is_err()
    {
        2
    } else if success {
        0
    } else {
        1
    }
}

#[cfg(any(windows, target_os = "linux"))]
fn list_requested_directory(request: BrowseRequest) -> Result<Vec<BrowseEntry>> {
    let original = PathBuf::from(native_string(&request.directory));
    if !can_elevate_directory(&original) {
        return Err(BExplorerError::InvalidPath(original));
    }
    let directory = original.canonicalize()?;
    // An alias can resolve onto a different filesystem from its lexical path.
    if !can_elevate_directory(&directory) {
        return Err(BExplorerError::InvalidPath(directory));
    }
    explorer::list_directory_for_elevation(&directory, request.show_hidden)?
        .into_iter()
        .map(BrowseEntry::from_file_entry)
        .collect()
}

#[cfg(any(windows, target_os = "linux"))]
fn response_from_reader(reader: impl Read) -> Result<Vec<BrowseEntry>> {
    read_json::<BrowseResponse>(reader, MAX_RESPONSE_BYTES)?.map_err(BExplorerError::Operation)
}

#[cfg(any(windows, target_os = "linux"))]
fn read_json<T: DeserializeOwned>(reader: impl Read, maximum: usize) -> Result<T> {
    let mut bytes = Vec::new();
    reader.take((maximum + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err(BExplorerError::Operation(
            "The elevated folder listing message is too large".into(),
        ));
    }
    Ok(serde_json::from_slice(&bytes)?)
}

#[cfg(any(windows, target_os = "linux"))]
struct LimitedResponse(Vec<u8>);

#[cfg(any(windows, target_os = "linux"))]
impl Write for LimitedResponse {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_RESPONSE_BYTES - self.0.len() {
            return Err(std::io::Error::other(
                "elevated listing exceeds its transport limit",
            ));
        }
        self.0.write(bytes)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(windows)]
type NativeUnit = u16;
#[cfg(target_os = "linux")]
type NativeUnit = u8;

#[cfg(any(windows, target_os = "linux"))]
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BrowseRequest {
    directory: Vec<NativeUnit>,
    show_hidden: bool,
}

#[cfg(any(windows, target_os = "linux"))]
type BrowseResponse = std::result::Result<Vec<BrowseEntry>, String>;

#[cfg(any(windows, target_os = "linux"))]
#[derive(Debug, Deserialize, Serialize)]
struct BrowseEntry {
    name: Vec<NativeUnit>,
    kind: EntryKind,
    size: Option<u64>,
    modified_time: Option<(bool, Duration)>,
    modified: Option<String>,
    created: Option<String>,
    is_hidden: bool,
}

#[cfg(any(windows, target_os = "linux"))]
impl BrowseEntry {
    fn from_file_entry(entry: FileEntry) -> Result<Self> {
        let name = entry
            .path
            .file_name()
            .ok_or_else(|| BExplorerError::InvalidPath(entry.path.clone()))?;
        Ok(Self {
            name: native_units(name),
            kind: entry.kind,
            size: entry.size,
            modified_time: entry
                .modified_time
                .map(|time| match time.duration_since(UNIX_EPOCH) {
                    Ok(duration) => (false, duration),
                    Err(error) => (true, error.duration()),
                }),
            modified: entry.modified,
            created: entry.created,
            is_hidden: entry.is_hidden,
        })
    }

    fn into_file_entry(self, directory: &Path) -> Result<FileEntry> {
        let name = PathBuf::from(native_string(&self.name));
        let mut components = name.components();
        if self.name.contains(&0)
            || !matches!(components.next(), Some(Component::Normal(value)) if value == name.as_os_str())
            || components.next().is_some()
        {
            return Err(BExplorerError::InvalidPath(name));
        }
        let path = directory.join(&name);
        Ok(FileEntry {
            name: name.to_string_lossy().into_owned(),
            category: explorer::classify_file_category(&path),
            path,
            kind: self.kind,
            drive_kind: None,
            file_system: String::new(),
            free_space: None,
            size: self.size,
            percent_full: None,
            modified_time: self.modified_time.and_then(|(before_epoch, duration)| {
                if before_epoch {
                    UNIX_EPOCH.checked_sub(duration)
                } else {
                    UNIX_EPOCH.checked_add(duration)
                }
            }),
            modified: self.modified,
            created: self.created,
            is_hidden: self.is_hidden,
        })
    }
}

#[cfg(windows)]
fn native_units(value: &OsStr) -> Vec<NativeUnit> {
    use std::os::windows::ffi::OsStrExt;
    value.encode_wide().collect()
}

#[cfg(windows)]
fn native_string(value: &[NativeUnit]) -> OsString {
    use std::os::windows::ffi::OsStringExt;
    OsString::from_wide(value)
}

#[cfg(target_os = "linux")]
fn native_units(value: &OsStr) -> Vec<NativeUnit> {
    use std::os::unix::ffi::OsStrExt;
    value.as_bytes().to_vec()
}

#[cfg(target_os = "linux")]
fn native_string(value: &[NativeUnit]) -> OsString {
    use std::os::unix::ffi::OsStrExt;
    OsStr::from_bytes(value).to_os_string()
}

#[cfg(all(test, any(windows, target_os = "linux")))]
mod tests {
    use std::fs;

    use super::*;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let stamp = std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir()
                .join(format!("bexplorer-browse-{}-{stamp}", std::process::id()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn helper_result(path: &Path, show_hidden: bool) -> (i32, Vec<u8>) {
        let request = serde_json::to_vec(&BrowseRequest {
            directory: native_units(path.as_os_str()),
            show_hidden,
        })
        .unwrap();
        let mut output = Vec::new();
        let code = run_helper(request.as_slice(), &mut output);
        (code, output)
    }

    #[test]
    fn helper_lists_one_directory_and_keeps_native_child_names() {
        let root = Fixture::new();
        let nested = root.0.join("subfolder");
        fs::create_dir(&nested).unwrap();
        fs::write(nested.join("nested.txt"), b"nested").unwrap();
        #[cfg(target_os = "linux")]
        let name = native_string(b"document-\xff.txt");
        #[cfg(windows)]
        let name = OsString::from("document-日本語.txt");
        let file = root.0.join(&name);
        fs::write(&file, b"keep").unwrap();
        let mut entry = explorer::list_directory_for_elevation(&root.0, true)
            .unwrap()
            .into_iter()
            .find(|entry| entry.path == file)
            .unwrap();
        let time = UNIX_EPOCH - Duration::from_secs(80);
        entry.modified_time = Some(time);
        let wire = serde_json::to_vec(&BrowseEntry::from_file_entry(entry).unwrap()).unwrap();
        let decoded: BrowseEntry = serde_json::from_slice(&wire).unwrap();
        assert_eq!(
            decoded.into_file_entry(&root.0).unwrap().modified_time,
            Some(time)
        );

        let (code, output) = helper_result(&root.0, true);
        assert_eq!(code, 0);
        let entries = response_from_reader(output.as_slice())
            .unwrap()
            .into_iter()
            .map(|entry| entry.into_file_entry(&root.0).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(entries.len(), 2);
        assert!(
            entries
                .iter()
                .any(|entry| entry.path == nested && entry.kind == EntryKind::Folder)
        );
        assert!(
            entries
                .iter()
                .any(|entry| entry.path == file && entry.size == Some(4))
        );
    }

    #[test]
    fn rejects_relative_and_archive_paths_and_oversized_requests() {
        let root = Fixture::new();
        let archive = root.0.join("sample.zip");
        fs::write(&archive, b"not a directory").unwrap();
        for invalid in [Path::new("relative"), &archive.join("nested")] {
            assert!(!can_elevate_directory(invalid));
            let (code, output) = helper_result(invalid, true);
            assert_eq!(code, 1);
            assert!(response_from_reader(output.as_slice()).is_err());
        }
        let oversized = std::io::repeat(b' ').take((MAX_REQUEST_BYTES + 1) as u64);
        let mut output = Vec::new();
        assert_eq!(run_helper(oversized, &mut output), 1);
        assert!(
            response_from_reader(output.as_slice())
                .unwrap_err()
                .to_string()
                .contains("too large")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn readonly_helper_preserves_hidden_filter_and_directory_mode() {
        use std::os::unix::fs::PermissionsExt;

        let root = Fixture::new();
        fs::write(root.0.join(".hidden"), b"metadata only").unwrap();
        fs::set_permissions(&root.0, fs::Permissions::from_mode(0o500)).unwrap();
        let (code, output) = helper_result(&root.0, false);
        let mode = fs::metadata(&root.0).unwrap().permissions().mode() & 0o777;
        fs::set_permissions(&root.0, fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(mode, 0o500);
        assert_eq!(code, 0);
        assert!(response_from_reader(output.as_slice()).unwrap().is_empty());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn elevated_helper_lists_symlinks_without_classifying_their_targets() {
        let root = Fixture::new();
        let target = Fixture::new();
        let entry_path = root.0.join("link");
        std::os::unix::fs::symlink(&target.0, &entry_path).unwrap();
        let regular = explorer::list_entries(Some(&root.0), true).unwrap();
        assert_eq!(regular[0].kind, EntryKind::SymlinkFolder);
        let (code, output) = helper_result(&root.0, true);
        assert_eq!(code, 0);
        let entries = response_from_reader(output.as_slice()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, EntryKind::Symlink);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn readable_but_not_searchable_directory_returns_permission_denied() {
        use std::io::ErrorKind;
        use std::os::unix::fs::PermissionsExt;

        let root = Fixture::new();
        fs::write(root.0.join("file.txt"), b"unreadable metadata").unwrap();
        fs::set_permissions(&root.0, fs::Permissions::from_mode(0o400)).unwrap();
        let can_bypass_permissions = fs::metadata(root.0.join(".")).is_ok();
        let result = explorer::list_entries(Some(&root.0), true);
        fs::set_permissions(&root.0, fs::Permissions::from_mode(0o700)).unwrap();
        if !can_bypass_permissions {
            assert!(
                matches!(result, Err(BExplorerError::Io(error)) if error.kind() == ErrorKind::PermissionDenied)
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn rejects_unc_and_windows_devices_and_preserves_native_units() {
        assert!(!can_elevate_directory(Path::new(r"\\server\share\path")));
        assert!(!can_elevate_directory(Path::new(r"\\?\UNC\server\share")));
        assert!(!can_elevate_directory(Path::new(r"\\.\C:\Windows")));
        let units = [0xD800_u16, b'a' as u16];
        assert_eq!(native_units(&native_string(&units)), units);
    }
}
