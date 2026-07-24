//! Linux-specific platform hooks live here.
//!
//! Keep Linux integrations behind the neutral functions exported from
//! `crate::platform` so application and filesystem code stay portable.

#[cfg(target_os = "linux")]
mod gnome_blur;
mod kio;
mod kwin_blur;
#[cfg(target_os = "linux")]
mod storage_watch;
mod wayland_drag;

#[cfg(target_os = "linux")]
pub use storage_watch::storage_change_receiver;

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::UNIX_EPOCH;

use directories::UserDirs;
use raw_window_handle::{
    DisplayHandle, HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle,
    WindowHandle,
};

use crate::platform::NativeIconImage;
use crate::platform::{NetworkComputerInfo, NetworkDeviceKind, NetworkShareInfo};
use crate::utils::errors::{BExplorerError, Result};

const ICON_EXTENSIONS: &[&str] = &["png", "svg"];
const FALLBACK_THEMES: &[&str] = &["Adwaita", "Breeze", "Yaru", "Papirus", "hicolor"];
const LINUX_DRAG_HELPERS: &[LinuxDragHelper] = &[
    LinuxDragHelper {
        program: "ripdrag",
        args: &[
            "--and-exit",
            "--all",
            "--no-click",
            "--basename",
            "--icon-size",
            "64",
            "--content-width",
            "360",
            "--content-height",
            "180",
        ],
    },
    LinuxDragHelper {
        program: "dragon-drag-and-drop",
        args: &["--and-exit"],
    },
    LinuxDragHelper {
        program: "dragon",
        args: &["--and-exit"],
    },
    LinuxDragHelper {
        program: "dragon-drop",
        args: &[],
    },
];

#[derive(Clone, Copy, Debug)]
struct LinuxDragHelper {
    program: &'static str,
    args: &'static [&'static str],
}

#[derive(Clone, Debug)]
struct IconTheme {
    inherits: Vec<String>,
    directories: Vec<IconThemeDirectory>,
}

#[derive(Clone, Debug)]
struct IconThemeDirectory {
    name: String,
    size: u32,
    min_size: u32,
    max_size: u32,
    threshold: u32,
    scale: u32,
    kind: IconThemeDirectoryKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IconThemeDirectoryKind {
    Fixed,
    Scalable,
    Threshold,
}

#[derive(Clone, Debug)]
struct IconThemeStore {
    themed_base_dirs: Vec<PathBuf>,
    fallback_dirs: Vec<PathBuf>,
    current_theme: String,
}

#[derive(Clone, Debug)]
struct MimeInfo {
    globs: Vec<MimeGlob>,
    aliases: HashMap<String, String>,
    generic_icons: HashMap<String, String>,
}

#[derive(Clone, Debug)]
struct MimeGlob {
    priority: i32,
    mime: String,
    pattern: String,
    suffix: Option<String>,
    literal: Option<String>,
}

pub fn native_file_icon(path: &Path, is_directory: bool, size: u32) -> Option<NativeIconImage> {
    desktop_icon_for_path(path, is_directory, size.clamp(16, 512))
}

pub fn native_file_icon_highres(path: &Path, is_directory: bool) -> Option<NativeIconImage> {
    desktop_icon_for_path(path, is_directory, 256)
}

/// Resolves a Freedesktop application icon name (or a direct icon path) using
/// the active desktop icon theme.
pub fn native_named_icon(name: &str, size: u32) -> Option<NativeIconImage> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }

    let size = size.clamp(16, 512);
    let path = Path::new(name);
    if path.is_file()
        && let Some(icon) = load_icon_path(path, size)
    {
        return Some(icon);
    }

    // Although the desktop-entry specification normally omits extensions for
    // themed icons, real-world entries occasionally include .png or .svg.
    // Strip only those known extensions; dots are otherwise valid in icon
    // names such as `org.gnome.Nautilus`.
    let themed_name = match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("png" | "svg") => path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or(name),
        _ => name,
    };
    let icon_path = icon_theme_store().find_best_icon(&[themed_name.to_owned()], size)?;
    load_icon_path(&icon_path, size)
}

pub fn cached_desktop_thumbnail(path: &Path, size: u32) -> Option<NativeIconImage> {
    let uri = canonical_file_uri(path)?;
    let hash = thumbnail_hash_for_uri(&uri);
    let cache_home = xdg_cache_home();
    let metadata = fs::metadata(path).ok()?;

    let directories = if size <= 128 {
        ["normal", "large", "x-large", "xx-large"]
    } else {
        ["xx-large", "x-large", "large", "normal"]
    };
    for directory in directories {
        let thumbnail_path = cache_home
            .join("thumbnails")
            .join(directory)
            .join(format!("{hash}.png"));
        let Ok(bytes) = fs::read(thumbnail_path) else {
            continue;
        };
        if !thumbnail_metadata_is_current(&bytes, &metadata, &uri) {
            continue;
        }
        if let Some(image) = load_png_icon(&bytes, size) {
            return Some(image);
        }
    }

    None
}

pub fn apply_window_corners(
    window_handle: &WindowHandle<'_>,
    display_handle: &DisplayHandle<'_>,
    width: u32,
    height: u32,
    radius: u32,
) -> Result<()> {
    if matches!(display_handle.as_raw(), RawDisplayHandle::Wayland(_))
        && matches!(window_handle.as_raw(), RawWindowHandle::Wayland(_))
    {
        return kwin_blur::update_window_blur_region(
            display_handle.as_raw(),
            window_handle.as_raw(),
            width,
            height,
            radius,
        );
    }

    let (RawDisplayHandle::Xlib(display_handle), RawWindowHandle::Xlib(window_handle)) =
        (display_handle.as_raw(), window_handle.as_raw())
    else {
        return Ok(());
    };

    let Some(display) = display_handle.display else {
        return Ok(());
    };

    apply_x11_window_shape(display.as_ptr(), window_handle.window, radius);
    Ok(())
}

/// Requests native KWin blur, or registers the application with Blur My Shell
/// on GNOME Wayland. Unsupported backends return an error when enabling so the
/// UI can switch to its readable opaque fallback.
pub fn apply_window_blur<W: HasWindowHandle + HasDisplayHandle + ?Sized>(
    window: &W,
    enabled: bool,
    intensity: u8,
    width: u32,
    height: u32,
    radius: u32,
) -> Result<bool> {
    #[cfg(target_os = "linux")]
    {
        if gnome_blur::is_gnome_wayland() {
            return gnome_blur::set_application_blur(enabled, intensity);
        }
    }

    let display_handle = window.display_handle().map_err(|error| {
        BExplorerError::Operation(format!("Could not access display handle for blur: {error}"))
    })?;
    let window_handle = window.window_handle().map_err(|error| {
        BExplorerError::Operation(format!("Could not access window handle for blur: {error}"))
    })?;

    if !matches!(display_handle.as_raw(), RawDisplayHandle::Wayland(_)) {
        return if enabled {
            Err(BExplorerError::Operation(
                "Window blur is unavailable on this Linux display backend".into(),
            ))
        } else {
            Ok(false)
        };
    }

    kwin_blur::set_window_blur(
        display_handle.as_raw(),
        window_handle.as_raw(),
        enabled,
        width,
        height,
        radius,
    )?;
    Ok(enabled)
}

pub fn is_gnome_wayland() -> bool {
    #[cfg(target_os = "linux")]
    {
        gnome_blur::is_gnome_wayland()
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// Loads KWin's built-in Blur effect through its session D-Bus interface.
///
/// Plasma does not publish `org_kde_kwin_blur_manager` until the effect is
/// loaded. A session restart can therefore make an otherwise supported KWin
/// compositor look unsupported to a Wayland client. Keeping this request here
/// means BExplorer can restore its own native blur without a shell command or
/// a manual visit to System Settings. Other desktops are intentionally left
/// untouched.
pub fn ensure_kwin_blur_effect() -> Result<bool> {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let session = std::env::var("DESKTOP_SESSION").unwrap_or_default();
    let session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    let is_plasma_wayland = session_type.eq_ignore_ascii_case("wayland")
        && (desktop.to_ascii_lowercase().contains("kde")
            || session.to_ascii_lowercase().contains("plasma"));
    if !is_plasma_wayland {
        return Ok(false);
    }

    let connection = zbus::blocking::Connection::session().map_err(|error| {
        BExplorerError::Operation(format!(
            "Could not connect to the Plasma session bus: {error}"
        ))
    })?;
    let reply = connection
        .call_method(
            Some("org.kde.KWin"),
            "/Effects",
            Some("org.kde.kwin.Effects"),
            "loadEffect",
            &("blur",),
        )
        .map_err(|error| {
            BExplorerError::Operation(format!("Could not load KWin Blur effect: {error}"))
        })?;
    let loaded = reply.body().deserialize::<bool>().map_err(|error| {
        BExplorerError::Operation(format!("Could not read KWin Blur response: {error}"))
    })?;
    if loaded {
        crate::utils::log::info("KWin Blur effect loaded for BExplorer");
    }
    Ok(loaded)
}

pub fn prepare_native_file_drag(
    raw_display_handle: RawDisplayHandle,
    raw_window_handle: RawWindowHandle,
) {
    wayland_drag::prepare(raw_display_handle, raw_window_handle);
}

pub fn release_native_window_resources(
    raw_display_handle: RawDisplayHandle,
    raw_window_handle: RawWindowHandle,
) {
    kwin_blur::release_window(raw_display_handle, raw_window_handle);
    wayland_drag::release_window(raw_display_handle, raw_window_handle);
}

pub fn release_native_display_resources(raw_display_handle: RawDisplayHandle) {
    kwin_blur::release_display(raw_display_handle);
    wayland_drag::release_display(raw_display_handle);
}

pub fn take_native_file_drops(
    raw_display_handle: RawDisplayHandle,
    raw_window_handle: RawWindowHandle,
) -> (Vec<Vec<PathBuf>>, bool) {
    if !matches!(raw_display_handle, RawDisplayHandle::Wayland(_)) {
        return (Vec::new(), false);
    }
    let _ = raw_window_handle;
    let drops = smithay_clipboard::take_file_drops();
    if !drops.is_empty() {
        let count = drops.iter().map(Vec::len).sum::<usize>();
        crate::utils::log::info(format!(
            "Wayland clipboard receiver delivered {count} dropped file path(s)"
        ));
    }
    (drops, false)
}

pub fn native_file_drop_receiver() -> std::sync::mpsc::Receiver<()> {
    smithay_clipboard::file_drop_receiver()
}

pub fn start_file_drag(
    paths: Vec<PathBuf>,
    raw_display_handle: RawDisplayHandle,
    raw_window_handle: RawWindowHandle,
) -> Result<()> {
    if paths.is_empty() {
        return Err(BExplorerError::Shell("No files to drag".into()));
    }
    if paths.iter().any(|path| !path.exists()) {
        return Err(BExplorerError::Shell(
            "Only existing local files can be dragged to other applications".into(),
        ));
    }

    let native_result =
        wayland_drag::start_file_drag(paths.clone(), raw_display_handle, raw_window_handle);
    let native_error = match native_result {
        Ok(_) => return Ok(()),
        Err(error) => error,
    };
    crate::utils::log::info(format!("Wayland native drag failed: {native_error}"));

    if let Some(program) = custom_drag_helper_program() {
        let helper_name = program.to_string_lossy().to_string();
        crate::utils::log::info(format!(
            "Using explicit Linux drag helper from BEXPLORER_DRAG_HELPER: {helper_name}"
        ));
        return spawn_drag_helper(program, &[], paths, helper_name);
    }

    if automatic_drag_helper_fallback_enabled() {
        for helper in LINUX_DRAG_HELPERS {
            if command_exists(helper.program) {
                crate::utils::log::info(format!(
                    "Using automatic Linux drag helper fallback: {}",
                    helper.program
                ));
                return spawn_drag_helper(
                    OsString::from(helper.program),
                    helper.args,
                    paths,
                    helper.program.to_string(),
                );
            }
        }
    }

    Err(native_error)
}

pub fn poll_native_file_drag(
    raw_display_handle: RawDisplayHandle,
    raw_window_handle: RawWindowHandle,
) -> Result<bool> {
    wayland_drag::poll_active_file_drag(raw_display_handle, raw_window_handle)
}

pub fn network_computers() -> Vec<NetworkComputerInfo> {
    let mut computers = Vec::new();
    computers.extend(network_computers_from_gio());
    computers.extend(kio::network_computers());
    computers.extend(network_computers_from_avahi());
    computers.extend(network_computers_from_smbtree());
    dedupe_network_computers(computers)
}

pub fn network_neighbor_addresses() -> Vec<String> {
    if !command_exists("avahi-browse") {
        return Vec::new();
    }
    Command::new("avahi-browse")
        .args(["-rtp", "_smb._tcp"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter_map(|line| line.split(';').nth(7))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub fn network_computer_at(address: &str) -> Option<NetworkComputerInfo> {
    (!address.trim().is_empty()).then(|| NetworkComputerInfo {
        name: address.trim().to_string(),
        comment: "Network host".into(),
        kind: NetworkDeviceKind::Computer,
    })
}

pub fn network_shares(host: &str) -> Vec<NetworkShareInfo> {
    let mut shares = if command_exists("smbclient") {
        Command::new("smbclient")
            .args(["-g", "-N", "-L"])
            .arg(format!("//{host}"))
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| parse_smbclient_shares(&String::from_utf8_lossy(&output.stdout)))
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    shares.extend(kio::network_shares(host));
    dedupe_network_shares(shares)
}

pub fn mounted_network_path(path: &Path) -> Option<PathBuf> {
    let (host, share, children) = unc_parts(path)?;
    mounted_gvfs_network_path(host, share, &children)
        .or_else(|| kio::mounted_network_path(host, share, &children))
}

fn mounted_gvfs_network_path(host: &str, share: &str, children: &[&str]) -> Option<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from)?;
    let entries = fs::read_dir(runtime.join("gvfs")).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("smb-share:") {
            continue;
        }
        let mut mounted_host = None;
        let mut mounted_share = None;
        for field in name.trim_start_matches("smb-share:").split(',') {
            if let Some(value) = field.strip_prefix("server=") {
                mounted_host = Some(value);
            } else if let Some(value) = field.strip_prefix("share=") {
                mounted_share = Some(value);
            }
        }
        if mounted_host.is_some_and(|value| value.eq_ignore_ascii_case(host))
            && mounted_share.is_some_and(|value| value.eq_ignore_ascii_case(share))
        {
            return Some(
                children
                    .iter()
                    .fold(entry.path(), |path, child| path.join(child)),
            );
        }
    }
    None
}

fn unc_parts(path: &Path) -> Option<(&str, &str, Vec<&str>)> {
    let text = path.to_str()?.trim_start_matches(['\\', '/']);
    let mut parts = text.split(['\\', '/']).filter(|part| !part.is_empty());
    let host = parts.next()?;
    let share = parts.next()?;
    Some((host, share, parts.collect()))
}

fn desktop_icon_for_path(path: &Path, is_directory: bool, size: u32) -> Option<NativeIconImage> {
    let icon_names = icon_names_for_path(path, is_directory);
    let store = icon_theme_store();
    let icon_path = store.find_best_icon(&icon_names, size)?;
    load_icon_path(&icon_path, size)
}

fn network_computers_from_gio() -> Vec<NetworkComputerInfo> {
    if !command_exists("gio") {
        return Vec::new();
    }
    Command::new("gio")
        .args(["mount", "-li"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| parse_gio_network_hosts(&String::from_utf8_lossy(&output.stdout)))
        .unwrap_or_default()
}

fn network_computers_from_avahi() -> Vec<NetworkComputerInfo> {
    if !command_exists("avahi-browse") {
        return Vec::new();
    }
    Command::new("avahi-browse")
        .args(["-rtp", "_smb._tcp"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter_map(parse_avahi_smb_host)
                .collect()
        })
        .unwrap_or_default()
}

fn network_computers_from_smbtree() -> Vec<NetworkComputerInfo> {
    if !command_exists("smbtree") {
        return Vec::new();
    }
    Command::new("smbtree")
        .args(["-N", "-b"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| parse_smbtree_hosts(&String::from_utf8_lossy(&output.stdout)))
        .unwrap_or_default()
}

fn parse_gio_network_hosts(text: &str) -> Vec<NetworkComputerInfo> {
    text.lines()
        .filter_map(network_host_from_uri)
        .map(|host| NetworkComputerInfo {
            name: host,
            comment: "Mounted network location".into(),
            kind: NetworkDeviceKind::Computer,
        })
        .collect()
}

fn network_host_from_uri(line: &str) -> Option<String> {
    for scheme in ["smb://", "sftp://", "ftp://", "dav://", "davs://"] {
        if let Some(index) = line.to_ascii_lowercase().find(scheme) {
            let rest = &line[index + scheme.len()..];
            let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
            return host_from_authority(authority);
        }
    }
    None
}

fn host_from_authority(authority: &str) -> Option<String> {
    let authority = authority.rsplit('@').next()?.trim();
    let host = if let Some(bracketed) = authority.strip_prefix('[') {
        let end = bracketed.find(']')?;
        &bracketed[..end]
    } else if authority.matches(':').count() == 1 {
        let (candidate, port) = authority.rsplit_once(':')?;
        if port.parse::<u16>().is_ok() {
            candidate
        } else {
            authority
        }
    } else {
        authority
    };
    let host = percent_decode_component(host)?;
    (!host.trim().is_empty()).then(|| host.trim().to_string())
}

fn percent_decode_component(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = *bytes.get(index + 1)?;
            let low = *bytes.get(index + 2)?;
            decoded.push(hex_value(high)? * 16 + hex_value(low)?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn parse_avahi_smb_host(line: &str) -> Option<NetworkComputerInfo> {
    let parts = line.split(';').collect::<Vec<_>>();
    let name = parts
        .get(3)
        .or_else(|| parts.get(6))
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())?;
    Some(NetworkComputerInfo {
        name: name.trim_end_matches(".local").to_string(),
        comment: "Avahi SMB service".into(),
        kind: NetworkDeviceKind::Computer,
    })
}

fn parse_smbtree_hosts(text: &str) -> Vec<NetworkComputerInfo> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            let host = line.strip_prefix("\\\\")?;
            let name = host.split_whitespace().next()?.trim_matches('\\');
            (!name.is_empty()).then(|| NetworkComputerInfo {
                name: name.to_string(),
                comment: "SMB host".into(),
                kind: NetworkDeviceKind::Computer,
            })
        })
        .collect()
}

fn parse_smbclient_shares(text: &str) -> Vec<NetworkShareInfo> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split('|');
            let kind = parts.next()?;
            let name = parts.next()?.trim();
            let remark = parts.next().unwrap_or("").trim();
            (kind == "Disk" && !name.is_empty() && !name.ends_with('$')).then(|| NetworkShareInfo {
                name: name.to_string(),
                remark: remark.to_string(),
            })
        })
        .collect()
}

fn dedupe_network_computers(computers: Vec<NetworkComputerInfo>) -> Vec<NetworkComputerInfo> {
    let mut seen = HashSet::new();
    computers
        .into_iter()
        .filter(|computer| seen.insert(network_host_identity(&computer.name)))
        .collect()
}

fn network_host_identity(host: &str) -> String {
    let normalized = host.trim().trim_end_matches('.').to_ascii_lowercase();
    normalized
        .strip_suffix(".local")
        .unwrap_or(&normalized)
        .to_string()
}

fn dedupe_network_shares(shares: Vec<NetworkShareInfo>) -> Vec<NetworkShareInfo> {
    let mut seen = HashSet::new();
    shares
        .into_iter()
        .filter(|share| seen.insert(share.name.to_ascii_lowercase()))
        .collect()
}

fn command_path(program: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|directory| directory.join(program))
        .find(|candidate| candidate.is_file())
}

fn command_exists(program: &str) -> bool {
    command_path(program).is_some()
}

fn custom_drag_helper_program() -> Option<OsString> {
    std::env::var_os("BEXPLORER_DRAG_HELPER").filter(|program| !program.is_empty())
}

fn automatic_drag_helper_fallback_enabled() -> bool {
    std::env::var("BEXPLORER_DRAG_HELPER_FALLBACK")
        .ok()
        .is_some_and(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
}

fn spawn_drag_helper(
    program: OsString,
    args: &[&str],
    paths: Vec<PathBuf>,
    helper_name: String,
) -> Result<()> {
    Command::new(&program)
        .args(args)
        .args(&paths)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| {
            BExplorerError::Shell(format!(
                "Could not start Linux drag helper {helper_name}: {error}"
            ))
        })?;

    Ok(())
}

// Icon themes, MIME lookup and Freedesktop thumbnails form one cohesive backend.
include!("linux/icons.rs");

#[allow(non_camel_case_types)]
type XDisplay = std::ffi::c_void;
type XDrawable = std::ffi::c_ulong;
type XPixmap = std::ffi::c_ulong;
type XWindow = std::ffi::c_ulong;

#[link(name = "X11")]
unsafe extern "C" {
    fn XGetGeometry(
        display: *mut XDisplay,
        drawable: XDrawable,
        root_return: *mut XWindow,
        x_return: *mut std::ffi::c_int,
        y_return: *mut std::ffi::c_int,
        width_return: *mut std::ffi::c_uint,
        height_return: *mut std::ffi::c_uint,
        border_width_return: *mut std::ffi::c_uint,
        depth_return: *mut std::ffi::c_uint,
    ) -> std::ffi::c_int;

    fn XCreateBitmapFromData(
        display: *mut XDisplay,
        drawable: XDrawable,
        data: *const std::ffi::c_char,
        width: std::ffi::c_uint,
        height: std::ffi::c_uint,
    ) -> XPixmap;

    fn XFreePixmap(display: *mut XDisplay, pixmap: XPixmap) -> std::ffi::c_int;
    fn XFlush(display: *mut XDisplay) -> std::ffi::c_int;
}

#[link(name = "Xext")]
unsafe extern "C" {
    fn XShapeCombineMask(
        display: *mut XDisplay,
        destination: XWindow,
        destination_kind: std::ffi::c_int,
        x_offset: std::ffi::c_int,
        y_offset: std::ffi::c_int,
        source: XPixmap,
        operation: std::ffi::c_int,
    );
}

fn apply_x11_window_shape(display: *mut std::ffi::c_void, window: XWindow, radius: u32) {
    const SHAPE_BOUNDING: std::ffi::c_int = 0;
    const SHAPE_SET: std::ffi::c_int = 0;

    if display.is_null() || window == 0 {
        return;
    }

    let mut root = 0;
    let mut x = 0;
    let mut y = 0;
    let mut width = 0;
    let mut height = 0;
    let mut border_width = 0;
    let mut depth = 0;
    let ok = unsafe {
        XGetGeometry(
            display,
            window,
            &mut root,
            &mut x,
            &mut y,
            &mut width,
            &mut height,
            &mut border_width,
            &mut depth,
        )
    };
    if ok == 0 || width < 8 || height < 8 {
        return;
    }

    let mask = rounded_x11_bitmap(width, height, radius.max(4));
    let pixmap = unsafe {
        XCreateBitmapFromData(
            display,
            window,
            mask.as_ptr().cast::<std::ffi::c_char>(),
            width,
            height,
        )
    };
    if pixmap == 0 {
        return;
    }

    unsafe {
        XShapeCombineMask(display, window, SHAPE_BOUNDING, 0, 0, pixmap, SHAPE_SET);
        XFreePixmap(display, pixmap);
        XFlush(display);
    }
}

fn rounded_x11_bitmap(width: u32, height: u32, radius: u32) -> Vec<u8> {
    let row_bytes = width.div_ceil(8) as usize;
    let mut mask = vec![0_u8; row_bytes * height as usize];
    let radius = radius.min(width / 2).min(height / 2) as f32;
    let max_x = width as f32 - 1.0;
    let max_y = height as f32 - 1.0;

    for y in 0..height {
        for x in 0..width {
            if point_inside_rounded_rect(x as f32, y as f32, max_x, max_y, radius) {
                let offset = y as usize * row_bytes + x as usize / 8;
                mask[offset] |= 1 << (x % 8);
            }
        }
    }

    mask
}

fn point_inside_rounded_rect(x: f32, y: f32, max_x: f32, max_y: f32, radius: f32) -> bool {
    let left = x < radius;
    let right = x > max_x - radius;
    let top = y < radius;
    let bottom = y > max_y - radius;

    if !(left || right) || !(top || bottom) {
        return true;
    }

    let center_x = if left { radius } else { max_x - radius };
    let center_y = if top { radius } else { max_y - radius };
    let dx = x - center_x;
    let dy = y - center_y;
    dx * dx + dy * dy <= radius * radius
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnail_hash_matches_freedesktop_example() {
        assert_eq!(
            thumbnail_hash_for_uri("file:///home/jens/photos/me.png"),
            "c6ee772d9e49320e97ec29a7eb5b1697"
        );
    }

    #[test]
    fn parses_mime_glob_suffix() {
        let glob = parse_mime_glob("50:text/plain:*.txt").expect("glob");

        assert_eq!(glob.mime, "text/plain");
        assert_eq!(glob.suffix.as_deref(), Some(".txt"));
    }

    #[test]
    fn picks_more_specific_mime_glob() {
        let info = MimeInfo {
            globs: vec![
                parse_mime_glob("50:application/gzip:*.gz").expect("gz"),
                parse_mime_glob("50:application/x-compressed-tar:*.tar.gz").expect("tar.gz"),
            ],
            aliases: HashMap::new(),
            generic_icons: HashMap::new(),
        };

        assert_eq!(
            info.mime_for_path(Path::new("backup.tar.gz")).as_deref(),
            Some("application/x-compressed-tar")
        );
    }

    #[test]
    fn parses_png_text_chunk() {
        let mut png = Vec::from(b"\x89PNG\r\n\x1a\n".as_slice());
        append_png_chunk(&mut png, b"tEXt", b"Thumb::MTime\x00123");
        append_png_chunk(&mut png, b"IEND", b"");

        let chunks = png_text_chunks(&png);
        assert_eq!(chunks.get("Thumb::MTime").map(String::as_str), Some("123"));
    }

    #[test]
    fn extracts_network_host_from_gio_uri_line() {
        assert_eq!(
            network_host_from_uri("activation_root=smb://SERVER/Share"),
            Some("SERVER".into())
        );
        assert_eq!(
            network_host_from_uri("default_location=SFTP://alice@example.local:22/home/alice"),
            Some("example.local".into())
        );
    }

    #[test]
    fn parses_smbclient_disk_shares() {
        let shares = parse_smbclient_shares("Disk|Public|Shared files\nIPC|IPC$|IPC\n");

        assert_eq!(shares.len(), 1);
        assert_eq!(shares[0].name, "Public");
    }

    #[test]
    fn named_icon_loader_accepts_a_direct_png_path() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icons/appicon.png");
        let icon = native_named_icon(path.to_str().expect("UTF-8 app icon path"), 24)
            .expect("direct app icon");

        assert!(icon.width > 0 && icon.width <= 24);
        assert!(icon.height > 0 && icon.height <= 24);
        assert_eq!(icon.rgba.len(), icon.width * icon.height * 4);
    }

    fn append_png_chunk(png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        png.extend_from_slice(&(data.len() as u32).to_be_bytes());
        png.extend_from_slice(kind);
        png.extend_from_slice(data);
        png.extend_from_slice(&0_u32.to_be_bytes());
    }
}
