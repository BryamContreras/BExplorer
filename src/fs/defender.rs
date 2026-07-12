#[cfg(target_os = "windows")]
use std::ffi::{OsStr, OsString};
#[cfg(target_os = "windows")]
use std::fs;
#[cfg(target_os = "windows")]
use std::path::Path;
use std::path::PathBuf;
#[cfg(target_os = "windows")]
use std::sync::Arc;
#[cfg(target_os = "windows")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "windows")]
use std::sync::mpsc::Sender;
use std::time::Instant;

use serde::{Deserialize, Serialize};

#[cfg(target_os = "windows")]
use crate::utils::errors::{BExplorerError, Result};

#[cfg(target_os = "windows")]
const ELEVATED_DEFENDER_HELPER_ARG: &str = "--bexplorer-elevated-defender-helper";

#[cfg(target_os = "windows")]
#[derive(Clone, Debug)]
pub struct DefenderJob {
    pub paths: Vec<PathBuf>,
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefenderScanState {
    Running,
    Finished,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug)]
pub struct DefenderProgress {
    pub state: DefenderScanState,
    pub current_path: Option<PathBuf>,
    pub scanned: usize,
    pub total: usize,
    pub threats_found: usize,
    pub started: Instant,
}

#[derive(Clone, Debug)]
pub struct DefenderThreat {
    pub name: String,
    pub path: Option<PathBuf>,
    pub status: String,
}

#[derive(Clone, Debug)]
pub struct DefenderScanOutput {
    pub target: PathBuf,
    pub exit_code: Option<i32>,
    pub output: String,
}

#[derive(Clone, Debug)]
pub struct DefenderSummary {
    pub state: DefenderScanState,
    pub paths: Vec<PathBuf>,
    pub scanned: usize,
    pub total: usize,
    pub threats: Vec<DefenderThreat>,
    pub outputs: Vec<DefenderScanOutput>,
    pub error: Option<String>,
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
#[derive(Clone, Debug)]
pub enum DefenderMessage {
    Progress(DefenderProgress),
    Finished(DefenderSummary),
    Failed(DefenderSummary),
    Cancelled(DefenderSummary),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum ElevatedDefenderAction {
    RemoveThreats,
    ExcludePaths { paths: Vec<PathBuf> },
}

#[cfg(target_os = "windows")]
pub fn run_scan(job: DefenderJob, tx: Sender<DefenderMessage>, cancel: Arc<AtomicBool>) {
    let started = Instant::now();
    let total = job.paths.len();
    let mut outputs = Vec::new();
    let mut threats = Vec::new();

    for (index, path) in job.paths.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            let summary = summary(
                &job,
                DefenderScanState::Cancelled,
                index,
                total,
                threats,
                outputs,
                None,
            );
            let _ = tx.send(DefenderMessage::Cancelled(summary));
            return;
        }

        let _ = tx.send(DefenderMessage::Progress(DefenderProgress {
            state: DefenderScanState::Running,
            current_path: Some(path.clone()),
            scanned: index,
            total,
            threats_found: threats.len(),
            started,
        }));

        match crate::platform::shell::scan_path_with_windows_defender(path, &cancel) {
            Ok(result) => {
                threats.extend(result.threats.into_iter().map(|threat| DefenderThreat {
                    name: threat.name,
                    path: threat.path,
                    status: threat.status,
                }));
                dedupe_threats(&mut threats);
                outputs.push(DefenderScanOutput {
                    target: result.target,
                    exit_code: result.exit_code,
                    output: result.output,
                });
            }
            Err(error) if cancel.load(Ordering::Relaxed) => {
                let summary = summary(
                    &job,
                    DefenderScanState::Cancelled,
                    index,
                    total,
                    threats,
                    outputs,
                    Some(error.to_string()),
                );
                let _ = tx.send(DefenderMessage::Cancelled(summary));
                return;
            }
            Err(error) => {
                let summary = summary(
                    &job,
                    DefenderScanState::Failed,
                    index,
                    total,
                    threats,
                    outputs,
                    Some(error.to_string()),
                );
                let _ = tx.send(DefenderMessage::Failed(summary));
                return;
            }
        }

        let _ = tx.send(DefenderMessage::Progress(DefenderProgress {
            state: DefenderScanState::Running,
            current_path: Some(path.clone()),
            scanned: index + 1,
            total,
            threats_found: threats.len(),
            started,
        }));
    }

    let summary = summary(
        &job,
        DefenderScanState::Finished,
        total,
        total,
        threats,
        outputs,
        None,
    );
    let _ = tx.send(DefenderMessage::Finished(summary));
}

#[cfg(target_os = "windows")]
pub fn run_elevated_defender_action(action: &ElevatedDefenderAction) -> Result<()> {
    let request_path = elevated_defender_request_path();
    let request_json = serde_json::to_string(action)?;
    fs::write(&request_path, request_json)?;

    let exit_code = crate::platform::shell::run_elevated_current_exe(&[
        OsString::from(ELEVATED_DEFENDER_HELPER_ARG),
        request_path.clone().into_os_string(),
    ]);

    if let Ok(code) = exit_code
        && code == 0
    {
        return Ok(());
    }

    if request_path.exists() {
        let _ = fs::remove_file(&request_path);
    }

    match exit_code {
        Ok(code) => Err(BExplorerError::Operation(format!(
            "Elevated Defender action failed with exit code {code}"
        ))),
        Err(error) => Err(error),
    }
}

#[cfg(target_os = "windows")]
pub fn try_run_elevated_defender_helper_from_args() -> Option<i32> {
    let mut args = std::env::args_os();
    let _exe = args.next();
    let marker = args.next()?;
    if marker != OsStr::new(ELEVATED_DEFENDER_HELPER_ARG) {
        return None;
    }

    let request_path = PathBuf::from(args.next()?);
    Some(match run_elevated_defender_helper(&request_path) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    })
}

#[cfg(target_os = "windows")]
fn run_elevated_defender_helper(request_path: &Path) -> Result<()> {
    let request_json = fs::read_to_string(request_path)?;
    let _ = fs::remove_file(request_path);
    let action: ElevatedDefenderAction =
        serde_json::from_str(request_json.trim_start_matches('\u{feff}')).map_err(|error| {
            BExplorerError::Operation(format!("Elevated Defender request decode failed: {error}"))
        })?;
    run_defender_action(&action)
}

#[cfg(target_os = "windows")]
fn run_defender_action(action: &ElevatedDefenderAction) -> Result<()> {
    match action {
        ElevatedDefenderAction::RemoveThreats => {
            crate::platform::shell::remove_windows_defender_threats()
        }
        ElevatedDefenderAction::ExcludePaths { paths } => {
            crate::platform::shell::exclude_windows_defender_paths(paths)
        }
    }
}

#[cfg(target_os = "windows")]
fn elevated_defender_request_path() -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "bexplorer-elevated-defender-{}-{stamp}.json",
        std::process::id()
    ))
}

#[cfg(target_os = "windows")]
fn summary(
    job: &DefenderJob,
    state: DefenderScanState,
    scanned: usize,
    total: usize,
    threats: Vec<DefenderThreat>,
    outputs: Vec<DefenderScanOutput>,
    error: Option<String>,
) -> DefenderSummary {
    DefenderSummary {
        state,
        paths: job.paths.clone(),
        scanned,
        total,
        threats,
        outputs,
        error,
    }
}

#[cfg(target_os = "windows")]
fn dedupe_threats(threats: &mut Vec<DefenderThreat>) {
    let mut seen = std::collections::BTreeSet::new();
    threats.retain(|threat| {
        seen.insert((
            threat.name.to_lowercase(),
            threat
                .path
                .as_ref()
                .map(|path| path.display().to_string().to_lowercase())
                .unwrap_or_default(),
        ))
    });
}
