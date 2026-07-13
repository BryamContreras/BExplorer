//! GNOME Wayland blur integration through Blur My Shell.
//!
//! Mutter does not expose `Shell.BlurEffect` to ordinary Wayland clients.
//! Blur My Shell runs inside GNOME Shell and can attach that effect to selected
//! application actors. BExplorer manages its own whitelist/blacklist entry
//! and keeps focused-window blur enabled so selecting the effect does not
//! produce transparency without blur while the explorer is in use.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

use crate::platform::LINUX_APPLICATION_ID;
use crate::utils::errors::{BExplorerError, Result};

const EXTENSION_UUID: &str = "blur-my-shell@aunetx";
const APPLICATION_SCHEMA: &str = "org.gnome.shell.extensions.blur-my-shell.applications";

pub(super) fn is_gnome_wayland() -> bool {
    static IS_GNOME_WAYLAND: OnceLock<bool> = OnceLock::new();
    *IS_GNOME_WAYLAND.get_or_init(|| {
        let session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
        session_type.eq_ignore_ascii_case("wayland")
            && [
                "XDG_CURRENT_DESKTOP",
                "XDG_SESSION_DESKTOP",
                "DESKTOP_SESSION",
            ]
            .into_iter()
            .filter_map(|name| std::env::var(name).ok())
            .any(|value| value.to_ascii_lowercase().contains("gnome"))
    })
}

/// Enables or disables blur for BExplorer without changing the extension's
/// policy for any other application.
pub(super) fn set_application_blur(enabled: bool, intensity: u8) -> Result<bool> {
    if !is_gnome_wayland() {
        return Ok(false);
    }

    let schema_dir = find_application_schema();
    if !enabled {
        if let Some(schema_dir) = schema_dir.as_deref() {
            update_application_lists(schema_dir, false)?;
        }
        return Ok(false);
    }

    if !extension_is_active() {
        return Err(BExplorerError::Operation(
            "GNOME window blur requires the enabled Blur My Shell extension".into(),
        ));
    }
    let schema_dir = schema_dir.ok_or_else(|| {
        BExplorerError::Operation(
            "Blur My Shell is enabled, but its application settings schema was not found".into(),
        )
    })?;
    if !can_manage_application_opacity(&schema_dir)? {
        update_application_lists(&schema_dir, false)?;
        return Err(BExplorerError::Operation(
            "Blur My Shell application blur is shared with other applications; BExplorer kept its opaque fallback to avoid changing their opacity".into(),
        ));
    }

    set_value(
        &schema_dir,
        "opacity",
        &application_opacity(intensity).to_string(),
    )?;
    keep_focused_window_blurred(&schema_dir)?;
    update_application_lists(&schema_dir, true)?;
    set_value(&schema_dir, "blur", "true")?;
    crate::utils::log::info("GNOME Blur My Shell application blur registered for BExplorer");
    Ok(true)
}

fn application_opacity(intensity: u8) -> u8 {
    // Blur My Shell applies this value to the complete client actor. Keep the
    // range deliberately subtle: zero must be fully opaque, while the highest
    // intensity still preserves enough contrast for a file manager.
    255_u8.saturating_sub((u16::from(intensity.min(100)) * 45 / 100) as u8)
}

fn can_manage_application_opacity(schema_dir: &Path) -> Result<bool> {
    if get_value(schema_dir, "enable-all")?
        .trim()
        .eq_ignore_ascii_case("true")
    {
        return Ok(false);
    }

    let whitelist = parse_string_array(&get_value(schema_dir, "whitelist")?);
    Ok(whitelist
        .iter()
        .all(|application| application.eq_ignore_ascii_case(LINUX_APPLICATION_ID)))
}

fn keep_focused_window_blurred(schema_dir: &Path) -> Result<()> {
    if get_value(schema_dir, "dynamic-opacity")?
        .trim()
        .eq_ignore_ascii_case("true")
    {
        set_value(schema_dir, "dynamic-opacity", "false")?;
        crate::utils::log::info(
            "Disabled Blur My Shell dynamic opacity so focused BExplorer windows stay blurred",
        );
    }
    Ok(())
}

fn update_application_lists(schema_dir: &Path, enabled: bool) -> Result<()> {
    let enable_all = get_value(schema_dir, "enable-all")?
        .trim()
        .eq_ignore_ascii_case("true");
    let (key, should_contain) = if enable_all {
        ("blacklist", !enabled)
    } else {
        ("whitelist", enabled)
    };
    let current = get_value(schema_dir, key)?;
    let mut applications = parse_string_array(&current);
    let contains = applications
        .iter()
        .any(|application| application.eq_ignore_ascii_case(LINUX_APPLICATION_ID));

    if should_contain && !contains {
        applications.push(LINUX_APPLICATION_ID.to_owned());
    } else if !should_contain && contains {
        applications.retain(|application| !application.eq_ignore_ascii_case(LINUX_APPLICATION_ID));
    } else {
        return Ok(());
    }

    set_value(schema_dir, key, &format_string_array(&applications))
}

fn extension_is_active() -> bool {
    let output = Command::new("gnome-extensions")
        .args(["info", EXTENSION_UUID])
        .output();
    if let Ok(output) = output
        && output.status.success()
    {
        let text = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
        if text.contains("state: active") || text.contains("enabled: yes") {
            return true;
        }
    }

    Command::new("gsettings")
        .args(["get", "org.gnome.shell", "enabled-extensions"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .is_some_and(|output| String::from_utf8_lossy(&output.stdout).contains(EXTENSION_UUID))
}

/// Returns an empty path when the schema is installed globally; otherwise the
/// path points to the extension-local compiled schema directory.
fn find_application_schema() -> Option<PathBuf> {
    let global = PathBuf::new();
    if gsettings_output(&global, &["list-keys", APPLICATION_SCHEMA]).is_ok() {
        return Some(global);
    }

    schema_directories().into_iter().find(|directory| {
        directory.join("gschemas.compiled").is_file()
            && gsettings_output(directory, &["list-keys", APPLICATION_SCHEMA]).is_ok()
    })
}

fn schema_directories() -> Vec<PathBuf> {
    let mut directories = Vec::new();
    if let Some(path) = std::env::var_os("BEXPLORER_BLUR_MY_SHELL_SCHEMA_DIR") {
        directories.push(PathBuf::from(path));
    }

    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".local/share"))
        });
    if let Some(data_home) = data_home {
        directories.push(
            data_home
                .join("gnome-shell/extensions")
                .join(EXTENSION_UUID)
                .join("schemas"),
        );
    }
    for root in ["/usr/local/share", "/usr/share"] {
        directories.push(
            Path::new(root)
                .join("gnome-shell/extensions")
                .join(EXTENSION_UUID)
                .join("schemas"),
        );
    }
    directories
}

fn get_value(schema_dir: &Path, key: &str) -> Result<String> {
    let output = gsettings_output(schema_dir, &["get", APPLICATION_SCHEMA, key])?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn set_value(schema_dir: &Path, key: &str, value: &str) -> Result<()> {
    gsettings_output(schema_dir, &["set", APPLICATION_SCHEMA, key, value]).map(|_| ())
}

fn gsettings_output(schema_dir: &Path, arguments: &[&str]) -> Result<Output> {
    let mut command = Command::new("gsettings");
    if !schema_dir.as_os_str().is_empty() {
        command.arg("--schemadir").arg(schema_dir);
    }
    let output = command
        .args(arguments)
        .output()
        .map_err(|error| BExplorerError::Operation(format!("Could not run gsettings: {error}")))?;
    if output.status.success() {
        Ok(output)
    } else {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(BExplorerError::Operation(if detail.is_empty() {
            "Could not update Blur My Shell settings".into()
        } else {
            format!("Could not update Blur My Shell settings: {detail}")
        }))
    }
}

fn parse_string_array(value: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut chars = value.chars().peekable();
    while let Some(character) = chars.next() {
        if !matches!(character, '\'' | '"') {
            continue;
        }
        let quote = character;
        let mut item = String::new();
        while let Some(character) = chars.next() {
            if character == quote {
                break;
            }
            if character == '\\' {
                if let Some(escaped) = chars.next() {
                    item.push(escaped);
                }
            } else {
                item.push(character);
            }
        }
        values.push(item);
    }
    values
}

fn format_string_array(values: &[String]) -> String {
    let values = values
        .iter()
        .map(|value| format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'")))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{values}]")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats_gsettings_application_lists() {
        let values = parse_string_array("['Firefox', 'org.example.App', 'It\\'s fine']");
        assert_eq!(values, ["Firefox", "org.example.App", "It's fine"]);
        assert_eq!(
            format_string_array(&values),
            "['Firefox', 'org.example.App', 'It\\'s fine']"
        );
    }

    #[test]
    fn application_opacity_is_opaque_at_zero_and_stays_readable_at_maximum() {
        assert_eq!(application_opacity(0), 255);
        assert_eq!(application_opacity(100), 210);
        assert!(application_opacity(15) > application_opacity(60));
    }
}
