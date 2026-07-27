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
use tokio::sync::mpsc;

use super::events::{PiEvent, PiEventBody};
use crate::repositories::pi::conversations;
use crate::repositories::pi::events::{self as event_repo, NewPiEvent};

/// Flush once this many frames are buffered, so a long turn does not sit
/// entirely in memory waiting for a lull.
const BATCH_FRAMES: usize = 32;

/// Longest a frame waits before it is written. Short enough that a reload
/// moments after an answer still finds it, long enough that a streaming turn
/// is a handful of round trips rather than hundreds.
const FLUSH_INTERVAL: std::time::Duration = std::time::Duration::from_millis(750);

/// Longest auto-generated title. A conversation list is scanned, not read.
const TITLE_LEN: usize = 60;

/// Start the writer for one conversation.
pub(super) fn start(
    pool: Arc<PgPool>,
    conversation_id: String,
    rx: mpsc::UnboundedReceiver<PiEvent>,
) {
    tokio::spawn(run(pool, conversation_id, rx));
}

async fn run(pool: Arc<PgPool>, conversation_id: String, mut rx: mpsc::UnboundedReceiver<PiEvent>) {
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

    // The channel closed: the session is gone. Whatever is still buffered is
    // the tail of the conversation, which is the part a viewer most wants back.
    journal.settle(&mut pending);
    flush(&pool, &conversation_id, &mut pending).await;
}

/// Turns a stream of frames into the rows that get stored.
///
/// All the shaping lives here so the writer loop is only batching and I/O, and
/// so the rules can be exercised without a database.
#[derive(Default)]
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

/// An in-progress run of same-kind streaming deltas.
#[derive(Default)]
struct DeltaRun {
    seq: i64,
    kind: &'static str,
    text: String,
}

impl DeltaRun {
    /// Fold one frame in. Returns the completed run, if this frame ended one.
    fn absorb(&mut self, event: &PiEvent) -> Option<NewPiEvent> {
        let delta = match event.body() {
            PiEventBody::TextDelta { text } => Some(("text_delta", text)),
            PiEventBody::ThinkingDelta { text } => Some(("thinking_delta", text)),
            _ => None,
        };
        let Some((kind, text)) = delta else {
            return self.take();
        };
        // A thinking run and a prose run are different frames even back to back.
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

/// The row a frame becomes, or nothing when it is not part of the transcript.
///
/// Three groups are skipped. Text and thinking deltas belong to the run being
/// coalesced. `Stderr` is an operator signal about the child process rather
/// than conversation content, and it is the frame most likely to carry
/// something from the host into a row. `SessionReady` and `Exit` each announce
/// a process starting or ending *now*, so replaying them into a restored
/// transcript would tell the viewer their live session had already exited.
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
        // The whole frame, `seq` included, so a stored transcript replays
        // through exactly the renderers the live stream feeds.
        body: serde_json::to_value(event).ok()?,
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

/// Name the conversation after its opening line.
///
/// Best-effort and fire-and-forget: an untitled conversation is a cosmetic
/// problem, and the frame that triggered it still has to be written.
async fn title(pool: &PgPool, conversation_id: &str, text: &str) {
    let first_line = text.lines().next().unwrap_or(text).trim();
    if first_line.is_empty() {
        return;
    }
    let title: String = first_line.chars().take(TITLE_LEN).collect();
    if let Err(e) =
        conversations::update_conversation_title_if_unset(pool, conversation_id, &title).await
    {
        tracing::warn!(error = %e, conversation_id, "could not auto-title a pi conversation");
    }
}

/// Write the batch, or drop it with a loud log.
///
/// Dropping is deliberate. A retry loop here would grow unboundedly behind a
/// database that is down while the child keeps producing frames, and the
/// conversation is still live and watchable — losing part of the stored
/// transcript is the cheaper failure.
async fn flush(pool: &PgPool, conversation_id: &str, pending: &mut Vec<NewPiEvent>) {
    if pending.is_empty() {
        return;
    }
    if let Err(e) = event_repo::insert_conversation_events(pool, conversation_id, pending).await {
        tracing::error!(
            error = %e,
            conversation_id,
            frames = pending.len(),
            "could not persist pi transcript frames"
        );
    }
    pending.clear();
}

/// `seq` is a `u64` on the wire and a `bigint` in Postgres. A session would
/// have to emit more frames than a signed 64-bit counter can hold for this to
/// saturate, which no child process lives long enough to do.
fn seq_of(event: &PiEvent) -> i64 {
    i64::try_from(event.seq()).unwrap_or(i64::MAX)
}
