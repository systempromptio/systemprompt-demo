//! Argv parsing for `sp-pi-jail`: a full invocation round-trips, and every
//! malformed shape is fatal — an ignored flag would look like a working jail
//! with a missing grant.

use systemprompt_pi_jail::args::Spec;

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn parses_a_full_invocation() {
    let spec = Spec::parse(&argv(&[
        "--workspace",
        "/ws",
        "--allow-read",
        "/usr/bin",
        "--allow-read",
        "/lib",
        "--allow-connect-tcp",
        "8080",
        "--",
        "/usr/bin/node",
        "--mode",
        "rpc",
    ]))
    .expect("valid invocation");
    assert_eq!(spec.workspace, std::path::Path::new("/ws"));
    assert_eq!(spec.allow_read.len(), 2);
    assert_eq!(spec.connect_tcp, vec![8080]);
    assert_eq!(spec.command_args, vec!["--mode", "rpc"]);
}

#[test]
fn rejects_unknown_flags_and_missing_pieces() {
    assert!(Spec::parse(&argv(&["--nope", "x", "--", "/bin/true"])).is_err());
    assert!(Spec::parse(&argv(&["--workspace", "/ws"])).is_err());
    assert!(Spec::parse(&argv(&["--workspace", "/ws", "--"])).is_err());
    assert!(Spec::parse(&argv(&["--", "/bin/true"])).is_err());
    assert!(Spec::parse(&argv(&["--workspace"])).is_err());
}
