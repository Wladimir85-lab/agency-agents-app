//! IntentOS-owned architecture contracts.
//!
//! External products implement replaceable bindings. They are never the
//! identity of the system: IntentOS owns the capability names and the
//! lifecycle from human intent to verified delivery.

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
    pub shell: &'static str,
    pub bindings: Vec<FabricBinding>,
}

pub fn status() -> FabricStatus {
    FabricStatus {
        owner: "IntentOS",
        principle: "maximum internal sophistication, minimum human complexity",
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
