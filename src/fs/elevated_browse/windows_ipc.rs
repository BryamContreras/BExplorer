use std::io::ErrorKind;
use std::net::{Ipv4Addr, Shutdown, SocketAddrV4, TcpListener, TcpStream};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;

use super::*;

const TOKEN_BYTES: usize = 64;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(3);

pub(super) fn launch_helper(request: &[u8]) -> Result<Option<Vec<BrowseEntry>>> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    listener.set_nonblocking(true)?;
    let endpoint = listener.local_addr()?;
    let client_token = crate::fs::transfer_queue::elevated_progress_token()?;
    let server_token = crate::fs::transfer_queue::elevated_progress_token()?;
    let stop = Arc::new(AtomicBool::new(false));
    let listener_stop = Arc::clone(&stop);
    let expected_client = client_token.clone();
    let expected_server = server_token.clone();
    let request = request.to_vec();
    let receiver = thread::Builder::new()
        .name("elevated-browse-ipc".into())
        .spawn(move || {
            exchange(
                listener,
                expected_client.as_bytes(),
                expected_server.as_bytes(),
                &request,
                &listener_stop,
            )
        })?;
    let status = crate::platform::shell::run_elevated_current_exe(&[
        OsString::from(HELPER_ARGUMENT),
        OsString::from(endpoint.to_string()),
        OsString::from(client_token),
        OsString::from(server_token),
    ]);
    stop.store(true, Ordering::Release);
    let received = receiver.join().map_err(|_| {
        BExplorerError::Shell("Could not receive the administrator folder listing".into())
    })?;
    match status {
        Err(BExplorerError::Io(error)) if error.kind() == ErrorKind::Interrupted => Ok(None),
        Err(error) => Err(error),
        Ok(code) => received?.map(Some).ok_or_else(|| {
            BExplorerError::Shell(format!(
                "The administrator folder listing helper exited with code {code} without a response"
            ))
        }),
    }
}

fn exchange(
    listener: TcpListener,
    client_token: &[u8],
    server_token: &[u8],
    request: &[u8],
    stop: &AtomicBool,
) -> Result<Option<Vec<BrowseEntry>>> {
    loop {
        match listener.accept() {
            Ok((mut stream, address)) if address.ip().is_loopback() => {
                // Winsock inherits FIONBIO from the listener. The selected
                // connection uses framed blocking reads on this worker.
                stream.set_nonblocking(false)?;
                stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
                let mut provided_token = [0_u8; TOKEN_BYTES];
                if stream.read_exact(&mut provided_token).is_err() || provided_token != client_token
                {
                    if stop.load(Ordering::Acquire) {
                        return Ok(None);
                    }
                    continue;
                }
                stream.set_read_timeout(None)?;
                stream.set_write_timeout(Some(HANDSHAKE_TIMEOUT))?;
                stream.write_all(server_token)?;
                stream.write_all(request)?;
                stream.shutdown(Shutdown::Write)?;
                return response_from_reader(stream).map(Some);
            }
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                if stop.load(Ordering::Acquire) {
                    return Ok(None);
                }
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

pub(super) fn run_helper_with_args(mut args: impl Iterator<Item = OsString>) -> i32 {
    let Some(endpoint) = args
        .next()
        .and_then(|value| value.to_str()?.parse::<SocketAddrV4>().ok())
        .filter(|value| *value.ip() == Ipv4Addr::LOCALHOST && value.port() != 0)
    else {
        return 2;
    };
    let (Some(client_token), Some(server_token)) =
        (parse_token(args.next()), parse_token(args.next()))
    else {
        return 2;
    };
    if args.next().is_some() {
        return 2;
    }
    let result = (|| -> std::io::Result<i32> {
        let mut stream = TcpStream::connect_timeout(&endpoint.into(), HANDSHAKE_TIMEOUT)?;
        stream.set_write_timeout(Some(HANDSHAKE_TIMEOUT))?;
        stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
        stream.write_all(client_token.as_bytes())?;
        let mut provided_token = [0_u8; TOKEN_BYTES];
        stream.read_exact(&mut provided_token)?;
        if provided_token != server_token.as_bytes() {
            return Ok(2);
        }
        let reader = stream.try_clone()?;
        Ok(run_helper(reader, stream))
    })();
    result.unwrap_or(2)
}

fn parse_token(value: Option<OsString>) -> Option<String> {
    value
        .and_then(|value| value.into_string().ok())
        .filter(|token| {
            token.len() == TOKEN_BYTES && token.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authenticates_before_sending_the_folder_request() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = listener.local_addr().unwrap();
        let folder = std::env::temp_dir();
        let request = serde_json::to_vec(&BrowseRequest {
            directory: native_units(folder.as_os_str()),
            show_hidden: true,
        })
        .unwrap();
        let token = "a".repeat(TOKEN_BYTES);
        let receiver_token = token.clone();
        let server_token = "c".repeat(TOKEN_BYTES);
        let expected_server = server_token.clone();
        let receiver = thread::spawn(move || {
            exchange(
                listener,
                receiver_token.as_bytes(),
                expected_server.as_bytes(),
                &request,
                &AtomicBool::new(false),
            )
        });

        let mut foreign = TcpStream::connect(endpoint).unwrap();
        foreign.set_read_timeout(Some(HANDSHAKE_TIMEOUT)).unwrap();
        foreign.write_all(&[b'b'; TOKEN_BYTES]).unwrap();
        let mut leaked = [0_u8; 1];
        match foreign.read(&mut leaked) {
            Ok(read) => assert_eq!(read, 0),
            Err(error) => assert!(matches!(
                error.kind(),
                ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted
            )),
        }

        let code = run_helper_with_args(
            [
                endpoint.to_string().into(),
                token.into(),
                server_token.into(),
            ]
            .into_iter(),
        );
        assert_eq!(code, 0);
        assert!(receiver.join().unwrap().unwrap().is_some());
    }

    #[test]
    fn rejects_remote_addresses_and_bad_tokens_without_connecting() {
        for (address, token) in [
            ("192.0.2.1:1201", "a".repeat(TOKEN_BYTES)),
            ("127.0.0.1:1201", "not a token".into()),
        ] {
            assert_eq!(
                run_helper_with_args(
                    [address.into(), token.into(), "c".repeat(TOKEN_BYTES).into()].into_iter()
                ),
                2
            );
        }
    }

    #[test]
    fn helper_rejects_a_server_without_its_independent_token() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let endpoint = listener.local_addr().unwrap();
        let fake_server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut token = [0_u8; TOKEN_BYTES];
            stream.read_exact(&mut token).unwrap();
            assert_eq!(token, [b'a'; TOKEN_BYTES]);
            stream.write_all(&[b'b'; TOKEN_BYTES]).unwrap();
            stream.shutdown(Shutdown::Write).unwrap();
            let mut response = Vec::new();
            stream.read_to_end(&mut response).unwrap();
            assert!(response.is_empty());
        });
        let args = [
            endpoint.to_string().into(),
            "a".repeat(TOKEN_BYTES).into(),
            "c".repeat(TOKEN_BYTES).into(),
        ];
        assert_eq!(run_helper_with_args(args.into_iter()), 2);
        fake_server.join().unwrap();
    }
}
