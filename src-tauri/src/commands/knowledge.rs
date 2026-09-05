//! Knowledge library commands — (re)index a set of local folders containing
//! reference PDFs/Markdown into the BM25 index (see `crate::knowledge`), and
//! expose its status for the Settings UI. Fully local: no network call, no
//! embeddings model — see `knowledge.rs`'s module doc for why.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::State;

use crate::error::AppError;
use crate::knowledge::{self, KnowledgeIndex, KnowledgeStatus};
use crate::state::AppState;
use crate::util::fs::atomic_write;

#[tauri::command]
pub async fn knowledge_status(state: State<'_, AppState>) -> Result<KnowledgeStatus, AppError> {
    let guard = state.knowledge_cache.lock().await;
    Ok(guard.as_ref().map(|idx| idx.status()).unwrap_or_default())
}

/// Rebuilds the index from scratch over `dirs` (each scanned recursively for
/// `.pdf`/`.md` files) plus `knowledge::default_dirs()` — the repo's own
/// curated corpus is always included so a user's own folders are additive,
/// never a replacement for it — and swaps the result into `AppState` +
/// persists a JSON cache so the next app launch doesn't have to re-extract
/// everything. Text extraction is CPU-bound, so it runs in `spawn_blocking`.
#[tauri::command]
pub async fn knowledge_index(
    dirs: Vec<String>,
    state: State<'_, AppState>,
) -> Result<KnowledgeStatus, AppError> {
    let mut dir_paths: Vec<PathBuf> = knowledge::default_dirs();
    dir_paths.extend(dirs.into_iter().map(PathBuf::from));
    let built: KnowledgeIndex = tokio::task::spawn_blocking(move || knowledge::build_index(&dir_paths))
        .await
        .map_err(|e| AppError::Internal {
            message: format!("indexing task panicked: {e}"),
        })??;

    let status = built.status();
    if let Ok(bytes) = serde_json::to_vec(&built) {
        let cache_path = state.app_data_dir.join(knowledge::CACHE_FILE);
        let _ = atomic_write(&cache_path, &bytes).await;
    }
    *state.knowledge_cache.lock().await = Some(Arc::new(built));
    Ok(status)
}
