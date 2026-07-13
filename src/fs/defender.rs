use std::path::PathBuf;
#[cfg(target_os = "windows")]
use std::sync::Arc;
#[cfg(target_os = "windows")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "windows")]
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

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
pub struct DefenderSummary {
    pub state: DefenderScanState,
    pub paths: Vec<PathBuf>,
    pub scanned: usize,
    pub total: usize,
    pub threats: Vec<DefenderThreat>,
    pub error: Option<String>,
    pub elapsed: Duration,
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
#[derive(Clone, Debug)]
pub enum DefenderMessage {
    Progress(DefenderProgress),
    Finished(DefenderSummary),
    Failed(DefenderSummary),
    Cancelled(DefenderSummary),
}

#[cfg(target_os = "windows")]
pub fn run_scan(job: DefenderJob, tx: Sender<DefenderMessage>, cancel: Arc<AtomicBool>) {
    let started = Instant::now();
    let total = job.paths.len();
    let mut threats = Vec::new();

    for (index, path) in job.paths.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            let summary = summary(
                &job,
                DefenderScanState::Cancelled,
                started,
                index,
                total,
                threats,
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
                    // The scan intentionally uses -DisableRemediation. The
                    // detection history can carry a stale remediation status,
                    // so the current result must remain pending until the
                    // user explicitly asks Defender to remediate it.
                    status: "Action required".into(),
                }));
                dedupe_threats(&mut threats);
            }
            Err(_error) if cancel.load(Ordering::Relaxed) => {
                let summary = summary(
                    &job,
                    DefenderScanState::Cancelled,
                    started,
                    index,
                    total,
                    threats,
                    None,
                );
                let _ = tx.send(DefenderMessage::Cancelled(summary));
                return;
            }
            Err(error) => {
                let summary = summary(
                    &job,
                    DefenderScanState::Failed,
                    started,
                    index,
                    total,
                    threats,
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
        started,
        total,
        total,
        threats,
        None,
    );
    let _ = tx.send(DefenderMessage::Finished(summary));
}

#[cfg(target_os = "windows")]
fn summary(
    job: &DefenderJob,
    state: DefenderScanState,
    started: Instant,
    scanned: usize,
    total: usize,
    threats: Vec<DefenderThreat>,
    error: Option<String>,
) -> DefenderSummary {
    DefenderSummary {
        state,
        paths: job.paths.clone(),
        scanned,
        total,
        threats,
        error,
        elapsed: started.elapsed(),
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
