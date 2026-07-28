//! Value types for the governance spine: hook payloads on the way in, and the
//! dashboard/analytics projections read back out of the audit tables.

pub mod control_center;
pub mod conversation_analytics;
mod dashboard;
mod dashboard_enterprise;
mod traffic;
pub mod webhook;

pub use control_center::{ActivityFeedEvent, TodayStats};
pub use conversation_analytics::{
    EntityEffectiveness, EntityUsageSummary, RateSessionRequest, RateSkillRequest,
    SessionEntityLink, SessionRating, SkillEffectiveness, SkillRating,
};
pub use dashboard::{
    AchievementInfo, ActivityStats, ContentPerformanceRow, DashboardData, DashboardQuery,
    DepartmentActivity, DepartmentQuery, DepartmentScore, EventBreakdown, EventFeedRow,
    EventTypeBreakdown, EventsQuery, EventsResponse, GovernanceEvent, HourlyActivity,
    IncidentGroup, LeaderboardEntry, McpAccessEvent, McpAccessSummary, ModelUsage, PaginationQuery,
    ProjectActivity, RealtimePulse, RecentMcpError, SkillCount, TimeSeriesBucket, TokenUsageRow,
    ToolSuccessRate, TopPageDailyBucket, TopUser, TrafficCountryBucket, TrafficData, TrafficDevice,
    TrafficGeo, TrafficKpis, TrafficReadingPattern, TrafficSource, TrafficTimeBucket,
    TrafficTopPage, UnlockedAchievement, UserGamificationProfile, WindowedCounts,
};

/// Content volume attributed to a single hook event.
#[derive(Debug, Clone, Copy)]
pub struct ContentBytes {
    pub input: i64,
    pub output: i64,
}
