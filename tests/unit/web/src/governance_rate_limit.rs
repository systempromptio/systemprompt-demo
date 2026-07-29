//! `rate_limit` counts calls, not evaluations.
//!
//! One tool call is judged by every enforcement point it passes, and a nested
//! point judges a call the outer one already judged. The counter must not move
//! for the repeat, or a demo of eight calls reports sixteen.

use serde_json::json;
use systemprompt::identifiers::{CallId, McpToolName, SessionId, UserId};
use systemprompt::security::authz::Decision;
use systemprompt::security::policy::types::AccessScope;
use systemprompt::security::policy::{
    AgentScope, ChainEntryResult, GovernanceConfig, GovernanceEngine, GovernedInput,
    GovernedTarget, McpToolInput, PolicyContext,
};

struct Fixture {
    target: GovernedTarget,
    session: SessionId,
    user: UserId,
    input: GovernedInput,
}

impl Fixture {
    fn new() -> Self {
        Self {
            target: GovernedTarget::Tool {
                tool: McpToolName::new("mcp__systemprompt__render_artifact"),
            },
            session: SessionId::generate(),
            user: UserId::new("u-rate-limit"),
            input: GovernedInput::tool_arguments(McpToolInput::new(
                json!({"artifact_type": "dashboard"}),
            )),
        }
    }

    fn context<'a>(&'a self, call_id: &'a CallId) -> PolicyContext<'a> {
        PolicyContext {
            target: self.target.clone(),
            agent_scope: AgentScope::User {
                user_id: self.user.clone(),
            },
            access_scope: AccessScope::User,
            session_id: &self.session,
            user_id: &self.user,
            input: &self.input,
            call_id,
        }
    }
}

// Why: each engine owns its rate-limit window, so a fresh engine per test is
// what keeps them from charging each other's buckets.
fn rate_limiter(window_secs: u64, limit: u64) -> GovernanceEngine {
    let yaml = format!(
        "governance:\n  policies:\n    - id: rate_limit\n      window_secs: {window_secs}\n      requests_per_window: {limit}\n"
    );
    GovernanceEngine::from_config(&GovernanceConfig::parse(&yaml).unwrap())
}

fn allow_detail(engine: &GovernanceEngine, ctx: &PolicyContext<'_>) -> String {
    let evaluation = engine.evaluate(ctx);
    match &evaluation.decision {
        Decision::Allow { .. } => evaluation
            .chain
            .iter()
            .find(|e| e.policy_id.as_str() == "rate_limit" && e.result == ChainEntryResult::Pass)
            .map(|e| e.detail.clone())
            .expect("rate_limit pass entry"),
        Decision::Deny { reason } => panic!("expected an allow, got a deny: {reason}"),
    }
}

#[test]
fn re_evaluating_one_call_does_not_charge_twice() {
    let fixture = Fixture::new();
    let engine = rate_limiter(60, 300);
    let call_id = CallId::generate();

    let first = allow_detail(&engine, &fixture.context(&call_id));
    let second = allow_detail(&engine, &fixture.context(&call_id));

    assert!(first.contains("0/300"), "first evaluation saw {first}");
    assert!(
        second.contains("0/300"),
        "the same call charged again: {second}"
    );
}

#[test]
fn separate_calls_each_charge() {
    let fixture = Fixture::new();
    let engine = rate_limiter(60, 300);

    let first = allow_detail(&engine, &fixture.context(&CallId::generate()));
    let second = allow_detail(&engine, &fixture.context(&CallId::generate()));

    assert!(first.contains("0/300"), "first call saw {first}");
    assert!(second.contains("1/300"), "second call saw {second}");
}

#[test]
fn the_limit_still_denies() {
    let fixture = Fixture::new();
    let engine = rate_limiter(60, 2);

    for _ in 0..2 {
        assert!(matches!(
            engine
                .evaluate(&fixture.context(&CallId::generate()))
                .decision,
            Decision::Allow { .. }
        ));
    }

    assert!(matches!(
        engine
            .evaluate(&fixture.context(&CallId::generate()))
            .decision,
        Decision::Deny { .. }
    ));
}

// Why: an over-limit call is deliberately never recorded, so re-judging it must
// re-derive the deny rather than find itself in the window and be waved
// through.
#[test]
fn a_denied_call_stays_denied_when_re_evaluated() {
    let fixture = Fixture::new();
    let engine = rate_limiter(60, 1);
    let denied = CallId::generate();

    assert!(matches!(
        engine
            .evaluate(&fixture.context(&CallId::generate()))
            .decision,
        Decision::Allow { .. }
    ));
    assert!(matches!(
        engine.evaluate(&fixture.context(&denied)).decision,
        Decision::Deny { .. }
    ));
    assert!(matches!(
        engine.evaluate(&fixture.context(&denied)).decision,
        Decision::Deny { .. }
    ));
}
