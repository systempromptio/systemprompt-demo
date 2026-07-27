//! The Landlock ruleset, applied to this process before it becomes the child.
//!
//! Landlock restrictions are inherited across `execve` and cannot be dropped,
//! so restricting *ourselves* and then `exec`ing pi is airtight in a way that
//! no post-spawn check can be. That is the whole reason this is a separate
//! binary: the ruleset must bind the child and nothing else, and the server
//! that spawns it must stay unrestricted.
//!
//! # Fail closed
//!
//! `CompatLevel::BestEffort` is the crate default and never errors — it will
//! happily produce a ruleset that enforces nothing. Everything here is built
//! at `HardRequirement` instead, and the ABI is negotiated by *descending*
//! from the newest set of access rights until one the kernel fully supports is
//! found. If that search reaches bottom, or the kernel reports anything short
//! of `FullyEnforced`, this returns an error and the caller exits before
//! `exec`. There is no path from here to an unsandboxed child.

use crate::args::Spec;

#[cfg(target_os = "linux")]
fn enforce(
    spec: &Spec,
    readable: &[std::path::PathBuf],
    abi: landlock::ABI,
    net: bool,
) -> Result<landlock::RestrictionStatus, landlock::RulesetError> {
    use landlock::{
        Access, AccessFs, AccessNet, CompatLevel, Compatible, NetPort, Ruleset, RulesetAttr,
        RulesetCreatedAttr, path_beneath_rules,
    };

    let mut ruleset = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::from_all(abi))?;
    if net {
        ruleset = ruleset.handle_access(AccessNet::ConnectTcp)?;
    }
    let mut created = ruleset
        .create()?
        .add_rules(path_beneath_rules(
            [&spec.workspace],
            AccessFs::from_all(abi),
        ))?
        .add_rules(path_beneath_rules(readable, AccessFs::from_read(abi)))?;
    if net {
        for &port in &spec.connect_tcp {
            created = created.add_rule(NetPort::new(port, AccessNet::ConnectTcp))?;
        }
    }
    created.restrict_self()
}

#[cfg(target_os = "linux")]
pub(crate) fn apply(spec: &Spec) -> Result<String, String> {
    use landlock::{ABI, RulesetStatus};

    const LEVELS: &[ABI] = &[
        ABI::V7,
        ABI::V6,
        ABI::V5,
        ABI::V4,
        ABI::V3,
        ABI::V2,
        ABI::V1,
    ];

    if !spec.workspace.is_dir() {
        return Err(format!(
            "workspace {} is not a directory",
            spec.workspace.display()
        ));
    }

    let readable: Vec<_> = spec
        .allow_read
        .iter()
        .filter(|p| p.exists())
        .cloned()
        .collect();

    let mut last_err = "no Landlock ABI was attempted".to_owned();
    for &abi in LEVELS {
        let net = !spec.connect_tcp.is_empty() && abi >= ABI::V4;

        match enforce(spec, &readable, abi, net) {
            Ok(status) => {
                if status.ruleset != RulesetStatus::FullyEnforced {
                    return Err(format!(
                        "Landlock reported {:?} rather than FullyEnforced",
                        status.ruleset
                    ));
                }
                if !status.no_new_privs {
                    return Err("Landlock did not set no_new_privs".to_owned());
                }
                if !spec.connect_tcp.is_empty() && !net {
                    eprintln!(
                        "sp-pi-jail: kernel Landlock ABI {abi:?} predates network rules \
                         (needs 6.7); the child's outbound TCP is NOT confined"
                    );
                }
                return Ok(format!("{abi:?}"));
            },
            Err(e) => last_err = e.to_string(),
        }
    }
    Err(format!(
        "no supported Landlock ABI (needs Linux 5.13+ with landlock enabled): {last_err}"
    ))
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn apply(_spec: &Spec) -> Result<String, String> {
    Err("Landlock is Linux-only; this host cannot sandbox a pi child".to_owned())
}
