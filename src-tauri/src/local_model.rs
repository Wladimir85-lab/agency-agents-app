//! IntentOS-owned local inference boundary.
//!
//! The renderer never supplies executable paths or command-line arguments.
//! Remote endpoints never satisfy `sovereign_ready`: the API must be bound to
//! loopback and the authorised model must live inside IntentOS app data (or be
//! supplied explicitly by an engineering environment variable).

use crate::{error::AppError, state::AppState};
use serde::{Deserialize, Serialize};
use std::{
    env,
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};
use tauri::State;
use tokio::process::Command;

const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:8080";
const SERVER_ENV: &str = "INTENTOS_LLAMA_SERVER_PATH";
const MODEL_ENV: &str = "INTENTOS_LOCAL_MODEL_PATH";
const ENDPOINT_ENV: &str = "INTENTOS_LOCAL_MODEL_ENDPOINT";
const RUNNER_VERSION: &str = "b10516";
const MODEL_FILE: &str = "qwen2.5-coder-1.5b-instruct-q4_k_m.gguf";
const MAX_PROMPT_BYTES: usize = 32 * 1024;

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
    pub local: bool,
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

async fn status_at(app_data_dir: &Path) -> LocalModelStatus {
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
    *guard = Some(command.spawn()?);
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
    }
    drop(guard);
    Ok(status_at(&state.app_data_dir).await)
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
    let status = status_at(&state.app_data_dir).await;
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
        "max_tokens": request.max_tokens.unwrap_or(256).clamp(1, 1024),
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
        .await?
        .error_for_status()?
        .json::<ChatResponse>()
        .await?;
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
    })
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
}
