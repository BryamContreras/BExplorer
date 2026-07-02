use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;

use crate::utils::errors::{BExplorerError, Result};

mod defender;
mod disk;
mod elevation;

#[allow(unused_imports)]
pub use defender::{WindowsDefenderScanResult, WindowsDefenderThreat};

#[cfg_attr(test, allow(dead_code))]
#[derive(Clone, Debug)]
pub struct ClipboardFiles {
    pub paths: Vec<PathBuf>,
    pub cut: bool,
}

pub fn copy_text(text: &str) -> Result<()> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|error| BExplorerError::Clipboard(error.to_string()))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|error| BExplorerError::Clipboard(error.to_string()))
}

pub fn read_text() -> Result<String> {
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

pub fn open_with(path: &Path) -> Result<()> {
    open_with_platform(path)
}

pub fn show_properties(path: &Path) -> Result<()> {
    show_properties_platform(path)
}

pub fn scan_path_with_windows_defender(
    path: &Path,
    cancel: &AtomicBool,
) -> Result<WindowsDefenderScanResult> {
    defender::scan_path_with_windows_defender(path, cancel)
}

pub fn remove_windows_defender_threats() -> Result<()> {
    defender::remove_windows_defender_threats()
}

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
fn copy_files_platform(paths: &[PathBuf], _cut: bool) -> Result<()> {
    let text = paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join("\n");
    copy_text(&text)
}

#[cfg(not(target_os = "windows"))]
fn read_files_platform() -> Result<ClipboardFiles> {
    let text = read_text()?;
    let paths = text
        .lines()
        .map(|line| line.trim().trim_matches('"'))
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .collect::<Vec<_>>();
    if paths.is_empty() {
        return Err(BExplorerError::Clipboard(
            "No file paths in clipboard".into(),
        ));
    }
    Ok(ClipboardFiles { paths, cut: false })
}

#[cfg(not(target_os = "windows"))]
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
fn open_with_platform(path: &Path) -> Result<()> {
    Command::new("rundll32.exe")
        .arg("shell32.dll,OpenAs_RunDLL")
        .arg(path)
        .spawn()
        .map_err(|error| BExplorerError::Shell(error.to_string()))?;
    Ok(())
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

#[cfg(not(target_os = "windows"))]
fn open_with_platform(_path: &Path) -> Result<()> {
    Err(BExplorerError::Shell(
        "Open with is currently available on Windows only".into(),
    ))
}

#[cfg(not(target_os = "windows"))]
fn show_properties_platform(_path: &Path) -> Result<()> {
    Err(BExplorerError::Shell(
        "Properties are currently available on Windows only".into(),
    ))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_terminal_platform(directory: &Path) -> Result<()> {
    let terminals = [
        "x-terminal-emulator",
        "gnome-terminal",
        "konsole",
        "xfce4-terminal",
        "alacritty",
        "kitty",
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
