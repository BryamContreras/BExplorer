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
    let partition_type = linux_udev_partition_type(&mount.major_minor);
    linux_mount_is_storage_candidate_with_partition_type(mount, partition_type.as_deref())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_mount_is_storage_candidate_with_partition_type(
    mount: &LinuxMount,
    partition_type: Option<&str>,
) -> bool {
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
    if linux_mount_is_firmware_partition(mount, partition_type) {
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
fn linux_mount_is_firmware_partition(
    mount: &LinuxMount,
    partition_type: Option<&str>,
) -> bool {
    linux_path_is_firmware_mount(&mount.mount_point, &mount.fs_type)
        || partition_type.is_some_and(linux_partition_type_is_firmware)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_path_is_firmware_mount(path: &Path, fs_type: &str) -> bool {
    path == Path::new("/boot/efi")
        || path == Path::new("/efi")
        || (path == Path::new("/boot") && linux_fs_type_is_fat(fs_type))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_fs_type_is_fat(fs_type: &str) -> bool {
    matches!(
        fs_type.trim().to_ascii_lowercase().as_str(),
        "fat" | "fat12" | "fat16" | "fat32" | "msdos" | "vfat"
    )
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_partition_type_is_firmware(partition_type: &str) -> bool {
    matches!(
        partition_type.trim().to_ascii_lowercase().as_str(),
        // GPT EFI System Partition and Extended Boot Loader Partition.
        "c12a7328-f81f-11d2-ba4b-00a0c93ec93b"
            | "bc13c2ff-59e6-4262-a352-b275fd6f7172"
            // MBR EFI System Partition identifier, as reported by udev.
            | "0xef"
            | "ef"
    )
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_udev_partition_type(major_minor: &str) -> Option<String> {
    if major_minor.is_empty()
        || !major_minor
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b':')
    {
        return None;
    }

    let data = fs::read_to_string(
        Path::new("/run/udev/data").join(format!("b{major_minor}")),
    )
    .ok()?;
    data.lines().find_map(|line| {
        line.strip_prefix("E:ID_PART_ENTRY_TYPE=")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
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
    if linux_mount_source_is_loop(mount) {
        return DriveKind::External;
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
fn linux_mount_source_is_loop(mount: &LinuxMount) -> bool {
    linux_mount_block_name(mount).is_some_and(|name| {
        let Some(suffix) = name.strip_prefix("loop") else {
            return false;
        };
        let (device, partition) = suffix.split_once('p').unwrap_or((suffix, ""));
        !device.is_empty()
            && device.bytes().all(|byte| byte.is_ascii_digit())
            && (partition.is_empty() || partition.bytes().all(|byte| byte.is_ascii_digit()))
    })
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
