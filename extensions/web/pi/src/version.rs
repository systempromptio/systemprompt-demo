//! Holding the pi binary to the version this module was written against.
//!
//! The dependency on pi is a subprocess contract, not a compiled one: RPC
//! frame shapes, CLI flags, and — load-bearing for security — `--skill`
//! staying additive under `--no-skills` (see [`super::skills`]). All of that
//! tracks whatever `npm install -g` last wrote to the pinned binary path, so
//! the pin in `services/config/pi.yaml` is asserted here by running
//! `pi --version` once per process, before the first session spawns.

use tokio::process::Command;

use super::config::{PiConfig, VersionCheckMode};

pub(super) async fn assert_supported(cfg: &PiConfig) -> Result<(), String> {
    if cfg.version_check == VersionCheckMode::Off {
        return Ok(());
    }
    let Some(expected) = cfg.expected_version.as_deref() else {
        tracing::warn!(
            "services/config/pi.yaml pins no expected_version; the terminal will run \
             whatever pi the binary path resolves to. Pin the version the RPC frames \
             and skill flags were verified against."
        );
        return Ok(());
    };

    let outcome = match probe(cfg).await {
        Ok(found) if found == expected => Ok(()),
        Ok(found) => Err(format!(
            "pi reports version {found} but services/config/pi.yaml expects \
             {expected}; align the install or the pin (or set version_check: warn)"
        )),
        Err(e) => Err(format!(
            "could not run `{} --version` to verify the pin in \
             services/config/pi.yaml: {e}",
            cfg.binary
        )),
    };

    match (outcome, cfg.version_check) {
        (Ok(()), _) => {
            tracing::info!(version = expected, "pi version pin verified");
            Ok(())
        },
        (Err(why), VersionCheckMode::Warn) => {
            tracing::warn!("{why} — version_check: warn, continuing anyway");
            Ok(())
        },
        (Err(why), _) => {
            tracing::error!("{why}");
            Err(why)
        },
    }
}

async fn probe(cfg: &PiConfig) -> Result<String, String> {
    let output = Command::new(&cfg.binary)
        .arg("--version")
        .env_clear()
        .env("PATH", &cfg.child_path)
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!("exited with {}", output.status));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    extract_version(&stdout)
        .map(str::to_owned)
        .ok_or_else(|| format!("no version in output {:?}", stdout.trim()))
}

/// The first token that looks like a version, `v` prefix stripped — tolerant
/// of both a bare `0.12.3` and a `pi 0.12.3 (…)` banner.
pub fn extract_version(output: &str) -> Option<&str> {
    output
        .split_whitespace()
        .map(|t| t.strip_prefix('v').unwrap_or(t))
        .find(|t| t.chars().next().is_some_and(|c| c.is_ascii_digit()))
}
