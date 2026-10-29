use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, TryLockError};
use std::task::{Context, Poll, Wake, Waker};
use std::thread;
use std::time::{Duration, Instant};

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{Fd, OwnedObjectPath};

use crate::platform::{PortableDeviceInfo, PortableObjectInfo};
use crate::utils::errors::{BExplorerError, Result};
use crate::utils::process::command_output_with_timeout;

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
const GVFS_VOLUME_CACHE_TTL: Duration = Duration::from_secs(2);
const GVFS_VOLUME_QUERY_TIMEOUT: Duration = Duration::from_millis(1_200);
const GVFS_MOUNT_TIMEOUT: Duration = Duration::from_secs(4);
const KIO_METHOD_TIMEOUT: Duration = Duration::from_secs(10);
const KIO_ACCESS_TIMEOUT: Duration = Duration::from_secs(10);
const KIO_TRANSFER_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

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

type GvfsVolumeCache = Option<(Instant, Vec<GvfsMtpVolume>)>;

static KIO_MTP_ACCESS: OnceLock<Mutex<()>> = OnceLock::new();
static KIO_QUARANTINE: OnceLock<Mutex<Option<QuarantinedKioTransfer>>> = OnceLock::new();
static GVFS_PORTABLE_ICON_NAMES: OnceLock<Mutex<HashMap<String, Vec<String>>>> = OnceLock::new();
static GVFS_VOLUME_CACHE: OnceLock<Mutex<GvfsVolumeCache>> = OnceLock::new();
#[cfg(test)]
static THUMBNAIL_SEQUENCE: AtomicU64 = AtomicU64::new(1);

struct QuarantinedKioTransfer {
    owner: String,
    reply_serial: Option<u32>,
    signals: zbus::proxy::SignalStream<'static>,
    // Keep uncertain downloads private until the daemon finishes or its
    // connection is replaced. The caller's destination is never exposed to it.
    _download: Option<PrivateKioDownload>,
}

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

/// Removes only the exported object, never new children added after a copy.
pub fn portable_delete_exported_object(
    device_id: &str,
    object_id: &str,
    is_folder: bool,
) -> Result<()> {
    if device_id.starts_with(KIO_DEVICE_PREFIX) {
        // KIO only exposes path-based deleteObject, with no nonrecursive or
        // stable-handle precondition. Retain the source after a completed copy
        // rather than risk deleting an object/child introduced meanwhile.
        return Err(BExplorerError::Operation("The copy completed, but KDE MTP cannot safely remove the original as part of Move. The original was kept; use a separate Delete action if you want to remove it.".into()));
    }
    if let Some(uri) = device_id.strip_prefix(GVFS_DEVICE_PREFIX) {
        if object_id == PORTABLE_ROOT_OBJECT_ID
            || decode_gvfs_object(object_id)?.as_os_str().is_empty()
        {
            return Err(BExplorerError::Operation(
                "Cannot remove a portable storage root".into(),
            ));
        }
        let path = gvfs_object_path(uri, object_id)?;
        return remove_exported_gvfs_object(&path, is_folder).map_err(Into::into);
    }
    Err(unsupported_device(device_id))
}

fn remove_exported_gvfs_object(path: &Path, is_folder: bool) -> std::io::Result<()> {
    if is_folder {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    }
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
        return download_gvfs_file(&source, target);
    }
    Err(unsupported_device(device_id))
}

fn download_gvfs_file(source: &Path, target: &Path) -> Result<u64> {
    let mut source = File::open(source)?;
    // Preserve another process's file (including symlinks) if it appeared
    // after the transfer queue reserved this destination name.
    let mut target = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)?;
    let copied = std::io::copy(&mut source, &mut target)?;
    target.sync_all()?;
    Ok(copied)
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
    if device_id.starts_with(KIO_DEVICE_PREFIX)
        && KIO_MTP_ACCESS
            .get()
            .is_some_and(|lock| lock.try_lock().is_err())
    {
        return None;
    }
    let info = portable_device_object_info(device_id, object_id).ok()?;
    if info.is_folder || info.size.is_none_or(|size| size > max_bytes as u64) {
        return None;
    }

    let directory = PrivateThumbnailDirectory::create().ok()?;
    let path = directory.0.join("thumbnail");
    let cleanup = TemporaryDownload(path.clone());
    if let Err(error) = portable_download_file(device_id, object_id, &path) {
        if error.operation_may_continue()
            && let Some(quarantine) = KIO_QUARANTINE.get()
            && let Some(pending) = quarantine
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_mut()
            && let Some(download) = pending._download.as_mut()
        {
            download._thumbnail_parent = Some(directory);
        }
        return None;
    }

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
    let Ok(_access) = kio_access_lock() else {
        return Vec::new();
    };
    let Some(connection) = kio_connection().ok() else {
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
    let _access = kio_access_lock()?;
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

    let _access = kio_access_lock()?;
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
    let _access = kio_access_lock()?;
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
    let _access = kio_access_lock()?;
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
    let parent = target
        .parent()
        .ok_or_else(|| BExplorerError::InvalidPath(target.into()))?;
    let download = PrivateKioDownload::create(parent)?;
    let file = File::create(&download.path)?;
    let download = kio_transfer_file(
        &storage,
        "getFileToFileDescriptor",
        &file,
        &object.remote_path,
        Some(download),
    )?
    .ok_or_else(|| BExplorerError::Operation("Missing private MTP download".into()))?;
    if file.metadata()?.len() != metadata.4 {
        return Err(BExplorerError::Operation(
            "The MTP download size did not match the source".into(),
        ));
    }
    file.sync_all()?;
    // The provider has confirmed completion. Only now publish the finished
    // file in the destination namespace, atomically on the same filesystem.
    publish_kio_download(&download.path, target)?;
    Ok(metadata.4)
}

fn publish_kio_download(staging: &Path, target: &Path) -> std::io::Result<()> {
    // Export callers reserve a new name; replacement jobs pass their own new
    // staging name. Never replace an unrelated entry that appeared meanwhile.
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        staging,
        rustix::fs::CWD,
        target,
        rustix::fs::RenameFlags::NOREPLACE,
    )?;
    Ok(())
}

fn kio_upload_file(
    device_path: &str,
    parent_object_id: &str,
    source: &Path,
    name: &str,
) -> Result<String> {
    let _access = kio_access_lock()?;
    let connection = kio_connection()?;
    let parent = decode_kio_object(parent_object_id)?;
    kio_validate_storage(&connection, device_path, &parent.storage_path)?;
    let destination = join_remote_path(&parent.remote_path, valid_child_name(name)?);
    let file = File::open(source)?;
    let storage = kio_proxy(&connection, &parent.storage_path, KIO_MTP_STORAGE_INTERFACE)
        .map_err(|error| kio_error("Could not open MTP storage", error))?;
    kio_transfer_file(
        &storage,
        "sendFileFromFileDescriptor",
        &file,
        &destination,
        None,
    )?;
    Ok(encode_kio_object(&parent.storage_path, &destination))
}

fn kio_create_folder(device_path: &str, parent_object_id: &str, name: &str) -> Result<String> {
    let _access = kio_access_lock()?;
    let connection = kio_connection()?;
    let parent = decode_kio_object(parent_object_id)?;
    kio_validate_storage(&connection, device_path, &parent.storage_path)?;
    let destination = join_remote_path(&parent.remote_path, valid_child_name(name)?);
    let storage = kio_proxy(&connection, &parent.storage_path, KIO_MTP_STORAGE_INTERFACE)
        .map_err(|error| kio_error("Could not open MTP storage", error))?;
    let reply = storage
        .call_method("createFolder", &destination)
        .map_err(|error| kio_error("Could not create the MTP folder", error))?;
    kio_created_folder_id(&reply)?;
    Ok(encode_kio_object(&parent.storage_path, &destination))
}

fn kio_transfer_file(
    storage: &Proxy<'_>,
    method: &str,
    file: &File,
    remote_path: &str,
    download: Option<PrivateKioDownload>,
) -> Result<Option<PrivateKioDownload>> {
    use iced::futures::StreamExt;
    let owner = kio_service_owner(storage.connection())?
        .ok_or_else(|| BExplorerError::Operation("The KDE MTP service is not available".into()))?;
    // Bind the operation to one daemon connection, including its start reply
    // and signals. A replacement owner cannot complete this daemon's job.
    let storage = Proxy::new_owned(
        storage.connection().clone(),
        owner.clone(),
        storage.path().to_owned(),
        KIO_MTP_STORAGE_INTERFACE.to_owned(),
    )
    .map_err(|error| kio_error("Could not bind the KDE MTP service connection", error))?;
    let mut signals = future_before_deadline(
        storage.inner().receive_all_signals(),
        Instant::now() + KIO_METHOD_TIMEOUT,
    )
    .ok_or_else(|| {
        BExplorerError::Operation("Could not monitor the KDE MTP transfer in time".into())
    })?
    .map_err(|error| kio_error("Could not monitor the MTP transfer", error))?;
    let descriptor = Fd::from(file);
    let reply = match storage.call_method(method, &(descriptor, remote_path)) {
        Ok(reply) => reply,
        Err(error) => {
            quarantine_kio_transfer(owner, None, signals, download);
            return Err(BExplorerError::OperationPending(format!(
                "The MTP transfer start could not be confirmed; the KDE provider was quarantined: {error}"
            )));
        }
    };
    let reply_serial = reply.primary_header().serial_num().get();
    let result: i32 = match reply.body().deserialize() {
        Ok(result) => result,
        Err(error) => {
            quarantine_kio_transfer(owner, Some(reply_serial), signals, download);
            return Err(BExplorerError::OperationPending(format!(
                "Invalid MTP transfer start response: {error}"
            )));
        }
    };
    if result != 0 {
        return Err(BExplorerError::Operation(format!(
            "The MTP transfer could not be started (code {result})"
        )));
    }
    let mut last_progress = Instant::now();
    let mut transferred = 0;
    loop {
        let Some(Some(message)) =
            future_before_deadline(signals.next(), last_progress + KIO_TRANSFER_IDLE_TIMEOUT)
        else {
            quarantine_kio_transfer(owner, Some(reply_serial), signals, download);
            return Err(kio_transfer_timeout());
        };
        if !signal_follows_reply(&message, reply_serial) {
            continue;
        }
        match message.header().member().map(|member| member.as_str()) {
            Some("copyProgress") => {
                if let Ok((bytes, _total)) = message.body().deserialize::<(u64, u64)>()
                    && bytes > transferred
                {
                    transferred = bytes;
                    last_progress = Instant::now();
                }
            }
            Some("copyFinished") => {
                let result: i32 = match message.body().deserialize() {
                    Ok(result) => result,
                    Err(error) => {
                        quarantine_kio_transfer(owner, Some(reply_serial), signals, download);
                        return Err(BExplorerError::OperationPending(format!(
                            "Invalid MTP transfer response: {error}"
                        )));
                    }
                };
                return if result == 0 {
                    Ok(download)
                } else {
                    Err(BExplorerError::Operation(format!(
                        "The MTP transfer failed (code {result})"
                    )))
                };
            }
            _ => {}
        }
    }
}

fn signal_follows_reply(message: &zbus::Message, reply_serial: u32) -> bool {
    // The daemon processes transfers asynchronously after replying. Ignore
    // signals buffered from an earlier operation/client before this reply.
    (message
        .primary_header()
        .serial_num()
        .get()
        .wrapping_sub(reply_serial) as i32)
        > 0
}

fn quarantine_kio_transfer(
    owner: String,
    reply_serial: Option<u32>,
    signals: zbus::proxy::SignalStream<'static>,
    download: Option<PrivateKioDownload>,
) {
    *KIO_QUARANTINE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(QuarantinedKioTransfer {
        owner,
        reply_serial,
        signals,
        _download: download,
    });
}

fn kio_service_owner(connection: &Connection) -> Result<Option<String>> {
    let bus = zbus::blocking::fdo::DBusProxy::new(connection)
        .map_err(|error| kio_error("Could not inspect the KDE MTP service", error))?;
    match bus.get_name_owner(
        KIO_MTP_SERVICE
            .try_into()
            .map_err(|error| kio_error("Invalid KDE MTP service name", error))?,
    ) {
        Ok(owner) => Ok(Some(owner.to_string())),
        Err(zbus::fdo::Error::NameHasNoOwner(_)) => Ok(None),
        Err(error) => Err(kio_error(
            "Could not inspect the KDE MTP service owner",
            error,
        )),
    }
}

fn drain_quarantined_signals<S: iced::futures::Stream<Item = zbus::Message> + Unpin>(
    signals: &mut S,
    reply_serial: Option<u32>,
) -> bool {
    let Some(reply_serial) = reply_serial else {
        return false;
    };
    let waker = Waker::from(Arc::new(ThreadWake(thread::current())));
    let mut context = Context::from_waker(&waker);
    for _ in 0..64 {
        match std::pin::Pin::new(&mut *signals).poll_next(&mut context) {
            Poll::Ready(Some(message))
                if signal_follows_reply(&message, reply_serial)
                    && message
                        .header()
                        .member()
                        .is_some_and(|member| member.as_str() == "copyFinished") =>
            {
                return true;
            }
            Poll::Ready(Some(_)) => (),
            // A closed stream is not itself proof the transfer finished.
            // Check the bus owner separately before releasing quarantine.
            Poll::Ready(None) | Poll::Pending => return false,
        }
    }
    false
}

fn check_kio_quarantine() -> Result<()> {
    let Some(quarantine) = KIO_QUARANTINE.get() else {
        return Ok(());
    };
    let mut quarantine = quarantine
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(pending) = quarantine.as_mut() else {
        return Ok(());
    };
    let finished = drain_quarantined_signals(&mut pending.signals, pending.reply_serial);
    let owner = if finished {
        None
    } else {
        // A bus failure does not prove that the external writer stopped.
        match kio_connection().and_then(|connection| kio_service_owner(&connection)) {
            Ok(owner) => owner,
            Err(_) => return Err(kio_transfer_timeout()),
        }
    };
    if finished || owner.as_deref() != Some(pending.owner.as_str()) {
        *quarantine = None;
        return Ok(());
    }
    Err(kio_transfer_timeout())
}

fn kio_created_folder_id(reply: &zbus::Message) -> Result<u32> {
    let body = reply.body();
    // Stable KIO releases return uint32; newer releases use int32 to expose
    // existing-file/folder errors. Inspect the reply instead of retrying an
    // operation which may already have created the folder.
    let id = match body.signature().to_string().as_str() {
        "u" => i64::from(
            body.deserialize::<u32>()
                .map_err(|error| kio_error("Invalid MTP folder response", error))?,
        ),
        "i" => i64::from(
            body.deserialize::<i32>()
                .map_err(|error| kio_error("Invalid MTP folder response", error))?,
        ),
        _ => {
            return Err(BExplorerError::Operation(
                "Invalid MTP folder response signature".into(),
            ));
        }
    };
    if id <= 0 {
        return Err(BExplorerError::Operation(format!(
            "The MTP device did not create the folder (code {id})"
        )));
    }
    Ok(id as u32)
}

fn kio_transfer_timeout() -> BExplorerError {
    BExplorerError::OperationPending("The KDE MTP service stopped responding. Further KDE MTP operations are blocked until that transfer finishes or the provider reconnects. Your source and destination were preserved.".into())
}

struct ThreadWake(thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

fn future_before_deadline<F: std::future::Future>(
    future: F,
    deadline: Instant,
) -> Option<F::Output> {
    let waker = Waker::from(Arc::new(ThreadWake(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        let remaining = deadline.checked_duration_since(Instant::now())?;
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return Some(value);
        }
        thread::park_timeout(remaining);
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
    let _access = kio_access_lock().ok()?;
    let connection = kio_connection().ok()?;
    kio_proxy(&connection, device_path, KIO_MTP_DEVICE_INTERFACE)
        .ok()?
        .get_property::<String>("friendlyName")
        .ok()
}

fn kio_connection() -> Result<Connection> {
    zbus::blocking::connection::Builder::session()
        .and_then(|builder| builder.method_timeout(KIO_METHOD_TIMEOUT).build())
        .map_err(|error| kio_error("Could not connect to the desktop session bus", error))
}

fn kio_proxy<'a>(
    connection: &'a Connection,
    path: &'a str,
    interface: &'a str,
) -> zbus::Result<Proxy<'a>> {
    Proxy::new(connection, KIO_MTP_SERVICE, path, interface)
}

fn kio_access_lock() -> Result<MutexGuard<'static, ()>> {
    let guard = lock_before_deadline(
        KIO_MTP_ACCESS.get_or_init(|| Mutex::new(())),
        Instant::now() + KIO_ACCESS_TIMEOUT,
    )?;
    check_kio_quarantine()?;
    Ok(guard)
}

fn lock_before_deadline<T>(lock: &Mutex<T>, deadline: Instant) -> Result<MutexGuard<'_, T>> {
    loop {
        match lock.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::Poisoned(error)) => return Ok(error.into_inner()),
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(5))
            }
            Err(TryLockError::WouldBlock) => {
                return Err(BExplorerError::Operation(
                    "The KDE MTP device is busy; try again after its current operation finishes"
                        .into(),
                ));
            }
        }
    }
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
    let cache = GVFS_VOLUME_CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(cache) = cache.lock()
        && let Some((loaded_at, volumes)) = cache.as_ref()
        && loaded_at.elapsed() <= GVFS_VOLUME_CACHE_TTL
    {
        return volumes.clone();
    }
    let volumes = query_gvfs_mtp_volumes();
    if let Ok(mut cache) = cache.lock() {
        *cache = Some((Instant::now(), volumes.clone()));
    }
    volumes
}

fn query_gvfs_mtp_volumes() -> Vec<GvfsMtpVolume> {
    if !command_exists("gio") || !portable_transport_present() {
        return Vec::new();
    }
    let mut command = Command::new("gio");
    command
        .args(["mount", "-li"])
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    let Some(output) = command_output_with_timeout(&mut command, GVFS_VOLUME_QUERY_TIMEOUT)
        .filter(|output| output.status.success())
    else {
        return Vec::new();
    };
    parse_gio_mtp_volumes(&String::from_utf8_lossy(&output.stdout))
}

fn portable_transport_present() -> bool {
    let usb_devices = match fs::read_dir("/sys/bus/usb/devices") {
        Ok(entries) => entries.flatten().any(|entry| {
            let path = entry.path();
            path.join("idVendor").is_file() && path.join("idProduct").is_file()
        }),
        // On systems without readable sysfs, retain the conservative GIO probe.
        Err(_) => true,
    };
    usb_devices || mounted_gvfs_portable_transport_present()
}

fn mounted_gvfs_portable_transport_present() -> bool {
    let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) else {
        return false;
    };
    fs::read_dir(runtime.join("gvfs"))
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            ["mtp:", "gphoto2:", "afc:"]
                .iter()
                .any(|prefix| name.starts_with(prefix))
        })
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
    let mut command = Command::new("gio");
    command
        .args(["mount", uri])
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let output =
        command_output_with_timeout(&mut command, GVFS_MOUNT_TIMEOUT).ok_or_else(|| {
            BExplorerError::Operation("The GVfs MTP mount did not respond in time".into())
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

struct PrivateThumbnailDirectory(PathBuf);

struct PrivateKioDownload {
    path: PathBuf,
    _directory: PrivateThumbnailDirectory,
    _thumbnail_parent: Option<PrivateThumbnailDirectory>,
}

impl PrivateKioDownload {
    fn create(parent: &Path) -> std::io::Result<Self> {
        let directory = PrivateThumbnailDirectory::create_in(parent)?;
        Ok(Self {
            path: directory.0.join("payload"),
            _directory: directory,
            _thumbnail_parent: None,
        })
    }
}

impl Drop for PrivateKioDownload {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

impl PrivateThumbnailDirectory {
    fn create() -> std::io::Result<Self> {
        Self::create_in(&std::env::temp_dir())
    }

    fn create_in(parent: &Path) -> std::io::Result<Self> {
        use std::os::unix::fs::DirBuilderExt;
        for _ in 0..16 {
            let mut random = [0_u8; 16];
            getrandom::fill(&mut random)
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            let name = random
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            let path = parent.join(format!(".bexplorer-mtp-private-{name}"));
            match fs::DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(std::io::Error::other(
            "Could not reserve a private MTP thumbnail directory",
        ))
    }
}

impl Drop for PrivateThumbnailDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.0);
    }
}

impl Drop for TemporaryDownload {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transfer_signal(member: &str, serial: u32) -> zbus::Message {
        zbus::Message::signal("/org/bexplorer/Test", "org.bexplorer.Test", member)
            .unwrap()
            .serial(std::num::NonZeroU32::new(serial).unwrap())
            .build(&0_i32)
            .unwrap()
    }

    #[test]
    fn delayed_finished_signal_cannot_release_a_different_transfer() {
        use iced::futures::stream;
        let mut older = stream::iter([transfer_signal("copyFinished", 9)]);
        assert!(!drain_quarantined_signals(&mut older, Some(10)));
        let mut progress_only = stream::iter([transfer_signal("copyProgress", 11)]);
        assert!(!drain_quarantined_signals(&mut progress_only, Some(10)));
        let mut completed = stream::iter([transfer_signal("copyFinished", 11)]);
        assert!(drain_quarantined_signals(&mut completed, Some(10)));
        let mut unknown_start = stream::iter([transfer_signal("copyFinished", 11)]);
        assert!(!drain_quarantined_signals(&mut unknown_start, None));
        assert!(signal_follows_reply(
            &transfer_signal("copyFinished", 2),
            u32::MAX - 1
        ));
    }

    #[test]
    fn uncertain_download_never_publishes_or_removes_the_destination() {
        let parent = PrivateThumbnailDirectory::create().unwrap();
        let destination = parent.0.join("existing.bin");
        fs::write(&destination, b"original").unwrap();
        let pending = PrivateKioDownload::create(&parent.0).unwrap();
        fs::write(&pending.path, b"partial").unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"original");
        let path = pending.path.clone();
        drop(pending);
        assert!(!path.exists());
        assert_eq!(fs::read(&destination).unwrap(), b"original");
        fs::remove_file(destination).unwrap();
    }

    #[test]
    fn completed_kio_download_does_not_replace_a_racing_destination() {
        let parent = PrivateThumbnailDirectory::create().unwrap();
        let pending = PrivateKioDownload::create(&parent.0).unwrap();
        fs::write(&pending.path, b"downloaded").unwrap();
        let target = parent.0.join("result.bin");
        fs::write(&target, b"other application").unwrap();
        assert_eq!(
            publish_kio_download(&pending.path, &target)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(&target).unwrap(), b"other application");
        fs::remove_file(&target).unwrap();
        publish_kio_download(&pending.path, &target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"downloaded");
        fs::remove_file(target).unwrap();
    }

    #[test]
    fn gvfs_download_does_not_truncate_an_existing_destination() {
        let parent = PrivateThumbnailDirectory::create().unwrap();
        let source = parent.0.join("source.bin");
        let target = parent.0.join("result.bin");
        fs::write(&source, b"downloaded").unwrap();
        fs::write(&target, b"other application").unwrap();
        assert!(download_gvfs_file(&source, &target).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"other application");
        fs::remove_file(&target).unwrap();
        assert_eq!(download_gvfs_file(&source, &target).unwrap(), 10);
        assert_eq!(fs::read(&target).unwrap(), b"downloaded");
        fs::remove_file(source).unwrap();
        fs::remove_file(target).unwrap();
    }

    #[test]
    fn gvfs_move_cleanup_preserves_children_added_after_export() {
        let parent = PrivateThumbnailDirectory::create().unwrap();
        let folder = parent.0.join("source");
        fs::create_dir(&folder).unwrap();
        let new_child = folder.join("added-after-copy.txt");
        fs::write(&new_child, b"keep").unwrap();
        assert!(remove_exported_gvfs_object(&folder, true).is_err());
        assert_eq!(fs::read(&new_child).unwrap(), b"keep");
        remove_exported_gvfs_object(&new_child, false).unwrap();
        remove_exported_gvfs_object(&folder, true).unwrap();
    }

    #[test]
    fn portable_thumbnail_temporary_directory_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let directory = PrivateThumbnailDirectory::create().unwrap();
        assert_eq!(
            fs::metadata(&directory.0).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let path = directory.0.clone();
        drop(directory);
        assert!(!path.exists());
    }

    #[test]
    fn stalled_kio_signal_wait_expires_without_a_reader_thread() {
        let started = Instant::now();
        assert!(
            future_before_deadline(
                std::future::pending::<()>(),
                started + Duration::from_millis(30)
            )
            .is_none()
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(
            future_before_deadline(
                std::future::ready(42),
                Instant::now() + Duration::from_secs(1)
            ),
            Some(42)
        );
    }

    #[test]
    fn busy_kio_access_expires_instead_of_waiting_forever() {
        let mutex = Mutex::new(());
        let guard = mutex.lock().unwrap();
        assert!(lock_before_deadline(&mutex, Instant::now() + Duration::from_millis(20)).is_err());
        drop(guard);
        assert!(lock_before_deadline(&mutex, Instant::now() + Duration::from_secs(1)).is_ok());
    }

    #[test]
    fn accepts_both_kio_folder_reply_versions_and_reports_existing_items() {
        // Build actual D-Bus messages so this exercises signature validation.
        let unsigned =
            zbus::Message::signal("/org/bexplorer/Test", "org.bexplorer.Test", "Created")
                .unwrap()
                .build(&42_u32)
                .unwrap();
        let signed = zbus::Message::signal("/org/bexplorer/Test", "org.bexplorer.Test", "Created")
            .unwrap()
            .build(&42_i32)
            .unwrap();
        let existing =
            zbus::Message::signal("/org/bexplorer/Test", "org.bexplorer.Test", "Created")
                .unwrap()
                .build(&(-1_i32))
                .unwrap();
        assert_eq!(kio_created_folder_id(&unsigned).unwrap(), 42);
        assert_eq!(kio_created_folder_id(&signed).unwrap(), 42);
        assert!(kio_created_folder_id(&existing).is_err());
    }

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
