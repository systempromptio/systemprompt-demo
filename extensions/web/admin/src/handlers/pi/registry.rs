//! The live-session table, plus the reaper that keeps it honest.
//!
//! Held as an axum `Extension` layer, mirroring how core injects
//! `CliBinaryPath`. Unlike the one-shot `/api/v1/admin/cli` endpoint, a session
//! here outlives the request that created it, so something has to own process
//! lifetime — that is this module.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use systemprompt::identifiers::{SessionId, UserId};

use super::config::PiConfig;
use super::session::PiSession;

/// Why a session could not be created. Distinguished because the widget shows
/// each differently: a cap is "try later", a spawn failure is "misconfigured".
#[derive(Debug)]
pub(super) enum SpawnError {
    PerUserCap(usize),
    TotalCap(usize),
    Io(std::io::Error),
}

impl std::fmt::Display for SpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PerUserCap(n) => write!(f, "you already have {n} pi session(s) running"),
            Self::TotalCap(n) => write!(f, "the server is at its limit of {n} pi sessions"),
            Self::Io(e) => write!(f, "could not start pi: {e}"),
        }
    }
}

#[derive(Clone)]
pub(crate) struct PiRegistry(Arc<Inner>);

struct Inner {
    cfg: PiConfig,
    sessions: Mutex<HashMap<String, Arc<PiSession>>>,
}

impl PiRegistry {
    /// Build the registry and start its reaper.
    pub(crate) fn new(cfg: PiConfig) -> Self {
        let registry = Self(Arc::new(Inner {
            cfg,
            sessions: Mutex::new(HashMap::new()),
        }));
        registry.spawn_reaper();
        registry
    }

    pub(super) fn config(&self) -> &PiConfig {
        &self.0.cfg
    }

    pub(super) fn get(&self, conversation_id: &str) -> Option<Arc<PiSession>> {
        self.0
            .sessions
            .lock()
            .ok()?
            .get(conversation_id)
            .map(Arc::clone)
    }

    /// Start a session, or explain why not.
    ///
    /// Caps are checked while holding the lock and the placeholder is inserted
    /// before the `await`, so two simultaneous requests cannot both pass a cap
    /// of one.
    pub(super) async fn create(
        &self,
        conversation_id: String,
        user_id: UserId,
        attested_session: SessionId,
        shim_source: &str,
    ) -> Result<SessionParts, SpawnError> {
        self.reserve(&user_id)?;

        let spawned = super::spawn::spawn(
            &self.0.cfg,
            &super::spawn::SpawnRequest {
                conversation_id: &conversation_id,
                attested_session: attested_session.as_str(),
                shim_source,
            },
        )
        .await;

        let mut spawned = match spawned {
            Ok(s) => s,
            Err(e) => {
                self.release(&conversation_id);
                return Err(SpawnError::Io(e));
            },
        };

        let Some(stdin) = spawned.child.stdin.take() else {
            self.release(&conversation_id);
            _ = spawned.child.kill().await;
            return Err(SpawnError::Io(std::io::Error::other(
                "pi child has no stdin",
            )));
        };
        let stdout = spawned.child.stdout.take();
        let stderr = spawned.child.stderr.take();

        let session = Arc::new(PiSession::new(super::session::PiSessionInit {
            conversation_id: conversation_id.clone(),
            user_id,
            attested_session,
            workspace: spawned.workspace,
            child: spawned.child,
            stdin,
        }));

        if let Ok(mut sessions) = self.0.sessions.lock() {
            sessions.insert(conversation_id, Arc::clone(&session));
        }
        Ok(SessionParts {
            session,
            stdout,
            stderr,
        })
    }

    /// Check caps. Separate from [`Self::create`] so the lock is never held
    /// across an `await`.
    fn reserve(&self, user_id: &UserId) -> Result<(), SpawnError> {
        let Ok(sessions) = self.0.sessions.lock() else {
            return Err(SpawnError::Io(std::io::Error::other(
                "session registry poisoned",
            )));
        };
        if sessions.len() >= self.0.cfg.max_sessions_total {
            return Err(SpawnError::TotalCap(self.0.cfg.max_sessions_total));
        }
        let mine = sessions.values().filter(|s| s.user_id == *user_id).count();
        if mine >= self.0.cfg.max_sessions_per_user {
            return Err(SpawnError::PerUserCap(self.0.cfg.max_sessions_per_user));
        }
        Ok(())
    }

    fn release(&self, conversation_id: &str) {
        if let Ok(mut sessions) = self.0.sessions.lock() {
            sessions.remove(conversation_id);
        }
    }

    /// Close a session and drop it from the table.
    pub(super) async fn remove(&self, conversation_id: &str, code: Option<i32>) {
        let session = {
            let Ok(mut sessions) = self.0.sessions.lock() else {
                return;
            };
            sessions.remove(conversation_id)
        };
        if let Some(session) = session {
            session.close(code).await;
        }
    }

    /// Kill sessions that have gone idle, outlived their ceiling, or whose child
    /// already exited.
    fn spawn_reaper(&self) {
        let registry = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                ticker.tick().await;
                let expired: Vec<(String, &'static str)> = {
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

/// The child's pipes, handed to the pump alongside the session.
pub(super) struct SessionParts {
    pub(super) session: Arc<PiSession>,
    pub(super) stdout: Option<tokio::process::ChildStdout>,
    pub(super) stderr: Option<tokio::process::ChildStderr>,
}
