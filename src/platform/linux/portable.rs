use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::Duration;

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{Fd, OwnedObjectPath};

use crate::platform::{PortableDeviceInfo, PortableObjectInfo};
use crate::utils::errors::{BExplorerError, Result};

const KIO_MTP_SERVICE: &str = "org.kde.kmtpd5";
const KIO_MTP_DAEMON_PATH: &str = "/modules/kmtpd";
const KIO_MTP_DAEMON_INTERFACE: &str = "org.kde.kmtp.Daemon";
const KIO_MTP_DEVICE_INTERFACE: &str = "org.kde.kmtp.Device";
const KIO_MTP_STORAGE_INTERFACE: &str = "org.kde.kmtp.Storage";
const KIO_DEVICE_PREFIX: &str = "linux-kio-mtp:";
const KIO_OBJECT_PREFIX: &str = "kio:";
const GVFS_DEVICE_PREFIX: &str = "linux-gvfs-mtp:";
const GVFS_OBJECT_PREFIX: &str = "gvfs:";
const PORTABLE_ROOT_OBJECT_ID: &str = "DEVICE";

type KmtpFile = (u32, u32, u32, String, u64, i64, String);

#[derive(Clone, Debug, PartialEq, Eq)]
struct GvfsMtpVolume {
    name: String,
    uri: String,
    icon_names: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct KioObject {
    storage_path: String,
    remote_path: String,
}

static KIO_MTP_ACCESS: OnceLock<Mutex<()>> = OnceLock::new();
static GVFS_PORTABLE_ICON_NAMES: OnceLock<Mutex<HashMap<String, Vec<String>>>> = OnceLock::new();
static THUMBNAIL_SEQUENCE: AtomicU64 = AtomicU64::new(1);

pub fn portable_devices() -> Vec<PortableDeviceInfo> {
    let gvfs_volumes = gvfs_mtp_volumes();
    remember_gvfs_portable_icon_names(&gvfs_volumes);
    if !gvfs_volumes.is_empty() {
        return gvfs_portable_devices(&gvfs_volumes);
    }

    kio_portable_devices()
}

pub(super) fn portable_device_icon_names(
    device_id: Option<&str>,
    mount_path: &Path,
) -> Vec<String> {
    let mut candidates = if device_id.is_some_and(|id| id.starts_with(KIO_DEVICE_PREFIX)) {
        vec!["multimedia-player".into()]
    } else if let Some(device_id) = device_id.filter(|id| id.starts_with(GVFS_DEVICE_PREFIX)) {
        cached_gvfs_icon_names(device_id)
    } else {
        gvfs_mount_icon_names(mount_path)
    };

    let gvfs_device = device_id.is_some_and(|id| id.starts_with(GVFS_DEVICE_PREFIX))
        || gvfs_mount_kind(mount_path).is_some();
    let kde_device = device_id.is_some_and(|id| id.starts_with(KIO_DEVICE_PREFIX));
    if kde_device {
        append_unique(&mut candidates, ["smartphone", "phone", "phone-symbolic"]);
    } else if gvfs_device {
        match gvfs_mount_kind(mount_path) {
            Some("gphoto2") => append_unique(
                &mut candidates,
                [
                    "camera-photo",
                    "camera-photo-symbolic",
                    "multimedia-player",
                    "phone",
                ],
            ),
            Some("afc") => append_unique(
                &mut candidates,
                [
                    "phone-apple-iphone",
                    "phone",
                    "phone-symbolic",
                    "smartphone",
                    "multimedia-player",
                ],
            ),
            _ => append_unique(
                &mut candidates,
                ["phone", "phone-symbolic", "smartphone", "multimedia-player"],
            ),
        }
    } else if is_kde_session() {
        append_unique(
            &mut candidates,
            ["multimedia-player", "smartphone", "phone", "phone-symbolic"],
        );
    } else {
        append_unique(
            &mut candidates,
            ["phone", "phone-symbolic", "smartphone", "multimedia-player"],
        );
    }
    candidates
}

pub fn portable_device_objects_result(
    device_id: &str,
    parent_object_id: &str,
) -> Result<Vec<PortableObjectInfo>> {
    if let Some(device_path) = device_id.strip_prefix(KIO_DEVICE_PREFIX) {
        return kio_device_objects(device_path, parent_object_id);
    }
    if let Some(uri) = device_id.strip_prefix(GVFS_DEVICE_PREFIX) {
        return gvfs_device_objects(uri, parent_object_id);
    }
    Err(unsupported_device(device_id))
}

pub fn portable_device_object_info(device_id: &str, object_id: &str) -> Result<PortableObjectInfo> {
    if let Some(device_path) = device_id.strip_prefix(KIO_DEVICE_PREFIX) {
        return kio_object_info(device_path, object_id);
    }
    if let Some(uri) = device_id.strip_prefix(GVFS_DEVICE_PREFIX) {
        return gvfs_object_info(uri, object_id);
    }
    Err(unsupported_device(device_id))
}

pub fn portable_delete_objects(device_id: &str, object_ids: &[String]) -> Result<usize> {
    if let Some(device_path) = device_id.strip_prefix(KIO_DEVICE_PREFIX) {
        return kio_delete_objects(device_path, object_ids);
    }
    if let Some(uri) = device_id.strip_prefix(GVFS_DEVICE_PREFIX) {
        return gvfs_delete_objects(uri, object_ids);
    }
    Err(unsupported_device(device_id))
}

pub fn portable_download_file(device_id: &str, object_id: &str, target: &Path) -> Result<u64> {
    if let Some(device_path) = device_id.strip_prefix(KIO_DEVICE_PREFIX) {
        return kio_download_file(device_path, object_id, target);
    }
    if let Some(uri) = device_id.strip_prefix(GVFS_DEVICE_PREFIX) {
        let source = gvfs_object_path(uri, object_id)?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        return fs::copy(source, target).map_err(Into::into);
    }
    Err(unsupported_device(device_id))
}

pub fn portable_upload_file(
    device_id: &str,
    parent_object_id: &str,
    source: &Path,
    name: &str,
) -> Result<String> {
    if let Some(device_path) = device_id.strip_prefix(KIO_DEVICE_PREFIX) {
        return kio_upload_file(device_path, parent_object_id, source, name);
    }
    if let Some(uri) = device_id.strip_prefix(GVFS_DEVICE_PREFIX) {
        let parent = gvfs_object_path(uri, parent_object_id)?;
        let target = parent.join(valid_child_name(name)?);
        fs::copy(source, &target)?;
        let relative = gvfs_relative_from_path(uri, &target)?;
        return Ok(encode_gvfs_object(&relative));
    }
    Err(unsupported_device(device_id))
}

pub fn portable_create_folder(
    device_id: &str,
    parent_object_id: &str,
    name: &str,
) -> Result<String> {
    if let Some(device_path) = device_id.strip_prefix(KIO_DEVICE_PREFIX) {
        return kio_create_folder(device_path, parent_object_id, name);
    }
    if let Some(uri) = device_id.strip_prefix(GVFS_DEVICE_PREFIX) {
        let parent = gvfs_object_path(uri, parent_object_id)?;
        let target = parent.join(valid_child_name(name)?);
        fs::create_dir(&target)?;
        let relative = gvfs_relative_from_path(uri, &target)?;
        return Ok(encode_gvfs_object(&relative));
    }
    Err(unsupported_device(device_id))
}

pub fn portable_device_thumbnail(
    device_id: &str,
    object_id: &str,
    max_bytes: usize,
    allow_default_resource: bool,
) -> Option<Vec<u8>> {
    if !allow_default_resource || max_bytes == 0 {
        return None;
    }
    let info = portable_device_object_info(device_id, object_id).ok()?;
    if info.is_folder || info.size.is_none_or(|size| size > max_bytes as u64) {
        return None;
    }

    let sequence = THUMBNAIL_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "bexplorer-mtp-thumbnail-{}-{sequence}",
        std::process::id()
    ));
    let cleanup = TemporaryDownload(path.clone());
    portable_download_file(device_id, object_id, &path).ok()?;

    let capacity = info.size.unwrap_or_default().min(max_bytes as u64) as usize;
    let mut bytes = Vec::with_capacity(capacity);
    File::open(&cleanup.0)
        .ok()?
        .take(max_bytes.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() <= max_bytes).then_some(bytes)
}

fn kio_portable_devices() -> Vec<PortableDeviceInfo> {
    let _access = kio_access_lock();
    let Some(connection) = Connection::session().ok() else {
        return Vec::new();
    };
    let Some(daemon) = kio_proxy(&connection, KIO_MTP_DAEMON_PATH, KIO_MTP_DAEMON_INTERFACE).ok()
    else {
        return Vec::new();
    };
    let Some(paths) = daemon
        .call::<_, _, Vec<OwnedObjectPath>>("listDevices", &())
        .ok()
    else {
        return Vec::new();
    };

    let mut devices = paths
        .into_iter()
        .filter_map(|path| {
            let path = path.to_string();
            let proxy = kio_proxy(&connection, &path, KIO_MTP_DEVICE_INTERFACE).ok()?;
            let name = proxy
                .get_property::<String>("friendlyName")
                .ok()
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| "MTP device".into());
            Some(PortableDeviceInfo {
                id: format!("{KIO_DEVICE_PREFIX}{path}"),
                name,
                manufacturer: String::new(),
                description: "KDE MTP".into(),
            })
        })
        .collect::<Vec<_>>();
    devices.sort_by_key(|device| device.name.to_lowercase());
    devices.dedup_by(|left, right| left.id == right.id);
    devices
}

fn kio_device_objects(
    device_path: &str,
    parent_object_id: &str,
) -> Result<Vec<PortableObjectInfo>> {
    let _access = kio_access_lock();
    let connection = kio_connection()?;
    if parent_object_id == PORTABLE_ROOT_OBJECT_ID {
        let device = kio_proxy(&connection, device_path, KIO_MTP_DEVICE_INTERFACE)
            .map_err(|error| kio_error("Could not open the MTP device", error))?;
        let storages: Vec<OwnedObjectPath> = device
            .call("listStorages", &())
            .map_err(|error| kio_error("Could not list the MTP device storage", error))?;
        if storages.is_empty() {
            return Err(BExplorerError::Operation(
                "Could not access the device. Unlock it, enable file transfer, and accept the access request on its screen.".into(),
            ));
        }
        return storages
            .into_iter()
            .map(|path| {
                let path = path.to_string();
                let storage = kio_proxy(&connection, &path, KIO_MTP_STORAGE_INTERFACE)
                    .map_err(|error| kio_error("Could not open MTP storage", error))?;
                let name = storage
                    .get_property::<String>("description")
                    .unwrap_or_else(|_| "Internal storage".into());
                Ok(PortableObjectInfo {
                    id: encode_kio_object(&path, "/"),
                    name,
                    is_folder: true,
                    size: None,
                })
            })
            .collect();
    }

    let object = decode_kio_object(parent_object_id)?;
    kio_validate_storage(&connection, device_path, &object.storage_path)?;
    let storage = kio_proxy(&connection, &object.storage_path, KIO_MTP_STORAGE_INTERFACE)
        .map_err(|error| kio_error("Could not open MTP storage", error))?;
    let (files, result): (Vec<KmtpFile>, i32) = storage
        .call("getFilesAndFolders", &object.remote_path)
        .map_err(|error| kio_error("Could not list the MTP folder", error))?;
    if result != 0 {
        return Err(BExplorerError::Operation(format!(
            "The MTP folder could not be read (code {result})"
        )));
    }

    Ok(files
        .into_iter()
        .filter_map(|file| {
            let name = file.3;
            if name.is_empty() || name == "." || name == ".." {
                return None;
            }
            let is_folder = file.6 == "inode/directory";
            Some(PortableObjectInfo {
                id: encode_kio_object(
                    &object.storage_path,
                    &join_remote_path(&object.remote_path, &name),
                ),
                name,
                is_folder,
                size: (!is_folder).then_some(file.4),
            })
        })
        .collect())
}

fn kio_object_info(device_path: &str, object_id: &str) -> Result<PortableObjectInfo> {
    if object_id == PORTABLE_ROOT_OBJECT_ID {
        let name = kio_device_name(device_path).unwrap_or_else(|| "MTP device".into());
        return Ok(PortableObjectInfo {
            id: object_id.into(),
            name,
            is_folder: true,
            size: None,
        });
    }

    let _access = kio_access_lock();
    let connection = kio_connection()?;
    let object = decode_kio_object(object_id)?;
    kio_validate_storage(&connection, device_path, &object.storage_path)?;
    let storage = kio_proxy(&connection, &object.storage_path, KIO_MTP_STORAGE_INTERFACE)
        .map_err(|error| kio_error("Could not open MTP storage", error))?;
    if object.remote_path == "/" {
        let name = storage
            .get_property::<String>("description")
            .unwrap_or_else(|_| "Internal storage".into());
        return Ok(PortableObjectInfo {
            id: object_id.into(),
            name,
            is_folder: true,
            size: None,
        });
    }

    let file: KmtpFile = storage
        .call("getFileMetadata", &object.remote_path)
        .map_err(|error| kio_error("Could not read MTP object metadata", error))?;
    if file.0 == 0 {
        return Err(BExplorerError::Operation(
            "The MTP object no longer exists".into(),
        ));
    }
    let is_folder = file.6 == "inode/directory";
    Ok(PortableObjectInfo {
        id: object_id.into(),
        name: file.3,
        is_folder,
        size: (!is_folder).then_some(file.4),
    })
}

fn kio_delete_objects(device_path: &str, object_ids: &[String]) -> Result<usize> {
    let _access = kio_access_lock();
    let connection = kio_connection()?;
    let mut completed = 0;
    for object_id in object_ids {
        let object = decode_kio_object(object_id)?;
        kio_validate_storage(&connection, device_path, &object.storage_path)?;
        if object.remote_path == "/" {
            return Err(BExplorerError::Operation(
                "Cannot delete an MTP storage root".into(),
            ));
        }
        let storage = kio_proxy(&connection, &object.storage_path, KIO_MTP_STORAGE_INTERFACE)
            .map_err(|error| kio_error("Could not open MTP storage", error))?;
        let result: i32 = storage
            .call("deleteObject", &object.remote_path)
            .map_err(|error| kio_error("Could not delete the MTP object", error))?;
        if result != 0 {
            return Err(BExplorerError::Operation(format!(
                "The MTP object could not be deleted (code {result})"
            )));
        }
        completed += 1;
    }
    Ok(completed)
}

fn kio_download_file(device_path: &str, object_id: &str, target: &Path) -> Result<u64> {
    let _access = kio_access_lock();
    let connection = kio_connection()?;
    let object = decode_kio_object(object_id)?;
    kio_validate_storage(&connection, device_path, &object.storage_path)?;
    if object.remote_path == "/" {
        return Err(BExplorerError::Operation(
            "An MTP storage root cannot be downloaded as a file".into(),
        ));
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = File::create(target)?;
    let storage = kio_proxy(&connection, &object.storage_path, KIO_MTP_STORAGE_INTERFACE)
        .map_err(|error| kio_error("Could not open MTP storage", error))?;
    let metadata: KmtpFile = storage
        .call("getFileMetadata", &object.remote_path)
        .map_err(|error| kio_error("Could not read MTP object metadata", error))?;
    if metadata.0 == 0 || metadata.6 == "inode/directory" {
        return Err(BExplorerError::Operation(
            "The MTP file no longer exists".into(),
        ));
    }
    kio_transfer_file(
        &storage,
        "getFileToFileDescriptor",
        &file,
        &object.remote_path,
    )?;
    Ok(metadata.4)
}

fn kio_upload_file(
    device_path: &str,
    parent_object_id: &str,
    source: &Path,
    name: &str,
) -> Result<String> {
    let _access = kio_access_lock();
    let connection = kio_connection()?;
    let parent = decode_kio_object(parent_object_id)?;
    kio_validate_storage(&connection, device_path, &parent.storage_path)?;
    let destination = join_remote_path(&parent.remote_path, valid_child_name(name)?);
    let file = File::open(source)?;
    let storage = kio_proxy(&connection, &parent.storage_path, KIO_MTP_STORAGE_INTERFACE)
        .map_err(|error| kio_error("Could not open MTP storage", error))?;
    kio_transfer_file(&storage, "sendFileFromFileDescriptor", &file, &destination)?;
    Ok(encode_kio_object(&parent.storage_path, &destination))
}

fn kio_create_folder(device_path: &str, parent_object_id: &str, name: &str) -> Result<String> {
    let _access = kio_access_lock();
    let connection = kio_connection()?;
    let parent = decode_kio_object(parent_object_id)?;
    kio_validate_storage(&connection, device_path, &parent.storage_path)?;
    let destination = join_remote_path(&parent.remote_path, valid_child_name(name)?);
    let storage = kio_proxy(&connection, &parent.storage_path, KIO_MTP_STORAGE_INTERFACE)
        .map_err(|error| kio_error("Could not open MTP storage", error))?;
    let id: u32 = storage
        .call("createFolder", &destination)
        .map_err(|error| kio_error("Could not create the MTP folder", error))?;
    if id == 0 {
        return Err(BExplorerError::Operation(
            "The MTP device did not create the folder".into(),
        ));
    }
    Ok(encode_kio_object(&parent.storage_path, &destination))
}

fn kio_transfer_file(
    storage: &Proxy<'_>,
    method: &str,
    file: &File,
    remote_path: &str,
) -> Result<()> {
    let mut finished = storage
        .receive_signal("copyFinished")
        .map_err(|error| kio_error("Could not monitor the MTP transfer", error))?;
    let descriptor = Fd::from(file);
    let result: i32 = storage
        .call(method, &(descriptor, remote_path))
        .map_err(|error| kio_error("Could not start the MTP transfer", error))?;
    if result != 0 {
        return Err(BExplorerError::Operation(format!(
            "The MTP transfer could not be started (code {result})"
        )));
    }
    let message = finished.next().ok_or_else(|| {
        BExplorerError::Operation("The MTP device disconnected during the transfer".into())
    })?;
    let result: i32 = message
        .body()
        .deserialize()
        .map_err(|error| kio_error("Invalid MTP transfer response", error))?;
    if result == 0 {
        Ok(())
    } else {
        Err(BExplorerError::Operation(format!(
            "The MTP transfer failed (code {result})"
        )))
    }
}

fn kio_validate_storage(
    connection: &Connection,
    device_path: &str,
    storage_path: &str,
) -> Result<()> {
    let device = kio_proxy(connection, device_path, KIO_MTP_DEVICE_INTERFACE)
        .map_err(|error| kio_error("Could not open the MTP device", error))?;
    let storages: Vec<OwnedObjectPath> = device
        .call("listStorages", &())
        .map_err(|error| kio_error("Could not list the MTP device storage", error))?;
    if storages
        .iter()
        .any(|candidate| candidate.as_str() == storage_path)
    {
        Ok(())
    } else {
        Err(BExplorerError::Operation(
            "The MTP storage no longer belongs to this device".into(),
        ))
    }
}

fn kio_device_name(device_path: &str) -> Option<String> {
    let _access = kio_access_lock();
    let connection = kio_connection().ok()?;
    kio_proxy(&connection, device_path, KIO_MTP_DEVICE_INTERFACE)
        .ok()?
        .get_property::<String>("friendlyName")
        .ok()
}

fn kio_connection() -> Result<Connection> {
    Connection::session()
        .map_err(|error| kio_error("Could not connect to the desktop session bus", error))
}

fn kio_proxy<'a>(
    connection: &'a Connection,
    path: &'a str,
    interface: &'a str,
) -> zbus::Result<Proxy<'a>> {
    Proxy::new(connection, KIO_MTP_SERVICE, path, interface)
}

fn kio_access_lock() -> MutexGuard<'static, ()> {
    KIO_MTP_ACCESS
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn kio_error(context: &str, error: impl std::fmt::Display) -> BExplorerError {
    BExplorerError::Operation(format!("{context}: {error}"))
}

fn encode_kio_object(storage_path: &str, remote_path: &str) -> String {
    format!(
        "{KIO_OBJECT_PREFIX}{}:{}",
        hex_encode(storage_path),
        hex_encode(remote_path)
    )
}

fn decode_kio_object(value: &str) -> Result<KioObject> {
    let encoded = value
        .strip_prefix(KIO_OBJECT_PREFIX)
        .ok_or_else(|| BExplorerError::Operation("Invalid KDE MTP object identifier".into()))?;
    let (storage, remote) = encoded
        .split_once(':')
        .ok_or_else(|| BExplorerError::Operation("Incomplete KDE MTP object identifier".into()))?;
    let storage_path = hex_decode(storage)
        .ok_or_else(|| BExplorerError::Operation("Invalid KDE MTP storage identifier".into()))?;
    let remote_path = hex_decode(remote)
        .filter(|path| path.starts_with('/'))
        .ok_or_else(|| BExplorerError::Operation("Invalid KDE MTP object path".into()))?;
    Ok(KioObject {
        storage_path,
        remote_path,
    })
}

fn gvfs_mtp_volumes() -> Vec<GvfsMtpVolume> {
    if !command_exists("gio") {
        return Vec::new();
    }
    let Some(output) = Command::new("gio")
        .args(["mount", "-li"])
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
    else {
        return Vec::new();
    };
    parse_gio_mtp_volumes(&String::from_utf8_lossy(&output.stdout))
}

fn parse_gio_mtp_volumes(output: &str) -> Vec<GvfsMtpVolume> {
    let mut current_name = None;
    let mut current_icon_names = Vec::new();
    let mut volumes = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.starts_with("Volume(") {
            current_name = line
                .split_once("):")
                .map(|(_, name)| name.trim().to_owned())
                .filter(|name| !name.is_empty());
            current_icon_names.clear();
            continue;
        }
        if line.starts_with("Drive(") {
            current_name = None;
            current_icon_names.clear();
            continue;
        }
        if let Some(icons) = line.strip_prefix("themed icons:") {
            current_icon_names = parse_gio_themed_icon_names(icons);
            continue;
        }
        let Some(uri) = line.strip_prefix("activation_root=") else {
            continue;
        };
        let uri = uri.trim().trim_matches('\'').trim_matches('"');
        if !uri.to_ascii_lowercase().starts_with("mtp://") {
            continue;
        }
        let name = current_name
            .clone()
            .unwrap_or_else(|| gvfs_name_from_uri(uri));
        volumes.push(GvfsMtpVolume {
            name,
            uri: uri.to_owned(),
            icon_names: current_icon_names.clone(),
        });
    }
    volumes.sort_by_key(|volume| volume.name.to_lowercase());
    volumes.dedup_by(|left, right| left.uri == right.uri);
    volumes
}

fn parse_gio_themed_icon_names(value: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut remainder = value;
    while let Some((_, after_open)) = remainder.split_once('[') {
        let Some((name, after_close)) = after_open.split_once(']') else {
            break;
        };
        let name = name.trim();
        if valid_themed_icon_name(name) && !names.iter().any(|candidate| candidate == name) {
            names.push(name.to_owned());
        }
        remainder = after_close;
    }
    names
}

fn valid_themed_icon_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+'))
}

fn remember_gvfs_portable_icon_names(volumes: &[GvfsMtpVolume]) {
    let mut cache = GVFS_PORTABLE_ICON_NAMES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.clear();
    cache.extend(volumes.iter().map(|volume| {
        (
            format!("{GVFS_DEVICE_PREFIX}{}", volume.uri),
            volume.icon_names.clone(),
        )
    }));
}

fn cached_gvfs_icon_names(device_id: &str) -> Vec<String> {
    GVFS_PORTABLE_ICON_NAMES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(device_id)
        .cloned()
        .unwrap_or_default()
}

fn gvfs_mount_icon_names(path: &Path) -> Vec<String> {
    let Some(identity) = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(normalized_mtp_identity)
    else {
        return Vec::new();
    };
    GVFS_PORTABLE_ICON_NAMES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .find_map(|(device_id, names)| {
            device_id
                .strip_prefix(GVFS_DEVICE_PREFIX)
                .and_then(normalized_mtp_identity)
                .filter(|candidate| candidate == &identity)
                .map(|_| names.clone())
        })
        .unwrap_or_default()
}

fn gvfs_mount_kind(path: &Path) -> Option<&str> {
    path.file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.split_once(':').map(|(kind, _)| kind))
        .filter(|kind| matches!(*kind, "mtp" | "gphoto2" | "afc"))
}

fn append_unique<const N: usize>(candidates: &mut Vec<String>, names: [&str; N]) {
    for name in names {
        if !candidates.iter().any(|candidate| candidate == name) {
            candidates.push(name.into());
        }
    }
}

fn is_kde_session() -> bool {
    [
        "XDG_CURRENT_DESKTOP",
        "XDG_SESSION_DESKTOP",
        "DESKTOP_SESSION",
    ]
    .into_iter()
    .filter_map(|name| std::env::var(name).ok())
    .any(|value| {
        let value = value.to_ascii_lowercase();
        value.contains("kde") || value.contains("plasma")
    })
}

fn gvfs_portable_devices(volumes: &[GvfsMtpVolume]) -> Vec<PortableDeviceInfo> {
    volumes
        .iter()
        // Mounted GVfs devices are already exposed as ordinary storage roots,
        // which gives them faster native filesystem operations.
        .filter(|volume| gvfs_mount_path_for_uri(&volume.uri).is_none())
        .map(|volume| PortableDeviceInfo {
            id: format!("{GVFS_DEVICE_PREFIX}{}", volume.uri),
            name: volume.name.clone(),
            manufacturer: String::new(),
            description: "GVfs MTP".into(),
        })
        .collect()
}

fn gvfs_device_objects(uri: &str, parent_object_id: &str) -> Result<Vec<PortableObjectInfo>> {
    let parent = gvfs_object_path(uri, parent_object_id)?;
    let root = ensure_gvfs_mounted(uri)?;
    let mut objects = Vec::new();
    for entry in fs::read_dir(&parent)? {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        let relative = path
            .strip_prefix(&root)
            .map_err(|_| BExplorerError::Operation("GVfs returned an invalid MTP path".into()))?;
        objects.push(PortableObjectInfo {
            id: encode_gvfs_object(relative),
            name,
            is_folder: metadata.is_dir(),
            size: metadata.is_file().then_some(metadata.len()),
        });
    }
    Ok(objects)
}

fn gvfs_object_info(uri: &str, object_id: &str) -> Result<PortableObjectInfo> {
    if object_id == PORTABLE_ROOT_OBJECT_ID {
        return Ok(PortableObjectInfo {
            id: object_id.into(),
            name: gvfs_name_from_uri(uri),
            is_folder: true,
            size: None,
        });
    }
    let path = gvfs_object_path(uri, object_id)?;
    let metadata = fs::metadata(&path)?;
    Ok(PortableObjectInfo {
        id: object_id.into(),
        name: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("MTP item")
            .into(),
        is_folder: metadata.is_dir(),
        size: metadata.is_file().then_some(metadata.len()),
    })
}

fn gvfs_delete_objects(uri: &str, object_ids: &[String]) -> Result<usize> {
    let mut completed = 0;
    for object_id in object_ids {
        if object_id == PORTABLE_ROOT_OBJECT_ID {
            return Err(BExplorerError::Operation(
                "Cannot delete the portable device root".into(),
            ));
        }
        let path = gvfs_object_path(uri, object_id)?;
        if fs::metadata(&path)?.is_dir() {
            fs::remove_dir_all(path)?;
        } else {
            fs::remove_file(path)?;
        }
        completed += 1;
    }
    Ok(completed)
}

fn gvfs_object_path(uri: &str, object_id: &str) -> Result<PathBuf> {
    let root = ensure_gvfs_mounted(uri)?;
    if object_id == PORTABLE_ROOT_OBJECT_ID {
        return Ok(root);
    }
    let relative = decode_gvfs_object(object_id)?;
    Ok(root.join(relative))
}

fn gvfs_relative_from_path(uri: &str, path: &Path) -> Result<PathBuf> {
    let root = ensure_gvfs_mounted(uri)?;
    path.strip_prefix(root)
        .map(Path::to_path_buf)
        .map_err(|_| BExplorerError::Operation("GVfs returned an invalid MTP path".into()))
}

fn ensure_gvfs_mounted(uri: &str) -> Result<PathBuf> {
    if let Some(path) = gvfs_mount_path_for_uri(uri) {
        return Ok(path);
    }
    let output = Command::new("gio")
        .args(["mount", uri])
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| {
            BExplorerError::Operation(format!("Could not start the GVfs MTP mount: {error}"))
        })?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        return Err(BExplorerError::Operation(format!(
            "Could not mount the MTP device with GVfs: {}",
            message.trim()
        )));
    }
    for _ in 0..40 {
        if let Some(path) = gvfs_mount_path_for_uri(uri) {
            return Ok(path);
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err(BExplorerError::Operation(
        "GVfs mounted the MTP device but did not expose its FUSE path".into(),
    ))
}

fn gvfs_mount_path_for_uri(uri: &str) -> Option<PathBuf> {
    let identity = normalized_mtp_identity(uri)?;
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from)?;
    fs::read_dir(runtime.join("gvfs"))
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .and_then(normalized_mtp_identity)
                    .is_some_and(|candidate| candidate == identity)
        })
}

fn normalized_mtp_identity(value: &str) -> Option<String> {
    let value = value
        .strip_prefix("mtp://")
        .or_else(|| value.strip_prefix("MTP://"))
        .or_else(|| value.strip_prefix("mtp:host="))?;
    let host = value.trim_end_matches('/').trim_matches(['[', ']']);
    let decoded = percent_decode(host).unwrap_or_else(|| host.to_owned());
    let normalized = decoded
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    (!normalized.is_empty()).then_some(normalized)
}

fn gvfs_name_from_uri(uri: &str) -> String {
    let value = uri
        .strip_prefix("mtp://")
        .or_else(|| uri.strip_prefix("MTP://"))
        .unwrap_or(uri)
        .trim_end_matches('/')
        .trim_matches(['[', ']']);
    percent_decode(value)
        .unwrap_or_else(|| value.to_owned())
        .replace('_', " ")
}

fn encode_gvfs_object(path: &Path) -> String {
    format!(
        "{GVFS_OBJECT_PREFIX}{}",
        hex_encode(&path.to_string_lossy())
    )
}

fn decode_gvfs_object(value: &str) -> Result<PathBuf> {
    let encoded = value
        .strip_prefix(GVFS_OBJECT_PREFIX)
        .ok_or_else(|| BExplorerError::Operation("Invalid GVfs MTP object identifier".into()))?;
    let decoded = hex_decode(encoded)
        .ok_or_else(|| BExplorerError::Operation("Invalid GVfs MTP object path".into()))?;
    let path = PathBuf::from(decoded);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(BExplorerError::Operation(
            "Unsafe GVfs MTP object path".into(),
        ));
    }
    Ok(path)
}

fn valid_child_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
    {
        return Err(BExplorerError::Operation(
            "Invalid name for an MTP item".into(),
        ));
    }
    Ok(name)
}

fn join_remote_path(parent: &str, child: &str) -> String {
    if parent == "/" {
        format!("/{child}")
    } else {
        format!("{}/{child}", parent.trim_end_matches('/'))
    }
}

fn command_exists(program: &str) -> bool {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .any(|directory| directory.join(program).is_file())
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

fn percent_decode(value: &str) -> Option<String> {
    let input = value.as_bytes();
    let mut bytes = Vec::with_capacity(input.len());
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

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn unsupported_device(device_id: &str) -> BExplorerError {
    BExplorerError::Operation(format!(
        "Unsupported Linux portable-device backend: {device_id}"
    ))
}

struct TemporaryDownload(PathBuf);

impl Drop for TemporaryDownload {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gio_mtp_volumes_and_ignores_regular_storage() {
        let output = "\
Drive(0): USB disk
  Volume(0): USB
    activation_root=file:///run/media/user/USB
Volume(1): Pixel de Ana
  Type: GProxyVolume (GProxyVolumeMonitorMTP)
  themed icons:  [phone]  [phone-symbolic]
  symbolic themed icons:  [phone-symbolic]  [phone]
  activation_root=mtp://Google_Pixel_ABC123/
Volume(2): Tablet
  activation_root='mtp://[usb:001,009]/'
";
        assert_eq!(
            parse_gio_mtp_volumes(output),
            vec![
                GvfsMtpVolume {
                    name: "Pixel de Ana".into(),
                    uri: "mtp://Google_Pixel_ABC123/".into(),
                    icon_names: vec!["phone".into(), "phone-symbolic".into()],
                },
                GvfsMtpVolume {
                    name: "Tablet".into(),
                    uri: "mtp://[usb:001,009]/".into(),
                    icon_names: Vec::new(),
                },
            ]
        );
    }

    #[test]
    fn portable_icon_candidates_follow_the_active_linux_backend() {
        let kio =
            portable_device_icon_names(Some("linux-kio-mtp:/org/kde/kmtp/device_1"), Path::new(""));
        assert_eq!(kio[0], "multimedia-player");

        remember_gvfs_portable_icon_names(&[GvfsMtpVolume {
            name: "Pixel".into(),
            uri: "mtp://Google_Pixel_ABC123/".into(),
            icon_names: vec!["phone".into(), "phone-symbolic".into()],
        }]);
        let gvfs = portable_device_icon_names(
            Some("linux-gvfs-mtp:mtp://Google_Pixel_ABC123/"),
            Path::new(""),
        );
        assert_eq!(&gvfs[..2], ["phone", "phone-symbolic"]);
    }

    #[test]
    fn mounted_gvfs_devices_reuse_the_icons_advertised_by_gio() {
        remember_gvfs_portable_icon_names(&[GvfsMtpVolume {
            name: "Pixel".into(),
            uri: "mtp://Google_Pixel_ABC123/".into(),
            icon_names: vec!["phone".into()],
        }]);
        let names = portable_device_icon_names(
            None,
            Path::new("/run/user/1000/gvfs/mtp:host=Google_Pixel_ABC123"),
        );
        assert_eq!(names[0], "phone");
    }

    #[test]
    fn round_trips_kio_object_identifiers() {
        let encoded = encode_kio_object("/org/kde/kmtp/device_1/storage_2", "/DCIM/Cámara");
        assert_eq!(
            decode_kio_object(&encoded).unwrap(),
            KioObject {
                storage_path: "/org/kde/kmtp/device_1/storage_2".into(),
                remote_path: "/DCIM/Cámara".into(),
            }
        );
    }

    #[test]
    fn matches_stable_and_usb_gvfs_mtp_identifiers() {
        assert_eq!(
            normalized_mtp_identity("mtp://Google_Pixel_ABC123/"),
            normalized_mtp_identity("mtp:host=Google_Pixel_ABC123")
        );
        assert_eq!(
            normalized_mtp_identity("mtp://[usb:001,009]/"),
            normalized_mtp_identity("mtp:host=%5Busb%3A001%2C009%5D")
        );
    }

    #[test]
    fn rejects_parent_components_in_gvfs_object_ids() {
        let encoded = format!("{GVFS_OBJECT_PREFIX}{}", hex_encode("../secrets"));
        assert!(decode_gvfs_object(&encoded).is_err());
    }

    #[test]
    #[ignore = "requires an unlocked MTP phone connected through KDE KIO"]
    fn live_kio_browses_and_downloads_a_small_file() {
        let device = kio_portable_devices()
            .into_iter()
            .next()
            .expect("connected KDE MTP device");
        let device_path = device
            .id
            .strip_prefix(KIO_DEVICE_PREFIX)
            .expect("KIO device identifier");
        let storages =
            kio_device_objects(device_path, PORTABLE_ROOT_OBJECT_ID).expect("list MTP storages");
        assert!(!storages.is_empty(), "the phone exposes no MTP storage");

        let candidate = storages.iter().find_map(|storage| {
            kio_device_objects(device_path, &storage.id)
                .ok()?
                .into_iter()
                .find(|item| {
                    !item.is_folder && item.size.is_some_and(|size| size <= 8 * 1024 * 1024)
                })
        });
        let Some(candidate) = candidate else {
            return;
        };

        let sequence = THUMBNAIL_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let target = std::env::temp_dir().join(format!(
            "bexplorer-live-mtp-test-{}-{sequence}",
            std::process::id()
        ));
        let cleanup = TemporaryDownload(target.clone());
        let downloaded =
            kio_download_file(device_path, &candidate.id, &target).expect("download MTP file");
        assert_eq!(Some(downloaded), candidate.size);
        assert_eq!(
            fs::metadata(&cleanup.0).expect("downloaded file").len(),
            downloaded
        );
    }

    #[test]
    #[ignore = "creates and removes a temporary folder on an unlocked KDE MTP phone"]
    fn live_kio_round_trips_a_temporary_file() {
        let device = kio_portable_devices()
            .into_iter()
            .next()
            .expect("connected KDE MTP device");
        let device_path = device
            .id
            .strip_prefix(KIO_DEVICE_PREFIX)
            .expect("KIO device identifier");
        let storage = kio_device_objects(device_path, PORTABLE_ROOT_OBJECT_ID)
            .expect("list MTP storages")
            .into_iter()
            .next()
            .expect("MTP storage");

        let sequence = THUMBNAIL_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let folder_name = format!("BExplorer MTP test {}-{sequence}", std::process::id());
        let folder_id =
            kio_create_folder(device_path, &storage.id, &folder_name).expect("create test folder");

        let source = TemporaryDownload(
            std::env::temp_dir().join(format!("bexplorer-mtp-source-{sequence}.txt")),
        );
        let downloaded = TemporaryDownload(
            std::env::temp_dir().join(format!("bexplorer-mtp-download-{sequence}.txt")),
        );
        let contents = b"BExplorer Linux MTP round trip\n";
        let round_trip = (|| -> Result<()> {
            fs::write(&source.0, contents)?;
            kio_upload_file(device_path, &folder_id, &source.0, "round-trip.txt")?;
            let uploaded = kio_device_objects(device_path, &folder_id)?
                .into_iter()
                .find(|item| item.name == "round-trip.txt")
                .ok_or_else(|| {
                    BExplorerError::Operation("The uploaded MTP file was not listed".into())
                })?;
            kio_download_file(device_path, &uploaded.id, &downloaded.0)?;
            if fs::read(&downloaded.0)? != contents {
                return Err(BExplorerError::Operation(
                    "The MTP round-trip contents did not match".into(),
                ));
            }
            Ok(())
        })();

        let cleanup = kio_delete_objects(device_path, std::slice::from_ref(&folder_id));
        round_trip.expect("round-trip MTP file");
        cleanup.expect("remove temporary MTP folder");
    }
}
