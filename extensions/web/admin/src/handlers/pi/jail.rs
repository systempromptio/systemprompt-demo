//! Building the `sp-pi-jail` argv that wraps a pi child.
//!
//! The jail is a separate binary because a Landlock ruleset binds the process
//! that applies it and everything it `exec`s — so it must be applied by
//! something that is about to *become* pi, never by this server. See
//! `extensions/pi-jail` for the ruleset itself.
//!
//! What lives here is the part that has to know about this deployment: which
//! directories the child needs, and how to find pi's package root on a host
//! where nvm puts it under `$HOME` (which is otherwise entirely denied).

use std::path::{Path, PathBuf};

use super::config::PiConfig;

/// Assemble `sp-pi-jail --workspace … --allow-read … -- <pi> …`.
///
/// The trailing `--` and pi's own path are included, so whatever the caller
/// appends lands as pi's arguments rather than as flags the jail would reject.
/// The child's outbound TCP is confined to the gateway's port,
/// which on a 6.7+ kernel is the only port it may reach — though Landlock is
/// port-based, so that port is permitted on any host, not just loopback.
pub(super) fn wrap_args(cfg: &PiConfig, workspace: &Path) -> Vec<String> {
    let mut args = vec![
        "--workspace".to_owned(),
        workspace.display().to_string(),
    ];
    for path in cfg
        .jail_read_paths
        .iter()
        .cloned()
        .chain(pi_read_paths(&cfg.binary, &cfg.child_path))
    {
        args.push("--allow-read".to_owned());
        args.push(path.display().to_string());
    }
    if let Some(port) = gateway_port(&cfg.base_url) {
        args.push("--allow-connect-tcp".to_owned());
        args.push(port.to_string());
    }
    args.push("--".to_owned());
    args.push(cfg.binary.clone());
    args
}

/// Where pi actually lives: the directory holding the `pi` entry point (which
/// is also node's, since the shebang is `#!/usr/bin/env node`) and the package
/// root the entry point symlinks into.
///
/// Derived rather than configured so an nvm upgrade — which moves both paths —
/// does not silently produce a jail that cannot start pi.
fn pi_read_paths(binary: &str, child_path: &str) -> Vec<PathBuf> {
    let Some(entry) = locate(binary, child_path) else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    if let Some(bin_dir) = entry.parent() {
        paths.push(bin_dir.to_path_buf());
    }
    // `bin/pi` is a symlink into `lib/node_modules/@…/dist/cli.js`; the grant
    // has to cover the package, not one file, because it loads its own
    // `node_modules` at runtime.
    if let Ok(real) = std::fs::canonicalize(&entry)
        && let Some(root) = package_root(&real)
    {
        paths.push(root);
    }
    paths
}

/// Resolve a possibly-bare binary name the way `execve` will: verbatim when it
/// contains a separator, otherwise against the child's own `PATH`.
fn locate(binary: &str, child_path: &str) -> Option<PathBuf> {
    if binary.contains('/') {
        let path = PathBuf::from(binary);
        return path.exists().then_some(path);
    }
    child_path
        .split(':')
        .filter(|dir| !dir.is_empty())
        .map(|dir| Path::new(dir).join(binary))
        .find(|candidate| candidate.exists())
}

/// Walk up from the resolved entry point to the nearest directory holding a
/// `package.json`. Falls back to the file's own directory, which grants less
/// than the package and so fails visibly rather than silently over-granting.
fn package_root(real: &Path) -> Option<PathBuf> {
    real.ancestors()
        .skip(1)
        .find(|dir| dir.join("package.json").exists())
        .or_else(|| real.parent())
        .map(Path::to_path_buf)
}

/// The port the child talks to the gateway on, so `--allow-connect-tcp` grants
/// exactly one. An explicit port wins; otherwise it is the scheme's default.
pub fn gateway_port(base_url: &str) -> Option<u16> {
    let (scheme, rest) = base_url.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, hp)| hp);
    // An IPv6 literal is full of colons, so its port is whatever follows the
    // closing bracket — splitting on the last colon would read address digits.
    let explicit = host_port.rfind(']').map_or_else(
        || host_port.rsplit_once(':').map(|(_, port)| port),
        |close| host_port[close + 1..].strip_prefix(':'),
    );
    if let Some(Ok(port)) = explicit.map(str::parse) {
        return Some(port);
    }
    match scheme {
        "https" => Some(443),
        "http" => Some(80),
        _ => None,
    }
}
