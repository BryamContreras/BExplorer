use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(target_os = "windows")]
use std::process::Stdio;

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
    let status = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "& { $DriveRoot = $env:BEXPLORER_DRIVE_ROOT; if ([string]::IsNullOrWhiteSpace($DriveRoot)) { exit 2 }; $drive = $DriveRoot.TrimEnd('\\'); $shell = New-Object -ComObject Shell.Application; $item = $shell.Namespace(17).ParseName($drive); if (-not $item) { $item = $shell.Namespace(17).ParseName($DriveRoot) }; if (-not $item) { throw \"Drive not found: $DriveRoot\" }; $item.InvokeVerb('Eject'); Start-Sleep -Milliseconds 700 }",
        ])
        .env("BEXPLORER_DRIVE_ROOT", path)
        .status()
        .map_err(|error| BExplorerError::Shell(error.to_string()))?;

    if status.success() {
        Ok(())
    } else {
        Err(BExplorerError::Shell(format!(
            "Could not eject drive {}",
            path.display()
        )))
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

    let unmount = Command::new("udisksctl")
        .args(["unmount", "--block-device"])
        .arg(&block)
        .output()
        .map_err(|error| BExplorerError::Shell(error.to_string()))?;
    if !unmount.status.success() && !udisks_error_allows_eject_continue(&unmount) {
        return Err(command_error("Could not unmount drive", &unmount));
    }

    if let Some(loop_block) = linux_loop_base_for_block(&block) {
        run_udisks_status(
            Command::new("udisksctl")
                .args(["loop-delete", "--block-device"])
                .arg(loop_block),
            "Could not delete loop device",
        )
    } else {
        let power_block = linux_parent_block_device(&block).unwrap_or(block);
        run_udisks_status(
            Command::new("udisksctl")
                .args(["power-off", "--block-device"])
                .arg(power_block),
            "Could not power off drive",
        )
    }
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
}
