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

/// Files a fresh workspace starts with, so a read-only agent has something to
/// read. Deliberately tiny and inert: no credentials, no repo.
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

/// Everything a spawned child needs, resolved before the process exists.
pub(super) struct SpawnRequest<'a> {
    pub(super) conversation_id: &'a str,
    /// The server-issued session the gateway attests. Provider spend and
    /// governance rows key on it, which is what ties the two spines together.
    pub(super) attested_session: &'a str,
    /// The PAT pi authenticates to `/v1/messages` with, minted for this
    /// conversation's own user. It has to be the same identity the gateway
    /// attests the session against — a credential for anyone else is rejected.
    pub(super) gateway_key: &'a str,
    pub(super) shim_source: &'a str,
    /// The MCP-client extension, compiled in beside the governance shim.
    pub(super) mcp_client_source: &'a str,
    /// The embed token the MCP-client extension authenticates its proxy calls
    /// with. Scoped to this conversation's own user and checked against the
    /// conversation on every call, so it is worth no more than the session.
    pub(super) mcp_token: &'a str,
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

    // The hub reaches the session as a second extension rather than as MCP
    // configuration: pi ships no MCP client. Compiled in beside the shim for
    // the same reason — a deployment must not be able to edit either one.
    let mcp_client_path = shim_dir.join("mcp-client.ts");
    tokio::fs::write(&mcp_client_path, req.mcp_client_source).await?;

    let skills_dir = super::skills::materialise(&workspace).await;

    write_models_json(cfg, req.gateway_key, &home).await?;
    // quietStartup keeps pi's banner off a stream the widget parses.
    // enableSkillCommands is what makes `/skill:<name>` resolve at all — without
    // it the skills load but the slash form is inert, and a viewer typing the
    // command gets it forwarded to the model as plain text.
    tokio::fs::write(
        home.join(".pi").join("agent").join("settings.json"),
        "{\"quietStartup\":true,\"enableSkillCommands\":true}",
    )
    .await?;

    let mut cmd = limited_command(cfg, &workspace)?;
    cmd.current_dir(&workspace)
        .arg("--mode")
        .arg("rpc")
        // Only our shim loads. Without this, any extension on the host — or in
        // a discovered project directory — would join the session.
        .arg("--no-extensions")
        .arg("-e")
        .arg(&shim_path)
        .arg("-e")
        .arg(&mcp_client_path)
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
        // Discovery stays off; `--skill` is additive even alongside it, so the
        // session gets exactly the skills written into its own workspace and
        // nothing from the host or a project tree.
        .arg("--no-skills")
        .arg("--no-prompt-templates")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    if let Some(dir) = &skills_dir {
        cmd.arg("--skill").arg(dir);
    }

    // Nothing is inherited: the server's environment holds provider keys and
    // database URLs that a read tool would otherwise reach via /proc/self/environ.
    cmd.env_clear()
        .env("HOME", &home)
        .env("PATH", &cfg.child_path)
        .env("SYSTEMPROMPT_BASE_URL", &cfg.base_url)
        .env("SYSTEMPROMPT_PI_SESSION", req.attested_session)
        .env("SP_PI_CONVERSATION", req.conversation_id)
        .env("SP_PI_MCP_TOKEN", req.mcp_token)
        // pi phones home for model catalogs on startup otherwise, which a
        // network-restricted deployment would hang on.
        .env("PI_OFFLINE", "1")
        // pi transpiles its TypeScript extensions through jiti, which caches
        // the output to `/tmp/jiti/`. The Landlock ruleset grants write access
        // to the session workspace and nothing else, so that open() is denied
        // and *every* extension fails to load — including the governance shim,
        // which takes the whole session down. jiti ignores JITI_CACHE as a
        // path here (pi sets its own), so the cache is turned off outright.
        // The cost is re-transpiling two small files per session; the
        // alternative is granting a shared, writable, predictably-named
        // directory to every jailed child, which is a worse trade.
        .env("JITI_CACHE", "false");

    let child = cmd.spawn()?;
    Ok(Spawned { child, workspace })
}

/// Build the child's argv: `sh` for the ulimits, `sp-pi-jail` for the Landlock
/// ruleset, then pi.
///
/// When `sandbox` is `required` — the default — a missing jail binary is
/// an error rather than a degraded spawn. There is no fallback path to an
/// unsandboxed child, because a boundary with a fallback is not a boundary:
/// this one was already, silently, doing nothing, masked by a credit guard
/// that would have evaporated the day trial credit was auto-granted.
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

/// Rewrite the command so the child starts under `ulimit` ceilings.
///
/// The obvious implementation is `pre_exec` with `setrlimit`, but that needs
/// `unsafe`, and this workspace denies `unsafe_code` outright with no existing
/// exemption. A one-line `sh` preamble reaches the same syscall and keeps that
/// property, at the cost of one `execve`.
///
/// `exec` matters: `sh` replaces itself with pi, so there is still exactly one
/// process, the handle still refers to pi, and `kill_on_drop` still reaps the
/// thing that matters. No argument is ever interpolated into the script text —
/// the binary and its argv arrive positionally through `"$@"` — so this adds no
/// quoting surface.
///
/// Failing to raise a limit is not fatal: it means the host's hard limit is
/// already below what we asked for, which is stricter than we wanted.
fn ulimit_command(limits: ChildLimits, argv: Vec<String>) -> Command {
    let mut script = String::new();
    // dash has no `ulimit -u`, and dash is `/bin/sh` on Debian. Rather than
    // emit a limit that silently does nothing, ask for a shell that supports
    // it and say so plainly when there isn't one.
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
        // ulimit counts 1KiB blocks; the config is in bytes.
        script.push_str(&format!("ulimit -f {} 2>/dev/null;", limits.fsize / 1024));
    }
    if limits.address_space > 0 {
        script.push_str(&format!(
            "ulimit -v {} 2>/dev/null;",
            limits.address_space / 1024
        ));
    }
    if script.is_empty() {
        // No ceilings to set, so drop the shell layer entirely and run the
        // argv directly. `limited_command` never builds an empty one.
        let mut parts = argv.into_iter();
        let mut cmd = Command::new(parts.next().unwrap_or_default());
        cmd.args(parts);
        return cmd;
    }
    script.push_str(" exec \"$@\"");

    let mut cmd = Command::new(shell);
    // The second `sh` is the child's $0; the real argv starts after it.
    cmd.arg("-c").arg(script).arg("sh").args(&argv);
    cmd
}

/// Point pi at this deployment's Anthropic-compatible gateway.
///
/// Written per session under the session's own `HOME`, so one conversation
/// cannot read or rewrite another's provider config — which matters more now
/// that the credential in it belongs to one user rather than the deployment.
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
