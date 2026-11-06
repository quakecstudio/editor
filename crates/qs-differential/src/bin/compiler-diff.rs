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
//! compiler-diff --mode <fast|full> --codebase <id> [--oracle <qcc|fteqcc>] [--summary <file>]
//!     --theirs <progs> --ours <progs>...
//! ```
//!
//! `--theirs` is the oracle compiler's progs: id's `qcc`'s for id 1.01, the pinned FTEQCC's
//! for the rerelease (DIFF-12.30), named by `--oracle`, `qcc` when it is not given (DIFF-12.11). With `--summary`, a row of the results table is added to the
//! file for each pair.
//!
//! `--mode fast` makes 32 rounds of calls to each function, `--mode full` 128 (DIFF-12.29).
//! It exits 0 when every pair has no divergence and the calls executed at least 90% of the
//! `qcc` progs' statements (DIFF-12.5), 1 otherwise, and 2 on a usage or setup error.
//!
//! Two more forms serve the generated-program differential (DIFF-14):
//!
//! ```text
//! compiler-diff --generate <dir> --seed <n> --count <n>
//! compiler-diff --pairs <file> [--mode <fast|full>] [--oracle <qcc|fteqcc>] [--engine <id|vkquake>]
//! ```
//!
//! `--generate` writes program `n` of the seed to `<dir>/<n>/t.qc` with a `progs.src` that
//! lists it, and the forms every program used, with their counts, to `<dir>/forms.tsv`.
//! `--pairs` compares each line's pair, `name`, the oracle's progs and ours separated by tabs,
//! and optionally the oracle with its defects fixed (DIFF-14.5), in one process,
//! printing one line per pair, and exits 0 when no pair diverges: a generated
//! program's calls need not reach 90% of its statements, since every function of it is
//! called and a bounded loop is left early by its counter. `--function-pairs` reads the same
//! lines, each named by the functions of a cut program its comparison calls, separated by
//! commas (DIFF-15.9), and adds the statements of the oracle's progs those calls executed to the `--executed`
//! file, by function and offset (DIFF-15.5). `--setup-functions` prints the name of each
//! function the scenarios' setups run in a progs, one a line (DIFF-15.8).

#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "compiler-diff is a command-line tool; this is its binary"
)]

use std::path::PathBuf;
use std::process::ExitCode;

use qc_builtins::EngineProfile;

use qs_differential::calls::{
    CodebaseReport, FAST_ROUNDS, FULL_ROUNDS, FUNCTION_ROUNDS, Oracle, executed_by_function,
    run_codebase, run_functions, setup_functions, uncovered,
};
use qs_differential::source::{self, FORMS};

/// The divergences printed for each pair; the rest are counted.
const PRINTED: usize = 10;

const USAGE: &str = "usage: compiler-diff --mode <fast|full> --codebase <id> \
                     [--oracle <qcc|fteqcc>] [--engine <id|vkquake>] [--summary <file>] [--uncovered <file>] \
                     --theirs <progs> \
                     --ours <progs> [--ours <progs>...]\n\
       compiler-diff --generate <dir> --seed <n> --count <n>\n\
       compiler-diff --pairs <file> [--mode <fast|full>] [--oracle <qcc|fteqcc>] [--engine <id|vkquake>]\n\
       compiler-diff --function-pairs <file> --executed <file> [--oracle <qcc|fteqcc>] [--engine <id|vkquake>]\n\
       compiler-diff --setup-functions <progs> [--engine <id|vkquake>]";

fn main() -> ExitCode {
    let flags = <Flags as clap::Parser>::parse();
    match flags.into_run().and_then(run) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("compiler-diff: {error}\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// The command line's flags, each form taking the ones it needs (see [`USAGE`]).
#[derive(clap::Parser, Debug)]
#[command(name = "compiler-diff")]
struct Flags {
    /// The CI subset's rounds, or the nightly run's.
    #[arg(long, default_value = "fast", value_parser = rounds)]
    mode: u32,
    /// The codebase compared.
    #[arg(long)]
    codebase: Option<String>,
    /// The compiler that wrote the oracle's progs.
    #[arg(long, default_value = "qcc", value_parser = oracle)]
    oracle: Oracle,
    /// Whose engine rules the mock follows (BUILTIN-18).
    #[arg(long, default_value = "id", value_parser = engine)]
    engine: EngineProfile,
    /// The results table to add a row to.
    #[arg(long)]
    summary: Option<PathBuf>,
    /// Where to write each function's statements the calls did not execute (DIFF-12.13).
    #[arg(long)]
    uncovered: Option<PathBuf>,
    /// The oracle's progs.
    #[arg(long)]
    theirs: Option<PathBuf>,
    /// Each of our progs.
    #[arg(long)]
    ours: Vec<PathBuf>,
    /// Write generated programs under this directory (DIFF-14.1).
    #[arg(long)]
    generate: Option<PathBuf>,
    /// The generated programs' seed.
    #[arg(long)]
    seed: Option<u64>,
    /// How many programs to generate.
    #[arg(long)]
    count: Option<u64>,
    /// The pairs to compare, one a line (DIFF-14.9).
    #[arg(long)]
    pairs: Option<PathBuf>,
    /// The function pairs to compare (DIFF-15.9).
    #[arg(long = "function-pairs")]
    function_pairs: Option<PathBuf>,
    /// Where the executed statements are added (DIFF-15.5).
    #[arg(long)]
    executed: Option<PathBuf>,
    /// Print the functions each scenario's setup runs in this progs (DIFF-15.8).
    #[arg(long = "setup-functions")]
    setup_functions: Option<PathBuf>,
}

/// The rounds `--mode` names.
fn rounds(text: &str) -> Result<u32, String> {
    match text {
        "fast" => Ok(FAST_ROUNDS),
        "full" => Ok(FULL_ROUNDS),
        other => Err(format!("--mode takes fast or full, not {other:?}")),
    }
}

/// The oracle `--oracle` names.
fn oracle(text: &str) -> Result<Oracle, String> {
    match text {
        "qcc" => Ok(Oracle::Qcc),
        "fteqcc" => Ok(Oracle::Fteqcc),
        other => Err(format!("--oracle takes qcc or fteqcc, not {other:?}")),
    }
}

/// The engine `--engine` names.
fn engine(text: &str) -> Result<EngineProfile, String> {
    match text {
        "id" => Ok(EngineProfile::Id),
        "vkquake" => Ok(EngineProfile::VkQuake),
        other => Err(format!("--engine takes id or vkquake, not {other:?}")),
    }
}

/// The parsed command line.
struct Options {
    rounds: u32,
    codebase: String,
    oracle: Oracle,
    /// Whose engine rules the mock follows (BUILTIN-18).
    engine: EngineProfile,
    summary: Option<PathBuf>,
    /// Where to write each function's statements the calls did not execute (DIFF-12.13).
    uncovered: Option<PathBuf>,
    theirs: PathBuf,
    ours: Vec<PathBuf>,
}

/// The `--generate` form: where to write the programs, the seed and how many.
struct Generate {
    dir: PathBuf,
    seed: u64,
    count: u64,
}

/// What the command line asks for.
enum Run {
    /// One oracle progs against each of ours.
    Suite(Options),
    /// Programs of a seed written out (DIFF-14.1).
    Generate(Generate),
    /// The functions each scenario's setup runs in a progs, one name a line (DIFF-15.8).
    SetupFunctions {
        progs: PathBuf,
        engine: EngineProfile,
        /// The codebase whose scenarios set the worlds up, or none for the shared ones.
        codebase: String,
    },
    /// The pairs a file lists, each compared (DIFF-14.9, DIFF-15.9).
    Pairs {
        file: PathBuf,
        calls: Calls,
        oracle: Oracle,
        engine: EngineProfile,
        /// The codebase whose scenarios set the worlds up, or none for the shared ones.
        codebase: String,
    },
}

/// The functions each pair's comparison calls.
enum Calls {
    /// Every function, in this many rounds (DIFF-14.9).
    Every(u32),
    /// The functions the line names, separated by commas, each in [`FUNCTION_ROUNDS`] rounds,
    /// the statements of the oracle's progs the calls executed added to this file (DIFF-15.9,
    /// DIFF-15.5).
    Named(PathBuf),
}

/// The run the command line `args` asks for, as clap reads it.
#[cfg(test)]
fn parse(args: &[String]) -> Result<Run, String> {
    let words = core::iter::once("compiler-diff").chain(args.iter().map(String::as_str));
    <Flags as clap::Parser>::try_parse_from(words)
        .map_err(|e| e.to_string())?
        .into_run()
}

impl Flags {
    /// The run these flags ask for: `--generate`, then `--setup-functions`, `--pairs`,
    /// `--function-pairs`, else the suite.
    fn into_run(self) -> Result<Run, String> {
        let Self {
            mode: rounds,
            codebase,
            oracle,
            engine,
            summary,
            uncovered,
            theirs,
            ours,
            generate,
            seed,
            count,
            pairs,
            function_pairs,
            executed,
            setup_functions: setup,
        } = self;
        if let Some(dir) = generate {
            return Ok(Run::Generate(Generate {
                dir,
                seed: seed.ok_or("a missing flag for this form")?,
                count: count.ok_or("a missing flag for this form")?,
            }));
        }
        if let Some(progs) = setup {
            return Ok(Run::SetupFunctions {
                progs,
                engine,
                codebase: codebase.unwrap_or_default(),
            });
        }
        if let Some(file) = pairs {
            return Ok(Run::Pairs {
                file,
                calls: Calls::Every(rounds),
                oracle,
                engine,
                codebase: codebase.unwrap_or_default(),
            });
        }
        if let Some(file) = function_pairs {
            return Ok(Run::Pairs {
                file,
                calls: Calls::Named(executed.ok_or("a missing flag for this form")?),
                oracle,
                engine,
                codebase: codebase.unwrap_or_default(),
            });
        }
        if ours.is_empty() {
            return Err("the suite needs one or more --ours".to_owned());
        }
        Ok(Run::Suite(Options {
            rounds,
            codebase: codebase.ok_or("a missing flag for this form")?,
            oracle,
            engine,
            summary,
            uncovered,
            theirs: theirs.ok_or("a missing flag for this form")?,
            ours,
        }))
    }
}

/// DIFF-14.1: writes the programs `count` of `seed` under `dir`, and the forms they used.
fn generate(generate: &Generate) -> Result<bool, String> {
    let mut forms: std::collections::BTreeMap<&'static str, usize> =
        std::collections::BTreeMap::new();
    for n in 0..generate.count {
        let program = source::program(generate.seed, n);
        let dir = generate.dir.join(n.to_string());
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        std::fs::write(dir.join("progs.src"), "progs.dat\nt.qc\n")
            .map_err(|e| format!("{}: {e}", dir.display()))?;
        std::fs::write(dir.join("t.qc"), &program.source)
            .map_err(|e| format!("{}: {e}", dir.display()))?;
        // DIFF-14.9: how many programs use each form, however often each uses it.
        for form in program.forms.keys() {
            let entry = forms.entry(form).or_insert(0);
            *entry = entry.saturating_add(1);
        }
    }
    let mut text = String::new();
    for form in FORMS {
        let uses = forms.get(form).copied().unwrap_or(0);
        text.push_str(&format!("{form}\t{uses}\n"));
    }
    let path = generate.dir.join("forms.tsv");
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    println!(
        "compiler-diff: wrote {} programs of seed {} under {}",
        generate.count,
        generate.seed,
        generate.dir.display()
    );
    Ok(true)
}

/// DIFF-14.9: compares every pair `file` lists, one per line as `name`, the oracle's progs and
/// ours separated by tabs; a pair passes when no call diverges. DIFF-14.5: a line may name a
/// fourth progs, the oracle with its known defects fixed, and then a pair whose oracle
/// diverges passes when the fixed oracle agrees with ours, as an oracle defect proved; it
/// fails when the fixed oracle diverges from ours, whatever the oracle did.
fn pairs(
    file: &PathBuf,
    calls: &Calls,
    (oracle, engine): (Oracle, EngineProfile),
    codebase: &str,
) -> Result<bool, String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let mut all_ok = true;
    let mut compared = 0_usize;
    let mut proved = 0_usize;
    let quiet = |_: &str, _: usize, _: usize| {};
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let mut fields = line.split('\t');
        let (Some(name), Some(theirs), Some(ours)) = (fields.next(), fields.next(), fields.next())
        else {
            return Err(format!(
                "{}: a line is not name, theirs, ours: {line:?}",
                file.display()
            ));
        };
        let fixed = fields.next();
        let read = |path: &str| std::fs::read(path).map_err(|e| format!("{path}: {e}"));
        let (theirs, ours) = (read(theirs)?, read(ours)?);
        let run = |oracle_progs: &[u8]| match calls {
            Calls::Every(rounds) => run_codebase(
                codebase,
                oracle,
                oracle_progs,
                &ours,
                *rounds,
                engine,
                &quiet,
            ),
            Calls::Named(_) => {
                let functions = name.split(',').map(|f| f.as_bytes().to_vec()).collect();
                run_functions(
                    codebase,
                    oracle,
                    oracle_progs,
                    &ours,
                    FUNCTION_ROUNDS,
                    engine,
                    &functions,
                )
            }
        };
        let report = run(&theirs).map_err(|e| format!("{name}: {e}"))?;
        compared = compared.saturating_add(1);
        let fixed_report = fixed
            .map(|path| run(&read(path)?).map_err(|e| format!("{name}, the fixed oracle: {e}")))
            .transpose()?;
        if let Calls::Named(executed) = calls {
            let text = executed_by_function(&theirs, &report.executed)
                .map_err(|e| format!("{name}: {e}"))?;
            let mut out = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(executed)
                .map_err(|e| format!("{}: {e}", executed.display()))?;
            std::io::Write::write_all(&mut out, text.as_bytes())
                .map_err(|e| format!("{}: {e}", executed.display()))?;
        }
        let divergences = report.divergences();
        let fixed_divergences = fixed_report.as_ref().map(CodebaseReport::divergences);
        match Outcome::of(divergences, fixed_divergences) {
            Outcome::Agrees => println!(
                "ok    {name}: {} of {} statements ({:.1}%)",
                report.executed.len(),
                report.total_statements,
                report.coverage() * 100.0
            ),
            Outcome::OracleDefect => {
                proved = proved.saturating_add(1);
                println!(
                    "ok    {name}: {divergences} divergences, an oracle defect: the fixed \
                     oracle agrees (DIFF-14.5)"
                );
            }
            Outcome::FixTooWide => {
                all_ok = false;
                println!(
                    "FAIL  {name}: the fixed oracle diverges where the oracle agrees: a fix \
                     changed more than its defect"
                );
                if let Some(fixed_report) = &fixed_report {
                    print_divergences(fixed_report);
                }
            }
            Outcome::Diverges => {
                all_ok = false;
                println!("FAIL  {name}: {divergences} divergences");
                print_divergences(&report);
            }
        }
    }
    println!(
        "compiler-diff: {compared} pairs compared, {proved} oracle defects proved, {}",
        if all_ok {
            "no other divergence"
        } else {
            "some diverged"
        }
    );
    Ok(all_ok)
}

/// What a pair's comparisons say (DIFF-14.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    /// Ours agrees with the oracle, and with the fixed oracle when there is one.
    Agrees,
    /// Ours diverges from the oracle and agrees with the fixed one: an oracle defect proved.
    OracleDefect,
    /// The fixed oracle diverges from ours where the oracle agrees.
    FixTooWide,
    /// Ours diverges from the oracle, and from the fixed one when there is one.
    Diverges,
}

impl Outcome {
    /// The outcome of `divergences` from the oracle and, when a fixed oracle was compared,
    /// `fixed` from it.
    const fn of(divergences: usize, fixed: Option<usize>) -> Self {
        match (divergences, fixed) {
            (0, None | Some(0)) => Self::Agrees,
            (_, Some(0)) => Self::OracleDefect,
            (0, Some(_)) => Self::FixTooWide,
            (_, _) => Self::Diverges,
        }
    }
}

/// Prints the first [`PRINTED`] divergences of `report`, each with its scenario, function and
/// round.
fn print_divergences(report: &CodebaseReport) {
    let mut printed = 0_usize;
    for (scenario, scenario_report) in &report.scenarios {
        for divergence in scenario_report
            .divergences
            .iter()
            .take(PRINTED.saturating_sub(printed))
        {
            let function = divergence.function.as_deref().map_or_else(
                || "the setup".to_owned(),
                |f| String::from_utf8_lossy(f).into_owned(),
            );
            println!(
                "      {scenario}: {function}, round {}: {:?}",
                divergence.round, divergence.what
            );
            printed = printed.saturating_add(1);
        }
    }
}

fn run(request: Run) -> Result<bool, String> {
    let options = match request {
        Run::Suite(options) => options,
        Run::Generate(request) => return generate(&request),
        Run::SetupFunctions {
            progs,
            engine,
            codebase,
        } => {
            let bytes = std::fs::read(&progs).map_err(|e| format!("{}: {e}", progs.display()))?;
            let names = setup_functions(&codebase, &bytes, engine).map_err(|e| e.to_string())?;
            for name in names {
                println!("{}", String::from_utf8_lossy(&name));
            }
            return Ok(true);
        }
        Run::Pairs {
            file,
            calls,
            oracle,
            engine,
            codebase,
        } => return pairs(&file, &calls, (oracle, engine), &codebase),
    };
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
                // Progress on stderr, since a full run of one codebase can take an hour.
                let progress = |scenario: &str, done: usize, of: usize| {
                    eprintln!("      {scenario}: {done} of {of} functions called");
                };
                let report = run_codebase(
                    &options.codebase,
                    options.oracle,
                    &theirs,
                    &ours,
                    options.rounds,
                    options.engine,
                    &progress,
                )
                .map_err(|e| e.to_string())?;
                reports.push((ours.clone(), report.clone()));
                report
            }
        };
        let passed = print(&options.codebase, path, ours == theirs, &report);
        if let Some(summary) = &options.summary {
            add_row(summary, &options, path, &report)?;
        }
        if let Some(path) = &options.uncovered {
            let text = uncovered(&theirs, &report.executed).map_err(|e| e.to_string())?;
            std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        all_ok &= passed;
    }
    Ok(all_ok)
}

/// Prints one pair's report and returns whether it passed (DIFF-12.5).
fn print(codebase: &str, ours: &std::path::Path, same: bool, report: &CodebaseReport) -> bool {
    println!(
        "compiler-diff: {codebase}, the oracle's against {}",
        ours.display()
    );
    if same {
        println!("      the same bytes as the oracle's");
    }
    let mut printed = 0_usize;
    for (name, scenario) in &report.scenarios {
        println!(
            "      {name}: {} calls, {} divergences, {:.1}% of statements",
            scenario.calls,
            scenario.divergences.len(),
            scenario.coverage() * 100.0
        );
        if let Some(error) = &scenario.setup_error {
            println!("      {name}: both setups ended in {error:?}");
        }
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

/// Adds the row of `report`, our progs at `ours` against the oracle's, to the results table
/// at `summary` (DIFF-12.30).
fn add_row(
    summary: &std::path::Path,
    options: &Options,
    ours: &std::path::Path,
    report: &CodebaseReport,
) -> Result<(), String> {
    use std::io::Write as _;
    let name = |path: &std::path::Path| {
        path.file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    };
    let row = report.summary_row(&options.codebase, &name(&options.theirs), &name(ours));
    std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(summary)
        .and_then(|mut file| file.write_all(row.as_bytes()))
        .map_err(|e| format!("{}: {e}", summary.display()))
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::unreachable,
        reason = "test code: a failed unwrap or a wrong variant is a failed test"
    )]

    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use qc_builtins::EngineProfile;
    use qs_differential::calls::{CodebaseReport, FULL_ROUNDS, Oracle};

    use super::{Calls, Generate, Options, Outcome, Run, add_row, parse};

    // spec DIFF-12.30: each pair compared adds its row to the results table, after the rows
    // already there.
    #[test]
    fn each_pair_adds_a_row_to_the_table() {
        let dir = std::env::temp_dir().join(format!("compiler-diff-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let summary = dir.join("summary.md");
        std::fs::write(&summary, "head\n").unwrap();
        let options = Options {
            rounds: 1,
            codebase: "rerelease".to_owned(),
            oracle: Oracle::Fteqcc,
            engine: EngineProfile::Id,
            summary: Some(summary.clone()),
            uncovered: None,
            theirs: PathBuf::from("x/oracle.dat"),
            ours: Vec::new(),
        };
        let report = CodebaseReport {
            scenarios: Vec::new(),
            executed: BTreeSet::new(),
            total_statements: 4,
        };
        add_row(&summary, &options, &PathBuf::from("x/ours.dat"), &report).unwrap();
        add_row(&summary, &options, &PathBuf::from("x/again.dat"), &report).unwrap();
        assert_eq!(
            std::fs::read_to_string(&summary).unwrap(),
            "head\n| rerelease | oracle.dat | ours.dat | 0 | 0 of 4 (0.0%) | fail |\n\
             | rerelease | oracle.dat | again.dat | 0 | 0 of 4 (0.0%) | fail |\n"
        );
        assert!(add_row(&dir.join("no/such/dir.md"), &options, &summary, &report).is_err());
    }

    // spec DIFF-14.9: `--generate` names the directory, the seed and the count, `--pairs` the
    // file, with the rounds, the oracle and the engine of a suite run; either leaves the
    // suite's own arguments unrequired
    #[test]
    fn the_generate_and_pairs_forms_parse() {
        let args = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        match parse(&args(&["--generate", "out", "--seed", "7", "--count", "3"])).unwrap() {
            Run::Generate(Generate { dir, seed, count }) => {
                assert_eq!((dir, seed, count), (PathBuf::from("out"), 7, 3));
            }
            Run::Suite(_) | Run::Pairs { .. } | Run::SetupFunctions { .. } => {
                unreachable!("not a generate run")
            }
        }
        assert!(parse(&args(&["--generate", "out", "--seed", "7"])).is_err());
        match parse(&args(&[
            "--pairs", "p.tsv", "--mode", "full", "--oracle", "fteqcc",
        ]))
        .unwrap()
        {
            Run::Pairs {
                file,
                calls: Calls::Every(rounds),
                oracle,
                engine,
                codebase,
            } => {
                assert_eq!(codebase, "", "no --codebase: the shared scenarios");
                assert_eq!(file, PathBuf::from("p.tsv"));
                assert_eq!(rounds, FULL_ROUNDS);
                assert_eq!(oracle, Oracle::Fteqcc);
                assert_eq!(engine, EngineProfile::Id);
            }
            Run::Pairs { .. } | Run::Suite(_) | Run::Generate(_) | Run::SetupFunctions { .. } => {
                unreachable!("not a pairs run")
            }
        }
        // spec DIFF-15.9: a function-pairs run calls the functions each line names and adds the
        // statements it executed to the file named
        match parse(&args(&[
            "--function-pairs",
            "f.tsv",
            "--executed",
            "e.tsv",
            "--engine",
            "vkquake",
        ]))
        .unwrap()
        {
            Run::Pairs {
                file,
                calls: Calls::Named(executed),
                engine,
                ..
            } => {
                assert_eq!(file, PathBuf::from("f.tsv"));
                assert_eq!(executed, PathBuf::from("e.tsv"));
                assert_eq!(engine, EngineProfile::VkQuake);
            }
            Run::Pairs { .. } | Run::Suite(_) | Run::Generate(_) | Run::SetupFunctions { .. } => {
                unreachable!("not a function-pairs run")
            }
        }
        assert!(parse(&args(&["--function-pairs", "f.tsv"])).is_err());
        // spec DIFF-15.8: the setup functions of a progs are listed on their own
        match parse(&args(&[
            "--setup-functions",
            "w.dat",
            "--engine",
            "vkquake",
            "--codebase",
            "crmod7",
        ]))
        .unwrap()
        {
            Run::SetupFunctions {
                progs,
                engine,
                codebase,
            } => {
                assert_eq!(codebase, "crmod7");
                assert_eq!(progs, PathBuf::from("w.dat"));
                assert_eq!(engine, EngineProfile::VkQuake);
            }
            Run::Pairs { .. } | Run::Suite(_) | Run::Generate(_) => {
                unreachable!("not a setup-functions run")
            }
        }
    }

    // spec DIFF-14.5: a divergence from the oracle passes only when the fixed oracle agrees with
    // ours; the fixed oracle diverging fails, where the oracle agrees too, and with no fixed
    // oracle every divergence fails
    #[test]
    fn a_divergence_passes_only_when_the_fixed_oracle_agrees() {
        assert_eq!(Outcome::of(0, None), Outcome::Agrees);
        assert_eq!(Outcome::of(0, Some(0)), Outcome::Agrees);
        assert_eq!(Outcome::of(3, Some(0)), Outcome::OracleDefect);
        assert_eq!(Outcome::of(0, Some(2)), Outcome::FixTooWide);
        assert_eq!(Outcome::of(3, Some(2)), Outcome::Diverges);
        assert_eq!(Outcome::of(3, None), Outcome::Diverges);
    }
}
