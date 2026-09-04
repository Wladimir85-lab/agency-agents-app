//! IntentOS execution runtime.
//!
//! This is deliberately not a generic shell bridge. The frontend selects a
//! provider id from a backend-owned allowlist; command, arguments, sandbox and
//! working directory policy remain entirely in Rust.

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{ipc::Channel, State};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use uuid::Uuid;

use crate::corpus;
use crate::error::AppError;
use crate::local_agent;
use crate::mission::{self, Mission};
use crate::session;
use crate::state::AppState;
use crate::util::fs::{atomic_write, read_capped};

const PROVIDER_ID: &str = "codexCli";
// Second runtime provider: Claude Code CLI. Added as a manually-selectable
// fallback so a run does not have to sit blocked when Codex CLI has no
// remaining usage quota. Kept as its own id/probe/execution path rather than
// generalizing run_codex_stage, so the already-verified Codex path (evidence
// sanitization, watchdog, cancellation — all covered by real tests run on
// Windows) is never touched by this addition.
const CLAUDE_PROVIDER_ID: &str = "claudeCode";
const MAX_INTENT_CHARS: usize = 20_000;
const MAX_RUN_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_EVENT_TEXT_CHARS: usize = 8_000;
const MAX_WORKSPACE_FILES: usize = 100_000;
const MAX_WORKSPACE_BYTES: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProvider {
    id: String,
    label: String,
    available: bool,
    version: Option<String>,
    unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRunRequest {
    intent: String,
    project_path: String,
    runbook_id: String,
    capability_id: String,
    #[serde(default)]
    capability_ids: Vec<String>,
    #[serde(default)]
    stage_ids: Vec<String>,
    #[serde(default)]
    stage_kinds: Vec<String>,
    stage_labels: Vec<String>,
    agent_slugs: Vec<String>,
    /// Absent/`None` means no external agentic CLI is configured for this
    /// run — not a third provider, just "no external executor available
    /// yet". `direction` stages never consult this (they always run on the
    /// local sovereign executor); every other stage kind requires it to be
    /// `Some` and fails the run explicitly, not silently, when it is not.
    #[serde(default)]
    provider_id: Option<String>,
    /// Optional Mission to attach to this run. Purely additive — omitting
    /// it (or passing None) preserves Runtime v0.1's frozen behavior
    /// byte-for-byte: the raw `intent` field is used exactly as before.
    #[serde(default)]
    mission_id: Option<String>,
    /// Optional conversation session (see `session.rs`) this run is a turn
    /// of. Purely additive — omitting it preserves the original per-run
    /// isolated-copy behavior exactly. When present, the workspace is
    /// resolved from the session (reusing its evolving copy, or creating
    /// it once on the session's first turn) instead of always copying the
    /// original project into a fresh run-scoped directory. See the
    /// `session_id` branch in `runtime_start` below.
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RunStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunStage {
    id: String,
    label: String,
    agent_slug: String,
    #[serde(default)]
    kind: String,
    status: String,
    attempt: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    id: String,
    intent: String,
    project_path: String,
    /// Isolated copy used by every provider process. The user-selected project
    /// remains immutable until a future explicit review/apply command exists.
    #[serde(default)]
    workspace_path: Option<String>,
    runbook_id: String,
    #[serde(default)]
    capability_id: String,
    #[serde(default)]
    capability_ids: Vec<String>,
    /// Was `String` (always some external CLI) before external providers
    /// became optional. `#[serde(default)]` makes every run persisted
    /// before this field could be `null` deserialize the same way whether
    /// the JSON has a string, `null`, or the key missing entirely — see
    /// `provider_id_deserializes_from_a_historical_plain_string_run` and
    /// its sibling tests. `None` here means the same thing it means on
    /// `StartRunRequest`: no external executor, not a third provider.
    #[serde(default)]
    provider_id: Option<String>,
    /// Mirrors StartRunRequest.mission_id — absent on every run persisted
    /// before this field existed; `#[serde(default)]` deserializes those
    /// old files exactly like the existing `capability_id` precedent.
    #[serde(default)]
    mission_id: Option<String>,
    /// Mirrors StartRunRequest.session_id — absent on every run persisted
    /// before conversational sessions existed, same `#[serde(default)]`
    /// precedent as `mission_id`/`capability_id` above.
    #[serde(default)]
    session_id: Option<String>,
    status: RunStatus,
    current_stage: Option<String>,
    stages: Vec<RunStage>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RunEvent {
    RunUpdated {
        run: RunSummary,
    },
    Output {
        run_id: String,
        stage_id: String,
        stream: String,
        text: String,
    },
    GatePassed {
        run_id: String,
        stage_id: String,
    },
    GateFailed {
        run_id: String,
        stage_id: String,
        reason: String,
        attempt: u8,
    },
}

fn stage(id: &str, label: &str, agent: &str, kind: &str) -> RunStage {
    RunStage {
        id: id.into(),
        label: label.into(),
        agent_slug: agent.into(),
        kind: kind.into(),
        status: "pending".into(),
        attempt: 0,
    }
}

fn initial_stages(
    agents: &[String],
    labels: &[String],
    ids: &[String],
    kinds: &[String],
) -> Vec<RunStage> {
    if agents.len() == labels.len()
        && agents.len() == ids.len()
        && agents.len() == kinds.len()
        && agents.len() >= 5
    {
        return agents
            .iter()
            .zip(labels)
            .zip(ids)
            .zip(kinds)
            .map(|(((agent, label), id), kind)| stage(id, label, agent, kind))
            .collect();
    }
    let pick = |needle: &str, fallback: &str| {
        agents
            .iter()
            .find(|s| s.contains(needle))
            .cloned()
            .unwrap_or_else(|| fallback.into())
    };
    vec![
        stage(
            "project-management",
            "Project Manager",
            &pick("project-manager", "project-manager-senior"),
            "direction",
        ),
        stage(
            "ux-architecture",
            "UX / Architecture",
            &pick("architect", "ux-architect"),
            "architecture",
        ),
        stage(
            "development",
            "Development",
            &pick("developer", "senior-developer"),
            "development",
        ),
        stage(
            "qa",
            "Quality Assurance",
            &pick("evidence", "testing-evidence-collector"),
            "qa",
        ),
        stage(
            "reality-check",
            "Final Reality Check",
            &pick("reality", "testing-reality-checker"),
            "reality",
        ),
    ]
}

fn runs_dir(state: &AppState) -> PathBuf {
    state.app_data_dir.join("state").join("runs")
}
fn run_path(state: &AppState, id: &str) -> PathBuf {
    runs_dir(state).join(format!("{id}.json"))
}

fn run_workspace_dir(app_data: &Path, id: &str) -> PathBuf {
    app_data
        .join("state")
        .join("run-workspaces")
        .join(id)
        .join("project")
}

pub(crate) fn run_evidence_dir(app_data: &Path, id: &str) -> PathBuf {
    app_data.join("state").join("run-evidence").join(id)
}

/// Convert internal stage identifiers into portable filename components.
pub(crate) fn evidence_file_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len().min(120));
    let mut separator = false;
    for ch in value.chars() {
        let valid =
            !ch.is_control() && !matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*');
        if valid && out.len() < 120 {
            out.push(ch);
            separator = false;
        } else if !separator && !out.is_empty() {
            out.push('_');
            separator = true;
        }
    }
    while out.ends_with([' ', '.', '_']) {
        out.pop();
    }
    if out.is_empty() {
        "stage".into()
    } else {
        out
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ManifestEntry {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceChange {
    path: String,
    kind: String,
    before_sha256: Option<String>,
    after_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeReview {
    run_id: String,
    source_path: String,
    workspace_path: String,
    source_unchanged: bool,
    changes: Vec<WorkspaceChange>,
}

/// Immutable delivery-facing proof assembled from the persisted run, gate
/// states, manifests and explicit apply record. This is intentionally derived
/// from evidence on disk instead of trusting a provider's prose.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryReceipt {
    run_id: String,
    mission_id: Option<String>,
    intent: String,
    verified: bool,
    evidence_complete: bool,
    applied: bool,
    completed_at: Option<DateTime<Utc>>,
    gates: Vec<RunStage>,
    changes: Vec<WorkspaceChange>,
    evidence_path: String,
}

fn delivery_evidence_complete(run: &RunSummary, evidence_dir: &Path) -> bool {
    run.stages.iter().all(|stage| {
        let component = evidence_file_component(&stage.id);
        (1..=stage.attempt.max(1)).any(|attempt| {
            evidence_dir
                .join(format!("{component}-{attempt}-after.manifest.json"))
                .is_file()
        })
    })
}

fn walk_files(root: &Path) -> Result<Vec<PathBuf>, AppError> {
    fn visit(
        root: &Path,
        dir: &Path,
        files: &mut Vec<PathBuf>,
        bytes: &mut u64,
    ) -> Result<(), AppError> {
        for entry in fs::read_dir(dir).map_err(|e| AppError::Io {
            message: e.to_string(),
        })? {
            let entry = entry.map_err(|e| AppError::Io {
                message: e.to_string(),
            })?;
            let file_type = entry.file_type().map_err(|e| AppError::Io {
                message: e.to_string(),
            })?;
            let path = entry.path();
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                if path.file_name().and_then(|name| name.to_str()) == Some(".git") {
                    continue;
                }
                visit(root, &path, files, bytes)?;
            } else if file_type.is_file() {
                *bytes = bytes.saturating_add(
                    entry
                        .metadata()
                        .map_err(|e| AppError::Io {
                            message: e.to_string(),
                        })?
                        .len(),
                );
                files.push(path);
                if files.len() > MAX_WORKSPACE_FILES || *bytes > MAX_WORKSPACE_BYTES {
                    return Err(AppError::InvalidArgument {
                        message: format!("project exceeds isolation limit ({MAX_WORKSPACE_FILES} files or {MAX_WORKSPACE_BYTES} bytes)"),
                    });
                }
            }
        }
        debug_assert!(files.iter().all(|path| path.starts_with(root)));
        Ok(())
    }

    let mut files = Vec::new();
    let mut bytes = 0;
    visit(root, root, &mut files, &mut bytes)?;
    files.sort();
    Ok(files)
}

fn copy_workspace(source: &Path, destination: &Path) -> Result<(), AppError> {
    if destination.exists() {
        return Err(AppError::InvalidArgument {
            message: "isolated workspace already exists".into(),
        });
    }
    fs::create_dir_all(destination).map_err(|e| AppError::Io {
        message: e.to_string(),
    })?;
    for source_file in walk_files(source)? {
        let relative = source_file
            .strip_prefix(source)
            .map_err(|e| AppError::Internal {
                message: e.to_string(),
            })?;
        let destination_file = destination.join(relative);
        if let Some(parent) = destination_file.parent() {
            fs::create_dir_all(parent).map_err(|e| AppError::Io {
                message: e.to_string(),
            })?;
        }
        fs::copy(&source_file, &destination_file).map_err(|e| AppError::Io {
            message: format!("could not isolate {}: {e}", relative.display()),
        })?;
    }
    Ok(())
}

fn workspace_manifest(root: &Path) -> Result<Vec<ManifestEntry>, AppError> {
    walk_files(root)?
        .into_iter()
        .map(|path| {
            let relative = path.strip_prefix(root).map_err(|e| AppError::Internal {
                message: e.to_string(),
            })?;
            let mut file = fs::File::open(&path).map_err(|e| AppError::Io {
                message: e.to_string(),
            })?;
            let mut hasher = Sha256::new();
            let mut buffer = [0u8; 64 * 1024];
            loop {
                let read = file.read(&mut buffer).map_err(|e| AppError::Io {
                    message: e.to_string(),
                })?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
            Ok(ManifestEntry {
                path: relative.to_string_lossy().replace('\\', "/"),
                bytes: file
                    .metadata()
                    .map_err(|e| AppError::Io {
                        message: e.to_string(),
                    })?
                    .len(),
                sha256: hex::encode(hasher.finalize()),
            })
        })
        .collect()
}

pub(crate) async fn persist_manifest(
    app_data: &Path,
    run_id: &str,
    name: &str,
    workspace: &Path,
) -> Result<(), AppError> {
    let workspace = workspace.to_path_buf();
    let entries = tokio::task::spawn_blocking(move || workspace_manifest(&workspace))
        .await
        .map_err(|e| AppError::Internal {
            message: e.to_string(),
        })??;
    let dir = run_evidence_dir(app_data, run_id);
    tokio::fs::create_dir_all(&dir).await?;
    // `name` is frequently built from a composite stage id (e.g.
    // "systems-data:architecture-1-before"). Sanitize it the same way the
    // stdout/stderr evidence writer does — every filename derived from a
    // stage id must go through this single chokepoint, not be sanitized
    // ad hoc at each call site. Without this, the `:` in the id is
    // interpreted by NTFS as an Alternate Data Stream separator, the
    // rename from the `.tmp` sibling fails with "the parameter is
    // incorrect" (os error 87), and the manifest silently never gets
    // written (see `run_codex_stage` / `evidence_file_component`).
    let safe_name = evidence_file_component(name);
    atomic_write(
        &dir.join(format!("{safe_name}.manifest.json")),
        &serde_json::to_vec_pretty(&entries)?,
    )
    .await
}

async fn load_initial_manifest(
    app_data: &Path,
    run_id: &str,
) -> Result<Vec<ManifestEntry>, AppError> {
    let path = run_evidence_dir(app_data, run_id).join("initial.manifest.json");
    let bytes = read_capped(&path, MAX_RUN_FILE_BYTES).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn manifest_changes(before: &[ManifestEntry], after: &[ManifestEntry]) -> Vec<WorkspaceChange> {
    let before: HashMap<&str, &ManifestEntry> = before
        .iter()
        .map(|entry| (entry.path.as_str(), entry))
        .collect();
    let after: HashMap<&str, &ManifestEntry> = after
        .iter()
        .map(|entry| (entry.path.as_str(), entry))
        .collect();
    let mut paths: Vec<&str> = before.keys().chain(after.keys()).copied().collect();
    paths.sort_unstable();
    paths.dedup();
    paths
        .into_iter()
        .filter_map(|path| match (before.get(path), after.get(path)) {
            (None, Some(new)) => Some(WorkspaceChange {
                path: path.into(),
                kind: "added".into(),
                before_sha256: None,
                after_sha256: Some(new.sha256.clone()),
            }),
            (Some(old), None) => Some(WorkspaceChange {
                path: path.into(),
                kind: "removed".into(),
                before_sha256: Some(old.sha256.clone()),
                after_sha256: None,
            }),
            (Some(old), Some(new)) if old.sha256 != new.sha256 => Some(WorkspaceChange {
                path: path.into(),
                kind: "modified".into(),
                before_sha256: Some(old.sha256.clone()),
                after_sha256: Some(new.sha256.clone()),
            }),
            _ => None,
        })
        .collect()
}

fn validated_workspace(run: &RunSummary, app_data: &Path) -> Result<PathBuf, AppError> {
    let raw = run
        .workspace_path
        .as_ref()
        .ok_or_else(|| AppError::InvalidArgument {
            message: "this historical run has no isolated workspace".into(),
        })?;
    let workspace = PathBuf::from(raw)
        .canonicalize()
        .map_err(|e| AppError::InvalidArgument {
            message: format!("isolated workspace is unavailable: {e}"),
        })?;
    let root = app_data
        .join("state")
        .join("run-workspaces")
        .canonicalize()
        .map_err(|e| AppError::Io {
            message: format!("runtime workspace root is unavailable: {e}"),
        })?;
    if !workspace.starts_with(&root) || !workspace.is_dir() {
        return Err(AppError::InvalidArgument {
            message: "run workspace escaped the managed runtime area".into(),
        });
    }
    Ok(workspace)
}

fn synchronize_snapshot(snapshot: &Path, destination: &Path) -> Result<(), AppError> {
    let snapshot_files = walk_files(snapshot)?;
    let expected: std::collections::HashSet<PathBuf> = snapshot_files
        .iter()
        .filter_map(|path| path.strip_prefix(snapshot).ok().map(Path::to_path_buf))
        .collect();
    for existing in walk_files(destination)? {
        let relative = existing
            .strip_prefix(destination)
            .map_err(|e| AppError::Internal {
                message: e.to_string(),
            })?;
        if !expected.contains(relative) {
            fs::remove_file(&existing).map_err(|e| AppError::Io {
                message: format!("rollback could not remove {}: {e}", relative.display()),
            })?;
        }
    }
    for source in snapshot_files {
        let relative = source
            .strip_prefix(snapshot)
            .map_err(|e| AppError::Internal {
                message: e.to_string(),
            })?;
        let target = destination.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| AppError::Io {
                message: e.to_string(),
            })?;
        }
        fs::copy(&source, &target).map_err(|e| AppError::Io {
            message: format!("rollback could not restore {}: {e}", relative.display()),
        })?;
    }
    Ok(())
}

fn apply_workspace_changes(
    source: &Path,
    workspace: &Path,
    changes: &[WorkspaceChange],
) -> Result<(), AppError> {
    for change in changes {
        let relative = Path::new(&change.path);
        if relative.is_absolute() || change.path.split('/').any(|part| part == "..") {
            return Err(AppError::InvalidArgument {
                message: "unsafe path in workspace diff".into(),
            });
        }
        let target = source.join(relative);
        if !target.starts_with(source) {
            return Err(AppError::InvalidArgument {
                message: "workspace diff escaped source project".into(),
            });
        }
        if change.kind == "removed" {
            if target.exists() {
                fs::remove_file(&target).map_err(|e| AppError::Io {
                    message: e.to_string(),
                })?;
            }
        } else {
            let from = workspace.join(relative);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|e| AppError::Io {
                    message: e.to_string(),
                })?;
            }
            fs::copy(&from, &target).map_err(|e| AppError::Io {
                message: format!("could not apply {}: {e}", change.path),
            })?;
        }
    }
    Ok(())
}

async fn persist(state: &AppState, run: &RunSummary) -> Result<(), AppError> {
    tokio::fs::create_dir_all(runs_dir(state)).await?;
    let bytes = serde_json::to_vec_pretty(run)?;
    atomic_write(&run_path(state, &run.id), &bytes).await
}

async fn load_run(state: &AppState, id: &str) -> Result<RunSummary, AppError> {
    Uuid::parse_str(id).map_err(|_| AppError::InvalidArgument {
        message: "invalid run id".into(),
    })?;
    let bytes = read_capped(&run_path(state, id), MAX_RUN_FILE_BYTES).await?;
    let mut run: RunSummary = serde_json::from_slice(&bytes)?;
    normalize_terminal_state(&mut run);
    Ok(run)
}

fn normalize_terminal_state(run: &mut RunSummary) {
    let replacement = match run.status {
        RunStatus::Succeeded => Some("passed"),
        RunStatus::Failed => Some("failed"),
        RunStatus::Cancelled => Some("cancelled"),
        RunStatus::Queued | RunStatus::Running => None,
    };
    if let Some(replacement) = replacement {
        for stage in &mut run.stages {
            if stage.status == "running" {
                stage.status = replacement.into();
            }
        }
        run.current_stage = None;
    }
}

fn validate_project(raw: &str, app_data: &Path) -> Result<PathBuf, AppError> {
    let path = PathBuf::from(raw)
        .canonicalize()
        .map_err(|e| AppError::InvalidArgument {
            message: format!("project path is unavailable: {e}"),
        })?;
    if !path.is_dir() {
        return Err(AppError::InvalidArgument {
            message: "project path is not a directory".into(),
        });
    }
    if path.parent().is_none() || app_data.starts_with(&path) || path == app_data {
        return Err(AppError::InvalidArgument {
            message: "project path is too broad or reserved".into(),
        });
    }
    Ok(path)
}

fn clean_text(s: &str) -> String {
    let mut out: String = s
        .chars()
        .filter(|c| *c == '\n' || *c == '\t' || !c.is_control())
        .take(MAX_EVENT_TEXT_CHARS)
        .collect();
    for marker in ["OPENAI_API_KEY=", "CODEX_API_KEY=", "ANTHROPIC_API_KEY="] {
        if let Some(pos) = out.find(marker) {
            out.truncate(pos);
            out.push_str("[redacted]");
        }
    }
    out
}

/// Whether an executor's raw output carries an explicit gate verdict at
/// all, and if so which one — `output_gate_passed` collapses `Fail` and
/// `Missing` into the same `false`, which is correct for pass/fail control
/// flow but hides a real distinction: a process that ends with no verdict
/// (crashed, got cut off, or simply never said `INTENTOS_GATE:` anything)
/// is a different failure mode than one that explicitly reported FAIL, and
/// P0 of the executor↔runtime contract audit needs to tell them apart for
/// diagnosis/evidence, not just for the boolean outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GateMarker {
    Pass,
    Fail,
    Missing,
}

pub(crate) fn gate_marker_status(output: &str) -> GateMarker {
    let passed = output.rfind("INTENTOS_GATE:PASS");
    let failed = output.rfind("INTENTOS_GATE:FAIL");
    match (passed, failed) {
        (Some(_), None) => GateMarker::Pass,
        (None, Some(_)) => GateMarker::Fail,
        (Some(pass), Some(fail)) => {
            if pass > fail {
                GateMarker::Pass
            } else {
                GateMarker::Fail
            }
        }
        (None, None) => GateMarker::Missing,
    }
}

pub(crate) fn output_gate_passed(output: &str) -> bool {
    gate_marker_status(output) == GateMarker::Pass
}

/// Stage kinds where "the model said PASS" is not, on its own, credible
/// evidence that real work happened — mirrors `stage_uses_local_executor`'s
/// kind set deliberately: these are exactly the kinds that are expected to
/// materialize something in the workspace (a brief, an architecture
/// artifact, real code), whether they ran on the sovereign local executor
/// or an external CLI. `qa` and `reality` stages legitimately can pass
/// having only *read* the workspace, so they are not held to this bar.
fn stage_requires_workspace_evidence(stage_kind: &str) -> bool {
    matches!(stage_kind, "direction" | "architecture" | "development")
}

/// Diagnosis of what actually happened to one external-executor stage
/// attempt, kept separate from — and strictly more informative than — the
/// plain pass/fail boolean the run loop consumes. Persisted as evidence
/// (`{stage}-{attempt}.completion.json`) alongside the existing stdout/
/// stderr/manifest/reality-verdict files, so "the model's process ended"
/// and "the stage's work is actually done" are always distinguishable
/// after the fact, per the executor↔runtime contract audit (P0).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "status", content = "reason")]
pub(crate) enum StageCompletionDiagnosis {
    Completed,
    Failed(String),
    Incomplete(String),
    ContractViolated(String),
}

impl StageCompletionDiagnosis {
    fn passed(&self) -> bool {
        matches!(self, StageCompletionDiagnosis::Completed)
    }
}

/// The authoritative completion decision for one external-executor (Claude
/// Code / Codex) stage attempt. Evidence can only ever downgrade a claimed
/// PASS to a failure — it never manufactures a PASS out of a genuine FAIL
/// or a crashed process — matching the constitutional rule that a gate
/// marker "puede formar parte del protocolo; no debe constituir la
/// realidad" on its own.
///
/// `workspace_changed` must be computed by the caller from a real
/// before/after manifest diff of the actual workspace directory — this
/// function is pure and never touches disk, precisely so the five
/// contract cases (A-E) can be asserted directly, with no process spawn
/// and no filesystem, in a unit test.
fn evaluate_external_stage_completion(
    stage_kind: &str,
    process_success: bool,
    combined_output: &str,
    workspace_changed: bool,
    is_reality: bool,
    strict_enabled: bool,
    criteria_len: usize,
) -> (bool, StageCompletionDiagnosis, Option<String>) {
    let (strict_or_sentinel_passed, strict_reason) =
        resolve_stage_outcome(is_reality, strict_enabled, combined_output, criteria_len);

    if !process_success {
        return (
            false,
            StageCompletionDiagnosis::Failed(
                "el proceso del executor externo terminó con un código de salida distinto de cero".into(),
            ),
            strict_reason,
        );
    }
    if gate_marker_status(combined_output) == GateMarker::Missing {
        return (
            false,
            StageCompletionDiagnosis::Incomplete(
                "el proceso del executor externo terminó sin ningún marcador INTENTOS_GATE:PASS o INTENTOS_GATE:FAIL explícito — el modelo del proceso terminó, pero el trabajo de la etapa no declaró un veredicto".into(),
            ),
            strict_reason,
        );
    }
    if !strict_or_sentinel_passed {
        let reason = strict_reason
            .clone()
            .unwrap_or_else(|| "INTENTOS_GATE:FAIL".into());
        return (false, StageCompletionDiagnosis::Failed(reason), strict_reason);
    }
    if stage_requires_workspace_evidence(stage_kind) && !workspace_changed {
        return (
            false,
            StageCompletionDiagnosis::ContractViolated(format!(
                "la etapa reportó INTENTOS_GATE:PASS pero el workspace no muestra ningún cambio real de archivos entre el inicio y el fin del intento, lo cual es requerido para una etapa de tipo '{stage_kind}'"
            )),
            strict_reason,
        );
    }
    (true, StageCompletionDiagnosis::Completed, strict_reason)
}

/// Narrow, explicit, secondary-only signal: text that reads as the model
/// promising to pick this stage back up in a future turn. This is
/// deliberately NOT part of `evaluate_external_stage_completion`'s pass/
/// fail decision — a one-shot invocation genuinely has no future turn, so
/// this is surfaced as a visible warning for the human/evidence trail,
/// never used on its own to fail an otherwise-contract-satisfying stage
/// (that would risk a false FAIL on a stage that merely *mentions* a
/// legitimate long-lived background process, e.g. a dev server left
/// running for Showroom/preview on purpose).
fn contains_deferred_continuation_claim(output: &str) -> bool {
    let lower = output.to_lowercase();
    const PHRASES: &[&str] = &[
        "continuaré cuando",
        "continuaré una vez",
        "seguiré cuando termine",
        "i'll continue once",
        "i will continue once",
        "will continue in the background and",
        "once it finishes i will",
        "cuando termine, continuaré",
        "cuando termine continuaré",
    ];
    PHRASES.iter().any(|phrase| lower.contains(phrase))
}

/// Whether a stage's *kind* is one the sovereign local executor
/// (`local_agent`) can run at all — only the kinds
/// `local_agent::local_stage_task` actually defines a task for
/// (`direction`, `architecture`, `development` today). This is necessary
/// but not sufficient: the call site additionally requires
/// `current.provider_id.is_none()` before actually routing a stage here —
/// an explicit provider choice (Codex/Claude Code) takes every stage,
/// including these, so sovereign-local is this kind's default, not a
/// forced floor. Mirrors the existing `is_qa`/`is_reality` dual
/// `kind`-or-`id` check used at the call site. Adding a stage kind here
/// without a matching `local_stage_task` entry fails closed at dispatch
/// time (`AppError::Internal`), not silently — see
/// `run_local_agentic_stage`.
fn stage_uses_local_executor(stage: &RunStage) -> bool {
    matches!(stage.kind.as_str(), "direction" | "architecture" | "development")
        || stage.id == "direction"
        || stage.id == "architecture"
        || stage.id == "development"
}

// ---------- Reality gate — domain-conditional verification guidance ----------
//
// The same single Reality Checker persona is used for every capability —
// `composePipeline()` in intentosCapabilities.ts creates exactly one
// `reality-check` stage per run, never one per domain — so this never
// creates a second Reality Checker or a second pipeline. It only changes
// what that one stage is told counts as real evidence, mirroring the
// existing IoT development mandate below: the persona markdown is static
// text with no templating, so only `stage_prompt` (which already has
// `run.capability_id`/`capability_ids` in scope) can make this
// domain-conditional at all.

/// One capability's idea of real, checkable evidence for the reality-check
/// stage. Falls back to a domain-neutral instruction for any id this list
/// doesn't recognize (including the empty string on historical runs) —
/// deliberately never assumes a web/Laravel stack by default, which is
/// exactly the behavior being replaced.
fn domain_verification_guidance(capability_id: &str) -> &'static str {
    match capability_id {
        "digital-experience" => "- Inspect the actual rendered output: serve/open the built pages and verify DOM structure, responsive behavior at common breakpoints, and basic accessibility (labels, contrast, keyboard focus) with whatever browser/HTTP tooling is available in this workspace.\n- Do not assume Laravel, Blade views, or any specific framework — verify whatever was actually built.",
        "systems-data" => "- Call the actual endpoints/contracts that were built (not just read the code) and check status codes, error responses, and input validation.\n- Verify persistence round-trips: write data, read it back, and confirm it survives a restart when that is in scope.",
        "iot" => "- Verify the actual telemetry/protocol path (e.g. MQTT topics, message shapes) end to end, using simulation when physical hardware is unavailable.\n- Confirm alert/automation rules actually fire on the conditions they claim to handle.",
        "tinyml-edge-ai" => "- Run real inference on representative sample inputs and record actual accuracy/latency numbers, not estimates.\n- Confirm the model artifact and its deployment path genuinely exist in the workspace.",
        "cybersecurity" => "- Produce real tool/scan output or a reproducible proof-of-concept for every finding — never a narrative claim alone.\n- Diff the actual configuration/permissions against what was supposed to change.",
        "devops-quality" => "- Check the actual pipeline/deployment status and any observability signal (logs, metrics, health checks) that was supposed to be wired up.\n- Report measured performance numbers from a real run, not assumptions.",
        "operations-automation" => "- Trigger the actual automation end to end (the real event -> the real action) and confirm it happened, including error/idempotency handling.\n- Inspect integration logs for the calls that actually happened, not a description of intended calls.",
        "data-decisions" => "- Validate schemas and run the actual transformations/queries against representative sample data; compare output to known-correct expectations.\n- Check that visualizations/reports reflect the real underlying data, not placeholder values.",
        "ai-agents" => "- Review actual tool-call transcripts and outputs from the agent under test, not a description of what it should do.\n- Check grounding — do its claims match what was actually retrieved/available? — and test its behavior on at least one deliberately bad or edge-case input.",
        "creative-technology" => "- Inspect the actual rendered output/artifact (frame, export, build) and measured performance (load time, fps) rather than a subjective description.\n- Apply visual/UX criteria only where they are mechanically checkable from the acceptance criteria; do not invent taste-based scoring.",
        "strategy-product" => "- This stage is not verifying software behavior: confirm the actual deliverable documents/artifacts exist, cover every required section, and cite real research or data rather than fabricated claims presented as findings.\n- Do not ask for screenshots, endpoints, or telemetry — they do not apply to this capability.",
        _ => "- No specific domain profile is recorded for this run; gather whatever objective evidence (commands run, files produced, outputs inspected) genuinely demonstrates this stage's claims, using the tools actually available in this workspace. Do not assume a web/Laravel stack by default.",
    }
}

/// Assembles the reality-check-only guidance section from every capability
/// this run actually selected (`capability_ids`), falling back to the
/// single primary `capability_id` for runs that predate the
/// multi-capability field — same null-safety precedent as
/// `resumable_run`'s `capability_ids.is_empty()` check elsewhere in this
/// file. `composePipeline()` only tags the shared reality-check stage with
/// the primary capability's id, so this is a genuine improvement over that
/// for combined-capability builds, not just a port of it.
fn reality_domain_section(run: &RunSummary) -> String {
    let domains: Vec<&str> = if run.capability_ids.is_empty() {
        vec![run.capability_id.as_str()]
    } else {
        run.capability_ids.iter().map(String::as_str).collect()
    };
    let mut section = String::from(
        "\nDOMAIN VERIFICATION GUIDANCE (what counts as real evidence for this build — additional to, never a replacement for, your own persona's methodology):\n",
    );
    for domain in &domains {
        let label = if domain.is_empty() { "unspecified" } else { domain };
        section.push_str(&format!(
            "[{label}]\n{}\n",
            domain_verification_guidance(domain)
        ));
    }
    section
}

// ---------- Reality gate — structured criteria (shadow mode) ----------
//
// `output_gate_passed` above remains the sole authority for every stage's
// PASS/FAIL, including reality-check. Everything below is additive
// telemetry: the reality-check stage is asked (via `stage_prompt`) to also
// emit a structured, per-criterion breakdown; if present and well-formed it
// is persisted as evidence and compared against the sentinel verdict for
// observability. A missing or malformed block — or any run/persona that
// predates this — behaves exactly as before: `parse_criteria_result`
// returns `None` and nothing downstream changes.

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum CriterionStatus {
    Pass,
    Fail,
    Unverified,
}

/// One criterion's verdict, correlated by position rather than by trusting
/// the model to reproduce criterion text verbatim. `criterion_index` is the
/// 0-based position in `Mission.acceptance_criteria` — the same order the
/// ACCEPTANCE CRITERIA list appears in the prompt — so a future consumer
/// can match a result back to the approved Mission deterministically even
/// if `criterion` (kept for readability/evidence only) drifts from the
/// original text.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CriterionResult {
    criterion_index: usize,
    criterion: String,
    status: CriterionStatus,
    evidence: String,
    explanation: String,
}

/// Wire shape asked of the model. Every field is optional/loosely typed on
/// purpose: a missing or unrecognized `status`, or fields the model omits,
/// must degrade a single entry to `Unverified`/empty text, never fail the
/// whole block — only a genuinely unparsable JSON array (or a JSON value
/// that isn't an array of objects) returns `None` for the full result.
#[derive(Debug, Clone, Deserialize)]
struct RawCriterionEntry {
    #[serde(rename = "criterionIndex")]
    criterion_index: Option<i64>,
    criterion: Option<String>,
    status: Option<String>,
    evidence: Option<String>,
    explanation: Option<String>,
}

/// Best-effort extraction of the `INTENTOS_CRITERIA:` block `stage_prompt`
/// asks the reality-check stage to emit. Returns `None` whenever the
/// output cannot be trusted as a real, in-range correlation to the
/// approved Mission's acceptance criteria: no anchor, no valid JSON array,
/// an empty array, or — after dropping any entry whose `criterionIndex` is
/// missing/out of range — nothing left. `criteria_len` is
/// `Mission.acceptance_criteria.len()` for the run's attached Mission (0
/// when no Mission is attached, which makes every index out of range by
/// construction, so `None` is returned — there is nothing to correlate
/// against).
fn parse_criteria_result(output: &str, criteria_len: usize) -> Option<Vec<CriterionResult>> {
    let anchor = output.rfind("INTENTOS_CRITERIA:")?;
    let after = &output[anchor + "INTENTOS_CRITERIA:".len()..];
    let start = after.find('[')?;
    // Parse exactly one JSON value starting at the array open bracket and
    // stop — this tolerates trailing content (a closing ```` ``` ```` fence,
    // more prose, the INTENTOS_GATE line itself) without needing to
    // hand-roll bracket/string-aware scanning.
    let mut stream =
        serde_json::Deserializer::from_str(&after[start..]).into_iter::<Vec<RawCriterionEntry>>();
    let raw = stream.next()?.ok()?;
    if raw.is_empty() {
        return None;
    }
    let results: Vec<CriterionResult> = raw
        .into_iter()
        .filter_map(|entry| {
            let index = usize::try_from(entry.criterion_index?).ok()?;
            if index >= criteria_len {
                return None;
            }
            let status = match entry.status.as_deref() {
                Some("pass") => CriterionStatus::Pass,
                Some("fail") => CriterionStatus::Fail,
                _ => CriterionStatus::Unverified,
            };
            Some(CriterionResult {
                criterion_index: index,
                criterion: entry.criterion.unwrap_or_default(),
                status,
                evidence: entry.evidence.unwrap_or_default(),
                explanation: entry.explanation.unwrap_or_default(),
            })
        })
        .collect();
    if results.is_empty() {
        None
    } else {
        Some(results)
    }
}

/// Shadow-mode side effect only — never returns anything, never touches
/// `output_gate_passed`'s result. Persists the structured breakdown next to
/// the existing stdout/stderr evidence (same directory, same naming
/// convention) when present, and logs a warning when its aggregate verdict
/// disagrees with the text sentinel, so real disagreement data can be
/// observed before any future increment considers making it authoritative.
async fn record_reality_shadow_evidence(
    evidence_dir: &Path,
    evidence_stage_id: &str,
    attempt: u8,
    run_id: &str,
    stage_id: &str,
    combined: &str,
    criteria_len: usize,
    sentinel_passed: bool,
) {
    let Some(criteria) = parse_criteria_result(combined, criteria_len) else {
        return;
    };
    if let Ok(bytes) = serde_json::to_vec_pretty(&criteria) {
        let _ = atomic_write(
            &evidence_dir.join(format!("{evidence_stage_id}-{attempt}.criteria.json")),
            &bytes,
        )
        .await;
    }
    let structured_passed = criteria
        .iter()
        .all(|c| c.status == CriterionStatus::Pass);
    if structured_passed != sentinel_passed {
        tracing::warn!(
            run_id = %run_id,
            stage_id = %stage_id,
            attempt,
            sentinel_passed,
            structured_passed,
            "Reality gate shadow mode: structured criteria verdict disagrees with the text sentinel; sentinel remains authoritative"
        );
    }
}

// ---------- Reality gate — strict mode (opt-in authority) ----------
//
// Everything above this point (shadow mode) is unconditional and
// unchanged: `record_reality_shadow_evidence` always persists the
// structured breakdown and always logs a disagreement, regardless of
// anything below. What follows only decides whether that structured
// breakdown gets to *decide* the reality-check stage's outcome instead of
// `output_gate_passed`'s text sentinel — and it does so only when the
// engineering escape hatch `INTENTOS_REALITY_GATE_STRICT` is explicitly
// set (any non-empty value other than "0"). Unset — the default, and the
// only behavior any run has ever exercised so far — `resolve_stage_outcome`
// returns exactly `output_gate_passed`'s result, byte-for-byte the same
// decision Runtime v0.1 always made. This mirrors the existing
// `INTENTOS_LLAMA_SERVER_PATH`-style engineering env vars in
// `local_model.rs`: an explicit opt-in, never a silent default change.

const REALITY_GATE_STRICT_ENV: &str = "INTENTOS_REALITY_GATE_STRICT";

/// Reads the opt-in switch. Unset, empty, or `"0"` → disabled (today's
/// behavior, unconditionally).
fn reality_gate_strict_enabled() -> bool {
    std::env::var(REALITY_GATE_STRICT_ENV)
        .map(|v| !v.is_empty() && v != "0")
        .unwrap_or(false)
}

/// The strict, criterion-by-criterion authority for the reality-check
/// stage. Total and pure: every branch returns an explicit, specific
/// reason — never panics, never silently defaults to `Pass`.
///
/// - `criteria_len == 0`: no criteria were ever approved to verify
///   structurally — not a missing-block failure, there was never anything
///   to report on — so the sentinel decides, unchanged.
/// - `criteria_len > 0` and `criteria` is `None`: a Mission with approved
///   criteria requires a structured report; its absence is never treated
///   as evidence of success.
/// - Otherwise: every index in `0..criteria_len` must appear in `criteria`
///   exactly once (an index reported zero times or more than once both
///   fail, naming the specific index) and every one of them must carry
///   `CriterionStatus::Pass` (`Fail` and `Unverified` both fail, carrying
///   that criterion's own explanation forward). `sentinel_passed` is not
///   consulted once real criteria are in play — the structured result is,
///   deliberately, the actual authority in both directions: it can fail a
///   run the sentinel called PASS, and it can pass one the sentinel called
///   FAIL when every approved criterion genuinely checks out.
fn reality_verdict(
    criteria: Option<&[CriterionResult]>,
    criteria_len: usize,
    sentinel_passed: bool,
) -> RealityVerdict {
    if criteria_len == 0 {
        return if sentinel_passed {
            RealityVerdict::Pass
        } else {
            RealityVerdict::Fail(
                "no acceptance criteria were approved for this mission; the INTENTOS_GATE sentinel reported FAIL".into(),
            )
        };
    }
    let Some(criteria) = criteria else {
        return RealityVerdict::Fail(format!(
            "no valid INTENTOS_CRITERIA block was found to verify the {criteria_len} approved acceptance criteria"
        ));
    };
    let mut seen: Vec<Option<&CriterionResult>> = vec![None; criteria_len];
    for entry in criteria {
        if entry.criterion_index >= criteria_len {
            // parse_criteria_result already excludes these — this is
            // defense against ever trusting an out-of-range index twice,
            // not a path any current caller can actually reach.
            return RealityVerdict::Fail(format!(
                "criterion index {} is out of range for {criteria_len} approved criteria",
                entry.criterion_index
            ));
        }
        if seen[entry.criterion_index].is_some() {
            return RealityVerdict::Fail(format!(
                "criterion {} was reported more than once in INTENTOS_CRITERIA",
                entry.criterion_index
            ));
        }
        seen[entry.criterion_index] = Some(entry);
    }
    for (index, slot) in seen.iter().enumerate() {
        let Some(entry) = slot else {
            return RealityVerdict::Fail(format!(
                "criterion {index} was never addressed in INTENTOS_CRITERIA"
            ));
        };
        let explain = || {
            if entry.explanation.is_empty() {
                "no explanation provided".to_string()
            } else {
                entry.explanation.clone()
            }
        };
        match entry.status {
            CriterionStatus::Fail => {
                return RealityVerdict::Fail(format!("criterion {index} failed: {}", explain()));
            }
            CriterionStatus::Unverified => {
                return RealityVerdict::Fail(format!(
                    "criterion {index} could not be verified: {}",
                    explain()
                ));
            }
            CriterionStatus::Pass => {}
        }
    }
    RealityVerdict::Pass
}

#[derive(Debug)]
enum RealityVerdict {
    Pass,
    Fail(String),
}

impl RealityVerdict {
    fn passed(&self) -> bool {
        matches!(self, RealityVerdict::Pass)
    }
}

/// The actual pass/fail decision for one stage attempt — pulled out of
/// `run_codex_stage`/`run_claude_stage` so it is testable without spawning
/// any process. Strict mode only ever applies to the reality-check stage;
/// every other stage always gets the plain sentinel, exactly as before.
/// The second element is `Some(reason)` only when a strict-mode structured
/// verdict actually decided the outcome (pass or fail) — the specific,
/// diagnosable text `reality_verdict` produced — so the caller can persist
/// it as evidence, not just infer "something failed" from a boolean.
fn resolve_stage_outcome(
    is_reality: bool,
    strict_enabled: bool,
    combined: &str,
    criteria_len: usize,
) -> (bool, Option<String>) {
    let sentinel_passed = output_gate_passed(combined);
    if is_reality && strict_enabled {
        let criteria = parse_criteria_result(combined, criteria_len);
        match reality_verdict(criteria.as_deref(), criteria_len, sentinel_passed) {
            RealityVerdict::Pass => (
                true,
                Some("all approved acceptance criteria passed".into()),
            ),
            RealityVerdict::Fail(reason) => (false, Some(reason)),
        }
    } else {
        (sentinel_passed, None)
    }
}

fn validate_start_request(request: &StartRunRequest) -> Result<(), AppError> {
    let stage_count = request.agent_slugs.len();
    let structured =
        request.stage_ids.len() == stage_count && request.stage_kinds.len() == stage_count;
    if request.runbook_id.trim().is_empty()
        || request.capability_id.trim().is_empty()
        || stage_count < 5
        || stage_count > 20
        || request.stage_labels.len() != stage_count
        || (!request.stage_ids.is_empty() && !structured)
        || (!request.stage_kinds.is_empty() && !structured)
        || request
            .agent_slugs
            .iter()
            .any(|value| value.trim().is_empty())
        || request
            .stage_labels
            .iter()
            .any(|value| value.trim().is_empty())
        || request.stage_kinds.iter().any(|kind| {
            !matches!(
                kind.as_str(),
                "direction" | "architecture" | "development" | "qa" | "reality"
            )
        })
    {
        return Err(AppError::InvalidArgument {
            message: "runbook, capabilities and a valid 5-20 stage pipeline are required".into(),
        });
    }
    Ok(())
}

async fn probe_codex() -> RuntimeProvider {
    let binary = codex_binary();
    let result = tokio::time::timeout(
        Duration::from_secs(4),
        Command::new(binary).arg("--version").output(),
    )
    .await;
    match result {
        Ok(Ok(out)) if out.status.success() => RuntimeProvider {
            id: PROVIDER_ID.into(),
            label: "Codex CLI".into(),
            available: true,
            version: Some(
                clean_text(&String::from_utf8_lossy(&out.stdout))
                    .trim()
                    .to_string(),
            ),
            unavailable_reason: None,
        },
        Ok(Ok(out)) => RuntimeProvider {
            id: PROVIDER_ID.into(),
            label: "Codex CLI".into(),
            available: false,
            version: None,
            unavailable_reason: Some(clean_text(&String::from_utf8_lossy(&out.stderr))),
        },
        Ok(Err(e)) => RuntimeProvider {
            id: PROVIDER_ID.into(),
            label: "Codex CLI".into(),
            available: false,
            version: None,
            unavailable_reason: Some(e.to_string()),
        },
        Err(_) => RuntimeProvider {
            id: PROVIDER_ID.into(),
            label: "Codex CLI".into(),
            available: false,
            version: None,
            unavailable_reason: Some("provider probe timed out".into()),
        },
    }
}

fn codex_binary() -> PathBuf {
    #[cfg(target_os = "windows")]
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let native = PathBuf::from(appdata).join("npm/node_modules/@openai/codex/node_modules/@openai/codex-win32-x64/vendor/x86_64-pc-windows-msvc/bin/codex.exe");
        if native.is_file() {
            return native;
        }
    }
    PathBuf::from("codex")
}

// Same shape and same honesty limitation as probe_codex: this only proves
// the `claude` binary exists and runs `--version` successfully within a
// short timeout. It says nothing about whether the account behind it is
// logged in or has remaining usage — exactly the gap we found in probe_codex
// (a quota-exhausted or logged-out CLI would still probe as "available").
// runtime_start still has to attempt a real stage and fail honestly if the
// provider rejects the request; this probe only gates the obviously-broken
// case (binary missing / not executable) before spending a run on it.
async fn probe_claude() -> RuntimeProvider {
    let result = tokio::time::timeout(
        Duration::from_secs(4),
        claude_command().arg("--version").output(),
    )
    .await;
    match result {
        Ok(Ok(out)) if out.status.success() => RuntimeProvider {
            id: CLAUDE_PROVIDER_ID.into(),
            label: "Claude Code".into(),
            available: true,
            version: Some(
                clean_text(&String::from_utf8_lossy(&out.stdout))
                    .trim()
                    .to_string(),
            ),
            unavailable_reason: None,
        },
        Ok(Ok(out)) => RuntimeProvider {
            id: CLAUDE_PROVIDER_ID.into(),
            label: "Claude Code".into(),
            available: false,
            version: None,
            unavailable_reason: Some(clean_text(&String::from_utf8_lossy(&out.stderr))),
        },
        Ok(Err(e)) => RuntimeProvider {
            id: CLAUDE_PROVIDER_ID.into(),
            label: "Claude Code".into(),
            available: false,
            version: None,
            unavailable_reason: Some(e.to_string()),
        },
        Err(_) => RuntimeProvider {
            id: CLAUDE_PROVIDER_ID.into(),
            label: "Claude Code".into(),
            available: false,
            version: None,
            unavailable_reason: Some("provider probe timed out".into()),
        },
    }
}

fn claude_binary() -> PathBuf {
    // npm installs global bins under %APPDATA%\npm on Windows as a .cmd
    // shim (there is no nested vendor binary like Codex's install layout).
    // Resolve the absolute path so the spawned process does not depend on
    // the app's inherited PATH being fresh (e.g. a window opened before
    // `npm install -g` ran would otherwise fail to find it by bare name).
    #[cfg(target_os = "windows")]
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let native = PathBuf::from(appdata).join("npm/claude.cmd");
        if native.is_file() {
            return native;
        }
    }
    PathBuf::from("claude")
}

/// Windows-safe process builder for the `claude` CLI.
///
/// Root cause found during the first live test: npm's Windows install of
/// `claude` is a `.cmd` batch shim, not a native PE executable (unlike
/// Codex, which ships a real `.exe`). Spawning a `.cmd` directly through
/// tokio::process::Command works for a short call with no stdin — which is
/// exactly why probe_claude()'s `--version` check reported "available" —
/// but a prompt piped over stdin (what every real stage does) never
/// reached the underlying process: run_claude_stage sat at zero output
/// until the watchdog fired repeatedly and the run had to be cancelled.
/// Routing through an explicit `cmd /C` spawns it the same way typing
/// `claude` into an interactive shell does, with stdio piped correctly end
/// to end.
///
/// Known follow-up, not fixed here: cancelling a Claude Code stage now
/// kills the `cmd.exe` wrapper via `kill_on_drop`, which does not
/// guarantee the underlying `node.exe` process (claude.cmd's real worker)
/// also dies — Windows does not propagate process termination to children
/// without a Job Object. Codex's cancellation guarantee (verified earlier
/// this session against Task Manager) does not automatically extend to
/// this provider. Left as a known gap rather than guessed at blind.
fn claude_command() -> Command {
    #[cfg(target_os = "windows")]
    {
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg(claude_binary());
        cmd
    }
    #[cfg(not(target_os = "windows"))]
    {
        Command::new(claude_binary())
    }
}

#[tauri::command]
pub async fn runtime_providers() -> Result<Vec<RuntimeProvider>, AppError> {
    // Ordered internal bindings. Runbooks consumes the first healthy runtime;
    // provider identity is deliberately absent from the human intention flow.
    // Claude Code first: found by hand this session that `available` here
    // only proves the binary runs `--version` successfully, not that its
    // account has remaining usage (see probe_codex's own doc comment on
    // that exact gap) — a quota-exhausted Codex CLI still probed as
    // available and, picked first, silently hung a real run for ~15
    // minutes with zero output (its `exec` JSON stream never got a chance
    // to report the actual "usage limit" error back through this path).
    // Not a permanent ranking of one provider over the other — just today's
    // honest default until probing verifies real usability, not presence.
    Ok(vec![probe_claude().await, probe_codex().await])
}

#[tauri::command]
pub async fn runtime_get(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<RunSummary, AppError> {
    load_run(&state, &run_id).await
}

#[tauri::command]
pub async fn runtime_list(
    state: State<'_, AppState>,
    project_path: Option<String>,
) -> Result<Vec<RunSummary>, AppError> {
    let dir = runs_dir(&state);
    let mut out = Vec::new();
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return Ok(out);
    };
    while let Some(entry) = entries.next_entry().await? {
        if entry.path().extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        if let Ok(bytes) = read_capped(&entry.path(), MAX_RUN_FILE_BYTES).await {
            if let Ok(mut run) = serde_json::from_slice::<RunSummary>(&bytes) {
                normalize_terminal_state(&mut run);
                if project_path.as_ref().is_none_or(|p| p == &run.project_path) {
                    out.push(run);
                }
            }
        }
    }
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(out)
}

async fn build_review(state: &AppState, run: &RunSummary) -> Result<RuntimeReview, AppError> {
    let source = validate_project(&run.project_path, &state.app_data_dir)?;
    let workspace = validated_workspace(run, &state.app_data_dir)?;
    let initial = load_initial_manifest(&state.app_data_dir, &run.id).await?;
    let source_for_manifest = source.clone();
    let workspace_for_manifest = workspace.clone();
    let (source_now, workspace_now) = tokio::try_join!(
        tokio::task::spawn_blocking(move || workspace_manifest(&source_for_manifest)),
        tokio::task::spawn_blocking(move || workspace_manifest(&workspace_for_manifest)),
    )
    .map_err(|e| AppError::Internal {
        message: e.to_string(),
    })?;
    let source_now = source_now?;
    let workspace_now = workspace_now?;
    let applied_path =
        run_evidence_dir(&state.app_data_dir, &run.id).join("applied-source.manifest.json");
    let applied = match tokio::fs::read(applied_path).await {
        Ok(bytes) => serde_json::from_slice::<Vec<ManifestEntry>>(&bytes).ok(),
        Err(_) => None,
    };
    let already_applied = applied.as_ref().is_some_and(|manifest| {
        manifest_changes(manifest, &source_now).is_empty()
            && manifest_changes(manifest, &workspace_now).is_empty()
    });
    Ok(RuntimeReview {
        run_id: run.id.clone(),
        source_path: source.to_string_lossy().into_owned(),
        workspace_path: workspace.to_string_lossy().into_owned(),
        source_unchanged: already_applied || manifest_changes(&initial, &source_now).is_empty(),
        changes: if already_applied {
            Vec::new()
        } else {
            manifest_changes(&initial, &workspace_now)
        },
    })
}

#[tauri::command]
pub async fn runtime_review(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<RuntimeReview, AppError> {
    let run = load_run(&state, &run_id).await?;
    build_review(&state, &run).await
}

#[tauri::command]
pub async fn runtime_delivery_receipt(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<DeliveryReceipt, AppError> {
    let run = load_run(&state, &run_id).await?;
    if run.status != RunStatus::Succeeded {
        return Err(AppError::InvalidArgument {
            message: "delivery receipt requires a run that passed every gate".into(),
        });
    }
    let evidence_dir = run_evidence_dir(&state.app_data_dir, &run.id);
    let evidence_complete = delivery_evidence_complete(&run, &evidence_dir);
    let applied_manifest = evidence_dir.join("applied-source.manifest.json");
    let review = build_review(&state, &run).await?;
    Ok(DeliveryReceipt {
        run_id: run.id,
        mission_id: run.mission_id,
        intent: run.intent,
        verified: run.stages.iter().all(|stage| stage.status == "passed"),
        evidence_complete,
        applied: applied_manifest.is_file() && review.source_unchanged && review.changes.is_empty(),
        completed_at: run.completed_at,
        gates: run.stages,
        changes: review.changes,
        evidence_path: evidence_dir.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub async fn runtime_apply(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<RuntimeReview, AppError> {
    let run = load_run(&state, &run_id).await?;
    if matches!(run.status, RunStatus::Queued | RunStatus::Running) {
        return Err(AppError::InvalidArgument {
            message: "a running workspace cannot be applied".into(),
        });
    }
    let review = build_review(&state, &run).await?;
    if !review.source_unchanged {
        return Err(AppError::InvalidArgument {
            message:
                "source project changed after this run started; review conflicts before applying"
                    .into(),
        });
    }
    let source = PathBuf::from(&review.source_path);
    let workspace = PathBuf::from(&review.workspace_path);
    let backup = state
        .app_data_dir
        .join("state")
        .join("run-backups")
        .join(&run.id)
        .join("project");
    let backup_source = source.clone();
    let backup_destination = backup.clone();
    tokio::task::spawn_blocking(move || copy_workspace(&backup_source, &backup_destination))
        .await
        .map_err(|e| AppError::Internal {
            message: e.to_string(),
        })??;
    let apply_source = source.clone();
    let apply_workspace = workspace.clone();
    let changes = review.changes.clone();
    let apply_result = tokio::task::spawn_blocking(move || {
        apply_workspace_changes(&apply_source, &apply_workspace, &changes)
    })
    .await
    .map_err(|e| AppError::Internal {
        message: e.to_string(),
    })?;
    if let Err(apply_error) = apply_result {
        let restore_source = source.clone();
        let restore_backup = backup.clone();
        tokio::task::spawn_blocking(move || synchronize_snapshot(&restore_backup, &restore_source))
            .await
            .map_err(|e| AppError::Internal {
                message: e.to_string(),
            })??;
        return Err(apply_error);
    }
    persist_manifest(&state.app_data_dir, &run.id, "applied-source", &source).await?;
    build_review(&state, &run).await
}

#[tauri::command]
pub async fn runtime_discard_workspace(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<(), AppError> {
    let mut run = load_run(&state, &run_id).await?;
    if matches!(run.status, RunStatus::Queued | RunStatus::Running) {
        return Err(AppError::InvalidArgument {
            message: "a running workspace cannot be discarded".into(),
        });
    }
    let workspace = validated_workspace(&run, &state.app_data_dir)?;
    let run_dir = workspace.parent().ok_or_else(|| AppError::Internal {
        message: "workspace has no managed parent".into(),
    })?;
    tokio::fs::remove_dir_all(run_dir)
        .await
        .map_err(|e| AppError::Io {
            message: format!("could not discard isolated workspace: {e}"),
        })?;
    run.workspace_path = None;
    run.updated_at = Utc::now();
    // The deleted directory was the session's shared evolving workspace,
    // not a run-private copy — clear the session's pointer too, or the
    // next conversational turn would try to reuse a path that no longer
    // exists instead of creating a fresh one.
    if let Some(session_id) = run.session_id.as_deref() {
        session::clear_workspace_path(&state, session_id).await?;
    }
    persist(&state, &run).await
}

async fn resumable_run(
    state: &AppState,
    intent: &str,
    project_path: &str,
    runbook_id: &str,
    capability_id: &str,
    capability_ids: &[String],
    expected: &[RunStage],
) -> Option<(String, Vec<RunStage>, PathBuf)> {
    let dir = runs_dir(state);
    let mut entries = tokio::fs::read_dir(dir).await.ok()?;
    let mut candidates = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        if entry.path().extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let Ok(bytes) = read_capped(&entry.path(), MAX_RUN_FILE_BYTES).await else {
            continue;
        };
        let Ok(run) = serde_json::from_slice::<RunSummary>(&bytes) else {
            continue;
        };
        let same_workflow = run.stages.len() == expected.len()
            && run.stages.iter().zip(expected).all(|(a, b)| a.id == b.id);
        if run.intent == intent
            && run.project_path == project_path
            && run.runbook_id == runbook_id
            && run.capability_id == capability_id
            && (run.capability_ids.is_empty() || run.capability_ids == capability_ids)
            && run.status != RunStatus::Succeeded
            && run
                .workspace_path
                .as_ref()
                .is_some_and(|path| Path::new(path).is_dir())
            && same_workflow
            && run.stages.iter().any(|stage| stage.status == "passed")
        {
            candidates.push(run);
        }
    }
    candidates.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    candidates.into_iter().next().map(|run| {
        let workspace = PathBuf::from(run.workspace_path.as_deref().expect("filtered above"));
        let stages = expected
            .iter()
            .cloned()
            .map(|mut stage| {
                if run.stages.iter().any(|old| {
                    old.id == stage.id
                        && old.agent_slug == stage.agent_slug
                        && old.status == "passed"
                }) {
                    stage.status = "passed".into();
                    stage.attempt = 1;
                }
                stage
            })
            .collect();
        (run.id, stages, workspace)
    })
}

#[tauri::command]
pub async fn runtime_cancel(state: State<'_, AppState>, run_id: String) -> Result<(), AppError> {
    let mut run = load_run(&state, &run_id).await?;
    if let Some(handle) = state.runtime_jobs.lock().await.remove(&run_id) {
        handle.abort();
    }
    if matches!(run.status, RunStatus::Queued | RunStatus::Running) {
        run.status = RunStatus::Cancelled;
        for stage in &mut run.stages {
            if stage.status == "running" {
                stage.status = "cancelled".into();
            }
        }
        run.current_stage = None;
        run.updated_at = Utc::now();
        run.completed_at = Some(Utc::now());
        persist(&state, &run).await?;
    }
    Ok(())
}

/// Resolves the workspace a conversational turn builds in: the session's
/// existing evolving copy if one is already there, or — only on the
/// session's first-ever turn — a fresh copy of the original project,
/// which is then attached to the session so every later turn reuses it.
/// Pulled out of `runtime_start` so it's testable without a `tauri::AppHandle`
/// or `Channel` (see `resolve_session_workspace_reuses_the_same_directory_across_turns`).
/// The session id is always derived from `project_path` here — never taken
/// as-is from the caller — so a stale or mismatched id on `StartRunRequest`
/// can never point this turn at a different session's workspace than the
/// one `session_get_or_create`/`session_append_message` for the same
/// project resolve to. Callers only need to signal "this is a
/// conversational turn" (`StartRunRequest.session_id: Some(_)`); the value
/// itself is not load-bearing.
async fn resolve_session_workspace(
    state: &AppState,
    project: &Path,
    project_path: &str,
) -> Result<(String, PathBuf), AppError> {
    let session_id = session::session_id_for(project_path);
    let existing_workspace = session::get_or_create(state, project_path)
        .await?
        .workspace_path
        .filter(|path| Path::new(path).is_dir());
    let workspace = match existing_workspace {
        Some(path) => PathBuf::from(path),
        None => {
            let workspace = session::session_workspace_dir(&state.app_data_dir, &session_id);
            let source = project.to_path_buf();
            let destination = workspace.clone();
            tokio::task::spawn_blocking(move || copy_workspace(&source, &destination))
                .await
                .map_err(|e| AppError::Internal {
                    message: e.to_string(),
                })??;
            session::set_workspace_path(state, &session_id, &workspace.to_string_lossy()).await?;
            workspace
        }
    };
    Ok((session_id, workspace))
}

#[tauri::command]
pub async fn runtime_start(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: StartRunRequest,
    on_event: Channel<RunEvent>,
) -> Result<RunSummary, AppError> {
    let intent = request.intent.trim();
    if intent.is_empty() || intent.chars().count() > MAX_INTENT_CHARS {
        return Err(AppError::InvalidArgument {
            message: "intent must contain 1 to 20000 characters".into(),
        });
    }
    if let Some(pid) = request.provider_id.as_deref() {
        if pid != PROVIDER_ID && pid != CLAUDE_PROVIDER_ID {
            return Err(AppError::InvalidArgument {
                message: "unsupported runtime provider".into(),
            });
        }
    }
    validate_start_request(&request)?;
    // Codex CLI and Claude Code are both outbound providers even though they
    // are launched as local child processes. The same fail-closed network
    // policy that protects GitHub, catalog sync, and updates must therefore
    // gate the runtime before the project is inspected or any provider
    // process is started. This entire gate is skipped when no external
    // provider was requested at all (`provider_id: None`) — a run with no
    // external executor never touches the network on IntentOS's behalf, so
    // there is nothing here for Paranoid Mode to block, and probing a CLI
    // that was never going to be used would only reject a run that the
    // `direction` stage (sovereign, loopback-only) could otherwise complete
    // on its own.
    if let Some(pid) = request.provider_id.as_deref() {
        let is_claude = pid == CLAUDE_PROVIDER_ID;
        state
            .require_network(if is_claude {
                "runtime_claude"
            } else {
                "runtime_codex"
            })
            .await?;
        let provider = if is_claude {
            probe_claude().await
        } else {
            probe_codex().await
        };
        if !provider.available {
            return Err(AppError::InvalidArgument {
                message: format!(
                    "{} is not executable: {}",
                    provider.label,
                    provider.unavailable_reason.unwrap_or_default()
                ),
            });
        }
    }
    let project = validate_project(&request.project_path, &state.app_data_dir)?;

    let catalog = corpus::ensure_corpus(&app, &state).await?;
    let profiles: HashMap<String, String> = request
        .agent_slugs
        .iter()
        .filter_map(|slug| catalog.get(slug).map(|agent| (slug.clone(), agent.body)))
        .collect();
    // NOTE: `profiles` is keyed by slug (a HashMap), so it naturally collapses
    // duplicate slugs. `agent_slugs` is a per-stage array and legitimately
    // contains the same slug more than once whenever two selected
    // capabilities share an agent at the same roster position. Comparing
    // `profiles.len()` against `agent_slugs.len()` therefore produces a false
    // positive on any runbook with a repeated (but perfectly valid) agent.
    // The correct check is "does every requested slug resolve", not "do the
    // counts match" — so we check membership per slug instead.
    let unresolved: Vec<&str> = request
        .agent_slugs
        .iter()
        .map(String::as_str)
        .filter(|slug| !profiles.contains_key(*slug))
        .collect();
    if !unresolved.is_empty() {
        let mut unresolved_sorted = unresolved.clone();
        unresolved_sorted.sort_unstable();
        unresolved_sorted.dedup();
        return Err(AppError::InvalidArgument {
            message: format!(
                "every runbook agent must resolve to a real catalog persona (unresolved: {})",
                unresolved_sorted.join(", ")
            ),
        });
    }
    // Mission is loaded once, at run start, and never re-read mid-run —
    // matches the existing resumable-run semantics and the explicit
    // decision to defer Mission Brief versioning past this pass. Absent
    // mission_id: mission stays None and every stage prompt is generated
    // exactly as in Runtime v0.1 today.
    let mission: Option<Mission> = match request.mission_id.as_ref() {
        Some(id) => Some(mission::load_mission(&state, id).await?),
        None => None,
    };
    let project_path = project.to_string_lossy().into_owned();
    let default_stages = initial_stages(
        &request.agent_slugs,
        &request.stage_labels,
        &request.stage_ids,
        &request.stage_kinds,
    );
    let run_id = Uuid::new_v4().to_string();
    // A conversational turn (`session_id` present) never recopies the
    // original project: it reuses the session's one evolving workspace, so
    // turn 2 builds on top of what turn 1 actually produced instead of
    // starting over from the untouched source. This is the fix for the gap
    // the pre-conversation checkpoint left open — see session.rs's module
    // doc comment. The crash-resume path below (`resumable_run`, matching
    // on an *identical* intent) is unrelated and only applies to
    // session-less runs; a new conversational instruction never has the
    // same intent text as the previous turn, so it would never have
    // matched anyway.
    let mut resolved_session_id: Option<String> = None;
    let (stages, workspace) = if request.session_id.is_some() {
        let (session_id, workspace) =
            resolve_session_workspace(&state, &project, &project_path).await?;
        resolved_session_id = Some(session_id);
        (default_stages, workspace)
    } else {
        let resume = resumable_run(
            &state,
            intent,
            &project_path,
            &request.runbook_id,
            &request.capability_id,
            &request.capability_ids,
            &default_stages,
        )
        .await;
        if let Some((previous_id, stages, workspace)) = resume {
            if let Some(handle) = state.runtime_jobs.lock().await.remove(&previous_id) {
                handle.abort();
            }
            (stages, workspace)
        } else {
            let workspace = run_workspace_dir(&state.app_data_dir, &run_id);
            let source = project.clone();
            let destination = workspace.clone();
            tokio::task::spawn_blocking(move || copy_workspace(&source, &destination))
                .await
                .map_err(|e| AppError::Internal {
                    message: e.to_string(),
                })??;
            (default_stages, workspace)
        }
    };
    // Captures this turn's *starting* workspace state, whether that's a
    // brand-new copy or (for a session turn past the first) the state left
    // by the previous turn. `build_review`/`runtime_apply` diff against
    // this, so a session turn's review/apply naturally scopes to "what
    // changed this turn", not the whole conversation's accumulated diff.
    persist_manifest(&state.app_data_dir, &run_id, "initial", &workspace).await?;
    let now = Utc::now();
    let run = RunSummary {
        id: run_id,
        intent: intent.into(),
        project_path,
        workspace_path: Some(workspace.to_string_lossy().into_owned()),
        runbook_id: request.runbook_id,
        capability_id: request.capability_id,
        capability_ids: request.capability_ids,
        mission_id: request.mission_id,
        session_id: resolved_session_id,
        provider_id: request.provider_id,
        status: RunStatus::Queued,
        current_stage: None,
        stages,
        created_at: now,
        updated_at: now,
        completed_at: None,
        error: None,
    };
    persist(&state, &run).await?;

    let response = run.clone();
    let run_id = run.id.clone();
    let app_data = state.app_data_dir.clone();
    let jobs = state.runtime_jobs.clone();
    let settings = state.settings.clone();
    let task_run_id = run_id.clone();
    let handle = tokio::spawn(async move {
        let mut current = run.clone();
        // Fixed for the whole run — the same value `stage_prompt`'s
        // ACCEPTANCE CRITERIA list is built from, so a structured criteria
        // entry's `criterionIndex` is validated against the exact list the
        // model was actually shown.
        let criteria_len = mission
            .as_ref()
            .map(|m| m.acceptance_criteria.len())
            .unwrap_or(0);
        current.status = RunStatus::Running;
        current.updated_at = Utc::now();
        let _ = persist_at(&app_data, &current).await;
        let _ = on_event.send(RunEvent::RunUpdated {
            run: current.clone(),
        });
        let total = current.stages.len();
        for index in 0..total {
            if current.stages[index].status == "passed" {
                continue;
            }
            let stage_id = current.stages[index].id.clone();
            current.current_stage = Some(stage_id.clone());
            current.stages[index].status = "running".into();
            current.stages[index].attempt += 1;
            current.updated_at = Utc::now();
            let _ = persist_at(&app_data, &current).await;
            let _ = on_event.send(RunEvent::RunUpdated {
                run: current.clone(),
            });
            let mut passed = false;
            let is_qa = current.stages[index].kind == "qa" || stage_id == "qa";
            let is_reality = current.stages[index].kind == "reality" || stage_id == "reality-check";
            let uses_local_executor =
                current.provider_id.is_none() && stage_uses_local_executor(&current.stages[index]);
            let max_attempts = if is_qa { 3 } else { 1 };
            while current.stages[index].attempt <= max_attempts {
                let attempt = current.stages[index].attempt;
                let workspace = PathBuf::from(
                    current
                        .workspace_path
                        .as_deref()
                        .unwrap_or(&current.project_path),
                );
                if let Err(e) = persist_manifest(
                    &app_data,
                    &current.id,
                    &format!("{stage_id}-{attempt}-before"),
                    &workspace,
                )
                .await
                {
                    // Do not fail the run over a "before" snapshot — but do
                    // not swallow it either. A silently-dropped manifest
                    // write is exactly how a real problem (e.g. an invalid
                    // filename, a full disk) looked identical to "nothing
                    // happened yet".
                    let _ = on_event.send(RunEvent::Output {
                        run_id: current.id.clone(),
                        stage_id: stage_id.clone(),
                        stream: "system".into(),
                        text: format!(
                            "No se pudo guardar el manifiesto 'before' de esta etapa (evidencia incompleta, la etapa continúa igualmente): {e}"
                        ),
                    });
                }
                let prompt = stage_prompt(
                    &current,
                    index,
                    profiles.get(&current.stages[index].agent_slug),
                    mission.as_ref(),
                );
                // Stages `local_agent::local_stage_task` covers (direction,
                // architecture, development) run on the sovereign local
                // executor only when this run has no external `provider_id`
                // — i.e. sovereign-local is the *default* for those kinds,
                // not a forced floor. A run that explicitly picked Codex or
                // Claude Code uses it for every stage, direction through
                // reality: an explicit provider choice is a real decision
                // about where the actual creative/build work happens, not
                // just about who checks it afterward. Every other stage
                // kind (qa, reality) is unaffected — a missing `provider_id`
                // there still fails that stage explicitly, same as before.
                let stage_result: Result<bool, AppError> = if uses_local_executor {
                    local_agent::run_local_agentic_stage(
                        &app_data,
                        &workspace,
                        &current.intent,
                        &current.id,
                        &stage_id,
                        &current.stages[index].kind,
                        attempt,
                        mission.as_ref(),
                        &on_event,
                        &settings,
                    )
                    .await
                    .map(|result| result.passed)
                } else {
                    match current.provider_id.as_deref() {
                        Some(provider_id) => {
                            run_provider_stage(
                                provider_id,
                                &workspace,
                                &prompt,
                                &current.id,
                                &stage_id,
                                &current.stages[index].kind,
                                attempt,
                                &run_evidence_dir(&app_data, &current.id),
                                &on_event,
                                is_reality,
                                criteria_len,
                            )
                            .await
                        }
                        None => {
                            // Named dynamically, not hardcoded to "Dirección
                            // de proyecto": as more stage kinds gain a
                            // sovereign local executor (see
                            // `stage_uses_local_executor`), this message
                            // must keep crediting whichever of them actually
                            // passed, not just the first one that ever did.
                            let completed_locally: Vec<&str> = current.stages[..index]
                                .iter()
                                .filter(|s| s.status == "passed" && stage_uses_local_executor(s))
                                .map(|s| s.label.as_str())
                                .collect();
                            let completed_note = if completed_locally.is_empty() {
                                String::new()
                            } else {
                                let verb = if completed_locally.len() > 1 {
                                    "se completaron"
                                } else {
                                    "se completó"
                                };
                                format!(
                                    "{} {verb} con el motor local soberano. ",
                                    completed_locally.join(" y ")
                                )
                            };
                            Err(AppError::CapabilityProviderUnavailable {
                                capability_id: format!("stage.{}", current.stages[index].kind),
                                message: format!(
                                    "{completed_note}La etapa '{}' requiere un ejecutor externo (Codex o Claude) que no fue configurado para esta corrida; deteniendo de forma controlada.",
                                    current.stages[index].label
                                ),
                            })
                        }
                    }
                };
                match stage_result {
                    Ok(true) => {
                        if let Err(e) = persist_manifest(
                            &app_data,
                            &current.id,
                            &format!("{stage_id}-{attempt}-after"),
                            &workspace,
                        )
                        .await
                        {
                            let _ = on_event.send(RunEvent::Output {
                                run_id: current.id.clone(),
                                stage_id: stage_id.clone(),
                                stream: "system".into(),
                                text: format!(
                                    "No se pudo guardar el manifiesto 'after' de esta etapa (evidencia incompleta; la etapa igual se marca como pasada porque el agente entregó INTENTOS_GATE:PASS): {e}"
                                ),
                            });
                        }
                        passed = true;
                        break;
                    }
                    Ok(false) if is_qa && current.stages[index].attempt < max_attempts => {
                        let Some(development_index) = remediation_stage_index(&current, index)
                        else {
                            current.error =
                                Some("QA has no corresponding development stage".into());
                            break;
                        };
                        let attempt = current.stages[index].attempt;
                        let _ = on_event.send(RunEvent::GateFailed {
                            run_id: current.id.clone(),
                            stage_id: stage_id.clone(),
                            reason: "QA gate failed; returning to Development".into(),
                            attempt,
                        });
                        current.stages[index].status = "failed".into();
                        current.stages[development_index].status = "running".into();
                        current.stages[development_index].attempt += 1;
                        current.current_stage = Some(current.stages[development_index].id.clone());
                        current.updated_at = Utc::now();
                        let _ = persist_at(&app_data, &current).await;
                        let _ = on_event.send(RunEvent::RunUpdated {
                            run: current.clone(),
                        });
                        let fix_prompt = format!(
                            "The QA gate failed on attempt {attempt}. This is a remediation pass. Inspect the QA evidence and implement every in-scope requirement that is still missing; do not limit the repair to your catalog specialty. Fix the root causes and run the relevant checks.\n\n{}",
                            stage_prompt(&current, development_index, profiles.get(&current.stages[development_index].agent_slug), mission.as_ref())
                        );
                        // Practically unreachable with `provider_id: None`
                        // today: reaching a QA remediation pass means the QA
                        // stage itself already ran on a `Some` provider (a
                        // `None` provider fails a non-direction stage before
                        // it can ever gate), but the type is `Option` now,
                        // so this still has to handle it defensively rather
                        // than assume the invariant holds forever.
                        let remediation_result = match current.provider_id.as_deref() {
                            Some(provider_id) => {
                                run_provider_stage(
                                    provider_id,
                                    &workspace,
                                    &fix_prompt,
                                    &current.id,
                                    "development",
                                    &current.stages[development_index].kind,
                                    current.stages[development_index].attempt,
                                    &run_evidence_dir(&app_data, &current.id),
                                    &on_event,
                                    false, // always the development stage — never reality
                                    criteria_len,
                                )
                                .await
                            }
                            None => Err(AppError::CapabilityProviderUnavailable {
                                capability_id: "stage.development".into(),
                                message: "No hay proveedor externo configurado para la remediación de Development.".into(),
                            }),
                        };
                        if !remediation_result.unwrap_or(false) {
                            current.error = Some(
                                "development remediation did not provide INTENTOS_GATE:PASS".into(),
                            );
                            break;
                        }
                        current.stages[development_index].status = "passed".into();
                        current.stages[index].attempt += 1;
                        current.stages[index].status = "running".into();
                        current.current_stage = Some(stage_id.clone());
                        current.updated_at = Utc::now();
                        let _ = persist_at(&app_data, &current).await;
                        let _ = on_event.send(RunEvent::RunUpdated {
                            run: current.clone(),
                        });
                    }
                    Ok(false) => {
                        if let Err(e) = persist_manifest(
                            &app_data,
                            &current.id,
                            &format!("{stage_id}-{attempt}-after"),
                            &workspace,
                        )
                        .await
                        {
                            let _ = on_event.send(RunEvent::Output {
                                run_id: current.id.clone(),
                                stage_id: stage_id.clone(),
                                stream: "system".into(),
                                text: format!(
                                    "No se pudo guardar el manifiesto 'after' de esta etapa (evidencia incompleta): {e}"
                                ),
                            });
                        }
                        let _ = on_event.send(RunEvent::GateFailed {
                            run_id: current.id.clone(),
                            stage_id: stage_id.clone(),
                            reason: "stage did not provide INTENTOS_GATE:PASS".into(),
                            attempt,
                        });
                        break;
                    }
                    Err(e) => {
                        current.error = Some(clean_text(&e.to_string()));
                        break;
                    }
                }
            }
            if passed {
                current.stages[index].status = "passed".into();
                let _ = on_event.send(RunEvent::GatePassed {
                    run_id: current.id.clone(),
                    stage_id,
                });
            } else {
                let reason = current
                    .error
                    .clone()
                    .unwrap_or_else(|| "stage did not provide INTENTOS_GATE:PASS".into());
                fail_run(&mut current, index, &reason);
                break;
            }
            let _ = persist_at(&app_data, &current).await;
        }
        if current.status == RunStatus::Running {
            current.status = RunStatus::Succeeded;
            current.current_stage = None;
            current.completed_at = Some(Utc::now());
            current.updated_at = Utc::now();
        }
        let _ = persist_at(&app_data, &current).await;
        let _ = on_event.send(RunEvent::RunUpdated { run: current });
        jobs.lock().await.remove(&task_run_id);
    });
    state
        .runtime_jobs
        .lock()
        .await
        .insert(run_id, handle.abort_handle());
    Ok(response)
}

fn fail_run(run: &mut RunSummary, index: usize, reason: &str) {
    run.status = RunStatus::Failed;
    for stage in &mut run.stages {
        if stage.status == "running" {
            stage.status = "failed".into();
        }
    }
    run.stages[index].status = "failed".into();
    run.error = Some(clean_text(reason));
    run.current_stage = None;
    run.completed_at = Some(Utc::now());
    run.updated_at = Utc::now();
}

fn remediation_stage_index(run: &RunSummary, qa_index: usize) -> Option<usize> {
    let qa_prefix = run.stages[qa_index].id.split(':').next();
    run.stages
        .iter()
        .enumerate()
        .take(qa_index)
        .rev()
        .find(|(_, stage)| {
            (stage.kind == "development" || stage.id == "development")
                && (qa_prefix == Some("qa") || stage.id.split(':').next() == qa_prefix)
        })
        .map(|(index, _)| index)
        .or_else(|| {
            run.stages
                .iter()
                .enumerate()
                .take(qa_index)
                .rev()
                .find(|(_, stage)| stage.kind == "development" || stage.id == "development")
                .map(|(index, _)| index)
        })
}

async fn persist_at(app_data: &Path, run: &RunSummary) -> Result<(), AppError> {
    let dir = app_data.join("state").join("runs");
    tokio::fs::create_dir_all(&dir).await?;
    atomic_write(
        &dir.join(format!("{}.json", run.id)),
        &serde_json::to_vec_pretty(run)?,
    )
    .await
}

fn stage_prompt(
    run: &RunSummary,
    index: usize,
    persona: Option<&String>,
    mission: Option<&Mission>,
) -> String {
    let s = &run.stages[index];
    // Additive only: when no Mission is attached, brief_section is empty
    // and the format! below is byte-identical to Runtime v0.1's original
    // output. When a Mission is attached, its deterministic brief is
    // interpolated ahead of the raw intent — nothing about USER INTENT or
    // PROJECT below changes.
    let brief_section = match mission {
        Some(m) => format!("MISSION BRIEF:\n{}\n\n", mission::mission_brief(m)),
        None => String::new(),
    };
    let implementation_scope = if (s.kind == "development" || s.id == "development")
        && (s.id.starts_with("iot:") || run.capability_id == "iot")
    {
        "\nIOT FULL-VERTICAL IMPLEMENTATION MANDATE:\n- This stage owns the complete local prototype described by the user, not firmware alone.\n- Preserve and verify existing firmware work, and also implement the in-scope MQTT path, consumer/backend, persistence, alert rules, API, and responsive dashboard when the intent requires them.\n- Hardware purchase and physical validation remain out of scope unless explicitly authorized; use reproducible local simulation for those boundaries.\n- Read QA evidence already present in the workspace and close every actionable in-scope gap before claiming PASS.\n"
    } else {
        ""
    };
    // Domain-conditional verification guidance — see "Reality gate —
    // domain-conditional verification guidance" above. Additive and
    // reality-stage-only: every other stage's prompt is byte-identical to
    // before.
    let domain_guidance = if s.kind == "reality" || s.id == "reality-check" {
        reality_domain_section(run)
    } else {
        String::new()
    };
    // Shadow-mode structured criteria report — see the "Reality gate —
    // structured criteria" block above `output_gate_passed`. Additive and
    // reality-stage-only: every other stage's prompt is byte-identical to
    // before. Asked in addition to, never instead of, the existing narrative
    // report and INTENTOS_GATE line — the sentinel below remains what this
    // stage's PASS/FAIL is actually decided by.
    let criteria_instruction = if s.kind == "reality" || s.id == "reality-check" {
        "\nSTRUCTURED CRITERIA REPORT (additional telemetry — your narrative report and INTENTOS_GATE verdict below still govern this stage's outcome):\nBefore your final INTENTOS_GATE line, add a line reading exactly `INTENTOS_CRITERIA:` followed by a JSON array with one object per criterion listed under ACCEPTANCE CRITERIA above, in the same order, each shaped as {\"criterionIndex\": <0-based position in that ACCEPTANCE CRITERIA list>, \"criterion\": \"<verbatim criterion text>\", \"status\": \"pass\"|\"fail\"|\"unverified\", \"evidence\": \"<short pointer to what you actually checked>\", \"explanation\": \"<1-2 sentences>\"}. Use \"unverified\" honestly when you could not check a criterion. Omit this block entirely if there are no ACCEPTANCE CRITERIA above.\n"
    } else {
        ""
    };
    let workspace = run.workspace_path.as_deref().unwrap_or(&run.project_path);
    format!("You are the {} agent ({}) in the IntentOS '{}' autonomous pipeline.\n\nINTENTOS ORCHESTRATOR OVERRIDES (highest priority for this run):\n- The USER INTENT below is the authoritative product specification.\n- Catalog persona references to missing templates, memory-bank files, frameworks, scripts, or organizational conventions are optional guidance, not prerequisites.\n- If useful project documentation is missing, create the minimal appropriate documentation yourself from the USER INTENT and continue autonomously.\n- Choose reasonable technical defaults when the user explicitly delegates the choice. Do not fail merely because an auxiliary file, preferred framework, or prior setup is absent.\n- Do not ask the user to implement or configure anything unless human authorization is genuinely required.\n- Stay within the requested scope and do not invent product requirements.\n- This is an isolated working copy. Never access or modify the source project outside WORKSPACE.\n- This invocation is a single non-interactive turn: there is no later turn, notification, or check-in where you could pick up a deferred background task. Run commands that must finish before you continue (npm install, builds, migrations, etc.) synchronously in the foreground and wait for their real exit code — never launch them as a background task expecting to resume afterward, since nothing will ever resume this turn. If a command is genuinely slow, wait for it; do not end your response early with an intention to continue later.\n{}\nCATALOG PERSONA INSTRUCTIONS:\n{}\n\n{}USER INTENT:\n{}\nSOURCE PROJECT (read-only reference; do not access): {}\nWORKSPACE: {}\n{}{}\nWork only inside WORKSPACE. Inspect existing work and perform this stage for real. Run relevant checks. Do not claim success without evidence. End your final response with exactly INTENTOS_GATE:PASS only if this stage genuinely passes; otherwise end with INTENTOS_GATE:FAIL and explain a genuine blocker. Previous stages are present in the workspace.", s.label, s.agent_slug, run.runbook_id, implementation_scope, persona.map(String::as_str).unwrap_or("Catalog persona unavailable; disclose this limitation."), brief_section, run.intent, run.project_path, workspace, domain_guidance, criteria_instruction)
}

async fn run_codex_stage(
    project: &Path,
    prompt: &str,
    run_id: &str,
    stage_id: &str,
    stage_kind: &str,
    attempt: u8,
    evidence_dir: &Path,
    channel: &Channel<RunEvent>,
    is_reality: bool,
    criteria_len: usize,
) -> Result<bool, AppError> {
    // Complex architecture and implementation stages routinely exceed thirty
    // minutes. The former 30-minute ceiling killed healthy Codex processes at
    // an arbitrary wall-clock boundary and incorrectly marked them as agent
    // failures. Keep a finite safety ceiling, but size it for real production.
    const MAX_STAGE_RUNTIME: Duration = Duration::from_secs(60 * 60 * 2);
    let execution = async {
        // Executor↔runtime contract (P0): a claimed INTENTOS_GATE:PASS from
        // an external CLI is only credible for stage kinds that are
        // expected to materialize real work if the workspace actually
        // changed. Snapshotting here (not reusing the outer loop's
        // before/after manifest files) keeps this self-contained and never
        // risks the already-verified before/after evidence write path.
        let before_manifest = {
            let root = project.to_path_buf();
            tokio::task::spawn_blocking(move || workspace_manifest(&root))
                .await
                .map_err(|e| AppError::Internal {
                    message: e.to_string(),
                })??
        };
        let mut child = Command::new(codex_binary())
            .args([
                "exec",
                "--json",
                "--sandbox",
                "workspace-write",
                "--skip-git-repo-check",
            ])
            .arg("--cd")
            .arg(project)
            .arg("-")
            .current_dir(project)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| AppError::Io {
                message: format!("could not start Codex CLI: {e}"),
            })?;
        let mut stdin = child.stdin.take().ok_or_else(|| AppError::Internal {
            message: "runtime stdin unavailable".into(),
        })?;
        stdin.write_all(prompt.as_bytes()).await?;
        stdin.shutdown().await?;
        let stdout = child.stdout.take().ok_or_else(|| AppError::Internal {
            message: "runtime stdout unavailable".into(),
        })?;
        let stderr = child.stderr.take().ok_or_else(|| AppError::Internal {
            message: "runtime stderr unavailable".into(),
        })?;
        let err_channel = channel.clone();
        let err_run = run_id.to_string();
        let err_stage = stage_id.to_string();
        let stderr_task = tokio::spawn(async move {
            let mut captured = String::new();
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                captured.push_str(&clean_text(&line));
                captured.push('\n');
                let _ = err_channel.send(RunEvent::Output {
                    run_id: err_run.clone(),
                    stage_id: err_stage.clone(),
                    stream: "stderr".into(),
                    text: clean_text(&line),
                });
            }
            captured
        });
        let mut lines = BufReader::new(stdout).lines();
        let mut combined = String::new();
        // Inactivity watchdog: MAX_STAGE_RUNTIME is a hard safety ceiling on
        // total wall-clock time, not a signal of health — a stage that is
        // genuinely working for 90 minutes and one that has been silently
        // hung for 90 minutes look identical to it. Track time since the
        // last real line of output instead, and surface a distinct "system"
        // event when that goes quiet for too long. This never kills the
        // stage on its own — a human can decide to cancel; it only makes a
        // stall observable instead of indistinguishable from progress.
        const STALL_WARNING_INTERVAL: Duration = Duration::from_secs(5 * 60);
        let mut last_progress_at = tokio::time::Instant::now();
        // `next_check_at` is deliberately separate from `last_progress_at`:
        // it is when the watchdog should look again, not when progress last
        // happened. Reusing `last_progress_at` for both would make the
        // deadline stay in the past forever once a stall starts, firing the
        // warning on every loop iteration instead of every 5 minutes.
        let mut next_check_at = last_progress_at + STALL_WARNING_INTERVAL;
        let mut currently_stalled = false;
        loop {
            tokio::select! {
                line = lines.next_line() => {
                    let Some(line) = line? else { break; };
                    let text = clean_text(&line);
                    combined.push_str(&text);
                    combined.push('\n');
                    last_progress_at = tokio::time::Instant::now();
                    next_check_at = last_progress_at + STALL_WARNING_INTERVAL;
                    if currently_stalled {
                        currently_stalled = false;
                        let _ = channel.send(RunEvent::Output {
                            run_id: run_id.into(),
                            stage_id: stage_id.into(),
                            stream: "system".into(),
                            text: "El agente volvió a producir salida; ya no está inactivo.".into(),
                        });
                    }
                    let _ = channel.send(RunEvent::Output {
                        run_id: run_id.into(),
                        stage_id: stage_id.into(),
                        stream: "stdout".into(),
                        text,
                    });
                }
                _ = tokio::time::sleep_until(next_check_at) => {
                    currently_stalled = true;
                    let waited = last_progress_at.elapsed().as_secs();
                    let _ = channel.send(RunEvent::Output {
                        run_id: run_id.into(),
                        stage_id: stage_id.into(),
                        stream: "system".into(),
                        text: format!(
                            "[watchdog] Sin salida nueva del agente hace {waited}s. Puede seguir trabajando en segundo plano (límite duro: 2h); si crees que está bloqueado, puedes cancelar la ejecución."
                        ),
                    });
                    next_check_at = tokio::time::Instant::now() + STALL_WARNING_INTERVAL;
                }
            }
        }
        let status = child.wait().await?;
        let captured_stderr = stderr_task.await.unwrap_or_default();
        tokio::fs::create_dir_all(evidence_dir).await?;
        let evidence_stage_id = evidence_file_component(stage_id);
        atomic_write(
            &evidence_dir.join(format!("{evidence_stage_id}-{attempt}.stdout.log")),
            combined.as_bytes(),
        )
        .await?;
        atomic_write(
            &evidence_dir.join(format!("{evidence_stage_id}-{attempt}.stderr.log")),
            captured_stderr.as_bytes(),
        )
        .await?;
        let sentinel_passed = output_gate_passed(&combined);
        if is_reality {
            record_reality_shadow_evidence(
                evidence_dir,
                &evidence_stage_id,
                attempt,
                run_id,
                stage_id,
                &combined,
                criteria_len,
                sentinel_passed,
            )
            .await;
        }
        let strict_enabled = reality_gate_strict_enabled();
        let after_manifest = {
            let root = project.to_path_buf();
            tokio::task::spawn_blocking(move || workspace_manifest(&root))
                .await
                .map_err(|e| AppError::Internal {
                    message: e.to_string(),
                })??
        };
        let workspace_changed = !manifest_changes(&before_manifest, &after_manifest).is_empty();
        let (final_passed, diagnosis, strict_reason) = evaluate_external_stage_completion(
            stage_kind,
            status.success(),
            &combined,
            workspace_changed,
            is_reality,
            strict_enabled,
            criteria_len,
        );
        if let Some(reason) = &strict_reason {
            // Persisted alongside the existing stdout/stderr/manifest/
            // criteria evidence — the specific, diagnosable reason behind
            // the strict verdict must outlive the log line.
            let _ = atomic_write(
                &evidence_dir.join(format!("{evidence_stage_id}-{attempt}.reality-verdict.txt")),
                reason.as_bytes(),
            )
            .await;
            tracing::warn!(
                run_id = %run_id,
                stage_id = %stage_id,
                attempt,
                sentinel_passed,
                final_passed,
                reason = %reason,
                "Reality gate strict mode: structured verdict is authoritative for this stage"
            );
        }
        let _ = atomic_write(
            &evidence_dir.join(format!("{evidence_stage_id}-{attempt}.completion.json")),
            &serde_json::to_vec_pretty(&diagnosis).unwrap_or_default(),
        )
        .await;
        if !matches!(diagnosis, StageCompletionDiagnosis::Completed) {
            tracing::warn!(
                run_id = %run_id,
                stage_id = %stage_id,
                attempt,
                workspace_changed,
                ?diagnosis,
                "External executor stage did not complete per the executor\u{2194}runtime contract"
            );
        }
        if contains_deferred_continuation_claim(&combined) {
            let _ = channel.send(RunEvent::Output {
                run_id: run_id.into(),
                stage_id: stage_id.into(),
                stream: "system".into(),
                text: "ADVERTENCIA: el texto del agente sugiere que continuará este trabajo en un futuro turno; esta es una invocación one-shot y no existe un futuro turno que la retome. Este aviso es informativo y no decide por sí solo si la etapa pasó.".into(),
            });
        }
        Ok::<bool, AppError>(diagnosis.passed())
    };
    tokio::time::timeout(MAX_STAGE_RUNTIME, execution)
        .await
        .map_err(|_| AppError::Io {
            message: "runtime stage exceeded the 2-hour safety limit".into(),
        })?
}

/// Claude Code CLI channel. Deliberately a full, independent copy of
/// run_codex_stage's process/watchdog/evidence machinery rather than a
/// shared abstraction: run_codex_stage is already verified (real cargo
/// test + cargo build runs, plus a live end-to-end run) and this addition
/// must not risk that path. Only the child-process spawn spec differs.
///
/// Flags:
/// - `-p` (print mode) with no query argument + the prompt piped over
///   stdin: same non-interactive, single-shot invocation shape as Codex's
///   `exec ... -`.
/// - `--output-format stream-json --verbose`: line-delimited JSON events,
///   same treatment as Codex's `--json` — we do not parse the JSON
///   structure, we just forward each raw line as evidence/output. The
///   final assistant message text (and therefore the INTENTOS_GATE
///   sentinel) is still a literal substring of that JSON, so
///   output_gate_passed keeps working unmodified.
/// - `--dangerously-skip-permissions`: without it, Claude Code would stop
///   to ask interactive permission for file writes / bash commands, which
///   has no TTY to answer it here and would hang indefinitely — exactly
///   the silent-stall failure mode this whole audit exists to prevent.
///   This is the direct analog of Codex's `--sandbox workspace-write`: the
///   process only ever runs inside `workspace`, which copy_workspace()
///   already isolated from the user's real project before either provider
///   is invoked. Full write access inside that throwaway copy carries the
///   same blast radius as Codex's sandbox, not a larger one.
async fn run_claude_stage(
    project: &Path,
    prompt: &str,
    run_id: &str,
    stage_id: &str,
    stage_kind: &str,
    attempt: u8,
    evidence_dir: &Path,
    channel: &Channel<RunEvent>,
    is_reality: bool,
    criteria_len: usize,
) -> Result<bool, AppError> {
    const MAX_STAGE_RUNTIME: Duration = Duration::from_secs(60 * 60 * 2);
    let execution = async {
        // See the matching comment in run_codex_stage: same executor↔runtime
        // contract (P0), snapshotted independently per provider so neither
        // path's already-verified behavior depends on the other.
        let before_manifest = {
            let root = project.to_path_buf();
            tokio::task::spawn_blocking(move || workspace_manifest(&root))
                .await
                .map_err(|e| AppError::Internal {
                    message: e.to_string(),
                })??
        };
        let mut child = claude_command()
            .args([
                "-p",
                "--output-format",
                "stream-json",
                "--verbose",
                "--dangerously-skip-permissions",
            ])
            .current_dir(project)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| AppError::Io {
                message: format!("could not start Claude Code: {e}"),
            })?;
        let mut stdin = child.stdin.take().ok_or_else(|| AppError::Internal {
            message: "runtime stdin unavailable".into(),
        })?;
        // Write stdin concurrently with reading stdout instead of writing it
        // all up front. Claude Code starts emitting stdout (its initial
        // `{"type":"system","subtype":"init",...}` line) almost immediately
        // on startup, before it has necessarily finished reading stdin. A
        // small manual test prompt never fills the OS pipe buffer, so the
        // old sequential write-then-read never showed a problem — but a real
        // production-sized prompt (several KB, catalog persona + mission
        // brief + intent) is big enough that the child can block writing to
        // an unread stdout pipe while we are still blocked writing the tail
        // of stdin, deadlocking both sides forever with zero output. Moving
        // the write into its own task (mirroring `stderr_task` below) lets
        // both directions make progress at the same time.
        let prompt_owned = prompt.to_string();
        let stdin_task = tokio::spawn(async move {
            stdin.write_all(prompt_owned.as_bytes()).await?;
            stdin.shutdown().await?;
            Ok::<(), std::io::Error>(())
        });
        let stdout = child.stdout.take().ok_or_else(|| AppError::Internal {
            message: "runtime stdout unavailable".into(),
        })?;
        let stderr = child.stderr.take().ok_or_else(|| AppError::Internal {
            message: "runtime stderr unavailable".into(),
        })?;
        let err_channel = channel.clone();
        let err_run = run_id.to_string();
        let err_stage = stage_id.to_string();
        let stderr_task = tokio::spawn(async move {
            let mut captured = String::new();
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                captured.push_str(&clean_text(&line));
                captured.push('\n');
                let _ = err_channel.send(RunEvent::Output {
                    run_id: err_run.clone(),
                    stage_id: err_stage.clone(),
                    stream: "stderr".into(),
                    text: clean_text(&line),
                });
            }
            captured
        });
        let mut lines = BufReader::new(stdout).lines();
        let mut combined = String::new();
        const STALL_WARNING_INTERVAL: Duration = Duration::from_secs(5 * 60);
        let mut last_progress_at = tokio::time::Instant::now();
        let mut next_check_at = last_progress_at + STALL_WARNING_INTERVAL;
        let mut currently_stalled = false;
        loop {
            tokio::select! {
                line = lines.next_line() => {
                    let Some(line) = line? else { break; };
                    let text = clean_text(&line);
                    combined.push_str(&text);
                    combined.push('\n');
                    last_progress_at = tokio::time::Instant::now();
                    next_check_at = last_progress_at + STALL_WARNING_INTERVAL;
                    if currently_stalled {
                        currently_stalled = false;
                        let _ = channel.send(RunEvent::Output {
                            run_id: run_id.into(),
                            stage_id: stage_id.into(),
                            stream: "system".into(),
                            text: "El agente volvió a producir salida; ya no está inactivo.".into(),
                        });
                    }
                    let _ = channel.send(RunEvent::Output {
                        run_id: run_id.into(),
                        stage_id: stage_id.into(),
                        stream: "stdout".into(),
                        text,
                    });
                }
                _ = tokio::time::sleep_until(next_check_at) => {
                    currently_stalled = true;
                    let waited = last_progress_at.elapsed().as_secs();
                    let _ = channel.send(RunEvent::Output {
                        run_id: run_id.into(),
                        stage_id: stage_id.into(),
                        stream: "system".into(),
                        text: format!(
                            "[watchdog] Sin salida nueva del agente hace {waited}s. Puede seguir trabajando en segundo plano (límite duro: 2h); si crees que está bloqueado, puedes cancelar la ejecución."
                        ),
                    });
                    next_check_at = tokio::time::Instant::now() + STALL_WARNING_INTERVAL;
                }
            }
        }
        let status = child.wait().await?;
        let captured_stderr = stderr_task.await.unwrap_or_default();
        // The write task should already be finished (stdout is closed once
        // the process exits, which only happens after it's done reading
        // stdin in practice), so this join is just to surface a genuine
        // write failure rather than silently discarding it — same spirit as
        // `stderr_task` above, but the write side is fallible, not just
        // absent, so unwrap_or_default() would hide a real error here.
        match stdin_task.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                return Err(AppError::Io {
                    message: format!("failed writing prompt to Claude Code stdin: {e}"),
                });
            }
            Err(e) => {
                return Err(AppError::Internal {
                    message: format!("stdin writer task panicked: {e}"),
                });
            }
        }
        tokio::fs::create_dir_all(evidence_dir).await?;
        let evidence_stage_id = evidence_file_component(stage_id);
        atomic_write(
            &evidence_dir.join(format!("{evidence_stage_id}-{attempt}.stdout.log")),
            combined.as_bytes(),
        )
        .await?;
        atomic_write(
            &evidence_dir.join(format!("{evidence_stage_id}-{attempt}.stderr.log")),
            captured_stderr.as_bytes(),
        )
        .await?;
        let sentinel_passed = output_gate_passed(&combined);
        if is_reality {
            record_reality_shadow_evidence(
                evidence_dir,
                &evidence_stage_id,
                attempt,
                run_id,
                stage_id,
                &combined,
                criteria_len,
                sentinel_passed,
            )
            .await;
        }
        let strict_enabled = reality_gate_strict_enabled();
        let after_manifest = {
            let root = project.to_path_buf();
            tokio::task::spawn_blocking(move || workspace_manifest(&root))
                .await
                .map_err(|e| AppError::Internal {
                    message: e.to_string(),
                })??
        };
        let workspace_changed = !manifest_changes(&before_manifest, &after_manifest).is_empty();
        let (final_passed, diagnosis, strict_reason) = evaluate_external_stage_completion(
            stage_kind,
            status.success(),
            &combined,
            workspace_changed,
            is_reality,
            strict_enabled,
            criteria_len,
        );
        if let Some(reason) = &strict_reason {
            // Persisted alongside the existing stdout/stderr/manifest/
            // criteria evidence — the specific, diagnosable reason behind
            // the strict verdict must outlive the log line.
            let _ = atomic_write(
                &evidence_dir.join(format!("{evidence_stage_id}-{attempt}.reality-verdict.txt")),
                reason.as_bytes(),
            )
            .await;
            tracing::warn!(
                run_id = %run_id,
                stage_id = %stage_id,
                attempt,
                sentinel_passed,
                final_passed,
                reason = %reason,
                "Reality gate strict mode: structured verdict is authoritative for this stage"
            );
        }
        // Executor↔runtime contract evidence (P0) — always written, not
        // only on a contract violation, so "why did this attempt end up
        // this way" never depends on remembering to check a second place
        // only when something went wrong.
        let _ = atomic_write(
            &evidence_dir.join(format!("{evidence_stage_id}-{attempt}.completion.json")),
            &serde_json::to_vec_pretty(&diagnosis).unwrap_or_default(),
        )
        .await;
        if !matches!(diagnosis, StageCompletionDiagnosis::Completed) {
            tracing::warn!(
                run_id = %run_id,
                stage_id = %stage_id,
                attempt,
                workspace_changed,
                ?diagnosis,
                "External executor stage did not complete per the executor\u{2194}runtime contract"
            );
        }
        if contains_deferred_continuation_claim(&combined) {
            let _ = channel.send(RunEvent::Output {
                run_id: run_id.into(),
                stage_id: stage_id.into(),
                stream: "system".into(),
                text: "ADVERTENCIA: el texto del agente sugiere que continuará este trabajo en un futuro turno; esta es una invocación one-shot y no existe un futuro turno que la retome. Este aviso es informativo y no decide por sí solo si la etapa pasó.".into(),
            });
        }
        Ok::<bool, AppError>(diagnosis.passed())
    };
    tokio::time::timeout(MAX_STAGE_RUNTIME, execution)
        .await
        .map_err(|_| AppError::Io {
            message: "runtime stage exceeded the 2-hour safety limit".into(),
        })?
}

/// Single dispatch point so the two call sites inside runtime_start's spawned
/// task do not need to know which provider a run picked — they just pass the
/// RunSummary's own `provider_id` through. Keeping this as an explicit match
/// (rather than a trait object or function pointer) makes an unsupported id
/// a compile-visible, exhaustively-checked error path instead of a silent
/// fallback to the wrong provider.
async fn run_provider_stage(
    provider_id: &str,
    project: &Path,
    prompt: &str,
    run_id: &str,
    stage_id: &str,
    stage_kind: &str,
    attempt: u8,
    evidence_dir: &Path,
    channel: &Channel<RunEvent>,
    is_reality: bool,
    criteria_len: usize,
) -> Result<bool, AppError> {
    if provider_id == CLAUDE_PROVIDER_ID {
        run_claude_stage(
            project,
            prompt,
            run_id,
            stage_id,
            stage_kind,
            attempt,
            evidence_dir,
            channel,
            is_reality,
            criteria_len,
        )
        .await
    } else {
        run_codex_stage(
            project,
            prompt,
            run_id,
            stage_id,
            stage_kind,
            attempt,
            evidence_dir,
            channel,
            is_reality,
            criteria_len,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Regression test for the "before"/"after" workspace-manifest bug: the
    // stdout/stderr evidence writer already sanitized composite stage ids
    // (see `evidence_filename_is_portable_for_composite_stage_ids` below),
    // but `persist_manifest` built its filename from the raw, unsanitized
    // name at three call sites in the run loop. On Windows, a stage id like
    // "systems-data:architecture" made the rename from the `.tmp` sibling
    // fail with os error 87 ("the parameter is incorrect"), because NTFS
    // reads the `:` as an Alternate Data Stream separator — and because
    // those calls were `let _ = ...`, the failure was completely silent.
    // This exercises `persist_manifest` itself (now the single chokepoint
    // that sanitizes `name`) end to end against a real temp directory, so a
    // regression here fails loudly instead of vanishing again.
    #[tokio::test]
    async fn persist_manifest_sanitizes_composite_names_on_disk() {
        let app_data = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        fs::write(workspace.path().join("a.txt"), b"hello").unwrap();

        persist_manifest(
            app_data.path(),
            "run-1",
            "systems-data:architecture-1-before",
            workspace.path(),
        )
        .await
        .expect("persist_manifest must not fail on a colon-bearing stage id");

        let evidence_dir = run_evidence_dir(app_data.path(), "run-1");
        assert!(
            evidence_dir
                .join("systems-data_architecture-1-before.manifest.json")
                .exists(),
            "manifest should be written under the sanitized filename"
        );
        assert!(
            !evidence_dir
                .join("systems-data:architecture-1-before.manifest.json")
                .exists(),
            "the raw colon-bearing name must never be used as a path component"
        );
        // The failure mode this regresses against also left a stray,
        // zero-byte file at the pre-colon prefix (Windows silently created
        // the ADS "base" file before the rename failed). Assert it is gone
        // too, so a future regression can't hide behind "the real manifest
        // exists elsewhere" while still leaving this litter behind.
        assert!(!evidence_dir.join("systems-data").exists());
    }
    fn valid_request() -> StartRunRequest {
        StartRunRequest {
            intent: "Build a verified product".into(),
            project_path: "/tmp/project".into(),
            runbook_id: "startup-mvp".into(),
            capability_id: "digital-experience".into(),
            capability_ids: vec!["digital-experience".into()],
            stage_ids: Vec::new(),
            stage_kinds: Vec::new(),
            stage_labels: (1..=5).map(|index| format!("Stage {index}")).collect(),
            agent_slugs: (1..=5).map(|index| format!("agent-{index}")).collect(),
            provider_id: Some(PROVIDER_ID.into()),
            mission_id: None,
            session_id: None,
        }
    }
    #[test]
    fn evidence_filename_is_portable_for_composite_stage_ids() {
        assert_eq!(
            evidence_file_component("systems-data:architecture"),
            "systems-data_architecture"
        );
        assert_eq!(
            evidence_file_component("bad<>:\"/\\|?*stage. "),
            "bad_stage"
        );
        assert_eq!(evidence_file_component(":"), "stage");
    }
    #[test]
    fn rejects_root_project() {
        let root = Path::new(if cfg!(windows) { r"C:\" } else { "/" });
        assert!(validate_project(root.to_str().unwrap(), Path::new("/tmp/app")).is_err());
    }
    #[test]
    fn creates_five_real_pipeline_stages() {
        let s = initial_stages(&[], &[], &[], &[]);
        assert_eq!(s.len(), 5);
        assert_eq!(s[3].id, "qa");
        assert_eq!(s[4].id, "reality-check");
    }
    #[test]
    fn preserves_capability_roster_and_stage_labels() {
        let agents = [
            "pm",
            "iot-architect",
            "firmware-engineer",
            "device-qa",
            "reality",
        ]
        .map(str::to_string);
        let labels = [
            "Dirección",
            "Arquitectura IoT",
            "Firmware",
            "QA físico",
            "Reality Check",
        ]
        .map(str::to_string);
        let ids = [
            "direction",
            "iot:architecture",
            "iot:development",
            "iot:qa",
            "reality-check",
        ]
        .map(str::to_string);
        let kinds =
            ["direction", "architecture", "development", "qa", "reality"].map(str::to_string);
        let s = initial_stages(&agents, &labels, &ids, &kinds);
        assert_eq!(s[1].label, "Arquitectura IoT");
        assert_eq!(s[2].agent_slug, "firmware-engineer");
        assert_eq!(s[3].label, "QA físico");
    }
    #[test]
    fn composes_variable_pipeline_and_maps_each_qa_to_its_developer() {
        let ids = [
            "direction",
            "digital-experience:architecture",
            "systems-data:architecture",
            "digital-experience:development",
            "systems-data:development",
            "digital-experience:qa",
            "systems-data:qa",
            "reality-check",
        ]
        .map(str::to_string);
        let kinds = [
            "direction",
            "architecture",
            "architecture",
            "development",
            "development",
            "qa",
            "qa",
            "reality",
        ]
        .map(str::to_string);
        let labels = ids.clone();
        let agents = ids.clone().map(|id| format!("agent-{id}"));
        let stages = initial_stages(&agents, &labels, &ids, &kinds);
        let now = Utc::now();
        let run = RunSummary {
            id: "multi".into(),
            intent: "hybrid".into(),
            project_path: "/tmp/project".into(),
            workspace_path: None,
            runbook_id: "startup-mvp".into(),
            capability_id: "digital-experience".into(),
            capability_ids: vec!["digital-experience".into(), "systems-data".into()],
            mission_id: None,
            session_id: None,
            provider_id: Some(PROVIDER_ID.into()),
            status: RunStatus::Running,
            current_stage: None,
            stages,
            created_at: now,
            updated_at: now,
            completed_at: None,
            error: None,
        };
        assert_eq!(run.stages.len(), 8);
        assert_eq!(remediation_stage_index(&run, 5), Some(3));
        assert_eq!(remediation_stage_index(&run, 6), Some(4));
    }
    #[test]
    fn truncates_and_redacts_output() {
        assert!(!clean_text("OPENAI_API_KEY=secret").contains("secret"));
        assert!(
            clean_text(&"x".repeat(MAX_EVENT_TEXT_CHARS + 5))
                .chars()
                .count()
                <= MAX_EVENT_TEXT_CHARS
        );
    }
    #[test]
    fn gate_uses_the_last_explicit_verdict() {
        assert!(output_gate_passed("work\nINTENTOS_GATE:PASS"));
        assert!(!output_gate_passed("work\nINTENTOS_GATE:FAIL"));
        assert!(!output_gate_passed("no explicit verdict"));
        assert!(output_gate_passed(
            "quoted INTENTOS_GATE:FAIL\nfinal INTENTOS_GATE:PASS"
        ));
        assert!(!output_gate_passed(
            "quoted INTENTOS_GATE:PASS\nfinal INTENTOS_GATE:FAIL"
        ));
    }

    // ---- Executor↔runtime contract (P0) — evaluate_external_stage_completion ----
    //
    // Cases A-E named to match the mandate's own acceptance criteria for
    // this audit item, plus two negative controls (qa-kind exemption and
    // the deferred-continuation heuristic never gating on its own).

    #[test]
    fn case_a_completes_correctly_with_real_workspace_evidence() {
        let (passed, diagnosis, _) = evaluate_external_stage_completion(
            "architecture",
            true,
            "Wrote the architecture doc.\nINTENTOS_GATE:PASS",
            true, // workspace really changed
            false,
            false,
            0,
        );
        assert!(passed);
        assert_eq!(diagnosis, StageCompletionDiagnosis::Completed);
    }

    #[test]
    fn case_b_process_ends_with_no_gate_marker_is_incomplete_not_fail() {
        let (passed, diagnosis, _) = evaluate_external_stage_completion(
            "development",
            true,
            "The process printed some progress and then just... ended.",
            true,
            false,
            false,
            0,
        );
        assert!(!passed);
        assert!(matches!(diagnosis, StageCompletionDiagnosis::Incomplete(_)));
    }

    #[test]
    fn case_c_claims_pass_but_workspace_never_changed_is_contract_violated() {
        let (passed, diagnosis, _) = evaluate_external_stage_completion(
            "architecture",
            true,
            "Everything looks good.\nINTENTOS_GATE:PASS",
            false, // no real file changed
            false,
            false,
            0,
        );
        assert!(!passed);
        assert!(matches!(
            diagnosis,
            StageCompletionDiagnosis::ContractViolated(_)
        ));
    }

    #[test]
    fn case_d_deferred_continuation_language_is_flagged_but_does_not_gate_on_its_own() {
        let output = "I started the migration.\nI'll continue once it finishes in the background.\nINTENTOS_GATE:PASS";
        assert!(contains_deferred_continuation_claim(output));
        // The heuristic is advisory only: a stage that otherwise satisfies
        // the real contract (workspace changed, exit ok, explicit PASS)
        // must still pass — the mandate is explicit that this signal alone
        // must never gate, only warn.
        let (passed, diagnosis, _) =
            evaluate_external_stage_completion("development", true, output, true, false, false, 0);
        assert!(passed);
        assert_eq!(diagnosis, StageCompletionDiagnosis::Completed);
    }

    #[test]
    fn case_e_legitimate_background_preview_process_does_not_cause_a_false_fail() {
        let output = "Started the dev server; it will keep running in the background for the Showroom preview.\nINTENTOS_GATE:PASS";
        // Must not be misread as a deferred-work promise.
        assert!(!contains_deferred_continuation_claim(output));
        let (passed, diagnosis, _) =
            evaluate_external_stage_completion("development", true, output, true, false, false, 0);
        assert!(passed);
        assert_eq!(diagnosis, StageCompletionDiagnosis::Completed);
    }

    #[test]
    fn process_exit_failure_overrides_even_a_claimed_pass() {
        let (passed, diagnosis, _) = evaluate_external_stage_completion(
            "architecture",
            false, // non-zero exit
            "INTENTOS_GATE:PASS",
            true,
            false,
            false,
            0,
        );
        assert!(!passed);
        assert!(matches!(diagnosis, StageCompletionDiagnosis::Failed(_)));
    }

    #[test]
    fn qa_and_reality_kinds_are_exempt_from_the_workspace_evidence_requirement() {
        // QA/reality legitimately only read and verify; requiring a
        // workspace mutation from them would produce a false FAIL on a
        // genuinely correct verification-only pass.
        let (passed, diagnosis, _) = evaluate_external_stage_completion(
            "qa",
            true,
            "Inspected the workspace, everything required is present.\nINTENTOS_GATE:PASS",
            false, // no file changed — correct for a pure verification stage
            false,
            false,
            0,
        );
        assert!(passed);
        assert_eq!(diagnosis, StageCompletionDiagnosis::Completed);
    }

    #[test]
    fn explicit_fail_is_reported_as_failed_not_incomplete() {
        let (passed, diagnosis, _) = evaluate_external_stage_completion(
            "development",
            true,
            "Could not finish: missing dependency.\nINTENTOS_GATE:FAIL",
            false,
            false,
            false,
            0,
        );
        assert!(!passed);
        assert!(matches!(diagnosis, StageCompletionDiagnosis::Failed(_)));
    }

    // ---- Executor↔runtime contract (P0) — real E2E against the real
    // Claude Code CLI, not a mock. Ignored like the other real-dependency
    // tests in this suite (Temporal, soup): requires `claude` installed and
    // authenticated on the machine running the test. This is the one
    // required by the mandate's own success criteria — a demonstrable real
    // run, not just green unit tests — proving the whole path: real
    // process spawn, real workspace mutation, real gate parsing, real
    // before/after manifest diff, real completion.json evidence written.
    #[tokio::test]
    #[ignore = "spawns the real Claude Code CLI; requires `claude` installed and authenticated"]
    async fn e2e_real_claude_code_stage_writes_evidence_and_satisfies_the_contract() {
        let workspace = tempfile::tempdir().unwrap();
        let evidence_dir = tempfile::tempdir().unwrap();
        let channel = Channel::new(|_| Ok(()));
        let prompt = "You are a test stage in an automated contract-verification suite for IntentOS. Create a file named INTENTOS_P0_PROOF.txt in the current working directory containing exactly the text: contract-check-ok\nThen end your final response with exactly INTENTOS_GATE:PASS";
        let result = run_claude_stage(
            workspace.path(),
            prompt,
            "e2e-run",
            "development",
            "development",
            1,
            evidence_dir.path(),
            &channel,
            false,
            0,
        )
        .await
        .expect("run_claude_stage must not error for a real, reachable Claude Code CLI");

        assert!(
            result,
            "a real Claude Code stage that actually wrote the required file and said PASS must satisfy the contract"
        );
        assert!(
            workspace.path().join("INTENTOS_P0_PROOF.txt").is_file(),
            "the real workspace must contain the file Claude Code was asked to create — not just a claimed PASS"
        );
        let completion_raw =
            std::fs::read_to_string(evidence_dir.path().join("development-1.completion.json"))
                .expect("completion.json evidence must be written for a real external stage");
        assert!(
            completion_raw.contains("\"completed\""),
            "completion evidence must record Completed for a genuinely satisfied contract: {completion_raw}"
        );
    }

    // ---- Reality gate — structured criteria (shadow mode) ----

    fn criteria_block(entries_json: &str) -> String {
        format!(
            "Narrative report...\n\nINTENTOS_CRITERIA:\n```json\n{entries_json}\n```\n\nINTENTOS_GATE:PASS"
        )
    }

    #[test]
    fn parse_criteria_result_extracts_pass_and_fail_by_index() {
        let output = criteria_block(
            r#"[
                {"criterionIndex": 0, "criterion": "El login funciona", "status": "pass", "evidence": "login.spec.ts:12 verde", "explanation": "Cubierto por test automatizado."},
                {"criterionIndex": 1, "criterion": "Envia email de bienvenida", "status": "fail", "evidence": "No se encontro integracion SMTP", "explanation": "No implementado."}
            ]"#,
        );
        let result = parse_criteria_result(&output, 2).expect("well-formed block must parse");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].criterion_index, 0);
        assert_eq!(result[0].status, CriterionStatus::Pass);
        assert_eq!(result[0].criterion, "El login funciona");
        assert_eq!(result[1].criterion_index, 1);
        assert_eq!(result[1].status, CriterionStatus::Fail);
        assert!(result[1].explanation.contains("No implementado"));
    }

    #[test]
    fn parse_criteria_result_defaults_missing_or_unrecognized_status_to_unverified() {
        let output = criteria_block(
            r#"[
                {"criterionIndex": 0, "criterion": "Criterio A", "status": "who-knows", "evidence": "", "explanation": ""},
                {"criterionIndex": 1, "criterion": "Criterio B", "evidence": "", "explanation": ""}
            ]"#,
        );
        let result = parse_criteria_result(&output, 2).expect("must still parse");
        assert_eq!(result[0].status, CriterionStatus::Unverified);
        assert_eq!(result[1].status, CriterionStatus::Unverified);
    }

    #[test]
    fn parse_criteria_result_returns_none_without_the_anchor() {
        let output = "Narrative report...\n\nINTENTOS_GATE:PASS";
        assert!(parse_criteria_result(output, 3).is_none());
    }

    #[test]
    fn parse_criteria_result_returns_none_for_malformed_json() {
        let output = "INTENTOS_CRITERIA:\n```json\n[ this is not valid json\n```\nINTENTOS_GATE:FAIL";
        assert!(parse_criteria_result(output, 3).is_none());
    }

    #[test]
    fn parse_criteria_result_returns_none_for_an_empty_array() {
        let output = criteria_block("[]");
        assert!(parse_criteria_result(&output, 3).is_none());
    }

    #[test]
    fn parse_criteria_result_drops_out_of_range_entries_but_keeps_valid_ones() {
        // Mission has exactly 1 acceptance criterion (criteria_len = 1). The
        // model hallucinated a second entry at index 5 — it must be dropped
        // silently, never trusted, and never take down the valid entry
        // alongside it.
        let output = criteria_block(
            r#"[
                {"criterionIndex": 0, "criterion": "Unico criterio real", "status": "pass", "evidence": "ok", "explanation": "ok"},
                {"criterionIndex": 5, "criterion": "Criterio inventado", "status": "fail", "evidence": "x", "explanation": "x"}
            ]"#,
        );
        let result = parse_criteria_result(&output, 1).expect("the valid entry must still parse");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].criterion_index, 0);
    }

    #[test]
    fn parse_criteria_result_returns_none_when_every_index_is_out_of_range() {
        // No Mission attached to the run (criteria_len == 0) — every index is
        // out of range by construction, so there is nothing to correlate
        // against and the whole block must be discarded, not half-trusted.
        let output = criteria_block(
            r#"[{"criterionIndex": 0, "criterion": "x", "status": "pass", "evidence": "", "explanation": ""}]"#,
        );
        assert!(parse_criteria_result(&output, 0).is_none());
    }

    #[test]
    fn parse_criteria_result_ignores_trailing_content_after_the_json_array() {
        // The closing ``` fence and the INTENTOS_GATE line both follow the
        // array on the same combined stdout — the parser must stop at the
        // end of the JSON value, not choke on what comes after it.
        let output = criteria_block(
            r#"[{"criterionIndex": 0, "criterion": "x", "status": "pass", "evidence": "e", "explanation": "e"}]"#,
        );
        assert!(output.contains("```\n\nINTENTOS_GATE:PASS"));
        assert!(parse_criteria_result(&output, 1).is_some());
    }

    #[test]
    fn shadow_mode_never_touches_the_actual_gate() {
        // A structured block that says everything failed, sitting alongside
        // a sentinel that says PASS — output_gate_passed (the real
        // authority) must still see only the sentinel. Parsing/using the
        // structured result is an entirely separate, additive step.
        let output = criteria_block(
            r#"[{"criterionIndex": 0, "criterion": "x", "status": "fail", "evidence": "", "explanation": ""}]"#,
        );
        assert!(output_gate_passed(&output));
        let criteria = parse_criteria_result(&output, 1).unwrap();
        assert_eq!(criteria[0].status, CriterionStatus::Fail);
        // Both facts are true at once — the mismatch is exactly what
        // record_reality_shadow_evidence logs, without changing either.
    }

    #[tokio::test]
    async fn record_reality_shadow_evidence_persists_the_structured_result_as_json() {
        let dir = tempfile::tempdir().unwrap();
        let output = criteria_block(
            r#"[{"criterionIndex": 0, "criterion": "x", "status": "pass", "evidence": "e", "explanation": "e"}]"#,
        );
        record_reality_shadow_evidence(
            dir.path(),
            "reality-check",
            1,
            "run-1",
            "reality-check",
            &output,
            1,
            true,
        )
        .await;
        let written = fs::read_to_string(dir.path().join("reality-check-1.criteria.json"))
            .expect("shadow evidence file must be written");
        let parsed: Vec<CriterionResult> = serde_json::from_str(&written).unwrap();
        assert_eq!(parsed[0].status, CriterionStatus::Pass);
    }

    #[tokio::test]
    async fn record_reality_shadow_evidence_writes_nothing_when_the_block_is_absent() {
        let dir = tempfile::tempdir().unwrap();
        record_reality_shadow_evidence(
            dir.path(),
            "reality-check",
            1,
            "run-1",
            "reality-check",
            "Narrative report...\nINTENTOS_GATE:PASS",
            2,
            true,
        )
        .await;
        assert!(!dir.path().join("reality-check-1.criteria.json").exists());
    }

    // ---- Reality gate — strict mode (opt-in authority) ----

    fn cr(index: usize, status: CriterionStatus, explanation: &str) -> CriterionResult {
        CriterionResult {
            criterion_index: index,
            criterion: format!("criterion {index}"),
            status,
            evidence: String::new(),
            explanation: explanation.into(),
        }
    }

    #[test]
    fn reality_verdict_passes_with_full_coverage_and_all_pass() {
        let criteria = vec![
            cr(0, CriterionStatus::Pass, ""),
            cr(1, CriterionStatus::Pass, ""),
            cr(2, CriterionStatus::Pass, ""),
        ];
        assert!(reality_verdict(Some(&criteria), 3, false).passed());
    }

    #[test]
    fn reality_verdict_fails_on_any_fail_status() {
        let criteria = vec![
            cr(0, CriterionStatus::Pass, ""),
            cr(1, CriterionStatus::Fail, "roto"),
        ];
        match reality_verdict(Some(&criteria), 2, true) {
            RealityVerdict::Fail(reason) => {
                assert!(reason.contains("criterion 1"));
                assert!(reason.contains("roto"));
            }
            RealityVerdict::Pass => panic!("expected Fail"),
        }
    }

    #[test]
    fn reality_verdict_fails_on_any_unverified_status() {
        let criteria = vec![cr(0, CriterionStatus::Unverified, "no se pudo probar")];
        match reality_verdict(Some(&criteria), 1, true) {
            RealityVerdict::Fail(reason) => {
                assert!(reason.contains("criterion 0"));
                assert!(reason.contains("could not be verified"));
            }
            RealityVerdict::Pass => panic!("expected Fail"),
        }
    }

    #[test]
    fn reality_verdict_fails_on_missing_index() {
        let criteria = vec![cr(0, CriterionStatus::Pass, "")];
        match reality_verdict(Some(&criteria), 2, true) {
            RealityVerdict::Fail(reason) => {
                assert!(reason.contains("criterion 1"));
                assert!(reason.contains("never addressed"));
            }
            RealityVerdict::Pass => panic!("expected Fail for missing coverage"),
        }
    }

    #[test]
    fn reality_verdict_fails_on_duplicate_index() {
        let criteria = vec![
            cr(0, CriterionStatus::Pass, ""),
            cr(0, CriterionStatus::Pass, ""),
        ];
        match reality_verdict(Some(&criteria), 1, true) {
            RealityVerdict::Fail(reason) => {
                assert!(reason.contains("criterion 0"));
                assert!(reason.contains("more than once"));
            }
            RealityVerdict::Pass => panic!("expected Fail for duplicate index"),
        }
    }

    #[test]
    fn reality_verdict_fails_on_out_of_range_index() {
        // parse_criteria_result would already exclude this — defense in
        // depth against ever trusting an out-of-range index.
        let criteria = vec![cr(5, CriterionStatus::Pass, "")];
        match reality_verdict(Some(&criteria), 2, true) {
            RealityVerdict::Fail(reason) => assert!(reason.contains("out of range")),
            RealityVerdict::Pass => panic!("expected Fail for out-of-range index"),
        }
    }

    #[test]
    fn reality_verdict_fails_when_block_is_absent_but_criteria_were_approved() {
        match reality_verdict(None, 3, true) {
            RealityVerdict::Fail(reason) => assert!(reason.contains('3')),
            RealityVerdict::Pass => panic!("expected Fail when no structured block exists"),
        }
    }

    #[test]
    fn reality_verdict_falls_back_to_sentinel_when_mission_has_no_criteria() {
        assert!(reality_verdict(None, 0, true).passed());
        assert!(!reality_verdict(None, 0, false).passed());
    }

    #[test]
    fn reality_verdict_structured_fail_overrides_a_passing_sentinel() {
        let criteria = vec![cr(0, CriterionStatus::Fail, "no cumple")];
        // sentinel_passed = true, but a real criterion failed — the
        // structured result must win.
        assert!(!reality_verdict(Some(&criteria), 1, true).passed());
    }

    #[test]
    fn reality_verdict_structured_pass_overrides_a_failing_sentinel() {
        let criteria = vec![cr(0, CriterionStatus::Pass, "")];
        // sentinel_passed = false, but every approved criterion genuinely
        // checks out — the structured result is authoritative in both
        // directions, not just when it agrees with a FAIL.
        assert!(reality_verdict(Some(&criteria), 1, false).passed());
    }

    #[test]
    fn resolve_stage_outcome_ignores_structured_block_when_strict_is_off() {
        // criteria_block() ends with INTENTOS_GATE:PASS; the structured
        // block says the criterion failed. Strict is off (the default) —
        // the sentinel must still win, and no reason is computed at all.
        let output = criteria_block(
            r#"[{"criterionIndex": 0, "criterion": "x", "status": "fail", "evidence": "", "explanation": "no importa"}]"#,
        );
        let (passed, reason) = resolve_stage_outcome(true, false, &output, 1);
        assert!(passed);
        assert!(reason.is_none());
    }

    #[test]
    fn resolve_stage_outcome_lets_structured_fail_override_sentinel_pass_when_strict_is_on() {
        let output = criteria_block(
            r#"[{"criterionIndex": 0, "criterion": "x", "status": "fail", "evidence": "", "explanation": "motivo real"}]"#,
        );
        let (passed, reason) = resolve_stage_outcome(true, true, &output, 1);
        assert!(!passed);
        assert!(reason.expect("a reason must be persisted").contains("motivo real"));
    }

    #[test]
    fn resolve_stage_outcome_only_applies_strict_mode_to_the_reality_stage() {
        // is_reality = false: strict mode must never engage, even with the
        // flag on and even with zero approved criteria — every other stage
        // keeps exactly today's sentinel-only behavior.
        let output = "Narrative report...\nINTENTOS_GATE:PASS";
        let (passed, reason) = resolve_stage_outcome(false, true, output, 3);
        assert!(passed);
        assert!(reason.is_none());
    }

    #[test]
    fn reality_gate_strict_flag_reads_the_environment_variable() {
        static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var(REALITY_GATE_STRICT_ENV);
        }
        assert!(!reality_gate_strict_enabled(), "unset must be disabled");
        unsafe {
            std::env::set_var(REALITY_GATE_STRICT_ENV, "1");
        }
        assert!(reality_gate_strict_enabled(), "any non-zero value enables it");
        unsafe {
            std::env::set_var(REALITY_GATE_STRICT_ENV, "0");
        }
        assert!(!reality_gate_strict_enabled(), "\"0\" must stay disabled");
        unsafe {
            std::env::remove_var(REALITY_GATE_STRICT_ENV);
        }
    }

    #[test]
    fn stage_prompt_adds_the_criteria_instruction_only_for_the_reality_stage() {
        let now = Utc::now();
        let run = RunSummary {
            id: "run".into(),
            intent: "Build a verified product".into(),
            project_path: "/tmp/project".into(),
            workspace_path: None,
            runbook_id: "startup-mvp".into(),
            capability_id: "digital-experience".into(),
            capability_ids: vec!["digital-experience".into()],
            mission_id: None,
            session_id: None,
            provider_id: Some(PROVIDER_ID.into()),
            status: RunStatus::Running,
            current_stage: None,
            stages: initial_stages(&[], &[], &[], &[]),
            created_at: now,
            updated_at: now,
            completed_at: None,
            error: None,
        };
        // index 2 = "development" in the fallback roster — must be
        // byte-unaffected, same guarantee the existing frozen-contract
        // tests already cover for the mission-brief section.
        let development_prompt = stage_prompt(&run, 2, None, None);
        assert!(!development_prompt.contains("INTENTOS_CRITERIA"));
        // index 4 = "reality-check" in the fallback roster.
        let reality_prompt = stage_prompt(&run, 4, None, None);
        assert!(reality_prompt.contains("INTENTOS_CRITERIA:"));
        assert!(reality_prompt.contains("criterionIndex"));
        // The gate sentinel instruction must still be present and unchanged
        // — the structured report is additive, not a replacement.
        assert!(reality_prompt.contains("End your final response with exactly INTENTOS_GATE:PASS"));
    }

    fn run_with_capabilities(capability_id: &str, capability_ids: Vec<String>) -> RunSummary {
        let now = Utc::now();
        RunSummary {
            id: "run".into(),
            intent: "Build a verified product".into(),
            project_path: "/tmp/project".into(),
            workspace_path: None,
            runbook_id: "startup-mvp".into(),
            capability_id: capability_id.into(),
            capability_ids,
            mission_id: None,
            session_id: None,
            provider_id: Some(PROVIDER_ID.into()),
            status: RunStatus::Running,
            current_stage: None,
            stages: initial_stages(&[], &[], &[], &[]),
            created_at: now,
            updated_at: now,
            completed_at: None,
            error: None,
        }
    }

    #[test]
    fn stage_prompt_reality_gives_domain_guidance_for_digital_experience() {
        // Web case: DOM/browser/accessibility guidance, no Laravel assumed,
        // no IoT-specific telemetry language leaking in.
        let run = run_with_capabilities("digital-experience", vec!["digital-experience".into()]);
        let reality_prompt = stage_prompt(&run, 4, None, None);
        assert!(reality_prompt.contains("DOMAIN VERIFICATION GUIDANCE"));
        assert!(reality_prompt.contains("[digital-experience]"));
        assert!(reality_prompt.contains("DOM structure"));
        // The guidance explicitly tells the model not to assume Laravel —
        // it names the word only to negate it, never to prescribe it.
        assert!(reality_prompt.contains("Do not assume Laravel"));
        assert!(!reality_prompt.contains(".blade.php"));
        assert!(!reality_prompt.contains("MQTT"));
    }

    #[test]
    fn stage_prompt_reality_gives_domain_guidance_for_iot() {
        // Non-web case: telemetry/protocol guidance, no browser/DOM language.
        let run = run_with_capabilities("iot", vec!["iot".into()]);
        let reality_prompt = stage_prompt(&run, 4, None, None);
        assert!(reality_prompt.contains("[iot]"));
        assert!(reality_prompt.contains("MQTT"));
        assert!(!reality_prompt.contains("DOM structure"));
        assert!(!reality_prompt.contains("resources/views"));
    }

    #[test]
    fn stage_prompt_reality_falls_back_to_generic_guidance_for_unknown_capability() {
        // Historical runs (no capability_id recorded) or a future id this
        // list doesn't know yet — must get domain-neutral guidance, never
        // default to assuming a web/Laravel stack.
        let run = run_with_capabilities("", vec![]);
        let reality_prompt = stage_prompt(&run, 4, None, None);
        assert!(reality_prompt.contains("[unspecified]"));
        assert!(reality_prompt.contains("Do not assume a web/Laravel stack by default"));
    }

    #[test]
    fn stage_prompt_reality_covers_every_selected_capability_in_multi_domain_runs() {
        // composePipeline() only tags the shared reality-check stage with
        // the primary capability — capability_ids (all selected domains)
        // must still each get their own guidance block.
        let run = run_with_capabilities(
            "iot",
            vec!["iot".into(), "systems-data".into()],
        );
        let reality_prompt = stage_prompt(&run, 4, None, None);
        assert!(reality_prompt.contains("[iot]"));
        assert!(reality_prompt.contains("[systems-data]"));
        assert!(reality_prompt.contains("MQTT"));
        assert!(reality_prompt.contains("persistence round-trips"));
    }

    #[test]
    fn domain_guidance_is_absent_outside_the_reality_stage() {
        let run = run_with_capabilities("iot", vec!["iot".into()]);
        // index 2 = "development" in the fallback roster.
        let development_prompt = stage_prompt(&run, 2, None, None);
        assert!(!development_prompt.contains("DOMAIN VERIFICATION GUIDANCE"));
    }

    #[test]
    fn isolated_workspace_preserves_source_and_excludes_git_metadata() {
        let source = tempfile::tempdir().unwrap();
        let destination_parent = tempfile::tempdir().unwrap();
        let destination = destination_parent.path().join("workspace");
        fs::create_dir_all(source.path().join("nested")).unwrap();
        fs::create_dir_all(source.path().join(".git")).unwrap();
        fs::write(source.path().join("nested/app.txt"), b"original").unwrap();
        fs::write(source.path().join(".git/config"), b"secret metadata").unwrap();

        copy_workspace(source.path(), &destination).unwrap();
        fs::write(destination.join("nested/app.txt"), b"agent edit").unwrap();

        assert_eq!(
            fs::read(source.path().join("nested/app.txt")).unwrap(),
            b"original"
        );
        assert_eq!(
            fs::read(destination.join("nested/app.txt")).unwrap(),
            b"agent edit"
        );
        assert!(!destination.join(".git").exists());
    }

    fn test_state(app_data: &Path) -> AppState {
        AppState {
            app_data_dir: app_data.to_path_buf(),
            corpus_cache: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
            corpus_refresh_in_flight: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            settings: std::sync::Arc::new(tokio::sync::RwLock::new(
                crate::commands::settings::SettingsLoadState::FirstLaunch,
            )),
            updater_state: crate::commands::updater::empty_state(),
            runtime_jobs: std::sync::Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            local_model_process: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
            preview_process: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
            public_preview_process: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    /// The exact bug the conversational rewrite fixes: before it, every
    /// follow-up turn called `copy_workspace(original_project, ...)` again
    /// (see the pre-fix git history), so turn 2 silently lost whatever
    /// turn 1 had just built. `resolve_session_workspace` must instead
    /// hand back the *same* directory turn 2, 3, ... — proven here by
    /// writing a file as "turn 1" and confirming "turn 2" still sees it
    /// rather than getting a fresh copy of the (untouched) source.
    #[tokio::test]
    async fn resolve_session_workspace_reuses_the_same_directory_across_turns() {
        let app_data = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        fs::write(source.path().join("index.html"), b"<h1>original</h1>").unwrap();
        let state = test_state(app_data.path());
        let project_path = source.path().to_string_lossy().into_owned();

        let (session_id_1, turn1) = resolve_session_workspace(&state, source.path(), &project_path)
            .await
            .unwrap();
        fs::write(turn1.join("index.html"), b"<h1>agrega el visor 3D</h1>").unwrap();
        fs::write(turn1.join("viewer3d.js"), b"// turn 1 output").unwrap();

        let (session_id_2, turn2) = resolve_session_workspace(&state, source.path(), &project_path)
            .await
            .unwrap();

        assert_eq!(session_id_1, session_id_2, "same project must resolve to the same session");
        assert_eq!(turn1, turn2, "turn 2 must reuse turn 1's exact workspace");
        assert_eq!(
            fs::read(turn2.join("index.html")).unwrap(),
            b"<h1>agrega el visor 3D</h1>",
            "turn 2 must see turn 1's edit, not a fresh copy of the original"
        );
        assert!(
            turn2.join("viewer3d.js").exists(),
            "turn 2 must see files turn 1 created"
        );
        // The protected original is untouched by either turn.
        assert_eq!(
            fs::read(source.path().join("index.html")).unwrap(),
            b"<h1>original</h1>"
        );

        // A different session for a different project must never share
        // this workspace.
        let other_source = tempfile::tempdir().unwrap();
        fs::write(other_source.path().join("index.html"), b"<h1>other</h1>").unwrap();
        let other_project_path = other_source.path().to_string_lossy().into_owned();
        let (other_session_id, other) =
            resolve_session_workspace(&state, other_source.path(), &other_project_path)
                .await
                .unwrap();
        assert_ne!(other_session_id, session_id_1);
        assert_ne!(other, turn1);
        assert!(!other.join("viewer3d.js").exists());
    }

    #[test]
    fn manifest_is_sorted_and_hashes_file_contents() {
        let workspace = tempfile::tempdir().unwrap();
        fs::write(workspace.path().join("b.txt"), b"second").unwrap();
        fs::write(workspace.path().join("a.txt"), b"first").unwrap();

        let manifest = workspace_manifest(workspace.path()).unwrap();
        assert_eq!(
            manifest
                .iter()
                .map(|entry| entry.path.as_str())
                .collect::<Vec<_>>(),
            vec!["a.txt", "b.txt"]
        );
        assert_eq!(manifest[0].bytes, 5);
        assert_eq!(manifest[0].sha256, hex::encode(Sha256::digest(b"first")));
    }
    #[test]
    fn manifest_diff_classifies_added_modified_and_removed() {
        let entry = |path: &str, hash: &str| ManifestEntry {
            path: path.into(),
            bytes: 1,
            sha256: hash.into(),
        };
        let before = vec![
            entry("modified.txt", "old"),
            entry("removed.txt", "gone"),
            entry("same.txt", "same"),
        ];
        let after = vec![
            entry("added.txt", "new"),
            entry("modified.txt", "newer"),
            entry("same.txt", "same"),
        ];
        let changes = manifest_changes(&before, &after);
        assert_eq!(
            changes
                .iter()
                .map(|change| (change.path.as_str(), change.kind.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("added.txt", "added"),
                ("modified.txt", "modified"),
                ("removed.txt", "removed")
            ]
        );
    }
    #[test]
    fn apply_changes_rejects_parent_traversal() {
        let source = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let changes = vec![WorkspaceChange {
            path: "../escape.txt".into(),
            kind: "added".into(),
            before_sha256: None,
            after_sha256: Some("hash".into()),
        }];
        assert!(apply_workspace_changes(source.path(), workspace.path(), &changes).is_err());
    }
    #[test]
    fn start_contract_requires_a_complete_five_stage_team() {
        assert!(validate_start_request(&valid_request()).is_ok());

        let mut missing_agent = valid_request();
        missing_agent.agent_slugs.pop();
        assert!(validate_start_request(&missing_agent).is_err());

        let mut blank_label = valid_request();
        blank_label.stage_labels[2] = "  ".into();
        assert!(validate_start_request(&blank_label).is_err());

        let mut missing_capability = valid_request();
        missing_capability.capability_id.clear();
        assert!(validate_start_request(&missing_capability).is_err());
    }
    #[test]
    fn terminal_failure_clears_every_running_stage() {
        let now = Utc::now();
        let mut run = RunSummary {
            id: "run".into(),
            intent: "intent".into(),
            project_path: "/tmp/project".into(),
            workspace_path: None,
            runbook_id: "startup-mvp".into(),
            capability_id: "iot".into(),
            capability_ids: vec!["iot".into()],
            mission_id: None,
            session_id: None,
            provider_id: Some(PROVIDER_ID.into()),
            status: RunStatus::Running,
            current_stage: Some("development".into()),
            stages: initial_stages(&[], &[], &[], &[]),
            created_at: now,
            updated_at: now,
            completed_at: None,
            error: None,
        };
        run.stages[2].status = "running".into();
        run.stages[3].status = "running".into();
        fail_run(&mut run, 3, "qa failed");
        assert_eq!(run.status, RunStatus::Failed);
        assert!(run.stages.iter().all(|stage| stage.status != "running"));
        assert!(run.current_stage.is_none());
        assert!(run.completed_at.is_some());
    }
    #[test]
    fn historical_terminal_run_is_sanitized_on_read() {
        let now = Utc::now();
        let mut run = RunSummary {
            id: "old-run".into(),
            intent: "intent".into(),
            project_path: "/tmp/project".into(),
            workspace_path: None,
            runbook_id: "startup-mvp".into(),
            capability_id: "iot".into(),
            capability_ids: vec!["iot".into()],
            mission_id: None,
            session_id: None,
            provider_id: Some(PROVIDER_ID.into()),
            status: RunStatus::Failed,
            current_stage: Some("development".into()),
            stages: initial_stages(&[], &[], &[], &[]),
            created_at: now,
            updated_at: now,
            completed_at: Some(now),
            error: Some("failed".into()),
        };
        run.stages[2].status = "running".into();
        normalize_terminal_state(&mut run);
        assert_eq!(run.stages[2].status, "failed");
        assert!(run.current_stage.is_none());
    }

    #[test]
    fn delivery_evidence_requires_an_after_manifest_for_every_gate() {
        let dir = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let mut stages = initial_stages(&[], &[], &[], &[]);
        for stage in &mut stages {
            stage.status = "passed".into();
            stage.attempt = 1;
            let component = evidence_file_component(&stage.id);
            fs::write(
                dir.path()
                    .join(format!("{component}-1-after.manifest.json")),
                b"[]",
            )
            .unwrap();
        }
        let run = RunSummary {
            id: "delivery-run".into(),
            intent: "build and verify".into(),
            project_path: "/tmp/project".into(),
            workspace_path: Some("/tmp/workspace".into()),
            runbook_id: "startup-mvp".into(),
            capability_id: "digital-experience".into(),
            capability_ids: vec!["digital-experience".into()],
            provider_id: Some(PROVIDER_ID.into()),
            mission_id: None,
            session_id: None,
            status: RunStatus::Succeeded,
            current_stage: None,
            stages,
            created_at: now,
            updated_at: now,
            completed_at: Some(now),
            error: None,
        };
        assert!(delivery_evidence_complete(&run, dir.path()));
        let missing = evidence_file_component(&run.stages[0].id);
        fs::remove_file(dir.path().join(format!("{missing}-1-after.manifest.json"))).unwrap();
        assert!(!delivery_evidence_complete(&run, dir.path()));
    }
    #[test]
    fn iot_development_prompt_owns_the_full_vertical() {
        let now = Utc::now();
        let mut run = RunSummary {
            id: "run".into(),
            intent: "ESP32, MQTT, API and dashboard".into(),
            project_path: "/tmp/project".into(),
            workspace_path: None,
            runbook_id: "startup-mvp".into(),
            capability_id: "iot".into(),
            capability_ids: vec!["iot".into()],
            mission_id: None,
            session_id: None,
            provider_id: Some(PROVIDER_ID.into()),
            status: RunStatus::Running,
            current_stage: Some("development".into()),
            stages: initial_stages(&[], &[], &[], &[]),
            created_at: now,
            updated_at: now,
            completed_at: None,
            error: None,
        };
        run.stages[2].id = "development".into();
        let prompt = stage_prompt(&run, 2, None, None);
        assert!(prompt.contains("IOT FULL-VERTICAL IMPLEMENTATION MANDATE"));
        assert!(prompt.contains("consumer/backend, persistence, alert rules, API"));
    }

    #[test]
    fn stage_prompt_without_mission_omits_the_brief_section() {
        // Backward-compatibility guarantee: a run with no Mission attached
        // must not gain a MISSION BRIEF section — this is what keeps
        // Runtime v0.1's frozen contract intact for every existing caller.
        let now = Utc::now();
        let run = RunSummary {
            id: "run".into(),
            intent: "Build a verified product".into(),
            project_path: "/tmp/project".into(),
            workspace_path: None,
            runbook_id: "startup-mvp".into(),
            capability_id: "digital-experience".into(),
            capability_ids: vec!["digital-experience".into()],
            mission_id: None,
            session_id: None,
            provider_id: Some(PROVIDER_ID.into()),
            status: RunStatus::Running,
            current_stage: Some("development".into()),
            stages: initial_stages(&[], &[], &[], &[]),
            created_at: now,
            updated_at: now,
            completed_at: None,
            error: None,
        };
        let prompt = stage_prompt(&run, 2, None, None);
        assert!(!prompt.contains("MISSION BRIEF"));
        assert!(prompt.contains("USER INTENT:\nBuild a verified product"));
    }

    #[test]
    fn stage_prompt_with_mission_prepends_the_brief_before_user_intent() {
        let now = Utc::now();
        let mission = Mission {
            id: "11111111-1111-1111-1111-111111111111".into(),
            project_path: "/tmp/project".into(),
            objective: "Build a booking website".into(),
            scope_statement: "Marketing site with a booking form".into(),
            exclusions: vec!["Payment processing".into()],
            acceptance_criteria: vec![],
            client_locale: Some("es-CL".into()),
            target_markets: vec![],
            delivery_locales: vec![],
            agency_jurisdiction: None,
            engagement_regime: crate::mission::EngagementRegime::Fixed,
            adjustment_budget: None,
            change_policy_note: None,
            status: crate::mission::MissionStatus::Approved,
            approved_by_ncto: true,
            approval_channel: Some(crate::mission::ApprovalChannel::Temporal),
            created_at: now,
            updated_at: now,
        };
        let run = RunSummary {
            id: "run".into(),
            intent: "Build a verified product".into(),
            project_path: "/tmp/project".into(),
            workspace_path: None,
            runbook_id: "startup-mvp".into(),
            capability_id: "digital-experience".into(),
            capability_ids: vec!["digital-experience".into()],
            mission_id: Some(mission.id.clone()),
            session_id: None,
            provider_id: Some(PROVIDER_ID.into()),
            status: RunStatus::Running,
            current_stage: Some("development".into()),
            stages: initial_stages(&[], &[], &[], &[]),
            created_at: now,
            updated_at: now,
            completed_at: None,
            error: None,
        };
        let prompt = stage_prompt(&run, 2, None, Some(&mission));
        assert!(prompt.contains("MISSION BRIEF"));
        assert!(prompt.contains("EXPLICITLY OUT OF SCOPE"));
        let brief_pos = prompt.find("MISSION BRIEF").unwrap();
        let intent_pos = prompt.find("USER INTENT:").unwrap();
        assert!(brief_pos < intent_pos);
    }

    // ---------- stage_uses_local_executor ----------

    #[test]
    fn stage_uses_local_executor_is_true_for_direction_kind() {
        let s = stage("direction", "Dirección de proyecto", "project-manager-senior", "direction");
        assert!(stage_uses_local_executor(&s));
    }

    #[test]
    fn stage_uses_local_executor_is_true_for_the_rust_fallback_direction_stage() {
        // initial_stages()'s fallback path (used when the frontend doesn't
        // supply matching-length arrays) ids this stage "project-management"
        // with kind "direction" — the dual kind-or-id check must still
        // catch it, same convention as is_qa/is_reality.
        let s = stage("project-management", "Project Manager", "project-manager-senior", "direction");
        assert!(stage_uses_local_executor(&s));
    }

    #[test]
    fn stage_uses_local_executor_is_true_for_architecture_kind() {
        let s = stage(
            "systems-data:architecture",
            "Arquitectura de sistema",
            "engineering-software-architect",
            "architecture",
        );
        assert!(stage_uses_local_executor(&s));
    }

    #[test]
    fn stage_uses_local_executor_is_true_for_development_kind() {
        let s = stage(
            "systems-data:development",
            "Backend y datos",
            "engineering-backend-architect",
            "development",
        );
        assert!(stage_uses_local_executor(&s));
    }

    #[test]
    fn stage_uses_local_executor_is_false_for_every_other_kind() {
        for kind in ["qa", "reality"] {
            let s = stage(kind, kind, "some-agent", kind);
            assert!(
                !stage_uses_local_executor(&s),
                "kind {kind} must not route to the local executor"
            );
        }
    }

    // ---------- provider_id: String -> Option<String> serde compatibility ----------
    //
    // Explicit regression coverage per the approved delta: do not assume
    // Option<String> is a drop-in backward-compatible replacement for
    // String without a test. Covers every shape a persisted run-state JSON
    // file could have: the historical plain string, an explicit null (the
    // new "no external provider" case going forward), and the key missing
    // entirely (defensive — #[serde(default)] covers it even though no
    // known historical run predates this field).

    fn minimal_run_json(provider_id_field: &str) -> String {
        format!(
            r#"{{
                "id": "run-1",
                "intent": "test intent",
                "projectPath": "/tmp/project",
                "runbookId": "intent-build",
                "capabilityId": "digital-experience",
                {provider_id_field}
                "status": "succeeded",
                "currentStage": null,
                "stages": [],
                "createdAt": "2026-01-01T00:00:00Z",
                "updatedAt": "2026-01-01T00:00:00Z",
                "completedAt": null,
                "error": null
            }}"#
        )
    }

    #[test]
    fn provider_id_deserializes_from_a_historical_plain_string_run() {
        let json = minimal_run_json(r#""providerId": "codexCli","#);
        let run: RunSummary = serde_json::from_str(&json).expect("historical run JSON must still parse");
        assert_eq!(run.provider_id, Some("codexCli".to_string()));
    }

    #[test]
    fn provider_id_deserializes_from_explicit_null() {
        let json = minimal_run_json(r#""providerId": null,"#);
        let run: RunSummary = serde_json::from_str(&json).expect("null providerId must parse");
        assert_eq!(run.provider_id, None);
    }

    #[test]
    fn provider_id_deserializes_when_the_field_is_entirely_absent() {
        let json = minimal_run_json("");
        let run: RunSummary = serde_json::from_str(&json).expect("missing providerId must default, not error");
        assert_eq!(run.provider_id, None);
    }

    #[test]
    fn provider_id_round_trips_through_serialize_then_deserialize() {
        let now = Utc::now();
        let run = RunSummary {
            id: "run-2".into(),
            intent: "test".into(),
            project_path: "/tmp/project".into(),
            workspace_path: None,
            runbook_id: "intent-build".into(),
            capability_id: "digital-experience".into(),
            capability_ids: vec![],
            provider_id: None,
            mission_id: None,
            session_id: None,
            status: RunStatus::Running,
            current_stage: Some("direction".into()),
            stages: vec![],
            created_at: now,
            updated_at: now,
            completed_at: None,
            error: None,
        };
        let json = serde_json::to_string(&run).unwrap();
        assert!(json.contains("\"providerId\":null"));
        let round_tripped: RunSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(round_tripped.provider_id, None);
    }
}
