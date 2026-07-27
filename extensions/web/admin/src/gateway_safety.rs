//! Gateway [`SafetyScanner`] implementation for the systemprompt template.
//!
//! [`SecretsScanner`] flags plaintext credentials (GitHub / Anthropic / AWS /
//! Stripe / … tokens, private keys, DB URLs with passwords) carried in
//! inference content, reusing the same `SECRET_PATTERNS` that the governance
//! tool-use webhook applies. It registers through `register_safety_scanner!`
//! under the name `secrets`; the gateway runs it for any policy whose
//! `safety.scanners` lists it and blocks the request when `safety
//! .block_categories` includes `secret`.
//!
//! It judges only the newest user turn — see `newest_user_text` for why a
//! blocking scanner must not read the whole conversation.

use systemprompt::ai::{Finding, SafetyScanner, Severity, register_safety_scanner};
use systemprompt::models::wire::canonical::{
    CanonicalContent, CanonicalRequest, CanonicalResponse, Role,
};

use crate::handlers::webhook::governance::secrets::scan_str_for_secret;

#[derive(Debug, Clone, Copy, Default)]
pub struct SecretsScanner;

impl SecretsScanner {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl SafetyScanner for SecretsScanner {
    fn name(&self) -> &'static str {
        "secrets"
    }

    async fn scan_request(&self, req: &CanonicalRequest) -> Vec<Finding> {
        scan("request", &newest_user_text(req))
    }

    async fn scan_response_final(&self, response: &CanonicalResponse) -> Vec<Finding> {
        let mut text = String::new();
        for part in &response.content {
            if let CanonicalContent::Text(t) = part {
                text.push_str(t);
                text.push('\n');
            }
        }
        scan("response", &text)
    }
}

fn newest_user_text(req: &CanonicalRequest) -> String {
    let Some(msg) = req.messages.iter().rev().find(|m| m.role == Role::User) else {
        return String::new();
    };
    let mut out = String::new();
    for part in &msg.content {
        if let CanonicalContent::Text(t) = part {
            out.push_str(t);
            out.push('\n');
        }
    }
    out
}

fn scan(phase: &'static str, text: &str) -> Vec<Finding> {
    scan_str_for_secret(text).map_or_else(Vec::new, |excerpt| {
        vec![Finding {
            phase,
            severity: Severity::High,
            category: "secret".to_owned(),
            excerpt: Some(excerpt),
            scanner: "secrets",
        }]
    })
}

register_safety_scanner!(SecretsScanner::new, name = "secrets");
