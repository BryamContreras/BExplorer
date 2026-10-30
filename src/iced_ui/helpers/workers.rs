use std::sync::{Arc, Mutex};

use iced::futures::{SinkExt, StreamExt, channel::mpsc, channel::oneshot, lock};

type Work = Box<dyn FnOnce() + Send>;

/// Limits blocking work without occupying the async executor while queued.
pub(super) struct BlockingPool {
    sender: lock::Mutex<mpsc::Sender<Work>>,
}

impl BlockingPool {
    pub(super) fn new(name: &'static str, workers: usize, capacity: usize) -> Self {
        let (sender, receiver) = mpsc::channel::<Work>(capacity);
        let receiver = Arc::new(Mutex::new(receiver));
        for index in 0..workers {
            let receiver = Arc::clone(&receiver);
            std::thread::Builder::new()
                .name(format!("bexplorer-{name}-{index}"))
                .spawn(move || {
                    loop {
                        let work = iced::futures::executor::block_on(
                            receiver
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .next(),
                        );
                        match work {
                            Some(work) => work(),
                            None => break,
                        }
                    }
                })
                .expect("could not start a blocking worker");
        }
        Self {
            sender: lock::Mutex::new(sender),
        }
    }

    pub(super) async fn run<T: Send + 'static>(
        &self,
        operation: impl FnOnce() -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .lock()
            .await
            .send(Box::new(move || {
                if sender.is_canceled() {
                    return;
                }
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation))
                    .unwrap_or_else(|_| Err("File operation worker panicked".into()));
                let _ = sender.send(result);
            }))
            .await
            .map_err(|_| "File operation worker stopped".to_owned())?;
        receiver
            .await
            .unwrap_or_else(|_| Err("File operation worker stopped unexpectedly".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[test]
    fn a_burst_keeps_blocking_work_within_the_worker_limit() {
        let pool = BlockingPool::new("test-burst", 2, 4);
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let jobs = (0..12).map(|index| {
            let active = Arc::clone(&active);
            let peak = Arc::clone(&peak);
            pool.run(move || {
                let count = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(count, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(5));
                active.fetch_sub(1, Ordering::SeqCst);
                Ok(index)
            })
        });
        let results = iced::futures::executor::block_on(iced::futures::future::join_all(jobs));
        assert_eq!(results, (0..12).map(Ok).collect::<Vec<_>>());
        assert!(peak.load(Ordering::SeqCst) <= 2);
    }

    #[test]
    fn a_panicking_job_does_not_remove_the_worker() {
        let pool = BlockingPool::new("test-panic", 1, 2);
        assert!(
            iced::futures::executor::block_on(
                pool.run(|| -> Result<(), String> { panic!("fixture") })
            )
            .is_err()
        );
        assert_eq!(iced::futures::executor::block_on(pool.run(|| Ok(7))), Ok(7));
    }

    #[test]
    fn canceled_queued_work_is_skipped() {
        use iced::futures::FutureExt;
        let pool = BlockingPool::new("test-cancel", 1, 4);
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let first = pool.run(move || {
            let _ = started_tx.send(());
            release_rx
                .recv_timeout(Duration::from_secs(2))
                .map_err(|error| error.to_string())
        });
        let mut first = Box::pin(first);
        assert!(first.as_mut().now_or_never().is_none());
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let ran = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&ran);
        let mut canceled = Box::pin(pool.run(move || {
            observed.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));
        assert!(canceled.as_mut().now_or_never().is_none());
        drop(canceled);
        release_tx.send(()).unwrap();
        iced::futures::executor::block_on(first).unwrap();
        iced::futures::executor::block_on(pool.run(|| Ok(()))).unwrap();
        assert_eq!(ran.load(Ordering::SeqCst), 0);
    }
}
