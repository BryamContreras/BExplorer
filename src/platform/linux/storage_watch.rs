use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread;
use std::time::Duration;

use zbus::blocking::{Connection, Proxy};

pub fn storage_change_receiver() -> Receiver<()> {
    let (sender, receiver) = mpsc::sync_channel(1);
    spawn_udev_listener(sender.clone());
    spawn_gio_volume_listener(sender.clone());
    spawn_mount_snapshot_listener(sender.clone());
    spawn_kio_mtp_listener(sender);
    receiver
}

fn spawn_udev_listener(sender: SyncSender<()>) {
    thread::spawn(move || {
        let Ok(mut child) = Command::new("udevadm")
            .args(["monitor", "--udev"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        else {
            return;
        };
        let Some(stdout) = child.stdout.take() else {
            return;
        };

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
    });
}

fn spawn_gio_volume_listener(sender: SyncSender<()>) {
    thread::spawn(move || {
        let Ok(mut child) = Command::new("gio")
            .args(["mount", "--monitor", "--detail"])
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        else {
            return;
        };
        let Some(stdout) = child.stdout.take() else {
            return;
        };

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
    });
}

fn spawn_mount_snapshot_listener(sender: SyncSender<()>) {
    thread::spawn(move || {
        let mut previous = storage_snapshot();
        loop {
            thread::sleep(Duration::from_secs(2));
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
                thread::sleep(Duration::from_secs(2));
                continue;
            };
            let Ok(proxy) = Proxy::new(
                &connection,
                "org.kde.kmtpd5",
                "/modules/kmtpd",
                "org.kde.kmtp.Daemon",
            ) else {
                thread::sleep(Duration::from_secs(2));
                continue;
            };
            let Ok(signals) = proxy.receive_signal("devicesChanged") else {
                thread::sleep(Duration::from_secs(2));
                continue;
            };

            for _signal in signals {
                match sender.try_send(()) {
                    Ok(()) | Err(mpsc::TrySendError::Full(_)) => {}
                    Err(mpsc::TrySendError::Disconnected(_)) => return,
                }
            }
            thread::sleep(Duration::from_secs(1));
        }
    });
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
