use super::*;

pub(super) fn compress_with_7zip(
    paths: &[PathBuf],
    destination: &Path,
    format: ArchiveFormat,
    method: ArchiveCompressionMethod,
    password: Option<&str>,
    cancel_flag: &AtomicU32,
) -> Result<()> {
    let workspace = TemporaryExtractDirectory::new_in(archive_destination_parent(destination))?;
    let staged = workspace
        .path()
        .join(format!("output.{}", format.extension()));
    copy_existing_archive(destination, &staged, cancel_flag)?;
    compress_with_7zip_in_place(
        paths,
        &staged,
        format,
        method,
        password,
        Some(destination),
        cancel_flag,
    )?;
    check_archive_cancelled(cancel_flag)?;
    fs::rename(&staged, destination)?;
    Ok(())
}

pub(super) fn compress_with_7zip_in_place(
    paths: &[PathBuf],
    destination: &Path,
    format: ArchiveFormat,
    method: ArchiveCompressionMethod,
    password: Option<&str>,
    original_destination: Option<&Path>,
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
    // A destination inside a selected source must not add its own work folder.
    if let Some(workspace_name) = destination.parent().and_then(Path::file_name) {
        args.push(format!("-xr!{}", workspace_name.to_string_lossy()));
    }
    if let Some(original) = original_destination {
        let original = fs::canonicalize(original).or_else(|_| std::path::absolute(original))?;
        for path in paths {
            let Ok(source) = fs::canonicalize(path) else {
                continue;
            };
            let Some(source_name) = path.file_name() else {
                continue;
            };
            let relative = if source.is_dir() {
                original
                    .strip_prefix(&source)
                    .ok()
                    .map(|suffix| Path::new(source_name).join(suffix))
            } else if source == original {
                Some(PathBuf::from(source_name))
            } else {
                None
            };
            if let Some(relative) = relative {
                args.push(format!("-x!{}", archive_name(&relative, false)?));
            }
        }
    }
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
    let staging = TemporaryExtractDirectory::new()?;
    let mut args = vec![
        "x".to_string(),
        "-y".to_string(),
        "-bso0".to_string(),
        "-bse0".to_string(),
        "-bsp0".to_string(),
        // The engine never writes into the user's directory. Disable symbolic
        // links and use its strict link policy inside our private staging area.
        "-snl-".to_string(),
        "-snh-".to_string(),
        "-snld0".to_string(),
        format!("-o{}", staging.path().display()),
    ];
    if let Some(arg) = password_arg(password) {
        args.push(arg);
    }
    args.push(path_arg(archive));
    run_7zip_ffi(&args, cancel_flag)?;
    check_archive_cancelled(cancel_flag)?;
    let output = output::ExtractionRoot::new(destination)?;
    for item in fs::read_dir(staging.path())? {
        let item = item?;
        check_archive_cancelled(cancel_flag)?;
        copy_archive_output_recursively(
            &item.path(),
            &output,
            &item.file_name().to_string_lossy(),
        )?;
    }
    Ok(())
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

    let temp_dir = TemporaryExtractDirectory::new()?;
    let mut args = vec![
        "x".to_string(),
        "-y".to_string(),
        "-bso0".to_string(),
        "-bse0".to_string(),
        "-bsp0".to_string(),
        "-snl-".to_string(),
        "-snh-".to_string(),
        "-snld0".to_string(),
        format!("-o{}", temp_dir.path().display()),
        path_arg(archive),
    ];
    args.extend(selection.extract_entries.iter().cloned());
    run_7zip_ffi(&args, cancel_flag)?;
    check_archive_cancelled(cancel_flag)?;
    copy_selected_outputs(temp_dir.path(), &selection.output_roots, destination)
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
    Ok(archive_selection_from_entries(&entries, selected_paths))
}

fn archive_selection_from_entries(
    entries: &[ArchiveListEntry],
    selected_paths: &[PathBuf],
) -> ArchiveSelection {
    let archive_names = entries
        .iter()
        .map(|entry| {
            let raw_name = entry.name.replace('\\', "/").trim_matches('/').to_string();
            let normalized_name = normalize_archive_item_name(&raw_name);
            (raw_name, normalized_name)
        })
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
        for (raw_name, normalized_name) in &archive_names {
            if normalized_name == &selected_name || normalized_name.starts_with(&prefix) {
                extract_entries.insert(raw_name.clone());
                matched = true;
            }
        }

        if !matched {
            extract_entries.insert(selected_name.clone());
        }
        output_roots.insert(selected_name);
    }

    ArchiveSelection {
        extract_entries: extract_entries.into_iter().collect(),
        output_roots: compact_archive_output_roots(output_roots),
    }
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
    list_7z_entries_cancellable(path, &std::sync::atomic::AtomicBool::new(false))
}

pub(super) fn list_7z_entries_cancellable(
    path: &Path,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<Vec<ArchiveListEntry>> {
    let exe = std::env::current_exe()?;
    let mut child = {
        let mut command = Command::new(exe);
        command
            .arg(ARCHIVE_LIST_HELPER_ARG)
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        command.spawn()?
    };
    let tree = contain_archive_helper(&mut child)?;
    let stdout = child.stdout.take().expect("piped archive listing stdout");
    let stderr = child.stderr.take().expect("piped archive listing stderr");
    // Concurrent drains prevent a full pipe from blocking the helper. Bounds
    // prevent corrupt metadata or diagnostics from exhausting the UI process.
    let read_failed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stdout_failed = Arc::clone(&read_failed);
    let stdout = thread::spawn(move || {
        let result = read_bounded_listing(stdout, 64 * 1024 * 1024);
        if result.is_err() {
            stdout_failed.store(true, AtomicOrdering::Relaxed);
        }
        result
    });
    let stderr_failed = Arc::clone(&read_failed);
    let stderr = thread::spawn(move || {
        let result = read_bounded_listing(stderr, 1024 * 1024);
        if result.is_err() {
            stderr_failed.store(true, AtomicOrdering::Relaxed);
        }
        result
    });
    let status = loop {
        if read_failed.load(AtomicOrdering::Relaxed) {
            tree.terminate();
            let _ = child.kill();
            let _ = child.wait();
            let stdout_result = stdout.join().ok();
            let stderr_result = stderr.join().ok();
            return Err(stdout_result
                .and_then(std::result::Result::err)
                .or_else(|| stderr_result.and_then(std::result::Result::err))
                .unwrap_or_else(|| {
                    BExplorerError::Operation("Archive listing reader failed".into())
                }));
        }
        if cancel.load(AtomicOrdering::Relaxed) {
            tree.terminate();
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout.join();
            let _ = stderr.join();
            return Err(BExplorerError::Operation(
                "Archive listing cancelled".into(),
            ));
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(Duration::from_millis(25)),
            Err(error) => {
                tree.terminate();
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout.join();
                let _ = stderr.join();
                return Err(error.into());
            }
        }
    };
    tree.terminate();
    let stdout = stdout
        .join()
        .map_err(|_| BExplorerError::Operation("Archive listing reader panicked".into()))??;
    let stderr = stderr
        .join()
        .map_err(|_| BExplorerError::Operation("Archive diagnostic reader panicked".into()))??;

    if !status.success() {
        let stderr = String::from_utf8_lossy(&stderr);
        let message = stderr.trim();
        if archive_helper_status_is_access_violation(status.code()) {
            return Err(BExplorerError::Operation(
                "7z listing crashed while reading this archive".into(),
            ));
        }
        return Err(BExplorerError::Operation(if message.is_empty() {
            format!("7z listing helper failed with status {status}")
        } else {
            message.to_string()
        }));
    }

    let stdout = String::from_utf8_lossy(&stdout);
    Ok(parse_7z_slt_entries(&stdout))
}

fn read_bounded_listing(reader: impl Read, limit: u64) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    reader.take(limit + 1).read_to_end(&mut output)?;
    if output.len() as u64 > limit {
        return Err(BExplorerError::Operation(
            "Archive listing exceeds the metadata memory limit".into(),
        ));
    }
    Ok(output)
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
    let has_entry_kind = block.contains_key("Folder") || block.contains_key("Attributes");
    let has_stream_sizes = block.contains_key("Size") && block.contains_key("Packed Size");
    if !has_entry_kind && !has_stream_sizes {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_pipe_reader_rejects_excessive_metadata() {
        assert_eq!(
            read_bounded_listing(b"metadata".as_slice(), 8).unwrap(),
            b"metadata"
        );
        assert!(read_bounded_listing(b"metadata!".as_slice(), 8).is_err());
    }

    #[test]
    fn seven_zip_update_is_transactional_and_does_not_include_its_output_or_workspace() {
        let staging = TemporaryExtractDirectory::new().unwrap();
        let source = staging.path().join("Source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("hello.txt"), b"original payload").unwrap();
        let destination = source.join("backup.7z");
        let cancel = AtomicU32::new(0);
        compress_with_7zip(
            std::slice::from_ref(&source),
            &destination,
            ArchiveFormat::SevenZip,
            ArchiveCompressionMethod::Fast,
            None,
            &cancel,
        )
        .unwrap();
        let original = fs::read(&destination).unwrap();
        fs::write(
            source.join("hello.txt"),
            b"updated payload with a different size",
        )
        .unwrap();
        assert!(
            compress_with_7zip(
                std::slice::from_ref(&source),
                &destination,
                ArchiveFormat::SevenZip,
                ArchiveCompressionMethod::Fast,
                None,
                &AtomicU32::new(1)
            )
            .is_err()
        );
        assert_eq!(fs::read(&destination).unwrap(), original);
        compress_with_7zip(
            std::slice::from_ref(&source),
            &destination,
            ArchiveFormat::SevenZip,
            ArchiveCompressionMethod::Fast,
            None,
            &cancel,
        )
        .unwrap();
        let extracted = staging.path().join("out");
        extract_with_7zip(&destination, &extracted, None, &cancel).unwrap();
        assert_eq!(
            fs::read(extracted.join("Source/hello.txt")).unwrap(),
            b"updated payload with a different size"
        );
        assert!(!extracted.join("Source/backup.7z").exists());
        assert!(!WalkDir::new(&extracted).into_iter().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("bexplorer-archive-extract-")
        }));
        assert_eq!(fs::read_dir(&source).unwrap().count(), 2);
        let invalid = staging.path().join("invalid.7z");
        fs::write(&invalid, b"original unrelated bytes").unwrap();
        assert!(
            compress_with_7zip(
                &[source],
                &invalid,
                ArchiveFormat::SevenZip,
                ArchiveCompressionMethod::Fast,
                None,
                &cancel
            )
            .is_err()
        );
        assert_eq!(fs::read(&invalid).unwrap(), b"original unrelated bytes");
    }

    fn entry(name: &str, is_dir: bool) -> ArchiveListEntry {
        ArchiveListEntry {
            name: name.to_string(),
            is_dir,
            size: (!is_dir).then_some(1),
            pack_size: None,
            modified: None,
        }
    }

    #[test]
    fn tar_selection_matches_normalized_paths_but_preserves_raw_7zip_names() {
        let entries = vec![
            entry(".", true),
            entry("./nested", true),
            entry("./nested/deep.txt", false),
            entry("./other.txt", false),
        ];
        let selected = vec![PathBuf::from("nested")];

        let selection = archive_selection_from_entries(&entries, &selected);

        assert_eq!(
            selection.extract_entries,
            vec!["./nested".to_string(), "./nested/deep.txt".to_string()]
        );
        assert_eq!(selection.output_roots, vec!["nested".to_string()]);
    }
}
