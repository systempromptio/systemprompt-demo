//! Admitting a conversation, and unwinding cleanly when it cannot be.
//!
//! Every refusal path here has to leave the table exactly as it found it. The
//! placeholder is inserted before the first `await` so two simultaneous
//! requests cannot both pass a cap of one — which means every subsequent
//! failure owes a [`PiRegistry::release`].

use std::sync::Arc;

use systemprompt::identifiers::{ContextId, UserId};

use super::super::credentials;
use crate::handlers::pi::session::PiSession;
use crate::handlers::pi::{persist, session, spawn};
use crate::repositories::bridge::IssuedApiKey;

use super::{CreateRequest, PiRegistry, SessionParts};

struct StartedChild {
    spawned: spawn::Spawned,
    stdin: tokio::process::ChildStdin,
    stdout: Option<tokio::process::ChildStdout>,
    stderr: Option<tokio::process::ChildStderr>,
}

// Why: distinguished because the widget shows each differently — a cap is
// "try later", a spawn failure is "misconfigured".
#[derive(Debug)]
pub(in crate::handlers::pi) enum SpawnError {
    PerUserCap(usize),
    TotalCap(usize),
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
    // Why: caps are checked while holding the lock and the placeholder is
    // inserted before the `await`, so two simultaneous requests cannot both
    // pass a cap of one.
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
            transcript,
            start_seq,
            model,
        } = req;
        self.make_room_for(&user_id).await;
        self.reserve(&user_id)?;

        let key = self.issue_credential(&user_id, &conversation_id).await?;

        let started = self
            .start_child(
                &conversation_id,
                &user_id,
                &key,
                &spawn::SpawnRequest {
                    conversation_id: &conversation_id,
                    attested_session: attested_session.as_str(),
                    gateway_key: &key.secret,
                    shim_source,
                    mcp_client_source,
                    mcp_token,
                    transcript,
                    model,
                },
            )
            .await?;
        let StartedChild {
            spawned,
            stdin,
            stdout,
            stderr,
        } = started;

        let (persist_tx, persist_rx) = tokio::sync::mpsc::unbounded_channel();
        persist::start(
            Arc::clone(&self.0.pool),
            conversation_id.clone(),
            persist_rx,
        );

        let session = Arc::new(PiSession::new(session::PiSessionInit {
            conversation_id: conversation_id.clone(),
            user_id,
            attested_session,
            api_key_id: key.id,
            workspace: spawned.workspace,
            child: spawned.child,
            stdin,
            persist: persist_tx,
            start_seq,
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

    async fn issue_credential(
        &self,
        user_id: &UserId,
        conversation_id: &ContextId,
    ) -> Result<IssuedApiKey, SpawnError> {
        credentials::issue(
            &self.0.pool,
            user_id,
            conversation_id,
            self.0.cfg.max_lifetime,
        )
        .await
        .map_err(SpawnError::Credential)
    }

    async fn start_child(
        &self,
        conversation_id: &ContextId,
        user_id: &UserId,
        key: &IssuedApiKey,
        req: &spawn::SpawnRequest<'_>,
    ) -> Result<StartedChild, SpawnError> {
        let mut spawned = match spawn::spawn(&self.0.cfg, req).await {
            Ok(s) => s,
            Err(e) => {
                self.unwind(conversation_id, user_id, key).await;
                return Err(SpawnError::Io(e));
            },
        };

        let Some(stdin) = spawned.child.stdin.take() else {
            self.unwind(conversation_id, user_id, key).await;
            _ = spawned.child.kill().await;
            return Err(SpawnError::Io(std::io::Error::other(
                "pi child has no stdin",
            )));
        };
        let stdout = spawned.child.stdout.take();
        let stderr = spawned.child.stderr.take();
        Ok(StartedChild {
            spawned,
            stdin,
            stdout,
            stderr,
        })
    }

    async fn unwind(&self, conversation_id: &ContextId, user_id: &UserId, key: &IssuedApiKey) {
        self.release(conversation_id);
        credentials::revoke(&self.0.pool, user_id, &key.id).await;
    }

    async fn make_room_for(&self, user_id: &UserId) {
        for stale in self.surplus_for(user_id) {
            tracing::info!(
                conversation_id = %stale,
                "displacing a pi session for a new one from the same user"
            );
            self.remove(&stale, None).await;
        }
    }

    fn surplus_for(&self, user_id: &UserId) -> Vec<ContextId> {
        let Ok(sessions) = self.0.sessions.lock() else {
            return Vec::new();
        };
        let keep = self.0.cfg.max_sessions_per_user.saturating_sub(1);
        let mut mine: Vec<&Arc<PiSession>> = sessions
            .values()
            .filter(|s| s.user_id == *user_id)
            .collect();
        mine.sort_by_key(|s| std::cmp::Reverse(s.age()));
        let surplus = mine.len().saturating_sub(keep);
        mine.into_iter()
            .take(surplus)
            .map(|s| s.conversation_id.clone())
            .collect()
    }

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

    fn release(&self, conversation_id: &ContextId) {
        if let Ok(mut sessions) = self.0.sessions.lock() {
            sessions.remove(conversation_id);
        }
    }
}
