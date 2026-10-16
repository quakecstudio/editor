// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask oracles fuzz <scenario | init | progs | all> [--time <seconds>]`: builds the
//! libFuzzer targets of `oracles/vm/fuzz.c` (the VM oracle's readers of the scenario file, the
//! init file of vector mode and the progs) and runs each for a fixed time (TEST-5,
//! `oracles/README.md`, Fuzzing).
//!
//! Each target is the oracle compiled with the address and undefined-behaviour sanitizers and
//! linked with libFuzzer, so it needs a clang that has libFuzzer (`CC`; Apple's clang has none,
//! Homebrew's LLVM and Linux's `clang` with its runtime do). A target keeps its corpus in
//! `target/oracles/fuzz/<target>/corpus`, seeded on its first run from the inputs the scenario
//! tests and the VM vectors already write, and a crashing input in `artifacts/`; its output is in
//! `fuzz.log`, whose end is printed when it fails.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::compile::{Compiler, Instrument};
use super::scenarios::{Input, all_cases};
use super::{OUT, Sources, compile_vm, fetch, read_sources, vectors};

/// The targets: a name and the define that selects it in `oracles/vm/fuzz.c`.
pub(crate) const TARGETS: [(&str, &str); 3] = [
    ("scenario", "QS_FUZZ_SCENARIO"),
    ("init", "QS_FUZZ_INIT"),
    ("progs", "QS_FUZZ_PROGS"),
];

const DEFAULT_SECONDS: u64 = 60;
const USAGE: &str =
    "usage: cargo xtask oracles fuzz <scenario | init | progs | all> [--time <seconds>]";

/// The lines of a failed target's log that are printed.
const LOG_TAIL: usize = 60;

#[derive(Debug, PartialEq, Eq)]
struct Options {
    targets: Vec<(&'static str, &'static str)>,
    seconds: u64,
}

pub(crate) fn main(root: &Path, args: &[&str]) -> Result<bool, String> {
    let options = parse_args(args)?;
    let sources = read_sources(root)?;
    let cache = root.join(OUT).join("src");
    fetch(&sources, &cache)?;
    let mut ok = true;
    for (name, define) in options.targets {
        let dir = root.join(OUT).join("fuzz").join(name);
        let binary = build(root, &sources, &cache, &dir, define)?;
        let corpus = dir.join("corpus");
        let seeded = seed(&corpus, &seeds(root, name)?)?;
        println!("oracles: fuzzing {name} for {}s ({seeded} seeds)", options.seconds);
        let passed = run(&binary, &dir, options.seconds)?;
        println!(
            "oracles: fuzz target {name} {}",
            if passed { "passed" } else { "failed" }
        );
        ok &= passed;
    }
    Ok(ok)
}

fn parse_args(args: &[&str]) -> Result<Options, String> {
    let mut targets = Vec::new();
    let mut seconds = DEFAULT_SECONDS;
    let mut words = args.iter();
    while let Some(word) = words.next() {
        match *word {
            "--time" => {
                seconds = words
                    .next()
                    .and_then(|text| text.parse::<u64>().ok())
                    .filter(|seconds| *seconds > 0)
                    .ok_or_else(|| format!("--time takes a number of seconds\n{USAGE}"))?;
            }
            "all" => targets.extend(TARGETS),
            name => {
                let target = TARGETS
                    .into_iter()
                    .find(|(target, _)| *target == name)
                    .ok_or_else(|| format!("no fuzz target {name:?}\n{USAGE}"))?;
                targets.push(target);
            }
        }
    }
    if targets.is_empty() {
        return Err(USAGE.to_owned());
    }
    Ok(Options { targets, seconds })
}

/// Builds the target that `define` selects in `dir`; returns its executable.
fn build(
    root: &Path,
    sources: &Sources,
    cache: &Path,
    dir: &Path,
    define: &str,
) -> Result<PathBuf, String> {
    let objects = dir.join("build");
    fs::create_dir_all(&objects).map_err(|e| format!("{}: {e}", objects.display()))?;
    let compiler = Compiler::find(&objects, Instrument::Fuzz)?;
    let defines = ["QS_ENGINE", define];
    let extra = [(root.join("oracles/vm/fuzz.c"), &defines[..])];
    let objects = compile_vm(root, sources, cache, &objects, &compiler, &extra)?;
    let binary = dir.join(Compiler::executable("fuzz"));
    compiler
        .link(&objects, &binary)
        .map_err(|e| format!("{e}\n(fuzzing needs a clang with libFuzzer; set CC)"))?;
    Ok(binary)
}

/// The inputs a target's corpus starts from: the scenario files, init files and progs the
/// scenario tests write, and for `init` the init files of the VM vectors.
pub(crate) fn seeds(root: &Path, target: &str) -> Result<Vec<Vec<u8>>, String> {
    let mut seeds = Vec::new();
    for case in all_cases() {
        match (target, &case.input) {
            ("scenario", Input::Scenario { text, .. } | Input::Relative(text)) => {
                seeds.push(text.clone());
            }
            ("init", Input::Vector { init }) => seeds.push(init.as_bytes().to_vec()),
            ("progs", _) => seeds.push(match &case.progs {
                Some(bytes) => bytes.clone(),
                None => case.program.bytes()?,
            }),
            _ => {}
        }
    }
    if target == "init" {
        seeds.extend(vectors::init_texts(root)?.into_iter().map(String::into_bytes));
    }
    seeds.sort();
    seeds.dedup();
    Ok(seeds)
}

/// Writes `seeds` into an empty or missing `corpus`; a corpus that already holds inputs is
/// kept, so that each run goes on from the last. Returns how many seeds were written.
fn seed(corpus: &Path, seeds: &[Vec<u8>]) -> Result<usize, String> {
    fs::create_dir_all(corpus).map_err(|e| format!("{}: {e}", corpus.display()))?;
    let mut entries = fs::read_dir(corpus).map_err(|e| format!("{}: {e}", corpus.display()))?;
    if entries.next().is_some() {
        return Ok(0);
    }
    for (index, bytes) in seeds.iter().enumerate() {
        let path = corpus.join(format!("seed-{index:04}"));
        fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(seeds.len())
}

/// libFuzzer's arguments for a run of `seconds` on the corpus in `dir`.
fn run_args(dir: &Path, seconds: u64) -> Vec<String> {
    let artifacts = dir.join("artifacts");
    vec![
        dir.join("corpus").display().to_string(),
        format!("-max_total_time={seconds}"),
        format!("-artifact_prefix={}/", artifacts.display()),
        "-print_final_stats=1".to_owned(),
    ]
}

/// Runs the target, its output into `fuzz.log`; prints the log's end when it fails.
fn run(binary: &Path, dir: &Path, seconds: u64) -> Result<bool, String> {
    let artifacts = dir.join("artifacts");
    fs::create_dir_all(&artifacts).map_err(|e| format!("{}: {e}", artifacts.display()))?;
    let log_path = dir.join("fuzz.log");
    let log = fs::File::create(&log_path).map_err(|e| format!("{}: {e}", log_path.display()))?;
    let status = Command::new(binary)
        .args(run_args(dir, seconds))
        .stdout(Stdio::null())
        .stderr(log)
        .status()
        .map_err(|e| format!("cannot run {}: {e}", binary.display()))?;
    if status.success() {
        return Ok(true);
    }
    let text = fs::read_to_string(&log_path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    for line in lines.iter().skip(lines.len().saturating_sub(LOG_TAIL)) {
        println!("{line}");
    }
    println!(
        "oracles: {} failed ({status}); the input that failed is in {}",
        binary.display(),
        artifacts.display()
    );
    Ok(false)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use super::*;

    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf()
    }

    #[test]
    fn the_command_line_names_targets_and_a_time() {
        assert_eq!(
            parse_args(&["all"]).unwrap(),
            Options {
                targets: TARGETS.to_vec(),
                seconds: DEFAULT_SECONDS
            }
        );
        let one = parse_args(&["init", "--time", "600"]).unwrap();
        assert_eq!(one.targets, [("init", "QS_FUZZ_INIT")]);
        assert_eq!(one.seconds, 600);
        for bad in [
            &[][..],
            &["nothing"],
            &["init", "--time"],
            &["init", "--time", "0"],
            &["init", "--time", "x"],
        ] {
            assert!(parse_args(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn every_target_is_one_fuzz_c_selects() {
        let source = fs::read_to_string(root().join("oracles/vm/fuzz.c")).unwrap();
        for (_, define) in TARGETS {
            assert!(source.contains(&format!("defined({define})")), "{define}");
        }
    }

    #[test]
    fn each_target_has_seeds_of_its_own_kind() {
        let scenario = seeds(&root(), "scenario").unwrap();
        assert!(scenario.len() > 50, "{}", scenario.len());
        assert!(scenario.iter().any(|seed| seed.starts_with(b"progs progs.dat\n")));
        let init = seeds(&root(), "init").unwrap();
        assert!(init.len() > 200, "{}", init.len());
        assert!(init.iter().all(|seed| seed.starts_with(b"edicts ")), "every init file says edicts first");
        let progs = seeds(&root(), "progs").unwrap();
        assert!(progs.iter().any(|seed| seed.len() < 60), "a short progs is a seed");
        assert!(progs.iter().any(|seed| qc_progs::Progs::read(seed).is_ok()));
        assert!(seeds(&root(), "other").unwrap().is_empty());
    }

    #[test]
    fn a_corpus_is_seeded_once() {
        let corpus = root().join("target/xtask-tests/oracle-fuzz-corpus");
        if corpus.exists() {
            fs::remove_dir_all(&corpus).unwrap();
        }
        let seeds = [b"a".to_vec(), b"b".to_vec()];
        assert_eq!(seed(&corpus, &seeds).unwrap(), 2);
        assert_eq!(fs::read(corpus.join("seed-0001")).unwrap(), b"b");
        assert_eq!(seed(&corpus, &seeds).unwrap(), 0, "a corpus that has inputs is kept");
    }

    #[test]
    fn a_run_is_timed_and_keeps_its_artifacts_beside_its_corpus() {
        let dir = Path::new("t");
        let args = run_args(dir, 30);
        assert_eq!(args.first().unwrap(), &dir.join("corpus").display().to_string());
        assert!(args.contains(&"-max_total_time=30".to_owned()));
        assert!(args.iter().any(|arg| arg.starts_with("-artifact_prefix=") && arg.ends_with('/')));
    }
}
