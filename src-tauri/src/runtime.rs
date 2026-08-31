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
use crate::mission::{self, Mission};
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
    provider_id: String,
    /// Optional Mission to attach to this run. Purely additive — omitting
    /// it (or passing None) preserves Runtime v0.1's frozen behavior
    /// byte-for-byte: the raw `intent` field is used exactly as before.
    #[serde(default)]
    mission_id: Option<String>,
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
    provider_id: String,
    /// Mirrors StartRunRequest.mission_id — absent on every run persisted
    /// before this field existed; `#[serde(default)]` deserializes those
    /// old files exactly like the existing `capability_id` precedent.
    #[serde(default)]
    mission_id: Option<String>,
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

fn run_evidence_dir(app_data: &Path, id: &str) -> PathBuf {
    app_data.join("state").join("run-evidence").join(id)
}

/// Convert internal stage identifiers into portable filename components.
fn evidence_file_component(value: &str) -> String {
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

async fn persist_manifest(
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

fn output_gate_passed(output: &str) -> bool {
    let passed = output.rfind("INTENTOS_GATE:PASS");
    let failed = output.rfind("INTENTOS_GATE:FAIL");
    match (passed, failed) {
        (Some(_), None) => true,
        (Some(pass), Some(fail)) => pass > fail,
        (None, _) => false,
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
    // Codex first preserves today's default (Runbooks.svelte auto-picks the
    // first available provider) for everyone who already has working Codex
    // quota; Claude Code only becomes the pick when Codex is unavailable —
    // or when a user explicitly selects it from the new provider dropdown.
    Ok(vec![probe_codex().await, probe_claude().await])
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
    if request.provider_id != PROVIDER_ID && request.provider_id != CLAUDE_PROVIDER_ID {
        return Err(AppError::InvalidArgument {
            message: "unsupported runtime provider".into(),
        });
    }
    validate_start_request(&request)?;
    // Codex CLI and Claude Code are both outbound providers even though they
    // are launched as local child processes. The same fail-closed network
    // policy that protects GitHub, catalog sync, and updates must therefore
    // gate the runtime before the project is inspected or any provider
    // process is started.
    let is_claude = request.provider_id == CLAUDE_PROVIDER_ID;
    state
        .require_network(if is_claude {
            "runtime_claude"
        } else {
            "runtime_codex"
        })
        .await?;
    let project = validate_project(&request.project_path, &state.app_data_dir)?;
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
    let run_id = Uuid::new_v4().to_string();
    let (stages, workspace) = if let Some((previous_id, stages, workspace)) = resume {
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
    };
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
    let task_run_id = run_id.clone();
    let handle = tokio::spawn(async move {
        let mut current = run.clone();
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
                match run_provider_stage(
                    &current.provider_id,
                    &workspace,
                    &prompt,
                    &current.id,
                    &stage_id,
                    attempt,
                    &run_evidence_dir(&app_data, &current.id),
                    &on_event,
                )
                .await
                {
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
                        if !run_provider_stage(
                            &current.provider_id,
                            &workspace,
                            &fix_prompt,
                            &current.id,
                            "development",
                            current.stages[development_index].attempt,
                            &run_evidence_dir(&app_data, &current.id),
                            &on_event,
                        )
                        .await
                        .unwrap_or(false)
                        {
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
    let workspace = run.workspace_path.as_deref().unwrap_or(&run.project_path);
    format!("You are the {} agent ({}) in the IntentOS '{}' autonomous pipeline.\n\nINTENTOS ORCHESTRATOR OVERRIDES (highest priority for this run):\n- The USER INTENT below is the authoritative product specification.\n- Catalog persona references to missing templates, memory-bank files, frameworks, scripts, or organizational conventions are optional guidance, not prerequisites.\n- If useful project documentation is missing, create the minimal appropriate documentation yourself from the USER INTENT and continue autonomously.\n- Choose reasonable technical defaults when the user explicitly delegates the choice. Do not fail merely because an auxiliary file, preferred framework, or prior setup is absent.\n- Do not ask the user to implement or configure anything unless human authorization is genuinely required.\n- Stay within the requested scope and do not invent product requirements.\n- This is an isolated working copy. Never access or modify the source project outside WORKSPACE.\n{}\nCATALOG PERSONA INSTRUCTIONS:\n{}\n\n{}USER INTENT:\n{}\nSOURCE PROJECT (read-only reference; do not access): {}\nWORKSPACE: {}\n\nWork only inside WORKSPACE. Inspect existing work and perform this stage for real. Run relevant checks. Do not claim success without evidence. End your final response with exactly INTENTOS_GATE:PASS only if this stage genuinely passes; otherwise end with INTENTOS_GATE:FAIL and explain a genuine blocker. Previous stages are present in the workspace.", s.label, s.agent_slug, run.runbook_id, implementation_scope, persona.map(String::as_str).unwrap_or("Catalog persona unavailable; disclose this limitation."), brief_section, run.intent, run.project_path, workspace)
}

async fn run_codex_stage(
    project: &Path,
    prompt: &str,
    run_id: &str,
    stage_id: &str,
    attempt: u8,
    evidence_dir: &Path,
    channel: &Channel<RunEvent>,
) -> Result<bool, AppError> {
    // Complex architecture and implementation stages routinely exceed thirty
    // minutes. The former 30-minute ceiling killed healthy Codex processes at
    // an arbitrary wall-clock boundary and incorrectly marked them as agent
    // failures. Keep a finite safety ceiling, but size it for real production.
    const MAX_STAGE_RUNTIME: Duration = Duration::from_secs(60 * 60 * 2);
    let execution = async {
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
        Ok::<bool, AppError>(status.success() && output_gate_passed(&combined))
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
    attempt: u8,
    evidence_dir: &Path,
    channel: &Channel<RunEvent>,
) -> Result<bool, AppError> {
    const MAX_STAGE_RUNTIME: Duration = Duration::from_secs(60 * 60 * 2);
    let execution = async {
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
        Ok::<bool, AppError>(status.success() && output_gate_passed(&combined))
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
    attempt: u8,
    evidence_dir: &Path,
    channel: &Channel<RunEvent>,
) -> Result<bool, AppError> {
    if provider_id == CLAUDE_PROVIDER_ID {
        run_claude_stage(project, prompt, run_id, stage_id, attempt, evidence_dir, channel).await
    } else {
        run_codex_stage(project, prompt, run_id, stage_id, attempt, evidence_dir, channel).await
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
            provider_id: PROVIDER_ID.into(),
            mission_id: None,
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
            provider_id: PROVIDER_ID.into(),
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
            provider_id: PROVIDER_ID.into(),
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
            provider_id: PROVIDER_ID.into(),
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
            provider_id: PROVIDER_ID.into(),
            mission_id: None,
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
        fs::remove_file(
            dir.path()
                .join(format!("{missing}-1-after.manifest.json")),
        )
        .unwrap();
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
            provider_id: PROVIDER_ID.into(),
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
            provider_id: PROVIDER_ID.into(),
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
            provider_id: PROVIDER_ID.into(),
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
}
