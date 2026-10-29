// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask`: build automation for the workspace.
//!
//! - `ci`: runs every check below plus fmt, clippy, the tests and cargo-deny;
//!   `make ci` calls it, and so does CI, with `--skip` leaving out the steps
//!   another CI job runs.
//! - `adr`: checks the decision records in `docs/adr/` and their index.
//! - `conventions`: checks every config value against `docs/conventions.md`.
//! - `clippy-fixtures`: proves the workspace lints reject the code they target.
//! - `deny-fixtures`: proves `deny.toml` rejects incompatible licenses and
//!   wildcard versions (needs network access to the crates.io index).
//! - `layers`: checks every crate's layer in `layers.toml` and that it depends
//!   only on its own layer or lower (`docs/layers.md`).
//! - `licenses`: checks the SPDX header of every file and the ported-file
//!   header rule in `docs/porting.md`.
//! - `attribution`: checks every asset file is listed in `ATTRIBUTION.md`.
//! - `contrast`: checks every colour pair in `docs/design/tokens.toml` meets
//!   WCAG AA in both themes.
//! - `bench`: runs the performance-budget benchmarks, checks them against
//!   `docs/perf-budgets.md` and the last run, and records them in the JSON
//!   history; `bench history` prints the history.
//! - `readiness`: measures one readiness check (coverage, mutation score, spec
//!   citations or flakes) for a list of crates and writes the numbers as JSON.
//! - `ratchet`: judges those numbers against the bar in ADR-0009 and the
//!   minimums in `ratchets.toml`, and checks how `ratchets.toml` has changed.
//! - `fuzz`: runs the cargo-fuzz targets in `fuzz/` on nightly for a fixed time
//!   each; `make test-fuzz` and the nightly `Fuzz` workflow call it.
//! - `corpus`: `corpus verify [--online]` checks `corpus/manifest.toml` and the repository
//!   against the corpus policy; `corpus fetch` downloads codebases and maps by checksum into a
//!   cache outside the source tree; `corpus build` compiles the codebases an oracle compiler
//!   builds and checks each progs.dat round-trips through `qc-progs`.
//! - `assets`: `assets fetch` downloads, checks and extracts the open test assets;
//!   `assets precache` checks they cover the rerelease's precaches.
//! - `lex-corpus`: lexes every `.qc` file of a codebase directory and
//!   preprocesses the files its `progs.src` lists, failing on any error.
//! - `oracles`: fetches id's and FTEQCC's sources by checksum, builds the
//!   oracles in `oracles/` (id's VM as a trace runner, id's qcc and FTEQCC) with
//!   the system C compiler and checks them; `oracles id101 <dir>` also compiles
//!   and runs id's QuakeC 1.01, and `oracles rerelease <dir>` compiles the
//!   rerelease with FTEQCC. CI and `make oracles` run it; it is not part of
//!   `ci`, which runs offline.
//! - `map-corpus`: reads every `.map` file under map directories with `qs-map` and
//!   checks that each writes back byte for byte.
//! - `parse-corpus`: parses every `.qc` and `.qh` file under codebase
//!   directories and checks the tree gives back each file; `--vanilla` fails on
//!   any error and `--deletions` checks that recovery from each deleted token is
//!   local.
//! - `project-corpus`: opens codebase directories as projects with
//!   `qc-project` and checks each module's file graph; `--progs` also checks it
//!   against the order of the functions in a `progs.dat` id's qcc compiled, and
//!   `--fteqcc` runs the pinned FTEQCC on every module and checks that the files
//!   it reports compiling and including match the graph, in order.
//! - `sema-corpus`: resolves id 1.01 and the rerelease through `qc-sema` and
//!   compares each index with its golden file, and its global definitions with
//!   the progs.dat an oracle compiler built (P2-54).
//! - `fteqcc-coverage`: parses every `.qc` and `.qh` file under the rerelease
//!   codebase directories with the vanilla parser and lists every construct it
//!   rejects, grouped into the classes of
//!   `docs/spec/dialect-fteqcc-rerelease.md` (P2-01); it fails on a rejected
//!   construct no class names.
//! - `fteqcc-full-coverage`: scans every `.qc`, `.qh` and `.h` file under the
//!   FTEQCC corpus mod directories (nuclide, freecs, freehl) for the full
//!   FTEQCC dialect constructs beyond the rerelease subset, grouped into the
//!   classes of `docs/spec/dialect-fteqcc.md` (P2-25); it fails on a class no
//!   section names.
//! - `fteqcc-profile`: preprocesses, parses and resolves every module of each
//!   rerelease codebase under the FTEQCC profile (P2-03) and fails on any error,
//!   then checks the vanilla profile rejects each extension; `--warnings` prints
//!   each warning too.
//! - `codegen-corpus`: compiles every module of each codebase with `qc-codegen` under the
//!   FTEQCC profile (P2-07) and fails on the first module that does not compile.
//! - `engines`: fetches the fact sources of the engine-target database pinned in
//!   `crates/qc-engines/sources.toml` by checksum and writes `crates/qc-engines/src/tables.rs`
//!   from them (P2-04); `engines --check` fails when the checked-in tables differ.
//! - `commands`: writes the command reference, `docs/commands.md`, from the
//!   `qs-commands` registry (P2-12); `commands --check` fails when it differs.
//!
//! `readiness` and `ratchet` run in the on-demand `Readiness` workflow, not in
//! `ci` (ADR-0009).

#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "xtask is a command-line tool; this is its crate root"
)]

mod adr;
mod attribution;
mod bench;
mod ci;
mod codegen_corpus;
mod commands;
mod contrast;
mod conventions;
mod corpus;
mod differential;
mod download;
mod engines;
mod exec;
mod fixtures;
mod fteqcc_coverage;
mod fteqcc_full_coverage;
mod fteqcc_profile;
mod fuzz;
mod host;
mod layers;
mod lex_corpus;
mod licenses;
mod map_corpus;
mod oracles;
mod parse_corpus;
mod ported;
mod project_corpus;
mod ratchet;
mod readiness;
mod repo;
mod sema_corpus;
mod test_code;
#[cfg(test)]
mod test_dirs;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask <ci | adr | conventions | clippy-fixtures | \
                     deny-fixtures | layers | licenses | attribution | contrast | bench | \
                     readiness | ratchet | fuzz | corpus | assets | lex-corpus | oracles | \
                     map-corpus | parse-corpus | project-corpus | sema-corpus | differential | \
                     fteqcc-coverage | fteqcc-full-coverage | fteqcc-profile | codegen-corpus | \
                     engines | commands>";

/// A command that takes arguments and says itself whether it passed.
type Command = fn(&Path, &[String]) -> Result<bool, String>;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if run(&workspace_root(), &args, &ci::STEPS) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Runs the command `args` names from `root`, with `ci_steps` as the steps of
/// `ci`; returns whether it passed.
fn run(root: &Path, args: &[String], ci_steps: &[ci::Step]) -> bool {
    if let Some((name, rest)) = args.split_first()
        && let Some(command) = command(name)
    {
        return match command(root, rest) {
            Ok(passed) => passed,
            Err(error) => {
                eprintln!("{name}: {error}");
                false
            }
        };
    }
    if let Some((name, rest)) = args.split_first()
        && name == "ci"
    {
        return match ci::main(root, rest, ci_steps) {
            Ok(passed) => passed,
            Err(error) => {
                eprintln!("ci: {error}");
                false
            }
        };
    }
    let [command] = args else {
        eprintln!("{USAGE}");
        return false;
    };
    let Some(check) = in_process_check(command) else {
        eprintln!("unknown command {command:?}\n{USAGE}");
        return false;
    };
    print_outcome(command, check(root))
}

/// The command named `name`, if it takes arguments.
fn command(name: &str) -> Option<Command> {
    match name {
        "assets" => Some(corpus::assets_main),
        "bench" => Some(bench::main),
        "codegen-corpus" => Some(codegen_corpus::main),
        "commands" => Some(commands::main),
        "corpus" => Some(corpus::main),
        "differential" => Some(differential::main),
        "engines" => Some(engines::main),
        "fteqcc-coverage" => Some(fteqcc_coverage::main),
        "fteqcc-full-coverage" => Some(fteqcc_full_coverage::main),
        "fteqcc-profile" => Some(fteqcc_profile::main),
        "fuzz" => Some(fuzz::main),
        "lex-corpus" => Some(lex_corpus::main),
        "map-corpus" => Some(map_corpus::main),
        "oracles" => Some(oracles::main),
        "parse-corpus" => Some(parse_corpus::main),
        "project-corpus" => Some(project_corpus::main),
        "readiness" => Some(readiness::main),
        "sema-corpus" => Some(sema_corpus::main),
        "ratchet" => Some(ratchet::main),
        _ => None,
    }
}

/// A check that runs inside xtask and returns a [`Report`].
pub(crate) type Check = fn(&Path) -> Result<Report, String>;

/// The check named `command`, if xtask runs it in-process.
pub(crate) fn in_process_check(command: &str) -> Option<Check> {
    match command {
        "adr" => Some(adr::check),
        "conventions" => Some(conventions::check),
        "clippy-fixtures" => Some(|root| fixtures::clippy(root, &exec::System)),
        "deny-fixtures" => Some(|root| fixtures::deny(root, &exec::System)),
        "layers" => Some(layers::check),
        "licenses" => Some(licenses::run),
        "attribution" => Some(attribution::run),
        "contrast" => Some(contrast::check),
        "commands" => Some(commands::check),
        _ => None,
    }
}

/// Prints a check's report and returns whether it passed.
pub(crate) fn print_outcome(command: &str, result: Result<Report, String>) -> bool {
    match result {
        Ok(report) => {
            for line in &report.passed {
                println!("ok    {line}");
            }
            for line in &report.problems {
                println!("FAIL  {line}");
            }
            if report.problems.is_empty() {
                println!("{command}: {} checks passed", report.passed.len());
                true
            } else {
                println!("{command}: {} problems", report.problems.len());
                false
            }
        }
        Err(error) => {
            eprintln!("{command}: {error}");
            false
        }
    }
}

fn workspace_root() -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map_or_else(|| manifest_dir.to_path_buf(), Path::to_path_buf)
}

/// What a check found: one line per passed check and per problem.
#[derive(Debug, Default)]
pub(crate) struct Report {
    pub(crate) passed: Vec<String>,
    pub(crate) problems: Vec<String>,
}

impl Report {
    pub(crate) fn record(&mut self, ok: bool, line: String) {
        if ok {
            self.passed.push(line);
        } else {
            self.problems.push(line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::Step;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    const PASSING: [Step; 1] = [Step::Cargo(&["--version"])];
    const FAILING: [Step; 1] = [Step::Cargo(&["no-such-command-xtask-main"])];

    #[test]
    fn every_command_with_arguments_is_found() {
        for name in [
            "assets",
            "bench",
            "corpus",
            "differential",
            "fteqcc-coverage",
            "fteqcc-full-coverage",
            "fteqcc-profile",
            "fuzz",
            "lex-corpus",
            "map-corpus",
            "oracles",
            "parse-corpus",
            "project-corpus",
            "readiness",
            "ratchet",
            "sema-corpus",
        ] {
            assert!(command(name).is_some(), "{name}");
            assert!(USAGE.contains(name), "{name}");
        }
        assert!(command("ci").is_none());
        assert!(command("layers").is_none());
    }

    #[test]
    fn ci_runs_the_steps_it_is_given() {
        let root = workspace_root();
        assert!(run(&root, &args(&["ci"]), &PASSING));
        assert!(!run(&root, &args(&["ci"]), &FAILING));
        let skip = args(&["ci", "--skip", "no-such-command-xtask-main"]);
        assert!(run(&root, &skip, &FAILING));
        assert!(!run(
            &root,
            &args(&["ci", "--skip", "no-such-step"]),
            &PASSING
        ));
        assert!(!run(&root, &args(&["ci", "--no-such-flag"]), &PASSING));
    }

    #[test]
    fn an_in_process_check_runs_without_the_ci_steps() {
        assert!(run(&workspace_root(), &args(&["layers"]), &FAILING));
    }

    #[test]
    fn a_command_with_arguments_reports_its_outcome() {
        let root = workspace_root();
        assert!(!run(&root, &args(&["ratchet", "--no-such-flag"]), &PASSING));
        assert!(run(&root, &args(&["ratchet"]), &FAILING));
    }

    #[test]
    fn a_missing_or_unknown_command_fails() {
        let root = workspace_root();
        assert!(!run(&root, &[], &PASSING));
        assert!(!run(&root, &args(&["no-such-command"]), &PASSING));
        assert!(!run(&root, &args(&["layers", "extra"]), &PASSING));
    }

    #[test]
    fn the_workspace_root_holds_the_workspace_manifest() {
        let manifest = std::fs::read_to_string(workspace_root().join("Cargo.toml"));
        assert!(manifest.is_ok_and(|text| text.contains("[workspace]")));
    }
}
