//! IntentOS-owned architecture contracts.
//!
//! External products implement replaceable bindings. They are never the
//! identity of the system: IntentOS owns the capability names and the
//! lifecycle from human intent to verified delivery.
//!
//! Superior definition (constitutional addendum, 2026-09-04 — "traducción
//! general de intención humana a computación"): IntentOS is not
//! fundamentally an IDE-with-AI, a code generator, a coding agent, or an
//! LLM wrapper. Those are possible internal mechanisms. The system's
//! actual identity is a translator of human intention into verified
//! computational results — `translation_pipeline` below names that chain
//! as real, inspectable data, the same way `bindings` makes replaceability
//! real data instead of a comment. Code is one materialization mechanism
//! among several (existing software, an API, a library, a specialized
//! engine); generating more of it is never the automatic answer to every
//! intention. This does not authorize a capability catalog, a new plugin
//! system, or any other large build — see the addendum's own explicit
//! scope limits — it only makes the principle a first-class, testable part
//! of the codebase instead of documentation nobody enforces.

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FabricBinding {
    pub capability: &'static str,
    pub contract: &'static str,
    pub active_binding: &'static str,
    pub replacement_policy: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FabricStatus {
    pub owner: &'static str,
    pub principle: &'static str,
    /// The authoritative stage chain a JOB moves through conceptually,
    /// from a human's intention to a verified result — see the module doc
    /// comment. Ordered; every stage's own machinery (runtime.rs's
    /// JobState/StageCompletionDiagnosis, the local_agent observation
    /// loop, QA/Reality-Check) already implements pieces of this chain,
    /// but this is the one place the chain itself is named as data rather
    /// than left implicit across those modules.
    pub translation_pipeline: &'static [&'static str],
    pub shell: &'static str,
    pub bindings: Vec<FabricBinding>,
}

pub fn status() -> FabricStatus {
    FabricStatus {
        owner: "IntentOS",
        principle: "translate human intention into verified computational results — code, models and tools are replaceable mechanisms, not the identity of the system",
        translation_pipeline: &[
            "intención humana",
            "inteligencia",
            "traducción computacional",
            "materialización",
            "verificación",
            "resultado",
        ],
        shell: "svelteKitTauri",
        bindings: vec![
            binding(
                "humanInterface",
                "IntentInterface",
                "esmeralda",
                "replaceable",
            ),
            binding(
                "capabilitySource",
                "CapabilityCatalog",
                "agencyAgentsCorpus",
                "replaceable",
            ),
            binding(
                "orchestration",
                "WorkflowOrchestrator",
                "intentosRuntimeV1",
                "replaceable",
            ),
            binding(
                "modelGateway",
                "ModelGateway",
                "runtimeProviderAdapters",
                "replaceable",
            ),
            // Detection and health boundary for IntentOS-owned inference.
            // This is deliberately separate from the active ModelGateway:
            // local execution must not be claimed until a native server,
            // authorised model and NVIDIA acceleration pass the readiness gate.
            binding(
                "sovereignInference",
                "LocalModelGateway",
                "localGatewayFoundationV1",
                "replaceable",
            ),
            binding(
                "tooling",
                "ToolGateway",
                "tauriCommandRegistry",
                "replaceable",
            ),
            binding(
                "execution",
                "ExecutionEnvironment",
                "isolatedWorkspace",
                "replaceable",
            ),
            binding(
                "engineeringMemory",
                "EngineeringMemory",
                "runEvidenceStore",
                "replaceable",
            ),
            binding(
                "verification",
                "VerificationGate",
                "qaRealityLoop",
                "replaceable",
            ),
            // Esmeralda's persistent memory (session.rs): one conversation
            // per project, with the workspace it evolves. Distinct from
            // EngineeringMemory (run evidence/manifests) — this is the
            // human-facing conversation itself, not the audit trail.
            binding(
                "conversationMemory",
                "ConversationMemory",
                "projectSessionsV1",
                "replaceable",
            ),
            binding(
                "showroom",
                "ShowroomPublisher",
                "localPreviewV1",
                "replaceable",
            ),
            // Publishes the approved workspace to a temporary
            // internet-reachable URL (open it from a phone, share it with
            // a client) without exposing the developer's own localhost —
            // `localPreviewV1` above only ever serves loopback. Bound to a
            // Cloudflare "quick tunnel" (see deploy.rs) after the user
            // explicitly chose that provider and was told its trust model
            // (anonymous, no account, Cloudflare's edge is a real
            // intermediary) — never wired silently.
            binding(
                "publicPreview",
                "PublicPreviewPublisher",
                "cloudflaredQuickTunnel",
                "replaceable",
            ),
            binding(
                "delivery",
                "DeliveryPublisher",
                "reviewApplyReceipt",
                "replaceable",
            ),
            // Added alongside `capability_resolution.rs`. Distinct from
            // CapabilityCatalog (persona/roster selection for the 5-stage
            // pipeline) and from ModelGateway (which agent CLI runs a
            // stage): this is the link from "I need a computational
            // capability" to "here is the available provider that does
            // it, if any" — e.g. dataset.inspect/validate/dedup resolving
            // to Soup today, without the caller ever naming Soup.
            binding(
                "capabilityResolution",
                "CapabilityResolution",
                "computeProviderRegistry",
                "replaceable",
            ),
        ],
    }
}

fn binding(
    capability: &'static str,
    contract: &'static str,
    active_binding: &'static str,
    replacement_policy: &'static str,
) -> FabricBinding {
    FabricBinding {
        capability,
        contract,
        active_binding,
        replacement_policy,
    }
}

#[tauri::command]
pub fn fabric_status() -> FabricStatus {
    status()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intentos_owns_every_contract_and_external_bindings_are_replaceable() {
        let fabric = status();
        assert_eq!(fabric.owner, "IntentOS");
        assert_eq!(fabric.bindings.len(), 14);
        assert!(fabric
            .bindings
            .iter()
            .all(|binding| binding.replacement_policy == "replaceable"));
        assert!(fabric.bindings.iter().any(|binding| {
            binding.contract == "CapabilityCatalog"
                && binding.active_binding == "agencyAgentsCorpus"
        }));
        assert!(fabric.bindings.iter().any(|binding| {
            binding.contract == "ShowroomPublisher" && binding.active_binding == "localPreviewV1"
        }));
        assert!(fabric.bindings.iter().any(|binding| {
            binding.contract == "LocalModelGateway"
                && binding.active_binding == "localGatewayFoundationV1"
        }));
        assert!(fabric.bindings.iter().any(|binding| {
            binding.contract == "ConversationMemory"
                && binding.active_binding == "projectSessionsV1"
        }));
        assert!(fabric.bindings.iter().any(|binding| {
            binding.contract == "PublicPreviewPublisher"
                && binding.active_binding == "cloudflaredQuickTunnel"
        }));
    }

    #[test]
    fn principle_states_translation_of_intention_not_code_generation() {
        // The constitutional addendum's own test: IntentOS's stated
        // identity must never collapse back into "generates code" — this
        // is the literal check that the doctrine is real, inspectable
        // data, not just a comment someone can drift away from silently.
        let fabric = status();
        assert!(fabric.principle.contains("intention"));
        assert!(fabric.principle.contains("verified"));
        assert!(
            !fabric.principle.to_lowercase().contains("code generat"),
            "the principle must not reduce IntentOS's identity to code generation"
        );
    }

    #[test]
    fn translation_pipeline_names_the_full_intention_to_result_chain_in_order() {
        let fabric = status();
        assert_eq!(
            fabric.translation_pipeline,
            &[
                "intención humana",
                "inteligencia",
                "traducción computacional",
                "materialización",
                "verificación",
                "resultado",
            ]
        );
    }

    #[test]
    fn capability_resolution_is_its_own_contract_distinct_from_capability_catalog() {
        let fabric = status();
        let resolution = fabric
            .bindings
            .iter()
            .find(|binding| binding.contract == "CapabilityResolution")
            .expect("CapabilityResolution contract must be present");
        assert_eq!(resolution.active_binding, "computeProviderRegistry");
        // The two must never collapse into the same binding — CapabilityCatalog
        // is persona/roster selection; CapabilityResolution is provider
        // discovery for a computational need. Confusing the two was the
        // exact naming risk this contract was created to avoid.
        assert_ne!(
            resolution.active_binding,
            fabric
                .bindings
                .iter()
                .find(|b| b.contract == "CapabilityCatalog")
                .unwrap()
                .active_binding
        );
    }
}
