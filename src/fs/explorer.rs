use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use fs2::free_space;
use serde::{Deserialize, Serialize};

use crate::utils::errors::BExplorerError;
use crate::utils::errors::Result;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntryKind {
    Drive,
    Folder,
    File,
    Symlink,
    Other,
}

impl EntryKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Drive => "Drive",
            Self::Folder => "Folder",
            Self::File => "File",
            Self::Symlink => "Symlink",
            Self::Other => "Other",
        }
    }

    pub fn is_container(&self) -> bool {
        matches!(self, Self::Drive | Self::Folder)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FileCategory {
    Application,
    Image,
    Audio,
    Video,
    Archive,
    Document,
    Spreadsheet,
    Presentation,
    Code,
    Font,
    System,
    DiskImage,
    Other,
}

pub fn classify_file_category(path: &Path) -> FileCategory {
    let Some(ext) = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
    else {
        return FileCategory::Other;
    };
    match ext.as_str() {
        "exe" | "msi" | "bat" | "cmd" | "ps1" | "sh" | "apk" | "ipa" | "appimage" | "deb"
        | "rpm" | "pkg" | "snap" | "flatpak" => FileCategory::Application,
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg" | "ico" | "tiff" | "tif"
        | "heic" | "jxl" | "avif" => FileCategory::Image,
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "wma" | "aac" | "opus" | "mid" | "midi"
        | "aiff" => FileCategory::Audio,
        "mp4" | "mkv" | "mov" | "avi" | "wmv" | "flv" | "webm" | "m4v" | "3gp" => {
            FileCategory::Video
        }
        _ if crate::fs::archive_listing::has_browsable_archive_extension(path) => {
            FileCategory::Archive
        }
        "pdf" | "doc" | "docx" | "odt" | "rtf" | "pages" | "txt" | "md" | "epub" | "mobi" => {
            FileCategory::Document
        }
        "xls" | "xlsx" | "ods" | "csv" | "numbers" | "tsv" => FileCategory::Spreadsheet,
        "ppt" | "pptx" | "odp" | "key" | "ppsx" => FileCategory::Presentation,
        "rs" | "py" | "js" | "ts" | "c" | "cpp" | "h" | "hpp" | "java" | "go" | "html" | "css"
        | "scss" | "sass" | "less" | "json" | "xml" | "yml" | "yaml" | "toml" | "ini" | "cfg"
        | "conf" | "sql" | "rb" | "php" | "swift" | "kt" | "dart" | "r" | "pl" | "lua" | "zig"
        | "s" | "asm" | "wasm" => FileCategory::Code,
        "ttf" | "otf" | "woff" | "woff2" | "eot" => FileCategory::Font,
        "dll" | "sys" | "so" | "dylib" | "bin" | "dat" | "log" | "lock" | "part" | "tmp"
        | "bak" | "swp" | "cache" => FileCategory::System,
        "iso" | "img" | "vhd" | "vmdk" | "vhdx" | "qcow2" | "dmg" => FileCategory::DiskImage,
        _ => FileCategory::Other,
    }
}

impl FileCategory {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Application => "Application",
            Self::Image => "Image",
            Self::Audio => "Audio",
            Self::Video => "Video",
            Self::Archive => "Archive",
            Self::Document => "Document",
            Self::Spreadsheet => "Spreadsheet",
            Self::Presentation => "Presentation",
            Self::Code => "Source code",
            Self::Font => "Font",
            Self::System => "System file",
            Self::DiskImage => "Disk image",
            Self::Other => "File",
        }
    }

    pub fn label_with(&self, path: &Path) -> String {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_uppercase());
        match ext {
            Some(ext) => format!("{} {}", self.label(), ext),
            None => self.label().to_string(),
        }
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub enum DriveKind {
    System,
    Local,
    External,
    Usb,
    DiskImage,
    Network,
    NetworkComputer,
    NetworkPrinter,
    NetworkScanner,
    NetworkMultifunction,
    NetworkDevice,
    Portable,
    Optical,
    RamDisk,
    Unknown,
}

impl DriveKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System Drive",
            Self::Local => "Local Disk",
            Self::External => "External Drive",
            Self::Usb => "USB Drive",
            Self::DiskImage => "Mounted Disk Image",
            Self::Network => "Network Drive",
            Self::NetworkComputer => "Network Computer",
            Self::NetworkPrinter => "Network Printer",
            Self::NetworkScanner => "Network Scanner",
            Self::NetworkMultifunction => "Network Multifunction Device",
            Self::NetworkDevice => "Network Device",
            Self::Portable => "Portable Device",
            Self::Optical => "Optical Drive",
            Self::RamDisk => "RAM Disk",
            Self::Unknown => "Drive",
        }
    }

    pub fn is_ejectable(self) -> bool {
        matches!(
            self,
            Self::External | Self::Usb | Self::DiskImage | Self::Optical
        )
    }

    pub fn is_formatable(self) -> bool {
        #[cfg(target_os = "windows")]
        {
            matches!(self, Self::Local | Self::External | Self::Usb)
        }
        #[cfg(target_os = "linux")]
        {
            matches!(self, Self::Local | Self::External | Self::Usb)
        }
        #[cfg(not(any(target_os = "windows", target_os = "linux")))]
        {
            matches!(self, Self::External | Self::Usb)
        }
    }
}

#[derive(Clone, Debug)]
pub struct FileEntry {
    pub name: String,
    pub path: PathBuf,
    pub kind: EntryKind,
    pub category: FileCategory,
    pub drive_kind: Option<DriveKind>,
    pub file_system: String,
    pub free_space: Option<u64>,
    pub size: Option<u64>,
    pub percent_full: Option<f32>,
    pub modified: Option<String>,
    pub created: Option<String>,
    pub is_hidden: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct StorageCacheEntry {
    name: String,
    path: PathBuf,
    drive_kind: DriveKind,
    file_system: String,
    free_space: Option<u64>,
    size: Option<u64>,
    percent_full: Option<f32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct StorageCache {
    entries: Vec<StorageCacheEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct NetworkCacheEntry {
    name: String,
    path: PathBuf,
    drive_kind: DriveKind,
    file_system: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct NetworkCache {
    entries: Vec<NetworkCacheEntry>,
}

/// Loads the last known This PC entries without touching the system's storage
/// APIs. A stale or unreadable cache is ignored and the normal asynchronous
/// refresh will repopulate it.
pub fn load_storage_cache() -> Vec<FileEntry> {
    let Ok(path) = crate::utils::paths::storage_cache_file() else {
        return Vec::new();
    };
    let Ok(bytes) = fs::read(path) else {
        return Vec::new();
    };
    let Ok(cache) = serde_json::from_slice::<StorageCache>(&bytes) else {
        return Vec::new();
    };

    cache
        .entries
        .into_iter()
        .filter(|_entry| {
            #[cfg(all(unix, not(target_os = "macos")))]
            {
                !linux_path_is_firmware_mount(&_entry.path, &_entry.file_system)
            }
            #[cfg(not(all(unix, not(target_os = "macos"))))]
            {
                true
            }
        })
        .map(|entry| FileEntry {
            name: entry.name,
            path: entry.path,
            kind: EntryKind::Drive,
            category: FileCategory::Other,
            drive_kind: Some(entry.drive_kind),
            file_system: entry.file_system,
            free_space: entry.free_space,
            size: entry.size,
            percent_full: entry.percent_full,
            modified: None,
            created: None,
            is_hidden: false,
        })
        .collect()
}

/// Persists the storage entries needed to paint This PC on the next launch.
/// Failures are non-critical and can be ignored by the caller.
pub fn save_storage_cache(entries: &[FileEntry]) -> Result<()> {
    let cache = StorageCache {
        entries: entries
            .iter()
            .filter_map(|entry| {
                Some(StorageCacheEntry {
                    name: entry.name.clone(),
                    path: entry.path.clone(),
                    drive_kind: entry.drive_kind?,
                    file_system: entry.file_system.clone(),
                    free_space: entry.free_space,
                    size: entry.size,
                    percent_full: entry.percent_full,
                })
            })
            .collect(),
    };
    let path = crate::utils::paths::storage_cache_file()?;
    crate::utils::atomic_file::write(&path, &serde_json::to_vec(&cache)?)
}

/// Loads the last discovered network devices without starting a new network
/// scan. This keeps the network root useful immediately while discovery runs.
pub fn load_network_cache() -> Vec<FileEntry> {
    let Ok(path) = crate::utils::paths::network_cache_file() else {
        return Vec::new();
    };
    let Ok(bytes) = fs::read(path) else {
        return Vec::new();
    };
    let Ok(cache) = serde_json::from_slice::<NetworkCache>(&bytes) else {
        return Vec::new();
    };

    cache
        .entries
        .into_iter()
        .map(|entry| FileEntry {
            name: entry.name,
            path: entry.path,
            kind: EntryKind::Drive,
            category: FileCategory::Other,
            drive_kind: Some(entry.drive_kind),
            file_system: entry.file_system,
            free_space: None,
            size: None,
            percent_full: None,
            modified: None,
            created: None,
            is_hidden: false,
        })
        .collect()
}

/// Persists the most recently known network devices. Like the storage cache,
/// this is opportunistic: failing to cache must never interrupt browsing.
pub fn save_network_cache(entries: &[FileEntry]) -> Result<()> {
    let cache = NetworkCache {
        entries: entries
            .iter()
            .filter_map(|entry| {
                Some(NetworkCacheEntry {
                    name: entry.name.clone(),
                    path: entry.path.clone(),
                    drive_kind: entry.drive_kind?,
                    file_system: entry.file_system.clone(),
                })
            })
            .collect(),
    };
    let path = crate::utils::paths::network_cache_file()?;
    crate::utils::atomic_file::write(&path, &serde_json::to_vec(&cache)?)
}

impl FileEntry {
    pub fn type_label(&self) -> String {
        self.drive_kind
            .map(DriveKind::label)
            .map(|l| l.to_string())
            .unwrap_or_else(|| match self.kind {
                EntryKind::File | EntryKind::Other => self.category.label_with(&self.path),
                _ => self.kind.label().to_string(),
            })
    }
}

#[derive(Clone, Debug)]
struct StorageDevice {
    path: PathBuf,
    name: String,
    file_system: String,
    drive_kind: DriveKind,
}

#[cfg(all(unix, not(target_os = "macos")))]
#[derive(Clone, Debug, PartialEq, Eq)]
struct LinuxMount {
    major_minor: String,
    mount_point: PathBuf,
    fs_type: String,
    source: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortableDevice {
    pub id: String,
    pub name: String,
    pub manufacturer: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VirtualLocation {
    NetworkRoot,
    NetworkHost {
        host: String,
    },
    PortableObject {
        device_id: String,
        object_id: String,
    },
}

const VIRTUAL_ROOT: &str = "__bexplorer_virtual__";
const VIRTUAL_NETWORK: &str = "network";
const VIRTUAL_PORTABLE: &str = "portable";
const WPD_ROOT_OBJECT_ID: &str = "DEVICE";
const NETWORK_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);
static NETWORK_DISCOVERY_ACTIVE: AtomicBool = AtomicBool::new(false);
static NETWORK_ROOT_CACHE: OnceLock<Mutex<Vec<FileEntry>>> = OnceLock::new();

#[derive(Clone, Copy, Debug)]
pub enum NetworkDiscoverySource {
    System,
    Fast,
    NetBiosCache,
    Printers,
    FunctionDevices,
    WindowsNetwork,
    Shell,
}

pub const NETWORK_DISCOVERY_SOURCES: &[NetworkDiscoverySource] = &[
    NetworkDiscoverySource::System,
    NetworkDiscoverySource::Fast,
    NetworkDiscoverySource::NetBiosCache,
    NetworkDiscoverySource::Printers,
    NetworkDiscoverySource::FunctionDevices,
    NetworkDiscoverySource::WindowsNetwork,
    NetworkDiscoverySource::Shell,
];

pub fn list_entries(path: Option<&Path>, show_hidden: bool) -> Result<Vec<FileEntry>> {
    match path {
        Some(path) if virtual_location(path).is_some() => list_virtual_entries(path),
        Some(path) if crate::fs::archive_listing::is_archive_navigation_path(path) => {
            crate::fs::archive_listing::list_archive_contents(path)
        }
        Some(path) if is_unc_path(path) => list_unc_directory(path, show_hidden),
        Some(path) => list_directory(path, show_hidden),
        None => list_this_pc_entries(),
    }
}

pub fn is_virtual_path(path: &Path) -> bool {
    virtual_location(path).is_some()
}

pub fn is_network_root_path(path: &Path) -> bool {
    matches!(virtual_location(path), Some(VirtualLocation::NetworkRoot))
}

pub fn is_portable_path(path: &Path) -> bool {
    matches!(
        virtual_location(path),
        Some(VirtualLocation::PortableObject { .. })
    )
}

pub fn is_unc_path(path: &Path) -> bool {
    path.display().to_string().starts_with(r"\\")
}

pub fn portable_object_from_path(path: &Path) -> Option<(String, String)> {
    match virtual_location(path)? {
        VirtualLocation::PortableObject {
            device_id,
            object_id,
        } => Some((device_id, object_id)),
        _ => None,
    }
}

pub fn virtual_display_name(path: &Path) -> Option<String> {
    match virtual_location(path)? {
        VirtualLocation::NetworkRoot => Some("Red".into()),
        VirtualLocation::NetworkHost { host } => Some(host),
        VirtualLocation::PortableObject { .. } => virtual_components(path)
            .last()
            .and_then(|segment| segment_label(segment))
            .or_else(|| Some("Dispositivo".into())),
    }
}

pub fn network_root_path() -> PathBuf {
    PathBuf::from(VIRTUAL_ROOT).join(VIRTUAL_NETWORK)
}

pub fn network_host_path(host: &str) -> PathBuf {
    network_root_path().join(encoded_segment(host, host))
}

pub fn portable_device_path(device_id: &str, name: &str) -> PathBuf {
    PathBuf::from(VIRTUAL_ROOT)
        .join(VIRTUAL_PORTABLE)
        .join(encoded_segment(device_id, name))
}

pub fn virtual_title(path: &Path) -> Option<String> {
    match virtual_location(path)? {
        VirtualLocation::NetworkRoot => Some("Red".into()),
        VirtualLocation::NetworkHost { host } => Some(host),
        VirtualLocation::PortableObject { .. } => virtual_components(path)
            .last()
            .and_then(|segment| segment_label(segment))
            .or_else(|| Some("Dispositivo".into())),
    }
}

pub fn virtual_breadcrumbs(path: &Path) -> Option<Vec<(String, Option<PathBuf>)>> {
    match virtual_location(path)? {
        VirtualLocation::NetworkRoot => Some(vec![
            ("This PC".into(), None),
            ("Red".into(), Some(network_root_path())),
        ]),
        VirtualLocation::NetworkHost { host } => Some(vec![
            ("This PC".into(), None),
            ("Red".into(), Some(network_root_path())),
            (host.clone(), Some(network_host_path(&host))),
        ]),
        VirtualLocation::PortableObject { .. } => {
            let components = virtual_components(path);
            let mut crumbs = vec![("This PC".into(), None)];
            let mut current = PathBuf::from(VIRTUAL_ROOT).join(VIRTUAL_PORTABLE);
            for segment in components.into_iter().skip(2) {
                current = current.join(&segment);
                let label = segment_label(&segment).unwrap_or_else(|| "Dispositivo".into());
                crumbs.push((label, Some(current.clone())));
            }
            Some(crumbs)
        }
    }
}

pub fn unc_breadcrumbs(path: &Path) -> Option<Vec<(String, Option<PathBuf>)>> {
    let display = path.display().to_string();
    let trimmed = display.trim_start_matches('\\');
    if trimmed.len() == display.len() {
        return None;
    }

    let parts: Vec<_> = trimmed
        .split('\\')
        .filter(|part| !part.is_empty())
        .collect();
    if parts.len() < 2 {
        return None;
    }

    let host = parts[0];
    let mut crumbs = vec![
        ("This PC".into(), None),
        ("Red".into(), Some(network_root_path())),
        (host.to_string(), Some(network_host_path(host))),
    ];

    let mut current = PathBuf::from(format!(r"\\{}\{}", host, parts[1]));
    crumbs.push((parts[1].to_string(), Some(current.clone())));
    for part in parts.into_iter().skip(2) {
        current = current.join(part);
        crumbs.push((part.to_string(), Some(current.clone())));
    }
    Some(crumbs)
}

pub fn list_portable_devices_for_storage(storage_entries: &[FileEntry]) -> Vec<PortableDevice> {
    crate::platform::portable_devices()
        .into_iter()
        .map(|device| PortableDevice {
            id: device.id,
            name: device.name,
            manufacturer: device.manufacturer,
            description: device.description,
        })
        .filter(|device| !portable_device_duplicates_storage(device, storage_entries))
        .collect()
}

pub fn list_storage_entries() -> Result<Vec<FileEntry>> {
    let mut entries = Vec::new();

    for device in storage_devices() {
        let total = fs2::total_space(&device.path).ok();
        let free = free_space(&device.path).ok();
        let percent_full = match (total, free) {
            (Some(total), Some(free)) if total > 0 => {
                Some(((total.saturating_sub(free)) as f32 / total as f32).clamp(0.0, 1.0))
            }
            _ => None,
        };

        entries.push(FileEntry {
            name: device.name,
            path: device.path,
            kind: EntryKind::Drive,
            category: FileCategory::Other,
            drive_kind: Some(device.drive_kind),
            file_system: device.file_system,
            free_space: free,
            size: total,
            percent_full,
            modified: None,
            created: None,
            is_hidden: false,
        });
    }

    sort_entries_by_name(&mut entries);
    Ok(entries)
}

pub fn list_this_pc_entries() -> Result<Vec<FileEntry>> {
    let storage = list_storage_entries()?;
    let portable = list_portable_devices_for_storage(&storage);
    Ok(combine_storage_and_portable_entries(&storage, &portable))
}

pub fn combine_storage_and_portable_entries(
    storage_entries: &[FileEntry],
    portable_devices: &[PortableDevice],
) -> Vec<FileEntry> {
    let mut entries = storage_entries.to_vec();
    for device in portable_devices {
        entries.push(portable_device_entry(device));
    }
    sort_entries_by_name(&mut entries);
    entries
}

fn portable_device_duplicates_storage(
    device: &PortableDevice,
    storage_entries: &[FileEntry],
) -> bool {
    let id = device.id.to_ascii_uppercase();
    if portable_device_id_is_mounted_storage(&id) {
        return true;
    }

    if portable_device_name_is_mounted_storage(device, storage_entries) {
        return true;
    }

    let device_name = normalized_storage_label(&device.name);
    if device_name.is_empty() {
        return false;
    }

    storage_entries.iter().any(|entry| {
        entry_is_mounted_storage(entry) && normalized_storage_label(&entry.name) == device_name
    })
}

fn portable_device_name_is_mounted_storage(
    device: &PortableDevice,
    storage_entries: &[FileEntry],
) -> bool {
    let Some(device_letter) = drive_root_letter(&device.name) else {
        return false;
    };

    storage_entries.iter().any(|entry| {
        entry_is_mounted_storage(entry)
            && drive_root_letter(&entry.path.to_string_lossy()) == Some(device_letter)
    })
}

fn drive_root_letter(value: &str) -> Option<u8> {
    let value = value.trim();
    let bytes = value.as_bytes();
    let (&letter, rest) = bytes.split_first()?;
    if !letter.is_ascii_alphabetic() || rest.first() != Some(&b':') {
        return None;
    }
    if rest.len() > 1 && !rest[1..].iter().all(|byte| matches!(byte, b'\\' | b'/')) {
        return None;
    }
    Some(letter.to_ascii_uppercase())
}

fn portable_device_id_is_mounted_storage(id: &str) -> bool {
    const STORAGE_ID_MARKERS: &[&str] = &[
        "USBSTOR",
        "STORAGE#VOLUME",
        "STORAGE#DISK",
        "SCSI#DISK",
        "IDE#DISK",
        "SATA#DISK",
        "NVME",
        "#DISK&VEN_",
        "DISK&VEN_",
    ];

    STORAGE_ID_MARKERS.iter().any(|marker| id.contains(marker))
}

fn entry_is_mounted_storage(entry: &FileEntry) -> bool {
    entry.kind == EntryKind::Drive
        && entry.drive_kind.is_some_and(|kind| {
            !matches!(
                kind,
                DriveKind::Network
                    | DriveKind::NetworkComputer
                    | DriveKind::NetworkPrinter
                    | DriveKind::NetworkScanner
                    | DriveKind::NetworkMultifunction
                    | DriveKind::NetworkDevice
                    | DriveKind::Portable
            )
        })
}

fn normalized_storage_label(label: &str) -> String {
    let trimmed = label.trim();
    let without_drive_suffix = trimmed
        .rsplit_once(" (")
        .and_then(|(prefix, suffix)| {
            let suffix = suffix.strip_suffix(')')?;
            let bytes = suffix.as_bytes();
            if bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
                Some(prefix)
            } else {
                None
            }
        })
        .unwrap_or(trimmed);

    without_drive_suffix
        .trim()
        .to_ascii_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn list_virtual_entries(path: &Path) -> Result<Vec<FileEntry>> {
    let Some(location) = virtual_location(path) else {
        return Ok(Vec::new());
    };

    let mut entries = match location {
        VirtualLocation::NetworkRoot => list_network_computers(),
        VirtualLocation::NetworkHost { host } => list_network_shares(&host),
        VirtualLocation::PortableObject {
            device_id,
            object_id,
        } => list_portable_objects(path, &device_id, &object_id)?,
    };
    sort_entries_by_name(&mut entries);
    Ok(entries)
}

fn list_network_computers() -> Vec<FileEntry> {
    let cached = network_root_cached_entries();
    if NETWORK_DISCOVERY_ACTIVE.swap(true, AtomicOrdering::AcqRel) {
        return cached;
    }

    enum SourceMessage {
        Entries(Vec<FileEntry>),
        Finished,
    }

    let (sender, receiver) = mpsc::channel();
    for source in NETWORK_DISCOVERY_SOURCES {
        let source = *source;
        let sender = sender.clone();
        thread::spawn(move || {
            let entries = list_network_discovery_source_entries(source);
            if !entries.is_empty() {
                let _ = sender.send(SourceMessage::Entries(entries));
            }
            let _ = sender.send(SourceMessage::Finished);
        });
    }

    let sender_for_netbios = sender.clone();
    thread::spawn(move || {
        let addresses = list_network_netbios_neighbor_addresses();
        let (host_sender, host_receiver) = mpsc::channel();
        for address in addresses {
            let host_sender = host_sender.clone();
            thread::spawn(move || {
                let _ = host_sender.send(network_computer_entry_netbios_address(&address));
            });
        }
        drop(host_sender);
        for entry in host_receiver.into_iter().flatten() {
            let _ = sender_for_netbios.send(SourceMessage::Entries(vec![entry]));
        }
        let _ = sender_for_netbios.send(SourceMessage::Finished);
    });
    drop(sender);

    let mut entries = cached;
    let mut pending_sources = 8_usize;
    let deadline = Instant::now() + NETWORK_DISCOVERY_TIMEOUT;
    while pending_sources > 0 && Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(remaining.min(Duration::from_millis(150))) {
            Ok(SourceMessage::Entries(discovered)) => {
                merge_network_entries(&mut entries, discovered);
            }
            Ok(SourceMessage::Finished) => pending_sources = pending_sources.saturating_sub(1),
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    NETWORK_DISCOVERY_ACTIVE.store(false, AtomicOrdering::Release);
    sort_entries_by_name(&mut entries);
    replace_network_root_cache(&entries);
    let _ = save_network_cache(&entries);
    entries
}

fn list_network_computer_entries_default() -> Vec<FileEntry> {
    crate::platform::network_computers()
        .into_iter()
        .map(network_computer_entry)
        .collect()
}

fn network_root_cache() -> &'static Mutex<Vec<FileEntry>> {
    NETWORK_ROOT_CACHE.get_or_init(|| Mutex::new(load_network_cache()))
}

pub fn network_root_cached_entries() -> Vec<FileEntry> {
    network_root_cache()
        .lock()
        .map(|entries| entries.clone())
        .unwrap_or_default()
}

/// Merges a batch found by a network source into the in-memory cache. This is
/// intentionally cheap enough to call for every partial discovery result.
pub fn merge_network_root_cache(entries: &[FileEntry]) -> Vec<FileEntry> {
    let Ok(mut cache) = network_root_cache().lock() else {
        return entries.to_vec();
    };
    merge_network_entries(&mut cache, entries.to_vec());
    sort_entries_by_name(&mut cache);
    cache.clone()
}

fn replace_network_root_cache(entries: &[FileEntry]) {
    if let Ok(mut cache) = network_root_cache().lock() {
        *cache = entries.to_vec();
    }
}

pub fn merge_network_entries(target: &mut Vec<FileEntry>, entries: Vec<FileEntry>) {
    for entry in entries {
        if let Some(existing) = target
            .iter_mut()
            .find(|existing| existing.path == entry.path)
        {
            if network_entry_priority(&entry) >= network_entry_priority(existing) {
                *existing = entry;
            }
        } else {
            target.push(entry);
        }
    }
}

pub fn list_network_discovery_source_entries(source: NetworkDiscoverySource) -> Vec<FileEntry> {
    match source {
        NetworkDiscoverySource::System => list_network_computer_entries_default(),
        NetworkDiscoverySource::Fast => list_network_computer_entries_fast(),
        NetworkDiscoverySource::NetBiosCache => list_network_computer_entries_netbios_cached(),
        NetworkDiscoverySource::Printers => list_network_printer_entries(),
        NetworkDiscoverySource::FunctionDevices => list_network_function_device_entries(),
        NetworkDiscoverySource::WindowsNetwork => list_network_computer_entries_wnet(),
        NetworkDiscoverySource::Shell => list_network_shell_entries(),
    }
}

/// Runs one discovery source behind the same timeout used by the aggregate
/// scanner. A slow network provider must not leave the incremental UI loading
/// forever.
pub fn list_network_discovery_source_entries_timed(
    source: NetworkDiscoverySource,
) -> Vec<FileEntry> {
    network_discovery_with_timeout(move || list_network_discovery_source_entries(source))
        .unwrap_or_default()
}

pub fn list_network_netbios_neighbor_addresses_timed() -> Vec<String> {
    network_discovery_with_timeout(list_network_netbios_neighbor_addresses).unwrap_or_default()
}

pub fn network_computer_entry_netbios_address_timed(address: String) -> Option<FileEntry> {
    network_discovery_with_timeout(move || network_computer_entry_netbios_address(&address))
        .flatten()
}

fn network_discovery_with_timeout<T, F>(operation: F) -> Option<T>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let _ = sender.send(operation());
    });
    receiver.recv_timeout(NETWORK_DISCOVERY_TIMEOUT).ok()
}

fn network_entry_priority(entry: &FileEntry) -> u8 {
    match entry.drive_kind {
        Some(DriveKind::NetworkMultifunction) => 70,
        Some(DriveKind::NetworkPrinter | DriveKind::NetworkScanner) => 65,
        Some(DriveKind::NetworkComputer) => 60,
        Some(DriveKind::NetworkDevice) => 40,
        Some(DriveKind::Network) => 30,
        Some(_) => 20,
        None => 10,
    }
}

pub fn list_network_computer_entries_fast() -> Vec<FileEntry> {
    let mut entries: Vec<FileEntry> = crate::platform::network_computers_fast()
        .into_iter()
        .map(network_computer_entry)
        .collect();
    sort_entries_by_name(&mut entries);
    entries
}

pub fn list_network_computer_entries_netbios_cached() -> Vec<FileEntry> {
    let mut entries: Vec<FileEntry> = crate::platform::network_computers_netbios_cached()
        .into_iter()
        .map(network_computer_entry)
        .collect();
    sort_entries_by_name(&mut entries);
    entries
}

pub fn list_network_netbios_neighbor_addresses() -> Vec<String> {
    crate::platform::network_netbios_neighbor_addresses()
}

pub fn network_computer_entry_netbios_address(address: &str) -> Option<FileEntry> {
    crate::platform::network_computer_netbios_at(address).map(network_computer_entry)
}

pub fn list_network_printer_entries() -> Vec<FileEntry> {
    let mut entries: Vec<FileEntry> = crate::platform::network_printer_devices()
        .into_iter()
        .map(network_computer_entry)
        .collect();
    sort_entries_by_name(&mut entries);
    entries
}

pub fn list_network_function_device_entries() -> Vec<FileEntry> {
    let mut entries: Vec<FileEntry> = crate::platform::network_function_devices()
        .into_iter()
        .map(network_computer_entry)
        .collect();
    sort_entries_by_name(&mut entries);
    entries
}

pub fn list_network_computer_entries_wnet() -> Vec<FileEntry> {
    let mut entries: Vec<FileEntry> = crate::platform::network_computers_wnet()
        .into_iter()
        .map(network_computer_entry)
        .collect();
    sort_entries_by_name(&mut entries);
    entries
}

pub fn list_network_shell_entries() -> Vec<FileEntry> {
    let mut entries: Vec<FileEntry> = crate::platform::network_shell_devices()
        .into_iter()
        .map(network_computer_entry)
        .collect();
    sort_entries_by_name(&mut entries);
    entries
}

fn network_computer_entry(computer: crate::platform::NetworkComputerInfo) -> FileEntry {
    let name = computer.name;
    let drive_kind = match computer.kind {
        crate::platform::NetworkDeviceKind::Computer => DriveKind::NetworkComputer,
        crate::platform::NetworkDeviceKind::Printer => DriveKind::NetworkPrinter,
        crate::platform::NetworkDeviceKind::Scanner => DriveKind::NetworkScanner,
        crate::platform::NetworkDeviceKind::Multifunction => DriveKind::NetworkMultifunction,
        crate::platform::NetworkDeviceKind::Other => DriveKind::NetworkDevice,
    };
    FileEntry {
        path: network_host_path(&name),
        name,
        kind: EntryKind::Drive,
        category: FileCategory::Other,
        drive_kind: Some(drive_kind),
        file_system: if computer.comment.trim().is_empty() {
            drive_kind.label().into()
        } else {
            computer.comment
        },
        free_space: None,
        size: None,
        percent_full: None,
        modified: None,
        created: None,
        is_hidden: false,
    }
}

fn list_network_shares(host: &str) -> Vec<FileEntry> {
    crate::platform::network_shares(host)
        .into_iter()
        .filter(|share| !share.name.ends_with('$'))
        .map(|share| FileEntry {
            name: share.name.clone(),
            path: PathBuf::from(format!(r"\\{}\{}", host, share.name)),
            kind: EntryKind::Drive,
            category: FileCategory::Other,
            drive_kind: Some(DriveKind::Network),
            file_system: "SMB".into(),
            free_space: None,
            size: None,
            percent_full: None,
            modified: None,
            created: None,
            is_hidden: false,
        })
        .collect()
}

fn list_portable_objects(
    parent_path: &Path,
    device_id: &str,
    object_id: &str,
) -> Result<Vec<FileEntry>> {
    Ok(
        crate::platform::portable_device_objects_result(device_id, object_id)?
            .into_iter()
            .map(|object| {
                let name_path = PathBuf::from(&object.name);
                let kind = if object.is_folder {
                    EntryKind::Folder
                } else {
                    EntryKind::File
                };
                FileEntry {
                    path: parent_path.join(encoded_segment(&object.id, &object.name)),
                    name: object.name,
                    kind,
                    category: classify_file_category(&name_path),
                    drive_kind: None,
                    file_system: "MTP".into(),
                    free_space: None,
                    size: object.size,
                    percent_full: None,
                    modified: None,
                    created: None,
                    is_hidden: false,
                }
            })
            .collect(),
    )
}

fn portable_device_entry(device: &PortableDevice) -> FileEntry {
    FileEntry {
        name: device.name.clone(),
        path: portable_device_path(&device.id, &device.name),
        kind: EntryKind::Drive,
        category: FileCategory::Other,
        drive_kind: Some(DriveKind::Portable),
        file_system: "MTP".into(),
        free_space: None,
        size: None,
        percent_full: None,
        modified: None,
        created: None,
        is_hidden: false,
    }
}

fn list_directory(path: &Path, show_hidden: bool) -> Result<Vec<FileEntry>> {
    let mut entries = Vec::new();

    for item in fs::read_dir(path)? {
        let Ok(item) = item else {
            continue;
        };

        let path = item.path();
        let name = item.file_name().to_string_lossy().to_string();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            crate::utils::log::error(format!("Could not read metadata: {}", path.display()));
            continue;
        };
        let is_hidden = is_hidden_entry(&metadata, &name);

        if is_hidden && !show_hidden {
            continue;
        }

        let file_type = metadata.file_type();
        let kind = if file_type.is_symlink() {
            EntryKind::Symlink
        } else if metadata.is_dir() {
            EntryKind::Folder
        } else if metadata.is_file() {
            EntryKind::File
        } else {
            EntryKind::Other
        };

        let category = classify_file_category(&path);

        entries.push(FileEntry {
            name,
            path,
            kind,
            category,
            drive_kind: None,
            file_system: String::new(),
            free_space: None,
            size: if metadata.is_file() {
                Some(metadata.len())
            } else {
                None
            },
            percent_full: None,
            modified: metadata.modified().ok().map(format_system_time),
            created: metadata.created().ok().map(format_system_time),
            is_hidden,
        });
    }

    sort_entries_by_name(&mut entries);
    Ok(entries)
}

fn list_unc_directory(path: &Path, show_hidden: bool) -> Result<Vec<FileEntry>> {
    if let Some(mounted_path) = crate::platform::mounted_network_path(path) {
        return list_directory(&mounted_path, show_hidden);
    }
    match list_directory(path, show_hidden) {
        Err(BExplorerError::Io(error))
            if error.kind() == std::io::ErrorKind::PermissionDenied
                && crate::platform::prompt_network_credentials_for_path(path) =>
        {
            list_directory(path, show_hidden)
        }
        result => result,
    }
}

pub fn sort_entries_by_name(entries: &mut [FileEntry]) {
    entries.sort_by(|left, right| {
        right
            .kind
            .is_container()
            .cmp(&left.kind.is_container())
            .then_with(|| compare_names_case_insensitive(&left.name, &right.name))
    });
}

pub fn compare_names_case_insensitive(left: &str, right: &str) -> std::cmp::Ordering {
    left.chars()
        .flat_map(char::to_lowercase)
        .cmp(right.chars().flat_map(char::to_lowercase))
}

fn virtual_location(path: &Path) -> Option<VirtualLocation> {
    let components = virtual_components(path);
    if components.first().map(String::as_str) != Some(VIRTUAL_ROOT) {
        return None;
    }

    match components.get(1).map(String::as_str) {
        Some(VIRTUAL_NETWORK) if components.len() == 2 => Some(VirtualLocation::NetworkRoot),
        Some(VIRTUAL_NETWORK) => {
            let host = components.get(2).and_then(|segment| segment_id(segment))?;
            Some(VirtualLocation::NetworkHost { host })
        }
        Some(VIRTUAL_PORTABLE) if components.len() >= 3 => {
            let device_id = components.get(2).and_then(|segment| segment_id(segment))?;
            let object_id = if components.len() > 3 {
                components
                    .last()
                    .and_then(|segment| segment_id(segment))
                    .unwrap_or_else(|| WPD_ROOT_OBJECT_ID.to_string())
            } else {
                WPD_ROOT_OBJECT_ID.to_string()
            };
            Some(VirtualLocation::PortableObject {
                device_id,
                object_id,
            })
        }
        _ => None,
    }
}

fn virtual_components(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => Some(value.to_string_lossy().replace('\\', "/")),
            _ => None,
        })
        .collect()
}

fn encoded_segment(id: &str, label: &str) -> String {
    format!("{}~{}", hex_encode(id), hex_encode(label))
}

fn segment_id(segment: &str) -> Option<String> {
    let id = segment.split_once('~').map(|(id, _)| id).unwrap_or(segment);
    hex_decode(id)
}

fn segment_label(segment: &str) -> Option<String> {
    let (_, label) = segment.split_once('~')?;
    hex_decode(label).filter(|label| !label.trim().is_empty())
}

fn hex_encode(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(value.len() * 2);
    for byte in value.as_bytes() {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn hex_decode(value: &str) -> Option<String> {
    if !value.len().is_multiple_of(2) {
        return None;
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks_exact(2) {
        let high = hex_value(pair[0])?;
        let low = hex_value(pair[1])?;
        bytes.push((high << 4) | low);
    }
    String::from_utf8(bytes).ok()
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

// Platform storage enumeration is isolated physically while sharing private parsing types.
include!("explorer/storage.rs");

fn format_system_time(time: SystemTime) -> String {
    let datetime: chrono::DateTime<chrono::Local> = time.into();
    datetime.format("%Y-%m-%d %H:%M").to_string()
}

#[cfg(target_os = "windows")]
fn is_hidden_entry(metadata: &fs::Metadata, name: &str) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    if name.starts_with('.') {
        return true;
    }

    metadata.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0
}

#[cfg(not(target_os = "windows"))]
fn is_hidden_entry(_metadata: &fs::Metadata, name: &str) -> bool {
    name.starts_with('.')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn storage_entry(name: &str, drive_kind: DriveKind) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            path: PathBuf::from("F:\\"),
            kind: EntryKind::Drive,
            category: FileCategory::Other,
            drive_kind: Some(drive_kind),
            file_system: "exFAT".into(),
            free_space: None,
            size: None,
            percent_full: None,
            modified: None,
            created: None,
            is_hidden: false,
        }
    }

    #[test]
    fn filters_portable_duplicate_for_mounted_usb_label() {
        let storage = [storage_entry("test (F:)", DriveKind::Usb)];
        let device = PortableDevice {
            id: "SWD\\WPDBUSENUM\\anything".into(),
            name: "test".into(),
            manufacturer: String::new(),
            description: String::new(),
        };

        assert!(portable_device_duplicates_storage(&device, &storage));
    }

    #[test]
    fn filters_portable_duplicate_for_usbstor_id() {
        let storage = [storage_entry("Backup (F:)", DriveKind::Usb)];
        let device = PortableDevice {
            id: "SWD\\WPDBUSENUM\\_??_USBSTOR#DISK&VEN_TEST".into(),
            name: "Different label".into(),
            manufacturer: String::new(),
            description: String::new(),
        };

        assert!(portable_device_duplicates_storage(&device, &storage));
    }

    #[test]
    fn filters_portable_duplicate_for_internal_disk_id() {
        let storage = [storage_entry("Data (D:)", DriveKind::Local)];
        let device = PortableDevice {
            id: "SWD\\WPDBUSENUM\\_??_SCSI#DISK&VEN_SAMSUNG&PROD_SSD".into(),
            name: "Samsung SSD".into(),
            manufacturer: String::new(),
            description: String::new(),
        };

        assert!(portable_device_duplicates_storage(&device, &storage));
    }

    #[test]
    fn filters_portable_duplicate_for_local_storage_label() {
        let storage = [storage_entry("NEXTCLOUD (D:)", DriveKind::Local)];
        let device = PortableDevice {
            id: "SWD\\WPDBUSENUM\\volume-like-device".into(),
            name: "NEXTCLOUD".into(),
            manufacturer: String::new(),
            description: String::new(),
        };

        assert!(portable_device_duplicates_storage(&device, &storage));
    }

    #[test]
    fn filters_portable_duplicate_named_as_mounted_drive_root() {
        let mut entry = storage_entry("Local Disk (E:)", DriveKind::Local);
        entry.path = PathBuf::from("E:\\");
        let storage = [entry];
        let device = PortableDevice {
            id: "SWD\\WPDBUSENUM\\volume-like-device".into(),
            name: "E:\\".into(),
            manufacturer: String::new(),
            description: String::new(),
        };

        assert!(portable_device_duplicates_storage(&device, &storage));
    }

    #[test]
    fn keeps_distinct_portable_device() {
        let storage = [storage_entry("test (F:)", DriveKind::Usb)];
        let device = PortableDevice {
            id: "MTP\\PHONE".into(),
            name: "Pixel".into(),
            manufacturer: "Google".into(),
            description: "Phone".into(),
        };

        assert!(!portable_device_duplicates_storage(&device, &storage));
    }

    #[test]
    fn merges_network_sources_without_duplicates_and_keeps_richer_kind() {
        let mut entries = vec![storage_entry("Office host", DriveKind::NetworkComputer)];
        let mut printer = storage_entry("Office printer", DriveKind::NetworkPrinter);
        printer.path = entries[0].path.clone();

        merge_network_entries(&mut entries, vec![printer]);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "Office printer");
        assert_eq!(entries[0].drive_kind, Some(DriveKind::NetworkPrinter));
    }

    #[test]
    fn recognizes_unc_paths() {
        assert!(is_unc_path(Path::new(r"\\SERVER\Share")));
        assert!(!is_unc_path(Path::new(r"C:\Users")));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn parses_linux_mountinfo_escaped_mount_path() {
        let text = "36 25 8:1 / /media/dev/My\\040Disk rw,relatime - ext4 /dev/sdb1 rw";
        let mounts = linux_mounts_from_mountinfo(text);

        assert_eq!(
            mounts,
            vec![LinuxMount {
                major_minor: "8:1".into(),
                mount_point: PathBuf::from("/media/dev/My Disk"),
                fs_type: "ext4".into(),
                source: "/dev/sdb1".into(),
            }]
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn filters_linux_virtual_mounts_but_keeps_network_mounts() {
        let proc_mount = LinuxMount {
            major_minor: "0:4".into(),
            mount_point: PathBuf::from("/proc"),
            fs_type: "proc".into(),
            source: "proc".into(),
        };
        let smb_mount = LinuxMount {
            major_minor: "0:55".into(),
            mount_point: PathBuf::from("/mnt/share"),
            fs_type: "cifs".into(),
            source: "//server/share".into(),
        };

        assert!(!linux_mount_is_storage_candidate(&proc_mount));
        assert!(linux_mount_is_storage_candidate(&smb_mount));
        assert_eq!(linux_drive_kind(&smb_mount), DriveKind::Network);
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn linux_allows_formatting_secondary_local_disks_but_not_system_mounts() {
        let system = LinuxMount {
            major_minor: "8:1".into(),
            mount_point: PathBuf::from("/"),
            fs_type: "ext4".into(),
            source: "/dev/sda1".into(),
        };
        let secondary = LinuxMount {
            major_minor: "999:998".into(),
            mount_point: PathBuf::from("/media/dev/PRUEBAS"),
            fs_type: "ext4".into(),
            source: "/dev/test-data".into(),
        };

        assert_eq!(linux_drive_kind(&system), DriveKind::System);
        assert!(!linux_drive_kind(&system).is_formatable());
        assert_eq!(linux_drive_kind(&secondary), DriveKind::Local);
        assert!(linux_drive_kind(&secondary).is_formatable());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn filters_linux_firmware_partitions_but_keeps_regular_vfat_storage() {
        let mounted_esp = LinuxMount {
            major_minor: "259:1".into(),
            mount_point: PathBuf::from("/boot/efi"),
            fs_type: "vfat".into(),
            source: "/dev/nvme0n1p1".into(),
        };
        let auto_mounted_esp = LinuxMount {
            major_minor: "259:1".into(),
            mount_point: PathBuf::from("/media/dev/EFI"),
            fs_type: "vfat".into(),
            source: "/dev/nvme0n1p1".into(),
        };
        let ubuntu_firmware = LinuxMount {
            major_minor: "179:1".into(),
            mount_point: PathBuf::from("/boot/firmware"),
            fs_type: "vfat".into(),
            source: "/dev/mmcblk0p1".into(),
        };
        let usb = LinuxMount {
            major_minor: "8:17".into(),
            mount_point: PathBuf::from("/media/dev/CAMERA"),
            fs_type: "vfat".into(),
            source: "/dev/sdb1".into(),
        };

        assert!(!linux_mount_is_storage_candidate_with_partition_type(
            &mounted_esp,
            None,
        ));
        assert!(!linux_mount_is_storage_candidate_with_partition_type(
            &auto_mounted_esp,
            Some("c12a7328-f81f-11d2-ba4b-00a0c93ec93b"),
        ));
        assert!(!linux_mount_is_storage_candidate_with_partition_type(
            &ubuntu_firmware,
            None,
        ));
        assert!(linux_mount_is_storage_candidate_with_partition_type(
            &usb,
            Some("0x0c"),
        ));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn classifies_linux_iso_filesystems_as_optical() {
        let mount = LinuxMount {
            major_minor: "7:0".into(),
            mount_point: PathBuf::from("/mnt/iso"),
            fs_type: "iso9660".into(),
            source: "/dev/loop0".into(),
        };

        assert_eq!(linux_drive_kind(&mount), DriveKind::Optical);
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn classifies_loop_partitions_as_ejectable_images() {
        let mount = LinuxMount {
            major_minor: "999:999".into(),
            mount_point: PathBuf::from("/media/dev/resizeme"),
            fs_type: "ext2".into(),
            source: "/dev/loop7p2".into(),
        };

        assert_eq!(linux_drive_kind(&mount), DriveKind::DiskImage);
        assert!(linux_drive_kind(&mount).is_ejectable());
        assert!(!linux_drive_kind(&mount).is_formatable());
    }
}
