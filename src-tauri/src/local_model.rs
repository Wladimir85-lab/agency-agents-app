//! IntentOS-owned local inference boundary.
//!
//! This module does not install or trust a particular model runner. It
//! discovers an OpenAI-compatible local endpoint and the native llama.cpp
//! assets that IntentOS may manage. External APIs never satisfy
//! `sovereign_ready`: the endpoint must be loopback and a local model must be
//! configured.

use serde::Serialize;
use std::{
    env,
    path::{Path, PathBuf},
    time::Duration,
};

const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:8080";
const SERVER_ENV: &str = "INTENTOS_LLAMA_SERVER_PATH";
const MODEL_ENV: &str = "INTENTOS_LOCAL_MODEL_PATH";
const ENDPOINT_ENV: &str = "INTENTOS_LOCAL_MODEL_ENDPOINT";

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
    pub sovereign_ready: bool,
    pub blockers: Vec<String>,
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

fn discover_server() -> Option<PathBuf> {
    existing_file(env::var(SERVER_ENV).ok())
        .or_else(|| find_on_path(executable_name(), env::var("PATH").ok().as_deref()))
}

fn discover_nvidia_driver() -> bool {
    if find_on_path("nvidia-smi.exe", env::var("PATH").ok().as_deref()).is_some() {
        return true;
    }
    #[cfg(windows)]
    {
        Path::new(r"C:\Windows\System32\nvidia-smi.exe").is_file()
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

pub async fn status() -> LocalModelStatus {
    let endpoint = env::var(ENDPOINT_ENV).unwrap_or_else(|_| DEFAULT_ENDPOINT.into());
    let loopback_endpoint = is_loopback_endpoint(&endpoint);
    let server = discover_server();
    let model = existing_file(env::var(MODEL_ENV).ok());
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
    if server.is_none() && !api_reachable {
        blockers.push("llama-server no está instalado ni ejecutándose.".into());
    }
    if model.is_none() {
        blockers.push("No hay un modelo local autorizado configurado.".into());
    }
    if !nvidia_driver_detected {
        blockers.push("El controlador NVIDIA/CUDA no está disponible para IntentOS.".into());
    }
    if api_reachable && !api_ready {
        blockers.push("El servidor local responde, pero el modelo aún no está listo.".into());
    }

    let sovereign_ready =
        loopback_endpoint && api_ready && model.is_some() && nvidia_driver_detected;

    LocalModelStatus {
        gateway: "LocalModelGateway",
        endpoint,
        loopback_endpoint,
        api_reachable,
        api_ready,
        server_executable: server.map(|path| path.to_string_lossy().into_owned()),
        model_path: model.map(|path| path.to_string_lossy().into_owned()),
        nvidia_driver_detected,
        sovereign_ready,
        blockers,
    }
}

#[tauri::command]
pub async fn local_model_status() -> LocalModelStatus {
    status().await
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
