//! Unit tests for the MCP extension crates' pure helpers:
//! - `systemprompt-mcp-agent`'s documentation-hub `topics` registry and search
//! - `systemprompt-mcp-shared`'s `truncate_on_char_boundary` (rejection-reason
//!   truncation with UTF-8 safety)

#[cfg(test)]
mod site_pages;
#[cfg(test)]
mod topics;
#[cfg(test)]
mod truncate_on_char_boundary;
