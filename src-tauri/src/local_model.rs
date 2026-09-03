//! IntentOS-owned inference boundary.
//!
//! The renderer never supplies executable paths or command-line arguments.
//! Remote endpoints never satisfy `sovereign_ready`: the loopback API must
//! be bound to loopback and the authorised model must live inside IntentOS
//! app data (or be supplied explicitly by an engineering environment
//! variable). `sovereign_ready`/`LocalModelStatus` describe that loopback
//! path exclusively and keep meaning exactly what they always meant.
//!
//! A second, explicitly-opt-in backend (Groq) can carry the same
//! `local_agent` turn loop over the network instead — see
//! [`InferenceBackend`], [`GroqBackendStatus`] and the `Groq` branch of
//! [`complete_raw`]. It is never "sovereign" (it leaves the machine), is
//! gated by [`crate::state::network_allowed`] on every call, and is
//! disjoint from the loopback path: it does not read `ENDPOINT_ENV` and is
//! never treated as reachable/ready without an actual completion call
//! having succeeded.

use crate::{
    commands::settings::SettingsLoadState,
    error::AppError,
    state::{network_allowed, AppState},
};
use serde::{Deserialize, Serialize};
use std::{
    env,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant},
};
use tauri::State;
use tokio::{process::Command, sync::RwLock};

const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:8080";
const SERVER_ENV: &str = "INTENTOS_LLAMA_SERVER_PATH";
const MODEL_ENV: &str = "INTENTOS_LOCAL_MODEL_PATH";
const ENDPOINT_ENV: &str = "INTENTOS_LOCAL_MODEL_ENDPOINT";
const RUNNER_VERSION: &str = "b10516";
const MODEL_FILE: &str = "qwen2.5-coder-1.5b-instruct-q4_k_m.gguf";
const MAX_PROMPT_BYTES: usize = 32 * 1024;
const PID_FILE: &str = "runner.pid";

// ---------- Groq backend (opt-in, non-sovereign, network-gated) ----------

/// Selects which backend `complete_raw` talks to. Read fresh on every call
/// (not cached) so flipping the env var between runs — or between the
/// health probe and the next turn — takes effect immediately, same
/// freshness contract as `configured_port`/`status_at` already have for
/// the loopback path.
const BACKEND_ENV: &str = "INTENTOS_INFERENCE_BACKEND";
const GROQ_API_KEY_ENV: &str = "INTENTOS_GROQ_API_KEY";
const GROQ_MODEL_ENV: &str = "INTENTOS_GROQ_MODEL";
const GROQ_DEFAULT_MODEL: &str = "openai/gpt-oss-120b";
/// Fixed and not overridable via `ENDPOINT_ENV` or any other env var: the
/// whole point is that no local-looking configuration can ever cause an
/// outbound call to be treated as loopback-exempt from `network_allowed`.
const GROQ_ENDPOINT: &str = "https://api.groq.com/openai/v1/chat/completions";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InferenceBackend {
    Loopback,
    Groq,
    Ollama,
}

fn configured_backend() -> InferenceBackend {
    match env::var(BACKEND_ENV) {
        Ok(value) if value.eq_ignore_ascii_case("groq") => InferenceBackend::Groq,
        Ok(value) if value.eq_ignore_ascii_case("ollama") => InferenceBackend::Ollama,
        _ => InferenceBackend::Loopback,
    }
}

// ---------- Ollama backend (opt-in, local, no network gate) ----------
//
// Unlike Groq, Ollama never leaves the machine — same trust category as
// the loopback llama-server path, so no `network_allowed` consultation
// here. It is kept as its own backend (not folded into the loopback
// path's `INTENTOS_LOCAL_MODEL_ENDPOINT` override) because the loopback
// path's readiness/model-resolution logic assumes IntentOS itself
// downloaded and manages a specific `.gguf` file (`discover_model`,
// `managed_model`, `status_at`'s `model_path`) — none of that applies to
// Ollama, which manages its own models independently. Verified live
// against a real `qwen2.5-coder:3b` pull: correctly recovers from a
// redundant-write observation with INTENTOS_GATE:PASS (4/4 trials) where
// the loopback path's Qwen2.5-Coder-1.5B never did, even with the
// redundant-write guard in place — see agentLog.md 2026-09-02.
const OLLAMA_MODEL_ENV: &str = "INTENTOS_OLLAMA_MODEL";
const OLLAMA_ENDPOINT_ENV: &str = "INTENTOS_OLLAMA_ENDPOINT";
const OLLAMA_DEFAULT_MODEL: &str = "qwen2.5-coder:3b";
const OLLAMA_DEFAULT_ENDPOINT: &str = "http://localhost:11434/v1/chat/completions";

/// Static readiness only — never the result of a network probe. Unlike
/// loopback (which has a cheap, local `/health` endpoint worth polling),
/// Groq has no separate health check IntentOS should be spending a network
/// call — and therefore a `network_allowed` gate consultation — on just to
/// answer a status query. `reachable`/`model_available` stay `None`
/// ("not verified") until an actual `complete_raw` call has been attempted;
/// see `record_groq_probe_result`. `backend_ready` reflects only
/// `configured` and must never be read as "verified working".
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GroqBackendStatus {
    pub configured: bool,
    pub reachable: Option<bool>,
    pub model_available: Option<bool>,
    pub backend_ready: bool,
    pub blockers: Vec<String>,
}

fn groq_status() -> GroqBackendStatus {
    let backend_selected = configured_backend() == InferenceBackend::Groq;
    let has_key = env::var(GROQ_API_KEY_ENV)
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false);
    let configured = backend_selected && has_key;
    let mut blockers = Vec::new();
    if !backend_selected {
        blockers.push(format!("{BACKEND_ENV} no está en \"groq\"."));
    }
    if !has_key {
        blockers.push(format!("{GROQ_API_KEY_ENV} no está configurada."));
    }
    GroqBackendStatus {
        configured,
        reachable: None,
        model_available: None,
        backend_ready: configured,
        blockers,
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalModelStatus {
    pub gateway: &'static str,
    pub endpoint: String,
    pub loopback_endpoint: bool,
    pub api_reachable: bool,
    pub api_ready: bool,
    pub server_executable: Option<String>,
    pub model_path: Option<String>,
    pub nvidia_driver_detected: bool,
    pub execution_mode: &'static str,
    pub sovereign_ready: bool,
    pub blockers: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalCompletionRequest {
    pub prompt: String,
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalCompletion {
    pub content: String,
    pub model: String,
    pub latency_ms: u64,
    /// `true` only for the loopback backend — meaning unchanged from
    /// before Groq existed. A Groq completion is never "local".
    pub local: bool,
    /// Explicit backend identification, additive: `"loopback"` or
    /// `"groq"`. Existing callers that only read `local` keep working
    /// unchanged.
    pub backend: &'static str,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    model: Option<String>,
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    content: String,
}

fn executable_name() -> &'static str {
    if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    }
}

fn existing_file(value: Option<String>) -> Option<PathBuf> {
    value.map(PathBuf::from).filter(|path| path.is_file())
}

fn find_on_path(name: &str, path_value: Option<&str>) -> Option<PathBuf> {
    path_value.and_then(|value| {
        env::split_paths(value)
            .map(|directory| directory.join(name))
            .find(|candidate| candidate.is_file())
    })
}

fn managed_server(app_data_dir: &Path) -> PathBuf {
    app_data_dir
        .join("local-model")
        .join("runner")
        .join(RUNNER_VERSION)
        .join(executable_name())
}

fn managed_model(app_data_dir: &Path) -> PathBuf {
    app_data_dir
        .join("local-model")
        .join("models")
        .join(MODEL_FILE)
}

fn pid_file_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("local-model").join(PID_FILE)
}

/// Best-effort: record the PID of a runner IntentOS just spawned, so a
/// later `local_model_stop` can still reach it after this process (and its
/// in-memory `Child` handle) is gone — e.g. a crashed or force-closed
/// previous session left the runner orphaned but listening on loopback.
fn record_pid(app_data_dir: &Path, pid: u32) {
    let _ = std::fs::write(pid_file_path(app_data_dir), pid.to_string());
}

fn clear_pid_file(app_data_dir: &Path) {
    let _ = std::fs::remove_file(pid_file_path(app_data_dir));
}

/// Stop a runner IntentOS no longer holds an in-memory `Child` handle for —
/// this process restarted, or a prior session exited (crashed, force-closed)
/// without running `local_model_stop` first. Tries the PID IntentOS itself
/// recorded at spawn time; failing that, falls back to whichever process is
/// actually bound to the configured loopback port, which also covers a
/// runner that predates the PID file or was started outside IntentOS
/// entirely (e.g. by hand during development). Either way, a PID is only
/// ever killed after confirming it still names the managed `llama-server`
/// executable — never an arbitrary/reused PID.
async fn stop_orphaned_runner(app_data_dir: &Path) -> bool {
    if kill_recorded_pid(app_data_dir).await {
        return true;
    }
    let Some(port) = configured_port() else {
        return false;
    };
    let Some(pid) = find_pid_on_port(port).await else {
        return false;
    };
    if !process_is_llama_server(pid).await {
        return false;
    }
    kill_pid(pid).await
}

async fn kill_recorded_pid(app_data_dir: &Path) -> bool {
    let Ok(raw) = std::fs::read_to_string(pid_file_path(app_data_dir)) else {
        return false;
    };
    let Ok(pid) = raw.trim().parse::<u32>() else {
        clear_pid_file(app_data_dir);
        return false;
    };
    if !process_is_llama_server(pid).await {
        clear_pid_file(app_data_dir);
        return false;
    }
    let killed = kill_pid(pid).await;
    clear_pid_file(app_data_dir);
    killed
}

fn configured_port() -> Option<u16> {
    let endpoint = env::var(ENDPOINT_ENV).unwrap_or_else(|_| DEFAULT_ENDPOINT.into());
    url::Url::parse(&endpoint)
        .ok()
        .and_then(|u| u.port_or_known_default())
}

#[cfg(windows)]
async fn find_pid_on_port(port: u16) -> Option<u32> {
    let mut command = Command::new("netstat");
    command.args(["-ano", "-p", "TCP"]);
    command.creation_flags(0x08000000);
    let out = command.output().await.ok()?;
    let needle = format!(":{port}");
    String::from_utf8_lossy(&out.stdout).lines().find_map(|line| {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() == 5 && cols[0] == "TCP" && cols[1].ends_with(&needle) && cols[3] == "LISTENING" {
            cols[4].parse::<u32>().ok()
        } else {
            None
        }
    })
}

#[cfg(not(windows))]
async fn find_pid_on_port(port: u16) -> Option<u32> {
    let out = Command::new("lsof")
        .args(["-t", "-iTCP", &format!(":{port}"), "-sTCP:LISTEN"])
        .output()
        .await
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .and_then(|s| s.trim().parse::<u32>().ok())
}

#[cfg(windows)]
async fn process_is_llama_server(pid: u32) -> bool {
    let filter = format!("PID eq {pid}");
    let mut command = Command::new("tasklist");
    command.args(["/FI", &filter, "/NH", "/FO", "CSV"]);
    command.creation_flags(0x08000000);
    match command.output().await {
        Ok(out) => String::from_utf8_lossy(&out.stdout).contains("llama-server"),
        Err(_) => false,
    }
}

#[cfg(not(windows))]
async fn process_is_llama_server(pid: u32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/comm")) {
        Ok(name) => name.trim().contains("llama-server"),
        Err(_) => false,
    }
}

#[cfg(windows)]
async fn kill_pid(pid: u32) -> bool {
    let mut command = Command::new("taskkill");
    command.args(["/PID", &pid.to_string(), "/F"]);
    command.creation_flags(0x08000000);
    command
        .output()
        .await
        .map(|out| out.status.success())
        .unwrap_or(false)
}

#[cfg(not(windows))]
async fn kill_pid(pid: u32) -> bool {
    Command::new("kill")
        .args(["-9", &pid.to_string()])
        .output()
        .await
        .map(|out| out.status.success())
        .unwrap_or(false)
}

fn discover_server(app_data_dir: &Path) -> Option<PathBuf> {
    existing_file(env::var(SERVER_ENV).ok())
        .or_else(|| {
            managed_server(app_data_dir)
                .is_file()
                .then(|| managed_server(app_data_dir))
        })
        .or_else(|| find_on_path(executable_name(), env::var("PATH").ok().as_deref()))
}

fn discover_model(app_data_dir: &Path) -> Option<PathBuf> {
    existing_file(env::var(MODEL_ENV).ok()).or_else(|| {
        managed_model(app_data_dir)
            .is_file()
            .then(|| managed_model(app_data_dir))
    })
}

fn discover_nvidia_driver() -> bool {
    if find_on_path("nvidia-smi.exe", env::var("PATH").ok().as_deref()).is_some() {
        return true;
    }
    #[cfg(windows)]
    {
        Path::new(r"C:\Windows\System32\nvidia-smi.exe").is_file()
            || Path::new(r"C:\Program Files\NVIDIA Corporation\NVSMI\nvidia-smi.exe").is_file()
    }
    #[cfg(not(windows))]
    {
        find_on_path("nvidia-smi", env::var("PATH").ok().as_deref()).is_some()
    }
}

fn is_loopback_endpoint(endpoint: &str) -> bool {
    let Ok(url) = url::Url::parse(endpoint) else {
        return false;
    };
    matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "::1"))
}

async fn probe_health(endpoint: &str) -> (bool, bool) {
    let url = format!("{}/health", endpoint.trim_end_matches('/'));
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_millis(900))
        .build()
    {
        Ok(client) => client,
        Err(_) => return (false, false),
    };
    match client.get(url).send().await {
        Ok(response) => (true, response.status().is_success()),
        Err(_) => (false, false),
    }
}

pub(crate) async fn status_at(app_data_dir: &Path) -> LocalModelStatus {
    let endpoint = env::var(ENDPOINT_ENV).unwrap_or_else(|_| DEFAULT_ENDPOINT.into());
    let loopback_endpoint = is_loopback_endpoint(&endpoint);
    let server = discover_server(app_data_dir);
    let model = discover_model(app_data_dir);
    let nvidia_driver_detected = discover_nvidia_driver();
    let (api_reachable, api_ready) = if loopback_endpoint {
        probe_health(&endpoint).await
    } else {
        (false, false)
    };
    let mut blockers = Vec::new();
    if !loopback_endpoint {
        blockers.push("El endpoint configurado no es local.".into());
    }
    if server.is_none() {
        blockers.push("llama-server no está instalado dentro de IntentOS.".into());
    }
    if model.is_none() {
        blockers.push("No hay un modelo local autorizado configurado.".into());
    }
    if !api_reachable {
        blockers.push("El motor local está apagado.".into());
    } else if !api_ready {
        blockers.push("El servidor local responde, pero el modelo aún no está listo.".into());
    }
    let sovereign_ready = loopback_endpoint && api_ready && server.is_some() && model.is_some();
    LocalModelStatus {
        gateway: "LocalModelGateway",
        endpoint,
        loopback_endpoint,
        api_reachable,
        api_ready,
        server_executable: server.map(|path| path.to_string_lossy().into_owned()),
        model_path: model.map(|path| path.to_string_lossy().into_owned()),
        nvidia_driver_detected,
        execution_mode: if nvidia_driver_detected {
            "nvidia"
        } else {
            "cpu"
        },
        sovereign_ready,
        blockers,
    }
}

#[tauri::command]
pub async fn local_model_status(state: State<'_, AppState>) -> Result<LocalModelStatus, AppError> {
    Ok(status_at(&state.app_data_dir).await)
}

#[tauri::command]
pub async fn local_model_start(state: State<'_, AppState>) -> Result<LocalModelStatus, AppError> {
    let current = status_at(&state.app_data_dir).await;
    if current.sovereign_ready {
        return Ok(current);
    }
    let server = discover_server(&state.app_data_dir).ok_or_else(|| AppError::InvalidArgument {
        message: "IntentOS no encuentra su runner local verificado".into(),
    })?;
    let model = discover_model(&state.app_data_dir).ok_or_else(|| AppError::InvalidArgument {
        message: "IntentOS no encuentra su modelo local autorizado".into(),
    })?;
    let mut guard = state.local_model_process.lock().await;
    if let Some(child) = guard.as_mut() {
        if child.try_wait()?.is_none() {
            drop(guard);
            return Ok(status_at(&state.app_data_dir).await);
        }
    }
    let mut command = Command::new(&server);
    command
        .arg("-m")
        .arg(&model)
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            "8080",
            "--ctx-size",
            "2048",
            "--threads",
            "4",
            "--threads-batch",
            "4",
            "--no-webui",
        ])
        .current_dir(server.parent().unwrap_or(&state.app_data_dir))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        command.creation_flags(0x08000000);
    }
    let child = command.spawn()?;
    if let Some(pid) = child.id() {
        record_pid(&state.app_data_dir, pid);
    }
    *guard = Some(child);
    drop(guard);
    for _ in 0..40 {
        let status = status_at(&state.app_data_dir).await;
        if status.sovereign_ready {
            return Ok(status);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Err(AppError::Internal {
        message: "el modelo local no quedó listo dentro de 20 segundos".into(),
    })
}

#[tauri::command]
pub async fn local_model_stop(state: State<'_, AppState>) -> Result<LocalModelStatus, AppError> {
    let mut guard = state.local_model_process.lock().await;
    if let Some(mut child) = guard.take() {
        child.kill().await?;
        clear_pid_file(&state.app_data_dir);
    } else {
        // No in-memory handle — this process didn't spawn the runner
        // that's answering on loopback (e.g. a prior IntentOS session
        // exited without stopping it first). Fall back to the PID
        // IntentOS itself recorded at spawn time.
        stop_orphaned_runner(&state.app_data_dir).await;
    }
    drop(guard);
    Ok(status_at(&state.app_data_dir).await)
}

/// Shared core of a single completion turn: checks readiness, builds the
/// chat-completion request and returns the model's raw reply. Used both by
/// the `local_model_complete` command (single-turn, human-facing) and by
/// `local_agent`'s multi-turn loop (each iteration is one call here) — one
/// chokepoint per backend, so callers can never drift on request shape,
/// timeout or readiness handling. `settings` is only ever consulted on the
/// Groq branch — the loopback branch is unchanged and ungated, exactly as
/// before Groq existed, because it never leaves the machine.
pub(crate) async fn complete_raw(
    app_data_dir: &Path,
    prompt: &str,
    max_tokens: Option<u32>,
    settings: &Arc<RwLock<SettingsLoadState>>,
) -> Result<LocalCompletion, AppError> {
    match configured_backend() {
        InferenceBackend::Loopback => complete_raw_loopback(app_data_dir, prompt, max_tokens).await,
        InferenceBackend::Groq => complete_raw_groq(prompt, max_tokens, settings).await,
        InferenceBackend::Ollama => complete_raw_ollama(prompt, max_tokens).await,
    }
}

/// No `network_allowed` gate: Ollama is local, same trust category as
/// `complete_raw_loopback`. No readiness probe either — same reasoning as
/// Groq's `GroqBackendStatus`: the completion call itself is the only
/// honest proof of reachability, so a failed connection surfaces as a
/// plain `reqwest` error via `?` rather than a separately-maintained
/// status that could drift from reality.
async fn complete_raw_ollama(prompt: &str, max_tokens: Option<u32>) -> Result<LocalCompletion, AppError> {
    let endpoint = env::var(OLLAMA_ENDPOINT_ENV).unwrap_or_else(|_| OLLAMA_DEFAULT_ENDPOINT.into());
    let model_name = env::var(OLLAMA_MODEL_ENV).unwrap_or_else(|_| OLLAMA_DEFAULT_MODEL.into());
    let body = serde_json::json!({
        "model": model_name,
        "messages": [
            {"role": "system", "content": "Eres el motor de inferencia de IntentOS. Entrega resultados concisos, técnicos y verificables."},
            {"role": "user", "content": prompt}
        ],
        "temperature": 0.1,
        "max_tokens": max_tokens.unwrap_or(256).clamp(1, 1024),
        "stream": false
    });
    let started = Instant::now();
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()?
        .post(&endpoint)
        .json(&body)
        .send()
        .await?;
    let response_status = response.status();
    if !response_status.is_success() {
        let body_text = response
            .text()
            .await
            .unwrap_or_else(|e| format!("<no se pudo leer el cuerpo de la respuesta: {e}>"));
        return Err(AppError::Internal {
            message: format!(
                "HTTP {} de Ollama en POST {endpoint} (prompt ~{} bytes enviados): {}",
                response_status.as_u16(),
                prompt.len(),
                body_text
            ),
        });
    }
    let response = response.json::<ChatResponse>().await?;
    let content = response
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content)
        .ok_or_else(|| AppError::Internal {
            message: "Ollama respondió sin una opción de resultado".into(),
        })?;
    Ok(LocalCompletion {
        content,
        model: response.model.unwrap_or(model_name),
        latency_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
        local: true,
        backend: "ollama",
    })
}

async fn complete_raw_loopback(
    app_data_dir: &Path,
    prompt: &str,
    max_tokens: Option<u32>,
) -> Result<LocalCompletion, AppError> {
    let status = status_at(app_data_dir).await;
    if !status.sovereign_ready {
        return Err(AppError::CapabilityProviderUnavailable {
            capability_id: "inference.local".into(),
            message: status.blockers.join(" "),
        });
    }
    let model = PathBuf::from(status.model_path.as_deref().unwrap_or(MODEL_FILE));
    let model_name = model
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(MODEL_FILE);
    let body = serde_json::json!({
        "model": model_name,
        "messages": [
            {"role": "system", "content": "Eres el motor local soberano de IntentOS. Entrega resultados concisos, técnicos y verificables."},
            {"role": "user", "content": prompt}
        ],
        "temperature": 0.1,
        "max_tokens": max_tokens.unwrap_or(256).clamp(1, 1024),
        "stream": false
    });
    let started = Instant::now();
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()?
        .post(format!(
            "{}/v1/chat/completions",
            status.endpoint.trim_end_matches('/')
        ))
        .json(&body)
        .send()
        .await?;
    // Observability only: a bare `.error_for_status()?` here converts any
    // non-2xx into a generic reqwest error and discards llama-server's own
    // response body — which is exactly where a context-overflow or
    // malformed-request explanation lives. Read and preserve it instead.
    // Does not change the success path at all: on 2xx this is a single
    // extra `is_success()` check before the existing `.json()` call below.
    let response_status = response.status();
    if !response_status.is_success() {
        let body_text = response
            .text()
            .await
            .unwrap_or_else(|e| format!("<no se pudo leer el cuerpo de la respuesta: {e}>"));
        return Err(AppError::Internal {
            message: format!(
                "HTTP {} del motor local en POST /v1/chat/completions (prompt ~{} bytes enviados): {}",
                response_status.as_u16(),
                prompt.len(),
                body_text
            ),
        });
    }
    let response = response.json::<ChatResponse>().await?;
    let content = response
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content)
        .ok_or_else(|| AppError::Internal {
            message: "el modelo local respondió sin una opción de resultado".into(),
        })?;
    Ok(LocalCompletion {
        content,
        model: response.model.unwrap_or_else(|| model_name.into()),
        latency_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
        local: true,
        backend: "loopback",
    })
}

/// Groq branch: `network_allowed` is consulted first and unconditionally,
/// before any request is built — a run with paranoid mode ON gets
/// `ParanoidModeBlocked`, never a real outbound call, no matter what
/// `groq_status()` would have reported. The API key is read once, used
/// only to set the `Authorization` header, and never appears in any
/// `Serialize` struct or error message this function returns.
async fn complete_raw_groq(
    prompt: &str,
    max_tokens: Option<u32>,
    settings: &Arc<RwLock<SettingsLoadState>>,
) -> Result<LocalCompletion, AppError> {
    {
        let guard = settings.read().await;
        network_allowed(&guard, "local_model_groq")?;
    }
    let status = groq_status();
    if !status.configured {
        return Err(AppError::CapabilityProviderUnavailable {
            capability_id: "inference.groq".into(),
            message: status.blockers.join(" "),
        });
    }
    // SAFETY of the unwrap: `status.configured` above already proved this
    // env var is set to a non-empty value.
    let api_key = env::var(GROQ_API_KEY_ENV).unwrap();
    let model_name = env::var(GROQ_MODEL_ENV).unwrap_or_else(|_| GROQ_DEFAULT_MODEL.into());
    groq_completion_request(GROQ_ENDPOINT, &api_key, &model_name, prompt, max_tokens).await
}

/// The actual HTTP request + response classification, parameterised over
/// the endpoint so tests can point it at a fake server. Production code
/// only ever reaches this through `complete_raw_groq` above, which always
/// passes the fixed `GROQ_ENDPOINT` constant and has already run the
/// `network_allowed` + `groq_status().configured` gates — this function
/// itself does not re-check either, so it must never be reachable from
/// outside this module in a non-test build.
#[cfg_attr(not(test), allow(dead_code))]
async fn groq_completion_request(
    endpoint: &str,
    api_key: &str,
    model_name: &str,
    prompt: &str,
    max_tokens: Option<u32>,
) -> Result<LocalCompletion, AppError> {
    let body = serde_json::json!({
        "model": model_name,
        "messages": [
            {"role": "system", "content": "Eres el motor de inferencia de IntentOS. Entrega resultados concisos, técnicos y verificables."},
            {"role": "user", "content": prompt}
        ],
        "temperature": 0.1,
        "max_tokens": max_tokens.unwrap_or(256).clamp(1, 1024),
        "stream": false,
        // Groq-specific, gpt-oss-family parameter (not part of the base
        // OpenAI chat-completions schema, and not sent on the loopback
        // llama-server path at all): asks the model to spend more of its
        // own reasoning budget before answering. Started at "high" per
        // an explicit request to reduce the kind of stuck,
        // repeat-without-progress turn seen on the Qwen loopback path;
        // lowered to "medium" after "high" measurably spent the entire
        // MAX_TURN_TOKENS budget on internal reasoning with zero visible
        // output in ~1 of 5 trivial-prompt calls (a turn with no
        // INTENTOS_ACTION/INTENTOS_GATE marker at all — its own
        // stuck-turn failure mode for the causality parser). Lowered
        // again to "low": a live 3-stage vertical run against the free
        // Groq tier hit its 8000-tokens/minute cap after 6 Direction
        // turns at "medium" — every reasoning token spent counts
        // against that same per-minute budget, so this is the
        // cheapest, zero-cost lever to try before assuming the account
        // tier itself is insufficient.
        "reasoning_effort": "low"
    });
    let started = Instant::now();
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()?
        .post(endpoint)
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await?;
    let response_status = response.status();
    if !response_status.is_success() {
        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let body_text = response
            .text()
            .await
            .unwrap_or_else(|e| format!("<no se pudo leer el cuerpo de la respuesta: {e}>"));
        let message = match response_status.as_u16() {
            401 | 403 => format!(
                "Groq rechazó la credencial configurada (HTTP {}): {}",
                response_status.as_u16(),
                body_text
            ),
            429 => format!(
                "Groq devolvió 429 (límite de tasa){}: {}",
                retry_after
                    .map(|value| format!(", retry-after {value}s"))
                    .unwrap_or_default(),
                body_text
            ),
            other => format!(
                "HTTP {other} de Groq en POST /openai/v1/chat/completions (prompt ~{} bytes enviados): {}",
                prompt.len(),
                body_text
            ),
        };
        return Err(AppError::Internal { message });
    }
    let response = response.json::<ChatResponse>().await?;
    let content = response
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content)
        .ok_or_else(|| AppError::Internal {
            message: "Groq respondió sin una opción de resultado".into(),
        })?;
    Ok(LocalCompletion {
        content,
        model: response.model.unwrap_or_else(|| model_name.to_owned()),
        latency_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
        local: false,
        backend: "groq",
    })
}

#[tauri::command]
pub async fn local_model_complete(
    state: State<'_, AppState>,
    request: LocalCompletionRequest,
) -> Result<LocalCompletion, AppError> {
    let prompt = request.prompt.trim();
    if prompt.is_empty() || prompt.len() > MAX_PROMPT_BYTES {
        return Err(AppError::InvalidArgument {
            message: format!("el prompt local debe contener entre 1 y {MAX_PROMPT_BYTES} bytes"),
        });
    }
    complete_raw(&state.app_data_dir, prompt, request.max_tokens, &state.settings).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_loopback_endpoints_can_be_sovereign() {
        assert!(is_loopback_endpoint("http://127.0.0.1:8080"));
        assert!(is_loopback_endpoint("http://localhost:8080/v1"));
        assert!(!is_loopback_endpoint("https://integrate.api.nvidia.com/v1"));
        assert!(!is_loopback_endpoint("not a url"));
    }
    #[test]
    fn managed_assets_stay_beneath_app_data() {
        let root = PathBuf::from("C:/intentos-test-data");
        assert!(managed_server(&root).starts_with(&root));
        assert!(managed_model(&root).starts_with(&root));
    }
    #[test]
    fn executable_discovery_is_explicit_and_bounded() {
        let temp = tempfile::tempdir().unwrap();
        let binary = temp.path().join(executable_name());
        std::fs::write(&binary, b"test").unwrap();
        let joined = env::join_paths([temp.path()]).unwrap();
        assert_eq!(
            find_on_path(executable_name(), joined.to_str()),
            Some(binary)
        );
        assert_eq!(find_on_path("missing", joined.to_str()), None);
    }

    // Guards every env var these tests mutate process-wide
    // (INTENTOS_LOCAL_MODEL_ENDPOINT/PATH, INTENTOS_LLAMA_SERVER_PATH,
    // INTENTOS_INFERENCE_BACKEND, INTENTOS_GROQ_API_KEY/MODEL) — same
    // precedent as the strict-mode env-var test in runtime.rs.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Test-only seam: exercises `groq_completion_request` (the request
    /// build + response classification `complete_raw_groq` delegates to
    /// after its gates pass) against a fake server, instead of the fixed
    /// `GROQ_ENDPOINT` constant. Bypasses `network_allowed` and
    /// `groq_status().configured` deliberately — those are proved
    /// separately by `complete_raw_groq_is_blocked_by_paranoid_mode_*`
    /// and `complete_raw_groq_fails_closed_when_not_configured`, which go
    /// through the real `complete_raw` entry point.
    async fn complete_raw_groq_over_fake_endpoint(
        endpoint: &str,
        prompt: &str,
        api_key: &str,
    ) -> Result<LocalCompletion, AppError> {
        groq_completion_request(endpoint, api_key, GROQ_DEFAULT_MODEL, prompt, Some(10)).await
    }

    fn settings_arc(paranoid_mode: bool) -> Arc<RwLock<SettingsLoadState>> {
        Arc::new(RwLock::new(SettingsLoadState::Loaded(
            crate::commands::settings::Settings {
                paranoid_mode,
                ..Default::default()
            },
        )))
    }

    /// Hand-rolled HTTP/1.1 server (no mocking crate in the dependency
    /// tree) that answers `/health` with 200 and anything else with a
    /// caller-supplied status + JSON body — enough to drive `complete_raw`
    /// through `status_at`'s readiness probe and then its real completion
    /// request, without needing a real llama-server.
    async fn spawn_fake_llama_server(
        completion_status: &'static str,
        completion_body: &'static str,
    ) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let request = String::from_utf8_lossy(&buf[..n]);
                    let (status_line, body): (&str, &str) = if request.starts_with("GET /health") {
                        ("200 OK", "{\"status\":\"ok\"}")
                    } else {
                        (completion_status, completion_body)
                    };
                    let response = format!(
                        "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        (addr, handle)
    }

    #[tokio::test]
    async fn complete_raw_preserves_http_status_and_body_on_a_non_success_response() {
        let _guard = ENV_LOCK.lock().unwrap();

        // A realistic llama.cpp-shaped context-overflow error body, so the
        // test also documents what that failure actually looks like on the
        // wire, not just an arbitrary 400.
        let error_body = "{\"error\":{\"code\":400,\"message\":\"the request exceeds the available context size, try increasing it\",\"type\":\"exceed_context_size_error\"}}";
        let (addr, server) = spawn_fake_llama_server("400 Bad Request", error_body).await;

        let model_file = tempfile::NamedTempFile::new().unwrap();
        let server_file = tempfile::NamedTempFile::new().unwrap();
        // SAFETY: guarded by ENV_LOCK; no other test reads these vars.
        unsafe {
            env::set_var("INTENTOS_LOCAL_MODEL_ENDPOINT", format!("http://{addr}"));
            env::set_var("INTENTOS_LOCAL_MODEL_PATH", model_file.path());
            env::set_var("INTENTOS_LLAMA_SERVER_PATH", server_file.path());
        }

        let app_data = tempfile::tempdir().unwrap();
        let long_prompt = "x".repeat(5000);
        let result = complete_raw(app_data.path(), &long_prompt, Some(10), &settings_arc(false)).await;

        // SAFETY: same guard.
        unsafe {
            env::remove_var("INTENTOS_LOCAL_MODEL_ENDPOINT");
            env::remove_var("INTENTOS_LOCAL_MODEL_PATH");
            env::remove_var("INTENTOS_LLAMA_SERVER_PATH");
        }
        server.abort();

        let err = result.expect_err("a non-2xx response must surface as Err");
        let message = err.to_string();
        assert!(message.contains("400"), "status not preserved: {message}");
        assert!(
            message.contains("exceed_context_size_error"),
            "response body not preserved: {message}"
        );
        assert!(
            message.contains("the request exceeds the available context size"),
            "response body not preserved: {message}"
        );
        // Approximate request-size diagnostic, per the design: bytes sent,
        // not a token count, and no truncation/compaction introduced.
        assert!(message.contains("5000 bytes"), "prompt size not reported: {message}");
    }

    #[tokio::test]
    async fn complete_raw_normal_success_path_is_unaffected() {
        let _guard = ENV_LOCK.lock().unwrap();
        let (addr, server) = spawn_fake_llama_server(
            "200 OK",
            "{\"model\":\"m\",\"choices\":[{\"message\":{\"content\":\"hola\"}}]}",
        )
        .await;

        let model_file = tempfile::NamedTempFile::new().unwrap();
        let server_file = tempfile::NamedTempFile::new().unwrap();
        // SAFETY: guarded by ENV_LOCK.
        unsafe {
            env::set_var("INTENTOS_LOCAL_MODEL_ENDPOINT", format!("http://{addr}"));
            env::set_var("INTENTOS_LOCAL_MODEL_PATH", model_file.path());
            env::set_var("INTENTOS_LLAMA_SERVER_PATH", server_file.path());
        }

        let app_data = tempfile::tempdir().unwrap();
        let result = complete_raw(app_data.path(), "hola", Some(10), &settings_arc(false)).await;

        // SAFETY: same guard.
        unsafe {
            env::remove_var("INTENTOS_LOCAL_MODEL_ENDPOINT");
            env::remove_var("INTENTOS_LOCAL_MODEL_PATH");
            env::remove_var("INTENTOS_LLAMA_SERVER_PATH");
        }
        server.abort();

        let completion = result.expect("a 2xx response must still succeed exactly as before");
        assert_eq!(completion.content, "hola");
        assert_eq!(completion.backend, "loopback");
        assert!(completion.local, "loopback completions must keep local == true");
    }

    // ---------- Ollama backend ----------

    fn clear_ollama_env() {
        // SAFETY: guarded by ENV_LOCK in every caller.
        unsafe {
            env::remove_var(BACKEND_ENV);
            env::remove_var(OLLAMA_ENDPOINT_ENV);
            env::remove_var(OLLAMA_MODEL_ENV);
        }
    }

    #[tokio::test]
    async fn complete_raw_ollama_success_marks_backend_and_stays_local() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_ollama_env();
        let (addr, server) = spawn_fake_llama_server(
            "200 OK",
            "{\"model\":\"qwen2.5-coder:3b\",\"choices\":[{\"message\":{\"content\":\"INTENTOS_GATE:PASS\"}}]}",
        )
        .await;
        // SAFETY: guarded by ENV_LOCK.
        unsafe {
            env::set_var(BACKEND_ENV, "ollama");
            env::set_var(OLLAMA_ENDPOINT_ENV, format!("http://{addr}"));
        }

        let result = complete_raw(Path::new("unused"), "hola", Some(10), &settings_arc(false)).await;

        clear_ollama_env();
        server.abort();

        let completion = result.expect("a 2xx response must succeed");
        assert_eq!(completion.content, "INTENTOS_GATE:PASS");
        assert_eq!(completion.backend, "ollama");
        assert!(completion.local, "Ollama runs locally — local must stay true, unlike Groq");
    }

    #[tokio::test]
    async fn complete_raw_ollama_never_consults_network_allowed() {
        // No `settings_arc` gate check here on purpose: paranoid mode ON
        // must NOT block Ollama, since it's local — proven by succeeding
        // with a paranoid-mode-ON settings handle, unlike the Groq
        // equivalent test which expects a block.
        let _guard = ENV_LOCK.lock().unwrap();
        clear_ollama_env();
        let (addr, server) = spawn_fake_llama_server(
            "200 OK",
            "{\"model\":\"qwen2.5-coder:3b\",\"choices\":[{\"message\":{\"content\":\"hola\"}}]}",
        )
        .await;
        // SAFETY: guarded by ENV_LOCK.
        unsafe {
            env::set_var(BACKEND_ENV, "ollama");
            env::set_var(OLLAMA_ENDPOINT_ENV, format!("http://{addr}"));
        }

        let result = complete_raw(Path::new("unused"), "hola", Some(10), &settings_arc(true)).await;

        clear_ollama_env();
        server.abort();

        assert!(
            result.is_ok(),
            "paranoid mode must never block the local Ollama backend: {result:?}"
        );
    }

    #[tokio::test]
    async fn complete_raw_ollama_preserves_http_status_and_body_on_a_non_success_response() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_ollama_env();
        let (addr, server) = spawn_fake_llama_server(
            "500 Internal Server Error",
            "{\"error\":\"model not found: qwen2.5-coder:3b\"}",
        )
        .await;
        // SAFETY: guarded by ENV_LOCK.
        unsafe {
            env::set_var(BACKEND_ENV, "ollama");
            env::set_var(OLLAMA_ENDPOINT_ENV, format!("http://{addr}"));
        }

        let result = complete_raw(Path::new("unused"), "hola", Some(10), &settings_arc(false)).await;

        clear_ollama_env();
        server.abort();

        let err = result.expect_err("a non-2xx response must surface as Err");
        let message = err.to_string();
        assert!(message.contains("500"), "status not preserved: {message}");
        assert!(
            message.contains("model not found"),
            "response body not preserved: {message}"
        );
    }

    // ---------- Groq backend ----------

    fn clear_groq_env() {
        // SAFETY: guarded by ENV_LOCK in every caller.
        unsafe {
            env::remove_var(BACKEND_ENV);
            env::remove_var(GROQ_API_KEY_ENV);
            env::remove_var(GROQ_MODEL_ENV);
        }
    }

    #[test]
    fn groq_status_is_not_configured_without_backend_selection_or_key() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_groq_env();
        let status = groq_status();
        assert!(!status.configured);
        assert!(!status.backend_ready);
        assert_eq!(status.reachable, None, "must never guess reachability");
        assert_eq!(status.model_available, None, "must never guess model availability");
        clear_groq_env();
    }

    #[test]
    fn groq_status_requires_both_backend_selection_and_a_non_empty_key() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_groq_env();
        // SAFETY: guarded by ENV_LOCK.
        unsafe {
            env::set_var(BACKEND_ENV, "groq");
            env::set_var(GROQ_API_KEY_ENV, "   ");
        }
        assert!(
            !groq_status().configured,
            "a blank key must not count as configured"
        );
        // SAFETY: guarded by ENV_LOCK.
        unsafe {
            env::set_var(GROQ_API_KEY_ENV, "gsk_real_key");
        }
        assert!(groq_status().configured);
        assert!(
            groq_status().backend_ready,
            "backend_ready mirrors configured, never a verified-live claim"
        );
        clear_groq_env();
    }

    #[tokio::test]
    async fn complete_raw_groq_is_blocked_by_paranoid_mode_before_any_request_is_built() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_groq_env();
        // SAFETY: guarded by ENV_LOCK.
        unsafe {
            env::set_var(BACKEND_ENV, "groq");
            env::set_var(GROQ_API_KEY_ENV, "gsk_real_key");
        }

        let result = complete_raw(
            Path::new("unused"),
            "hola",
            Some(10),
            &settings_arc(true),
        )
        .await;

        clear_groq_env();

        match result {
            Err(AppError::ParanoidModeBlocked { feature }) => {
                assert_eq!(feature, "local_model_groq");
            }
            other => panic!("expected ParanoidModeBlocked, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn complete_raw_groq_fails_closed_when_not_configured() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_groq_env();
        // SAFETY: guarded by ENV_LOCK.
        unsafe {
            env::set_var(BACKEND_ENV, "groq");
        }

        let result = complete_raw(Path::new("unused"), "hola", Some(10), &settings_arc(false)).await;

        clear_groq_env();

        match result {
            Err(AppError::CapabilityProviderUnavailable { capability_id, .. }) => {
                assert_eq!(capability_id, "inference.groq");
            }
            other => panic!("expected CapabilityProviderUnavailable, got {other:?}"),
        }
    }

    /// Same fake-server technique as `spawn_fake_llama_server`, but this
    /// one also records the full raw request it received (headers + JSON
    /// body) so tests can prove both the bearer token and the request
    /// body shape (e.g. `reasoning_effort`) were actually sent — and,
    /// since `complete_raw_groq` posts to the fixed `GROQ_ENDPOINT`
    /// constant rather than a configurable one, these tests exercise it
    /// indirectly, through `groq_completion_request` pointed at this fake
    /// server via `complete_raw_groq_over_fake_endpoint`.
    async fn spawn_fake_groq_server(
        completion_status: &'static str,
        completion_body: &'static str,
        retry_after: Option<&'static str>,
    ) -> (
        std::net::SocketAddr,
        tokio::task::JoinHandle<()>,
        Arc<tokio::sync::Mutex<Option<String>>>,
    ) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen_request = Arc::new(tokio::sync::Mutex::new(None));
        let seen_request_writer = seen_request.clone();
        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let seen_request = seen_request_writer.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let request = String::from_utf8_lossy(&buf[..n]).into_owned();
                    *seen_request.lock().await = Some(request);
                    let retry_after_header = retry_after
                        .map(|value| format!("Retry-After: {value}\r\n"))
                        .unwrap_or_default();
                    let response = format!(
                        "HTTP/1.1 {completion_status}\r\nContent-Type: application/json\r\n{retry_after_header}Content-Length: {}\r\nConnection: close\r\n\r\n{completion_body}",
                        completion_body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        (addr, handle, seen_request)
    }

    /// `GROQ_ENDPOINT` is a fixed constant (deliberately not overridable
    /// via env var — see its doc comment), so these tests cannot redirect
    /// `complete_raw_groq` to the fake server above the way the loopback
    /// tests redirect via `INTENTOS_LOCAL_MODEL_ENDPOINT`. What they CAN
    /// and do verify without a real Groq account: the paranoid-mode gate
    /// fires before any request exists (previous test), and the
    /// not-configured fail-closed path (previous test). The request/error
    /// shaping this fake server exists to prove (bearer header sent,
    /// 401/403/429 + Retry-After surfaced distinctly) is instead verified
    /// by calling the request-building/error-classification logic
    /// directly against this fake server through a temporary endpoint
    /// override — see `complete_raw_groq_over_fake_endpoint` below, used
    /// only by these tests via `cfg(test)`.
    #[tokio::test]
    async fn complete_raw_groq_success_sends_bearer_auth_and_marks_backend() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_groq_env();
        let (addr, server, seen_request) = spawn_fake_groq_server(
            "200 OK",
            "{\"model\":\"openai/gpt-oss-120b\",\"choices\":[{\"message\":{\"content\":\"hola\"}}]}",
            None,
        )
        .await;

        let result =
            complete_raw_groq_over_fake_endpoint(&format!("http://{addr}"), "hola", "gsk_real_key")
                .await;
        server.abort();

        let completion = result.expect("2xx must succeed");
        assert_eq!(completion.content, "hola");
        assert_eq!(completion.backend, "groq");
        assert!(!completion.local, "a Groq completion must never claim local == true");

        let request = seen_request.lock().await.clone().expect("a request must have arrived");
        // HTTP header names are case-insensitive on the wire — reqwest
        // happens to send a lowercase "authorization:", so compare
        // case-insensitively rather than assuming a specific casing.
        assert!(
            request.to_ascii_lowercase().contains("authorization: bearer gsk_real_key"),
            "bearer token not sent: {request}"
        );
        assert!(
            request.contains(r#""reasoning_effort":"low""#),
            "reasoning_effort not sent in the Groq request body: {request}"
        );
        assert!(
            request.contains(r#""temperature":0.1"#),
            "temperature not sent in the Groq request body: {request}"
        );
    }

    #[tokio::test]
    async fn complete_raw_groq_distinguishes_401_403_429_with_retry_after() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_groq_env();

        let (addr, server, _) = spawn_fake_groq_server(
            "401 Unauthorized",
            "{\"error\":{\"message\":\"invalid api key\"}}",
            None,
        )
        .await;
        let err = complete_raw_groq_over_fake_endpoint(&format!("http://{addr}"), "hola", "bad_key")
            .await
            .expect_err("401 must surface as Err");
        server.abort();
        let message = err.to_string();
        assert!(message.contains("401"), "status not classified: {message}");
        assert!(
            !message.contains("bad_key"),
            "the API key must never appear in an error message: {message}"
        );

        let (addr, server, _) =
            spawn_fake_groq_server("429 Too Many Requests", "{\"error\":{\"message\":\"rate limited\"}}", Some("7"))
                .await;
        let err = complete_raw_groq_over_fake_endpoint(&format!("http://{addr}"), "hola", "gsk_real_key")
            .await
            .expect_err("429 must surface as Err");
        server.abort();
        let message = err.to_string();
        assert!(message.contains("429"), "status not classified: {message}");
        assert!(
            message.contains("retry-after 7s"),
            "Retry-After header not surfaced: {message}"
        );
    }
}
