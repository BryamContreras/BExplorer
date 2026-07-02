use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use crate::fs::explorer::{self, FileEntry};

use super::types::LoadMessage;

const NETWORK_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);
static NETWORK_DISCOVERY_ACTIVE: AtomicBool = AtomicBool::new(false);
static NETWORK_ROOT_CACHE: OnceLock<Mutex<Vec<FileEntry>>> = OnceLock::new();

enum NetworkSourceMessage {
    Entries(Vec<FileEntry>),
    Finished,
}

pub(super) fn spawn_network_root_load(tx: Sender<LoadMessage>, request_id: u64) {
    thread::spawn(move || {
        let cached_entries = network_root_cached_entries();
        if !cached_entries.is_empty() {
            let _ = tx.send(LoadMessage {
                request_id,
                finished: false,
                append: false,
                result: Ok(cached_entries.clone()),
            });
        }

        let mut discovered_entries = Vec::new();

        if NETWORK_DISCOVERY_ACTIVE.swap(true, AtomicOrdering::AcqRel) {
            let _ = tx.send(LoadMessage {
                request_id,
                finished: true,
                append: false,
                result: Ok(cached_entries),
            });
            return;
        }

        let (source_tx, source_rx) = mpsc::channel();

        let mut pending_sources = 0_usize;
        spawn_network_source(&source_tx, &mut pending_sources, || {
            explorer::list_network_computer_entries_netbios_cached()
        });
        spawn_network_source(&source_tx, &mut pending_sources, || {
            explorer::list_network_printer_entries()
        });
        spawn_network_source(&source_tx, &mut pending_sources, || {
            explorer::list_network_shell_entries()
        });
        spawn_network_source(&source_tx, &mut pending_sources, || {
            explorer::list_network_computer_entries_fast()
        });
        spawn_network_source(&source_tx, &mut pending_sources, || {
            explorer::list_network_function_device_entries()
        });
        spawn_network_source(&source_tx, &mut pending_sources, || {
            explorer::list_network_computer_entries_wnet()
        });
        spawn_netbios_neighbor_source(&source_tx, &mut pending_sources);
        drop(source_tx);

        let deadline = Instant::now() + NETWORK_DISCOVERY_TIMEOUT;
        while pending_sources > 0 && Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match source_rx.recv_timeout(remaining.min(Duration::from_millis(150))) {
                Ok(NetworkSourceMessage::Entries(entries)) => {
                    if entries.is_empty() {
                        continue;
                    }
                    let previous_entries = discovered_entries.clone();
                    merge_load_entries(&mut discovered_entries, entries);
                    if file_entries_equal(&previous_entries, &discovered_entries) {
                        continue;
                    }
                    store_network_root_cache(&discovered_entries);
                    let mut visible_entries = cached_entries.clone();
                    merge_load_entries(&mut visible_entries, discovered_entries.clone());
                    let _ = tx.send(LoadMessage {
                        request_id,
                        finished: false,
                        append: false,
                        result: Ok(visible_entries),
                    });
                }
                Ok(NetworkSourceMessage::Finished) => {
                    pending_sources = pending_sources.saturating_sub(1);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        NETWORK_DISCOVERY_ACTIVE.store(false, AtomicOrdering::Release);
        if !discovered_entries.is_empty() {
            store_network_root_cache(&discovered_entries);
        }
        let final_entries = if discovered_entries.is_empty() {
            cached_entries
        } else {
            discovered_entries
        };
        let _ = tx.send(LoadMessage {
            request_id,
            finished: true,
            append: false,
            result: Ok(final_entries),
        });
    });
}

fn spawn_network_source<F>(
    source_tx: &Sender<NetworkSourceMessage>,
    pending_sources: &mut usize,
    load: F,
) where
    F: FnOnce() -> Vec<FileEntry> + Send + 'static,
{
    *pending_sources += 1;
    let source_tx = source_tx.clone();
    thread::spawn(move || {
        let entries = load();
        if !entries.is_empty() {
            let _ = source_tx.send(NetworkSourceMessage::Entries(entries));
        }
        let _ = source_tx.send(NetworkSourceMessage::Finished);
    });
}

fn spawn_netbios_neighbor_source(
    source_tx: &Sender<NetworkSourceMessage>,
    pending_sources: &mut usize,
) {
    *pending_sources += 1;
    let source_tx = source_tx.clone();
    thread::spawn(move || {
        let addresses = explorer::list_network_netbios_neighbor_addresses();
        if addresses.is_empty() {
            let _ = source_tx.send(NetworkSourceMessage::Finished);
            return;
        }

        let (host_tx, host_rx) = mpsc::channel();
        for address in addresses.iter().cloned() {
            let host_tx = host_tx.clone();
            thread::spawn(move || {
                let entry = explorer::network_computer_entry_netbios_address(&address);
                let _ = host_tx.send(entry);
            });
        }
        drop(host_tx);

        let deadline = Instant::now() + Duration::from_millis(8000);
        let mut pending_hosts = addresses.len();
        while pending_hosts > 0 && Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match host_rx.recv_timeout(remaining.min(Duration::from_millis(150))) {
                Ok(Some(entry)) => {
                    pending_hosts = pending_hosts.saturating_sub(1);
                    let _ = source_tx.send(NetworkSourceMessage::Entries(vec![entry]));
                }
                Ok(None) => {
                    pending_hosts = pending_hosts.saturating_sub(1);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        let _ = source_tx.send(NetworkSourceMessage::Finished);
    });
}

pub(super) fn network_root_cache() -> &'static Mutex<Vec<FileEntry>> {
    NETWORK_ROOT_CACHE.get_or_init(|| Mutex::new(Vec::new()))
}

pub(super) fn network_root_cached_entries() -> Vec<FileEntry> {
    network_root_cache()
        .lock()
        .map(|entries| entries.clone())
        .unwrap_or_default()
}

pub(super) fn store_network_root_cache(entries: &[FileEntry]) {
    if let Ok(mut cache) = network_root_cache().lock() {
        *cache = entries.to_vec();
    }
}

pub(super) fn merge_load_entries(target: &mut Vec<FileEntry>, entries: Vec<FileEntry>) {
    for entry in entries {
        if let Some(existing) = target
            .iter_mut()
            .find(|existing| existing.path == entry.path)
        {
            if file_entry_merge_priority(&entry) >= file_entry_merge_priority(existing) {
                *existing = entry;
            }
        } else {
            target.push(entry);
        }
    }
    explorer::sort_entries_by_name(target);
}

pub(super) fn file_entry_merge_priority(entry: &FileEntry) -> u8 {
    match entry.drive_kind {
        Some(explorer::DriveKind::NetworkMultifunction) => 70,
        Some(explorer::DriveKind::NetworkPrinter | explorer::DriveKind::NetworkScanner) => 65,
        Some(explorer::DriveKind::NetworkComputer) => 60,
        Some(explorer::DriveKind::NetworkDevice) => 40,
        Some(explorer::DriveKind::Network) => 30,
        Some(_) => 20,
        None => 10,
    }
}

pub(super) fn file_entries_equal(left: &[FileEntry], right: &[FileEntry]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.name == right.name
                && left.path == right.path
                && left.kind == right.kind
                && left.category == right.category
                && left.drive_kind == right.drive_kind
                && left.file_system == right.file_system
                && left.free_space == right.free_space
                && left.size == right.size
                && left.percent_full == right.percent_full
                && left.modified == right.modified
                && left.is_hidden == right.is_hidden
        })
}
