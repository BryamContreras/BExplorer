use super::*;
use std::sync::mpsc;

fn fixture() -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "bexplorer-transfer-integrity-{}-{stamp}",
        std::process::id()
    ));
    fs::create_dir(&root).unwrap();
    root
}

fn job(source: PathBuf, destination: PathBuf, kind: TransferKind) -> TransferJob {
    TransferJob {
        id: 1,
        sources: vec![source],
        destination,
        kind,
        conflict_policy: ConflictPolicy::Replace,
    }
}

fn run(job: &TransferJob) -> Result<TransferOutcome> {
    let (tx, _rx) = mpsc::channel();
    run_transfer_inner(job, &tx, &TransferControl::new())
}

#[test]
fn move_replace_with_hardlinked_destination_preserves_both_names() {
    let root = fixture();
    let input = root.join("input");
    let output = root.join("output");
    fs::create_dir(&input).unwrap();
    fs::create_dir(&output).unwrap();
    let source = input.join("data.bin");
    let target = output.join("data.bin");
    fs::write(&source, vec![42; 1024]).unwrap();
    fs::hard_link(&source, &target).unwrap();
    run(&job(source.clone(), output, TransferKind::Move)).unwrap();
    assert_eq!(fs::read(&source).unwrap(), vec![42; 1024]);
    assert_eq!(fs::read(&target).unwrap(), vec![42; 1024]);
    assert_eq!(
        path_identity(&source).unwrap(),
        path_identity(&target).unwrap()
    );
    fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[test]
fn move_replace_with_case_or_extended_prefix_alias_preserves_source() {
    let root = fixture();
    let source = root.join("Data.bin");
    fs::write(&source, vec![42; 1024]).unwrap();
    let canonical_source = fs::canonicalize(&source).unwrap();
    let canonical_parent = fs::canonicalize(&root).unwrap();
    for (source_alias, destination_alias) in [
        (canonical_source, root.clone()),
        (source.clone(), canonical_parent),
        (
            PathBuf::from(source.to_string_lossy().to_uppercase()),
            root.clone(),
        ),
    ] {
        run(&job(source_alias, destination_alias, TransferKind::Move)).unwrap();
        assert_eq!(fs::read(&source).unwrap(), vec![42; 1024]);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn descendant_destination_is_rejected_before_creating_outputs() {
    let root = fixture();
    let source = root.join("project");
    let destination = source.join("nested");
    fs::create_dir_all(&destination).unwrap();
    fs::write(source.join("data.bin"), b"keep").unwrap();
    for kind in [TransferKind::Copy, TransferKind::Move] {
        assert!(run(&job(source.clone(), destination.clone(), kind)).is_err());
        assert!(fs::read_dir(&destination).unwrap().next().is_none());
        assert_eq!(fs::read(source.join("data.bin")).unwrap(), b"keep");
    }
    fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
fn junction(link: &Path, target: &Path) {
    let status = std::process::Command::new("cmd.exe")
        .args(["/c", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "junction creation failed: {}",
        String::from_utf8_lossy(&status.stderr)
    );
}

#[cfg(windows)]
#[test]
fn junction_sources_and_cycles_are_rejected_before_mutation() {
    let root = fixture();
    let source = root.join("project");
    let destination = root.join("output");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&destination).unwrap();
    fs::write(source.join("data.bin"), b"keep").unwrap();
    let alias = root.join("alias");
    junction(&alias, &source);
    assert!(run(&job(alias.clone(), destination.clone(), TransferKind::Copy)).is_err());
    let cycle = source.join("loop");
    junction(&cycle, &source);
    assert!(
        run(&job(
            source.clone(),
            destination.clone(),
            TransferKind::Copy
        ))
        .is_err()
    );
    assert!(fs::read_dir(&destination).unwrap().next().is_none());
    assert_eq!(fs::read(source.join("data.bin")).unwrap(), b"keep");
    fs::remove_dir(&cycle).unwrap();
    fs::remove_dir(&alias).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[test]
fn junction_destination_cannot_hide_a_source_descendant() {
    let root = fixture();
    let source = root.join("project");
    let nested = source.join("nested");
    fs::create_dir_all(&nested).unwrap();
    fs::write(source.join("data.bin"), b"keep").unwrap();
    let alias = root.join("alias");
    junction(&alias, &nested);
    assert!(run(&job(source.clone(), alias.clone(), TransferKind::Copy)).is_err());
    assert!(fs::read_dir(&nested).unwrap().next().is_none());
    fs::remove_dir(&alias).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn preparation_honors_cancellation_without_output() {
    let root = fixture();
    let source = root.join("data.bin");
    let destination = root.join("output");
    fs::create_dir(&destination).unwrap();
    fs::write(&source, b"keep").unwrap();
    let control = TransferControl::new();
    control.cancel.store(true, Ordering::Relaxed);
    let (tx, _rx) = mpsc::channel();
    assert!(
        run_transfer_inner(
            &job(source.clone(), destination.clone(), TransferKind::Copy),
            &tx,
            &control
        )
        .is_err()
    );
    assert!(fs::read_dir(&destination).unwrap().next().is_none());
    assert_eq!(fs::read(source).unwrap(), b"keep");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cancellation_after_move_commit_never_removes_the_destination() {
    let root = fixture();
    let source = root.join("data.bin");
    let target = root.join("copied.bin");
    fs::write(&source, b"keep").unwrap();
    let control = TransferControl::new();
    let (tx, _rx) = mpsc::channel();
    let mut runtime = TransferRuntime::new(4, 1);
    let copy = job(source.clone(), root.clone(), TransferKind::Move);
    copy_path(&copy, &source, &target, &tx, &control, &mut runtime).unwrap();
    sync_copied_path(&target).unwrap();
    runtime.protect_destination(&target);
    control.cancel.store(true, Ordering::Relaxed);
    assert!(local::remove_copied_source(&source, &runtime.copied_sources, &control).is_err());
    cleanup_created_targets(&runtime.created_targets);
    assert_eq!(fs::read(source).unwrap(), b"keep");
    assert_eq!(fs::read(target).unwrap(), b"keep");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn move_cleanup_preserves_source_files_added_after_copy() {
    let root = fixture();
    let source = root.join("project");
    let target = root.join("copied");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("before.bin"), b"copied").unwrap();
    let control = TransferControl::new();
    let (tx, _rx) = mpsc::channel();
    let mut runtime = TransferRuntime::new(6, 1);
    let copy = job(source.clone(), root.clone(), TransferKind::Move);
    copy_path(&copy, &source, &target, &tx, &control, &mut runtime).unwrap();
    fs::write(source.join("new.bin"), b"added later").unwrap();
    assert!(local::remove_copied_source(&source, &runtime.copied_sources, &control).is_err());
    assert_eq!(fs::read(source.join("new.bin")).unwrap(), b"added later");
    assert_eq!(fs::read(target.join("before.bin")).unwrap(), b"copied");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn pending_external_provider_preserves_staging_even_after_cancel() {
    let root = fixture();
    let staging = root.join("staging");
    fs::create_dir(&staging).unwrap();
    let path = staging.join("data.bin");
    let mut writer = fs::File::create(&path).unwrap();
    writer.write_all(b"first").unwrap();
    let control = TransferControl::new();
    control.cancel.store(true, Ordering::Relaxed);
    let error = BExplorerError::OperationPending("external provider still owns the handle".into());
    cleanup_failed_staging(&staging, &error);
    cleanup_failed_transfer(&[staging.clone(), path.clone()], &error, &control);
    writer.write_all(b" later").unwrap();
    writer.flush().unwrap();
    drop(writer);
    assert_eq!(fs::read(&path).unwrap(), b"first later");
    // Only clean this fixture after the simulated provider closes its handle.
    fs::remove_file(path).unwrap();
    fs::remove_dir(staging).unwrap();
    fs::remove_dir(root).unwrap();
}

#[test]
fn cancellation_before_portable_target_creation_preserves_another_writers_file() {
    let root = fixture();
    let target = root.join("download.bin");
    fs::write(&target, b"other writer").unwrap();
    let control = TransferControl::new();
    control.cancel.store(true, Ordering::Relaxed);
    let (tx, _rx) = mpsc::channel();
    let mut runtime = TransferRuntime::new(4, 1);
    let transfer = job(
        PathBuf::from("portable-source"),
        root.clone(),
        TransferKind::Copy,
    );
    let error = handle_portable_event(
        &transfer,
        portable::PortableTransferEvent::BeforeItem("download.bin"),
        &mut runtime,
        &tx,
        &control,
    )
    .unwrap_err();
    cleanup_failed_transfer(&runtime.created_targets, &error, &control);
    assert!(runtime.created_targets.is_empty());
    assert_eq!(fs::read(&target).unwrap(), b"other writer");
    fs::remove_file(target).unwrap();
    fs::remove_dir(root).unwrap();
}

#[cfg(unix)]
#[test]
fn directory_symlink_cycles_are_copied_without_following_them() {
    let root = fixture();
    let source = root.join("project");
    let output = root.join("output");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&output).unwrap();
    fs::write(source.join("data.bin"), b"keep").unwrap();
    std::os::unix::fs::symlink(".", source.join("loop")).unwrap();
    run(&job(source.clone(), output.clone(), TransferKind::Copy)).unwrap();
    sync_copied_path(&output.join("project")).unwrap();
    assert!(source.exists());
    assert_eq!(
        fs::read_link(output.join("project/loop")).unwrap(),
        PathBuf::from(".")
    );
    assert_eq!(fs::read(output.join("project/data.bin")).unwrap(), b"keep");
    fs::remove_file(output.join("project/loop")).unwrap();
    fs::remove_file(source.join("loop")).unwrap();
    fs::remove_dir_all(root).unwrap();
}
