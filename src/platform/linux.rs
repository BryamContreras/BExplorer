//! Linux-specific platform hooks live here.
//!
//! Keep Linux integrations behind the neutral functions exported from
//! `crate::platform` so application and filesystem code stay portable.

#[cfg(target_os = "linux")]
mod gnome_blur;
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

pub fn cached_desktop_thumbnail(path: &Path) -> Option<NativeIconImage> {
    let uri = canonical_file_uri(path)?;
    let hash = thumbnail_hash_for_uri(&uri);
    let cache_home = xdg_cache_home();
    let metadata = fs::metadata(path).ok()?;

    for directory in ["large", "x-large", "xx-large", "normal"] {
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
        if let Some(image) = load_png_icon(&bytes, 256) {
            return Some(image);
        }
    }

    None
}

pub fn apply_window_corners(
    window_handle: &WindowHandle<'_>,
    display_handle: &DisplayHandle<'_>,
    radius: u32,
) -> Result<()> {
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
) -> Result<bool> {
    #[cfg(target_os = "linux")]
    {
        if gnome_blur::is_gnome_wayland() {
            return gnome_blur::set_application_blur(enabled);
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

    kwin_blur::set_window_blur(display_handle.as_raw(), window_handle.as_raw(), enabled)?;
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
    if !command_exists("smbclient") {
        return Vec::new();
    }
    Command::new("smbclient")
        .args(["-g", "-N", "-L"])
        .arg(format!("//{host}"))
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| parse_smbclient_shares(&String::from_utf8_lossy(&output.stdout)))
        .unwrap_or_default()
}

pub fn mounted_network_path(path: &Path) -> Option<PathBuf> {
    let (host, share, children) = unc_parts(path)?;
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from)?;
    let gvfs = runtime.join("gvfs");
    let entries = fs::read_dir(gvfs).ok()?;
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
        .filter_map(|line| network_host_from_uri(line))
        .map(|host| NetworkComputerInfo {
            name: host,
            comment: "Mounted network location".into(),
            kind: NetworkDeviceKind::Computer,
        })
        .collect()
}

fn network_host_from_uri(line: &str) -> Option<String> {
    for scheme in ["smb://", "sftp://", "ftp://", "dav://", "davs://"] {
        if let Some(index) = line.find(scheme) {
            let rest = &line[index + scheme.len()..];
            let host = rest.split(['/', ':', '?', '#']).next().unwrap_or("").trim();
            if !host.is_empty() {
                return Some(host.to_string());
            }
        }
    }
    None
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
        .filter(|computer| seen.insert(computer.name.to_ascii_lowercase()))
        .collect()
}

fn command_exists(program: &str) -> bool {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .any(|directory| directory.join(program).is_file())
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

fn icon_names_for_path(path: &Path, is_directory: bool) -> Vec<String> {
    if is_directory {
        if path == Path::new("/") {
            return names([
                "drive-harddisk",
                "drive-harddisk-symbolic",
                "folder-root",
                "folder",
            ]);
        }
        if path.starts_with("/media") || path.starts_with("/run/media") {
            return names([
                "drive-removable-media",
                "drive-removable-media-usb",
                "folder-removable",
                "folder",
            ]);
        }
        if path.starts_with("/mnt") {
            return names(["folder-remote", "network-server", "folder"]);
        }
        if let Some(icon_names) = user_directory_icon_names(path) {
            return icon_names;
        }
        return names(["folder"]);
    }

    let mime = mime_info()
        .mime_for_path(path)
        .unwrap_or_else(|| "application/octet-stream".into());
    let mut candidates = Vec::new();
    candidates.push(mime.replace('/', "-"));
    if let Some(generic) = mime_info().generic_icon_for_mime(&mime) {
        candidates.push(generic);
    }
    if let Some(generic) = fallback_generic_icon_for_mime(&mime) {
        candidates.push(generic);
    }
    candidates.push("text-x-generic".into());
    candidates.push("unknown".into());
    dedupe(candidates)
}

fn user_directory_icon_names(path: &Path) -> Option<Vec<String>> {
    let directories = UserDirs::new()?;
    let candidates: [(Option<&Path>, &[&str]); 9] = [
        (
            Some(directories.home_dir()),
            &["user-home", "folder-home", "folder"],
        ),
        (
            directories.desktop_dir(),
            &["user-desktop", "folder-desktop", "folder"],
        ),
        (
            directories.document_dir(),
            &["folder-documents", "folder-document", "folder"],
        ),
        (
            directories.download_dir(),
            &["folder-download", "folder-downloads", "folder"],
        ),
        (
            directories.audio_dir(),
            &["folder-music", "folder-audio", "folder"],
        ),
        (
            directories.picture_dir(),
            &["folder-pictures", "folder-images", "folder"],
        ),
        (
            directories.public_dir(),
            &["folder-publicshare", "folder-public", "folder"],
        ),
        (
            directories.template_dir(),
            &["folder-templates", "folder-template", "folder"],
        ),
        (
            directories.video_dir(),
            &["folder-videos", "folder-video", "folder"],
        ),
    ];
    candidates.into_iter().find_map(|(candidate, icon_names)| {
        candidate
            .filter(|candidate| *candidate == path)
            .map(|_| icon_names.iter().map(|name| (*name).to_owned()).collect())
    })
}

fn names(values: impl IntoIterator<Item = &'static str>) -> Vec<String> {
    values.into_iter().map(str::to_owned).collect()
}

fn dedupe(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

fn fallback_generic_icon_for_mime(mime: &str) -> Option<String> {
    let (top, _subtype) = mime.split_once('/')?;
    match top {
        "application" => Some("application-x-generic".into()),
        "audio" => Some("audio-x-generic".into()),
        "font" => Some("font-x-generic".into()),
        "image" => Some("image-x-generic".into()),
        "inode" => Some("inode-x-generic".into()),
        "message" => Some("message-x-generic".into()),
        "model" => Some("model-x-generic".into()),
        "multipart" => Some("multipart-x-generic".into()),
        "text" => Some("text-x-generic".into()),
        "video" => Some("video-x-generic".into()),
        _ => None,
    }
}

fn load_icon_path(path: &Path, size: u32) -> Option<NativeIconImage> {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => fs::read(path)
            .ok()
            .as_deref()
            .and_then(|bytes| load_png_icon(bytes, size)),
        Some("svg") => fs::read(path)
            .ok()
            .as_deref()
            .and_then(|bytes| load_svg_icon(bytes, size)),
        _ => None,
    }
}

fn load_png_icon(bytes: &[u8], size: u32) -> Option<NativeIconImage> {
    let image = image::load_from_memory(bytes).ok()?;
    let image = if image.width().max(image.height()) > size {
        image.thumbnail(size, size)
    } else {
        image
    }
    .to_rgba8();
    Some(NativeIconImage {
        width: image.width() as usize,
        height: image.height() as usize,
        rgba: image.into_raw(),
    })
}

fn load_svg_icon(bytes: &[u8], size: u32) -> Option<NativeIconImage> {
    let options = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(bytes, &options).ok()?;
    let base_size = tree.size().to_int_size();
    let max_edge = base_size.width().max(base_size.height()).max(1);
    let scale = (size as f32 / max_edge as f32).clamp(0.01, 8.0);
    let width = ((base_size.width() as f32 * scale).round() as u32).max(1);
    let height = ((base_size.height() as f32 * scale).round() as u32).max(1);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)?;
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let mut rgba = pixmap.data().to_vec();
    unpremultiply_rgba(&mut rgba);
    Some(NativeIconImage {
        rgba,
        width: width as usize,
        height: height as usize,
    })
}

fn unpremultiply_rgba(data: &mut [u8]) {
    for pixel in data.chunks_exact_mut(4) {
        let alpha = pixel[3] as u32;
        if alpha == 0 || alpha == 255 {
            continue;
        }
        pixel[0] = ((pixel[0] as u32 * 255) / alpha).min(255) as u8;
        pixel[1] = ((pixel[1] as u32 * 255) / alpha).min(255) as u8;
        pixel[2] = ((pixel[2] as u32 * 255) / alpha).min(255) as u8;
    }
}

impl IconThemeStore {
    fn find_best_icon(&self, names: &[String], size: u32) -> Option<PathBuf> {
        let mut visited = HashSet::new();
        self.find_best_icon_in_theme(&self.current_theme, names, size, &mut visited)
            .or_else(|| {
                let mut visited = HashSet::new();
                self.find_best_icon_in_theme("hicolor", names, size, &mut visited)
            })
            .or_else(|| self.lookup_fallback_icon(names))
    }

    fn find_best_icon_in_theme(
        &self,
        theme_name: &str,
        names: &[String],
        size: u32,
        visited: &mut HashSet<String>,
    ) -> Option<PathBuf> {
        if !visited.insert(theme_name.to_string()) {
            return None;
        }
        let theme = self.load_theme(theme_name)?;
        if let Some(path) = self.lookup_icon_in_theme(theme_name, &theme, names, size) {
            return Some(path);
        }
        for parent in &theme.inherits {
            if let Some(path) = self.find_best_icon_in_theme(parent, names, size, visited) {
                return Some(path);
            }
        }
        if theme_name != "hicolor" {
            self.find_best_icon_in_theme("hicolor", names, size, visited)
        } else {
            None
        }
    }

    fn lookup_icon_in_theme(
        &self,
        theme_name: &str,
        theme: &IconTheme,
        names: &[String],
        size: u32,
    ) -> Option<PathBuf> {
        for name in names {
            for directory in &theme.directories {
                if directory.matches_size(size) {
                    if let Some(path) = self.icon_file_in_directory(theme_name, directory, name) {
                        return Some(path);
                    }
                }
            }
        }

        let mut best: Option<(u32, PathBuf)> = None;
        for name in names {
            for directory in &theme.directories {
                if let Some(path) = self.icon_file_in_directory(theme_name, directory, name) {
                    let distance = directory.size_distance(size);
                    if best
                        .as_ref()
                        .is_none_or(|(best_distance, _)| distance < *best_distance)
                    {
                        best = Some((distance, path));
                    }
                }
            }
        }
        best.map(|(_, path)| path)
    }

    fn icon_file_in_directory(
        &self,
        theme_name: &str,
        directory: &IconThemeDirectory,
        icon_name: &str,
    ) -> Option<PathBuf> {
        for base_dir in &self.themed_base_dirs {
            for extension in ICON_EXTENSIONS {
                let path = base_dir
                    .join(theme_name)
                    .join(&directory.name)
                    .join(format!("{icon_name}.{extension}"));
                if path.is_file() {
                    return Some(path);
                }
            }
        }
        None
    }

    fn lookup_fallback_icon(&self, names: &[String]) -> Option<PathBuf> {
        for name in names {
            for directory in self.themed_base_dirs.iter().chain(&self.fallback_dirs) {
                for extension in ICON_EXTENSIONS {
                    let path = directory.join(format!("{name}.{extension}"));
                    if path.is_file() {
                        return Some(path);
                    }
                }
            }
        }
        None
    }

    fn load_theme(&self, theme_name: &str) -> Option<IconTheme> {
        let index = self
            .themed_base_dirs
            .iter()
            .map(|base| base.join(theme_name).join("index.theme"))
            .find(|path| path.is_file())?;
        let content = fs::read_to_string(index).ok()?;
        parse_icon_theme(&content)
    }
}

impl IconThemeDirectory {
    fn matches_size(&self, size: u32) -> bool {
        match self.kind {
            IconThemeDirectoryKind::Fixed => self.size.saturating_mul(self.scale) == size,
            IconThemeDirectoryKind::Scalable => {
                let scaled_size = size.saturating_mul(self.scale);
                self.min_size <= scaled_size && scaled_size <= self.max_size
            }
            IconThemeDirectoryKind::Threshold => {
                let scaled_size = size.saturating_mul(self.scale);
                self.size.saturating_sub(self.threshold) <= scaled_size
                    && scaled_size <= self.size.saturating_add(self.threshold)
            }
        }
    }

    fn size_distance(&self, size: u32) -> u32 {
        let target = size.saturating_mul(self.scale);
        match self.kind {
            IconThemeDirectoryKind::Fixed => self.size.abs_diff(target),
            IconThemeDirectoryKind::Scalable => {
                if target < self.min_size {
                    self.min_size - target
                } else if target > self.max_size {
                    target - self.max_size
                } else {
                    0
                }
            }
            IconThemeDirectoryKind::Threshold => {
                let min = self.size.saturating_sub(self.threshold);
                let max = self.size.saturating_add(self.threshold);
                if target < min {
                    min - target
                } else if target > max {
                    target - max
                } else {
                    0
                }
            }
        }
    }
}

fn parse_icon_theme(content: &str) -> Option<IconTheme> {
    let sections = parse_ini_sections(content);
    let root = sections.get("Icon Theme")?;
    let mut inherits = csv_values(root.get("Inherits").map(String::as_str).unwrap_or(""));
    if inherits.is_empty() {
        inherits.push("hicolor".into());
    }
    let directories = csv_values(root.get("Directories").map(String::as_str).unwrap_or(""))
        .into_iter()
        .chain(csv_values(
            root.get("ScaledDirectories")
                .map(String::as_str)
                .unwrap_or(""),
        ))
        .filter_map(|name| {
            let section = sections.get(&name)?;
            Some(IconThemeDirectory {
                name,
                size: parse_u32(section.get("Size")).unwrap_or(48),
                scale: parse_u32(section.get("Scale")).unwrap_or(1),
                min_size: parse_u32(section.get("MinSize"))
                    .or_else(|| parse_u32(section.get("Size")))
                    .unwrap_or(48),
                max_size: parse_u32(section.get("MaxSize"))
                    .or_else(|| parse_u32(section.get("Size")))
                    .unwrap_or(48),
                threshold: parse_u32(section.get("Threshold")).unwrap_or(2),
                kind: match section.get("Type").map(String::as_str) {
                    Some("Fixed") => IconThemeDirectoryKind::Fixed,
                    Some("Scalable") => IconThemeDirectoryKind::Scalable,
                    _ => IconThemeDirectoryKind::Threshold,
                },
            })
        })
        .collect();

    Some(IconTheme {
        inherits: dedupe(inherits),
        directories,
    })
}

fn parse_ini_sections(content: &str) -> HashMap<String, HashMap<String, String>> {
    let mut sections: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut current = String::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            current = line[1..line.len() - 1].trim().to_string();
            sections.entry(current.clone()).or_default();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        sections
            .entry(current.clone())
            .or_default()
            .insert(key.trim().to_string(), value.trim().to_string());
    }
    sections
}

fn csv_values(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn parse_u32(value: Option<&String>) -> Option<u32> {
    value?.trim().parse().ok()
}

impl MimeInfo {
    fn mime_for_path(&self, path: &Path) -> Option<String> {
        let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
        let mut best: Option<(i32, usize, &str)> = None;
        for glob in &self.globs {
            let specificity = glob.specificity();
            let matches = glob
                .literal
                .as_ref()
                .is_some_and(|literal| literal == &name)
                || glob
                    .suffix
                    .as_ref()
                    .is_some_and(|suffix| name.ends_with(suffix))
                || glob_matches(&glob.pattern, &name);
            if matches
                && best.as_ref().is_none_or(|(priority, length, _)| {
                    glob.priority > *priority
                        || (glob.priority == *priority && specificity > *length)
                })
            {
                best = Some((glob.priority, specificity, &glob.mime));
            }
        }

        best.map(|(_, _, mime)| self.resolve_alias(mime))
    }

    fn generic_icon_for_mime(&self, mime: &str) -> Option<String> {
        self.generic_icons
            .get(mime)
            .cloned()
            .or_else(|| self.generic_icons.get(&self.resolve_alias(mime)).cloned())
    }

    fn resolve_alias(&self, mime: &str) -> String {
        self.aliases
            .get(mime)
            .cloned()
            .unwrap_or_else(|| mime.to_string())
    }
}

impl MimeGlob {
    fn specificity(&self) -> usize {
        self.literal
            .as_ref()
            .map(|value| value.len())
            .or_else(|| self.suffix.as_ref().map(|value| value.len()))
            .unwrap_or_else(|| self.pattern.len())
    }
}

fn glob_matches(pattern: &str, name: &str) -> bool {
    if !pattern.contains('*') && !pattern.contains('?') {
        return pattern.eq_ignore_ascii_case(name);
    }
    if let Some(suffix) = pattern.strip_prefix('*') {
        return name.ends_with(&suffix.to_ascii_lowercase());
    }
    false
}

fn parse_mime_glob(line: &str) -> Option<MimeGlob> {
    let mut parts = line.splitn(4, ':');
    let priority = parts.next()?.trim().parse().ok()?;
    let mime = parts.next()?.trim().to_string();
    let pattern = parts.next()?.trim().to_ascii_lowercase();
    if mime.is_empty() || pattern.is_empty() || pattern == "__NOGLOBS__" {
        return None;
    }
    let suffix = pattern
        .strip_prefix("*.")
        .filter(|suffix| !suffix.contains(['*', '?', '[']))
        .map(|suffix| format!(".{suffix}"));
    let literal = (!pattern.contains(['*', '?', '['])).then_some(pattern.clone());
    Some(MimeGlob {
        priority,
        mime,
        pattern,
        suffix,
        literal,
    })
}

fn load_mime_info() -> MimeInfo {
    let mut globs = Vec::new();
    let mut aliases = HashMap::new();
    let mut generic_icons = HashMap::new();

    for base in xdg_data_dirs_for_mime() {
        let mime_dir = base.join("mime");
        if let Ok(content) = fs::read_to_string(mime_dir.join("globs2")) {
            globs.extend(
                content
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty() && !line.starts_with('#'))
                    .filter_map(parse_mime_glob),
            );
        }
        if let Ok(content) = fs::read_to_string(mime_dir.join("aliases")) {
            for line in content.lines().map(str::trim) {
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let mut parts = line.split_whitespace();
                if let (Some(alias), Some(canonical)) = (parts.next(), parts.next()) {
                    aliases.insert(alias.to_string(), canonical.to_string());
                }
            }
        }
        if let Ok(content) = fs::read_to_string(mime_dir.join("generic-icons")) {
            for line in content.lines().map(str::trim) {
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((mime, icon)) = line.split_once(':') {
                    generic_icons.insert(mime.trim().to_string(), icon.trim().to_string());
                }
            }
        }
    }

    MimeInfo {
        globs,
        aliases,
        generic_icons,
    }
}

fn icon_theme_store() -> &'static IconThemeStore {
    static STORE: OnceLock<IconThemeStore> = OnceLock::new();
    STORE.get_or_init(|| {
        let themed_base_dirs = themed_icon_base_dirs();
        let current_theme = configured_icon_theme()
            .or_else(|| first_existing_theme(&themed_base_dirs, FALLBACK_THEMES))
            .unwrap_or_else(|| "hicolor".into());
        IconThemeStore {
            themed_base_dirs,
            fallback_dirs: vec![PathBuf::from("/usr/share/pixmaps")],
            current_theme,
        }
    })
}

fn mime_info() -> &'static MimeInfo {
    static MIME_INFO: OnceLock<MimeInfo> = OnceLock::new();
    MIME_INFO.get_or_init(load_mime_info)
}

fn themed_icon_base_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = home_dir() {
        dirs.push(home.join(".icons"));
    }
    dirs.push(xdg_data_home().join("icons"));
    dirs.extend(
        xdg_data_dirs_for_mime()
            .into_iter()
            .map(|dir| dir.join("icons")),
    );
    dedupe_paths(dirs)
}

fn xdg_data_dirs_for_mime() -> Vec<PathBuf> {
    let mut dirs = vec![xdg_data_home()];
    let data_dirs = std::env::var_os("XDG_DATA_DIRS")
        .map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
        .unwrap_or_else(|| {
            vec![
                PathBuf::from("/usr/local/share"),
                PathBuf::from("/usr/share"),
            ]
        });
    dirs.extend(data_dirs);
    dedupe_paths(dirs)
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

fn configured_icon_theme() -> Option<String> {
    std::env::var("BEXPLORER_ICON_THEME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| gtk_icon_theme_from_settings("gtk-4.0/settings.ini"))
        .or_else(|| gtk_icon_theme_from_settings("gtk-3.0/settings.ini"))
        .or_else(kde_icon_theme_from_settings)
}

fn gtk_icon_theme_from_settings(relative_path: &str) -> Option<String> {
    let settings = fs::read_to_string(xdg_config_home().join(relative_path)).ok()?;
    parse_ini_sections(&settings)
        .get("Settings")
        .and_then(|settings| settings.get("gtk-icon-theme-name"))
        .cloned()
        .filter(|value| !value.trim().is_empty())
}

fn kde_icon_theme_from_settings() -> Option<String> {
    let settings = fs::read_to_string(xdg_config_home().join("kdeglobals")).ok()?;
    parse_ini_sections(&settings)
        .get("Icons")
        .and_then(|settings| settings.get("Theme"))
        .cloned()
        .filter(|value| !value.trim().is_empty())
}

fn first_existing_theme(base_dirs: &[PathBuf], names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        base_dirs
            .iter()
            .any(|base| base.join(name).join("index.theme").is_file())
            .then(|| (*name).to_string())
    })
}

fn xdg_config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn xdg_data_home() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("/usr/local/share"))
}

fn xdg_cache_home() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join(".cache")))
        .unwrap_or_else(|| PathBuf::from(".cache"))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn canonical_file_uri(path: &Path) -> Option<String> {
    let path = fs::canonicalize(path).ok().or_else(|| {
        path.is_absolute()
            .then(|| path.to_path_buf())
            .or_else(|| std::env::current_dir().ok().map(|dir| dir.join(path)))
    })?;
    let mut uri = String::from("file://");
    for byte in path.as_os_str().as_bytes() {
        match *byte {
            b'/' => uri.push('/'),
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                uri.push(*byte as char)
            }
            value => uri.push_str(&format!("%{value:02X}")),
        }
    }
    Some(uri)
}

fn thumbnail_hash_for_uri(uri: &str) -> String {
    format!("{:x}", md5::compute(uri.as_bytes()))
}

fn thumbnail_metadata_is_current(
    thumbnail_bytes: &[u8],
    original_metadata: &fs::Metadata,
    original_uri: &str,
) -> bool {
    let text = png_text_chunks(thumbnail_bytes);
    if text.get("Thumb::URI").map(String::as_str) != Some(original_uri) {
        return false;
    }
    let Some(mtime) = original_metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs().to_string())
    else {
        return false;
    };
    if text.get("Thumb::MTime") != Some(&mtime) {
        return false;
    }
    if let Some(size) = text.get("Thumb::Size") {
        if size.parse::<u64>().ok() != Some(original_metadata.len()) {
            return false;
        }
    }
    true
}

fn png_text_chunks(bytes: &[u8]) -> HashMap<String, String> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    let mut output = HashMap::new();
    if bytes.len() < 8 || &bytes[..8] != PNG_SIGNATURE {
        return output;
    }

    let mut index = 8;
    while index + 8 <= bytes.len() {
        let length = u32::from_be_bytes([
            bytes[index],
            bytes[index + 1],
            bytes[index + 2],
            bytes[index + 3],
        ]) as usize;
        let chunk_type = &bytes[index + 4..index + 8];
        let data_start = index + 8;
        let data_end = data_start.saturating_add(length);
        if data_end + 4 > bytes.len() {
            break;
        }
        let data = &bytes[data_start..data_end];
        if chunk_type == b"tEXt" {
            if let Some((key, value)) = split_png_text(data) {
                output.insert(key, value);
            }
        } else if chunk_type == b"iTXt" {
            if let Some((key, value)) = split_png_itxt(data) {
                output.insert(key, value);
            }
        } else if chunk_type == b"IEND" {
            break;
        }
        index = data_end + 4;
    }
    output
}

fn split_png_text(data: &[u8]) -> Option<(String, String)> {
    let split = data.iter().position(|byte| *byte == 0)?;
    let key = String::from_utf8_lossy(&data[..split]).to_string();
    let value = String::from_utf8_lossy(&data[split + 1..]).to_string();
    Some((key, value))
}

fn split_png_itxt(data: &[u8]) -> Option<(String, String)> {
    let key_end = data.iter().position(|byte| *byte == 0)?;
    let key = String::from_utf8_lossy(&data[..key_end]).to_string();
    let compression_flag = *data.get(key_end + 1)?;
    let _compression_method = *data.get(key_end + 2)?;
    if compression_flag != 0 {
        return None;
    }
    let mut rest = data.get(key_end + 3..)?;
    let language_end = rest.iter().position(|byte| *byte == 0)?;
    rest = &rest[language_end + 1..];
    let translated_end = rest.iter().position(|byte| *byte == 0)?;
    let value = String::from_utf8_lossy(&rest[translated_end + 1..]).to_string();
    Some((key, value))
}

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
    }

    #[test]
    fn parses_smbclient_disk_shares() {
        let shares = parse_smbclient_shares("Disk|Public|Shared files\nIPC|IPC$|IPC\n");

        assert_eq!(shares.len(), 1);
        assert_eq!(shares[0].name, "Public");
    }

    fn append_png_chunk(png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        png.extend_from_slice(&(data.len() as u32).to_be_bytes());
        png.extend_from_slice(kind);
        png.extend_from_slice(data);
        png.extend_from_slice(&0_u32.to_be_bytes());
    }
}
