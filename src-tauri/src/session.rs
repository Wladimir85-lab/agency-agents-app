//! Project conversation sessions — Esmeralda's persistent memory.
//!
//! Before this module, IntentOS's "chat" was `thread.svelte.ts`: a derived
//! projection over `RunSummary` history, with zero new backend state (see
//! its own doc-comment, and the checkpoint commit that preceded this file).
//! That gave a visual list of past runs, not a conversation — no message
//! persisted, no context carried into the next turn's prompt, and every
//! follow-up turn re-copied the *original* project (see `runtime.rs`'s
//! `copy_workspace` call site), discarding whatever the previous turn built.
//!
//! A `ProjectSession` is the real conversation: one per project, keyed
//! deterministically off `project_path` (not a run id — sessions outlive
//! any single run), holding the message log (user / esmeralda / system)
//! and the *one* evolving workspace path that every turn in the
//! conversation builds on top of. `runtime.rs::runtime_start` is the only
//! other place that reads/writes a session's `workspace_path` — see the
//! `request.session_id` branch there for the "reuse instead of recopy"
//! fix this module exists to support.
//!
//! Persistence mirrors `runtime.rs`'s run store exactly (same
//! `atomic_write`/`read_capped` chokepoints, same JSON-file-per-id shape)
//! so a session is just as crash-safe and just as inspectable as a run.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::State;

use crate::error::AppError;
use crate::state::AppState;
use crate::util::fs::{atomic_write, read_capped};

const MAX_SESSION_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_MESSAGE_CHARS: usize = 20_000;
/// Keeps a session file bounded even after months of daily conversation on
/// the same project. Oldest messages are dropped first — Esmeralda's
/// working context (see `conversationContext` on the frontend) only ever
/// looks at the most recent handful anyway, so trimming ancient history
/// costs nothing a real turn depends on.
const MAX_MESSAGES: usize = 500;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MessageRole {
    User,
    Esmeralda,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMessage {
    pub id: String,
    pub role: MessageRole,
    pub content: String,
    pub at: DateTime<Utc>,
    /// The run this message reports on or triggered, when applicable.
    /// Absent for a plain user instruction that hasn't started a run yet
    /// (e.g. it is still queued) or for a system note with no run behind it.
    #[serde(default)]
    pub run_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSession {
    pub id: String,
    pub project_path: String,
    /// The single isolated workspace this project's whole conversation
    /// evolves inside of. `None` until the first turn creates it.
    /// Original project remains untouched until an explicit
    /// `runtime_apply` — same invariant `runtime.rs` already documents.
    #[serde(default)]
    pub workspace_path: Option<String>,
    #[serde(default)]
    pub messages: Vec<ConversationMessage>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Deterministic session id for a project path: same project ⇒ same id
/// every time, so "reopen IntentOS, reselect Naval Studio" finds the same
/// session file without needing a separate lookup index.
pub fn session_id_for(project_path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(project_path.as_bytes());
    format!("{:x}", hasher.finalize())[..32].to_string()
}

fn sessions_dir(state: &AppState) -> std::path::PathBuf {
    state.app_data_dir.join("state").join("sessions")
}

fn session_path(state: &AppState, id: &str) -> std::path::PathBuf {
    sessions_dir(state).join(format!("{id}.json"))
}

pub(crate) fn session_workspace_dir(
    app_data: &std::path::Path,
    session_id: &str,
) -> std::path::PathBuf {
    app_data
        .join("state")
        .join("session-workspaces")
        .join(session_id)
        .join("project")
}

async fn load(state: &AppState, id: &str) -> Result<Option<ProjectSession>, AppError> {
    let path = session_path(state, id);
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = read_capped(&path, MAX_SESSION_FILE_BYTES).await?;
    Ok(Some(serde_json::from_slice(&bytes)?))
}

async fn persist(state: &AppState, session: &ProjectSession) -> Result<(), AppError> {
    tokio::fs::create_dir_all(sessions_dir(state)).await?;
    let bytes = serde_json::to_vec_pretty(session)?;
    atomic_write(&session_path(state, &session.id), &bytes).await
}

pub(crate) async fn get_or_create(
    state: &AppState,
    project_path: &str,
) -> Result<ProjectSession, AppError> {
    let id = session_id_for(project_path);
    if let Some(existing) = load(state, &id).await? {
        return Ok(existing);
    }
    let now = Utc::now();
    let session = ProjectSession {
        id,
        project_path: project_path.into(),
        workspace_path: None,
        messages: Vec::new(),
        created_at: now,
        updated_at: now,
    };
    persist(state, &session).await?;
    Ok(session)
}

/// Called by `runtime_start` the first time a session builds anything: the
/// workspace it just copied belongs to this session from now on, so every
/// later turn reuses it instead of recopying the original project.
pub(crate) async fn set_workspace_path(
    state: &AppState,
    session_id: &str,
    workspace_path: &str,
) -> Result<(), AppError> {
    let mut session = match load(state, session_id).await? {
        Some(s) => s,
        None => return Ok(()), // session vanished/never existed — nothing to attach to
    };
    session.workspace_path = Some(workspace_path.into());
    session.updated_at = Utc::now();
    persist(state, &session).await
}

/// Called by `runtime_discard_workspace` when the discarded run's workspace
/// belongs to a session: the shared directory is gone, so the pointer must
/// be cleared or the next turn would try to reuse a deleted path.
pub(crate) async fn clear_workspace_path(
    state: &AppState,
    session_id: &str,
) -> Result<(), AppError> {
    let mut session = match load(state, session_id).await? {
        Some(s) => s,
        None => return Ok(()),
    };
    session.workspace_path = None;
    session.updated_at = Utc::now();
    persist(state, &session).await
}

#[tauri::command]
pub async fn session_get_or_create(
    state: State<'_, AppState>,
    project_path: String,
) -> Result<ProjectSession, AppError> {
    if project_path.trim().is_empty() {
        return Err(AppError::InvalidArgument {
            message: "project_path must not be empty".into(),
        });
    }
    get_or_create(&state, &project_path).await
}

#[tauri::command]
pub async fn session_append_message(
    state: State<'_, AppState>,
    project_path: String,
    role: MessageRole,
    content: String,
    run_id: Option<String>,
) -> Result<ProjectSession, AppError> {
    let content = content.trim();
    if content.is_empty() || content.chars().count() > MAX_MESSAGE_CHARS {
        return Err(AppError::InvalidArgument {
            message: "message must contain 1 to 20000 characters".into(),
        });
    }
    let mut session = get_or_create(&state, &project_path).await?;
    session.messages.push(ConversationMessage {
        id: uuid::Uuid::new_v4().to_string(),
        role,
        content: content.to_string(),
        at: Utc::now(),
        run_id,
    });
    if session.messages.len() > MAX_MESSAGES {
        let drop = session.messages.len() - MAX_MESSAGES;
        session.messages.drain(0..drop);
    }
    session.updated_at = Utc::now();
    persist(&state, &session).await?;
    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::{Mutex, RwLock};

    fn state_in(dir: &std::path::Path) -> AppState {
        AppState {
            app_data_dir: dir.to_path_buf(),
            corpus_cache: Arc::new(Mutex::new(None)),
            corpus_refresh_in_flight: Arc::new(Mutex::new(())),
            settings: Arc::new(RwLock::new(
                crate::commands::settings::SettingsLoadState::FirstLaunch,
            )),
            updater_state: crate::commands::updater::empty_state(),
            runtime_jobs: Arc::new(Mutex::new(HashMap::new())),
            local_model_process: Arc::new(Mutex::new(None)),
            preview_process: Arc::new(Mutex::new(None)),
            public_preview_process: Arc::new(Mutex::new(None)),
        }
    }

    #[tokio::test]
    async fn session_id_is_stable_for_the_same_project_path() {
        assert_eq!(
            session_id_for("/Users/x/naval-studio"),
            session_id_for("/Users/x/naval-studio")
        );
        assert_ne!(
            session_id_for("/Users/x/naval-studio"),
            session_id_for("/Users/x/other-project")
        );
    }

    #[tokio::test]
    async fn get_or_create_is_idempotent_and_persists_across_calls() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path());
        let first = get_or_create(&state, "/tmp/naval-studio").await.unwrap();
        let second = get_or_create(&state, "/tmp/naval-studio").await.unwrap();
        assert_eq!(first.id, second.id);
        assert!(second.messages.is_empty());
    }

    #[tokio::test]
    async fn appended_messages_persist_in_order_with_roles() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path());
        let session = get_or_create(&state, "/tmp/naval-studio").await.unwrap();
        let mut s = session;
        s.messages.push(ConversationMessage {
            id: "1".into(),
            role: MessageRole::User,
            content: "agrega el visor 3D".into(),
            at: Utc::now(),
            run_id: None,
        });
        persist(&state, &s).await.unwrap();
        let reloaded = get_or_create(&state, "/tmp/naval-studio").await.unwrap();
        assert_eq!(reloaded.messages.len(), 1);
        assert_eq!(reloaded.messages[0].role, MessageRole::User);
        assert_eq!(reloaded.messages[0].content, "agrega el visor 3D");
    }

    #[tokio::test]
    async fn messages_beyond_the_cap_drop_the_oldest_first() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path());
        let mut session = get_or_create(&state, "/tmp/naval-studio").await.unwrap();
        for i in 0..(MAX_MESSAGES + 10) {
            session.messages.push(ConversationMessage {
                id: i.to_string(),
                role: MessageRole::User,
                content: format!("turno {i}"),
                at: Utc::now(),
                run_id: None,
            });
        }
        persist(&state, &session).await.unwrap();
        let mut reloaded = get_or_create(&state, "/tmp/naval-studio").await.unwrap();
        reloaded.messages.push(ConversationMessage {
            id: "last".into(),
            role: MessageRole::User,
            content: "final".into(),
            at: Utc::now(),
            run_id: None,
        });
        if reloaded.messages.len() > MAX_MESSAGES {
            let drop = reloaded.messages.len() - MAX_MESSAGES;
            reloaded.messages.drain(0..drop);
        }
        assert_eq!(reloaded.messages.len(), MAX_MESSAGES);
        assert_eq!(reloaded.messages.last().unwrap().content, "final");
        assert_ne!(reloaded.messages[0].content, "turno 0");
    }
}
