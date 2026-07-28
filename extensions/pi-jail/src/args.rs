//! Argv parsing for `sp-pi-jail`.
//!
//! Hand-rolled rather than clap: the whole surface is four flags and a
//! command tail, and a jail that runs before `exec` is worth keeping free of
//! dependencies it does not need.

use std::path::PathBuf;

#[derive(Debug)]
pub struct Spec {
    pub workspace: PathBuf,
    // Why: never `/proc` — `/proc/<server-pid>/environ` is readable by this uid
    // and holds the credentials this jail exists to hide.
    pub allow_read: Vec<PathBuf>,
    pub connect_tcp: Vec<u16>,
    pub command: PathBuf,
    pub command_args: Vec<String>,
}

pub const USAGE: &str = "usage: sp-pi-jail --workspace <dir> \
[--allow-read <dir>]… [--allow-connect-tcp <port>]… -- <binary> [args…]";

impl Spec {
    pub fn parse(argv: &[String]) -> Result<Self, String> {
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
            let value = rest.next().ok_or_else(|| format!("{flag} needs a value"))?;
            match flag.as_str() {
                "--workspace" => workspace = Some(PathBuf::from(value)),
                "--allow-read" => allow_read.push(PathBuf::from(value)),
                "--allow-connect-tcp" => connect_tcp.push(
                    value
                        .parse::<u16>()
                        .map_err(|e| format!("--allow-connect-tcp {value}: {e}"))?,
                ),
                // Why: an ignored `--allow-read` would look like a working jail
                // with a missing grant, so an unknown flag is fatal.
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
