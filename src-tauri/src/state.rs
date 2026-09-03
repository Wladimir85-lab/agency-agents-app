//! Tauri-managed application state.
//!
//! Holds the agency subsystems:
//! the corpus cache, persisted settings (the source of truth for the
//! network/feature gates), the updater mirror, and the resolved
//! app-data directory that the corpus / install / github / updater
//! modules derive their paths from.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{Mutex, RwLock};

use crate::commands::settings::{self, SettingsLoadState};
use crate::commands::updater::UpdaterState;
use crate::error::AppError;

/// Shared application state. Registered via `Builder::manage()`.
pub struct AppState {
    /// Resolved app-data root — the OS-canonical
    /// `~/Library/Application Support/com.zerologic.agency-agents-app/` directory. The
    /// corpus, install ledger, github cache, and settings file all
    /// derive their paths from this; the security gates that check "is
    /// this path inside our app data dir?" anchor on it too.
    pub app_data_dir: PathBuf,

    /// Phase 1 (corpus) — memoized in-memory corpus (parsed agents +
    /// index). Built lazily on the first `corpus_*` command (seed + parse
    /// + persist index), then served from this cache. `corpus_refresh`
    /// swaps the inner Arc after re-indexing the freshly-fetched tree.
    /// Mirrors the `categories_cache` lazy-`Option<Arc<_>>` pattern.
    pub corpus_cache: Arc<Mutex<Option<Arc<crate::corpus::Corpus>>>>,

    /// Single-flight mutex for `corpus_refresh`, same contract as
    /// `catalog_refresh_in_flight`.
    pub corpus_refresh_in_flight: Arc<Mutex<()>>,

    /// Persisted user settings (Phase 12d). Three-state container that
    /// distinguishes file-absent (defaults apply) from file-corrupt
    /// (fail closed — every outbound call denied until repaired).
    /// `require_network` consults this on the first line of every
    /// network-touching command.
    pub settings: Arc<RwLock<SettingsLoadState>>,

    /// Phase 15 — in-memory mirror of the latest update check + cached
    /// `Available` payload. The auto-check scheduler updates this on
    /// every wake, and `update_install` validates the caller-supplied
    /// version arg against the cached entry to defend against UI
    /// staleness. See `crate::commands::updater::UpdaterState` for the
    /// shape and the rationale.
    pub updater_state: Arc<RwLock<UpdaterState>>,

    /// IntentOS runtime jobs. Handles are backend-owned so the renderer can
    /// cancel a known run without ever supplying a process or shell command.
    pub runtime_jobs: Arc<Mutex<HashMap<String, tokio::task::AbortHandle>>>,

    /// Optional llama.cpp process started by IntentOS. Keeping the child in
    /// backend state gives the renderer start/stop semantics without ever
    /// receiving an executable path or arbitrary command line.
    pub local_model_process: Arc<Mutex<Option<tokio::process::Child>>>,

    /// Showroom — the single live-preview dev-server process, if any. One
    /// slot, same convention as `local_model_process`: a project is either
    /// previewed or it isn't, IntentOS doesn't juggle several at once.
    pub preview_process: Arc<Mutex<Option<crate::preview::RunningPreview>>>,

    /// Public preview — the single `cloudflared` quick-tunnel process, if
    /// any, publishing the Showroom above to a `*.trycloudflare.com` URL.
    /// Same one-slot convention; see deploy.rs.
    pub public_preview_process: Arc<Mutex<Option<crate::deploy::RunningPublicPreview>>>,
}

impl AppState {
    /// Build the state at startup. Resolves the app-data directory and
    /// loads persisted settings; the corpus and updater caches start
    /// empty and hydrate lazily on first use.
    pub fn build() -> Result<Self, AppError> {
        let app_data_dir = resolve_app_data_dir()?;
        if !app_data_dir.exists() {
            std::fs::create_dir_all(&app_data_dir).map_err(|e| AppError::Io {
                message: format!(
                    "could not create app data dir {}: {}",
                    app_data_dir.display(),
                    e
                ),
            })?;
        }

        // Load settings synchronously at startup. The loader handles
        // file-absent (FirstLaunch → defaults), file-corrupt (Corrupt →
        // fail closed in `require_network`), and good parse (Loaded(s)).
        // Tracing warnings for corrupt cases happen inside the loader.
        let settings_state = settings::load_at_startup(&app_data_dir);
        if matches!(settings_state, SettingsLoadState::Corrupt { .. }) {
            tracing::warn!(
                "settings: load failed at startup; require_network will deny outbound calls until user resets"
            );
        }

        Ok(Self {
            app_data_dir,
            corpus_cache: Arc::new(Mutex::new(None)),
            corpus_refresh_in_flight: Arc::new(Mutex::new(())),
            settings: Arc::new(RwLock::new(settings_state)),
            updater_state: crate::commands::updater::empty_state(),
            runtime_jobs: Arc::new(Mutex::new(HashMap::new())),
            local_model_process: Arc::new(Mutex::new(None)),
            preview_process: Arc::new(Mutex::new(None)),
            public_preview_process: Arc::new(Mutex::new(None)),
        })
    }

    /// Consult paranoid mode + settings load state. Returns `Ok(())` if
    /// the outbound call is allowed, or `AppError::ParanoidModeBlocked`
    /// otherwise. **Every outbound command must call this as its first
    /// line** — see the security review §12d "Cross-cutting concerns".
    ///
    /// Three cases:
    /// - `Loaded(s)` with `paranoid_mode == false` → allow.
    /// - `FirstLaunch` → allow (defaults apply, paranoid OFF — preserves
    ///   the v0.1.0 behaviour for users with no settings file yet).
    /// - `Loaded(s)` with `paranoid_mode == true` OR `Corrupt(...)` →
    ///   deny. Corrupt is a deliberate fail-closed: we don't know what
    ///   the user wanted, so we don't make outbound calls until they
    ///   repair the file (or hit Reset to defaults in the UI).
    pub async fn require_network(&self, feature: &'static str) -> Result<(), AppError> {
        let guard = self.settings.read().await;
        network_allowed(&guard, feature)
    }
}

/// Single source of truth for the paranoid-mode decision, shared by
/// `AppState::require_network` (called with a lock already held on
/// `AppState.settings`) and any other caller that only has a cloned
/// `Arc<RwLock<SettingsLoadState>>` in scope — e.g. a spawned run task
/// that cloned `state.settings` before moving into `tokio::spawn`, the
/// same way it already clones `app_data`/`runtime_jobs`. Same three
/// cases as `require_network`'s doc comment: `Loaded` with paranoid
/// off, or `FirstLaunch`, allow; `Loaded` with paranoid on, or
/// `Corrupt`, deny (fail closed).
pub(crate) fn network_allowed(
    settings: &SettingsLoadState,
    feature: &'static str,
) -> Result<(), AppError> {
    match settings {
        SettingsLoadState::Loaded(s) if !s.paranoid_mode => Ok(()),
        SettingsLoadState::FirstLaunch => Ok(()),
        SettingsLoadState::Loaded(_) | SettingsLoadState::Corrupt { .. } => {
            Err(AppError::ParanoidModeBlocked {
                feature: feature.to_string(),
            })
        }
    }
}

/// Resolve the canonical app-data root:
/// `~/Library/Application Support/com.zerologic.agency-agents-app/`. The corpus, install
/// ledger, github cache, and settings file all derive their paths from
/// this; the security gates that check "is this path inside our app data
/// dir?" anchor on it too.
fn resolve_app_data_dir() -> Result<PathBuf, AppError> {
    let mut base = dirs::data_dir().ok_or_else(|| AppError::Internal {
        message: "could not resolve OS data dir".into(),
    })?;
    base.push("com.zerologic.agency-agents-app");
    Ok(base)
}

/// Tauri setup hook — instantiates and manages `AppState`.
pub fn initialize<R: tauri::Runtime>(
    app: &mut tauri::App<R>,
) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::Manager;
    let state = AppState::build()?;
    app.manage(state);
    Ok(())
}

// ---------- Tests ----------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::settings::Settings;

    /// Build a minimal AppState whose only meaningful field is `settings`.
    /// All other fields use whatever `AppState::build` resolves — for the
    /// gate-only tests below the app-data path lookup, catalog load, etc., are
    /// irrelevant. Settings slot is overwritten *after* construction so we
    /// don't depend on whatever happens to be on disk for the test user.
    async fn build_state_with(slot: SettingsLoadState) -> AppState {
        let state = AppState::build().expect("AppState::build");
        {
            let mut guard = state.settings.write().await;
            *guard = slot;
        }
        state
    }

    #[tokio::test]
    async fn require_network_allows_first_launch() {
        let state = build_state_with(SettingsLoadState::FirstLaunch).await;
        assert!(state.require_network("trending_fetch").await.is_ok());
    }

    #[tokio::test]
    async fn require_network_allows_loaded_with_paranoid_off() {
        let s = Settings {
            paranoid_mode: false,
            ..Settings::default()
        };
        let state = build_state_with(SettingsLoadState::Loaded(s)).await;
        assert!(state.require_network("catalog_refresh").await.is_ok());
    }

    #[tokio::test]
    async fn require_network_blocks_when_paranoid_on() {
        let s = Settings {
            paranoid_mode: true,
            ..Settings::default()
        };
        let state = build_state_with(SettingsLoadState::Loaded(s)).await;
        let r = state.require_network("trending_fetch").await;
        match r {
            Err(AppError::ParanoidModeBlocked { feature }) => {
                assert_eq!(feature, "trending_fetch");
            }
            other => panic!("expected ParanoidModeBlocked, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn require_network_blocks_when_corrupt() {
        // Fail-closed: corrupt settings file → deny even though paranoid
        // would default false. This is the load-bearing security gate from
        // the §12d review.
        let state = build_state_with(SettingsLoadState::Corrupt {
            message: "bad json".into(),
        })
        .await;
        let r = state.require_network("cask_icon_from_homepage").await;
        match r {
            Err(AppError::ParanoidModeBlocked { feature }) => {
                assert_eq!(feature, "cask_icon_from_homepage");
            }
            other => panic!("expected ParanoidModeBlocked from corrupt, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn require_network_feature_string_round_trips() {
        // The static-str argument must be carried verbatim into the error
        // so the frontend can route the toast to the right setting.
        let state = build_state_with(SettingsLoadState::Corrupt {
            message: "x".into(),
        })
        .await;
        for feat in [
            "trending_fetch",
            "cask_icon_from_homepage",
            "catalog_refresh",
            "runtime_codex",
        ] {
            let r = state.require_network(feat).await;
            match r {
                Err(AppError::ParanoidModeBlocked { feature }) => {
                    assert_eq!(feature, feat);
                }
                other => panic!("expected block for {feat}, got {other:?}"),
            }
        }
    }
}
