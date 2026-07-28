use std::io::Read;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Runs a child process without allowing an optional desktop helper to block
/// its caller indefinitely.
///
/// Both output streams are drained while the process is alive so a verbose
/// helper cannot fill an OS pipe and deadlock before it exits. Timed-out
/// children are killed and reaped before returning `None`.
pub fn command_output_with_timeout(command: &mut Command, timeout: Duration) -> Option<Output> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let mut stderr = child.stderr.take()?;
    let stdout_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes);
        bytes
    });
    let stderr_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stderr.read_to_end(&mut bytes);
        bytes
    });

    let status = wait_for_child_with_timeout(&mut child, timeout);

    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    status.map(|status| Output {
        status,
        stdout,
        stderr,
    })
}

/// Waits for an already spawned child, killing and reaping it at the deadline.
pub fn wait_for_child_with_timeout(child: &mut Child, timeout: Duration) -> Option<ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(10));
            }
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn captures_stdout_and_stderr() {
        let mut command = Command::new("sh");
        command.args(["-c", "printf output; printf error >&2"]);
        let output = command_output_with_timeout(&mut command, Duration::from_secs(1))
            .expect("command should complete");
        assert!(output.status.success());
        assert_eq!(output.stdout, b"output");
        assert_eq!(output.stderr, b"error");
    }

    #[test]
    fn terminates_a_slow_child() {
        let mut command = Command::new("sleep");
        command.arg("2");
        let started = Instant::now();
        assert!(command_output_with_timeout(&mut command, Duration::from_millis(40)).is_none());
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
