pub mod shell;

#[cfg(all(unix, not(target_os = "macos")))]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "windows")]
pub mod windows;

use std::path::{Path, PathBuf};

use raw_window_handle::{
    DisplayHandle, HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle,
    WindowHandle,
};

use crate::app::config::VibrancyMode;
use crate::utils::errors::Result;

#[cfg(target_os = "linux")]
pub const LINUX_APPLICATION_ID: &str = "bexplorer";

#[cfg(target_os = "windows")]
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

#[cfg(target_os = "windows")]
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

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
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

pub fn storage_change_receiver() -> std::sync::mpsc::Receiver<()> {
    #[cfg(target_os = "windows")]
    return windows::storage_change_receiver();

    #[cfg(target_os = "linux")]
    return linux::storage_change_receiver();

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let (_sender, receiver) = std::sync::mpsc::channel();
        receiver
    }
}

pub fn prepare_storage_change_notifications(window: &WindowHandle<'_>) -> Result<()> {
    #[cfg(target_os = "windows")]
    return windows::install_storage_change_notifications(window);

    #[cfg(not(target_os = "windows"))]
    {
        let _ = window;
        Ok(())
    }
}

/// Initializes the native drag-and-drop bridge for a window. This is a no-op
/// on platforms whose implementation does not need window registration.
pub fn prepare_external_file_drag(display: RawDisplayHandle, window: RawWindowHandle) {
    #[cfg(all(unix, not(target_os = "macos")))]
    linux::prepare_native_file_drag(display, window);

    #[cfg(not(all(unix, not(target_os = "macos"))))]
    let _ = (display, window);
}

/// Releases integrations that borrow a native window before that window is
/// destroyed. Backends without borrowed per-window resources are a no-op.
pub fn release_external_window_resources(display: RawDisplayHandle, window: RawWindowHandle) {
    #[cfg(all(unix, not(target_os = "macos")))]
    linux::release_native_window_resources(display, window);

    #[cfg(not(all(unix, not(target_os = "macos"))))]
    let _ = (display, window);
}

/// Releases every integration borrowing objects from a native display.
///
/// This must run while the display is still alive. It is used during
/// application shutdown because auxiliary windows may still own KWin blur
/// contexts when the main window closes.
pub fn release_external_display_resources(display: RawDisplayHandle) {
    #[cfg(all(unix, not(target_os = "macos")))]
    linux::release_native_display_resources(display);

    #[cfg(not(all(unix, not(target_os = "macos"))))]
    let _ = display;
}

/// Starts an operating-system file drag so another application can receive
/// real file references, rather than an in-app approximation.
pub fn start_external_file_drag(
    paths: Vec<std::path::PathBuf>,
    display: RawDisplayHandle,
    window: RawWindowHandle,
) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        let _ = (display, window);
        windows::release_mouse_capture();
        windows::start_file_drag(paths).map(|_| ())
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        linux::start_file_drag(paths, display, window).map(|_| ())
    }

    #[cfg(target_os = "macos")]
    {
        let _ = (paths, display, window);
        Err(crate::utils::errors::BExplorerError::Shell(
            "Native file dragging is not available on this macOS backend yet".into(),
        ))
    }
}

/// Returns whether an external drag remains active after dispatching any
/// pending native data-transfer events.
pub fn poll_external_file_drag(display: RawDisplayHandle, window: RawWindowHandle) -> Result<bool> {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        linux::poll_native_file_drag(display, window)
    }

    #[cfg(not(all(unix, not(target_os = "macos"))))]
    {
        let _ = (display, window);
        Ok(false)
    }
}

/// Retrieves local file paths delivered by a native drag-and-drop operation.
/// Window toolkits currently do not surface this event on Wayland, so Linux
/// reads it from its data-device bridge; other platforms can use the toolkit
/// event and therefore return an empty list here.
pub fn take_external_file_drops(
    display: RawDisplayHandle,
    window: RawWindowHandle,
) -> Vec<Vec<PathBuf>> {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        linux::take_native_file_drops(display, window).0
    }

    #[cfg(not(all(unix, not(target_os = "macos"))))]
    {
        let _ = (display, window);
        Vec::new()
    }
}

/// Returns an event-driven notification channel for completed native file
/// drops. Platforms whose window backend already reports `FileDropped` return
/// a disconnected receiver because no auxiliary listener is necessary.
pub fn external_file_drop_receiver() -> std::sync::mpsc::Receiver<()> {
    #[cfg(all(unix, not(target_os = "macos")))]
    return linux::native_file_drop_receiver();

    #[cfg(not(all(unix, not(target_os = "macos"))))]
    {
        let (_sender, receiver) = std::sync::mpsc::channel();
        receiver
    }
}

#[cfg(target_os = "windows")]
pub use windows::PortableDeviceSession;

pub fn mounted_network_path(path: &Path) -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        path.exists().then(|| path.to_path_buf())
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        linux::mounted_network_path(path)
    }

    #[cfg(target_os = "macos")]
    {
        macos::mounted_network_path(path)
    }

    #[cfg(not(any(target_os = "windows", unix)))]
    {
        let _ = path;
        None
    }
}

#[cfg(target_os = "windows")]
pub fn apply_window_corners(
    window: &WindowHandle<'_>,
    _display: &DisplayHandle<'_>,
    _width: u32,
    _height: u32,
    radius: u32,
) -> Result<()> {
    windows::apply_small_window_corners(window, radius)
}

pub fn apply_window_vibrancy<W: HasWindowHandle + HasDisplayHandle + ?Sized>(
    window: &W,
    mode: VibrancyMode,
    intensity: u8,
    dark: bool,
    _width: u32,
    _height: u32,
    _radius: u32,
) -> Result<bool> {
    #[cfg(target_os = "windows")]
    {
        use window_vibrancy::{apply_acrylic, clear_acrylic, clear_blur, clear_mica};

        let _ = clear_mica(window);
        let _ = clear_acrylic(window);
        let _ = clear_blur(window);
        match mode {
            VibrancyMode::None => Ok(false),
            VibrancyMode::Mica | VibrancyMode::Blur => Ok(false),
            VibrancyMode::Acrylic => {
                // Keep the native tint a touch lighter than the configured
                // intensity so the acrylic backdrop is more noticeable while
                // retaining enough contrast for the explorer content.
                let alpha = (intensity.min(100) as u16 * 9 / 5) as u8;
                let color = if dark {
                    (24, 29, 32, alpha)
                } else {
                    (246, 246, 248, alpha)
                };
                apply_acrylic(window, Some(color))
                    .map(|_| true)
                    .map_err(|error| {
                        crate::utils::errors::BExplorerError::Operation(error.to_string())
                    })
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        use window_vibrancy::{
            NSVisualEffectMaterial, NSVisualEffectState, apply_vibrancy, clear_vibrancy,
        };

        let _ = clear_vibrancy(window);
        match mode {
            VibrancyMode::None => Ok(false),
            VibrancyMode::Mica | VibrancyMode::Acrylic | VibrancyMode::Blur => apply_vibrancy(
                window,
                NSVisualEffectMaterial::WindowBackground,
                Some(NSVisualEffectState::FollowsWindowActiveState),
                Some(f64::from(intensity.min(100)) / 7.0),
            )
            .map(|_| true)
            .map_err(|error| crate::utils::errors::BExplorerError::Operation(error.to_string())),
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        // KWin exposes blur through an optional Wayland protocol. GNOME uses
        // Blur My Shell because Mutter has no client-facing blur protocol.
        // Unsupported backends report an inactive result so the UI retains its
        // opaque, readable fallback.
        let _ = dark;
        if mode == VibrancyMode::Blur {
            match linux::ensure_kwin_blur_effect() {
                Ok(true) => crate::utils::log::info(
                    "Requested KWin Blur effect before binding BExplorer's Wayland surface",
                ),
                Ok(false) => {}
                Err(error) => crate::utils::log::info(format!(
                    "Could not load KWin Blur effect automatically: {error}"
                )),
            }
        }
        match linux::apply_window_blur(
            window,
            mode == VibrancyMode::Blur,
            intensity,
            _width,
            _height,
            _radius,
        ) {
            Ok(active) => Ok(active),
            Err(error) => {
                crate::utils::log::info(format!(
                    "Native Wayland blur unavailable; using opaque fallback: {error}"
                ));
                Ok(false)
            }
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn apply_window_corners(
    window: &WindowHandle<'_>,
    display: &DisplayHandle<'_>,
    width: u32,
    height: u32,
    radius: u32,
) -> Result<()> {
    linux::apply_window_corners(window, display, width, height, radius)
}

#[cfg(target_os = "macos")]
pub fn apply_window_corners(
    _window: &WindowHandle<'_>,
    _display: &DisplayHandle<'_>,
    _width: u32,
    _height: u32,
    _radius: u32,
) -> Result<()> {
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn install_main_window_hooks(handle: &raw_window_handle::WindowHandle<'_>) -> Result<()> {
    windows::install_main_window_hooks(handle)
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

#[cfg(target_os = "macos")]
pub fn network_computers() -> Vec<NetworkComputerInfo> {
    macos::network_computers()
}

#[cfg(not(any(target_os = "windows", unix)))]
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

#[cfg(target_os = "macos")]
pub fn network_computers_fast() -> Vec<NetworkComputerInfo> {
    macos::network_computers()
}

#[cfg(not(any(target_os = "windows", unix)))]
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
    Vec::new()
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
    Vec::new()
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
    Vec::new()
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

#[cfg(target_os = "macos")]
pub fn network_shares(host: &str) -> Vec<NetworkShareInfo> {
    macos::network_shares(host)
}

#[cfg(not(any(target_os = "windows", unix)))]
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

#[cfg(target_os = "linux")]
pub fn native_named_icon(name: &str, size: u32) -> Option<NativeIconImage> {
    linux::native_named_icon(name, size)
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
