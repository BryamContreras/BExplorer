use super::*;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{SystemTime, UNIX_EPOCH};

static TRACE_ACTIVE: AtomicBool = AtomicBool::new(false);
static COPY_IO: Mutex<(&str, u64, Option<Instant>)> = Mutex::new(("preparing", 0, None));

pub(super) fn trace_io(phase: &'static str, offset: u64) {
    if TRACE_ACTIVE.load(Ordering::Relaxed) {
        *COPY_IO.lock().unwrap_or_else(|error| error.into_inner()) =
            (phase, offset, Some(Instant::now()));
    }
}

#[test]
#[ignore = "requires BEXPLORER_LARGE_COPY_SOURCE and BEXPLORER_LARGE_COPY_DESTINATION; copies and verifies the full file in an isolated folder"]
fn large_file_copy_to_external_drive() -> std::result::Result<(), String> {
    let source = fs::canonicalize(
        std::env::var_os("BEXPLORER_LARGE_COPY_SOURCE").ok_or("missing source path")?,
    )
    .map_err(|error| error.to_string())?;
    let parent = fs::canonicalize(
        std::env::var_os("BEXPLORER_LARGE_COPY_DESTINATION").ok_or("missing destination drive")?,
    )
    .map_err(|error| error.to_string())?;
    let before = fs::metadata(&source).map_err(|error| error.to_string())?;
    if !before.is_file() || before.len() <= 12 * 1024 * 1024 * 1024 {
        return Err("the fixture must be a regular file larger than 12 GiB".into());
    }
    if fs2::available_space(&parent).map_err(|error| error.to_string())?
        < before.len().saturating_add(1024 * 1024 * 1024)
    {
        return Err("not enough free space for the isolated copy".into());
    }
    let manifest =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("target/large-copy-test-fixture.json");
    if manifest.exists() {
        return Err(
            "a previous test fixture is recorded; inspect it before starting another copy".into(),
        );
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let name = format!("BExplorer-transfer-test-{}-{nonce}", std::process::id());
    let directory = parent.join(&name);
    fs::create_dir(&directory).map_err(|error| error.to_string())?;
    let target = directory.join(source.file_name().ok_or("source has no filename")?);
    fs::write(
        &manifest,
        serde_json::to_vec(&serde_json::json!({
            "source": source,
            "directory": directory,
            "target": target,
            "source_bytes": before.len(),
        }))
        .unwrap(),
    )
    .map_err(|error| error.to_string())?;
    eprintln!(
        "COPY source={} bytes={} destination={}",
        source.display(),
        before.len(),
        target.display()
    );
    let job = TransferJob {
        id: 1,
        sources: vec![source.clone()],
        destination: directory.clone(),
        kind: TransferKind::Copy,
        conflict_policy: ConflictPolicy::Skip,
    };
    let control = TransferControl::new();
    let worker_control = control.clone();
    let (sender, receiver) = mpsc::channel();
    TRACE_ACTIVE.store(true, Ordering::Relaxed);
    let started = Instant::now();
    let worker = std::thread::spawn(move || run_transfer(job, sender, worker_control));
    let mut last_report = Instant::now() - Duration::from_secs(5);
    let mut last_advance = Instant::now();
    let mut copied = 0;
    let mut passed_twelve = false;
    let outcome = loop {
        match receiver.recv_timeout(Duration::from_secs(1)) {
            Ok(TransferMessage::Progress(progress)) => {
                if progress.copied_bytes != copied {
                    copied = progress.copied_bytes;
                    last_advance = Instant::now();
                }
                if !passed_twelve && copied >= 12 * 1024 * 1024 * 1024 {
                    passed_twelve = true;
                    eprintln!("PASSED 12 GiB after {:?}", started.elapsed());
                }
                if last_report.elapsed() >= Duration::from_secs(5) {
                    eprintln!(
                        "PROGRESS bytes={copied}/{} elapsed={:?} average={:.1} MiB/s",
                        before.len(),
                        started.elapsed(),
                        progress.bytes_per_second / (1024.0 * 1024.0)
                    );
                    last_report = Instant::now();
                }
            }
            Ok(TransferMessage::Finished {
                completed_files, ..
            }) => {
                break if completed_files == 1 {
                    Ok(())
                } else {
                    Err("unexpected completed-file count".into())
                };
            }
            Ok(TransferMessage::Failed { error, .. }) => break Err(error),
            Ok(TransferMessage::Cancelled { .. }) => break Err("copy was cancelled".into()),
            Err(RecvTimeoutError::Disconnected) => break Err("copy worker disconnected".into()),
            Err(RecvTimeoutError::Timeout) => {
                if last_report.elapsed() >= Duration::from_secs(5) {
                    let (phase, offset, entered) =
                        *COPY_IO.lock().unwrap_or_else(|error| error.into_inner());
                    eprintln!(
                        "WAIT phase={phase} offset={offset} call_elapsed={:?} no_progress={:?}",
                        entered.map(|time| time.elapsed()),
                        last_advance.elapsed()
                    );
                    last_report = Instant::now();
                }
            }
        }
    };
    // A quiet progress channel is not proof that Windows stopped writing.
    // Always wait for the worker to close its handles before verification or cleanup.
    let joined = worker.join();
    TRACE_ACTIVE.store(false, Ordering::Relaxed);
    let result = (|| -> std::result::Result<(), String> {
        joined.map_err(|_| "copy worker panicked".to_string())?;
        outcome?;
        eprintln!(
            "COPY FINISHED elapsed={:?}; synchronizing test file",
            started.elapsed()
        );
        fs::OpenOptions::new()
            .write(true)
            .open(&target)
            .and_then(|file| file.sync_all())
            .map_err(|error| error.to_string())?;
        let after = fs::metadata(&source).map_err(|error| error.to_string())?;
        if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
            return Err("source changed during the test".into());
        }
        if fs::metadata(&target)
            .map_err(|error| error.to_string())?
            .len()
            != before.len()
        {
            return Err("copied file size differs from source".into());
        }
        let source_hash = checksum(&source, "source")?;
        let destination_hash = checksum(&target, "destination")?;
        if source_hash != destination_hash {
            return Err("copied file checksum differs from source".into());
        }
        eprintln!(
            "VERIFIED bytes={} md5={source_hash:x} elapsed={:?}",
            before.len(),
            started.elapsed()
        );
        Ok(())
    })();
    // Validate the exact folder we created; remove only the test's single file.
    let resolved = fs::canonicalize(&directory).map_err(|error| error.to_string())?;
    if resolved.parent() != Some(parent.as_path())
        || resolved.file_name() != Some(std::ffi::OsStr::new(&name))
    {
        return Err("test directory moved; refusing cleanup".into());
    }
    if target.exists() {
        fs::remove_file(&target).map_err(|error| error.to_string())?;
    }
    fs::remove_dir(&resolved).map_err(|error| error.to_string())?;
    fs::remove_file(&manifest).map_err(|error| error.to_string())?;
    eprintln!("CLEANUP complete; original file preserved");
    result
}

fn checksum(path: &Path, label: &str) -> std::result::Result<md5::Digest, String> {
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut buffer = vec![0; 4 * 1024 * 1024];
    let mut digest = md5::Context::new();
    let mut bytes = 0_u64;
    let mut reported = Instant::now();
    loop {
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        digest.consume(&buffer[..count]);
        bytes += count as u64;
        if reported.elapsed() >= Duration::from_secs(5) {
            eprintln!("VERIFY {label} bytes={bytes}");
            reported = Instant::now();
        }
    }
    Ok(digest.compute())
}
