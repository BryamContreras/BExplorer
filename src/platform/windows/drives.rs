use super::util::wide_to_string;

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowsDriveKind {
    Unknown,
    NoRootDir,
    Removable,
    Fixed,
    External,
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
    // GetDriveType describes the medium, not its connection. USB HDDs and
    // SSDs commonly report DRIVE_FIXED even though the device is external.
    let kind = if kind == WindowsDriveKind::Fixed {
        classify_storage_bus(kind, storage_bus_type(path))
    } else {
        kind
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
fn classify_storage_bus(
    kind: WindowsDriveKind,
    bus: Option<windows::Win32::Storage::FileSystem::STORAGE_BUS_TYPE>,
) -> WindowsDriveKind {
    use windows::Win32::Storage::FileSystem::{BusType1394, BusTypeUsb};

    if kind == WindowsDriveKind::Fixed
        && bus.is_some_and(|bus| bus == BusTypeUsb || bus == BusType1394)
    {
        WindowsDriveKind::External
    } else {
        kind
    }
}

#[cfg(target_os = "windows")]
fn volume_device_name(path: &std::path::Path) -> Option<Vec<u16>> {
    use std::path::{Component, Prefix};

    let Component::Prefix(prefix) = path.components().next()? else {
        return None;
    };
    let (Prefix::Disk(letter) | Prefix::VerbatimDisk(letter)) = prefix.kind() else {
        return None;
    };
    Some(super::util::wide_null(&format!(
        r"\\.\{}:",
        char::from(letter)
    )))
}

#[cfg(target_os = "windows")]
fn open_volume_metadata(path: &std::path::Path) -> Option<std::os::windows::io::OwnedHandle> {
    use std::os::windows::io::{FromRawHandle, OwnedHandle};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::core::PCWSTR;

    let name = volume_device_name(path)?;
    // Metadata access only: no data read/write permission, volume lock or
    // administrator elevation. Share the handle with normal file transfers.
    let handle = unsafe {
        CreateFileW(
            PCWSTR(name.as_ptr()),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    }
    .ok()?;
    Some(unsafe { OwnedHandle::from_raw_handle(handle.0) })
}

#[cfg(target_os = "windows")]
fn storage_bus_type(
    path: &std::path::Path,
) -> Option<windows::Win32::Storage::FileSystem::STORAGE_BUS_TYPE> {
    use std::mem::size_of;
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::IO::DeviceIoControl;
    use windows::Win32::System::Ioctl::{
        IOCTL_STORAGE_QUERY_PROPERTY, PropertyStandardQuery, STORAGE_DESCRIPTOR_HEADER,
        STORAGE_DEVICE_DESCRIPTOR, STORAGE_PROPERTY_QUERY, StorageDeviceProperty,
    };

    let handle = open_volume_metadata(path)?;
    let query = STORAGE_PROPERTY_QUERY {
        PropertyId: StorageDeviceProperty,
        QueryType: PropertyStandardQuery,
        ..Default::default()
    };
    let mut header = STORAGE_DESCRIPTOR_HEADER::default();
    let mut returned = 0;
    unsafe {
        DeviceIoControl(
            HANDLE(handle.as_raw_handle()),
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some((&query as *const STORAGE_PROPERTY_QUERY).cast()),
            size_of::<STORAGE_PROPERTY_QUERY>() as u32,
            Some((&mut header as *mut STORAGE_DESCRIPTOR_HEADER).cast()),
            size_of::<STORAGE_DESCRIPTOR_HEADER>() as u32,
            Some(&mut returned),
            None,
        )
    }
    .ok()?;
    if returned < size_of::<STORAGE_DESCRIPTOR_HEADER>() as u32 {
        return None;
    }
    let size = usize::try_from(header.Size).ok()?;
    // A malformed driver response must not allocate an arbitrary amount.
    if !(size_of::<STORAGE_DEVICE_DESCRIPTOR>()..=64 * 1024).contains(&size) {
        return None;
    }
    let mut descriptor = vec![0_u8; size];
    unsafe {
        DeviceIoControl(
            HANDLE(handle.as_raw_handle()),
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some((&query as *const STORAGE_PROPERTY_QUERY).cast()),
            size_of::<STORAGE_PROPERTY_QUERY>() as u32,
            Some(descriptor.as_mut_ptr().cast()),
            descriptor.len() as u32,
            Some(&mut returned),
            None,
        )
    }
    .ok()?;
    storage_bus_from_descriptor(descriptor.get(..returned as usize)?)
}

#[cfg(target_os = "windows")]
fn storage_bus_from_descriptor(
    bytes: &[u8],
) -> Option<windows::Win32::Storage::FileSystem::STORAGE_BUS_TYPE> {
    use std::mem::size_of;
    use windows::Win32::System::Ioctl::STORAGE_DEVICE_DESCRIPTOR;

    if bytes.len() < size_of::<STORAGE_DEVICE_DESCRIPTOR>() {
        return None;
    }
    // DeviceIoControl writes a C structure into a byte buffer, which need not
    // have the alignment of STORAGE_DEVICE_DESCRIPTOR.
    let descriptor = unsafe {
        bytes
            .as_ptr()
            .cast::<STORAGE_DEVICE_DESCRIPTOR>()
            .read_unaligned()
    };
    if descriptor.Version < size_of::<STORAGE_DEVICE_DESCRIPTOR>() as u32
        || descriptor.Size < size_of::<STORAGE_DEVICE_DESCRIPTOR>() as u32
        || descriptor.Size as usize > bytes.len()
    {
        return None;
    }
    Some(descriptor.BusType)
}

#[cfg(target_os = "windows")]
fn storage_device_number(path: &std::path::Path) -> Option<(u32, u32)> {
    use std::mem::size_of;
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::IO::DeviceIoControl;
    use windows::Win32::System::Ioctl::{IOCTL_STORAGE_GET_DEVICE_NUMBER, STORAGE_DEVICE_NUMBER};

    let handle = open_volume_metadata(path)?;
    let mut number = STORAGE_DEVICE_NUMBER::default();
    let mut returned = 0;
    unsafe {
        DeviceIoControl(
            HANDLE(handle.as_raw_handle()),
            IOCTL_STORAGE_GET_DEVICE_NUMBER,
            None,
            0,
            Some((&mut number as *mut STORAGE_DEVICE_NUMBER).cast()),
            size_of::<STORAGE_DEVICE_NUMBER>() as u32,
            Some(&mut returned),
            None,
        )
    }
    .ok()?;
    (returned as usize >= size_of::<STORAGE_DEVICE_NUMBER>() && number.DeviceNumber != u32::MAX)
        .then_some((number.DeviceType, number.DeviceNumber))
}

/// Safe removal affects the whole physical disk. Find its other drive-letter
/// volumes so their directory watches can also be released before requesting it.
#[cfg(target_os = "windows")]
pub(crate) fn volume_roots_on_same_disk(path: &std::path::Path) -> Vec<std::path::PathBuf> {
    use std::path::PathBuf;
    use windows::Win32::Storage::FileSystem::GetDriveTypeW;
    use windows::Win32::Storage::FileSystem::GetLogicalDrives;
    use windows::Win32::System::WindowsProgramming::{DRIVE_FIXED, DRIVE_REMOVABLE};
    use windows::core::PCWSTR;

    let mut roots = vec![path.to_path_buf()];
    let Some(device) = storage_device_number(path) else {
        return roots;
    };
    let drives = unsafe { GetLogicalDrives() };
    for index in 0..26 {
        if drives & (1 << index) == 0 {
            continue;
        }
        let root = PathBuf::from(format!("{}:\\", char::from(b'A' + index)));
        if roots.contains(&root) {
            continue;
        }
        let wide = super::util::wide_null(&root.to_string_lossy());
        let kind = unsafe { GetDriveTypeW(PCWSTR(wide.as_ptr())) };
        if matches!(kind, DRIVE_FIXED | DRIVE_REMOVABLE)
            && storage_device_number(&root) == Some(device)
        {
            roots.push(root);
        }
    }
    roots
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

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};
    use std::path::Path;
    use windows::Win32::Storage::FileSystem::{
        BusType1394, BusTypeNvme, BusTypeSata, BusTypeScsi, BusTypeUnknown, BusTypeUsb,
    };
    use windows::Win32::System::Ioctl::STORAGE_DEVICE_DESCRIPTOR;

    #[test]
    fn fixed_usb_and_firewire_disks_are_external_without_changing_internal_disks() {
        for bus in [BusTypeUsb, BusType1394] {
            assert_eq!(
                classify_storage_bus(WindowsDriveKind::Fixed, Some(bus)),
                WindowsDriveKind::External
            );
        }
        for bus in [
            None,
            Some(BusTypeSata),
            Some(BusTypeNvme),
            Some(BusTypeScsi),
            Some(BusTypeUnknown),
        ] {
            assert_eq!(
                classify_storage_bus(WindowsDriveKind::Fixed, bus),
                WindowsDriveKind::Fixed
            );
        }
        for kind in [
            WindowsDriveKind::Removable,
            WindowsDriveKind::CdRom,
            WindowsDriveKind::Remote,
        ] {
            assert_eq!(classify_storage_bus(kind, Some(BusTypeUsb)), kind);
        }
    }

    #[test]
    fn volume_device_names_accept_drive_prefixes_and_reject_network_paths() {
        for path in [r"E:\", "E:/", r"\\?\E:\"] {
            assert_eq!(
                String::from_utf16_lossy(&volume_device_name(Path::new(path)).unwrap()),
                "\\\\.\\E:\0"
            );
        }
        for path in [r"\\server\share\", r"\\?\UNC\server\share\", "relative"] {
            assert!(volume_device_name(Path::new(path)).is_none());
        }
    }

    #[test]
    fn storage_descriptor_rejects_truncated_and_malformed_responses() {
        let size = size_of::<STORAGE_DEVICE_DESCRIPTOR>();
        let mut bytes = vec![0_u8; size];
        bytes[..4].copy_from_slice(&(size as u32).to_le_bytes());
        bytes[4..8].copy_from_slice(&(size as u32).to_le_bytes());
        let bus_offset = offset_of!(STORAGE_DEVICE_DESCRIPTOR, BusType);
        bytes[bus_offset..bus_offset + 4].copy_from_slice(&BusTypeUsb.0.to_le_bytes());
        assert_eq!(storage_bus_from_descriptor(&bytes), Some(BusTypeUsb));
        assert_eq!(storage_bus_from_descriptor(&bytes[..size - 1]), None);
        bytes[4..8].copy_from_slice(&((size + 1) as u32).to_le_bytes());
        assert_eq!(storage_bus_from_descriptor(&bytes), None);
        bytes[4..8].copy_from_slice(&(size as u32).to_le_bytes());
        bytes[..4].copy_from_slice(&0_u32.to_le_bytes());
        assert_eq!(storage_bus_from_descriptor(&bytes), None);
    }

    #[test]
    #[ignore = "requires BEXPLORER_TEST_EXTERNAL_DRIVE pointing to a connected USB HDD or SSD; queries metadata only"]
    fn connected_usb_fixed_drive_is_classified_as_external() {
        let path = std::env::var_os("BEXPLORER_TEST_EXTERNAL_DRIVE")
            .expect("set BEXPLORER_TEST_EXTERNAL_DRIVE to the external drive root");
        let info = crate::fs::explorer::list_storage_entries(false).expect("list storage devices");
        for entry in &info {
            eprintln!(
                "{} {}: {:?}",
                entry.path.display(),
                entry.name,
                entry.drive_kind
            );
        }
        eprintln!(
            "Volumes on the same disk: {:?}",
            volume_roots_on_same_disk(Path::new(&path))
        );
        let entry = info
            .iter()
            .find(|entry| entry.path == Path::new(&path))
            .expect("external drive is listed");
        assert_eq!(
            entry.drive_kind,
            Some(crate::fs::explorer::DriveKind::External)
        );
        assert!(entry.drive_kind.unwrap().is_ejectable());
    }
}
