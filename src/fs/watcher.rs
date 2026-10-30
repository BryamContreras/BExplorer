use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

const CHANGE_QUIET_PERIOD: Duration = Duration::from_millis(700);
const MAX_CHANGE_BATCH_AGE: Duration = Duration::from_secs(2);
const MAX_CHANGE_PATHS: usize = 2048;
const WATCH_SIGNAL_CAPACITY: usize = 32;
const WATCH_OUTPUT_CAPACITY: usize = 2;
#[cfg(target_os = "windows")]
const WATCH_PAUSE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DirectoryChange {
    pub directories: Vec<PathBuf>,
    pub paths: Vec<PathBuf>,
}

#[derive(Debug)]
enum WatchSignal {
    Reconfigure,
    OutputReady,
    Event(notify::Result<Event>),
    #[cfg(target_os = "windows")]
    Synchronize(SyncSender<std::result::Result<(), String>>),
}

struct DirectoryWatcherRuntime {
    desired_directories: Arc<Mutex<HashSet<PathBuf>>>,
    desired_version: Arc<AtomicU64>,
    #[cfg(target_os = "windows")]
    suspended_roots: Arc<Mutex<HashMap<PathBuf, usize>>>,
    signal_sender: SyncSender<WatchSignal>,
    output_receiver: Mutex<Option<Receiver<DirectoryChange>>>,
}

static DIRECTORY_WATCHER: OnceLock<DirectoryWatcherRuntime> = OnceLock::new();

/// Reading an update releases one output slot. Wake the producer so a final
/// pending update is delivered even when no more filesystem events arrive.
pub struct DirectoryChangeReceiver {
    receiver: Receiver<DirectoryChange>,
    signal_sender: SyncSender<WatchSignal>,
}

impl DirectoryChangeReceiver {
    pub fn recv(&self) -> std::result::Result<DirectoryChange, mpsc::RecvError> {
        let change = self.receiver.recv()?;
        let _ = self.signal_sender.try_send(WatchSignal::OutputReady);
        Ok(change)
    }
}

pub fn set_visible_directories<I>(directories: I)
where
    I: IntoIterator<Item = PathBuf>,
{
    let runtime = directory_watcher();
    let next = directories
        .into_iter()
        .filter(|path| path.is_absolute())
        .collect::<HashSet<_>>();
    let Ok(mut desired) = runtime.desired_directories.lock() else {
        return;
    };
    if *desired == next {
        return;
    }
    *desired = next;
    drop(desired);
    runtime.desired_version.fetch_add(1, Ordering::Release);
    let _ = runtime.signal_sender.try_send(WatchSignal::Reconfigure);
}

pub fn directory_change_receiver() -> DirectoryChangeReceiver {
    let runtime = directory_watcher();
    let receiver = runtime
        .output_receiver
        .lock()
        .ok()
        .and_then(|mut receiver| receiver.take())
        .unwrap_or_else(|| {
            let (_sender, receiver) = mpsc::sync_channel(1);
            receiver
        });
    DirectoryChangeReceiver {
        receiver,
        signal_sender: runtime.signal_sender.clone(),
    }
}

/// Temporarily releases our directory handles below a volume before safe
/// removal. Visible tabs keep their paths; dropping the guard restores their
/// watches if the volume remains connected.
#[cfg(target_os = "windows")]
pub struct DirectoryWatchPause {
    root: PathBuf,
    suspended_roots: Arc<Mutex<HashMap<PathBuf, usize>>>,
    desired_version: Arc<AtomicU64>,
    signal_sender: SyncSender<WatchSignal>,
}

#[cfg(target_os = "windows")]
impl Drop for DirectoryWatchPause {
    fn drop(&mut self) {
        if let Ok(mut roots) = self.suspended_roots.lock() {
            if let Some(count) = roots.get_mut(&self.root) {
                *count -= 1;
                if *count == 0 {
                    roots.remove(&self.root);
                }
            }
            drop(roots);
            self.desired_version.fetch_add(1, Ordering::Release);
            let _ = self.signal_sender.try_send(WatchSignal::Reconfigure);
        }
    }
}

#[cfg(target_os = "windows")]
pub fn pause_directory_watches(root: &Path) -> std::io::Result<DirectoryWatchPause> {
    let runtime = directory_watcher();
    let root = root.to_path_buf();
    let mut roots = runtime
        .suspended_roots
        .lock()
        .map_err(|_| std::io::Error::other("Directory monitoring state is unavailable"))?;
    *roots.entry(root.clone()).or_default() += 1;
    drop(roots);
    let pause = DirectoryWatchPause {
        root,
        suspended_roots: Arc::clone(&runtime.suspended_roots),
        desired_version: Arc::clone(&runtime.desired_version),
        signal_sender: runtime.signal_sender.clone(),
    };
    runtime.desired_version.fetch_add(1, Ordering::Release);
    synchronize_directory_watcher(runtime)?;
    Ok(pause)
}

#[cfg(target_os = "windows")]
fn synchronize_directory_watcher(runtime: &DirectoryWatcherRuntime) -> std::io::Result<()> {
    let deadline = Instant::now() + WATCH_PAUSE_TIMEOUT;
    let (reply, receiver) = mpsc::sync_channel(1);
    let mut signal = WatchSignal::Synchronize(reply);
    loop {
        match runtime.signal_sender.try_send(signal) {
            Ok(()) => break,
            Err(mpsc::TrySendError::Full(pending)) => {
                signal = pending;
                if Instant::now() >= deadline {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "Directory monitoring did not release its handles in time",
                    ));
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                return Err(std::io::Error::other("Directory monitoring stopped"));
            }
        }
    }
    receiver
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "Directory monitoring did not release its handles in time",
            )
        })?
        .map_err(std::io::Error::other)
}

fn directory_watcher() -> &'static DirectoryWatcherRuntime {
    DIRECTORY_WATCHER.get_or_init(|| {
        let desired_directories = Arc::new(Mutex::new(HashSet::new()));
        let desired_version = Arc::new(AtomicU64::new(0));
        let suspended_roots = Arc::new(Mutex::new(HashMap::new()));
        let (signal_sender, signal_receiver) = mpsc::sync_channel(WATCH_SIGNAL_CAPACITY);
        let (output_sender, output_receiver) = mpsc::sync_channel(WATCH_OUTPUT_CAPACITY);
        let worker_directories = Arc::clone(&desired_directories);
        let worker_version = Arc::clone(&desired_version);
        let worker_suspended_roots = Arc::clone(&suspended_roots);
        let event_sender = signal_sender.clone();
        thread::spawn(move || {
            run_directory_watcher(
                worker_directories,
                worker_version,
                worker_suspended_roots,
                event_sender,
                signal_receiver,
                output_sender,
            );
        });
        DirectoryWatcherRuntime {
            desired_directories,
            desired_version,
            #[cfg(target_os = "windows")]
            suspended_roots,
            signal_sender,
            output_receiver: Mutex::new(Some(output_receiver)),
        }
    })
}

fn run_directory_watcher(
    desired_directories: Arc<Mutex<HashSet<PathBuf>>>,
    desired_version: Arc<AtomicU64>,
    suspended_roots: Arc<Mutex<HashMap<PathBuf, usize>>>,
    event_sender: SyncSender<WatchSignal>,
    signal_receiver: Receiver<WatchSignal>,
    output_sender: SyncSender<DirectoryChange>,
) {
    let overflow = Arc::new(AtomicBool::new(false));
    let callback_overflow = Arc::clone(&overflow);
    let Ok(mut watcher) = notify::recommended_watcher(move |event| {
        queue_directory_event(&event_sender, &callback_overflow, event);
    }) else {
        return;
    };
    let mut watched = HashSet::new();
    let mut applied_version = 0;
    let mut batch: Option<ChangeBatch> = None;
    let mut pending_output = None;

    loop {
        // No periodic timer is needed while idle. A pending output is woken
        // by DirectoryChangeReceiver after the consumer frees an output slot.
        let signal = match &batch {
            Some(batch) => signal_receiver
                .recv_timeout(batch.deadline().saturating_duration_since(Instant::now())),
            None => signal_receiver
                .recv()
                .map_err(|_| mpsc::RecvTimeoutError::Disconnected),
        };
        if matches!(signal, Err(mpsc::RecvTimeoutError::Disconnected)) {
            return;
        }
        let version = desired_version.load(Ordering::Acquire);
        if applied_version != version {
            sync_watched_directories(
                &mut watcher,
                &desired_directories,
                &suspended_roots,
                &mut watched,
            );
            applied_version = version;
            if let Some(batch) = &mut batch {
                batch.retain_watched(&watched);
            }
            if let Some(change) = &mut pending_output {
                retain_watched_change(change, &watched);
            }
        }

        if overflow.swap(false, Ordering::AcqRel) {
            record_full_refresh(&mut batch, &watched, Instant::now());
        }
        match signal {
            Ok(WatchSignal::Reconfigure | WatchSignal::OutputReady) => {}
            #[cfg(target_os = "windows")]
            Ok(WatchSignal::Synchronize(reply)) => {
                synchronize_notify_unwatches(&mut watcher, reply);
            }
            Ok(WatchSignal::Event(Ok(event))) => {
                if event.need_rescan() {
                    record_full_refresh(&mut batch, &watched, Instant::now());
                } else if event_kind_changes_directory(&event.kind) {
                    let directories = directories_for_event(&event, &watched);
                    if !directories.is_empty() {
                        let now = Instant::now();
                        batch.get_or_insert_with(|| ChangeBatch::new(now)).record(
                            directories,
                            event.paths,
                            now,
                        );
                    }
                }
            }
            Ok(WatchSignal::Event(Err(_))) => {
                record_full_refresh(&mut batch, &watched, Instant::now());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        if batch
            .as_ref()
            .is_some_and(|batch| batch.directories.is_empty())
        {
            batch = None;
        }
        if batch
            .as_ref()
            .is_some_and(|batch| Instant::now() >= batch.deadline())
            && let Some(batch) = batch.take()
        {
            let change = batch.into_change();
            pending_output = Some(match pending_output.take() {
                Some(previous) => merge_directory_changes(previous, change),
                None => change,
            });
        }
        if !try_publish_change(&output_sender, &mut pending_output) {
            return;
        }
    }
}

fn queue_directory_event(
    sender: &SyncSender<WatchSignal>,
    overflow: &AtomicBool,
    event: notify::Result<Event>,
) {
    if event
        .as_ref()
        .is_ok_and(|event| !event.need_rescan() && !event_kind_changes_directory(&event.kind))
    {
        return;
    }
    if event
        .as_ref()
        .is_ok_and(|event| event.paths.len() > MAX_CHANGE_PATHS)
    {
        overflow.store(true, Ordering::Release);
        let _ = sender.try_send(WatchSignal::OutputReady);
        return;
    }
    if let Err(mpsc::TrySendError::Full(_)) = sender.try_send(WatchSignal::Event(event)) {
        overflow.store(true, Ordering::Release);
        // Store before waking: if this send is also full, a queued signal
        // already guarantees that the worker will observe the overflow flag.
        let _ = sender.try_send(WatchSignal::OutputReady);
    }
}

struct ChangeBatch {
    directories: HashSet<PathBuf>,
    paths: HashSet<PathBuf>,
    full_refresh: bool,
    quiet_deadline: Instant,
    max_deadline: Instant,
}

impl ChangeBatch {
    fn new(now: Instant) -> Self {
        Self {
            directories: HashSet::new(),
            paths: HashSet::new(),
            full_refresh: false,
            quiet_deadline: now + CHANGE_QUIET_PERIOD,
            max_deadline: now + MAX_CHANGE_BATCH_AGE,
        }
    }

    fn deadline(&self) -> Instant {
        self.quiet_deadline.min(self.max_deadline)
    }

    fn record(
        &mut self,
        directories: HashSet<PathBuf>,
        paths: impl IntoIterator<Item = PathBuf>,
        now: Instant,
    ) {
        self.directories.extend(directories);
        self.quiet_deadline = now + CHANGE_QUIET_PERIOD;
        if !self.full_refresh {
            for path in paths {
                if self.paths.len() >= MAX_CHANGE_PATHS {
                    self.full_refresh = true;
                    self.paths.clear();
                    break;
                }
                self.paths.insert(path);
            }
        }
    }

    fn retain_watched(&mut self, watched: &HashSet<PathBuf>) {
        self.directories.retain(|path| watched.contains(path));
        self.paths.retain(|path| {
            self.directories
                .iter()
                .any(|directory| path == directory || direct_child_of(path, directory))
        });
    }

    fn into_change(self) -> DirectoryChange {
        let mut directories = self.directories.into_iter().collect::<Vec<_>>();
        directories.sort();
        // Directory roots in paths are a conservative invalidation marker
        // when exact filenames were coalesced or the OS reported lost events.
        let mut paths = if self.full_refresh {
            directories.clone()
        } else {
            self.paths.into_iter().collect::<Vec<_>>()
        };
        paths.sort();
        DirectoryChange { directories, paths }
    }
}

fn record_full_refresh(batch: &mut Option<ChangeBatch>, watched: &HashSet<PathBuf>, now: Instant) {
    if watched.is_empty() {
        return;
    }
    let batch = batch.get_or_insert_with(|| ChangeBatch::new(now));
    batch.record(watched.clone(), [], now);
    batch.full_refresh = true;
    batch.paths.clear();
}

fn retain_watched_change(change: &mut DirectoryChange, watched: &HashSet<PathBuf>) {
    change.directories.retain(|path| watched.contains(path));
    change.paths.retain(|path| {
        change
            .directories
            .iter()
            .any(|directory| path == directory || direct_child_of(path, directory))
    });
}

fn merge_directory_changes(previous: DirectoryChange, next: DirectoryChange) -> DirectoryChange {
    let full_refresh = previous
        .paths
        .iter()
        .any(|path| previous.directories.contains(path))
        || next
            .paths
            .iter()
            .any(|path| next.directories.contains(path));
    let mut batch = ChangeBatch::new(Instant::now());
    batch.full_refresh = full_refresh;
    batch.record(
        previous
            .directories
            .into_iter()
            .chain(next.directories)
            .collect(),
        previous.paths.into_iter().chain(next.paths),
        Instant::now(),
    );
    batch.into_change()
}

/// False means the consumer has gone away. A full channel retains ownership
/// of the update and merges later updates rather than dropping the final one.
fn try_publish_change(
    sender: &SyncSender<DirectoryChange>,
    pending: &mut Option<DirectoryChange>,
) -> bool {
    let Some(change) = pending.take() else {
        return true;
    };
    if change.directories.is_empty() {
        return true;
    }
    match sender.try_send(change) {
        Ok(()) => true,
        Err(mpsc::TrySendError::Full(change)) => {
            *pending = Some(change);
            true
        }
        Err(mpsc::TrySendError::Disconnected(_)) => false,
    }
}

fn sync_watched_directories(
    watcher: &mut RecommendedWatcher,
    desired_directories: &Mutex<HashSet<PathBuf>>,
    suspended_roots: &Mutex<HashMap<PathBuf, usize>>,
    watched: &mut HashSet<PathBuf>,
) {
    let mut desired = desired_directories
        .lock()
        .map(|directories| directories.clone())
        .unwrap_or_default();
    if let Ok(roots) = suspended_roots.lock() {
        desired.retain(|path| !roots.keys().any(|root| watcher_path_is_within(path, root)));
    }

    for path in watched.difference(&desired).cloned().collect::<Vec<_>>() {
        let _ = watcher.unwatch(&path);
        watched.remove(&path);
    }
    for path in desired.difference(watched).cloned().collect::<Vec<_>>() {
        if watcher.watch(&path, RecursiveMode::NonRecursive).is_ok() {
            watched.insert(path);
        }
    }
}

#[cfg(target_os = "windows")]
fn synchronize_notify_unwatches(
    watcher: &mut RecommendedWatcher,
    reply: SyncSender<std::result::Result<(), String>>,
) {
    // notify 8.2's Windows unwatch only queues removal. configure queues an
    // acknowledged command on that same FIFO, after CancelIo/CloseHandle and
    // the outstanding read's completion. Do not begin eject on an async unwatch.
    let result = watcher
        .configure(notify::Config::default())
        .map(|_| ())
        .map_err(|error| error.to_string());
    let _ = reply.try_send(result);
}

fn watcher_path_is_within(path: &Path, root: &Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::path::{Component, Prefix};
        let drive_letter = |path: &Path| match path.components().next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                    Some(letter.to_ascii_uppercase())
                }
                _ => None,
            },
            _ => None,
        };
        if root.has_root()
            && root.components().count() == 2
            && let Some(letter) = drive_letter(root)
        {
            return drive_letter(path) == Some(letter);
        }
    }
    path.starts_with(root)
}

fn event_kind_changes_directory(kind: &EventKind) -> bool {
    !matches!(kind, EventKind::Access(_))
}

fn directories_for_event(event: &Event, watched: &HashSet<PathBuf>) -> HashSet<PathBuf> {
    if event.paths.is_empty() {
        return watched.clone();
    }
    watched
        .iter()
        .filter(|directory| {
            event
                .paths
                .iter()
                .any(|path| path == *directory || direct_child_of(path, directory))
        })
        .cloned()
        .collect()
}

fn direct_child_of(path: &Path, directory: &Path) -> bool {
    path.parent().is_some_and(|parent| parent == directory)
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{AccessKind, CreateKind, DataChange, ModifyKind};

    #[test]
    fn continuous_directory_changes_have_a_maximum_batch_age() {
        let start = Instant::now();
        let directory = PathBuf::from("/work");
        let mut batch = ChangeBatch::new(start);
        for index in 0..8 {
            let now = start + Duration::from_millis(index * 300);
            batch.record(
                HashSet::from([directory.clone()]),
                [directory.join(format!("file-{index}"))],
                now,
            );
            assert!(batch.deadline() <= start + MAX_CHANGE_BATCH_AGE);
        }
        assert_eq!(batch.deadline(), start + MAX_CHANGE_BATCH_AGE);
    }

    #[test]
    fn excessive_change_paths_become_a_bounded_full_directory_refresh() {
        let directory = PathBuf::from("/work");
        let now = Instant::now();
        let mut batch = ChangeBatch::new(now);
        batch.record(
            HashSet::from([directory.clone()]),
            (0..MAX_CHANGE_PATHS + 100).map(|index| directory.join(format!("file-{index}"))),
            now,
        );
        assert!(batch.paths.len() <= MAX_CHANGE_PATHS);
        let change = batch.into_change();
        assert_eq!(change.directories, vec![directory.clone()]);
        assert_eq!(change.paths, vec![directory]);
    }

    #[test]
    fn event_overflow_keeps_a_conservative_directory_invalidation() {
        let directory = PathBuf::from("/work");
        let mut batch = None;
        record_full_refresh(
            &mut batch,
            &HashSet::from([directory.clone()]),
            Instant::now(),
        );
        let batch = batch.unwrap();
        assert_eq!(batch.into_change().paths, vec![directory]);
    }

    #[test]
    fn a_full_signal_queue_keeps_a_conservative_refresh_pending() {
        let (sender, receiver) = mpsc::sync_channel(1);
        sender.try_send(WatchSignal::Reconfigure).unwrap();
        let overflow = AtomicBool::new(false);
        queue_directory_event(
            &sender,
            &overflow,
            Ok(Event::new(EventKind::Create(CreateKind::File))
                .add_path(PathBuf::from("/work/last-file"))),
        );
        assert!(overflow.load(Ordering::Acquire));
        assert!(matches!(receiver.recv().unwrap(), WatchSignal::Reconfigure));
        let mut batch = None;
        let directory = PathBuf::from("/work");
        if overflow.swap(false, Ordering::AcqRel) {
            record_full_refresh(
                &mut batch,
                &HashSet::from([directory.clone()]),
                Instant::now(),
            );
        }
        assert_eq!(batch.unwrap().into_change().paths, vec![directory]);
    }

    #[test]
    fn freeing_an_output_slot_delivers_the_final_pending_change_without_new_events() {
        let (output, receiver) = mpsc::sync_channel(1);
        let (signal_sender, signals) = mpsc::sync_channel(1);
        let receiver = DirectoryChangeReceiver {
            receiver,
            signal_sender,
        };
        let first = DirectoryChange {
            directories: vec!["/work".into()],
            paths: vec!["/work/first".into()],
        };
        let last = DirectoryChange {
            directories: vec!["/work".into()],
            paths: vec!["/work/last".into()],
        };
        output.try_send(first.clone()).unwrap();
        let mut pending = Some(last.clone());
        assert!(try_publish_change(&output, &mut pending));
        assert_eq!(pending, Some(last.clone()));
        assert_eq!(receiver.recv().unwrap(), first);
        assert!(matches!(signals.recv().unwrap(), WatchSignal::OutputReady));
        assert!(try_publish_change(&output, &mut pending));
        assert!(pending.is_none());
        assert_eq!(receiver.recv().unwrap(), last);
    }

    #[test]
    fn merging_saturated_output_preserves_all_dirty_directories_with_bounded_paths() {
        let left = DirectoryChange {
            directories: vec!["/left".into()],
            paths: vec!["/left".into()],
        };
        let right = DirectoryChange {
            directories: vec!["/right".into()],
            paths: vec!["/right/new".into()],
        };
        let merged = merge_directory_changes(left, right);
        assert_eq!(
            merged.directories,
            vec![PathBuf::from("/left"), PathBuf::from("/right")]
        );
        assert_eq!(merged.paths, merged.directories);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn eject_watch_scope_matches_drive_aliases_without_including_other_volumes() {
        let root = Path::new(r"E:\");
        assert!(watcher_path_is_within(Path::new(r"e:\Photos"), root));
        assert!(watcher_path_is_within(Path::new(r"\\?\E:\Photos"), root));
        assert!(!watcher_path_is_within(Path::new(r"F:\Photos"), root));
        assert!(!watcher_path_is_within(
            Path::new(r"\\server\share\Photos"),
            root
        ));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn eject_watch_pause_releases_handles_and_restores_them_after_an_error() {
        use std::fs::{File, OpenOptions};
        use std::os::windows::fs::OpenOptionsExt;
        use std::time::{SystemTime, UNIX_EPOCH};

        fn open_exclusively(path: &Path) -> std::io::Result<File> {
            OpenOptions::new()
                .access_mode(1) // FILE_LIST_DIRECTORY participates in handle sharing checks.
                .share_mode(0)
                .custom_flags(0x0200_0000) // FILE_FLAG_BACKUP_SEMANTICS for a directory.
                .open(path)
        }

        struct Fixture(PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                set_visible_directories(Vec::new());
                let _ = synchronize_directory_watcher(directory_watcher());
                let _ = std::fs::remove_dir(self.0.join("external").join("child"));
                let _ = std::fs::remove_dir(self.0.join("external"));
                let _ = std::fs::remove_dir(self.0.join("unrelated"));
                let _ = std::fs::remove_dir(&self.0);
            }
        }

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let fixture = Fixture(std::env::temp_dir().join(format!(
            "bexplorer-watch-pause-{}-{nonce}",
            std::process::id()
        )));
        let external = fixture.0.join("external");
        let child = external.join("child");
        let unrelated = fixture.0.join("unrelated");
        std::fs::create_dir_all(&child).unwrap();
        std::fs::create_dir(&unrelated).unwrap();
        set_visible_directories([external.clone(), unrelated.clone()]);
        synchronize_directory_watcher(directory_watcher()).unwrap();
        assert_eq!(
            open_exclusively(&external).unwrap_err().raw_os_error(),
            Some(32)
        );

        let result = (|| -> std::io::Result<()> {
            let _pause = pause_directory_watches(&external)?;
            drop(open_exclusively(&external)?);
            let nested_pause = pause_directory_watches(&external)?;
            drop(nested_pause);
            set_visible_directories([external.clone(), child.clone(), unrelated.clone()]);
            synchronize_directory_watcher(directory_watcher())?;
            drop(open_exclusively(&external)?);
            drop(open_exclusively(&child)?);
            assert_eq!(
                open_exclusively(&unrelated).unwrap_err().raw_os_error(),
                Some(32)
            );
            Err(std::io::Error::other("Simulated removal refused"))
        })();
        assert!(result.is_err());
        synchronize_directory_watcher(directory_watcher()).unwrap();
        assert_eq!(
            open_exclusively(&external).unwrap_err().raw_os_error(),
            Some(32)
        );
        assert_eq!(
            open_exclusively(&child).unwrap_err().raw_os_error(),
            Some(32)
        );
    }

    #[test]
    fn ignores_access_events_but_keeps_content_and_structure_changes() {
        assert!(!event_kind_changes_directory(&EventKind::Access(
            AccessKind::Any
        )));
        assert!(event_kind_changes_directory(&EventKind::Create(
            CreateKind::File
        )));
        assert!(event_kind_changes_directory(&EventKind::Modify(
            ModifyKind::Data(DataChange::Content)
        )));
    }

    #[test]
    fn maps_events_only_to_their_directly_watched_directory() {
        let downloads = PathBuf::from("/home/example/Downloads");
        let pictures = PathBuf::from("/home/example/Pictures");
        let watched = HashSet::from([downloads.clone(), pictures]);
        let event =
            Event::new(EventKind::Create(CreateKind::File)).add_path(downloads.join("archive.zip"));

        assert_eq!(
            directories_for_event(&event, &watched),
            HashSet::from([downloads])
        );
    }

    #[test]
    fn does_not_treat_nested_descendants_as_non_recursive_events() {
        let downloads = PathBuf::from("/home/example/Downloads");
        let watched = HashSet::from([downloads.clone()]);
        let event = Event::new(EventKind::Create(CreateKind::File))
            .add_path(downloads.join("nested").join("archive.zip"));

        assert!(directories_for_event(&event, &watched).is_empty());
    }
}
