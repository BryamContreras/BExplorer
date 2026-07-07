use std::path::{Path, PathBuf};
#[cfg(target_os = "windows")]
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;
#[cfg(target_os = "windows")]
use std::sync::atomic::Ordering;
#[cfg(target_os = "windows")]
use std::time::Duration;

use crate::utils::errors::{BExplorerError, Result};

#[derive(Clone, Debug)]
pub struct WindowsDefenderThreat {
    pub name: String,
    pub path: Option<PathBuf>,
    pub status: String,
}

#[derive(Clone, Debug)]
pub struct WindowsDefenderScanResult {
    pub target: PathBuf,
    pub exit_code: Option<i32>,
    pub output: String,
    pub threats: Vec<WindowsDefenderThreat>,
}

#[cfg(target_os = "windows")]
pub(super) fn scan_path_with_windows_defender(
    path: &Path,
    cancel: &AtomicBool,
) -> Result<WindowsDefenderScanResult> {
    if let Some(mp_cmd_run) = windows_defender_command() {
        let mut command = Command::new(&mp_cmd_run);
        command.args(["-Scan", "-ScanType", "3", "-File"]);
        command.arg(path);
        hide_command_window(&mut command);
        let (exit_code, output) = run_defender_command(command, cancel)?;
        let mut threats = query_windows_defender_threats_for(path);
        add_output_attention_threat(path, exit_code, &output, &mut threats);
        return Ok(WindowsDefenderScanResult {
            target: path.to_path_buf(),
            exit_code,
            output,
            threats,
        });
    }

    let mut command = Command::new("powershell.exe");
    command.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        "& { param([string]$ScanPath) Start-MpScan -ScanPath $ScanPath -ScanType CustomScan }",
    ]);
    command.arg(path);
    hide_command_window(&mut command);
    let (exit_code, output) = run_defender_command(command, cancel)?;
    let mut threats = query_windows_defender_threats_for(path);
    add_output_attention_threat(path, exit_code, &output, &mut threats);
    Ok(WindowsDefenderScanResult {
        target: path.to_path_buf(),
        exit_code,
        output,
        threats,
    })
}

#[cfg(target_os = "windows")]
fn run_defender_command(
    mut command: Command,
    cancel: &AtomicBool,
) -> Result<(Option<i32>, String)> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| BExplorerError::Shell(error.to_string()))?;

    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(BExplorerError::Operation(
                "Windows Defender scan cancelled".into(),
            ));
        }
        if child
            .try_wait()
            .map_err(|error| BExplorerError::Shell(error.to_string()))?
            .is_some()
        {
            let output = child
                .wait_with_output()
                .map_err(|error| BExplorerError::Shell(error.to_string()))?;
            let mut text = String::new();
            text.push_str(&String::from_utf8_lossy(&output.stdout));
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            return Ok((output.status.code(), text));
        }
        std::thread::sleep(Duration::from_millis(120));
    }
}

#[cfg(target_os = "windows")]
fn add_output_attention_threat(
    path: &Path,
    exit_code: Option<i32>,
    output: &str,
    threats: &mut Vec<WindowsDefenderThreat>,
) {
    if !threats.is_empty() || matches!(exit_code, Some(0) | None) {
        return;
    }
    let status = output
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("Action required")
        .to_string();
    threats.push(WindowsDefenderThreat {
        name: "Windows Defender alert".into(),
        path: Some(path.to_path_buf()),
        status,
    });
}

#[cfg(target_os = "windows")]
fn query_windows_defender_threats_for(target: &Path) -> Vec<WindowsDefenderThreat> {
    match query_windows_defender_threats() {
        Ok(threats) => threats
            .into_iter()
            .filter(|threat| {
                threat
                    .path
                    .as_ref()
                    .is_some_and(|path| path_matches_scan_target(path, target))
            })
            .collect(),
        Err(error) => {
            crate::utils::log::error(format!("Could not query Defender threats: {error}"));
            Vec::new()
        }
    }
}

#[cfg(target_os = "windows")]
fn query_windows_defender_threats() -> Result<Vec<WindowsDefenderThreat>> {
    let mut command = Command::new("powershell.exe");
    command.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        "& { $items = @(Get-MpThreatDetection | Select-Object ThreatName,ActionSuccess,Resources,ThreatStatusID,ThreatStatusErrorCode); if ($items.Count -eq 0) { '[]' } else { $items | ConvertTo-Json -Depth 5 -Compress } }",
    ]);
    hide_command_window(&mut command);
    let output = command
        .output()
        .map_err(|error| BExplorerError::Shell(error.to_string()))?;
    if !output.status.success() {
        return Err(BExplorerError::Shell(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }

    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let value: serde_json::Value = serde_json::from_str(&text)?;
    let items = match value {
        serde_json::Value::Array(items) => items,
        item @ serde_json::Value::Object(_) => vec![item],
        _ => Vec::new(),
    };

    let mut threats = Vec::new();
    for item in items {
        let name = json_string(&item, "ThreatName").unwrap_or_else(|| "Threat".into());
        let status = json_string(&item, "ThreatStatusID")
            .or_else(|| json_string(&item, "ThreatStatusErrorCode"))
            .unwrap_or_else(|| {
                if json_bool(&item, "ActionSuccess").unwrap_or(false) {
                    "Remediated".into()
                } else {
                    "Action required".into()
                }
            });
        let resources = json_strings(&item, "Resources");
        if resources.is_empty() {
            threats.push(WindowsDefenderThreat {
                name,
                path: None,
                status,
            });
            continue;
        }
        for resource in resources {
            threats.push(WindowsDefenderThreat {
                name: name.clone(),
                path: defender_resource_path(&resource),
                status: status.clone(),
            });
        }
    }
    dedupe_windows_defender_threats(&mut threats);
    Ok(threats)
}

#[cfg(target_os = "windows")]
pub(super) fn remove_windows_defender_threats() -> Result<()> {
    run_defender_powershell_action("Remove-MpThreat")
}

#[cfg(target_os = "windows")]
pub(super) fn exclude_windows_defender_paths(paths: &[PathBuf]) -> Result<()> {
    if paths.is_empty() {
        return Err(BExplorerError::Operation(
            "No exclusion target selected".into(),
        ));
    }
    let mut command = Command::new("powershell.exe");
    command.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        "& { foreach ($TargetPath in $args) { Add-MpPreference -ExclusionPath $TargetPath -ErrorAction Stop } }",
    ]);
    command.args(paths);
    hide_command_window(&mut command);
    let output = command
        .output()
        .map_err(|error| BExplorerError::Shell(error.to_string()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(BExplorerError::Shell(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

#[cfg(target_os = "windows")]
pub(super) fn open_windows_security() -> Result<()> {
    open_windows_uri("windowsdefender://threat")
        .or_else(|_| open_windows_uri("ms-settings:windowsdefender"))
}

#[cfg(target_os = "windows")]
fn run_defender_powershell_action(script: &str) -> Result<()> {
    let mut command = Command::new("powershell.exe");
    command.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        script,
    ]);
    hide_command_window(&mut command);
    let output = command
        .output()
        .map_err(|error| BExplorerError::Shell(error.to_string()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(BExplorerError::Shell(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

#[cfg(target_os = "windows")]
fn open_windows_uri(uri: &str) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;

    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::PCWSTR;

    let uri: Vec<u16> = std::ffi::OsStr::new(uri).encode_wide().chain([0]).collect();
    let result = unsafe {
        ShellExecuteW(
            HWND::default(),
            PCWSTR::null(),
            PCWSTR(uri.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };

    let result_code = result.0 as isize;
    if result_code > 32 {
        Ok(())
    } else {
        Err(BExplorerError::Shell(format!(
            "Could not open Windows URI, ShellExecuteW returned {}",
            result_code
        )))
    }
}

#[cfg(target_os = "windows")]
fn json_string(value: &serde_json::Value, key: &str) -> Option<String> {
    let value = value.get(key)?;
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        serde_json::Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

#[cfg(target_os = "windows")]
fn json_bool(value: &serde_json::Value, key: &str) -> Option<bool> {
    value.get(key)?.as_bool()
}

#[cfg(target_os = "windows")]
fn json_strings(value: &serde_json::Value, key: &str) -> Vec<String> {
    let Some(value) = value.get(key) else {
        return Vec::new();
    };
    match value {
        serde_json::Value::Array(items) => items
            .iter()
            .filter_map(|item| item.as_str().map(ToOwned::to_owned))
            .collect(),
        serde_json::Value::String(text) => vec![text.clone()],
        _ => Vec::new(),
    }
}

#[cfg(target_os = "windows")]
fn defender_resource_path(resource: &str) -> Option<PathBuf> {
    let mut value = resource.trim();
    value = value.strip_prefix("file:").unwrap_or(value);
    value = value.strip_prefix("containerfile:").unwrap_or(value);
    value = value.trim_start_matches('_');
    if value.is_empty() {
        None
    } else {
        Some(PathBuf::from(value))
    }
}

#[cfg(target_os = "windows")]
fn path_matches_scan_target(path: &Path, target: &Path) -> bool {
    let path = normalize_windows_path(path);
    let target = normalize_windows_path(target);
    path == target || path.starts_with(&format!("{target}\\")) || target.starts_with(&path)
}

#[cfg(target_os = "windows")]
fn normalize_windows_path(path: &Path) -> String {
    path.display()
        .to_string()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

#[cfg(target_os = "windows")]
fn dedupe_windows_defender_threats(threats: &mut Vec<WindowsDefenderThreat>) {
    let mut seen = std::collections::BTreeSet::new();
    threats.retain(|threat| {
        seen.insert((
            threat.name.to_lowercase(),
            threat
                .path
                .as_ref()
                .map(|path| normalize_windows_path(path))
                .unwrap_or_default(),
        ))
    });
}

#[cfg(target_os = "windows")]
fn hide_command_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(target_os = "windows")]
#[allow(dead_code)]
fn windows_defender_command() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(program_data) = std::env::var_os("ProgramData") {
        let platform = PathBuf::from(program_data)
            .join("Microsoft")
            .join("Windows Defender")
            .join("Platform");
        if let Ok(entries) = std::fs::read_dir(platform) {
            let mut platform_dirs = entries
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect::<Vec<_>>();
            platform_dirs.sort_by(|left, right| right.file_name().cmp(&left.file_name()));
            candidates.extend(
                platform_dirs
                    .into_iter()
                    .map(|path| path.join("MpCmdRun.exe")),
            );
        }
    }
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        candidates.push(
            PathBuf::from(program_files)
                .join("Windows Defender")
                .join("MpCmdRun.exe"),
        );
    }
    candidates.into_iter().find(|path| path.is_file())
}

#[cfg(not(target_os = "windows"))]
pub(super) fn scan_path_with_windows_defender(
    path: &Path,
    _cancel: &AtomicBool,
) -> Result<WindowsDefenderScanResult> {
    Err(BExplorerError::Shell(format!(
        "Windows Defender scan is only available on Windows: {}",
        path.display()
    )))
}

#[cfg(not(target_os = "windows"))]
pub(super) fn remove_windows_defender_threats() -> Result<()> {
    Err(BExplorerError::Shell(
        "Windows Defender threat removal is only available on Windows".into(),
    ))
}

#[cfg(not(target_os = "windows"))]
pub(super) fn exclude_windows_defender_paths(_paths: &[PathBuf]) -> Result<()> {
    Err(BExplorerError::Shell(
        "Windows Defender exclusions are only available on Windows".into(),
    ))
}

#[cfg(not(target_os = "windows"))]
pub(super) fn open_windows_security() -> Result<()> {
    Err(BExplorerError::Shell(
        "Windows Security is only available on Windows".into(),
    ))
}
