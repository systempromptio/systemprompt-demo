//! Persistence for users: identity, access, and activity.

pub mod access_control;
pub mod activity;
pub mod magic_links;
pub mod mutations;
pub mod queries;
pub mod registration;
pub mod share_token;
pub mod user_queries;

pub use mutations::{create_user, delete_user, update_user};
pub use share_token::{find_or_create_share_token_version, find_share_token_version};
