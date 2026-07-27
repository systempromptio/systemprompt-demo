//! One live pi conversation: its child process, its viewers, and the tool calls
//! currently waiting on a human.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use systemprompt::identifiers::{SessionId, UserId};
use tokio::io::AsyncWriteExt as _;
use tokio::process::{Child, ChildStdin};
use tokio::sync::{broadcast, oneshot};

use super::events::{PiEvent, PiEventBody};

/// Frames retained for `Last-Event-ID` replay after a reconnect. Two hundred
/// covers a long turn's worth of deltas without letting a forgotten tab pin
/// unbounded memory.
const REPLAY_CAPACITY: usize = 200;

/// Broadcast backlog. A viewer that falls this far behind is dropped and must
/// reconnect, which the replay buffer then repairs.
const BROADCAST_CAPACITY: usize = 512;

/// How a pending approval was settled.
#[derive(Debug, Clone, Copy)]
pub(super) enum Verdict {
    Allow,
    Deny,
}

/// Constructor arguments, grouped because they are always assembled together at
/// the one call site that spawns a child.
pub(super) struct PiSessionInit {
    pub(super) conversation_id: String,
    pub(super) user_id: UserId,
    pub(super) attested_session: SessionId,
    pub(super) api_key_id: String,
    pub(super) workspace: PathBuf,
    pub(super) child: Child,
    pub(super) stdin: ChildStdin,
}

pub(super) struct PiSession {
    pub(super) conversation_id: String,
    pub(super) user_id: UserId,
    /// The server-issued session both spines key on.
    pub(super) attested_session: SessionId,
    /// The PAT minted for this conversation, so teardown knows what to revoke.
    /// The secret itself is never held here — it lives only in the child's
    /// `models.json` — but the id is all revocation needs.
    pub(super) api_key_id: String,
    pub(super) workspace: PathBuf,

    stdin: tokio::sync::Mutex<Option<ChildStdin>>,
    child: tokio::sync::Mutex<Option<Child>>,
    events: broadcast::Sender<PiEvent>,
    replay: Mutex<VecDeque<PiEvent>>,
    pending: Mutex<HashMap<String, oneshot::Sender<Verdict>>>,
    seq: AtomicU64,
    last_activity: Mutex<Instant>,
    started: Instant,
    closed: AtomicBool,
}

impl PiSession {
    pub(super) fn new(init: PiSessionInit) -> Self {
        let PiSessionInit {
            conversation_id,
            user_id,
            attested_session,
            api_key_id,
            workspace,
            child,
            stdin,
        } = init;
        let (events, _) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            conversation_id,
            user_id,
            attested_session,
            api_key_id,
            workspace,
            stdin: tokio::sync::Mutex::new(Some(stdin)),
            child: tokio::sync::Mutex::new(Some(child)),
            events,
            replay: Mutex::new(VecDeque::with_capacity(REPLAY_CAPACITY)),
            pending: Mutex::new(HashMap::new()),
            seq: AtomicU64::new(0),
            last_activity: Mutex::new(Instant::now()),
            started: Instant::now(),
            closed: AtomicBool::new(false),
        }
    }

    /// Publish one frame to every viewer and retain it for replay.
    ///
    /// A send failure means nobody is watching, which is normal and not an
    /// error — the frame still goes in the replay buffer so a reconnecting
    /// viewer catches up.
    pub(super) fn emit(&self, body: PiEventBody) -> u64 {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let event = PiEvent::new(seq, body);
        if let Ok(mut replay) = self.replay.lock() {
            if replay.len() == REPLAY_CAPACITY {
                replay.pop_front();
            }
            replay.push_back(event.clone());
        }
        _ = self.events.send(event);
        seq
    }

    pub(super) fn subscribe(&self) -> broadcast::Receiver<PiEvent> {
        self.events.subscribe()
    }

    pub(super) fn replay_since(&self, after_seq: u64) -> Vec<PiEvent> {
        self.replay.lock().map_or_else(
            |_| Vec::new(),
            |r| r.iter().filter(|e| e.seq() > after_seq).cloned().collect(),
        )
    }

    /// True when no viewer is attached, used to abandon approvals nobody can
    /// answer rather than making the model wait out the full timeout.
    pub(super) fn has_viewers(&self) -> bool {
        self.events.receiver_count() > 0
    }

    pub(super) fn touch(&self) {
        if let Ok(mut t) = self.last_activity.lock() {
            *t = Instant::now();
        }
    }

    pub(super) fn idle_for(&self) -> Duration {
        self.last_activity
            .lock()
            .map_or(Duration::ZERO, |t| t.elapsed())
    }

    pub(super) fn age(&self) -> Duration {
        self.started.elapsed()
    }

    pub(super) fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    /// Write one JSONL line to the child.
    ///
    /// Serialised through a mutex because two concurrent writes would
    /// interleave mid-line and desynchronise pi's parser for the rest of
    /// the session.
    pub(super) async fn write_line(&self, line: &str) -> std::io::Result<()> {
        let mut guard = self.stdin.lock().await;
        let result = match guard.as_mut() {
            Some(stdin) => {
                stdin.write_all(line.as_bytes()).await?;
                stdin.flush().await
            },
            None => Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "pi session is closed",
            )),
        };
        drop(guard);
        result
    }

    pub(super) fn park_approval(&self, approval_id: String) -> oneshot::Receiver<Verdict> {
        let (tx, rx) = oneshot::channel();
        if let Ok(mut pending) = self.pending.lock() {
            pending.insert(approval_id, tx);
        }
        rx
    }

    /// Settle a pending approval. `false` when the id is unknown or already
    /// resolved, which the API surfaces as a conflict so the widget can show
    /// "expired" rather than silently doing nothing.
    pub(super) fn resolve_approval(&self, approval_id: &str, verdict: Verdict) -> bool {
        let Ok(mut pending) = self.pending.lock() else {
            return false;
        };
        pending
            .remove(approval_id)
            .is_some_and(|tx| tx.send(verdict).is_ok())
    }

    pub(super) fn forget_approval(&self, approval_id: &str) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(approval_id);
        }
    }

    /// Kill the child and drop every waiter.
    ///
    /// Dropping the pending senders is what makes teardown fail closed: each
    /// waiting gate sees its receiver close and denies, so no tool runs on the
    /// way out.
    pub(super) async fn close(&self, code: Option<i32>) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Ok(mut pending) = self.pending.lock() {
            pending.clear();
        }
        // Closing stdin asks pi to exit on its own before we escalate.
        drop(self.stdin.lock().await.take());
        let child = self.child.lock().await.take();
        if let Some(mut child) = child {
            _ = child.kill().await;
        }
        self.emit(PiEventBody::Exit { code });
        super::spawn::cleanup(&self.workspace).await;
    }
}
