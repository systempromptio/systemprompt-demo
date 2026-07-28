//! `rate_limit` counts calls, not evaluations.
//!
//! One tool call is judged by every enforcement point it passes, and a nested
//! point judges a call the outer one already judged. The counter must not move
//! for the repeat, or a demo of eight calls reports sixteen.

use serde_json::json;
use systemprompt::identifiers::{CallId, McpToolName, SessionId, UserId};
use systemprompt::security::authz::Decision;
use systemprompt::security::policy::types::AccessScope;
use systemprompt::security::policy::{AgentScope, GovernancePolicy, McpToolInput, PolicyContext};
use systemprompt_web_admin::test_support::RateLimit;

struct Fixture {
    tool: McpToolName,
    session: SessionId,
    user: UserId,
    input: McpToolInput,
}

impl Fixture {
    // Why: the counter is a process-global keyed on {session,user}, so a fresh
    // session per test is what keeps them from charging each other's buckets.
    fn new() -> Self {
        Self {
            tool: McpToolName::new("mcp__systemprompt__render_artifact"),
            session: SessionId::generate(),
            user: UserId::new("u-rate-limit"),
            input: McpToolInput::new(json!({"artifact_type": "dashboard"})),
        }
    }

    fn context<'a>(&'a self, call_id: &'a CallId) -> PolicyContext<'a> {
        PolicyContext {
            tool: self.tool.clone(),
            agent_scope: AgentScope::User {
                user_id: self.user.clone(),
            },
            access_scope: AccessScope::User,
            session_id: &self.session,
            user_id: &self.user,
            tool_input: &self.input,
            call_id,
        }
    }
}

fn allow_detail(decision: &Decision) -> String {
    match decision {
        Decision::Allow { matched_by } => format!("{matched_by:?}"),
        Decision::Deny { reason } => panic!("expected an allow, got a deny: {reason}"),
    }
}

#[test]
fn re_evaluating_one_call_does_not_charge_twice() {
    let fixture = Fixture::new();
    let policy = RateLimit::new(60, 300);
    let call_id = CallId::generate();

    let first = allow_detail(&policy.evaluate(&fixture.context(&call_id)));
    let second = allow_detail(&policy.evaluate(&fixture.context(&call_id)));

    assert!(first.contains("0/300"), "first evaluation saw {first}");
    assert!(
        second.contains("0/300"),
        "the same call charged again: {second}"
    );
}

#[test]
fn separate_calls_each_charge() {
    let fixture = Fixture::new();
    let policy = RateLimit::new(60, 300);

    let first = allow_detail(&policy.evaluate(&fixture.context(&CallId::generate())));
    let second = allow_detail(&policy.evaluate(&fixture.context(&CallId::generate())));

    assert!(first.contains("0/300"), "first call saw {first}");
    assert!(second.contains("1/300"), "second call saw {second}");
}

#[test]
fn the_limit_still_denies() {
    let fixture = Fixture::new();
    let policy = RateLimit::new(60, 2);

    for _ in 0..2 {
        assert!(matches!(
            policy.evaluate(&fixture.context(&CallId::generate())),
            Decision::Allow { .. }
        ));
    }

    assert!(matches!(
        policy.evaluate(&fixture.context(&CallId::generate())),
        Decision::Deny { .. }
    ));
}

// Why: an over-limit call is deliberately never recorded, so re-judging it must
// re-derive the deny rather than find itself in the window and be waved through.
#[test]
fn a_denied_call_stays_denied_when_re_evaluated() {
    let fixture = Fixture::new();
    let policy = RateLimit::new(60, 1);
    let denied = CallId::generate();

    assert!(matches!(
        policy.evaluate(&fixture.context(&CallId::generate())),
        Decision::Allow { .. }
    ));
    assert!(matches!(
        policy.evaluate(&fixture.context(&denied)),
        Decision::Deny { .. }
    ));
    assert!(matches!(
        policy.evaluate(&fixture.context(&denied)),
        Decision::Deny { .. }
    ));
}
