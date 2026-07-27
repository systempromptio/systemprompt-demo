//! Reading the one port the jailed child is allowed to dial.
//!
//! The jail grants outbound TCP to the gateway's port alone. Misreading the
//! port would either strand every session or widen the grant.

use systemprompt_web_admin::test_support::gateway_port;

#[test]
fn reads_the_port_the_child_will_dial() {
    assert_eq!(gateway_port("http://127.0.0.1:8080"), Some(8080));
    assert_eq!(gateway_port("https://example.com/v1"), Some(443));
    assert_eq!(gateway_port("http://example.com"), Some(80));
    assert_eq!(gateway_port("https://[::1]:9090/x"), Some(9090));
    assert_eq!(gateway_port("not a url"), None);
}

/// An IPv6 literal without a port must not have its address digits read as
/// one — that would grant a port nobody asked for and deny the real one.
#[test]
fn does_not_mistake_an_ipv6_literal_for_a_port() {
    assert_eq!(gateway_port("https://[2001:db8::1]/v1"), Some(443));
}
