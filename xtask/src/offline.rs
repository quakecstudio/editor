// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask offline <codebase> [--game <dir>]`: the app with the network denied (P4-16,
//! PRIN-9). It is Linux only, and CI runs it in the Test Assets job.
//!
//! Each step runs in a network namespace of its own (`unshare -rn`), which has no network but
//! a loopback that is down, under `strace`, which records every `connect`, `sendto` and
//! `sendmsg`. A step fails when it fails, or when its trace holds an address that is not a
//! local socket (`AF_INET` or `AF_INET6`).
//!
//! The steps, with no model provider set up, as on a first start:
//! - **control:** `quakec-ide new id1` with no local copy, which fetches the template from
//!   GitHub. It must fail, and its trace must show the request: this proves the trace sees one.
//! - `quakec-ide new id1 --from <codebase>`, then `build`, `check` and `test` (with `--game`
//!   when given) of the mod it made.
//! - `quakec-ide mcp` on the mod: `initialize`, `tools/list` and two tool calls.
//! - The desktop app's session (`apps/desktop/examples/offline.rs`): it opens the mod, opens
//!   every file, builds, checks, tests, searches, opens the palette and asks for an agent thread.
//!
//! `docs/privacy.md` lists every network request the app can make.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// `cargo xtask offline <codebase> [--game <dir>]`.
#[derive(clap::Parser, Debug)]
pub(crate) struct Args {
    /// A local copy of id's QuakeC 1.01, the `id1` template's upstream directory.
    codebase: PathBuf,
    /// A game folder the mod's tests play their scenarios over.
    #[arg(long)]
    game: Option<PathBuf>,
}

/// The MCP session the `mcp` step sends.
const MCP_SESSION: [&str; 5] = [
    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"offline","version":"0"}}}"#,
    r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
    r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"compile","arguments":{}}}"#,
    r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"project.files","arguments":{}}}"#,
];

/// One step: what it runs, what it is sent on stdin, and whether it must succeed.
struct Step {
    name: &'static str,
    program: PathBuf,
    args: Vec<String>,
    input: Option<String>,
    /// The control: it must fail and its trace must show a request.
    control: bool,
}

/// Runs every step; true when each did as it must.
pub(crate) fn main(root: &Path, args: &Args) -> Result<bool, String> {
    if !cfg!(target_os = "linux") {
        return Err("offline runs on Linux only: it needs unshare and strace".to_owned());
    }
    build(root)?;
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| root.join("target"), PathBuf::from)
        .join("debug");
    let cli = target.join("quakec-ide");
    let desktop = target.join("examples").join("offline");
    let work = std::env::temp_dir().join(format!("qs-offline-{}", std::process::id()));
    if work.exists() {
        fs::remove_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    }
    fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    let made = work.join("offline-mod");
    let text = |path: &Path| path.to_string_lossy().into_owned();
    let mut test = vec!["test".to_owned(), text(&made)];
    if let Some(game) = &args.game {
        test.extend(["--game".to_owned(), text(game)]);
    }
    let cli_step = |name, args: Vec<String>| Step {
        name,
        program: cli.clone(),
        args,
        input: None,
        control: false,
    };
    let steps = [
        Step {
            control: true,
            ..cli_step(
                "control: new id1 from GitHub",
                vec!["new".into(), "id1".into(), text(&work.join("control-mod"))],
            )
        },
        cli_step(
            "new id1 from a local copy",
            vec![
                "new".into(),
                "id1".into(),
                text(&made),
                "--from".into(),
                text(&args.codebase),
            ],
        ),
        cli_step("build", vec!["build".into(), text(&made)]),
        cli_step("check", vec!["check".into(), text(&made)]),
        cli_step("test", test),
        Step {
            input: Some(MCP_SESSION.join("\n") + "\n"),
            ..cli_step("mcp", vec!["mcp".into(), text(&made)])
        },
        Step {
            name: "the desktop app's session",
            program: desktop,
            args: vec![text(&made)],
            input: None,
            control: false,
        },
    ];
    let mut passed = true;
    for (number, step) in steps.iter().enumerate() {
        let trace = work.join(format!("trace-{number}.log"));
        let succeeded = run(step, &trace, &work)?;
        let log = fs::read_to_string(&trace).map_err(|e| format!("{}: {e}", trace.display()))?;
        let requests = network_calls(&log);
        let ok = if step.control {
            !succeeded && !requests.is_empty()
        } else {
            succeeded && requests.is_empty()
        };
        println!(
            "{}  {}: {}, {} network calls",
            if ok { "ok  " } else { "FAIL" },
            step.name,
            if succeeded { "succeeded" } else { "failed" },
            requests.len()
        );
        if !step.control {
            for request in &requests {
                println!("        {request}");
            }
        }
        passed &= ok;
    }
    Ok(passed)
}

/// Builds the CLI and the desktop app's session.
fn build(root: &Path) -> Result<(), String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    for args in [
        &["build", "--locked", "-p", "quakec-ide", "--bins"][..],
        &[
            "build",
            "--locked",
            "-p",
            "quakec-studio",
            "--example",
            "offline",
        ],
    ] {
        let status = Command::new(&cargo)
            .args(args)
            .current_dir(root)
            .status()
            .map_err(|e| format!("cargo: {e}"))?;
        if !status.success() {
            return Err(format!("cargo {}: {status}", args.join(" ")));
        }
    }
    Ok(())
}

/// Runs `step` in a network namespace of its own, traced into `trace`; whether it succeeded.
fn run(step: &Step, trace: &Path, work: &Path) -> Result<bool, String> {
    let mut command = Command::new("unshare");
    command
        .args(["--user", "--map-root-user", "--net", "--"])
        .args([
            "strace",
            "-f",
            "-qq",
            "-e",
            "trace=connect,sendto,sendmsg",
            "-o",
        ])
        .arg(trace)
        .arg(&step.program)
        .args(&step.args)
        .current_dir(work)
        .stdin(if step.input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|e| format!("unshare (with strace): {e}"))?;
    if let (Some(input), Some(mut stdin)) = (&step.input, child.stdin.take()) {
        stdin
            .write_all(input.as_bytes())
            .map_err(|e| format!("{}: {e}", step.name))?;
    }
    let status = child.wait().map_err(|e| format!("{}: {e}", step.name))?;
    Ok(status.success())
}

/// The calls of an `strace` log that address the network: each line that names an
/// `AF_INET` or `AF_INET6` address, or systemd-resolved's socket, which looks a name up on the
/// network for the caller. Other local sockets (`AF_UNIX`) and netlink are not the network.
fn network_calls(log: &str) -> Vec<&str> {
    log.lines()
        .filter(|line| line.contains("sa_family=AF_INET") || line.contains("/systemd/resolve/"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // No spec statement: P4-16, a trace's network calls are its internet addresses, IPv4 or
    // IPv6, failed or not, and name lookups through systemd-resolved; other local sockets are
    // not
    #[test]
    fn only_internet_addresses_are_network_calls() {
        let log = "\
101 connect(3, {sa_family=AF_UNIX, sun_path=\"/var/run/nscd/socket\"}, 110) = -1 ENOENT\n\
101 connect(5, {sa_family=AF_INET, sin_port=htons(53), sin_addr=inet_addr(\"127.0.0.53\")}, 16) = -1 ENETUNREACH (Network is unreachable)\n\
102 sendto(6, \"x\", 1, 0, {sa_family=AF_INET6, sin6_port=htons(443)}, 28) = -1 ENETUNREACH\n\
103 sendmsg(7, {msg_name={sa_family=AF_NETLINK, nl_pid=0}}, 0) = 20\n\
104 connect(8, {sa_family=AF_UNIX, sun_path=\"/run/systemd/resolve/io.systemd.Resolve\"}, 42) = 0\n";
        let calls = network_calls(log);
        assert_eq!(calls.len(), 3, "{calls:?}");
        assert!(calls.first().is_some_and(|c| c.contains("127.0.0.53")));
        assert!(calls.get(1).is_some_and(|c| c.contains("AF_INET6")));
        assert!(network_calls("").is_empty());
    }

    #[test]
    fn other_arguments_are_a_usage_error() {
        for bad in [&[][..], &["a", "b"], &["a", "--game"]] {
            assert!(crate::parse_cli::<Args, _>(bad).is_err(), "{bad:?}");
        }
    }
}
