use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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

#[cfg(not(target_os = "windows"))]
pub(super) fn mount_disk_image(_path: &Path) -> Result<()> {
    Err(BExplorerError::Shell(
        "Mounting disk images is currently available on Windows only".into(),
    ))
}

#[cfg(not(target_os = "windows"))]
pub(super) fn mounted_disk_image_root(_path: &Path) -> Result<PathBuf> {
    Err(BExplorerError::Shell(
        "Resolving mounted disk images is currently available on Windows only".into(),
    ))
}

#[cfg(not(target_os = "windows"))]
pub(super) fn suppress_file_explorer_windows_at(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub(super) fn eject_drive(_path: &Path) -> Result<()> {
    Err(BExplorerError::Shell(
        "Ejecting drives is currently available on Windows only".into(),
    ))
}
