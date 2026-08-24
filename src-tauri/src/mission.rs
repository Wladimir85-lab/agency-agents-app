//! IntentOS Mission — persistence and Mission Brief serialization.
//!
//! A Mission is Mission/Engagement-level intent: objective, scope,
//! exclusions, acceptance criteria, locale/market context, and the
//! commercial parameters (engagement regime, adjustment budget, change
//! policy note) that a levantamiento produces before Factory runs. It is
//! deliberately separate from `ProjectInfo` (the pre-existing install-
//! tracking registry in `types.rs`, unrelated to Mission semantics) and
//! from Runtime v0.1's `RunSummary`.
//!
//! Runtime v0.1's own frozen contract (StartRunRequest, the PASS/FAIL gate,
//! stage_prompt's output when no Mission is attached) is untouched by this
//! module — `mission_id` on `StartRunRequest`/`RunSummary` is additive and
//! optional. See memory-bank/intentosNavigationCharter.md and the Agency
//! Operating Specification v1 for the architectural rationale: this is
//! schema and a pure serialization function, not a rules engine, not a
//! Policy Ledger, and not a new Ramo/Department/Capability.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use crate::error::AppError;
use crate::state::AppState;
use crate::util::fs::{atomic_write, read_capped};

const MAX_MISSION_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_TEXT_FIELD_CHARS: usize = 20_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EngagementRegime {
    Fixed,
    Variable,
    Retainer,
}

fn default_engagement_regime() -> EngagementRegime {
    EngagementRegime::Fixed
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MissionStatus {
    Draft,
    Approved,
    InProduction,
    Delivered,
    InSupport,
    Closed,
}

fn default_mission_status() -> MissionStatus {
    MissionStatus::Draft
}

/// Mission/Engagement-level intent. Every field here is data — none of it
/// is executable policy. `exclusions`, `acceptance_criteria`,
/// `adjustment_budget` and `change_policy_note` intentionally hold plain
/// text/lists rather than a rules schema: the Agency Operating
/// Specification v1 found real evidence for the need, but explicitly
/// deferred designing a Policy Ledger engine until further levantamiento.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mission {
    pub id: String,
    pub project_path: String,
    pub objective: String,
    #[serde(default)]
    pub scope_statement: String,
    #[serde(default)]
    pub exclusions: Vec<String>,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    #[serde(default)]
    pub client_locale: Option<String>,
    #[serde(default)]
    pub target_markets: Vec<String>,
    #[serde(default)]
    pub delivery_locales: Vec<String>,
    /// Jurisdiction the agency itself operates/contracts from — distinct
    /// from client_locale/target_markets. Added from the Agency Operational
    /// Pattern Survey finding that IP/rights rules depend on the assignor's
    /// jurisdiction, not only the client's market.
    #[serde(default)]
    pub agency_jurisdiction: Option<String>,
    #[serde(default = "default_engagement_regime")]
    pub engagement_regime: EngagementRegime,
    #[serde(default)]
    pub adjustment_budget: Option<String>,
    #[serde(default)]
    pub change_policy_note: Option<String>,
    #[serde(default = "default_mission_status")]
    pub status: MissionStatus,
    #[serde(default)]
    pub approved_by_ncto: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateMissionRequest {
    pub project_path: String,
    pub objective: String,
    #[serde(default)]
    pub scope_statement: String,
    #[serde(default)]
    pub exclusions: Vec<String>,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    #[serde(default)]
    pub client_locale: Option<String>,
    #[serde(default)]
    pub target_markets: Vec<String>,
    #[serde(default)]
    pub delivery_locales: Vec<String>,
    #[serde(default)]
    pub agency_jurisdiction: Option<String>,
    #[serde(default = "default_engagement_regime")]
    pub engagement_regime: EngagementRegime,
    #[serde(default)]
    pub adjustment_budget: Option<String>,
    #[serde(default)]
    pub change_policy_note: Option<String>,
}

/// Full-replace update — deliberately not a partial patch (no
/// double-Option field-by-field merge). Minimal v1: every editable field is
/// resent on every update, same shape as create plus id/status/approval.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMissionRequest {
    pub id: String,
    pub objective: String,
    #[serde(default)]
    pub scope_statement: String,
    #[serde(default)]
    pub exclusions: Vec<String>,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    #[serde(default)]
    pub client_locale: Option<String>,
    #[serde(default)]
    pub target_markets: Vec<String>,
    #[serde(default)]
    pub delivery_locales: Vec<String>,
    #[serde(default)]
    pub agency_jurisdiction: Option<String>,
    #[serde(default = "default_engagement_regime")]
    pub engagement_regime: EngagementRegime,
    #[serde(default)]
    pub adjustment_budget: Option<String>,
    #[serde(default)]
    pub change_policy_note: Option<String>,
    #[serde(default = "default_mission_status")]
    pub status: MissionStatus,
    #[serde(default)]
    pub approved_by_ncto: bool,
}

fn missions_dir(state: &AppState) -> PathBuf {
    state.app_data_dir.join("state").join("missions")
}

fn mission_path(state: &AppState, id: &str) -> PathBuf {
    missions_dir(state).join(format!("{id}.json"))
}

fn validate_text_len(field: &str, s: &str) -> Result<(), AppError> {
    if s.chars().count() > MAX_TEXT_FIELD_CHARS {
        return Err(AppError::InvalidArgument {
            message: format!("{field} exceeds the maximum allowed length"),
        });
    }
    Ok(())
}

async fn persist(state: &AppState, mission: &Mission) -> Result<(), AppError> {
    tokio::fs::create_dir_all(missions_dir(state)).await?;
    let bytes = serde_json::to_vec_pretty(mission)?;
    atomic_write(&mission_path(state, &mission.id), &bytes).await
}

/// Load a Mission by id. Used both by the `mission_get` command and by
/// Runtime v0.1's `runtime_start` when a `StartRunRequest.mission_id` is
/// present.
pub async fn load_mission(state: &AppState, id: &str) -> Result<Mission, AppError> {
    Uuid::parse_str(id).map_err(|_| AppError::InvalidArgument {
        message: "invalid mission id".into(),
    })?;
    let bytes = read_capped(&mission_path(state, id), MAX_MISSION_FILE_BYTES).await?;
    let mission: Mission = serde_json::from_slice(&bytes)?;
    Ok(mission)
}

#[tauri::command]
pub async fn mission_create(
    state: State<'_, AppState>,
    request: CreateMissionRequest,
) -> Result<Mission, AppError> {
    let objective = request.objective.trim();
    if objective.is_empty() {
        return Err(AppError::InvalidArgument {
            message: "objective must not be empty".into(),
        });
    }
    validate_text_len("objective", objective)?;
    let project_path = request.project_path.trim();
    if project_path.is_empty() {
        return Err(AppError::InvalidArgument {
            message: "project_path must not be empty".into(),
        });
    }
    let now = Utc::now();
    let mission = Mission {
        id: Uuid::new_v4().to_string(),
        project_path: project_path.to_string(),
        objective: objective.to_string(),
        scope_statement: request.scope_statement,
        exclusions: request.exclusions,
        acceptance_criteria: request.acceptance_criteria,
        client_locale: request.client_locale,
        target_markets: request.target_markets,
        delivery_locales: request.delivery_locales,
        agency_jurisdiction: request.agency_jurisdiction,
        engagement_regime: request.engagement_regime,
        adjustment_budget: request.adjustment_budget,
        change_policy_note: request.change_policy_note,
        status: MissionStatus::Draft,
        approved_by_ncto: false,
        created_at: now,
        updated_at: now,
    };
    persist(&state, &mission).await?;
    Ok(mission)
}

#[tauri::command]
pub async fn mission_get(
    state: State<'_, AppState>,
    mission_id: String,
) -> Result<Mission, AppError> {
    load_mission(&state, &mission_id).await
}

#[tauri::command]
pub async fn mission_list(
    state: State<'_, AppState>,
    project_path: Option<String>,
) -> Result<Vec<Mission>, AppError> {
    let dir = missions_dir(&state);
    let mut out = Vec::new();
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return Ok(out);
    };
    while let Some(entry) = entries.next_entry().await? {
        if entry.path().extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        if let Ok(bytes) = read_capped(&entry.path(), MAX_MISSION_FILE_BYTES).await {
            if let Ok(mission) = serde_json::from_slice::<Mission>(&bytes) {
                if project_path
                    .as_ref()
                    .is_none_or(|p| p == &mission.project_path)
                {
                    out.push(mission);
                }
            }
        }
    }
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(out)
}

#[tauri::command]
pub async fn mission_update(
    state: State<'_, AppState>,
    request: UpdateMissionRequest,
) -> Result<Mission, AppError> {
    let existing = load_mission(&state, &request.id).await?;
    let objective = request.objective.trim();
    if objective.is_empty() {
        return Err(AppError::InvalidArgument {
            message: "objective must not be empty".into(),
        });
    }
    validate_text_len("objective", objective)?;
    let mission = Mission {
        id: existing.id,
        project_path: existing.project_path,
        objective: objective.to_string(),
        scope_statement: request.scope_statement,
        exclusions: request.exclusions,
        acceptance_criteria: request.acceptance_criteria,
        client_locale: request.client_locale,
        target_markets: request.target_markets,
        delivery_locales: request.delivery_locales,
        agency_jurisdiction: request.agency_jurisdiction,
        engagement_regime: request.engagement_regime,
        adjustment_budget: request.adjustment_budget,
        change_policy_note: request.change_policy_note,
        status: request.status,
        approved_by_ncto: request.approved_by_ncto,
        created_at: existing.created_at,
        updated_at: Utc::now(),
    };
    persist(&state, &mission).await?;
    Ok(mission)
}

/// Deterministic serialization of a Mission into the shared operational
/// context every Factory stage receives. Pure function — same Mission in,
/// same string out, always. No LLM, no agent, no interpretation on this
/// path; this is the Mission Brief mechanism itself, not a description of
/// one. `stage_prompt()` in `runtime.rs` interpolates this in place of the
/// raw free-text intent whenever a run has a Mission attached.
pub fn mission_brief(mission: &Mission) -> String {
    let mut sections = Vec::new();

    sections.push(format!("OBJECTIVE:\n{}", mission.objective));

    if !mission.scope_statement.trim().is_empty() {
        sections.push(format!("SCOPE:\n{}", mission.scope_statement));
    }

    if !mission.exclusions.is_empty() {
        sections.push(format!(
            "EXPLICITLY OUT OF SCOPE (do not implement without a new approved change):\n- {}",
            mission.exclusions.join("\n- ")
        ));
    }

    if !mission.acceptance_criteria.is_empty() {
        sections.push(format!(
            "ACCEPTANCE CRITERIA:\n- {}",
            mission.acceptance_criteria.join("\n- ")
        ));
    }

    let mut locale_lines = Vec::new();
    if let Some(client_locale) = &mission.client_locale {
        locale_lines.push(format!("client locale: {client_locale}"));
    }
    if !mission.target_markets.is_empty() {
        locale_lines.push(format!(
            "target markets: {}",
            mission.target_markets.join(", ")
        ));
    }
    if !mission.delivery_locales.is_empty() {
        locale_lines.push(format!(
            "delivery locales: {}",
            mission.delivery_locales.join(", ")
        ));
    }
    if let Some(jurisdiction) = &mission.agency_jurisdiction {
        locale_lines.push(format!("agency jurisdiction: {jurisdiction}"));
    }
    if !locale_lines.is_empty() {
        sections.push(format!(
            "LOCALE / MARKET CONTEXT:\n{}",
            locale_lines.join("\n")
        ));
    }

    if let Some(budget) = &mission.adjustment_budget {
        sections.push(format!(
            "INCLUDED ADJUSTMENT BUDGET (no formal change needed within this):\n{budget}"
        ));
    }
    if let Some(note) = &mission.change_policy_note {
        sections.push(format!(
            "CHANGE POLICY (anything beyond scope/budget above):\n{note}"
        ));
    }

    sections.join("\n\n")
}

// ---------- Tests ----------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_mission() -> Mission {
        let now = Utc::now();
        Mission {
            id: "11111111-1111-1111-1111-111111111111".into(),
            project_path: "/tmp/project".into(),
            objective: "Build a booking website".into(),
            scope_statement: "Marketing site with a booking form".into(),
            exclusions: vec!["Payment processing".into(), "Checkout".into()],
            acceptance_criteria: vec!["Form submits to the configured inbox".into()],
            client_locale: Some("es-CL".into()),
            target_markets: vec!["CL".into()],
            delivery_locales: vec!["es-CL".into()],
            agency_jurisdiction: Some("CL".into()),
            engagement_regime: EngagementRegime::Fixed,
            adjustment_budget: Some("2 revision rounds per phase".into()),
            change_policy_note: Some(
                "Anything beyond scope requires written approval before work starts".into(),
            ),
            status: MissionStatus::Approved,
            approved_by_ncto: true,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn mission_brief_is_deterministic() {
        let mission = sample_mission();
        assert_eq!(mission_brief(&mission), mission_brief(&mission));
    }

    #[test]
    fn mission_brief_includes_exclusions_and_locale() {
        let brief = mission_brief(&sample_mission());
        assert!(brief.contains("EXPLICITLY OUT OF SCOPE"));
        assert!(brief.contains("Payment processing"));
        assert!(brief.contains("client locale: es-CL"));
        assert!(brief.contains("agency jurisdiction: CL"));
    }

    #[test]
    fn mission_brief_omits_empty_sections() {
        let mut mission = sample_mission();
        mission.exclusions.clear();
        mission.acceptance_criteria.clear();
        mission.client_locale = None;
        mission.target_markets.clear();
        mission.delivery_locales.clear();
        mission.agency_jurisdiction = None;
        mission.adjustment_budget = None;
        mission.change_policy_note = None;
        let brief = mission_brief(&mission);
        assert!(!brief.contains("EXPLICITLY OUT OF SCOPE"));
        assert!(!brief.contains("LOCALE / MARKET CONTEXT"));
        assert!(brief.contains("OBJECTIVE:\nBuild a booking website"));
    }

    #[test]
    fn missing_fields_deserialize_to_defaults() {
        let json = r#"{
            "id": "11111111-1111-1111-1111-111111111111",
            "projectPath": "/tmp/project",
            "objective": "Build a booking website",
            "createdAt": "2026-01-01T00:00:00Z",
            "updatedAt": "2026-01-01T00:00:00Z"
        }"#;
        let mission: Mission =
            serde_json::from_str(json).expect("defaults must fill missing fields");
        assert!(mission.exclusions.is_empty());
        assert_eq!(mission.engagement_regime, EngagementRegime::Fixed);
        assert_eq!(mission.status, MissionStatus::Draft);
        assert!(!mission.approved_by_ncto);
    }
}
