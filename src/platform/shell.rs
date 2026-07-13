use std::ffi::OsString;
#[cfg(not(target_os = "windows"))]
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(not(target_os = "windows"))]
use std::process::Stdio;
#[cfg(target_os = "windows")]
use std::sync::atomic::AtomicBool;

use crate::utils::errors::{BExplorerError, Result};

#[cfg(target_os = "linux")]
mod clipboard;
mod defender;
mod disk;
#[cfg(target_os = "windows")]
mod elevation;

#[cfg(target_os = "windows")]
pub use defender::WindowsDefenderScanResult;

#[cfg_attr(test, allow(dead_code))]
#[derive(Clone, Debug)]
pub struct ClipboardFiles {
    pub paths: Vec<PathBuf>,
    pub cut: bool,
}

#[cfg(target_os = "linux")]
fn copy_text(text: &str) -> Result<()> {
    clipboard::set_text(text)
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn copy_text(text: &str) -> Result<()> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|error| BExplorerError::Clipboard(error.to_string()))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|error| BExplorerError::Clipboard(error.to_string()))
}

#[cfg(target_os = "linux")]
fn read_text() -> Result<String> {
    clipboard::text()
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn read_text() -> Result<String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|error| BExplorerError::Clipboard(error.to_string()))?;
    clipboard
        .get_text()
        .map_err(|error| BExplorerError::Clipboard(error.to_string()))
}

pub fn copy_files(paths: &[PathBuf], cut: bool) -> Result<()> {
    copy_files_platform(paths, cut)
}

pub fn read_files() -> Result<ClipboardFiles> {
    read_files_platform()
}

pub fn clear_clipboard() -> Result<()> {
    clear_clipboard_platform()
}

pub fn open_terminal_at(path: &Path) -> Result<()> {
    let directory = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };

    open_terminal_platform(directory)
}

pub fn open_path(path: &Path) -> Result<()> {
    open_path_platform(path)
}

pub fn open_with(path: &Path) -> Result<()> {
    open_with_platform(path)
}

#[derive(Clone, Debug)]
pub struct OpenWithApplication {
    pub name: String,
    pub(crate) id: String,
    pub(crate) icon_path: Option<PathBuf>,
}

pub fn open_with_applications(path: &Path) -> Result<Vec<OpenWithApplication>> {
    open_with_applications_platform(path)
}

pub fn open_with_application(path: &Path, index: usize) -> Result<()> {
    open_with_application_platform(path, index)
}

pub fn show_properties(path: &Path) -> Result<()> {
    show_properties_platform(path)
}

#[cfg(target_os = "windows")]
pub fn scan_path_with_windows_defender(
    path: &Path,
    cancel: &AtomicBool,
) -> Result<WindowsDefenderScanResult> {
    defender::scan_path_with_windows_defender(path, cancel)
}

#[cfg(target_os = "windows")]
pub fn remove_windows_defender_threats() -> Result<()> {
    defender::remove_windows_defender_threats()
}

#[cfg(target_os = "windows")]
pub fn exclude_windows_defender_paths(paths: &[PathBuf]) -> Result<()> {
    defender::exclude_windows_defender_paths(paths)
}

pub fn open_windows_security() -> Result<()> {
    defender::open_windows_security()
}

pub fn mount_disk_image(path: &Path) -> Result<()> {
    disk::mount_disk_image(path)
}

pub fn mounted_disk_image_root(path: &Path) -> Result<PathBuf> {
    disk::mounted_disk_image_root(path)
}

pub fn suppress_file_explorer_windows_at(path: &Path) -> Result<()> {
    disk::suppress_file_explorer_windows_at(path)
}

pub fn eject_drive(path: &Path) -> Result<()> {
    disk::eject_drive(path)
}

#[cfg(target_os = "windows")]
pub fn run_elevated_current_exe(args: &[OsString]) -> Result<i32> {
    elevation::run_elevated_current_exe(args)
}

#[cfg(target_os = "windows")]
fn copy_files_platform(paths: &[PathBuf], cut: bool) -> Result<()> {
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    use windows::Win32::Foundation::{BOOL, GlobalFree, HANDLE, HGLOBAL, POINT};
    use windows::Win32::System::DataExchange::{
        EmptyClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
    use windows::Win32::System::Ole::{CF_HDROP, DROPEFFECT_COPY, DROPEFFECT_MOVE};
    use windows::Win32::UI::Shell::{CFSTR_PREFERREDDROPEFFECT, DROPFILES};

    if paths.is_empty() {
        return Err(BExplorerError::Clipboard("No files to copy".into()));
    }

    let mut encoded_paths = Vec::new();
    for path in paths {
        let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        if wide.is_empty() {
            continue;
        }
        wide.push(0);
        encoded_paths.extend(wide);
    }
    encoded_paths.push(0);

    let header_size = size_of::<DROPFILES>();
    let bytes_len = header_size + encoded_paths.len() * size_of::<u16>();
    let drop_effect = unsafe { RegisterClipboardFormatW(CFSTR_PREFERREDDROPEFFECT) };
    if drop_effect == 0 {
        return Err(BExplorerError::Clipboard(
            "Could not register Preferred DropEffect".into(),
        ));
    }

    let hdrop = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes_len) }
        .map_err(|error| BExplorerError::Clipboard(error.to_string()))?;
    let effect_mem = unsafe { GlobalAlloc(GMEM_MOVEABLE, size_of::<u32>()) }.map_err(|error| {
        unsafe {
            let _ = GlobalFree(hdrop);
        }
        BExplorerError::Clipboard(error.to_string())
    })?;

    let mut hdrop_owned = true;
    let mut effect_owned = true;
    let result = (|| -> Result<()> {
        let drop_ptr = unsafe { GlobalLock(hdrop) } as *mut u8;
        if drop_ptr.is_null() {
            return Err(BExplorerError::Clipboard(
                "Could not lock file clipboard memory".into(),
            ));
        }
        unsafe {
            let drop_files = DROPFILES {
                pFiles: header_size as u32,
                pt: POINT { x: 0, y: 0 },
                fNC: BOOL(0),
                fWide: BOOL(1),
            };
            ptr::write_unaligned(drop_ptr as *mut DROPFILES, drop_files);
            ptr::copy_nonoverlapping(
                encoded_paths.as_ptr() as *const u8,
                drop_ptr.add(header_size),
                encoded_paths.len() * size_of::<u16>(),
            );
            let _ = GlobalUnlock(hdrop);
        }

        let effect_ptr = unsafe { GlobalLock(effect_mem) } as *mut u32;
        if effect_ptr.is_null() {
            return Err(BExplorerError::Clipboard(
                "Could not lock drop effect memory".into(),
            ));
        }
        unsafe {
            *effect_ptr = if cut {
                DROPEFFECT_MOVE.0
            } else {
                DROPEFFECT_COPY.0
            };
            let _ = GlobalUnlock(effect_mem);
        }

        let _guard = ClipboardGuard::open()?;
        unsafe {
            EmptyClipboard().map_err(|error| BExplorerError::Clipboard(error.to_string()))?;
            SetClipboardData(CF_HDROP.0 as u32, HANDLE(hdrop.0))
                .map_err(|error| BExplorerError::Clipboard(error.to_string()))?;
            hdrop_owned = false;
            SetClipboardData(drop_effect, HANDLE(effect_mem.0))
                .map_err(|error| BExplorerError::Clipboard(error.to_string()))?;
            effect_owned = false;
        }

        Ok(())
    })();

    if result.is_err() {
        unsafe {
            if hdrop_owned {
                let _ = GlobalFree(HGLOBAL(hdrop.0));
            }
            if effect_owned {
                let _ = GlobalFree(HGLOBAL(effect_mem.0));
            }
        }
    }
    result
}

#[cfg(target_os = "windows")]
fn read_files_platform() -> Result<ClipboardFiles> {
    use std::path::PathBuf;

    use windows::Win32::System::DataExchange::{
        GetClipboardData, IsClipboardFormatAvailable, RegisterClipboardFormatW,
    };
    use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
    use windows::Win32::System::Ole::{CF_HDROP, DROPEFFECT_MOVE};
    use windows::Win32::UI::Shell::{CFSTR_PREFERREDDROPEFFECT, DragQueryFileW, HDROP};

    let _guard = ClipboardGuard::open()?;
    unsafe {
        IsClipboardFormatAvailable(CF_HDROP.0 as u32)
            .map_err(|error| BExplorerError::Clipboard(error.to_string()))?;

        let handle = GetClipboardData(CF_HDROP.0 as u32)
            .map_err(|error| BExplorerError::Clipboard(error.to_string()))?;
        let hdrop = HDROP(handle.0);
        let count = DragQueryFileW(hdrop, u32::MAX, None);
        let mut paths = Vec::with_capacity(count as usize);
        for index in 0..count {
            let len = DragQueryFileW(hdrop, index, None);
            if len == 0 {
                continue;
            }
            let mut buffer = vec![0_u16; len as usize + 1];
            let written = DragQueryFileW(hdrop, index, Some(&mut buffer));
            if written == 0 {
                continue;
            }
            buffer.truncate(written as usize);
            paths.push(PathBuf::from(String::from_utf16_lossy(&buffer)));
        }

        if paths.is_empty() {
            return Err(BExplorerError::Clipboard(
                "No file paths in clipboard".into(),
            ));
        }

        let drop_effect = RegisterClipboardFormatW(CFSTR_PREFERREDDROPEFFECT);
        let cut = if drop_effect != 0 && IsClipboardFormatAvailable(drop_effect).is_ok() {
            match GetClipboardData(drop_effect) {
                Ok(effect_handle) => {
                    let effect_global = windows::Win32::Foundation::HGLOBAL(effect_handle.0);
                    let ptr = GlobalLock(effect_global) as *const u32;
                    if ptr.is_null() {
                        false
                    } else {
                        let value = *ptr;
                        let _ = GlobalUnlock(effect_global);
                        value & DROPEFFECT_MOVE.0 != 0
                    }
                }
                Err(_) => false,
            }
        } else {
            false
        };

        Ok(ClipboardFiles { paths, cut })
    }
}

#[cfg(target_os = "windows")]
struct ClipboardGuard;

#[cfg(target_os = "windows")]
impl ClipboardGuard {
    fn open() -> Result<Self> {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::System::DataExchange::OpenClipboard;

        unsafe {
            OpenClipboard(HWND::default())
                .map_err(|error| BExplorerError::Clipboard(error.to_string()))?;
        }
        Ok(Self)
    }
}

#[cfg(target_os = "windows")]
impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::System::DataExchange::CloseClipboard();
        }
    }
}

#[cfg(target_os = "windows")]
fn clear_clipboard_platform() -> Result<()> {
    use windows::Win32::System::DataExchange::EmptyClipboard;

    let _guard = ClipboardGuard::open()?;
    unsafe {
        EmptyClipboard().map_err(|error| BExplorerError::Clipboard(error.to_string()))?;
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn copy_files_platform(paths: &[PathBuf], cut: bool) -> Result<()> {
    if paths.is_empty() {
        return Err(BExplorerError::Clipboard("No files to copy".into()));
    }

    let operation = if cut { "cut" } else { "copy" };
    let text = std::iter::once(operation.to_string())
        .chain(
            paths
                .iter()
                .map(|path| file_uri_from_path(path).unwrap_or_else(|| path.display().to_string())),
        )
        .collect::<Vec<_>>()
        .join("\n");

    // A file list is the portable native representation on X11, Wayland, and
    // macOS. It avoids relying on helper executables such as wl-copy/xclip.
    // GNOME's private MIME retains the distinction between Copy and Cut when
    // such a helper is available.
    #[cfg(all(unix, not(target_os = "macos")))]
    if cut && copy_files_linux_mime(&text).is_ok() {
        return Ok(());
    }

    copy_file_list(paths).or_else(|_| copy_text(&text))
}

#[cfg(not(target_os = "windows"))]
fn read_files_platform() -> Result<ClipboardFiles> {
    #[cfg(all(unix, not(target_os = "macos")))]
    if let Ok(files) = read_files_linux_mime() {
        return Ok(files);
    }

    if let Ok(paths) = read_file_list() {
        return Ok(ClipboardFiles { paths, cut: false });
    }

    let text = read_text()?;
    clipboard_files_from_text(&text)
}

#[cfg(target_os = "linux")]
fn copy_file_list(paths: &[PathBuf]) -> Result<()> {
    clipboard::set_file_list(paths)
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn copy_file_list(paths: &[PathBuf]) -> Result<()> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|error| BExplorerError::Clipboard(error.to_string()))?;
    clipboard
        .set()
        .file_list(paths)
        .map_err(|error| BExplorerError::Clipboard(error.to_string()))
}

#[cfg(target_os = "linux")]
fn read_file_list() -> Result<Vec<PathBuf>> {
    let paths = clipboard::file_list()?
        .into_iter()
        .filter(|path| path.exists())
        .collect::<Vec<_>>();
    if paths.is_empty() {
        return Err(BExplorerError::Clipboard(
            "No file paths in clipboard".into(),
        ));
    }
    Ok(paths)
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn read_file_list() -> Result<Vec<PathBuf>> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|error| BExplorerError::Clipboard(error.to_string()))?;
    let paths = clipboard
        .get()
        .file_list()
        .map_err(|error| BExplorerError::Clipboard(error.to_string()))?
        .into_iter()
        .filter(|path| path.exists())
        .collect::<Vec<_>>();
    if paths.is_empty() {
        return Err(BExplorerError::Clipboard(
            "No file paths in clipboard".into(),
        ));
    }
    Ok(paths)
}

#[cfg(not(target_os = "windows"))]
fn clipboard_files_from_text(text: &str) -> Result<ClipboardFiles> {
    let mut cut = false;
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    let first = lines.next();
    let remaining = match first {
        Some("cut") => {
            cut = true;
            lines.collect::<Vec<_>>()
        }
        Some("copy") => lines.collect::<Vec<_>>(),
        Some(line) => std::iter::once(line).chain(lines).collect::<Vec<_>>(),
        None => Vec::new(),
    };

    let paths = remaining
        .iter()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| clipboard_line_to_path(line))
        .filter(|path| path.exists())
        .collect::<Vec<_>>();
    if paths.is_empty() {
        return Err(BExplorerError::Clipboard(
            "No file paths in clipboard".into(),
        ));
    }
    Ok(ClipboardFiles { paths, cut })
}

#[cfg(all(unix, not(target_os = "macos")))]
fn copy_files_linux_mime(gnome_files: &str) -> Result<()> {
    let uri_list = gnome_files
        .lines()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\r\n");
    linux_set_clipboard_mime("x-special/gnome-copied-files", gnome_files)
        .or_else(|_| linux_set_clipboard_mime("text/uri-list", &uri_list))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn read_files_linux_mime() -> Result<ClipboardFiles> {
    for mime in ["x-special/gnome-copied-files", "text/uri-list"] {
        if let Ok(text) = linux_get_clipboard_mime(mime)
            && let Ok(files) = clipboard_files_from_text(&text)
        {
            return Ok(files);
        }
    }
    Err(BExplorerError::Clipboard(
        "No native file paths in clipboard".into(),
    ))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_set_clipboard_mime(mime: &str, text: &str) -> Result<()> {
    let candidates = if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        [
            vec!["wl-copy", "--type", mime],
            vec!["xclip", "-selection", "clipboard", "-t", mime],
            vec!["xsel", "--clipboard", "--input", "--mime-type", mime],
        ]
    } else {
        [
            vec!["xclip", "-selection", "clipboard", "-t", mime],
            vec!["xsel", "--clipboard", "--input", "--mime-type", mime],
            vec!["wl-copy", "--type", mime],
        ]
    };

    for args in candidates {
        let Some((program, program_args)) = args.split_first() else {
            continue;
        };
        if !command_exists(program) {
            continue;
        }
        let mut child = Command::new(program)
            .args(program_args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| BExplorerError::Clipboard(error.to_string()))?;
        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(text.as_bytes())
                .map_err(|error| BExplorerError::Clipboard(error.to_string()))?;
        }
        let status = child
            .wait()
            .map_err(|error| BExplorerError::Clipboard(error.to_string()))?;
        if status.success() {
            return Ok(());
        }
    }

    Err(BExplorerError::Clipboard(
        "No native clipboard helper found".into(),
    ))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_get_clipboard_mime(mime: &str) -> Result<String> {
    let candidates = if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        [
            vec!["wl-paste", "--type", mime, "--no-newline"],
            vec!["xclip", "-selection", "clipboard", "-t", mime, "-o"],
            vec!["xsel", "--clipboard", "--output", "--mime-type", mime],
        ]
    } else {
        [
            vec!["xclip", "-selection", "clipboard", "-t", mime, "-o"],
            vec!["xsel", "--clipboard", "--output", "--mime-type", mime],
            vec!["wl-paste", "--type", mime, "--no-newline"],
        ]
    };

    for args in candidates {
        let Some((program, program_args)) = args.split_first() else {
            continue;
        };
        if !command_exists(program) {
            continue;
        }
        let output = Command::new(program)
            .args(program_args)
            .output()
            .map_err(|error| BExplorerError::Clipboard(error.to_string()))?;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).to_string());
        }
    }

    Err(BExplorerError::Clipboard(
        "No native clipboard helper found".into(),
    ))
}

#[cfg(not(target_os = "windows"))]
fn clipboard_line_to_path(line: &str) -> Option<PathBuf> {
    path_from_file_uri(line).or_else(|| {
        let path = PathBuf::from(line.trim_matches('"'));
        path.exists().then_some(path)
    })
}

#[cfg(all(unix, not(target_os = "windows")))]
fn file_uri_from_path(path: &Path) -> Option<String> {
    use std::os::unix::ffi::OsStrExt;

    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    let mut uri = String::from("file://");
    for byte in absolute.as_os_str().as_bytes() {
        match *byte {
            b'/' => uri.push('/'),
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                uri.push(*byte as char)
            }
            value => uri.push_str(&format!("%{value:02X}")),
        }
    }
    Some(uri)
}

#[cfg(not(any(unix, target_os = "windows")))]
fn file_uri_from_path(path: &Path) -> Option<String> {
    path.to_str().map(|path| format!("file://{path}"))
}

#[cfg(all(unix, not(target_os = "windows")))]
pub(crate) fn path_from_file_uri(uri: &str) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;

    let rest = uri.strip_prefix("file://")?;
    let path = rest.strip_prefix("localhost").unwrap_or(rest);
    if !path.starts_with('/') {
        return None;
    }
    let bytes = percent_decode(path.as_bytes())?;
    Some(OsString::from_vec(bytes).into())
}

#[cfg(not(any(unix, target_os = "windows")))]
pub(crate) fn path_from_file_uri(uri: &str) -> Option<PathBuf> {
    let path = uri.strip_prefix("file://")?;
    Some(PathBuf::from(path))
}

#[cfg(not(target_os = "windows"))]
fn percent_decode(value: &[u8]) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity(value.len());
    let mut index = 0;
    while index < value.len() {
        if value[index] == b'%' {
            let high = *value.get(index + 1)?;
            let low = *value.get(index + 2)?;
            output.push((hex_value(high)? << 4) | hex_value(low)?);
            index += 3;
        } else {
            output.push(value[index]);
            index += 1;
        }
    }
    Some(output)
}

#[cfg(not(target_os = "windows"))]
fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

#[cfg(not(target_os = "windows"))]
fn command_exists(program: &str) -> bool {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .any(|directory| directory.join(program).is_file())
}

#[cfg(target_os = "linux")]
fn clear_clipboard_platform() -> Result<()> {
    clipboard::clear()
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn clear_clipboard_platform() -> Result<()> {
    copy_text("")
}

#[cfg(target_os = "windows")]
fn open_terminal_platform(directory: &Path) -> Result<()> {
    let has_wt = Command::new("cmd")
        .args(["/C", "where", "wt.exe"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);

    if has_wt {
        Command::new("wt.exe")
            .arg("-d")
            .arg(directory)
            .spawn()
            .map_err(|error| BExplorerError::Shell(error.to_string()))?;
    } else {
        Command::new("cmd")
            .args(["/C", "start", "", "cmd", "/K", "cd", "/d"])
            .arg(directory)
            .spawn()
            .map_err(|error| BExplorerError::Shell(error.to_string()))?;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn open_path_platform(path: &Path) -> Result<()> {
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;

    use windows::Win32::UI::Shell::{SHELLEXECUTEINFOW, ShellExecuteExW};
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::PCWSTR;

    let file = path
        .as_os_str()
        .encode_wide()
        .chain([0])
        .collect::<Vec<_>>();
    let working_directory = windows_launch_working_directory(path).map(|directory| {
        directory
            .as_os_str()
            .encode_wide()
            .chain([0])
            .collect::<Vec<_>>()
    });

    let mut info = SHELLEXECUTEINFOW::default();
    info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
    info.lpFile = PCWSTR(file.as_ptr());
    info.lpDirectory = working_directory
        .as_ref()
        .map_or(PCWSTR::null(), |directory| PCWSTR(directory.as_ptr()));
    info.nShow = SW_SHOWNORMAL.0;

    unsafe { ShellExecuteExW(&mut info) }.map_err(|error| {
        BExplorerError::Shell(format!("Could not open {}: {error}", path.display()))
    })
}

#[cfg(target_os = "windows")]
fn windows_launch_working_directory(path: &Path) -> Option<&Path> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default();
    if extension.eq_ignore_ascii_case("lnk") || extension.eq_ignore_ascii_case("url") {
        // A shortcut can define its own `Start in` directory. Passing one here
        // would override the launch context stored by the Windows shell.
        None
    } else {
        path.parent()
    }
}

#[cfg(not(target_os = "windows"))]
fn open_path_platform(path: &Path) -> Result<()> {
    open::that(path).map_err(|error| {
        BExplorerError::Shell(format!("Could not open {}: {error}", path.display()))
    })
}

#[cfg(target_os = "windows")]
fn open_with_platform(path: &Path) -> Result<()> {
    // Use the system copy explicitly. Packaged applications can inherit a
    // reduced PATH where System32 is not listed, even though it is available
    // to the Windows shell.
    let open_with = std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .map(|root| root.join("System32").join("OpenWith.exe"))
        .filter(|candidate| candidate.is_file())
        .unwrap_or_else(|| PathBuf::from("OpenWith.exe"));
    Command::new(open_with)
        .arg(path)
        .spawn()
        .map_err(|error| BExplorerError::Shell(error.to_string()))?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn open_with_applications_platform(path: &Path) -> Result<Vec<OpenWithApplication>> {
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemFree};
    use windows::Win32::UI::Shell::{ASSOC_FILTER_RECOMMENDED, IAssocHandler, SHAssocEnumHandlers};
    use windows::core::PCWSTR;

    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| !extension.is_empty())
        .map(|extension| format!(".{extension}"));
    let Some(extension) = extension else {
        return Ok(Vec::new());
    };
    let extension = extension.encode_utf16().chain([0]).collect::<Vec<_>>();
    let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
    let result = (|| -> Result<Vec<OpenWithApplication>> {
        let handlers =
            unsafe { SHAssocEnumHandlers(PCWSTR(extension.as_ptr()), ASSOC_FILTER_RECOMMENDED) }
                .map_err(|error| BExplorerError::Shell(error.to_string()))?;
        let mut applications = Vec::new();
        loop {
            let mut item: [Option<IAssocHandler>; 1] = [None];
            let mut fetched = 0;
            if unsafe { handlers.Next(&mut item, Some(&mut fetched)) }.is_err() || fetched == 0 {
                break;
            }
            let Some(handler) = item[0].take() else {
                continue;
            };
            let handler_id_ptr = unsafe { handler.GetName() }
                .map_err(|error| BExplorerError::Shell(error.to_string()))?;
            let handler_id = unsafe { handler_id_ptr.to_string() }
                .map_err(|error| BExplorerError::Shell(error.to_string()))?;
            unsafe { CoTaskMemFree(Some(handler_id_ptr.0.cast())) };
            let name = unsafe { handler.GetUIName() }
                .ok()
                .map(|value| {
                    let text = unsafe { value.to_string() }.unwrap_or_default();
                    unsafe { CoTaskMemFree(Some(value.0.cast())) };
                    text
                })
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| "Aplicación".into());
            if !applications
                .iter()
                .any(|application: &OpenWithApplication| {
                    application.id.eq_ignore_ascii_case(&handler_id)
                })
            {
                let mut icon_location = windows::core::PWSTR::null();
                let mut icon_index = 0;
                let icon_path = unsafe {
                    handler
                        .GetIconLocation(&mut icon_location, &mut icon_index)
                        .ok()
                        .and_then(|_| {
                            let path = icon_location.to_string().ok();
                            CoTaskMemFree(Some(icon_location.0.cast()));
                            path
                        })
                }
                .filter(|path| !path.trim().is_empty())
                .map(|path| {
                    path.rsplit_once(',')
                        .filter(|(_, index)| index.trim().parse::<i32>().is_ok())
                        .map(|(path, _)| path.to_owned())
                        .unwrap_or(path)
                })
                .map(PathBuf::from)
                .filter(|path| path.is_file())
                .or_else(|| {
                    Path::new(&handler_id)
                        .is_file()
                        .then(|| PathBuf::from(&handler_id))
                })
                .or_else(|| Some(path.to_path_buf()));
                applications.push(OpenWithApplication {
                    name,
                    id: handler_id,
                    icon_path,
                });
            }
        }
        Ok(applications)
    })();
    if initialized {
        unsafe { windows::Win32::System::Com::CoUninitialize() };
    }
    result
}

#[cfg(target_os = "windows")]
fn open_with_application_platform(path: &Path, index: usize) -> Result<()> {
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemFree};
    use windows::Win32::UI::Shell::{ASSOC_FILTER_RECOMMENDED, IAssocHandler, SHAssocEnumHandlers};
    use windows::core::PCWSTR;

    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| !extension.is_empty())
        .map(|extension| format!(".{extension}"))
        .ok_or_else(|| BExplorerError::Shell("El archivo no tiene extensión".into()))?;
    let extension = extension.encode_utf16().chain([0]).collect::<Vec<_>>();
    let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
    let result = (|| -> Result<()> {
        let handlers =
            unsafe { SHAssocEnumHandlers(PCWSTR(extension.as_ptr()), ASSOC_FILTER_RECOMMENDED) }
                .map_err(|error| BExplorerError::Shell(error.to_string()))?;
        let mut selected: Option<IAssocHandler> = None;
        let mut unique_index = 0;
        let mut seen_ids = Vec::<String>::new();
        loop {
            let mut item: [Option<IAssocHandler>; 1] = [None];
            let mut fetched = 0;
            if unsafe { handlers.Next(&mut item, Some(&mut fetched)) }.is_err() || fetched == 0 {
                break;
            }
            let Some(candidate) = item[0].take() else {
                continue;
            };
            let id_ptr = unsafe { candidate.GetName() }
                .map_err(|error| BExplorerError::Shell(error.to_string()))?;
            let id = unsafe { id_ptr.to_string() }
                .map_err(|error| BExplorerError::Shell(error.to_string()))?;
            unsafe { CoTaskMemFree(Some(id_ptr.0.cast())) };
            if seen_ids.iter().any(|seen| seen.eq_ignore_ascii_case(&id)) {
                continue;
            }
            seen_ids.push(id);
            if unique_index == index {
                selected = Some(candidate);
                break;
            }
            unique_index += 1;
        }
        let Some(handler) = selected else {
            return Err(BExplorerError::Shell(
                "La aplicación seleccionada ya no está disponible".into(),
            ));
        };
        let handler_name_ptr = unsafe { handler.GetName() }
            .map_err(|error| BExplorerError::Shell(error.to_string()))?;
        let handler_name = unsafe { handler_name_ptr.to_string() }
            .map_err(|error| BExplorerError::Shell(error.to_string()))?;
        unsafe { CoTaskMemFree(Some(handler_name_ptr.0.cast())) };
        if Path::new(&handler_name).is_file() {
            return launch_association_command(&format!("\"{handler_name}\" \"%1\""), path);
        }
        match query_association_command(&handler_name) {
            Ok(command) => launch_association_command(&command, path),
            // Packaged/UWP handlers such as Fotos expose no Win32 command.
            // Let the Windows shell resolve those handlers instead of
            // reporting a misleading “command not found” error.
            Err(_) => open_path_platform(path),
        }
    })();
    if initialized {
        unsafe { windows::Win32::System::Com::CoUninitialize() };
    }
    result
}

#[cfg(target_os = "windows")]
fn query_association_command(progid: &str) -> Result<String> {
    use std::os::windows::ffi::OsStrExt;

    use windows::Win32::UI::Shell::{ASSOCF_NONE, ASSOCSTR_COMMAND, AssocQueryStringW};
    use windows::core::{PCWSTR, PWSTR};

    let progid = std::ffi::OsStr::new(progid)
        .encode_wide()
        .chain([0])
        .collect::<Vec<_>>();
    let mut length = 0_u32;
    unsafe {
        let _ = AssocQueryStringW(
            ASSOCF_NONE,
            ASSOCSTR_COMMAND,
            PCWSTR(progid.as_ptr()),
            PCWSTR::null(),
            PWSTR::null(),
            &mut length,
        );
    }
    if length == 0 {
        return Err(BExplorerError::Shell(
            "La aplicación no tiene un comando de apertura".into(),
        ));
    }
    let mut command = vec![0_u16; length as usize + 1];
    unsafe {
        AssocQueryStringW(
            ASSOCF_NONE,
            ASSOCSTR_COMMAND,
            PCWSTR(progid.as_ptr()),
            PCWSTR::null(),
            PWSTR(command.as_mut_ptr()),
            &mut length,
        )
        .ok()
        .map_err(|error| BExplorerError::Shell(error.to_string()))?;
    }
    Ok(String::from_utf16_lossy(
        &command[..length.saturating_sub(1) as usize],
    ))
}

#[cfg(target_os = "windows")]
fn launch_association_command(command: &str, path: &Path) -> Result<()> {
    let path_argument = format!("\"{}\"", path.to_string_lossy().replace('"', "\\\""));
    let mut command = command.to_owned();
    for placeholder in ["%1", "%L", "%l", "%*", "%~1"] {
        // Handle both `%1` and `"%1"` association templates without creating
        // doubled quotes for applications that already quote the argument.
        command = command.replace(&format!("\"{placeholder}\""), &path_argument);
        command = command.replace(placeholder, &path_argument);
    }
    let (program, parameters) = split_association_command(&command)?;
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::UI::Shell::{SHELLEXECUTEINFOW, ShellExecuteExW};
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::PCWSTR;

    let program = std::ffi::OsStr::new(&program)
        .encode_wide()
        .chain([0])
        .collect::<Vec<_>>();
    let parameters = std::ffi::OsStr::new(&parameters)
        .encode_wide()
        .chain([0])
        .collect::<Vec<_>>();
    let mut info = SHELLEXECUTEINFOW::default();
    info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
    info.lpFile = PCWSTR(program.as_ptr());
    info.lpParameters = PCWSTR(parameters.as_ptr());
    info.nShow = SW_SHOWNORMAL.0;
    unsafe { ShellExecuteExW(&mut info) }
        .map_err(|error| BExplorerError::Shell(error.to_string()))?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn split_association_command(command: &str) -> Result<(String, String)> {
    let command = command.trim();
    if command.is_empty() {
        return Err(BExplorerError::Shell("Comando de apertura vacío".into()));
    }
    if let Some(rest) = command.strip_prefix('"') {
        let end = rest
            .find('"')
            .ok_or_else(|| BExplorerError::Shell("Comando de apertura inválido".into()))?;
        let program = rest[..end].to_owned();
        return Ok((program, rest[end + 1..].trim().to_owned()));
    }
    let mut parts = command.splitn(2, char::is_whitespace);
    let program = parts.next().unwrap_or_default().to_owned();
    let parameters = parts.next().unwrap_or_default().trim().to_owned();
    Ok((program, parameters))
}

#[cfg(target_os = "windows")]
fn show_properties_platform(path: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;

    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Shell::{SHOP_FILEPATH, SHObjectProperties};
    use windows::core::PCWSTR;

    let file: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let ok = unsafe {
        SHObjectProperties(
            HWND::default(),
            SHOP_FILEPATH,
            PCWSTR(file.as_ptr()),
            PCWSTR::null(),
        )
    };

    if ok.0 != 0 {
        Ok(())
    } else {
        Err(BExplorerError::Shell(format!(
            "Could not open properties for {}",
            path.display()
        )))
    }
}

#[cfg(target_os = "macos")]
fn open_terminal_platform(directory: &Path) -> Result<()> {
    Command::new("open")
        .arg("-a")
        .arg("Terminal")
        .arg(directory)
        .spawn()
        .map_err(|error| BExplorerError::Shell(error.to_string()))?;
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_with_platform(path: &Path) -> Result<()> {
    if command_exists("mimeopen") && Command::new("mimeopen").arg("-d").arg(path).spawn().is_ok() {
        return Ok(());
    }
    for program in ["gio", "xdg-open"] {
        if !command_exists(program) {
            continue;
        }
        let mut command = Command::new(program);
        if program == "gio" {
            command.arg("open");
        }
        if command.arg(path).spawn().is_ok() {
            return Ok(());
        }
    }
    open::that(path).map(|_| ()).map_err(|error| {
        BExplorerError::Shell(format!("Could not open {}: {error}", path.display()))
    })
}

#[cfg(not(any(target_os = "windows", all(unix, not(target_os = "macos")))))]
fn open_with_platform(_path: &Path) -> Result<()> {
    Err(BExplorerError::Shell(
        "Open with is currently available on Windows only".into(),
    ))
}

#[cfg(not(target_os = "windows"))]
fn open_with_applications_platform(_path: &Path) -> Result<Vec<OpenWithApplication>> {
    Ok(Vec::new())
}

#[cfg(not(target_os = "windows"))]
fn open_with_application_platform(_path: &Path, _index: usize) -> Result<()> {
    Err(BExplorerError::Shell(
        "La selección de aplicaciones está disponible actualmente en Windows".into(),
    ))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn show_properties_platform(path: &Path) -> Result<()> {
    // KiO owns Plasma's native Properties dialog. Prefer the KDE 6 client,
    // but retain the KDE 5 name for distributions that still ship it.
    let Some(client) = ["kioclient6", "kioclient5"]
        .into_iter()
        .find(|candidate| command_exists(candidate))
    else {
        return Err(BExplorerError::Shell(
            "No native properties client is available on this Linux desktop".into(),
        ));
    };
    let target = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    Command::new(client)
        .arg("openProperties")
        .arg(target)
        .spawn()
        .map_err(|error| {
            BExplorerError::Shell(format!("Could not open native properties: {error}"))
        })?;
    Ok(())
}

#[cfg(not(any(target_os = "windows", all(unix, not(target_os = "macos")))))]
fn show_properties_platform(_path: &Path) -> Result<()> {
    Err(BExplorerError::Shell(
        "Properties are not available on this platform yet".into(),
    ))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_terminal_platform(directory: &Path) -> Result<()> {
    let terminals = [
        "xdg-terminal-exec",
        "x-terminal-emulator",
        "gnome-terminal",
        "konsole",
        "xfce4-terminal",
        "mate-terminal",
        "lxterminal",
        "tilix",
        "wezterm",
        "foot",
        "alacritty",
        "kitty",
        "terminator",
    ];

    for terminal in terminals {
        if Command::new(terminal)
            .current_dir(directory)
            .spawn()
            .is_ok()
        {
            return Ok(());
        }
    }

    Err(BExplorerError::Shell(
        "Could not find a terminal application".into(),
    ))
}

#[cfg(not(any(target_os = "windows", target_os = "macos", unix)))]
fn open_terminal_platform(_directory: &Path) -> Result<()> {
    Err(BExplorerError::Shell(
        "Could not find a terminal application".into(),
    ))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn roundtrips_unix_file_uri_with_spaces() {
        let path = Path::new("/tmp/BExplorer Test/file name.txt");
        let uri = file_uri_from_path(path).expect("uri");

        assert_eq!(uri, "file:///tmp/BExplorer%20Test/file%20name.txt");
        assert_eq!(path_from_file_uri(&uri), Some(path.to_path_buf()));
    }

    #[cfg(unix)]
    #[test]
    fn parses_localhost_file_uri() {
        assert_eq!(
            path_from_file_uri("file://localhost/tmp/example.txt"),
            Some(PathBuf::from("/tmp/example.txt"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn parses_gnome_copied_files_clipboard_text() {
        let files = clipboard_files_from_text("cut\nfile:///tmp\n").expect("clipboard files");

        assert!(files.cut);
        assert_eq!(files.paths, vec![PathBuf::from("/tmp")]);
    }
}
