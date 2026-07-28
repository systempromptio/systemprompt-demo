//! Transcript redaction for the conversations data layer.
//!
//! Turn bodies pass through [`redact_text`] before they leave the database, so
//! PII-bearing substrings are replaced with sentinels unless the caller holds
//! `transcript:view_pii`.

mod redact;

pub use redact::redact_text;
