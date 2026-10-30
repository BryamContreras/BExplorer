#[cfg(all(unix, not(target_os = "macos")))]
use std::collections::HashMap;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(target_os = "windows")]
use std::process::Stdio;
#[cfg(all(unix, not(target_os = "macos")))]
use zbus::blocking::{Connection, Proxy};
#[cfg(all(unix, not(target_os = "macos")))]
use zbus::zvariant::{OwnedObjectPath, Value};

use crate::utils::errors::{BExplorerError, Result};

#[cfg(target_os = "windows")]
pub(super) fn mount_disk_image(path: &Path) -> Result<()> {
    let status = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "& { $ImagePath = $env:BEXPLORER_IMAGE_PATH; if ([string]::IsNullOrWhiteSpace($ImagePath)) { exit 2 }; $image = Get-DiskImage -ImagePath $ImagePath -ErrorAction SilentlyContinue | Select-Object -First 1; if ($image -and $image.Attached) { $volume = $image | Get-Volume -ErrorAction SilentlyContinue | Where-Object { $_.DriveLetter } | Select-Object -First 1; if ($volume) { exit 0 }; Dismount-DiskImage -ImagePath $ImagePath -ErrorAction SilentlyContinue | Out-Null; Start-Sleep -Milliseconds 250 }; Mount-DiskImage -ImagePath $ImagePath -ErrorAction Stop | Out-Null }",
        ])
        .env("BEXPLORER_IMAGE_PATH", path)
        .status()
        .map_err(|error| BExplorerError::Shell(error.to_string()))?;

    if status.success() {
        Ok(())
    } else {
        Err(BExplorerError::Shell(format!(
            "Could not mount disk image {}",
            path.display()
        )))
    }
}

#[cfg(target_os = "windows")]
pub(super) fn mounted_disk_image_root(path: &Path) -> Result<PathBuf> {
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "& { $ImagePath = $env:BEXPLORER_IMAGE_PATH; if ([string]::IsNullOrWhiteSpace($ImagePath)) { exit 2 }; for ($i = 0; $i -lt 20; $i++) { $volume = Get-DiskImage -ImagePath $ImagePath -ErrorAction SilentlyContinue | Get-Volume -ErrorAction SilentlyContinue | Where-Object { $_.DriveLetter } | Select-Object -First 1; if ($volume) { Write-Output ($volume.DriveLetter + ':\\'); exit 0 }; Start-Sleep -Milliseconds 150 }; exit 2 }",
        ])
        .env("BEXPLORER_IMAGE_PATH", path)
        .output()
        .map_err(|error| BExplorerError::Shell(error.to_string()))?;

    if !output.status.success() {
        return Err(BExplorerError::Shell(format!(
            "Could not locate mounted disk image volume for {}",
            path.display()
        )));
    }

    let root = String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .ok_or_else(|| {
            BExplorerError::Shell(format!(
                "Mounted disk image did not report a drive letter for {}",
                path.display()
            ))
        })?;

    Ok(PathBuf::from(root))
}

#[cfg(target_os = "windows")]
pub(super) fn suppress_file_explorer_windows_at(path: &Path) -> Result<()> {
    Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-WindowStyle",
            "Hidden",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            r#"
& {
    $DriveRoot = $env:BEXPLORER_DRIVE_ROOT
    if ([string]::IsNullOrWhiteSpace($DriveRoot)) { return }

    function Normalize-ShellPath([object]$Value) {
        if ($null -eq $Value) { return $null }
        $text = ([string]$Value).Trim()
        if ([string]::IsNullOrWhiteSpace($text)) { return $null }

        try {
            if ($text.StartsWith('file:', [System.StringComparison]::OrdinalIgnoreCase)) {
                $text = ([System.Uri]$text).LocalPath
            }
        } catch {}

        $text = $text.Replace('/', '\').TrimEnd([char]92)
        try {
            $text = [System.IO.Path]::GetFullPath($text).TrimEnd([char]92)
        } catch {}

        if ([string]::IsNullOrWhiteSpace($text)) { return $null }
        return $text.ToUpperInvariant()
    }

    $target = Normalize-ShellPath $DriveRoot
    if (-not $target) { return }
    $targetPrefix = $target + '\'
    $driveMarker = '(' + $target + ')'

    function Test-ExplorerAtTarget([object]$Window) {
        try {
            $fullName = ([string]$Window.FullName).ToLowerInvariant()
            if (-not $fullName.EndsWith('explorer.exe')) { return $false }
        } catch {}

        $candidates = @()
        try { $candidates += $Window.Document.Folder.Self.Path } catch {}
        try { $candidates += $Window.LocationURL } catch {}

        foreach ($candidate in $candidates) {
            $normalized = Normalize-ShellPath $candidate
            if (-not $normalized) { continue }
            if ($normalized -eq $target -or $normalized.StartsWith($targetPrefix)) {
                return $true
            }
        }

        try {
            $locationName = ([string]$Window.LocationName).ToUpperInvariant()
            if ($locationName.EndsWith($driveMarker)) { return $true }
        } catch {}

        return $false
    }

    function Close-ExplorerAtRoot([string]$Root) {
        try {
            $shell = New-Object -ComObject Shell.Application
            foreach ($window in @($shell.Windows())) {
                try {
                    if (Test-ExplorerAtTarget $window) {
                        $window.Quit()
                    }
                } catch {}
            }
        } catch {}
    }

    for ($i = 0; $i -lt 40; $i++) {
        Close-ExplorerAtRoot $DriveRoot
        Start-Sleep -Milliseconds 125
    }
}
"#,
        ])
        .env("BEXPLORER_DRIVE_ROOT", path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| BExplorerError::Shell(error.to_string()))?;
    Ok(())
}

#[cfg(target_os = "windows")]
pub(super) fn eject_drive(path: &Path) -> Result<()> {
    let drive_root = windows_eject_drive_root(path)
        .ok_or_else(|| BExplorerError::Shell("Safe removal requires a drive-letter root".into()))?;
    let mut command = Command::new("powershell.exe");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            concat!(
                "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)\n",
                include_str!("windows_eject.ps1"),
                "\ntry { Invoke-BExplorerDriveEject $env:BEXPLORER_DRIVE_ROOT; exit 0 } ",
                "catch { Write-BExplorerEjectError $_.Exception; exit 1 }"
            ),
        ])
        .env("BEXPLORER_DRIVE_ROOT", drive_root)
        .creation_flags(0x0800_0000);
    let output = crate::utils::process::command_output_with_timeout(
        &mut command,
        std::time::Duration::from_secs(45),
    )
    .ok_or_else(|| {
        BExplorerError::Shell(format!(
            "Windows did not finish the safe-removal request for {}; check whether the drive is still connected before retrying",
            path.display()
        ))
    })?;

    if output.status.success() {
        Ok(())
    } else {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let detail = if detail.is_empty() {
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        } else {
            detail
        };
        let message = if detail.is_empty() {
            format!("Could not eject drive {}", path.display())
        } else {
            format!("Could not eject drive {}: {detail}", path.display())
        };
        Err(BExplorerError::Shell(message))
    }
}

#[cfg(target_os = "windows")]
fn windows_eject_drive_root(path: &Path) -> Option<String> {
    let text = path.to_str()?.replace('/', "\\");
    let bytes = text.as_bytes();
    if matches!(bytes.len(), 2 | 3)
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes.len() == 2 || bytes[2] == b'\\')
    {
        Some(format!("{}:\\", (bytes[0] as char).to_ascii_uppercase()))
    } else {
        None
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
pub(super) fn mount_disk_image(path: &Path) -> Result<()> {
    ensure_command("udisksctl")?;
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if mounted_disk_image_root(&canonical).is_ok() {
        return Ok(());
    }
    let existing_loops = linux_loop_devices_for_file(&canonical);
    for loop_device in &existing_loops {
        if mount_linux_block_or_partition(loop_device).is_ok()
            && mounted_disk_image_root(&canonical).is_ok()
        {
            return Ok(());
        }
    }
    if !existing_loops.is_empty() {
        return Err(BExplorerError::Shell(format!(
            "Could not mount the primary volume from {}",
            path.display()
        )));
    }

    let output = Command::new("udisksctl")
        .args(["loop-setup", "--read-only", "--file"])
        .arg(&canonical)
        .output()
        .map_err(|error| BExplorerError::Shell(error.to_string()))?;
    if !output.status.success() {
        return Err(command_error("Could not set up loop device", &output));
    }

    let block = parse_udisks_block_device(&udisks_output_text(&output))
        .ok_or_else(|| BExplorerError::Shell("udisksctl did not report a loop device".into()))?;
    let result = mount_linux_block_or_partition(&block)
        .and_then(|_| wait_for_mounted_disk_image_root(&canonical).map(|_| ()));
    if result.is_err() {
        let _ = run_udisks_status(
            Command::new("udisksctl")
                .args(["loop-delete", "--block-device"])
                .arg(&block),
            "Could not clean up loop device",
        );
    }
    result
}

#[cfg(target_os = "macos")]
pub(super) fn mount_disk_image(path: &Path) -> Result<()> {
    if mounted_disk_image_root(path).is_ok() {
        return Ok(());
    }
    let output = Command::new("hdiutil")
        .args(["attach", "-readonly", "-nobrowse"])
        .arg(path)
        .output()
        .map_err(|error| BExplorerError::Shell(error.to_string()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(command_error("Could not mount disk image", &output))
    }
}

#[cfg(not(any(target_os = "windows", unix)))]
pub(super) fn mount_disk_image(_path: &Path) -> Result<()> {
    Err(BExplorerError::Shell(
        "Mounting disk images is not supported on this platform".into(),
    ))
}

#[cfg(all(unix, not(target_os = "macos")))]
pub(super) fn mounted_disk_image_root(path: &Path) -> Result<PathBuf> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if let Some(root) = mounted_disk_image_root_once(&canonical) {
        return Ok(root);
    }

    Err(BExplorerError::Shell(format!(
        "Could not locate mounted disk image volume for {}",
        path.display()
    )))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn wait_for_mounted_disk_image_root(path: &Path) -> Result<PathBuf> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        if let Some(root) = mounted_disk_image_root_once(path) {
            return Ok(root);
        }
        if std::time::Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(80));
    }

    Err(BExplorerError::Shell(format!(
        "Could not locate mounted disk image volume for {}",
        path.display()
    )))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn mounted_disk_image_root_once(canonical: &Path) -> Option<PathBuf> {
    for loop_device in linux_loop_devices_for_file(canonical) {
        if let Some(mount) = linux_mount_for_block_device(&loop_device) {
            return Some(mount);
        }
        for partition in linux_block_partitions(&loop_device) {
            if let Some(mount) = linux_mount_for_block_device(&partition) {
                return Some(mount);
            }
        }
    }
    None
}

#[cfg(target_os = "macos")]
pub(super) fn mounted_disk_image_root(path: &Path) -> Result<PathBuf> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let output = Command::new("hdiutil")
        .arg("info")
        .output()
        .map_err(|error| BExplorerError::Shell(error.to_string()))?;
    if !output.status.success() {
        return Err(command_error(
            "Could not inspect mounted disk images",
            &output,
        ));
    }
    parse_hdiutil_mount_root(&String::from_utf8_lossy(&output.stdout), &canonical).ok_or_else(
        || {
            BExplorerError::Shell(format!(
                "Could not locate mounted disk image volume for {}",
                path.display()
            ))
        },
    )
}

#[cfg(target_os = "macos")]
fn parse_hdiutil_mount_root(text: &str, image_path: &Path) -> Option<PathBuf> {
    let expected = image_path.to_string_lossy();
    let mut matching_image = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("image-path") {
            let value = value.trim_start_matches([' ', ':']).trim();
            let candidate = PathBuf::from(value);
            matching_image = candidate == image_path
                || candidate
                    .canonicalize()
                    .is_ok_and(|path| path == image_path)
                || value == expected;
            continue;
        }
        if matching_image && trimmed.starts_with("/dev/") {
            let mount = trimmed
                .split('\t')
                .next_back()
                .map(str::trim)
                .filter(|value| value.starts_with('/'))?;
            return Some(PathBuf::from(mount));
        }
    }
    None
}

#[cfg(not(any(target_os = "windows", unix)))]
pub(super) fn mounted_disk_image_root(_path: &Path) -> Result<PathBuf> {
    Err(BExplorerError::Shell(
        "Resolving mounted disk images is not supported on this platform".into(),
    ))
}

#[cfg(not(target_os = "windows"))]
pub(super) fn suppress_file_explorer_windows_at(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
pub(super) fn eject_drive(path: &Path) -> Result<()> {
    ensure_command("udisksctl")?;
    let block = linux_mount_source_for_path(path).ok_or_else(|| {
        BExplorerError::Shell(format!(
            "Could not find mounted block device for {}",
            path.display()
        ))
    })?;
    let loop_block = linux_loop_base_for_block(&block);
    let physical_drive = if loop_block.is_none() {
        Some(linux_udisks_drive_for_block(&block)?)
    } else {
        None
    };

    let unmount = Command::new("udisksctl")
        .args(["unmount", "--block-device"])
        .arg(&block)
        .output()
        .map_err(|error| BExplorerError::Shell(error.to_string()))?;
    if !unmount.status.success() && !udisks_error_allows_eject_continue(&unmount) {
        return Err(command_error("Could not unmount drive", &unmount));
    }

    if let Some(loop_block) = loop_block {
        run_udisks_status(
            Command::new("udisksctl")
                .args(["loop-delete", "--block-device"])
                .arg(loop_block),
            "Could not delete loop device",
        )
    } else {
        let Some(drive) = physical_drive else {
            return Err(BExplorerError::Shell(
                "Could not resolve the physical drive before ejecting it".into(),
            ));
        };
        match linux_drive_removal_action(&drive) {
            LinuxDriveRemovalAction::Eject => linux_eject_udisks_drive(&drive.object_path),
            LinuxDriveRemovalAction::PowerOff => {
                let power_block = linux_parent_block_device(&block).unwrap_or(block);
                run_udisks_status(
                    Command::new("udisksctl")
                        .args(["power-off", "--block-device"])
                        .arg(power_block),
                    "Could not power off drive",
                )
            }
            // Some removable-media readers expose neither Eject nor PowerOff.
            // A successful unmount is the complete safe-removal operation for
            // those devices and must not be reported as a failure.
            LinuxDriveRemovalAction::UnmountOnly => Ok(()),
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
#[derive(Clone, Debug)]
struct LinuxUdisksDrive {
    object_path: OwnedObjectPath,
    optical: bool,
    ejectable: bool,
    can_power_off: bool,
}

#[cfg(all(unix, not(target_os = "macos")))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LinuxDriveRemovalAction {
    Eject,
    PowerOff,
    UnmountOnly,
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_drive_removal_action(drive: &LinuxUdisksDrive) -> LinuxDriveRemovalAction {
    if drive.optical || (drive.ejectable && !drive.can_power_off) {
        LinuxDriveRemovalAction::Eject
    } else if drive.can_power_off {
        LinuxDriveRemovalAction::PowerOff
    } else {
        LinuxDriveRemovalAction::UnmountOnly
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_udisks_drive_for_block(block: &Path) -> Result<LinuxUdisksDrive> {
    const SERVICE: &str = "org.freedesktop.UDisks2";
    let connection = Connection::system()
        .map_err(|error| BExplorerError::Shell(format!("Could not connect to UDisks2: {error}")))?;
    let manager = Proxy::new(
        &connection,
        SERVICE,
        "/org/freedesktop/UDisks2/Manager",
        "org.freedesktop.UDisks2.Manager",
    )
    .map_err(|error| BExplorerError::Shell(format!("Could not access UDisks2: {error}")))?;
    let block_path = block.to_string_lossy().into_owned();
    let mut specification = HashMap::<&str, Value<'_>>::new();
    specification.insert("path", Value::from(block_path.as_str()));
    let options = HashMap::<&str, Value<'_>>::new();
    let devices: Vec<OwnedObjectPath> = manager
        .call("ResolveDevice", &(specification, options))
        .map_err(|error| {
            BExplorerError::Shell(format!(
                "Could not resolve optical or removable drive: {error}"
            ))
        })?;
    let [block_object] = devices.as_slice() else {
        return Err(BExplorerError::Shell(format!(
            "UDisks2 resolved {} to {} block devices",
            block.display(),
            devices.len()
        )));
    };
    let block_proxy = Proxy::new(
        &connection,
        SERVICE,
        block_object.as_str(),
        "org.freedesktop.UDisks2.Block",
    )
    .map_err(|error| BExplorerError::Shell(format!("Could not inspect UDisks2 block: {error}")))?;
    let drive_path: OwnedObjectPath = block_proxy.get_property("Drive").map_err(|error| {
        BExplorerError::Shell(format!("Could not locate UDisks2 drive: {error}"))
    })?;
    if drive_path.as_str() == "/" {
        return Err(BExplorerError::Shell(
            "The selected block device has no ejectable physical drive".into(),
        ));
    }
    let drive_proxy = Proxy::new(
        &connection,
        SERVICE,
        drive_path.as_str(),
        "org.freedesktop.UDisks2.Drive",
    )
    .map_err(|error| BExplorerError::Shell(format!("Could not inspect UDisks2 drive: {error}")))?;
    Ok(LinuxUdisksDrive {
        object_path: drive_path.clone(),
        optical: drive_proxy.get_property("Optical").map_err(|error| {
            BExplorerError::Shell(format!("Could not inspect optical drive state: {error}"))
        })?,
        ejectable: drive_proxy.get_property("Ejectable").map_err(|error| {
            BExplorerError::Shell(format!("Could not inspect drive eject support: {error}"))
        })?,
        can_power_off: drive_proxy.get_property("CanPowerOff").map_err(|error| {
            BExplorerError::Shell(format!(
                "Could not inspect drive power-off support: {error}"
            ))
        })?,
    })
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_eject_udisks_drive(drive_path: &OwnedObjectPath) -> Result<()> {
    let connection = Connection::system()
        .map_err(|error| BExplorerError::Shell(format!("Could not connect to UDisks2: {error}")))?;
    let drive = Proxy::new(
        &connection,
        "org.freedesktop.UDisks2",
        drive_path.as_str(),
        "org.freedesktop.UDisks2.Drive",
    )
    .map_err(|error| BExplorerError::Shell(format!("Could not access UDisks2 drive: {error}")))?;
    let options = HashMap::<&str, Value<'_>>::new();
    drive
        .call::<_, _, ()>("Eject", &(options,))
        .map_err(|error| BExplorerError::Shell(format!("Could not eject optical media: {error}")))
}

#[cfg(target_os = "macos")]
pub(super) fn eject_drive(path: &Path) -> Result<()> {
    let output = Command::new("diskutil")
        .arg("eject")
        .arg(path)
        .output()
        .map_err(|error| BExplorerError::Shell(error.to_string()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(command_error("Could not eject drive", &output))
    }
}

#[cfg(not(any(target_os = "windows", unix)))]
pub(super) fn eject_drive(_path: &Path) -> Result<()> {
    Err(BExplorerError::Shell(
        "Ejecting drives is not supported on this platform".into(),
    ))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn mount_linux_block_or_partition(block: &Path) -> Result<()> {
    if run_udisks_status(
        Command::new("udisksctl")
            .args(["mount", "--block-device"])
            .arg(block),
        "Could not mount disk image",
    )
    .is_ok()
    {
        return Ok(());
    }

    for partition in linux_block_partitions(block) {
        if run_udisks_status(
            Command::new("udisksctl")
                .args(["mount", "--block-device"])
                .arg(partition),
            "Could not mount disk image partition",
        )
        .is_ok()
        {
            return Ok(());
        }
    }

    Err(BExplorerError::Shell(format!(
        "Could not mount disk image device {}",
        block.display()
    )))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn run_udisks_status(command: &mut Command, context: &str) -> Result<()> {
    let output = command
        .output()
        .map_err(|error| BExplorerError::Shell(error.to_string()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(command_error(context, &output))
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn udisks_output_text(output: &std::process::Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    if !output.stderr.is_empty() {
        if !text.ends_with('\n') && !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    text
}

#[cfg(unix)]
fn command_error(context: &str, output: &std::process::Output) -> BExplorerError {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() { stderr } else { stdout };
    if detail.is_empty() {
        BExplorerError::Shell(context.into())
    } else {
        BExplorerError::Shell(format!("{context}: {detail}"))
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn udisks_error_allows_eject_continue(output: &std::process::Output) -> bool {
    let detail = udisks_output_text(output).to_ascii_lowercase();
    detail.contains("not mounted")
        || detail.contains("not a mounted filesystem")
        || detail.contains("is not mounted")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn ensure_command(program: &str) -> Result<()> {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .any(|directory| directory.join(program).is_file())
        .then_some(())
        .ok_or_else(|| BExplorerError::Shell(format!("{program} is required for this operation")))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn parse_udisks_block_device(text: &str) -> Option<PathBuf> {
    text.split_whitespace()
        .map(|token| {
            token.trim_matches(|ch| matches!(ch, '.' | ',' | ';' | ':' | '\'' | '"' | '`'))
        })
        .find(|token| token.starts_with("/dev/"))
        .map(PathBuf::from)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_loop_devices_for_file(image_path: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir("/sys/block") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("loop") {
                return None;
            }
            let backing = std::fs::read_to_string(entry.path().join("loop/backing_file")).ok()?;
            let backing = PathBuf::from(backing.trim());
            let backing = backing.canonicalize().unwrap_or(backing);
            (backing == image_path).then(|| PathBuf::from("/dev").join(name))
        })
        .collect()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_block_partitions(block: &Path) -> Vec<PathBuf> {
    let Some(name) = block.file_name().and_then(|name| name.to_str()) else {
        return Vec::new();
    };
    let sys_block = Path::new("/sys/class/block").join(name);
    let Ok(entries) = std::fs::read_dir(sys_block) else {
        return Vec::new();
    };
    let partitions = entries
        .flatten()
        .filter_map(|entry| {
            let child = entry.file_name().to_string_lossy().to_string();
            child
                .starts_with(name)
                .then(|| PathBuf::from("/dev").join(child))
        })
        .collect::<Vec<_>>();
    preferred_linux_partition_paths(
        partitions
            .into_iter()
            .map(|path| {
                let size = linux_block_size_sectors(&path).unwrap_or(0);
                (path, size)
            })
            .collect(),
    )
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_block_size_sectors(block: &Path) -> Option<u64> {
    let name = block.file_name()?.to_str()?;
    std::fs::read_to_string(Path::new("/sys/class/block").join(name).join("size"))
        .ok()?
        .trim()
        .parse()
        .ok()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn preferred_linux_partition_paths(mut partitions: Vec<(PathBuf, u64)>) -> Vec<PathBuf> {
    partitions.sort_by(|(left_path, left_size), (right_path, right_size)| {
        right_size
            .cmp(left_size)
            .then_with(|| left_path.cmp(right_path))
    });
    let largest = partitions.first().map(|(_, size)| *size).unwrap_or(0);
    partitions
        .into_iter()
        .filter(|(_, size)| largest == 0 || *size == 0 || size.saturating_mul(100) >= largest)
        .map(|(path, _)| path)
        .collect()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_mount_for_block_device(block: &Path) -> Option<PathBuf> {
    let block = block.display().to_string();
    linux_mountinfo_entries()
        .into_iter()
        .find(|entry| entry.source == block)
        .map(|entry| entry.mount_point)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_mount_source_for_path(path: &Path) -> Option<PathBuf> {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    linux_mountinfo_entries()
        .into_iter()
        .filter(|entry| path == entry.mount_point || path.starts_with(&entry.mount_point))
        .max_by_key(|entry| entry.mount_point.as_os_str().len())
        .and_then(|entry| {
            entry
                .source
                .starts_with("/dev/")
                .then(|| PathBuf::from(entry.source))
        })
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_loop_base_for_block(block: &Path) -> Option<PathBuf> {
    let name = block.file_name()?.to_str()?;
    if name.starts_with("loop") && !name.contains('p') {
        return Some(block.to_path_buf());
    }
    let parent = linux_parent_block_device(block)?;
    parent
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("loop"))
        .then_some(parent)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_parent_block_device(block: &Path) -> Option<PathBuf> {
    let name = block.file_name()?.to_str()?;
    let canonical = std::fs::canonicalize(Path::new("/sys/class/block").join(name)).ok()?;
    let parent_name = canonical.parent()?.file_name()?.to_str()?;
    (parent_name != "block").then(|| PathBuf::from("/dev").join(parent_name))
}

#[cfg(all(unix, not(target_os = "macos")))]
#[derive(Clone, Debug)]
struct LinuxMountInfoEntry {
    mount_point: PathBuf,
    source: String,
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_mountinfo_entries() -> Vec<LinuxMountInfoEntry> {
    let Ok(text) = std::fs::read_to_string("/proc/self/mountinfo") else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let (before, after) = line.split_once(" - ")?;
            let before_fields = before.split_whitespace().collect::<Vec<_>>();
            let after_fields = after.split_whitespace().collect::<Vec<_>>();
            Some(LinuxMountInfoEntry {
                mount_point: PathBuf::from(decode_mountinfo_field(before_fields.get(4)?)),
                source: decode_mountinfo_field(after_fields.get(1)?),
            })
        })
        .collect()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn decode_mountinfo_field(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 3 < bytes.len() {
            let digits = &bytes[index + 1..index + 4];
            if digits.iter().all(|byte| matches!(byte, b'0'..=b'7')) {
                output.push(
                    ((digits[0] - b'0') * 64 + (digits[1] - b'0') * 8 + digits[2] - b'0') as char,
                );
                index += 4;
                continue;
            }
        }
        output.push(bytes[index] as char);
        index += 1;
    }
    output
}

#[cfg(all(test, target_os = "windows"))]
mod windows_eject_tests {
    use super::*;

    fn check_discovery_script(checks: &str) -> std::process::Output {
        // The embedded helper defines functions only. These tests call its
        // discovery functions with fake providers, never the removal action.
        let script = format!(
            "$ErrorActionPreference = 'Stop'\n{}\n{checks}",
            include_str!("windows_eject.ps1")
        );
        let mut command = Command::new("powershell.exe");
        command
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .creation_flags(0x0800_0000);
        let output = crate::utils::process::command_output_with_timeout(
            &mut command,
            std::time::Duration::from_secs(20),
        )
        .expect("PowerShell discovery check should finish within its deadline");
        assert!(
            output.status.success(),
            "Discovery check failed: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    #[test]
    fn windows_eject_accepts_only_drive_letter_roots() {
        for (path, expected) in [("E:", "E:\\"), ("e:/", "E:\\"), ("F:\\", "F:\\")] {
            assert_eq!(
                windows_eject_drive_root(Path::new(path)).as_deref(),
                Some(expected)
            );
        }
        for path in ["", "C:\\folder", "\\\\server\\share", "C:\\..\\", "1:\\"] {
            assert_eq!(windows_eject_drive_root(Path::new(path)), None);
        }
    }

    #[test]
    fn windows_eject_selects_the_removable_parent_and_never_a_usb_hub() {
        check_discovery_script(
            r#"
$script:nodes = @{
    'USBSTOR\DISK' = @{ Capabilities = 16; Parent = 'USB\VID_BRIDGE'; Service = 'disk' }
    'USB\VID_BRIDGE' = @{ Capabilities = 148; Parent = 'USB\VID_HUB'; Service = 'USBSTOR' }
    'USB\VID_HUB' = @{ Capabilities = 148; Parent = 'PCI\CONTROLLER'; Service = 'USBHUB3' }
}
function Get-PnpDeviceProperty {
    param($InstanceId, $KeyName, $ErrorAction)
    $node = $script:nodes[$InstanceId]
    if ($null -eq $node) { throw 'Unexpected device inspection' }
    [pscustomobject]@{ KeyName = 'DEVPKEY_Device_Capabilities'; Data = $node.Capabilities }
    [pscustomobject]@{ KeyName = 'DEVPKEY_Device_Parent'; Data = $node.Parent }
    [pscustomobject]@{ KeyName = 'DEVPKEY_Device_Service'; Data = $node.Service }
}
if ((Get-BExplorerPnpRemovalTarget 'USBSTOR\DISK') -ne 'USB\VID_BRIDGE') { throw 'Wrong removable parent' }
$script:nodes['USB\VID_BRIDGE'].Capabilities = 16
$blocked = $false
try { Get-BExplorerPnpRemovalTarget 'USBSTOR\DISK' | Out-Null } catch { $blocked = $_.Exception.Message -like '*hub or host controller*' }
if (-not $blocked) { throw 'Hub removal was not blocked' }
$script:nodes['USBSTOR\DISK'].Parent = 'USBSTOR\DISK'
$blocked = $false
try { Get-BExplorerPnpRemovalTarget 'USBSTOR\DISK' | Out-Null } catch { $blocked = $true }
if (-not $blocked) { throw 'Parent cycle was not blocked' }
# Compile the native declarations, without invoking any device-removal API.
Initialize-BExplorerDeviceRemoval
"#,
        );
    }

    #[test]
    fn windows_eject_discovery_rejects_system_disks_and_internal_buses() {
        check_discovery_script(
            r#"
function Get-Partition { param($DriveLetter, $ErrorAction) [pscustomobject]@{ DiskNumber = 3 } }
function Get-Disk { param($ErrorAction) process { $script:disk } }
function Get-CimInstance { throw 'Physical lookup must not run for an unsafe disk' }
foreach ($disk in @(
    @{ IsBoot = $true; IsSystem = $false; BusType = 'USB'; Number = 3 },
    @{ IsBoot = $false; IsSystem = $true; BusType = 'USB'; Number = 3 },
    @{ IsBoot = $false; IsSystem = $false; BusType = 'SATA'; Number = 3 },
    @{ IsBoot = $false; IsSystem = $false; BusType = 'NVMe'; Number = 3 }
)) {
    $script:disk = [pscustomobject]$disk
    $blocked = $false
    try { Resolve-BExplorerUsbRemovalTarget 'E:\' | Out-Null } catch {
        $blocked = $_.Exception.Message -like '*boot or system*' -or $_.Exception.Message -like '*not on an external*'
    }
    if (-not $blocked) { throw 'Unsafe disk reached physical-device lookup' }
}
"#,
        );
    }

    #[test]
    fn windows_eject_resolves_the_physical_usb_parent_instead_of_a_volume_child() {
        check_discovery_script(
            r#"
function Get-Partition { param($DriveLetter, $ErrorAction) if ($DriveLetter -ne 'E') { throw 'Wrong letter' }; [pscustomobject]@{ DiskNumber = 2 } }
function Get-Disk { param($ErrorAction) process { [pscustomobject]@{ Number = 2; IsBoot = $false; IsSystem = $false; BusType = 'USB' } } }
function Get-CimInstance {
    param($ClassName, $Filter, $ErrorAction)
    if ($ClassName -ne 'Win32_DiskDrive' -or $Filter -ne 'Index = 2') { throw 'Wrong physical lookup' }
    [pscustomobject]@{ PNPDeviceID = 'USBSTOR\DISK_PHYSICAL' }
}
function Get-PnpDeviceProperty {
    param($InstanceId, $KeyName, $ErrorAction)
    if ($InstanceId -eq 'USBSTOR\DISK_PHYSICAL') { $capabilities = 16; $parent = 'USB\VID_ENCLOSURE'; $service = 'disk' }
    elseif ($InstanceId -eq 'USB\VID_ENCLOSURE') { $capabilities = 148; $parent = 'USB\ROOT_HUB30'; $service = 'USBSTOR' }
    else { throw 'Discovery reached a volume child or host controller' }
    [pscustomobject]@{ KeyName = 'DEVPKEY_Device_Capabilities'; Data = $capabilities }
    [pscustomobject]@{ KeyName = 'DEVPKEY_Device_Parent'; Data = $parent }
    [pscustomobject]@{ KeyName = 'DEVPKEY_Device_Service'; Data = $service }
}
if ((Resolve-BExplorerUsbRemovalTarget 'E:\') -ne 'USB\VID_ENCLOSURE') { throw 'The USB physical parent was not selected' }
"#,
        );
    }

    #[test]
    fn windows_eject_veto_reports_utf8_underlying_error_without_invocation_wrapper() {
        let output = check_discovery_script(
            r#"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
Initialize-BExplorerDeviceRemoval
$message = [BExplorerDeviceRemoval]::DescribeFailure(23, 5, 'STORAGE\Volume\CHILD')
if ($message -ne 'BEXPLORER_EJECT_BUSY|PnP=23|Veto=5|Device=STORAGE\Volume\CHILD') { throw 'Missing veto diagnostics' }
$inner = [System.InvalidOperationException]::new($message + ' ' + [char]0x00f3 + [char]0x00f1)
$outer = [System.Reflection.TargetInvocationException]::new('Invocation wrapper must be removed', $inner)
Write-BExplorerEjectError $outer
"#,
        );
        let message = String::from_utf8(output.stderr).expect("Eject errors must use UTF-8");
        assert!(
            message.contains("BEXPLORER_EJECT_BUSY|PnP=23|Veto=5|Device=STORAGE\\Volume\\CHILD")
        );
        assert!(message.contains("óñ"));
        assert!(!message.contains("Invocation wrapper"));
    }

    #[test]
    fn windows_eject_retries_only_outstanding_opens_with_a_fixed_identity_and_limit() {
        check_discovery_script(
            r#"
Initialize-BExplorerDeviceRemoval
$script:attempts = 0
$script:waits = 0
$script:succeed = $true
$script:veto = 5
function Invoke-BExplorerPnpRemovalAttempt {
    param($InstanceId)
    if ($InstanceId -ne 'USB\ORIGINAL_ID') { throw 'Retry changed the device identity' }
    $script:attempts++
    if ($script:succeed -and $script:attempts -eq 3) { return [pscustomobject]@{ Result = 0; Veto = 0; Name = '' } }
    [pscustomobject]@{ Result = 23; Veto = $script:veto; Name = 'STORAGE\Volume\CHILD' }
}
function Start-Sleep {
    param($Milliseconds)
    if ($Milliseconds -ne 300) { throw 'Unexpected retry delay' }
    $script:waits++
}
Request-BExplorerPnpRemoval 'USB\ORIGINAL_ID'
if ($script:attempts -ne 3 -or $script:waits -ne 2) { throw 'A transient outstanding open was not retried within bounds' }

$script:succeed = $false
$script:attempts = 0
$script:waits = 0
$blocked = $false
try { Request-BExplorerPnpRemoval 'USB\ORIGINAL_ID' } catch { $blocked = $_.Exception.Message -like 'BEXPLORER_EJECT_BUSY|PnP=23|Veto=5|Device=*' }
if (-not $blocked -or $script:attempts -ne 3 -or $script:waits -ne 2) { throw 'A persistent open veto exceeded the retry bound or lost its diagnostics' }

foreach ($veto in @(3, 7, 12)) {
    $script:veto = $veto
    $script:attempts = 0
    $script:waits = 0
    $blocked = $false
    try { Request-BExplorerPnpRemoval 'USB\ORIGINAL_ID' } catch { $blocked = $_.Exception.Message -like 'BEXPLORER_EJECT_*' }
    if (-not $blocked -or $script:attempts -ne 1 -or $script:waits -ne 0) { throw 'A different veto was retried or swallowed' }
}
"#,
        );
    }

    #[test]
    fn windows_eject_releases_shell_objects_before_pnp_and_preserves_a_rejected_request() {
        check_discovery_script(
            r#"
$script:item = [pscustomobject]@{ Kind = 'item' }
$script:item | Add-Member ScriptMethod Verbs { @() }
$script:folder = [pscustomobject]@{ Kind = 'folder' }
$script:folder | Add-Member ScriptMethod ParseName { param($Path) $script:item }
$script:shell = [pscustomobject]@{ Kind = 'shell' }
$script:shell | Add-Member ScriptMethod Namespace { param($Id) $script:folder }
function New-Object { param($ComObject) if ($ComObject -ne 'Shell.Application') { throw 'Unexpected COM class' }; $script:shell }
$script:released = @()
function Release-BExplorerComObject { param($Object) if ($null -ne $Object) { $script:released += $Object.Kind } }
function Resolve-BExplorerUsbRemovalTarget {
    param($DriveRoot)
    if (($script:released -join ',') -ne 'item,folder,shell') { throw 'Shell objects were retained during physical discovery' }
    'USB\VID_PHYSICAL'
}
$script:requests = 0
function Request-BExplorerPnpRemoval {
    param($InstanceId)
    if ($InstanceId -ne 'USB\VID_PHYSICAL') { throw 'Wrong removal target' }
    $script:requests++
    throw 'OutstandingOpen veto from STORAGE\Volume\CHILD'
}
function Start-Sleep { throw 'A rejected request must not enter the completion wait' }
function Test-BExplorerDrivePresent { throw 'A rejected request must not be reported as completed' }
$blocked = $false
try { Invoke-BExplorerDriveEject 'E:\' } catch { $blocked = $_.Exception.Message -like '*OutstandingOpen veto*' }
if (-not $blocked -or $script:requests -ne 1) { throw 'The veto was swallowed or retried' }
"#,
        );
    }

    #[test]
    fn windows_eject_fallback_waits_for_the_drive_letter_to_disappear() {
        check_discovery_script(
            r#"
$script:item = [pscustomobject]@{}
$script:item | Add-Member ScriptMethod Verbs { @() }
$script:folder = [pscustomobject]@{}
$script:folder | Add-Member ScriptMethod ParseName { param($Path) $script:item }
$script:shell = [pscustomobject]@{}
$script:shell | Add-Member ScriptMethod Namespace { param($Id) $script:folder }
function New-Object { param($ComObject) if ($ComObject -ne 'Shell.Application') { throw 'Unexpected COM class' }; $script:shell }
function Resolve-BExplorerUsbRemovalTarget { param($DriveRoot) if ($DriveRoot -ne 'E:\') { throw 'Wrong drive' }; 'USB\VID_TARGET' }
$script:requests = 0
function Request-BExplorerPnpRemoval { param($InstanceId) if ($InstanceId -ne 'USB\VID_TARGET') { throw 'Wrong device' }; $script:requests++ }
function Start-Sleep { param($Milliseconds) }
$script:presenceChecks = 0
function Test-BExplorerDrivePresent { param($DriveRoot) $script:presenceChecks++; $script:presenceChecks -le 2 }
Invoke-BExplorerDriveEject 'E:\'
if ($script:requests -ne 1 -or $script:presenceChecks -ne 3) { throw 'Removal was not requested exactly once and observed through completion' }
$script:presenceChecks = 0
function Test-BExplorerDrivePresent { param($DriveRoot) $script:presenceChecks++; $true }
$blocked = $false
try { Invoke-BExplorerDriveEject 'E:\' } catch { $blocked = $_.Exception.Message -like '*did not remove*' }
if (-not $blocked -or $script:requests -ne 2 -or $script:presenceChecks -ne 40) { throw 'A still-assigned letter was incorrectly reported as removed' }
"#,
        );
    }

    #[test]
    fn windows_eject_accepts_removed_optical_media_but_not_an_unready_usb_disk() {
        check_discovery_script(
            r#"
$script:verb = [pscustomobject]@{ Name = '&Eject' }
$script:verb | Add-Member ScriptMethod DoIt { $script:mediaReady = $false; $script:requests++ }
$script:item = [pscustomobject]@{}
$script:item | Add-Member ScriptMethod Verbs { @($script:verb) }
$script:folder = [pscustomobject]@{}
$script:folder | Add-Member ScriptMethod ParseName { param($Path) $script:item }
$script:shell = [pscustomobject]@{}
$script:shell | Add-Member ScriptMethod Namespace { param($Id) $script:folder }
function New-Object { param($ComObject) if ($ComObject -ne 'Shell.Application') { throw 'Unexpected COM class' }; $script:shell }
function Resolve-BExplorerUsbRemovalTarget { throw 'A Shell eject verb must not trigger the USB fallback' }
function Request-BExplorerPnpRemoval { throw 'A Shell eject verb must not request PnP removal' }
function Start-Sleep { param($Milliseconds) }
$script:presenceChecks = 0
function Test-BExplorerDrivePresent { param($DriveRoot) $script:presenceChecks++; $true }
$script:infoChecks = 0
function Get-BExplorerDriveInfo {
    param($DriveRoot)
    $script:infoChecks++
    [pscustomobject]@{ DriveType = $script:driveType; IsReady = $script:mediaReady }
}
$script:driveType = [System.IO.DriveType]::CDRom
$script:mediaReady = $true
$script:requests = 0
Invoke-BExplorerDriveEject 'G:\'
if ($script:requests -ne 1 -or $script:presenceChecks -ne 1 -or $script:infoChecks -ne 2) { throw 'An optical disc was not observed becoming empty while its drive letter remained assigned' }

# An unready USB volume with a retained letter must still fail removal.
$script:driveType = [System.IO.DriveType]::Fixed
$script:mediaReady = $false
$script:presenceChecks = 0
$blocked = $false
try { Invoke-BExplorerDriveEject 'E:\' } catch { $blocked = $_.Exception.Message -like '*did not remove*' }
if (-not $blocked -or $script:presenceChecks -ne 40 -or $script:requests -ne 2) { throw 'An unready USB disk was incorrectly treated as ejected optical media' }

# Once the letter disappears, do not query a stale DriveInfo handle again.
$script:driveType = [System.IO.DriveType]::CDRom
$script:mediaReady = $true
$script:infoChecks = 0
function Test-BExplorerDrivePresent { param($DriveRoot) $false }
function Get-BExplorerDriveInfo {
    param($DriveRoot)
    $script:infoChecks++
    if ($script:infoChecks -gt 1) { throw 'The drive was already removed' }
    [pscustomobject]@{ DriveType = $script:driveType; IsReady = $script:mediaReady }
}
Invoke-BExplorerDriveEject 'G:\'
if ($script:infoChecks -ne 1 -or $script:requests -ne 3) { throw 'A removed drive was queried after its letter disappeared' }
"#,
        );
    }
}

#[cfg(all(test, unix, not(target_os = "macos")))]
mod tests {
    use super::*;

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn parses_udisks_loop_device_output() {
        assert_eq!(
            parse_udisks_block_device("Mapped file image.iso as /dev/loop7."),
            Some(PathBuf::from("/dev/loop7"))
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn disk_image_partition_selection_ignores_tiny_auxiliary_volume() {
        let partitions = preferred_linux_partition_paths(vec![
            (PathBuf::from("/dev/loop7p2"), 256),
            (PathBuf::from("/dev/loop7p1"), 1_662_912),
        ]);
        assert_eq!(partitions, vec![PathBuf::from("/dev/loop7p1")]);
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn decodes_mountinfo_escape() {
        assert_eq!(
            decode_mountinfo_field("/media/My\\040Disk"),
            "/media/My Disk"
        );
    }

    #[test]
    fn linux_removal_uses_eject_for_optical_media_and_power_off_for_usb() {
        let drive = |optical, ejectable, can_power_off| LinuxUdisksDrive {
            object_path: OwnedObjectPath::try_from("/org/freedesktop/UDisks2/drives/TestDrive")
                .unwrap(),
            optical,
            ejectable,
            can_power_off,
        };

        assert_eq!(
            linux_drive_removal_action(&drive(true, true, false)),
            LinuxDriveRemovalAction::Eject
        );
        assert_eq!(
            linux_drive_removal_action(&drive(false, false, true)),
            LinuxDriveRemovalAction::PowerOff
        );
        assert_eq!(
            linux_drive_removal_action(&drive(false, true, true)),
            LinuxDriveRemovalAction::PowerOff
        );
        assert_eq!(
            linux_drive_removal_action(&drive(false, false, false)),
            LinuxDriveRemovalAction::UnmountOnly
        );
    }
}
