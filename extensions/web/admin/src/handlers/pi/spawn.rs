//! Building a pi child: its workspace, its environment, and its argv.
//!
//! pi ships no sandbox — its tools run with this process's permissions — so
//! everything here is containment rather than convenience. The two loads
//! bearing most weight are `env_clear` (so no gateway or provider credential is
//! inherited) and the `--tools` allowlist (pi itself refuses anything outside
//! it, which is a stronger guarantee than a policy we evaluate after the fact).

use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::process::{Child, Command};

use super::config::PiConfig;

/// Files a fresh workspace starts with, so a read-only agent has something to
/// read. Deliberately tiny and inert: no credentials, no repo.
const SEED_README: &str = "\
# Scratch workspace

This is a throwaway workspace for one governed pi session. Nothing here is
persisted, and the agent that can read it cannot reach the rest of the host.
";

/// Everything a spawned child needs, resolved before the process exists.
pub(super) struct SpawnRequest<'a> {
    pub(super) conversation_id: &'a str,
    /// The server-issued session the gateway attests. Provider spend and
    /// governance rows key on it, which is what ties the two spines together.
    pub(super) attested_session: &'a str,
    pub(super) shim_source: &'a str,
}

pub(super) struct Spawned {
    pub(super) child: Child,
    pub(super) workspace: PathBuf,
}

/// Create the workspace tree and start `pi --mode rpc` inside it.
pub(super) async fn spawn(cfg: &PiConfig, req: &SpawnRequest<'_>) -> std::io::Result<Spawned> {
    let workspace = cfg.workspace_root.join(req.conversation_id);
    let home = workspace.join("home");
    let shim_dir = workspace.join(".pi");

    // A leftover directory from a crashed session must not be inherited: it
    // could carry a previous conversation's files into this one.
    if tokio::fs::try_exists(&workspace).await.unwrap_or(false) {
        tokio::fs::remove_dir_all(&workspace).await?;
    }
    tokio::fs::create_dir_all(home.join(".pi").join("agent")).await?;
    tokio::fs::create_dir_all(&shim_dir).await?;
    tokio::fs::write(workspace.join("README.md"), SEED_README).await?;

    let shim_path = shim_dir.join("governance-shim.ts");
    tokio::fs::write(&shim_path, req.shim_source).await?;

    write_models_json(cfg, &home).await?;
    // quietStartup keeps pi's banner off a stream the widget parses.
    tokio::fs::write(
        home.join(".pi").join("agent").join("settings.json"),
        "{\"quietStartup\":true}",
    )
    .await?;

    let mut cmd = Command::new(&cfg.binary);
    cmd.current_dir(&workspace)
        .arg("--mode")
        .arg("rpc")
        // Only our shim loads. Without this, any extension on the host — or in
        // a discovered project directory — would join the session.
        .arg("--no-extensions")
        .arg("-e")
        .arg(&shim_path)
        .arg("--provider")
        .arg(&cfg.provider)
        .arg("--model")
        .arg(&cfg.model)
        // pi enforces this itself, so a tool outside the set cannot run even if
        // the governance gate were bypassed.
        .arg("--tools")
        .arg(cfg.tools.join(","))
        .arg("--no-session")
        .arg("--no-context-files")
        .arg("--no-skills")
        .arg("--no-prompt-templates")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    // Nothing is inherited: the server's environment holds provider keys and
    // database URLs that a read tool would otherwise reach via /proc/self/environ.
    cmd.env_clear()
        .env("HOME", &home)
        .env("PATH", &cfg.child_path)
        .env("SYSTEMPROMPT_BASE_URL", &cfg.base_url)
        .env("SYSTEMPROMPT_PI_SESSION", req.attested_session)
        .env("SP_PI_CONVERSATION", req.conversation_id)
        // pi phones home for model catalogs on startup otherwise, which a
        // network-restricted deployment would hang on.
        .env("PI_OFFLINE", "1");

    let child = cmd.spawn()?;
    Ok(Spawned { child, workspace })
}

/// Point pi at this deployment's Anthropic-compatible gateway.
///
/// Written per session under the session's own `HOME`, so one conversation
/// cannot read or rewrite another's provider config.
async fn write_models_json(cfg: &PiConfig, home: &Path) -> std::io::Result<()> {
    let models = serde_json::json!({
        "providers": {
            &cfg.provider: {
                "baseUrl": &cfg.base_url,
                "api": "anthropic-messages",
                "apiKey": &cfg.gateway_key,
                "models": [{
                    "id": &cfg.model,
                    "name": "Governed gateway model",
                    "reasoning": true,
                    "input": ["text"],
                    "contextWindow": 200_000,
                    "maxTokens": 8_000,
                }],
            }
        }
    });
    let path = home.join(".pi").join("agent").join("models.json");
    tokio::fs::write(&path, serde_json::to_vec_pretty(&models).unwrap_or_default()).await?;
    restrict(&path).await
}

/// Owner-only, because the file holds the gateway credential and the agent
/// running beside it has a `read` tool.
#[cfg(unix)]
async fn restrict(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).await
}

#[cfg(not(unix))]
async fn restrict(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Remove a finished session's workspace. Best-effort: a failure here is worth
/// a warning but must not keep a dead session in the registry.
pub(super) async fn cleanup(workspace: &Path) {
    if let Err(e) = tokio::fs::remove_dir_all(workspace).await
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(
            workspace = %workspace.display(),
            error = %e,
            "could not remove pi workspace"
        );
    }
}
