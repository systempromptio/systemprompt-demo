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

use super::events::{ErrorDeduper, PiEvent, PiEventBody};
use super::ledger::CallLedger;

const REPLAY_CAPACITY: usize = 200;

const BROADCAST_CAPACITY: usize = 512;

// Why: who answered an approval, and when they clicked — captured at the HTTP
// handler where the embed token was verified, not at audit-write time.
#[derive(Debug, Clone)]
pub(crate) struct Attribution {
    pub(crate) user_id: UserId,
    pub(crate) username: String,
    pub(crate) decided_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub(super) enum Verdict {
    Allow(Attribution),
    Deny(Attribution),
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
    pub(super) calls: CallLedger,
    seq: AtomicU64,
    dedupe: Mutex<ErrorDeduper>,
    stats_push_pending: AtomicBool,
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
            calls: CallLedger::default(),
            seq: AtomicU64::new(start_seq),
            dedupe: Mutex::new(ErrorDeduper::default()),
            stats_push_pending: AtomicBool::new(false),
            last_activity: Mutex::new(Instant::now()),
            started: Instant::now(),
            closed: AtomicBool::new(false),
        }
    }

    // Why: a suppressed repeat consumes no seq, so replay windows stay gapless
    pub(super) fn emit(&self, body: PiEventBody) -> u64 {
        if self
            .dedupe
            .lock()
            .is_ok_and(|mut dedupe| dedupe.is_repeat(&body))
        {
            return self.seq.load(Ordering::SeqCst);
        }
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

    // Why: no seq, no replay, no persist — an ephemeral frame must not create
    // a gap a reconnecting viewer would read as lost transcript.
    pub(super) fn emit_ephemeral(&self, body: PiEventBody) {
        _ = self.events.send(PiEvent::ephemeral(body));
    }

    // Why: one in-flight push per session — the claimer must call
    // stats_push_done, and a false return means someone else already holds it
    pub(super) fn stats_push_begin(&self) -> bool {
        !self.stats_push_pending.swap(true, Ordering::SeqCst)
    }

    pub(super) fn stats_push_done(&self) {
        self.stats_push_pending.store(false, Ordering::SeqCst);
    }

    pub(super) fn subscribe(&self) -> broadcast::Receiver<PiEvent> {
        self.events.subscribe()
    }

    pub(super) fn replay_since(&self, after_seq: u64) -> Vec<PiEvent> {
        self.replay.lock().map_or_else(
            |_| Vec::new(),
            |r| {
                r.iter()
                    .filter(|e| e.seq().is_some_and(|s| s > after_seq))
                    .cloned()
                    .collect()
            },
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

    pub(super) async fn close(&self, code: Option<i32>) -> Option<i32> {
        if self.closed.swap(true, Ordering::SeqCst) {
            return code;
        }
        if let Ok(mut pending) = self.pending.lock() {
            pending.clear();
        }
        drop(self.stdin.lock().await.take());
        let child = self.child.lock().await.take();
        let mut code = code;
        if let Some(mut child) = child {
            // Why: the EOF path closes with no code, but a child that already
            // exited has one — reap it before the kill would erase it.
            if code.is_none()
                && let Ok(Some(status)) = child.try_wait()
            {
                code = status.code();
            }
            _ = child.kill().await;
        }
        self.emit(PiEventBody::Exit { code });
        super::spawn::cleanup(&self.workspace).await;
        code
    }
}
