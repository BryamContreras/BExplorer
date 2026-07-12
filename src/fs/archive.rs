use std::collections::{BTreeMap, BTreeSet};
#[cfg(not(windows))]
use std::ffi::CString;
use std::ffi::OsStr;
use std::ffi::{CStr, c_void};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::os::raw::c_char;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crc32fast::Hasher;
use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::utils::errors::{BExplorerError, Result};

mod seven_zip;
pub mod types;
mod zip;

pub use seven_zip::list_7z_entries;
#[cfg(test)]
use seven_zip::parse_7z_slt_entries;
use seven_zip::{
    archive_selection_entries, compress_with_7zip, extract_selected_with_7zip, extract_with_7zip,
    run_7zip_list_to_stdout,
};
pub use zip::list_zip_entries;
use zip::{
    create_zip_archive, create_zip_archive_with_progress, extract_zip_archive,
    extract_zip_archive_with_progress,
};

pub use types::{
    ArchiveCompressionMethod, ArchiveFormat, ArchiveJob, ArchiveJobKind, ArchiveListEntry,
    ArchiveProgress, ArchiveProgressMsg, ArchiveState, ExtractMode,
};

const ZIP_LOCAL_FILE_HEADER: u32 = 0x0403_4b50;
const ZIP_CENTRAL_DIRECTORY_HEADER: u32 = 0x0201_4b50;
const ZIP_END_OF_CENTRAL_DIRECTORY: u32 = 0x0605_4b50;
const ZIP_VERSION: u16 = 20;
const ZIP_UTF8_FLAG: u16 = 1 << 11;
const ZIP_ENCRYPTED_FLAG: u16 = 1;
const ZIP_METHOD_STORE: u16 = 0;
const ZIP_METHOD_DEFLATE: u16 = 8;
const ARCHIVE_PASSWORD_REQUIRED: &str = "Archive password required";
const DOS_TIME_MIDNIGHT: u16 = 0;
const DOS_DATE_1980_01_01: u16 = 33;
const ARCHIVE_PROGRESS_INTERVAL: Duration = Duration::from_millis(80);
const ARCHIVE_HELPER_ARG: &str = "--bexplorer-archive-helper";
const ARCHIVE_LIST_HELPER_ARG: &str = "--bexplorer-archive-list-helper";
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", content = "payload")]
enum ArchiveHelperMessage {
    Progress(ArchiveProgress),
}

#[cfg(windows)]
unsafe extern "C" {
    fn bfp_7zr_run_w(command_line: *const u16) -> i32;
}

#[cfg(not(windows))]
unsafe extern "C" {
    fn bfp_7zr_run_argv(argc: i32, argv: *const *const c_char) -> i32;
}

// ---- 7-Zip progress callback & cancel FFI ----

type ProgressCb = unsafe extern "C" fn(u64, u64, u64, *const c_char, *const c_char, *mut c_void);

unsafe extern "C" {
    fn bfp_7zr_set_progress_callback(cb: Option<ProgressCb>);
    fn bfp_7zr_set_progress_user_data(user_data: *mut c_void);
    /// Set the cancel flag pointer for the current 7-Zip invocation. Pass a pointer to a
    /// non-zero unsigned to signal cancellation; NULL to clear.
    fn bfp_7zr_set_cancel_flag(flag: *const std::ffi::c_void);
}

struct ArchiveProgressEmitter {
    tx: Sender<ArchiveProgressMsg>,
    total: u64,
    completed: u64,
    files: u64,
    command: &'static str,
    last_emit: Instant,
}

impl ArchiveProgressEmitter {
    fn new(tx: Sender<ArchiveProgressMsg>, total: u64, command: &'static str) -> Self {
        let mut emitter = Self {
            tx,
            total,
            completed: 0,
            files: 0,
            command,
            last_emit: Instant::now() - ARCHIVE_PROGRESS_INTERVAL,
        };
        emitter.emit("", true);
        emitter
    }

    fn add_bytes(&mut self, bytes: u64, file_name: &str) {
        self.completed = self.completed.saturating_add(bytes).min(self.total);
        self.emit(file_name, false);
    }

    fn finish_file(&mut self, file_name: &str) {
        self.files = self.files.saturating_add(1);
        self.emit(file_name, true);
    }

    fn finish(&mut self, file_name: &str) {
        if self.total > 0 {
            self.completed = self.total;
        }
        self.emit(file_name, true);
    }

    fn emit(&mut self, file_name: &str, force: bool) {
        if !force && self.last_emit.elapsed() < ARCHIVE_PROGRESS_INTERVAL {
            return;
        }
        self.last_emit = Instant::now();
        let _ = self.tx.send(ArchiveProgressMsg::Progress(ArchiveProgress {
            completed: self.completed,
            total: self.total,
            files: self.files,
            command: self.command.to_string(),
            file_name: file_name.to_string(),
        }));
    }
}

unsafe extern "C" fn bfp_progress_rust_cb(
    completed: u64,
    total: u64,
    files: u64,
    command: *const c_char,
    file_name: *const c_char,
    user_data: *mut c_void,
) {
    if user_data.is_null() {
        return;
    }

    let cmd = if command.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(command).to_string_lossy().into_owned() }
    };
    let fname = if file_name.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(file_name).to_string_lossy().into_owned() }
    };

    let tx = unsafe { &*(user_data as *const Sender<ArchiveProgressMsg>) };
    let _ = tx.send(ArchiveProgressMsg::Progress(ArchiveProgress {
        completed,
        total,
        files,
        command: cmd,
        file_name: fname,
    }));
}

unsafe extern "C" fn bfp_progress_stdout_cb(
    completed: u64,
    total: u64,
    files: u64,
    command: *const c_char,
    file_name: *const c_char,
    _user_data: *mut c_void,
) {
    let cmd = if command.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(command).to_string_lossy().into_owned() }
    };
    let fname = if file_name.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(file_name).to_string_lossy().into_owned() }
    };

    let message = ArchiveHelperMessage::Progress(ArchiveProgress {
        completed,
        total,
        files,
        command: cmd,
        file_name: fname,
    });
    if let Ok(line) = serde_json::to_string(&message) {
        let mut stdout = std::io::stdout().lock();
        let _ = writeln!(stdout, "{line}");
        let _ = stdout.flush();
    }
}

struct ArchiveProgressRegistration {
    _tx: Box<Sender<ArchiveProgressMsg>>,
}

impl Drop for ArchiveProgressRegistration {
    fn drop(&mut self) {
        unsafe {
            bfp_7zr_set_progress_callback(None);
            bfp_7zr_set_progress_user_data(std::ptr::null_mut());
        }
    }
}

/// Register the progress callback for this archive operation.
fn register_progress_callback(tx: Sender<ArchiveProgressMsg>) -> ArchiveProgressRegistration {
    let tx = Box::new(tx);
    let user_data = (&*tx as *const Sender<ArchiveProgressMsg>) as *mut c_void;
    unsafe {
        bfp_7zr_set_progress_user_data(user_data);
        bfp_7zr_set_progress_callback(Some(bfp_progress_rust_cb));
    }
    ArchiveProgressRegistration { _tx: tx }
}

struct ArchiveStdoutProgressRegistration;

impl Drop for ArchiveStdoutProgressRegistration {
    fn drop(&mut self) {
        unsafe {
            bfp_7zr_set_progress_callback(None);
            bfp_7zr_set_progress_user_data(std::ptr::null_mut());
        }
    }
}

fn register_stdout_progress_callback() -> ArchiveStdoutProgressRegistration {
    unsafe {
        bfp_7zr_set_progress_user_data(std::ptr::null_mut());
        bfp_7zr_set_progress_callback(Some(bfp_progress_stdout_cb));
    }
    ArchiveStdoutProgressRegistration
}

impl ArchiveCompressionMethod {
    fn zip_compression(self) -> Compression {
        match self {
            Self::Store => Compression::none(),
            Self::Fast => Compression::fast(),
            Self::Normal => Compression::default(),
            Self::Maximum => Compression::best(),
        }
    }

    fn seven_zip_level(self) -> &'static str {
        match self {
            Self::Store => "-mx=0",
            Self::Fast => "-mx=1",
            Self::Normal => "-mx=5",
            Self::Maximum => "-mx=9",
        }
    }
}

#[allow(dead_code)]
pub fn compress(paths: &[PathBuf], destination: &Path, format: ArchiveFormat) -> Result<PathBuf> {
    compress_with_method(
        paths,
        destination,
        format,
        ArchiveCompressionMethod::default(),
    )
}

pub fn compress_with_method(
    paths: &[PathBuf],
    destination: &Path,
    format: ArchiveFormat,
    method: ArchiveCompressionMethod,
) -> Result<PathBuf> {
    compress_with_method_and_password(paths, destination, format, method, None)
}

pub fn compress_with_method_and_password(
    paths: &[PathBuf],
    destination: &Path,
    format: ArchiveFormat,
    method: ArchiveCompressionMethod,
    password: Option<&str>,
) -> Result<PathBuf> {
    if paths.is_empty() {
        return Err(BExplorerError::Operation(
            "No items selected to compress".into(),
        ));
    }

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }

    let dummy_cancel = AtomicU32::new(0);
    if use_7zip_for_compression(format, password) {
        compress_with_7zip(paths, destination, format, method, password, &dummy_cancel)?;
    } else {
        match format {
            ArchiveFormat::Zip => create_zip_archive(paths, destination, method, &dummy_cancel)?,
            ArchiveFormat::SevenZip => {
                compress_with_7zip(paths, destination, format, method, password, &dummy_cancel)?
            }
        }
    }

    Ok(destination.to_path_buf())
}

/// Like [`compress`] but sends progress updates to `tx` during the
/// 7-Zip part. The caller must poll the receiver and must send a
/// Finished message — this function only returns the result.
pub fn compress_with_progress(
    paths: &[PathBuf],
    destination: &Path,
    format: ArchiveFormat,
    method: ArchiveCompressionMethod,
    password: Option<&str>,
    tx: Sender<ArchiveProgressMsg>,
    cancel_flag: &AtomicU32,
) -> Result<PathBuf> {
    if paths.is_empty() {
        return Err(BExplorerError::Operation(
            "No items selected to compress".into(),
        ));
    }

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }

    if use_7zip_for_compression(format, password) {
        {
            let total = archive_sources_total_bytes(paths);
            let display_name = paths
                .first()
                .map(|path| current_name(path).to_string())
                .unwrap_or_default();
            let mut fallback = ArchiveProgressEmitter::new(tx.clone(), total, "Compress");
            fallback.emit(&display_name, true);
            let _progress_registration = register_progress_callback(tx);
            let result =
                compress_with_7zip(paths, destination, format, method, password, cancel_flag);
            if result.is_ok() {
                fallback.finish(&display_name);
            }
            result.map(|_| destination.to_path_buf())
        }
    } else {
        match format {
            ArchiveFormat::Zip => {
                create_zip_archive_with_progress(paths, destination, method, tx, cancel_flag)?;
                Ok(destination.to_path_buf())
            }
            ArchiveFormat::SevenZip => unreachable!("7z compression should use the 7z path"),
        }
    }
}

fn use_7zip_for_compression(format: ArchiveFormat, password: Option<&str>) -> bool {
    format == ArchiveFormat::SevenZip || password_has_value(password)
}

fn password_has_value(password: Option<&str>) -> bool {
    password.is_some_and(|value| !value.is_empty())
}

fn archive_password_required_error() -> BExplorerError {
    BExplorerError::Operation(ARCHIVE_PASSWORD_REQUIRED.into())
}

fn password_arg(password: Option<&str>) -> Option<String> {
    password
        .filter(|value| !value.is_empty())
        .map(|value| format!("-p{value}"))
}

fn push_password_args(args: &mut Vec<String>, format: ArchiveFormat, password: Option<&str>) {
    if let Some(arg) = password_arg(password) {
        args.push(arg);
        if format == ArchiveFormat::SevenZip {
            args.push("-mhe=on".to_string());
        }
    }
}

fn should_run_archive_job_in_helper(job: &ArchiveJob) -> bool {
    match job.kind {
        ArchiveJobKind::Compress => use_7zip_for_compression(job.format, job.password.as_deref()),
        ArchiveJobKind::Extract => !is_zip(&job.archive_path) || job.has_password(),
    }
}

fn archive_helper_request_path(job_id: u64) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!(
        "bexplorer-archive-job-{}-{job_id}-{stamp}.json",
        std::process::id()
    ))
}

fn read_archive_helper_stdout<R: Read + Send + 'static>(reader: R, tx: Sender<ArchiveProgressMsg>) {
    let reader = BufReader::new(reader);
    for line in reader.lines().map_while(std::result::Result::ok) {
        let Ok(message) = serde_json::from_str::<ArchiveHelperMessage>(&line) else {
            continue;
        };
        match message {
            ArchiveHelperMessage::Progress(progress) => {
                let _ = tx.send(ArchiveProgressMsg::Progress(progress));
            }
        }
    }
}

fn run_archive_job_in_helper(
    job: &ArchiveJob,
    tx: Sender<ArchiveProgressMsg>,
    cancel_flag: &AtomicU32,
) -> Result<PathBuf> {
    let request_path = archive_helper_request_path(job.id);
    let request_json = serde_json::to_vec(job).map_err(|error| {
        BExplorerError::Operation(format!("Archive helper request failed: {error}"))
    })?;
    fs::write(&request_path, request_json)?;

    let exe = std::env::current_exe()?;
    let mut command = Command::new(exe);
    command
        .arg(ARCHIVE_HELPER_ARG)
        .arg(&request_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    let mut child = command.spawn()?;

    let stdout_reader = child.stdout.take().map(|stdout| {
        let tx = tx.clone();
        thread::spawn(move || read_archive_helper_stdout(stdout, tx))
    });
    let stderr_reader = child.stderr.take().map(|stderr| {
        thread::spawn(move || {
            let mut text = String::new();
            let _ = BufReader::new(stderr).read_to_string(&mut text);
            text
        })
    });

    let status = loop {
        if cancel_flag.load(AtomicOrdering::Relaxed) != 0 {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(&request_path);
            cleanup_partial_archive_destination(job);
            if let Some(handle) = stdout_reader {
                let _ = handle.join();
            }
            if let Some(handle) = stderr_reader {
                let _ = handle.join();
            }
            return Err(BExplorerError::Operation(
                "Archive operation cancelled".into(),
            ));
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        thread::sleep(Duration::from_millis(40));
    };

    let _ = fs::remove_file(&request_path);
    if let Some(handle) = stdout_reader {
        let _ = handle.join();
    }
    let stderr = stderr_reader
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();

    if !status.success() {
        cleanup_partial_archive_destination(job);
        let error = stderr.trim();
        let message = if error.is_empty() {
            format!("Archive helper failed with status {status}")
        } else {
            error.to_string()
        };
        return Err(BExplorerError::Operation(message));
    }

    match job.kind {
        ArchiveJobKind::Compress => Ok(job.destination.clone()),
        ArchiveJobKind::Extract => {
            if job.destination.as_os_str().is_empty() {
                extract_destination(&job.archive_path, job.extract_mode)
            } else {
                Ok(job.destination.clone())
            }
        }
    }
}

pub fn try_run_archive_helper_from_args() -> Option<i32> {
    let mut args = std::env::args_os();
    let _exe = args.next();
    let marker = args.next()?;
    if marker == OsStr::new(ARCHIVE_HELPER_ARG) {
        let request_path = PathBuf::from(args.next()?);
        return Some(match run_archive_helper(&request_path) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("{error}");
                1
            }
        });
    }

    if marker == OsStr::new(ARCHIVE_LIST_HELPER_ARG) {
        let archive_path = PathBuf::from(args.next()?);
        return Some(match run_archive_list_helper(&archive_path) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("{error}");
                1
            }
        });
    }

    None
}

fn run_archive_helper(request_path: &Path) -> Result<()> {
    let request_json = fs::read_to_string(request_path)?;
    let job: ArchiveJob = serde_json::from_str(request_json.trim_start_matches('\u{feff}'))
        .map_err(|error| {
            BExplorerError::Operation(format!("Archive helper request decode failed: {error}"))
        })?;
    let cancel_flag = AtomicU32::new(0);
    let _progress_registration = register_stdout_progress_callback();

    match job.kind {
        ArchiveJobKind::Compress => {
            if use_7zip_for_compression(job.format, job.password.as_deref()) {
                let total = archive_sources_total_bytes(&job.sources);
                let display_name = job
                    .sources
                    .first()
                    .map(|path| current_name(path).to_string())
                    .unwrap_or_default();
                emit_archive_helper_progress(ArchiveProgress {
                    completed: 0,
                    total,
                    files: 0,
                    command: "Compress".to_string(),
                    file_name: display_name,
                });
                compress_with_7zip(
                    &job.sources,
                    &job.destination,
                    job.format,
                    job.method,
                    job.password.as_deref(),
                    &cancel_flag,
                )
            } else {
                create_zip_archive(&job.sources, &job.destination, job.method, &cancel_flag)
            }
        }
        ArchiveJobKind::Extract => {
            let destination = if job.destination.as_os_str().is_empty() {
                extract_destination(&job.archive_path, job.extract_mode)?
            } else {
                job.destination.clone()
            };
            fs::create_dir_all(&destination)?;
            if is_zip(&job.archive_path) && !job.has_password() {
                extract_zip_archive(&job.archive_path, &destination, &cancel_flag)
            } else {
                extract_with_7zip(
                    &job.archive_path,
                    &destination,
                    job.password.as_deref(),
                    &cancel_flag,
                )
            }
        }
    }
}

fn run_archive_list_helper(archive_path: &Path) -> Result<()> {
    let cancel_flag = AtomicU32::new(0);
    run_7zip_list_to_stdout(archive_path, &cancel_flag)
}

fn cleanup_partial_archive_destination(job: &ArchiveJob) {
    if job.kind != ArchiveJobKind::Compress || job.destination.as_os_str().is_empty() {
        return;
    }
    if let Err(error) = fs::remove_file(&job.destination)
        && error.kind() != io::ErrorKind::NotFound
    {
        crate::utils::log::error(format!(
            "Could not remove partial archive {}: {error}",
            job.destination.display()
        ));
    }
}

fn emit_archive_helper_progress(progress: ArchiveProgress) {
    let message = ArchiveHelperMessage::Progress(progress);
    if let Ok(line) = serde_json::to_string(&message) {
        let mut stdout = std::io::stdout().lock();
        let _ = writeln!(stdout, "{line}");
        let _ = stdout.flush();
    }
}

/// Run an archive job in a background thread, sending progress and
/// result messages through `tx`.
pub fn run_archive_job(
    job: ArchiveJob,
    tx: Sender<ArchiveProgressMsg>,
    cancel_flag: Arc<AtomicU32>,
) {
    let result = if should_run_archive_job_in_helper(&job) {
        run_archive_job_in_helper(&job, tx.clone(), &cancel_flag)
    } else {
        match job.kind {
            ArchiveJobKind::Compress => compress_with_progress(
                &job.sources,
                &job.destination,
                job.format,
                job.method,
                job.password.as_deref(),
                tx.clone(),
                &cancel_flag,
            ),
            ArchiveJobKind::Extract => extract_with_progress(
                &job.archive_path,
                job.extract_mode,
                job.password.as_deref(),
                tx.clone(),
                &cancel_flag,
            ),
        }
    };

    let msg = match result {
        Ok(dest) => ArchiveProgressMsg::Finished(Ok(dest)),
        Err(e) => {
            if cancel_flag.load(AtomicOrdering::Relaxed) != 0 {
                ArchiveProgressMsg::Cancelled
            } else {
                ArchiveProgressMsg::Finished(Err(e.to_string()))
            }
        }
    };
    let _ = tx.send(msg);
}

#[allow(dead_code)]
pub fn extract(archive: &Path, mode: ExtractMode) -> Result<PathBuf> {
    if !archive.is_file() {
        return Err(BExplorerError::InvalidPath(archive.to_path_buf()));
    }

    let destination = extract_destination(archive, mode)?;
    fs::create_dir_all(&destination)?;

    let dummy_cancel = AtomicU32::new(0);
    if is_zip(archive) {
        extract_zip_archive(archive, &destination, &dummy_cancel)?;
    } else {
        extract_with_7zip(archive, &destination, None, &dummy_cancel)?;
    }

    Ok(destination)
}

pub fn extract_virtual_paths_to_destination(
    paths: &[PathBuf],
    destination: &Path,
) -> Result<usize> {
    if paths.is_empty() {
        return Err(BExplorerError::Operation(
            "No archive items selected to extract".into(),
        ));
    }
    if !destination.is_dir() {
        return Err(BExplorerError::InvalidPath(destination.to_path_buf()));
    }

    let mut grouped: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
    for path in paths {
        let Some((archive_path, internal_path)) = crate::fs::archive_listing::resolve_archive(path)
        else {
            return Err(BExplorerError::InvalidPath(path.clone()));
        };
        if internal_path.as_os_str().is_empty() {
            return Err(BExplorerError::Operation(format!(
                "Select files or folders inside the archive: {}",
                archive_path.display()
            )));
        }
        grouped.entry(archive_path).or_default().push(internal_path);
    }

    let cancel_flag = AtomicU32::new(0);
    let mut extracted_items = 0;
    for (archive_path, internal_paths) in grouped {
        let selection = archive_selection_entries(&archive_path, &internal_paths)?;
        if selection.extract_entries.is_empty() || selection.output_roots.is_empty() {
            continue;
        }
        extracted_items += selection.output_roots.len();
        extract_selected_with_7zip(&archive_path, &selection, destination, &cancel_flag)?;
    }

    Ok(extracted_items)
}

/// Like [`extract`] but sends progress updates to `tx` during the
/// 7-Zip part.
pub fn extract_with_progress(
    archive: &Path,
    mode: ExtractMode,
    password: Option<&str>,
    tx: Sender<ArchiveProgressMsg>,
    cancel_flag: &AtomicU32,
) -> Result<PathBuf> {
    if !archive.is_file() {
        return Err(BExplorerError::InvalidPath(archive.to_path_buf()));
    }
    let destination = extract_destination(archive, mode)?;
    fs::create_dir_all(&destination)?;
    if is_zip(archive) && !password_has_value(password) {
        extract_zip_archive_with_progress(archive, &destination, tx, cancel_flag)?;
    } else {
        let total = archive.metadata().map(|meta| meta.len()).unwrap_or(0);
        let display_name = current_name(archive).to_string();
        let mut fallback = ArchiveProgressEmitter::new(tx.clone(), total, "Extract");
        fallback.emit(&display_name, true);
        let _progress_registration = register_progress_callback(tx);
        let result = extract_with_7zip(archive, &destination, password, cancel_flag);
        if result.is_ok() {
            fallback.finish(&display_name);
        }
        result?;
    }
    Ok(destination)
}

#[allow(dead_code)]
fn copy_selected_outputs(temp_dir: &Path, roots: &[String], destination: &Path) -> Result<()> {
    for root in roots {
        let source = archive_temp_path(temp_dir, root)?;
        if !source.exists() {
            return Err(BExplorerError::InvalidPath(source));
        }
        let Some(name) = root.rsplit('/').find(|part| !part.is_empty()) else {
            continue;
        };
        let target = unique_extract_destination(&destination.join(name), source.is_dir());
        copy_archive_output_recursively(&source, &target)?;
    }
    Ok(())
}

fn archive_temp_path(temp_dir: &Path, internal_path: &str) -> Result<PathBuf> {
    let mut path = temp_dir.to_path_buf();
    for part in internal_path.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(BExplorerError::Operation(format!(
                "Unsafe archive path: {internal_path}"
            )));
        }
        path.push(part);
    }
    Ok(path)
}

fn copy_archive_output_recursively(source: &Path, target: &Path) -> Result<()> {
    if source.is_dir() {
        fs::create_dir_all(target)?;
        for item in fs::read_dir(source)? {
            let item = item?;
            copy_archive_output_recursively(&item.path(), &target.join(item.file_name()))?;
        }
    } else {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, target)?;
    }
    Ok(())
}

fn unique_extract_destination(base: &Path, is_dir: bool) -> PathBuf {
    if !base.exists() {
        return base.to_path_buf();
    }

    let parent = base.parent().unwrap_or_else(|| Path::new(""));
    let stem = base
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("Copy");
    let extension = base.extension().and_then(|value| value.to_str());

    for index in 2..10_000 {
        let candidate_name = if is_dir {
            format!("{stem} ({index})")
        } else if let Some(extension) = extension {
            format!("{stem} ({index}).{extension}")
        } else {
            format!("{stem} ({index})")
        };
        let candidate = parent.join(candidate_name);
        if !candidate.exists() {
            return candidate;
        }
    }

    base.to_path_buf()
}

fn create_temp_extract_dir() -> Result<PathBuf> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "bexplorer-archive-extract-{}-{stamp}",
        std::process::id()
    ));
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn extract_destination(archive: &Path, mode: ExtractMode) -> Result<PathBuf> {
    let parent = archive
        .parent()
        .ok_or_else(|| BExplorerError::InvalidPath(archive.to_path_buf()))?;

    match mode {
        ExtractMode::Here => Ok(parent.to_path_buf()),
        ExtractMode::ToNamedFolder => {
            let stem = archive
                .file_stem()
                .and_then(|name| name.to_str())
                .filter(|name| !name.trim().is_empty())
                .unwrap_or("Extracted");
            Ok(unique_path(&parent.join(stem), true))
        }
    }
}

pub fn planned_extract_destination(archive: &Path, mode: ExtractMode) -> Result<PathBuf> {
    extract_destination(archive, mode)
}

fn archive_name(path: &Path, is_dir: bool) -> Result<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => parts.push(value.to_string_lossy().to_string()),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(BExplorerError::Operation(format!(
                    "Invalid archive path: {}",
                    path.display()
                )));
            }
        }
    }

    let mut name = parts.join("/");
    if is_dir && !name.ends_with('/') {
        name.push('/');
    }
    Ok(name)
}

fn safe_output_path(destination: &Path, name: &str) -> Result<PathBuf> {
    let mut output = destination.to_path_buf();
    for component in Path::new(name).components() {
        match component {
            Component::Normal(value) => output.push(value),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(BExplorerError::Operation(format!(
                    "Unsafe archive path blocked: {name}"
                )));
            }
        }
    }
    Ok(output)
}

fn unique_path(base: &Path, is_dir: bool) -> PathBuf {
    if !base.exists() {
        return base.to_path_buf();
    }

    let parent = base.parent().unwrap_or_else(|| Path::new(""));
    let stem = base
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("Archive");
    let extension = base.extension().and_then(|value| value.to_str());

    for index in 1..10_000 {
        let name = if is_dir {
            format!("{stem} ({index})")
        } else if let Some(extension) = extension {
            format!("{stem} ({index}).{extension}")
        } else {
            format!("{stem} ({index})")
        };
        let candidate = parent.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }

    base.to_path_buf()
}

fn temp_path_for(path: &Path, extension: &str) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("archive");
    parent.join(format!(
        ".bexplorer-{}-{stamp}-{name}.{extension}",
        std::process::id()
    ))
}

fn path_arg(path: &Path) -> String {
    path.display().to_string()
}

fn current_name(path: &Path) -> &str {
    path.file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("")
}

fn archive_sources_total_bytes(paths: &[PathBuf]) -> u64 {
    paths.iter().map(|path| path_total_bytes(path)).sum()
}

fn path_total_bytes(path: &Path) -> u64 {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return 0;
    };
    if metadata.is_file() {
        return metadata.len();
    }
    if !metadata.is_dir() {
        return 0;
    }

    fs::read_dir(path)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|item| item.ok())
        .map(|item| path_total_bytes(&item.path()))
        .sum()
}

fn check_archive_cancelled(cancel_flag: &AtomicU32) -> Result<()> {
    if cancel_flag.load(AtomicOrdering::Relaxed) != 0 {
        Err(BExplorerError::Operation(
            "Archive operation cancelled".into(),
        ))
    } else {
        Ok(())
    }
}

fn is_zip(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufWriter;
    use std::sync::mpsc;
    use std::thread;

    fn temp_test_dir(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "bexplorer-archive-{name}-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("create temp test dir");
        dir
    }

    fn write_pattern_file(path: &Path, size: usize) {
        let mut writer = BufWriter::new(File::create(path).expect("create pattern file"));
        let mut state = 0x1234_5678_9abc_def0_u64;
        let mut remaining = size;
        let mut buffer = [0_u8; 64 * 1024];

        while remaining > 0 {
            for byte in &mut buffer {
                state ^= state << 7;
                state ^= state >> 9;
                state ^= state << 8;
                *byte = state as u8;
            }
            let len = remaining.min(buffer.len());
            writer
                .write_all(&buffer[..len])
                .expect("write pattern bytes");
            remaining -= len;
        }
        writer.flush().expect("flush pattern file");
    }

    #[test]
    fn creates_and_extracts_zip_archive() {
        let root = temp_test_dir("zip");
        let source = root.join("Source");
        let nested = source.join("Nested");
        fs::create_dir_all(&nested).expect("create nested dir");
        fs::write(source.join("hello.txt"), b"hello archive").expect("write file");
        fs::write(nested.join("deep.txt"), b"deep file").expect("write nested file");

        let archive = root.join("Source.zip");
        compress(std::slice::from_ref(&source), &archive, ArchiveFormat::Zip)
            .expect("create zip archive");

        let extracted = extract(&archive, ExtractMode::ToNamedFolder).expect("extract zip");
        assert_eq!(
            fs::read(extracted.join("Source").join("hello.txt")).expect("read extracted"),
            b"hello archive"
        );
        assert_eq!(
            fs::read(extracted.join("Source").join("Nested").join("deep.txt"))
                .expect("read nested extracted"),
            b"deep file"
        );

        fs::remove_dir_all(root).expect("cleanup temp test dir");
    }

    #[test]
    fn extracts_selected_virtual_zip_item_to_destination() {
        let root = temp_test_dir("zip-selection");
        let source = root.join("Source");
        fs::create_dir_all(&source).expect("create source dir");
        fs::write(source.join("keep.txt"), b"copy me").expect("write selected file");
        fs::write(source.join("skip.txt"), b"leave me").expect("write unselected file");

        let archive = root.join("Selection.zip");
        compress(std::slice::from_ref(&source), &archive, ArchiveFormat::Zip)
            .expect("create zip archive");

        let destination = root.join("Out");
        fs::create_dir_all(&destination).expect("create destination");
        let selected = archive.join("Source").join("keep.txt");
        let count = extract_virtual_paths_to_destination(&[selected], &destination)
            .expect("extract selected item");

        assert_eq!(count, 1);
        assert_eq!(
            fs::read(destination.join("keep.txt")).expect("read selected file"),
            b"copy me"
        );
        assert!(
            !destination.join("skip.txt").exists(),
            "unselected archive entries should not be extracted"
        );

        fs::remove_dir_all(root).expect("cleanup temp test dir");
    }

    #[test]
    fn blocks_zip_slip_paths() {
        let root = temp_test_dir("zipslip");
        let unsafe_path = safe_output_path(&root, "../bad.txt");
        assert!(unsafe_path.is_err());
        fs::remove_dir_all(root).expect("cleanup temp test dir");
    }

    #[test]
    fn parses_7z_technical_listing() {
        let output = r#"
Path = payload.7z
Type = 7z
Physical Size = 1024

----------
Path = folder
Size = 0
Packed Size = 0
Modified = 2026-06-25 12:34:56
Attributes = D
Folder = +

Path = folder/file.txt
Size = 42
Packed Size = 21
Modified = 2026-06-25 12:35:00
Attributes = A
Folder = -
"#;

        let entries = parse_7z_slt_entries(output);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "folder");
        assert!(entries[0].is_dir);
        assert_eq!(entries[0].size, None);
        assert_eq!(entries[1].name, "folder/file.txt");
        assert!(!entries[1].is_dir);
        assert_eq!(entries[1].size, Some(42));
        assert_eq!(entries[1].pack_size, Some(21));
    }

    #[test]
    fn creates_and_extracts_7z_archive_through_ffi() {
        let root = temp_test_dir("7z");
        let source = root.join("Source7z");
        fs::create_dir_all(&source).expect("create source dir");
        fs::write(source.join("hello.txt"), b"hello from ffi 7z").expect("write source file");

        let archive = root.join("Source7z.7z");
        compress(
            std::slice::from_ref(&source),
            &archive,
            ArchiveFormat::SevenZip,
        )
        .expect("create 7z archive through ffi");

        let extracted = extract(&archive, ExtractMode::ToNamedFolder).expect("extract 7z");
        assert_eq!(
            fs::read(extracted.join("Source7z").join("hello.txt")).expect("read extracted"),
            b"hello from ffi 7z"
        );

        fs::remove_dir_all(root).expect("cleanup temp test dir");
    }

    #[test]
    fn password_protected_7z_extracts_with_password() {
        let root = temp_test_dir("7z-password");
        let source = root.join("Protected7z");
        fs::create_dir_all(&source).expect("create source dir");
        fs::write(source.join("secret.txt"), b"protected 7z").expect("write source file");

        let archive = root.join("Protected7z.7z");
        compress_with_method_and_password(
            std::slice::from_ref(&source),
            &archive,
            ArchiveFormat::SevenZip,
            ArchiveCompressionMethod::Normal,
            Some("secret"),
        )
        .expect("create password-protected 7z archive");

        let no_password = extract(&archive, ExtractMode::ToNamedFolder);
        assert!(
            no_password.is_err(),
            "protected 7z extraction should require password"
        );

        let (tx, _rx) = mpsc::channel();
        let cancel = AtomicU32::new(0);
        let extracted = extract_with_progress(
            &archive,
            ExtractMode::ToNamedFolder,
            Some("secret"),
            tx,
            &cancel,
        )
        .expect("extract password-protected 7z archive");
        assert_eq!(
            fs::read(extracted.join("Protected7z").join("secret.txt")).expect("read extracted"),
            b"protected 7z"
        );

        fs::remove_dir_all(root).expect("cleanup temp test dir");
    }

    #[test]
    fn password_protected_zip_extracts_with_password() {
        let root = temp_test_dir("zip-password");
        let source = root.join("ProtectedZip");
        fs::create_dir_all(&source).expect("create source dir");
        fs::write(source.join("secret.txt"), b"protected zip").expect("write source file");

        let archive = root.join("ProtectedZip.zip");
        compress_with_method_and_password(
            std::slice::from_ref(&source),
            &archive,
            ArchiveFormat::Zip,
            ArchiveCompressionMethod::Normal,
            Some("secret"),
        )
        .expect("create password-protected zip archive");

        let no_password = extract(&archive, ExtractMode::ToNamedFolder);
        assert!(
            no_password.is_err(),
            "protected ZIP extraction should require password"
        );

        let (tx, _rx) = mpsc::channel();
        let cancel = AtomicU32::new(0);
        let extracted = extract_with_progress(
            &archive,
            ExtractMode::ToNamedFolder,
            Some("secret"),
            tx,
            &cancel,
        )
        .expect("extract password-protected zip archive");
        assert_eq!(
            fs::read(extracted.join("ProtectedZip").join("secret.txt")).expect("read extracted"),
            b"protected zip"
        );

        fs::remove_dir_all(root).expect("cleanup temp test dir");
    }

    #[test]
    fn zip_compression_reports_progress() {
        let root = temp_test_dir("zip-progress");
        let source = root.join("payload.bin");
        write_pattern_file(&source, 2 * 1024 * 1024);

        let archive = root.join("payload.zip");
        let (tx, rx) = mpsc::channel();
        let cancel = AtomicU32::new(0);
        compress_with_progress(
            std::slice::from_ref(&source),
            &archive,
            ArchiveFormat::Zip,
            ArchiveCompressionMethod::Normal,
            None,
            tx,
            &cancel,
        )
        .expect("create zip archive with progress");

        let progress: Vec<_> = rx.try_iter().collect();
        assert!(
            progress.iter().any(|message| matches!(
                message,
                ArchiveProgressMsg::Progress(progress)
                    if progress.total > 0 && progress.completed > 0
            )),
            "expected at least one non-zero ZIP progress update"
        );

        fs::remove_dir_all(root).expect("cleanup temp test dir");
    }

    #[test]
    fn seven_zip_compression_reports_intermediate_progress() {
        let root = temp_test_dir("7z-progress");
        let source = root.join("payload.bin");
        write_pattern_file(&source, 32 * 1024 * 1024);

        let archive = root.join("payload.7z");
        let (tx, rx) = mpsc::channel();
        let cancel = AtomicU32::new(0);
        compress_with_progress(
            std::slice::from_ref(&source),
            &archive,
            ArchiveFormat::SevenZip,
            ArchiveCompressionMethod::Normal,
            None,
            tx,
            &cancel,
        )
        .expect("create 7z archive with progress");

        let progress: Vec<_> = rx.try_iter().collect();
        assert!(
            progress.iter().any(|message| matches!(
                message,
                ArchiveProgressMsg::Progress(progress)
                    if progress.total > 0
                        && progress.completed > 0
                        && progress.completed < progress.total
            )),
            "expected at least one intermediate 7z progress update"
        );

        fs::remove_dir_all(root).expect("cleanup temp test dir");
    }

    #[test]
    fn two_7z_compressions_can_run_concurrently() {
        let root = temp_test_dir("7z-parallel");
        let source_a = root.join("a.bin");
        let source_b = root.join("b.bin");
        write_pattern_file(&source_a, 32 * 1024 * 1024);
        write_pattern_file(&source_b, 32 * 1024 * 1024);

        let archive_a = root.join("a.7z");
        let archive_b = root.join("b.7z");
        let (tx_a, rx_a) = mpsc::channel();
        let (tx_b, rx_b) = mpsc::channel();

        let handle_a = thread::spawn({
            let source = source_a.clone();
            let archive = archive_a.clone();
            move || {
                let cancel = AtomicU32::new(0);
                compress_with_progress(
                    std::slice::from_ref(&source),
                    &archive,
                    ArchiveFormat::SevenZip,
                    ArchiveCompressionMethod::Normal,
                    None,
                    tx_a,
                    &cancel,
                )
            }
        });
        let handle_b = thread::spawn({
            let source = source_b.clone();
            let archive = archive_b.clone();
            move || {
                let cancel = AtomicU32::new(0);
                compress_with_progress(
                    std::slice::from_ref(&source),
                    &archive,
                    ArchiveFormat::SevenZip,
                    ArchiveCompressionMethod::Normal,
                    None,
                    tx_b,
                    &cancel,
                )
            }
        });

        handle_a
            .join()
            .expect("join 7z worker a")
            .expect("compress a");
        handle_b
            .join()
            .expect("join 7z worker b")
            .expect("compress b");

        assert!(archive_a.is_file());
        assert!(archive_b.is_file());
        let progress_a: Vec<_> = rx_a.try_iter().collect();
        let progress_b: Vec<_> = rx_b.try_iter().collect();
        assert!(
            progress_a.iter().any(|message| matches!(
                message,
                ArchiveProgressMsg::Progress(progress)
                    if progress.total > 0
                        && progress.completed > 0
                        && progress.completed < progress.total
            )),
            "expected worker a to report intermediate 7z progress"
        );
        assert!(
            progress_b.iter().any(|message| matches!(
                message,
                ArchiveProgressMsg::Progress(progress)
                    if progress.total > 0
                        && progress.completed > 0
                        && progress.completed < progress.total
            )),
            "expected worker b to report intermediate 7z progress"
        );

        fs::remove_dir_all(root).expect("cleanup temp test dir");
    }
}
