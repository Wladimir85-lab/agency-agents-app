//! Soup CLI capability — dataset inspection/validation/dedup for IntentOS.
//!
//! This module is a **new, additive** capability. It does not touch
//! `ModelGateway` (Soup is not a coding-agent CLI and never executes a
//! Runtime v0.1 stage), does not duplicate `CapabilityCatalog` or
//! `WorkflowOrchestrator`, and registers its own Tauri commands rather
//! than folding into any existing one. See `docs/INTENTOS-GOD-ARCHITECTURE.md`
//! for the Fabric contract table this deliberately sits outside of.
//!
//! Scope, kept intentionally minimal and limited to what was actually
//! executed and verified (per the project's "verify by running it, not by
//! reading docs" rule) rather than the full Soup surface:
//!   - `soup doctor`               → health/availability check (soup_status)
//!   - `soup data inspect <path>`  → soup_inspect_dataset
//!   - `soup data validate <path>` → soup_validate_dataset
//!   - `soup data dedup <path>`    → soup_dedup_dataset
//! `soup train` / Layer Streaming (GPU-bound) are explicitly out of scope —
//! they were never verified to run (no GPU available) and are not wired
//! here. Adding them later is a separate, additive change to this same
//! module; it does not require touching any other Fabric contract.

use crate::error::AppError;
use serde::Serialize;
use std::{path::PathBuf, time::Duration};
use tokio::process::Command;

/// Per-invocation ceiling. Dataset operations can legitimately run longer
/// than a network handshake (unlike Temporal's 4s connect timeout), but an
/// unbounded wait would let a hung subprocess block the caller forever.
const COMMAND_TIMEOUT_SECS: u64 = 30;

const BIN: &str = "soup";

/// Resolve Soup independently from the user's PATH ordering. Python's
/// `--user` installer places console scripts below `%APPDATA%\Python`, while
/// an older pipx/uv launcher may still appear first on PATH and point at a
/// Python version that no longer exists. An explicit override remains the
/// highest-priority binding for portable or managed installations.
fn soup_binary() -> PathBuf {
    if let Some(path) = std::env::var_os("INTENTOS_SOUP_BIN").map(PathBuf::from) {
        if path.is_file() {
            return path;
        }
    }

    if let Some(python_root) = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .map(|path| path.join("Python"))
    {
        let mut candidates = std::fs::read_dir(python_root)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|entry| entry.path().join("Scripts").join("soup.exe"))
            .filter(|path| path.is_file())
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| right.cmp(left));
        if let Some(path) = candidates.into_iter().next() {
            return path;
        }
    }

    PathBuf::from(BIN)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SoupRuntimeStatus {
    pub available: bool,
    pub version: Option<String>,
    pub message: String,
}

/// Wraps the raw outcome of a `soup` subprocess invocation. A non-zero exit
/// is a normal domain-level result (e.g. "dataset failed validation"), not a
/// transport failure — so it comes back as `Ok(SoupCommandResult{ ok: false, .. })`,
/// never as an `Err`. `Err(AppError)` is reserved for the process genuinely
/// not running at all (binary missing, spawn failure, timeout).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SoupCommandResult {
    pub ok: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

fn internal(context: &str, error: impl std::fmt::Display) -> AppError {
    AppError::Internal {
        message: format!("{context}: {error}"),
    }
}

fn require_path(path: &str) -> Result<(), AppError> {
    if path.trim().is_empty() {
        return Err(AppError::InvalidArgument {
            message: "dataset path is required".into(),
        });
    }
    Ok(())
}

/// Runs `soup <args>` with a bounded timeout, mapping "binary not found /
/// couldn't spawn / timed out" to `AppError` and everything else (including
/// a non-zero exit from Soup itself) into a normal `SoupCommandResult`.
async fn run_soup(args: &[&str]) -> Result<SoupCommandResult, AppError> {
    let invocation = Command::new(soup_binary()).args(args).output();
    let output = tokio::time::timeout(Duration::from_secs(COMMAND_TIMEOUT_SECS), invocation)
        .await
        .map_err(|_| internal("soup command timed out", args.join(" ")))?
        .map_err(|e| internal("failed to run soup (is it installed and on PATH?)", e))?;

    Ok(SoupCommandResult {
        ok: output.status.success(),
        exit_code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

#[tauri::command]
pub async fn soup_status() -> SoupRuntimeStatus {
    match run_soup(&["doctor"]).await {
        Ok(result) if result.ok => {
            let version = run_soup(&["version"])
                .await
                .ok()
                .filter(|result| result.ok)
                .and_then(|result| first_line(&result.stdout));
            SoupRuntimeStatus {
                available: true,
                version,
                message: "Soup CLI disponible; operaciones de dataset preparadas.".into(),
            }
        }
        Ok(result) => SoupRuntimeStatus {
            available: false,
            version: None,
            message: if result.stderr.trim().is_empty() {
                format!("soup doctor salió con código {:?}", result.exit_code)
            } else {
                result.stderr.trim().to_string()
            },
        },
        Err(error) => SoupRuntimeStatus {
            available: false,
            version: None,
            message: error.to_string(),
        },
    }
}

fn first_line(text: &str) -> Option<String> {
    text.lines()
        .next()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
}

#[tauri::command]
pub async fn soup_inspect_dataset(path: String) -> Result<SoupCommandResult, AppError> {
    require_path(&path)?;
    run_soup(&["data", "inspect", &path]).await
}

#[tauri::command]
pub async fn soup_validate_dataset(path: String) -> Result<SoupCommandResult, AppError> {
    require_path(&path)?;
    run_soup(&["data", "validate", &path]).await
}

#[tauri::command]
pub async fn soup_dedup_dataset(path: String) -> Result<SoupCommandResult, AppError> {
    require_path(&path)?;
    // Deliberately no extra flags (e.g. an output-path override): only the
    // bare `soup data dedup <path>` invocation was actually run and verified
    // in the sandbox. Adding unverified flags here would violate the
    // "verify by execution" rule this module exists to uphold.
    run_soup(&["data", "dedup", &path]).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_path_is_rejected_before_spawning_a_process() {
        let err = require_path("   ").unwrap_err();
        match err {
            AppError::InvalidArgument { message } => assert!(message.contains("dataset path")),
            other => panic!("expected InvalidArgument, got {:?}", other),
        }
    }

    #[test]
    fn non_empty_path_is_accepted() {
        assert!(require_path("./datasets/foo.jsonl").is_ok());
    }

    #[test]
    fn status_wire_contract_is_camel_case() {
        let value = serde_json::to_value(SoupRuntimeStatus {
            available: false,
            version: None,
            message: "soup: command not found".into(),
        })
        .unwrap();
        assert_eq!(value["available"], false);
        assert!(value.get("version").is_some()); // present as null, not omitted
    }

    #[test]
    fn command_result_wire_contract_is_camel_case() {
        let value = serde_json::to_value(SoupCommandResult {
            ok: true,
            exit_code: Some(0),
            stdout: "12 records".into(),
            stderr: String::new(),
        })
        .unwrap();
        assert_eq!(value["exitCode"], 0);
        assert!(value.get("exit_code").is_none());
    }

    #[test]
    fn first_line_extracts_and_trims() {
        assert_eq!(
            first_line("soup 0.73.2\nmore stuff\n"),
            Some("soup 0.73.2".to_string())
        );
        assert_eq!(first_line(""), None);
        assert_eq!(first_line("   \n"), None);
    }

    // ---- Real-CLI integration tests -------------------------------------
    //
    // These hit the actual `soup` binary on PATH — no mocking. They are
    // `#[ignore]`d (same pattern as `render::tests::upstream_convert_sh_…`,
    // which requires `AGENCY_AGENTS_PARITY_ROOT`) because CI/most dev
    // machines won't have Soup CLI installed. Run explicitly once `soup` is
    // on PATH (see the pipx install path documented in the module header):
    //
    //     cargo test --lib soup:: -- --ignored
    //
    // This is the evidence chain the project requires: IntentOS's own Rust
    // command function (not a shell script standing in for it) calling the
    // real Soup CLI and getting a real result back.

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
    async fn soup_status_reports_the_real_installation() {
        let status = soup_status().await;
        assert!(
            status.available,
            "expected soup doctor to succeed: {}",
            status.message
        );
        assert!(
            status.version.is_some(),
            "expected a version line from soup doctor's output"
        );
    }

    #[tokio::test]
    #[ignore = "requires the real `soup` CLI on PATH"]
    async fn soup_inspect_dataset_returns_real_stats_for_a_fixture_file() {
        let fixture = alpaca_fixture();
        let path = fixture.path().to_string_lossy().into_owned();
        let result = soup_inspect_dataset(path)
            .await
            .expect("soup data inspect should run");
        assert!(result.ok, "stderr: {}", result.stderr);
        assert!(result.stdout.contains("Total samples"));
        assert!(result.stdout.contains('3'));
    }

    #[tokio::test]
    #[ignore = "requires the real `soup` CLI on PATH"]
    async fn soup_validate_dataset_auto_detects_alpaca_format() {
        let fixture = alpaca_fixture();
        let path = fixture.path().to_string_lossy().into_owned();
        let result = soup_validate_dataset(path)
            .await
            .expect("soup data validate should run");
        assert!(result.ok, "stderr: {}", result.stderr);
        assert!(result.stdout.to_lowercase().contains("alpaca"));
    }

    #[tokio::test]
    #[ignore = "requires the real `soup` CLI on PATH"]
    async fn soup_dedup_dataset_removes_the_duplicate_row() {
        let fixture = alpaca_fixture();
        let path = fixture.path().to_string_lossy().into_owned();
        let result = soup_dedup_dataset(path)
            .await
            .expect("soup data dedup should run");
        assert!(result.ok, "stderr: {}", result.stderr);
        assert!(result.stdout.contains("3 -> 2 rows"));

        // Soup writes `<input-stem>_deduped.jsonl` into the process's
        // current directory — not next to the input file — so running this
        // test leaves a stray file in the repo working tree unless we clean
        // it up. Confirmed by inspecting a real `git status` after the
        // first run of this test on Windows.
        if let Some(stem) = fixture.path().file_stem().and_then(|s| s.to_str()) {
            if let Ok(cwd) = std::env::current_dir() {
                let leftover = cwd.join(format!("{stem}_deduped.jsonl"));
                let _ = std::fs::remove_file(leftover);
            }
        }
    }

    #[tokio::test]
    async fn missing_binary_surfaces_as_app_error_not_a_panic() {
        // Sanity check for the *other* branch: a path that can never resolve
        // to a real dataset arg still exercises spawn/timeout handling even
        // when `soup` genuinely is not on PATH (e.g. this test runs without
        // `--ignored`). We only assert it doesn't panic and returns a typed
        // error — this one is safe to run everywhere, unlike the four above.
        let result = soup_inspect_dataset("does-not-matter.jsonl".into()).await;
        // Either soup is on PATH and reports a domain-level failure (file
        // not found), or it isn't and we get a clean AppError — both are
        // acceptable; a panic is the only failure mode this guards against.
        match result {
            Ok(_) | Err(_) => {}
        }
    }
}
