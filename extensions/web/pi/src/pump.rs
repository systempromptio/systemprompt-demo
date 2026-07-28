//! The tasks that own a child's pipes.
//!
//! One reader per session owns stdout and is the only thing that ever answers
//! an `extension_ui_request`. Each request is handled in its own task so
//! several concurrent tool calls can be pending at once — the model can issue
//! parallel calls, and serialising them here would deadlock the second behind
//! the first's human.

use std::sync::Arc;

use tokio::io::{AsyncBufReadExt as _, BufReader};
use tokio::process::{ChildStderr, ChildStdout};

use super::events::{self, PiEventBody};
use super::gate::{self, PiDeps};
use super::registry::PiRegistry;
use super::rpc::{self, ExtensionUiResponse, RpcFrame};
use super::session::PiSession;

pub(super) fn start(
    registry: PiRegistry,
    deps: Arc<PiDeps>,
    session: Arc<PiSession>,
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
) {
    if let Some(stdout) = stdout {
        tokio::spawn(read_stdout(registry, deps, Arc::clone(&session), stdout));
    }
    if let Some(stderr) = stderr {
        tokio::spawn(read_stderr(session, stderr));
    }
}

async fn read_stdout(
    registry: PiRegistry,
    deps: Arc<PiDeps>,
    session: Arc<PiSession>,
    stdout: ChildStdout,
) {
    let mut lines = BufReader::new(stdout).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => handle_line(&deps, &session, &line),
            Ok(None) => break,
            Err(e) => {
                tracing::warn!(
                    conversation_id = %session.conversation_id,
                    error = %e,
                    "pi stdout read failed"
                );
                break;
            },
        }
    }
    registry
        .remove_if(&session.conversation_id, &session, None)
        .await;
}

fn handle_line(deps: &Arc<PiDeps>, session: &Arc<PiSession>, line: &str) {
    match rpc::parse_frame(line) {
        RpcFrame::UiRequest(req) => {
            if req.method != "confirm" {
                tracing::debug!(
                    conversation_id = %session.conversation_id,
                    method = %req.method,
                    "ignoring non-confirm extension UI request"
                );
                return;
            }
            let Ok(payload) = serde_json::from_str::<rpc::GovernancePayload>(&req.message) else {
                deny_unparseable(session, req.id);
                return;
            };
            let deps = Arc::clone(deps);
            let session = Arc::clone(session);
            tokio::spawn(async move {
                let allow = gate::decide(&deps, &session, &req.id, &payload).await;
                answer(&session, &req.id, allow).await;
                // Why: gate::decide has landed its audit rows by now, so this
                // is the earliest point a snapshot would include the decision
                super::stats::push_soon(Arc::clone(&deps.pool), session);
            });
        },
        RpcFrame::Response { success, error } => {
            if !success {
                let message = error.unwrap_or_else(|| "pi rejected the command".to_owned());
                session.emit(PiEventBody::rpc_error(message));
            }
        },
        RpcFrame::Event(value) => {
            if let Some(body) = events::translate(&value) {
                let settled = matches!(body, PiEventBody::TurnEnd | PiEventBody::ToolEnd { .. });
                session.emit(body);
                if settled {
                    super::stats::push_soon(Arc::clone(&deps.pool), Arc::clone(session));
                }
            }
        },
        RpcFrame::Unparseable(raw) => {
            tracing::debug!(
                conversation_id = %session.conversation_id,
                line = %raw,
                "non-JSON line on pi stdout"
            );
        },
    }
}

fn deny_unparseable(session: &Arc<PiSession>, id: String) {
    let session = Arc::clone(session);
    tokio::spawn(async move {
        session.emit(PiEventBody::governance_error(
            "unparseable approval request — denied",
        ));
        answer(&session, &id, false).await;
    });
}

async fn answer(session: &Arc<PiSession>, id: &str, allow: bool) {
    let Ok(line) = ExtensionUiResponse::new(id, allow).to_line() else {
        return;
    };
    if let Err(e) = session.write_line(&line).await {
        tracing::warn!(
            conversation_id = %session.conversation_id,
            error = %e,
            "could not answer pi's approval request; the call stays blocked"
        );
    }
}

async fn read_stderr(session: Arc<PiSession>, stderr: ChildStderr) {
    let mut lines = BufReader::new(stderr).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        // Why: also logged, because a child that dies at spawn has no viewer
        // attached yet — the broadcast frame reaches nobody, and this line is
        // the only record of why it died.
        tracing::warn!(
            conversation_id = %session.conversation_id,
            line = %line,
            "pi child stderr"
        );
        session.emit(PiEventBody::Stderr { line });
    }
}
