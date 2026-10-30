#[cfg(any(unix, windows))]
use std::io::{self, Read};
use std::process::Child;
#[cfg(unix)]
use std::process::ExitStatus;
#[cfg(any(unix, windows))]
use std::process::{Command, Output, Stdio};
#[cfg(any(unix, windows))]
use std::thread;
#[cfg(any(unix, windows))]
use std::time::{Duration, Instant};

#[cfg(any(unix, windows))]
const MAX_CAPTURE_BYTES: usize = 8 * 1024 * 1024;

/// Scope a Linux monitor to its spawning thread, including abrupt app exit.
/// The child only executes async-signal-safe syscalls between fork and exec.
#[cfg(target_os = "linux")]
pub(crate) fn terminate_with_parent(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    let parent_pid = std::process::id() as i32;
    unsafe {
        command.pre_exec(move || {
            unsafe extern "C" {
                fn prctl(option: i32, ...) -> i32;
                fn getppid() -> i32;
            }
            // PR_SET_PDEATHSIG / SIGKILL: no signal handler or allocator is
            // involved. Recheck PPID after arming to close the fork/exit race.
            if prctl(1, 9_usize, 0_usize, 0_usize, 0_usize) < 0 {
                return Err(io::Error::last_os_error());
            }
            if getppid() != parent_pid {
                return Err(io::Error::from_raw_os_error(3));
            }
            Ok(())
        });
    }
}

/// One deadline covers both execution and output. Nonblocking pipes avoid
/// reader threads that could outlive a timed-out helper and its descendants.
#[cfg(any(unix, windows))]
pub fn command_output_with_timeout(command: &mut Command, timeout: Duration) -> Option<Output> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(target_os = "linux")]
    terminate_with_parent(command);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = ReapedChild(command.spawn().ok()?);
    let tree = ProcessTree::attach(&mut child.0)?;
    let mut stdout = child.0.stdout.take()?;
    let mut stderr = child.0.stderr.take()?;
    make_nonblocking(&stdout).ok()?;
    make_nonblocking(&stderr).ok()?;
    let deadline = Instant::now() + timeout;
    let (mut stdout_bytes, mut stderr_bytes) = (Vec::new(), Vec::new());
    let (mut stdout_done, mut stderr_done) = (false, false);
    let mut status = None;
    loop {
        stdout_done |= drain_available(&mut stdout, &mut stdout_bytes).ok()?;
        stderr_done |= drain_available(&mut stderr, &mut stderr_bytes).ok()?;
        if status.is_none() {
            status = child.0.try_wait().ok()?;
        }
        if stdout_done
            && stderr_done
            && let Some(status) = status
        {
            return Some(Output {
                status,
                stdout: stdout_bytes,
                stderr: stderr_bytes,
            });
        }
        if Instant::now() >= deadline {
            tree.terminate();
            return None;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(any(unix, windows))]
struct ReapedChild(Child);
#[cfg(any(unix, windows))]
impl Drop for ReapedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn make_nonblocking(pipe: &impl std::os::fd::AsFd) -> io::Result<()> {
    use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};
    let flags = fcntl_getfl(pipe)?;
    fcntl_setfl(pipe, flags | OFlags::NONBLOCK)?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn make_nonblocking(pipe: &impl std::os::fd::AsRawFd) -> io::Result<()> {
    unsafe extern "C" {
        fn fcntl(fd: i32, command: i32, ...) -> i32;
    }
    let flags = unsafe { fcntl(pipe.as_raw_fd(), 3) };
    if flags < 0 || unsafe { fcntl(pipe.as_raw_fd(), 4, flags | 4) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(windows)]
fn make_nonblocking(_pipe: &impl std::os::windows::io::AsRawHandle) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn drain_available(pipe: &mut impl Read, bytes: &mut Vec<u8>) -> io::Result<bool> {
    let mut buffer = [0; 16 * 1024];
    loop {
        match pipe.read(&mut buffer) {
            Ok(0) => return Ok(true),
            Ok(count) => append_output(bytes, &buffer[..count])?,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(false),
            Err(error) => return Err(error),
        }
    }
}

#[cfg(windows)]
fn drain_available<R: Read + std::os::windows::io::AsRawHandle>(
    pipe: &mut R,
    bytes: &mut Vec<u8>,
) -> io::Result<bool> {
    use windows::Win32::Foundation::{ERROR_BROKEN_PIPE, HANDLE};
    use windows::Win32::System::Pipes::PeekNamedPipe;
    let mut buffer = [0; 16 * 1024];
    loop {
        let mut available = 0;
        if let Err(error) = unsafe {
            PeekNamedPipe(
                HANDLE(pipe.as_raw_handle()),
                None,
                0,
                None,
                Some(&mut available),
                None,
            )
        } {
            return if error.code() == ERROR_BROKEN_PIPE.to_hresult() {
                Ok(true)
            } else {
                Err(io::Error::other(error))
            };
        }
        if available == 0 {
            return Ok(false);
        }
        let count = (available as usize).min(buffer.len());
        let count = pipe.read(&mut buffer[..count])?;
        if count == 0 {
            return Ok(true);
        }
        append_output(bytes, &buffer[..count])?;
    }
}

#[cfg(any(unix, windows))]
fn append_output(bytes: &mut Vec<u8>, chunk: &[u8]) -> io::Result<()> {
    if bytes.len().saturating_add(chunk.len()) > MAX_CAPTURE_BYTES {
        return Err(io::Error::other("Desktop helper output exceeded its limit"));
    }
    bytes.extend_from_slice(chunk);
    Ok(())
}

#[cfg(unix)]
pub(crate) struct ProcessTree {
    group: i32,
}

#[cfg(unix)]
impl ProcessTree {
    pub(crate) fn attach(child: &mut Child) -> Option<Self> {
        Some(Self {
            group: child.id().try_into().ok()?,
        })
    }
    pub(crate) fn terminate(&self) {
        unsafe extern "C" {
            fn kill(pid: i32, signal: i32) -> i32;
        }
        // Captured helpers start in their own group, never the application's.
        let _ = unsafe { kill(-self.group, 9) };
    }
}

#[cfg(windows)]
pub(crate) struct ProcessTree {
    handle: Option<windows::Win32::Foundation::HANDLE>,
}

#[cfg(windows)]
impl ProcessTree {
    pub(crate) fn attach(child: &mut Child) -> Option<Self> {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };
        let handle = unsafe { CreateJobObjectW(None, None) }.ok()?;
        let tree = Self {
            handle: Some(handle),
        };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let assigned = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                std::mem::size_of_val(&limits) as u32,
            )
            .and_then(|()| {
                AssignProcessToJobObject(
                    handle,
                    windows::Win32::Foundation::HANDLE(child.as_raw_handle()),
                )
            })
        };
        if assigned.is_err() {
            // Fast helpers can exit before job assignment. Their pipe output
            // remains valid and must still be collected.
            if child.try_wait().ok().flatten().is_some() {
                return Some(Self { handle: None });
            }
            return None;
        }
        Some(tree)
    }
    pub(crate) fn terminate(&self) {
        if let Some(handle) = self.handle {
            let _ = unsafe { windows::Win32::System::JobObjects::TerminateJobObject(handle, 1) };
        }
    }
}

impl Drop for ProcessTree {
    fn drop(&mut self) {
        self.terminate();
        #[cfg(windows)]
        if let Some(handle) = self.handle {
            let _ = unsafe { windows::Win32::Foundation::CloseHandle(handle) };
        }
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn fast_windows_helpers_preserve_their_output() {
        for _ in 0..20 {
            let mut command = Command::new("cmd.exe");
            command.args(["/D", "/C", "echo captured"]);
            let output = command_output_with_timeout(&mut command, Duration::from_secs(3))
                .expect("capture short-lived helper");
            assert!(output.status.success());
            assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "captured");
        }
    }

    #[test]
    fn job_assignment_tolerates_an_already_exited_windows_helper() {
        let mut child = Command::new("cmd.exe")
            .args(["/D", "/C", "exit 0"])
            .spawn()
            .unwrap();
        child.wait().unwrap();
        assert!(ProcessTree::attach(&mut child).is_some());
    }

    #[test]
    fn windows_descendants_and_inherited_pipes_respect_the_deadline() {
        let mut command = Command::new("cmd.exe");
        command.args([
            "/D",
            "/C",
            "start /B ping.exe -n 6 127.0.0.1 & ping.exe -n 6 127.0.0.1",
        ]);
        let started = Instant::now();
        assert!(command_output_with_timeout(&mut command, Duration::from_millis(250)).is_none());
        assert!(started.elapsed() < Duration::from_secs(3));
    }
}

/// Waits for an already spawned child, killing and reaping it at the deadline.
#[cfg(unix)]
pub fn wait_for_child_with_timeout(child: &mut Child, timeout: Duration) -> Option<ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
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

    #[cfg(target_os = "linux")]
    #[test]
    #[allow(
        clippy::zombie_processes,
        reason = "the fixture must exit without waiting to exercise PDEATHSIG"
    )]
    fn parent_lifetime_fixture() {
        if std::env::var_os("BEXPLORER_TEST_PARENT_LIFETIME").is_none() {
            return;
        }
        let mut command = Command::new("sleep");
        command
            .arg("10")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        terminate_with_parent(&mut command);
        let child = command.spawn().expect("spawn private monitor fixture");
        println!("BEXPLORER_MONITOR_PID={}", child.id());
        // Child intentionally outlives this fixture thread; PDEATHSIG must
        // stop it without relying on a Rust destructor or a hardware event.
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn monitor_stops_when_its_spawning_parent_exits() {
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "utils::process::tests::parent_lifetime_fixture",
                "--nocapture",
            ])
            .env("BEXPLORER_TEST_PARENT_LIFETIME", "1")
            .output()
            .expect("run private monitor parent");
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        let pid: i32 = stdout
            .lines()
            .find_map(|line| {
                line.split_once("BEXPLORER_MONITOR_PID=")
                    .map(|(_, pid)| pid)
            })
            .unwrap()
            .parse()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let status = std::fs::read_to_string(format!("/proc/{pid}/status"));
            if status.as_ref().is_err()
                || status.as_ref().is_ok_and(|text| {
                    text.lines().any(|line| {
                        line.starts_with("State:") && (line.contains('Z') || line.contains('X'))
                    })
                })
            {
                break;
            }
            if Instant::now() >= deadline {
                unsafe extern "C" {
                    fn kill(pid: i32, signal: i32) -> i32;
                }
                let _ = unsafe { kill(pid, 9) };
                panic!("monitor survived the parent fixture");
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

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

    #[test]
    fn inherited_pipes_do_not_extend_the_deadline() {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 4 & wait"]);
        let started = Instant::now();
        assert!(command_output_with_timeout(&mut command, Duration::from_millis(80)).is_none());
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn rejects_unbounded_output() {
        let mut command = Command::new("sh");
        command.args(["-c", "yes output"]);
        let started = Instant::now();
        assert!(command_output_with_timeout(&mut command, Duration::from_secs(3)).is_none());
        assert!(started.elapsed() < Duration::from_secs(3));
    }
}
