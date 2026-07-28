//! Input shapes for the `systemprompt` MCP tools.
//!
//! Each tool's arguments are a named type rather than a loose JSON object, so
//! an unknown field or an unknown variant is a schema error the client sees
//! before the call is dispatched. The no-arg tools still need a struct because
//! serde has to have an object shape to deserialise `{}` into.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
#[expect(
    clippy::empty_structs_with_brackets,
    reason = "serde needs an empty object shape to deserialize a no-arg tool input from {}"
)]
pub struct ListTopicsInput {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetTopicInput {
    /// The topic id to fetch, e.g. "governance-pipeline". Use `list_topics` to
    /// discover valid ids.
    pub topic_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchDocsInput {
    pub query: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
#[expect(
    clippy::empty_structs_with_brackets,
    reason = "serde needs an empty object shape to deserialize a no-arg tool input from {}"
)]
pub struct GovernanceStatsInput {}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
#[expect(
    clippy::empty_structs_with_brackets,
    reason = "serde needs an empty object shape to deserialize a no-arg tool input from {}"
)]
pub struct SafetyFindingsInput {}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
#[expect(
    clippy::empty_structs_with_brackets,
    reason = "serde needs an empty object shape to deserialize a no-arg tool input from {}"
)]
pub struct AdminAuditDumpInput {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FetchRemoteDocsInput {
    pub path: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
#[expect(
    clippy::empty_structs_with_brackets,
    reason = "serde needs an empty object shape to deserialize a no-arg tool input from {}"
)]
pub struct ListSitePagesInput {}

/// The two site sections a page can live in. An enum rather than a string so
/// that "which hosts can this tool reach" is answered by the type system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SitePageSection {
    Documentation,
    Blog,
}

impl SitePageSection {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Documentation => "documentation",
            Self::Blog => "blog",
        }
    }
}

/// The artifact types `render_artifact` can produce. An enum rather than a
/// string so an unknown type is a schema error, not a runtime branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DemoArtifactType {
    Table,
    Chart,
    List,
    Dashboard,
    PresentationCard,
    Message,
    CopyPasteText,
    Text,
}

impl DemoArtifactType {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Table => "table",
            Self::Chart => "chart",
            Self::List => "list",
            Self::Dashboard => "dashboard",
            Self::PresentationCard => "presentation_card",
            Self::Message => "message",
            Self::CopyPasteText => "copy_paste_text",
            Self::Text => "text",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
pub struct RenderArtifactInput {
    /// Which artifact type to render, e.g. "chart". One of: `table`, `chart`,
    /// `list`, `dashboard`, `presentation_card`, `message`, `copy_paste_text`,
    /// `text`.
    pub artifact_type: DemoArtifactType,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FetchSitePageInput {
    /// Which section of the site the page lives in.
    pub section: SitePageSection,
    /// The page slug as listed by `list_site_pages`, e.g. "services/ai".
    pub slug: String,
}
