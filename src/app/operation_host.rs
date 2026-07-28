use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, SocketAddrV4, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::utils::{atomic_file, paths};

const ACTIVATION_PROTOCOL_VERSION: u8 = 1;
const ACTIVATION_ACK: &[u8] = b"BEXPLORER-ACTIVATED\n";
const ACTIVATION_LIMIT: u64 = 64 * 1024;
const ACTIVATION_TIMEOUT: Duration = Duration::from_millis(900);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct OperationHostMarker {
    version: u8,
    process_id: u32,
    port: u16,
    token: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct OperationHostActivation {
    version: u8,
    token: String,
    path: Option<PathBuf>,
}

pub struct OperationHostServer {
    marker: OperationHostMarker,
    marker_path: PathBuf,
    address: SocketAddr,
    receiver: Receiver<Option<PathBuf>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    published: bool,
}

impl OperationHostServer {
    pub fn new() -> Result<Self, String> {
        let directory = paths::operation_hosts_dir().map_err(|error| error.to_string())?;
        Self::bind_in(directory)
    }

    fn bind_in(directory: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
            .map_err(|error| error.to_string())?;
        let address = listener.local_addr().map_err(|error| error.to_string())?;
        let process_id = std::process::id();
        let token = activation_token(process_id, address.port());
        let marker = OperationHostMarker {
            version: ACTIVATION_PROTOCOL_VERSION,
            process_id,
            port: address.port(),
            token: token.clone(),
        };
        let marker_path = directory.join(format!("host-{process_id}-{}.json", address.port()));
        let (sender, receiver) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let thread = thread::Builder::new()
            .name("bexplorer-operation-host".into())
            .spawn(move || operation_host_loop(listener, token, sender, worker_stop))
            .map_err(|error| error.to_string())?;

        Ok(Self {
            marker,
            marker_path,
            address,
            receiver,
            stop,
            thread: Some(thread),
            published: false,
        })
    }

    pub fn publish(&mut self) -> Result<(), String> {
        if self.published {
            return Ok(());
        }
        let bytes = serde_json::to_vec(&self.marker).map_err(|error| error.to_string())?;
        atomic_file::write(&self.marker_path, &bytes).map_err(|error| error.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            if let Err(error) =
                fs::set_permissions(&self.marker_path, fs::Permissions::from_mode(0o600))
            {
                let _ = fs::remove_file(&self.marker_path);
                return Err(error.to_string());
            }
        }
        self.published = true;
        Ok(())
    }

    pub fn drain_activations(&self) -> Vec<Option<PathBuf>> {
        self.receiver.try_iter().collect()
    }

    fn unpublish(&mut self) {
        if !self.published {
            return;
        }
        let owns_marker = fs::read(&self.marker_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<OperationHostMarker>(&bytes).ok())
            .is_some_and(|marker| marker == self.marker);
        if owns_marker {
            let _ = fs::remove_file(&self.marker_path);
        }
        self.published = false;
    }
}

impl Drop for OperationHostServer {
    fn drop(&mut self) {
        self.unpublish();
        self.stop.store(true, Ordering::Release);
        let _ = TcpStream::connect_timeout(&self.address, Duration::from_millis(100));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub fn try_activate_existing(path: Option<&Path>) -> bool {
    let Ok(directory) = paths::operation_hosts_dir() else {
        return false;
    };
    try_activate_in(&directory, path)
}

fn try_activate_in(directory: &Path, path: Option<&Path>) -> bool {
    let Ok(entries) = fs::read_dir(directory) else {
        return false;
    };
    let mut markers = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("host-") && name.ends_with(".json"))
        })
        .map(|entry| {
            let modified = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .unwrap_or(UNIX_EPOCH);
            (modified, entry.path())
        })
        .collect::<Vec<_>>();
    markers.sort_by_key(|marker| std::cmp::Reverse(marker.0));

    for (_, marker_path) in markers {
        let marker = fs::read(&marker_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<OperationHostMarker>(&bytes).ok());
        let Some(marker) = marker
            .filter(|marker| marker.version == ACTIVATION_PROTOCOL_VERSION && marker.port != 0)
        else {
            let _ = fs::remove_file(marker_path);
            continue;
        };
        if activate_marker(&marker, path) {
            return true;
        }
        let _ = fs::remove_file(marker_path);
    }
    false
}

fn activate_marker(marker: &OperationHostMarker, path: Option<&Path>) -> bool {
    let address = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, marker.port));
    let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_millis(250)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(ACTIVATION_TIMEOUT));
    let _ = stream.set_write_timeout(Some(ACTIVATION_TIMEOUT));
    let activation = OperationHostActivation {
        version: ACTIVATION_PROTOCOL_VERSION,
        token: marker.token.clone(),
        path: path.map(Path::to_path_buf),
    };
    let Ok(bytes) = serde_json::to_vec(&activation) else {
        return false;
    };
    if stream.write_all(&bytes).is_err() || stream.shutdown(Shutdown::Write).is_err() {
        return false;
    }
    let mut response = Vec::new();
    stream
        .take(ACTIVATION_ACK.len() as u64 + 1)
        .read_to_end(&mut response)
        .is_ok()
        && response == ACTIVATION_ACK
}

fn operation_host_loop(
    listener: TcpListener,
    token: String,
    sender: Sender<Option<PathBuf>>,
    stop: Arc<AtomicBool>,
) {
    while !stop.load(Ordering::Acquire) {
        let Ok((stream, address)) = listener.accept() else {
            if !stop.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(20));
            }
            continue;
        };
        if stop.load(Ordering::Acquire) {
            break;
        }
        if address.ip().is_loopback() {
            handle_activation(stream, &token, &sender);
        }
    }
}

fn handle_activation(mut stream: TcpStream, token: &str, sender: &Sender<Option<PathBuf>>) {
    let _ = stream.set_read_timeout(Some(ACTIVATION_TIMEOUT));
    let _ = stream.set_write_timeout(Some(ACTIVATION_TIMEOUT));
    let mut bytes = Vec::new();
    if Read::by_ref(&mut stream)
        .take(ACTIVATION_LIMIT + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() as u64 > ACTIVATION_LIMIT
    {
        return;
    }
    let Ok(activation) = serde_json::from_slice::<OperationHostActivation>(&bytes) else {
        return;
    };
    if activation.version != ACTIVATION_PROTOCOL_VERSION || activation.token != token {
        return;
    }
    if sender.send(activation.path).is_ok() {
        let _ = stream.write_all(ACTIVATION_ACK);
        let _ = stream.flush();
    }
}

fn activation_token(process_id: u32, port: u16) -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{timestamp:032x}-{process_id:08x}-{port:04x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_directory(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "bexplorer-operation-host-{label}-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).expect("create operation host test directory");
        directory
    }

    #[test]
    fn published_host_receives_a_reactivation_path() {
        let directory = temporary_directory("activation");
        let mut server =
            OperationHostServer::bind_in(directory.clone()).expect("start operation host");
        server.publish().expect("publish operation host");
        let requested = directory.join("Vídeos");

        assert!(try_activate_in(&directory, Some(&requested)));
        assert_eq!(server.drain_activations(), vec![Some(requested)]);

        drop(server);
        assert_eq!(
            fs::read_dir(&directory)
                .expect("read operation host directory")
                .count(),
            0
        );
        fs::remove_dir_all(directory).expect("cleanup operation host test directory");
    }

    #[test]
    fn unpublished_host_is_not_discoverable() {
        let directory = temporary_directory("unpublished");
        let server = OperationHostServer::bind_in(directory.clone()).expect("start operation host");

        assert!(!try_activate_in(&directory, None));

        drop(server);
        fs::remove_dir_all(directory).expect("cleanup operation host test directory");
    }
}
