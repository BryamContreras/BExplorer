use std::ffi::OsString;

use crate::utils::errors::{BExplorerError, Result};

#[cfg(target_os = "windows")]
pub(super) fn run_elevated_current_exe(args: &[OsString]) -> Result<i32> {
    use std::mem::size_of;

    use windows::Win32::Foundation::{CloseHandle, HWND};
    use windows::Win32::System::Threading::{GetExitCodeProcess, INFINITE, WaitForSingleObject};
    use windows::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
    use windows::core::PCWSTR;

    let exe = std::env::current_exe().map_err(|error| BExplorerError::Shell(error.to_string()))?;
    let verb = wide_null("runas");
    let file = wide_os_null(exe.as_os_str());
    let params = wide_null(&join_windows_args(args));

    let mut info = SHELLEXECUTEINFOW::default();
    info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS;
    info.hwnd = HWND::default();
    info.lpVerb = PCWSTR(verb.as_ptr());
    info.lpFile = PCWSTR(file.as_ptr());
    info.lpParameters = PCWSTR(params.as_ptr());
    info.nShow = SW_HIDE.0;

    unsafe {
        ShellExecuteExW(&mut info).map_err(|error| {
            BExplorerError::Shell(format!("Could not request administrator access: {error}"))
        })?;
        if info.hProcess.is_invalid() {
            return Err(BExplorerError::Shell(
                "Administrator process did not start".into(),
            ));
        }
        let _ = WaitForSingleObject(info.hProcess, INFINITE);
        let mut exit_code = 1_u32;
        GetExitCodeProcess(info.hProcess, &mut exit_code).map_err(|error| {
            let _ = CloseHandle(info.hProcess);
            BExplorerError::Shell(format!("Could not read elevated transfer result: {error}"))
        })?;
        let _ = CloseHandle(info.hProcess);
        Ok(exit_code as i32)
    }
}

#[cfg(target_os = "windows")]
fn join_windows_args(args: &[OsString]) -> String {
    args.iter()
        .map(|arg| quote_windows_arg(&arg.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(target_os = "windows")]
fn quote_windows_arg(arg: &str) -> String {
    if !arg.is_empty()
        && !arg
            .chars()
            .any(|ch| ch.is_whitespace() || matches!(ch, '"'))
    {
        return arg.to_string();
    }

    let mut quoted = String::from("\"");
    let mut backslashes = 0;
    for ch in arg.chars() {
        match ch {
            '\\' => backslashes += 1,
            '"' => {
                quoted.extend(std::iter::repeat('\\').take(backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            _ => {
                quoted.extend(std::iter::repeat('\\').take(backslashes));
                backslashes = 0;
                quoted.push(ch);
            }
        }
    }
    quoted.extend(std::iter::repeat('\\').take(backslashes * 2));
    quoted.push('"');
    quoted
}

#[cfg(target_os = "windows")]
fn wide_null(value: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain([0])
        .collect()
}

#[cfg(target_os = "windows")]
fn wide_os_null(value: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    value.encode_wide().chain([0]).collect()
}

#[cfg(not(target_os = "windows"))]
pub(super) fn run_elevated_current_exe(_args: &[OsString]) -> Result<i32> {
    Err(BExplorerError::Shell(
        "Administrator elevation is currently available on Windows only".into(),
    ))
}
