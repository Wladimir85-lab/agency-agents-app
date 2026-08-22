//! IntentOS execution runtime.
//!
//! This is deliberately not a generic shell bridge. The frontend selects a
//! provider id from a backend-owned allowlist; command, arguments, sandbox and
//! working directory policy remain entirely in Rust.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tauri::{ipc::Channel, State};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use uuid::Uuid;

use crate::corpus;
use crate::error::AppError;
use crate::state::AppState;
use crate::util::fs::{atomic_write, read_capped};

const PROVIDER_ID: &str = "codexCli";
const MAX_INTENT_CHARS: usize = 20_000;
const MAX_RUN_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_EVENT_TEXT_CHARS: usize = 8_000;

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
    stage_labels: Vec<String>,
    agent_slugs: Vec<String>,
    provider_id: String,
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
    status: String,
    attempt: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    id: String,
    intent: String,
    project_path: String,
    runbook_id: String,
    #[serde(default)]
    capability_id: String,
    provider_id: String,
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

fn stage(id: &str, label: &str, agent: &str) -> RunStage {
    RunStage {
        id: id.into(),
        label: label.into(),
        agent_slug: agent.into(),
        status: "pending".into(),
        attempt: 0,
    }
}

fn initial_stages(agents: &[String], labels: &[String]) -> Vec<RunStage> {
    if agents.len() >= 5 && labels.len() == 5 {
        return vec![
            stage("project-management", &labels[0], &agents[0]),
            stage("ux-architecture", &labels[1], &agents[1]),
            stage("development", &labels[2], &agents[2]),
            stage("qa", &labels[3], &agents[3]),
            stage("reality-check", &labels[4], &agents[4]),
        ];
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
        ),
        stage(
            "ux-architecture",
            "UX / Architecture",
            &pick("architect", "ux-architect"),
        ),
        stage(
            "development",
            "Development",
            &pick("developer", "senior-developer"),
        ),
        stage(
            "qa",
            "Quality Assurance",
            &pick("evidence", "testing-evidence-collector"),
        ),
        stage(
            "reality-check",
            "Final Reality Check",
            &pick("reality", "testing-reality-checker"),
        ),
    ]
}

fn runs_dir(state: &AppState) -> PathBuf {
    state.app_data_dir.join("state").join("runs")
}
fn run_path(state: &AppState, id: &str) -> PathBuf {
    runs_dir(state).join(format!("{id}.json"))
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
    for marker in ["OPENAI_API_KEY=", "CODEX_API_KEY="] {
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
    if request.runbook_id.trim().is_empty()
        || request.capability_id.trim().is_empty()
        || request.agent_slugs.len() != 5
        || request.stage_labels.len() != 5
        || request
            .agent_slugs
            .iter()
            .any(|value| value.trim().is_empty())
        || request
            .stage_labels
            .iter()
            .any(|value| value.trim().is_empty())
    {
        return Err(AppError::InvalidArgument {
            message: "runbook, capability, five agents and five stage labels are required".into(),
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

#[tauri::command]
pub async fn runtime_providers() -> Result<Vec<RuntimeProvider>, AppError> {
    Ok(vec![probe_codex().await])
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

async fn resumable_run(
    state: &AppState,
    intent: &str,
    project_path: &str,
    runbook_id: &str,
    capability_id: &str,
    expected: &[RunStage],
) -> Option<(String, Vec<RunStage>)> {
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
            && run.status != RunStatus::Succeeded
            && same_workflow
            && run.stages.iter().any(|stage| stage.status == "passed")
        {
            candidates.push(run);
        }
    }
    candidates.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    candidates.into_iter().next().map(|run| {
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
        (run.id, stages)
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
    if request.provider_id != PROVIDER_ID {
        return Err(AppError::InvalidArgument {
            message: "unsupported runtime provider".into(),
        });
    }
    validate_start_request(&request)?;
    let project = validate_project(&request.project_path, &state.app_data_dir)?;
    let provider = probe_codex().await;
    if !provider.available {
        return Err(AppError::InvalidArgument {
            message: format!(
                "Codex CLI is not executable: {}",
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
    if profiles.is_empty() {
        return Err(AppError::InvalidArgument {
            message: "none of the runbook agents resolved to real catalog personas".into(),
        });
    }
    let project_path = project.to_string_lossy().into_owned();
    let default_stages = initial_stages(&request.agent_slugs, &request.stage_labels);
    let resume = resumable_run(
        &state,
        intent,
        &project_path,
        &request.runbook_id,
        &request.capability_id,
        &default_stages,
    )
    .await;
    let stages = if let Some((previous_id, stages)) = resume {
        if let Some(handle) = state.runtime_jobs.lock().await.remove(&previous_id) {
            handle.abort();
        }
        stages
    } else {
        default_stages
    };
    let now = Utc::now();
    let run = RunSummary {
        id: Uuid::new_v4().to_string(),
        intent: intent.into(),
        project_path,
        runbook_id: request.runbook_id,
        capability_id: request.capability_id,
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
            let max_attempts = if stage_id == "qa" { 3 } else { 1 };
            while current.stages[index].attempt <= max_attempts {
                let prompt = stage_prompt(
                    &current,
                    index,
                    profiles.get(&current.stages[index].agent_slug),
                );
                match run_codex_stage(
                    Path::new(&current.project_path),
                    &prompt,
                    &current.id,
                    &stage_id,
                    &on_event,
                )
                .await
                {
                    Ok(true) => {
                        passed = true;
                        break;
                    }
                    Ok(false)
                        if stage_id == "qa" && current.stages[index].attempt < max_attempts =>
                    {
                        let attempt = current.stages[index].attempt;
                        let _ = on_event.send(RunEvent::GateFailed {
                            run_id: current.id.clone(),
                            stage_id: stage_id.clone(),
                            reason: "QA gate failed; returning to Development".into(),
                            attempt,
                        });
                        current.stages[index].status = "failed".into();
                        current.stages[2].status = "running".into();
                        current.stages[2].attempt += 1;
                        current.current_stage = Some("development".into());
                        current.updated_at = Utc::now();
                        let _ = persist_at(&app_data, &current).await;
                        let _ = on_event.send(RunEvent::RunUpdated {
                            run: current.clone(),
                        });
                        let fix_prompt = format!(
                            "The QA gate failed on attempt {attempt}. This is a remediation pass. Inspect the QA evidence and implement every in-scope requirement that is still missing; do not limit the repair to your catalog specialty. Fix the root causes and run the relevant checks.\n\n{}",
                            stage_prompt(&current, 2, profiles.get(&current.stages[2].agent_slug))
                        );
                        if !run_codex_stage(
                            Path::new(&current.project_path),
                            &fix_prompt,
                            &current.id,
                            "development",
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
                        current.stages[2].status = "passed".into();
                        current.stages[index].attempt += 1;
                        current.stages[index].status = "running".into();
                        current.current_stage = Some(stage_id.clone());
                        current.updated_at = Utc::now();
                        let _ = persist_at(&app_data, &current).await;
                        let _ = on_event.send(RunEvent::RunUpdated {
                            run: current.clone(),
                        });
                    }
                    Ok(false) => break,
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

async fn persist_at(app_data: &Path, run: &RunSummary) -> Result<(), AppError> {
    let dir = app_data.join("state").join("runs");
    tokio::fs::create_dir_all(&dir).await?;
    atomic_write(
        &dir.join(format!("{}.json", run.id)),
        &serde_json::to_vec_pretty(run)?,
    )
    .await
}

fn stage_prompt(run: &RunSummary, index: usize, persona: Option<&String>) -> String {
    let s = &run.stages[index];
    let implementation_scope = if s.id == "development" && run.capability_id == "iot" {
        "\nIOT FULL-VERTICAL IMPLEMENTATION MANDATE:\n- This stage owns the complete local prototype described by the user, not firmware alone.\n- Preserve and verify existing firmware work, and also implement the in-scope MQTT path, consumer/backend, persistence, alert rules, API, and responsive dashboard when the intent requires them.\n- Hardware purchase and physical validation remain out of scope unless explicitly authorized; use reproducible local simulation for those boundaries.\n- Read QA evidence already present in the workspace and close every actionable in-scope gap before claiming PASS.\n"
    } else {
        ""
    };
    format!("You are the {} agent ({}) in the IntentOS '{}' autonomous pipeline.\n\nINTENTOS ORCHESTRATOR OVERRIDES (highest priority for this run):\n- The USER INTENT below is the authoritative product specification.\n- Catalog persona references to missing templates, memory-bank files, frameworks, scripts, or organizational conventions are optional guidance, not prerequisites.\n- If useful project documentation is missing, create the minimal appropriate documentation yourself from the USER INTENT and continue autonomously.\n- Choose reasonable technical defaults when the user explicitly delegates the choice. Do not fail merely because an auxiliary file, preferred framework, or prior setup is absent.\n- Do not ask the user to implement or configure anything unless human authorization is genuinely required.\n- Stay within the requested scope and do not invent product requirements.\n{}\nCATALOG PERSONA INSTRUCTIONS:\n{}\n\nUSER INTENT:\n{}\nPROJECT: {}\n\nWork only inside the project. Inspect existing work and perform this stage for real. Run relevant checks. Do not claim success without evidence. End your final response with exactly INTENTOS_GATE:PASS only if this stage genuinely passes; otherwise end with INTENTOS_GATE:FAIL and explain a genuine blocker. Previous stages are present in the workspace.", s.label, s.agent_slug, run.runbook_id, implementation_scope, persona.map(String::as_str).unwrap_or("Catalog persona unavailable; disclose this limitation."), run.intent, run.project_path)
}

async fn run_codex_stage(
    project: &Path,
    prompt: &str,
    run_id: &str,
    stage_id: &str,
    channel: &Channel<RunEvent>,
) -> Result<bool, AppError> {
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
            .arg(prompt)
            .current_dir(project)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| AppError::Io {
                message: format!("could not start Codex CLI: {e}"),
            })?;
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
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = err_channel.send(RunEvent::Output {
                    run_id: err_run.clone(),
                    stage_id: err_stage.clone(),
                    stream: "stderr".into(),
                    text: clean_text(&line),
                });
            }
        });
        let mut lines = BufReader::new(stdout).lines();
        let mut combined = String::new();
        while let Some(line) = lines.next_line().await? {
            let text = clean_text(&line);
            combined.push_str(&text);
            combined.push('\n');
            let _ = channel.send(RunEvent::Output {
                run_id: run_id.into(),
                stage_id: stage_id.into(),
                stream: "stdout".into(),
                text,
            });
        }
        let status = child.wait().await?;
        let _ = stderr_task.await;
        Ok::<bool, AppError>(status.success() && output_gate_passed(&combined))
    };
    tokio::time::timeout(Duration::from_secs(60 * 30), execution)
        .await
        .map_err(|_| AppError::Io {
            message: "runtime stage timed out".into(),
        })?
}

#[cfg(test)]
mod tests {
    use super::*;
    fn valid_request() -> StartRunRequest {
        StartRunRequest {
            intent: "Build a verified product".into(),
            project_path: "/tmp/project".into(),
            runbook_id: "startup-mvp".into(),
            capability_id: "digital-experience".into(),
            stage_labels: (1..=5).map(|index| format!("Stage {index}")).collect(),
            agent_slugs: (1..=5).map(|index| format!("agent-{index}")).collect(),
            provider_id: PROVIDER_ID.into(),
        }
    }
    #[test]
    fn rejects_root_project() {
        let root = Path::new(if cfg!(windows) { r"C:\" } else { "/" });
        assert!(validate_project(root.to_str().unwrap(), Path::new("/tmp/app")).is_err());
    }
    #[test]
    fn creates_five_real_pipeline_stages() {
        let s = initial_stages(&[], &[]);
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
        let s = initial_stages(&agents, &labels);
        assert_eq!(s[1].label, "Arquitectura IoT");
        assert_eq!(s[2].agent_slug, "firmware-engineer");
        assert_eq!(s[3].label, "QA físico");
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
            runbook_id: "startup-mvp".into(),
            capability_id: "iot".into(),
            provider_id: PROVIDER_ID.into(),
            status: RunStatus::Running,
            current_stage: Some("development".into()),
            stages: initial_stages(&[], &[]),
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
            runbook_id: "startup-mvp".into(),
            capability_id: "iot".into(),
            provider_id: PROVIDER_ID.into(),
            status: RunStatus::Failed,
            current_stage: Some("development".into()),
            stages: initial_stages(&[], &[]),
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
    fn iot_development_prompt_owns_the_full_vertical() {
        let now = Utc::now();
        let mut run = RunSummary {
            id: "run".into(),
            intent: "ESP32, MQTT, API and dashboard".into(),
            project_path: "/tmp/project".into(),
            runbook_id: "startup-mvp".into(),
            capability_id: "iot".into(),
            provider_id: PROVIDER_ID.into(),
            status: RunStatus::Running,
            current_stage: Some("development".into()),
            stages: initial_stages(&[], &[]),
            created_at: now,
            updated_at: now,
            completed_at: None,
            error: None,
        };
        run.stages[2].id = "development".into();
        let prompt = stage_prompt(&run, 2, None);
        assert!(prompt.contains("IOT FULL-VERTICAL IMPLEMENTATION MANDATE"));
        assert!(prompt.contains("consumer/backend, persistence, alert rules, API"));
    }
}
