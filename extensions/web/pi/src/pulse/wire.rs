//! The JSON shapes this endpoint returns, and the humanising helpers.
//!
//! Split from the collection logic because the two change for different
//! reasons: these are the browser's contract, the queries beside them are how
//! the numbers are found.

use std::sync::Arc;

use chrono::Utc;
use serde::Serialize;
use systemprompt::identifiers::{AgentId, UserId};

use super::super::normalize;
use systemprompt_web_governance::types::{
    ActivityStats, HourlyActivity, RealtimePulse, SkillCount, ToolSuccessRate, TopUser, TrafficData,
};

#[derive(Debug, Clone, Serialize)]
pub(super) struct PulseResponse {
    pub(super) age_seconds: u64,
    pub(super) window_hours: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) window: Option<PulseWindowOut>,
    pub(super) all_time: PulseTotalsOut,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) detail: Option<Box<AdminDetailOut>>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct PulseWindowOut {
    pub(super) people: String,
    pub(super) sessions: String,
    pub(super) requests: String,
    pub(super) tool_calls: String,
    pub(super) allowed: String,
    pub(super) denied: String,
    pub(super) allow_rate_percent: Option<i64>,
    pub(super) latency_p50_ms: Option<i32>,
    pub(super) input_tokens: String,
    pub(super) output_tokens: String,
    pub(super) cost_display: String,
    pub(super) model_mix: Vec<ModelShareOut>,
    pub(super) blocked_tools: Vec<BlockedToolOut>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct ModelShareOut {
    pub(super) model: String,
    pub(super) requests: String,
    pub(super) percent: i64,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct BlockedToolOut {
    pub(super) tool_name: String,
    pub(super) denials: String,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct PulseTotalsOut {
    pub(super) sessions: String,
    pub(super) requests: String,
    pub(super) tool_calls: String,
    pub(super) secrets_caught: String,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct AdminDetailOut {
    pub(super) traffic: Arc<TrafficData>,
    pub(super) realtime: RealtimePulse,
    pub(super) activity: ActivityStats,
    pub(super) active_users_24h: i64,
    pub(super) top_users: Vec<TopUserOut>,
    pub(super) popular_skills: Vec<SkillCount>,
    pub(super) hourly_activity: Vec<HourlyActivity>,
    pub(super) tool_success: Vec<ToolSuccessRate>,
    pub(super) tools: Vec<ToolRollupOut>,
    pub(super) agents: Vec<AgentRollupOut>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct TopUserOut {
    pub(super) user_id: UserId,
    pub(super) display_name: String,
    pub(super) logins: i64,
    pub(super) edits: i64,
    pub(super) mcp_calls: i64,
    pub(super) last_active: chrono::DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct ToolRollupOut {
    pub(super) tool_name: String,
    pub(super) calls: i64,
    pub(super) errors: i64,
    pub(super) sessions: i64,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct AgentRollupOut {
    pub(super) agent_id: AgentId,
    pub(super) calls: i64,
    pub(super) errors: i64,
    pub(super) sessions: i64,
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

pub(super) fn count(n: i64, exact: bool) -> String {
    if exact {
        n.to_string()
    } else {
        normalize::bucket(n)
    }
}

pub(super) fn tokens(n: i64, exact: bool) -> String {
    if exact {
        n.to_string()
    } else {
        normalize::bucket_tokens(n)
    }
}
