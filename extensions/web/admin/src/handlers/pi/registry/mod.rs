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
mod waitlist;

pub(super) use admission::SpawnError;

#[derive(Clone)]
pub(crate) struct PiRegistry(Arc<Inner>);

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
        if let Some(session) = slot.as_ref().and_then(Slot::live).map(Arc::clone) {
            if code.is_some()
                && session.age() < std::time::Duration::from_secs(2)
                && self.0.cfg.limits.address_space > 0
            {
                tracing::warn!(
                    conversation_id = %session.conversation_id,
                    "pi child died within 2s of spawn; if this repeats, \
                     limits.address_space in services/config/pi.yaml may be below \
                     what node needs at startup — re-measure before lowering it"
                );
            }
            session.close(code).await;
            if let Err(e) = crate::repositories::pi::conversations::update_conversation_closed(
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

    // Why: clean closes remove their own workspace directory; this catches
    // what a crash or SIGKILL left behind. The mtime grace keeps it from
    // racing a spawn that has materialised its files but not yet registered.
    async fn sweep_workspaces(&self) {
        const GRACE: std::time::Duration = std::time::Duration::from_secs(600);
        let root = self.0.cfg.workspace_root.clone();
        let live: std::collections::HashSet<String> = self
            .0
            .sessions
            .lock()
            .map(|s| s.keys().map(|id| id.as_str().to_owned()).collect())
            .unwrap_or_default();
        let Ok(mut entries) = tokio::fs::read_dir(&root).await else {
            return;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().into_owned();
            if live.contains(&name) {
                continue;
            }
            let age = entry
                .metadata()
                .await
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|m| m.elapsed().ok());
            if age.is_none_or(|a| a < GRACE) {
                continue;
            }
            match tokio::fs::remove_dir_all(entry.path()).await {
                Ok(()) => {
                    tracing::info!(workspace = %name, "removed an orphaned pi workspace");
                },
                Err(e) => tracing::warn!(
                    workspace = %name,
                    error = %e,
                    "could not remove an orphaned pi workspace"
                ),
            }
        }
    }

    fn spawn_reaper(&self) {
        let registry = self.clone();
        tokio::spawn(async move {
            credentials::sweep_orphans(&registry.0.pool).await;
            registry.sweep_workspaces().await;
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(30));
            let mut tick: u64 = 0;
            loop {
                ticker.tick().await;
                // Why: every ~10th tick (5 min), also re-sweep what a crash
                // leaves behind — workspaces and PATs whose sessions no
                // longer exist. Cheap enough that precision is not worth a
                // second task.
                tick += 1;
                if tick % 10 == 0 {
                    credentials::sweep_orphans(&registry.0.pool).await;
                    registry.sweep_workspaces().await;
                }
                let expired: Vec<(ContextId, &'static str)> = {
                    let Ok(sessions) = registry.0.sessions.lock() else {
                        continue;
                    };
                    sessions
                        .iter()
                        .filter_map(|(id, slot)| {
                            let why = match slot {
                                Slot::Reserving { at, .. } => {
                                    if at.elapsed() > STALE_RESERVATION {
                                        "stale reservation"
                                    } else {
                                        return None;
                                    }
                                },
                                Slot::Live(s) => {
                                    if s.is_closed() {
                                        "child exited"
                                    } else if s.age() > registry.0.cfg.max_lifetime {
                                        "max lifetime"
                                    } else if s.idle_for() > registry.0.cfg.idle_timeout {
                                        "idle"
                                    } else {
                                        return None;
                                    }
                                },
                            };
                            Some((id.clone(), why))
                        })
                        .collect()
                };
                for (id, why) in expired {
                    tracing::info!(conversation_id = %id, reason = why, "reaping pi session");
                    registry.remove(&id, None).await;
                }
                registry.waitlist_prune();
            }
        });
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
