//! The JSON shapes this endpoint returns, and the humanising helpers.
//!
//! Split from the collection logic because the two change for different
//! reasons: these are the browser's contract, the queries beside them are how
//! the numbers are found.

use serde::Serialize;

use super::super::normalize;
use crate::types::TopUser;

#[derive(Debug, Clone, Serialize)]
pub(super) struct PulseResponse {
    age_seconds: u64,
    window_hours: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    window: Option<PulseWindowOut>,
    all_time: PulseTotalsOut,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<Box<AdminDetailOut>>,
}

#[derive(Debug, Clone, Serialize)]
struct PulseWindowOut {
    people: String,
    sessions: String,
    requests: String,
    tool_calls: String,
    allowed: String,
    denied: String,
    allow_rate_percent: Option<i64>,
    latency_p50_ms: Option<i32>,
    input_tokens: String,
    output_tokens: String,
    cost_display: String,
    model_mix: Vec<ModelShareOut>,
    blocked_tools: Vec<BlockedToolOut>,
}

#[derive(Debug, Clone, Serialize)]
struct ModelShareOut {
    model: String,
    requests: String,
    percent: i64,
}

#[derive(Debug, Clone, Serialize)]
struct BlockedToolOut {
    tool_name: String,
    denials: String,
}

#[derive(Debug, Clone, Serialize)]
struct PulseTotalsOut {
    sessions: String,
    requests: String,
    tool_calls: String,
    secrets_caught: String,
}

#[derive(Debug, Clone, Serialize)]
struct AdminDetailOut {
    traffic: Arc<TrafficData>,
    realtime: RealtimePulse,
    activity: ActivityStats,
    active_users_24h: i64,
    top_users: Vec<TopUserOut>,
    popular_skills: Vec<SkillCount>,
    hourly_activity: Vec<HourlyActivity>,
    tool_success: Vec<ToolSuccessRate>,
    tools: Vec<ToolRollupOut>,
    agents: Vec<AgentRollupOut>,
}

#[derive(Debug, Clone, Serialize)]
struct TopUserOut {
    user_id: UserId,
    display_name: String,
    logins: i64,
    edits: i64,
    mcp_calls: i64,
    last_active: chrono::DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
struct ToolRollupOut {
    tool_name: String,
    calls: i64,
    errors: i64,
    sessions: i64,
}

#[derive(Debug, Clone, Serialize)]
struct AgentRollupOut {
    agent_id: AgentId,
    calls: i64,
    errors: i64,
    sessions: i64,
}

impl From<TopUser> for TopUserOut {
    fn from(u: TopUser) -> Self {
        Self {
            user_id: u.user_id,
            display_name: u.display_name,
            logins: u.logins,
            edits: u.edits,
            mcp_calls: u.mcp_calls,
            last_active: u.last_active,
        }
    }
}

fn count(n: i64, exact: bool) -> String {
    if exact {
        n.to_string()
    } else {
        normalize::bucket(n)
    }
}

fn tokens(n: i64, exact: bool) -> String {
    if exact {
        n.to_string()
    } else {
        normalize::bucket_tokens(n)
    }
}
