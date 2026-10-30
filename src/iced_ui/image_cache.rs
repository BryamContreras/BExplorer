use super::*;
use std::cell::Cell;

const MAX_CACHE_ENTRIES: usize = 2048;

/// The listing worker already collected these values. Comparing them on the
/// UI thread does not open the source again or truncate its timestamp.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ImageSourceIdentity {
    path: PathBuf,
    size: Option<u64>,
    modified: Option<std::time::SystemTime>,
    kind: EntryKind,
}

impl ImageSourceIdentity {
    fn from_entry(entry: &FileEntry) -> Self {
        Self {
            path: entry.path.clone(),
            size: entry.size,
            modified: entry.modified_time,
            kind: entry.kind.clone(),
        }
    }

    fn matches_entry(&self, entry: &FileEntry) -> bool {
        self.path == entry.path
            && self.size == entry.size
            && self.modified == entry.modified_time
            && self.kind == entry.kind
    }
}

struct CachedImage {
    state: IcedImageState,
    source: Option<ImageSourceIdentity>,
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

    /// Returns true only when a new request is needed. Generic Shell icons use
    /// no source identity so many files can keep sharing one extension icon.
    pub(super) fn prepare(&mut self, key: PathBuf, source: Option<&FileEntry>) -> bool {
        if let Some(entry) = self.entries.get(&key)
            && match (&entry.source, source) {
                (Some(cached), Some(source)) => cached.matches_entry(source),
                (None, None) => true,
                _ => false,
            }
        {
            self.touch(entry);
            return false;
        }
        self.insert_with_source(
            key,
            IcedImageState::Loading,
            source.map(ImageSourceIdentity::from_entry),
        );
        true
    }

    fn touch(&self, entry: &CachedImage) {
        self.clock.set(self.clock.get().wrapping_add(1));
        entry.used.set(self.clock.get());
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
            !matches!(entry.state, IcedImageState::Loading) || entry_is_kept(path, entry, keep)
        });
    }

    pub(super) fn invalidate_paths(&mut self, paths: &[PathBuf]) {
        let paths = paths.iter().map(PathBuf::as_path).collect::<HashSet<_>>();
        self.remove_where(|key, entry| {
            paths.contains(key.as_path())
                || entry
                    .source
                    .as_ref()
                    .is_some_and(|source| paths.contains(source.path.as_path()))
        });
    }

    pub(super) fn reconcile_sources(&mut self, entries: &[FileEntry]) -> HashSet<PathBuf> {
        let interested = self
            .entries
            .values()
            .filter_map(|entry| entry.source.as_ref().map(|source| source.path.as_path()))
            .collect::<HashSet<_>>();
        if interested.is_empty() {
            return HashSet::new();
        }
        // Only index files this cache actually retains, keeping temporary
        // bookkeeping bounded even when the directory has 100,000 entries.
        let sources = entries
            .iter()
            .filter(|entry| interested.contains(entry.path.as_path()))
            .map(|entry| (entry.path.as_path(), entry))
            .collect::<HashMap<_, _>>();
        let mut changed = HashSet::new();
        let removed = self
            .entries
            .iter()
            .filter_map(|(key, cached)| {
                let source = cached.source.as_ref()?;
                let current = sources.get(source.path.as_path())?;
                if source.matches_entry(current) {
                    return None;
                }
                changed.insert(source.path.clone());
                Some(key.clone())
            })
            .collect::<Vec<_>>();
        drop(interested);
        for key in removed {
            self.remove(&key);
        }
        changed
    }

    /// F5 also retries prior misses and cancels a decode that is now obsolete.
    /// Extension icons without a concrete source remain reusable.
    pub(super) fn invalidate_directory(&mut self, directory: &Path) {
        self.remove_where(|key, entry| {
            key.parent() == Some(directory)
                || entry
                    .source
                    .as_ref()
                    .is_some_and(|source| source.path.parent() == Some(directory))
        });
    }

    fn remove_where(&mut self, predicate: impl Fn(&PathBuf, &CachedImage) -> bool) {
        let removed = self
            .entries
            .iter()
            .filter(|(key, entry)| predicate(key, entry))
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for path in removed {
            self.remove(&path);
        }
    }

    pub(super) fn remove(&mut self, path: &Path) -> Option<IcedImageState> {
        let mut entry = self.entries.remove(path)?;
        self.bytes = self.bytes.saturating_sub(entry.bytes);
        Some(std::mem::replace(&mut entry.state, IcedImageState::Missing))
    }

    pub(super) fn insert(&mut self, path: PathBuf, state: IcedImageState) {
        self.insert_with_source(path, state, None);
    }

    fn insert_with_source(
        &mut self,
        path: PathBuf,
        state: IcedImageState,
        source: Option<ImageSourceIdentity>,
    ) {
        self.remove(&path);
        let bytes = image_bytes(&state);
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
                source,
                bytes,
                used: Cell::new(self.clock.get()),
                cancel: Arc::new(AtomicBool::new(false)),
                abort,
                registration: RefCell::new(Some(registration)),
            },
        );
        self.enforce_budget();
    }

    /// Accept a completion only for the exact request still occupying this
    /// slot. A late worker cannot overwrite a replacement after F5/navigation.
    pub(super) fn complete(
        &mut self,
        path: &Path,
        token: &Arc<AtomicBool>,
        mut state: IcedImageState,
    ) -> bool {
        let Some(entry) = self.entries.get_mut(path) else {
            return false;
        };
        if token.load(AtomicOrdering::Relaxed)
            || !Arc::ptr_eq(token, &entry.cancel)
            || !matches!(entry.state, IcedImageState::Loading)
        {
            return false;
        }
        let mut bytes = image_bytes(&state);
        if bytes > self.max_bytes {
            // Do not repeatedly decode an image that cannot fit this cache.
            state = IcedImageState::Missing;
            bytes = 0;
        }
        self.bytes = self.bytes.saturating_sub(entry.bytes).saturating_add(bytes);
        entry.state = state;
        entry.bytes = bytes;
        self.clock.set(self.clock.get().wrapping_add(1));
        entry.used.set(self.clock.get());
        self.enforce_budget();
        true
    }

    /// Reduce retained images during inactivity without evicting the selected
    /// preview or another visible pane. The normal maximum never increases.
    pub(super) fn trim_to(&mut self, target_bytes: usize, keep: &HashSet<PathBuf>) {
        let target_bytes = target_bytes.min(self.max_bytes);
        self.remove_where(|path, entry| entry.bytes == 0 && !entry_is_kept(path, entry, keep));
        while self.bytes > target_bytes {
            let oldest = self
                .entries
                .iter()
                .filter(|(path, entry)| !entry_is_kept(path, entry, keep))
                .min_by_key(|(_, entry)| entry.used.get())
                .map(|(path, _)| path.clone());
            let Some(oldest) = oldest else { break };
            self.remove(&oldest);
        }
    }

    fn enforce_budget(&mut self) {
        while self.bytes > self.max_bytes || self.entries.len() > MAX_CACHE_ENTRIES {
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

fn entry_is_kept(key: &Path, entry: &CachedImage, keep: &HashSet<PathBuf>) -> bool {
    keep.contains(key)
        || entry
            .source
            .as_ref()
            .is_some_and(|source| keep.contains(&source.path))
}

fn image_bytes(state: &IcedImageState) -> usize {
    match state {
        IcedImageState::Ready(iced_image::Handle::Rgba { pixels, .. }) => pixels.len(),
        IcedImageState::Ready(iced_image::Handle::Bytes(_, bytes)) => bytes.len(),
        _ => 0,
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
    use std::time::{Duration, SystemTime};

    fn image() -> IcedImageState {
        IcedImageState::Ready(iced_image::Handle::from_rgba(1, 1, vec![0; 4]))
    }

    fn source(path: &str, clock_tick: u64) -> FileEntry {
        FileEntry {
            path: PathBuf::from(path),
            name: "image.png".into(),
            kind: EntryKind::File,
            category: FileCategory::Image,
            drive_kind: None,
            file_system: String::new(),
            free_space: None,
            size: Some(4),
            percent_full: None,
            modified: Some("2026-10-29 12:00".into()),
            // Windows SystemTime stores 100 ns ticks. Keep the simulated
            // versions distinct on both Windows and Unix timestamp backends.
            modified_time: Some(SystemTime::UNIX_EPOCH + Duration::from_nanos(clock_tick * 100)),
            created: None,
            is_hidden: false,
        }
    }

    #[test]
    fn precise_identity_replaces_same_size_images_without_accepting_an_old_completion() {
        let mut cache = ImageCache::new(16);
        let first = source("images/photo.png", 1);
        assert!(cache.prepare(first.path.clone(), Some(&first)));
        let (old_token, _) = cache.request_token(&first.path).unwrap();
        assert!(cache.complete(&first.path, &old_token, image()));
        assert!(!cache.prepare(first.path.clone(), Some(&first)));

        // Displayed date and file length are unchanged; nanoseconds identify
        // a replacement made outside the currently watched directory.
        let changed = source("images/photo.png", 2);
        assert!(cache.prepare(changed.path.clone(), Some(&changed)));
        let (new_token, _) = cache.request_token(&changed.path).unwrap();
        assert!(old_token.load(AtomicOrdering::Relaxed));
        assert!(!cache.complete(&first.path, &old_token, image()));
        assert!(matches!(
            cache.get(&first.path),
            Some(IcedImageState::Loading)
        ));
        assert!(cache.complete(&changed.path, &new_token, image()));
        assert!(!cache.prepare(changed.path.clone(), Some(&changed)));
        assert_eq!(cache.bytes, 4);
    }

    #[test]
    fn changed_files_retry_prior_misses_and_refresh_only_invalidates_the_selected_directory() {
        let mut cache = ImageCache::new(16);
        let entry = source("images/photo.png", 1);
        assert!(cache.prepare(entry.path.clone(), Some(&entry)));
        let (token, _) = cache.request_token(&entry.path).unwrap();
        assert!(cache.complete(&entry.path, &token, IcedImageState::Missing));
        assert!(!cache.prepare(entry.path.clone(), Some(&entry)));
        let mut changed = entry.clone();
        changed.size = Some(8);
        assert!(cache.prepare(changed.path.clone(), Some(&changed)));
        let (refresh_token, _) = cache.request_token(&entry.path).unwrap();
        cache.insert("elsewhere/kept.png".into(), image());
        cache.insert("generic-png-icon".into(), image());

        cache.invalidate_directory(Path::new("images"));
        assert!(refresh_token.load(AtomicOrdering::Relaxed));
        assert!(!cache.contains_key(&entry.path));
        assert!(cache.contains_key(Path::new("elsewhere/kept.png")));
        assert!(cache.contains_key(Path::new("generic-png-icon")));
        assert_eq!(cache.bytes, 8);
    }

    #[test]
    fn trimming_preserves_visible_sources_and_cancels_obsolete_pending_requests() {
        let mut cache = ImageCache::new(16);
        let selected = source("images/selected.png", 1);
        assert!(cache.prepare(selected.path.clone(), Some(&selected)));
        let (token, _) = cache.request_token(&selected.path).unwrap();
        assert!(cache.complete(&selected.path, &token, image()));
        cache.insert("old-a".into(), image());
        cache.insert("old-b".into(), image());
        cache.insert("pending".into(), IcedImageState::Loading);
        let (pending, _) = cache.request_token(Path::new("pending")).unwrap();

        cache.trim_to(4, &HashSet::from([selected.path.clone()]));
        assert!(cache.contains_key(&selected.path));
        assert_eq!(cache.entries.len(), 1);
        assert_eq!(cache.bytes, 4);
        assert!(pending.load(AtomicOrdering::Relaxed));
        cache.trim_to(0, &HashSet::new());
        assert_eq!(cache.bytes, 0);
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn invalidating_a_source_also_removes_its_specific_native_icon() {
        let mut cache = ImageCache::new(16);
        let entry = source("programs/app.exe", 1);
        let icon_key = PathBuf::from("specific-app-icon");
        assert!(cache.prepare(icon_key.clone(), Some(&entry)));
        let (token, _) = cache.request_token(&icon_key).unwrap();
        cache.invalidate_paths(std::slice::from_ref(&entry.path));
        assert!(token.load(AtomicOrdering::Relaxed));
        assert!(!cache.complete(&icon_key, &token, image()));
        assert_eq!(cache.bytes, 0);
    }

    #[test]
    fn reconciling_a_listing_only_invalidates_changed_cached_sources() {
        let mut cache = ImageCache::new(16);
        let first = source("images/first.png", 1);
        let unchanged = source("images/second.png", 1);
        for entry in [&first, &unchanged] {
            assert!(cache.prepare(entry.path.clone(), Some(entry)));
            let (token, _) = cache.request_token(&entry.path).unwrap();
            assert!(cache.complete(&entry.path, &token, image()));
        }
        let modified = source("images/first.png", 2);
        let uncached = source("images/uncached.png", 1);
        let changed = cache.reconcile_sources(&[modified, unchanged.clone(), uncached.clone()]);
        assert_eq!(changed, HashSet::from([first.path.clone()]));
        assert!(!cache.contains_key(&first.path));
        assert!(cache.contains_key(&unchanged.path));
        assert!(
            !cache.contains_key(&uncached.path),
            "reconcile does not queue hidden images"
        );
        assert_eq!(cache.bytes, 4);
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
