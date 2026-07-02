use super::util::wide_to_string;

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowsDriveKind {
    Unknown,
    NoRootDir,
    Removable,
    Fixed,
    Remote,
    CdRom,
    RamDisk,
}

#[cfg(target_os = "windows")]
#[derive(Clone, Debug)]
pub struct WindowsDriveInfo {
    pub volume_label: Option<String>,
    pub file_system: Option<String>,
    pub kind: WindowsDriveKind,
}

#[cfg(target_os = "windows")]
pub fn drive_info(path: &std::path::Path) -> WindowsDriveInfo {
    use std::os::windows::ffi::OsStrExt;

    use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetVolumeInformationW};
    use windows::Win32::System::WindowsProgramming::{
        DRIVE_CDROM, DRIVE_FIXED, DRIVE_NO_ROOT_DIR, DRIVE_RAMDISK, DRIVE_REMOTE, DRIVE_REMOVABLE,
        DRIVE_UNKNOWN,
    };
    use windows::core::PCWSTR;

    let mut root: Vec<u16> = path.as_os_str().encode_wide().collect();
    root.push(0);

    let kind = match unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) } {
        DRIVE_REMOVABLE => WindowsDriveKind::Removable,
        DRIVE_FIXED => WindowsDriveKind::Fixed,
        DRIVE_REMOTE => WindowsDriveKind::Remote,
        DRIVE_CDROM => WindowsDriveKind::CdRom,
        DRIVE_RAMDISK => WindowsDriveKind::RamDisk,
        DRIVE_NO_ROOT_DIR => WindowsDriveKind::NoRootDir,
        DRIVE_UNKNOWN => WindowsDriveKind::Unknown,
        _ => WindowsDriveKind::Unknown,
    };

    let mut volume_name = [0_u16; 260];
    let mut file_system = [0_u16; 64];
    let volume_result = unsafe {
        GetVolumeInformationW(
            PCWSTR(root.as_ptr()),
            Some(&mut volume_name),
            None,
            None,
            None,
            Some(&mut file_system),
        )
    };

    let (volume_label, file_system) = if volume_result.is_ok() {
        (wide_to_string(&volume_name), wide_to_string(&file_system))
    } else {
        (None, None)
    };

    WindowsDriveInfo {
        volume_label,
        file_system,
        kind,
    }
}

#[cfg(target_os = "windows")]
pub fn set_volume_label(path: &std::path::Path, label: &str) -> crate::utils::errors::Result<()> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    use windows::Win32::Storage::FileSystem::SetVolumeLabelW;
    use windows::core::PCWSTR;

    let mut root = path.display().to_string().replace('/', "\\");
    if !root.ends_with('\\') {
        root.push('\\');
    }

    let mut root_wide: Vec<u16> = OsStr::new(&root).encode_wide().collect();
    root_wide.push(0);
    let mut label_wide: Vec<u16> = OsStr::new(label).encode_wide().collect();
    label_wide.push(0);

    unsafe {
        SetVolumeLabelW(PCWSTR(root_wide.as_ptr()), PCWSTR(label_wide.as_ptr())).map_err(
            |error| {
                crate::utils::errors::BExplorerError::Operation(format!(
                    "Could not rename drive {}: {error}",
                    path.display()
                ))
            },
        )?;
    }

    Ok(())
}
