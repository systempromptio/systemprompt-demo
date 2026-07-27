//! Tool handlers, authentication, and dispatch for the `systemprompt` MCP
//! documentation hub.
//!
//! The server in the parent module owns the rmcp `ServerHandler` surface; this
//! module owns what happens per tool call: RBAC enforcement against the
//! registry, access auditing, and turning topic content into text artifacts.

use crate::repositories::{self, DECISION_LIMIT};
use crate::tools::{
    FetchRemoteDocsInput, GetTopicInput, GovernanceStatsInput, ListTopicsInput, SearchDocsInput,
};
use crate::topics;
use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::service::{RequestContext, RoleServer};
use std::future::{self, Future};
use systemprompt::database::DbPool;
use systemprompt::identifiers::McpExecutionId;
use systemprompt::mcp::middleware::enforce_rbac_from_registry;
use systemprompt::mcp::{McpToolExecutor, McpToolHandler};
use systemprompt::models::artifacts::{CliArtifact, TextArtifact};
use systemprompt::models::execution::context::RequestContext as SysRequestContext;
use systemprompt::security::authz::SharedAuthzHook;
use systemprompt_mcp_shared::{record_mcp_access, record_mcp_access_rejected};

/// How long `fetch_remote_docs` waits before giving up. Short on purpose: where
/// the boundary is a firewall rather than a refusal the failure mode is a hang,
/// and a demo tool that appears to freeze teaches the wrong lesson about what
/// stopped it.
const REMOTE_FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Host and port `fetch_remote_docs` dials. Deliberately the public docs site
/// on 443 — the thing a deployment with no egress must not be able to reach.
const REMOTE_FETCH_HOST: &str = "systemprompt.io";
const REMOTE_FETCH_PORT: u16 = 443;

fn text_artifact(title: &str, body: impl Into<String>) -> CliArtifact {
    CliArtifact::text(TextArtifact::new(body).with_title(title))
}

// Why: `list_topics` — enumerate every documentation topic.
pub(super) struct ListTopicsHandler;

impl McpToolHandler for ListTopicsHandler {
    type Input = ListTopicsInput;
    type Output = CliArtifact;

    fn tool_name(&self) -> &'static str {
        "list_topics"
    }

    fn description(&self) -> &'static str {
        "List every systemprompt.io documentation topic."
    }

    fn handle(
        &self,
        _input: Self::Input,
        _ctx: &SysRequestContext,
        _exec_id: &McpExecutionId,
    ) -> impl Future<Output = Result<(Self::Output, String), McpError>> + Send {
        let mut body = String::from("# systemprompt.io documentation topics\n\n");
        for topic in topics::TOPICS {
            body.push_str(&format!(
                "- **{}** (`{}`) — {}\n",
                topic.title, topic.id, topic.summary
            ));
        }
        body.push_str(
            "\nRead one with `get_topic {\"topic_id\": \"<id>\"}` or search with `search_docs`.\n",
        );
        let summary = format!("{} documentation topics available", topics::TOPICS.len());
        future::ready(Ok((text_artifact("Documentation Topics", body), summary)))
    }
}

// Why: `get_topic` — return the full Markdown of one topic.
pub(super) struct GetTopicHandler;

impl McpToolHandler for GetTopicHandler {
    type Input = GetTopicInput;
    type Output = CliArtifact;

    fn tool_name(&self) -> &'static str {
        "get_topic"
    }

    fn description(&self) -> &'static str {
        "Return the full Markdown of one documentation topic by id."
    }

    fn handle(
        &self,
        input: Self::Input,
        _ctx: &SysRequestContext,
        _exec_id: &McpExecutionId,
    ) -> impl Future<Output = Result<(Self::Output, String), McpError>> + Send {
        let Some(topic) = topics::find(&input.topic_id) else {
            let ids: Vec<&str> = topics::TOPICS.iter().map(|t| t.id).collect();
            return future::ready(Err(McpError::invalid_params(
                format!(
                    "Unknown topic '{}'. Valid topic ids: {}. Call `list_topics` to see them.",
                    input.topic_id,
                    ids.join(", ")
                ),
                None,
            )));
        };
        let summary = format!("Topic: {}", topic.title);
        future::ready(Ok((text_artifact(topic.title, topic.body), summary)))
    }
}

// Why: `search_docs` — keyword search across all topics.
pub(super) struct SearchDocsHandler;

impl McpToolHandler for SearchDocsHandler {
    type Input = SearchDocsInput;
    type Output = CliArtifact;

    fn tool_name(&self) -> &'static str {
        "search_docs"
    }

    fn description(&self) -> &'static str {
        "Keyword search across all documentation topics."
    }

    fn handle(
        &self,
        input: Self::Input,
        _ctx: &SysRequestContext,
        _exec_id: &McpExecutionId,
    ) -> impl Future<Output = Result<(Self::Output, String), McpError>> + Send {
        let terms: Vec<String> = input
            .query
            .split_whitespace()
            .map(|t| {
                t.trim_matches(|c: char| !c.is_alphanumeric())
                    .to_lowercase()
            })
            .filter(|t| t.len() >= 2)
            .collect();

        let hits = topics::search(&input.query);

        let mut body = format!("# Search results for: {}\n\n", input.query.trim());
        if hits.is_empty() {
            body.push_str(
                "No topics matched. Try broader terms, or call `list_topics` to browse everything.\n",
            );
        } else {
            for hit in &hits {
                body.push_str(&format!(
                    "## {} (`{}`)\n\n{}\n\n> {}\n\n",
                    hit.topic.title,
                    hit.topic.id,
                    hit.topic.summary,
                    topics::excerpt(hit.topic, &terms)
                ));
            }
            body.push_str("Read any of these in full with `get_topic {\"topic_id\": \"<id>\"}`.\n");
        }

        let summary = format!("{} topic(s) matched \"{}\"", hits.len(), input.query.trim());
        future::ready(Ok((text_artifact("Search Results", body), summary)))
    }
}

/// `governance_stats` — read the caller's own audit rows back.
///
/// Holds a pool because this is the one handler that answers from the database
/// rather than from compiled-in content. The caller is taken from the
/// authenticated request context, never from the input, which is why the input
/// type has no fields.
pub(super) struct GovernanceStatsHandler {
    pub(super) db_pool: DbPool,
}

impl McpToolHandler for GovernanceStatsHandler {
    type Input = GovernanceStatsInput;
    type Output = CliArtifact;

    fn tool_name(&self) -> &'static str {
        "governance_stats"
    }

    fn description(&self) -> &'static str {
        "Return the calling identity's own governance verdicts, spend, and tool fires."
    }

    fn handle(
        &self,
        _input: Self::Input,
        ctx: &SysRequestContext,
        _exec_id: &McpExecutionId,
    ) -> impl Future<Output = Result<(Self::Output, String), McpError>> + Send {
        let db_pool = std::sync::Arc::<systemprompt::database::Database>::clone(&self.db_pool);
        let user_id = ctx.user_id().clone();
        async move {
            // No pool means the server started without a database. Reporting
            // that plainly beats an empty table, which would read as "nothing
            // was governed" rather than "nothing could be read".
            let Some(pool) = db_pool.pool() else {
                return Err(McpError::internal_error(
                    "This server has no database connection, so the governance spine \
                     cannot be read.",
                    None,
                ));
            };
            let pool = pool.as_ref();
            let stats = Spine {
                tallies: repositories::list_policy_tallies(pool, &user_id)
                    .await
                    .map_err(|e| db_error(&e))?,
                decisions: repositories::list_recent_decisions(pool, &user_id, DECISION_LIMIT)
                    .await
                    .map_err(|e| db_error(&e))?,
                spend: repositories::get_spend(pool, &user_id)
                    .await
                    .map_err(|e| db_error(&e))?,
                fires: repositories::list_tool_fires(pool, &user_id, DECISION_LIMIT)
                    .await
                    .map_err(|e| db_error(&e))?,
            };

            let summary = format!(
                "{} allowed, {} denied, {} request(s)",
                stats.allowed(),
                stats.denied(),
                stats.spend.requests
            );
            Ok((
                text_artifact("Governance Statistics", render_spine(&stats)),
                summary,
            ))
        }
    }
}

/// One caller's spine, as the four queries return it.
struct Spine {
    tallies: Vec<repositories::PolicyTally>,
    decisions: Vec<repositories::DecisionRow>,
    spend: repositories::SpendRow,
    fires: Vec<repositories::ToolFireRow>,
}

impl Spine {
    fn allowed(&self) -> i64 {
        self.tallies.iter().map(|t| t.allowed).sum()
    }

    fn denied(&self) -> i64 {
        self.tallies.iter().map(|t| t.denied).sum()
    }
}

/// Render the spine as Markdown for a model to read and summarise.
///
/// Every section says something explicit when it is empty. A blank section
/// reads to a model as "nothing to report", and it would then tell the user
/// that nothing was governed — the opposite of what an empty table means here.
fn render_spine(stats: &Spine) -> String {
    let mut body = String::from("# Governance statistics for the calling identity\n\n## Spend\n\n");

    let spend = &stats.spend;
    body.push_str(&format!(
        "- Provider requests: {}\n- Tokens: {} in / {} out\n- Cost: ${:.4}\n",
        spend.requests,
        spend.input_tokens,
        spend.output_tokens,
        spend.cost_microdollars as f64 / 1_000_000.0,
    ));
    match spend.mean_latency_ms {
        Some(ms) => body.push_str(&format!("- Mean latency: {ms:.0} ms\n")),
        None => body.push_str("- Mean latency: no completed request yet\n"),
    }
    body.push_str(&format!(
        "- Most recent model: {}\n\n## Verdicts by policy\n\n",
        spend.model.as_deref().unwrap_or("none reached yet")
    ));

    if stats.tallies.is_empty() {
        body.push_str("No governance decisions recorded for this identity yet.\n\n");
    } else {
        body.push_str(&format!(
            "{} allowed, {} denied across all policies.\n\n\
             | Policy | Allowed | Denied |\n|---|---|---|\n",
            stats.allowed(),
            stats.denied()
        ));
        for tally in &stats.tallies {
            body.push_str(&format!(
                "| `{}` | {} | {} |\n",
                tally.policy, tally.allowed, tally.denied
            ));
        }
        body.push('\n');
    }

    body.push_str(&format!(
        "## Recent decisions (newest {DECISION_LIMIT} max)\n\n"
    ));
    if stats.decisions.is_empty() {
        body.push_str("None.\n\n");
    } else {
        body.push_str("| When | Tool | Outcome | Policy | Reason |\n|---|---|---|---|---|\n");
        for row in &stats.decisions {
            // A pipe in a policy reason would silently break the table, and the
            // reason is the one field here written by something other than this
            // crate.
            let reason = row.reason.replace('|', "\\|");
            body.push_str(&format!(
                "| {} | `{}` | {} | `{}` | {} |\n",
                row.at.format("%Y-%m-%d %H:%M:%S"),
                row.tool_name,
                row.decision,
                row.policy,
                reason
            ));
        }
        body.push('\n');
    }

    body.push_str("## Tools that actually ran\n\n");
    if stats.fires.is_empty() {
        body.push_str("None recorded.\n");
    } else {
        for row in &stats.fires {
            body.push_str(&format!("- `{}` — {} fire(s)\n", row.tool_name, row.fires));
        }
    }
    body
}

/// `fetch_remote_docs` — the tool policy is expected to refuse.
///
/// It is implemented rather than stubbed. A refusal demonstration is only worth
/// watching if the thing refused could genuinely have happened: a stub that
/// returns "would have fetched" proves that a string was returned, not that a
/// boundary held. Reaching this code at all means the `tool_blocklist` policy
/// was bypassed or disabled, so it says so.
///
/// It opens a TCP connection rather than speaking HTTPS. That is the whole of
/// what the boundary underneath the policy actually controls — Landlock's
/// network rules gate `connect()` by port, and this session's jail grants the
/// gateway's port alone — so a successful connect is the honest evidence that
/// egress was possible, and a TLS stack would add a large dependency to prove
/// nothing further.
pub(super) struct FetchRemoteDocsHandler;

impl McpToolHandler for FetchRemoteDocsHandler {
    type Input = FetchRemoteDocsInput;
    type Output = CliArtifact;

    fn tool_name(&self) -> &'static str {
        "fetch_remote_docs"
    }

    fn description(&self) -> &'static str {
        "Fetch a documentation page from the public site. Expected to be refused by policy."
    }

    fn handle(
        &self,
        input: Self::Input,
        _ctx: &SysRequestContext,
        _exec_id: &McpExecutionId,
    ) -> impl Future<Output = Result<(Self::Output, String), McpError>> + Send {
        let target = format!("{REMOTE_FETCH_HOST}:{REMOTE_FETCH_PORT}");
        let path = format!("/{}", input.path.trim_start_matches('/'));
        async move {
            tracing::warn!(
                target = %target,
                path = %path,
                "fetch_remote_docs executed: the tool_blocklist policy did not stop it"
            );

            let attempt =
                tokio::time::timeout(REMOTE_FETCH_TIMEOUT, tokio::net::TcpStream::connect(&target))
                    .await;

            let (body, summary) = match attempt {
                Ok(Ok(stream)) => {
                    let peer = stream
                        .peer_addr()
                        .map_or_else(|_| target.clone(), |addr| addr.to_string());
                    (
                        format!(
                            "# Egress succeeded\n\n\
                             Opened a TCP connection to `{target}` ({peer}) while trying to \
                             fetch `{path}`.\n\n\
                             **This deployment was not supposed to permit that.** The \
                             `tool_blocklist` policy should have refused the call at the gate, \
                             and the session's sandbox should have refused the connection. \
                             Either both were bypassed or disabled, or the caller holds a \
                             scope exempt from the blocklist — admin callers are. Worth \
                             checking before presenting this as a governance demonstration.\n"
                        ),
                        format!("egress to {target} succeeded — no boundary held"),
                    )
                },
                Ok(Err(e)) => (
                    format!(
                        "# Egress refused\n\n\
                         Could not connect to `{target}` while trying to fetch `{path}`: {e}\n\n\
                         The call reached this tool, which means the `tool_blocklist` policy \
                         did not refuse it — the connection was stopped one layer down \
                         instead. This session's sandbox permits outbound TCP to the \
                         gateway's port alone. The policy chain is the layer that produces \
                         a reason a person can read; this is the layer that holds when the \
                         configuration above it is wrong.\n"
                    ),
                    format!("egress to {target} refused at the network boundary"),
                ),
                Err(_) => (
                    format!(
                        "# Egress timed out\n\n\
                         No response from `{target}` within {}s while trying to fetch \
                         `{path}`.\n\n\
                         A timeout rather than a refusal usually means a firewall is \
                         dropping the packets silently, rather than the kernel refusing the \
                         `connect()`. Either way nothing left this host — but note that the \
                         `tool_blocklist` policy did not stop the call, which it should \
                         have.\n",
                        REMOTE_FETCH_TIMEOUT.as_secs()
                    ),
                    format!("egress to {target} timed out"),
                ),
            };
            Ok((text_artifact("Upstream Documentation Fetch", body), summary))
        }
    }
}

fn db_error(e: &sqlx::Error) -> McpError {
    tracing::error!(error = %e, "governance_stats query failed");
    McpError::internal_error("Could not read the governance spine.", None)
}

pub(super) async fn authenticate_tool_request(
    db_pool: &DbPool,
    tool_name: &str,
    service_id: &str,
    ctx: &RequestContext<RoleServer>,
    authz_hook: &SharedAuthzHook,
) -> Result<(SysRequestContext, String), McpError> {
    let server_name = service_id;
    let rbac_result = enforce_rbac_from_registry(ctx, service_id, authz_hook).await;

    match rbac_result {
        Ok(result) => {
            match result
                .expect_authenticated("BUG: systemprompt requires OAuth but auth was not enforced")
            {
                Ok(authenticated) => {
                    record_mcp_access(
                        db_pool,
                        authenticated.context.user_id(),
                        server_name,
                        tool_name,
                        "authenticated",
                    )
                    .await;
                    let token = authenticated.token().to_owned();
                    Ok((authenticated.context.clone(), token))
                },
                Err(e) => {
                    record_mcp_access_rejected(db_pool, server_name, tool_name, e.message.as_ref())
                        .await;
                    Err(e)
                },
            }
        },
        Err(e) => {
            record_mcp_access_rejected(db_pool, server_name, tool_name, &format!("{e}")).await;
            Err(e)
        },
    }
}

pub(super) async fn dispatch_tool(
    executor: &McpToolExecutor,
    db_pool: &DbPool,
    tool_name: &str,
    request: &CallToolRequestParams,
    request_context: &SysRequestContext,
) -> Result<CallToolResult, McpError> {
    match tool_name {
        "list_topics" => {
            executor
                .execute(&ListTopicsHandler, request, request_context)
                .await
        },
        "get_topic" => {
            executor
                .execute(&GetTopicHandler, request, request_context)
                .await
        },
        "search_docs" => {
            executor
                .execute(&SearchDocsHandler, request, request_context)
                .await
        },
        "governance_stats" => {
            executor
                .execute(
                    &GovernanceStatsHandler {
                        db_pool: std::sync::Arc::<systemprompt::database::Database>::clone(db_pool),
                    },
                    request,
                    request_context,
                )
                .await
        },
        "fetch_remote_docs" => {
            executor
                .execute(&FetchRemoteDocsHandler, request, request_context)
                .await
        },
        _ => Err(McpError::invalid_params(
            format!(
                "Unknown tool: '{tool_name}'. Available tools: list_topics, get_topic, \
                 search_docs, governance_stats, fetch_remote_docs. Call `list_topics` \
                 first to see the documentation topics."
            ),
            None,
        )),
    }
}
