//! `SystemPrompt` Template
//!
//! This crate re-exports extensions for use with the `SystemPrompt` runtime.
//! Extensions are automatically discovered via the `inventory` crate.

pub use systemprompt::{cli, *};
pub use systemprompt_credits_extension as credits;
pub use systemprompt_email_extension as email;
pub use systemprompt_web_extension as web;
