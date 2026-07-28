//! The task that writes a conversation down.
//!
//! One per live child, owning the receiving end of [`PiSession::emit`]'s tee.
//! It exists as a task rather than an `await` inside `emit` for two reasons:
//! the emit path is synchronous and runs on the stdout reader, and a transcript
//! that could stall the SSE stream would trade the thing a visitor is watching
//! for the thing they might read later.
//!
//! Two shaping decisions matter more than the plumbing:
//!
//! * **Text deltas are coalesced.** pi streams prose a token at a time; one row
//!   each would be a row per token, which is the wrong granularity for storage
//!   and for reading it back. Runs of `TextDelta` (and `ThinkingDelta`) are
//!   accumulated and flushed as a single frame carrying the joined text, under
//!   the `seq` of the run's first delta — so a viewer that resumes at
//!   `last_seq` still gets every frame exactly once and in order.
//! * **`Stderr` is not stored.** It is an operator signal about the child
//!   process, surfaced live because a provider misconfiguration appears nowhere
//!   else. It is not part of the conversation, and it is the frame most likely
//!   to carry something from the host into a row.

use std::sync::Arc;

use sqlx::PgPool;
use systemprompt::identifiers::ContextId;
use tokio::sync::mpsc;

use super::events::{PiEvent, PiEventBody};
use crate::repositories::pi::conversations;
use crate::repositories::pi::events::{self as event_repo, NewPiEvent};

const BATCH_FRAMES: usize = 32;

const FLUSH_INTERVAL: std::time::Duration = std::time::Duration::from_millis(750);

const TITLE_LEN: usize = 60;

pub(super) fn start(
    pool: Arc<PgPool>,
    conversation_id: ContextId,
    rx: mpsc::UnboundedReceiver<PiEvent>,
) {
    tokio::spawn(run(pool, conversation_id, rx));
}

async fn run(
    pool: Arc<PgPool>,
    conversation_id: ContextId,
    mut rx: mpsc::UnboundedReceiver<PiEvent>,
) {
    let mut pending: Vec<NewPiEvent> = Vec::new();
    let mut journal = Journal::default();
    let mut titled = false;
    let mut ticker = tokio::time::interval(FLUSH_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            received = rx.recv() => {
                let Some(event) = received else { break };
                if let Some(text) = first_user_message(&event, titled) {
                    titled = true;
                    title(&pool, &conversation_id, &text).await;
                }
                journal.absorb(&event, &mut pending);
                if pending.len() >= BATCH_FRAMES {
                    flush(&pool, &conversation_id, &mut pending).await;
                }
            },
            _ = ticker.tick() => {
                journal.settle(&mut pending);
                flush(&pool, &conversation_id, &mut pending).await;
            },
        }
    }

    journal.settle(&mut pending);
    flush(&pool, &conversation_id, &mut pending).await;
}

/// Turns a stream of frames into the rows that get stored.
///
/// All the shaping lives here so the writer loop is only batching and I/O, and
/// so the rules can be exercised without a database.
#[derive(Debug, Default)]
pub struct Journal {
    run: DeltaRun,
}

impl Journal {
    /// Fold one frame in, appending whatever rows it completes.
    ///
    /// A frame that is not a delta ends whatever run preceded it, and the
    /// accumulated prose has to land *before* it: a tool call that appeared
    /// above the sentence introducing it would read as the agent acting
    /// without saying anything.
    pub fn absorb(&mut self, event: &PiEvent, out: &mut Vec<NewPiEvent>) {
        out.extend(self.run.absorb(event));
        out.extend(storable(event));
    }

    /// Close any run still open, so a pause in the output does not leave the
    /// last sentence unwritten.
    pub fn settle(&mut self, out: &mut Vec<NewPiEvent>) {
        out.extend(self.run.take());
    }
}

#[derive(Debug, Default)]
struct DeltaRun {
    seq: i64,
    kind: &'static str,
    text: String,
}

impl DeltaRun {
    fn absorb(&mut self, event: &PiEvent) -> Option<NewPiEvent> {
        let delta = match event.body() {
            PiEventBody::TextDelta { text } => Some(("text_delta", text)),
            PiEventBody::ThinkingDelta { text } => Some(("thinking_delta", text)),
            _ => None,
        };
        let Some((kind, text)) = delta else {
            return self.take();
        };
        let flushed = (self.kind != kind).then(|| self.take()).flatten();
        if self.text.is_empty() {
            self.seq = seq_of(event);
            self.kind = kind;
        }
        self.text.push_str(text);
        flushed
    }

    fn take(&mut self) -> Option<NewPiEvent> {
        if self.text.is_empty() {
            return None;
        }
        let text = std::mem::take(&mut self.text);
        let kind = self.kind;
        self.kind = "";
        Some(NewPiEvent {
            seq: self.seq,
            kind: kind.to_owned(),
            body: serde_json::json!({ "type": kind, "text": text }),
        })
    }
}

fn storable(event: &PiEvent) -> Option<NewPiEvent> {
    let body = event.body();
    if matches!(
        body,
        PiEventBody::TextDelta { .. }
            | PiEventBody::ThinkingDelta { .. }
            | PiEventBody::Stderr { .. }
            | PiEventBody::SessionReady { .. }
            | PiEventBody::Exit { .. }
    ) {
        return None;
    }
    Some(NewPiEvent {
        seq: seq_of(event),
        kind: body.kind().to_owned(),
        body: serde_json::to_value(event)
            .inspect_err(|e| tracing::warn!(error = %e, "dropped an unserialisable pi frame"))
            .ok()?,
    })
}

fn first_user_message(event: &PiEvent, titled: bool) -> Option<String> {
    if titled {
        return None;
    }
    match event.body() {
        PiEventBody::UserMessage { text, .. } => Some(text.clone()),
        _ => None,
    }
}

async fn title(pool: &PgPool, conversation_id: &ContextId, text: &str) {
    let first_line = text.lines().next().unwrap_or(text).trim();
    if first_line.is_empty() {
        return;
    }
    let title: String = first_line.chars().take(TITLE_LEN).collect();
    if let Err(e) =
        conversations::update_conversation_title_if_unset(pool, conversation_id, &title).await
    {
        tracing::warn!(error = %e, conversation_id = %conversation_id, "could not auto-title a pi conversation");
    }
}

async fn flush(pool: &PgPool, conversation_id: &ContextId, pending: &mut Vec<NewPiEvent>) {
    if pending.is_empty() {
        return;
    }
    if let Err(e) = event_repo::insert_conversation_events(pool, conversation_id, pending).await {
        tracing::error!(
            error = %e,
            conversation_id = %conversation_id,
            frames = pending.len(),
            "could not persist pi transcript frames"
        );
    }
    pending.clear();
}

fn seq_of(event: &PiEvent) -> i64 {
    i64::try_from(event.seq()).unwrap_or(i64::MAX)
}
