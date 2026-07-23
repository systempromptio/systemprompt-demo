//! Static registry of documentation topics served by the `systemprompt` MCP
//! server, plus a small keyword-scoring search over them.
//!
//! Each topic's Markdown body is embedded at compile time with `include_str!`,
//! so the resource hub ships as a single self-contained binary with no runtime
//! file dependency.

/// One documentation topic served by the hub.
///
/// Carries a stable id, a human title, a one-line summary used by
/// `list_topics`, keywords that bias search scoring, and the full Markdown body
/// served by `get_topic` and the `systemprompt://docs/<id>` resource.
#[derive(Debug, Clone, Copy)]
pub struct Topic {
    pub id: &'static str,
    pub title: &'static str,
    pub summary: &'static str,
    pub keywords: &'static [&'static str],
    pub body: &'static str,
}

/// Every topic, in the order `list_topics` presents them (introductory first).
pub static TOPICS: &[Topic] = &[
    Topic {
        id: "what-is-systemprompt",
        title: "What is systemprompt.io?",
        summary: "The open governance gateway for AI agents — what it is and who it is for.",
        keywords: &[
            "what",
            "overview",
            "intro",
            "introduction",
            "product",
            "gateway",
            "governance",
            "library",
            "pitch",
            "about",
            "summary",
        ],
        body: include_str!("../content/what-is-systemprompt.md"),
    },
    Topic {
        id: "getting-started",
        title: "Getting Started",
        summary: "The three hub tools and how to make your first governed call.",
        keywords: &[
            "start",
            "getting",
            "started",
            "begin",
            "first",
            "call",
            "connect",
            "client",
            "tools",
            "list_topics",
            "get_topic",
            "search_docs",
            "quickstart",
        ],
        body: include_str!("../content/getting-started.md"),
    },
    Topic {
        id: "governance-pipeline",
        title: "The Governance Pipeline",
        summary: "The four-stage check — scope, secret scan, blocklist, rate limit — on every tool call.",
        keywords: &[
            "governance",
            "pipeline",
            "policy",
            "policies",
            "scope",
            "secret",
            "scan",
            "blocklist",
            "rate",
            "limit",
            "deny",
            "allow",
            "enforce",
            "enforcement",
            "stage",
            "check",
            "credentials",
        ],
        body: include_str!("../content/governance-pipeline.md"),
    },
    Topic {
        id: "access-control",
        title: "Access Control",
        summary: "Roles, marketplace cascade, and deny-overrides that decide who reaches what.",
        keywords: &[
            "access",
            "control",
            "rbac",
            "role",
            "roles",
            "permission",
            "permissions",
            "scope",
            "deny",
            "override",
            "marketplace",
            "cascade",
            "entitlement",
            "grant",
        ],
        body: include_str!("../content/access-control.md"),
    },
    Topic {
        id: "architecture",
        title: "Architecture",
        summary: "Compile-time extensions, flat YAML config, Postgres spine, and the deploy flow.",
        keywords: &[
            "architecture",
            "design",
            "extension",
            "extensions",
            "inventory",
            "crate",
            "config",
            "yaml",
            "postgres",
            "database",
            "deploy",
            "build",
            "compile",
            "library",
        ],
        body: include_str!("../content/architecture.md"),
    },
    Topic {
        id: "skills-and-marketplace",
        title: "Skills and the Marketplace",
        summary: "How skills, MCP servers, and the exported plugin reach a connected client.",
        keywords: &[
            "skill",
            "skills",
            "marketplace",
            "plugin",
            "export",
            "mcp",
            "server",
            "bundle",
            "cowork",
            "claude",
            "desktop",
            "client",
            "capabilities",
        ],
        body: include_str!("../content/skills-and-marketplace.md"),
    },
    Topic {
        id: "audit-trail",
        title: "The Audit Trail",
        summary: "Reading requests, traces, decisions, and costs back out of the spine.",
        keywords: &[
            "audit",
            "trail",
            "logs",
            "log",
            "trace",
            "request",
            "requests",
            "decision",
            "decisions",
            "cost",
            "costs",
            "analytics",
            "spend",
            "metering",
            "history",
        ],
        body: include_str!("../content/audit-trail.md"),
    },
];

/// Look up a topic by its exact id.
#[must_use]
pub fn find(topic_id: &str) -> Option<&'static Topic> {
    TOPICS.iter().find(|t| t.id == topic_id)
}

/// A search hit: the matched topic and the score it earned for the query.
#[derive(Debug, Clone, Copy)]
pub struct Hit {
    pub topic: &'static Topic,
    pub score: u32,
}

/// Rank topics against a free-text query with simple keyword scoring.
///
/// Each whitespace token of the query is matched, case-insensitively, against
/// the topic id, title, summary, declared keywords, and body. Structured fields
/// (id/title/keywords) weigh more than an incidental body mention. Only topics
/// with a non-zero score are returned, highest first.
#[must_use]
pub fn search(query: &str) -> Vec<Hit> {
    let terms: Vec<String> = query
        .split_whitespace()
        .map(|t| {
            t.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|t| t.len() >= 2)
        .collect();

    let mut hits: Vec<Hit> = TOPICS
        .iter()
        .filter_map(|topic| {
            let id = topic.id.to_lowercase();
            let title = topic.title.to_lowercase();
            let summary = topic.summary.to_lowercase();
            let body = topic.body.to_lowercase();

            let mut score = 0u32;
            for term in &terms {
                if id.contains(term.as_str()) {
                    score += 5;
                }
                if title.contains(term.as_str()) {
                    score += 4;
                }
                if topic.keywords.iter().any(|k| k == term) {
                    score += 4;
                }
                if summary.contains(term.as_str()) {
                    score += 2;
                }
                if body.contains(term.as_str()) {
                    score += 1;
                }
            }

            (score > 0).then_some(Hit { topic, score })
        })
        .collect();

    hits.sort_by(|a, b| b.score.cmp(&a.score).then(a.topic.id.cmp(b.topic.id)));
    hits
}

/// A short excerpt from a topic body around the first line mentioning any term,
/// for search result previews. Falls back to the summary when nothing matches.
#[must_use]
pub fn excerpt(topic: &Topic, terms: &[String]) -> String {
    for line in topic.body.lines() {
        let lower = line.to_lowercase();
        if !line.trim().is_empty()
            && !line.starts_with('#')
            && terms.iter().any(|t| lower.contains(t.as_str()))
        {
            return line.trim().chars().take(200).collect();
        }
    }
    topic.summary.to_owned()
}
