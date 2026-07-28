//! The identity of a governed call, and how a later enforcement point
//! recognises one an earlier point already governed.
//!
//! A model-issued tool call is judged twice: the gate holds it at the shim's
//! `tool_call` hook, and the proxy judges it again on arrival. Both must run —
//! the proxy is reachable by callers that never passed the gate — but the two
//! are one call, and a policy that counts calls must not count it twice.
//!
//! The identity is minted here rather than accepted from the caller. A child
//! holding `SP_PI_MCP_TOKEN` can reach the proxy directly, and an id it supplied
//! would be an id it could replay to stay uncounted forever. So the proxy never
//! names a call: it describes one, and may only claim an entry the gate itself
//! wrote. Riding one means first making the gate govern an identical call, which
//! was charged when it did.
//!
//! Entries are claim-once and short-lived. A description that matches nothing
//! mints a fresh identity, so the failure direction is to charge twice rather
//! than to charge nothing.

use std::collections::VecDeque;
use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use systemprompt::identifiers::CallId;

use crate::handlers::webhook::governance::types::AuditOrigin;

const TTL: Duration = Duration::from_secs(10);

const CAPACITY: usize = 64;

#[derive(Debug)]
struct Entry {
    call_id: CallId,
    tool_name: String,
    fingerprint: u64,
    minted_at: Instant,
    claimed: bool,
}

#[derive(Debug, Default)]
pub struct CallLedger {
    entries: Mutex<VecDeque<Entry>>,
}

impl CallLedger {
    pub fn mint(&self, tool_name: &str, arguments: Option<&serde_json::Value>) -> CallId {
        let call_id = CallId::generate();
        let Ok(mut entries) = self.entries.lock() else {
            return call_id;
        };
        sweep(&mut entries);
        if entries.len() >= CAPACITY {
            entries.pop_front();
        }
        entries.push_back(Entry {
            call_id: call_id.clone(),
            tool_name: tool_name.to_owned(),
            fingerprint: fingerprint(arguments),
            minted_at: Instant::now(),
            claimed: false,
        });
        call_id
    }

    pub fn claim(
        &self,
        tool_name: &str,
        arguments: Option<&serde_json::Value>,
    ) -> (CallId, AuditOrigin) {
        let wanted = fingerprint(arguments);
        let Ok(mut entries) = self.entries.lock() else {
            return (CallId::generate(), AuditOrigin::Governed);
        };
        sweep(&mut entries);
        entries
            .iter_mut()
            .find(|e| !e.claimed && e.tool_name == tool_name && e.fingerprint == wanted)
            .map_or_else(
                || (CallId::generate(), AuditOrigin::Governed),
                |entry| {
                    entry.claimed = true;
                    (entry.call_id.clone(), AuditOrigin::Reverified)
                },
            )
    }
}

fn sweep(entries: &mut VecDeque<Entry>) {
    let now = Instant::now();
    entries.retain(|e| now.duration_since(e.minted_at) < TTL);
}

fn fingerprint(arguments: Option<&serde_json::Value>) -> u64 {
    let mut hasher = DefaultHasher::new();
    arguments
        .map(ToString::to_string)
        .unwrap_or_default()
        .hash(&mut hasher);
    hasher.finish()
}
