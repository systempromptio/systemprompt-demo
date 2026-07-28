//! `fetch_remote_docs` — the tool that is meant to be refused.
//!
//! It dials the public docs site on 443, which is precisely what a deployment
//! with no egress must not be able to reach. Its value is the refusal: a
//! governed denial a reader can watch land in the audit record.

use crate::tools::FetchRemoteDocsInput;
use rmcp::ErrorData as McpError;
use std::future::Future;
use systemprompt::identifiers::McpExecutionId;
use systemprompt::mcp::McpToolHandler;
use systemprompt::models::artifacts::CliArtifact;
use systemprompt::models::execution::context::RequestContext as SysRequestContext;

use super::text_artifact;

use super::{REMOTE_FETCH_HOST, REMOTE_FETCH_PORT, REMOTE_FETCH_TIMEOUT};

// Why: implemented rather than stubbed — a refusal is only evidence if the
// refused thing could genuinely happen. Bare TCP, not HTTPS: Landlock gates
// `connect()` by port, so a successful connect is the whole proof and a TLS
// stack would prove nothing further.
pub(in crate::server) struct FetchRemoteDocsHandler;

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

            let attempt = tokio::time::timeout(
                REMOTE_FETCH_TIMEOUT,
                tokio::net::TcpStream::connect(&target),
            )
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
