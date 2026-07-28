//! The model catalogue the terminal offers.
//!
//! Derived from the profile's provider registry — the same source the gateway
//! itself routes by — rather than a hand-typed list. Every advertised model of
//! every advertised provider (anthropic, openai, gemini, and whatever else the
//! profile registers) is offered, filtered through the gateway's own
//! `is_model_exposed` so the picker can never show a model the gateway would
//! then refuse. The child always speaks `anthropic-messages`; the gateway
//! translates to the provider's wire, so a non-Anthropic id is still one the
//! session can genuinely run.
//!
//! `services/config/pi.yaml`'s `models` key is a *narrowing* allow-list on top
//! of that: empty means the whole catalogue, non-empty keeps only the listed
//! ids (plus the default). It is no longer the source of the models.

use serde::Serialize;
use systemprompt::config::ProfileBootstrap;

use super::config::PiConfig;

#[derive(Debug, Clone, Serialize)]
pub(super) struct GatewayModel {
    pub(super) id: String,
    pub(super) provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) context_window: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) max_output_tokens: Option<u32>,
}

/// Every model a session may run, default first.
pub(super) fn catalogue(cfg: &PiConfig) -> Vec<GatewayModel> {
    let mut models = advertised(cfg);

    // The registry is the truth, but the terminal cannot come up empty: a
    // profile that has not bootstrapped (tests, degraded start) still serves
    // the configured default.
    if models.is_empty() {
        models.push(GatewayModel {
            id: cfg.model.clone(),
            provider: cfg.provider.clone(),
            context_window: None,
            max_output_tokens: None,
        });
    }

    if !cfg.models.is_empty() {
        models.retain(|m| m.id == cfg.model || cfg.models.iter().any(|w| w == &m.id));
    }

    // Default first: it is what a session gets when the visitor never touches
    // the picker, so it leads the list the picker renders.
    models.sort_by_key(|m| m.id != cfg.model);
    models
}

/// Resolve a client's model request. `None` in is the default; an id outside
/// the catalogue resolves to `None` out, which the caller must refuse.
pub(super) fn resolve(cfg: &PiConfig, requested: Option<&str>) -> Option<String> {
    match requested {
        None => Some(cfg.model.clone()),
        Some(m) => {
            let m = m.trim();
            catalogue(cfg).into_iter().find(|c| c.id == m).map(|c| c.id)
        },
    }
}

fn advertised(cfg: &PiConfig) -> Vec<GatewayModel> {
    let Ok(profile) = ProfileBootstrap::get() else {
        return Vec::new();
    };
    let gateway = profile.gateway.as_ref().and_then(|g| g.resolved());

    profile
        .providers
        .advertised_providers()
        .flat_map(|entry| {
            entry.models.iter().map(|m| GatewayModel {
                id: m.id.as_str().to_owned(),
                provider: entry.name.as_str().to_owned(),
                context_window: (m.limits.context_window > 0).then_some(m.limits.context_window),
                max_output_tokens: (m.limits.max_output_tokens > 0)
                    .then_some(m.limits.max_output_tokens),
            })
        })
        // Advertised but not routable would 4xx at the gateway; keep the
        // picker honest by applying the gateway's own exposure rule.
        .filter(|m| {
            gateway.is_none_or(|g| g.is_model_exposed(&profile.providers, &m.id))
        })
        .collect()
}
