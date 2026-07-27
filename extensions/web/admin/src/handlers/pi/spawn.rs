//! Building a pi child: its workspace, its environment, and its argv.
//!
//! pi ships no sandbox — its tools run with whatever permissions it is given —
//! so everything here is containment rather than convenience. Three loads bear
//! most of the weight: `env_clear` (so no gateway or provider credential is
//! inherited), the `--tools` allowlist (pi itself refuses anything outside it,
//! which is stronger than a policy evaluated after the fact), and the
//! `sp-pi-jail` wrapper, which applies a Landlock ruleset to itself and then
//! `exec`s pi — so the child's filesystem view really is the workspace plus a
//! read-only interpreter, rather than everything this uid can open.
//!
//! Argv is layered outside-in, and the order is load-bearing: `sh` sets the
//! ulimits, `sp-pi-jail` applies the ruleset, pi runs. Both wrappers `exec`,
//! so there is still exactly one process, and Landlock survives `execve` by
//! design — a ruleset applied before the last `exec` cannot be dropped after.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::process::{Child, Command};

use super::config::{ChildLimits, PiConfig, SandboxMode};

const SEED_README: &str = "\
# Scratch workspace

This is a throwaway workspace for one governed pi session. Nothing here is
persisted; it is deleted when the session ends.

The agent reading this runs inside a Landlock ruleset. This directory is the
only path it can write, and the only path outside a read-only interpreter and
its shared libraries that it can read at all — the host's configuration,
credentials, and other sessions' workspaces are not reachable from here. Its
outbound network is confined to the governed gateway's port.
";

const SEED_README_TRANSCRIPT: &str = "
## TRANSCRIPT.md

This conversation is a continuation of an earlier one. `TRANSCRIPT.md` in this
directory holds what was already said. Read it when the user refers to something
you have no record of; you do not need it to answer an ordinary follow-up.
";

const TRANSCRIPT_FILE: &str = "TRANSCRIPT.md";

pub(super) struct SpawnRequest<'a> {
    pub(super) conversation_id: &'a str,
    pub(super) attested_session: &'a str,
    pub(super) gateway_key: &'a str,
    pub(super) shim_source: &'a str,
    pub(super) mcp_client_source: &'a str,
    pub(super) mcp_token: &'a str,
    pub(super) transcript: Option<&'a str>,
}

pub(super) struct Spawned {
    pub(super) child: Child,
    pub(super) workspace: PathBuf,
}

pub(super) async fn spawn(cfg: &PiConfig, req: &SpawnRequest<'_>) -> std::io::Result<Spawned> {
    let workspace = cfg.workspace_root.join(req.conversation_id);
    let home = workspace.join("home");
    let shim_dir = workspace.join(".pi");

    if tokio::fs::try_exists(&workspace).await.unwrap_or(false) {
        tokio::fs::remove_dir_all(&workspace).await?;
    }
    tokio::fs::create_dir_all(home.join(".pi").join("agent")).await?;
    tokio::fs::create_dir_all(&shim_dir).await?;
    let readme = match req.transcript {
        Some(transcript) => {
            tokio::fs::write(workspace.join(TRANSCRIPT_FILE), transcript).await?;
            format!("{SEED_README}{SEED_README_TRANSCRIPT}")
        },
        None => SEED_README.to_owned(),
    };
    tokio::fs::write(workspace.join("README.md"), readme).await?;

    let shim_path = shim_dir.join("governance-shim.ts");
    tokio::fs::write(&shim_path, req.shim_source).await?;

    let mcp_client_path = shim_dir.join("mcp-client.ts");
    tokio::fs::write(&mcp_client_path, req.mcp_client_source).await?;

    let skills_dir = super::skills::materialise(&workspace).await;

    write_models_json(cfg, req.gateway_key, &home).await?;
    tokio::fs::write(
        home.join(".pi").join("agent").join("settings.json"),
        "{\"quietStartup\":true,\"enableSkillCommands\":true}",
    )
    .await?;

    let mut cmd = limited_command(cfg, &workspace)?;
    cmd.current_dir(&workspace)
        .arg("--mode")
        .arg("rpc")
        .arg("--no-extensions")
        .arg("-e")
        .arg(&shim_path)
        .arg("-e")
        .arg(&mcp_client_path)
        .arg("--provider")
        .arg(&cfg.provider)
        .arg("--model")
        .arg(&cfg.model)
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

    if let Some(dir) = &skills_dir {
        cmd.arg("--skill").arg(dir);
    }

    // Why: `--system-prompt` would replace pi's own, whose tool-use half the
    // RPC plumbing and approval flow depend on.
    if !cfg.persona.trim().is_empty() {
        cmd.arg("--append-system-prompt").arg(&cfg.persona);
    }

    // Why: the server's environment holds provider keys and database URLs a
    // read tool would otherwise reach through /proc/self/environ.
    cmd.env_clear()
        .env("HOME", &home)
        .env("PATH", &cfg.child_path)
        .env("SYSTEMPROMPT_BASE_URL", &cfg.base_url)
        .env("SYSTEMPROMPT_PI_SESSION", req.attested_session)
        .env("SP_PI_CONVERSATION", req.conversation_id)
        .env("SP_PI_MCP_TOKEN", req.mcp_token)
        .env("PI_OFFLINE", "1")
        // Why: jiti caches transpiled extensions to /tmp/jiti/, which Landlock
        // denies; the failed open() takes down every extension including the
        // governance shim, and jiti ignores JITI_CACHE as a path here.
        .env("JITI_CACHE", "false");

    let child = cmd.spawn()?;
    Ok(Spawned { child, workspace })
}

fn limited_command(cfg: &PiConfig, workspace: &Path) -> std::io::Result<Command> {
    let mut argv: Vec<String> = Vec::new();
    if cfg.sandbox == SandboxMode::Required {
        if !cfg.jail_binary.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "services/config/pi.yaml sets sandbox: required but the jail \
                     binary {} is missing; build sp-pi-jail or set jail_binary",
                    cfg.jail_binary.display()
                ),
            ));
        }
        argv.push(cfg.jail_binary.display().to_string());
        argv.extend(super::jail::wrap_args(cfg, workspace));
    } else {
        argv.push(cfg.binary.clone());
    }
    Ok(ulimit_command(cfg.limits, argv))
}

// Why: `setrlimit` via `pre_exec` needs `unsafe`, which this workspace denies.
// `sh` must `exec` so the handle still refers to pi and `kill_on_drop` reaps
// it.
fn ulimit_command(limits: ChildLimits, argv: Vec<String>) -> Command {
    let mut script = String::new();
    let mut shell = "/bin/sh";
    if limits.nproc > 0 {
        if Path::new("/bin/bash").exists() {
            shell = "/bin/bash";
            script.push_str(&format!("ulimit -u {};", limits.nproc));
        } else {
            tracing::warn!(
                "limits.nproc is set but /bin/bash is absent; \
                 /bin/sh cannot apply it, so the process cap is NOT in effect"
            );
        }
    }
    if limits.fsize > 0 {
        // Why: ulimit counts 1KiB blocks; the config is in bytes.
        script.push_str(&format!("ulimit -f {} 2>/dev/null;", limits.fsize / 1024));
    }
    if limits.address_space > 0 {
        script.push_str(&format!(
            "ulimit -v {} 2>/dev/null;",
            limits.address_space / 1024
        ));
    }
    if script.is_empty() {
        let mut parts = argv.into_iter();
        let mut cmd = Command::new(parts.next().unwrap_or_default());
        cmd.args(parts);
        return cmd;
    }
    script.push_str(" exec \"$@\"");

    let mut cmd = Command::new(shell);
    // Why: the second `sh` is the child's $0; the real argv starts after it.
    cmd.arg("-c").arg(script).arg("sh").args(&argv);
    cmd
}

async fn write_models_json(cfg: &PiConfig, gateway_key: &str, home: &Path) -> std::io::Result<()> {
    let models = serde_json::json!({
        "providers": {
            &cfg.provider: {
                "baseUrl": &cfg.base_url,
                "api": "anthropic-messages",
                "apiKey": gateway_key,
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
    tokio::fs::write(
        &path,
        serde_json::to_vec_pretty(&models).unwrap_or_default(),
    )
    .await?;
    restrict(&path).await
}

#[cfg(unix)]
async fn restrict(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).await
}

#[cfg(not(unix))]
async fn restrict(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

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
