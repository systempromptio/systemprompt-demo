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
use crate::tempdb::TempDb;
use crate::{globals, principal};

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

/// Quota is spent per account minted, not per request served. Re-submitting an
/// email that already has a row re-issues a setup token and grants no further
/// credit, so it must not count. Shared office NAT is the case this protects:
/// colleagues retrying a half-finished signup — which is the common way to
/// recover a lost setup token — must not lock out someone who has not signed up
/// at all.
#[tokio::test(flavor = "multi_thread")]
async fn resubmitting_a_registered_email_does_not_spend_quota() {
    let Some((db, app)) = harness().await else {
        return;
    };

    const IP: &str = "203.0.113.20";

    for email in ["first@example.com", "second@example.com"] {
        let (status, body) = register(&app, IP, email).await;
        assert_eq!(status, StatusCode::OK, "{email}: {body}");
    }

    // Three repeats, not more: the separate email-keyed limiter allows five
    // setup tokens per address per quarter hour, and tripping that would prove
    // nothing about the per-IP quota this test is here for.
    for _ in 0..3 {
        let (status, body) = register(&app, IP, "first@example.com").await;
        assert_eq!(status, StatusCode::OK, "repeat submission: {body}");
    }

    // Two accounts minted, so the third is still within the cap. Were repeats
    // counted, five requests would already have exhausted it.
    let (status, body) = register(&app, IP, "third@example.com").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "repeats of a known email must not have consumed the quota: {body}"
    );

    let (status, _) = register(&app, IP, "fourth@example.com").await;
    assert_eq!(
        status,
        StatusCode::TOO_MANY_REQUESTS,
        "the cap must still bite once three accounts really have been minted"
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
