use super::*;

#[derive(Default)]
struct PendingCaches {
    storage: Option<Vec<FileEntry>>,
    network: Option<Vec<FileEntry>>,
}

struct CacheWriter {
    pending: Arc<std::sync::Mutex<PendingCaches>>,
    wake: std::sync::mpsc::SyncSender<()>,
}

fn writer() -> &'static CacheWriter {
    static WRITER: std::sync::OnceLock<CacheWriter> = std::sync::OnceLock::new();
    WRITER.get_or_init(|| {
        let pending = Arc::new(std::sync::Mutex::new(PendingCaches::default()));
        let (wake, receiver) = std::sync::mpsc::sync_channel(1);
        let worker_pending = Arc::clone(&pending);
        std::thread::Builder::new()
            .name("bexplorer-cache-writer".into())
            .spawn(move || {
                while receiver.recv().is_ok() {
                    let batch = {
                        let mut pending = worker_pending
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        std::mem::take(&mut *pending)
                    };
                    if let Some(entries) = batch.storage
                        && let Err(error) = explorer::save_storage_cache(&entries)
                    {
                        crate::utils::log::error(format!(
                            "Could not persist storage cache: {error}"
                        ));
                    }
                    if let Some(entries) = batch.network
                        && let Err(error) = explorer::save_network_cache(&entries)
                    {
                        crate::utils::log::error(format!(
                            "Could not persist network cache: {error}"
                        ));
                    }
                }
            })
            .expect("could not start cache persistence worker");
        CacheWriter { pending, wake }
    })
}

pub(in crate::iced_ui) fn schedule_storage_cache(entries: Vec<FileEntry>) {
    let writer = writer();
    writer
        .pending
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .storage = Some(entries);
    let _ = writer.wake.try_send(());
}

pub(in crate::iced_ui) fn schedule_network_cache(entries: Vec<FileEntry>) {
    let writer = writer();
    writer
        .pending
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .network = Some(entries);
    let _ = writer.wake.try_send(());
}
