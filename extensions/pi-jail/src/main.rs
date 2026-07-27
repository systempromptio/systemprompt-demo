//! `sp-pi-jail` — confine a process to one directory, then become it.
//!
//! The governed pi web terminal spawns a child that runs the server's uid.
//! pi's `read` tool applies no path confinement of its own, so before this
//! existed a single approved tool call could read anything that uid could:
//! provider keys, the database URL, the OAuth at-rest pepper, the child's own
//! gateway credential. The approval prompt was the only barrier, and in this
//! deployment the person clicking it is the untrusted party.
//!
//! So: apply a Landlock ruleset to ourselves, then `exec` the real binary.
//! Landlock is inherited across `execve` and cannot be dropped, which makes
//! the order the guarantee.
//!
//! ```text
//! sp-pi-jail --workspace <dir> --allow-read <dir>… [--allow-connect-tcp <port>]… -- <binary> <args>…
//! ```
//!
//! # What this is not
//!
//! Landlock is a path- and port-based LSM, not a namespace. The child still
//! shares the pid and network namespaces, still runs as the server's uid, and
//! `--allow-connect-tcp 8080` permits port 8080 on *any* reachable host rather
//! than loopback alone. It makes `read` safe; it does not make arbitrary
//! execution safe. A container per session is still the answer before `bash`
//! is ever enabled.

#![expect(clippy::print_stderr, reason = "the pre-exec jail has no logger")]

mod args;
mod jail;

use std::process::ExitCode;

const EXIT_USAGE: u8 = 2;
const EXIT_NO_SANDBOX: u8 = 3;
const EXIT_EXEC: u8 = 4;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let spec = match args::Spec::parse(&argv) {
        Ok(spec) => spec,
        Err(e) => {
            eprintln!("sp-pi-jail: {e}\n{}", args::USAGE);
            return ExitCode::from(EXIT_USAGE);
        },
    };

    match jail::apply(&spec) {
        Ok(abi) => eprintln!(
            "sp-pi-jail: confined to {} (Landlock {abi})",
            spec.workspace.display()
        ),
        Err(e) => {
            eprintln!("sp-pi-jail: refusing to run unconfined: {e}");
            return ExitCode::from(EXIT_NO_SANDBOX);
        },
    }

    let e = exec(&spec);
    eprintln!("sp-pi-jail: exec {}: {e}", spec.command.display());
    ExitCode::from(EXIT_EXEC)
}

#[cfg(unix)]
fn exec(spec: &args::Spec) -> std::io::Error {
    use std::os::unix::process::CommandExt as _;

    std::process::Command::new(&spec.command)
        .args(&spec.command_args)
        .exec()
}

#[cfg(not(unix))]
fn exec(_spec: &args::Spec) -> std::io::Error {
    std::io::Error::other("exec is unavailable on this platform")
}
