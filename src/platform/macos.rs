//! macOS-specific platform hooks live here.
//!
//! Keep AppKit, Finder, Uniform Type Identifier, and volume integrations behind
//! the neutral functions exported from `crate::platform`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{NetworkComputerInfo, NetworkDeviceKind, NetworkShareInfo};

pub fn network_computers() -> Vec<NetworkComputerInfo> {
    let mut seen = HashSet::new();
    mounted_smb_volumes()
        .into_iter()
        .filter(|volume| seen.insert(volume.host.to_ascii_lowercase()))
        .map(|volume| NetworkComputerInfo {
            name: volume.host,
            comment: "Mounted SMB host".into(),
            kind: NetworkDeviceKind::Computer,
        })
        .collect()
}

pub fn network_shares(host: &str) -> Vec<NetworkShareInfo> {
    mounted_smb_volumes()
        .into_iter()
        .filter(|volume| volume.host.eq_ignore_ascii_case(host))
        .map(|volume| NetworkShareInfo {
            name: volume.share,
            remark: "Mounted SMB share".into(),
        })
        .collect()
}

pub fn mounted_network_path(path: &Path) -> Option<PathBuf> {
    let (host, share, children) = unc_parts(path)?;
    let volume = mounted_smb_volumes().into_iter().find(|volume| {
        volume.host.eq_ignore_ascii_case(host) && volume.share.eq_ignore_ascii_case(share)
    })?;
    Some(
        children
            .iter()
            .fold(volume.mount_point, |path, child| path.join(child)),
    )
}

struct MountedSmbVolume {
    host: String,
    share: String,
    mount_point: PathBuf,
}

fn mounted_smb_volumes() -> Vec<MountedSmbVolume> {
    let output = Command::new("mount").output().ok();
    let text = output
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default();
    text.lines().filter_map(parse_smb_mount).collect()
}

fn parse_smb_mount(line: &str) -> Option<MountedSmbVolume> {
    let (remote, mounted) = line.split_once(" on ")?;
    if !mounted.contains("smbfs") {
        return None;
    }
    let remote = remote.trim_start_matches('/');
    let remote = remote
        .rsplit_once('@')
        .map(|(_, value)| value)
        .unwrap_or(remote);
    let (host, share) = remote.split_once('/')?;
    let mount_point = mounted
        .split_once(" (")
        .map(|(path, _)| path)
        .unwrap_or(mounted);
    Some(MountedSmbVolume {
        host: host.to_string(),
        share: share.to_string(),
        mount_point: PathBuf::from(mount_point),
    })
}

fn unc_parts(path: &Path) -> Option<(&str, &str, Vec<&str>)> {
    let text = path.to_str()?.trim_start_matches(['\\', '/']);
    let mut parts = text.split(['\\', '/']).filter(|part| !part.is_empty());
    let host = parts.next()?;
    let share = parts.next()?;
    Some((host, share, parts.collect()))
}
