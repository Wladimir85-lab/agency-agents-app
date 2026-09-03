//! Showroom — live preview of the isolated workspace a run is building.
//!
//! Fulfils the `ShowroomPublisher` / `localPreviewV1` contract already
//! reserved in `fabric.rs` ("T1 — visible desire: interactive showroom
//! with a reviewable URL or local embedded preview", see
//! `docs/INTENTOS-GOD-ARCHITECTURE.md`). Only ever points at
//! `RunSummary.workspace_path` (the disposable copy), never the original
//! project — the same "source remains protected until explicit apply"
//! invariant `runtime.rs` already documents for construction itself.
//!
//! Process lifecycle mirrors `local_model.rs`'s single-slot
//! `Option<Child>` in `AppState` (`local_model_start`/`local_model_stop`).
//! It deliberately does **not** follow `runtime_start`'s pattern of
//! awaiting the child inline while streaming progress: a build pipeline
//! terminates on its own, a dev server does not, so `preview_start`
//! spawns the process, hands log/ready streaming to a background task,
//! and returns immediately.
//!
//! Windows gotcha already diagnosed in `runtime.rs` for the `claude`
//! CLI applies identically to `npm`/`npx`: both are `.cmd` shims, and
//! spawning a `.cmd` directly through `tokio::process::Command` hangs the
//! moment stdio is piped. Route through `cmd /C`, same fix, same reason.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use tauri::{ipc::Channel, State};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use crate::error::AppError;
use crate::state::AppState;

pub struct RunningPreview {
    child: Child,
    workspace_path: String,
    url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PreviewEvent {
    Starting,
    Ready { url: String },
    Log { text: String },
    Stopped,
    Failed { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PreviewStatus {
    pub running: bool,
    pub workspace_path: Option<String>,
    pub url: Option<String>,
}

/// Matches the first `http://localhost:<port>` (or loopback IP) printed by
/// a dev server. Verified by hand this session against Naval Studio
/// (`vinext dev` → "Local: http://localhost:3000/") and IntentOS itself
/// (`tauri dev` → "Local: http://localhost:1430/") — every one of Vite,
/// Next.js, vinext and Tauri's own dev command prints this same shape.
fn find_preview_url(line: &str) -> Option<String> {
    for prefix in ["http://localhost:", "http://127.0.0.1:"] {
        if let Some(start) = line.find(prefix) {
            let rest = &line[start..];
            let end = rest
                .find(|c: char| c.is_whitespace())
                .unwrap_or(rest.len());
            return Some(rest[..end].trim_end_matches('/').to_string());
        }
    }
    None
}

/// Reads the workspace's `package.json` and decides how to start it.
/// Requires an explicit `dev` script — IntentOS never guesses a run
/// command for a project shape it doesn't recognise.
fn detect_dev_command(workspace_path: &Path) -> Result<(&'static str, Vec<String>), AppError> {
    let package_json = workspace_path.join("package.json");
    if !package_json.is_file() {
        return Err(AppError::InvalidArgument {
            message: "No hay package.json en la copia de trabajo; IntentOS no sabe cómo levantar la vista previa.".into(),
        });
    }
    let raw = std::fs::read_to_string(&package_json)?;
    let parsed: serde_json::Value = serde_json::from_str(&raw).map_err(|e| AppError::Internal {
        message: format!("package.json inválido: {e}"),
    })?;
    let has_dev_script = parsed
        .get("scripts")
        .and_then(|s| s.as_object())
        .map(|scripts| scripts.contains_key("dev"))
        .unwrap_or(false);
    if !has_dev_script {
        return Err(AppError::InvalidArgument {
            message: "El proyecto no define un script \"dev\"; IntentOS no puede levantar vista previa automática.".into(),
        });
    }
    if workspace_path.join("src-tauri").is_dir() {
        Ok(("npm", vec!["run".into(), "tauri".into(), "dev".into()]))
    } else {
        Ok(("npm", vec!["run".into(), "dev".into()]))
    }
}

#[tauri::command]
pub async fn preview_start(
    state: State<'_, AppState>,
    workspace_path: String,
    on_event: Channel<PreviewEvent>,
) -> Result<PreviewStatus, AppError> {
    let workspace = PathBuf::from(&workspace_path);
    if !workspace.is_dir() {
        return Err(AppError::InvalidArgument {
            message: "La copia de trabajo no existe.".into(),
        });
    }
    let (program, args) = detect_dev_command(&workspace)?;

    let mut guard = state.preview_process.lock().await;
    if let Some(running) = guard.as_mut() {
        if running.child.try_wait()?.is_none() {
            return Err(AppError::InvalidArgument {
                message: "Ya hay una vista previa corriendo; deténla antes de iniciar otra.".into(),
            });
        }
    }

    let _ = on_event.send(PreviewEvent::Starting);

    #[cfg(windows)]
    let mut command = {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(program).args(&args);
        c
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut c = Command::new(program);
        c.args(&args);
        c
    };
    command
        .current_dir(&workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = command.spawn()?;
    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");

    *guard = Some(RunningPreview {
        child,
        workspace_path: workspace_path.clone(),
        url: None,
    });
    drop(guard);

    // Cloned handles, not the `State` borrow — same pattern already used
    // for `runtime_jobs`/`settings` (see state.rs) to move backend state
    // into a task that outlives this command invocation.
    let preview_process = state.preview_process.clone();
    let events = on_event.clone();
    let workspace_for_task = workspace_path.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(text)) => {
                    let _ = events.send(PreviewEvent::Log { text: text.clone() });
                    if let Some(url) = find_preview_url(&text) {
                        let mut guard = preview_process.lock().await;
                        if let Some(running) = guard.as_mut() {
                            if running.workspace_path == workspace_for_task {
                                running.url = Some(url.clone());
                            }
                        }
                        drop(guard);
                        let _ = events.send(PreviewEvent::Ready { url });
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    let _ = events.send(PreviewEvent::Failed {
                        reason: e.to_string(),
                    });
                    break;
                }
            }
        }
        let _ = events.send(PreviewEvent::Stopped);
    });

    let stderr_events = on_event.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(text)) = lines.next_line().await {
            let _ = stderr_events.send(PreviewEvent::Log { text });
        }
    });

    Ok(PreviewStatus {
        running: true,
        workspace_path: Some(workspace_path),
        url: None,
    })
}

#[tauri::command]
pub async fn preview_stop(state: State<'_, AppState>) -> Result<PreviewStatus, AppError> {
    let mut guard = state.preview_process.lock().await;
    if let Some(mut running) = guard.take() {
        // `child.kill()` alone only signals the direct child — here that's
        // `cmd.exe`, not the `npm.cmd` → node.exe dev server underneath it.
        // We hit exactly this orphaned-process failure by hand this session
        // (leftover `node.exe` still holding port 1430 after the parent
        // terminal was closed). `taskkill /T` kills the whole tree.
        #[cfg(windows)]
        {
            if let Some(pid) = running.child.id() {
                let _ = Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/T", "/F"])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .await;
            } else {
                let _ = running.child.kill().await;
            }
        }
        #[cfg(not(windows))]
        {
            let _ = running.child.kill().await;
        }
    }
    Ok(PreviewStatus {
        running: false,
        workspace_path: None,
        url: None,
    })
}

#[tauri::command]
pub async fn preview_status(state: State<'_, AppState>) -> Result<PreviewStatus, AppError> {
    let guard = state.preview_process.lock().await;
    Ok(match guard.as_ref() {
        Some(running) => PreviewStatus {
            running: true,
            workspace_path: Some(running.workspace_path.clone()),
            url: running.url.clone(),
        },
        None => PreviewStatus {
            running: false,
            workspace_path: None,
            url: None,
        },
    })
}

// ---------- Tests ----------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_localhost_url_with_trailing_slash() {
        assert_eq!(
            find_preview_url("  ➜  Local:   http://localhost:3000/"),
            Some("http://localhost:3000".to_string())
        );
    }

    #[test]
    fn finds_url_without_trailing_slash() {
        assert_eq!(
            find_preview_url("Local: http://localhost:1430"),
            Some("http://localhost:1430".to_string())
        );
    }

    #[test]
    fn finds_loopback_ip_url() {
        assert_eq!(
            find_preview_url("ready at http://127.0.0.1:5173/ now"),
            Some("http://127.0.0.1:5173".to_string())
        );
    }

    #[test]
    fn ignores_lines_without_a_url() {
        assert_eq!(find_preview_url("Watching for changes..."), None);
    }

    #[test]
    fn detect_dev_command_requires_a_dev_script() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"build":"vite build"}}"#,
        )
        .unwrap();
        assert!(detect_dev_command(dir.path()).is_err());
    }

    #[test]
    fn detect_dev_command_requires_a_package_json() {
        let dir = tempfile::tempdir().unwrap();
        assert!(detect_dev_command(dir.path()).is_err());
    }

    #[test]
    fn detect_dev_command_finds_plain_dev_script() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"dev":"vite dev"}}"#,
        )
        .unwrap();
        let (program, args) = detect_dev_command(dir.path()).unwrap();
        assert_eq!(program, "npm");
        assert_eq!(args, vec!["run".to_string(), "dev".to_string()]);
    }

    #[test]
    fn detect_dev_command_prefers_tauri_dev_when_src_tauri_present() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"dev":"vite dev","tauri":"tauri"}}"#,
        )
        .unwrap();
        std::fs::create_dir(dir.path().join("src-tauri")).unwrap();
        let (program, args) = detect_dev_command(dir.path()).unwrap();
        assert_eq!(program, "npm");
        assert_eq!(
            args,
            vec!["run".to_string(), "tauri".to_string(), "dev".to_string()]
        );
    }
}
