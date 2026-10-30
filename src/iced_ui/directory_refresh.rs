use super::*;

#[derive(Debug)]
pub(super) struct DirectoryLoadToken {
    pub(super) cancel: Arc<AtomicBool>,
    abort: iced::futures::future::AbortHandle,
}

impl DirectoryLoadToken {
    pub(super) fn new() -> (Self, iced::futures::future::AbortRegistration) {
        let (abort, registration) = iced::futures::future::AbortHandle::new_pair();
        (
            Self {
                cancel: Arc::new(AtomicBool::new(false)),
                abort,
            },
            registration,
        )
    }
}

impl Drop for DirectoryLoadToken {
    fn drop(&mut self) {
        self.cancel.store(true, AtomicOrdering::Relaxed);
        self.abort.abort();
    }
}

#[derive(Default)]
pub(super) struct DirectoryRefreshes {
    sequence: u64,
    pending: HashSet<PathBuf>,
    active: HashMap<PathBuf, (u64, DirectoryLoadToken)>,
}

impl DirectoryRefreshes {
    pub(super) fn queue(&mut self, path: PathBuf) {
        self.pending.insert(path);
    }

    pub(super) fn ready(&self) -> Vec<PathBuf> {
        self.pending
            .iter()
            .filter(|path| !self.active.contains_key(*path))
            .cloned()
            .collect()
    }

    pub(super) fn start(
        &mut self,
        path: PathBuf,
    ) -> Option<(
        u64,
        Arc<AtomicBool>,
        iced::futures::future::AbortRegistration,
    )> {
        if self.active.contains_key(&path) {
            return None;
        }
        self.pending.remove(&path);
        self.sequence = self.sequence.wrapping_add(1);
        let (token, registration) = DirectoryLoadToken::new();
        let cancel = Arc::clone(&token.cancel);
        self.active.insert(path, (self.sequence, token));
        Some((self.sequence, cancel, registration))
    }

    pub(super) fn finish(&mut self, path: &Path, ticket: u64) -> bool {
        if self.active.get(path).is_none_or(|(id, _)| *id != ticket) {
            return false;
        }
        self.active.remove(path);
        true
    }

    pub(super) fn cancel(&mut self, path: &Path) -> bool {
        let active = self.active.remove(path).is_some();
        self.pending.remove(path) || active
    }

    pub(super) fn retain(&mut self, visible: &HashSet<PathBuf>) {
        self.active.retain(|path, _| visible.contains(path));
        self.pending.retain(|path| visible.contains(path));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changes_during_a_load_coalesce_into_one_followup() {
        let mut loads = DirectoryRefreshes::default();
        let path = PathBuf::from("folder");
        loads.queue(path.clone());
        let (first, _, _) = loads.start(path.clone()).unwrap();
        for _ in 0..100 {
            loads.queue(path.clone());
        }
        assert!(loads.ready().is_empty());
        assert!(loads.start(path.clone()).is_none());
        assert!(loads.finish(&path, first));
        assert_eq!(loads.ready(), vec![path.clone()]);
        let (second, _, _) = loads.start(path.clone()).unwrap();
        assert!(!loads.finish(&path, first));
        assert!(loads.ready().is_empty());
        assert!(loads.finish(&path, second));
        assert!(loads.ready().is_empty());
    }

    #[test]
    fn navigation_cancels_stale_work_and_keeps_the_other_pane() {
        let mut loads = DirectoryRefreshes::default();
        let first = PathBuf::from("old");
        let second = PathBuf::from("other-pane");
        let (_, canceled, _) = loads.start(first.clone()).unwrap();
        let (ticket, kept, _) = loads.start(second.clone()).unwrap();
        loads.queue(first);
        loads.retain(&HashSet::from([second.clone()]));
        assert!(canceled.load(AtomicOrdering::Relaxed));
        assert!(!kept.load(AtomicOrdering::Relaxed));
        assert!(loads.finish(&second, ticket));
    }
}
