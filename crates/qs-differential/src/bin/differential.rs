// SPDX-License-Identifier: GPL-3.0-or-later

//! `differential`: the end-to-end differential VM suite over the corpus (P1-13, FND-8, QA-1).
//!
//! Runs each corpus codebase's scenarios in the id VM oracle and in QuakeC Studio's runner and
//! compares the traces (`docs/spec/differential.md` DIFF-9), reporting the first divergence and
//! the statement coverage per codebase (DIFF-11). It needs the oracle built
//! (`cargo xtask oracles`) and the corpus progs built (`cargo xtask corpus build`); `cargo
//! xtask differential` and CI build those and run this binary.
//!
//! ```text
//! differential --mode <fast|full> --oracle <vm-oracle path> --assets <asset cache>
//! ```
//!
//! `--mode fast` (the default) runs id 1.01 and enforces zero divergences (DIFF-9.6); `--mode
//! full` runs every oracle-built codebase and enforces zero divergences and the 90%
//! statement-coverage bar (TEST-6). It exits 0 when every enforced criterion passed, 1
//! otherwise, and 2 on a usage or setup error.

#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "differential is a command-line tool; this is its binary"
)]

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use qc_trace::decode;
use qs_differential::runner::{self, Mode};
use qs_differential::suite::SuiteError;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("differential: {error}");
            ExitCode::from(2)
        }
    }
}

/// The parsed command line: the mode, the oracle binary and the asset cache.
struct Options {
    mode: Mode,
    oracle: PathBuf,
    assets: PathBuf,
}

const USAGE: &str =
    "usage: differential --mode <fast|full> --oracle <vm-oracle> --assets <asset cache>";

fn parse(args: &[String]) -> Result<Options, String> {
    let mut mode = Mode::Fast;
    let mut oracle = None;
    let mut assets = None;
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        match flag.as_str() {
            "--mode" => {
                mode = match rest.next().map(String::as_str) {
                    Some("fast") => Mode::Fast,
                    Some("full") => Mode::Full,
                    other => return Err(format!("--mode takes fast or full, not {other:?}")),
                };
            }
            "--oracle" => {
                oracle = Some(PathBuf::from(rest.next().ok_or("--oracle needs a path")?));
            }
            "--assets" => {
                assets = Some(PathBuf::from(rest.next().ok_or("--assets needs a path")?));
            }
            other => return Err(format!("unknown argument {other:?}\n{USAGE}")),
        }
    }
    Ok(Options {
        mode,
        oracle: oracle.ok_or(USAGE)?,
        assets: assets.ok_or(USAGE)?,
    })
}

fn run(args: &[String]) -> Result<bool, String> {
    let options = parse(args)?;
    if !options.oracle.is_file() {
        return Err(format!(
            "the VM oracle is not at {}; run `cargo xtask oracles` first",
            options.oracle.display()
        ));
    }
    let work = PathBuf::from("target").join("differential");
    std::fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;

    let mut all_ok = true;
    for id in options.mode.codebases() {
        let progs_path = runner::progs_path(&options.assets, id);
        let progs = match std::fs::read(&progs_path) {
            Ok(bytes) => bytes,
            Err(e) => {
                println!(
                    "FAIL  {id}: {}: {e}; run `cargo xtask corpus build {id}` first",
                    progs_path.display()
                );
                all_ok = false;
                continue;
            }
        };
        let oracle_bin = options.oracle.clone();
        let mut oracle =
            move |scenario: &Path, trace: &Path| run_oracle(&oracle_bin, scenario, trace);
        match runner::run_codebase(id, &progs, &work, &mut oracle) {
            Ok(report) => all_ok &= runner::report_line(&report, options.mode),
            Err(e) => {
                println!("FAIL  {id}: {e}");
                all_ok = false;
            }
        }
    }
    if all_ok {
        println!(
            "differential: every codebase agreed with the oracle{}",
            if options.mode.enforces_coverage() {
                " and met the 90% statement-coverage bar"
            } else {
                ""
            }
        );
    }
    Ok(all_ok)
}

/// Runs the id VM oracle on `scenario`, writing `trace`, and returns the decoded trace bytes.
fn run_oracle(oracle: &Path, scenario: &Path, trace: &Path) -> Result<Vec<u8>, SuiteError> {
    if trace.exists() {
        std::fs::remove_file(trace)
            .map_err(|e| SuiteError::OracleTrace(format!("{}: {e}", trace.display())))?;
    }
    let output = Command::new(oracle)
        .arg("scenario")
        .arg(scenario)
        .arg(trace)
        .output()
        .map_err(|e| SuiteError::OracleTrace(format!("cannot run the VM oracle: {e}")))?;
    if !output.status.success() {
        return Err(SuiteError::OracleTrace(format!(
            "the VM oracle failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let bytes = std::fs::read(trace)
        .map_err(|e| SuiteError::OracleTrace(format!("{}: {e}", trace.display())))?;
    decode(&bytes).map_err(|e| SuiteError::OracleTrace(format!("{e:?}")))?;
    Ok(bytes)
}
