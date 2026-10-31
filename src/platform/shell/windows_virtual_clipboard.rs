//! Stream Shell virtual clipboard files, including RDP files, on their OLE STA.
//! The system clipboard is never replaced and remote sources are never deleted.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf, Prefix};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::S_FALSE;
use windows::Win32::System::Com::{
    DVASPECT_CONTENT, FORMATETC, IDataObject, STGMEDIUM, TYMED_HGLOBAL, TYMED_ISTREAM,
};
use windows::Win32::System::DataExchange::{
    GetClipboardSequenceNumber, IsClipboardFormatAvailable, RegisterClipboardFormatW,
};
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::System::Ole::{
    CF_HDROP, OleGetClipboard, OleInitialize, OleUninitialize, ReleaseStgMedium,
};
use windows::Win32::UI::Shell::{
    CFSTR_FILECONTENTS, CFSTR_FILEDESCRIPTORW, FD_ATTRIBUTES, FD_FILESIZE, FILEDESCRIPTORW,
};
use windows::core::PCWSTR;

use crate::fs::transfer_queue::{
    TransferControl, TransferJob, TransferKind, TransferMessage, TransferProgress, TransferState,
};
use crate::utils::errors::{BExplorerError, Result};

const COPY_BUFFER_SIZE: usize = 1024 * 1024;
const REPARSE_POINT: u32 = 0x400;
const DIRECTORY: u32 = 0x10;
const MAX_DESCRIPTOR_BYTES: usize = 64 * 1024 * 1024;
static PARTIAL_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Do not render delayed clipboard data merely to enable a menu item.
pub(super) fn virtual_files_available() -> bool {
    unsafe {
        if IsClipboardFormatAvailable(CF_HDROP.0 as u32).is_ok() {
            return false;
        }
        let descriptor = RegisterClipboardFormatW(CFSTR_FILEDESCRIPTORW);
        descriptor != 0 && IsClipboardFormatAvailable(descriptor).is_ok()
    }
}

/// The transfer queue supplies a fresh thread. Keep every COM object on that
/// thread, and release it before the corresponding OleUninitialize call.
pub(super) fn run_virtual_transfer(
    job: TransferJob,
    tx: Sender<TransferMessage>,
    control: TransferControl,
    expected_sequence: u32,
) {
    let result = (|| {
        let _ole = OleClipboardGuard::new()?;
        if job.kind != TransferKind::Copy {
            return Err(invalid("Virtual clipboard transfers must be copies"));
        }
        if unsafe { GetClipboardSequenceNumber() } != expected_sequence {
            return Err(invalid(
                "The clipboard changed before this transfer started",
            ));
        }
        let data_object = unsafe { OleGetClipboard() }
            .map_err(|error| invalid(format!("Could not read the remote clipboard: {error}")))?;
        // Delayed rendering can itself change the clipboard sequence. Once
        // acquired, this IDataObject is the operation's source; do not compare
        // the global sequence again while rendering its remote formats.
        transfer_data_object(&job, &data_object, &tx, &control)
    })();
    let message = match result {
        Ok(completed_files) => TransferMessage::Finished {
            job_id: job.id,
            kind: TransferKind::Copy,
            completed_files,
            completed_roots: Vec::new(),
        },
        Err(_) if control.cancel.load(Ordering::Relaxed) => {
            TransferMessage::Cancelled { job_id: job.id }
        }
        Err(error) => TransferMessage::Failed {
            job_id: job.id,
            error: error.to_string(),
            permission_denied: false,
        },
    };
    let _ = tx.send(message);
}

fn invalid(message: impl Into<String>) -> BExplorerError {
    BExplorerError::Clipboard(message.into())
}

fn clipboard_format(name: PCWSTR, index: i32, medium: u32) -> Result<FORMATETC> {
    let format = unsafe { RegisterClipboardFormatW(name) };
    if format == 0 {
        return Err(invalid(
            "Could not register a virtual file clipboard format",
        ));
    }
    Ok(FORMATETC {
        cfFormat: format as u16,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: index,
        tymed: medium,
    })
}

struct Medium(STGMEDIUM);

impl Drop for Medium {
    fn drop(&mut self) {
        unsafe { ReleaseStgMedium(&mut self.0) };
    }
}

struct LockedGlobal(windows::Win32::Foundation::HGLOBAL);

impl Drop for LockedGlobal {
    fn drop(&mut self) {
        let _ = unsafe { GlobalUnlock(self.0) };
    }
}

#[derive(Debug)]
struct VirtualEntry {
    components: Vec<OsString>,
    directory: bool,
    size: Option<u64>,
}

fn read_entries(data_object: &IDataObject) -> Result<Vec<VirtualEntry>> {
    let format = clipboard_format(CFSTR_FILEDESCRIPTORW, -1, TYMED_HGLOBAL.0 as u32)?;
    let medium =
        Medium(unsafe { data_object.GetData(&format) }.map_err(|error| {
            invalid(format!("Could not read remote file descriptions: {error}"))
        })?);
    if medium.0.tymed != TYMED_HGLOBAL.0 as u32 {
        return Err(invalid(
            "The remote file description has an unsupported medium",
        ));
    }
    let memory = unsafe { medium.0.u.hGlobal };
    let length = unsafe { GlobalSize(memory) };
    if !(4..=MAX_DESCRIPTOR_BYTES).contains(&length) {
        return Err(invalid("The remote file description has an invalid size"));
    }
    let pointer = unsafe { GlobalLock(memory) }.cast::<u8>();
    if pointer.is_null() {
        return Err(invalid("Could not lock the remote file descriptions"));
    }
    let _lock = LockedGlobal(memory);
    let count = unsafe { std::ptr::read_unaligned(pointer.cast::<u32>()) } as usize;
    if count == 0 || count > (length - 4) / std::mem::size_of::<FILEDESCRIPTORW>() {
        return Err(invalid(
            "The remote file count exceeds its description buffer",
        ));
    }
    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        let descriptor = unsafe {
            std::ptr::read_unaligned(
                pointer
                    .add(4 + index * std::mem::size_of::<FILEDESCRIPTORW>())
                    .cast::<FILEDESCRIPTORW>(),
            )
        };
        entries.push(parse_descriptor(descriptor)?);
    }
    validate_entry_tree(&entries)?;
    Ok(entries)
}

fn parse_descriptor(descriptor: FILEDESCRIPTORW) -> Result<VirtualEntry> {
    let name = descriptor.cFileName;
    let end = name
        .iter()
        .position(|value| *value == 0)
        .ok_or_else(|| invalid("A remote filename is not terminated"))?;
    let name = &name[..end];
    if name.is_empty() || matches!(name.first(), Some(47 | 92)) {
        return Err(invalid(
            "Absolute and empty remote filenames are not supported",
        ));
    }
    if descriptor.dwFileAttributes & REPARSE_POINT != 0 {
        return Err(invalid(
            "Remote symbolic links and reparse points are not supported",
        ));
    }
    let directory = descriptor.dwFlags & FD_ATTRIBUTES.0 as u32 != 0
        && descriptor.dwFileAttributes & DIRECTORY != 0;
    let mut components = Vec::new();
    for part in name.split(|value| matches!(value, 47 | 92)) {
        validate_component(part)?;
        components.push(OsString::from_wide(part));
    }
    let size = (!directory && descriptor.dwFlags & FD_FILESIZE.0 as u32 != 0)
        .then_some(((descriptor.nFileSizeHigh as u64) << 32) | descriptor.nFileSizeLow as u64);
    Ok(VirtualEntry {
        components,
        directory,
        size,
    })
}

fn validate_component(part: &[u16]) -> Result<()> {
    if part.is_empty()
        || part == [46]
        || part == [46, 46]
        || matches!(part.last(), Some(32 | 46))
        || part.len() > 255
        || part
            .iter()
            .any(|value| *value < 32 || matches!(value, 34 | 42 | 58 | 60 | 62 | 63 | 124))
    {
        return Err(invalid(
            "A remote filename contains an unsafe path component",
        ));
    }
    let base = part.split(|value| *value == 46).next().unwrap_or_default();
    // Win32 also recognizes device basenames with spaces before an extension.
    let base = base
        .iter()
        .rposition(|value| *value != 32)
        .map(|end| &base[..=end])
        .unwrap_or_default();
    let upper = base
        .iter()
        .map(|value| {
            if (97..=122).contains(value) {
                *value - 32
            } else {
                *value
            }
        })
        .collect::<Vec<_>>();
    let reserved = ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"]
        .iter()
        .any(|name| upper == name.encode_utf16().collect::<Vec<_>>());
    let numbered_device = upper.len() == 4
        && (upper[..3] == [67, 79, 77] || upper[..3] == [76, 80, 84])
        && matches!(upper[3], 49..=57 | 0xB9 | 0xB2 | 0xB3);
    if reserved || numbered_device {
        return Err(invalid(
            "A remote filename uses a reserved Windows device name",
        ));
    }
    Ok(())
}

fn path_key(components: &[OsString]) -> Vec<u16> {
    components
        .iter()
        .flat_map(|part| {
            part.encode_wide()
                .map(|value| {
                    if (65..=90).contains(&value) {
                        value + 32
                    } else {
                        value
                    }
                })
                .chain([92])
        })
        .collect()
}

fn validate_entry_tree(entries: &[VirtualEntry]) -> Result<()> {
    let mut kinds = HashMap::new();
    for entry in entries {
        if kinds
            .insert(path_key(&entry.components), entry.directory)
            .is_some()
        {
            return Err(invalid("The remote file list contains duplicate paths"));
        }
    }
    for entry in entries {
        for length in 1..entry.components.len() {
            if kinds.get(&path_key(&entry.components[..length])) == Some(&false) {
                return Err(invalid("A remote file is also used as a parent folder"));
            }
        }
    }
    Ok(())
}

/// Deny deletion/renaming while copying and open reparse points themselves.
/// Holding every ancestor prevents a checked directory from being swapped for
/// a junction while a remote stream is being written underneath it.
fn lock_directory(path: &Path) -> Result<File> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_TAG_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FileAttributeTagInfo, GetFileInformationByHandleEx,
    };
    let file = OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES.0)
        .share_mode(FILE_SHARE_READ.0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_dir() {
        return Err(invalid("The destination contains a non-directory"));
    }
    if metadata.file_attributes() & REPARSE_POINT != 0 {
        let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
        unsafe {
            GetFileInformationByHandleEx(
                HANDLE(file.as_raw_handle()),
                FileAttributeTagInfo,
                &mut tag as *mut _ as *mut _,
                std::mem::size_of_val(&tag) as u32,
            )?;
        }
        // Cloud placeholders and other nonredirecting tags retain this path.
        // Junctions, symlinks and other name surrogates redirect it elsewhere.
        if tag.ReparseTag & 0x2000_0000 != 0 {
            return Err(invalid(
                "The destination contains a redirecting reparse point",
            ));
        }
    }
    Ok(file)
}

fn lock_destination(destination: &Path) -> Result<Vec<File>> {
    if !destination.is_absolute() {
        return Err(invalid(
            "The destination must be an absolute directory path",
        ));
    }
    let mut current = PathBuf::new();
    let mut locks = Vec::new();
    for component in destination.components() {
        match component {
            Component::Prefix(prefix) => {
                if !matches!(
                    prefix.kind(),
                    Prefix::Disk(_)
                        | Prefix::UNC(_, _)
                        | Prefix::VerbatimDisk(_)
                        | Prefix::VerbatimUNC(_, _)
                ) {
                    return Err(invalid("Windows device destinations are not supported"));
                }
                current.push(component.as_os_str());
            }
            Component::RootDir | Component::Normal(_) => {
                current.push(component.as_os_str());
                locks.push(lock_directory(&current)?);
            }
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(invalid(
                    "Parent components are not supported in the destination",
                ));
            }
        }
    }
    Ok(locks)
}

fn alternate_name(original: &std::ffi::OsStr, index: u32, directory: bool) -> OsString {
    if index == 0 {
        return original.to_os_string();
    }
    let path = Path::new(original);
    let (stem, extension) = if directory {
        (original, None)
    } else {
        (path.file_stem().unwrap_or(original), path.extension())
    };
    let suffix = format!(" (copy {index})")
        .encode_utf16()
        .collect::<Vec<_>>();
    let extension = extension
        .map(|extension| extension.encode_wide().collect::<Vec<_>>())
        .unwrap_or_default();
    let extension_length = extension.len() + usize::from(!extension.is_empty());
    let stem_budget = 255_usize
        .saturating_sub(suffix.len() + extension_length)
        .max(1);
    let mut wide = stem.encode_wide().take(stem_budget).collect::<Vec<_>>();
    wide.extend(suffix);
    if !extension.is_empty() {
        wide.push(46);
        wide.extend(extension);
    }
    OsString::from_wide(&wide)
}

fn ensure_subdirectories(
    root: &Path,
    components: &[OsString],
    directories: &mut HashMap<Vec<u16>, PathBuf>,
    locks: &mut Vec<File>,
) -> Result<PathBuf> {
    let mut parent = root.to_path_buf();
    for length in 1..=components.len() {
        let key = path_key(&components[..length]);
        if let Some(existing) = directories.get(&key) {
            parent = existing.clone();
            continue;
        }
        let original = &components[length - 1];
        let mut created = None;
        for index in 0..10_000 {
            let candidate = parent.join(alternate_name(original, index, true));
            match fs::create_dir(&candidate) {
                Ok(()) => {
                    created = Some(candidate);
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }
        parent =
            created.ok_or_else(|| invalid("Could not choose an unused destination folder name"))?;
        locks.push(lock_directory(&parent)?);
        directories.insert(key, parent.clone());
    }
    Ok(parent)
}

fn new_partial(parent: &Path, job_id: u64) -> Result<(PathBuf, File)> {
    for _ in 0..100 {
        let sequence = PARTIAL_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(
            ".bexplorer-partial-{}-{job_id}-{sequence}",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .share_mode(0)
            .open(&path)
        {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    Err(invalid("Could not create an unused partial transfer file"))
}

fn publish_partial(partial: &Path, parent: &Path, name: &std::ffi::OsStr) -> Result<()> {
    use windows::Win32::Storage::FileSystem::MoveFileW;
    let source = partial
        .as_os_str()
        .encode_wide()
        .chain([0])
        .collect::<Vec<_>>();
    for index in 0..10_000 {
        let target = parent.join(alternate_name(name, index, false));
        let target = target
            .as_os_str()
            .encode_wide()
            .chain([0])
            .collect::<Vec<_>>();
        match unsafe { MoveFileW(PCWSTR(source.as_ptr()), PCWSTR(target.as_ptr())) } {
            Ok(()) => return Ok(()),
            Err(error) if matches!(error.code().0 as u32 & 0xffff, 80 | 183) => {}
            Err(error) => return Err(error.into()),
        }
    }
    Err(invalid("Could not choose an unused destination filename"))
}

struct VirtualProgress {
    copied: u64,
    total: u64,
    done: usize,
    files: usize,
    last_emit: Instant,
    last_bytes: u64,
}

impl VirtualProgress {
    fn emit(
        &mut self,
        job: &TransferJob,
        tx: &Sender<TransferMessage>,
        name: &str,
        state: TransferState,
        force: bool,
    ) {
        let elapsed = self.last_emit.elapsed();
        if !force && elapsed < Duration::from_millis(80) {
            return;
        }
        let speed = if state == TransferState::Copying {
            self.copied.saturating_sub(self.last_bytes) as f64 / elapsed.as_secs_f64().max(0.001)
        } else {
            0.0
        };
        let _ = tx.send(TransferMessage::Progress(TransferProgress {
            job_id: job.id,
            kind: TransferKind::Copy,
            state,
            current_name: name.to_owned(),
            destination: job.destination.clone(),
            copied_bytes: self.copied,
            total_bytes: self.total,
            files_done: self.done,
            total_files: self.files,
            bytes_per_second: speed,
        }));
        self.last_bytes = self.copied;
        self.last_emit = Instant::now();
    }

    fn wait(
        &mut self,
        job: &TransferJob,
        tx: &Sender<TransferMessage>,
        control: &TransferControl,
        name: &str,
    ) -> Result<()> {
        while control.pause.load(Ordering::Relaxed) {
            if control.cancel.load(Ordering::Relaxed) {
                return Err(invalid("Transfer cancelled"));
            }
            self.emit(job, tx, name, TransferState::Paused, false);
            std::thread::sleep(Duration::from_millis(80));
        }
        if control.cancel.load(Ordering::Relaxed) {
            return Err(invalid("Transfer cancelled"));
        }
        Ok(())
    }
}

fn transfer_data_object(
    job: &TransferJob,
    data_object: &IDataObject,
    tx: &Sender<TransferMessage>,
    control: &TransferControl,
) -> Result<usize> {
    // Validate every descriptor and the complete source tree before creating
    // any destination file or folder. Metadata is bounded; file data is streamed.
    let entries = read_entries(data_object)?;
    let known_sizes = entries
        .iter()
        .all(|entry| entry.directory || entry.size.is_some());
    let total = entries
        .iter()
        .filter_map(|entry| entry.size)
        .try_fold(0_u64, |sum, size| {
            sum.checked_add(size)
                .ok_or_else(|| invalid("The remote file sizes exceed the supported range"))
        })?;
    let mut progress = VirtualProgress {
        copied: 0,
        total: if known_sizes { total } else { 0 },
        done: 0,
        files: entries.len(),
        last_emit: Instant::now(),
        last_bytes: 0,
    };
    progress.wait(job, tx, control, "Preparing remote files")?;
    // Resolve the explicitly chosen destination once. Then pin that resolved
    // root, using its extended Windows path for every child and final rename.
    // This allows an intentional junction alias without following new aliases
    // injected underneath it, and supports roots longer than MAX_PATH.
    let destination = fs::canonicalize(&job.destination)?;
    let mut locks = lock_destination(&destination)?;
    let mut directories = HashMap::new();
    let mut buffer = vec![0_u8; COPY_BUFFER_SIZE];
    for (index, entry) in entries.iter().enumerate() {
        let name = entry.components.last().unwrap().to_string_lossy();
        progress.wait(job, tx, control, &name)?;
        progress.emit(job, tx, &name, TransferState::Copying, true);
        if entry.directory {
            ensure_subdirectories(
                &destination,
                &entry.components,
                &mut directories,
                &mut locks,
            )?;
        } else {
            let parent = ensure_subdirectories(
                &destination,
                &entry.components[..entry.components.len() - 1],
                &mut directories,
                &mut locks,
            )?;
            let format =
                clipboard_format(CFSTR_FILECONTENTS, index as i32, TYMED_ISTREAM.0 as u32)?;
            let medium = Medium(unsafe { data_object.GetData(&format) }.map_err(|error| {
                invalid(format!("Could not open a remote file stream: {error}"))
            })?);
            if medium.0.tymed != TYMED_ISTREAM.0 as u32 {
                return Err(invalid(
                    "The remote provider does not offer a supported file stream",
                ));
            }
            let stream = unsafe { medium.0.u.pstm.as_ref() }
                .ok_or_else(|| invalid("The remote provider returned an empty file stream"))?;
            let (partial, mut output) = new_partial(&parent, job.id)?;
            let mut file_bytes = 0_u64;
            loop {
                progress.wait(job, tx, control, &name)?;
                let mut read = 0_u32;
                let status = unsafe {
                    stream.Read(
                        buffer.as_mut_ptr().cast(),
                        buffer.len() as u32,
                        Some(&mut read),
                    )
                };
                status
                    .ok()
                    .map_err(|error| invalid(format!("Reading a remote file failed: {error}")))?;
                if read as usize > buffer.len() {
                    return Err(invalid("The remote stream returned an invalid byte count"));
                }
                if read == 0 {
                    break;
                }
                file_bytes = file_bytes
                    .checked_add(read as u64)
                    .ok_or_else(|| invalid("The remote file size overflowed"))?;
                if entry.size.is_some_and(|expected| file_bytes > expected) {
                    return Err(invalid("The remote file exceeded its advertised size"));
                }
                output.write_all(&buffer[..read as usize])?;
                progress.copied = progress
                    .copied
                    .checked_add(read as u64)
                    .ok_or_else(|| invalid("The transferred size overflowed"))?;
                progress.emit(job, tx, &name, TransferState::Copying, false);
                // S_FALSE may accompany a non-empty final chunk. Keep those
                // bytes and avoid another remote round trip just to read EOF.
                if status == S_FALSE {
                    break;
                }
            }
            if entry.size.is_some_and(|expected| expected != file_bytes) {
                return Err(invalid("The remote file ended before its advertised size"));
            }
            progress.wait(job, tx, control, &name)?;
            output.flush()?;
            progress.wait(job, tx, control, &name)?;
            drop(output);
            publish_partial(&partial, &parent, entry.components.last().unwrap())?;
        }
        progress.done += 1;
        progress.emit(job, tx, &name, TransferState::Copying, false);
    }
    if !known_sizes {
        progress.total = progress.copied;
    }
    progress.emit(job, tx, "", TransferState::Finished, true);
    Ok(progress.done)
}

struct OleClipboardGuard;

impl OleClipboardGuard {
    fn new() -> Result<Self> {
        unsafe { OleInitialize(None) }
            .map_err(|error| invalid(format!("Could not initialize OLE: {error}")))?;
        Ok(Self)
    }
}

impl Drop for OleClipboardGuard {
    fn drop(&mut self) {
        unsafe { OleUninitialize() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{ManuallyDrop, size_of};
    use windows::Win32::Foundation::{
        BOOL, DV_E_FORMATETC, DV_E_TYMED, E_INVALIDARG, E_NOTIMPL, GlobalFree, HGLOBAL,
        OLE_E_ADVISENOTSUPPORTED, S_OK,
    };
    use windows::Win32::System::Com::{
        DATADIR_GET, IAdviseSink, IEnumFORMATETC, IEnumSTATDATA, STGMEDIUM_0,
    };
    use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc};
    use windows::Win32::UI::Shell::{FD_UNICODE, SHCreateMemStream, SHCreateStdEnumFmtEtc};
    use windows::core::{Error, HRESULT};

    struct FixtureEntry {
        descriptor: FILEDESCRIPTORW,
        contents: Vec<u8>,
    }

    impl FixtureEntry {
        fn file(name: &str, contents: &[u8]) -> Self {
            let mut filename = [0_u16; 260];
            let encoded = name.encode_utf16().collect::<Vec<_>>();
            filename[..encoded.len()].copy_from_slice(&encoded);
            Self {
                descriptor: FILEDESCRIPTORW {
                    dwFlags: (FD_ATTRIBUTES.0 | FD_FILESIZE.0 | FD_UNICODE.0) as u32,
                    dwFileAttributes: 0x80,
                    nFileSizeLow: contents.len() as u32,
                    cFileName: filename,
                    ..Default::default()
                },
                contents: contents.to_vec(),
            }
        }

        fn directory(name: &str) -> Self {
            let mut entry = Self::file(name, &[]);
            entry.descriptor.dwFileAttributes = DIRECTORY;
            entry
        }
    }

    // This source offers no paths or Shell IDList. It never enters the system
    // clipboard, so test execution preserves the user's copied remote files.
    #[windows::core::implement(IDataObject)]
    struct VirtualFileDataObject {
        entries: Vec<FixtureEntry>,
        advertised_count: Option<u32>,
    }

    fn descriptor_memory(
        entries: &[FixtureEntry],
        advertised_count: Option<u32>,
    ) -> windows::core::Result<HGLOBAL> {
        unsafe {
            let memory = GlobalAlloc(
                GMEM_MOVEABLE,
                4 + entries.len() * size_of::<FILEDESCRIPTORW>(),
            )?;
            let pointer = GlobalLock(memory).cast::<u8>();
            if pointer.is_null() {
                let _ = GlobalFree(memory);
                return Err(Error::from_win32());
            }
            std::ptr::write_unaligned(
                pointer.cast::<u32>(),
                advertised_count.unwrap_or(entries.len() as u32),
            );
            for (index, entry) in entries.iter().enumerate() {
                std::ptr::write_unaligned(
                    pointer
                        .add(4 + index * size_of::<FILEDESCRIPTORW>())
                        .cast::<FILEDESCRIPTORW>(),
                    entry.descriptor,
                );
            }
            let _ = GlobalUnlock(memory);
            Ok(memory)
        }
    }

    impl VirtualFileDataObject_Impl {
        fn query(&self, format: *const FORMATETC) -> HRESULT {
            if format.is_null() {
                return E_INVALIDARG;
            }
            let format = unsafe { *format };
            if format.dwAspect != DVASPECT_CONTENT.0 {
                return DV_E_FORMATETC;
            }
            let descriptors =
                clipboard_format(CFSTR_FILEDESCRIPTORW, -1, TYMED_HGLOBAL.0 as u32).unwrap();
            if format.cfFormat == descriptors.cfFormat && format.lindex == -1 {
                return if format.tymed & descriptors.tymed != 0 {
                    S_OK
                } else {
                    DV_E_TYMED
                };
            }
            let contents = clipboard_format(CFSTR_FILECONTENTS, 0, TYMED_ISTREAM.0 as u32).unwrap();
            if format.cfFormat == contents.cfFormat
                && format.lindex >= 0
                && (format.lindex as usize) < self.entries.len()
            {
                return if format.tymed & contents.tymed != 0 {
                    S_OK
                } else {
                    DV_E_TYMED
                };
            }
            DV_E_FORMATETC
        }
    }

    #[allow(non_snake_case)]
    impl windows::Win32::System::Com::IDataObject_Impl for VirtualFileDataObject_Impl {
        fn GetData(&self, format: *const FORMATETC) -> windows::core::Result<STGMEDIUM> {
            self.query(format).ok()?;
            let format = unsafe { *format };
            if format.lindex == -1 {
                return Ok(STGMEDIUM {
                    tymed: TYMED_HGLOBAL.0 as u32,
                    u: STGMEDIUM_0 {
                        hGlobal: descriptor_memory(&self.entries, self.advertised_count)?,
                    },
                    pUnkForRelease: ManuallyDrop::new(None),
                });
            }
            let stream =
                unsafe { SHCreateMemStream(Some(&self.entries[format.lindex as usize].contents)) }
                    .ok_or_else(Error::from_win32)?;
            Ok(STGMEDIUM {
                tymed: TYMED_ISTREAM.0 as u32,
                u: STGMEDIUM_0 {
                    pstm: ManuallyDrop::new(Some(stream)),
                },
                pUnkForRelease: ManuallyDrop::new(None),
            })
        }

        fn GetDataHere(&self, _: *const FORMATETC, _: *mut STGMEDIUM) -> windows::core::Result<()> {
            Err(Error::from(E_NOTIMPL))
        }
        fn QueryGetData(&self, format: *const FORMATETC) -> HRESULT {
            self.query(format)
        }
        fn GetCanonicalFormatEtc(&self, _: *const FORMATETC, _: *mut FORMATETC) -> HRESULT {
            E_NOTIMPL
        }
        fn SetData(
            &self,
            _: *const FORMATETC,
            _: *const STGMEDIUM,
            _: BOOL,
        ) -> windows::core::Result<()> {
            Err(Error::from(E_NOTIMPL))
        }
        fn EnumFormatEtc(&self, direction: u32) -> windows::core::Result<IEnumFORMATETC> {
            if direction != DATADIR_GET.0 as u32 {
                return Err(Error::from(E_NOTIMPL));
            }
            unsafe {
                SHCreateStdEnumFmtEtc(&[
                    clipboard_format(CFSTR_FILEDESCRIPTORW, -1, TYMED_HGLOBAL.0 as u32).unwrap(),
                    clipboard_format(CFSTR_FILECONTENTS, 0, TYMED_ISTREAM.0 as u32).unwrap(),
                ])
            }
        }
        fn DAdvise(
            &self,
            _: *const FORMATETC,
            _: u32,
            _: Option<&IAdviseSink>,
        ) -> windows::core::Result<u32> {
            Err(Error::from(OLE_E_ADVISENOTSUPPORTED))
        }
        fn DUnadvise(&self, _: u32) -> windows::core::Result<()> {
            Err(Error::from(OLE_E_ADVISENOTSUPPORTED))
        }
        fn EnumDAdvise(&self) -> windows::core::Result<IEnumSTATDATA> {
            Err(Error::from(OLE_E_ADVISENOTSUPPORTED))
        }
    }

    static TEST_DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    struct TestDirectory(PathBuf);
    impl TestDirectory {
        fn new() -> Self {
            let local = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap()).join("Temp");
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            for _ in 0..1_000 {
                let sequence = TEST_DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
                let path = local.join(format!(
                    "bexplorer-virtual-clipboard-test-{}-{timestamp}-{sequence}",
                    std::process::id()
                ));
                assert!(path.is_absolute());
                match fs::create_dir(&path) {
                    // Ownership starts only after our atomic directory creation
                    // succeeds; an existing candidate is never removed/reused.
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => panic!("could not create virtual clipboard fixture: {error}"),
                }
            }
            panic!("could not reserve a unique virtual clipboard fixture directory");
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let local = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap()).join("Temp");
            assert_eq!(self.0.parent(), Some(local.as_path()));
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn in_sta(test: impl FnOnce(&Path) + Send + 'static) {
        std::thread::spawn(move || {
            let _ole = OleClipboardGuard::new().unwrap();
            let directory = TestDirectory::new();
            test(&directory.0);
        })
        .join()
        .unwrap();
    }

    fn copy_fixture(destination: &Path, entries: Vec<FixtureEntry>) -> Result<usize> {
        let object: IDataObject = VirtualFileDataObject {
            entries,
            advertised_count: None,
        }
        .into();
        let job = TransferJob {
            id: 901,
            sources: Vec::new(),
            destination: destination.to_path_buf(),
            kind: TransferKind::Copy,
            conflict_policy: crate::fs::transfer_queue::ConflictPolicy::KeepBoth,
        };
        let (tx, _rx) = std::sync::mpsc::channel();
        transfer_data_object(&job, &object, &tx, &TransferControl::new())
    }

    #[test]
    fn virtual_clipboard_fixture_directories_remain_unique_when_created_concurrently() {
        use std::collections::HashSet;
        use std::sync::{Arc, Barrier};

        const WORKERS: usize = 16;
        const DIRECTORIES_PER_WORKER: usize = 4;
        let start = Arc::new(Barrier::new(WORKERS));
        let workers = (0..WORKERS)
            .map(|worker| {
                let start = Arc::clone(&start);
                std::thread::spawn(move || {
                    start.wait();
                    (0..DIRECTORIES_PER_WORKER)
                        .map(|index| {
                            let directory = TestDirectory::new();
                            let owner = format!("{worker}:{index}");
                            fs::write(directory.0.join("fixture-owner.txt"), &owner).unwrap();
                            (directory, owner)
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>();
        // Returned fixtures remain alive in their join result or this vector,
        // so every path must coexist until all workers have finished.
        let directories = workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>();
        let paths = directories
            .iter()
            .map(|(directory, _)| directory.0.clone())
            .collect::<HashSet<_>>();
        assert_eq!(paths.len(), WORKERS * DIRECTORIES_PER_WORKER);
        for (directory, owner) in &directories {
            assert!(directory.0.is_dir());
            assert_eq!(
                fs::read_to_string(directory.0.join("fixture-owner.txt")).unwrap(),
                *owner
            );
        }
    }

    #[test]
    fn virtual_stream_import_handles_unicode_directories_empty_and_final_short_chunks() {
        in_sta(|directory| {
            let payload = b"Remote virtual file\r\n\0payload";
            assert_eq!(
                copy_fixture(
                    directory,
                    vec![
                        FixtureEntry::directory("Carpeta á"),
                        FixtureEntry::file("Carpeta á\\日本語.txt", payload),
                        FixtureEntry::file("Carpeta á\\vacío.txt", &[]),
                    ]
                )
                .unwrap(),
                3
            );
            assert_eq!(
                fs::read(directory.join("Carpeta á\\日本語.txt")).unwrap(),
                payload
            );
            assert_eq!(
                fs::metadata(directory.join("Carpeta á\\vacío.txt"))
                    .unwrap()
                    .len(),
                0
            );
            assert_eq!(
                fs::read_dir(directory.join("Carpeta á")).unwrap().count(),
                2
            );
        });
    }

    #[test]
    fn virtual_stream_import_preserves_existing_file_and_directory_collisions() {
        in_sta(|directory| {
            fs::write(directory.join("report.txt"), b"original").unwrap();
            fs::create_dir(directory.join("folder")).unwrap();
            fs::write(directory.join("folder\\original.txt"), b"keep").unwrap();
            copy_fixture(
                directory,
                vec![
                    FixtureEntry::file("report.txt", b"new"),
                    FixtureEntry::directory("folder"),
                    FixtureEntry::file("folder\\child.txt", b"child"),
                ],
            )
            .unwrap();
            assert_eq!(fs::read(directory.join("report.txt")).unwrap(), b"original");
            assert_eq!(
                fs::read(directory.join("report (copy 1).txt")).unwrap(),
                b"new"
            );
            assert_eq!(
                fs::read(directory.join("folder\\original.txt")).unwrap(),
                b"keep"
            );
            assert!(!directory.join("folder\\child.txt").exists());
            assert_eq!(
                fs::read(directory.join("folder (copy 1)\\child.txt")).unwrap(),
                b"child"
            );
        });
    }

    #[test]
    fn virtual_stream_import_rejects_all_unsafe_descriptions_before_creating_anything() {
        in_sta(|directory| {
            for name in [
                "..\\escape.txt",
                "C:\\escape.txt",
                "\\\\server\\share\\file",
                "file:stream",
                "CON.txt",
                "CON .txt",
                "safe\\..\\escape",
                "trailing.",
            ] {
                assert!(
                    copy_fixture(
                        directory,
                        vec![
                            FixtureEntry::file("good.txt", b"good"),
                            FixtureEntry::file(name, b"bad")
                        ]
                    )
                    .is_err()
                );
                assert_eq!(fs::read_dir(directory).unwrap().count(), 0);
            }
            let mut reparse = FixtureEntry::file("link", b"bad");
            reparse.descriptor.dwFileAttributes |= REPARSE_POINT;
            assert!(copy_fixture(directory, vec![reparse]).is_err());
            assert_eq!(fs::read_dir(directory).unwrap().count(), 0);
        });
    }

    #[test]
    fn virtual_stream_import_preserves_partial_when_provider_ends_early() {
        in_sta(|directory| {
            let mut short = FixtureEntry::file("incomplete.txt", b"partial");
            short.descriptor.nFileSizeLow = 100;
            assert!(copy_fixture(directory, vec![short]).is_err());
            assert!(!directory.join("incomplete.txt").exists());
            let partials = fs::read_dir(directory)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect::<Vec<_>>();
            assert_eq!(partials.len(), 1);
            assert_eq!(fs::read(&partials[0]).unwrap(), b"partial");
        });
    }

    #[test]
    fn virtual_descriptions_bound_count_and_preserve_wide_paths_and_large_sizes() {
        in_sta(|_| {
            let object: IDataObject = VirtualFileDataObject {
                entries: vec![FixtureEntry::file("ok", &[])],
                advertised_count: Some(u32::MAX),
            }
            .into();
            assert!(read_entries(&object).is_err());
            let mut descriptor = FixtureEntry::file("x", &[]).descriptor;
            descriptor.nFileSizeHigh = 5;
            descriptor.nFileSizeLow = 17;
            let mut wide_name = [0_u16; 260];
            wide_name[..2].copy_from_slice(&[0xD801, 120]);
            descriptor.cFileName = wide_name;
            let parsed = parse_descriptor(descriptor).unwrap();
            assert_eq!(parsed.size, Some((5_u64 << 32) | 17));
            assert_eq!(
                parsed.components[0].encode_wide().collect::<Vec<_>>(),
                [0xD801, 120]
            );
        });
    }

    #[test]
    fn virtual_stream_import_keeps_unknown_sizes_indeterminate_until_finished() {
        in_sta(|directory| {
            let mut entry = FixtureEntry::file("unknown.txt", b"unknown size");
            entry.descriptor.dwFlags &= !(FD_FILESIZE.0 as u32);
            let object: IDataObject = VirtualFileDataObject {
                entries: vec![entry],
                advertised_count: None,
            }
            .into();
            let job = TransferJob {
                id: 902,
                sources: Vec::new(),
                destination: directory.to_path_buf(),
                kind: TransferKind::Copy,
                conflict_policy: crate::fs::transfer_queue::ConflictPolicy::KeepBoth,
            };
            let (tx, rx) = std::sync::mpsc::channel();
            transfer_data_object(&job, &object, &tx, &TransferControl::new()).unwrap();
            let progress = rx
                .try_iter()
                .filter_map(|message| {
                    if let TransferMessage::Progress(progress) = message {
                        Some(progress)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            assert!(
                progress
                    .iter()
                    .filter(|item| item.state != TransferState::Finished)
                    .all(|item| item.total_bytes == 0)
            );
            let final_progress = progress.last().unwrap();
            assert_eq!(final_progress.state, TransferState::Finished);
            assert_eq!(final_progress.total_bytes, b"unknown size".len() as u64);
            assert_eq!(final_progress.copied_bytes, final_progress.total_bytes);
        });
    }

    #[test]
    fn virtual_stream_import_supports_long_destination_paths() {
        in_sta(|directory| {
            let mut long_root = directory.to_path_buf();
            for index in 0..8 {
                long_root.push(format!("long-root-{index}-abcdefghijklmnopqrstuv"));
            }
            fs::create_dir_all(&long_root).unwrap();
            assert!(long_root.as_os_str().encode_wide().count() > 260);
            copy_fixture(
                &long_root,
                vec![FixtureEntry::file("nested\\long.txt", b"long path")],
            )
            .unwrap();
            assert_eq!(
                fs::read(long_root.join("nested\\long.txt")).unwrap(),
                b"long path"
            );
        });
    }

    #[test]
    fn virtual_stream_import_resolves_an_explicit_junction_destination_once() {
        in_sta(|directory| {
            use std::os::windows::process::CommandExt;
            let target = directory.join("selected-target");
            let alias = directory.join("selected-alias");
            fs::create_dir(&target).unwrap();
            // Both paths belong to this unique local test directory. Creating
            // a junction through PowerShell requires no symlink privilege.
            assert_eq!(target.parent(), Some(directory));
            assert_eq!(alias.parent(), Some(directory));
            let quote = |path: &Path| path.to_str().unwrap().replace('\'', "''");
            let script = format!(
                "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path '{}' -Target '{}' | Out-Null",
                quote(&alias),
                quote(&target)
            );
            let result = std::process::Command::new("powershell.exe")
                .creation_flags(0x0800_0000)
                .args(["-NoProfile", "-NonInteractive", "-Command", &script])
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert!(
                lock_directory(&alias).is_err(),
                "an unchecked redirector must be rejected"
            );
            copy_fixture(
                &alias,
                vec![FixtureEntry::file("resolved.txt", b"explicit alias")],
            )
            .unwrap();
            assert_eq!(
                fs::read(target.join("resolved.txt")).unwrap(),
                b"explicit alias"
            );
        });
    }
}
