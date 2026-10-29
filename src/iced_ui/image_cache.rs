use super::*;
use std::cell::Cell;

struct CachedImage {
    state: IcedImageState,
    bytes: usize,
    used: Cell<u64>,
    cancel: Arc<AtomicBool>,
    abort: iced::futures::future::AbortHandle,
    registration: RefCell<Option<iced::futures::future::AbortRegistration>>,
}

impl Drop for CachedImage {
    fn drop(&mut self) {
        self.cancel.store(true, AtomicOrdering::Relaxed);
        self.abort.abort();
    }
}

/// CPU pixel storage is retained by Iced handles even after GPU eviction.
pub(super) struct ImageCache {
    entries: HashMap<PathBuf, CachedImage>,
    bytes: usize,
    max_bytes: usize,
    clock: Cell<u64>,
}

impl ImageCache {
    pub(super) fn new(max_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            bytes: 0,
            max_bytes,
            clock: Cell::new(0),
        }
    }

    pub(super) fn get(&self, path: &Path) -> Option<&IcedImageState> {
        let entry = self.entries.get(path)?;
        self.clock.set(self.clock.get().wrapping_add(1));
        entry.used.set(self.clock.get());
        Some(&entry.state)
    }

    pub(super) fn contains_key(&self, path: &Path) -> bool {
        self.get(path).is_some()
    }

    fn request_token(
        &self,
        path: &Path,
    ) -> Option<(Arc<AtomicBool>, iced::futures::future::AbortRegistration)> {
        let entry = self.entries.get(path)?;
        Some((
            Arc::clone(&entry.cancel),
            entry.registration.borrow_mut().take()?,
        ))
    }

    pub(super) fn clear_portable_misses(&mut self) {
        self.entries.retain(|path, entry| {
            !explorer::is_portable_path(path) || !matches!(entry.state, IcedImageState::Missing)
        });
    }

    pub(super) fn cancel_pending_except(&mut self, keep: &HashSet<PathBuf>) {
        self.entries.retain(|path, entry| {
            !matches!(entry.state, IcedImageState::Loading) || keep.contains(path)
        });
    }

    pub(super) fn remove(&mut self, path: &Path) -> Option<IcedImageState> {
        let mut entry = self.entries.remove(path)?;
        self.bytes = self.bytes.saturating_sub(entry.bytes);
        Some(std::mem::replace(&mut entry.state, IcedImageState::Missing))
    }

    pub(super) fn insert(&mut self, path: PathBuf, state: IcedImageState) {
        self.remove(&path);
        let bytes = match &state {
            IcedImageState::Ready(iced_image::Handle::Rgba { pixels, .. }) => pixels.len(),
            IcedImageState::Ready(iced_image::Handle::Bytes(_, bytes)) => bytes.len(),
            _ => 0,
        };
        if bytes > self.max_bytes {
            return;
        }
        self.clock.set(self.clock.get().wrapping_add(1));
        self.bytes += bytes;
        let (abort, registration) = iced::futures::future::AbortHandle::new_pair();
        self.entries.insert(
            path,
            CachedImage {
                state,
                bytes,
                used: Cell::new(self.clock.get()),
                cancel: Arc::new(AtomicBool::new(false)),
                abort,
                registration: RefCell::new(Some(registration)),
            },
        );
        while self.bytes > self.max_bytes || self.entries.len() > 2048 {
            let oldest = self
                .entries
                .iter()
                .filter(|(_, entry)| !matches!(entry.state, IcedImageState::Loading))
                .min_by_key(|(_, entry)| entry.used.get())
                .map(|(path, _)| path.clone())
                .or_else(|| {
                    self.entries
                        .iter()
                        .min_by_key(|(_, entry)| entry.used.get())
                        .map(|(path, _)| path.clone())
                });
            let Some(oldest) = oldest else { break };
            self.remove(&oldest);
        }
    }
}

impl BExplorerIced {
    pub(in crate::iced_ui) fn load_iced_image_task(&self, job: IcedImageJob) -> Task<Message> {
        let request = match iced_image_job_key(&job) {
            IcedImageKey::Thumbnail { path, variant } => {
                self.thumbnail_cache_for(variant).request_token(&path)
            }
            IcedImageKey::Preview(path) => self.preview_cache.request_token(&path),
            IcedImageKey::NativeIcon { path, variant } => {
                self.native_icon_cache_for(variant).request_token(&path)
            }
        };
        match request {
            Some((token, registration)) => load_iced_image_task(job, token, registration),
            None => Task::none(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image() -> IcedImageState {
        IcedImageState::Ready(iced_image::Handle::from_rgba(1, 1, vec![0; 4]))
    }

    #[test]
    fn evicts_least_recently_used_pixels_and_counts_replacements() {
        let mut cache = ImageCache::new(8);
        cache.insert("a".into(), image());
        cache.insert("b".into(), image());
        assert!(cache.get(Path::new("a")).is_some());
        cache.insert("c".into(), image());
        assert!(cache.get(Path::new("b")).is_none());
        assert!(cache.get(Path::new("a")).is_some());
        cache.insert("a".into(), IcedImageState::Missing);
        assert_eq!(cache.bytes, 4);
        cache.remove(Path::new("c"));
        assert_eq!(cache.bytes, 0);
    }

    #[test]
    fn an_inflight_request_is_not_evicted_by_completed_images() {
        let mut cache = ImageCache::new(4);
        cache.insert("loading".into(), IcedImageState::Loading);
        cache.insert("a".into(), image());
        cache.insert("b".into(), image());
        assert!(matches!(
            cache.get(Path::new("loading")),
            Some(IcedImageState::Loading)
        ));
        assert_eq!(cache.bytes, 4);
    }

    #[test]
    fn refresh_retries_missing_mobile_thumbnails_without_losing_ready_images() {
        let path = explorer::portable_device_path("device", "image");
        let mut cache = ImageCache::new(8);
        cache.insert(path.clone(), IcedImageState::Missing);
        cache.insert("local".into(), image());
        cache.clear_portable_misses();
        assert!(!cache.contains_key(&path));
        assert!(cache.contains_key(Path::new("local")));
        assert_eq!(cache.bytes, 4);
    }

    #[test]
    fn removing_a_pending_image_aborts_its_async_waiter() {
        let mut cache = ImageCache::new(4);
        cache.insert("old".into(), IcedImageState::Loading);
        let (cancel, registration) = cache.request_token(Path::new("old")).unwrap();
        cache.cancel_pending_except(&HashSet::new());
        assert!(cancel.load(AtomicOrdering::Relaxed));
        assert!(
            iced::futures::executor::block_on(iced::futures::future::Abortable::new(
                iced::futures::future::pending::<()>(),
                registration
            ))
            .is_err()
        );
    }

    #[test]
    fn even_pending_entries_have_a_hard_count_limit() {
        let mut cache = ImageCache::new(4);
        cache.insert("old".into(), IcedImageState::Loading);
        let (cancel, _) = cache.request_token(Path::new("old")).unwrap();
        for index in 0..2050 {
            cache.insert(format!("pending-{index}").into(), IcedImageState::Loading);
        }
        assert_eq!(cache.entries.len(), 2048);
        assert!(cancel.load(AtomicOrdering::Relaxed));
    }
}
