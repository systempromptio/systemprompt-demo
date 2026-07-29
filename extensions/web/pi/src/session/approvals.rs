//! Who has answered for this session's tool calls, and who is going to.
//!
//! Three pieces of the same question, kept together because the gate reads
//! them in one breath: the mode (does anyone get asked at all), the standing
//! rules (has someone already answered for this tool), and the calls parked
//! mid-flight waiting on a click.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use systemprompt::identifiers::UserId;
use tokio::sync::oneshot;

// Why: who answered an approval, and when they clicked — captured at the HTTP
// handler where the embed token was verified, not at audit-write time.
#[derive(Debug, Clone)]
pub(crate) struct Attribution {
    pub(crate) user_id: UserId,
    pub(crate) username: String,
    pub(crate) decided_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub(crate) enum Verdict {
    Allow(Attribution),
    Deny(Attribution),
}

struct Parked {
    tool_name: String,
    tx: oneshot::Sender<Verdict>,
}

pub(crate) struct Approvals {
    pending: Mutex<HashMap<String, Parked>>,
    // Why: keyed by tool name, holding the attribution of the click that armed
    // it — every later skip is stamped with the person who pre-answered.
    standing: Mutex<HashMap<String, Attribution>>,
    // Why: read per call rather than per session, so a mid-turn flip applies to
    // the very next tool call.
    manual: AtomicBool,
}

impl Approvals {
    pub(super) fn new(manual: bool) -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            standing: Mutex::new(HashMap::new()),
            manual: AtomicBool::new(manual),
        }
    }

    pub(crate) fn manual(&self) -> bool {
        self.manual.load(Ordering::SeqCst)
    }

    pub(crate) fn set_manual(&self, manual: bool) {
        self.manual.store(manual, Ordering::SeqCst);
    }

    pub(crate) fn park(
        &self,
        approval_id: String,
        tool_name: String,
    ) -> oneshot::Receiver<Verdict> {
        let (tx, rx) = oneshot::channel();
        if let Ok(mut pending) = self.pending.lock() {
            pending.insert(approval_id, Parked { tool_name, tx });
        }
        rx
    }

    // Why: `arm_standing` is answered from the parked entry rather than from the
    // request body — the browser names an approval, never a tool, so a client
    // cannot arm a standing rule for a tool it was not asked about.
    pub(crate) fn resolve(&self, approval_id: &str, verdict: Verdict, arm_standing: bool) -> bool {
        let Ok(mut pending) = self.pending.lock() else {
            return false;
        };
        let Some(parked) = pending.remove(approval_id) else {
            return false;
        };
        drop(pending);
        let attribution = match &verdict {
            Verdict::Allow(a) => Some(a.clone()),
            Verdict::Deny(_) => None,
        };
        if parked.tx.send(verdict).is_err() {
            return false;
        }
        if arm_standing
            && let Some(attribution) = attribution
            && let Ok(mut standing) = self.standing.lock()
        {
            standing.insert(parked.tool_name, attribution);
        }
        true
    }

    pub(crate) fn standing_for(&self, tool_name: &str) -> Option<Attribution> {
        self.standing
            .lock()
            .ok()
            .and_then(|standing| standing.get(tool_name).cloned())
    }

    pub(crate) fn forget(&self, approval_id: &str) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(approval_id);
        }
    }

    pub(super) fn clear(&self) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.clear();
        }
        if let Ok(mut standing) = self.standing.lock() {
            standing.clear();
        }
    }
}
