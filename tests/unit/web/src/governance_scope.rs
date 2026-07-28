//! The per-surface scope ceiling.
//!
//! Scope is resolved *upwards* — DB roles joined with the agent's declared
//! scope, taking the higher — which is right for the admin console and wrong
//! for a sandboxed surface. `cap_at` is the other half: it lets a surface hold
//! every caller at a maximum, so an operator signed in as admin cannot skip the
//! policies that surface exists to demonstrate.
//!
//! The regression these pin: the pi terminal evaluated admins at `Admin`, which
//! exempts `tool_blocklist`, so `fetch_remote_docs` was *allowed and executed*
//! for an admin — real egress from the demo whose entire point is refusing it.

use systemprompt::security::policy::types::AccessScope;
use systemprompt_web_governance::test_support::cap_at;

#[test]
fn a_ceiling_lowers_admin_to_user() {
    assert_eq!(
        cap_at(AccessScope::Admin, AccessScope::User),
        AccessScope::User
    );
}

/// The direction that matters. A ceiling that could raise privilege would be a
/// privilege-escalation primitive rather than a confinement one.
#[test]
fn a_ceiling_never_raises() {
    assert_eq!(
        cap_at(AccessScope::User, AccessScope::Admin),
        AccessScope::User
    );
    assert_eq!(
        cap_at(AccessScope::Unknown, AccessScope::Admin),
        AccessScope::Unknown
    );
    assert_eq!(
        cap_at(AccessScope::Unknown, AccessScope::User),
        AccessScope::Unknown
    );
}

#[test]
fn an_admin_ceiling_is_no_ceiling() {
    assert_eq!(
        cap_at(AccessScope::Admin, AccessScope::Admin),
        AccessScope::Admin
    );
}

/// `Unknown` is the floor, not a wildcard: capping it at anything leaves it
/// `Unknown`, so an unrecognised caller cannot be promoted by a permissive
/// surface.
#[test]
fn unknown_stays_unknown_under_every_ceiling() {
    for ceiling in [AccessScope::Admin, AccessScope::User, AccessScope::Unknown] {
        assert_eq!(cap_at(AccessScope::Unknown, ceiling), AccessScope::Unknown);
    }
}
