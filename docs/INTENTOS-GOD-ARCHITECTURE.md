# IntentOS GOD — Authoritative Architecture

**Status:** active foundation (2026-08-31)
**Owner:** IntentOS  
**Product boundary:** private, sovereign AI-native IDE/factory. Agency Agents is a capability source, not the factory.

## Governing principle

> Maximum internal sophistication. Minimum human complexity.

The human supplies intent, judgment and irreversible product decisions. IntentOS owns the internal complexity required to turn that intent into a working, verified and deliverable technological result.

## End-to-end lifecycle

`intent -> proposal -> interactive showroom -> approval -> mission -> capability composition -> isolated construction -> QA -> Reality Check -> review -> apply -> delivery receipt`

The current vertical is real and incremental:

1. Esmeralda's production surface captures intent and proposes a visible solution.
2. A draft `Mission` enters the Temporal control plane and waits for an explicit NCTO decision.
3. Temporal durably records approval, rejection or cancellation. Only a confirmed approval may enter production.
4. IntentOS selects reusable professional capabilities from the Agency Agents corpus.
5. The native orchestrator creates an isolated workspace and runs a variable staged workflow.
6. Model-provider adapters execute each stage. Their output is not trusted as proof by itself.
7. QA can return work to Development; Reality Check remains a mandatory final gate.
8. File manifests and stage evidence are persisted independently of conversational output.
9. The human reviews the delta; apply is conflict-checked and backup-first.
10. A delivery receipt is derived from persisted run state, gate state, manifests and apply evidence.

## IntentOS Fabric contracts

The backend command `fabric_status` is the machine-readable source for active bindings. Contract names are owned by IntentOS; every implementation can be replaced without redefining the product.

| IntentOS capability | Contract | Active binding | Boundary |
|---|---|---|---|
| Human interface | `IntentInterface` | Esmeralda | intent and judgment, not orchestration internals |
| Professional knowledge | `CapabilityCatalog` | Agency Agents corpus | knowledge/capabilities only |
| Orchestration | `WorkflowOrchestrator` | IntentOS Runtime v1 | owns workflow, state, repair and gates |
| Models | `ModelGateway` | runtime provider adapters | Codex/Claude today; OmniRoute is only a candidate binding |
| Sovereign inference | `LocalModelGateway` | local gateway foundation v1 | detects loopback inference, an authorised local model and optional hardware acceleration; it is not yet the production runtime |
| Tools | `ToolGateway` | Tauri command registry | policy-controlled tool access |
| Execution | `ExecutionEnvironment` | isolated workspace | source remains protected until explicit apply |
| Memory | `EngineeringMemory` | run evidence store | current truth, manifests and decisions |
| Verification | `VerificationGate` | QA/Reality Loop | explicit PASS/FAIL with remediation |
| Interactive preview | `ShowroomPublisher` | local preview v1 | makes the desire visible before full production; shareable hosting remains an adapter |
| Delivery | `DeliveryPublisher` | review/apply/receipt | verified and applied are separate states |

NeMo, OmniRoute, Theia, Daytona, E2B and future systems may implement one or more contracts. None is an architectural identity of IntentOS.

## Builders, internal tools and external engines

Codex and Claude are engineering systems used to build IntentOS from outside the product. They are not architectural organs of the finished factory. The inherited Agency Agents tool installers (Gemini CLI, Qwen Code, Kimi, OpenClaw and similar applications) remain available as compatibility integrations, but they are classified as **external engines**, not as IntentOS tools.

IntentOS tools are owned capabilities: files and code, terminal and processes, browser and interactive preview, data operations, testing, security/isolation, media production and verified delivery. NVIDIA Build, NVIDIA local inference, open models and future providers supply computation beneath those contracts without becoming the human-facing product model.

### Local inference decision ADR-002

**Decision: BUILD an IntentOS-owned, OpenAI-compatible local gateway around native `llama.cpp`; do not adopt Ollama as an architectural dependency and do not claim NVIDIA NIM compatibility on this machine without a verified runtime.**

The gateway contract is independent of the runner, model and accelerator. Its readiness probe requires a loopback endpoint, a healthy loaded model, an explicitly authorised local model file and an IntentOS-managed runner. NVIDIA availability is reported independently: acceleration improves performance but does not define sovereignty. NVIDIA Build endpoints may be used for evaluation, but remote free inference does not satisfy the sovereignty gate. Codex and Claude remain external engineering collaborators while the production runtime is migrated behind this contract.

Initial model candidates are replaceable profiles rather than identities: a compact code-specialised model such as Qwen2.5-Coder 7B GGUF, and an NVIDIA Nemotron profile only after its local format, licence and measured RTX 3060 performance are verified. Model quality, latency, VRAM use, tool-call correctness and recovery success must be benchmarked before a profile becomes a default.

The first verified hardware target is an HP Pavilion x360 with an Intel i5-8265U, 8 GB RAM and Intel UHD 620; Windows exposes no NVIDIA adapter on this machine. The bootstrap profile is therefore the official Qwen2.5-Coder 1.5B Instruct Q4_K_M GGUF running through the official llama.cpp CPU build. It is a sovereignty and bounded-task profile, not a claim of frontier-model equivalence. The first direct completion loaded the model in about 7.3 seconds, used about 1.73 GB working memory and produced 156 completion tokens in about 17 seconds.

## Showroom before production

IntentOS must make the desire visible before committing to full production. The Showroom is an interactive, shareable proposal used to validate experience, navigation and the central flow with the NCTO or client. Every simulated behavior must be labelled honestly. Approval of the Showroom creates the production commitment; it is not itself proof that persistence, integrations, security or operations are complete.

The product therefore distinguishes three times:

1. **T1 — visible desire:** interactive showroom with a reviewable URL or local embedded preview.
2. **T2 — functional product:** real central flow and persistence.
3. **T3 — verified delivery:** QA, Reality Check, ownership artifacts, apply and receipt.

"Built" never means hidden in an isolated workspace. A completed outcome must be visible, verifiable, recoverable and deliverable.

## Chassis decision ADR-001

**Decision: KEEP SvelteKit + Tauri for this stage. Do not migrate to Theia or Electron.**

Evidence from this repository:

- Production build succeeds on the current SvelteKit/Tauri application.
- `svelte-check` reports zero errors and zero warnings.
- The Rust core passes 314 tests with environment-dependent integration tests ignored by default.
- Tauri already provides the security-sensitive native boundary, persistent state, filesystem isolation, child-process execution, channels and backup-first apply path.
- The current user journey already reaches Mission -> isolated runtime -> QA/Reality gates -> reviewed apply. A chassis migration would replace working system boundaries before a demonstrated limitation exists.

Theia is a serious **EXTRACT/WATCH** candidate. Its official architecture provides a modular desktop/browser IDE platform, frontend/backend separation, dependency-injection extension points, VS Code extension compatibility and an AI framework. Those capabilities matter when IntentOS needs industrial editor, terminal, LSP, debugging or extension-host functionality. They do not yet justify replacing the working product shell, because Theia's Node/Electron backend and UI composition would impose a second platform model and a migration of the existing Rust/Tauri security boundary.

**Reopen this decision only with a benchmark** against a concrete missing capability. Required evidence: integration spike, cold start/RAM, packaged size, terminal/PTY behavior, LSP/editor integration, extension isolation, Windows reliability, security boundary, migration cost and preservation of the Esmeralda experience.

## Reality boundary

Passing unit tests, type checks and production builds proves structural correctness. It does not prove native visual behavior or that external CLIs have quota, credentials and reliable cancellation on every OS. A full release gate still requires a native end-to-end run that creates a small product, passes QA and Reality Check, reviews the resulting delta, applies it and opens the delivery receipt.

## Current state and next increments

Completed in this foundation:

- IntentOS-owned Fabric registry with replaceable bindings.
- Mission-linked autonomous runtime preserved.
- Evidence-derived delivery receipt added to backend and production UI.
- Existing Agency Agents corpus retained as a capability source.
- Chassis decision recorded from repository evidence.
- Temporal approval control plane connected to Runbooks: a mission is started, signalled and queried before production begins.
- Real Temporal-server smoke covers approval, rejection, cancellation and querying a completed workflow for recovery.
- `LocalModelGateway` foundation reports native server, model, loopback API and NVIDIA readiness without exposing Codex/Claude as user-facing engines.

Temporal's current boundary is deliberate: it owns durable mission decisions and recovery history. IntentOS Runtime still owns construction stages, QA/Reality gates, workspace isolation and delivery evidence. Moving those stages into Temporal Activities remains a future increment and must preserve the existing runtime contracts; the current integration must not be described as fully Temporal-orchestrated execution.

Next increments, in order:

1. Route one bounded planning/coding stage through the verified local completion command with external fallback and persisted evidence; then expand by benchmark, not by branding.
2. Benchmark an NVIDIA/CUDA profile only on hardware where an NVIDIA adapter and driver are actually detected.
3. Version engineering decisions and supersession in `EngineeringMemory`.
4. Promote tools to schema-described, policy-checked capabilities (MCP-compatible where useful).
5. Add process-tree containment on Windows and selectable local/container/remote execution adapters.
6. Run a native golden-path factory mission and retain screenshots, logs, manifests and receipt as release evidence.
7. Benchmark Theia components only when editor/LSP/terminal scope enters an approved mission.
