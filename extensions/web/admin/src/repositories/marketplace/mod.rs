//! Plugin marketplace: installed plugin config and usage events.

pub mod plugin_env;
pub(crate) mod plugin_loader;
pub(crate) mod plugin_resolvers;
pub mod plugins;

pub use systemprompt_web_governance::repositories::usage_events as webhook;
