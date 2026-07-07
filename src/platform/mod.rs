pub mod shell;

#[cfg(all(unix, not(target_os = "macos")))]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "windows")]
pub mod windows;

use std::path::Path;

use crate::utils::errors::Result;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriveKind {
    Unknown,
    NoRootDir,
    Removable,
    Fixed,
    Remote,
    CdRom,
    RamDisk,
}

#[derive(Clone, Debug)]
pub struct DriveInfo {
    pub volume_label: Option<String>,
    pub file_system: Option<String>,
    pub kind: DriveKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortableDeviceInfo {
    pub id: String,
    pub name: String,
    pub manufacturer: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortableObjectInfo {
    pub id: String,
    pub name: String,
    pub is_folder: bool,
    pub size: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkDeviceKind {
    Computer,
    Printer,
    Scanner,
    Multifunction,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkComputerInfo {
    pub name: String,
    pub comment: String,
    pub kind: NetworkDeviceKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkShareInfo {
    pub name: String,
    pub remark: String,
}

pub struct NativeIconImage {
    pub rgba: Vec<u8>,
    pub width: usize,
    pub height: usize,
}

#[cfg(target_os = "windows")]
pub use windows::PortableDeviceSession;

#[cfg(not(target_os = "windows"))]
pub struct PortableDeviceSession;

#[cfg(target_os = "windows")]
pub fn apply_small_window_corners(handle: &raw_window_handle::WindowHandle<'_>) -> Result<()> {
    windows::apply_small_window_corners(handle)
}

#[cfg(target_os = "windows")]
pub fn install_autoplay_cancel(handle: &raw_window_handle::WindowHandle<'_>) -> Result<()> {
    windows::install_autoplay_cancel(handle)
}

#[cfg(target_os = "windows")]
pub fn file_paste_shortcut_down() -> bool {
    windows::file_paste_shortcut_down()
}

#[cfg(not(target_os = "windows"))]
pub fn file_paste_shortcut_down() -> bool {
    false
}

#[cfg(not(target_os = "windows"))]
pub fn apply_small_window_corners(_handle: &raw_window_handle::WindowHandle<'_>) -> Result<()> {
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn drive_info(path: &Path) -> DriveInfo {
    let info = windows::drive_info(path);
    DriveInfo {
        volume_label: info.volume_label,
        file_system: info.file_system,
        kind: match info.kind {
            windows::WindowsDriveKind::Unknown => DriveKind::Unknown,
            windows::WindowsDriveKind::NoRootDir => DriveKind::NoRootDir,
            windows::WindowsDriveKind::Removable => DriveKind::Removable,
            windows::WindowsDriveKind::Fixed => DriveKind::Fixed,
            windows::WindowsDriveKind::Remote => DriveKind::Remote,
            windows::WindowsDriveKind::CdRom => DriveKind::CdRom,
            windows::WindowsDriveKind::RamDisk => DriveKind::RamDisk,
        },
    }
}

#[cfg(target_os = "windows")]
pub fn set_volume_label(path: &Path, label: &str) -> Result<()> {
    windows::set_volume_label(path, label)
}

#[cfg(not(target_os = "windows"))]
pub fn set_volume_label(_path: &Path, _label: &str) -> Result<()> {
    Err(crate::utils::errors::BExplorerError::Operation(
        "Renaming drive labels is currently available on Windows only".into(),
    ))
}

#[cfg(not(target_os = "windows"))]
pub fn drive_info(_path: &Path) -> DriveInfo {
    DriveInfo {
        volume_label: None,
        file_system: None,
        kind: DriveKind::Unknown,
    }
}

#[cfg(target_os = "windows")]
pub fn portable_devices() -> Vec<PortableDeviceInfo> {
    windows::portable_devices()
        .into_iter()
        .map(|device| PortableDeviceInfo {
            id: device.id,
            name: device.name,
            manufacturer: device.manufacturer,
            description: device.description,
        })
        .collect()
}

#[cfg(not(target_os = "windows"))]
pub fn portable_devices() -> Vec<PortableDeviceInfo> {
    Vec::new()
}

#[cfg(target_os = "windows")]
pub fn portable_device_objects_result(
    device_id: &str,
    parent_object_id: &str,
) -> Result<Vec<PortableObjectInfo>> {
    windows::portable_device_objects_result(device_id, parent_object_id).map(|objects| {
        objects
            .into_iter()
            .map(|object| PortableObjectInfo {
                id: object.id,
                name: object.name,
                is_folder: object.is_folder,
                size: object.size,
            })
            .collect()
    })
}

#[cfg(not(target_os = "windows"))]
pub fn portable_device_objects_result(
    _device_id: &str,
    _parent_object_id: &str,
) -> Result<Vec<PortableObjectInfo>> {
    unsupported_portable_devices()
}

#[cfg(target_os = "windows")]
pub fn portable_device_object_info(device_id: &str, object_id: &str) -> Result<PortableObjectInfo> {
    windows::portable_device_object_info(device_id, object_id).map(|object| PortableObjectInfo {
        id: object.id,
        name: object.name,
        is_folder: object.is_folder,
        size: object.size,
    })
}

#[cfg(not(target_os = "windows"))]
pub fn portable_device_object_info(
    _device_id: &str,
    _object_id: &str,
) -> Result<PortableObjectInfo> {
    Err(crate::utils::errors::BExplorerError::Operation(
        "Portable devices are not supported on this platform yet".into(),
    ))
}

#[cfg(target_os = "windows")]
pub fn portable_delete_objects(device_id: &str, object_ids: &[String]) -> Result<usize> {
    windows::portable_delete_objects(device_id, object_ids)
}

#[cfg(not(target_os = "windows"))]
pub fn portable_delete_objects(_device_id: &str, _object_ids: &[String]) -> Result<usize> {
    Err(crate::utils::errors::BExplorerError::Operation(
        "Portable devices are not supported on this platform yet".into(),
    ))
}

#[cfg(target_os = "windows")]
pub fn portable_device_thumbnail(
    device_id: &str,
    object_id: &str,
    max_bytes: usize,
    allow_default_resource: bool,
) -> Option<Vec<u8>> {
    windows::portable_device_thumbnail(device_id, object_id, max_bytes, allow_default_resource)
}

#[cfg(not(target_os = "windows"))]
pub fn portable_device_thumbnail(
    _device_id: &str,
    _object_id: &str,
    _max_bytes: usize,
    _allow_default_resource: bool,
) -> Option<Vec<u8>> {
    None
}

#[cfg(target_os = "windows")]
pub fn network_computers() -> Vec<NetworkComputerInfo> {
    windows::network_computers()
        .into_iter()
        .map(|computer| NetworkComputerInfo {
            name: computer.name,
            comment: computer.comment,
            kind: computer.kind,
        })
        .collect()
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn network_computers() -> Vec<NetworkComputerInfo> {
    linux::network_computers()
}

#[cfg(not(any(target_os = "windows", all(unix, not(target_os = "macos")))))]
pub fn network_computers() -> Vec<NetworkComputerInfo> {
    Vec::new()
}

#[cfg(target_os = "windows")]
pub fn network_computers_fast() -> Vec<NetworkComputerInfo> {
    windows::network_computers_fast()
        .into_iter()
        .map(|computer| NetworkComputerInfo {
            name: computer.name,
            comment: computer.comment,
            kind: computer.kind,
        })
        .collect()
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn network_computers_fast() -> Vec<NetworkComputerInfo> {
    linux::network_computers()
}

#[cfg(not(any(target_os = "windows", all(unix, not(target_os = "macos")))))]
pub fn network_computers_fast() -> Vec<NetworkComputerInfo> {
    Vec::new()
}

#[cfg(target_os = "windows")]
pub fn network_computers_netbios_cached() -> Vec<NetworkComputerInfo> {
    windows::network_computers_netbios_cached()
        .into_iter()
        .map(|computer| NetworkComputerInfo {
            name: computer.name,
            comment: computer.comment,
            kind: computer.kind,
        })
        .collect()
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn network_computers_netbios_cached() -> Vec<NetworkComputerInfo> {
    linux::network_computers()
}

#[cfg(not(any(target_os = "windows", all(unix, not(target_os = "macos")))))]
pub fn network_computers_netbios_cached() -> Vec<NetworkComputerInfo> {
    Vec::new()
}

#[cfg(target_os = "windows")]
pub fn network_printer_devices() -> Vec<NetworkComputerInfo> {
    windows::network_printer_devices()
        .into_iter()
        .map(|computer| NetworkComputerInfo {
            name: computer.name,
            comment: computer.comment,
            kind: computer.kind,
        })
        .collect()
}

#[cfg(not(target_os = "windows"))]
pub fn network_printer_devices() -> Vec<NetworkComputerInfo> {
    Vec::new()
}

#[cfg(target_os = "windows")]
pub fn network_function_devices() -> Vec<NetworkComputerInfo> {
    windows::network_function_devices()
        .into_iter()
        .map(|computer| NetworkComputerInfo {
            name: computer.name,
            comment: computer.comment,
            kind: computer.kind,
        })
        .collect()
}

#[cfg(not(target_os = "windows"))]
pub fn network_function_devices() -> Vec<NetworkComputerInfo> {
    Vec::new()
}

#[cfg(target_os = "windows")]
pub fn network_computers_wnet() -> Vec<NetworkComputerInfo> {
    windows::network_computers_wnet()
        .into_iter()
        .map(|computer| NetworkComputerInfo {
            name: computer.name,
            comment: computer.comment,
            kind: computer.kind,
        })
        .collect()
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn network_computers_wnet() -> Vec<NetworkComputerInfo> {
    linux::network_computers()
}

#[cfg(not(any(target_os = "windows", all(unix, not(target_os = "macos")))))]
pub fn network_computers_wnet() -> Vec<NetworkComputerInfo> {
    Vec::new()
}

#[cfg(target_os = "windows")]
pub fn network_shell_devices() -> Vec<NetworkComputerInfo> {
    windows::network_shell_devices()
        .into_iter()
        .map(|computer| NetworkComputerInfo {
            name: computer.name,
            comment: computer.comment,
            kind: computer.kind,
        })
        .collect()
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn network_shell_devices() -> Vec<NetworkComputerInfo> {
    linux::network_computers()
}

#[cfg(not(any(target_os = "windows", all(unix, not(target_os = "macos")))))]
pub fn network_shell_devices() -> Vec<NetworkComputerInfo> {
    Vec::new()
}

#[cfg(target_os = "windows")]
pub fn network_netbios_neighbor_addresses() -> Vec<String> {
    windows::network_netbios_neighbor_addresses()
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn network_netbios_neighbor_addresses() -> Vec<String> {
    linux::network_neighbor_addresses()
}

#[cfg(not(any(target_os = "windows", all(unix, not(target_os = "macos")))))]
pub fn network_netbios_neighbor_addresses() -> Vec<String> {
    Vec::new()
}

#[cfg(target_os = "windows")]
pub fn network_computer_netbios_at(address: &str) -> Option<NetworkComputerInfo> {
    windows::network_computer_netbios_at(address).map(|computer| NetworkComputerInfo {
        name: computer.name,
        comment: computer.comment,
        kind: computer.kind,
    })
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn network_computer_netbios_at(address: &str) -> Option<NetworkComputerInfo> {
    linux::network_computer_at(address)
}

#[cfg(not(any(target_os = "windows", all(unix, not(target_os = "macos")))))]
pub fn network_computer_netbios_at(_address: &str) -> Option<NetworkComputerInfo> {
    None
}

#[cfg(target_os = "windows")]
pub fn network_shares(host: &str) -> Vec<NetworkShareInfo> {
    windows::network_shares(host)
        .into_iter()
        .map(|share| NetworkShareInfo {
            name: share.name,
            remark: share.remark,
        })
        .collect()
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn network_shares(host: &str) -> Vec<NetworkShareInfo> {
    linux::network_shares(host)
}

#[cfg(not(any(target_os = "windows", all(unix, not(target_os = "macos")))))]
pub fn network_shares(_host: &str) -> Vec<NetworkShareInfo> {
    Vec::new()
}

#[cfg(target_os = "windows")]
pub fn prompt_network_credentials_for_path(path: &Path) -> bool {
    windows::prompt_network_credentials_for_path(path)
}

#[cfg(not(target_os = "windows"))]
pub fn prompt_network_credentials_for_path(_path: &Path) -> bool {
    false
}

#[cfg(target_os = "windows")]
pub fn native_file_icon(path: &Path, is_directory: bool, size: u32) -> Option<NativeIconImage> {
    windows::native_file_icon(path, is_directory, size).map(|icon| NativeIconImage {
        rgba: icon.rgba,
        width: icon.width,
        height: icon.height,
    })
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn native_file_icon(path: &Path, is_directory: bool, size: u32) -> Option<NativeIconImage> {
    linux::native_file_icon(path, is_directory, size)
}

#[cfg(not(any(target_os = "windows", all(unix, not(target_os = "macos")))))]
pub fn native_file_icon(_path: &Path, _is_directory: bool, _size: u32) -> Option<NativeIconImage> {
    None
}

#[cfg(target_os = "windows")]
pub fn native_file_icon_highres(path: &Path, is_directory: bool) -> Option<NativeIconImage> {
    windows::native_file_icon_highres(path, is_directory).map(|icon| NativeIconImage {
        rgba: icon.rgba,
        width: icon.width,
        height: icon.height,
    })
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn native_file_icon_highres(path: &Path, is_directory: bool) -> Option<NativeIconImage> {
    linux::native_file_icon_highres(path, is_directory)
}

#[cfg(not(any(target_os = "windows", all(unix, not(target_os = "macos")))))]
pub fn native_file_icon_highres(_path: &Path, _is_directory: bool) -> Option<NativeIconImage> {
    None
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn cached_desktop_thumbnail(path: &Path) -> Option<NativeIconImage> {
    linux::cached_desktop_thumbnail(path)
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
pub fn cached_desktop_thumbnail(_path: &Path) -> Option<NativeIconImage> {
    None
}

#[cfg(not(target_os = "windows"))]
fn unsupported_portable_devices<T>() -> Result<T> {
    Err(crate::utils::errors::BExplorerError::Operation(
        "Portable devices are not supported on this platform yet".into(),
    ))
}
