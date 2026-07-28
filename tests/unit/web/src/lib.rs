//! Unit tests for `systemprompt-web-shared` pure logic:
//! - `CampaignLink::full_url` UTM query assembly and `?`/`&` separator choice
//! - `BlogConfigValidated::validate` base-URL scheme/parse validation
//! - hook-event ingest leniency, which the governance record depends on
//!
//! and for `systemprompt-web-admin` items reached through its `test_support`
//! re-exports: `PiConfig` parsing and validation.

#[cfg(test)]
mod bridge_downloads;
#[cfg(test)]
mod campaign_link_full_url;
#[cfg(test)]
mod config_base_url;
#[cfg(test)]
mod governance_scope;
#[cfg(test)]
mod hook_event_dispatch;
#[cfg(test)]
mod hub_tool_lists;
#[cfg(test)]
mod middleware_gates;
#[cfg(test)]
mod pi_config;
#[cfg(test)]
mod pi_error_dedupe;
#[cfg(test)]
mod pi_events;
#[cfg(test)]
mod pi_format;
#[cfg(test)]
mod pi_jail;
#[cfg(test)]
mod pi_jail_args;
#[cfg(test)]
mod pi_mcp_render;
#[cfg(test)]
mod pi_normalize;
#[cfg(test)]
mod pi_persist;
#[cfg(test)]
mod pi_rpc;
#[cfg(test)]
mod pi_scope;
#[cfg(test)]
mod pi_shim;
#[cfg(test)]
mod pi_skills;
#[cfg(test)]
mod pi_token;
#[cfg(test)]
mod pi_transcript;
#[cfg(test)]
mod pi_version;
#[cfg(test)]
mod seed_contract;
#[cfg(test)]
mod site_markdown_routes;
