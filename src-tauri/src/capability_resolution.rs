//! Capability resolution — the Fabric link between "I need a computational
//! capability" and "here is the available provider that does it, if any".
//!
//! Fabric contract: `CapabilityResolution` → `computeProviderRegistry` (see
//! `fabric.rs`). This is deliberately **not** the same concept as
//! `IntentOSCapability` in `src/lib/data/intentosCapabilities.ts` — that is
//! a fixed roster of 5 personas per domain area, used to assemble a Runtime
//! v0.1 pipeline team. This module never touches that, and the capability
//! ids it registers (`dataset.inspect`, `dataset.validate`, `dataset.dedup`)
//! deliberately live in a different namespace so the two are never
//! confusable at a glance. It is also not `ModelGateway` — that resolves
//! which agent CLI executes a Runtime v0.1 *stage*; this resolves which
//! backend executes one atomic computational operation.
//!
//! This module owns exactly two things: a small, fixed registry mapping a
//! capability id to an ordered list of provider bindings, and two Tauri
//! commands (`capability_resolve`, `capability_invoke`) that walk it. It
//! does not know what a "dataset" is beyond a JSON `path` field, and it
//! does not know what "Soup" is beyond one registry row naming it — the
//! caller of `capability_invoke` never names Soup at all. Adding a second
//! provider for an existing capability id, or a new capability id bound to
//! a different provider, is a registry-only change; the resolution logic
//! below never needs to change for that.
//!
//! Deliberately not a plugin system: providers are plain Rust types
//! implementing a small trait, registered in a fixed `match`, not a
//! dynamic loader or a config-driven registry. `async_trait` is already a
//! dependency (see `commands::updater::UpdaterBackend`) — reused here
//! rather than inventing a second abstraction mechanism.

use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;

use crate::error::AppError;
use crate::soup;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    pub provider_id: String,
    pub available: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityResolution {
    pub capability_id: String,
    pub resolved_provider_id: String,
    /// Every provider that was probed for this capability, in registry
    /// order — not just the winner. Lets a caller see *why* a resolution
    /// picked what it picked, without a second round trip.
    pub candidates: Vec<ProviderStatus>,
}

#[async_trait]
trait CapabilityProvider: Send + Sync {
    async fn probe(&self) -> ProviderStatus;
    async fn invoke(&self, args: Value) -> Result<Value, AppError>;
}

fn require_path(args: &Value) -> Result<String, AppError> {
    args.get("path")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| AppError::InvalidArgument {
            message: "args.path is required".into(),
        })
}

async fn soup_provider_status() -> ProviderStatus {
    let status = soup::soup_status().await;
    ProviderStatus {
        provider_id: "soup".into(),
        available: status.available,
        message: status.message,
    }
}

struct SoupInspect;
#[async_trait]
impl CapabilityProvider for SoupInspect {
    async fn probe(&self) -> ProviderStatus {
        soup_provider_status().await
    }
    async fn invoke(&self, args: Value) -> Result<Value, AppError> {
        let path = require_path(&args)?;
        let result = soup::soup_inspect_dataset(path).await?;
        Ok(serde_json::to_value(result)?)
    }
}

struct SoupValidate;
#[async_trait]
impl CapabilityProvider for SoupValidate {
    async fn probe(&self) -> ProviderStatus {
        soup_provider_status().await
    }
    async fn invoke(&self, args: Value) -> Result<Value, AppError> {
        let path = require_path(&args)?;
        let result = soup::soup_validate_dataset(path).await?;
        Ok(serde_json::to_value(result)?)
    }
}

struct SoupDedup;
#[async_trait]
impl CapabilityProvider for SoupDedup {
    async fn probe(&self) -> ProviderStatus {
        soup_provider_status().await
    }
    async fn invoke(&self, args: Value) -> Result<Value, AppError> {
        let path = require_path(&args)?;
        let result = soup::soup_dedup_dataset(path).await?;
        Ok(serde_json::to_value(result)?)
    }
}

/// The registry. Adding a provider: append to the `Vec`. Adding a
/// capability: add a `match` arm. Nothing else in this file changes.
fn providers_for(capability_id: &str) -> Option<Vec<Box<dyn CapabilityProvider>>> {
    match capability_id {
        "dataset.inspect" => Some(vec![Box::new(SoupInspect)]),
        "dataset.validate" => Some(vec![Box::new(SoupValidate)]),
        "dataset.dedup" => Some(vec![Box::new(SoupDedup)]),
        _ => None,
    }
}

async fn probe_all(providers: &[Box<dyn CapabilityProvider>]) -> Vec<ProviderStatus> {
    let mut candidates = Vec::with_capacity(providers.len());
    for provider in providers {
        candidates.push(provider.probe().await);
    }
    candidates
}

fn unavailable(capability_id: &str) -> AppError {
    AppError::CapabilityProviderUnavailable {
        capability_id: capability_id.to_string(),
        message: "no provider registered for this capability is currently available".into(),
    }
}

#[tauri::command]
pub async fn capability_resolve(capability_id: String) -> Result<CapabilityResolution, AppError> {
    let providers = providers_for(&capability_id).ok_or_else(|| AppError::UnknownCapability {
        capability_id: capability_id.clone(),
    })?;

    let candidates = probe_all(&providers).await;
    match candidates.iter().find(|status| status.available) {
        Some(winner) => Ok(CapabilityResolution {
            capability_id,
            resolved_provider_id: winner.provider_id.clone(),
            candidates,
        }),
        None => Err(unavailable(&capability_id)),
    }
}

#[tauri::command]
pub async fn capability_invoke(capability_id: String, args: Value) -> Result<Value, AppError> {
    let providers = providers_for(&capability_id).ok_or_else(|| AppError::UnknownCapability {
        capability_id: capability_id.clone(),
    })?;

    for provider in providers.iter() {
        if provider.probe().await.available {
            return provider.invoke(args).await;
        }
    }

    Err(unavailable(&capability_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resolving_an_unregistered_capability_id_fails_as_unknown_not_unavailable() {
        let err = capability_resolve("dataset.frobnicate".into())
            .await
            .expect_err("must fail — nothing simulates success for an unregistered id");
        match err {
            AppError::UnknownCapability { capability_id } => {
                assert_eq!(capability_id, "dataset.frobnicate");
            }
            other => panic!("expected UnknownCapability, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn invoking_an_unregistered_capability_id_fails_as_unknown_not_unavailable() {
        let err = capability_invoke("dataset.frobnicate".into(), serde_json::json!({}))
            .await
            .expect_err("must fail — nothing simulates success for an unregistered id");
        assert!(matches!(err, AppError::UnknownCapability { .. }));
    }

    #[test]
    fn require_path_rejects_missing_and_empty_path() {
        assert!(require_path(&serde_json::json!({})).is_err());
        assert!(require_path(&serde_json::json!({"path": ""})).is_err());
        assert!(require_path(&serde_json::json!({"path": "   "})).is_err());
    }

    #[test]
    fn require_path_accepts_a_real_path() {
        let path = require_path(&serde_json::json!({"path": "./sample.jsonl"})).unwrap();
        assert_eq!(path, "./sample.jsonl");
    }

    #[test]
    fn every_registered_capability_id_lives_in_the_dataset_namespace() {
        // Guards against accidentally reusing an IntentOSCapability-style id
        // (e.g. "digital-experience") here — the two id spaces must never
        // collide, since they mean structurally different things.
        for id in ["dataset.inspect", "dataset.validate", "dataset.dedup"] {
            assert!(
                providers_for(id).is_some(),
                "{id} should resolve to a provider list"
            );
            assert!(id.starts_with("dataset."));
        }
        assert!(providers_for("digital-experience").is_none());
    }

    // ---- Real-CLI integration tests (same doctrine as soup.rs) -----------
    //
    //     cargo test --lib capability_resolution:: -- --ignored

    fn alpaca_fixture() -> tempfile::NamedTempFile {
        use std::io::Write;
        let mut file = tempfile::Builder::new()
            .suffix(".jsonl")
            .tempfile()
            .expect("create temp fixture");
        writeln!(file, r#"{{"instruction": "What is 2+2?", "output": "4"}}"#).unwrap();
        writeln!(file, r#"{{"instruction": "What is 2+2?", "output": "4"}}"#).unwrap();
        writeln!(
            file,
            r#"{{"instruction": "Capital of France?", "output": "Paris"}}"#
        )
        .unwrap();
        file.flush().unwrap();
        file
    }

    #[tokio::test]
    #[ignore = "requires the real `soup` CLI on PATH"]
    async fn resolves_dataset_inspect_to_soup_without_the_caller_naming_it() {
        let resolution = capability_resolve("dataset.inspect".into())
            .await
            .expect("soup should be available and resolve");
        assert_eq!(resolution.resolved_provider_id, "soup");
        assert!(resolution
            .candidates
            .iter()
            .any(|c| c.provider_id == "soup" && c.available));
    }

    #[tokio::test]
    #[ignore = "requires the real `soup` CLI on PATH"]
    async fn invokes_dataset_inspect_through_the_registry_and_gets_real_soup_output() {
        let fixture = alpaca_fixture();
        let path = fixture.path().to_string_lossy().into_owned();
        let value = capability_invoke(
            "dataset.inspect".into(),
            serde_json::json!({ "path": path }),
        )
        .await
        .expect("resolution + invocation should succeed against the real soup install");
        let stdout = value
            .get("stdout")
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert!(stdout.contains("Total samples"));
    }

    #[tokio::test]
    #[ignore = "requires the real `soup` CLI on PATH"]
    async fn invokes_dataset_dedup_through_the_registry_and_cleans_up_after_itself() {
        let fixture = alpaca_fixture();
        let path = fixture.path().to_string_lossy().into_owned();
        let value = capability_invoke("dataset.dedup".into(), serde_json::json!({ "path": path }))
            .await
            .expect("resolution + invocation should succeed against the real soup install");
        let stdout = value
            .get("stdout")
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert!(stdout.contains("3 -> 2 rows"));

        // Same cwd-output quirk as soup.rs's own dedup test — clean it up.
        if let Some(stem) = fixture.path().file_stem().and_then(|s| s.to_str()) {
            if let Ok(cwd) = std::env::current_dir() {
                let _ = std::fs::remove_file(cwd.join(format!("{stem}_deduped.jsonl")));
            }
        }
    }

    #[tokio::test]
    #[ignore = "requires the real `soup` CLI on PATH"]
    async fn invoking_a_known_capability_with_a_bad_path_fails_as_a_domain_result_not_a_panic() {
        let value = capability_invoke(
            "dataset.inspect".into(),
            serde_json::json!({ "path": "this-file-does-not-exist.jsonl" }),
        )
        .await
        .expect("soup itself runs; it just reports a non-zero exit for a missing file");
        let ok = value.get("ok").and_then(Value::as_bool).unwrap_or(true);
        assert!(
            !ok,
            "expected a domain-level failure (ok:false), got: {value:?}"
        );
    }
}
