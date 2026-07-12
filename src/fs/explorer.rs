use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use fs2::free_space;

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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriveKind {
    Local,
    External,
    Usb,
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
            Self::Local => "Local Disk",
            Self::External => "External Drive",
            Self::Usb => "USB Drive",
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
        matches!(self, Self::External | Self::Usb | Self::Optical)
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
const NETWORK_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(8);
static NETWORK_DISCOVERY_ACTIVE: AtomicBool = AtomicBool::new(false);
static NETWORK_ROOT_CACHE: OnceLock<Mutex<Vec<FileEntry>>> = OnceLock::new();

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

    let device_name = normalized_storage_label(&device.name);
    if device_name.is_empty() {
        return false;
    }

    storage_entries.iter().any(|entry| {
        entry_is_mounted_storage(entry) && normalized_storage_label(&entry.name) == device_name
    })
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
    let sources: [fn() -> Vec<FileEntry>; 7] = [
        list_network_computer_entries_default,
        list_network_computer_entries_fast,
        list_network_computer_entries_netbios_cached,
        list_network_printer_entries,
        list_network_function_device_entries,
        list_network_computer_entries_wnet,
        list_network_shell_entries,
    ];
    for source in sources {
        let sender = sender.clone();
        thread::spawn(move || {
            let entries = source();
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
    if let Ok(mut cache) = network_root_cache().lock() {
        *cache = entries.clone();
    }
    entries
}

fn list_network_computer_entries_default() -> Vec<FileEntry> {
    crate::platform::network_computers()
        .into_iter()
        .map(network_computer_entry)
        .collect()
}

fn network_root_cache() -> &'static Mutex<Vec<FileEntry>> {
    NETWORK_ROOT_CACHE.get_or_init(|| Mutex::new(Vec::new()))
}

fn network_root_cached_entries() -> Vec<FileEntry> {
    network_root_cache()
        .lock()
        .map(|entries| entries.clone())
        .unwrap_or_default()
}

fn merge_network_entries(target: &mut Vec<FileEntry>, entries: Vec<FileEntry>) {
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
    if value.len() % 2 != 0 {
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

#[allow(dead_code)]
pub fn storage_roots() -> Vec<PathBuf> {
    let roots = storage_devices()
        .into_iter()
        .map(|device| device.path)
        .collect::<Vec<_>>();
    let mut seen = BTreeSet::new();
    roots
        .into_iter()
        .filter(|path| path.exists())
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

#[cfg(target_os = "windows")]
fn storage_devices() -> Vec<StorageDevice> {
    ('A'..='Z')
        .map(|letter| PathBuf::from(format!("{letter}:\\")))
        .filter(|path| path.exists())
        .map(windows_storage_device)
        .collect()
}

#[cfg(target_os = "windows")]
fn windows_storage_device(path: PathBuf) -> StorageDevice {
    let info = crate::platform::drive_info(&path);
    let drive_kind = match info.kind {
        crate::platform::DriveKind::Removable => DriveKind::Usb,
        crate::platform::DriveKind::Remote => DriveKind::Network,
        crate::platform::DriveKind::CdRom => DriveKind::Optical,
        crate::platform::DriveKind::RamDisk => DriveKind::RamDisk,
        crate::platform::DriveKind::Unknown | crate::platform::DriveKind::NoRootDir => {
            DriveKind::Unknown
        }
        crate::platform::DriveKind::Fixed => DriveKind::Local,
    };
    let letter = drive_letter(&path).unwrap_or('?');
    let name = info
        .volume_label
        .filter(|label| !label.trim().is_empty())
        .map(|label| format!("{label} ({letter}:)"))
        .unwrap_or_else(|| format!("{} ({letter}:)", drive_kind.label()));

    StorageDevice {
        path,
        name,
        file_system: info.file_system.unwrap_or_default(),
        drive_kind,
    }
}

#[cfg(target_os = "macos")]
fn storage_devices() -> Vec<StorageDevice> {
    let mut roots = vec![storage_device_from_path(PathBuf::from("/"))];
    if let Ok(volumes) = fs::read_dir("/Volumes") {
        for volume in volumes.flatten() {
            let path = volume.path();
            if path
                .canonicalize()
                .is_ok_and(|canonical| canonical == Path::new("/"))
            {
                continue;
            }
            let mut device = storage_device_from_path(path);
            device.drive_kind = DriveKind::External;
            roots.push(device);
        }
    }
    roots
}

#[cfg(all(unix, not(target_os = "macos")))]
fn storage_devices() -> Vec<StorageDevice> {
    let mountinfo = fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
    let mounts = linux_mounts_from_mountinfo(&mountinfo);
    let mut devices = Vec::new();
    let mut seen = BTreeSet::new();

    for mount in mounts {
        if !linux_mount_is_storage_candidate(&mount) {
            continue;
        }
        if !mount.mount_point.exists() || !seen.insert(mount.mount_point.clone()) {
            continue;
        }
        devices.push(linux_storage_device_from_mount(&mount));
    }

    for device in linux_gvfs_portable_devices() {
        if device.path.exists() && seen.insert(device.path.clone()) {
            devices.push(device);
        }
    }

    if !devices.iter().any(|device| device.path == Path::new("/")) {
        devices.insert(0, storage_device_from_path(PathBuf::from("/")));
    }

    devices
}

#[cfg(not(any(target_os = "windows", unix)))]
fn storage_devices() -> Vec<StorageDevice> {
    vec![storage_device_from_path(PathBuf::from("/"))]
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_mounts_from_mountinfo(text: &str) -> Vec<LinuxMount> {
    text.lines()
        .filter_map(parse_linux_mountinfo_line)
        .collect()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn parse_linux_mountinfo_line(line: &str) -> Option<LinuxMount> {
    let (before, after) = line.split_once(" - ")?;
    let before_fields = before.split_whitespace().collect::<Vec<_>>();
    let after_fields = after.split_whitespace().collect::<Vec<_>>();
    if before_fields.len() < 5 || after_fields.len() < 2 {
        return None;
    }

    Some(LinuxMount {
        major_minor: before_fields[2].to_string(),
        mount_point: PathBuf::from(decode_linux_mount_field(before_fields[4])),
        fs_type: decode_linux_mount_field(after_fields[0]),
        source: decode_linux_mount_field(after_fields[1]),
    })
}

#[cfg(all(unix, not(target_os = "macos")))]
fn decode_linux_mount_field(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 3 < bytes.len() {
            let digits = &bytes[index + 1..index + 4];
            if digits.iter().all(|byte| matches!(byte, b'0'..=b'7')) {
                let value = (digits[0] - b'0') * 64 + (digits[1] - b'0') * 8 + digits[2] - b'0';
                output.push(value as char);
                index += 4;
                continue;
            }
        }
        output.push(bytes[index] as char);
        index += 1;
    }

    output
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_mount_is_storage_candidate(mount: &LinuxMount) -> bool {
    if mount.mount_point == Path::new("/") {
        return true;
    }

    if linux_mount_point_is_runtime_noise(&mount.mount_point) {
        return false;
    }

    let fs_type = mount.fs_type.to_ascii_lowercase();
    if linux_fs_type_is_virtual(&fs_type) {
        return false;
    }

    linux_fs_type_is_network(&fs_type)
        || mount.source.starts_with("/dev/")
        || fs_type == "fuseblk"
        || mount.mount_point.starts_with("/media")
        || mount.mount_point.starts_with("/mnt")
        || mount.mount_point.starts_with("/run/media")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_mount_point_is_runtime_noise(path: &Path) -> bool {
    path.starts_with("/dev")
        || path.starts_with("/proc")
        || path.starts_with("/sys")
        || path.starts_with("/run/user")
        || path.starts_with("/snap")
        || path.starts_with("/var/lib/snapd")
        || path.starts_with("/var/lib/docker")
        || path.starts_with("/var/lib/containers")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_fs_type_is_virtual(fs_type: &str) -> bool {
    matches!(
        fs_type,
        "autofs"
            | "binfmt_misc"
            | "bpf"
            | "cgroup"
            | "cgroup2"
            | "configfs"
            | "debugfs"
            | "devpts"
            | "devtmpfs"
            | "efivarfs"
            | "fusectl"
            | "hugetlbfs"
            | "mqueue"
            | "nsfs"
            | "overlay"
            | "proc"
            | "pstore"
            | "ramfs"
            | "rpc_pipefs"
            | "securityfs"
            | "squashfs"
            | "sysfs"
            | "tmpfs"
            | "tracefs"
    )
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_fs_type_is_network(fs_type: &str) -> bool {
    matches!(
        fs_type,
        "9p" | "afs"
            | "cifs"
            | "davfs"
            | "fuse.rclone"
            | "fuse.sshfs"
            | "nfs"
            | "nfs4"
            | "smb3"
            | "sshfs"
    )
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_storage_device_from_mount(mount: &LinuxMount) -> StorageDevice {
    let drive_kind = linux_drive_kind(mount);
    let name = linux_mount_label(&mount.mount_point);

    StorageDevice {
        path: mount.mount_point.clone(),
        name,
        file_system: mount.fs_type.clone(),
        drive_kind,
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_gvfs_portable_devices() -> Vec<StorageDevice> {
    let Some(runtime_dir) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) else {
        return Vec::new();
    };
    let gvfs = runtime_dir.join("gvfs");
    let Ok(entries) = fs::read_dir(gvfs) else {
        return Vec::new();
    };

    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_dir() {
                return None;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let (prefix, label) = name.split_once(':')?;
            if !matches!(prefix, "mtp" | "gphoto2" | "afc") {
                return None;
            }
            Some(StorageDevice {
                path,
                name: linux_gvfs_device_label(prefix, label),
                file_system: "GVfs".into(),
                drive_kind: DriveKind::Portable,
            })
        })
        .collect()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_gvfs_device_label(prefix: &str, label: &str) -> String {
    let decoded = percent_decode_utf8(label).unwrap_or_else(|| label.to_string());
    let readable = decoded
        .replace("host=", "")
        .replace(['[', ']'], "")
        .replace(',', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let readable = readable.trim();
    if readable.is_empty() {
        match prefix {
            "mtp" => "MTP Device".into(),
            "gphoto2" => "Camera".into(),
            "afc" => "iOS Device".into(),
            _ => "Portable Device".into(),
        }
    } else {
        format!(
            "{} ({readable})",
            match prefix {
                "mtp" => "MTP Device",
                "gphoto2" => "Camera",
                "afc" => "iOS Device",
                _ => "Portable Device",
            }
        )
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn percent_decode_utf8(value: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(value.len());
    let input = value.as_bytes();
    let mut index = 0;
    while index < input.len() {
        if input[index] == b'%' {
            let high = *input.get(index + 1)?;
            let low = *input.get(index + 2)?;
            bytes.push((hex_value(high)? << 4) | hex_value(low)?);
            index += 3;
        } else {
            bytes.push(input[index]);
            index += 1;
        }
    }
    String::from_utf8(bytes).ok()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_mount_label(path: &Path) -> String {
    if path == Path::new("/") {
        return "Filesystem".into();
    }

    path.file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_drive_kind(mount: &LinuxMount) -> DriveKind {
    let fs_type = mount.fs_type.to_ascii_lowercase();
    if linux_fs_type_is_network(&fs_type) {
        return DriveKind::Network;
    }
    if matches!(fs_type.as_str(), "iso9660" | "udf") || linux_mount_source_is_optical(mount) {
        return DriveKind::Optical;
    }
    if matches!(fs_type.as_str(), "ramfs" | "tmpfs") {
        return DriveKind::RamDisk;
    }
    if linux_mount_source_is_removable(mount) {
        return DriveKind::Usb;
    }
    DriveKind::Local
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_mount_source_is_removable(mount: &LinuxMount) -> bool {
    linux_mount_block_name(mount).is_some_and(|name| linux_block_flag_is_one(&name, "removable"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_mount_source_is_optical(mount: &LinuxMount) -> bool {
    linux_mount_block_name(mount)
        .and_then(|name| linux_block_value(&name, "device/type"))
        .is_some_and(|value| value.trim() == "5")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_mount_block_name(mount: &LinuxMount) -> Option<String> {
    linux_block_name_from_major_minor(&mount.major_minor)
        .or_else(|| linux_block_name_from_source(&mount.source))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_block_name_from_major_minor(major_minor: &str) -> Option<String> {
    let path = Path::new("/sys/dev/block").join(major_minor);
    fs::canonicalize(path).ok().and_then(|path| {
        path.file_name()
            .map(|value| value.to_string_lossy().to_string())
    })
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_block_name_from_source(source: &str) -> Option<String> {
    if !source.starts_with("/dev/") {
        return None;
    }

    let path = Path::new(source);
    fs::canonicalize(path)
        .ok()
        .as_deref()
        .unwrap_or(path)
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_block_flag_is_one(block_name: &str, file_name: &str) -> bool {
    linux_block_value(block_name, file_name).is_some_and(|value| value.trim() == "1")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_block_value(block_name: &str, file_name: &str) -> Option<String> {
    let class_path = Path::new("/sys/class/block").join(block_name);
    let direct = fs::read_to_string(class_path.join(file_name)).ok();
    if direct.is_some() {
        return direct;
    }

    let canonical = fs::canonicalize(class_path).ok()?;
    for ancestor in canonical.ancestors().skip(1) {
        if let Ok(value) = fs::read_to_string(ancestor.join(file_name)) {
            return Some(value);
        }
    }
    None
}

#[cfg(not(target_os = "windows"))]
fn storage_device_from_path(path: PathBuf) -> StorageDevice {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| path.display().to_string());

    StorageDevice {
        path,
        name,
        file_system: String::new(),
        drive_kind: DriveKind::Local,
    }
}

#[cfg(target_os = "windows")]
fn drive_letter(path: &Path) -> Option<char> {
    path.display().to_string().chars().next()
}

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
    fn classifies_linux_iso_filesystems_as_optical() {
        let mount = LinuxMount {
            major_minor: "7:0".into(),
            mount_point: PathBuf::from("/mnt/iso"),
            fs_type: "iso9660".into(),
            source: "/dev/loop0".into(),
        };

        assert_eq!(linux_drive_kind(&mount), DriveKind::Optical);
    }
}
