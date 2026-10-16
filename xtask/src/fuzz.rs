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

const USAGE: &str = "usage: cargo xtask fuzz [<target>...] [--time <duration>]\n       \
                     <duration> is seconds, or a number with s, m or h (default 60s)";
const DEFAULT_SECONDS: u64 = 60;
/// The toolchain cargo-fuzz runs on: it needs nightly for the sanitizer flags.
const TOOLCHAIN: &str = "+nightly";

/// What to fuzz and for how long.
#[derive(Debug, PartialEq, Eq)]
struct Options {
    targets: Vec<String>,
    seconds: u64,
}

pub(crate) fn main(root: &Path, args: &[String]) -> Result<bool, String> {
    let options = parse_args(args)?;
    let targets = if options.targets.is_empty() {
        list_targets(root)?
    } else {
        options.targets
    };
    if targets.is_empty() {
        return Err("fuzz/ has no targets".to_owned());
    }
    let host = host_triple(root)?;
    for target in &targets {
        println!("fuzz: running {target} for {}s", options.seconds);
        let status = Command::new("cargo")
            .args(run_args(target, &host, options.seconds))
            .current_dir(root)
            .status()
            .map_err(|error| format!("could not run cargo fuzz: {error}"))?;
        if !status.success() {
            println!("fuzz: {target} failed");
            return Ok(false);
        }
    }
    Ok(true)
}

fn list_targets(root: &Path) -> Result<Vec<String>, String> {
    let output = Command::new("cargo")
        .args([TOOLCHAIN, "fuzz", "list"])
        .current_dir(root)
        .output()
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
fn host_triple(root: &Path) -> Result<String, String> {
    let output = Command::new("rustc")
        .args([TOOLCHAIN, "-vV"])
        .current_dir(root)
        .output()
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

/// The triple on the `host:` line of `rustc -vV` output.
fn parse_host(version: &str) -> Option<&str> {
    version
        .lines()
        .find_map(|line| line.strip_prefix("host:"))
        .map(str::trim)
        .filter(|host| !host.is_empty())
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

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        targets: Vec::new(),
        seconds: DEFAULT_SECONDS,
    };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--time" => {
                let value = args
                    .next()
                    .ok_or_else(|| format!("--time needs a value\n{USAGE}"))?;
                options.seconds = parse_duration(value)?;
            }
            flag if flag.starts_with('-') => return Err(format!("unknown flag {flag}\n{USAGE}")),
            target => options.targets.push(target.to_owned()),
        }
    }
    Ok(options)
}

/// Seconds in `text`: a positive whole number, optionally followed by `s`, `m` or `h`.
fn parse_duration(text: &str) -> Result<u64, String> {
    let invalid = || format!("invalid duration {text:?}\n{USAGE}");
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
        reason = "test code: a failed unwrap is a failed test"
    )]

    use super::{DEFAULT_SECONDS, Options, parse_args, parse_duration, parse_host, run_args};

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
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
            parse_args(&[]).unwrap(),
            Options {
                targets: Vec::new(),
                seconds: DEFAULT_SECONDS
            }
        );
        assert_eq!(
            parse_args(&args(&["qc-progs", "--time", "10m"])).unwrap(),
            Options {
                targets: args(&["qc-progs"]),
                seconds: 600
            }
        );
        assert!(parse_args(&args(&["--time"])).is_err());
        assert!(parse_args(&args(&["--jobs", "2"])).is_err());
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

    #[test]
    fn host_line() {
        let version = "rustc 1.99.0-nightly (abc 2026-10-01)\nbinary: rustc\n\
                       host: x86_64-unknown-linux-gnu\nrelease: 1.99.0-nightly\n";
        assert_eq!(parse_host(version), Some("x86_64-unknown-linux-gnu"));
        assert_eq!(parse_host("rustc 1.99.0\nrelease: 1.99.0\n"), None);
        assert_eq!(parse_host("host:   \n"), None);
    }
}
