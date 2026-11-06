// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask fuzz`: runs the cargo-fuzz targets in `fuzz/` for a fixed time each.
//!
//! Fuzzing needs nightly Rust and cargo-fuzz (docs/conventions.md). With no target named,
//! every target `cargo fuzz list` reports runs, one after another; the command fails at the
//! first target that finds a crash. `make test-fuzz` and the nightly `Fuzz` workflow call it.
//!
//! It builds for the host triple nightly `rustc` reports. Left to itself, cargo-fuzz builds for
//! the triple it was compiled for, and the prebuilt Linux cargo-fuzz is a musl binary, whose
//! static libc the sanitizers reject.

use std::path::Path;
use std::process::Command;

use crate::exec::{Exec, System};
use crate::host::parse_host;

const DEFAULT_SECONDS: u64 = 60;
/// The toolchain cargo-fuzz runs on: it needs nightly for the sanitizer flags.
const TOOLCHAIN: &str = "+nightly";

/// `cargo xtask fuzz [<target>...] [--time <duration>]`: what to fuzz and for how long.
#[derive(clap::Parser, Debug, PartialEq, Eq)]
pub(crate) struct Args {
    /// The targets, every one in `fuzz/` when none is named.
    targets: Vec<String>,
    /// How long each target runs: seconds, or a number with s, m or h.
    #[arg(long = "time", value_name = "duration", default_value_t = DEFAULT_SECONDS, value_parser = parse_duration)]
    seconds: u64,
}

pub(crate) fn main(root: &Path, args: &Args) -> Result<bool, String> {
    run(root, args, &System)
}

/// [`main`] with the programs `exec` runs.
fn run(root: &Path, options: &Args, exec: &dyn Exec) -> Result<bool, String> {
    let targets = if options.targets.is_empty() {
        list_targets(root, exec)?
    } else {
        options.targets.clone()
    };
    if targets.is_empty() {
        return Err("fuzz/ has no targets".to_owned());
    }
    let host = host_triple(root, exec)?;
    for target in &targets {
        println!("fuzz: running {target} for {}s", options.seconds);
        let status = exec
            .status(
                Command::new("cargo")
                    .args(run_args(target, &host, options.seconds))
                    .current_dir(root),
            )
            .map_err(|error| format!("could not run cargo fuzz: {error}"))?;
        if !status.success() {
            println!("fuzz: {target} failed");
            return Ok(false);
        }
    }
    Ok(true)
}

fn list_targets(root: &Path, exec: &dyn Exec) -> Result<Vec<String>, String> {
    let output = exec
        .output(
            Command::new("cargo")
                .args([TOOLCHAIN, "fuzz", "list"])
                .current_dir(root),
        )
        .map_err(|error| format!("could not run cargo fuzz list: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo fuzz list failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect())
}

/// The host triple of the nightly toolchain, from `rustc -vV`.
fn host_triple(root: &Path, exec: &dyn Exec) -> Result<String, String> {
    let output = exec
        .output(
            Command::new("rustc")
                .args([TOOLCHAIN, "-vV"])
                .current_dir(root),
        )
        .map_err(|error| format!("could not run rustc -vV: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "rustc -vV failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    parse_host(&String::from_utf8_lossy(&output.stdout))
        .map(str::to_owned)
        .ok_or_else(|| "rustc -vV printed no host line".to_owned())
}

/// The cargo arguments that fuzz `target`, built for `host`, for `seconds`.
///
/// `-print_funcs=0` stops libFuzzer symbolizing each newly covered function, which on macOS
/// runs `atos` and can hang the run.
fn run_args(target: &str, host: &str, seconds: u64) -> Vec<String> {
    vec![
        TOOLCHAIN.to_owned(),
        "fuzz".to_owned(),
        "run".to_owned(),
        "--target".to_owned(),
        host.to_owned(),
        target.to_owned(),
        "--".to_owned(),
        format!("-max_total_time={seconds}"),
        "-print_funcs=0".to_owned(),
    ]
}

/// Seconds in `text`: a positive whole number, optionally followed by `s`, `m` or `h`.
fn parse_duration(text: &str) -> Result<u64, String> {
    let invalid = || format!("invalid duration {text:?}: seconds, or a number with s, m or h");
    let (number, unit) = match text.char_indices().last() {
        Some((index, 's' | 'm' | 'h')) => (text.get(..index), text.get(index..)),
        Some(_) => (Some(text), None),
        None => return Err(invalid()),
    };
    let value: u64 = number
        .filter(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|digits| digits.parse().ok())
        .filter(|&value| value > 0)
        .ok_or_else(invalid)?;
    let scale = match unit {
        Some("m") => 60,
        Some("h") => 3600,
        Some(_) | None => 1,
    };
    value.checked_mul(scale).ok_or_else(invalid)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "test code: a failed unwrap or index is a failed test"
    )]

    use std::path::Path;
    use std::process::Command;

    use super::{Args, DEFAULT_SECONDS, parse_duration, run, run_args};
    use crate::exec::{Fake, Reply};
    use crate::parse_cli;

    /// cargo-fuzz and rustc as `fails` says: listing two targets, the first run passing and the
    /// second failing when `fails` is set.
    fn tools(fails: bool) -> impl Fn(&Command) -> Reply {
        move |command: &Command| {
            let args: Vec<String> = command
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect();
            if command.get_program() == "rustc" {
                return Reply {
                    stdout: "rustc 1.99.0-nightly\nhost: x86_64-unknown-linux-gnu\n".to_owned(),
                    ..Reply::default()
                };
            }
            if args.contains(&"list".to_owned()) {
                return Reply {
                    stdout: "qs-map\n\nqs-pak\n".to_owned(),
                    ..Reply::default()
                };
            }
            Reply::exit(i32::from(fails && args.contains(&"qs-pak".to_owned())))
        }
    }

    #[test]
    fn every_listed_target_runs_for_the_host() {
        let fake = Fake::new(tools(false));
        assert!(
            run(
                Path::new("."),
                &parse_cli::<Args, _>(&["--time", "2m"]).unwrap(),
                &fake
            )
            .unwrap()
        );
        let seen = fake.seen.lock().unwrap();
        assert_eq!(seen[0], "cargo +nightly fuzz list");
        assert_eq!(seen[1], "rustc +nightly -vV");
        assert!(
            seen[2].contains("--target x86_64-unknown-linux-gnu qs-map -- -max_total_time=120")
        );
        assert!(seen[3].contains(" qs-pak "));
    }

    #[test]
    fn a_failing_target_stops_the_run() {
        let fake = Fake::new(tools(true));
        assert!(
            !run(
                Path::new("."),
                &parse_cli::<Args, &str>(&[]).unwrap(),
                &fake
            )
            .unwrap()
        );
        let named = Fake::new(tools(false));
        assert!(
            run(
                Path::new("."),
                &parse_cli::<Args, _>(&["qs-map"]).unwrap(),
                &named
            )
            .unwrap()
        );
        assert_eq!(
            named.seen.lock().unwrap().len(),
            2,
            "a named target needs no listing"
        );
    }

    #[test]
    fn the_tools_failing_is_an_error() {
        let fails = Fake::new(|_: &Command| Reply {
            code: 1,
            stderr: "no fuzz".to_owned(),
            ..Reply::default()
        });
        assert!(
            run(
                Path::new("."),
                &parse_cli::<Args, &str>(&[]).unwrap(),
                &fails
            )
            .unwrap_err()
            .contains("cargo fuzz list failed")
        );
        assert!(
            run(
                Path::new("."),
                &parse_cli::<Args, _>(&["qs-map"]).unwrap(),
                &fails
            )
            .unwrap_err()
            .contains("rustc -vV failed")
        );
        let empty = Fake::new(|_: &Command| Reply::exit(0));
        assert!(
            run(
                Path::new("."),
                &parse_cli::<Args, &str>(&[]).unwrap(),
                &empty
            )
            .unwrap_err()
            .contains("no targets")
        );
        assert!(
            run(
                Path::new("."),
                &parse_cli::<Args, _>(&["qs-map"]).unwrap(),
                &empty
            )
            .unwrap_err()
            .contains("no host")
        );
        let cannot = Fake::new(|_: &Command| Reply {
            cannot_run: true,
            ..Reply::default()
        });
        assert!(
            run(
                Path::new("."),
                &parse_cli::<Args, &str>(&[]).unwrap(),
                &cannot
            )
            .is_err()
        );
        assert!(
            run(
                Path::new("."),
                &parse_cli::<Args, _>(&["qs-map"]).unwrap(),
                &cannot
            )
            .is_err()
        );
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn the_entry_point_refuses_a_bad_duration() {
        assert!(parse_cli::<Args, _>(&["--time", "ten"]).is_err());
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("45").unwrap(), 45);
        assert_eq!(parse_duration("30s").unwrap(), 30);
        assert_eq!(parse_duration("10m").unwrap(), 600);
        assert_eq!(parse_duration("2h").unwrap(), 7200);
        for bad in ["", "m", "0", "0m", "-5", "1.5m", "10d", "ten", "5 m"] {
            assert!(parse_duration(bad).is_err(), "{bad:?}");
        }
        assert!(parse_duration(&format!("{}h", u64::MAX)).is_err());
    }

    #[test]
    fn arguments() {
        assert_eq!(
            parse_cli::<Args, &str>(&[]).unwrap(),
            Args {
                targets: Vec::new(),
                seconds: DEFAULT_SECONDS
            }
        );
        assert_eq!(
            parse_cli::<Args, _>(&["qc-progs", "--time", "10m"]).unwrap(),
            Args {
                targets: args(&["qc-progs"]),
                seconds: 600
            }
        );
        assert!(parse_cli::<Args, _>(&["--time"]).is_err());
        assert!(parse_cli::<Args, _>(&["--jobs", "2"]).is_err());
        assert!(parse_cli::<Args, _>(&["--time", "0"]).is_err());
    }

    #[test]
    fn run_command() {
        assert_eq!(
            run_args("qc-progs", "x86_64-unknown-linux-gnu", 600),
            args(&[
                "+nightly",
                "fuzz",
                "run",
                "--target",
                "x86_64-unknown-linux-gnu",
                "qc-progs",
                "--",
                "-max_total_time=600",
                "-print_funcs=0"
            ])
        );
    }
}
