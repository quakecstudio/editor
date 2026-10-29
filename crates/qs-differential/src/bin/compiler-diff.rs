// SPDX-License-Identifier: GPL-3.0-or-later

//! `compiler-diff`: the compiler differential suite against id's `qcc` (P2-09, COMP-4,
//! `docs/spec/differential.md` DIFF-12).
//!
//! Calls every QuakeC function of the progs `qcc` compiled and of each progs `qc-codegen`
//! compiled from the same codebase, in `qc-vm` from the worlds of the differential scenarios,
//! and compares the calls (DIFF-12.3, DIFF-12.4). `cargo xtask compiler-diff` compiles the
//! codebases and runs this binary.
//!
//! ```text
//! compiler-diff --mode <fast|full> --codebase <id> --theirs <qcc progs> --ours <progs>...
//! ```
//!
//! `--mode fast` makes 32 rounds of calls to each function, `--mode full` 128 (DIFF-12.6).
//! It exits 0 when every pair has no divergence and the calls executed at least 90% of the
//! `qcc` progs' statements (DIFF-12.5), 1 otherwise, and 2 on a usage or setup error.

#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "compiler-diff is a command-line tool; this is its binary"
)]

use std::path::PathBuf;
use std::process::ExitCode;

use qs_differential::calls::{CodebaseReport, FAST_ROUNDS, FULL_ROUNDS, run_codebase};

/// The divergences printed for each pair; the rest are counted.
const PRINTED: usize = 10;

const USAGE: &str = "usage: compiler-diff --mode <fast|full> --codebase <id> --theirs <progs> \
                     --ours <progs> [--ours <progs>...]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("compiler-diff: {error}");
            ExitCode::from(2)
        }
    }
}

/// The parsed command line.
struct Options {
    rounds: u32,
    codebase: String,
    theirs: PathBuf,
    ours: Vec<PathBuf>,
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut rounds = FAST_ROUNDS;
    let mut codebase = None;
    let mut theirs = None;
    let mut ours = Vec::new();
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        let mut value = || rest.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--mode" => {
                rounds = match value()?.as_str() {
                    "fast" => FAST_ROUNDS,
                    "full" => FULL_ROUNDS,
                    other => return Err(format!("--mode takes fast or full, not {other:?}")),
                };
            }
            "--codebase" => codebase = Some(value()?.clone()),
            "--theirs" => theirs = Some(PathBuf::from(value()?)),
            "--ours" => ours.push(PathBuf::from(value()?)),
            other => return Err(format!("unknown argument {other:?}\n{USAGE}")),
        }
    }
    if ours.is_empty() {
        return Err(USAGE.to_owned());
    }
    Ok(Options {
        rounds,
        codebase: codebase.ok_or(USAGE)?,
        theirs: theirs.ok_or(USAGE)?,
        ours,
    })
}

fn run(args: &[String]) -> Result<bool, String> {
    let options = parse(args)?;
    let read = |path: &PathBuf| std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()));
    let theirs = read(&options.theirs)?;
    let mut all_ok = true;
    // Two of our progs with the same bytes have the same report: the run is deterministic.
    let mut reports: Vec<(Vec<u8>, CodebaseReport)> = Vec::new();
    for path in &options.ours {
        let ours = read(path)?;
        let report = match reports.iter().find(|(bytes, _)| *bytes == ours) {
            Some((_, report)) => report.clone(),
            None => {
                let report =
                    run_codebase(&theirs, &ours, options.rounds).map_err(|e| e.to_string())?;
                reports.push((ours.clone(), report.clone()));
                report
            }
        };
        all_ok &= print(&options.codebase, path, ours == theirs, &report);
    }
    Ok(all_ok)
}

/// Prints one pair's report and returns whether it passed (DIFF-12.5).
fn print(codebase: &str, ours: &std::path::Path, same: bool, report: &CodebaseReport) -> bool {
    println!("compiler-diff: {codebase}, qcc against {}", ours.display());
    if same {
        println!("      the same bytes as qcc's");
    }
    let mut printed = 0_usize;
    for (name, scenario) in &report.scenarios {
        println!(
            "      {name}: {} calls, {} divergences, {:.1}% of statements",
            scenario.calls,
            scenario.divergences.len(),
            scenario.coverage() * 100.0
        );
        for divergence in scenario
            .divergences
            .iter()
            .take(PRINTED.saturating_sub(printed))
        {
            let function = divergence.function.as_deref().map_or_else(
                || "the setup".to_owned(),
                |f| String::from_utf8_lossy(f).into_owned(),
            );
            println!(
                "FAIL  {name}: {function}, round {}: {:?}",
                divergence.round, divergence.what
            );
            printed = printed.saturating_add(1);
        }
    }
    let line = format!(
        "{codebase}: {} divergences, {} of {} statements ({:.1}%)",
        report.divergences(),
        report.executed.len(),
        report.total_statements,
        report.coverage() * 100.0
    );
    if report.passes() {
        println!("ok    {line}");
    } else {
        println!("FAIL  {line}; the bar is no divergence and 90% of statements");
    }
    report.passes()
}
