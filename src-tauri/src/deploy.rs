//! Public preview — publishes the Showroom's already-running local dev
//! server through a Cloudflare "quick tunnel" (`cloudflared tunnel --url`),
//! so it's reachable from a phone, or by someone who isn't on the same
//! machine, without exposing the developer's own network. Fulfils the
//! `PublicPreviewPublisher` contract reserved (as `active_binding: "none"`)
//! in `fabric.rs` until this module existed.
//!
//! Deliberately narrow in scope: `public_preview_start` never accepts a
//! URL from the caller. It only ever tunnels whatever `preview.rs`'s own
//! `preview_process.url` already is — the loopback Showroom already
//! running — so this command can never be pointed at an arbitrary host.
//!
//! `cloudflared` is not bundled with IntentOS: the first time this is
//! used, IntentOS downloads one specific, pinned release from Cloudflare's
//! official GitHub releases, verifies its SHA-256 against a hash checked
//! into this file, and refuses to run anything that doesn't match byte for
//! byte. Cloudflare does not publish a signature scheme for third-party
//! verification of `cloudflared` releases the way IntentOS's own updater
//! verifies itself (minisign) — pinned version + pinned hash, verified
//! before every single execution (not just once after download), is the
//! honest equivalent here. Each hash below was computed by hand this
//! session against the real release asset (`sha256sum`), not copied from
//! anywhere unverified — see the delivered report for how.
//!
//! "Quick tunnels" need no Cloudflare account or token: `cloudflared`
//! opens an outbound-only connection to Cloudflare's edge and gets back a
//! random `*.trycloudflare.com` hostname, no signup involved. That also
//! means it is anonymous but not private — anyone who has the URL can
//! reach the tunnelled Showroom while it's up, and Cloudflare's edge is a
//! real intermediary the same way any CDN is. This was explained to, and
//! approved by, the user before this module was written.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{ipc::Channel, State};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use crate::error::AppError;
use crate::state::AppState;

/// Pinned exactly — never "latest". A newer cloudflared release is only
/// adopted by deliberately updating this constant and its hashes below,
/// the same discipline `local_model.rs` documents for the managed model
/// path.
const CLOUDFLARED_VERSION: &str = "2026.8.3";
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(180);

pub struct RunningPublicPreview {
    child: Child,
    local_url: String,
    url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PublicPreviewEvent {
    Preparing,
    Starting,
    Ready { url: String },
    Log { text: String },
    Stopped,
    Failed { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PublicPreviewStatus {
    pub running: bool,
    pub local_url: Option<String>,
    pub url: Option<String>,
}

struct PlatformAsset {
    file_name: &'static str,
    /// `sha256sum` of the exact asset above, computed by hand against the
    /// real GitHub release this session.
    sha256: &'static str,
    /// `true` for the macOS `.tgz` releases (a `cloudflared` file at the
    /// archive root); `false` where the asset is already the executable.
    archive: bool,
}

fn platform_asset() -> Result<PlatformAsset, AppError> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Ok(PlatformAsset {
            file_name: "cloudflared-windows-amd64.exe",
            sha256: "83e726ed18ea78c5ad5213c4c3a3a27051393950d2bc8ed4de69bec12d14eaae",
            archive: false,
        }),
        ("macos", "x86_64") => Ok(PlatformAsset {
            file_name: "cloudflared-darwin-amd64.tgz",
            sha256: "61e1316266a00fd70ce40da011d612badc805367fb65293dd1925f938f704c99",
            archive: true,
        }),
        ("macos", "aarch64") => Ok(PlatformAsset {
            file_name: "cloudflared-darwin-arm64.tgz",
            sha256: "40c9144d86df8937c5b43293a1f7d2d2107029aa74725023dd46b1b27154352f",
            archive: true,
        }),
        ("linux", "x86_64") => Ok(PlatformAsset {
            file_name: "cloudflared-linux-amd64",
            sha256: "f29324fe934d1e100617484c78deef803c4dc2cd351d645bbde42e96b4fccc5e",
            archive: false,
        }),
        (os, arch) => Err(AppError::InvalidArgument {
            message: format!(
                "vista previa pública no soportada en {os}/{arch}: no hay una versión de cloudflared verificada para esta plataforma"
            ),
        }),
    }
}

fn managed_dir(app_data: &Path) -> PathBuf {
    app_data
        .join("state")
        .join("cloudflared")
        .join(CLOUDFLARED_VERSION)
}

fn managed_binary_path(app_data: &Path) -> PathBuf {
    let name = if cfg!(windows) {
        "cloudflared.exe"
    } else {
        "cloudflared"
    };
    managed_dir(app_data).join(name)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Downloads + verifies cloudflared if IntentOS doesn't already have a
/// verified copy of this exact pinned version. Re-verifies the hash of an
/// already-present binary too (cheap: hashing ~20-55MB is sub-second) — a
/// file that was modified on disk after IntentOS wrote it is treated as
/// absent, never executed.
async fn ensure_cloudflared(
    state: &AppState,
    on_event: &Channel<PublicPreviewEvent>,
) -> Result<PathBuf, AppError> {
    let asset = platform_asset()?;
    let binary_path = managed_binary_path(&state.app_data_dir);

    if binary_path.is_file() {
        let bytes = tokio::fs::read(&binary_path).await?;
        if sha256_hex(&bytes) == asset.sha256 {
            return Ok(binary_path);
        }
        let _ = tokio::fs::remove_file(&binary_path).await;
    }

    // Downloading an executable from the internet is exactly what
    // Paranoid Mode exists to fail closed on — see state.rs's documented
    // "every outbound command must call this first" contract.
    state.require_network("publicPreview").await?;
    let _ = on_event.send(PublicPreviewEvent::Preparing);

    let url = format!(
        "https://github.com/cloudflare/cloudflared/releases/download/{CLOUDFLARED_VERSION}/{}",
        asset.file_name
    );
    let client = reqwest::Client::builder().timeout(DOWNLOAD_TIMEOUT).build()?;
    let response = client.get(&url).send().await?;
    if !response.status().is_success() {
        return Err(AppError::HttpStatus {
            url,
            status: response.status().as_u16(),
        });
    }
    let bytes = response.bytes().await?.to_vec();
    let actual = sha256_hex(&bytes);
    if actual != asset.sha256 {
        return Err(AppError::Internal {
            message: format!(
                "la descarga de cloudflared no coincide con el hash esperado (esperado {}, obtenido {actual}); IntentOS se niega a ejecutarla",
                asset.sha256
            ),
        });
    }

    let dir = managed_dir(&state.app_data_dir);
    tokio::fs::create_dir_all(&dir).await?;
    if asset.archive {
        extract_cloudflared_from_tgz(&bytes, &binary_path).await?;
    } else {
        tokio::fs::write(&binary_path, &bytes).await?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = tokio::fs::metadata(&binary_path).await?.permissions();
        perms.set_mode(0o755);
        tokio::fs::set_permissions(&binary_path, perms).await?;
    }

    Ok(binary_path)
}

async fn extract_cloudflared_from_tgz(bytes: &[u8], destination: &Path) -> Result<(), AppError> {
    let bytes = bytes.to_vec();
    let destination = destination.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let decoder = flate2::read::GzDecoder::new(&bytes[..]);
        let mut archive = tar::Archive::new(decoder);
        for entry in archive.entries()? {
            let mut entry = entry?;
            let is_cloudflared = entry
                .path()?
                .file_name()
                .map(|n| n == "cloudflared")
                .unwrap_or(false);
            if is_cloudflared {
                let mut file = std::fs::File::create(&destination)?;
                std::io::copy(&mut entry, &mut file)?;
                return Ok(());
            }
        }
        Err(AppError::Internal {
            message: "el binario 'cloudflared' no está dentro del archivo descargado".into(),
        })
    })
    .await
    .map_err(|e| AppError::Internal {
        message: e.to_string(),
    })?
}

/// Matches the `https://*.trycloudflare.com` line cloudflared prints once
/// the quick tunnel is live (inside a box-drawn banner on stderr).
fn find_tunnel_url(line: &str) -> Option<String> {
    let start = line.find("https://")?;
    let rest = &line[start..];
    let end = rest
        .find(|c: char| c.is_whitespace() || c == '|')
        .unwrap_or(rest.len());
    let candidate = rest[..end].trim_end_matches('/').to_string();
    candidate.contains(".trycloudflare.com").then_some(candidate)
}

/// The `host:port` cloudflared should present to the local origin as the
/// `Host` header, via `--http-host-header`. Without this, a request that
/// arrives at the Showroom's dev server carrying the tunnel's own
/// `*.trycloudflare.com` `Host` header gets rejected outright: Vite (and
/// everything built on it — Astro, SvelteKit, every dev-script shape
/// `preview.rs` actually launches) refuses any `Host` not in its
/// `server.allowedHosts` allowlist as a DNS-rebinding protection, and
/// IntentOS has no business relaxing that allowlist inside a generated
/// project just to make tunnelling convenient. Rewriting the header back
/// to what the dev server already trusts (its own `local_url`) satisfies
/// that check without touching the Showroom's own config at all. Found by
/// hand this session: the very first real quick tunnel against a real
/// Astro Showroom 403'd with exactly this message before this existed.
fn local_host_header(local_url: &str) -> Option<&str> {
    local_url
        .strip_prefix("http://")
        .or_else(|| local_url.strip_prefix("https://"))
}

#[tauri::command]
pub async fn public_preview_start(
    state: State<'_, AppState>,
    on_event: Channel<PublicPreviewEvent>,
) -> Result<PublicPreviewStatus, AppError> {
    // Never a caller-supplied URL — only ever whatever the Showroom itself
    // is already serving locally. See the module doc comment.
    let local_url = {
        let guard = state.preview_process.lock().await;
        guard
            .as_ref()
            .and_then(|running| running.url.clone())
            .ok_or_else(|| AppError::InvalidArgument {
                message: "El Showroom debe estar corriendo (con una URL local lista) antes de publicar una vista previa pública.".into(),
            })?
    };

    {
        let mut guard = state.public_preview_process.lock().await;
        if let Some(running) = guard.as_mut() {
            if running.child.try_wait()?.is_none() {
                return Err(AppError::InvalidArgument {
                    message: "Ya hay una vista previa pública activa; detenla antes de iniciar otra.".into(),
                });
            }
        }
    }

    let binary = ensure_cloudflared(&state, &on_event).await?;
    let _ = on_event.send(PublicPreviewEvent::Starting);

    let mut command = Command::new(&binary);
    command
        .args(["tunnel", "--no-autoupdate", "--url", &local_url]);
    if let Some(host_header) = local_host_header(&local_url) {
        command.args(["--http-host-header", host_header]);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn()?;
    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");

    {
        let mut guard = state.public_preview_process.lock().await;
        *guard = Some(RunningPublicPreview {
            child,
            local_url: local_url.clone(),
            url: None,
        });
    }

    // cloudflared prints its banner (including the tunnel URL) on stderr;
    // stdout carries connection/request logs. Both are surfaced as Log
    // events so the UI can show real activity, not a silent black box.
    let public_preview_process = state.public_preview_process.clone();
    let events = on_event.clone();
    let local_url_for_task = local_url.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(text)) => {
                    let _ = events.send(PublicPreviewEvent::Log { text: text.clone() });
                    if let Some(url) = find_tunnel_url(&text) {
                        let mut guard = public_preview_process.lock().await;
                        if let Some(running) = guard.as_mut() {
                            if running.local_url == local_url_for_task {
                                running.url = Some(url.clone());
                            }
                        }
                        drop(guard);
                        let _ = events.send(PublicPreviewEvent::Ready { url });
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    let _ = events.send(PublicPreviewEvent::Failed {
                        reason: e.to_string(),
                    });
                    break;
                }
            }
        }
        let _ = events.send(PublicPreviewEvent::Stopped);
    });

    let stdout_events = on_event.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(text)) = lines.next_line().await {
            let _ = stdout_events.send(PublicPreviewEvent::Log { text });
        }
    });

    Ok(PublicPreviewStatus {
        running: true,
        local_url: Some(local_url),
        url: None,
    })
}

#[tauri::command]
pub async fn public_preview_stop(
    state: State<'_, AppState>,
) -> Result<PublicPreviewStatus, AppError> {
    kill_public_preview(&state).await;
    Ok(PublicPreviewStatus {
        running: false,
        local_url: None,
        url: None,
    })
}

/// Shared by the `public_preview_stop` command and the app's window-close
/// handler (see lib.rs) — cloudflared itself doesn't spawn a further
/// child process tree the way the Showroom's `cmd /C npm run dev` does,
/// but it's tree-killed the same way regardless: cheap, and consistent
/// with `preview.rs`'s own hard-won fix for exactly this class of bug.
pub(crate) async fn kill_public_preview(state: &AppState) {
    let mut guard = state.public_preview_process.lock().await;
    if let Some(mut running) = guard.take() {
        #[cfg(windows)]
        {
            if let Some(pid) = running.child.id() {
                let _ = Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/T", "/F"])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .await;
            } else {
                let _ = running.child.kill().await;
            }
        }
        #[cfg(not(windows))]
        {
            let _ = running.child.kill().await;
        }
    }
}

#[tauri::command]
pub async fn public_preview_status(
    state: State<'_, AppState>,
) -> Result<PublicPreviewStatus, AppError> {
    let guard = state.public_preview_process.lock().await;
    Ok(match guard.as_ref() {
        Some(running) => PublicPreviewStatus {
            running: true,
            local_url: Some(running.local_url.clone()),
            url: running.url.clone(),
        },
        None => PublicPreviewStatus {
            running: false,
            local_url: None,
            url: None,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state(app_data: &Path) -> AppState {
        AppState {
            app_data_dir: app_data.to_path_buf(),
            corpus_cache: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
            corpus_refresh_in_flight: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            settings: std::sync::Arc::new(tokio::sync::RwLock::new(
                crate::commands::settings::SettingsLoadState::FirstLaunch,
            )),
            updater_state: crate::commands::updater::empty_state(),
            runtime_jobs: std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            local_model_process: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
            preview_process: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
            public_preview_process: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    // Real network + real GitHub release, not mocked — proves the pinned
    // version/hash actually download and verify against the live release,
    // and that the resulting binary is genuinely executable, not just a
    // file that happens to exist. Ignored by default (needs network):
    //   cargo test --lib deploy::tests::ensure_cloudflared_downloads_and_verifies_the_real_release -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn ensure_cloudflared_downloads_and_verifies_the_real_release() {
        let app_data = tempfile::tempdir().unwrap();
        let state = test_state(app_data.path());
        let channel = Channel::new(|_| Ok(()));

        let path = ensure_cloudflared(&state, &channel)
            .await
            .expect("ensure_cloudflared should download and verify the real pinned release");
        assert!(path.is_file(), "managed cloudflared binary should exist at {path:?}");

        let output = Command::new(&path)
            .arg("--version")
            .output()
            .await
            .expect("the downloaded binary should actually be executable");
        let version_text = String::from_utf8_lossy(&output.stdout);
        println!("cloudflared --version -> {version_text}");
        assert!(output.status.success(), "cloudflared --version should exit 0");
        assert!(
            version_text.contains(CLOUDFLARED_VERSION),
            "expected the pinned version {CLOUDFLARED_VERSION} in output: {version_text}"
        );

        // Re-running against an already-verified file must be a no-op read,
        // not a second download — same file path, same content.
        let path2 = ensure_cloudflared(&state, &channel).await.unwrap();
        assert_eq!(path, path2);
    }

    #[test]
    fn finds_tunnel_url_inside_a_box_drawn_banner_line() {
        assert_eq!(
            find_tunnel_url("2026-01-01T00:00:00Z INF |  https://random-two-words.trycloudflare.com  |"),
            Some("https://random-two-words.trycloudflare.com".to_string())
        );
    }

    #[test]
    fn ignores_lines_without_a_trycloudflare_url() {
        assert_eq!(find_tunnel_url("2026-01-01T00:00:00Z INF Registered tunnel connection"), None);
        assert_eq!(
            find_tunnel_url("some https://example.com unrelated link"),
            None
        );
    }

    #[test]
    fn local_host_header_strips_the_scheme() {
        assert_eq!(local_host_header("http://localhost:4321"), Some("localhost:4321"));
        assert_eq!(local_host_header("http://127.0.0.1:1430"), Some("127.0.0.1:1430"));
        assert_eq!(local_host_header("https://localhost:3000"), Some("localhost:3000"));
    }

    #[test]
    fn sha256_hex_matches_a_known_vector() {
        // "abc" — a standard NIST test vector, independent of any
        // cloudflared asset, just proving the hashing plumbing itself.
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn every_supported_platform_has_a_64_character_lowercase_hex_hash() {
        for (file_name, sha256) in [
            (
                "cloudflared-windows-amd64.exe",
                "83e726ed18ea78c5ad5213c4c3a3a27051393950d2bc8ed4de69bec12d14eaae",
            ),
            (
                "cloudflared-darwin-amd64.tgz",
                "61e1316266a00fd70ce40da011d612badc805367fb65293dd1925f938f704c99",
            ),
            (
                "cloudflared-darwin-arm64.tgz",
                "40c9144d86df8937c5b43293a1f7d2d2107029aa74725023dd46b1b27154352f",
            ),
            (
                "cloudflared-linux-amd64",
                "f29324fe934d1e100617484c78deef803c4dc2cd351d645bbde42e96b4fccc5e",
            ),
        ] {
            assert_eq!(sha256.len(), 64, "{file_name} hash must be 64 hex chars");
            assert!(
                sha256.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
                "{file_name} hash must be lowercase hex"
            );
        }
    }
}
