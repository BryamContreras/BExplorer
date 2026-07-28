use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowsSendToTarget {
    pub name: String,
    pub path: PathBuf,
}

pub fn send_to_targets(spanish: bool) -> Vec<WindowsSendToTarget> {
    let Some(directory) = send_to_directory() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };

    let mut targets = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !should_include_send_to_entry(&path) {
                return None;
            }
            let raw_name = path
                .file_stem()
                .filter(|stem| !stem.is_empty())
                .unwrap_or_else(|| path.file_name().expect("SendTo entry has a file name"))
                .to_string_lossy()
                .trim()
                .to_owned();
            let name = localized_send_to_name(&path, &raw_name, spanish);
            (!name.is_empty()).then_some(WindowsSendToTarget { name, path })
        })
        .collect::<Vec<_>>();
    targets.sort_by_key(|target| target.name.to_lowercase());
    targets.dedup_by(|left, right| left.name.eq_ignore_ascii_case(&right.name));
    targets
}

fn should_include_send_to_entry(path: &Path) -> bool {
    if path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("desktop.ini"))
    {
        return false;
    }

    !path.extension().is_some_and(|extension| {
        matches!(
            extension.to_string_lossy().to_ascii_lowercase().as_str(),
            "mydocs" | "mapimail" | "zfsendtotarget"
        )
    })
}

fn localized_send_to_name(path: &Path, raw_name: &str, spanish: bool) -> String {
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    if extension == "desklink" {
        if spanish {
            "Escritorio (crear acceso directo)"
        } else {
            "Desktop (create shortcut)"
        }
        .to_owned()
    } else if extension == "lnk" && raw_name.to_ascii_lowercase().contains("bluetooth") {
        if spanish {
            "Transferencia de archivos Bluetooth"
        } else {
            "Bluetooth file transfer"
        }
        .to_owned()
    } else {
        raw_name.to_owned()
    }
}

fn send_to_directory() -> Option<PathBuf> {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{FOLDERID_SendTo, KF_FLAG_DEFAULT, SHGetKnownFolderPath};

    let path =
        unsafe { SHGetKnownFolderPath(&FOLDERID_SendTo, KF_FLAG_DEFAULT, HANDLE::default()).ok()? };
    if path.is_null() {
        return None;
    }
    let text = super::util::pwstr_to_string(path);
    unsafe {
        CoTaskMemFree(Some(path.0.cast()));
    }
    (!text.is_empty()).then(|| PathBuf::from(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_windows_shell_targets_that_duplicate_bexplorer_actions() {
        assert!(!should_include_send_to_entry(Path::new(
            "Documentos.mydocs"
        )));
        assert!(!should_include_send_to_entry(Path::new(
            "Mail Recipient.MAPIMail"
        )));
        assert!(!should_include_send_to_entry(Path::new(
            "Compressed (zipped) Folder.ZFSendToTarget"
        )));
        assert!(!should_include_send_to_entry(Path::new("DESKTOP.INI")));

        assert!(should_include_send_to_entry(Path::new(
            "Desktop (create shortcut).DeskLink"
        )));
        assert!(should_include_send_to_entry(Path::new("LocalSend.lnk")));
    }

    #[test]
    fn localizes_windows_shell_destinations_without_renaming_applications() {
        let desktop = Path::new("Desktop (create shortcut).DeskLink");
        let bluetooth = Path::new("Transferencia de archivos Bluetooth.LNK");
        let local_send = Path::new("LocalSend.lnk");

        assert_eq!(
            localized_send_to_name(desktop, "Desktop (create shortcut)", true),
            "Escritorio (crear acceso directo)"
        );
        assert_eq!(
            localized_send_to_name(desktop, "Escritorio (crear acceso directo)", false),
            "Desktop (create shortcut)"
        );
        assert_eq!(
            localized_send_to_name(bluetooth, "Transferencia de archivos Bluetooth", true),
            "Transferencia de archivos Bluetooth"
        );
        assert_eq!(
            localized_send_to_name(bluetooth, "Transferencia de archivos Bluetooth", false),
            "Bluetooth file transfer"
        );
        assert_eq!(
            localized_send_to_name(local_send, "LocalSend", true),
            "LocalSend"
        );
    }
}
