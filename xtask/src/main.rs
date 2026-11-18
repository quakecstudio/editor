// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask`: build automation for the workspace.
//!
//! - `ci`: runs every check below plus fmt, clippy, the tests and cargo-deny;
//!   `make ci` calls it, and so does CI, with `--skip` leaving out the steps
//!   another CI job runs.
//! - `adr`: checks the decision records in `docs/adr/` and their index.
//! - `conventions`: checks every config value against `docs/conventions.md`.
//! - `layers`: checks every crate's layer in `layers.toml` and that it depends
//!   only on its own layer or lower (`docs/layers.md`), and that the document's
//!   crate table is the one `layers.toml` renders; `layers --write` writes it.
//! - `licenses`: checks the SPDX header of every file and the ported-file
//!   header rule in `docs/porting.md`.
//! - `attribution`: checks every asset file is listed in `ATTRIBUTION.md`.
//! - `notices`: writes the third-party notices a release package carries
//!   (`--out`), or checks every crate the binaries link states a license.
//! - `contrast`: checks every colour pair in `docs/design/tokens.toml` meets
//!   WCAG AA in both themes.
//! - `bench`: runs the performance-budget benchmarks, checks them against
//!   `docs/perf-budgets.md` and the last run, and records them in the JSON
//!   history; `bench history` prints the history.
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
//! - `entity-corpus`: derives the entity definitions of id 1.01 and the rerelease
//!   with `qc-sema`, compares each FGD with its golden file, and with `--maps`
//!   checks that every classname a game's maps use has a class (P5-03).
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
//! - `lsp-corpus`: drives the language server against the rerelease over the
//!   protocol, every request at a set of probes, and compares the answers with
//!   their golden file (P2-14).
//! - `fteqcc-profile`: preprocesses, parses and resolves every module of each
//!   rerelease codebase under the FTEQCC profile (P2-03) and fails on any error,
//!   then checks the vanilla profile rejects each extension; `--warnings` prints
//!   each warning too.
//! - `format-corpus [--vanilla] <dir>...`: formats every file of each codebase twice, the
//!   second time changing nothing, and compiles each module before and after to the same
//!   progs.dat (P4-34).
//! - `codegen-corpus`: compiles every module of each codebase with `qc-codegen` under the
//!   FTEQCC profile (P2-07) and fails when any module does not compile.
//! - `engines`: fetches the fact sources of the engine-target database pinned in
//!   `crates/qc-engines/sources.toml` by checksum and writes `crates/qc-engines/src/tables.rs`
//!   from them (P2-04); `engines --check` fails when the checked-in tables differ.
//! - `commands`: writes the command reference, `docs/commands.md`, from the
//!   `qs-commands` registry (P2-12); `commands --check` fails when it differs.

#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "xtask is a command-line tool; this is its crate root"
)]

mod adr;
mod attribution;
mod bench;
mod bridge_client;
mod ci;
mod codegen_corpus;
mod commands;
mod compatibility;
mod compiler_diff;
mod contrast;
mod conv17;
mod conventions;
mod corpus;
mod differential;
mod download;
mod engines;
mod entity_corpus;
mod evals;
mod exec;
mod format_corpus;
mod fteqcc_profile;
mod fuzz;
mod host;
mod keymap;
mod layers;
mod lex_corpus;
mod licenses;
mod light_diff;
mod lsp_corpus;
mod map_corpus;
mod map_diff;
mod notices;
mod offline;
mod oracles;
mod parse_corpus;
mod perf_baseline;
mod ported;
mod project_corpus;
mod release_host;
mod repo;
mod scenario;
mod sema_corpus;
mod shard;
mod sim_diff;
#[cfg(test)]
mod test_dirs;
mod view_diff;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// `cargo xtask <command>`: the repository's build automation.
#[derive(clap::Parser, Debug)]
#[command(name = "cargo xtask", bin_name = "cargo xtask")]
struct Cli {
    #[command(subcommand)]
    command: Task,
}

/// Each command.
#[derive(clap::Subcommand, Debug)]
enum Task {
    /// Every check CI runs.
    Ci(ci::Args),
    /// ADRs follow the template and are indexed.
    Adr,
    /// Every asset is listed in ATTRIBUTION.md.
    Attribution,
    /// Fetch or precache the test assets.
    Assets(corpus::AssetsArgs),
    /// The performance budgets.
    Bench(bench::Args),
    /// The bridge client against the real preview host, headless.
    BridgeClient(bridge_client::Args),
    /// Every corpus codebase compiles with our compiler.
    CodegenCorpus(codegen_corpus::Args),
    /// Formatting each codebase is stable and changes no progs.dat.
    FormatCorpus(format_corpus::Args),
    /// Write or check docs/commands.md.
    Commands(commands::Args),
    /// Write or check docs/keymap.md.
    Keymap(keymap::Args),
    /// The compiler differential.
    CompilerDiff(compiler_diff::Args),
    /// The theme's colours meet their contrast.
    Contrast,
    /// Config files match docs/conventions.md, and no CONV-17 violation is beyond its baseline.
    Conventions,
    /// Writes docs/audits/conv-17-baseline.tsv from the code as it is (P5-18).
    Conv17Baseline,
    /// The corpus manifest and cache.
    Corpus(corpus::Args),
    /// The VM differential suite.
    Differential(differential::Args),
    /// Write or check the engine-target tables.
    Engines(engines::Args),
    /// A scenario played in qs-sim on a game, repeated, each result the same (SCN-2.5).
    Scenario(scenario::Args),
    /// The agent's evals: check each task's scenario, or run the tasks on a model (P4-15).
    Evals(evals::Args),
    /// Write docs/compatibility.md from the conformance corpus (P4-21, COMP-9).
    Compatibility(compatibility::Args),
    /// The app with the network denied and traced: no request with no provider (P4-16).
    Offline(offline::Args),
    /// The server simulation differential: qs-sim against id's server on a route.
    SimDiff(sim_diff::Args),
    /// The view differential: the preview host's frames and sound against id's engine.
    ViewDiff(view_diff::Args),
    /// Every module parses and resolves under the FTEQCC profile.
    FteqccProfile(fteqcc_profile::Args),
    /// Run the fuzz targets.
    Fuzz(fuzz::Args),
    /// No crate depends on a higher layer; write or check the crate table of docs/layers.md.
    Layers(layers::Args),
    /// Every corpus file lexes.
    LexCorpus(lex_corpus::Args),
    /// SPDX lines and ported-file headers.
    Licenses,
    /// Write the third-party notices a release package carries, or check every linked crate
    /// states a license.
    Notices(notices::Args),
    /// The clippy performance lints left off are held to docs/audits/perf-baseline.tsv, which
    /// only shrinks (L-18).
    PerfBaseline(perf_baseline::Args),
    /// The language server over a codebase, against its golden file.
    LspCorpus(lsp_corpus::Args),
    /// Every .map round-trips.
    MapCorpus(map_corpus::Args),
    /// The map compiler differential: qs-map-bsp against id's and ericw-tools' qbsp.
    MapDiff(map_diff::Args),
    /// The light compiler differential: qs-map-light's lightmaps against id's light, and its
    /// coloured light against ericw-tools'.
    LightDiff(light_diff::Args),
    /// Each codebase's entity definitions, against their golden FGD and the maps' classnames.
    EntityCorpus(entity_corpus::Args),
    /// Build and run the oracles.
    Oracles(oracles::Args),
    /// Every corpus file parses.
    ParseCorpus(parse_corpus::Args),
    /// Every corpus project's file graph.
    ProjectCorpus(project_corpus::Args),
    /// Every corpus module's index, against its golden file.
    SemaCorpus(sema_corpus::Args),
}

fn main() -> ExitCode {
    let cli = match <Cli as clap::Parser>::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            // `--help` and `--version` are not failures.
            let _printed = error.print();
            return if error.use_stderr() {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            };
        }
    };
    if run(&workspace_root(), &cli.command, &ci::STEPS) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Runs `task` from `root`, with `ci_steps` as the steps of `ci`; returns whether it passed.
fn run(root: &Path, task: &Task, ci_steps: &[ci::Step]) -> bool {
    let (name, result) = match task {
        Task::Ci(args) => ("ci", ci::main(root, args, ci_steps)),
        Task::Assets(args) => ("assets", corpus::assets_main(root, args)),
        Task::Bench(args) => ("bench", bench::main(root, args)),
        Task::BridgeClient(args) => ("bridge-client", bridge_client::main(root, args)),
        Task::CodegenCorpus(args) => ("codegen-corpus", codegen_corpus::main(root, args)),
        Task::FormatCorpus(args) => ("format-corpus", format_corpus::main(root, args)),
        Task::Commands(args) => ("commands", commands::main(root, args)),
        Task::Keymap(args) => ("keymap", keymap::main(root, args)),
        Task::Layers(args) => ("layers", layers::main(root, args)),
        Task::CompilerDiff(args) => ("compiler-diff", compiler_diff::main(root, args)),
        Task::Corpus(args) => ("corpus", corpus::main(root, args)),
        Task::Differential(args) => ("differential", differential::main(root, args)),
        Task::Engines(args) => ("engines", engines::main(root, args)),
        Task::FteqccProfile(args) => ("fteqcc-profile", fteqcc_profile::main(root, args)),
        Task::Fuzz(args) => ("fuzz", fuzz::main(root, args)),
        Task::LexCorpus(args) => ("lex-corpus", lex_corpus::main(root, args)),
        Task::LspCorpus(args) => ("lsp-corpus", lsp_corpus::main(root, args)),
        Task::MapCorpus(args) => ("map-corpus", map_corpus::main(root, args)),
        Task::MapDiff(args) => (
            "map-diff",
            map_diff::main(root, &exec::System, &download::CURL, args),
        ),
        Task::LightDiff(args) => (
            "light-diff",
            light_diff::main(root, &exec::System, &download::CURL, args),
        ),
        Task::EntityCorpus(args) => ("entity-corpus", entity_corpus::main(root, args)),
        Task::Oracles(args) => ("oracles", oracles::main(root, args)),
        Task::ParseCorpus(args) => ("parse-corpus", parse_corpus::main(root, args)),
        Task::ProjectCorpus(args) => ("project-corpus", project_corpus::main(root, args)),
        Task::SemaCorpus(args) => ("sema-corpus", sema_corpus::main(root, args)),
        Task::Notices(args) => ("notices", notices::main(root, args)),
        Task::PerfBaseline(args) => ("perf-baseline", perf_baseline::main(root, args)),
        Task::Scenario(args) => ("scenario", scenario::main(root, args)),
        Task::Evals(args) => ("evals", evals::main(root, args)),
        Task::Compatibility(args) => ("compatibility", compatibility::main(root, args)),
        Task::Offline(args) => ("offline", offline::main(root, args)),
        Task::SimDiff(args) => ("sim-diff", sim_diff::main(root, args)),
        Task::ViewDiff(args) => ("view-diff", view_diff::main(root, args)),
        Task::Adr => return print_outcome("adr", adr::check(root)),
        Task::Attribution => return print_outcome("attribution", attribution::run(root)),
        Task::Contrast => return print_outcome("contrast", contrast::check(root)),
        Task::Conventions => return print_outcome("conventions", conventions::check(root)),
        Task::Conv17Baseline => {
            return match conv17::write_baseline(root) {
                Ok(count) => {
                    println!(
                        "conv17-baseline: wrote {} with {count} entries",
                        conv17::BASELINE
                    );
                    true
                }
                Err(error) => {
                    eprintln!("conv17-baseline: {error}");
                    false
                }
            };
        }
        Task::Licenses => return print_outcome("licenses", licenses::run(root)),
    };
    match result {
        Ok(passed) => passed,
        Err(error) => {
            eprintln!("{name}: {error}");
            false
        }
    }
}

/// A check that runs inside xtask and returns a [`Report`].
pub(crate) type Check = fn(&Path) -> Result<Report, String>;

/// The check named `command`, if xtask runs it in-process.
pub(crate) fn in_process_check(command: &str) -> Option<Check> {
    match command {
        "adr" => Some(adr::check),
        "conventions" => Some(conventions::check),
        "layers" => Some(layers::check),
        "licenses" => Some(licenses::run),
        "attribution" => Some(attribution::run),
        "notices" => Some(notices::check),
        "contrast" => Some(contrast::check),
        "commands" => Some(commands::check),
        "keymap" => Some(keymap::check),
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

/// `T` from the command line `args`, after a program name, as clap reads it; for the tests of
/// each command's options.
#[cfg(test)]
pub(crate) fn parse_cli<T: clap::Parser, S: AsRef<str>>(args: &[S]) -> Result<T, clap::Error> {
    T::try_parse_from(core::iter::once("xtask").chain(args.iter().map(AsRef::as_ref)))
}

/// `main` on the command line `list` as clap reads it, a parse error as its message.
#[cfg(test)]
pub(crate) fn run_cli<T: clap::Parser, S: AsRef<str>>(
    list: &[S],
    main: impl FnOnce(&T) -> Result<bool, String>,
) -> Result<bool, String> {
    let parsed = parse_cli::<T, S>(list).map_err(|e| e.to_string())?;
    main(&parsed)
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
    use clap::CommandFactory as _;

    use super::*;
    use crate::ci::Step;

    /// The task the command line `list` names.
    fn task<S: AsRef<str>>(list: &[S]) -> Result<Task, clap::Error> {
        parse_cli::<Cli, S>(list).map(|cli| cli.command)
    }

    const PASSING: [Step; 1] = [Step::Cargo(&["--version"])];
    const FAILING: [Step; 1] = [Step::Cargo(&["no-such-command-xtask-main"])];

    #[test]
    fn every_command_parses_by_its_name() {
        for name in [
            "adr",
            "attribution",
            "contrast",
            "conventions",
            "layers",
            "licenses",
            "commands",
            "engines",
            "bench",
            "fuzz",
            "differential",
            "ci",
        ] {
            assert!(task(&[name]).is_ok(), "{name}");
        }
        for name in [
            "assets",
            "codegen-corpus",
            "format-corpus",
            "compiler-diff",
            "corpus",
            "fteqcc-profile",
            "lex-corpus",
            "lsp-corpus",
            "map-corpus",
            "entity-corpus",
            "oracles",
            "parse-corpus",
            "project-corpus",
            "sema-corpus",
        ] {
            assert!(Cli::command().find_subcommand(name).is_some(), "{name}");
        }
        Cli::command().debug_assert();
    }

    #[test]
    fn ci_runs_the_steps_it_is_given() {
        let root = workspace_root();
        let ci = |list: &[&str]| task(list).unwrap_or(Task::Adr);
        assert!(run(&root, &ci(&["ci"]), &PASSING));
        assert!(!run(&root, &ci(&["ci"]), &FAILING));
        let skip = ci(&["ci", "--skip", "no-such-command-xtask-main"]);
        assert!(run(&root, &skip, &FAILING));
        assert!(!run(
            &root,
            &ci(&["ci", "--skip", "no-such-step"]),
            &PASSING
        ));
        assert!(task(&["ci", "--no-such-flag"]).is_err());
    }

    #[test]
    fn an_in_process_check_runs_without_the_ci_steps() {
        assert!(run(
            &workspace_root(),
            &Task::Layers(layers::Args::default()),
            &FAILING
        ));
    }

    #[test]
    fn a_command_with_arguments_reports_its_outcome() {
        let root = workspace_root();
        assert!(task(&["corpus", "verify", "--no-such-flag"]).is_err());
        assert!(task(&["corpus", "verify"]).is_ok_and(|verify| run(&root, &verify, &FAILING)));
    }

    #[test]
    fn a_missing_or_unknown_command_fails() {
        assert!(task::<&str>(&[]).is_err());
        assert!(task(&["no-such-command"]).is_err());
        assert!(task(&["layers", "extra"]).is_err());
    }

    /// Every command line a workflow runs (a `run:` line or a step's continuation that starts
    /// with `cargo xtask`), its shell variables and expansions as one word and anything after
    /// a pipe or `||` dropped, as clap reads it.
    #[test]
    fn every_command_line_the_workflows_run_parses() {
        let dir = workspace_root().join(".github/workflows");
        let mut seen = 0_usize;
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let text = std::fs::read_to_string(entry.path()).unwrap_or_default();
            let mut lines = text.lines().map(str::trim).peekable();
            while let Some(line) = lines.next() {
                let line = line.trim_start_matches("run:").trim();
                let Some(at) = line.find("cargo xtask ") else {
                    continue;
                };
                // Only `VAR=value` assignments may come before the command.
                let before = line.get(..at).unwrap_or_default();
                if !before.split_whitespace().all(|word| word.contains('=')) {
                    continue;
                }
                let mut command = line.get(at..).unwrap_or_default().to_owned();
                while command.ends_with('\\') {
                    command.pop();
                    command.push(' ');
                    command.push_str(lines.next().unwrap_or_default());
                }
                let command = command.split(" | ").next().unwrap_or_default();
                let command = command.split(" || ").next().unwrap_or_default();
                let words: Vec<String> = command
                    .split_whitespace()
                    .skip(2)
                    .map(|word| {
                        if word.contains("$SHARD") {
                            // A shard, `k/n`, as the matrix gives it.
                            "0/1".to_owned()
                        } else if word.contains('$') {
                            "x".to_owned()
                        } else {
                            word.trim_matches('"').to_owned()
                        }
                    })
                    .collect();
                assert!(
                    task(&words).is_ok(),
                    "{}: {command}",
                    entry.path().display()
                );
                seen = seen.saturating_add(1);
            }
        }
        assert!(seen >= 20, "{seen} command lines");
    }

    #[test]
    fn the_workspace_root_holds_the_workspace_manifest() {
        let manifest = std::fs::read_to_string(workspace_root().join("Cargo.toml"));
        assert!(manifest.is_ok_and(|text| text.contains("[workspace]")));
    }
}
