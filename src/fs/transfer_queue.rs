use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::fs::{explorer, portable};
use crate::utils::errors::{BExplorerError, Result};

const COPY_BUFFER_SIZE: usize = 1024 * 1024;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(80);
static RESERVED_TARGETS: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
static TRANSFER_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum TransferKind {
    Copy,
    Move,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferState {
    Pending,
    Copying,
    Paused,
    Cancelled,
    Finished,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum ConflictPolicy {
    Replace,
    Skip,
    KeepBoth,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TransferJob {
    pub id: u64,
    pub sources: Vec<PathBuf>,
    pub destination: PathBuf,
    pub kind: TransferKind,
    pub conflict_policy: ConflictPolicy,
}

#[derive(Clone)]
pub struct TransferControl {
    pub cancel: Arc<AtomicBool>,
    pub pause: Arc<AtomicBool>,
}

impl TransferControl {
    pub fn new() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            pause: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[derive(Clone, Debug)]
pub struct TransferProgress {
    pub job_id: u64,
    pub kind: TransferKind,
    pub state: TransferState,
    pub current_name: String,
    pub destination: PathBuf,
    pub copied_bytes: u64,
    pub total_bytes: u64,
    pub files_done: usize,
    pub total_files: usize,
    pub bytes_per_second: f64,
}

/// A completed top-level item and the exact destination chosen for it. The
/// destination can differ from the source filename when Keep Both resolves a
/// collision, so callers must use this instead of reconstructing a path.
#[derive(Clone, Debug)]
pub struct TransferCompletedRoot {
    pub source: PathBuf,
    pub target: PathBuf,
}

#[derive(Debug)]
struct TransferOutcome {
    completed_files: usize,
    completed_roots: Vec<TransferCompletedRoot>,
}

impl TransferProgress {
    pub fn pending(job: &TransferJob) -> Self {
        Self {
            job_id: job.id,
            kind: job.kind,
            state: TransferState::Pending,
            current_name: job
                .sources
                .first()
                .map(|path| display_name(path))
                .unwrap_or_else(|| "Preparing...".into()),
            destination: job.destination.clone(),
            copied_bytes: 0,
            total_bytes: 0,
            files_done: 0,
            total_files: 0,
            bytes_per_second: 0.0,
        }
    }
}

#[derive(Clone, Debug)]
pub enum TransferMessage {
    Progress(TransferProgress),
    Finished {
        job_id: u64,
        kind: TransferKind,
        completed_files: usize,
        completed_roots: Vec<TransferCompletedRoot>,
    },
    Failed {
        job_id: u64,
        error: String,
    },
    Cancelled {
        job_id: u64,
    },
}

struct TransferRuntime {
    copied_bytes: u64,
    files_done: usize,
    total_bytes: u64,
    total_files: usize,
    started: Instant,
    last_emit: Instant,
    created_targets: Vec<PathBuf>,
    tracked_targets: HashSet<PathBuf>,
    reserved_targets: Vec<PathBuf>,
    completed_roots: Vec<TransferCompletedRoot>,
}

impl TransferRuntime {
    fn new(total_bytes: u64, total_files: usize) -> Self {
        Self {
            copied_bytes: 0,
            files_done: 0,
            total_bytes,
            total_files,
            started: Instant::now(),
            last_emit: Instant::now() - Duration::from_secs(1),
            created_targets: Vec::new(),
            tracked_targets: HashSet::new(),
            reserved_targets: Vec::new(),
            completed_roots: Vec::new(),
        }
    }

    fn track_created(&mut self, path: &Path) {
        let path = path.to_path_buf();
        if self.tracked_targets.insert(path.clone()) {
            self.created_targets.push(path);
        }
    }

    fn track_reserved(&mut self, path: &Path) {
        self.reserved_targets.push(path.to_path_buf());
    }

    fn track_completed_root(&mut self, source: &Path, target: &Path) {
        self.completed_roots.push(TransferCompletedRoot {
            source: source.to_path_buf(),
            target: target.to_path_buf(),
        });
    }
}

pub fn run_transfer(job: TransferJob, tx: Sender<TransferMessage>, control: TransferControl) {
    match run_transfer_inner(&job, &tx, &control) {
        Ok(outcome) => {
            let _ = tx.send(TransferMessage::Finished {
                job_id: job.id,
                kind: job.kind,
                completed_files: outcome.completed_files,
                completed_roots: outcome.completed_roots,
            });
        }
        Err(error) if control.cancel.load(Ordering::Relaxed) => {
            let _ = tx.send(TransferMessage::Cancelled { job_id: job.id });
            crate::utils::log::error(error.to_string());
        }
        Err(error) => {
            let _ = tx.send(TransferMessage::Failed {
                job_id: job.id,
                error: error.to_string(),
            });
        }
    }
}

fn run_transfer_inner(
    job: &TransferJob,
    tx: &Sender<TransferMessage>,
    control: &TransferControl,
) -> Result<TransferOutcome> {
    let has_portable_destination = explorer::is_portable_path(&job.destination);
    let has_portable_sources = job
        .sources
        .iter()
        .any(|source| explorer::is_portable_path(source));
    if has_portable_destination || has_portable_sources {
        return run_portable_transfer(job, tx, control);
    }
    if explorer::is_virtual_path(&job.destination)
        || job
            .sources
            .iter()
            .any(|source| explorer::is_virtual_path(source))
    {
        return Err(BExplorerError::Operation(
            "Transfers with this virtual location are not available".into(),
        ));
    }

    if !job.destination.is_dir() {
        return Err(BExplorerError::InvalidPath(job.destination.clone()));
    }

    let total_bytes = job.sources.iter().map(|path| path_total_bytes(path)).sum();
    let total_files = job.sources.iter().map(|path| path_file_count(path)).sum();
    let mut runtime = TransferRuntime::new(total_bytes, total_files);

    emit_progress(job, "", TransferState::Copying, &runtime, tx);

    let result = (|| {
        for source in &job.sources {
            check_cancelled(control)?;
            wait_if_paused(job, "", &runtime, tx, control)?;
            if !source.exists() {
                continue;
            }

            let Some(name) = source.file_name() else {
                continue;
            };
            let Some(target) = reserve_destination(
                &job.destination.join(name),
                source.is_dir(),
                job.conflict_policy,
            ) else {
                mark_source_skipped(job, source, &mut runtime, tx);
                continue;
            };
            // Replacing an item means replacing the complete top-level entry,
            // not merging two directories and leaving stale files behind. A
            // source pasted into its own directory is the lone exception:
            // replacing it would destroy the input before it can be read, so
            // treat it as a completed no-op instead.
            if source == &target {
                mark_source_skipped(job, source, &mut runtime, tx);
                continue;
            }
            runtime.track_reserved(&target);
            let replacing = job.conflict_policy == ConflictPolicy::Replace && target.exists();
            match (job.kind, replacing) {
                (TransferKind::Copy, true) => {
                    replace_path_staged(job, source, &target, tx, control, &mut runtime)?
                }
                (TransferKind::Move, true) => {
                    replace_path_staged(job, source, &target, tx, control, &mut runtime)?;
                    remove_source(source)?;
                }
                (TransferKind::Copy, false) => {
                    copy_path(job, source, &target, tx, control, &mut runtime)?
                }
                (TransferKind::Move, false) => {
                    move_path(job, source, &target, tx, control, &mut runtime)?
                }
            }
            runtime.track_completed_root(source, &target);
        }
        Ok(TransferOutcome {
            completed_files: runtime.files_done,
            completed_roots: runtime.completed_roots.clone(),
        })
    })();

    if result.is_err() && control.cancel.load(Ordering::Relaxed) {
        cleanup_created_targets(&runtime.created_targets);
    }
    release_reserved_targets(&runtime.reserved_targets);

    result
}

fn run_portable_transfer(
    job: &TransferJob,
    tx: &Sender<TransferMessage>,
    control: &TransferControl,
) -> Result<TransferOutcome> {
    if explorer::is_virtual_path(&job.destination) && !explorer::is_portable_path(&job.destination)
    {
        return Err(BExplorerError::Operation(
            "Transfers with this virtual location are not available".into(),
        ));
    }
    if job
        .sources
        .iter()
        .any(|source| explorer::is_virtual_path(source) && !explorer::is_portable_path(source))
    {
        return Err(BExplorerError::Operation(
            "Transfers with this virtual location are not available".into(),
        ));
    }

    let total_bytes = job
        .sources
        .iter()
        .map(|path| {
            if explorer::is_portable_path(path) {
                portable::path_total_bytes(path)
            } else {
                path_total_bytes(path)
            }
        })
        .sum();
    let total_files = job
        .sources
        .iter()
        .map(|path| {
            if explorer::is_portable_path(path) {
                portable::path_file_count(path)
            } else {
                path_file_count(path)
            }
        })
        .sum();
    let mut runtime = TransferRuntime::new(total_bytes, total_files);
    emit_progress(job, "", TransferState::Copying, &runtime, tx);

    let result = if explorer::is_portable_path(&job.destination) {
        run_local_to_portable_transfer(job, tx, control, &mut runtime)
    } else {
        run_portable_to_local_transfer(job, tx, control, &mut runtime)
    };

    release_reserved_targets(&runtime.reserved_targets);
    if result.is_err() && control.cancel.load(Ordering::Relaxed) {
        cleanup_created_targets(&runtime.created_targets);
    }
    result.map(|completed_files| TransferOutcome {
        completed_files,
        // Portable locations do not have a native local path that can be
        // safely removed or restored by undo yet.
        completed_roots: Vec::new(),
    })
}

fn run_local_to_portable_transfer(
    job: &TransferJob,
    tx: &Sender<TransferMessage>,
    control: &TransferControl,
    runtime: &mut TransferRuntime,
) -> Result<usize> {
    if job
        .sources
        .iter()
        .any(|source| explorer::is_portable_path(source))
    {
        return Err(BExplorerError::Operation(
            "Copying directly between portable device folders is not available yet".into(),
        ));
    }

    for source in &job.sources {
        check_cancelled(control)?;
        wait_if_paused(job, &display_name(source), runtime, tx, control)?;
        if !source.exists() {
            continue;
        }

        let mut event = |event: portable::PortableTransferEvent<'_>| {
            handle_portable_event(job, event, runtime, tx, control)
        };
        portable::import_from_local(source, &job.destination, &mut event)?;
        if job.kind == TransferKind::Move {
            remove_source(source)?;
        }
    }
    Ok(runtime.files_done)
}

fn run_portable_to_local_transfer(
    job: &TransferJob,
    tx: &Sender<TransferMessage>,
    control: &TransferControl,
    runtime: &mut TransferRuntime,
) -> Result<usize> {
    if !job.destination.is_dir() {
        return Err(BExplorerError::InvalidPath(job.destination.clone()));
    }

    for source in &job.sources {
        check_cancelled(control)?;
        wait_if_paused(job, &display_name(source), runtime, tx, control)?;

        if explorer::is_portable_path(source) {
            let name = portable::path_name(source);
            let Some(target) = reserve_destination(
                &job.destination.join(name),
                portable::path_is_folder(source),
                job.conflict_policy,
            ) else {
                mark_portable_source_skipped(job, source, runtime, tx);
                continue;
            };
            runtime.track_reserved(&target);
            let replacing = job.conflict_policy == ConflictPolicy::Replace && target.exists();
            if replacing {
                let staging = unused_transfer_sibling(&target, "staging");
                let mut event = |event: portable::PortableTransferEvent<'_>| {
                    handle_portable_event(job, event, runtime, tx, control)
                };
                let export_result = portable::export_to_local(source, &staging, &mut event);
                if let Err(error) = export_result {
                    let _ = remove_source(&staging);
                    return Err(error);
                }
                sync_copied_path(&staging)?;
                if let Err(error) = commit_staged_path(&staging, &target) {
                    let _ = remove_source(&staging);
                    return Err(error);
                }
            } else {
                runtime.track_created(&target);
                let mut event = |event: portable::PortableTransferEvent<'_>| {
                    handle_portable_event(job, event, runtime, tx, control)
                };
                portable::export_to_local(source, &target, &mut event)?;
                sync_copied_path(&target)?;
            }
            continue;
        }

        if !source.exists() {
            continue;
        }
        let Some(name) = source.file_name() else {
            continue;
        };
        let Some(target) = reserve_destination(
            &job.destination.join(name),
            source.is_dir(),
            job.conflict_policy,
        ) else {
            mark_source_skipped(job, source, runtime, tx);
            continue;
        };
        // Replacing an item means replacing the complete top-level entry, not
        // merging two directories and leaving stale files behind. A source
        // pasted into its own directory is the lone exception: replacing it
        // would destroy the input before it can be read, so treat it as a
        // completed no-op instead.
        if source == &target {
            mark_source_skipped(job, source, runtime, tx);
            continue;
        }
        runtime.track_reserved(&target);
        let replacing = job.conflict_policy == ConflictPolicy::Replace && target.exists();
        match (job.kind, replacing) {
            (TransferKind::Copy, true) => {
                replace_path_staged(job, source, &target, tx, control, runtime)?
            }
            (TransferKind::Move, true) => {
                replace_path_staged(job, source, &target, tx, control, runtime)?;
                remove_source(source)?;
            }
            (TransferKind::Copy, false) => copy_path(job, source, &target, tx, control, runtime)?,
            (TransferKind::Move, false) => move_path(job, source, &target, tx, control, runtime)?,
        }
        runtime.track_completed_root(source, &target);
    }

    Ok(runtime.files_done)
}

fn handle_portable_event(
    job: &TransferJob,
    event: portable::PortableTransferEvent<'_>,
    runtime: &mut TransferRuntime,
    tx: &Sender<TransferMessage>,
    control: &TransferControl,
) -> Result<()> {
    match event {
        portable::PortableTransferEvent::BeforeItem(name) => {
            check_cancelled(control)?;
            wait_if_paused(job, name, runtime, tx, control)?;
            emit_progress(job, name, TransferState::Copying, runtime, tx);
        }
        portable::PortableTransferEvent::Bytes(name, bytes) => {
            check_cancelled(control)?;
            wait_if_paused(job, name, runtime, tx, control)?;
            runtime.copied_bytes = runtime.copied_bytes.saturating_add(bytes);
            if runtime.last_emit.elapsed() >= PROGRESS_INTERVAL {
                emit_progress(job, name, TransferState::Copying, runtime, tx);
                runtime.last_emit = Instant::now();
            }
        }
        portable::PortableTransferEvent::FileDone(name) => {
            runtime.files_done = runtime.files_done.saturating_add(1);
            emit_progress(job, name, TransferState::Copying, runtime, tx);
        }
    }
    Ok(())
}

fn move_path(
    job: &TransferJob,
    source: &Path,
    target: &Path,
    tx: &Sender<TransferMessage>,
    control: &TransferControl,
    runtime: &mut TransferRuntime,
) -> Result<()> {
    check_cancelled(control)?;
    wait_if_paused(job, current_name(source), runtime, tx, control)?;

    if !(job.conflict_policy == ConflictPolicy::Replace && target.exists())
        && fs::rename(source, target).is_ok()
    {
        runtime.copied_bytes = runtime
            .copied_bytes
            .saturating_add(path_total_bytes(target));
        runtime.files_done = runtime.files_done.saturating_add(path_file_count(target));
        emit_progress(
            job,
            current_name(target),
            TransferState::Copying,
            runtime,
            tx,
        );
        return Ok(());
    }

    copy_path(job, source, target, tx, control, runtime)?;
    remove_source(source)?;
    Ok(())
}

/// Copies an entire top-level item beside its final destination and commits it
/// only after every file has been flushed. The existing destination remains
/// untouched if copying, pausing, cancellation, or syncing fails.
fn replace_path_staged(
    job: &TransferJob,
    source: &Path,
    target: &Path,
    tx: &Sender<TransferMessage>,
    control: &TransferControl,
    runtime: &mut TransferRuntime,
) -> Result<()> {
    let staging = unused_transfer_sibling(target, "staging");
    let result = (|| {
        copy_path(job, source, &staging, tx, control, runtime)?;
        sync_copied_path(&staging)?;
        commit_staged_path(&staging, target)
    })();
    if result.is_err() {
        let _ = remove_source(&staging);
    }
    result
}

fn commit_staged_path(staging: &Path, target: &Path) -> Result<()> {
    let staging_metadata = fs::symlink_metadata(staging)?;
    let target_metadata = fs::symlink_metadata(target)?;
    if staging_metadata.is_file() && target_metadata.is_file() {
        crate::utils::atomic_file::replace_file(staging, target)?;
        crate::utils::atomic_file::sync_parent(target);
        return Ok(());
    }

    let backup = unused_transfer_sibling(target, "backup");
    fs::rename(target, &backup)?;
    if let Err(commit_error) = fs::rename(staging, target) {
        if let Err(rollback_error) = fs::rename(&backup, target) {
            return Err(BExplorerError::Operation(format!(
                "Could not install the completed replacement ({commit_error}); the original remains at {} because restoring it also failed: {rollback_error}",
                backup.display()
            )));
        }
        return Err(commit_error.into());
    }
    crate::utils::atomic_file::sync_parent(target);
    if let Err(error) = remove_source(&backup) {
        crate::utils::log::error(format!(
            "Replacement completed but its backup could not be removed at {}: {error}",
            backup.display()
        ));
    }
    Ok(())
}

fn unused_transfer_sibling(target: &Path, purpose: &str) -> PathBuf {
    let parent = target.parent().unwrap_or_else(|| Path::new(""));
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("item");
    loop {
        let sequence = TRANSFER_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".{name}.bexplorer-{purpose}-{}-{sequence}",
            std::process::id()
        ));
        if !candidate.exists() {
            return candidate;
        }
    }
}

fn sync_copied_path(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_file() {
        fs::File::open(path)?.sync_all()?;
    } else if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            sync_copied_path(&entry?.path())?;
        }
        #[cfg(unix)]
        fs::File::open(path)?.sync_all()?;
    }
    Ok(())
}

fn copy_path(
    job: &TransferJob,
    source: &Path,
    target: &Path,
    tx: &Sender<TransferMessage>,
    control: &TransferControl,
    runtime: &mut TransferRuntime,
) -> Result<()> {
    check_cancelled(control)?;
    wait_if_paused(job, current_name(source), runtime, tx, control)?;

    if source.is_dir() {
        let existed = target.exists();
        fs::create_dir_all(target)?;
        if !existed {
            runtime.track_created(target);
        }
        for item in fs::read_dir(source)? {
            let item = item?;
            copy_path(
                job,
                &item.path(),
                &target.join(item.file_name()),
                tx,
                control,
                runtime,
            )?;
        }
        return Ok(());
    }

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut input = fs::File::open(source)?;
    let existed = target.exists();
    let mut output = fs::File::create(target)?;
    if !existed {
        runtime.track_created(target);
    }
    let mut buffer = vec![0_u8; COPY_BUFFER_SIZE];
    let current_name = current_name(source).to_string();

    loop {
        check_cancelled(control)?;
        wait_if_paused(job, &current_name, runtime, tx, control)?;
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        output.write_all(&buffer[..read])?;
        runtime.copied_bytes = runtime.copied_bytes.saturating_add(read as u64);
        if runtime.last_emit.elapsed() >= PROGRESS_INTERVAL {
            emit_progress(job, &current_name, TransferState::Copying, runtime, tx);
            runtime.last_emit = Instant::now();
        }
    }
    output.sync_all()?;
    runtime.files_done = runtime.files_done.saturating_add(1);
    emit_progress(job, &current_name, TransferState::Copying, runtime, tx);
    Ok(())
}

fn mark_source_skipped(
    job: &TransferJob,
    source: &Path,
    runtime: &mut TransferRuntime,
    tx: &Sender<TransferMessage>,
) {
    runtime.copied_bytes = runtime
        .copied_bytes
        .saturating_add(path_total_bytes(source));
    runtime.files_done = runtime.files_done.saturating_add(path_file_count(source));
    emit_progress(
        job,
        current_name(source),
        TransferState::Copying,
        runtime,
        tx,
    );
}

fn mark_portable_source_skipped(
    job: &TransferJob,
    source: &Path,
    runtime: &mut TransferRuntime,
    tx: &Sender<TransferMessage>,
) {
    runtime.copied_bytes = runtime
        .copied_bytes
        .saturating_add(portable::path_total_bytes(source));
    runtime.files_done = runtime
        .files_done
        .saturating_add(portable::path_file_count(source));
    emit_progress(
        job,
        &portable::path_name(source),
        TransferState::Copying,
        runtime,
        tx,
    );
}

fn wait_if_paused(
    job: &TransferJob,
    current_name: &str,
    runtime: &TransferRuntime,
    tx: &Sender<TransferMessage>,
    control: &TransferControl,
) -> Result<()> {
    if !control.pause.load(Ordering::Relaxed) {
        return Ok(());
    }

    emit_progress(job, current_name, TransferState::Paused, runtime, tx);
    while control.pause.load(Ordering::Relaxed) {
        check_cancelled(control)?;
        std::thread::sleep(Duration::from_millis(80));
    }
    emit_progress(job, current_name, TransferState::Copying, runtime, tx);
    Ok(())
}

fn emit_progress(
    job: &TransferJob,
    current_name: &str,
    state: TransferState,
    runtime: &TransferRuntime,
    tx: &Sender<TransferMessage>,
) {
    let elapsed = runtime.started.elapsed().as_secs_f64().max(0.001);
    let _ = tx.send(TransferMessage::Progress(TransferProgress {
        job_id: job.id,
        kind: job.kind,
        state,
        current_name: current_name.to_string(),
        destination: job.destination.clone(),
        copied_bytes: runtime.copied_bytes,
        total_bytes: runtime.total_bytes,
        files_done: runtime.files_done,
        total_files: runtime.total_files,
        bytes_per_second: runtime.copied_bytes as f64 / elapsed,
    }));
}

fn check_cancelled(control: &TransferControl) -> Result<()> {
    if control.cancel.load(Ordering::Relaxed) {
        Err(BExplorerError::Operation("Transfer cancelled".into()))
    } else {
        Ok(())
    }
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

fn path_file_count(path: &Path) -> usize {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return 0;
    };
    if metadata.is_file() {
        return 1;
    }
    if !metadata.is_dir() {
        return 0;
    }

    fs::read_dir(path)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|item| item.ok())
        .map(|item| path_file_count(&item.path()))
        .sum()
}

fn remove_source(path: &Path) -> Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path)?;
    } else if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn cleanup_created_targets(paths: &[PathBuf]) {
    for path in paths.iter().rev() {
        let result = if path.is_dir() {
            fs::remove_dir_all(path)
        } else if path.exists() {
            fs::remove_file(path)
        } else {
            Ok(())
        };
        if let Err(error) = result {
            crate::utils::log::error(format!(
                "Could not clean cancelled transfer target: {error}"
            ));
        }
    }
}

fn reserved_targets() -> &'static Mutex<HashSet<PathBuf>> {
    RESERVED_TARGETS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn reserve_destination(base: &Path, is_dir: bool, policy: ConflictPolicy) -> Option<PathBuf> {
    let mut reserved = reserved_targets()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target = resolve_conflict_with_reservations(base, is_dir, policy, &reserved)?;
    reserved.insert(target.clone());
    Some(target)
}

fn release_reserved_targets(paths: &[PathBuf]) {
    let mut reserved = reserved_targets()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for path in paths {
        reserved.remove(path);
    }
}

fn resolve_conflict_with_reservations(
    base: &Path,
    is_dir: bool,
    policy: ConflictPolicy,
    reserved: &HashSet<PathBuf>,
) -> Option<PathBuf> {
    match policy {
        ConflictPolicy::Replace => {
            if reserved.contains(base) {
                Some(unique_destination(base, is_dir, reserved))
            } else {
                Some(base.to_path_buf())
            }
        }
        ConflictPolicy::Skip => {
            if base.exists() || reserved.contains(base) {
                None
            } else {
                Some(base.to_path_buf())
            }
        }
        ConflictPolicy::KeepBoth => Some(unique_destination(base, is_dir, reserved)),
    }
}

fn unique_destination(base: &Path, is_dir: bool, reserved: &HashSet<PathBuf>) -> PathBuf {
    if !base.exists() && !reserved.contains(base) {
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
        if !candidate.exists() && !reserved.contains(&candidate) {
            return candidate;
        }
    }

    base.to_path_buf()
}

fn current_name(path: &Path) -> &str {
    path.file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("")
}

fn display_name(path: &Path) -> String {
    if explorer::is_portable_path(path) {
        return portable::path_name(path);
    }
    path.file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_transfer_dir(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "bexplorer-transfer-{name}-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("create temp transfer dir");
        dir
    }

    fn run_test_transfer(
        source: PathBuf,
        destination: PathBuf,
        conflict_policy: ConflictPolicy,
    ) -> usize {
        let (tx, _rx) = mpsc::channel();
        let job = TransferJob {
            id: 1,
            sources: vec![source],
            destination,
            kind: TransferKind::Copy,
            conflict_policy,
        };
        run_transfer_inner(&job, &tx, &TransferControl::new())
            .expect("run transfer")
            .completed_files
    }

    fn assert_no_transfer_artifacts(directory: &Path) {
        let artifacts = fs::read_dir(directory)
            .expect("read transfer directory")
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().contains(".bexplorer-"))
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        assert!(artifacts.is_empty(), "temporary artifacts: {artifacts:?}");
    }

    #[test]
    fn keep_both_creates_numbered_copy_on_conflict() {
        let root = temp_transfer_dir("keep-both");
        let source_dir = root.join("source");
        let destination = root.join("destination");
        fs::create_dir_all(&source_dir).expect("create source");
        fs::create_dir_all(&destination).expect("create destination");
        fs::write(source_dir.join("report.txt"), b"new").expect("write source");
        fs::write(destination.join("report.txt"), b"old").expect("write destination");

        let completed = run_test_transfer(
            source_dir.join("report.txt"),
            destination.clone(),
            ConflictPolicy::KeepBoth,
        );

        assert_eq!(completed, 1);
        assert_eq!(
            fs::read(destination.join("report.txt")).expect("read original"),
            b"old"
        );
        assert_eq!(
            fs::read(destination.join("report (2).txt")).expect("read numbered copy"),
            b"new"
        );
        fs::remove_dir_all(root).expect("cleanup temp transfer dir");
    }

    #[test]
    fn records_the_exact_keep_both_target_for_undo() {
        let root = temp_transfer_dir("undo-target");
        let source_dir = root.join("source");
        let destination = root.join("destination");
        fs::create_dir_all(&source_dir).expect("create source");
        fs::create_dir_all(&destination).expect("create destination");
        let source = source_dir.join("report.txt");
        fs::write(&source, b"new").expect("write source");
        fs::write(destination.join("report.txt"), b"old").expect("write destination");
        let job = TransferJob {
            id: 1,
            sources: vec![source.clone()],
            destination: destination.clone(),
            kind: TransferKind::Copy,
            conflict_policy: ConflictPolicy::KeepBoth,
        };
        let (tx, _rx) = mpsc::channel();

        let outcome = run_transfer_inner(&job, &tx, &TransferControl::new()).expect("run transfer");

        assert_eq!(outcome.completed_roots.len(), 1);
        assert_eq!(outcome.completed_roots[0].source, source);
        assert_eq!(
            outcome.completed_roots[0].target,
            destination.join("report (2).txt")
        );
        fs::remove_dir_all(root).expect("cleanup temp transfer dir");
    }

    #[test]
    fn replace_overwrites_existing_file_on_conflict() {
        let root = temp_transfer_dir("replace");
        let source_dir = root.join("source");
        let destination = root.join("destination");
        fs::create_dir_all(&source_dir).expect("create source");
        fs::create_dir_all(&destination).expect("create destination");
        fs::write(source_dir.join("report.txt"), b"new").expect("write source");
        fs::write(destination.join("report.txt"), b"old").expect("write destination");

        let completed = run_test_transfer(
            source_dir.join("report.txt"),
            destination.clone(),
            ConflictPolicy::Replace,
        );

        assert_eq!(completed, 1);
        assert_eq!(
            fs::read(destination.join("report.txt")).expect("read replaced"),
            b"new"
        );
        assert!(!destination.join("report (2).txt").exists());
        assert_no_transfer_artifacts(&destination);
        fs::remove_dir_all(root).expect("cleanup temp transfer dir");
    }

    #[test]
    fn replace_replaces_a_conflicting_directory_instead_of_merging_it() {
        let root = temp_transfer_dir("replace-directory");
        let source_dir = root.join("source");
        let destination = root.join("destination");
        let source = source_dir.join("project");
        let existing = destination.join("project");
        fs::create_dir_all(&source).expect("create source project");
        fs::create_dir_all(&existing).expect("create destination project");
        fs::write(source.join("current.txt"), b"new").expect("write source file");
        fs::write(existing.join("stale.txt"), b"old").expect("write stale file");

        let completed = run_test_transfer(source, destination.clone(), ConflictPolicy::Replace);

        assert_eq!(completed, 1);
        assert_eq!(
            fs::read(existing.join("current.txt")).expect("read copied file"),
            b"new"
        );
        assert!(!existing.join("stale.txt").exists());
        assert_no_transfer_artifacts(&destination);
        fs::remove_dir_all(root).expect("cleanup temp transfer dir");
    }

    #[cfg(unix)]
    #[test]
    fn failed_staged_directory_copy_preserves_the_existing_destination() {
        use std::os::unix::net::UnixListener;

        let root = temp_transfer_dir("replace-failure");
        let source_dir = root.join("source");
        let destination = root.join("destination");
        let source = source_dir.join("project");
        let existing = destination.join("project");
        fs::create_dir_all(&source).expect("create source project");
        fs::create_dir_all(&existing).expect("create destination project");
        fs::write(existing.join("important.txt"), b"keep me").expect("write existing file");
        let _socket = UnixListener::bind(source.join("not-a-regular-file"))
            .expect("create unsupported source socket");

        let (tx, _rx) = mpsc::channel();
        let job = TransferJob {
            id: 1,
            sources: vec![source],
            destination: destination.clone(),
            kind: TransferKind::Copy,
            conflict_policy: ConflictPolicy::Replace,
        };
        assert!(run_transfer_inner(&job, &tx, &TransferControl::new()).is_err());

        assert_eq!(
            fs::read(existing.join("important.txt")).expect("read preserved destination"),
            b"keep me"
        );
        assert_no_transfer_artifacts(&destination);
        fs::remove_dir_all(root).expect("cleanup temp transfer dir");
    }

    #[test]
    fn skip_preserves_existing_file_on_conflict() {
        let root = temp_transfer_dir("skip");
        let source_dir = root.join("source");
        let destination = root.join("destination");
        fs::create_dir_all(&source_dir).expect("create source");
        fs::create_dir_all(&destination).expect("create destination");
        fs::write(source_dir.join("report.txt"), b"new").expect("write source");
        fs::write(destination.join("report.txt"), b"old").expect("write destination");

        let completed = run_test_transfer(
            source_dir.join("report.txt"),
            destination.clone(),
            ConflictPolicy::Skip,
        );

        assert_eq!(completed, 1);
        assert_eq!(
            fs::read(destination.join("report.txt")).expect("read preserved"),
            b"old"
        );
        assert!(!destination.join("report (2).txt").exists());
        fs::remove_dir_all(root).expect("cleanup temp transfer dir");
    }
}
