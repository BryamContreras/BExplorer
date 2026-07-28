use std::path::PathBuf;

use crate::utils::errors::{BExplorerError, Result};

/// A destination supplied by the current desktop environment rather than by
/// BExplorer's own mounted-storage list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeSendToTarget {
    label: String,
    icon: &'static str,
    action: NativeSendToAction,
}

impl NativeSendToTarget {
    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn icon(&self) -> &'static str {
        self.icon
    }

    #[cfg(target_os = "windows")]
    pub fn icon_path(&self) -> &std::path::Path {
        let NativeSendToAction::WindowsShell { path } = &self.action;
        path
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum NativeSendToAction {
    #[cfg(target_os = "linux")]
    BlueDevil,
    #[cfg(target_os = "linux")]
    GnomeBluetooth { address: String },
    #[cfg(target_os = "linux")]
    KdeConnect { device_id: String },
    #[cfg(target_os = "linux")]
    Email,
    #[cfg(target_os = "windows")]
    WindowsShell { path: PathBuf },
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    Unsupported,
}

/// Discovers only providers that are installed and usable in the current
/// session. Mounted removable storage is intentionally handled by the UI,
/// where it can use BExplorer's persistent transfer queue.
pub fn native_send_to_targets(paths: &[PathBuf], spanish: bool) -> Vec<NativeSendToTarget> {
    if paths.is_empty() {
        return Vec::new();
    }

    #[cfg(target_os = "linux")]
    {
        linux_send_to_targets(paths, spanish)
    }

    #[cfg(target_os = "windows")]
    {
        if !paths.iter().all(|path| path.exists()) {
            return Vec::new();
        }
        crate::platform::windows::send_to_targets(spanish)
            .into_iter()
            .map(|target| NativeSendToTarget {
                icon: windows_target_icon(&target.name),
                label: target.name,
                action: NativeSendToAction::WindowsShell { path: target.path },
            })
            .collect()
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        let _ = (paths, spanish);
        Vec::new()
    }
}

pub fn invoke_native_send_to(target: &NativeSendToTarget, paths: &[PathBuf]) -> Result<()> {
    if paths.is_empty() {
        return Err(BExplorerError::Shell("There are no files to send".into()));
    }

    #[cfg(target_os = "linux")]
    {
        invoke_linux_send_to(&target.action, paths)
    }

    #[cfg(target_os = "windows")]
    {
        let NativeSendToAction::WindowsShell { path } = &target.action;
        crate::platform::windows::send_files_to_shell_target(paths.to_vec(), path)
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        let _ = (target, paths);
        Err(BExplorerError::Shell(
            "Send to is not available on this platform".into(),
        ))
    }
}

#[cfg(target_os = "linux")]
fn linux_send_to_targets(paths: &[PathBuf], spanish: bool) -> Vec<NativeSendToTarget> {
    // Bluetooth, KDE Connect, and xdg-email all transfer individual regular
    // files. Directories remain available for mounted-storage destinations,
    // which BExplorer copies recursively itself.
    if !paths.iter().all(|path| path.is_file()) {
        return Vec::new();
    }

    let mut targets = Vec::new();
    let desktop = linux_desktop_name();
    let kde_session = desktop.contains("kde") || desktop.contains("plasma");
    let gnome_session = desktop.contains("gnome");
    let has_bluedevil = executable_exists("bluedevil-sendfile");
    let has_gnome_bluetooth = executable_exists("bluetooth-sendto");

    if kde_session && has_bluedevil {
        targets.push(NativeSendToTarget {
            label: if spanish {
                "Dispositivo Bluetooth…"
            } else {
                "Bluetooth device…"
            }
            .into(),
            icon: "bluetooth",
            action: NativeSendToAction::BlueDevil,
        });
    } else if gnome_session && has_gnome_bluetooth {
        targets.extend(gnome_bluetooth_targets());
    } else if has_bluedevil {
        targets.push(NativeSendToTarget {
            label: if spanish {
                "Dispositivo Bluetooth…"
            } else {
                "Bluetooth device…"
            }
            .into(),
            icon: "bluetooth",
            action: NativeSendToAction::BlueDevil,
        });
    } else if has_gnome_bluetooth {
        targets.extend(gnome_bluetooth_targets());
    }

    if executable_exists("kdeconnect-cli") {
        targets.extend(kde_connect_targets());
    }

    if mail_composer_available() {
        targets.push(NativeSendToTarget {
            label: if spanish {
                "Destinatario de correo…"
            } else {
                "Mail recipient…"
            }
            .into(),
            icon: "mail",
            action: NativeSendToAction::Email,
        });
    }

    targets
}

#[cfg(target_os = "linux")]
fn linux_desktop_name() -> String {
    [
        "XDG_CURRENT_DESKTOP",
        "XDG_SESSION_DESKTOP",
        "DESKTOP_SESSION",
    ]
    .into_iter()
    .filter_map(|name| std::env::var(name).ok())
    .collect::<Vec<_>>()
    .join(":")
    .to_ascii_lowercase()
}

#[cfg(target_os = "linux")]
fn executable_exists(program: &str) -> bool {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .any(|directory| directory.join(program).is_file())
}

#[cfg(target_os = "linux")]
fn mail_composer_available() -> bool {
    use std::process::{Command, Stdio};

    if !executable_exists("xdg-email") || !executable_exists("xdg-mime") {
        return false;
    }
    Command::new("xdg-mime")
        .args(["query", "default", "x-scheme-handler/mailto"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .is_ok_and(|output| {
            output.status.success() && !String::from_utf8_lossy(&output.stdout).trim().is_empty()
        })
}

#[cfg(target_os = "linux")]
fn gnome_bluetooth_targets() -> Vec<NativeSendToTarget> {
    use zbus::blocking::{Connection, Proxy};
    use zbus::fdo::ManagedObjects;

    let Some(objects) = (|| {
        let connection = Connection::system().ok()?;
        let proxy = Proxy::new(
            &connection,
            "org.bluez",
            "/",
            "org.freedesktop.DBus.ObjectManager",
        )
        .ok()?;
        proxy
            .call::<_, _, ManagedObjects>("GetManagedObjects", &())
            .ok()
    })() else {
        return Vec::new();
    };

    let mut devices = objects
        .values()
        .filter_map(|interfaces| {
            let (_, properties) = interfaces
                .iter()
                .find(|(name, _)| name.as_str() == "org.bluez.Device1")?;
            let paired = properties
                .get("Paired")
                .and_then(|value| bool::try_from(value).ok())
                .unwrap_or(false);
            if !paired {
                return None;
            }
            let address = properties
                .get("Address")
                .and_then(|value| <&str>::try_from(value).ok())?
                .to_owned();
            let name = properties
                .get("Alias")
                .or_else(|| properties.get("Name"))
                .and_then(|value| <&str>::try_from(value).ok())
                .filter(|name| !name.trim().is_empty())
                .unwrap_or(&address)
                .to_owned();
            Some((name, address))
        })
        .collect::<Vec<_>>();
    devices.sort_by_key(|(name, _)| name.to_lowercase());
    let mut seen = std::collections::HashSet::new();
    devices.retain(|(_, address)| seen.insert(address.to_ascii_lowercase()));
    devices
        .into_iter()
        .map(|(name, address)| NativeSendToTarget {
            label: format!("Bluetooth — {name}"),
            icon: "bluetooth",
            action: NativeSendToAction::GnomeBluetooth { address },
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn kde_connect_targets() -> Vec<NativeSendToTarget> {
    let devices = kde_connect_output(&["--list-available", "--id-name-only"])
        .map(|output| parse_kde_connect_devices(&output))
        .or_else(|| {
            // Older KDE Connect releases did not yet have the combined
            // id-name output switch. The legacy lists retain the same order,
            // so pair them without parsing localized prose.
            let ids = kde_connect_output(&["--list-available", "--id-only"])?;
            let names = kde_connect_output(&["--list-available", "--name-only"])?;
            Some(
                ids.lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .zip(names.lines().map(str::trim).filter(|line| !line.is_empty()))
                    .filter(|(id, name)| valid_kde_connect_id(id) && !name.is_empty())
                    .map(|(id, name)| (id.to_owned(), name.to_owned()))
                    .collect(),
            )
        })
        .unwrap_or_default();

    devices
        .into_iter()
        .map(|(device_id, name)| NativeSendToTarget {
            label: format!("KDE Connect — {name}"),
            icon: "portable",
            action: NativeSendToAction::KdeConnect { device_id },
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn kde_connect_output(arguments: &[&str]) -> Option<String> {
    use std::process::{Command, Stdio};

    let output = Command::new("kdeconnect-cli")
        .args(arguments)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(target_os = "linux")]
fn parse_kde_connect_devices(output: &str) -> Vec<(String, String)> {
    let mut devices = output
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let split = line.find(char::is_whitespace)?;
            let id = line[..split].trim();
            let name = line[split..].trim();
            let valid_id = valid_kde_connect_id(id);
            (valid_id && !name.is_empty()).then(|| (id.to_owned(), name.to_owned()))
        })
        .collect::<Vec<_>>();
    devices.sort_by_key(|(_, name)| name.to_lowercase());
    let mut seen = std::collections::HashSet::new();
    devices.retain(|(id, _)| seen.insert(id.clone()));
    devices
}

#[cfg(target_os = "linux")]
fn valid_kde_connect_id(id: &str) -> bool {
    id.len() >= 8
        && id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_".contains(character))
}

#[cfg(target_os = "linux")]
fn invoke_linux_send_to(action: &NativeSendToAction, paths: &[PathBuf]) -> Result<()> {
    use std::process::{Command, Stdio};

    let launch = |command: &mut Command, provider: &str| {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|error| BExplorerError::Shell(format!("Could not start {provider}: {error}")))
    };

    match action {
        NativeSendToAction::BlueDevil => {
            let mut command = Command::new("bluedevil-sendfile");
            for path in paths {
                command.arg("--files").arg(path);
            }
            launch(&mut command, "Bluetooth")
        }
        NativeSendToAction::GnomeBluetooth { address } => {
            let mut command = Command::new("bluetooth-sendto");
            command.arg(format!("--device={address}"));
            command.args(paths);
            launch(&mut command, "Bluetooth")
        }
        NativeSendToAction::KdeConnect { device_id } => {
            for path in paths {
                let mut command = Command::new("kdeconnect-cli");
                command
                    .arg("--device")
                    .arg(device_id)
                    .arg("--share")
                    .arg(path);
                launch(&mut command, "KDE Connect")?;
            }
            Ok(())
        }
        NativeSendToAction::Email => {
            let mut command = Command::new("xdg-email");
            command.arg("--utf8");
            for path in paths {
                command.arg("--attach").arg(path);
            }
            launch(&mut command, "the mail composer")
        }
    }
}

#[cfg(target_os = "windows")]
fn windows_target_icon(name: &str) -> &'static str {
    let normalized = name.to_ascii_lowercase();
    if normalized.contains("bluetooth") {
        "bluetooth"
    } else if normalized.contains("mail") || normalized.contains("correo") {
        "mail"
    } else if normalized.contains("zip") || normalized.contains("comprim") {
        "archive"
    } else if normalized.contains("desktop") || normalized.contains("escritorio") {
        "lnk"
    } else {
        "send"
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::parse_kde_connect_devices;

    #[test]
    fn parses_kde_connect_ids_without_losing_spaces_in_names() {
        assert_eq!(
            parse_kde_connect_devices(
                "device-b My Phone\n\
                 device-a Tablet de trabajo\n"
            ),
            vec![
                ("device-b".into(), "My Phone".into()),
                ("device-a".into(), "Tablet de trabajo".into()),
            ]
        );
    }

    #[test]
    fn ignores_kde_connect_noise_and_duplicate_ids() {
        assert_eq!(
            parse_kde_connect_devices(
                "No devices found\n\
                 device-a Phone\n\
                 device-a Phone duplicate\n"
            ),
            vec![("device-a".into(), "Phone".into())]
        );
    }
}
