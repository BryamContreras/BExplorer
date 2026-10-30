use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use crate::utils::process::terminate_with_parent;
use zbus::blocking::{Connection, MessageIterator};
use zbus::{MatchRule, message::Type};

const KIO_MTP_SERVICE: &str = "org.kde.kmtpd5";
static STORAGE_RECEIVER: OnceLock<Mutex<Option<Receiver<()>>>> = OnceLock::new();

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct SessionMonitors {
    available: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MonitorPlan {
    udev: bool,
    gio: bool,
    kio: bool,
}

#[derive(Default)]
struct MonitorHealth {
    udev: AtomicBool,
    gio: AtomicBool,
}

struct ActiveMonitor<'a>(&'a AtomicBool);

impl<'a> ActiveMonitor<'a> {
    fn new(active: &'a AtomicBool) -> Self {
        active.store(true, Ordering::Release);
        Self(active)
    }
}

impl Drop for ActiveMonitor<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

pub fn storage_change_receiver() -> Receiver<()> {
    STORAGE_RECEIVER
        .get_or_init(|| Mutex::new(Some(start_storage_monitors())))
        .lock()
        .ok()
        .and_then(|mut receiver| receiver.take())
        .unwrap_or_else(|| {
            let (_sender, receiver) = mpsc::sync_channel(1);
            receiver
        })
}

fn start_storage_monitors() -> Receiver<()> {
    let (sender, receiver) = mpsc::sync_channel(1);
    let plan = monitor_plan(
        command_available("udevadm"),
        command_available("gio"),
        session_monitors(),
    );
    let health = Arc::new(MonitorHealth::default());
    if plan.udev {
        spawn_udev_listener(sender.clone(), Arc::clone(&health));
    }
    if plan.gio {
        spawn_gio_volume_listener(sender.clone(), Arc::clone(&health));
    }
    if plan.kio {
        spawn_kio_mtp_listener(sender.clone());
    }
    // Mounts created manually need not emit udev or desktop signals. Keep a
    // lightweight snapshot as a backstop, at a slower rate while those event
    // listeners are healthy, rather than removing this coverage entirely.
    spawn_mount_snapshot_listener(sender, health);
    receiver
}

fn command_available(program: &str) -> bool {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .any(|directory| directory.join(program).is_file())
}

fn monitor_plan(udev: bool, gio: bool, session: SessionMonitors) -> MonitorPlan {
    MonitorPlan {
        udev,
        // GIO also sees virtual mounts without FUSE directories or /proc
        // entries. Keep it on KDE and unknown desktops too: GVfs can start
        // later, so the current desktop/owner cannot exclude this source.
        gio: gio && session.available,
        // A raw match only subscribes on the bus. It does not launch KIO,
        // and also covers an owner that appears after BExplorer starts.
        kio: session.available,
    }
}

fn session_monitors() -> SessionMonitors {
    SessionMonitors {
        available: Connection::session().is_ok(),
    }
}

fn snapshot_interval(udev: bool, gio: bool) -> Duration {
    match (udev, gio) {
        (true, true) => Duration::from_secs(10),
        (true, false) | (false, true) => Duration::from_secs(5),
        (false, false) => Duration::from_secs(2),
    }
}

fn spawn_udev_listener(sender: SyncSender<()>, health: Arc<MonitorHealth>) {
    thread::spawn(move || {
        let mut command = Command::new("udevadm");
        command
            .args(["monitor", "--udev"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        terminate_with_parent(&mut command);
        let Ok(mut child) = command.spawn() else {
            return;
        };
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return;
        };
        let _active = ActiveMonitor::new(&health.udev);

        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if !line.starts_with("UDEV") || !(line.ends_with("(block)") || line.ends_with("(usb)"))
            {
                continue;
            }
            match sender.try_send(()) {
                Ok(()) | Err(mpsc::TrySendError::Full(_)) => {}
                Err(mpsc::TrySendError::Disconnected(_)) => break,
            }
        }
        let _ = child.kill();
        let _ = child.wait();
    });
}

fn spawn_gio_volume_listener(sender: SyncSender<()>, health: Arc<MonitorHealth>) {
    thread::spawn(move || {
        let mut command = Command::new("gio");
        command
            .args(["mount", "--monitor", "--detail"])
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        terminate_with_parent(&mut command);
        let Ok(mut child) = command.spawn() else {
            return;
        };
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return;
        };
        let _active = ActiveMonitor::new(&health.gio);

        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if line.trim().is_empty() {
                continue;
            }
            match sender.try_send(()) {
                Ok(()) | Err(mpsc::TrySendError::Full(_)) => {}
                Err(mpsc::TrySendError::Disconnected(_)) => break,
            }
        }
        let _ = child.kill();
        let _ = child.wait();
    });
}

fn spawn_mount_snapshot_listener(sender: SyncSender<()>, health: Arc<MonitorHealth>) {
    thread::spawn(move || {
        let mut previous = storage_snapshot();
        loop {
            thread::sleep(snapshot_interval(
                health.udev.load(Ordering::Acquire),
                health.gio.load(Ordering::Acquire),
            ));
            let current = storage_snapshot();
            if current == previous {
                continue;
            }
            previous = current;
            match sender.try_send(()) {
                Ok(()) | Err(mpsc::TrySendError::Full(_)) => {}
                Err(mpsc::TrySendError::Disconnected(_)) => break,
            }
        }
    });
}

fn spawn_kio_mtp_listener(sender: SyncSender<()>) {
    thread::spawn(move || {
        loop {
            let Ok(connection) = Connection::session() else {
                thread::sleep(Duration::from_secs(10));
                continue;
            };
            // A bus signal match also works before the service acquires its
            // name, and after it restarts. It neither activates KIO nor polls
            // for the service every two seconds on desktops without it.
            let Ok(rule) = kio_devices_changed_rule() else {
                return;
            };
            let Ok(signals) = MessageIterator::for_match_rule(rule, &connection, Some(4)) else {
                thread::sleep(Duration::from_secs(10));
                continue;
            };

            for signal in signals {
                if signal.is_err() {
                    break;
                }
                match sender.try_send(()) {
                    Ok(()) | Err(mpsc::TrySendError::Full(_)) => {}
                    Err(mpsc::TrySendError::Disconnected(_)) => return,
                }
            }
            thread::sleep(Duration::from_secs(10));
        }
    });
}

fn kio_devices_changed_rule() -> zbus::Result<MatchRule<'static>> {
    Ok(MatchRule::builder()
        .msg_type(Type::Signal)
        .sender(KIO_MTP_SERVICE)?
        .path("/modules/kmtpd")?
        .interface("org.kde.kmtp.Daemon")?
        .member("devicesChanged")?
        .build())
}

fn storage_snapshot() -> (Vec<u8>, Vec<PathBuf>) {
    let mountinfo = fs::read("/proc/self/mountinfo").unwrap_or_default();
    let mut portable_mounts = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .and_then(|runtime| fs::read_dir(runtime.join("gvfs")).ok())
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    portable_mounts.sort();
    (mountinfo, portable_mounts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_keep_physical_gvfs_and_nonactivating_kio_notifications() {
        let plan = monitor_plan(true, true, SessionMonitors { available: true });
        assert_eq!(
            plan,
            MonitorPlan {
                udev: true,
                gio: true,
                kio: true
            }
        );
    }

    #[test]
    fn missing_helpers_do_not_disable_other_complementary_sources() {
        let session = SessionMonitors { available: true };
        assert_eq!(
            monitor_plan(true, false, session),
            MonitorPlan {
                udev: true,
                gio: false,
                kio: true
            }
        );
        assert_eq!(
            monitor_plan(false, true, session),
            MonitorPlan {
                udev: false,
                gio: true,
                kio: true
            }
        );
    }

    #[test]
    fn absent_commands_or_session_bus_keep_only_the_snapshot_backstop() {
        assert_eq!(
            monitor_plan(false, false, SessionMonitors::default()),
            MonitorPlan {
                udev: false,
                gio: false,
                kio: false
            }
        );
        assert_eq!(snapshot_interval(false, false), Duration::from_secs(2));
    }

    #[test]
    fn healthy_event_listeners_reduce_snapshot_wakeups_and_failure_restores_the_fallback() {
        let health = MonitorHealth::default();
        let active = ActiveMonitor::new(&health.udev);
        assert_eq!(
            snapshot_interval(health.udev.load(Ordering::Acquire), false),
            Duration::from_secs(5)
        );
        {
            let _gio = ActiveMonitor::new(&health.gio);
            assert_eq!(
                snapshot_interval(
                    health.udev.load(Ordering::Acquire),
                    health.gio.load(Ordering::Acquire)
                ),
                Duration::from_secs(10)
            );
        }
        drop(active);
        assert_eq!(
            snapshot_interval(
                health.udev.load(Ordering::Acquire),
                health.gio.load(Ordering::Acquire)
            ),
            Duration::from_secs(2)
        );
    }

    #[test]
    fn kio_subscription_is_a_specific_bus_signal_match() {
        let rule = kio_devices_changed_rule().unwrap().to_string();
        for field in [
            "type='signal'",
            "sender='org.kde.kmtpd5'",
            "interface='org.kde.kmtp.Daemon'",
            "member='devicesChanged'",
            "path='/modules/kmtpd'",
        ] {
            assert!(rule.contains(field), "Missing signal match field: {field}");
        }
    }
}
