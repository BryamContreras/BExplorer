#[cfg(target_os = "windows")]
use std::path::{Path, PathBuf};
#[cfg(target_os = "windows")]
use std::process::{Command, Stdio};
#[cfg(target_os = "windows")]
use std::sync::atomic::AtomicBool;
#[cfg(target_os = "windows")]
use std::sync::atomic::Ordering;
#[cfg(target_os = "windows")]
use std::time::Duration;

use crate::utils::errors::{BExplorerError, Result};

#[cfg(target_os = "windows")]
#[derive(Clone, Debug)]
pub struct WindowsDefenderThreat {
    pub name: String,
    pub path: Option<PathBuf>,
}

#[cfg(target_os = "windows")]
#[derive(Clone, Debug)]
pub struct WindowsDefenderScanResult {
    pub exit_code: Option<i32>,
    pub threats: Vec<WindowsDefenderThreat>,
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DefenderScanDisposition {
    DetectOnly,
    Remediate,
}

#[cfg(target_os = "windows")]
pub(super) fn scan_path_with_windows_defender(
    path: &Path,
    cancel: &AtomicBool,
) -> Result<WindowsDefenderScanResult> {
    scan_path_with_windows_defender_with_disposition(
        path,
        cancel,
        DefenderScanDisposition::DetectOnly,
    )
}

#[cfg(target_os = "windows")]
pub(super) fn remediate_paths_with_windows_defender(paths: &[PathBuf]) -> Result<usize> {
    let cancel = AtomicBool::new(false);
    for path in paths {
        let result = scan_path_with_windows_defender_with_disposition(
            path,
            &cancel,
            DefenderScanDisposition::Remediate,
        )?;
        // MpCmdRun can report a non-zero exit code when it finds a threat,
        // even after it has successfully quarantined or removed the file.
        // The removed path is the authoritative result in that case.
        let removed_by_defender = !path.exists();
        if !removed_by_defender && !matches!(result.exit_code, Some(0) | None) {
            return Err(BExplorerError::Operation(format!(
                "Microsoft Defender could not confirm remediation for {} (exit code {})",
                path.display(),
                result
                    .exit_code
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "unknown".into())
            )));
        }
    }
    Ok(paths.len())
}

#[cfg(target_os = "windows")]
fn scan_path_with_windows_defender_with_disposition(
    path: &Path,
    cancel: &AtomicBool,
    disposition: DefenderScanDisposition,
) -> Result<WindowsDefenderScanResult> {
    if let Some(mp_cmd_run) = windows_defender_command() {
        let mut command = Command::new(&mp_cmd_run);
        command.args(["-Scan", "-ScanType", "3", "-File"]);
        command.arg(path);
        if disposition == DefenderScanDisposition::DetectOnly {
            command.arg("-DisableRemediation");
        }
        hide_command_window(&mut command);
        let (exit_code, output) = run_defender_command(command, cancel)?;
        let mut threats = query_windows_defender_threats_for(path);
        add_output_attention_threat(path, exit_code, &output, &mut threats);
        return Ok(WindowsDefenderScanResult { exit_code, threats });
    }

    if disposition == DefenderScanDisposition::DetectOnly {
        return Err(BExplorerError::Operation(
            "This version of Microsoft Defender does not support detection-only scans".into(),
        ));
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
    Ok(WindowsDefenderScanResult { exit_code, threats })
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
    _output: &str,
    threats: &mut Vec<WindowsDefenderThreat>,
) {
    if !threats.is_empty() || matches!(exit_code, Some(0) | None) {
        return;
    }
    threats.push(WindowsDefenderThreat {
        name: "Windows Defender alert".into(),
        path: Some(path.to_path_buf()),
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
        "& { $detections = @(Get-MpThreatDetection); $catalog = @{}; foreach ($d in $detections) { if ($null -ne $d.ThreatID -and -not $catalog.ContainsKey([string]$d.ThreatID)) { $catalog[[string]$d.ThreatID] = @(Get-MpThreat -ThreatID $d.ThreatID -ErrorAction SilentlyContinue | Select-Object -First 1)[0] } }; $items = @(foreach ($d in $detections) { $id = [string]$d.ThreatID; $entry = $catalog[$id]; $name = if (-not [string]::IsNullOrWhiteSpace([string]$d.ThreatName)) { [string]$d.ThreatName } elseif ($null -ne $entry -and -not [string]::IsNullOrWhiteSpace([string]$entry.ThreatName)) { [string]$entry.ThreatName } else { $null }; [PSCustomObject]@{ ThreatName = $name; ThreatID = $d.ThreatID; Resources = $d.Resources } }); if ($items.Count -eq 0) { '[]' } else { $items | ConvertTo-Json -Depth 5 -Compress } }",
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
        let threat_id = json_string(&item, "ThreatID");
        let name = json_string(&item, "ThreatName").unwrap_or_else(|| {
            threat_id
                .as_deref()
                .map(|id| format!("Microsoft Defender threat ({id})"))
                .unwrap_or_else(|| "Microsoft Defender detection".into())
        });
        let resources = json_strings(&item, "Resources");
        if resources.is_empty() {
            threats.push(WindowsDefenderThreat { name, path: None });
            continue;
        }
        for resource in resources {
            threats.push(WindowsDefenderThreat {
                name: name.clone(),
                path: defender_resource_path(&resource),
            });
        }
    }
    dedupe_windows_defender_threats(&mut threats);
    Ok(threats)
}

#[cfg(target_os = "windows")]
pub(super) fn open_windows_security() -> Result<()> {
    open_windows_uri("windowsdefender://threat")
        .or_else(|_| open_windows_uri("ms-settings:windowsdefender"))
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
pub(super) fn open_windows_security() -> Result<()> {
    Err(BExplorerError::Shell(
        "Windows Security is only available on Windows".into(),
    ))
}
