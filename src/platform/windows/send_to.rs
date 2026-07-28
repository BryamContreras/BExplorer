use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowsSendToTarget {
    pub name: String,
    pub path: PathBuf,
}

pub fn send_to_targets() -> Vec<WindowsSendToTarget> {
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
            let file_name = path.file_name()?.to_string_lossy();
            if file_name.eq_ignore_ascii_case("desktop.ini") {
                return None;
            }
            let name = path
                .file_stem()
                .filter(|stem| !stem.is_empty())
                .unwrap_or_else(|| path.file_name().expect("SendTo entry has a file name"))
                .to_string_lossy()
                .trim()
                .to_owned();
            (!name.is_empty()).then_some(WindowsSendToTarget { name, path })
        })
        .collect::<Vec<_>>();
    targets.sort_by_key(|target| target.name.to_lowercase());
    targets.dedup_by(|left, right| left.name.eq_ignore_ascii_case(&right.name));
    targets
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
