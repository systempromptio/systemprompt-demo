//! One live pi conversation: its child process, its viewers, and the tool calls
//! currently waiting on a human.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use systemprompt::identifiers::{ContextId, SessionId, UserId};
use tokio::io::AsyncWriteExt as _;
use tokio::process::{Child, ChildStdin};
use tokio::sync::{broadcast, mpsc, oneshot};

use super::events::{PiEvent, PiEventBody};

const REPLAY_CAPACITY: usize = 200;

const BROADCAST_CAPACITY: usize = 512;

#[derive(Debug, Clone, Copy)]
pub(super) enum Verdict {
    Allow,
    Deny,
}

pub(super) struct PiSessionInit {
    pub(super) conversation_id: ContextId,
    pub(super) user_id: UserId,
    pub(super) attested_session: SessionId,
    pub(super) api_key_id: String,
    pub(super) workspace: PathBuf,
    pub(super) child: Child,
    pub(super) stdin: ChildStdin,
    pub(super) persist: mpsc::UnboundedSender<PiEvent>,
    pub(super) start_seq: u64,
}

pub(super) struct PiSession {
    pub(super) conversation_id: ContextId,
    pub(super) user_id: UserId,
    pub(super) attested_session: SessionId,
    pub(super) api_key_id: String,
    pub(super) workspace: PathBuf,

    stdin: tokio::sync::Mutex<Option<ChildStdin>>,
    child: tokio::sync::Mutex<Option<Child>>,
    events: broadcast::Sender<PiEvent>,
    persist: mpsc::UnboundedSender<PiEvent>,
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
            persist,
            start_seq,
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
            persist,
            replay: Mutex::new(VecDeque::with_capacity(REPLAY_CAPACITY)),
            pending: Mutex::new(HashMap::new()),
            seq: AtomicU64::new(start_seq),
            last_activity: Mutex::new(Instant::now()),
            started: Instant::now(),
            closed: AtomicBool::new(false),
        }
    }

    pub(super) fn emit(&self, body: PiEventBody) -> u64 {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let event = PiEvent::new(seq, body);
        if let Ok(mut replay) = self.replay.lock() {
            if replay.len() == REPLAY_CAPACITY {
                replay.pop_front();
            }
            replay.push_back(event.clone());
        }
        if self.persist.send(event.clone()).is_err() {
            tracing::debug!(
                conversation_id = %self.conversation_id,
                "pi transcript writer has stopped; frame not persisted"
            );
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

    pub(super) async fn close(&self, code: Option<i32>) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Ok(mut pending) = self.pending.lock() {
            pending.clear();
        }
        drop(self.stdin.lock().await.take());
        let child = self.child.lock().await.take();
        if let Some(mut child) = child {
            _ = child.kill().await;
        }
        self.emit(PiEventBody::Exit { code });
        super::spawn::cleanup(&self.workspace).await;
    }
}
