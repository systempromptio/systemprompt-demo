//! The pending-approval gate: who may move before an admin has approved them.

use systemprompt_web_admin::test_support::{is_pending_allowed_path, may_pass_pending_gate};

#[test]
fn unapproved_user_is_held_at_the_pending_page() {
    for path in ["/admin/devices/pats", "/bridge-auth/device-link"] {
        assert!(
            !may_pass_pending_gate(false, false, path),
            "{path} must bounce an unapproved user"
        );
    }
}

#[test]
fn sign_in_and_sign_out_survive_the_gate() {
    // A bounce target that is itself bounced is an infinite redirect, and
    // an account that cannot reach logout is stuck in the browser session.
    for path in [
        "/admin/pending",
        "/admin/login",
        "/admin/logout",
        "/admin/continue",
        "/admin/register",
        "/admin/api/auth/me",
        "/admin/auth/me",
    ] {
        assert!(
            is_pending_allowed_path(path),
            "{path} must stay reachable while under review"
        );
        assert!(may_pass_pending_gate(false, false, path));
    }
}

#[test]
fn admins_bypass_even_without_an_approval_row() {
    // Accounts predating the review gate carry no approval row. Locking the
    // only role that can approve anyone out of the queue is unrecoverable.
    assert!(may_pass_pending_gate(true, false, "/admin/devices/pats"));
}

#[test]
fn approved_user_reaches_the_bridge_endpoints() {
    assert!(may_pass_pending_gate(false, true, "/admin/devices/pats"));
}
