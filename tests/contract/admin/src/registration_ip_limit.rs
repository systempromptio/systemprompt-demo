//! The per-IP cap on self-registration.
//!
//! Signup mints an account and grants it a credit with no human in the loop, so
//! the cap is the only thing standing between one actor and an arbitrary number
//! of funded accounts. The properties worth pinning are not just "the fourth
//! request is refused" but the two ways the cap could be wrong in the other
//! direction: refusing a different network, and spending a network's quota on
//! requests that mint nothing.

use std::net::SocketAddr;

use axum::http::StatusCode;

use crate::app::App;
use crate::globals;
use crate::principal;
use crate::tempdb::TempDb;

const REGISTER: &str = "/admin/api/register";

fn body(email: &str) -> String {
    format!(
        r#"{{"name":"Test Person","email":"{email}","company":"Acme",
            "role":"Engineer","team_size":"1-10","why_assessing":"evaluating"}}"#
    )
}

fn peer(addr: &str) -> SocketAddr {
    SocketAddr::new(addr.parse().expect("peer address parses"), 1234)
}

async fn register(app: &App, addr: &str, email: &str) -> (StatusCode, String) {
    app.send_json_from("post", REGISTER, peer(addr), &body(email))
        .await
}

async fn harness() -> Option<(TempDb, App)> {
    if !globals::init() {
        return None;
    }
    let db = TempDb::create().await?;
    let credentials = principal::provision(&db.pool).await;
    let app = App::new(&db.pool, credentials);
    Some((db, app))
}

/// The whole point: a public address gets three accounts a day and no more.
#[tokio::test(flavor = "multi_thread")]
async fn a_public_address_is_capped_after_three_signups() {
    let Some((db, app)) = harness().await else {
        return;
    };

    for i in 0..3 {
        let (status, body) = register(&app, "203.0.113.7", &format!("cap{i}@example.com")).await;
        assert_eq!(status, StatusCode::OK, "signup {i} should succeed: {body}");
    }

    let (status, body) = register(&app, "203.0.113.7", "cap3@example.com").await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "body: {body}");
    assert!(
        body.contains("signup limit"),
        "the refusal must say what happened and how to get unblocked: {body}"
    );

    db.cleanup().await;
}

/// A saturated network must not leak into anyone else's.
#[tokio::test(flavor = "multi_thread")]
async fn the_cap_is_per_address_not_global() {
    let Some((db, app)) = harness().await else {
        return;
    };

    for i in 0..3 {
        register(&app, "203.0.113.10", &format!("other{i}@example.com")).await;
    }

    let (status, body) = register(&app, "203.0.113.11", "neighbour@example.com").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");

    db.cleanup().await;
}

/// Re-submitting an address that already has an account writes nothing and
/// grants nothing, so it must not spend the network's quota. Shared office NAT
/// is the case this protects: three colleagues rediscovering they already have
/// accounts should not lock out the fourth who does not.
#[tokio::test(flavor = "multi_thread")]
async fn resubmitting_a_registered_email_does_not_spend_quota() {
    let Some((db, app)) = harness().await else {
        return;
    };

    let (status, body) = register(&app, "203.0.113.20", "known@example.com").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");

    for _ in 0..5 {
        let (status, body) = register(&app, "203.0.113.20", "known@example.com").await;
        assert_eq!(status, StatusCode::OK, "repeat submission: {body}");
    }

    let (status, body) = register(&app, "203.0.113.20", "fresh@example.com").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "repeats of a known email must not have consumed the quota: {body}"
    );

    db.cleanup().await;
}

/// A deployment with no proxy in front of it resolves every client to one
/// private address. Enforcing there would lock out the whole deployment after
/// three signups rather than slow one abuser down.
#[tokio::test(flavor = "multi_thread")]
async fn a_private_peer_is_never_capped() {
    let Some((db, app)) = harness().await else {
        return;
    };

    for i in 0..6 {
        let (status, body) = register(&app, "172.17.0.1", &format!("local{i}@example.com")).await;
        assert_eq!(status, StatusCode::OK, "signup {i} behind NAT: {body}");
    }

    db.cleanup().await;
}
