use std::path::Path;
use std::process::Command;

use crate::utils::errors::{BExplorerError, Result};

#[cfg(target_os = "windows")]
pub(super) fn available_format_filesystems(path: &Path) -> Vec<String> {
    // These are the file systems exposed by the Windows Format-Volume
    // provider for ordinary fixed and removable volumes. ReFS is intentionally
    // omitted because it is not available for every drive type and Windows
    // does not offer it in the normal removable-volume dialog.
    let mut filesystems = vec!["NTFS".to_owned(), "exFAT".to_owned()];
    const FAT32_LIMIT: u64 = 32 * 1024 * 1024 * 1024;
    if fs2::total_space(path).is_ok_and(|capacity| capacity <= FAT32_LIMIT) {
        filesystems.push("FAT32".to_owned());
    }
    filesystems
}

#[cfg(target_os = "windows")]
pub(super) fn format_drive(
    path: &Path,
    filesystem: &str,
    label: &str,
    quick: bool,
    allocation_unit_size: Option<u64>,
) -> Result<()> {
    let drive = path
        .to_str()
        .filter(|value| value.len() >= 2 && value.as_bytes().get(1) == Some(&b':'))
        .ok_or_else(|| BExplorerError::Operation("A drive root is required".into()))?;
    let filesystem = filesystem.trim();
    if !matches!(
        filesystem.to_ascii_lowercase().as_str(),
        "ntfs" | "exfat" | "fat32"
    ) {
        return Err(BExplorerError::Operation(format!(
            "Unsupported Windows file system: {filesystem}"
        )));
    }

    let error_file = std::env::temp_dir().join(format!(
        "bexplorer-format-error-{}-{}.txt",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let _ = std::fs::remove_file(&error_file);
    let drive_literal = powershell_single_quoted(drive);
    let filesystem_literal = powershell_single_quoted(filesystem);
    let label_literal = powershell_single_quoted(label);
    let error_file_literal = powershell_single_quoted(&error_file.to_string_lossy());
    let quick_literal = powershell_single_quoted(if quick { "1" } else { "0" });
    let mut script = format!(
        "$drive = {drive_literal}; $letter = $drive.TrimEnd('\\\\').Substring(0, 1); $params = @{{ DriveLetter = $letter; FileSystem = {filesystem_literal}; Force = $true }}; if (-not [string]::IsNullOrWhiteSpace({label_literal})) {{ $params.NewFileSystemLabel = {label_literal} }}; if ({quick_literal} -ne '1') {{ $params.Full = $true }}; "
    );
    if let Some(size) = allocation_unit_size.filter(|size| *size > 0) {
        script.push_str(&format!(
            "$params.AllocationUnitSize = [uint32]{}; ",
            size.min(u32::MAX as u64)
        ));
    }
    script.push_str(&format!(
        "try {{ Format-Volume @params -Confirm:$false -ErrorAction Stop | Out-Null }} catch {{ $_ | Out-String | Set-Content -LiteralPath {error_file_literal} -Encoding UTF8; exit 1 }}"
    ));
    let encoded = powershell_encoded_command(&script);
    let launcher = format!(
        "$encoded = '{}'; $process = Start-Process -FilePath 'powershell.exe' -WindowStyle Hidden -Verb RunAs -Wait -PassThru -ArgumentList @('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-EncodedCommand',$encoded); exit $process.ExitCode",
        encoded
    );
    use std::os::windows::process::CommandExt;
    let status = Command::new("powershell.exe")
        // CREATE_NO_WINDOW keeps the non-elevated launcher out of the user's
        // taskbar while the elevated formatter runs in the background.
        .creation_flags(0x0800_0000)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            launcher.as_str(),
        ])
        .status()
        .map_err(|error| BExplorerError::Shell(error.to_string()))?;
    let detail = std::fs::read_to_string(&error_file)
        .ok()
        .map(|detail| detail.trim().to_owned())
        .filter(|detail| !detail.is_empty());
    let _ = std::fs::remove_file(&error_file);
    if status.success() {
        Ok(())
    } else {
        let message = detail.unwrap_or_else(|| {
            format!(
                "PowerShell exit code {}. The drive may be in use or the administrator confirmation may have been canceled.",
                status.code().unwrap_or(-1)
            )
        });
        Err(BExplorerError::Operation(format!(
            "Could not format {}: {message}",
            path.display()
        )))
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
pub(super) fn available_format_filesystems(_path: &Path) -> Vec<String> {
    [
        ("ext4", "mkfs.ext4"),
        ("btrfs", "mkfs.btrfs"),
        ("xfs", "mkfs.xfs"),
        ("exfat", "mkfs.exfat"),
        ("vfat", "mkfs.vfat"),
        ("ntfs", "mkfs.ntfs"),
    ]
    .into_iter()
    .filter(|(_, command)| command_exists(command))
    .map(|(filesystem, _)| filesystem.to_owned())
    .collect()
}

#[cfg(all(unix, not(target_os = "macos")))]
pub(super) fn format_drive(
    path: &Path,
    filesystem: &str,
    label: &str,
    _quick: bool,
    _allocation_unit_size: Option<u64>,
) -> Result<()> {
    if !command_exists("udisksctl") {
        return Err(BExplorerError::Operation(
            "udisksctl is required to format drives safely".into(),
        ));
    }
    let source = linux_mount_source_for_path(path).ok_or_else(|| {
        BExplorerError::Operation(format!(
            "Could not find the block device for {}",
            path.display()
        ))
    })?;
    let mut unmount = Command::new("udisksctl");
    unmount.args(["unmount", "--block-device"]).arg(&source);
    run_udisks_command(&mut unmount, "Could not unmount drive")?;

    let filesystem = filesystem.trim().to_ascii_lowercase();
    let supported = ["ext4", "btrfs", "xfs", "exfat", "vfat", "ntfs"];
    if !supported.contains(&filesystem.as_str()) {
        return Err(BExplorerError::Operation(format!(
            "Unsupported Linux file system: {filesystem}"
        )));
    }
    let mut format = Command::new("udisksctl");
    format
        .args(["format", "--block-device"])
        .arg(&source)
        .args(["--type", filesystem.as_str()]);
    if !label.trim().is_empty() {
        format.args(["--label", label]);
    }
    run_udisks_command(&mut format, "Could not format drive")
}

#[cfg(target_os = "macos")]
pub(super) fn available_format_filesystems(_path: &Path) -> Vec<String> {
    ["APFS", "ExFAT", "MS-DOS (FAT)"]
        .into_iter()
        .map(str::to_owned)
        .collect()
}

#[cfg(target_os = "macos")]
pub(super) fn format_drive(
    path: &Path,
    filesystem: &str,
    label: &str,
    _quick: bool,
    _allocation_unit_size: Option<u64>,
) -> Result<()> {
    let filesystem = match filesystem.to_ascii_lowercase().as_str() {
        "apfs" => "APFS",
        "exfat" => "ExFAT",
        "ms-dos (fat)" => "MS-DOS",
        _ => {
            return Err(BExplorerError::Operation(format!(
                "Unsupported macOS file system: {filesystem}"
            )));
        }
    };
    let status = Command::new("diskutil")
        .args(["eraseVolume", filesystem, label])
        .arg(path)
        .status()
        .map_err(|error| BExplorerError::Shell(error.to_string()))?;
    if status.success() {
        Ok(())
    } else {
        Err(BExplorerError::Operation(format!(
            "Could not format {} (diskutil exit code {}).",
            path.display(),
            status.code().unwrap_or(-1)
        )))
    }
}

#[cfg(not(any(target_os = "windows", unix)))]
pub(super) fn available_format_filesystems(_path: &Path) -> Vec<String> {
    Vec::new()
}

#[cfg(not(any(target_os = "windows", unix)))]
pub(super) fn format_drive(
    _path: &Path,
    _filesystem: &str,
    _label: &str,
    _quick: bool,
    _allocation_unit_size: Option<u64>,
) -> Result<()> {
    Err(BExplorerError::Operation(
        "Drive formatting is not supported on this platform".into(),
    ))
}

#[cfg(target_os = "windows")]
fn powershell_encoded_command(script: &str) -> String {
    use std::os::windows::ffi::OsStrExt;

    let bytes = std::ffi::OsStr::new(script)
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes)
}

#[cfg(target_os = "windows")]
fn powershell_single_quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn command_exists(command: &str) -> bool {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .any(|directory| directory.join(command).is_file())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_mount_source_for_path(path: &Path) -> Option<std::path::PathBuf> {
    let text = std::fs::read_to_string("/proc/self/mountinfo").ok()?;
    text.lines()
        .filter_map(parse_mountinfo_source)
        .filter_map(|(mount_point, source)| {
            path.starts_with(&mount_point)
                .then_some((mount_point, source))
        })
        .max_by_key(|(mount_point, _)| mount_point.as_os_str().len())
        .map(|(_, source)| source)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn parse_mountinfo_source(line: &str) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let (before, after) = line.split_once(" - ")?;
    let before = before.split_whitespace().collect::<Vec<_>>();
    let after = after.split_whitespace().collect::<Vec<_>>();
    if before.len() < 5 || after.len() < 2 {
        return None;
    }
    Some((
        std::path::PathBuf::from(decode_mount_field(before[4])),
        std::path::PathBuf::from(decode_mount_field(after[1])),
    ))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn decode_mount_field(value: &str) -> String {
    value
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn run_udisks_command(command: &mut Command, context: &str) -> Result<()> {
    let output = command
        .output()
        .map_err(|error| BExplorerError::Shell(error.to_string()))?;
    if output.status.success() {
        Ok(())
    } else {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(BExplorerError::Operation(if detail.is_empty() {
            context.to_owned()
        } else {
            format!("{context}: {detail}")
        }))
    }
}
