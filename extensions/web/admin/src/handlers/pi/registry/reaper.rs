//! The background task that retires sessions the table has outlived.
//!
//! Eviction is time-based and therefore cannot live on a request path: nothing
//! calls back when a child goes idle, exceeds its lifetime, or dies without
//! closing its pipes. This is also where a crash's leftovers — workspaces and
//! gateway credentials with no session behind them — are swept.

use systemprompt::identifiers::ContextId;

use super::super::credentials;
use super::{PiRegistry, Slot, STALE_RESERVATION};

impl PiRegistry {
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

    pub(super) fn spawn_reaper(&self) {
        let registry = self.clone();
        tokio::spawn(async move {
            credentials::sweep_orphans(&registry.0.pool, &[]).await;
            registry.sweep_workspaces().await;
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(30));
            let mut tick: u64 = 0;
            loop {
                ticker.tick().await;
                // Why: every ~10th tick (5 min), also re-sweep what a crash
                // leaves behind — workspaces and PATs whose sessions no
                // longer exist. Cheap enough that precision is not worth a
                // second task. The live set must ride along: without it the
                // sweep revokes running conversations' credentials and every
                // chat dies with a provider "Connection error." mid-session.
                tick += 1;
                if tick.is_multiple_of(10) {
                    // Why: a poisoned lock skips the credential sweep — an
                    // unknown live set must fail toward keeping keys, not
                    // revoking them.
                    let live: Option<Vec<ContextId>> = registry
                        .0
                        .sessions
                        .lock()
                        .map(|s| s.keys().cloned().collect())
                        .ok();
                    if let Some(live) = live {
                        credentials::sweep_orphans(&registry.0.pool, &live).await;
                    }
                    registry.sweep_workspaces().await;
                }
                for (id, why) in registry.expired() {
                    tracing::info!(conversation_id = %id, reason = why, "reaping pi session");
                    registry.remove(&id, None).await;
                }
                registry.waitlist_prune();
            }
        });
    }

    fn expired(&self) -> Vec<(ContextId, &'static str)> {
        let Ok(sessions) = self.0.sessions.lock() else {
            return Vec::new();
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
                        } else if s.age() > self.0.cfg.max_lifetime {
                            "max lifetime"
                        } else if s.idle_for() > self.0.cfg.idle_timeout {
                            "idle"
                        } else {
                            return None;
                        }
                    },
                };
                Some((id.clone(), why))
            })
            .collect()
    }
}
