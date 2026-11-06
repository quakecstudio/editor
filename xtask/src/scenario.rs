// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask scenario <file> [--repeat <n>] [-- <engine word>...]`: a scenario played in
//! `qs-sim` on a game, as `scenario.run` plays it (SCN-2.1), `n` times, failing unless every
//! run passed and gave the same result (SCN-2.5, P3-19).

use std::fs;
use std::path::{Path, PathBuf};

use qs_sim::scenario::Scenario;
use qs_workbench::scenario::play_on_game;

/// `cargo xtask scenario <file> [--repeat <n>] [-- <engine word>...]`.
#[derive(clap::Parser, Debug)]
pub(crate) struct Args {
    /// The scenario (`docs/spec/scenario.md`).
    scenario: PathBuf,
    /// How many times to play it; every result must be the same.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
    repeat: u32,
    /// The engine's command line, after `--`: `-basedir <dir>` and the like.
    #[arg(last = true)]
    engine: Vec<String>,
}

/// Plays the scenario `args.repeat` times; true when every run passed and all were the same.
pub(crate) fn main(_root: &Path, args: &Args) -> Result<bool, String> {
    let text = fs::read_to_string(&args.scenario)
        .map_err(|e| format!("{}: {e}", args.scenario.display()))?;
    let scenario =
        Scenario::parse(&text).map_err(|e| format!("{}: {e}", args.scenario.display()))?;
    let mut words = args.engine.clone();
    words.extend(scenario.engine.iter().cloned());
    let first = play_on_game(&words, &scenario).map_err(|e| e.to_string())?;
    for check in &first.checks {
        println!(
            "{}  line {}: expect {} (found {})",
            if check.passed { "ok  " } else { "FAIL" },
            check.line,
            check.text,
            check.found
        );
    }
    if let Some(error) = &first.error {
        println!("FAIL  {error}");
    }
    let mut same = true;
    for run in 2..=args.repeat {
        if play_on_game(&words, &scenario).map_err(|e| e.to_string())? != first {
            println!("FAIL  run {run} gave another result than run 1 (SCN-2.5)");
            same = false;
        }
    }
    println!(
        "{}  scenario {}: {} frames, {} assertions, {} runs {}",
        if first.passed && same { "ok  " } else { "FAIL" },
        args.scenario.display(),
        first.frames,
        first.checks.len(),
        args.repeat,
        if same { "the same" } else { "not the same" }
    );
    Ok(first.passed && same)
}
