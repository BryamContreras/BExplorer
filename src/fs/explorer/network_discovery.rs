//! Shared, bounded network queries. A UI deadline only stops that waiter;
//! the native query keeps its key until it actually returns.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, mpsc};
use std::thread;
use std::time::{Duration, Instant};

pub(super) const QUERY_TTL: Duration = Duration::from_secs(10);
pub(super) const HOST_WAIT_TIMEOUT: Duration = Duration::from_secs(65);
const HOST_WORKERS: usize = 8;
const HOST_QUEUE: usize = 64;
const MAX_QUERY_KEYS: usize = 256;

type Job = Box<dyn FnOnce() + Send>;

fn host_workers() -> &'static mpsc::SyncSender<Job> {
    static WORKERS: OnceLock<mpsc::SyncSender<Job>> = OnceLock::new();
    WORKERS.get_or_init(|| {
        let (sender, receiver) = mpsc::sync_channel::<Job>(HOST_QUEUE);
        let receiver = Arc::new(Mutex::new(receiver));
        for _ in 0..HOST_WORKERS {
            let receiver = Arc::clone(&receiver);
            thread::spawn(move || {
                loop {
                    let job = receiver
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .recv();
                    match job {
                        Ok(job) => job(),
                        Err(_) => return,
                    }
                }
            });
        }
        sender
    })
}

enum QueryState<T> {
    Pending,
    Running,
    Ready(T, Instant),
    Cancelled,
}

struct SharedQuery<T> {
    state: Mutex<QueryState<T>>,
    ready: Condvar,
    waiters: AtomicUsize,
}

struct Waiter<'a>(&'a AtomicUsize);

impl Drop for Waiter<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Release);
    }
}

pub(super) struct QueryCache<T> {
    queries: Mutex<HashMap<String, Arc<SharedQuery<T>>>>,
}

impl<T> Default for QueryCache<T> {
    fn default() -> Self {
        Self {
            queries: Mutex::new(HashMap::new()),
        }
    }
}

impl<T: Clone + Default + Send + 'static> QueryCache<T> {
    pub(super) fn has_pending(&self) -> bool {
        self.queries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .any(|query| {
                matches!(
                    *query
                        .state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner),
                    QueryState::Pending | QueryState::Running
                )
            })
    }

    pub(super) fn invalidate_completed(&self) {
        self.queries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|_, query| {
                matches!(
                    *query
                        .state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner),
                    QueryState::Pending | QueryState::Running
                )
            });
    }

    pub(super) fn run(
        &self,
        key: String,
        cancel: &AtomicBool,
        timeout: Duration,
        host: bool,
        operation: impl FnOnce() -> T + Send + 'static,
    ) -> Option<T> {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let mut queries = self
            .queries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        queries.retain(|_, query| {
            match &*query
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
            {
                QueryState::Ready(_, completed) => now.duration_since(*completed) < QUERY_TTL,
                QueryState::Cancelled => false,
                QueryState::Pending | QueryState::Running => true,
            }
        });
        let mut existing = None;
        if let Some(query) = queries.get(&key).cloned() {
            let state = query
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match &*state {
                QueryState::Ready(value, _) => return Some(value.clone()),
                QueryState::Pending | QueryState::Running => {
                    query.waiters.fetch_add(1, Ordering::AcqRel);
                    existing = Some(Arc::clone(&query));
                }
                QueryState::Cancelled => {
                    // A queued operation may have observed its last waiter
                    // leaving between pruning the map and subscribing here.
                    // Start a fresh request rather than losing this lookup.
                    queries.remove(&key);
                }
            }
        }
        let (query, launch) = if let Some(query) = existing {
            (query, false)
        } else {
            if queries.len() >= MAX_QUERY_KEYS {
                let oldest = queries
                    .iter()
                    .filter_map(|(key, query)| {
                        match &*query
                            .state
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                        {
                            QueryState::Ready(_, completed) => Some((key.clone(), *completed)),
                            _ => None,
                        }
                    })
                    .min_by_key(|(_, completed)| *completed)
                    .map(|(key, _)| key);
                if let Some(oldest) = oldest {
                    queries.remove(&oldest);
                } else {
                    return None;
                }
            }
            let query = Arc::new(SharedQuery {
                state: Mutex::new(QueryState::Pending),
                ready: Condvar::new(),
                waiters: AtomicUsize::new(1),
            });
            queries.insert(key, Arc::clone(&query));
            (query, true)
        };
        drop(queries);
        let _waiter = Waiter(&query.waiters);
        if launch {
            let running = Arc::clone(&query);
            let job = Box::new(move || {
                {
                    let mut state = running
                        .state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if running.waiters.load(Ordering::Acquire) == 0 {
                        *state = QueryState::Cancelled;
                        running.ready.notify_all();
                        return;
                    }
                    *state = QueryState::Running;
                }
                let value = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation))
                    .unwrap_or_default();
                *running
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) =
                    QueryState::Ready(value, Instant::now());
                running.ready.notify_all();
            });
            let submitted = if host {
                host_workers().try_send(job).is_ok()
            } else {
                thread::Builder::new()
                    .name("bexplorer-network-query".into())
                    .spawn(job)
                    .is_ok()
            };
            if !submitted {
                *query
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = QueryState::Cancelled;
                query.ready.notify_all();
            }
        }
        let deadline = Instant::now() + timeout;
        let mut state = query
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            match &*state {
                QueryState::Ready(value, _) => return Some(value.clone()),
                QueryState::Cancelled => return None,
                QueryState::Pending | QueryState::Running => {}
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            state = query
                .ready
                .wait_timeout(state, remaining.min(Duration::from_millis(50)))
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_timed_out_provider_keeps_its_key_until_the_real_operation_finishes() {
        let cache = Arc::new(QueryCache::<Vec<u8>>::default());
        let cancel = AtomicBool::new(false);
        let owner_cancel = Arc::new(AtomicBool::new(false));
        let (started_tx, started_rx) = mpsc::channel();
        let (finish_tx, finish_rx) = mpsc::channel();
        let owner_cache = Arc::clone(&cache);
        let worker_cancel = Arc::clone(&owner_cancel);
        let owner = thread::spawn(move || {
            owner_cache.run(
                "source".into(),
                &worker_cancel,
                Duration::from_secs(5),
                false,
                move || {
                    started_tx.send(()).unwrap();
                    finish_rx.recv().unwrap();
                    vec![42]
                },
            )
        });
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(cache.has_pending());
        cache.invalidate_completed();
        assert_eq!(
            cache.run(
                "source".into(),
                &cancel,
                Duration::from_millis(10),
                false,
                || panic!("duplicate native provider")
            ),
            None
        );
        owner_cancel.store(true, Ordering::Release);
        assert_eq!(owner.join().unwrap(), None);
        assert!(
            cache.has_pending(),
            "a timeout or cancellation is not native completion"
        );
        finish_tx.send(()).unwrap();
        assert_eq!(
            cache.run(
                "source".into(),
                &cancel,
                Duration::from_secs(1),
                false,
                || panic!("must reuse result")
            ),
            Some(vec![42])
        );
        assert!(!cache.has_pending());
        assert_eq!(
            cache.run(
                "source".into(),
                &cancel,
                Duration::from_secs(1),
                false,
                || panic!("TTL should reuse result")
            ),
            Some(vec![42])
        );
        cache.invalidate_completed();
        assert_eq!(
            cache.run(
                "source".into(),
                &cancel,
                Duration::from_secs(1),
                false,
                || vec![7]
            ),
            Some(vec![7])
        );
    }

    #[test]
    fn expired_completed_queries_run_again_without_waiting_for_a_real_ttl() {
        let cache = QueryCache::<Vec<u8>>::default();
        let cancel = AtomicBool::new(false);
        assert_eq!(
            cache.run(
                "source".into(),
                &cancel,
                Duration::from_secs(1),
                false,
                || vec![1]
            ),
            Some(vec![1])
        );
        let query = Arc::clone(cache.queries.lock().unwrap().get("source").unwrap());
        *query.state.lock().unwrap() = QueryState::Ready(vec![1], Instant::now() - QUERY_TTL);
        assert_eq!(
            cache.run(
                "source".into(),
                &cancel,
                Duration::from_secs(1),
                false,
                || vec![2]
            ),
            Some(vec![2])
        );
    }

    #[test]
    fn cancelled_waiters_do_not_start_new_queries() {
        let cache = QueryCache::<Vec<u8>>::default();
        assert_eq!(
            cache.run(
                "source".into(),
                &AtomicBool::new(true),
                Duration::from_secs(1),
                false,
                || panic!("cancelled query started")
            ),
            None
        );
        assert!(cache.queries.lock().unwrap().is_empty());
    }

    #[test]
    fn a_new_waiter_receives_the_native_result_when_the_original_waiter_is_cancelled() {
        let cache = Arc::new(QueryCache::<Vec<u8>>::default());
        let owner_cancel = Arc::new(AtomicBool::new(false));
        let (started_tx, started_rx) = mpsc::channel();
        let (finish_tx, finish_rx) = mpsc::channel();
        let owner_cache = Arc::clone(&cache);
        let cancel = Arc::clone(&owner_cancel);
        let owner = thread::spawn(move || {
            owner_cache.run(
                "source".into(),
                &cancel,
                Duration::from_secs(5),
                false,
                move || {
                    started_tx.send(()).unwrap();
                    finish_rx.recv().unwrap();
                    vec![42]
                },
            )
        });
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let new_cache = Arc::clone(&cache);
        let newer = thread::spawn(move || {
            new_cache.run(
                "source".into(),
                &AtomicBool::new(false),
                Duration::from_secs(5),
                false,
                || panic!("the native source must remain shared"),
            )
        });
        let query = Arc::clone(cache.queries.lock().unwrap().get("source").unwrap());
        let deadline = Instant::now() + Duration::from_secs(2);
        while query.waiters.load(Ordering::Acquire) != 2 && Instant::now() < deadline {
            thread::yield_now();
        }
        assert_eq!(query.waiters.load(Ordering::Acquire), 2);
        owner_cancel.store(true, Ordering::Release);
        assert_eq!(owner.join().unwrap(), None);
        finish_tx.send(()).unwrap();
        assert_eq!(newer.join().unwrap(), Some(vec![42]));
    }

    #[test]
    fn host_queries_share_an_eight_worker_limit() {
        let cache = Arc::new(QueryCache::<Vec<u8>>::default());
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let (release_tx, release_rx) = mpsc::channel();
        let release = Arc::new(Mutex::new(release_rx));
        let mut callers = Vec::new();
        for index in 0..24 {
            let cache = Arc::clone(&cache);
            let active = Arc::clone(&active);
            let peak = Arc::clone(&peak);
            let release = Arc::clone(&release);
            callers.push(thread::spawn(move || {
                cache.run(
                    index.to_string(),
                    &AtomicBool::new(false),
                    Duration::from_secs(5),
                    true,
                    move || {
                        let count = active.fetch_add(1, Ordering::AcqRel) + 1;
                        peak.fetch_max(count, Ordering::AcqRel);
                        release.lock().unwrap().recv().unwrap();
                        active.fetch_sub(1, Ordering::AcqRel);
                        vec![1]
                    },
                )
            }));
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while active.load(Ordering::Acquire) < HOST_WORKERS && Instant::now() < deadline {
            thread::yield_now();
        }
        assert_eq!(active.load(Ordering::Acquire), HOST_WORKERS);
        for _ in 0..24 {
            release_tx.send(()).unwrap();
        }
        for caller in callers {
            assert_eq!(caller.join().unwrap(), Some(vec![1]));
        }
        assert_eq!(peak.load(Ordering::Acquire), HOST_WORKERS);
    }
}
