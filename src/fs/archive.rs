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

pub mod types;

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

#[derive(Clone)]
struct ZipSource {
    path: PathBuf,
    name: String,
    is_dir: bool,
}

#[derive(Clone)]
struct ZipCentralEntry {
    name: Vec<u8>,
    method: u16,
    crc32: u32,
    compressed_size: u64,
    uncompressed_size: u64,
    local_header_offset: u64,
    external_attributes: u32,
}

struct ZipReadEntry {
    name: String,
    method: u16,
    encrypted: bool,
    crc32: u32,
    compressed_size: u64,
    uncompressed_size: u64,
    local_header_offset: u64,
    is_dir: bool,
    modified: Option<SystemTime>,
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
fn create_zip_archive(
    paths: &[PathBuf],
    destination: &Path,
    method: ArchiveCompressionMethod,
    cancel_flag: &AtomicU32,
) -> Result<()> {
    create_zip_archive_inner(paths, destination, method, None, cancel_flag)
}

fn create_zip_archive_with_progress(
    paths: &[PathBuf],
    destination: &Path,
    method: ArchiveCompressionMethod,
    tx: Sender<ArchiveProgressMsg>,
    cancel_flag: &AtomicU32,
) -> Result<()> {
    let total = archive_sources_total_bytes(paths);
    let mut progress = ArchiveProgressEmitter::new(tx, total, "Compress");
    create_zip_archive_inner(paths, destination, method, Some(&mut progress), cancel_flag)?;
    let file_name = current_name(destination).to_string();
    progress.finish(&file_name);
    Ok(())
}

fn create_zip_archive_inner(
    paths: &[PathBuf],
    destination: &Path,
    method: ArchiveCompressionMethod,
    mut progress: Option<&mut ArchiveProgressEmitter>,
    cancel_flag: &AtomicU32,
) -> Result<()> {
    let sources = collect_zip_sources(paths)?;
    if sources.is_empty() {
        return Err(BExplorerError::Operation(
            "No readable items were found to compress".into(),
        ));
    }

    let temp_archive = temp_path_for(destination, "tmpzip");
    let mut writer = BufWriter::new(File::create(&temp_archive)?);
    let mut central_entries = Vec::with_capacity(sources.len());

    let result = (|| {
        for source in sources {
            check_archive_cancelled(cancel_flag)?;
            let local_header_offset = writer.stream_position()?;
            let name = source.name.as_bytes().to_vec();

            if source.is_dir {
                write_zip_local_header(
                    &mut writer,
                    &name,
                    ZIP_METHOD_STORE,
                    0,
                    0,
                    0,
                    local_header_offset,
                    &mut central_entries,
                    true,
                )?;
                continue;
            }

            if method == ArchiveCompressionMethod::Store {
                let stored =
                    stored_source_file(&source.path, progress.as_deref_mut(), cancel_flag)?;
                write_zip_local_header(
                    &mut writer,
                    &name,
                    ZIP_METHOD_STORE,
                    stored.crc32,
                    stored.size,
                    stored.size,
                    local_header_offset,
                    &mut central_entries,
                    false,
                )?;
                copy_source_file_to_writer(&source.path, &mut writer, cancel_flag)?;
            } else {
                let compressed = deflate_source_file(
                    &source.path,
                    method.zip_compression(),
                    progress.as_deref_mut(),
                    cancel_flag,
                )?;
                write_zip_local_header(
                    &mut writer,
                    &name,
                    ZIP_METHOD_DEFLATE,
                    compressed.crc32,
                    compressed.compressed_size,
                    compressed.uncompressed_size,
                    local_header_offset,
                    &mut central_entries,
                    false,
                )?;

                let mut temp_file = BufReader::new(File::open(&compressed.path)?);
                io::copy(&mut temp_file, &mut writer)?;
                let _ = fs::remove_file(&compressed.path);
            }
            if let Some(progress) = progress.as_deref_mut() {
                progress.finish_file(&source.name);
            }
        }

        write_zip_central_directory(&mut writer, &central_entries)?;
        writer.flush()?;
        Ok::<(), BExplorerError>(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp_archive);
        return result;
    }

    if destination.exists() {
        fs::remove_file(destination)?;
    }
    fs::rename(&temp_archive, destination).or_else(|_| {
        fs::copy(&temp_archive, destination)?;
        fs::remove_file(&temp_archive)?;
        Ok::<(), std::io::Error>(())
    })?;

    Ok(())
}

struct DeflatedFile {
    path: PathBuf,
    crc32: u32,
    compressed_size: u64,
    uncompressed_size: u64,
}

struct StoredFile {
    crc32: u32,
    size: u64,
}

fn stored_source_file(
    path: &Path,
    mut progress: Option<&mut ArchiveProgressEmitter>,
    cancel_flag: &AtomicU32,
) -> Result<StoredFile> {
    let mut input = BufReader::new(File::open(path)?);
    let mut hasher = Hasher::new();
    let mut size = 0_u64;
    let mut buffer = [0_u8; 1024 * 128];
    let file_name = current_name(path).to_string();

    loop {
        check_archive_cancelled(cancel_flag)?;
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size = size.saturating_add(read as u64);
        if let Some(progress) = progress.as_deref_mut() {
            progress.add_bytes(read as u64, &file_name);
        }
    }

    Ok(StoredFile {
        crc32: hasher.finalize(),
        size,
    })
}

fn copy_source_file_to_writer<W: Write>(
    path: &Path,
    writer: &mut W,
    cancel_flag: &AtomicU32,
) -> Result<()> {
    let mut input = BufReader::new(File::open(path)?);
    let mut buffer = [0_u8; 1024 * 128];
    loop {
        check_archive_cancelled(cancel_flag)?;
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        writer.write_all(&buffer[..read])?;
    }
    Ok(())
}

fn deflate_source_file(
    path: &Path,
    compression: Compression,
    mut progress: Option<&mut ArchiveProgressEmitter>,
    cancel_flag: &AtomicU32,
) -> Result<DeflatedFile> {
    let temp = temp_path_for(path, "deflate");
    let mut input = BufReader::new(File::open(path)?);
    let output = File::create(&temp)?;
    let mut encoder = DeflateEncoder::new(output, compression);
    let mut hasher = Hasher::new();
    let mut uncompressed_size = 0_u64;
    let mut buffer = [0_u8; 1024 * 128];
    let file_name = current_name(path).to_string();

    loop {
        check_archive_cancelled(cancel_flag)?;
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        encoder.write_all(&buffer[..read])?;
        uncompressed_size = uncompressed_size.saturating_add(read as u64);
        if let Some(progress) = progress.as_deref_mut() {
            progress.add_bytes(read as u64, &file_name);
        }
    }

    let output = encoder.finish()?;
    let compressed_size = output.metadata()?.len();

    Ok(DeflatedFile {
        path: temp,
        crc32: hasher.finalize(),
        compressed_size,
        uncompressed_size,
    })
}

#[allow(clippy::too_many_arguments)]
fn write_zip_local_header<W: Write>(
    writer: &mut W,
    name: &[u8],
    method: u16,
    crc32: u32,
    compressed_size: u64,
    uncompressed_size: u64,
    local_header_offset: u64,
    central_entries: &mut Vec<ZipCentralEntry>,
    is_dir: bool,
) -> Result<()> {
    let compressed_size_32 = to_zip_u32(compressed_size, "compressed size")?;
    let uncompressed_size_32 = to_zip_u32(uncompressed_size, "uncompressed size")?;
    let name_len = to_zip_u16(name.len(), "file name length")?;

    write_u32(writer, ZIP_LOCAL_FILE_HEADER)?;
    write_u16(writer, ZIP_VERSION)?;
    write_u16(writer, ZIP_UTF8_FLAG)?;
    write_u16(writer, method)?;
    write_u16(writer, DOS_TIME_MIDNIGHT)?;
    write_u16(writer, DOS_DATE_1980_01_01)?;
    write_u32(writer, crc32)?;
    write_u32(writer, compressed_size_32)?;
    write_u32(writer, uncompressed_size_32)?;
    write_u16(writer, name_len)?;
    write_u16(writer, 0)?;
    writer.write_all(name)?;

    central_entries.push(ZipCentralEntry {
        name: name.to_vec(),
        method,
        crc32,
        compressed_size,
        uncompressed_size,
        local_header_offset,
        external_attributes: if is_dir { 0x10 } else { 0 },
    });

    Ok(())
}

fn write_zip_central_directory<W: Write + Seek>(
    writer: &mut W,
    entries: &[ZipCentralEntry],
) -> Result<()> {
    let central_offset = writer.stream_position()?;

    for entry in entries {
        write_u32(writer, ZIP_CENTRAL_DIRECTORY_HEADER)?;
        write_u16(writer, ZIP_VERSION)?;
        write_u16(writer, ZIP_VERSION)?;
        write_u16(writer, ZIP_UTF8_FLAG)?;
        write_u16(writer, entry.method)?;
        write_u16(writer, DOS_TIME_MIDNIGHT)?;
        write_u16(writer, DOS_DATE_1980_01_01)?;
        write_u32(writer, entry.crc32)?;
        write_u32(
            writer,
            to_zip_u32(entry.compressed_size, "compressed size")?,
        )?;
        write_u32(
            writer,
            to_zip_u32(entry.uncompressed_size, "uncompressed size")?,
        )?;
        write_u16(writer, to_zip_u16(entry.name.len(), "file name length")?)?;
        write_u16(writer, 0)?;
        write_u16(writer, 0)?;
        write_u16(writer, 0)?;
        write_u16(writer, 0)?;
        write_u32(writer, entry.external_attributes)?;
        write_u32(
            writer,
            to_zip_u32(entry.local_header_offset, "local header offset")?,
        )?;
        writer.write_all(&entry.name)?;
    }

    let central_size = writer.stream_position()?.saturating_sub(central_offset);
    write_u32(writer, ZIP_END_OF_CENTRAL_DIRECTORY)?;
    write_u16(writer, 0)?;
    write_u16(writer, 0)?;
    write_u16(writer, to_zip_u16(entries.len(), "entry count")?)?;
    write_u16(writer, to_zip_u16(entries.len(), "entry count")?)?;
    write_u32(writer, to_zip_u32(central_size, "central directory size")?)?;
    write_u32(
        writer,
        to_zip_u32(central_offset, "central directory offset")?,
    )?;
    write_u16(writer, 0)?;

    Ok(())
}

fn collect_zip_sources(paths: &[PathBuf]) -> Result<Vec<ZipSource>> {
    let mut sources = Vec::new();

    for path in paths {
        if !path.exists() {
            continue;
        }

        let base = path.parent().unwrap_or_else(|| Path::new(""));
        if path.is_dir() {
            for entry in WalkDir::new(path).follow_links(false) {
                let entry = entry.map_err(|error| BExplorerError::Operation(error.to_string()))?;
                let entry_path = entry.path();
                let relative = entry_path
                    .strip_prefix(base)
                    .map_err(|error| BExplorerError::Operation(error.to_string()))?;
                let is_dir = entry.file_type().is_dir();
                sources.push(ZipSource {
                    path: entry_path.to_path_buf(),
                    name: archive_name(relative, is_dir)?,
                    is_dir,
                });
            }
        } else {
            let Some(name) = path.file_name() else {
                continue;
            };
            sources.push(ZipSource {
                path: path.to_path_buf(),
                name: archive_name(Path::new(name), false)?,
                is_dir: false,
            });
        }
    }

    Ok(sources)
}

#[allow(dead_code)]
fn extract_zip_archive(archive: &Path, destination: &Path, cancel_flag: &AtomicU32) -> Result<()> {
    extract_zip_archive_inner(archive, destination, None, cancel_flag)
}

fn extract_zip_archive_with_progress(
    archive: &Path,
    destination: &Path,
    tx: Sender<ArchiveProgressMsg>,
    cancel_flag: &AtomicU32,
) -> Result<()> {
    let mut file = File::open(archive)?;
    let entries = read_zip_central_directory(&mut file)?;
    let total = entries.iter().map(|entry| entry.uncompressed_size).sum();
    let mut progress = ArchiveProgressEmitter::new(tx, total, "Extract");
    extract_zip_entries(file, entries, destination, Some(&mut progress), cancel_flag)?;
    let file_name = current_name(archive).to_string();
    progress.finish(&file_name);
    Ok(())
}

#[allow(dead_code)]
fn extract_zip_archive_inner(
    archive: &Path,
    destination: &Path,
    progress: Option<&mut ArchiveProgressEmitter>,
    cancel_flag: &AtomicU32,
) -> Result<()> {
    let mut file = File::open(archive)?;
    let entries = read_zip_central_directory(&mut file)?;
    extract_zip_entries(file, entries, destination, progress, cancel_flag)
}

fn extract_zip_entries(
    mut file: File,
    entries: Vec<ZipReadEntry>,
    destination: &Path,
    mut progress: Option<&mut ArchiveProgressEmitter>,
    cancel_flag: &AtomicU32,
) -> Result<()> {
    for entry in entries {
        check_archive_cancelled(cancel_flag)?;
        if entry.encrypted {
            return Err(archive_password_required_error());
        }
        let output_path = safe_output_path(destination, &entry.name)?;
        if entry.name.ends_with('/') {
            fs::create_dir_all(&output_path)?;
            continue;
        }

        let output_path = unique_path(&output_path, false);
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }

        file.seek(SeekFrom::Start(entry.local_header_offset))?;
        let mut local_header = [0_u8; 30];
        file.read_exact(&mut local_header)?;
        if read_u32(&local_header, 0)? != ZIP_LOCAL_FILE_HEADER {
            return Err(BExplorerError::Operation(format!(
                "Invalid ZIP local header for {}",
                entry.name
            )));
        }

        let name_len = read_u16(&local_header, 26)? as u64;
        let extra_len = read_u16(&local_header, 28)? as u64;
        file.seek(SeekFrom::Current((name_len + extra_len) as i64))?;

        let mut output = BufWriter::new(File::create(&output_path)?);
        let bytes_and_crc = match entry.method {
            ZIP_METHOD_STORE => {
                let mut source = (&mut file).take(entry.compressed_size);
                copy_with_crc(
                    &mut source,
                    &mut output,
                    progress.as_deref_mut(),
                    cancel_flag,
                    &entry.name,
                )?
            }
            ZIP_METHOD_DEFLATE => {
                let source = (&mut file).take(entry.compressed_size);
                let mut decoder = DeflateDecoder::new(source);
                copy_with_crc(
                    &mut decoder,
                    &mut output,
                    progress.as_deref_mut(),
                    cancel_flag,
                    &entry.name,
                )?
            }
            method => {
                return Err(BExplorerError::Operation(format!(
                    "Unsupported ZIP method {method} in {}",
                    entry.name
                )));
            }
        };
        output.flush()?;

        if bytes_and_crc.0 != entry.uncompressed_size || bytes_and_crc.1 != entry.crc32 {
            return Err(BExplorerError::Operation(format!(
                "CRC or size mismatch while extracting {}",
                entry.name
            )));
        }
        if let Some(progress) = progress.as_deref_mut() {
            progress.finish_file(&entry.name);
        }
    }

    Ok(())
}

fn read_zip_central_directory(file: &mut File) -> Result<Vec<ZipReadEntry>> {
    let file_len = file.metadata()?.len();
    let search_len = file_len.min(66_000) as usize;
    file.seek(SeekFrom::End(-(search_len as i64)))?;
    let mut buffer = vec![0_u8; search_len];
    file.read_exact(&mut buffer)?;

    let mut eocd_at = None;
    for index in (0..search_len.saturating_sub(3)).rev() {
        if read_u32(&buffer, index)? == ZIP_END_OF_CENTRAL_DIRECTORY {
            eocd_at = Some(index);
            break;
        }
    }

    let Some(eocd_at) = eocd_at else {
        return Err(BExplorerError::Operation(
            "Could not find ZIP central directory".into(),
        ));
    };

    let entries = read_u16(&buffer, eocd_at + 10)? as usize;
    let central_size = read_u32(&buffer, eocd_at + 12)? as u64;
    let central_offset = read_u32(&buffer, eocd_at + 16)? as u64;

    if central_offset.saturating_add(central_size) > file_len {
        return Err(BExplorerError::Operation(
            "ZIP central directory points outside the archive".into(),
        ));
    }

    file.seek(SeekFrom::Start(central_offset))?;
    let mut output = Vec::with_capacity(entries);

    for _ in 0..entries {
        let mut header = [0_u8; 46];
        file.read_exact(&mut header)?;
        if read_u32(&header, 0)? != ZIP_CENTRAL_DIRECTORY_HEADER {
            return Err(BExplorerError::Operation(
                "Invalid ZIP central directory entry".into(),
            ));
        }

        let flags = read_u16(&header, 8)?;
        let method = read_u16(&header, 10)?;
        let dos_time = read_u16(&header, 12)?;
        let dos_date = read_u16(&header, 14)?;
        let crc32 = read_u32(&header, 16)?;
        let compressed_size = read_u32(&header, 20)? as u64;
        let uncompressed_size = read_u32(&header, 24)? as u64;
        let name_len = read_u16(&header, 28)? as usize;
        let extra_len = read_u16(&header, 30)? as usize;
        let comment_len = read_u16(&header, 32)? as usize;
        let local_header_offset = read_u32(&header, 42)? as u64;

        let mut name = vec![0_u8; name_len];
        file.read_exact(&mut name)?;
        file.seek(SeekFrom::Current((extra_len + comment_len) as i64))?;

        let name_str = String::from_utf8_lossy(&name).into_owned();
        let is_dir = name_str.ends_with('/');
        let modified = dos_time_date_to_system_time(dos_time, dos_date);

        output.push(ZipReadEntry {
            name: name_str,
            method,
            encrypted: flags & ZIP_ENCRYPTED_FLAG != 0,
            crc32,
            compressed_size,
            uncompressed_size,
            local_header_offset,
            is_dir,
            modified,
        });
    }

    Ok(output)
}

fn compress_with_7zip(
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

fn extract_with_7zip(
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

fn run_7zip_list_to_stdout(archive: &Path, cancel_flag: &AtomicU32) -> Result<()> {
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

struct ArchiveSelection {
    extract_entries: Vec<String>,
    output_roots: Vec<String>,
}

fn extract_selected_with_7zip(
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

fn archive_selection_entries(
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

fn copy_with_crc<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    mut progress: Option<&mut ArchiveProgressEmitter>,
    cancel_flag: &AtomicU32,
    file_name: &str,
) -> Result<(u64, u32)> {
    let mut hasher = Hasher::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 1024 * 128];

    loop {
        check_archive_cancelled(cancel_flag)?;
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        writer.write_all(&buffer[..read])?;
        total = total.saturating_add(read as u64);
        if let Some(progress) = progress.as_deref_mut() {
            progress.add_bytes(read as u64, file_name);
        }
    }

    Ok((total, hasher.finalize()))
}

fn write_u16<W: Write>(writer: &mut W, value: u16) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn write_u32<W: Write>(writer: &mut W, value: u32) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn read_u16(buffer: &[u8], offset: usize) -> Result<u16> {
    let bytes = buffer
        .get(offset..offset + 2)
        .ok_or_else(|| BExplorerError::Operation("Unexpected end of ZIP data".into()))?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32(buffer: &[u8], offset: usize) -> Result<u32> {
    let bytes = buffer
        .get(offset..offset + 4)
        .ok_or_else(|| BExplorerError::Operation("Unexpected end of ZIP data".into()))?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn to_zip_u16(value: usize, label: &str) -> Result<u16> {
    u16::try_from(value).map_err(|_| BExplorerError::Operation(format!("{label} exceeds ZIP32")))
}

fn to_zip_u32(value: u64, label: &str) -> Result<u32> {
    u32::try_from(value).map_err(|_| BExplorerError::Operation(format!("{label} exceeds ZIP32")))
}

fn dos_time_date_to_system_time(dos_time: u16, dos_date: u16) -> Option<SystemTime> {
    let hour = ((dos_time >> 11) & 0x1F) as u32;
    let minute = ((dos_time >> 5) & 0x3F) as u32;
    let second = ((dos_time & 0x1F) * 2) as u32;
    let day = (dos_date & 0x1F) as u32;
    let month = ((dos_date >> 5) & 0x0F) as u32;
    let year = ((dos_date >> 9) & 0x7F) as u32 + 1980;

    let dt = chrono::NaiveDate::from_ymd_opt(year as i32, month, day)?
        .and_hms_opt(hour, minute, second)?;
    Some(dt.and_utc().into())
}

pub fn list_zip_entries(path: &Path) -> Result<Vec<ArchiveListEntry>> {
    let mut file = File::open(path)?;
    let zip_entries = read_zip_central_directory(&mut file)?;
    Ok(zip_entries
        .into_iter()
        .map(|e| ArchiveListEntry {
            name: e.name,
            is_dir: e.is_dir,
            size: if e.is_dir {
                None
            } else {
                Some(e.uncompressed_size)
            },
            pack_size: Some(e.compressed_size),
            modified: e.modified,
        })
        .collect())
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

fn parse_7z_slt_entries(output: &str) -> Vec<ArchiveListEntry> {
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
