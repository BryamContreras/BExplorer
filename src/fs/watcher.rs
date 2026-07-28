use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

const CHANGE_QUIET_PERIOD: Duration = Duration::from_millis(700);
const WATCH_SIGNAL_CAPACITY: usize = 32;
const WATCH_OUTPUT_CAPACITY: usize = 2;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DirectoryChange {
    pub directories: Vec<PathBuf>,
    pub paths: Vec<PathBuf>,
}

enum WatchSignal {
    Reconfigure,
    Acknowledge(Vec<PathBuf>),
    Event(notify::Result<Event>),
}

struct DirectoryWatcherRuntime {
    desired_directories: Arc<Mutex<HashSet<PathBuf>>>,
    desired_version: Arc<AtomicU64>,
    signal_sender: SyncSender<WatchSignal>,
    output_receiver: Mutex<Option<Receiver<DirectoryChange>>>,
}

static DIRECTORY_WATCHER: OnceLock<DirectoryWatcherRuntime> = OnceLock::new();

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

pub fn directory_change_receiver() -> Receiver<DirectoryChange> {
    let runtime = directory_watcher();
    runtime
        .output_receiver
        .lock()
        .ok()
        .and_then(|mut receiver| receiver.take())
        .unwrap_or_else(|| {
            let (_sender, receiver) = mpsc::sync_channel(1);
            receiver
        })
}

pub fn acknowledge_directories<I>(directories: I)
where
    I: IntoIterator<Item = PathBuf>,
{
    let directories = directories.into_iter().collect::<Vec<_>>();
    if directories.is_empty() {
        return;
    }
    let _ = directory_watcher()
        .signal_sender
        .try_send(WatchSignal::Acknowledge(directories));
}

fn directory_watcher() -> &'static DirectoryWatcherRuntime {
    DIRECTORY_WATCHER.get_or_init(|| {
        let desired_directories = Arc::new(Mutex::new(HashSet::new()));
        let desired_version = Arc::new(AtomicU64::new(0));
        let (signal_sender, signal_receiver) = mpsc::sync_channel(WATCH_SIGNAL_CAPACITY);
        let (output_sender, output_receiver) = mpsc::sync_channel(WATCH_OUTPUT_CAPACITY);
        let worker_directories = Arc::clone(&desired_directories);
        let worker_version = Arc::clone(&desired_version);
        let event_sender = signal_sender.clone();
        thread::spawn(move || {
            run_directory_watcher(
                worker_directories,
                worker_version,
                event_sender,
                signal_receiver,
                output_sender,
            );
        });
        DirectoryWatcherRuntime {
            desired_directories,
            desired_version,
            signal_sender,
            output_receiver: Mutex::new(Some(output_receiver)),
        }
    })
}

fn run_directory_watcher(
    desired_directories: Arc<Mutex<HashSet<PathBuf>>>,
    desired_version: Arc<AtomicU64>,
    event_sender: SyncSender<WatchSignal>,
    signal_receiver: Receiver<WatchSignal>,
    output_sender: SyncSender<DirectoryChange>,
) {
    let callback_sender = event_sender;
    let Ok(mut watcher) = notify::recommended_watcher(move |event| {
        let _ = callback_sender.try_send(WatchSignal::Event(event));
    }) else {
        return;
    };
    let mut watched = HashSet::new();
    let mut applied_version = 0;

    while let Ok(signal) = signal_receiver.recv() {
        let version = desired_version.load(Ordering::Acquire);
        if applied_version != version {
            sync_watched_directories(&mut watcher, &desired_directories, &mut watched);
            applied_version = version;
        }
        if matches!(signal, WatchSignal::Reconfigure) {
            continue;
        }
        let WatchSignal::Event(event) = signal else {
            continue;
        };
        let Ok(event) = event else {
            continue;
        };
        if !event_kind_changes_directory(&event.kind) {
            continue;
        }

        let mut dirty_directories = directories_for_event(&event, &watched);
        if dirty_directories.is_empty() {
            continue;
        }
        let mut dirty_paths = event.paths.into_iter().collect::<HashSet<_>>();
        let mut deadline = Instant::now() + CHANGE_QUIET_PERIOD;

        loop {
            let timeout = deadline.saturating_duration_since(Instant::now());
            let signal = signal_receiver.recv_timeout(timeout);
            if signal.is_ok() {
                let version = desired_version.load(Ordering::Acquire);
                if applied_version != version {
                    sync_watched_directories(&mut watcher, &desired_directories, &mut watched);
                    applied_version = version;
                }
            }
            match signal {
                Ok(WatchSignal::Reconfigure) => {
                    dirty_directories.retain(|path| watched.contains(path));
                    if dirty_directories.is_empty() {
                        break;
                    }
                }
                Ok(WatchSignal::Acknowledge(directories)) => {
                    dirty_directories.retain(|path| !directories.contains(path));
                    dirty_paths.retain(|path| {
                        !directories
                            .iter()
                            .any(|directory| path == directory || direct_child_of(path, directory))
                    });
                    if dirty_directories.is_empty() {
                        break;
                    }
                }
                Ok(WatchSignal::Event(Ok(event))) => {
                    if !event_kind_changes_directory(&event.kind) {
                        continue;
                    }
                    let event_directories = directories_for_event(&event, &watched);
                    if event_directories.is_empty() {
                        continue;
                    }
                    dirty_directories.extend(event_directories);
                    dirty_paths.extend(event.paths);
                    deadline = Instant::now() + CHANGE_QUIET_PERIOD;
                }
                Ok(WatchSignal::Event(Err(_))) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let mut directories = dirty_directories.into_iter().collect::<Vec<_>>();
                    directories.sort();
                    let mut paths = dirty_paths.into_iter().collect::<Vec<_>>();
                    paths.sort();
                    let _ = output_sender.try_send(DirectoryChange { directories, paths });
                    break;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}

fn sync_watched_directories(
    watcher: &mut RecommendedWatcher,
    desired_directories: &Mutex<HashSet<PathBuf>>,
    watched: &mut HashSet<PathBuf>,
) {
    let desired = desired_directories
        .lock()
        .map(|directories| directories.clone())
        .unwrap_or_default();

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
