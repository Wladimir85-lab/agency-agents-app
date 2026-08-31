//! Temporal durable control plane for IntentOS missions.
//! IntentOS owns mission semantics and gates; Temporal owns replay and recovery.

use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::{str::FromStr, time::Duration};
use temporalio_client::{
    Client, ClientOptions, Connection, ConnectionOptions, Url, WorkflowQueryOptions,
    WorkflowSignalOptions, WorkflowStartOptions,
};
use temporalio_macros::{workflow, workflow_methods};
use temporalio_sdk::{
    Runtime, SyncWorkflowContext, Worker, WorkerOptions, WorkflowContext, WorkflowContextView,
    WorkflowResult,
};

const ENDPOINT: &str = "http://localhost:7233";
const NAMESPACE: &str = "default";
const TASK_QUEUE: &str = "intentos-missions-v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DurableMissionInput {
    pub mission_id: String,
    pub proposal_revision: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DurableMissionDecision {
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DurableMissionSnapshot {
    pub mission_id: String,
    pub proposal_revision: u32,
    pub status: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemporalRuntimeStatus {
    pub configured: bool,
    pub reachable: bool,
    pub endpoint: String,
    pub namespace: String,
    pub task_queue: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemporalMissionRef {
    pub workflow_id: String,
    pub mission_id: String,
    pub status: String,
}

#[workflow]
pub struct IntentOsMissionWorkflow {
    mission_id: String,
    proposal_revision: u32,
    status: String,
    reason: Option<String>,
}

#[workflow_methods]
impl IntentOsMissionWorkflow {
    #[init]
    fn new(_ctx: &WorkflowContextView, input: DurableMissionInput) -> Self {
        Self {
            mission_id: input.mission_id,
            proposal_revision: input.proposal_revision,
            status: "awaitingApproval".into(),
            reason: None,
        }
    }

    #[run(name = "intentos-mission-v1")]
    pub async fn run(ctx: &mut WorkflowContext<Self>) -> WorkflowResult<DurableMissionSnapshot> {
        ctx.wait_condition(|state| state.status != "awaitingApproval")
            .await?;
        Ok(ctx.state(|state| state.as_snapshot()))
    }

    #[signal]
    pub fn approve(
        &mut self,
        _ctx: &mut SyncWorkflowContext<Self>,
        _input: DurableMissionDecision,
    ) {
        if self.status == "awaitingApproval" {
            self.status = "approved".into();
            self.reason = None;
        }
    }

    #[signal]
    pub fn reject(&mut self, _ctx: &mut SyncWorkflowContext<Self>, input: DurableMissionDecision) {
        if self.status == "awaitingApproval" {
            self.status = "rejected".into();
            self.reason = input.reason;
        }
    }

    #[signal]
    pub fn cancel(&mut self, _ctx: &mut SyncWorkflowContext<Self>, input: DurableMissionDecision) {
        if self.status == "awaitingApproval" {
            self.status = "cancelled".into();
            self.reason = input.reason;
        }
    }

    #[query]
    pub fn current_state(&self, _ctx: &WorkflowContextView, _input: ()) -> DurableMissionSnapshot {
        self.as_snapshot()
    }
}

impl IntentOsMissionWorkflow {
    fn as_snapshot(&self) -> DurableMissionSnapshot {
        DurableMissionSnapshot {
            mission_id: self.mission_id.clone(),
            proposal_revision: self.proposal_revision,
            status: self.status.clone(),
            reason: self.reason.clone(),
        }
    }
}

fn internal(context: &str, error: impl std::fmt::Display) -> AppError {
    AppError::Internal {
        message: format!("{context}: {error}"),
    }
}

async fn client() -> Result<Client, AppError> {
    let url = Url::from_str(ENDPOINT).map_err(|e| internal("invalid Temporal endpoint", e))?;
    let options = ConnectionOptions::new(url)
        .identity("intentos-desktop")
        .build();
    let connection = tokio::time::timeout(Duration::from_secs(4), Connection::connect(options))
        .await
        .map_err(|_| internal("Temporal connection timed out", ENDPOINT))?
        .map_err(|e| internal("Temporal connection failed", e))?;
    Client::new(connection, ClientOptions::new(NAMESPACE).build())
        .map_err(|e| internal("Temporal client initialization failed", e))
}

async fn run_worker() -> Result<(), AppError> {
    let runtime = Runtime::new_assume_tokio(Default::default())
        .map_err(|e| internal("Temporal runtime initialization failed", e))?;
    let options = WorkerOptions::new(TASK_QUEUE)
        .register_workflow::<IntentOsMissionWorkflow>()
        .map_err(|e| internal("Temporal workflow registration failed", e))?
        .build();
    let mut worker = Worker::new(&runtime, client().await?, options)
        .map_err(|e| internal("Temporal worker initialization failed", e))?;
    worker
        .run()
        .await
        .map_err(|e| internal("Temporal worker stopped", e))
}

pub fn spawn_worker_supervisor() {
    std::thread::Builder::new()
        .name("intentos-temporal-worker".into())
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("build Temporal supervisor runtime");
            let local = tokio::task::LocalSet::new();
            local.block_on(&runtime, async {
                loop {
                    if let Err(error) = run_worker().await {
                        tracing::warn!(error = %error, "Temporal worker unavailable; retrying");
                        tokio::time::sleep(Duration::from_secs(10)).await;
                    }
                }
            });
        })
        .expect("start IntentOS Temporal worker supervisor");
}

#[tauri::command]
pub async fn temporal_status() -> TemporalRuntimeStatus {
    let result = client().await;
    TemporalRuntimeStatus {
        configured: true,
        reachable: result.is_ok(),
        endpoint: ENDPOINT.into(),
        namespace: NAMESPACE.into(),
        task_queue: TASK_QUEUE.into(),
        message: result
            .map(|_| "Temporal Service disponible; ejecución durable preparada.".into())
            .unwrap_or_else(|e| e.to_string()),
    }
}

fn workflow_id(mission_id: &str) -> String {
    format!("intentos-mission-{}", mission_id.trim())
}

#[tauri::command]
pub async fn temporal_start_mission(
    mission_id: String,
    proposal_revision: u32,
) -> Result<TemporalMissionRef, AppError> {
    if mission_id.trim().is_empty() {
        return Err(AppError::InvalidArgument {
            message: "missionId is required".into(),
        });
    }
    let id = workflow_id(&mission_id);
    client()
        .await?
        .start_workflow(
            IntentOsMissionWorkflow::run,
            DurableMissionInput {
                mission_id: mission_id.trim().into(),
                proposal_revision,
            },
            WorkflowStartOptions::new(TASK_QUEUE, id.clone()).build(),
        )
        .await
        .map_err(|e| internal("Temporal mission start failed", e))?;
    Ok(TemporalMissionRef {
        workflow_id: id,
        mission_id,
        status: "awaitingApproval".into(),
    })
}

#[tauri::command]
pub async fn temporal_mission_status(
    mission_id: String,
) -> Result<DurableMissionSnapshot, AppError> {
    client()
        .await?
        .get_workflow_handle::<IntentOsMissionWorkflow>(workflow_id(&mission_id))
        .query(
            IntentOsMissionWorkflow::current_state,
            (),
            WorkflowQueryOptions::default(),
        )
        .await
        .map_err(|e| internal("Temporal mission query failed", e))
}

#[tauri::command]
pub async fn temporal_approve_mission(
    mission_id: String,
) -> Result<DurableMissionSnapshot, AppError> {
    let handle = client()
        .await?
        .get_workflow_handle::<IntentOsMissionWorkflow>(workflow_id(&mission_id));
    handle
        .signal(
            IntentOsMissionWorkflow::approve,
            DurableMissionDecision { reason: None },
            WorkflowSignalOptions::default(),
        )
        .await
        .map_err(|e| internal("Temporal approval signal failed", e))?;
    wait_for_decision(&handle, "approved").await
}

async fn wait_for_decision(
    handle: &temporalio_client::WorkflowHandle<Client, IntentOsMissionWorkflow>,
    expected: &str,
) -> Result<DurableMissionSnapshot, AppError> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(4);
    loop {
        let snapshot = handle
            .query(
                IntentOsMissionWorkflow::current_state,
                (),
                WorkflowQueryOptions::default(),
            )
            .await
            .map_err(|e| internal("Temporal mission decision query failed", e))?;
        if snapshot.status == expected {
            return Ok(snapshot);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(internal(
                "Temporal mission decision timed out",
                format!("expected {expected}, observed {}", snapshot.status),
            ));
        }
        tokio::time::sleep(Duration::from_millis(75)).await;
    }
}

#[tauri::command]
pub async fn temporal_reject_mission(
    mission_id: String,
    reason: String,
) -> Result<DurableMissionSnapshot, AppError> {
    let handle = client()
        .await?
        .get_workflow_handle::<IntentOsMissionWorkflow>(workflow_id(&mission_id));
    handle
        .signal(
            IntentOsMissionWorkflow::reject,
            DurableMissionDecision {
                reason: Some(reason),
            },
            WorkflowSignalOptions::default(),
        )
        .await
        .map_err(|e| internal("Temporal rejection signal failed", e))?;
    wait_for_decision(&handle, "rejected").await
}

#[tauri::command]
pub async fn temporal_cancel_mission(
    mission_id: String,
    reason: Option<String>,
) -> Result<DurableMissionSnapshot, AppError> {
    let handle = client()
        .await?
        .get_workflow_handle::<IntentOsMissionWorkflow>(workflow_id(&mission_id));
    handle
        .signal(
            IntentOsMissionWorkflow::cancel,
            DurableMissionDecision { reason },
            WorkflowSignalOptions::default(),
        )
        .await
        .map_err(|e| internal("Temporal cancellation signal failed", e))?;
    wait_for_decision(&handle, "cancelled").await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workflow_identity_is_stable() {
        assert_eq!(workflow_id("abc"), "intentos-mission-abc");
    }
    #[test]
    fn status_wire_contract_is_camel_case() {
        let value = serde_json::to_value(TemporalRuntimeStatus {
            configured: true,
            reachable: false,
            endpoint: ENDPOINT.into(),
            namespace: NAMESPACE.into(),
            task_queue: TASK_QUEUE.into(),
            message: "offline".into(),
        })
        .unwrap();
        assert_eq!(value["taskQueue"], TASK_QUEUE);
        assert!(value.get("task_queue").is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires a real Temporal server on localhost:7233"]
    async fn real_server_preserves_all_ncto_decisions_and_recovery_queries() {
        spawn_worker_supervisor();
        tokio::time::sleep(Duration::from_millis(800)).await;

        let approved_id = format!("temporal-smoke-approved-{}", uuid::Uuid::new_v4());
        temporal_start_mission(approved_id.clone(), 7)
            .await
            .expect("start approved mission");
        let approved = temporal_approve_mission(approved_id.clone())
            .await
            .expect("approve mission");
        assert_eq!(approved.status, "approved");
        assert_eq!(approved.proposal_revision, 7);
        let recovered = temporal_mission_status(approved_id)
            .await
            .expect("query closed approved mission");
        assert_eq!(recovered, approved);

        let rejected_id = format!("temporal-smoke-rejected-{}", uuid::Uuid::new_v4());
        temporal_start_mission(rejected_id.clone(), 8)
            .await
            .expect("start rejected mission");
        let rejected = temporal_reject_mission(rejected_id, "Cambiar alcance".into())
            .await
            .expect("reject mission");
        assert_eq!(rejected.status, "rejected");
        assert_eq!(rejected.reason.as_deref(), Some("Cambiar alcance"));

        let cancelled_id = format!("temporal-smoke-cancelled-{}", uuid::Uuid::new_v4());
        temporal_start_mission(cancelled_id.clone(), 9)
            .await
            .expect("start cancelled mission");
        let cancelled = temporal_cancel_mission(cancelled_id, Some("Prueba controlada".into()))
            .await
            .expect("cancel mission");
        assert_eq!(cancelled.status, "cancelled");
        assert_eq!(cancelled.reason.as_deref(), Some("Prueba controlada"));
    }
}
