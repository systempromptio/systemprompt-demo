//! First-come-first-served admission when the session table is full.
//!
//! The queue is in-memory and heartbeat-kept: a waiter who stops polling
//! `/api/public/pi/capacity` for [`WAITLIST_TTL`] is pruned, so an abandoned
//! tab cannot hold a place in line. There is no separate claim token — the
//! gate in [`super::admission`] admits a user only while their position is
//! within the free slots, which is the claim window: the head of the queue
//! has [`WAITLIST_TTL`] of missed heartbeats before the line moves on.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use systemprompt::identifiers::UserId;

use super::PiRegistry;

/// A waiter who has not polled for this long has left.
pub(super) const WAITLIST_TTL: Duration = Duration::from_secs(30);

pub(super) struct Waiter {
    user_id: UserId,
    last_seen: Instant,
}

/// What the admission gate decided while the caps were being checked.
pub(in crate::handlers::pi) enum Gate {
    Admit,
    Wait { position: usize, queue_len: usize },
}

impl PiRegistry {
    /// FIFO fairness at the capacity boundary: with `free` slots open, only
    /// the first `free` waiters may pass; anyone else is (re)enqueued. A user
    /// who passes is removed from the line.
    pub(super) fn waitlist_gate(&self, user_id: &UserId, free: usize) -> Gate {
        let Ok(mut queue) = self.0.waitlist.lock() else {
            // Why: fail open to plain capacity behaviour — a poisoned queue
            // must not brick admission entirely.
            return if free > 0 {
                Gate::Admit
            } else {
                Gate::Wait {
                    position: 0,
                    queue_len: 0,
                }
            };
        };
        prune(&mut queue);
        let position = queue.iter().position(|w| w.user_id == *user_id);
        match position {
            Some(p) if p < free => {
                queue.remove(p);
                Gate::Admit
            },
            Some(p) => {
                if let Some(w) = queue.get_mut(p) {
                    w.last_seen = Instant::now();
                }
                Gate::Wait {
                    position: p,
                    queue_len: queue.len(),
                }
            },
            None if queue.len() < free => Gate::Admit,
            None => {
                queue.push_back(Waiter {
                    user_id: user_id.clone(),
                    last_seen: Instant::now(),
                });
                Gate::Wait {
                    position: queue.len() - 1,
                    queue_len: queue.len(),
                }
            },
        }
    }

    /// One poll of the line: heartbeats the caller's entry (re-joining if it
    /// expired and `join` is set) and reports `(queue_len, position)`.
    pub(in crate::handlers::pi) fn waitlist_status(
        &self,
        user_id: Option<&UserId>,
        join: bool,
    ) -> (usize, Option<usize>) {
        let Ok(mut queue) = self.0.waitlist.lock() else {
            return (0, None);
        };
        prune(&mut queue);
        let position = user_id.and_then(|user_id| {
            let found = queue.iter().position(|w| w.user_id == *user_id);
            if let Some(p) = found {
                if let Some(w) = queue.get_mut(p) {
                    w.last_seen = Instant::now();
                }
                Some(p)
            } else if join {
                queue.push_back(Waiter {
                    user_id: user_id.clone(),
                    last_seen: Instant::now(),
                });
                Some(queue.len() - 1)
            } else {
                None
            }
        });
        (queue.len(), position)
    }

    pub(super) fn waitlist_prune(&self) {
        if let Ok(mut queue) = self.0.waitlist.lock() {
            prune(&mut queue);
        }
    }
}

fn prune(queue: &mut VecDeque<Waiter>) {
    queue.retain(|w| w.last_seen.elapsed() <= WAITLIST_TTL);
}
