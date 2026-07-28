//! Library surface of `sp-pi-jail` — argv parsing only.
//!
//! The jail itself (Landlock ruleset + exec) lives in the binary; this target
//! exists so `tests/unit/web` can drive [`args::Spec::parse`] without
//! spawning the confined process.

pub mod args;
