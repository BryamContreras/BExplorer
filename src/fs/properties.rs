//! Linux-native file properties backend.
//!
//! This module deliberately contains no UI state. Metadata collection, size
//! calculation and mutations can therefore run on background workers while
//! Iced remains responsive.

#![cfg(target_os = "linux")]

use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::utils::errors::{BExplorerError, Result};

const ELEVATED_HELPER_ARG: &str = "--bexplorer-elevated-properties-helper";
const MAX_ELEVATED_REQUEST_BYTES: usize = 16 * 1024 * 1024;
const VALID_PERMISSION_BITS: u32 = 0o7777;
const SIZE_PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
const SIZE_PROGRESS_ITEMS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropertyKind {
    File,
    Directory,
    SymlinkFile,
    SymlinkDirectory,
    BrokenSymlink,
    Other,
    Multiple,
}

impl PropertyKind {
    pub fn is_directory(self) -> bool {
        matches!(self, Self::Directory | Self::SymlinkDirectory)
    }

    fn is_real_directory(self) -> bool {
        self == Self::Directory
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertySymlink {
    pub raw_target: PathBuf,
    pub resolved_target: PathBuf,
    pub broken: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertyMount {
    pub mount_point: PathBuf,
    pub source: String,
    pub file_system: String,
    pub read_only: bool,
    pub total: Option<u64>,
    pub free: Option<u64>,
    pub available: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertyApplication {
    pub desktop_id: String,
    pub name: String,
    pub icon: Option<String>,
    pub desktop_file: PathBuf,
}

impl fmt::Display for PropertyApplication {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertyIdentity {
    pub id: u32,
    pub name: String,
}

impl fmt::Display for PropertyIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertyIcon {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Loads an application's desktop-entry icon through the active Linux theme.
/// A generic application icon keeps the selector useful when an entry omits
/// its icon or references one unavailable in the current theme.
#[cfg(target_os = "linux")]
pub fn load_application_icon(application: &PropertyApplication, size: u32) -> Option<PropertyIcon> {
    application
        .icon
        .as_deref()
        .and_then(|name| crate::platform::native_named_icon(name, size))
        .or_else(|| crate::platform::native_named_icon("application-x-executable", size))
        .or_else(|| crate::platform::native_named_icon("application-x-generic", size))
        .map(|icon| PropertyIcon {
            rgba: icon.rgba,
            width: icon.width as u32,
            height: icon.height as u32,
        })
}

#[derive(Clone, Debug)]
pub struct PropertyItem {
    pub path: PathBuf,
    pub display_name: String,
    pub kind: PropertyKind,
    pub location: Option<PathBuf>,
    pub mime_type: String,
    pub logical_size: u64,
    pub allocated_size: u64,
    pub accessed: Option<SystemTime>,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
    pub changed: Option<SystemTime>,
    pub mode: u32,
    pub owner: String,
    pub group: String,
    pub uid: u32,
    pub gid: u32,
    pub inode: u64,
    pub device: u64,
    pub hard_links: u64,
    pub symlink: Option<PropertySymlink>,
    pub mount: Option<PropertyMount>,
}

#[derive(Clone, Debug)]
pub struct PropertiesSnapshot {
    pub paths: Vec<PathBuf>,
    pub display_name: String,
    pub kind: PropertyKind,
    pub location: Option<PathBuf>,
    pub mime_type: Option<String>,
    pub default_application: Option<PropertyApplication>,
    pub applications: Vec<PropertyApplication>,
    pub logical_size: u64,
    pub allocated_size: u64,
    pub accessed: Option<SystemTime>,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
    pub changed: Option<SystemTime>,
    pub mode: Option<u32>,
    pub owner: Option<String>,
    pub group: Option<String>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub inode: Option<u64>,
    pub device: Option<u64>,
    pub hard_links: Option<u64>,
    pub symlink: Option<PropertySymlink>,
    pub mount: Option<PropertyMount>,
    pub contains_directory: bool,
    pub icon: Option<PropertyIcon>,
    #[allow(dead_code)]
    pub items: Vec<PropertyItem>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DirectorySize {
    pub bytes: u64,
    pub allocated: u64,
    pub files: u64,
    pub directories: u64,
    pub links: u64,
    pub unreadable: u64,
    pub cancelled: bool,
}

#[derive(Clone, Debug, Default)]
pub struct PropertiesChanges {
    pub paths: Vec<PathBuf>,
    pub new_name: Option<String>,
    /// Permission bits to replace. A zero mask leaves all modes unchanged.
    pub permission_mask: u32,
    /// Values for the bits selected by `permission_mask`.
    pub permission_value: u32,
    pub owner: Option<u32>,
    pub group: Option<u32>,
    pub recursive: bool,
    pub mime_type: Option<String>,
    /// Freedesktop desktop-file id, for example `org.gnome.TextEditor.desktop`.
    pub default_application: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplyOutcome {
    pub paths: Vec<PathBuf>,
    pub renamed: Option<PathBuf>,
    pub permission_entries: usize,
    pub association_changed: bool,
    pub elevated: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct PermissionRequest {
    paths: Vec<PathBuf>,
    permission_mask: u32,
    permission_value: u32,
    owner: Option<u32>,
    group: Option<u32>,
    recursive: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct PermissionResult {
    entries: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct RenameRequest {
    paths: Vec<PathBuf>,
    new_name: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
enum ElevatedPropertiesRequest {
    Permissions(PermissionRequest),
    Rename(RenameRequest),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
enum ElevatedPropertiesResponse {
    Permissions(PermissionResult),
    Rename(PathBuf),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PermissionApplyMode {
    Unprivileged,
    ElevatedSecure,
}

#[derive(Clone, Debug)]
struct MountInfoEntry {
    major: u32,
    minor: u32,
    mount_point: PathBuf,
    options: Vec<String>,
    file_system: String,
    source: String,
    super_options: Vec<String>,
}

#[derive(Clone, Debug)]
struct DesktopEntry {
    application: PropertyApplication,
    mime_types: Vec<String>,
    hidden: bool,
    no_display: bool,
    only_show_in: Vec<String>,
    not_show_in: Vec<String>,
    try_exec: Option<String>,
}

/// Collects a coherent properties snapshot for one or more local paths.
pub fn load(paths: &[PathBuf]) -> Result<PropertiesSnapshot> {
    if paths.is_empty() {
        return Err(BExplorerError::Operation(
            "At least one path is required for properties".into(),
        ));
    }

    let mounts = load_mountinfo();
    let mut owner_names = HashMap::new();
    let mut group_names = HashMap::new();
    let mut items = Vec::with_capacity(paths.len());
    for path in paths {
        items.push(load_item(
            path,
            &mounts,
            &mut owner_names,
            &mut group_names,
        )?);
    }

    let paths = items
        .iter()
        .map(|item| item.path.clone())
        .collect::<Vec<_>>();
    let one = items.len() == 1;
    let display_name = if one {
        items[0].display_name.clone()
    } else {
        format!("{} items", items.len())
    };
    let kind = if one {
        items[0].kind
    } else {
        PropertyKind::Multiple
    };
    let location = common_value(items.iter().map(|item| item.location.clone())).flatten();
    let mime_type = common_value(items.iter().map(|item| item.mime_type.clone()));
    let locale = desktop_locale();
    let (applications, default_application) = mime_type
        .as_deref()
        .map(|mime| applications_for_mime(mime, &locale))
        .unwrap_or_default();
    let logical_size = items
        .iter()
        .fold(0_u64, |total, item| total.saturating_add(item.logical_size));
    let allocated_size = items.iter().fold(0_u64, |total, item| {
        total.saturating_add(item.allocated_size)
    });
    let accessed = common_value(items.iter().map(|item| item.accessed)).flatten();
    let modified = common_value(items.iter().map(|item| item.modified)).flatten();
    let created = common_value(items.iter().map(|item| item.created)).flatten();
    let changed = common_value(items.iter().map(|item| item.changed)).flatten();
    let mode = common_value(items.iter().map(|item| item.mode));
    let owner = common_value(items.iter().map(|item| item.owner.clone()));
    let group = common_value(items.iter().map(|item| item.group.clone()));
    let uid = common_value(items.iter().map(|item| item.uid));
    let gid = common_value(items.iter().map(|item| item.gid));
    let mount = common_value(items.iter().map(|item| item.mount.clone())).flatten();
    let contains_directory = items.iter().any(|item| item.kind.is_real_directory());
    let symlink = one.then(|| items[0].symlink.clone()).flatten();
    let inode = one.then_some(items[0].inode);
    let device = one.then_some(items[0].device);
    let hard_links = one.then_some(items[0].hard_links);
    let icon = one
        .then(|| {
            crate::platform::native_file_icon_highres(&items[0].path, items[0].kind.is_directory())
        })
        .flatten()
        .map(|icon| PropertyIcon {
            rgba: icon.rgba,
            width: icon.width as u32,
            height: icon.height as u32,
        });

    Ok(PropertiesSnapshot {
        paths,
        display_name,
        kind,
        location,
        mime_type,
        default_application,
        applications,
        logical_size,
        allocated_size,
        accessed,
        modified,
        created,
        changed,
        mode,
        owner,
        group,
        uid,
        gid,
        inode,
        device,
        hard_links,
        symlink,
        mount,
        contains_directory,
        icon,
        items,
    })
}

fn load_item(
    path: &Path,
    mounts: &[MountInfoEntry],
    owner_names: &mut HashMap<u32, String>,
    group_names: &mut HashMap<u32, String>,
) -> Result<PropertyItem> {
    let metadata = fs::symlink_metadata(path)?;
    let file_type = metadata.file_type();
    let (kind, symlink) = if file_type.is_symlink() {
        let raw_target = fs::read_link(path)?;
        let resolved_target = if raw_target.is_absolute() {
            raw_target.clone()
        } else {
            path.parent()
                .unwrap_or_else(|| Path::new("/"))
                .join(&raw_target)
        };
        let target_metadata = fs::metadata(path);
        let kind = match target_metadata.as_ref() {
            Ok(target) if target.is_dir() => PropertyKind::SymlinkDirectory,
            Ok(target) if target.is_file() => PropertyKind::SymlinkFile,
            Ok(_) => PropertyKind::Other,
            Err(_) => PropertyKind::BrokenSymlink,
        };
        (
            kind,
            Some(PropertySymlink {
                raw_target,
                resolved_target,
                broken: target_metadata.is_err(),
            }),
        )
    } else if metadata.is_dir() {
        (PropertyKind::Directory, None)
    } else if metadata.is_file() {
        (PropertyKind::File, None)
    } else {
        (PropertyKind::Other, None)
    };

    let uid = metadata.uid();
    let gid = metadata.gid();
    let owner = owner_names
        .entry(uid)
        .or_insert_with(|| identity_name("passwd", uid).unwrap_or_else(|| uid.to_string()))
        .clone();
    let group = group_names
        .entry(gid)
        .or_insert_with(|| identity_name("group", gid).unwrap_or_else(|| gid.to_string()))
        .clone();
    let mime_type = mime_type_for_path(path, kind);
    // Properties describe the directory entry itself. A symlink may point to
    // another filesystem, but its filesystem card must identify the mount on
    // which the link lives; the resolved target remains separate metadata.
    let mount = property_mount_for_path(path, mounts);

    Ok(PropertyItem {
        path: path.to_path_buf(),
        display_name: path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned(),
        kind,
        location: path.parent().map(Path::to_path_buf),
        mime_type,
        logical_size: metadata.len(),
        allocated_size: metadata.blocks().saturating_mul(512),
        accessed: metadata.accessed().ok(),
        modified: metadata.modified().ok(),
        created: metadata.created().ok(),
        changed: unix_system_time(metadata.ctime(), metadata.ctime_nsec()),
        mode: metadata.mode() & VALID_PERMISSION_BITS,
        owner,
        group,
        uid,
        gid,
        inode: metadata.ino(),
        device: metadata.dev(),
        hard_links: metadata.nlink(),
        symlink,
        mount,
    })
}

fn common_value<T: Clone + PartialEq>(mut values: impl Iterator<Item = T>) -> Option<T> {
    let first = values.next()?;
    values.all(|value| value == first).then_some(first)
}

fn unix_system_time(seconds: i64, nanoseconds: i64) -> Option<SystemTime> {
    if !(0..1_000_000_000).contains(&nanoseconds) {
        return None;
    }
    let duration = Duration::new(seconds.unsigned_abs(), nanoseconds as u32);
    if seconds >= 0 {
        UNIX_EPOCH.checked_add(duration)
    } else if nanoseconds == 0 {
        UNIX_EPOCH.checked_sub(duration)
    } else {
        // A negative timespec is `seconds + nanoseconds`, not the negative of
        // both components.
        UNIX_EPOCH.checked_sub(Duration::new(
            seconds.unsigned_abs().saturating_sub(1),
            1_000_000_000_u32 - nanoseconds as u32,
        ))
    }
}

fn load_mountinfo() -> Vec<MountInfoEntry> {
    fs::read_to_string("/proc/self/mountinfo")
        .ok()
        .map(|text| text.lines().filter_map(parse_mountinfo_line).collect())
        .unwrap_or_default()
}

fn parse_mountinfo_line(line: &str) -> Option<MountInfoEntry> {
    let (before, after) = line.split_once(" - ")?;
    let before = before.split_whitespace().collect::<Vec<_>>();
    let after = after.split_whitespace().collect::<Vec<_>>();
    if before.len() < 6 || after.len() < 3 {
        return None;
    }
    let (major, minor) = before[2].split_once(':')?;
    Some(MountInfoEntry {
        major: major.parse().ok()?,
        minor: minor.parse().ok()?,
        mount_point: PathBuf::from(decode_mountinfo_field(before[4])),
        options: before[5].split(',').map(str::to_owned).collect(),
        file_system: decode_mountinfo_field(after[0]),
        source: decode_mountinfo_field(after[1]),
        super_options: after[2].split(',').map(str::to_owned).collect(),
    })
}

fn decode_mountinfo_field(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 3 < bytes.len() {
            let digits = &bytes[index + 1..index + 4];
            if digits.iter().all(|digit| matches!(digit, b'0'..=b'7')) {
                decoded.push((digits[0] - b'0') * 64 + (digits[1] - b'0') * 8 + digits[2] - b'0');
                index += 4;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn property_mount_for_path(path: &Path, mounts: &[MountInfoEntry]) -> Option<PropertyMount> {
    let metadata = fs::symlink_metadata(path).ok()?;
    let is_symlink = metadata.file_type().is_symlink();
    let device = metadata.dev();
    let major = rustix::fs::major(device);
    let minor = rustix::fs::minor(device);
    let comparable = if is_symlink {
        path.parent()
            .and_then(|parent| parent.canonicalize().ok())
            .and_then(|parent| path.file_name().map(|name| parent.join(name)))
            .unwrap_or_else(|| path.to_path_buf())
    } else {
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    };
    let entry = mounts
        .iter()
        .filter(|entry| entry.major == major && entry.minor == minor)
        .filter(|entry| {
            comparable == entry.mount_point || comparable.starts_with(&entry.mount_point)
        })
        .max_by_key(|entry| entry.mount_point.as_os_str().len())?;
    let capacity_path = if is_symlink {
        path.parent().unwrap_or_else(|| Path::new("/"))
    } else {
        path
    };
    let capacity = rustix::fs::statvfs(capacity_path).ok();
    let block_size = capacity.as_ref().map(|stats| {
        if stats.f_frsize == 0 {
            stats.f_bsize
        } else {
            stats.f_frsize
        }
    });
    let blocks_to_bytes = |blocks: u64| block_size.map(|size| blocks.saturating_mul(size));
    Some(PropertyMount {
        mount_point: entry.mount_point.clone(),
        source: entry.source.clone(),
        file_system: entry.file_system.clone(),
        read_only: entry.options.iter().any(|option| option == "ro")
            || entry.super_options.iter().any(|option| option == "ro"),
        total: capacity
            .as_ref()
            .and_then(|stats| blocks_to_bytes(stats.f_blocks)),
        free: capacity
            .as_ref()
            .and_then(|stats| blocks_to_bytes(stats.f_bfree)),
        available: capacity
            .as_ref()
            .and_then(|stats| blocks_to_bytes(stats.f_bavail)),
    })
}

fn mime_type_for_path(path: &Path, kind: PropertyKind) -> String {
    if kind == PropertyKind::Directory || kind == PropertyKind::SymlinkDirectory {
        return "inode/directory".into();
    }
    if kind == PropertyKind::BrokenSymlink {
        return "inode/symlink".into();
    }
    Command::new("xdg-mime")
        .args([
            OsStr::new("query"),
            OsStr::new("filetype"),
            path.as_os_str(),
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|mime| mime.trim().to_owned())
        .filter(|mime| valid_mime_type(mime))
        .unwrap_or_else(|| fallback_mime_type(path).to_owned())
}

fn fallback_mime_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("txt" | "md" | "log") => "text/plain",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("pdf") => "application/pdf",
        Some("zip") => "application/zip",
        Some("json") => "application/json",
        Some("xml") => "application/xml",
        Some("mp3") => "audio/mpeg",
        Some("mp4") => "video/mp4",
        _ => "application/octet-stream",
    }
}

fn valid_mime_type(mime: &str) -> bool {
    let Some((top, subtype)) = mime.split_once('/') else {
        return false;
    };
    !top.is_empty()
        && !subtype.is_empty()
        && mime.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'+' | b'.' | b'_')
        })
}

/// Enumerates user identities through the system NSS database. Numeric ids
/// remain available in snapshots even when NSS lookup is unavailable.
pub fn list_users() -> Vec<PropertyIdentity> {
    list_identities("passwd", Path::new("/etc/passwd"), 2)
}

/// Enumerates group identities through the system NSS database.
pub fn list_groups() -> Vec<PropertyIdentity> {
    list_identities("group", Path::new("/etc/group"), 2)
}

fn identity_name(database: &str, id: u32) -> Option<String> {
    let id = id.to_string();
    let output = Command::new("getent")
        .args([database, &id])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    parse_identity_line(&String::from_utf8_lossy(&output.stdout), 2)
        .filter(|identity| identity.id.to_string() == id)
        .map(|identity| identity.name)
}

fn list_identities(database: &str, fallback: &Path, id_field: usize) -> Vec<PropertyIdentity> {
    let text = Command::new("getent")
        .arg(database)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .or_else(|| fs::read_to_string(fallback).ok())
        .unwrap_or_default();
    let mut seen = HashSet::new();
    let mut identities = text
        .lines()
        .filter_map(|line| parse_identity_line(line, id_field))
        .filter(|identity| seen.insert(identity.id))
        // A broken or very large network NSS database must not make the
        // properties selector unbounded.
        .take(4096)
        .collect::<Vec<_>>();
    identities.sort_by(|left, right| {
        left.name
            .to_ascii_lowercase()
            .cmp(&right.name.to_ascii_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });
    identities
}

fn parse_identity_line(line: &str, id_field: usize) -> Option<PropertyIdentity> {
    let fields = line.trim().split(':').collect::<Vec<_>>();
    let name = fields.first()?.trim();
    let id = fields.get(id_field)?.trim().parse().ok()?;
    (!name.is_empty()).then(|| PropertyIdentity {
        id,
        name: name.to_owned(),
    })
}

fn applications_for_mime(
    mime_type: &str,
    locale: &str,
) -> (Vec<PropertyApplication>, Option<PropertyApplication>) {
    let default_id = query_default_application(mime_type);
    let entries = scan_desktop_entries(locale);
    let current_desktops = current_desktops();
    let mut default_application = default_id.as_deref().and_then(|id| {
        entries
            .iter()
            .find(|entry| entry.application.desktop_id == id)
            .filter(|entry| try_exec_available(entry.try_exec.as_deref()))
            .map(|entry| entry.application.clone())
    });
    let mut applications = entries
        .iter()
        .filter(|entry| entry.mime_types.iter().any(|mime| mime == mime_type))
        .filter(|entry| !entry.hidden && !entry.no_display)
        .filter(|entry| desktop_entry_visible(entry, &current_desktops))
        .filter(|entry| try_exec_available(entry.try_exec.as_deref()))
        .map(|entry| entry.application.clone())
        .collect::<Vec<_>>();
    applications.sort_by(|left, right| {
        left.name
            .to_ascii_lowercase()
            .cmp(&right.name.to_ascii_lowercase())
            .then_with(|| left.desktop_id.cmp(&right.desktop_id))
    });
    applications.dedup_by(|left, right| left.desktop_id == right.desktop_id);

    if default_application.is_none() {
        default_application = default_id.as_deref().map(|id| PropertyApplication {
            desktop_id: id.to_owned(),
            name: id.trim_end_matches(".desktop").to_owned(),
            icon: None,
            desktop_file: PathBuf::new(),
        });
    }
    if let Some(default) = default_application.as_ref()
        && !applications
            .iter()
            .any(|application| application.desktop_id == default.desktop_id)
    {
        applications.insert(0, default.clone());
    }
    (applications, default_application)
}

/// Returns the visible desktop applications registered for a local path.
///
/// This is shared by the Properties selector and the explorer context menu so
/// both surfaces follow the same Freedesktop MIME associations.
pub fn applications_for_path(path: &Path) -> Vec<PropertyApplication> {
    let kind = if path.is_dir() {
        PropertyKind::Directory
    } else {
        PropertyKind::File
    };
    let mime_type = mime_type_for_path(path, kind);
    applications_for_mime(&mime_type, &desktop_locale()).0
}

fn query_default_application(mime_type: &str) -> Option<String> {
    if !valid_mime_type(mime_type) {
        return None;
    }
    Command::new("xdg-mime")
        .args(["query", "default", mime_type])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|output| output.lines().next().map(str::trim).map(str::to_owned))
        .filter(|id| valid_desktop_id(id))
}

fn scan_desktop_entries(locale: &str) -> Vec<DesktopEntry> {
    let mut entries = Vec::new();
    let mut seen_ids = HashSet::new();
    for directory in application_directories() {
        if !directory.is_dir() {
            continue;
        }
        for item in WalkDir::new(&directory)
            .min_depth(1)
            .max_depth(8)
            .follow_links(false)
            .into_iter()
            .filter_map(std::result::Result::ok)
            .filter(|item| item.file_type().is_file())
        {
            let path = item.path();
            if path.extension().and_then(OsStr::to_str) != Some("desktop") {
                continue;
            }
            let Some(desktop_id) = desktop_id_for_path(&directory, path) else {
                continue;
            };
            // A file in a higher-precedence XDG data directory masks every
            // lower-precedence file with the same desktop id, including when
            // the override is Hidden=true.
            if !seen_ids.insert(desktop_id.clone()) {
                continue;
            }
            let Ok(text) = fs::read_to_string(path) else {
                continue;
            };
            if let Some(entry) = parse_desktop_entry(path, desktop_id, &text, locale) {
                entries.push(entry);
            }
        }
    }
    entries
}

fn application_directories() -> Vec<PathBuf> {
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .map(|home| home.join(".local/share"))
        });
    let mut directories = Vec::new();
    if let Some(home) = data_home {
        directories.push(home.join("applications"));
    }
    let data_dirs = std::env::var_os("XDG_DATA_DIRS")
        .filter(|value| !value.is_empty())
        .map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
        .unwrap_or_else(|| {
            vec![
                PathBuf::from("/usr/local/share"),
                PathBuf::from("/usr/share"),
            ]
        });
    directories.extend(
        data_dirs
            .into_iter()
            .map(|directory| directory.join("applications")),
    );
    let mut seen = HashSet::new();
    directories
        .into_iter()
        .filter(|directory| seen.insert(directory.clone()))
        .collect()
}

fn desktop_id_for_path(directory: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(directory).ok()?;
    let components = relative
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => Some(value.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>();
    (!components.is_empty()).then(|| {
        components
            .into_iter()
            .map(|component| component.into_owned())
            .collect::<Vec<_>>()
            .join("-")
    })
}

fn parse_desktop_entry(
    path: &Path,
    desktop_id: String,
    text: &str,
    locale: &str,
) -> Option<DesktopEntry> {
    let mut fields = HashMap::<String, String>::new();
    let mut in_desktop_entry = false;
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_desktop_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_desktop_entry {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        fields.insert(key.trim().to_owned(), desktop_unescape(value.trim()));
    }
    if fields.get("Type").is_none_or(|kind| kind != "Application") {
        return None;
    }
    let name = localized_desktop_name(&fields, locale)
        .or_else(|| fields.get("Name").cloned())
        .filter(|name| !name.trim().is_empty())?;
    Some(DesktopEntry {
        application: PropertyApplication {
            desktop_id,
            name,
            icon: fields.get("Icon").cloned().filter(|icon| !icon.is_empty()),
            desktop_file: path.to_path_buf(),
        },
        mime_types: desktop_list(fields.get("MimeType")),
        hidden: desktop_bool(fields.get("Hidden")),
        no_display: desktop_bool(fields.get("NoDisplay")),
        only_show_in: desktop_list(fields.get("OnlyShowIn")),
        not_show_in: desktop_list(fields.get("NotShowIn")),
        try_exec: fields
            .get("TryExec")
            .cloned()
            .filter(|value| !value.is_empty()),
    })
}

fn localized_desktop_name(fields: &HashMap<String, String>, locale: &str) -> Option<String> {
    let locale = locale.trim();
    let base = locale.split(['_', '-']).next().unwrap_or(locale);
    [
        (!locale.is_empty()).then(|| format!("Name[{locale}]")),
        (!base.is_empty() && base != locale).then(|| format!("Name[{base}]")),
    ]
    .into_iter()
    .flatten()
    .find_map(|key| fields.get(&key).cloned())
}

fn desktop_unescape(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            output.push(character);
            continue;
        }
        match characters.next() {
            Some('s') => output.push(' '),
            Some('n') => output.push('\n'),
            Some('t') => output.push('\t'),
            Some('r') => output.push('\r'),
            Some('\\') => output.push('\\'),
            Some(other) => {
                output.push('\\');
                output.push(other);
            }
            None => output.push('\\'),
        }
    }
    output
}

fn desktop_list(value: Option<&String>) -> Vec<String> {
    value
        .into_iter()
        .flat_map(|value| value.split(';'))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

fn desktop_bool(value: Option<&String>) -> bool {
    value.is_some_and(|value| value.eq_ignore_ascii_case("true"))
}

fn current_desktops() -> Vec<String> {
    std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split([':', ';'])
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

fn desktop_entry_visible(entry: &DesktopEntry, current: &[String]) -> bool {
    let current_matches = |values: &[String]| {
        values.iter().any(|value| {
            current
                .iter()
                .any(|desktop| desktop.eq_ignore_ascii_case(value))
        })
    };
    (entry.only_show_in.is_empty() || current_matches(&entry.only_show_in))
        && !current_matches(&entry.not_show_in)
}

fn try_exec_available(try_exec: Option<&str>) -> bool {
    let Some(program) = try_exec
        .map(str::trim)
        .filter(|program| !program.is_empty())
    else {
        return true;
    };
    let path = Path::new(program);
    if path.components().count() > 1 {
        return path.is_file();
    }
    command_exists(program)
}

fn command_exists(program: &str) -> bool {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .any(|directory| directory.join(program).is_file())
}

fn desktop_locale() -> String {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
        .unwrap_or_default()
        .split(['.', '@'])
        .next()
        .unwrap_or_default()
        .to_owned()
}

fn valid_desktop_id(id: &str) -> bool {
    id.ends_with(".desktop") && !id.contains(['/', '\\', '\0']) && !id.trim().is_empty()
}

/// Calculates aggregate contents without following symbolic links or crossing
/// into a different mounted filesystem.
pub fn calculate_size(paths: &[PathBuf], cancel: &AtomicBool) -> DirectorySize {
    calculate_size_with_progress(paths, cancel, |_| true)
}

/// Streaming form of [`calculate_size`]. The callback is coalesced to avoid
/// flooding the UI and may return `false` to stop the worker.
pub fn calculate_size_with_progress<F>(
    paths: &[PathBuf],
    cancel: &AtomicBool,
    mut on_progress: F,
) -> DirectorySize
where
    F: FnMut(&DirectorySize) -> bool,
{
    let roots = minimal_size_roots(paths);
    let mut size = DirectorySize::default();
    let mut hard_links = HashSet::<(u64, u64)>::new();
    let mut last_progress = Instant::now();
    let mut items_since_progress = 0_usize;

    'roots: for root in roots {
        if cancel.load(Ordering::Relaxed) {
            size.cancelled = true;
            break;
        }
        let root_metadata = match fs::symlink_metadata(&root) {
            Ok(metadata) => metadata,
            Err(_) => {
                size.unreadable = size.unreadable.saturating_add(1);
                continue;
            }
        };
        if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
            accumulate_size(&mut size, &root_metadata, true, &mut hard_links);
            continue;
        }

        for entry in WalkDir::new(&root)
            .follow_links(false)
            .follow_root_links(false)
            .same_file_system(true)
            .into_iter()
        {
            if cancel.load(Ordering::Relaxed) {
                size.cancelled = true;
                break 'roots;
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    size.unreadable = size.unreadable.saturating_add(1);
                    continue;
                }
            };
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(_) => {
                    size.unreadable = size.unreadable.saturating_add(1);
                    continue;
                }
            };
            accumulate_size(&mut size, &metadata, entry.depth() != 0, &mut hard_links);
            items_since_progress = items_since_progress.saturating_add(1);
            if items_since_progress >= SIZE_PROGRESS_ITEMS
                || last_progress.elapsed() >= SIZE_PROGRESS_INTERVAL
            {
                if !on_progress(&size) {
                    size.cancelled = true;
                    break 'roots;
                }
                items_since_progress = 0;
                last_progress = Instant::now();
            }
        }
    }
    if cancel.load(Ordering::Relaxed) {
        size.cancelled = true;
    }
    let _ = on_progress(&size);
    size
}

fn accumulate_size(
    size: &mut DirectorySize,
    metadata: &fs::Metadata,
    count_item: bool,
    hard_links: &mut HashSet<(u64, u64)>,
) {
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        size.bytes = size.bytes.saturating_add(metadata.len());
        size.allocated = size
            .allocated
            .saturating_add(metadata.blocks().saturating_mul(512));
        if count_item {
            size.links = size.links.saturating_add(1);
        }
        return;
    }
    if metadata.is_file() {
        // POSIX hard links expose one allocation through several directory
        // entries. Count their inode only once across every selected root.
        if hard_links.insert((metadata.dev(), metadata.ino())) {
            size.bytes = size.bytes.saturating_add(metadata.len());
            size.allocated = size
                .allocated
                .saturating_add(metadata.blocks().saturating_mul(512));
            if count_item {
                size.files = size.files.saturating_add(1);
            }
        }
        return;
    }
    size.bytes = size.bytes.saturating_add(metadata.len());
    size.allocated = size
        .allocated
        .saturating_add(metadata.blocks().saturating_mul(512));
    if metadata.is_dir() && count_item {
        size.directories = size.directories.saturating_add(1);
    }
}

fn minimal_size_roots(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots = Vec::<PathBuf>::new();
    for path in paths {
        if roots.iter().any(|existing| existing == path) {
            continue;
        }
        let nested = roots.iter().any(|existing| {
            fs::symlink_metadata(existing).is_ok_and(|metadata| metadata.is_dir())
                && path.starts_with(existing)
        });
        if !nested {
            roots.push(path.clone());
        }
    }
    // A parent may occur after its child in the original selection.
    roots.retain(|candidate| {
        !paths.iter().any(|other| {
            other != candidate
                && fs::symlink_metadata(other).is_ok_and(|metadata| metadata.is_dir())
                && candidate.starts_with(other)
        })
    });
    roots
}

/// Applies the editable portion of a properties dialog. Permission changes
/// are attempted unprivileged first and transparently fall back to the
/// existing polkit-backed BExplorer helper only for EACCES/EPERM.
pub fn apply(changes: PropertiesChanges) -> Result<ApplyOutcome> {
    if changes.paths.is_empty() {
        return Err(BExplorerError::Operation(
            "At least one path is required to apply properties".into(),
        ));
    }
    validate_permission_fields(
        changes.permission_mask,
        changes.permission_value,
        changes.owner,
        changes.group,
    )?;
    for path in &changes.paths {
        fs::symlink_metadata(path)?;
    }

    // Validate every request before the first mutation. In particular, a
    // malformed rename or association must never leave permissions partially
    // changed before returning an error.
    let rename_request = changes.new_name.as_ref().map(|new_name| RenameRequest {
        paths: changes.paths.clone(),
        new_name: new_name.clone(),
    });
    if let Some(request) = rename_request.as_ref() {
        validate_rename_request(request)?;
    }
    let association_request = match (
        changes.mime_type.as_deref(),
        changes.default_application.as_deref(),
    ) {
        (Some(mime_type), Some(desktop_id)) => {
            validate_default_application_change(mime_type, desktop_id)?;
            Some((mime_type.to_owned(), desktop_id.to_owned()))
        }
        (None, Some(_)) => {
            return Err(BExplorerError::Operation(
                "A MIME type is required to change the default application".into(),
            ));
        }
        _ => None,
    };

    let permission_request = PermissionRequest {
        paths: changes.paths.clone(),
        permission_mask: changes.permission_mask,
        permission_value: changes.permission_value,
        owner: changes.owner,
        group: changes.group,
        recursive: changes.recursive,
    };
    let has_permission_changes = permission_request.permission_mask != 0
        || permission_request.owner.is_some()
        || permission_request.group.is_some();
    let (permission_entries, permission_elevated) = if has_permission_changes {
        match apply_permission_request(&permission_request, PermissionApplyMode::Unprivileged) {
            Ok(entries) => (entries, false),
            Err(BExplorerError::Io(error))
                if error.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                (run_elevated_permission_request(&permission_request)?, true)
            }
            Err(error) => return Err(error),
        }
    } else {
        (0, false)
    };

    // Rename is deliberately last. If an association command fails or an
    // earlier operation is cancelled, the frontend can safely keep referring
    // to the original path.
    let association_changed = association_request
        .as_ref()
        .map(|(mime_type, desktop_id)| set_default_application_validated(mime_type, desktop_id))
        .transpose()?
        .unwrap_or(false);

    let mut paths = changes.paths;
    let (renamed, rename_elevated) = if let Some(request) = rename_request {
        let (renamed, elevated) = rename_single_path_with_elevation(&request)?;
        paths[0] = renamed.clone();
        (Some(renamed), elevated)
    } else {
        (None, false)
    };

    Ok(ApplyOutcome {
        paths,
        renamed,
        permission_entries,
        association_changed,
        elevated: permission_elevated || rename_elevated,
    })
}

fn validate_permission_fields(
    mask: u32,
    value: u32,
    owner: Option<u32>,
    group: Option<u32>,
) -> Result<()> {
    if mask & !VALID_PERMISSION_BITS != 0 || value & !VALID_PERMISSION_BITS != 0 {
        return Err(BExplorerError::Operation(
            "Permission bits must be between 0000 and 7777".into(),
        ));
    }
    if owner == Some(u32::MAX) || group == Some(u32::MAX) {
        return Err(BExplorerError::Operation(
            "The reserved -1 owner/group id cannot be assigned".into(),
        ));
    }
    Ok(())
}

fn validate_rename_request(request: &RenameRequest) -> Result<(PathBuf, PathBuf)> {
    if request.paths.len() != 1 {
        return Err(BExplorerError::Operation(
            "Renaming from properties requires exactly one selected item".into(),
        ));
    }
    if request.new_name.is_empty()
        || matches!(request.new_name.as_str(), "." | "..")
        || request.new_name.contains(['/', '\0'])
    {
        return Err(BExplorerError::Operation("The new name is invalid".into()));
    }
    let source = request.paths[0].clone();
    let current_name = source
        .file_name()
        .ok_or_else(|| BExplorerError::Operation("The filesystem root cannot be renamed".into()))?;
    let target = source
        .parent()
        .ok_or_else(|| BExplorerError::Operation("The item has no parent directory".into()))?
        .join(&request.new_name);
    if current_name == OsStr::new(&request.new_name) {
        debug_assert_eq!(source, target);
    }
    Ok((source, target))
}

fn rename_single_path(request: &RenameRequest) -> Result<PathBuf> {
    let (source, target) = validate_rename_request(request)?;
    if source == target {
        return Ok(source);
    }
    if let Err(error) = rustix::fs::renameat_with(
        rustix::fs::CWD,
        &source,
        rustix::fs::CWD,
        &target,
        rustix::fs::RenameFlags::NOREPLACE,
    ) {
        let error = std::io::Error::from(error);
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            return Err(BExplorerError::Operation(format!(
                "An item named {} already exists",
                target.display()
            )));
        }
        return Err(BExplorerError::Io(error));
    }
    Ok(target)
}

fn rename_single_path_with_elevation(request: &RenameRequest) -> Result<(PathBuf, bool)> {
    match rename_single_path(request) {
        Ok(path) => Ok((path, false)),
        Err(BExplorerError::Io(error)) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            match run_elevated_properties_request(&ElevatedPropertiesRequest::Rename(
                request.clone(),
            ))? {
                ElevatedPropertiesResponse::Rename(path) => Ok((path, true)),
                ElevatedPropertiesResponse::Permissions(_) => Err(BExplorerError::Operation(
                    "Elevated properties helper returned the wrong response".into(),
                )),
            }
        }
        Err(error) => Err(error),
    }
}

fn apply_permission_request(
    request: &PermissionRequest,
    apply_mode: PermissionApplyMode,
) -> Result<usize> {
    if request.paths.is_empty() {
        return Err(BExplorerError::Operation(
            "At least one path is required to apply permissions".into(),
        ));
    }
    validate_permission_fields(
        request.permission_mask,
        request.permission_value,
        request.owner,
        request.group,
    )?;
    let roots = if request.recursive {
        minimal_size_roots(&request.paths)
    } else {
        request.paths.clone()
    };
    let mut changed = 0_usize;
    for root in roots {
        let metadata = fs::symlink_metadata(&root)?;
        if request.recursive && metadata.is_dir() && !metadata.file_type().is_symlink() {
            for entry in WalkDir::new(&root)
                .follow_links(false)
                .follow_root_links(false)
                .same_file_system(true)
                .contents_first(true)
                .into_iter()
            {
                let entry = entry.map_err(walkdir_io_error)?;
                changed = changed.saturating_add(apply_permissions_to_path(
                    entry.path(),
                    request,
                    apply_mode,
                )?);
            }
        } else {
            changed =
                changed.saturating_add(apply_permissions_to_path(&root, request, apply_mode)?);
        }
    }
    Ok(changed)
}

fn walkdir_io_error(error: walkdir::Error) -> BExplorerError {
    let kind = error
        .io_error()
        .map(std::io::Error::kind)
        .unwrap_or(std::io::ErrorKind::Other);
    BExplorerError::Io(std::io::Error::new(kind, error.to_string()))
}

fn apply_permissions_to_path(
    path: &Path,
    request: &PermissionRequest,
    apply_mode: PermissionApplyMode,
) -> Result<usize> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        let owner = request.owner.filter(|owner| *owner != metadata.uid());
        let group = request.group.filter(|group| *group != metadata.gid());
        if owner.is_none() && group.is_none() {
            // Linux has no meaningful chmod operation for a symlink itself.
            return Ok(0);
        }
        rustix::fs::chownat(
            rustix::fs::CWD,
            path,
            owner.map(rustix::fs::Uid::from_raw),
            group.map(rustix::fs::Gid::from_raw),
            rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
        )
        .map_err(std::io::Error::from)?;
        return Ok(1);
    }

    match apply_mode {
        PermissionApplyMode::Unprivileged => {
            apply_permissions_to_path_unprivileged(path, request, &metadata)
        }
        PermissionApplyMode::ElevatedSecure => {
            apply_permissions_to_path_by_descriptor(path, request, &metadata)
        }
    }
}

fn apply_permissions_to_path_unprivileged(
    path: &Path,
    request: &PermissionRequest,
    metadata: &fs::Metadata,
) -> Result<usize> {
    let owner = request.owner.filter(|owner| *owner != metadata.uid());
    let group = request.group.filter(|group| *group != metadata.gid());
    let ownership_change = owner.is_some() || group.is_some();
    let current_mode = metadata.mode() & VALID_PERMISSION_BITS;
    let new_mode = (current_mode & !request.permission_mask)
        | (request.permission_value & request.permission_mask);
    let change_mode =
        request.permission_mask != 0 && (new_mode != current_mode || ownership_change);
    if !ownership_change && !change_mode {
        return Ok(0);
    }

    if ownership_change {
        rustix::fs::chown(
            path,
            owner.map(rustix::fs::Uid::from_raw),
            group.map(rustix::fs::Gid::from_raw),
        )
        .map_err(std::io::Error::from)?;
    }
    if change_mode {
        fs::set_permissions(path, fs::Permissions::from_mode(new_mode))?;
    }
    Ok(1)
}

fn apply_permissions_to_path_by_descriptor(
    path: &Path,
    request: &PermissionRequest,
    metadata: &fs::Metadata,
) -> Result<usize> {
    // Pin the non-symlink entry before inspecting or mutating it. O_NOFOLLOW
    // turns a concurrent regular-file-to-symlink replacement into ELOOP,
    // while the inode comparison catches replacement by another regular file.
    let descriptor = rustix::fs::openat(
        rustix::fs::CWD,
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC
            | rustix::fs::OFlags::NONBLOCK,
        rustix::fs::Mode::empty(),
    )
    .map_err(std::io::Error::from)?;
    let stat = rustix::fs::fstat(&descriptor).map_err(std::io::Error::from)?;
    if stat.st_dev != metadata.dev() || stat.st_ino != metadata.ino() {
        return Err(BExplorerError::Operation(format!(
            "The item changed while applying properties: {}",
            path.display()
        )));
    }

    let owner = request.owner.filter(|owner| *owner != stat.st_uid);
    let group = request.group.filter(|group| *group != stat.st_gid);
    let ownership_change = owner.is_some() || group.is_some();
    let current_mode = stat.st_mode & VALID_PERMISSION_BITS;
    let new_mode = (current_mode & !request.permission_mask)
        | (request.permission_value & request.permission_mask);
    // chown may clear setuid/setgid. If a mode edit and ownership edit are
    // combined, restore the complete computed mode even when the selected
    // mask happened to leave its bits numerically unchanged.
    let change_mode =
        request.permission_mask != 0 && (new_mode != current_mode || ownership_change);
    if !ownership_change && !change_mode {
        return Ok(0);
    }

    if ownership_change {
        rustix::fs::fchown(
            &descriptor,
            owner.map(rustix::fs::Uid::from_raw),
            group.map(rustix::fs::Gid::from_raw),
        )
        .map_err(std::io::Error::from)?;
    }
    if change_mode {
        rustix::fs::fchmod(&descriptor, rustix::fs::Mode::from_raw_mode(new_mode))
            .map_err(std::io::Error::from)?;
    }
    Ok(1)
}

fn validate_default_application_change(mime_type: &str, desktop_id: &str) -> Result<()> {
    if !valid_mime_type(mime_type) {
        return Err(BExplorerError::Operation(format!(
            "Invalid MIME type: {mime_type}"
        )));
    }
    if !valid_desktop_id(desktop_id) {
        return Err(BExplorerError::Operation(format!(
            "Invalid desktop application id: {desktop_id}"
        )));
    }
    let installed = scan_desktop_entries(&desktop_locale())
        .into_iter()
        .any(|entry| entry.application.desktop_id == desktop_id && !entry.hidden);
    if !installed {
        return Err(BExplorerError::Operation(format!(
            "The application {desktop_id} is no longer installed"
        )));
    }
    Ok(())
}

fn set_default_application_validated(mime_type: &str, desktop_id: &str) -> Result<bool> {
    if query_default_application(mime_type).as_deref() == Some(desktop_id) {
        return Ok(false);
    }
    let output = Command::new("xdg-mime")
        .args(["default", desktop_id, mime_type])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| BExplorerError::Operation(format!("Could not run xdg-mime: {error}")))?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(BExplorerError::Operation(if error.is_empty() {
            format!("xdg-mime rejected {desktop_id} as the default for {mime_type}")
        } else {
            format!("Could not change the default application: {error}")
        }));
    }
    if query_default_application(mime_type).as_deref() != Some(desktop_id) {
        return Err(BExplorerError::Operation(
            "The desktop did not retain the new default application".into(),
        ));
    }
    Ok(true)
}

fn run_elevated_permission_request(request: &PermissionRequest) -> Result<usize> {
    match run_elevated_properties_request(&ElevatedPropertiesRequest::Permissions(request.clone()))?
    {
        ElevatedPropertiesResponse::Permissions(result) => Ok(result.entries),
        ElevatedPropertiesResponse::Rename(_) => Err(BExplorerError::Operation(
            "Elevated properties helper returned the wrong response".into(),
        )),
    }
}

fn run_elevated_properties_request(
    request: &ElevatedPropertiesRequest,
) -> Result<ElevatedPropertiesResponse> {
    let input = serde_json::to_vec(request)?;
    if input.len() > MAX_ELEVATED_REQUEST_BYTES {
        return Err(BExplorerError::Operation(
            "The elevated properties request is too large".into(),
        ));
    }
    let output = crate::platform::shell::run_elevated_current_exe_with_input(
        &[OsString::from(ELEVATED_HELPER_ARG)],
        &input,
    )?;
    if let Ok(result) = serde_json::from_slice::<
        std::result::Result<ElevatedPropertiesResponse, String>,
    >(&output.stdout)
    {
        return result.map_err(BExplorerError::Operation);
    }

    let code = output.status.code().unwrap_or(1);
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(BExplorerError::Operation(if stderr.is_empty() {
        format!("Elevated properties helper failed with exit code {code}")
    } else {
        format!("Elevated properties helper failed with exit code {code}: {stderr}")
    }))
}

/// Handles the private elevated helper invocation before the GUI starts.
pub fn try_run_elevated_helper_from_args() -> Option<i32> {
    let mut args = std::env::args_os();
    let _executable = args.next();
    if args.next()?.as_os_str() != OsStr::new(ELEVATED_HELPER_ARG) {
        return None;
    }
    Some(run_elevated_helper())
}

fn run_elevated_helper() -> i32 {
    let result = (|| -> Result<ElevatedPropertiesResponse> {
        let request = read_elevated_properties_request(std::io::stdin().lock())?;
        match request {
            ElevatedPropertiesRequest::Permissions(request) => {
                let entries =
                    apply_permission_request(&request, PermissionApplyMode::ElevatedSecure)?;
                Ok(ElevatedPropertiesResponse::Permissions(PermissionResult {
                    entries,
                }))
            }
            ElevatedPropertiesRequest::Rename(request) => {
                rename_single_path(&request).map(ElevatedPropertiesResponse::Rename)
            }
        }
    })();
    let serialized: std::result::Result<ElevatedPropertiesResponse, String> =
        result.map_err(|error| error.to_string());
    let successful = serialized.is_ok();
    let wrote_result = serde_json::to_vec(&serialized).ok().is_some_and(|bytes| {
        use std::io::Write;

        let mut stdout = std::io::stdout().lock();
        stdout
            .write_all(&bytes)
            .and_then(|()| stdout.flush())
            .is_ok()
    });
    if !wrote_result {
        2
    } else if successful {
        0
    } else {
        1
    }
}

fn read_elevated_properties_request(
    reader: impl std::io::Read,
) -> Result<ElevatedPropertiesRequest> {
    use std::io::Read;

    let mut bytes = Vec::new();
    reader
        .take((MAX_ELEVATED_REQUEST_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_ELEVATED_REQUEST_BYTES {
        return Err(BExplorerError::Operation(
            "The elevated properties request is too large".into(),
        ));
    }
    Ok(serde_json::from_slice(&bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::os::unix::fs::symlink;

    fn temporary_directory(label: &str) -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "bexplorer-properties-{label}-{}-{timestamp}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create properties test directory");
        path
    }

    #[test]
    fn mountinfo_parser_decodes_fields_and_read_only_options() {
        let entry = parse_mountinfo_line(
            "36 25 8:1 / /media/My\\040Disk ro,nosuid - ext4 /dev/sda1 rw,errors=remount-ro",
        )
        .expect("mountinfo entry");

        assert_eq!(entry.major, 8);
        assert_eq!(entry.minor, 1);
        assert_eq!(entry.mount_point, Path::new("/media/My Disk"));
        assert_eq!(entry.source, "/dev/sda1");
        assert_eq!(entry.file_system, "ext4");
        assert!(entry.options.iter().any(|option| option == "ro"));
    }

    #[test]
    fn symlink_mount_uses_the_filesystem_containing_the_link() {
        let root = temporary_directory("link-mount");
        let link = root.join("broken-link");
        symlink("missing-target", &link).expect("create broken link");
        let metadata = fs::symlink_metadata(&link).expect("link metadata");
        let mount_point = root.canonicalize().expect("canonical test root");
        let mounts = vec![MountInfoEntry {
            major: rustix::fs::major(metadata.dev()),
            minor: rustix::fs::minor(metadata.dev()),
            mount_point: mount_point.clone(),
            options: vec!["rw".into()],
            file_system: "testfs".into(),
            source: "test-device".into(),
            super_options: vec!["rw".into()],
        }];

        let mount = property_mount_for_path(&link, &mounts).expect("link mount");

        assert_eq!(mount.mount_point, mount_point);
        assert_eq!(mount.source, "test-device");
        assert_eq!(mount.file_system, "testfs");
        fs::remove_dir_all(root).expect("cleanup link mount test");
    }

    #[test]
    fn desktop_parser_uses_localized_name_and_unescapes_values() {
        let entry = parse_desktop_entry(
            Path::new("/usr/share/applications/example.desktop"),
            "example.desktop".into(),
            concat!(
                "[Desktop Entry]\n",
                "Type=Application\n",
                "Name=Example\n",
                "Name[es]=Editor\\sSimple\n",
                "Icon=example\n",
                "MimeType=text/plain;application/json;\n",
            ),
            "es_ES",
        )
        .expect("desktop entry");

        assert_eq!(entry.application.name, "Editor Simple");
        assert_eq!(entry.application.icon.as_deref(), Some("example"));
        assert_eq!(entry.mime_types, ["text/plain", "application/json"]);
    }

    #[test]
    fn item_snapshot_preserves_directory_file_and_broken_symlink_identity() {
        let root = temporary_directory("symlinks");
        let directory = root.join("folder");
        let file = root.join("file.txt");
        let directory_link = root.join("folder-link");
        let file_link = root.join("file-link");
        let broken_link = root.join("broken-link");
        fs::create_dir(&directory).expect("create target directory");
        fs::write(&file, b"contents").expect("write target file");
        symlink("folder", &directory_link).expect("directory symlink");
        symlink("file.txt", &file_link).expect("file symlink");
        symlink("missing", &broken_link).expect("broken symlink");
        let mut owners = HashMap::new();
        let mut groups = HashMap::new();

        let directory_item = load_item(&directory_link, &[], &mut owners, &mut groups)
            .expect("directory link properties");
        let file_item =
            load_item(&file_link, &[], &mut owners, &mut groups).expect("file link properties");
        let broken_item =
            load_item(&broken_link, &[], &mut owners, &mut groups).expect("broken link properties");

        assert_eq!(directory_item.kind, PropertyKind::SymlinkDirectory);
        assert_eq!(file_item.kind, PropertyKind::SymlinkFile);
        assert_eq!(broken_item.kind, PropertyKind::BrokenSymlink);
        assert_eq!(
            directory_item
                .symlink
                .as_ref()
                .map(|link| link.raw_target.as_path()),
            Some(Path::new("folder"))
        );
        assert!(broken_item.symlink.as_ref().is_some_and(|link| link.broken));
        fs::remove_dir_all(root).expect("cleanup symlink test");
    }

    #[test]
    fn recursive_size_does_not_follow_links_and_deduplicates_hard_links() {
        let root = temporary_directory("size");
        let nested = root.join("nested");
        fs::create_dir(&nested).expect("create nested directory");
        let file = nested.join("data.bin");
        let hard_link = nested.join("hard-link.bin");
        fs::write(&file, vec![7_u8; 4096]).expect("write data");
        fs::hard_link(&file, &hard_link).expect("create hard link");
        symlink("nested", root.join("directory-link")).expect("create directory link");
        symlink("..", nested.join("loop")).expect("create loop link");

        let cancel = AtomicBool::new(false);
        let size = calculate_size(std::slice::from_ref(&root), &cancel);

        assert!(!size.cancelled);
        assert_eq!(size.files, 1);
        assert_eq!(size.directories, 1);
        assert_eq!(size.links, 2);
        assert!(size.bytes >= 4096);
        assert_eq!(size.unreadable, 0);
        fs::remove_dir_all(root).expect("cleanup size test");
    }

    #[test]
    fn recursive_size_honors_preexisting_cancellation() {
        let root = temporary_directory("cancel");
        fs::write(root.join("file"), b"contents").expect("write file");
        let cancel = AtomicBool::new(true);

        let size = calculate_size(std::slice::from_ref(&root), &cancel);

        assert!(size.cancelled);
        assert_eq!(size.files, 0);
        fs::remove_dir_all(root).expect("cleanup cancellation test");
    }

    #[test]
    fn permission_masks_preserve_unselected_bits() {
        let root = temporary_directory("permissions");
        let file = root.join("file");
        fs::write(&file, b"contents").expect("write file");
        fs::set_permissions(&file, fs::Permissions::from_mode(0o640))
            .expect("set initial permissions");

        let outcome = apply(PropertiesChanges {
            paths: vec![file.clone()],
            permission_mask: 0o007,
            permission_value: 0o004,
            ..PropertiesChanges::default()
        })
        .expect("apply permissions");

        assert_eq!(outcome.permission_entries, 1);
        assert!(!outcome.elevated);
        assert_eq!(
            fs::metadata(&file).expect("metadata").permissions().mode() & 0o777,
            0o644
        );
        fs::remove_dir_all(root).expect("cleanup permissions test");
    }

    #[test]
    fn owner_can_chmod_a_mode_zero_file_without_elevation() {
        let root = temporary_directory("mode-zero-permissions");
        let file = root.join("file");
        fs::write(&file, b"contents").expect("write file");
        fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).expect("set mode zero");

        let outcome = apply(PropertiesChanges {
            paths: vec![file.clone()],
            permission_mask: 0o600,
            permission_value: 0o600,
            ..PropertiesChanges::default()
        })
        .expect("owner chmod mode-zero file");

        assert!(!outcome.elevated);
        assert_eq!(
            fs::metadata(&file).expect("metadata").permissions().mode() & 0o777,
            0o600
        );
        fs::remove_dir_all(root).expect("cleanup mode-zero test");
    }

    #[test]
    fn elevated_permission_mode_mutates_regular_files_through_a_descriptor() {
        let root = temporary_directory("descriptor-permissions");
        let file = root.join("file");
        fs::write(&file, b"contents").expect("write file");
        fs::set_permissions(&file, fs::Permissions::from_mode(0o640)).expect("set initial mode");
        let request = PermissionRequest {
            paths: vec![file.clone()],
            permission_mask: 0o007,
            permission_value: 0o004,
            owner: None,
            group: None,
            recursive: false,
        };

        let changed = apply_permission_request(&request, PermissionApplyMode::ElevatedSecure)
            .expect("descriptor permission change");

        assert_eq!(changed, 1);
        assert_eq!(
            fs::metadata(&file).expect("metadata").permissions().mode() & 0o777,
            0o644
        );
        fs::remove_dir_all(root).expect("cleanup descriptor permission test");
    }

    #[test]
    fn chmod_request_never_changes_a_symlink_target() {
        let root = temporary_directory("link-permissions");
        let target = root.join("target");
        let link = root.join("link");
        fs::write(&target, b"contents").expect("write target");
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).expect("set target mode");
        symlink("target", &link).expect("create link");

        let outcome = apply(PropertiesChanges {
            paths: vec![link],
            permission_mask: 0o777,
            permission_value: 0o777,
            ..PropertiesChanges::default()
        })
        .expect("apply link permissions");

        assert_eq!(outcome.permission_entries, 0);
        assert_eq!(
            fs::metadata(&target)
                .expect("target metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        fs::remove_dir_all(root).expect("cleanup link permissions test");
    }

    #[test]
    fn property_rename_changes_only_the_selected_entry_name() {
        let root = temporary_directory("rename");
        let source = root.join("before.txt");
        fs::write(&source, b"contents").expect("write source");

        let outcome = apply(PropertiesChanges {
            paths: vec![source.clone()],
            new_name: Some("after.txt".into()),
            ..PropertiesChanges::default()
        })
        .expect("rename from properties");

        let target = root.join("after.txt");
        assert_eq!(outcome.renamed.as_ref(), Some(&target));
        assert_eq!(outcome.paths.as_slice(), std::slice::from_ref(&target));
        assert!(!source.exists());
        assert!(target.exists());
        fs::remove_dir_all(root).expect("cleanup rename test");
    }

    #[test]
    fn property_rename_never_replaces_an_existing_destination() {
        let root = temporary_directory("rename-no-replace");
        let source = root.join("source.txt");
        let target = root.join("target.txt");
        fs::write(&source, b"source").expect("write source");
        fs::write(&target, b"target").expect("write target");

        let result = apply(PropertiesChanges {
            paths: vec![source.clone()],
            new_name: Some("target.txt".into()),
            ..PropertiesChanges::default()
        });

        assert!(result.is_err());
        assert_eq!(fs::read(&source).expect("source remains"), b"source");
        assert_eq!(fs::read(&target).expect("target remains"), b"target");
        fs::remove_dir_all(root).expect("cleanup no-replace rename test");
    }

    #[test]
    fn apply_prevalidates_rename_before_changing_permissions() {
        let root = temporary_directory("rename-prevalidation");
        let first = root.join("first");
        let second = root.join("second");
        fs::write(&first, b"first").expect("write first");
        fs::write(&second, b"second").expect("write second");
        fs::set_permissions(&first, fs::Permissions::from_mode(0o600)).expect("set first mode");
        fs::set_permissions(&second, fs::Permissions::from_mode(0o600)).expect("set second mode");

        let result = apply(PropertiesChanges {
            paths: vec![first.clone(), second.clone()],
            new_name: Some("renamed".into()),
            permission_mask: 0o777,
            permission_value: 0o777,
            ..PropertiesChanges::default()
        });

        assert!(result.is_err());
        assert_eq!(
            fs::metadata(&first)
                .expect("first metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(&second)
                .expect("second metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        fs::remove_dir_all(root).expect("cleanup prevalidation test");
    }

    #[test]
    fn elevated_properties_request_json_round_trips_both_operations() {
        let requests = [
            ElevatedPropertiesRequest::Permissions(PermissionRequest {
                paths: vec![PathBuf::from("/tmp/item")],
                permission_mask: 0o700,
                permission_value: 0o500,
                owner: Some(1000),
                group: Some(1000),
                recursive: true,
            }),
            ElevatedPropertiesRequest::Rename(RenameRequest {
                paths: vec![PathBuf::from("/tmp/before")],
                new_name: "after".into(),
            }),
        ];

        for request in requests {
            let json = serde_json::to_vec(&request).expect("serialize elevated request");
            let restored = read_elevated_properties_request(std::io::Cursor::new(json))
                .expect("deserialize elevated request from bounded stream");
            assert_eq!(restored, request);
        }
    }

    #[test]
    fn elevated_properties_request_rejects_oversized_input() {
        let oversized = std::io::repeat(b' ').take((MAX_ELEVATED_REQUEST_BYTES + 1) as u64);

        let result = read_elevated_properties_request(oversized);

        assert!(result.is_err());
    }

    #[test]
    fn identity_parser_rejects_malformed_records() {
        assert_eq!(
            parse_identity_line("alice:x:1000:1000::/home/alice:/bin/sh", 2),
            Some(PropertyIdentity {
                id: 1000,
                name: "alice".into(),
            })
        );
        assert_eq!(parse_identity_line("missing-fields", 2), None);
    }
}
