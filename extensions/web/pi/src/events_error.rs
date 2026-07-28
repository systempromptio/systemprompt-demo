//! Error frames in the widget vocabulary: classification, deduplication, and
//! the read-side upgrade of rows persisted before errors carried `kind`/`code`.

use super::events::PiEventBody;

/// Where an error came from, so the widget can render each source distinctly
/// instead of inferring it from message prefixes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// The model provider (via the gateway) refused or failed the request.
    Provider,
    /// The governance layer refused something before it ran.
    Governance,
    /// pi itself rejected an RPC command.
    Rpc,
}

/// The credit guard's sentence, matched on the *normalised* provider error.
///
/// The literal is owned by `extensions/credits/src/guard.rs`; a source-parsing
/// test pins the two against each other so the classification cannot silently
/// stop matching when the guard's wording changes.
pub const CREDIT_EXHAUSTED_NEEDLE: &str = "Credit exhausted.";

pub const CREDIT_EXHAUSTED_CODE: &str = "credit_exhausted";

/// Suppresses an error identical to the last one observed, across turns.
///
/// A failed provider request surfaces as up to two identical frames per
/// attempt, and pi retries a failed prompt as a fresh turn — without this,
/// one exhausted credit balance prints the same sentence eight times.
/// `TurnStart`/`TurnEnd` are deliberately transparent to this memory: they are
/// exactly what a retry interleaves. Any substantive frame clears it, so a
/// genuinely new error is never hidden and re-prompting re-surfaces the same
/// one. Pure state, held behind `session::PiSession`'s emit funnel;
/// the read-side mirror for already-persisted rows lives in
/// `conversations::collapse_duplicate_errors`.
#[derive(Debug, Default)]
pub struct ErrorDeduper {
    last: Option<(ErrorKind, Option<&'static str>, String)>,
}

impl ErrorDeduper {
    /// Observe the next frame; `true` means it repeats the last error and
    /// should not be emitted.
    pub fn is_repeat(&mut self, body: &PiEventBody) -> bool {
        match body {
            PiEventBody::Error {
                message,
                kind,
                code,
            } => {
                if self
                    .last
                    .as_ref()
                    .is_some_and(|(k, c, m)| k == kind && c == code && m == message)
                {
                    return true;
                }
                self.last = Some((*kind, *code, message.clone()));
                false
            },
            PiEventBody::TurnStart | PiEventBody::TurnEnd => false,
            _ => {
                self.last = None;
                false
            },
        }
    }
}

impl PiEventBody {
    // Why: both wire frames of a failed request must come through this one
    // constructor so they yield identical bodies the emit-level dedupe can
    // collapse.
    pub(super) fn provider_error(raw: &str) -> Self {
        let message = readable_provider_error(raw);
        let code = message
            .contains(CREDIT_EXHAUSTED_NEEDLE)
            .then_some(CREDIT_EXHAUSTED_CODE);
        Self::Error {
            message,
            kind: ErrorKind::Provider,
            code,
        }
    }

    pub(super) fn governance_error(message: impl Into<String>) -> Self {
        Self::Error {
            message: message.into(),
            kind: ErrorKind::Governance,
            code: None,
        }
    }

    pub(super) fn rpc_error(message: impl Into<String>) -> Self {
        Self::Error {
            message: message.into(),
            kind: ErrorKind::Rpc,
            code: None,
        }
    }
}

/// Bring a stored error frame up to the current vocabulary, in place.
///
/// Rows persisted before errors carried `kind`/`code` hold either a raw
/// provider envelope or a `[GOVERNANCE] `-prefixed sentence. Upgrading them on
/// read — same normalisation, same classification as [`PiEventBody`]'s
/// constructors — means old conversations replay exactly like new ones and no
/// migration or second rendering path exists. A frame that already has a
/// `kind` is current and passes through untouched.
pub fn upgrade_legacy_error(event: &mut serde_json::Value) {
    if event.get("type").and_then(serde_json::Value::as_str) != Some("error")
        || event.get("kind").is_some()
    {
        return;
    }
    let Some(raw) = event.get("message").and_then(serde_json::Value::as_str) else {
        return;
    };
    let (kind, message) = raw.strip_prefix("[GOVERNANCE] ").map_or_else(
        || ("provider", readable_provider_error(raw)),
        |rest| ("governance", rest.to_owned()),
    );
    if message.contains(CREDIT_EXHAUSTED_NEEDLE) {
        event["code"] = CREDIT_EXHAUSTED_CODE.into();
    }
    event["kind"] = kind.into();
    event["message"] = message.into();
}

/// Pull the human sentence out of a provider error.
///
/// pi hands over the transport status and the raw body — `400 {"type":"error",
/// "error":{"message":"Credit exhausted…"}}` — and the sentence a person needs
/// is the innermost `message`. Rendering the envelope instead buries it in
/// JSON, which in a terminal reads as a crash rather than as an answer.
/// Anything that does not parse is passed through untouched: an unfamiliar
/// error still beats no error.
pub fn readable_provider_error(raw: &str) -> String {
    // JSON: provider error envelopes vary by upstream; only the innermost
    // `message` is wanted and anything unparseable passes through untouched
    let Some(start) = raw.find('{') else {
        return raw.to_owned();
    };
    let Ok(body) = serde_json::from_str::<serde_json::Value>(&raw[start..]) else {
        return raw.to_owned();
    };
    body.pointer("/error/message")
        .and_then(serde_json::Value::as_str)
        .or_else(|| body.get("message").and_then(serde_json::Value::as_str))
        .map_or_else(|| raw.to_owned(), ToOwned::to_owned)
}
