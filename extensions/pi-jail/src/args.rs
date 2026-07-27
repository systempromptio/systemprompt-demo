//! Argv parsing for `sp-pi-jail`.
//!
//! Hand-rolled rather than clap: the whole surface is four flags and a
//! command tail, and a jail that runs before `exec` is worth keeping free of
//! dependencies it does not need.

use std::path::PathBuf;

/// A parsed invocation: what to confine to, and what to run inside it.
#[derive(Debug)]
pub(crate) struct Spec {
    /// The one directory the child may read *and* write. Its cwd and `HOME`
    /// live inside it.
    pub(crate) workspace: PathBuf,
    /// Directories the child may read and execute from — the interpreter, its
    /// libraries, CA bundles. Never `/proc`: `/proc/<server-pid>/environ` is
    /// readable by this uid and holds the credentials this jail exists to hide.
    pub(crate) allow_read: Vec<PathBuf>,
    /// TCP ports the child may `connect()` to. Landlock is port-based, not
    /// host-based, so this permits the port on *any* reachable address.
    pub(crate) connect_tcp: Vec<u16>,
    pub(crate) command: PathBuf,
    pub(crate) command_args: Vec<String>,
}

pub(crate) const USAGE: &str = "usage: sp-pi-jail --workspace <dir> \
[--allow-read <dir>]… [--allow-connect-tcp <port>]… -- <binary> [args…]";

impl Spec {
    /// Parse argv (without argv\[0\]). Every error is fatal: a jail that
    /// guesses at a malformed allowlist is a jail with an unknown shape.
    pub(crate) fn parse(argv: &[String]) -> Result<Self, String> {
        let mut workspace: Option<PathBuf> = None;
        let mut allow_read = Vec::new();
        let mut connect_tcp = Vec::new();
        let mut rest = argv.iter();

        let tail = loop {
            let Some(flag) = rest.next() else {
                return Err("missing `--` and the command to run".to_owned());
            };
            if flag == "--" {
                break rest.map(String::as_str).collect::<Vec<_>>();
            }
            let value = rest
                .next()
                .ok_or_else(|| format!("{flag} needs a value"))?;
            match flag.as_str() {
                "--workspace" => workspace = Some(PathBuf::from(value)),
                "--allow-read" => allow_read.push(PathBuf::from(value)),
                "--allow-connect-tcp" => connect_tcp.push(
                    value
                        .parse::<u16>()
                        .map_err(|e| format!("--allow-connect-tcp {value}: {e}"))?,
                ),
                other => return Err(format!("unknown flag {other}")),
            }
        };

        let workspace = workspace.ok_or_else(|| "--workspace is required".to_owned())?;
        let (command, command_args) = tail
            .split_first()
            .ok_or_else(|| "nothing to run after `--`".to_owned())?;

        Ok(Self {
            workspace,
            allow_read,
            connect_tcp,
            command: PathBuf::from(command),
            command_args: command_args.iter().map(|s| (*s).to_owned()).collect(),
        })
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "assertions in tests")]
mod tests {
    use super::Spec;

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

    /// A flag the child controls must never be silently ignored — an ignored
    /// `--allow-read` would look like a working jail with a missing grant.
    #[test]
    fn rejects_unknown_flags_and_missing_pieces() {
        assert!(Spec::parse(&argv(&["--nope", "x", "--", "/bin/true"])).is_err());
        assert!(Spec::parse(&argv(&["--workspace", "/ws"])).is_err());
        assert!(Spec::parse(&argv(&["--workspace", "/ws", "--"])).is_err());
        assert!(Spec::parse(&argv(&["--", "/bin/true"])).is_err());
        assert!(Spec::parse(&argv(&["--workspace"])).is_err());
    }
}
