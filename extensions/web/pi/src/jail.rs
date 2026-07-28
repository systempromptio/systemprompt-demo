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

pub(super) fn wrap_args(cfg: &PiConfig, workspace: &Path) -> Vec<String> {
    let mut args = vec!["--workspace".to_owned(), workspace.display().to_string()];
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

fn pi_read_paths(binary: &str, child_path: &str) -> Vec<PathBuf> {
    let Some(entry) = locate(binary, child_path) else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    if let Some(bin_dir) = entry.parent() {
        paths.push(bin_dir.to_path_buf());
    }
    if let Ok(real) = std::fs::canonicalize(&entry)
        && let Some(root) = package_root(&real)
    {
        paths.push(root);
    }
    paths
}

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
