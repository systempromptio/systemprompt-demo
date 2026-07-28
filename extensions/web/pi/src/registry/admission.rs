//! Admitting a conversation, and unwinding cleanly when it cannot be.
//!
//! Every refusal path here has to leave the table exactly as it found it. The
//! placeholder is inserted before the first `await` so two simultaneous
//! requests cannot both pass a cap of one — which means every subsequent
//! failure owes a [`PiRegistry::release`].

use std::sync::Arc;

use systemprompt::identifiers::{ContextId, UserId};

use super::super::credentials;
use crate::session::PiSession;
use crate::{persist, session, spawn};
use systemprompt_web_governance::repositories::bridge::IssuedApiKey;

use super::{CreateRequest, PiRegistry, SessionParts, Slot};

// Why: holds a claimed slot until the session is live — dropped on any error
// path, which vacates the slot; `defuse` disarms it once the live session has
// replaced the placeholder.
struct Reservation<'a> {
    registry: &'a PiRegistry,
    conversation_id: ContextId,
    armed: bool,
}

impl Reservation<'_> {
    const fn defuse(&mut self) {
        self.armed = false;
    }
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.registry.release(&self.conversation_id);
        }
    }
}

struct StartedChild {
    spawned: spawn::Spawned,
    stdin: tokio::process::ChildStdin,
    stdout: Option<tokio::process::ChildStdout>,
    stderr: Option<tokio::process::ChildStderr>,
}

// Why: distinguished because the widget shows each differently — a cap is
// "try later", a spawn failure is "misconfigured".
#[derive(Debug)]
pub(crate) enum SpawnError {
    PerUserCap(usize),
    Waitlisted { position: usize, queue_len: usize },
    Credential(String),
    Io(std::io::Error),
    Version(String),
}

impl std::fmt::Display for SpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PerUserCap(n) => write!(f, "you already have {n} pi session(s) running"),
            Self::Waitlisted { position, .. } => {
                write!(
                    f,
                    "the server is at capacity; you are #{} in line",
                    position + 1
                )
            },
            Self::Credential(e) => write!(f, "could not mint a gateway credential: {e}"),
            Self::Io(e) => write!(f, "could not start pi: {e}"),
            Self::Version(e) => write!(f, "{e}"),
        }
    }
}


impl PiRegistry {
    // Why: caps are checked while holding the lock and the placeholder is
    // inserted before the `await`, so two simultaneous requests cannot both
    // pass a cap of one.
    pub(crate) async fn create(
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
        self.version_gate().await.map_err(SpawnError::Version)?;
        self.make_room_for(&user_id, &conversation_id).await;
        let mut reservation = self.reserve(conversation_id.clone(), &user_id)?;

        let key = self.issue_credential(&user_id, &conversation_id).await?;

        let started = self
            .start_child(
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
            sessions.insert(conversation_id, Slot::Live(Arc::clone(&session)));
        }
        reservation.defuse();
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
        user_id: &UserId,
        key: &IssuedApiKey,
        req: &spawn::SpawnRequest<'_>,
    ) -> Result<StartedChild, SpawnError> {
        let mut spawned = match spawn::spawn(&self.0.cfg, req).await {
            Ok(s) => s,
            Err(e) => {
                self.unwind(user_id, key).await;
                return Err(SpawnError::Io(e));
            },
        };

        let Some(stdin) = spawned.child.stdin.take() else {
            self.unwind(user_id, key).await;
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

    // Why: only the credential needs unwinding here — the claimed slot is
    // vacated by the `Reservation` guard when `create` returns the error.
    async fn unwind(&self, user_id: &UserId, key: &IssuedApiKey) {
        credentials::revoke(&self.0.pool, user_id, &key.id).await;
    }

    // Why: resuming retires the conversation's own predecessor in place — it is
    // a replacement, not a surplus, so counting it against the per-user cap
    // would evict a second, unrelated session for no reason. Retiring it here
    // also orders the old child's workspace cleanup before the new spawn
    // recreates that same directory.
    async fn make_room_for(&self, user_id: &UserId, incoming: &ContextId) {
        self.remove(incoming, None).await;
        for stale in self.surplus_for(user_id, incoming) {
            tracing::info!(
                conversation_id = %stale,
                "displacing a pi session for a new one from the same user"
            );
            self.remove(&stale, None).await;
        }
    }

    fn surplus_for(&self, user_id: &UserId, incoming: &ContextId) -> Vec<ContextId> {
        let Ok(sessions) = self.0.sessions.lock() else {
            return Vec::new();
        };
        let keep = self.0.cfg.max_sessions_per_user.saturating_sub(1);
        let mut mine: Vec<&Arc<PiSession>> = sessions
            .values()
            .filter_map(Slot::live)
            .filter(|s| s.user_id == *user_id && s.conversation_id != *incoming)
            .collect();
        mine.sort_by_key(|s| std::cmp::Reverse(s.age()));
        let surplus = mine.len().saturating_sub(keep);
        mine.into_iter()
            .take(surplus)
            .map(|s| s.conversation_id.clone())
            .collect()
    }

    // Why: the cap check and the slot claim happen under one lock acquisition,
    // so two requests racing at capacity-minus-one cannot both pass — the
    // loser sees the winner's placeholder.
    fn reserve(
        &self,
        conversation_id: ContextId,
        user_id: &UserId,
    ) -> Result<Reservation<'_>, SpawnError> {
        let Ok(mut sessions) = self.0.sessions.lock() else {
            return Err(SpawnError::Io(std::io::Error::other(
                "session registry poisoned",
            )));
        };
        let others = sessions.keys().filter(|id| **id != conversation_id).count();
        let mine = sessions
            .iter()
            .filter(|(id, slot)| **id != conversation_id && *slot.user_id() == *user_id)
            .count();
        if mine >= self.0.cfg.max_sessions_per_user {
            return Err(SpawnError::PerUserCap(self.0.cfg.max_sessions_per_user));
        }
        // Why: at the capacity boundary, admission is FIFO — a free slot goes
        // to the front of the wait line, and a newcomer joins the back rather
        // than racing it.
        let free = self.0.cfg.max_sessions_total.saturating_sub(others);
        match self.waitlist_gate(user_id, free) {
            super::waitlist::Gate::Admit => {},
            super::waitlist::Gate::Wait {
                position,
                queue_len,
            } => {
                return Err(SpawnError::Waitlisted {
                    position,
                    queue_len,
                });
            },
        }
        sessions.insert(
            conversation_id.clone(),
            Slot::Reserving {
                user_id: user_id.clone(),
                at: std::time::Instant::now(),
            },
        );
        drop(sessions);
        Ok(Reservation {
            registry: self,
            conversation_id,
            armed: true,
        })
    }

    fn release(&self, conversation_id: &ContextId) {
        if let Ok(mut sessions) = self.0.sessions.lock() {
            sessions.remove(conversation_id);
        }
    }
}
