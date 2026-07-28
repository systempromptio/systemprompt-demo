//! Classification of a resolved client address.
//!
//! Resolution itself belongs to core's trust-gated
//! `services::middleware::client_addr` — nothing here parses a header. This is
//! only the question core does not expose: whether the address it returned is
//! one a per-IP limit can meaningfully key on.
//!
//! A deployment with no proxy in front of it — plain `docker-compose`, local
//! `just start`, a same-host reverse proxy absent from `server.trusted_proxies`
//! — resolves every client to one private address. A cap keyed on that would
//! lock out the whole deployment rather than one abuser, so callers skip it.
//! Core has an equivalent private predicate that is not `pub`; this duplicates
//! it rather than taking a core revision bump for four lines.

use std::net::IpAddr;

#[must_use]
pub const fn is_private_range(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            let is_cgnat = octets[0] == 100 && (octets[1] & 0xc0) == 64;
            v4.is_loopback() || v4.is_private() || v4.is_link_local() || is_cgnat
        },
        IpAddr::V6(v6) => {
            let seg0 = v6.segments()[0];
            let is_unique_local = (seg0 & 0xfe00) == 0xfc00;
            let is_link_local = (seg0 & 0xffc0) == 0xfe80;
            v6.is_loopback() || is_unique_local || is_link_local
        },
    }
}
