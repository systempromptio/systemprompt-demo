//! Admitting a conversation, and unwinding cleanly when it cannot be.
//!
//! Every refusal path here has to leave the table exactly as it found it. The
//! placeholder is inserted before the first `await` so two simultaneous
//! requests cannot both pass a cap of one — which means every subsequent
//! failure owes a [`PiRegistry::release`].

use std::sync::Arc;

use systemprompt::identifiers::UserId;

use super::super::credentials;
use crate::handlers::pi::session::PiSession;
use crate::handlers::pi::{session, spawn};

use super::{CreateRequest, PiRegistry, SessionParts};

/// Why a session could not be created. Distinguished because the widget shows
/// each differently: a cap is "try later", a spawn failure is "misconfigured".
#[derive(Debug)]
pub(in crate::handlers::pi) enum SpawnError {
    PerUserCap(usize),
    TotalCap(usize),
    /// The per-conversation gateway credential could not be minted. Nothing was
    /// spawned: pi with no credential would fail on the first turn instead.
    Credential(String),
    Io(std::io::Error),
}

impl std::fmt::Display for SpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PerUserCap(n) => write!(f, "you already have {n} pi session(s) running"),
            Self::TotalCap(n) => write!(f, "the server is at its limit of {n} pi sessions"),
            Self::Credential(e) => write!(f, "could not mint a gateway credential: {e}"),
            Self::Io(e) => write!(f, "could not start pi: {e}"),
        }
    }
}


impl PiRegistry {
    /// Caps are checked while holding the lock and the placeholder is inserted
    /// before the `await`, so two simultaneous requests cannot both pass a cap
    /// of one.
    pub(in crate::handlers::pi) async fn create(
        &self,
        req: CreateRequest<'_>,
    ) -> Result<SessionParts, SpawnError> {
        let CreateRequest {
            conversation_id,
            user_id,
            attested_session,
            shim_source,
            mcp_client_source,
            mcp_token,
        } = req;
        self.make_room_for(&user_id).await;
        self.reserve(&user_id)?;

        // Minted here rather than in the handler so a conversation refused by a
        // cap never gets a credential at all. It is for this conversation's own
        // user, because the gateway rejects a PAT whose owner is not the owner
        // of the attested session it arrives with.
        let key = match credentials::issue(
            &self.0.pool,
            &user_id,
            &conversation_id,
            self.0.cfg.max_lifetime,
        )
        .await
        {
            Ok(key) => key,
            Err(e) => return Err(SpawnError::Credential(e)),
        };

        let spawned = spawn::spawn(
            &self.0.cfg,
            &spawn::SpawnRequest {
                conversation_id: &conversation_id,
                attested_session: attested_session.as_str(),
                gateway_key: &key.secret,
                shim_source,
                mcp_client_source,
                mcp_token,
            },
        )
        .await;

        let mut spawned = match spawned {
            Ok(s) => s,
            Err(e) => {
                self.release(&conversation_id);
                credentials::revoke(&self.0.pool, &user_id, &key.id).await;
                return Err(SpawnError::Io(e));
            },
        };

        let Some(stdin) = spawned.child.stdin.take() else {
            self.release(&conversation_id);
            credentials::revoke(&self.0.pool, &user_id, &key.id).await;
            _ = spawned.child.kill().await;
            return Err(SpawnError::Io(std::io::Error::other(
                "pi child has no stdin",
            )));
        };
        let stdout = spawned.child.stdout.take();
        let stderr = spawned.child.stderr.take();

        let session = Arc::new(PiSession::new(session::PiSessionInit {
            conversation_id: conversation_id.clone(),
            user_id,
            attested_session,
            api_key_id: key.id,
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

    /// Close whatever this user already holds, so a new conversation can start.
    ///
    /// A user asking for a conversation while already holding one has almost
    /// always lost the tab that owns it — a reload is the ordinary way this
    /// happens. Refusing them leaves that terminal stranded until the idle
    /// timeout with no way to reclaim it, so the newest request wins and the
    /// old conversation is closed. The cap is unchanged; only which session it
    /// keeps.
    async fn make_room_for(&self, user_id: &UserId) {
        for stale in self.surplus_for(user_id) {
            tracing::info!(
                conversation_id = %stale,
                "displacing a pi session for a new one from the same user"
            );
            self.remove(&stale, None).await;
        }
    }

    /// This user's conversations beyond the number a new one may join, oldest
    /// first. Empty when they are under the cap.
    ///
    /// Separate from [`Self::reserve`] because closing them is asynchronous and
    /// the lock must not be held across the `await`.
    fn surplus_for(&self, user_id: &UserId) -> Vec<String> {
        let Ok(sessions) = self.0.sessions.lock() else {
            return Vec::new();
        };
        let keep = self.0.cfg.max_sessions_per_user.saturating_sub(1);
        let mut mine: Vec<&Arc<PiSession>> = sessions
            .values()
            .filter(|s| s.user_id == *user_id)
            .collect();
        // Oldest first, so a cap above one displaces the least recently started
        // rather than an arbitrary member of a hash map.
        mine.sort_by_key(|s| std::cmp::Reverse(s.age()));
        let surplus = mine.len().saturating_sub(keep);
        mine.into_iter()
            .take(surplus)
            .map(|s| s.conversation_id.clone())
            .collect()
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
}
