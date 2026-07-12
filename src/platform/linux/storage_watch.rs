use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread;
use std::time::Duration;

pub fn storage_change_receiver() -> Receiver<()> {
    let (sender, receiver) = mpsc::sync_channel(1);
    spawn_udev_listener(sender.clone());
    spawn_mount_snapshot_listener(sender);
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
