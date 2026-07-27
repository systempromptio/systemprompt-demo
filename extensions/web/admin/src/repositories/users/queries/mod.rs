//! Read paths for user data, split by the page that consumes them.

mod detail;
mod events;
mod listing;
mod role;

pub use detail::{
    find_user_detail, list_user_event_type_breakdown, list_user_sessions, list_user_top_tools,
};
pub use events::list_user_usage;
pub use listing::{list_distinct_roles, list_users};
pub use role::{UserAccess, find_user_access};
