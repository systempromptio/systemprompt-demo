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

pub(super) use admission::SpawnError;

#[derive(Clone)]
pub(crate) struct PiRegistry(Arc<Inner>);

struct Inner {
    cfg: PiConfig,
    pool: Arc<PgPool>,
    analytics: Arc<dyn AnalyticsProvider>,
    sessions: Mutex<HashMap<ContextId, Arc<PiSession>>>,
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
        }));
        registry.spawn_reaper();
        registry
    }

    pub(super) fn config(&self) -> &PiConfig {
        &self.0.cfg
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
            .map(Arc::clone)
    }

    pub(super) async fn remove(&self, conversation_id: &ContextId, code: Option<i32>) {
        let session = {
            let Ok(mut sessions) = self.0.sessions.lock() else {
                return;
            };
            sessions.remove(conversation_id)
        };
        if let Some(session) = session {
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

    fn spawn_reaper(&self) {
        let registry = self.clone();
        tokio::spawn(async move {
            credentials::sweep_orphans(&registry.0.pool).await;
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                ticker.tick().await;
                let expired: Vec<(ContextId, &'static str)> = {
                    let Ok(sessions) = registry.0.sessions.lock() else {
                        continue;
                    };
                    sessions
                        .values()
                        .filter_map(|s| {
                            let why = if s.is_closed() {
                                "child exited"
                            } else if s.age() > registry.0.cfg.max_lifetime {
                                "max lifetime"
                            } else if s.idle_for() > registry.0.cfg.idle_timeout {
                                "idle"
                            } else {
                                return None;
                            };
                            Some((s.conversation_id.clone(), why))
                        })
                        .collect()
                };
                for (id, why) in expired {
                    tracing::info!(conversation_id = %id, reason = why, "reaping pi session");
                    registry.remove(&id, None).await;
                }
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
