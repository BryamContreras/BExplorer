use super::*;

pub(super) fn compress_with_7zip(
    paths: &[PathBuf],
    destination: &Path,
    format: ArchiveFormat,
    method: ArchiveCompressionMethod,
    password: Option<&str>,
    cancel_flag: &AtomicU32,
) -> Result<()> {
    let mut args = vec![
        "a".to_string(),
        format!("-t{}", format.extension()),
        method.seven_zip_level().to_string(),
        "-y".to_string(),
        "-bso0".to_string(),
        "-bse0".to_string(),
        "-bsp0".to_string(),
    ];
    push_password_args(&mut args, format, password);
    args.push(path_arg(destination));
    args.extend(paths.iter().map(|path| path_arg(path)));
    run_7zip_ffi(&args, cancel_flag)
}

pub(super) fn extract_with_7zip(
    archive: &Path,
    destination: &Path,
    password: Option<&str>,
    cancel_flag: &AtomicU32,
) -> Result<()> {
    let mut args = vec![
        "x".to_string(),
        "-y".to_string(),
        "-bso0".to_string(),
        "-bse0".to_string(),
        "-bsp0".to_string(),
        format!("-o{}", destination.display()),
    ];
    if let Some(arg) = password_arg(password) {
        args.push(arg);
    }
    args.push(path_arg(archive));
    run_7zip_ffi(&args, cancel_flag)
}

pub(super) fn run_7zip_list_to_stdout(archive: &Path, cancel_flag: &AtomicU32) -> Result<()> {
    run_7zip_ffi(
        &[
            "l".to_string(),
            "-slt".to_string(),
            "-bso1".to_string(),
            "-bse2".to_string(),
            "-bsp0".to_string(),
            path_arg(archive),
        ],
        cancel_flag,
    )
}

pub(super) struct ArchiveSelection {
    pub(super) extract_entries: Vec<String>,
    pub(super) output_roots: Vec<String>,
}

pub(super) fn extract_selected_with_7zip(
    archive: &Path,
    selection: &ArchiveSelection,
    destination: &Path,
    cancel_flag: &AtomicU32,
) -> Result<()> {
    if selection.extract_entries.is_empty() {
        return Ok(());
    }

    let temp_dir = create_temp_extract_dir()?;
    let mut args = vec![
        "x".to_string(),
        "-y".to_string(),
        "-bso0".to_string(),
        "-bse0".to_string(),
        "-bsp0".to_string(),
        format!("-o{}", temp_dir.display()),
        path_arg(archive),
    ];
    args.extend(selection.extract_entries.iter().cloned());
    let result = run_7zip_ffi(&args, cancel_flag)
        .and_then(|()| copy_selected_outputs(&temp_dir, &selection.output_roots, destination));
    let cleanup = fs::remove_dir_all(&temp_dir);
    if let Err(error) = cleanup {
        crate::utils::log::error(format!(
            "Could not clean temporary archive extract folder {}: {error}",
            temp_dir.display()
        ));
    }
    result
}

pub(super) fn archive_selection_entries(
    archive: &Path,
    selected_paths: &[PathBuf],
) -> Result<ArchiveSelection> {
    let entries = if is_zip(archive) {
        list_zip_entries(archive)?
    } else {
        list_7z_entries(archive)?
    };

    let archive_names = entries
        .iter()
        .map(|entry| normalize_archive_item_name(&entry.name))
        .collect::<Vec<_>>();
    let mut extract_entries = BTreeSet::new();
    let mut output_roots = BTreeSet::new();

    for selected_path in selected_paths {
        let selected_name = normalize_archive_path(selected_path);
        if selected_name.is_empty() {
            continue;
        }

        let mut matched = false;
        let prefix = format!("{selected_name}/");
        for entry_name in &archive_names {
            if entry_name == &selected_name || entry_name.starts_with(&prefix) {
                extract_entries.insert(entry_name.clone());
                matched = true;
            }
        }

        if !matched {
            extract_entries.insert(selected_name.clone());
        }
        output_roots.insert(selected_name);
    }

    Ok(ArchiveSelection {
        extract_entries: extract_entries.into_iter().collect(),
        output_roots: compact_archive_output_roots(output_roots),
    })
}

fn normalize_archive_path(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy().replace('\\', "/")),
            Component::CurDir => None,
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => None,
        })
        .collect::<Vec<_>>()
        .join("/")
        .trim_matches('/')
        .to_string()
}

fn normalize_archive_item_name(name: &str) -> String {
    name.replace('\\', "/").trim_matches('/').to_string()
}

fn compact_archive_output_roots(roots: BTreeSet<String>) -> Vec<String> {
    let mut compact = Vec::new();
    for root in roots {
        let covered_by_parent = compact.iter().any(|parent: &String| {
            root.len() > parent.len()
                && root.as_bytes().get(parent.len()) == Some(&b'/')
                && root.starts_with(parent)
        });
        if !covered_by_parent {
            compact.push(root);
        }
    }
    compact
}

fn run_7zip_ffi(args: &[String], cancel_flag: &AtomicU32) -> Result<()> {
    // Tell C++ where to find the cancel flag for this FFI call.
    unsafe {
        let ptr = cancel_flag as *const AtomicU32 as *const std::ffi::c_void;
        bfp_7zr_set_cancel_flag(ptr);
    }

    let exit_code = cfg_run_7zip(args);

    // Clear the cancel flag pointer
    unsafe {
        bfp_7zr_set_cancel_flag(std::ptr::null());
    }

    let safe_args = sanitize_7zip_args(args);
    let description = describe_7zip_command(&safe_args);

    match exit_code {
        0 => Ok(()),
        1 => Err(BExplorerError::Operation(format!(
            "Embedded 7-Zip completed with warnings: {description}"
        ))),
        7 => Err(BExplorerError::Operation(format!(
            "Embedded 7-Zip command line error: {description}"
        ))),
        8 => Err(BExplorerError::Operation(format!(
            "Embedded 7-Zip out of memory: {description}"
        ))),
        255 => Err(BExplorerError::Operation(format!(
            "Embedded 7-Zip operation cancelled: {description}"
        ))),
        code => Err(BExplorerError::Operation(format!(
            "Embedded 7-Zip operation failed with exit code {code}: {description}"
        ))),
    }
}

fn sanitize_7zip_args(args: &[String]) -> Vec<String> {
    args.iter()
        .map(|arg| {
            if arg.starts_with("-p") && arg.len() > 2 {
                "-p********".to_string()
            } else {
                arg.clone()
            }
        })
        .collect()
}

#[cfg(windows)]
fn cfg_run_7zip(args: &[String]) -> i32 {
    let command_line = build_windows_command_line(args);
    let mut wide: Vec<u16> = command_line.encode_utf16().collect();
    wide.push(0);
    unsafe { bfp_7zr_run_w(wide.as_ptr()) }
}

#[cfg(not(windows))]
fn cfg_run_7zip(args: &[String]) -> i32 {
    let mut cstrs = Vec::with_capacity(args.len() + 1);
    cstrs.push(CString::new("bexplorer-7zr").expect("CString"));
    for arg in args {
        cstrs.push(CString::new(arg.as_str()).expect("CString"));
    }
    let ptrs: Vec<*const c_char> = cstrs.iter().map(|s| s.as_ptr()).collect();
    unsafe { bfp_7zr_run_argv(ptrs.len() as i32, ptrs.as_ptr()) }
}

#[cfg(windows)]
fn describe_7zip_command(args: &[String]) -> String {
    build_windows_command_line(args)
}

#[cfg(not(windows))]
fn describe_7zip_command(args: &[String]) -> String {
    let mut command = String::from("bexplorer-7zr");
    for arg in args {
        command.push(' ');
        command.push_str(arg);
    }
    command
}

#[cfg(windows)]
fn build_windows_command_line(args: &[String]) -> String {
    let mut parts = Vec::with_capacity(args.len() + 1);
    parts.push(quote_windows_arg("bexplorer-7zr"));
    parts.extend(args.iter().map(|arg| quote_windows_arg(arg)));
    parts.join(" ")
}

#[cfg(windows)]
fn quote_windows_arg(arg: &str) -> String {
    if arg.is_empty() || arg.chars().any(|c| c == ' ' || c == '\t' || c == '"') {
        let escaped = arg.replace('"', "\\\"");
        format!("\"{escaped}\"")
    } else {
        arg.to_string()
    }
}

pub fn list_7z_entries(path: &Path) -> Result<Vec<ArchiveListEntry>> {
    list_7z_entries_via_helper(path)
}

fn list_7z_entries_via_helper(path: &Path) -> Result<Vec<ArchiveListEntry>> {
    let exe = std::env::current_exe()?;
    let output = {
        let mut command = Command::new(exe);
        command
            .arg(ARCHIVE_LIST_HELPER_ARG)
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);
        command.output()?
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = stderr.trim();
        if archive_helper_status_is_access_violation(output.status.code()) {
            return Err(BExplorerError::Operation(
                "7z listing crashed while reading this archive".into(),
            ));
        }
        return Err(BExplorerError::Operation(if message.is_empty() {
            format!("7z listing helper failed with status {}", output.status)
        } else {
            message.to_string()
        }));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_7z_slt_entries(&stdout))
}

fn archive_helper_status_is_access_violation(code: Option<i32>) -> bool {
    code.is_some_and(|code| code == -1073741819 || code as u32 == 0xC000_0005)
}

pub(super) fn parse_7z_slt_entries(output: &str) -> Vec<ArchiveListEntry> {
    let mut entries = Vec::new();
    let mut block = BTreeMap::<String, String>::new();

    for line in output.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with("----------") {
            push_7z_slt_entry(&block, &mut entries);
            block.clear();
            continue;
        }

        if let Some((key, value)) = line.split_once(" = ") {
            block.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    push_7z_slt_entry(&block, &mut entries);

    entries
}

fn push_7z_slt_entry(block: &BTreeMap<String, String>, entries: &mut Vec<ArchiveListEntry>) {
    if !block.contains_key("Folder") && !block.contains_key("Attributes") {
        return;
    }

    let Some(name) = block
        .get("Path")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    else {
        return;
    };

    let is_dir = block.get("Folder").is_some_and(|value| value.trim() == "+")
        || block
            .get("Attributes")
            .is_some_and(|value| value.contains('D'));

    entries.push(ArchiveListEntry {
        name: name.replace('\\', "/"),
        is_dir,
        size: if is_dir {
            None
        } else {
            parse_7z_u64(block.get("Size"))
        },
        pack_size: parse_7z_u64(block.get("Packed Size")),
        modified: block
            .get("Modified")
            .and_then(|value| parse_7z_modified_time(value)),
    });
}

fn parse_7z_u64(value: Option<&String>) -> Option<u64> {
    value
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<u64>().ok())
}

fn parse_7z_modified_time(value: &str) -> Option<SystemTime> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    let without_fraction = value.split('.').next().unwrap_or(value);
    let parsed =
        chrono::NaiveDateTime::parse_from_str(without_fraction, "%Y-%m-%d %H:%M:%S").ok()?;
    Some(parsed.and_utc().into())
}
