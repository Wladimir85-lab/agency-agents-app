//! Sovereign local agentic loop, wired into the real `direction` stage of
//! `runtime_start`'s dispatch (see `stage_uses_local_executor` and its call
//! site in `runtime.rs`) — not a second pipeline, and not a third
//! `provider_id`. `runtime_start` still owns stage state, evidence
//! manifests, gate detection and Mission linkage; this module only supplies
//! what runs *inside* one `direction` attempt.
//!
//! IntentOS verified empirically (see the investigation this module comes
//! from) that neither OpenAI-style `tools`/`tool_choice` nor llama-server's
//! own experimental `--tools`/`--agent` server-side execution produce
//! reliable structured tool calls with the current Qwen2.5-Coder-1.5B
//! build: the model reasons about the right action but the response lands
//! as free-text JSON in `message.content`, never a parsed `tool_calls`
//! field, and the server-side executor never engaged either. So this loop
//! does not depend on either mechanism. It treats the local model purely as
//! a text-completion engine (`local_model::complete_raw`, the same call
//! `local_model_complete` already makes) and does the model -> action ->
//! tool -> observation -> model cycle itself, in Rust, with IntentOS as the
//! only trust boundary for what gets read or written.
//!
//! Evidence/manifest ownership: the *caller* (`runtime_start`'s stage loop)
//! writes the before/after workspace manifests, exactly as it already does
//! for `run_codex_stage`/`run_claude_stage` — this module does not call
//! `persist_manifest` itself, to avoid writing the same manifest twice. It
//! does write its own `<stage>-<attempt>.stdout.log`, mirroring exactly
//! where `run_codex_stage` writes its own stdout log (evidence-log writing
//! lives with the executor; manifest writing lives with the loop, for all
//! three executors alike).

use std::{
    collections::{HashMap, HashSet},
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use serde::Deserialize;
use tauri::ipc::Channel;
use tokio::sync::RwLock;

use crate::{
    commands::settings::SettingsLoadState,
    error::AppError,
    local_model,
    mission::{self, Mission},
    runtime::{evidence_file_component, run_evidence_dir, RunEvent},
    util::fs::{atomic_write, read_capped},
};

/// Hard ceiling on model <-> tool round trips for one stage attempt. Sized
/// for a single small planning artifact, not a real development stage.
const MAX_ITERATIONS: usize = 6;
/// Hard wall-clock ceiling for the whole loop. Far below the 2h ceiling the
/// external-provider stages use — that ceiling exists for a process we
/// don't control; here IntentOS drives every round trip itself, so a much
/// tighter budget is both possible and appropriate.
const MAX_STAGE_SECONDS: u64 = 240;
/// Per-turn output budget. Qwen2.5-Coder-1.5B runs at roughly 13 tok/s on
/// CPU on this class of machine (measured); keeping turns short keeps the
/// whole loop's wall-clock cost predictable.
const MAX_TURN_TOKENS: u32 = 420;
/// How many consecutive turns the Stage Contract can verify as already
/// satisfied (via `render_contract_satisfied_write_observation`'s guard)
/// before IntentOS concludes the stage on its own verification, without
/// waiting for the model to say `INTENTOS_GATE:PASS`. The contract check
/// (`validate_stage_contract`) is already the sole authority for whether
/// a model-claimed PASS is *accepted* — this only extends that same
/// authority to also *conclude* the stage when the model never claims it
/// at all. Set to 2, not 1, so the model gets a second real chance to
/// self-correct before IntentOS steps in. Evidence for why this exists:
/// two separate live vertical runs (Qwen2.5-Coder-1.5B loopback and
/// qwen2.5-coder:3b via Ollama) both reached a contract-satisfied state
/// and then exhausted every remaining turn up to MAX_ITERATIONS without
/// once emitting a standalone PASS — see agentLog.md 2026-09-02/03.
const CONTRACT_SATISFIED_AUTO_CONCLUDE_THRESHOLD: usize = 2;
/// Cap on a `read_file` result, mirroring the read caps used elsewhere in
/// the codebase (e.g. `MAX_RUN_FILE_BYTES` in runtime.rs) sized down for a
/// small-context local model rather than a 2MB run-state file.
const MAX_READ_BYTES: u64 = 64 * 1024;
/// How much of the single carried-forward observation is shown to the
/// model. The full text is always kept in the persisted transcript; only
/// what re-enters the model's own context is capped.
const MAX_ECHO_CHARS: usize = 400;
/// NOT a ceiling on the whole rendered working context — despite the name
/// this constant almost had. Precisely what it does: in
/// `render_working_context`, `reserved = fixed_header.len() +
/// contract_status_section.len() + observation_section.len()` is computed
/// first (those three are never truncated, by design), and only the
/// *remainder*, `reads_budget = MAX_OBSERVED_READS_BUDGET_CHARS -
/// reserved`, is what this constant actually bounds — the room left for
/// `observed_reads` after the non-negotiable parts are already accounted
/// for. `fixed_header` (task instructions + the full composed intent,
/// which for a real Mission-backed run can itself run several KB), the
/// contract-status listing, the observation, and the protocol-control
/// trailer are all appended afterward with no ceiling applied to them at
/// all. This is why a real run's total (~4771 bytes measured for
/// `direction`, which has zero `observed_reads`) exceeds this constant's
/// value — there was nothing here to bound it against. `observed_reads` is
/// not limited to a stage's `required_reads` (any successful `read_file`
/// is kept), so sizing this deliberately, not as a per-source count, still
/// matters — it just isn't the "global" authority the old name implied.
///
/// The 4000 value itself is unchanged and still reasoned from the real
/// failure this module's context-management rework replaced: llama-server
/// reported `"n_prompt_tokens":2123` against `"n_ctx":2048` at roughly 3.9
/// bytes per token measured on this engine.
const MAX_OBSERVED_READS_BUDGET_CHARS: usize = 4000;
/// Per-entry target size for one `observed_reads` entry before the global
/// budget is even considered. Still subject to further reduction by
/// `render_observed_reads_section` when the global budget is tight.
const MAX_OBSERVED_READ_CHARS: usize = 800;

fn truncate_for_context(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars).collect();
    out.push_str(" […]");
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LocalAction {
    ReadFile { path: String },
    WriteFile { path: String, content: String },
}

impl LocalAction {
    fn tool_name(&self) -> &'static str {
        match self {
            LocalAction::ReadFile { .. } => "read_file",
            LocalAction::WriteFile { .. } => "write_file",
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawAction {
    tool: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    content: Option<String>,
}

/// Extracts the *first* `INTENTOS_ACTION:` block from a model turn and
/// parses it into a [`LocalAction`], restricted to the allowlisted tool
/// names. First, not last: `resolve_turn_directive` is the authority on
/// which protocol marker governs a turn, and it always resolves to
/// whichever marker appears earliest in the text — a later, second action
/// (real or hallucinated) never gets a chance to override the first one.
/// Tolerant of surrounding prose and of markdown code fences (finds the
/// first balanced `{...}` after the marker rather than requiring the whole
/// remainder of the string to be valid JSON) — matching the codebase's
/// existing precedent for lenient sentinel-block parsing
/// (`parse_criteria_result` for `INTENTOS_CRITERIA:`). Also tolerates the
/// doubled-brace malformation (`{{...}}`) empirically observed from this
/// model during the investigation this module implements, by retrying with
/// one layer of braces stripped when the first parse fails.
fn parse_local_action(text: &str) -> Option<LocalAction> {
    const MARKER: &str = "INTENTOS_ACTION:";
    let idx = text.find(MARKER)?;
    let after = &text[idx + MARKER.len()..];
    let json_slice = extract_balanced_braces(after)?;
    let raw = serde_json::from_str::<RawAction>(json_slice)
        .ok()
        .or_else(|| {
            let trimmed = json_slice.trim();
            if trimmed.starts_with("{{") && trimmed.ends_with("}}") && trimmed.len() >= 4 {
                serde_json::from_str::<RawAction>(&trimmed[1..trimmed.len() - 1]).ok()
            } else {
                None
            }
        })?;
    // A structurally-valid parse with an empty `path` means the real
    // fields were nested somewhere our flat schema doesn't look (e.g. the
    // doubled-brace malformation's fields living under an "arguments" key
    // one level deeper) — not a usable action. Treat it the same as "no
    // action recognized" rather than handing an empty path down to the
    // tool layer, which would just bounce with a more confusing error.
    if raw.path.trim().is_empty() {
        return None;
    }
    match raw.tool.as_str() {
        "write_file" => Some(LocalAction::WriteFile {
            path: raw.path,
            content: raw.content.unwrap_or_default(),
        }),
        "read_file" => Some(LocalAction::ReadFile { path: raw.path }),
        _ => None,
    }
}

/// Finds the first `{`, then returns the slice up to its matching `}`
/// (brace-depth counted, so nested objects in `content` don't confuse it).
fn extract_balanced_braces(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let mut depth = 0i32;
    for (i, ch) in text[start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[start..start + i + 1]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Position of whichever of `INTENTOS_GATE:PASS`/`INTENTOS_GATE:FAIL`
/// appears first in `text`, and which one it is. `None` if neither is
/// present.
fn earliest_gate_claim(text: &str) -> Option<(usize, bool)> {
    let pass_idx = text.find("INTENTOS_GATE:PASS");
    let fail_idx = text.find("INTENTOS_GATE:FAIL");
    match (pass_idx, fail_idx) {
        (Some(p), Some(f)) => Some(if p <= f { (p, true) } else { (f, false) }),
        (Some(p), None) => Some((p, true)),
        (None, Some(f)) => Some((f, false)),
        (None, None) => None,
    }
}

/// The single authority over what one completion "means" as a protocol
/// turn: one completion = one turn = whichever of `INTENTOS_ACTION:` or
/// `INTENTOS_GATE:PASS`/`FAIL` appears *first* in the raw text. Everything
/// after that first marker — a second action, a fabricated observation, a
/// hallucinated future turn, a gate claim — has no operational authority,
/// even though the full raw text is still preserved verbatim in the
/// transcript/evidence for audit (callers never truncate `text` before
/// storing it; this function only decides what gets *executed*).
///
/// `InvalidAction` is modeled distinctly from `None` on purpose: a
/// malformed `INTENTOS_ACTION:` block that happens to be followed by a
/// well-formed `INTENTOS_GATE:PASS` later in the same text must not let
/// that later PASS take over — the first marker's failure to parse ends
/// the turn's authority right there, it is never treated as if the
/// action marker had never existed.
#[derive(Debug, PartialEq)]
enum TurnDirective {
    Action(LocalAction),
    /// The first marker was `INTENTOS_ACTION:` but its JSON did not parse
    /// into a recognized action. Deliberately not collapsed into `None`.
    InvalidAction,
    Gate(bool),
    /// Neither marker appears anywhere in the text.
    None,
}

fn resolve_turn_directive(text: &str) -> TurnDirective {
    let action_idx = text.find("INTENTOS_ACTION:");
    let gate_claim = earliest_gate_claim(text);

    let gate_wins = match (action_idx, gate_claim) {
        (Some(a), Some((g, _))) => g < a,
        (None, Some(_)) => true,
        _ => false,
    };

    if gate_wins {
        TurnDirective::Gate(gate_claim.expect("gate_wins implies gate_claim is Some").1)
    } else if action_idx.is_some() {
        match parse_local_action(text) {
            Some(action) => TurnDirective::Action(action),
            None => TurnDirective::InvalidAction,
        }
    } else {
        TurnDirective::None
    }
}

fn reject(reason: &str, requested: &str) -> AppError {
    AppError::InvalidArgument {
        message: format!("acción rechazada ({reason}): {requested}"),
    }
}

/// Resolves `requested` (as given by the model) to an absolute path
/// strictly inside `workspace`, or rejects it. This is the single
/// containment chokepoint every tool call goes through — no filesystem
/// mutation happens here, only resolution, so `read_file` and `write_file`
/// share one code path for "is this allowed" with no side effects of their
/// own to reason about.
///
/// Three checks, each independently sufficient to catch the traversal it
/// targets:
/// - absolute paths (including a bare drive prefix on Windows) are rejected
///   lexically, before touching the filesystem;
/// - any `..` component is rejected lexically, same reason;
/// - the deepest *existing* ancestor of the resolved candidate is
///   canonicalized (resolving any symlink in the chain) and required to
///   stay inside the canonicalized workspace — this is what catches a
///   symlink planted inside the workspace that points outside it, without
///   requiring the target file itself to exist yet (the common case for
///   `write_file` creating a new file).
fn resolve_contained_path(workspace: &Path, requested: &str) -> Result<PathBuf, AppError> {
    if requested.trim().is_empty() {
        return Err(reject("ruta vacía", requested));
    }
    let requested_path = Path::new(requested);
    for component in requested_path.components() {
        match component {
            Component::ParentDir => return Err(reject("traversal (..)", requested)),
            Component::RootDir | Component::Prefix(_) => {
                return Err(reject("ruta absoluta", requested))
            }
            _ => {}
        }
    }
    if requested_path.is_absolute() {
        return Err(reject("ruta absoluta", requested));
    }
    let workspace_canonical = workspace.canonicalize().map_err(|e| AppError::Io {
        message: format!("workspace inaccesible: {e}"),
    })?;
    let candidate = workspace_canonical.join(requested_path);

    let mut probe = candidate.clone();
    let existing_ancestor = loop {
        if probe.exists() {
            break probe;
        }
        match probe.parent() {
            Some(parent) => probe = parent.to_path_buf(),
            None => break workspace_canonical.clone(),
        }
    };
    let existing_ancestor_canonical =
        existing_ancestor.canonicalize().map_err(|e| AppError::Io {
            message: format!("no se pudo resolver la ruta destino: {e}"),
        })?;
    if !existing_ancestor_canonical.starts_with(&workspace_canonical) {
        return Err(reject("la ruta escapa del workspace (symlink)", requested));
    }
    Ok(candidate)
}

async fn apply_write_file(
    workspace: &Path,
    requested_path: &str,
    content: &str,
) -> Result<usize, AppError> {
    let resolved = resolve_contained_path(workspace, requested_path)?;
    if let Some(parent) = resolved.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| AppError::Io {
                message: format!("no se pudo crear el directorio destino: {e}"),
            })?;
    }
    atomic_write(&resolved, content.as_bytes()).await?;
    Ok(content.len())
}

async fn apply_read_file(workspace: &Path, requested_path: &str) -> Result<String, AppError> {
    let resolved = resolve_contained_path(workspace, requested_path)?;
    let bytes = read_capped(&resolved, MAX_READ_BYTES).await?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Declarative, verifiable definition of what a stage must actually have
/// done before IntentOS accepts `INTENTOS_GATE:PASS` as real. The model's
/// own PASS claim is a signal of intent, never the authority — this is.
///
/// `required_content_markers` checks only minimal observable contractual
/// structure (did the artifact name the categories it was asked to name),
/// not semantic quality or correctness of Architecture's actual design
/// judgment — that is deliberately out of scope here; richer verification
/// is a separate, later concern (and explicitly not the Reality Gate,
/// which judges the finished product, not a stage's contractual
/// compliance during execution).
struct StageContract {
    /// Paths that must have been successfully read (a real, successful
    /// `read_file` execution recorded in `executed_reads` — never inferred
    /// from what the model's text claims) before PASS is accepted.
    required_reads: &'static [&'static str],
    /// The stage's own artifact must exist and be non-empty on disk.
    require_artifact_nonempty: bool,
    /// Substrings that must literally appear in the artifact's real
    /// content on disk.
    required_content_markers: &'static [&'static str],
}

/// Per-stage-kind task definition: what artifact to produce, the
/// instructions for producing it, which tools are authorized, and the
/// contract that gates whether a PASS claim is actually accepted. This is
/// the *only* thing that varies between stages sharing the sovereign local
/// executor — the loop, parser, containment, evidence, and contract
/// *validation mechanism* below are identical for every kind.
/// `allowed_tools` is enforced in code (see the `match resolve_turn_directive`
/// arms in `run_local_agentic_stage`), not left to the prompt to
/// self-police — and neither is `contract`.
struct LocalStageTask {
    artifact_filename: &'static str,
    task_instructions: &'static str,
    allowed_tools: &'static [&'static str],
    contract: StageContract,
}

/// Adding a stage kind here is the entire integration surface for a new
/// sovereign-local stage — no loop changes, no new tools, no new evidence
/// path, no new validation code. `None` for a kind not (yet) covered
/// locally.
fn local_stage_task(stage_kind: &str) -> Option<LocalStageTask> {
    match stage_kind {
        "direction" => Some(LocalStageTask {
            artifact_filename: "direction-plan.md",
            task_instructions: "Debes crear un archivo llamado direction-plan.md dentro del workspace, con UNA sola frase breve (máximo 150 caracteres, sin viñetas, sin listas, sin saltos de línea) que resuma el alcance del pedido del usuario. Sé específico a ese pedido, no genérico. La brevedad es obligatoria: una respuesta larga no cabe en el formato y se descarta.",
            allowed_tools: &["write_file", "read_file"],
            contract: StageContract {
                required_reads: &[],
                require_artifact_nonempty: true,
                required_content_markers: &[],
            },
        }),
        // Mirrors the real catalog persona's actual contract (verified
        // against engineering-software-architect.md and a real past
        // Codex/Claude architecture-stage run): Architecture reads
        // Direction's output and produces a decision artifact Development
        // can act on — components, responsibilities, technical decisions,
        // and how the pieces relate. Kept to one line (no embedded
        // newlines, so it stays valid inside the JSON content field) and a
        // firm length cap: this is a real architecture contract, not
        // ceremonial documentation, sized for what a 1.5B model can
        // reliably close inside one write_file call.
        "architecture" => Some(LocalStageTask {
            artifact_filename: "architecture-plan.md",
            task_instructions: "Esta etapa tiene 3 pasos, uno por turno, en este orden exacto:\n\
                PASO 1 (ahora): responde con INTENTOS_ACTION usando \"tool\":\"read_file\" para leer 'direction-plan.md'. No hagas nada más en este turno.\n\
                PASO 2 (en tu siguiente turno, después de ver el contenido leído): responde con INTENTOS_ACTION usando \"tool\":\"write_file\" para crear 'architecture-plan.md'. El contenido debe ser UNA sola línea (sin saltos de línea), máximo 500 caracteres, con estas 4 partes separadas por ' | ': COMPONENTES: los módulos o piezas principales | RESPONSABILIDADES: qué hace cada componente | DECISIONES: stack/tecnología elegida y por qué | RELACIONES: cómo se conectan los componentes entre sí. Básalo en lo que leíste en el paso 1 — no inventes alcance nuevo. No hagas nada más en este turno.\n\
                PASO 3 (en tu siguiente turno, después de ver la confirmación de escritura): responde ÚNICAMENTE con INTENTOS_GATE:PASS, sin repetir el paso 2.",
            allowed_tools: &["write_file", "read_file"],
            contract: StageContract {
                required_reads: &["direction-plan.md"],
                require_artifact_nonempty: true,
                required_content_markers: &[
                    "COMPONENTES",
                    "RESPONSABILIDADES",
                    "DECISIONES",
                    "RELACIONES",
                ],
            },
        }),
        // Materializes what Architecture decided. Real Development stages
        // in general need shell (package managers, build tools — verified
        // against a real past Codex/Claude development-stage manifest,
        // which included a full npm/Next.js/Prisma toolchain run). This
        // vertical proof deliberately targets a case where the intent
        // itself rules that out ("no debe requerir servidor, base de datos
        // ni dependencias externas"): the entire deliverable is one
        // self-contained file, so read_file + write_file are sufficient
        // and shell stays disabled. `required_content_markers` here check
        // only minimal observable structure (a real HTML document, with
        // embedded JavaScript, with a periodic-update mechanism) — not
        // that the clock actually renders correctly. That deeper
        // verification belongs to QA/Reality, not this contract.
        "development" => Some(LocalStageTask {
            artifact_filename: "index.html",
            task_instructions: "Esta etapa tiene 4 pasos, uno por turno, en este orden exacto:\n\
                PASO 1 (ahora): responde con INTENTOS_ACTION usando \"tool\":\"read_file\" para leer 'direction-plan.md'. No hagas nada más en este turno.\n\
                PASO 2 (en tu siguiente turno, después de ver el contenido leído): responde con INTENTOS_ACTION usando \"tool\":\"read_file\" para leer 'architecture-plan.md'. No hagas nada más en este turno.\n\
                PASO 3 (en tu siguiente turno, después de ver el contenido leído): responde con INTENTOS_ACTION usando \"tool\":\"write_file\" para crear 'index.html'. El contenido debe ser UN SOLO archivo HTML autocontenido, en UNA sola línea (sin saltos de línea), con <style> y <script> embebidos, que muestre la fecha y hora actual y se actualice cada segundo usando setInterval. Sin servidor, sin base de datos, sin dependencias externas. Básalo en lo que leíste en los pasos 1 y 2. No hagas nada más en este turno.\n\
                PASO 4 (en tu siguiente turno, después de ver la confirmación de escritura): responde ÚNICAMENTE con INTENTOS_GATE:PASS, sin repetir el paso 3.",
            allowed_tools: &["write_file", "read_file"],
            contract: StageContract {
                required_reads: &["direction-plan.md", "architecture-plan.md"],
                require_artifact_nonempty: true,
                required_content_markers: &["<html", "<script", "setInterval"],
            },
        }),
        _ => None,
    }
}

/// Deterministic, code-side check of whether `task.contract` is actually
/// satisfied — reads the real workspace and the real record of executed
/// tool calls, never the model's own claim. One function, reused for every
/// stage kind; what gets checked is entirely data (`StageContract`), never
/// a per-stage branch here.
async fn validate_stage_contract(
    task: &LocalStageTask,
    workspace: &Path,
    executed_reads: &HashSet<String>,
) -> Vec<String> {
    let mut missing = Vec::new();
    for required in task.contract.required_reads {
        if !executed_reads.contains(*required) {
            missing.push(format!(
                "falta leer '{required}' con la herramienta read_file"
            ));
        }
    }
    let content = tokio::fs::read_to_string(workspace.join(task.artifact_filename))
        .await
        .unwrap_or_default();
    if task.contract.require_artifact_nonempty && content.trim().is_empty() {
        missing.push(format!(
            "'{}' no existe o está vacío",
            task.artifact_filename
        ));
    }
    for marker in task.contract.required_content_markers {
        if !content.contains(marker) {
            missing.push(format!(
                "'{}' no contiene la categoría '{marker}'",
                task.artifact_filename
            ));
        }
    }
    missing
}

/// Additive only, mirroring `stage_prompt`'s `brief_section` precedent
/// (`runtime.rs`): when no Mission is attached, `brief_section` is empty
/// and the prompt is unchanged from before Mission support existed here.
fn local_stage_prompt(task: &LocalStageTask, intent: &str, mission: Option<&Mission>) -> String {
    let brief_section = match mission {
        Some(m) => format!("MISSION BRIEF:\n{}\n\n", mission::mission_brief(m)),
        None => String::new(),
    };
    let artifact = task.artifact_filename;
    format!(
        "{instructions}\n\n\
        {brief_section}PEDIDO DEL USUARIO:\n{intent}\n\n\
        Para escribir el archivo, responde EXACTAMENTE con esta línea, reemplazando <contenido aquí> por el contenido real (nada de saltos de línea dentro del texto, sin explicación antes o después):\n\
        INTENTOS_ACTION: {{\"tool\":\"write_file\",\"path\":\"{artifact}\",\"content\":\"<contenido aquí>\"}}\n\n\
        Para leer un archivo existente: INTENTOS_ACTION: {{\"tool\":\"read_file\",\"path\":\"<ruta>\"}}\n\n\
        Cuando recibas una OBSERVACIÓN confirmando que el archivo se escribió, responde en tu siguiente turno únicamente con esta línea, sin nada más:\n\
        INTENTOS_GATE:PASS",
        instructions = task.task_instructions
    )
}

/// Records a successful `read_file`'s content, keyed by path — re-reading
/// the same path refreshes it rather than duplicating an entry. Not
/// limited to `required_reads`: any file the model actually reads is kept,
/// which is exactly why the global budget in `render_working_context` (not
/// a per-required-read count) is the real authority against overflow.
fn record_observed_read(reads: &mut Vec<(String, String)>, path: String, content: String) {
    if let Some(entry) = reads.iter_mut().find(|(existing, _)| *existing == path) {
        entry.1 = content;
    } else {
        reads.push((path, content));
    }
}

/// True when `content` is byte-identical to the last content this loop
/// actually wrote to `path` — the redundant-write guard's sole decision
/// point. Keyed per path (not "the single last write overall"), so
/// writing A, then B, then A again with the same content as before is
/// still caught, while A then a *different* B is never a false positive.
/// Deliberately a plain string comparison, not a hash/signature: turn
/// content is already bounded by MAX_TURN_TOKENS, so there is no real
/// cost to keeping the literal text, and a direct comparison is strictly
/// easier to reason about and test than an added hashing layer.
fn is_redundant_write(last_write_by_path: &HashMap<String, String>, path: &str, content: &str) -> bool {
    last_write_by_path.get(path).map(String::as_str) == Some(content)
}

/// Deterministic, code-generated observation for a caught redundant
/// write — never a re-execution of the write, never an auto-PASS. It
/// only states the fact (nothing changed) and leaves the *next* action
/// to be governed by the real Stage Contract status, which
/// `render_working_context`'s protocol-control section already renders
/// fresh every turn from the actual workspace/executed-reads state, not
/// from this observation.
fn render_redundant_write_observation(path: &str) -> String {
    format!(
        "OBSERVACIÓN: '{path}' ya tiene exactamente este contenido — esta escritura no cambió nada en el workspace. Repetir la misma escritura no hace avanzar la etapa; revisa el estado del contrato más abajo."
    )
}

/// Deterministic observation for a write attempted while the Stage
/// Contract is *already* satisfied — broader than
/// `render_redundant_write_observation`: it does not matter whether this
/// particular write's content differs from the last one, because the
/// contract's own verdict (computed the same way `INTENTOS_GATE:PASS` is
/// checked) already does not depend on it. Same non-negotiables as the
/// redundant-write guard: never a re-execution, never an auto-PASS —
/// this only states the fact and points at the contract status already
/// rendered below.
fn render_contract_satisfied_write_observation(path: &str) -> String {
    format!(
        "OBSERVACIÓN: el contrato de esta etapa ya está cumplido — escribir de nuevo en '{path}' (con el mismo contenido o con una redacción distinta) no es necesario y no cambia el resultado. Revisa el estado del contrato más abajo."
    )
}

/// Renders the `observed_reads` section within `budget` characters —
/// budget is the *reducible* part of the working context (see
/// `render_working_context`): instructions, contract status, and the last
/// observation are never touched here. Each entry is capped at
/// `MAX_OBSERVED_READ_CHARS` first; if the sum still doesn't fit, later
/// entries are truncated further or, if there is no meaningful room left,
/// replaced with an explicit "omitted" marker naming the file — the model
/// is never silently handed a cut string it can't tell is incomplete, and
/// it can always re-read a file it needs (the tool stays available).
fn render_observed_reads_section(reads: &[(String, String)], mut budget: usize) -> String {
    if reads.is_empty() {
        return String::new();
    }
    let mut out = String::from("\n\nARCHIVOS YA LEÍDOS (contenido real, disponible sin volver a leerlos):");
    for (path, content) in reads {
        let capped = truncate_for_context(content, MAX_OBSERVED_READ_CHARS);
        let cost = capped.chars().count();
        if cost <= budget {
            out.push_str(&format!("\n--- {path} ---\n{capped}"));
            budget -= cost;
        } else if budget > 30 {
            let partial = truncate_for_context(&capped, budget.saturating_sub(15));
            out.push_str(&format!("\n--- {path} (truncado por presupuesto de contexto) ---\n{partial}"));
            budget = 0;
        } else {
            out.push_str(&format!(
                "\n--- {path} ---\n[omitido por presupuesto de contexto — ya fue leído; usa read_file de nuevo si necesitas verlo]"
            ));
        }
    }
    out
}

/// Deterministic protocol-control instruction — separate from
/// `last_observation` on purpose. `last_observation` reports *reality*
/// (what actually happened); this reports *control* (what to do about
/// it), generated entirely by IntentOS from the Stage Contract's current
/// state, never from the model's own text. Recomputed fresh every call
/// from `contract_status`, so it always reflects the current state and
/// never accumulates history — the previous turn's instruction is not
/// carried forward, it is simply not asked for again.
fn render_protocol_control(contract_status: &[String]) -> String {
    if contract_status.is_empty() {
        "\n\nCONTRATO CUMPLIDO. No ejecutes más herramientas. Responde únicamente con INTENTOS_GATE:PASS.".to_string()
    } else {
        "\n\nCONTRATO PENDIENTE. Ejecuta una sola acción válida que avance los requisitos pendientes. No declares PASS todavía.".to_string()
    }
}

/// Builds the compact input actually sent to the model this turn — the
/// "working context" role, reconstructed fresh every turn from real state,
/// as opposed to `transcript` (the complete, ever-growing audit log
/// written to evidence). Nothing here accumulates turn over turn: a
/// 20-turn stage produces a working context roughly the same size as a
/// 2-turn one, because this function is called anew each time rather than
/// appending to a growing string.
///
/// Priority, deterministic, never a blind truncation of the final string:
/// 1. `fixed_header` (instructions/mission/intent) — never truncated.
/// 2. `contract_status` — never truncated.
/// 3. `last_observation` — high priority, capped at the existing
///    `MAX_ECHO_CHARS`, computed before the reducible budget.
/// 4. `observed_reads` — gets whatever budget remains; the only reducible
///    part (see `render_observed_reads_section`).
///
/// Always ends with `render_protocol_control` — the deterministic
/// PASS/keep-going directive is the last thing the model sees.
fn render_working_context(
    fixed_header: &str,
    contract_status: &[String],
    observed_reads: &[(String, String)],
    last_observation: Option<&str>,
) -> String {
    let mut out = String::with_capacity(fixed_header.len() + 512);
    out.push_str(fixed_header);

    if contract_status.is_empty() {
        out.push_str("\n\nESTADO DEL CONTRATO: cumplido.");
    } else {
        out.push_str(&format!(
            "\n\nESTADO DEL CONTRATO — pendiente:\n- {}",
            contract_status.join("\n- ")
        ));
    }

    let observation_section = last_observation
        .map(|obs| {
            format!(
                "\n\nOBSERVACIÓN (resultado real de tu turno anterior):\n{}",
                truncate_for_context(obs, MAX_ECHO_CHARS)
            )
        })
        .unwrap_or_default();

    let reserved = out.len() + observation_section.len();
    let reads_budget = MAX_OBSERVED_READS_BUDGET_CHARS.saturating_sub(reserved);
    out.push_str(&render_observed_reads_section(observed_reads, reads_budget));
    out.push_str(&observation_section);
    out.push_str(&render_protocol_control(contract_status));
    out
}

/// Result of one full local-agent stage attempt, mirroring what
/// `run_codex_stage`/`run_claude_stage` report to their caller today —
/// `passed` is the same `Result<bool, _>` contract, `combined` is the same
/// "full transcript" evidence-writers already persist, so a future
/// integration into the real dispatch loop is a thin wrapper, not a
/// redesign.
#[derive(Debug, Clone)]
pub struct LocalAgentResult {
    pub passed: bool,
    pub combined: String,
    pub iterations: usize,
    pub wrote_file: bool,
}

fn emit(channel: &Channel<RunEvent>, run_id: &str, stage_id: &str, stream: &str, text: String) {
    let _ = channel.send(RunEvent::Output {
        run_id: run_id.to_string(),
        stage_id: stage_id.to_string(),
        stream: stream.to_string(),
        text,
    });
}

/// Runs the local sovereign agent through one stage attempt against
/// `workspace`, streaming each model turn and tool observation to `channel`
/// exactly like `run_codex_stage` streams its stdout/system lines (model
/// text as `"stdout"`, IntentOS's own notes as `"system"`), and applying
/// `resolve_turn_directive` + `validate_stage_contract` to decide pass/fail.
///
/// `stage_kind` selects the [`LocalStageTask`] (artifact, instructions,
/// authorized tools) — it is the only thing that varies; the loop below is
/// identical for every kind `local_stage_task` recognizes. An unrecognized
/// `stage_kind` is a caller bug (the dispatch site in `runtime.rs` only
/// routes here for kinds `local_stage_task` covers), so it fails closed
/// with `AppError::Internal` rather than guessing a default task.
///
/// Does not write manifests itself — see the module doc comment. The
/// caller (`runtime_start`'s stage loop) is responsible for the
/// before/after workspace manifest, same as for every other executor.
#[allow(clippy::too_many_arguments)]
pub async fn run_local_agentic_stage(
    app_data_dir: &Path,
    workspace: &Path,
    intent: &str,
    run_id: &str,
    stage_id: &str,
    stage_kind: &str,
    attempt: u8,
    mission: Option<&Mission>,
    channel: &Channel<RunEvent>,
    settings: &Arc<RwLock<SettingsLoadState>>,
) -> Result<LocalAgentResult, AppError> {
    let task = local_stage_task(stage_kind).ok_or_else(|| AppError::Internal {
        message: format!("local_agent has no task defined for stage kind '{stage_kind}'"),
    })?;
    let deadline = Instant::now() + Duration::from_secs(MAX_STAGE_SECONDS);
    // Fixed once, per stage — never mutated. Everything else the model
    // sees is reconstructed fresh each turn by render_working_context.
    let fixed_header = local_stage_prompt(&task, intent, mission);
    let mut transcript = String::new();
    let mut wrote_file = false;
    // Evidence of tool calls the loop actually executed successfully —
    // `validate_stage_contract`'s only source of truth for `required_reads`.
    // Populated exclusively where a `read_file` action returns `Ok`, never
    // from anything the model's own text claims.
    let mut executed_reads: HashSet<String> = HashSet::new();
    // Real content of every successful read so far, keyed by path — the
    // working-context counterpart to `executed_reads`. Not accumulated
    // conversation history: this is IntentOS's own record, re-rendered
    // (and budget-reduced if needed) fresh each turn, never re-echoed from
    // the model's own raw text.
    let mut observed_reads: Vec<(String, String)> = Vec::new();
    // Exactly one — the real consequence of the immediately preceding
    // turn's action, sourced from IntentOS's own execution, never from
    // re-parsing the model's (possibly hallucinated) raw completion.
    // Replaced, never appended, preserving causality without accumulation.
    let mut last_observation: Option<String> = None;
    // Real content of the last successful write to each path — the
    // redundant-write guard's own record, keyed the same way
    // observed_reads/executed_reads are: only ever updated from a real,
    // successful apply_write_file, never from the model's own claims.
    let mut last_write_by_path: HashMap<String, String> = HashMap::new();
    // Counts consecutive turns where the contract was already satisfied
    // and the model attempted a write or a redundant read anyway instead
    // of PASS — see CONTRACT_SATISFIED_AUTO_CONCLUDE_THRESHOLD. Monotonic
    // within a run: once the contract is satisfied it cannot become
    // unsatisfied again (every write while satisfied is blocked before
    // touching disk by the very guard that increments this), so no reset
    // case exists.
    let mut consecutive_satisfied_actions_without_pass: usize = 0;
    let mut iterations = 0usize;
    let passed: bool;

    loop {
        iterations += 1;
        if iterations > MAX_ITERATIONS {
            let note = "[local-agent] límite de iteraciones alcanzado sin INTENTOS_GATE.";
            transcript.push_str(&format!("\n{note}\n"));
            emit(channel, run_id, stage_id, "system", note.into());
            passed = false;
            break;
        }
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            let note = "[local-agent] límite de tiempo alcanzado sin INTENTOS_GATE.";
            transcript.push_str(&format!("\n{note}\n"));
            emit(channel, run_id, stage_id, "system", note.into());
            passed = false;
            break;
        };

        // Fresh, bounded working context every turn — see
        // render_working_context. `missing` is computed once here and
        // reused below for the PASS-claim check in this same iteration:
        // nothing mutates state between building the prompt and getting
        // the response back, so it is still exactly correct there.
        let missing = validate_stage_contract(&task, workspace, &executed_reads).await;
        let prompt = render_working_context(
            &fixed_header,
            &missing,
            &observed_reads,
            last_observation.as_deref(),
        );

        // Observability only: tag whatever error complete_raw surfaces with
        // which turn it happened on and how large the working context was
        // by then — neither changes anything about a successful completion.
        let completion = match tokio::time::timeout(
            remaining,
            local_model::complete_raw(app_data_dir, &prompt, Some(MAX_TURN_TOKENS), settings),
        )
        .await
        {
            Ok(Ok(completion)) => completion,
            Ok(Err(e)) => {
                return Err(AppError::Internal {
                    message: format!(
                        "turno {iterations} (working context ~{} bytes): {e}",
                        prompt.len()
                    ),
                });
            }
            Err(_) => {
                return Err(AppError::Internal {
                    message: format!(
                        "turno {iterations}: el motor local no respondió dentro del presupuesto de tiempo"
                    ),
                });
            }
        };

        let text = completion.content;
        // Diagnostic annotation only, part of the audit trail — records
        // what was actually sent this turn so the fix's core claim
        // (context does not grow with turn count) is directly verifiable
        // from evidence, without changing what is sent to the model.
        transcript.push_str(&format!(
            "\n--- turno {iterations} (modelo) [contexto de entrada enviado: {} bytes] ---\n{text}\n",
            prompt.len()
        ));
        emit(channel, run_id, stage_id, "stdout", text.clone());

        // One completion = one turn = whichever protocol marker appears
        // first has authority. Everything after it in `text` (a second
        // action, a fabricated observation, a hallucinated PASS) was
        // already preserved verbatim above (transcript/emit) for audit,
        // but has no effect on execution — see `resolve_turn_directive`.
        match resolve_turn_directive(&text) {
            TurnDirective::Gate(claims_pass) => {
                if claims_pass {
                    // The model's INTENTOS_GATE:PASS is a claim of intent,
                    // never the authority — `missing` (computed above, for
                    // this same iteration's prompt) is what the real
                    // workspace and the real record of executed tool calls
                    // actually show.
                    if !missing.is_empty() {
                        let note = format!(
                            "OBSERVACIÓN: tu INTENTOS_GATE:PASS fue rechazado — el contrato de esta etapa no está cumplido:\n- {}\nCorrige lo anterior y vuelve a intentar terminar.",
                            missing.join("\n- ")
                        );
                        transcript.push_str(&format!("\n{note}\n"));
                        emit(channel, run_id, stage_id, "system", note.clone());
                        last_observation = Some(note);
                        continue;
                    }
                }
                passed = claims_pass;
                break;
            }
            TurnDirective::Action(action)
                if !task.allowed_tools.contains(&action.tool_name()) =>
            {
                // Enforced in code, not left to the prompt to self-police: a
                // recognized action for a tool this stage's LocalStageTask
                // does not authorize is treated the same as an unrecognized
                // action — rejected with an explanatory observation, never
                // executed.
                let note = format!(
                    "OBSERVACIÓN: la herramienta '{}' no está autorizada para esta etapa. Herramientas permitidas: {}.",
                    action.tool_name(),
                    task.allowed_tools.join(", ")
                );
                transcript.push_str(&format!("\n{note}\n"));
                emit(channel, run_id, stage_id, "system", note.clone());
                last_observation = Some(note);
            }
            TurnDirective::Action(LocalAction::WriteFile { path, .. }) if missing.is_empty() => {
                // Broader than is_redundant_write: `missing` (computed
                // fresh above, for this same turn) is the Stage Contract's
                // own verdict, not a text comparison. Once it is already
                // satisfied, no further write — identical or merely
                // reworded — can change that verdict, so there is nothing
                // left for a write to accomplish. This is what actually
                // catches the oscillating-near-duplicate pattern observed
                // live (turn 2/3/5 rewording the same sentence back and
                // forth), which byte-for-byte `is_redundant_write` alone
                // does not, since each variant's exact text differs from
                // the one immediately before it.
                let obs = render_contract_satisfied_write_observation(&path);
                transcript.push_str(&format!("\n{obs}\n"));
                emit(channel, run_id, stage_id, "system", obs.clone());
                last_observation = Some(obs);
                consecutive_satisfied_actions_without_pass += 1;
                if consecutive_satisfied_actions_without_pass
                    >= CONTRACT_SATISFIED_AUTO_CONCLUDE_THRESHOLD
                {
                    // IntentOS's own verification, not the model's claim,
                    // concludes the stage here — visible in the evidence,
                    // never silent. `missing` (computed fresh this same
                    // iteration, above) is genuinely empty; this is not a
                    // relaxation of the contract, only of the requirement
                    // that the model be the one to say so.
                    let note = format!(
                        "[local-agent] el contrato lleva {consecutive_satisfied_actions_without_pass} turnos consecutivos verificado como cumplido sin que el modelo respondiera INTENTOS_GATE:PASS. IntentOS concluye la etapa por su propia verificación del contrato."
                    );
                    transcript.push_str(&format!("\n{note}\n"));
                    emit(channel, run_id, stage_id, "system", note);
                    passed = true;
                    break;
                }
            }
            TurnDirective::Action(LocalAction::WriteFile { path, content })
                if is_redundant_write(&last_write_by_path, &path, &content) =>
            {
                // Caught before touching the filesystem: no re-execution,
                // no auto-PASS (missing/passed are untouched here — the
                // Gate branch is the only place that ever sets `passed`).
                // Just a real, deterministic observation, same shape as
                // every other branch, so the model's next turn is driven
                // by the actual (still-fresh-rendered) contract status.
                let obs = render_redundant_write_observation(&path);
                transcript.push_str(&format!("\n{obs}\n"));
                emit(channel, run_id, stage_id, "system", obs.clone());
                last_observation = Some(obs);
            }
            TurnDirective::Action(LocalAction::WriteFile { path, content }) => {
                match apply_write_file(workspace, &path, &content).await {
                    Ok(bytes) => {
                        wrote_file = true;
                        last_write_by_path.insert(path.clone(), content);
                        let obs = format!("OBSERVACIÓN: se escribió '{path}' ({bytes} bytes).");
                        transcript.push_str(&format!("\n{obs}\n"));
                        emit(channel, run_id, stage_id, "system", obs.clone());
                        last_observation = Some(obs);
                    }
                    Err(e) => {
                        let obs = format!("OBSERVACIÓN: la escritura falló: {e}");
                        transcript.push_str(&format!("\n{obs}\n"));
                        emit(channel, run_id, stage_id, "system", obs.clone());
                        last_observation = Some(obs);
                    }
                }
            }
            TurnDirective::Action(LocalAction::ReadFile { path })
                if executed_reads.contains(&path) =>
            {
                // Same guard philosophy as the write side, mirrored for
                // reads: nothing in this stage's tool set can change a
                // path's content except write_file, and this loop only
                // ever writes to the stage's own artifact — never to a
                // prior-stage artifact it reads (direction-plan.md,
                // architecture-plan.md). So a path already in
                // `executed_reads` is guaranteed to yield identical
                // content on a re-read; skip the redundant I/O and tell
                // the model plainly that it already has this, instead of
                // silently re-executing and burning a turn with no new
                // information — exactly the live-observed pattern where
                // Architecture re-read direction-plan.md six times in a
                // row and never reached its own write_file at all.
                // The generic "continúa con el paso pendiente" version of
                // this message (no longer used) was live-verified to not
                // be concrete enough: the model kept re-reading anyway
                // instead of pivoting to the next real action. `missing`
                // (computed fresh this same turn, above) already names
                // exactly what remains — reusing it here, inline in the
                // observation itself, instead of only in the "ESTADO DEL
                // CONTRATO" section further down the same prompt.
                let obs = if missing.is_empty() {
                    format!(
                        "OBSERVACIÓN: ya leíste '{path}' — su contenido sigue disponible abajo en ARCHIVOS YA LEÍDOS, releerlo no aporta nada nuevo. El contrato ya está cumplido; responde INTENTOS_GATE:PASS."
                    )
                } else {
                    format!(
                        "OBSERVACIÓN: ya leíste '{path}' — su contenido sigue disponible abajo en ARCHIVOS YA LEÍDOS, releerlo no aporta nada nuevo. Lo que falta de verdad es:\n- {}\nHaz eso ahora, no releas.",
                        missing.join("\n- ")
                    )
                };
                transcript.push_str(&format!("\n{obs}\n"));
                emit(channel, run_id, stage_id, "system", obs.clone());
                last_observation = Some(obs);
                if missing.is_empty() {
                    // Same auto-conclude circuit breaker as the write
                    // side, reached via a different action this time —
                    // the contract being satisfied is what matters, not
                    // which tool the model happened to retry.
                    consecutive_satisfied_actions_without_pass += 1;
                    if consecutive_satisfied_actions_without_pass
                        >= CONTRACT_SATISFIED_AUTO_CONCLUDE_THRESHOLD
                    {
                        let note = format!(
                            "[local-agent] el contrato lleva {consecutive_satisfied_actions_without_pass} turnos consecutivos verificado como cumplido sin que el modelo respondiera INTENTOS_GATE:PASS. IntentOS concluye la etapa por su propia verificación del contrato."
                        );
                        transcript.push_str(&format!("\n{note}\n"));
                        emit(channel, run_id, stage_id, "system", note);
                        passed = true;
                        break;
                    }
                }
            }
            TurnDirective::Action(LocalAction::ReadFile { path }) => {
                match apply_read_file(workspace, &path).await {
                    Ok(content) => {
                        // Evidence of a real, successful read — the only
                        // thing validate_stage_contract trusts for
                        // `required_reads`.
                        executed_reads.insert(path.clone());
                        // Full content goes to transcript/evidence (role A:
                        // complete audit) and into observed_reads (real
                        // working data, budget-managed on render). What
                        // becomes `last_observation` — the one thing fed
                        // back into the next prompt — is a short pointer,
                        // never the full content: a large file read must
                        // not itself blow up the working context turn over
                        // turn.
                        let full_obs = format!(
                            "OBSERVACIÓN: contenido de '{path}' ({} bytes):\n{content}",
                            content.len()
                        );
                        transcript.push_str(&format!("\n{full_obs}\n"));
                        emit(channel, run_id, stage_id, "system", full_obs);
                        record_observed_read(&mut observed_reads, path.clone(), content);
                        last_observation = Some(format!(
                            "se leyó '{path}' — contenido disponible abajo en ARCHIVOS YA LEÍDOS."
                        ));
                    }
                    Err(e) => {
                        let obs = format!("OBSERVACIÓN: la lectura falló: {e}");
                        transcript.push_str(&format!("\n{obs}\n"));
                        emit(channel, run_id, stage_id, "system", obs.clone());
                        last_observation = Some(obs);
                    }
                }
            }
            TurnDirective::InvalidAction | TurnDirective::None => {
                let note = "OBSERVACIÓN: no se reconoció ninguna INTENTOS_ACTION válida en este turno. Responde con una línea INTENTOS_ACTION: seguida de un objeto JSON {\"tool\":\"write_file\"|\"read_file\",\"path\":\"...\",\"content\":\"...\"}, o con INTENTOS_GATE:PASS/FAIL si ya terminaste.";
                transcript.push_str(&format!("\n{note}\n"));
                emit(channel, run_id, stage_id, "system", note.into());
                last_observation = Some(note.to_string());
            }
        }
    }

    let evidence_dir = run_evidence_dir(app_data_dir, run_id);
    tokio::fs::create_dir_all(&evidence_dir)
        .await
        .map_err(|e| AppError::Io {
            message: e.to_string(),
        })?;
    let safe_stage = evidence_file_component(stage_id);
    atomic_write(
        &evidence_dir.join(format!("{safe_stage}-{attempt}.stdout.log")),
        transcript.as_bytes(),
    )
    .await?;

    Ok(LocalAgentResult {
        passed,
        combined: transcript,
        iterations,
        wrote_file,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Paranoid mode off, loaded settings — every test in this module
    /// exercises the loopback path (the default backend), which never
    /// consults this beyond the presence of the argument itself.
    fn settings_arc() -> Arc<RwLock<SettingsLoadState>> {
        Arc::new(RwLock::new(SettingsLoadState::Loaded(
            crate::commands::settings::Settings::default(),
        )))
    }

    // ---------- resolve_contained_path ----------

    #[test]
    fn resolve_contained_path_accepts_nested_new_file_inside_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        let resolved = resolve_contained_path(tmp.path(), "docs/direction-plan.md").unwrap();
        assert!(resolved.starts_with(tmp.path().canonicalize().unwrap()));
        assert!(resolved.ends_with("docs/direction-plan.md"));
    }

    #[test]
    fn resolve_contained_path_rejects_parent_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        let err = resolve_contained_path(tmp.path(), "../escape.txt").unwrap_err();
        assert!(matches!(err, AppError::InvalidArgument { .. }));
    }

    #[test]
    fn resolve_contained_path_rejects_traversal_in_the_middle() {
        let tmp = tempfile::tempdir().unwrap();
        let err = resolve_contained_path(tmp.path(), "docs/../../escape.txt").unwrap_err();
        assert!(matches!(err, AppError::InvalidArgument { .. }));
    }

    #[test]
    fn resolve_contained_path_rejects_absolute_paths() {
        let tmp = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        let absolute = "C:\\Windows\\System32\\evil.txt";
        #[cfg(not(windows))]
        let absolute = "/etc/passwd";
        let err = resolve_contained_path(tmp.path(), absolute).unwrap_err();
        assert!(matches!(err, AppError::InvalidArgument { .. }));
    }

    #[test]
    fn resolve_contained_path_rejects_empty_path() {
        let tmp = tempfile::tempdir().unwrap();
        let err = resolve_contained_path(tmp.path(), "").unwrap_err();
        assert!(matches!(err, AppError::InvalidArgument { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn resolve_contained_path_rejects_symlink_escape() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let link = tmp.path().join("escape");
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();
        let err = resolve_contained_path(tmp.path(), "escape/evil.txt").unwrap_err();
        assert!(matches!(err, AppError::InvalidArgument { .. }));
    }

    // ---------- parse_local_action ----------

    #[test]
    fn parse_local_action_reads_write_file_with_surrounding_prose() {
        let text = "Claro, voy a crear el archivo.\nINTENTOS_ACTION: {\"tool\":\"write_file\",\"path\":\"plan.md\",\"content\":\"hola\"}\nListo.";
        let action = parse_local_action(text).unwrap();
        assert_eq!(
            action,
            LocalAction::WriteFile {
                path: "plan.md".into(),
                content: "hola".into()
            }
        );
    }

    #[test]
    fn parse_local_action_reads_read_file() {
        let text = "INTENTOS_ACTION: {\"tool\":\"read_file\",\"path\":\"plan.md\"}";
        let action = parse_local_action(text).unwrap();
        assert_eq!(
            action,
            LocalAction::ReadFile {
                path: "plan.md".into()
            }
        );
    }

    #[test]
    fn parse_local_action_tolerates_fenced_code_block() {
        let text = "```json\nINTENTOS_ACTION: {\"tool\":\"write_file\",\"path\":\"a.md\",\"content\":\"x\"}\n```";
        let action = parse_local_action(text).unwrap();
        assert_eq!(
            action,
            LocalAction::WriteFile {
                path: "a.md".into(),
                content: "x".into()
            }
        );
    }

    #[test]
    fn parse_local_action_tolerates_doubled_braces() {
        // Empirically observed malformation from this exact model/server
        // combination during the investigation this module implements.
        let text = "INTENTOS_ACTION: {{\"tool\": \"write_file\", \"arguments\": {\"path\": \"a.md\", \"content\": \"x\"}}}";
        // Note: this specific malformed shape nests "arguments" one level
        // deeper than our schema expects, so it correctly fails to parse as
        // a valid action — asserted explicitly so a future schema change
        // doesn't silently start accepting it without a matching test.
        assert!(parse_local_action(text).is_none());
    }

    #[test]
    fn parse_local_action_tolerates_doubled_braces_flat_schema() {
        let text = "INTENTOS_ACTION: {{\"tool\":\"write_file\",\"path\":\"a.md\",\"content\":\"x\"}}";
        let action = parse_local_action(text).unwrap();
        assert_eq!(
            action,
            LocalAction::WriteFile {
                path: "a.md".into(),
                content: "x".into()
            }
        );
    }

    #[test]
    fn parse_local_action_rejects_unknown_tool() {
        let text = "INTENTOS_ACTION: {\"tool\":\"exec_shell_command\",\"path\":\"x\"}";
        assert!(parse_local_action(text).is_none());
    }

    #[test]
    fn parse_local_action_returns_none_without_marker() {
        let text = "No voy a hacer nada en particular.";
        assert!(parse_local_action(text).is_none());
    }

    #[test]
    fn parse_local_action_uses_the_first_action_when_several_appear() {
        // Inverted from "last wins": resolve_turn_directive's causality
        // rule requires the first marker to govern the turn, and
        // parse_local_action is what it relies on to extract that first
        // action — a later, second action (real or hallucinated) must
        // never override it.
        let text = "INTENTOS_ACTION: {\"tool\":\"read_file\",\"path\":\"a.md\"}\n\
                    luego\n\
                    INTENTOS_ACTION: {\"tool\":\"write_file\",\"path\":\"b.md\",\"content\":\"y\"}";
        let action = parse_local_action(text).unwrap();
        assert_eq!(action, LocalAction::ReadFile { path: "a.md".into() });
    }

    // ---------- earliest_gate_claim / resolve_turn_directive / truncate_for_context ----------

    #[test]
    fn earliest_gate_claim_prefers_whichever_of_pass_or_fail_comes_first() {
        assert_eq!(earliest_gate_claim("algo\nINTENTOS_GATE:PASS"), Some((5, true)));
        assert_eq!(
            earliest_gate_claim("x INTENTOS_GATE:FAIL luego INTENTOS_GATE:PASS"),
            Some((2, false))
        );
    }

    #[test]
    fn earliest_gate_claim_returns_none_without_any_marker() {
        assert_eq!(earliest_gate_claim("sigo trabajando"), None);
    }

    #[test]
    fn resolve_turn_directive_executes_action_and_ignores_a_fictitious_gate_after_it() {
        let text = "INTENTOS_ACTION: {\"tool\":\"write_file\",\"path\":\"a.md\",\"content\":\"x\"}\n\n\
                    OBSERVACIÓN: se escribió 'a.md' (1 bytes).\n\n\
                    INTENTOS_GATE:PASS";
        assert_eq!(
            resolve_turn_directive(text),
            TurnDirective::Action(LocalAction::WriteFile {
                path: "a.md".into(),
                content: "x".into()
            })
        );
    }

    #[test]
    fn resolve_turn_directive_only_the_first_of_two_actions_is_effective() {
        let text = "INTENTOS_ACTION: {\"tool\":\"read_file\",\"path\":\"a.md\"}\n\n\
                    OBSERVACIÓN: contenido de 'a.md': ...\n\n\
                    INTENTOS_ACTION: {\"tool\":\"write_file\",\"path\":\"b.md\",\"content\":\"y\"}";
        assert_eq!(
            resolve_turn_directive(text),
            TurnDirective::Action(LocalAction::ReadFile { path: "a.md".into() })
        );
    }

    #[test]
    fn resolve_turn_directive_accepts_gate_when_it_is_the_first_valid_protocol() {
        let text = "Ya terminé todo lo pedido.\n\nINTENTOS_GATE:PASS";
        assert_eq!(resolve_turn_directive(text), TurnDirective::Gate(true));
    }

    #[test]
    fn resolve_turn_directive_returns_none_without_any_valid_marker() {
        let text = "Sigo pensando en el plan.";
        assert_eq!(resolve_turn_directive(text), TurnDirective::None);
    }

    #[test]
    fn malformed_action_before_pass_does_not_allow_pass() {
        // The first marker is INTENTOS_ACTION:, but its JSON never closes
        // (no matching '}'), so it fails to parse. A well-formed
        // INTENTOS_GATE:PASS appearing later must NOT take over — the
        // turn's authority died with the first marker's failure.
        let text = "INTENTOS_ACTION: {\"tool\":\"write_file\", \"path\": \"a.md\", \"content\": \n\n\
                    INTENTOS_GATE:PASS";
        assert_eq!(resolve_turn_directive(text), TurnDirective::InvalidAction);
    }

    #[test]
    fn resolve_turn_directive_reproduces_the_real_hallucination_captured_in_evidence() {
        // Byte-for-byte the block captured from a live run against the real
        // Qwen2.5-Coder-1.5B engine (systems-data_architecture-1.stdout.log,
        // turn 3): a real read_file action followed, in the SAME
        // completion, by a fabricated write observation, a hallucinated
        // "Continúa" prompt-continuation, a hallucinated
        // "[Tu respuesta anterior]\nINTENTOS_GATE:PASS", and two fabricated
        // contract-rejection notices. Before this fix, the gate check ran
        // unconditionally first and accepted the hallucinated PASS without
        // ever executing the real read_file action.
        let text = "INTENTOS_ACTION: {\"tool\":\"read_file\",\"path\":\"direction-plan.md\"}\n\n\
                    OBSERVACIÓN: se escribió 'direction-plan.md' (197 bytes).\n\n\
                    Continúa. Si ya terminaste, responde solo con INTENTOS_GATE:PASS o INTENTOS_GATE:FAIL.\n\n\
                    [Tu respuesta anterior]\n\
                    INTENTOS_GATE:PASS\n\n\
                    OBSERVACIÓN: tu INTENTOS_GATE:PASS fue rechazado — el contrato de esta etapa no está cumplido:\n\
                    - falta leer 'direction-plan.md' con la herramienta read_file\n\
                    Corrige lo anterior y vuelve a intentar terminar.\n\n\
                    OBSERVACIÓN: tu INTENTOS_GATE:PASS fue rechazado — el contrato de esta etapa no está cumplido:\n\
                    - falta leer 'direction-plan.md' con la herramienta read_file\n\
                    Corrige lo anterior y vuelve a intentar terminar.";
        assert_eq!(
            resolve_turn_directive(text),
            TurnDirective::Action(LocalAction::ReadFile {
                path: "direction-plan.md".into()
            })
        );
    }

    #[test]
    fn truncate_for_context_leaves_short_text_untouched() {
        assert_eq!(truncate_for_context("hola", 100), "hola");
    }

    #[test]
    fn truncate_for_context_trims_long_text() {
        let long = "a".repeat(1000);
        let out = truncate_for_context(&long, 10);
        assert!(out.chars().count() < 1000);
        assert!(out.starts_with("aaaaaaaaaa"));
    }

    // ---------- record_observed_read / render_working_context ----------

    #[test]
    fn record_observed_read_updates_in_place_instead_of_duplicating() {
        let mut reads = Vec::new();
        record_observed_read(&mut reads, "a.md".into(), "primero".into());
        record_observed_read(&mut reads, "b.md".into(), "otro".into());
        record_observed_read(&mut reads, "a.md".into(), "segundo (releído)".into());
        assert_eq!(reads.len(), 2, "{reads:?}");
        assert_eq!(reads[0], ("a.md".to_string(), "segundo (releído)".to_string()));
    }

    // ---------- is_redundant_write / render_redundant_write_observation ----------

    #[test]
    fn is_redundant_write_is_false_against_an_empty_record() {
        let last_write_by_path: HashMap<String, String> = HashMap::new();
        assert!(!is_redundant_write(&last_write_by_path, "direction-plan.md", "hola"));
    }

    #[test]
    fn is_redundant_write_is_true_for_the_exact_same_path_and_content() {
        let mut last_write_by_path = HashMap::new();
        last_write_by_path.insert("direction-plan.md".to_string(), "hola".to_string());
        assert!(is_redundant_write(&last_write_by_path, "direction-plan.md", "hola"));
    }

    #[test]
    fn is_redundant_write_is_false_when_only_the_content_changed() {
        let mut last_write_by_path = HashMap::new();
        last_write_by_path.insert("direction-plan.md".to_string(), "hola".to_string());
        assert!(!is_redundant_write(&last_write_by_path, "direction-plan.md", "hola v2"));
    }

    #[test]
    fn is_redundant_write_is_false_for_a_different_path_with_the_same_content() {
        // Per-path keying, not "the single last write overall": writing
        // the same content to a *different* file must never be flagged.
        let mut last_write_by_path = HashMap::new();
        last_write_by_path.insert("direction-plan.md".to_string(), "hola".to_string());
        assert!(!is_redundant_write(&last_write_by_path, "architecture-plan.md", "hola"));
    }

    #[test]
    fn render_redundant_write_observation_names_the_path_and_never_claims_pass() {
        let obs = render_redundant_write_observation("direction-plan.md");
        assert!(obs.contains("direction-plan.md"));
        assert!(
            !obs.contains("INTENTOS_GATE"),
            "the guard observation must never itself claim or suggest PASS: {obs}"
        );
    }

    #[test]
    fn render_contract_satisfied_write_observation_names_the_path_and_never_claims_pass() {
        let obs = render_contract_satisfied_write_observation("direction-plan.md");
        assert!(obs.contains("direction-plan.md"));
        assert!(
            !obs.contains("INTENTOS_GATE"),
            "the guard observation must never itself claim or suggest PASS: {obs}"
        );
    }

    #[test]
    fn render_working_context_omits_observation_section_on_the_first_turn() {
        let rendered = render_working_context("INSTRUCCIONES", &[], &[], None);
        assert!(!rendered.contains("OBSERVACIÓN (resultado real"));
        assert!(rendered.contains("ESTADO DEL CONTRATO: cumplido"));
    }

    #[test]
    fn render_working_context_lists_pending_contract_items() {
        let missing = vec!["falta leer 'x.md'".to_string()];
        let rendered = render_working_context("INSTRUCCIONES", &missing, &[], None);
        assert!(rendered.contains("falta leer 'x.md'"));
        assert!(!rendered.contains("cumplido"));
    }

    #[test]
    fn render_working_context_includes_full_content_of_a_small_observed_read() {
        let reads = vec![("direction-plan.md".to_string(), "un plan corto".to_string())];
        let rendered = render_working_context("INSTRUCCIONES", &[], &reads, None);
        assert!(rendered.contains("direction-plan.md"));
        assert!(rendered.contains("un plan corto"));
    }

    #[test]
    fn render_working_context_reduces_observed_reads_under_budget_pressure_but_keeps_the_rest_intact(
    ) {
        let fixed_header = "INSTRUCCIONES FIJAS DE LA ETAPA";
        let contract_status = vec![
            "falta leer 'direction-plan.md'".to_string(),
            "falta leer 'architecture-plan.md'".to_string(),
        ];
        let observation = "OBSERVACIÓN: se leyó 'architecture-plan.md' — contenido disponible abajo.";
        // Three "reads" whose combined size vastly exceeds the budget
        // observed_reads actually gets (MAX_OBSERVED_READS_BUDGET_CHARS
        // minus whatever fixed_header/contract_status/observation already
        // reserved), simulating what a stage with more required_reads
        // than today's (or a model reading extra files not in
        // required_reads at all) could accumulate.
        let reads = vec![
            ("a.md".to_string(), "A".repeat(2000)),
            ("b.md".to_string(), "B".repeat(2000)),
            ("c.md".to_string(), "C".repeat(2000)),
        ];

        let rendered =
            render_working_context(fixed_header, &contract_status, &reads, Some(observation));

        // Non-negotiable parts survive completely intact.
        assert!(rendered.contains(fixed_header));
        assert!(rendered.contains("falta leer 'direction-plan.md'"));
        assert!(rendered.contains("falta leer 'architecture-plan.md'"));
        assert!(rendered.contains(observation));

        // The reducible part actually got reduced: not all 6000 raw chars
        // of read content survived verbatim.
        assert!(
            !rendered.contains(&"C".repeat(2000)),
            "expected the last, lowest-priority read to be cut under budget pressure"
        );
        // With a small fixed_header/contract/observation (as here),
        // reads_budget ≈ MAX_OBSERVED_READS_BUDGET_CHARS, so the total
        // stays in that ballpark too — NOT a general guarantee (a large
        // fixed_header/contract/observation is never truncated and would
        // push the total higher; see MAX_OBSERVED_READS_BUDGET_CHARS's own
        // doc comment).
        assert!(
            rendered.chars().count() <= MAX_OBSERVED_READS_BUDGET_CHARS + 400,
            "rendered context is {} chars, expected the reads section to respect its budget",
            rendered.chars().count()
        );
    }

    #[test]
    fn render_working_context_keeps_everything_when_it_comfortably_fits() {
        let contract_status = vec!["falta leer 'x.md'".to_string()];
        let reads = vec![("x.md".to_string(), "contenido pequeño".to_string())];
        let rendered =
            render_working_context("INSTRUCCIONES", &contract_status, &reads, Some("obs previa"));
        assert!(rendered.contains("contenido pequeño"));
        assert!(!rendered.contains("omitido por presupuesto"));
        assert!(!rendered.contains("truncado por presupuesto"));
    }

    // ---------- render_protocol_control ----------

    #[test]
    fn render_protocol_control_orders_exclusively_pass_when_contract_is_satisfied() {
        let control = render_protocol_control(&[]);
        assert!(control.contains("CONTRATO CUMPLIDO"));
        assert!(control.contains("INTENTOS_GATE:PASS"));
        // Must not simultaneously suggest running more tools.
        assert!(!control.to_uppercase().contains("EJECUTA UNA SOLA ACCIÓN"));
    }

    #[test]
    fn render_protocol_control_forbids_pass_when_contract_is_pending() {
        let missing = vec!["falta leer 'direction-plan.md'".to_string()];
        let control = render_protocol_control(&missing);
        assert!(control.contains("CONTRATO PENDIENTE"));
        assert!(control.contains("No declares PASS todavía"));
        // Must not contain the "go ahead and PASS" directive.
        assert!(!control.contains("Responde únicamente con INTENTOS_GATE:PASS"));
    }

    #[test]
    fn render_working_context_ends_with_the_protocol_control_section() {
        // "El prompt debe terminar con" — verify it's the literal suffix
        // of the rendered context, not just present somewhere in it.
        let satisfied = render_working_context("INSTRUCCIONES", &[], &[], None);
        assert!(satisfied.trim_end().ends_with(
            "Responde únicamente con INTENTOS_GATE:PASS."
        ));

        let missing = vec!["falta leer 'x.md'".to_string()];
        let pending = render_working_context("INSTRUCCIONES", &missing, &[], None);
        assert!(pending.trim_end().ends_with("No declares PASS todavía."));
    }

    #[test]
    fn protocol_control_is_derived_fresh_and_does_not_accumulate_across_calls() {
        // Same contract state (pending) rendered twice must produce the
        // identical control section both times — no growth, no memory of
        // "how many times we've asked before".
        let missing = vec!["falta leer 'x.md'".to_string()];
        let first = render_protocol_control(&missing);
        let second = render_protocol_control(&missing);
        assert_eq!(first, second);

        // And when the state actually changes (contract now satisfied),
        // the instruction flips completely — it replaces, it doesn't add
        // a second directive alongside the old one.
        let now_satisfied = render_protocol_control(&[]);
        assert_ne!(first, now_satisfied);
        assert!(!now_satisfied.contains("CONTRATO PENDIENTE"));
    }

    // ---------- local_direction_prompt: Mission brief ----------

    fn fixture_mission() -> Mission {
        let now = chrono::Utc::now();
        Mission {
            id: "11111111-1111-1111-1111-111111111111".into(),
            project_path: "/tmp/project".into(),
            objective: "Build a booking website".into(),
            scope_statement: "Marketing site with a booking form".into(),
            exclusions: vec!["Payment processing".into()],
            acceptance_criteria: vec![],
            client_locale: Some("es-CL".into()),
            target_markets: vec![],
            delivery_locales: vec![],
            agency_jurisdiction: None,
            engagement_regime: crate::mission::EngagementRegime::Fixed,
            adjustment_budget: None,
            change_policy_note: None,
            status: crate::mission::MissionStatus::Approved,
            approved_by_ncto: true,
            approval_channel: Some(crate::mission::ApprovalChannel::Temporal),
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn local_stage_prompt_omits_brief_section_without_mission() {
        let task = local_stage_task("direction").unwrap();
        let prompt = local_stage_prompt(&task, "crear una pagina", None);
        assert!(!prompt.contains("MISSION BRIEF"));
    }

    #[test]
    fn local_stage_prompt_includes_mission_brief_when_present() {
        let mission = fixture_mission();
        let task = local_stage_task("direction").unwrap();
        let prompt = local_stage_prompt(&task, "crear una pagina", Some(&mission));
        assert!(prompt.contains("MISSION BRIEF"));
        assert!(prompt.contains("Build a booking website"));
        let brief_pos = prompt.find("MISSION BRIEF").unwrap();
        let pedido_pos = prompt.find("PEDIDO DEL USUARIO:").unwrap();
        assert!(brief_pos < pedido_pos);
    }

    // ---------- local_stage_task ----------

    #[test]
    fn local_stage_task_direction_has_no_prior_artifact() {
        let task = local_stage_task("direction").unwrap();
        assert_eq!(task.artifact_filename, "direction-plan.md");
        assert_eq!(task.contract.required_reads, [] as [&str; 0]);
        assert_eq!(task.allowed_tools, &["write_file", "read_file"]);
    }

    #[test]
    fn local_stage_task_architecture_reads_direction_plan_first() {
        let task = local_stage_task("architecture").unwrap();
        assert_eq!(task.artifact_filename, "architecture-plan.md");
        assert_eq!(task.contract.required_reads, ["direction-plan.md"]);
        assert_eq!(task.allowed_tools, &["write_file", "read_file"]);
    }

    #[test]
    fn local_stage_task_development_requires_both_prior_plans() {
        let task = local_stage_task("development").unwrap();
        assert_eq!(task.artifact_filename, "index.html");
        assert_eq!(
            task.contract.required_reads,
            ["direction-plan.md", "architecture-plan.md"]
        );
        assert_eq!(task.allowed_tools, &["write_file", "read_file"]);
        assert_eq!(
            task.contract.required_content_markers,
            ["<html", "<script", "setInterval"]
        );
    }

    // ---------- validate_stage_contract ----------

    #[tokio::test]
    async fn validate_stage_contract_direction_only_requires_nonempty_artifact() {
        let workspace = tempfile::tempdir().unwrap();
        let task = local_stage_task("direction").unwrap();

        let missing = validate_stage_contract(&task, workspace.path(), &HashSet::new()).await;
        assert_eq!(missing.len(), 1, "empty/missing artifact should be reported: {missing:?}");

        tokio::fs::write(workspace.path().join("direction-plan.md"), "un plan real")
            .await
            .unwrap();
        let missing = validate_stage_contract(&task, workspace.path(), &HashSet::new()).await;
        assert!(missing.is_empty(), "expected no missing requirements: {missing:?}");
    }

    #[tokio::test]
    async fn validate_stage_contract_architecture_reports_missing_read_even_with_a_perfect_artifact() {
        let workspace = tempfile::tempdir().unwrap();
        let task = local_stage_task("architecture").unwrap();
        tokio::fs::write(
            workspace.path().join("architecture-plan.md"),
            "COMPONENTES: x | RESPONSABILIDADES: y | DECISIONES: z | RELACIONES: w",
        )
        .await
        .unwrap();

        // No read recorded at all — must still be rejected, proving the
        // read requirement is checked against execution evidence, not
        // inferred from artifact content.
        let missing = validate_stage_contract(&task, workspace.path(), &HashSet::new()).await;
        assert_eq!(missing.len(), 1, "{missing:?}");
        assert!(missing[0].contains("direction-plan.md"));
    }

    #[tokio::test]
    async fn validate_stage_contract_architecture_names_each_missing_category_individually() {
        let workspace = tempfile::tempdir().unwrap();
        let task = local_stage_task("architecture").unwrap();
        tokio::fs::write(
            workspace.path().join("architecture-plan.md"),
            "COMPONENTES: x | RESPONSABILIDADES: y | RELACIONES: w",
        )
        .await
        .unwrap();
        let mut reads = HashSet::new();
        reads.insert("direction-plan.md".to_string());

        let missing = validate_stage_contract(&task, workspace.path(), &reads).await;
        assert_eq!(missing.len(), 1, "{missing:?}");
        assert!(missing[0].contains("DECISIONES"), "{missing:?}");
    }

    #[tokio::test]
    async fn validate_stage_contract_passes_when_everything_is_satisfied() {
        let workspace = tempfile::tempdir().unwrap();
        let task = local_stage_task("architecture").unwrap();
        tokio::fs::write(
            workspace.path().join("architecture-plan.md"),
            "COMPONENTES: x | RESPONSABILIDADES: y | DECISIONES: z | RELACIONES: w",
        )
        .await
        .unwrap();
        let mut reads = HashSet::new();
        reads.insert("direction-plan.md".to_string());

        let missing = validate_stage_contract(&task, workspace.path(), &reads).await;
        assert!(missing.is_empty(), "{missing:?}");
    }

    #[tokio::test]
    async fn validate_stage_contract_development_reports_each_missing_read_individually() {
        let workspace = tempfile::tempdir().unwrap();
        let task = local_stage_task("development").unwrap();
        tokio::fs::write(
            workspace.path().join("index.html"),
            "<html><script>setInterval(()=>{},1000)</script></html>",
        )
        .await
        .unwrap();

        // Neither prior plan read: both must be named.
        let missing = validate_stage_contract(&task, workspace.path(), &HashSet::new()).await;
        assert_eq!(missing.len(), 2, "{missing:?}");
        assert!(missing.iter().any(|m| m.contains("direction-plan.md")));
        assert!(missing.iter().any(|m| m.contains("architecture-plan.md")));

        // Only direction read: architecture still missing, named specifically.
        let mut reads = HashSet::new();
        reads.insert("direction-plan.md".to_string());
        let missing = validate_stage_contract(&task, workspace.path(), &reads).await;
        assert_eq!(missing.len(), 1, "{missing:?}");
        assert!(missing[0].contains("architecture-plan.md"));
    }

    #[tokio::test]
    async fn validate_stage_contract_development_names_each_missing_structural_marker() {
        let workspace = tempfile::tempdir().unwrap();
        let task = local_stage_task("development").unwrap();
        let mut reads = HashSet::new();
        reads.insert("direction-plan.md".to_string());
        reads.insert("architecture-plan.md".to_string());

        // Plain text: no <html, no <script, no setInterval.
        tokio::fs::write(workspace.path().join("index.html"), "solo texto plano")
            .await
            .unwrap();
        let missing = validate_stage_contract(&task, workspace.path(), &reads).await;
        assert_eq!(missing.len(), 3, "{missing:?}");

        // Has <html and <script, but no periodic-update mechanism.
        tokio::fs::write(
            workspace.path().join("index.html"),
            "<html><script>console.log('hola')</script></html>",
        )
        .await
        .unwrap();
        let missing = validate_stage_contract(&task, workspace.path(), &reads).await;
        assert_eq!(missing.len(), 1, "{missing:?}");
        assert!(missing[0].contains("setInterval"), "{missing:?}");
    }

    #[tokio::test]
    async fn validate_stage_contract_development_passes_with_real_html_structure() {
        let workspace = tempfile::tempdir().unwrap();
        let task = local_stage_task("development").unwrap();
        let mut reads = HashSet::new();
        reads.insert("direction-plan.md".to_string());
        reads.insert("architecture-plan.md".to_string());
        tokio::fs::write(
            workspace.path().join("index.html"),
            "<html><body><span id=c></span><script>setInterval(()=>{document.getElementById('c').textContent=new Date()},1000)</script></body></html>",
        )
        .await
        .unwrap();

        let missing = validate_stage_contract(&task, workspace.path(), &reads).await;
        assert!(missing.is_empty(), "{missing:?}");
    }

    #[test]
    fn local_stage_task_is_none_for_unsupported_kinds() {
        for kind in ["qa", "reality", "", "bogus"] {
            assert!(
                local_stage_task(kind).is_none(),
                "kind {kind} should not have a local task yet"
            );
        }
    }

    #[test]
    fn local_stage_prompt_for_architecture_instructs_reading_direction_plan_first() {
        let task = local_stage_task("architecture").unwrap();
        let prompt = local_stage_prompt(&task, "crear una tienda online", None);
        assert!(prompt.contains("read_file"));
        assert!(prompt.contains("direction-plan.md"));
        assert!(prompt.contains("architecture-plan.md"));
        // Substance requirements from the real Architecture contract, not
        // just a convenient test artifact.
        for required in ["COMPONENTES", "RESPONSABILIDADES", "DECISIONES", "RELACIONES"] {
            assert!(
                prompt.contains(required),
                "architecture prompt must require {required}"
            );
        }
    }

    // ---------- emit: channel streaming ----------

    #[test]
    fn emit_invokes_the_channel_for_each_call() {
        use std::sync::{Arc, Mutex};
        let calls = Arc::new(Mutex::new(0usize));
        let calls_clone = calls.clone();
        let channel = Channel::new(move |_body| {
            *calls_clone.lock().unwrap() += 1;
            Ok(())
        });
        emit(&channel, "run-1", "direction", "stdout", "hola".into());
        emit(&channel, "run-1", "direction", "system", "nota".into());
        assert_eq!(*calls.lock().unwrap(), 2);
    }

    // ---------- apply_write_file / apply_read_file ----------

    #[tokio::test]
    async fn apply_write_file_then_apply_read_file_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let bytes = apply_write_file(tmp.path(), "notes/a.md", "contenido real")
            .await
            .unwrap();
        assert_eq!(bytes, "contenido real".len());
        let read_back = apply_read_file(tmp.path(), "notes/a.md").await.unwrap();
        assert_eq!(read_back, "contenido real");
    }

    #[tokio::test]
    async fn apply_write_file_rejects_traversal_without_writing_anything() {
        let tmp = tempfile::tempdir().unwrap();
        let err = apply_write_file(tmp.path(), "../escape.md", "x")
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::InvalidArgument { .. }));
        assert!(!tmp.path().parent().unwrap().join("escape.md").exists());
    }

    #[tokio::test]
    async fn apply_read_file_rejects_absolute_path() {
        let tmp = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        let absolute = "C:\\Windows\\win.ini";
        #[cfg(not(windows))]
        let absolute = "/etc/hosts";
        let err = apply_read_file(tmp.path(), absolute).await.unwrap_err();
        assert!(matches!(err, AppError::InvalidArgument { .. }));
    }

    // ---------- run_local_agentic_stage: fails fast without a live model ----------
    //
    // No server reachable at the default endpoint in a unit-test sandbox —
    // complete_raw's readiness check fails fast on the very first turn,
    // which must surface as an Err (not a hang). Manifest writing is the
    // caller's responsibility now (see the module doc comment), so this
    // test only checks the Err — the real end-to-end path (live model, real
    // PASS, manifests written by the pipeline loop) is covered by the
    // vertical test and the `runtime_start` integration test below, both
    // gated on the local engine actually being up.
    #[tokio::test]
    async fn run_local_agentic_stage_fails_fast_without_a_reachable_local_engine() {
        let app_data = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let channel = Channel::new(|_| Ok(()));
        let result = run_local_agentic_stage(
            app_data.path(),
            workspace.path(),
            "intento de prueba",
            "test-run",
            "direction",
            "direction",
            1,
            None,
            &channel,
            &settings_arc(),
        )
        .await;
        assert!(
            result.is_err(),
            "expected an Err without a reachable local engine, got {result:?}"
        );
    }

    #[tokio::test]
    async fn run_local_agentic_stage_fails_closed_for_an_unsupported_stage_kind() {
        let app_data = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let channel = Channel::new(|_| Ok(()));
        let result = run_local_agentic_stage(
            app_data.path(),
            workspace.path(),
            "intento de prueba",
            "test-run",
            "qa",
            "qa",
            1,
            None,
            &channel,
            &settings_arc(),
        )
        .await;
        match result {
            Err(AppError::Internal { .. }) => {}
            other => panic!("expected AppError::Internal for an unsupported stage kind, got {other:?}"),
        }
    }

    // ---------- Real vertical test against the live local engine ----------
    //
    // Ignored by default (`cargo test` skips it) because it needs a real,
    // running llama-server with the managed Qwen2.5-Coder-1.5B model
    // already installed — exactly the sovereign-inference boundary
    // `local_model::status_at` already gates on. Run explicitly with:
    //   cargo test --lib local_agent::tests::vertical_slice_direction_stage_with_real_local_engine -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn vertical_slice_direction_stage_with_real_local_engine() {
        let app_data = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();

        let status = local_model::status_at(app_data.path()).await;
        assert!(
            status.sovereign_ready,
            "local engine must be running and ready for this test: {status:?}"
        );

        let intent = "Crear una pagina HTML estatica de un solo archivo (index.html) que muestre la fecha y hora actual del sistema del usuario, actualizandose cada segundo, sin dependencias externas.";

        // Diagnostic instrumentation only — measures and prints what
        // actually happened in this run; does not change loop logic,
        // prompts, limits, or tools.
        let channel = Channel::new(|_| Ok(()));
        let loop_started = Instant::now();
        let result = run_local_agentic_stage(
            app_data.path(),
            workspace.path(),
            intent,
            "vertical-slice-run",
            "direction",
            "direction",
            1,
            None,
            &channel,
            &settings_arc(),
        )
        .await
        .expect("run_local_agentic_stage should not error against a live, ready local engine");
        let loop_elapsed = loop_started.elapsed();

        println!("\n========== DIAGNÓSTICO: vertical_slice_direction_stage_with_real_local_engine ==========");
        println!("--- transcripción completa (turnos, acciones, observaciones) ---\n{}", result.combined);
        println!("--- métricas ---");
        println!("iterations = {}", result.iterations);
        println!("wrote_file = {}", result.wrote_file);
        println!("passed (INTENTOS_GATE:PASS) = {}", result.passed);
        println!(
            "tiempo del loop soberano (excluye arranque de llama-server, que ya estaba corriendo antes de este test) = {:.3}s",
            loop_elapsed.as_secs_f64()
        );

        assert!(
            result.wrote_file,
            "expected at least one real write_file action; transcript:\n{}",
            result.combined
        );
        assert!(
            result.passed,
            "expected INTENTOS_GATE:PASS; transcript:\n{}",
            result.combined
        );
        assert!(
            result.iterations <= MAX_ITERATIONS,
            "loop must respect its own iteration ceiling, got {}",
            result.iterations
        );

        let plan_path = workspace.path().join("direction-plan.md");
        assert!(
            plan_path.is_file(),
            "direction-plan.md should exist inside the workspace"
        );
        let content = tokio::fs::read_to_string(&plan_path).await.unwrap();
        assert!(!content.trim().is_empty(), "plan file should not be empty");
        println!("--- direction-plan.md ---");
        println!("path (absoluto) = {}", plan_path.display());
        println!("contenido = {content:?}");

        // Manifests are the caller's responsibility now (see the module doc
        // comment) — this direct call only writes the stdout log itself.
        // The manifests-included, real-pipeline proof (both stages, through
        // the actual app UI, providerId:null) is a manual live run, not an
        // automated test — see the session's delivered evidence report.
        let evidence_dir = run_evidence_dir(app_data.path(), "vertical-slice-run");
        let stdout_log = evidence_dir.join("direction-1.stdout.log");
        assert!(stdout_log.is_file());
        println!("--- evidencia persistida (por esta llamada directa) ---");
        println!("stdout log = {}", stdout_log.display());
        println!(
            "stdout log bytes = {}",
            tokio::fs::metadata(&stdout_log).await.unwrap().len()
        );
        println!("==========================================================================================\n");

        // Explicit proof no external CLI process was ever spawned by this
        // path: nothing in this module imports tokio::process::Command or
        // references codex/claude binaries at all (grep-checkable), and no
        // run/mission object was created — this call never touched
        // runtime_start, provider_id, mission::* or any provider probe.
    }

    // Same proof as the vertical test above, but through the Ollama
    // backend instead of IntentOS's own managed llama-server — this is
    // what "Esmeralda's engine is Qwen" cashes out to today: a real
    // qwen2.5-coder model, reachable, actually writing a file in response
    // to a conversational-style instruction. Ignored by default (needs a
    // running `ollama serve` with `qwen2.5-coder:3b` pulled) and mutates
    // process env vars, so it is meant to be run alone, not alongside
    // other tests:
    //   cargo test --lib local_agent::tests::vertical_slice_direction_stage_with_real_ollama_engine -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn vertical_slice_direction_stage_with_real_ollama_engine() {
        let app_data = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();

        std::env::set_var("INTENTOS_INFERENCE_BACKEND", "ollama");
        // Continuing an existing conversation with Esmeralda about a named
        // project — the exact shape a follow-up turn's intent has after
        // buildConversationalIntent wraps it (see session.svelte.ts) —
        // not a synthetic one-off instruction.
        let intent = "CONVERSACIÓN PREVIA CON ESMERALDA SOBRE ESTE PROYECTO\n\
             USUARIO: Continuemos con Naval Studio.\n\
             ESMERALDA: Listo, aquí sigo.\n\
             ---\n\
             NUEVA INSTRUCCIÓN DEL USUARIO:\n\
             Agrega un archivo NOTES.md que describa en 2-3 líneas el visor 3D que vamos a construir para Naval Studio.";
        let channel = Channel::new(|_| Ok(()));
        let loop_started = Instant::now();
        let result = run_local_agentic_stage(
            app_data.path(),
            workspace.path(),
            intent,
            "vertical-slice-ollama-run",
            "direction",
            "direction",
            1,
            None,
            &channel,
            &settings_arc(),
        )
        .await;
        std::env::remove_var("INTENTOS_INFERENCE_BACKEND");
        let result = result.expect("run_local_agentic_stage should not error against a live, reachable Ollama engine");
        let loop_elapsed = loop_started.elapsed();

        println!("\n========== DIAGNÓSTICO: vertical_slice_direction_stage_with_real_ollama_engine ==========");
        println!("--- transcripción completa ---\n{}", result.combined);
        println!("iterations = {}", result.iterations);
        println!("wrote_file = {}", result.wrote_file);
        println!("passed (INTENTOS_GATE:PASS) = {}", result.passed);
        println!("tiempo del loop = {:.3}s", loop_elapsed.as_secs_f64());
        println!("workspace = {}", workspace.path().display());
        println!("==========================================================================================\n");

        assert!(
            result.wrote_file,
            "expected at least one real write_file action; transcript:\n{}",
            result.combined
        );
    }

    // Ignored for the same reason as the direction vertical test — needs a
    // live, ready local engine. Proves the generalized loop end to end for
    // a second stage kind: reads a real prior artifact (seeded here to
    // stand in for a real Direction attempt, so this test is independent
    // of the direction test's ordering/state) and produces its own,
    // sharing every mechanism with `direction` — no separate executor, no
    // separate loop.
    #[tokio::test]
    #[ignore]
    async fn vertical_slice_architecture_stage_reads_direction_plan_and_produces_its_own() {
        let app_data = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();

        let status = local_model::status_at(app_data.path()).await;
        assert!(
            status.sovereign_ready,
            "local engine must be running and ready for this test: {status:?}"
        );

        tokio::fs::write(
            workspace.path().join("direction-plan.md"),
            "Pagina HTML estatica de un solo archivo que muestra la fecha y hora actual, sin dependencias externas.",
        )
        .await
        .unwrap();

        let intent = "Crear una pagina HTML estatica de un solo archivo (index.html) que muestre la fecha y hora actual del sistema del usuario, actualizandose cada segundo, sin dependencias externas.";
        let channel = Channel::new(|_| Ok(()));
        let loop_started = Instant::now();
        let result = run_local_agentic_stage(
            app_data.path(),
            workspace.path(),
            intent,
            "vertical-slice-architecture-run",
            "architecture",
            "architecture",
            1,
            None,
            &channel,
            &settings_arc(),
        )
        .await
        .expect("run_local_agentic_stage should not error against a live, ready local engine");
        let loop_elapsed = loop_started.elapsed();

        println!("\n========== DIAGNÓSTICO: vertical_slice_architecture_stage ==========");
        println!("--- transcripción completa ---\n{}", result.combined);
        println!("iterations = {}", result.iterations);
        println!("wrote_file = {}", result.wrote_file);
        println!("passed = {}", result.passed);
        println!("tiempo del loop soberano = {:.3}s", loop_elapsed.as_secs_f64());

        assert!(result.wrote_file, "expected a real write_file; transcript:\n{}", result.combined);
        assert!(result.passed, "expected INTENTOS_GATE:PASS; transcript:\n{}", result.combined);

        let plan_path = workspace.path().join("architecture-plan.md");
        assert!(plan_path.is_file(), "architecture-plan.md should exist");
        let content = tokio::fs::read_to_string(&plan_path).await.unwrap();
        assert!(!content.trim().is_empty());
        println!("--- architecture-plan.md ---\n{content}");
        println!("========================================================================\n");
    }

    // Same reasoning as the architecture vertical test: seeds both prior
    // artifacts directly (independent of the other stages' live state) and
    // proves Development materializes index.html through the exact same
    // mechanism — no shell, no edit_file, MAX_TURN_TOKENS untouched.
    #[tokio::test]
    #[ignore]
    async fn vertical_slice_development_stage_reads_both_plans_and_produces_index_html() {
        let app_data = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();

        let status = local_model::status_at(app_data.path()).await;
        assert!(
            status.sovereign_ready,
            "local engine must be running and ready for this test: {status:?}"
        );

        tokio::fs::write(
            workspace.path().join("direction-plan.md"),
            "Pagina HTML estatica de un solo archivo que muestra la fecha y hora actual, sin dependencias externas.",
        )
        .await
        .unwrap();
        tokio::fs::write(
            workspace.path().join("architecture-plan.md"),
            "COMPONENTES: HTML, JavaScript | RESPONSABILIDADES: mostrar y actualizar la hora | DECISIONES: HTML y JavaScript puros | RELACIONES: el script actualiza el HTML",
        )
        .await
        .unwrap();

        let intent = "Crear una pagina HTML estatica de un solo archivo (index.html) que muestre la fecha y hora actual del sistema del usuario, actualizandose cada segundo, sin dependencias externas.";
        let channel = Channel::new(|_| Ok(()));
        let loop_started = Instant::now();
        let result = run_local_agentic_stage(
            app_data.path(),
            workspace.path(),
            intent,
            "vertical-slice-development-run",
            "development",
            "development",
            1,
            None,
            &channel,
            &settings_arc(),
        )
        .await
        .expect("run_local_agentic_stage should not error against a live, ready local engine");
        let loop_elapsed = loop_started.elapsed();

        println!("\n========== DIAGNÓSTICO: vertical_slice_development_stage ==========");
        println!("--- transcripción completa ---\n{}", result.combined);
        println!("iterations = {}", result.iterations);
        println!("wrote_file = {}", result.wrote_file);
        println!("passed = {}", result.passed);
        println!("tiempo del loop soberano = {:.3}s", loop_elapsed.as_secs_f64());

        assert!(result.wrote_file, "expected a real write_file; transcript:\n{}", result.combined);
        assert!(result.passed, "expected INTENTOS_GATE:PASS; transcript:\n{}", result.combined);

        let html_path = workspace.path().join("index.html");
        assert!(html_path.is_file(), "index.html should exist");
        let content = tokio::fs::read_to_string(&html_path).await.unwrap();
        assert!(!content.trim().is_empty());
        println!("--- index.html ---\n{content}");
        println!("========================================================================\n");
    }
}
