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
//! ids (plus the default).

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
    // Why: the registry's own price card rides along so the picker can show
    // what a model costs — the meters already show spend, and a picker that
    // hides the rate while the header meters the bill is half an answer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) input_per_million: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) output_per_million: Option<f64>,
}

pub(super) fn catalogue(cfg: &PiConfig) -> Vec<GatewayModel> {
    let mut models = advertised();

    // Why: the registry is the truth, but the terminal cannot come up empty —
    // a profile that has not bootstrapped (tests, degraded start) still serves
    // the configured default.
    if models.is_empty() {
        models.push(GatewayModel {
            id: cfg.model.clone(),
            provider: cfg.provider.clone(),
            context_window: None,
            max_output_tokens: None,
            input_per_million: None,
            output_per_million: None,
        });
    }

    if !cfg.models.is_empty() {
        models.retain(|m| m.id == cfg.model || cfg.models.iter().any(|w| w == &m.id));
    }

    // Why: default first — it is what a session gets when the visitor never
    // touches the picker, so it leads the list the picker renders.
    models.sort_by_key(|m| m.id != cfg.model);
    models
}

// Why: `None` in means the default; an id outside the catalogue resolves to
// `None` out, which the caller must refuse rather than silently downgrade.
pub(super) fn resolve(cfg: &PiConfig, requested: Option<&str>) -> Option<String> {
    requested.map_or_else(
        || Some(cfg.model.clone()),
        |m| {
            let m = m.trim();
            catalogue(cfg).into_iter().find(|c| c.id == m).map(|c| c.id)
        },
    )
}

fn advertised() -> Vec<GatewayModel> {
    let Ok(profile) = ProfileBootstrap::get() else {
        return Vec::new();
    };
    let gateway = profile.gateway.as_ref().and_then(|g| g.resolved());

    profile
        .providers
        .providers
        .iter()
        // Why: backend-surface providers (e.g. an OpenAI-compatible Cerebras)
        // stay off front-door advertised lists by design, but a model of
        // theirs with an explicit gateway route is one this terminal can
        // genuinely run — the route is the operator saying so. Advertised
        // surfaces keep the blanket exposure rule; backend needs the route.
        .flat_map(|entry| {
            let advertised_surface = entry.surface.is_advertised();
            entry.models.iter().filter_map(move |m| {
                let offered = if advertised_surface {
                    gateway.is_none_or(|g| g.is_model_exposed(&profile.providers, m.id.as_str()))
                } else {
                    gateway.is_some_and(|g| g.find_route(m.id.as_str()).is_some())
                };
                offered.then(|| GatewayModel {
                    id: m.id.as_str().to_owned(),
                    provider: entry.name.as_str().to_owned(),
                    context_window: (m.limits.context_window > 0)
                        .then_some(m.limits.context_window),
                    max_output_tokens: (m.limits.max_output_tokens > 0)
                        .then_some(m.limits.max_output_tokens),
                    input_per_million: (m.pricing.input_per_million > 0.0)
                        .then_some(m.pricing.input_per_million),
                    output_per_million: (m.pricing.output_per_million > 0.0)
                        .then_some(m.pricing.output_per_million),
                })
            })
        })
        .collect()
}
