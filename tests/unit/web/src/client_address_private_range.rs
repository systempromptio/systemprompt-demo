//! Which addresses a per-IP limit refuses to key on.
//!
//! This predicate decides whether the registration cap is active at all, so a
//! range wrongly classified as public bricks signups for a whole deployment
//! that resolves every client to one gateway address.

use std::net::IpAddr;

use systemprompt_web_admin::util::client_address::is_private_range;

fn ip(s: &str) -> IpAddr {
    s.parse().unwrap_or_else(|e| panic!("parse {s}: {e}"))
}

#[test]
fn addresses_a_deployment_can_collapse_onto_are_private() {
    for addr in [
        "127.0.0.1",
        "10.0.0.1",
        // The Docker bridge gateway every plain compose deployment presents.
        "172.17.0.1",
        "192.168.1.1",
        "169.254.1.1",
        "::1",
        "fd00::1",
        // Fly's 6PN peer range, which `fc00::/7` covers.
        "fdaa::1",
        "fe80::1",
    ] {
        assert!(is_private_range(ip(addr)), "{addr} should be private");
    }
}

/// Carrier-grade NAT is the range that makes this more than a loopback check:
/// it is routable, it is not RFC1918, and a mobile network puts thousands of
/// unrelated users behind one of these.
#[test]
fn cgnat_is_private() {
    assert!(is_private_range(ip("100.64.0.1")));
    assert!(is_private_range(ip("100.127.255.254")));
}

/// The boundaries of `100.64.0.0/10`. `100.63.x` and `100.128.x` are ordinary
/// public space, and a mask that swallowed them would silently disable the cap
/// for real clients.
#[test]
fn cgnat_does_not_bleed_into_neighbouring_public_space() {
    assert!(!is_private_range(ip("100.63.255.255")));
    assert!(!is_private_range(ip("100.128.0.0")));
}

#[test]
fn routable_addresses_are_public() {
    for addr in ["8.8.8.8", "203.0.113.7", "66.241.64.1", "2001:4860::1"] {
        assert!(!is_private_range(ip(addr)), "{addr} should be public");
    }
}
