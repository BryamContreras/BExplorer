//! Conservative KDE/KIO network discovery.
//!
//! KIO remains an additional source: GVfs, Avahi and Samba continue to be
//! queried by the parent module. Stable local state (Dolphin places and active
//! KIOFuse mounts) is preferred over active network browsing.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use crate::platform::{NetworkComputerInfo, NetworkDeviceKind, NetworkShareInfo};

const DISCOVERY_BUDGET: Duration = Duration::from_millis(2_500);
const COMMAND_TIMEOUT: Duration = Duration::from_millis(900);
const MAX_WORKGROUPS: usize = 4;
const DISCOVERY_CACHE_TTL: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, PartialEq, Eq)]
struct SmbLocation {
    host: String,
    share: Option<String>,
}

#[derive(Debug)]
struct Tools {
    client: Option<PathBuf>,
    fuse_mounts: Vec<PathBuf>,
}

type NetworkCache = Option<(Instant, Vec<NetworkComputerInfo>)>;

static NETWORK_CACHE: OnceLock<Mutex<NetworkCache>> = OnceLock::new();

pub(super) fn network_computers() -> Vec<NetworkComputerInfo> {
    let tools = detect_tools();
    let mut computers = smb_locations(&tools.fuse_mounts)
        .into_iter()
        .map(|location| NetworkComputerInfo {
            name: location.host,
            comment: "KIO network location".into(),
            kind: NetworkDeviceKind::Computer,
        })
        .collect::<Vec<_>>();

    // `smb:/` may wait on an unreachable network. Probe only in Plasma, with
    // no dialogs or display connection, and enforce one short shared budget.
    if is_kde_session()
        && let Some(client) = tools.client.as_deref()
    {
        computers.extend(network_computers_from_client_cached(client));
    }

    super::dedupe_network_computers(computers)
}

pub(super) fn network_shares(host: &str) -> Vec<NetworkShareInfo> {
    let tools = detect_tools();
    let mut shares = smb_locations(&tools.fuse_mounts)
        .into_iter()
        .filter(|location| {
            super::network_host_identity(&location.host) == super::network_host_identity(host)
        })
        .filter_map(|location| {
            location.share.map(|name| NetworkShareInfo {
                name,
                remark: "KIO saved location".into(),
            })
        })
        .collect::<Vec<_>>();

    // KIO may already have credentials that an anonymous smbclient call does
    // not. Listing one explicitly selected host is still bounded and silent.
    if is_kde_session()
        && let Some(client) = tools.client.as_deref()
        && let Some(listing) = run_ls(client, &format!("smb://{host}"), COMMAND_TIMEOUT)
    {
        shares.extend(
            parse_listing_names(&listing)
                .into_iter()
                .filter(|name| is_plausible_share(name))
                .map(|name| NetworkShareInfo {
                    name,
                    remark: "KIO SMB share".into(),
                }),
        );
    }

    dedupe_shares(shares)
}

pub(super) fn mounted_network_path(host: &str, share: &str, children: &[&str]) -> Option<PathBuf> {
    mounted_network_path_in_roots(&kio_fuse_mount_roots(), host, share, children)
}

fn mounted_network_path_in_roots(
    roots: &[PathBuf],
    host: &str,
    share: &str,
    children: &[&str],
) -> Option<PathBuf> {
    for root in roots {
        let Ok(authorities) = fs::read_dir(root.join("smb")) else {
            continue;
        };
        for authority in authorities.flatten() {
            let authority_name = authority.file_name().to_string_lossy().into_owned();
            if super::host_from_authority(&authority_name).is_some_and(|mounted_host| {
                super::network_host_identity(&mounted_host) == super::network_host_identity(host)
            }) {
                let path = authority.path().join(share);
                if !path.try_exists().unwrap_or(false) {
                    continue;
                }
                return Some(children.iter().fold(path, |path, child| path.join(child)));
            }
        }
    }
    None
}

fn detect_tools() -> Tools {
    let fuse_mounts = kio_fuse_mount_roots();
    Tools {
        client: ["kioclient6", "kioclient", "kioclient5"]
            .into_iter()
            .find_map(super::command_path),
        fuse_mounts,
    }
}

fn smb_locations(fuse_mounts: &[PathBuf]) -> Vec<SmbLocation> {
    let mut locations = places_smb_locations();
    if !fuse_mounts.is_empty() {
        locations.extend(kio_fuse_smb_locations(fuse_mounts));
    }
    dedupe_locations(locations)
}

fn places_smb_locations() -> Vec<SmbLocation> {
    let Some(path) = places_path() else {
        return Vec::new();
    };
    fs::read_to_string(path)
        .ok()
        .map(|text| parse_places_smb_locations(&text))
        .unwrap_or_default()
}

fn places_path() -> Option<PathBuf> {
    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME").filter(|path| !path.is_empty()) {
        return Some(PathBuf::from(data_home).join("user-places.xbel"));
    }
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .map(|home| home.join(".local/share/user-places.xbel"))
}

fn parse_places_smb_locations(text: &str) -> Vec<SmbLocation> {
    let mut locations = Vec::new();
    let mut remaining = text;
    while let Some(start) = remaining.find("<bookmark") {
        remaining = &remaining[start + "<bookmark".len()..];
        let Some(end) = remaining.find('>') else {
            break;
        };
        let opening_tag = &remaining[..end];
        if let Some(href) = xml_attribute_value(opening_tag, "href")
            && let Some(location) = smb_location_from_uri(&xml_unescape(&href))
        {
            locations.push(location);
        }
        remaining = &remaining[end + 1..];
    }
    dedupe_locations(locations)
}

fn xml_attribute_value(tag: &str, attribute: &str) -> Option<String> {
    for (index, _) in tag.match_indices(attribute) {
        let before = tag[..index].chars().next_back();
        if before.is_some_and(|character| !character.is_ascii_whitespace() && character != '<') {
            continue;
        }
        let mut rest = &tag[index + attribute.len()..];
        if rest
            .chars()
            .next()
            .is_some_and(|character| !character.is_ascii_whitespace() && character != '=')
        {
            continue;
        }
        rest = rest.trim_start();
        let Some(after_equals) = rest.strip_prefix('=') else {
            continue;
        };
        rest = after_equals.trim_start();
        let Some(quote) = rest.chars().next() else {
            continue;
        };
        if !matches!(quote, '\'' | '"') {
            continue;
        }
        let value = &rest[quote.len_utf8()..];
        let Some(end) = value.find(quote) else {
            continue;
        };
        return Some(value[..end].to_string());
    }
    None
}

fn xml_unescape(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn smb_location_from_uri(uri: &str) -> Option<SmbLocation> {
    let scheme_index = uri.to_ascii_lowercase().find("smb://")?;
    let rest = &uri[scheme_index + "smb://".len()..];
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = super::host_from_authority(&rest[..authority_end])?;
    let path = rest[authority_end..]
        .split(['?', '#'])
        .next()
        .unwrap_or_default();
    let share = path
        .split('/')
        .find(|component| !component.is_empty())
        .and_then(super::percent_decode_component)
        .filter(|component| !component.is_empty());
    Some(SmbLocation { host, share })
}

fn kio_fuse_mount_roots() -> Vec<PathBuf> {
    let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
    else {
        return Vec::new();
    };
    fs::read_dir(runtime)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("kio-fuse-"))
        .map(|entry| entry.path())
        .collect()
}

fn kio_fuse_smb_locations(roots: &[PathBuf]) -> Vec<SmbLocation> {
    roots
        .iter()
        .filter_map(|root| fs::read_dir(root.join("smb")).ok())
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let authority = entry.file_name().to_string_lossy().into_owned();
            super::host_from_authority(&authority).map(|host| SmbLocation { host, share: None })
        })
        .collect()
}

fn network_computers_from_client_cached(client: &Path) -> Vec<NetworkComputerInfo> {
    let cache = NETWORK_CACHE.get_or_init(|| Mutex::new(None));
    let Ok(mut cache) = cache.lock() else {
        return network_computers_from_client(client);
    };
    if let Some((updated, computers)) = cache.as_ref()
        && updated.elapsed() < DISCOVERY_CACHE_TTL
    {
        return computers.clone();
    }
    let computers = network_computers_from_client(client);
    *cache = Some((Instant::now(), computers.clone()));
    computers
}

fn network_computers_from_client(client: &Path) -> Vec<NetworkComputerInfo> {
    let started = Instant::now();
    let Some(workgroups) = run_ls(client, "smb:/", COMMAND_TIMEOUT) else {
        return Vec::new();
    };
    let mut computers = Vec::new();
    for workgroup in parse_listing_names(&workgroups)
        .into_iter()
        .take(MAX_WORKGROUPS)
    {
        let remaining = DISCOVERY_BUDGET.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            break;
        }
        let timeout = remaining.min(COMMAND_TIMEOUT);
        let url = format!("smb://{}", percent_encode_uri_component(&workgroup));
        let Some(hosts) = run_ls(client, &url, timeout) else {
            continue;
        };
        computers.extend(parse_listing_names(&hosts).into_iter().filter_map(|name| {
            let host = super::network_host_from_uri(&name).unwrap_or(name);
            is_plausible_host(&host).then(|| NetworkComputerInfo {
                name: host,
                comment: "KIO SMB host".into(),
                kind: NetworkDeviceKind::Computer,
            })
        }));
    }
    super::dedupe_network_computers(computers)
}

fn run_ls(client: &Path, url: &str, timeout: Duration) -> Option<String> {
    let mut command = Command::new(client);
    command
        .args(["--noninteractive", "--platform", "offscreen", "ls"])
        .arg(url)
        .env("QT_QPA_PLATFORM", "offscreen")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    bounded_command_output(command, timeout)
}

fn bounded_command_output(mut command: Command, timeout: Duration) -> Option<String> {
    let mut child = command.spawn().ok()?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let output = child.wait_with_output().ok()?;
                return output
                    .status
                    .success()
                    .then(|| String::from_utf8_lossy(&output.stdout).into_owned());
            }
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(20));
            }
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

fn parse_listing_names(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .map(|line| line.trim_end_matches('/'))
        .filter(|line| !line.is_empty() && !matches!(*line, "." | ".."))
        .filter(|line| !line.chars().any(char::is_control))
        .map(str::to_string)
        .collect()
}

fn percent_encode_uri_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn is_plausible_host(host: &str) -> bool {
    !host.is_empty() && !host.chars().any(char::is_whitespace) && !host.contains(['/', '\\', '='])
}

fn is_plausible_share(share: &str) -> bool {
    !share.is_empty() && !share.ends_with('$') && !share.contains(['/', '\\', '='])
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

fn dedupe_locations(locations: Vec<SmbLocation>) -> Vec<SmbLocation> {
    let mut seen = HashSet::new();
    locations
        .into_iter()
        .filter(|location| {
            seen.insert((
                super::network_host_identity(&location.host),
                location.share.as_deref().map(str::to_ascii_lowercase),
            ))
        })
        .collect()
}

fn dedupe_shares(shares: Vec<NetworkShareInfo>) -> Vec<NetworkShareInfo> {
    let mut seen = HashSet::new();
    shares
        .into_iter()
        .filter(|share| seen.insert(share.name.to_ascii_lowercase()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_deduplicates_smb_places() {
        let places = parse_places_smb_locations(
            r#"<?xml version="1.0"?>
            <xbel>
              <bookmark href="file:///home/alice"/>
              <bookmark href="smb://alice@NAS/Public%20Files/reports"/>
              <bookmark href="smb://nas.local/Public%20Files"/>
              <bookmark href='smb://backup.local/Archive?label=A&amp;B'/>
            </xbel>"#,
        );

        assert_eq!(places.len(), 2);
        assert_eq!(places[0].host, "NAS");
        assert_eq!(places[0].share.as_deref(), Some("Public Files"));
        assert_eq!(places[1].host, "backup.local");
        assert_eq!(places[1].share.as_deref(), Some("Archive"));
    }

    #[test]
    fn parses_plain_listing_without_dot_entries() {
        assert_eq!(
            parse_listing_names(".\n..\nWORKGROUP\nOFFICE\n"),
            vec!["WORKGROUP", "OFFICE"]
        );
    }

    #[test]
    fn resolves_a_fuse_smb_mount_without_reading_share_contents() {
        let unique = format!(
            "bexplorer-kio-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        let authority = root.join("smb/alice@NAS.local");
        fs::create_dir_all(authority.join("Public")).expect("temporary KIOFuse tree");

        let resolved = mounted_network_path_in_roots(
            std::slice::from_ref(&root),
            "nas",
            "Public",
            &["folder", "file.txt"],
        );

        assert_eq!(resolved, Some(authority.join("Public/folder/file.txt")));
        fs::remove_dir_all(root).expect("remove temporary KIOFuse tree");
    }
}
