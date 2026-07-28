//! The live-session table, plus the reaper that keeps it honest.
//!
//! Held as an axum `Extension` layer, mirroring how core injects
//! `CliBinaryPath`. Unlike the one-shot `/api/v1/admin/cli` endpoint, a session
//! here outlives the request that created it, so something has to own process
//! lifetime — that is this module.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use sqlx::PgPool;
use systemprompt::identifiers::{ContextId, SessionId, UserId};
use systemprompt::traits::AnalyticsProvider;

use super::config::PiConfig;
use super::credentials;
use super::session::PiSession;

mod admission;
mod reaper;
mod waitlist;

pub(super) use admission::SpawnError;

#[derive(Clone)]
pub struct PiRegistry(Arc<Inner>);

impl std::fmt::Debug for PiRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PiRegistry").finish_non_exhaustive()
    }
}

// Why: a slot is claimed under the lock *before* the spawn's first await, so
// the cap can never be double-passed; `Reserving` is what fills the gap
// between the claim and the live session.
enum Slot {
    Reserving {
        user_id: UserId,
        at: std::time::Instant,
    },
    Live(Arc<PiSession>),
}

impl Slot {
    fn user_id(&self) -> &UserId {
        match self {
            Self::Reserving { user_id, .. } => user_id,
            Self::Live(s) => &s.user_id,
        }
    }

    const fn live(&self) -> Option<&Arc<PiSession>> {
        match self {
            Self::Reserving { .. } => None,
            Self::Live(s) => Some(s),
        }
    }
}

// Why: a reservation older than this with no live session behind it can only
// be a leaked error path; the reaper clears it.
const STALE_RESERVATION: std::time::Duration = std::time::Duration::from_secs(60);

struct Inner {
    cfg: PiConfig,
    pool: Arc<PgPool>,
    analytics: Arc<dyn AnalyticsProvider>,
    sessions: Mutex<HashMap<ContextId, Slot>>,
    waitlist: Mutex<std::collections::VecDeque<waitlist::Waiter>>,
    version_gate: tokio::sync::OnceCell<Result<(), String>>,
}

impl PiRegistry {
    pub(crate) fn new(
        cfg: PiConfig,
        pool: Arc<PgPool>,
        analytics: Arc<dyn AnalyticsProvider>,
    ) -> Self {
        let registry = Self(Arc::new(Inner {
            cfg,
            pool,
            analytics,
            sessions: Mutex::new(HashMap::new()),
            waitlist: Mutex::new(std::collections::VecDeque::new()),
            version_gate: tokio::sync::OnceCell::new(),
        }));
        registry.spawn_reaper();
        registry
    }

    pub(super) fn config(&self) -> &PiConfig {
        &self.0.cfg
    }

    // Why: probed once per process and cached — the binary path is fixed for
    // the process lifetime, so a per-spawn probe would only add latency.
    pub(super) async fn version_gate(&self) -> Result<(), String> {
        self.0
            .version_gate
            .get_or_init(|| async { super::version::assert_supported(&self.0.cfg).await })
            .await
            .clone()
    }

    pub(super) fn get(&self, conversation_id: &ContextId) -> Option<Arc<PiSession>> {
        self.0
            .sessions
            .lock()
            .inspect_err(|_| {
                tracing::error!("pi session registry mutex poisoned; every lookup will miss");
            })
            .ok()?
            .get(conversation_id)
            .and_then(Slot::live)
            .map(Arc::clone)
    }

    // Why: reservations count toward occupancy — a claimed slot is spoken for
    // even before its child is up.
    pub(super) fn occupancy(&self) -> (usize, usize) {
        let used = self.0.sessions.lock().map_or(0, |s| s.len());
        (used, self.0.cfg.max_sessions_total)
    }

    pub(super) async fn remove(&self, conversation_id: &ContextId, code: Option<i32>) {
        let slot = {
            let Ok(mut sessions) = self.0.sessions.lock() else {
                return;
            };
            sessions.remove(conversation_id)
        };
        self.tear_down(conversation_id, slot, code).await;
    }

    // Why: a conversation id can be re-admitted (resume) while the previous
    // child's pump task is still draining. That task must only retire the slot
    // it owns — a blind remove would delete its successor, leaving a live
    // session unreachable and every lookup for it a 404.
    pub(super) async fn remove_if(
        &self,
        conversation_id: &ContextId,
        expected: &Arc<PiSession>,
        code: Option<i32>,
    ) {
        let slot = {
            let Ok(mut sessions) = self.0.sessions.lock() else {
                return;
            };
            let is_ours = sessions
                .get(conversation_id)
                .and_then(Slot::live)
                .is_some_and(|s| Arc::ptr_eq(s, expected));
            if !is_ours {
                return;
            }
            sessions.remove(conversation_id)
        };
        self.tear_down(conversation_id, slot, code).await;
    }

    async fn tear_down(&self, conversation_id: &ContextId, slot: Option<Slot>, code: Option<i32>) {
        if let Some(session) = slot.as_ref().and_then(Slot::live).map(Arc::clone) {
            let fast_exit = session.age() < std::time::Duration::from_secs(2);
            let code = session.close(code).await;
            if fast_exit {
                tracing::warn!(
                    conversation_id = %session.conversation_id,
                    exit_code = ?code,
                    "pi child died within 2s of spawn; its stderr lines above say \
                     why — usual causes are sp-pi-jail refusing to run (no Landlock \
                     in the kernel) or limits.* in services/config/pi.yaml below \
                     what node needs at startup"
                );
            }
            if let Err(e) = crate::repositories::conversations::update_conversation_closed(
                &self.0.pool,
                conversation_id,
            )
            .await
            {
                tracing::warn!(
                    conversation_id = %session.conversation_id,
                    error = %e,
                    "could not mark a pi conversation closed"
                );
            }
            credentials::revoke(&self.0.pool, &session.user_id, &session.api_key_id).await;
            if let Err(e) = self
                .0
                .analytics
                .revoke_session(&session.attested_session)
                .await
            {
                tracing::warn!(
                    conversation_id = %session.conversation_id,
                    error = %e,
                    "could not revoke a pi conversation's attested session"
                );
            }
        }
    }
}

pub(super) struct CreateRequest<'a> {
    pub(super) conversation_id: ContextId,
    pub(super) user_id: UserId,
    pub(super) attested_session: SessionId,
    pub(super) shim_source: &'a str,
    pub(super) mcp_client_source: &'a str,
    pub(super) mcp_token: &'a str,
    pub(super) transcript: Option<&'a str>,
    pub(super) start_seq: u64,
    pub(super) model: &'a str,
}

pub(super) struct SessionParts {
    pub(super) session: Arc<PiSession>,
    pub(super) stdout: Option<tokio::process::ChildStdout>,
    pub(super) stderr: Option<tokio::process::ChildStderr>,
}
