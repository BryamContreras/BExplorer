use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, Value};

use super::super::{FormatDriveIdentity, FormatDriveOutcome};
use crate::utils::errors::{BExplorerError, Result};

pub(in crate::platform::shell) fn available_format_filesystems(_path: &Path) -> Vec<String> {
    let Ok(connection) = Connection::system() else {
        return Vec::new();
    };
    let Ok(manager) = Proxy::new(
        &connection,
        UDISKS_SERVICE,
        UDISKS_MANAGER_PATH,
        UDISKS_MANAGER_INTERFACE,
    ) else {
        return Vec::new();
    };

    LINUX_FORMAT_FILESYSTEMS
        .iter()
        .filter_map(|filesystem| {
            let availability: zbus::Result<(bool, String)> =
                manager.call("CanFormat", &(*filesystem,));
            availability
                .ok()
                .and_then(|(available, _)| available.then(|| (*filesystem).to_owned()))
        })
        .collect()
}

pub(in crate::platform::shell) fn format_drive_identity(
    path: &Path,
) -> Result<Option<FormatDriveIdentity>> {
    let target = linux_format_target(path)?;
    let connection = Connection::system()
        .map_err(|error| udisks_error("Could not connect to the UDisks system service", error))?;
    let device = resolve_and_validate_udisks_device(&connection, &target)?;
    Ok(Some(device.identity()))
}

pub(in crate::platform::shell) fn format_drive(
    path: &Path,
    filesystem: &str,
    label: &str,
    quick: bool,
    _allocation_unit_size: Option<u64>,
    expected_identity: Option<&FormatDriveIdentity>,
) -> Result<FormatDriveOutcome> {
    let filesystem = normalize_linux_format_filesystem(filesystem).ok_or_else(|| {
        BExplorerError::Operation(format!("Unsupported Linux file system: {filesystem}"))
    })?;
    let target = linux_format_target(path)?;

    let connection = Connection::system()
        .map_err(|error| udisks_error("Could not connect to the UDisks system service", error))?;
    let device = resolve_and_validate_udisks_device(&connection, &target)?;
    let expected_identity = expected_identity.ok_or_else(|| {
        BExplorerError::Operation(
            "The drive identity is missing; close the format dialog and select the drive again"
                .into(),
        )
    })?;
    if &device.identity() != expected_identity {
        return Err(BExplorerError::Operation(
            "The selected drive changed while the format dialog was open; formatting was canceled"
                .into(),
        ));
    }
    if linux_btrfs_has_multiple_devices(&device.block_id_type, &device.block_id_uuid)? {
        return Err(BExplorerError::Operation(
            "The Btrfs device topology changed; formatting was canceled".into(),
        ));
    }
    unmount_udisks_filesystem(&connection, &device)?;

    if let Err(error) = revalidate_udisks_device(&connection, &device) {
        return Err(error_with_remount_attempt(
            &connection,
            &device.object_path,
            error,
        ));
    }

    let block = udisks_proxy(
        &connection,
        device.object_path.as_str(),
        UDISKS_BLOCK_INTERFACE,
    )?;
    let mut options = HashMap::<&str, Value<'_>>::new();
    if !label.trim().is_empty() {
        options.insert("label", Value::from(label.trim()));
    }
    options.insert("update-partition-type", Value::from(true));
    if matches!(filesystem, "ext4" | "btrfs" | "xfs") {
        options.insert("take-ownership", Value::from(true));
    }
    if !quick {
        options.insert("erase", Value::from("zero"));
    }

    let format_result: zbus::Result<()> = block.call("Format", &(filesystem, options));
    if let Err(error) = format_result {
        return Err(error_with_remount_attempt(
            &connection,
            &device.object_path,
            udisks_error("Could not format drive", error),
        ));
    }

    let rescan_options = HashMap::<&str, Value<'_>>::new();
    let _: zbus::Result<()> = block.call("Rescan", &(rescan_options,));

    match mount_udisks_filesystem(&connection, &device.object_path) {
        Ok(mount_path) => Ok(FormatDriveOutcome {
            mount_path: Some(mount_path),
            warning: None,
        }),
        Err(error) => Ok(FormatDriveOutcome {
            mount_path: None,
            warning: Some(format!(
                "The drive was formatted successfully, but it could not be mounted again: {error}"
            )),
        }),
    }
}

fn linux_format_target(path: &Path) -> Result<LinuxMountTarget> {
    let target = linux_mount_target_for_path(path).ok_or_else(|| {
        BExplorerError::Operation(format!(
            "Could not find an exact mounted block device for {}",
            path.display()
        ))
    })?;
    validate_linux_mount_target(&target)?;
    Ok(target)
}

const UDISKS_SERVICE: &str = "org.freedesktop.UDisks2";
const UDISKS_MANAGER_PATH: &str = "/org/freedesktop/UDisks2/Manager";
const UDISKS_MANAGER_INTERFACE: &str = "org.freedesktop.UDisks2.Manager";
const UDISKS_BLOCK_INTERFACE: &str = "org.freedesktop.UDisks2.Block";
const UDISKS_DRIVE_INTERFACE: &str = "org.freedesktop.UDisks2.Drive";
const UDISKS_FILESYSTEM_INTERFACE: &str = "org.freedesktop.UDisks2.Filesystem";
const UDISKS_PARTITION_INTERFACE: &str = "org.freedesktop.UDisks2.Partition";
const UDISKS_PARTITION_TABLE_INTERFACE: &str = "org.freedesktop.UDisks2.PartitionTable";
const LINUX_FORMAT_FILESYSTEMS: &[&str] = &["ext4", "btrfs", "xfs", "exfat", "vfat", "ntfs"];

#[derive(Clone, Debug, PartialEq, Eq)]
struct LinuxMountTarget {
    mount_point: PathBuf,
    source: PathBuf,
}

#[derive(Clone, Debug)]
struct UdisksFormatDevice {
    object_path: OwnedObjectPath,
    drive_path: OwnedObjectPath,
    device_number: u64,
    block_id: String,
    block_id_type: String,
    block_id_uuid: String,
    block_size: u64,
    drive_id: String,
    drive_serial: String,
    drive_wwn: String,
    drive_size: u64,
}

impl UdisksFormatDevice {
    fn identity(&self) -> FormatDriveIdentity {
        FormatDriveIdentity::from_components(vec![
            self.object_path.to_string(),
            self.drive_path.to_string(),
            self.device_number.to_string(),
            self.block_id.clone(),
            self.block_id_type.clone(),
            self.block_id_uuid.clone(),
            self.block_size.to_string(),
            self.drive_id.clone(),
            self.drive_serial.clone(),
            self.drive_wwn.clone(),
            self.drive_size.to_string(),
        ])
    }
}

fn normalize_linux_format_filesystem(filesystem: &str) -> Option<&'static str> {
    let filesystem = filesystem.trim();
    LINUX_FORMAT_FILESYSTEMS
        .iter()
        .copied()
        .find(|candidate| candidate.eq_ignore_ascii_case(filesystem))
}

fn linux_mount_target_for_path(path: &Path) -> Option<LinuxMountTarget> {
    let text = std::fs::read_to_string("/proc/self/mountinfo").ok()?;
    let requested = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let device = std::fs::metadata(&requested).ok()?.dev();
    linux_mount_target_for_path_from_text(
        &text,
        &requested,
        Some((rustix::fs::major(device), rustix::fs::minor(device))),
    )
}

fn linux_mount_target_for_path_from_text(
    text: &str,
    path: &Path,
    expected_device: Option<(u32, u32)>,
) -> Option<LinuxMountTarget> {
    let requested = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let matches = text
        .lines()
        .filter_map(parse_mountinfo_source)
        .filter_map(|(mount_point, source, device)| {
            let canonical_mount = mount_point
                .canonicalize()
                .unwrap_or_else(|_| mount_point.clone());
            (requested == canonical_mount
                && expected_device.is_none_or(|expected| expected == device))
            .then_some(LinuxMountTarget {
                mount_point,
                source,
            })
        })
        .collect::<Vec<_>>();
    let [target] = matches.as_slice() else {
        return None;
    };
    Some(target.clone())
}

fn parse_mountinfo_source(
    line: &str,
) -> Option<(std::path::PathBuf, std::path::PathBuf, (u32, u32))> {
    let (before, after) = line.split_once(" - ")?;
    let before = before.split_whitespace().collect::<Vec<_>>();
    let after = after.split_whitespace().collect::<Vec<_>>();
    if before.len() < 5 || after.len() < 2 {
        return None;
    }
    let (major, minor) = before[2].split_once(':')?;
    Some((
        std::path::PathBuf::from(decode_mount_field(before[4])),
        std::path::PathBuf::from(decode_mount_field(after[1])),
        (major.parse().ok()?, minor.parse().ok()?),
    ))
}

fn decode_mount_field(value: &str) -> String {
    value
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

fn validate_linux_mount_target(target: &LinuxMountTarget) -> Result<()> {
    if linux_mount_point_is_protected(&target.mount_point) {
        return Err(BExplorerError::Operation(
            "System and firmware volumes cannot be formatted from BExplorer".into(),
        ));
    }
    if !target.source.starts_with("/dev") {
        return Err(BExplorerError::Operation(format!(
            "Formatting requires a local block device, not {}",
            target.source.display()
        )));
    }
    let name = target
        .source
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if linux_block_name_is_loop(name) {
        return Err(BExplorerError::Operation(
            "Mounted disk images cannot be formatted as external drives".into(),
        ));
    }
    Ok(())
}

fn linux_mount_point_is_protected(path: &Path) -> bool {
    path == Path::new("/")
        || path == Path::new("/home")
        || path == Path::new("/opt")
        || path == Path::new("/srv")
        || path == Path::new("/usr")
        || path == Path::new("/var")
        || path.starts_with("/boot")
        || path.starts_with("/efi")
}

fn linux_block_name_is_loop(name: &str) -> bool {
    let Some(suffix) = name.strip_prefix("loop") else {
        return false;
    };
    let (device, partition) = suffix.split_once('p').unwrap_or((suffix, ""));
    !device.is_empty()
        && device.bytes().all(|byte| byte.is_ascii_digit())
        && (partition.is_empty() || partition.bytes().all(|byte| byte.is_ascii_digit()))
}

fn resolve_and_validate_udisks_device(
    connection: &Connection,
    target: &LinuxMountTarget,
) -> Result<UdisksFormatDevice> {
    let manager = udisks_proxy(connection, UDISKS_MANAGER_PATH, UDISKS_MANAGER_INTERFACE)?;
    let source = target.source.to_string_lossy().into_owned();
    let mut specification = HashMap::<&str, Value<'_>>::new();
    specification.insert("path", Value::from(source.as_str()));
    let options = HashMap::<&str, Value<'_>>::new();
    let devices: Vec<OwnedObjectPath> = manager
        .call("ResolveDevice", &(specification, options))
        .map_err(|error| udisks_error("Could not resolve the selected block device", error))?;
    let [object_path] = devices.as_slice() else {
        return Err(BExplorerError::Operation(format!(
            "UDisks resolved {} to {} devices; exactly one is required",
            target.source.display(),
            devices.len()
        )));
    };

    let block = udisks_proxy(connection, object_path.as_str(), UDISKS_BLOCK_INTERFACE)?;
    let read_only: bool = udisks_property(&block, "ReadOnly")?;
    let hint_ignore: bool = udisks_property(&block, "HintIgnore")?;
    let id_usage: String = udisks_property(&block, "IdUsage")?;
    let drive_path: OwnedObjectPath = udisks_property(&block, "Drive")?;
    let crypto_backing: OwnedObjectPath = udisks_property(&block, "CryptoBackingDevice")?;
    let mdraid: OwnedObjectPath = udisks_property(&block, "MDRaid")?;
    let mdraid_member: OwnedObjectPath = udisks_property(&block, "MDRaidMember")?;
    let device_number: u64 = udisks_property(&block, "DeviceNumber")?;
    let block_id: String = udisks_property(&block, "Id")?;
    let block_id_type: String = udisks_property(&block, "IdType")?;
    let block_id_uuid: String = udisks_property(&block, "IdUUID")?;
    let block_size: u64 = udisks_property(&block, "Size")?;

    if read_only {
        return Err(BExplorerError::Operation(
            "The selected drive is read-only".into(),
        ));
    }
    if hint_ignore {
        return Err(BExplorerError::Operation(
            "UDisks identifies this as a hidden device; formatting was blocked".into(),
        ));
    }
    if id_usage != "filesystem" {
        return Err(BExplorerError::Operation(format!(
            "Only ordinary filesystems can be formatted safely (detected usage: {id_usage})"
        )));
    }
    if drive_path.as_str() == "/" {
        return Err(BExplorerError::Operation(
            "The selected volume is not backed by a directly identifiable physical drive".into(),
        ));
    }
    if udisks_drive_hosts_system_mount(connection, &drive_path)? {
        return Err(BExplorerError::Operation(
            "The selected disk contains a mounted system volume; formatting was blocked".into(),
        ));
    }
    if crypto_backing.as_str() != "/" || mdraid.as_str() != "/" || mdraid_member.as_str() != "/" {
        return Err(BExplorerError::Operation(
            "Encrypted, RAID, and layered storage cannot be formatted from BExplorer".into(),
        ));
    }
    if linux_block_has_holders(device_number)? {
        return Err(BExplorerError::Operation(
            "The selected block device is in use by another storage layer; formatting was blocked"
                .into(),
        ));
    }
    if linux_btrfs_has_multiple_devices(&block_id_type, &block_id_uuid)? {
        return Err(BExplorerError::Operation(
            "Multi-device Btrfs filesystems cannot be formatted from BExplorer".into(),
        ));
    }
    if udisks_partition_table_has_partitions(connection, object_path.as_str())? {
        return Err(BExplorerError::Operation(
            "A whole disk containing partitions cannot be formatted as a single filesystem from BExplorer"
                .into(),
        ));
    }
    if udisks_partition_type(connection, object_path.as_str())?
        .as_deref()
        .is_some_and(linux_partition_type_is_firmware)
    {
        return Err(BExplorerError::Operation(
            "EFI and boot-loader partitions cannot be formatted from BExplorer".into(),
        ));
    }

    let drive = udisks_proxy(connection, drive_path.as_str(), UDISKS_DRIVE_INTERFACE)?;
    let optical: bool = udisks_property(&drive, "Optical")?;
    let drive_id: String = udisks_property(&drive, "Id")?;
    let drive_serial: String = udisks_property(&drive, "Serial")?;
    let drive_wwn: String = udisks_property(&drive, "WWN")?;
    let drive_size: u64 = udisks_property(&drive, "Size")?;
    if optical {
        return Err(BExplorerError::Operation(
            "Optical media cannot be formatted from BExplorer".into(),
        ));
    }

    let filesystem = udisks_proxy(
        connection,
        object_path.as_str(),
        UDISKS_FILESYSTEM_INTERFACE,
    )?;
    let mount_points: Vec<Vec<u8>> = udisks_property(&filesystem, "MountPoints")?;
    let mount_points = mount_points
        .iter()
        .map(|bytes| {
            udisks_mount_point(bytes).ok_or_else(|| {
                BExplorerError::Operation("UDisks returned an invalid mount path".into())
            })
        })
        .collect::<Result<Vec<_>>>()?;
    if mount_points
        .iter()
        .any(|path| linux_mount_point_is_protected(path))
    {
        return Err(BExplorerError::Operation(
            "The selected filesystem is also mounted on a system or firmware path; formatting was blocked"
                .into(),
        ));
    }
    if !mount_points
        .iter()
        .any(|mount_point| same_path(mount_point, &target.mount_point))
    {
        return Err(BExplorerError::Operation(format!(
            "{} is no longer mounted on the selected device",
            target.mount_point.display()
        )));
    }

    Ok(UdisksFormatDevice {
        object_path: object_path.clone(),
        drive_path: drive_path.clone(),
        device_number,
        block_id,
        block_id_type,
        block_id_uuid,
        block_size,
        drive_id,
        drive_serial,
        drive_wwn,
        drive_size,
    })
}

fn linux_partition_type_is_firmware(partition_type: &str) -> bool {
    matches!(
        partition_type.trim().to_ascii_lowercase().as_str(),
        "c12a7328-f81f-11d2-ba4b-00a0c93ec93b"
            | "bc13c2ff-59e6-4262-a352-b275fd6f7172"
            | "0xef"
            | "ef"
    )
}

fn unmount_udisks_filesystem(connection: &Connection, device: &UdisksFormatDevice) -> Result<()> {
    let filesystem = udisks_proxy(
        connection,
        device.object_path.as_str(),
        UDISKS_FILESYSTEM_INTERFACE,
    )?;
    let options = HashMap::<&str, Value<'_>>::new();
    filesystem
        .call::<_, _, ()>("Unmount", &(options,))
        .map_err(|error| {
            udisks_error(
                "Could not unmount the drive; close files and applications that are using it",
                error,
            )
        })?;

    let filesystem = udisks_proxy(
        connection,
        device.object_path.as_str(),
        UDISKS_FILESYSTEM_INTERFACE,
    )?;
    let mount_points: Vec<Vec<u8>> = udisks_property(&filesystem, "MountPoints")?;
    if !mount_points.is_empty() {
        return Err(BExplorerError::Operation(
            "UDisks reported success but the filesystem still has active mount points".into(),
        ));
    }
    Ok(())
}

fn revalidate_udisks_device(connection: &Connection, device: &UdisksFormatDevice) -> Result<()> {
    let block = udisks_proxy(
        connection,
        device.object_path.as_str(),
        UDISKS_BLOCK_INTERFACE,
    )?;
    let device_number: u64 = udisks_property(&block, "DeviceNumber")?;
    let drive_path: OwnedObjectPath = udisks_property(&block, "Drive")?;
    let read_only: bool = udisks_property(&block, "ReadOnly")?;
    let hint_ignore: bool = udisks_property(&block, "HintIgnore")?;
    let id_usage: String = udisks_property(&block, "IdUsage")?;
    let crypto_backing: OwnedObjectPath = udisks_property(&block, "CryptoBackingDevice")?;
    let mdraid: OwnedObjectPath = udisks_property(&block, "MDRaid")?;
    let mdraid_member: OwnedObjectPath = udisks_property(&block, "MDRaidMember")?;
    let block_id: String = udisks_property(&block, "Id")?;
    let block_id_type: String = udisks_property(&block, "IdType")?;
    let block_id_uuid: String = udisks_property(&block, "IdUUID")?;
    let block_size: u64 = udisks_property(&block, "Size")?;
    let has_holders = linux_block_has_holders(device_number)?;
    let has_partitions =
        udisks_partition_table_has_partitions(connection, device.object_path.as_str())?;
    let hosts_system_mount = udisks_drive_hosts_system_mount(connection, &drive_path)?;
    if device_number != device.device_number
        || drive_path != device.drive_path
        || block_id != device.block_id
        || block_id_type != device.block_id_type
        || block_id_uuid != device.block_id_uuid
        || block_size != device.block_size
        || read_only
        || hint_ignore
        || id_usage != "filesystem"
        || crypto_backing.as_str() != "/"
        || mdraid.as_str() != "/"
        || mdraid_member.as_str() != "/"
        || has_holders
        || has_partitions
        || hosts_system_mount
    {
        return Err(BExplorerError::Operation(
            "The selected block device changed after it was unmounted; formatting was canceled"
                .into(),
        ));
    }
    if udisks_partition_type(connection, device.object_path.as_str())?
        .as_deref()
        .is_some_and(linux_partition_type_is_firmware)
    {
        return Err(BExplorerError::Operation(
            "The selected partition is reserved for firmware or boot files; formatting was canceled"
                .into(),
        ));
    }
    let drive = udisks_proxy(connection, drive_path.as_str(), UDISKS_DRIVE_INTERFACE)?;
    let drive_id: String = udisks_property(&drive, "Id")?;
    let drive_serial: String = udisks_property(&drive, "Serial")?;
    let drive_wwn: String = udisks_property(&drive, "WWN")?;
    let drive_size: u64 = udisks_property(&drive, "Size")?;
    let optical: bool = udisks_property(&drive, "Optical")?;
    if drive_id != device.drive_id
        || drive_serial != device.drive_serial
        || drive_wwn != device.drive_wwn
        || drive_size != device.drive_size
        || optical
    {
        return Err(BExplorerError::Operation(
            "The external drive identity changed after unmounting; formatting was canceled".into(),
        ));
    }
    Ok(())
}

fn mount_udisks_filesystem(
    connection: &Connection,
    object_path: &OwnedObjectPath,
) -> Result<PathBuf> {
    let mut last_error = None;
    for attempt in 0..20 {
        let filesystem = udisks_proxy(
            connection,
            object_path.as_str(),
            UDISKS_FILESYSTEM_INTERFACE,
        )?;
        let options = HashMap::<&str, Value<'_>>::new();
        let result: zbus::Result<String> = filesystem.call("Mount", &(options,));
        match result {
            Ok(path) if !path.trim().is_empty() => return Ok(PathBuf::from(path)),
            Ok(_) => {
                return Err(BExplorerError::Operation(
                    "UDisks mounted the drive without returning its mount path".into(),
                ));
            }
            Err(error) if attempt < 19 && udisks_interface_is_refreshing(&error) => {
                last_error = Some(error.to_string());
                thread::sleep(Duration::from_millis(150));
            }
            Err(error) => {
                return Err(udisks_error("Could not mount the drive", error));
            }
        }
    }
    Err(BExplorerError::Operation(format!(
        "The formatted filesystem did not become mountable in time{}",
        last_error
            .map(|error| format!(": {error}"))
            .unwrap_or_default()
    )))
}

fn error_with_remount_attempt(
    connection: &Connection,
    object_path: &OwnedObjectPath,
    error: BExplorerError,
) -> BExplorerError {
    match mount_udisks_filesystem(connection, object_path) {
        Ok(_) => error,
        Err(remount_error) => BExplorerError::Operation(format!(
            "{error}. The drive also could not be mounted again: {remount_error}"
        )),
    }
}

fn udisks_proxy<'a>(
    connection: &Connection,
    path: &'a str,
    interface: &'a str,
) -> Result<Proxy<'a>> {
    Proxy::new(connection, UDISKS_SERVICE, path, interface)
        .map_err(|error| udisks_error("Could not access the UDisks device", error))
}

fn udisks_partition_type(connection: &Connection, object_path: &str) -> Result<Option<String>> {
    udisks_optional_property(connection, object_path, UDISKS_PARTITION_INTERFACE, "Type")
}

fn udisks_partition_table_has_partitions(
    connection: &Connection,
    object_path: &str,
) -> Result<bool> {
    let partitions: Option<Vec<OwnedObjectPath>> = udisks_optional_property(
        connection,
        object_path,
        UDISKS_PARTITION_TABLE_INTERFACE,
        "Partitions",
    )?;
    Ok(partitions.is_some_and(|partitions| !partitions.is_empty()))
}

fn udisks_drive_hosts_system_mount(
    connection: &Connection,
    selected_drive: &OwnedObjectPath,
) -> Result<bool> {
    let mountinfo = std::fs::read_to_string("/proc/self/mountinfo").map_err(|error| {
        BExplorerError::Operation(format!(
            "Could not identify the disks containing system volumes: {error}"
        ))
    })?;
    let system_sources = linux_system_mount_sources(&mountinfo);
    if system_sources.is_empty() {
        return Err(BExplorerError::Operation(
            "Could not identify the block device containing the operating system".into(),
        ));
    }

    let manager = udisks_proxy(connection, UDISKS_MANAGER_PATH, UDISKS_MANAGER_INTERFACE)?;
    for source in system_sources {
        let source = source.to_string_lossy().into_owned();
        let mut specification = HashMap::<&str, Value<'_>>::new();
        specification.insert("path", Value::from(source.as_str()));
        let options = HashMap::<&str, Value<'_>>::new();
        let devices: Vec<OwnedObjectPath> = manager
            .call("ResolveDevice", &(specification, options))
            .map_err(|error| {
                udisks_error("Could not resolve a mounted system block device", error)
            })?;
        let [block_path] = devices.as_slice() else {
            return Err(BExplorerError::Operation(format!(
                "UDisks resolved the system source {source} to {} devices; exactly one is required",
                devices.len()
            )));
        };
        let block = udisks_proxy(connection, block_path.as_str(), UDISKS_BLOCK_INTERFACE)?;
        let system_drive: OwnedObjectPath = udisks_property(&block, "Drive")?;
        if system_drive.as_str() == "/" {
            return Err(BExplorerError::Operation(format!(
                "Could not trace the system source {source} to a physical disk"
            )));
        }
        if system_drive == *selected_drive {
            return Ok(true);
        }
    }
    Ok(false)
}

fn linux_system_mount_sources(mountinfo: &str) -> HashSet<PathBuf> {
    mountinfo
        .lines()
        .filter_map(parse_mountinfo_source)
        .filter(|(mount_point, source, _)| {
            linux_mount_point_is_protected(mount_point) && source.starts_with("/dev")
        })
        .map(|(_, source, _)| source)
        .collect()
}

fn udisks_optional_property<T>(
    connection: &Connection,
    object_path: &str,
    interface: &str,
    property: &str,
) -> Result<Option<T>>
where
    T: TryFrom<zbus::zvariant::OwnedValue>,
    T::Error: Into<zbus::Error>,
{
    let proxy = udisks_proxy(connection, object_path, interface)?;
    match proxy.get_property(property) {
        Ok(value) => Ok(Some(value)),
        Err(error) if udisks_optional_interface_is_absent(&error) => Ok(None),
        Err(error) => Err(udisks_error(
            &format!("Could not read UDisks property {interface}.{property}"),
            error,
        )),
    }
}

fn udisks_optional_interface_is_absent(error: &zbus::Error) -> bool {
    match error {
        zbus::Error::InterfaceNotFound => true,
        zbus::Error::MethodError(name, detail, _) => match name.as_str() {
            "org.freedesktop.DBus.Error.UnknownInterface"
            | "org.freedesktop.DBus.Error.UnknownMethod" => true,
            "org.freedesktop.DBus.Error.InvalidArgs" => detail
                .as_deref()
                .is_some_and(dbus_message_reports_missing_interface),
            _ => false,
        },
        zbus::Error::FDO(error) => match error.as_ref() {
            zbus::fdo::Error::UnknownInterface(_) | zbus::fdo::Error::UnknownMethod(_) => true,
            zbus::fdo::Error::InvalidArgs(detail) => dbus_message_reports_missing_interface(detail),
            _ => false,
        },
        _ => false,
    }
}

fn dbus_message_reports_missing_interface(detail: &str) -> bool {
    let detail = detail.to_ascii_lowercase();
    detail.contains("no such interface") || detail.contains("unknown interface")
}

fn udisks_property<T>(proxy: &Proxy<'_>, property: &str) -> Result<T>
where
    T: TryFrom<zbus::zvariant::OwnedValue>,
    T::Error: Into<zbus::Error>,
{
    proxy
        .get_property(property)
        .map_err(|error| udisks_error(&format!("Could not read UDisks property {property}"), error))
}

fn linux_block_has_holders(device_number: u64) -> Result<bool> {
    let major = rustix::fs::major(device_number);
    let minor = rustix::fs::minor(device_number);
    let holders = PathBuf::from(format!("/sys/dev/block/{major}:{minor}/holders"));
    let mut entries = std::fs::read_dir(&holders).map_err(|error| {
        BExplorerError::Operation(format!(
            "Could not verify whether the selected block device has active users: {error}"
        ))
    })?;
    entries
        .next()
        .transpose()
        .map(|entry| entry.is_some())
        .map_err(|error| {
            BExplorerError::Operation(format!(
                "Could not inspect the selected block device holders: {error}"
            ))
        })
}

fn linux_btrfs_has_multiple_devices(id_type: &str, id_uuid: &str) -> Result<bool> {
    linux_btrfs_has_multiple_devices_at(Path::new("/sys/fs/btrfs"), id_type, id_uuid)
}

fn linux_btrfs_has_multiple_devices_at(
    sysfs_root: &Path,
    id_type: &str,
    id_uuid: &str,
) -> Result<bool> {
    if !id_type.eq_ignore_ascii_case("btrfs") {
        return Ok(false);
    }
    let id_uuid = id_uuid.trim();
    if id_uuid.is_empty() {
        return Err(BExplorerError::Operation(
            "Could not verify the identity of the selected Btrfs filesystem".into(),
        ));
    }
    // `devices` only exposes members currently present. `devinfo` also keeps
    // the missing members of a degraded multi-device filesystem, which must
    // still make the format operation unsafe.
    let devinfo = sysfs_root.join(id_uuid).join("devinfo");
    let mut entries = std::fs::read_dir(&devinfo).map_err(|error| {
        BExplorerError::Operation(format!(
            "Could not inspect the selected Btrfs filesystem devices: {error}"
        ))
    })?;
    let first = entries.next().transpose().map_err(|error| {
        BExplorerError::Operation(format!(
            "Could not inspect the selected Btrfs filesystem devices: {error}"
        ))
    })?;
    if first.is_none() {
        return Err(BExplorerError::Operation(
            "The selected Btrfs filesystem did not report any backing devices".into(),
        ));
    }
    entries
        .next()
        .transpose()
        .map(|entry| entry.is_some())
        .map_err(|error| {
            BExplorerError::Operation(format!(
                "Could not inspect the selected Btrfs filesystem devices: {error}"
            ))
        })
}

fn udisks_mount_point(bytes: &[u8]) -> Option<PathBuf> {
    let bytes = bytes.strip_suffix(&[0]).unwrap_or(bytes);
    (!bytes.is_empty()).then(|| PathBuf::from(OsString::from_vec(bytes.to_vec())))
}

fn same_path(left: &Path, right: &Path) -> bool {
    let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
    left == right
}

fn udisks_interface_is_refreshing(error: &zbus::Error) -> bool {
    let error = error.to_string().to_ascii_lowercase();
    error.contains("unknownmethod")
        || error.contains("unknowninterface")
        || error.contains("unknownobject")
        || error.contains("no such interface")
}

fn udisks_error(context: &str, error: impl std::fmt::Display) -> BExplorerError {
    let detail = error.to_string();
    let detail_lower = detail.to_ascii_lowercase();
    let hint = if detail_lower.contains("notauthorized") || detail_lower.contains("authorization") {
        " Administrator authorization may have been canceled or denied."
    } else if detail_lower.contains("devicebusy") || detail_lower.contains("busy") {
        " Close files and applications that are using the drive, then try again."
    } else {
        ""
    };
    BExplorerError::Operation(format!("{context}: {detail}.{hint}"))
}

#[cfg(test)]
mod linux_tests {
    use super::*;

    #[test]
    fn linux_format_filesystem_is_limited_to_the_supported_allowlist() {
        assert_eq!(normalize_linux_format_filesystem(" EXT4 "), Some("ext4"));
        assert_eq!(normalize_linux_format_filesystem("ExFaT"), Some("exfat"));
        assert_eq!(normalize_linux_format_filesystem("vfat"), Some("vfat"));
        assert_eq!(normalize_linux_format_filesystem("ext3"), None);
        assert_eq!(normalize_linux_format_filesystem("ext4; reboot"), None);
    }

    #[test]
    fn mountinfo_lookup_requires_the_exact_mount_root_and_decodes_spaces() {
        let mountinfo = concat!(
            "35 24 8:17 / /media/dev/My\\040USB rw,nosuid,nodev - exfat /dev/sdb1 rw\n",
            "36 24 8:18 / /media/dev/Other rw,nosuid,nodev - ext4 /dev/sdc1 rw\n",
        );

        let target = linux_mount_target_for_path_from_text(
            mountinfo,
            Path::new("/media/dev/My USB"),
            Some((8, 17)),
        )
        .expect("the exact mount root should be found");
        assert_eq!(target.mount_point, Path::new("/media/dev/My USB"));
        assert_eq!(target.source, Path::new("/dev/sdb1"));

        assert!(
            linux_mount_target_for_path_from_text(
                mountinfo,
                Path::new("/media/dev/My USB/documents"),
                Some((8, 17)),
            )
            .is_none()
        );
    }

    #[test]
    fn mountinfo_lookup_uses_the_visible_device_for_stacked_mounts() {
        let mountinfo = concat!(
            "35 24 8:17 / /media/dev/USB rw - ext4 /dev/sdb1 rw\n",
            "36 24 8:33 / /media/dev/USB rw - exfat /dev/sdc1 rw\n",
        );

        let target = linux_mount_target_for_path_from_text(
            mountinfo,
            Path::new("/media/dev/USB"),
            Some((8, 33)),
        )
        .expect("the device reported by stat should disambiguate stacked mounts");
        assert_eq!(target.source, Path::new("/dev/sdc1"));
        assert!(
            linux_mount_target_for_path_from_text(mountinfo, Path::new("/media/dev/USB"), None,)
                .is_none()
        );
    }

    #[test]
    fn linux_mount_target_blocks_system_non_block_and_loop_sources() {
        let target = |mount_point: &str, source: &str| LinuxMountTarget {
            mount_point: PathBuf::from(mount_point),
            source: PathBuf::from(source),
        };

        assert!(validate_linux_mount_target(&target("/media/dev/USB", "/dev/sdb1")).is_ok());
        assert!(validate_linux_mount_target(&target("/", "/dev/sda2")).is_err());
        assert!(validate_linux_mount_target(&target("/boot", "/dev/sda1")).is_err());
        assert!(validate_linux_mount_target(&target("/boot/firmware", "/dev/mmcblk0p1")).is_err());
        assert!(validate_linux_mount_target(&target("/boot/efi", "/dev/sda1")).is_err());
        assert!(validate_linux_mount_target(&target("/efi/EFI", "/dev/sda1")).is_err());
        assert!(validate_linux_mount_target(&target("/mnt/share", "server:/share")).is_err());
        assert!(validate_linux_mount_target(&target("/media/dev/ISO", "/dev/loop7p2")).is_err());
    }

    #[test]
    fn loop_device_detection_does_not_confuse_regular_block_devices() {
        assert!(linux_block_name_is_loop("loop0"));
        assert!(linux_block_name_is_loop("loop12p3"));
        assert!(!linux_block_name_is_loop("loop"));
        assert!(!linux_block_name_is_loop("sdb1"));
        assert!(!linux_block_name_is_loop("nvme0n1p1"));
    }

    #[test]
    fn system_mount_sources_exclude_secondary_data_mounts() {
        let mountinfo = concat!(
            "35 24 8:1 / / rw,relatime - ext4 /dev/sda1 rw\n",
            "36 24 8:17 / /media/dev/PRUEBAS rw,relatime - ext4 /dev/sdb1 rw\n",
            "37 24 8:33 / /home rw,relatime - ext4 /dev/sdc1 rw\n",
            "38 24 0:55 / /var/lib/data rw,relatime - tmpfs tmpfs rw\n",
        );

        let sources = linux_system_mount_sources(mountinfo);

        assert_eq!(sources.len(), 2);
        assert!(sources.contains(Path::new("/dev/sda1")));
        assert!(sources.contains(Path::new("/dev/sdc1")));
        assert!(!sources.contains(Path::new("/dev/sdb1")));
    }

    #[test]
    fn firmware_partition_types_are_always_blocked() {
        assert!(linux_partition_type_is_firmware(
            "c12a7328-f81f-11d2-ba4b-00a0c93ec93b"
        ));
        assert!(linux_partition_type_is_firmware(
            "bc13c2ff-59e6-4262-a352-b275fd6f7172"
        ));
        assert!(linux_partition_type_is_firmware("0xEF"));
        assert!(!linux_partition_type_is_firmware("0x83"));
    }

    #[test]
    fn btrfs_requires_a_verifiable_single_device_filesystem() {
        assert!(!linux_btrfs_has_multiple_devices("ext4", "").unwrap());
        assert!(linux_btrfs_has_multiple_devices("btrfs", "").is_err());

        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "bexplorer-btrfs-sysfs-test-{}-{unique}",
            std::process::id()
        ));
        let devinfo = root.join("example-fsid/devinfo");
        std::fs::create_dir_all(devinfo.join("1")).unwrap();
        assert!(!linux_btrfs_has_multiple_devices_at(&root, "btrfs", "example-fsid").unwrap());

        // `devinfo` retains the DEVID of a missing member in a degraded
        // filesystem, so a second directory must always block formatting.
        std::fs::create_dir_all(devinfo.join("2")).unwrap();
        assert!(linux_btrfs_has_multiple_devices_at(&root, "btrfs", "example-fsid").unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn format_identity_changes_when_the_physical_drive_changes() {
        let device = UdisksFormatDevice {
            object_path: OwnedObjectPath::try_from("/org/freedesktop/UDisks2/block_devices/sdb1")
                .unwrap(),
            drive_path: OwnedObjectPath::try_from("/org/freedesktop/UDisks2/drives/Example")
                .unwrap(),
            device_number: 2_049,
            block_id: "by-uuid-example".into(),
            block_id_type: "ext4".into(),
            block_id_uuid: "11111111-2222-3333-4444-555555555555".into(),
            block_size: 64 * 1024 * 1024,
            drive_id: "Example".into(),
            drive_serial: "SERIAL-A".into(),
            drive_wwn: String::new(),
            drive_size: 64 * 1024 * 1024,
        };
        let mut replacement = device.clone();
        replacement.drive_serial = "SERIAL-B".into();

        assert_ne!(device.identity(), replacement.identity());
    }

    #[test]
    fn optional_interfaces_only_ignore_dbus_absence_errors() {
        let absent: zbus::Error = zbus::fdo::Error::UnknownInterface("missing".into()).into();
        assert!(udisks_optional_interface_is_absent(&absent));

        // UDisks/GLib reports a missing optional interface as InvalidArgs
        // when org.freedesktop.DBus.Properties.Get is used.
        let glib_absent: zbus::Error = zbus::fdo::Error::InvalidArgs(
            "No such interface “org.freedesktop.UDisks2.PartitionTable”".into(),
        )
        .into();
        assert!(udisks_optional_interface_is_absent(&glib_absent));

        let unrelated_invalid_args: zbus::Error =
            zbus::fdo::Error::InvalidArgs("Invalid property type".into()).into();
        assert!(!udisks_optional_interface_is_absent(
            &unrelated_invalid_args
        ));
        assert!(!udisks_optional_interface_is_absent(&zbus::Error::Failure(
            "permission denied".into()
        )));
    }

    #[test]
    fn udisks_mount_point_removes_the_terminal_nul_only() {
        assert_eq!(
            udisks_mount_point(b"/media/dev/My USB\0"),
            Some(PathBuf::from("/media/dev/My USB"))
        );
        assert_eq!(udisks_mount_point(b""), None);
        assert_eq!(udisks_mount_point(b"\0"), None);
    }

    #[test]
    fn mountinfo_escape_decoding_is_not_applied_twice() {
        assert_eq!(decode_mount_field(r"/x\134040y"), r"/x\040y");
    }
}
