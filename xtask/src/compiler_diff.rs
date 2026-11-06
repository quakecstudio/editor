// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask compiler-diff`: the compiler differential suite (P2-09, P2-10, COMP-4, COMP-7,
//! COMP-8, `docs/spec/differential.md` DIFF-12).
//!
//! For each codebase, this compiles it with its oracle compiler, id's `qcc` for id 1.01 and the
//! pinned FTEQCC for the rerelease, and with `qc-codegen` in the same dialect, writes the two
//! progs under `target/compiler-diff/<id>/`, and runs the `compiler-diff` binary of
//! `qs-differential`, which calls every function of the two progs in `qc-vm` and compares
//! them (DIFF-12, DIFF-12.9). It runs that binary with `cargo run`, as `cargo xtask
//! differential` does, because `qs-differential` is a `tests`-layer crate nothing may depend on
//! (`layers.toml`). Each pair adds a row to `target/compiler-diff/<id>/summary.md`
//! (DIFF-12.30).
//!
//! - `cargo xtask compiler-diff` and `--fast`: the CI subset, id 1.01, fewer rounds of calls.
//! - `cargo xtask compiler-diff --full <codebase>`: the nightly run's 128 rounds over one
//!   codebase, id 1.01, a rerelease codebase or a corpus mod, which the nightly workflow runs
//!   as one job per codebase; `--full` may also name a corpus mod not yet at the bar.
//! - `cargo xtask compiler-diff --generated <count> [--seed <n>]`: generated programs (DIFF-14).
//! - `cargo xtask compiler-diff --functions <codebase>...`: each function of a codebase the
//!   pinned FTEQCC compiles, cut into a program of its own and called alone (DIFF-15,
//!   `functions.rs`).
//!
//! Both fail on any divergence and on a codebase whose oracle progs has less than 90% of its
//! statements executed (DIFF-12.5). The codebases come from the asset cache
//! (`$QS_TEST_ASSETS/codebase/<id>`, written by `cargo xtask corpus fetch`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::differential::asset_root;
use crate::oracles::codegen::Profile;
use crate::oracles::{self, Oracles, codegen, id101, rerelease};
use crate::readiness::Shard;

mod functions;

/// The vanilla corpus codebases, the ones id's `qcc` compiles (`corpus/manifest.toml`).
const VANILLA: [&str; 1] = ["id-1.01"];

/// The six rerelease codebases the nightly run compares with the pinned FTEQCC's progs
/// (`oracles/rerelease.rs`, DIFF-12.30).
const RERELEASE: [&str; 6] = [
    "rerelease",
    "rerelease-hipnotic",
    "rerelease-rogue",
    "rerelease-mg1",
    "rerelease-mg3",
    "rerelease-ctf",
];

/// The corpus mods (`oracles/rerelease.rs`'s `MODS`) at DIFF-12.5's bar, which the nightly run
/// compares; a run may name the others until each joins (DIFF-12.18).
const MODS_AT_BAR: [&str; 5] = [
    "arcane-dimensions",
    "copper",
    "librequake",
    "qc-conduit",
    "swab",
];

/// The corpus mods written for an engine with extension builtins, which the mock runs under
/// vkQuake's rules (BUILTIN-18): crmod7 calls them while it sets up, and Arcane Dimensions
/// precaches more sounds than id's engine holds.
const VKQUAKE_MODS: [&str; 2] = ["arcane-dimensions", "crmod7"];

/// The binary's `--engine` for codebase `id`: vkQuake's for [`VKQUAKE_MODS`], id's for the
/// rest (DIFF-12.19).
fn engine(id: &str) -> &'static str {
    if VKQUAKE_MODS.contains(&id) {
        "vkquake"
    } else {
        "id"
    }
}

/// `cargo xtask compiler-diff [--fast [<codebase>...] | --full <codebase>]`, `cargo xtask
/// compiler-diff --generated <count> [--seed <n>]` or `cargo xtask compiler-diff --functions
/// <codebase>...`.
#[derive(clap::Parser, Debug)]
#[command(group(clap::ArgGroup::new("kind").args(["fast", "full", "generated", "functions"])))]
pub(crate) struct Args {
    /// The CI subset, 32 rounds; the default.
    #[arg(long)]
    fast: bool,
    /// The nightly run, 128 rounds over the one codebase named.
    #[arg(long)]
    full: bool,
    /// The generated-program differential over this many programs (DIFF-14).
    #[arg(long, value_name = "count", conflicts_with = "codebases")]
    generated: Option<u64>,
    /// The generated programs' seed, 1 unless given.
    #[arg(long, value_name = "n", requires = "generated")]
    seed: Option<u64>,
    /// Each function of each codebase run on its own (DIFF-15).
    #[arg(long)]
    functions: bool,
    /// With `--functions`, only every n-th group of functions from the k-th, written `k/n`;
    /// the coverage bar is left to `--merge` (DIFF-15.10).
    #[arg(long, value_name = "k/n", requires = "functions", value_parser = Shard::parse)]
    shard: Option<Shard>,
    /// With `--functions` and one codebase, the shards' `executed.tsv` files, whose
    /// statements together are held to the coverage bar (DIFF-15.10).
    #[arg(long, value_name = "executed.tsv", num_args = 1.., requires = "functions", conflicts_with = "shard")]
    merge: Vec<PathBuf>,
    /// The codebases to compare.
    codebases: Vec<String>,
}

/// Runs `cargo xtask compiler-diff`: the CI subset over id 1.01, or 128 rounds over the one
/// codebase named, which runs for up to an hour on its own, so that the codebases run one at a
/// time or in parallel, each in its own directory under `target/compiler-diff/<id>/` with its
/// own results table (DIFF-12.29); or `--generated <count> [--seed <n>]`, the
/// generated-program differential (DIFF-14); or `--functions <codebase>...`, each function of
/// each codebase run on its own (DIFF-15).
pub(crate) fn main(root: &Path, args: &Args) -> Result<bool, String> {
    if let Some(count) = args.generated {
        return generated(root, count, args.seed.unwrap_or(1));
    }
    if args.functions {
        if !args.merge.is_empty() {
            return functions::merge(root, &args.codebases, &args.merge);
        }
        return functions::run(root, &args.codebases, args.shard);
    }
    let (mode, selected) = select(args)?;
    let oracles = oracles::build(root)?;
    let codebases = asset_root()?.join("codebase");
    let out = root.join("target").join("compiler-diff");
    let mut ok = true;
    for id in selected {
        let work = out.join(id);
        let progs = write_progs(&oracles, &codebases, id, &work)?;
        let summary = work.join("summary.md");
        write(&summary, SUMMARY_HEADER.as_bytes())?;
        ok &= run_suite(root, mode, id, &progs, &summary)?;
    }
    Ok(ok)
}

/// How long one FTEQCC run of a generated program may take.
const FTEQCC_LIMIT: std::time::Duration = std::time::Duration::from_secs(20);

/// Runs the FTEQCC `fteqcc` on the `progs.src` in `dir`, as the grammar differential does
/// (DIFF-13.2); returns what it printed. Its progs is `dir/progs.dat` when it accepted.
fn fteqcc_in(oracles: &Oracles, fteqcc: &Path, dir: &Path) -> Result<String, String> {
    let mut command = Command::new(fteqcc);
    command
        .args(["-Tid", "-srcfile", "progs.src"])
        .current_dir(dir);
    let output = oracles
        .exec
        .output_within(&mut command, FTEQCC_LIMIT)
        .map_err(|e| format!("cannot run fteqcc: {e}"))?;
    Ok(output.map_or_else(
        || "fteqcc ran out of time".to_owned(),
        |output| {
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        },
    ))
}

/// The generated-program differential (DIFF-14): the `compiler-diff` binary writes `count`
/// programs of `seed` under `target/compiler-diff/generated/<seed>/`, each is compiled with
/// the pinned FTEQCC, the fixed FTEQCC (DIFF-14.12) and `qc-codegen` in the FTEQCC dialect, and the
/// binary compares every pair in one process. A program FTEQCC rejects is a
/// defect of the generator and fails the run; each divergence names the program's directory,
/// whose `t.qc` is the case to keep. The forms every program used are totalled in
/// `forms.tsv`, and the run prints any form DIFF-14.1 names that fewer than one program in a
/// hundred used.
fn generated(root: &Path, count: u64, seed: u64) -> Result<bool, String> {
    let oracles = oracles::build(root)?;
    let dir = root
        .join("target")
        .join("compiler-diff")
        .join("generated")
        .join(seed.to_string());
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let dir_text = dir.to_string_lossy().into_owned();
    if !run_binary(
        root,
        &[
            "--generate",
            &dir_text,
            "--seed",
            &seed.to_string(),
            "--count",
            &count.to_string(),
        ],
    )? {
        return Err("the compiler-diff binary could not write the programs".to_owned());
    }
    let mut pairs = String::new();
    let mut rejected = Vec::new();
    let mut fixed_rejected = Vec::new();
    let mut refused = Vec::new();
    for n in 0..count {
        let program = dir.join(n.to_string());
        // DIFF-14.12: the fixed FTEQCC first, its progs kept as `fixed.dat`, then the pinned one.
        let fixed = program.join("fixed.dat");
        let fixed_log = fteqcc_in(&oracles, &oracles.fteqcc_fixed, &program)?;
        let built = program.join("progs.dat");
        if built.is_file() {
            fs::rename(&built, &fixed).map_err(|e| format!("{}: {e}", fixed.display()))?;
        }
        let log = fteqcc_in(&oracles, &oracles.fteqcc, &program)?;
        let theirs = program.join("progs.dat");
        if !theirs.is_file() {
            rejected.push((n, log));
            continue;
        }
        // DIFF-14.9: a program the pinned FTEQCC compiles and the fixed one does not is a
        // defect of a fix, reported on its own.
        if !fixed.is_file() {
            fixed_rejected.push((n, fixed_log));
            continue;
        }
        let profile = Profile::Fteqcc { autoproto: false };
        // A program FTEQCC accepted and qc-codegen refuses is a divergence of its own.
        let compiled = match codegen::compile_codebase(&program, profile) {
            Ok(compiled) => compiled,
            Err(error) => {
                refused.push((n, error));
                continue;
            }
        };
        let ours = program.join("ours.dat");
        write(&ours, &codegen::bytes(&compiled)?)?;
        pairs.push_str(&format!(
            "generated-{seed}-{n}\t{}\t{}\t{}\n",
            theirs.display(),
            ours.display(),
            fixed.display()
        ));
    }
    let pairs_path = dir.join("pairs.tsv");
    write(&pairs_path, pairs.as_bytes())?;
    for (n, log) in &rejected {
        eprintln!(
            "compiler-diff: FTEQCC rejected program {n} ({}): {}",
            dir.join(n.to_string()).join("t.qc").display(),
            log.lines().find(|l| l.contains("error")).unwrap_or("")
        );
    }
    for (n, log) in &fixed_rejected {
        eprintln!(
            "FAIL  generated-{seed}-{n}: the fixed FTEQCC rejected what FTEQCC compiled ({}): {}",
            dir.join(n.to_string()).join("t.qc").display(),
            log.lines().find(|l| l.contains("error")).unwrap_or("")
        );
    }
    for (n, error) in &refused {
        eprintln!(
            "FAIL  generated-{seed}-{n}: qc-codegen refused what FTEQCC compiled ({}): {error}",
            dir.join(n.to_string()).join("t.qc").display()
        );
    }
    let compared = run_binary(
        root,
        &[
            "--pairs",
            &pairs_path.to_string_lossy(),
            "--mode",
            "fast",
            "--oracle",
            "fteqcc",
            "--engine",
            "id",
        ],
    )?;
    let forms = fs::read_to_string(dir.join("forms.tsv"))
        .map_err(|e| format!("{}: {e}", dir.join("forms.tsv").display()))?;
    let floor = count.checked_div(100).unwrap_or(0);
    let mut rare = Vec::new();
    for line in forms.lines() {
        if let Some((form, uses)) = line.split_once('\t')
            && uses.parse::<u64>().is_ok_and(|uses| uses < floor)
        {
            rare.push(format!("{form} ({uses})"));
        }
    }
    if !rare.is_empty() {
        eprintln!(
            "compiler-diff: forms used by fewer than one program in a hundred: {}",
            rare.join(", ")
        );
    }
    eprintln!(
        "compiler-diff: {count} programs of seed {seed}, {} rejected by FTEQCC, {} refused by \
         qc-codegen, {}",
        rejected.len(),
        refused.len(),
        if compared {
            "no divergence among the rest"
        } else {
            "divergences above"
        }
    );
    Ok(compared
        && rejected.is_empty()
        && fixed_rejected.is_empty()
        && refused.is_empty()
        && rare.is_empty())
}

/// Runs the `compiler-diff` binary with `args`, as [`run_suite`] does; true when it exits 0.
fn run_binary(root: &Path, args: &[&str]) -> Result<bool, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .current_dir(root)
        .args([
            "run",
            "--release",
            "--locked",
            "-p",
            "qs-differential",
            "--bin",
            "compiler-diff",
            "--",
        ])
        .args(args)
        .status()
        .map_err(|e| format!("cannot run the compiler differential: {e}"))?;
    Ok(status.success())
}

/// The head of a codebase's results table, `target/compiler-diff/<id>/summary.md`; the
/// binary adds a row for each pair it compares (DIFF-12.30).
const SUMMARY_HEADER: &str = "| Codebase | Oracle | Ours | Divergences | Statements | Result |\n\
                              | --- | --- | --- | ---: | ---: | --- |\n";

/// How many rounds of calls the binary makes (DIFF-12.29).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// The CI subset, 32 rounds.
    Fast,
    /// The nightly run, 128 rounds.
    Full,
}

impl Mode {
    /// The codebases this mode compares: id 1.01 for the CI subset, and the rerelease
    /// codebases at the bar too for the nightly run (DIFF-12.29, DIFF-12.30).
    fn codebases(self) -> Vec<&'static str> {
        match self {
            Self::Fast => VANILLA.to_vec(),
            Self::Full => VANILLA
                .into_iter()
                .chain(RERELEASE)
                .chain(MODS_AT_BAR)
                .collect(),
        }
    }

    /// The codebases a run of this mode may name: its own, and in the nightly run the corpus
    /// mods not yet at the bar.
    fn named(self) -> Vec<&'static str> {
        match self {
            Self::Fast => self.codebases(),
            Self::Full => self
                .codebases()
                .into_iter()
                .chain(
                    rerelease::MODS
                        .into_iter()
                        .filter(|id| !MODS_AT_BAR.contains(id)),
                )
                .collect(),
        }
    }

    /// The value of the binary's `--mode`.
    const fn arg(self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::Full => "full",
        }
    }
}

/// The mode `args` name, fast for none or `--fast` and full for `--full`, and the codebases
/// to compare: those named after it, each one the mode compares. A fast run with none named
/// compares its whole subset; a full run names exactly one codebase, each full run being
/// long enough to run on its own.
fn select(args: &Args) -> Result<(Mode, Vec<&'static str>), String> {
    let mode = if args.full { Mode::Full } else { Mode::Fast };
    let named = &args.codebases;
    if mode == Mode::Full && named.len() != 1 {
        let all = mode.named().join(", ");
        return Err(format!("--full takes one codebase: one of {all}"));
    }
    if named.is_empty() {
        return Ok((mode, mode.codebases()));
    }
    let named_ok = mode.named();
    let mut selected = Vec::new();
    for name in named {
        let id = named_ok
            .iter()
            .find(|id| **id == name.as_str())
            .ok_or_else(|| format!("{name} is not a codebase this mode compares"))?;
        selected.push(*id);
    }
    Ok((mode, selected))
}

/// Compiles the codebase `id`, in `codebases/<id>`, with its oracle compiler (`qcc` for id
/// 1.01, the pinned FTEQCC for the rerelease) and with `qc-codegen` in the same dialect, writes
/// the two progs in `work` and returns their paths, the oracle's first.
fn write_progs(
    oracles: &Oracles,
    codebases: &Path,
    id: &str,
    work: &Path,
) -> Result<[PathBuf; 2], String> {
    fs::create_dir_all(work).map_err(|e| format!("{}: {e}", work.display()))?;
    let codebase = codebases.join(id);
    let (theirs, profile) = if rerelease::MODS.contains(&id) {
        let path = rerelease::build_mod(oracles, codebases, id)?.join("progs.dat");
        let theirs = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        (theirs, Profile::Fteqcc { autoproto: false })
    } else if rerelease::compiles(id) {
        let (dir, _) = rerelease::build(oracles, codebases, id)?;
        let path = dir.join("progs.dat");
        let theirs = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let autoproto = rerelease::autoprototypes(id);
        (theirs, Profile::Fteqcc { autoproto })
    } else {
        (id101::compile(oracles, &codebase)?.0, Profile::Vanilla)
    };
    let paths = [work.join("oracle.dat"), work.join("ours.dat")];
    let [oracle, ours] = &paths;
    write(oracle, &theirs)?;
    let compiled = codegen::compile_codebase(&codebase, profile)?;
    write(ours, &codegen::bytes(&compiled)?)?;
    Ok(paths)
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Runs the `compiler-diff` binary on the oracle's progs against ours, adding its row to
/// `summary`.
fn run_suite(
    root: &Path,
    mode: Mode,
    id: &str,
    progs: &[PathBuf; 2],
    summary: &Path,
) -> Result<bool, String> {
    let [oracle, ours] = progs;
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .current_dir(root)
        .args([
            "run",
            "--release",
            "--locked",
            "-p",
            "qs-differential",
            "--bin",
            "compiler-diff",
            "--",
            "--mode",
            mode.arg(),
            "--codebase",
            id,
            "--oracle",
            if rerelease::compiles(id) || rerelease::MODS.contains(&id) {
                "fteqcc"
            } else {
                "qcc"
            },
            "--engine",
            engine(id),
            "--summary",
        ])
        .arg(summary)
        .arg("--theirs")
        .arg(oracle)
        .arg("--ours")
        .arg(ours)
        .status()
        .map_err(|e| format!("cannot run the compiler differential: {e}"))?;
    Ok(status.success())
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use std::fs;
    use std::path::Path;
    use std::process::Command;

    use super::{
        Args, MODS_AT_BAR, Mode, RERELEASE, VANILLA, VKQUAKE_MODS, engine, select, write_progs,
    };
    use crate::exec::{Fake, Reply};
    use crate::oracles::codegen::{self, Profile};
    use crate::oracles::rerelease;
    use crate::oracles::testing::{fake_oracles, fresh_dir, root};

    /// Writes a codebase `id` of one file, `world.qc`, under `codebases`.
    fn codebase(codebases: &Path, id: &str, source: &str) -> std::path::PathBuf {
        let dir = codebases.join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("progs.src"), "../progs.dat\nworld.qc\n").unwrap();
        fs::write(dir.join("world.qc"), source).unwrap();
        dir
    }

    /// An oracle compiler that writes `progs.dat` (and `progs.lno`) beside the directory it
    /// runs in, as both `qcc` and FTEQCC do.
    fn oracle() -> Fake<impl Fn(&Command) -> Reply + Sync> {
        Fake::new(|command: &Command| {
            let out = command.get_current_dir().unwrap().parent().unwrap();
            Reply {
                files: vec![
                    (out.join("progs.dat"), b"oracle's".to_vec()),
                    (out.join("progs.lno"), b"lno".to_vec()),
                ],
                ..Reply::default()
            }
        })
    }

    fn compiled(codebase: &Path, profile: Profile) -> Vec<u8> {
        codegen::bytes(&codegen::compile_codebase(codebase, profile).unwrap()).unwrap()
    }

    // spec DIFF-12.29: the codebase is compiled with qcc and with qc-codegen, qcc's progs first.
    #[test]
    fn a_codebase_is_compiled_both_ways() {
        let dir = fresh_dir("compiler-diff");
        let codebases = dir.join("codebases");
        let id101 = codebase(
            &codebases,
            "id-1.01",
            "float x;\nvoid() main = { x = 1 + 2; };\n",
        );
        let fake = oracle();
        let oracles = fake_oracles(&dir.join("work"), &fake);
        let [qcc, ours] = write_progs(&oracles, &codebases, "id-1.01", &dir.join("out")).unwrap();
        assert_eq!(fs::read(qcc).unwrap(), b"oracle's");
        assert_eq!(fs::read(&ours).unwrap(), compiled(&id101, Profile::Vanilla));
        assert!(write_progs(&oracles, &codebases, "nowhere", &dir.join("out2")).is_err());
    }

    // spec DIFF-12.30: a rerelease codebase is compiled with the pinned FTEQCC, the oracle, and
    // with qc-codegen in the FTEQCC dialect, with Dawn of the Machine's prototypes collected
    // first as `-Fautoproto` collects them.
    #[test]
    fn a_rerelease_codebase_is_compiled_in_the_fteqcc_dialect() {
        let dir = fresh_dir("compiler-diff-rerelease");
        let codebases = dir.join("codebases");
        // `for` and `+=` are FTEQCC's; `g` calls `h` before it is declared.
        let source = "float x;\nvoid() g = { h(); };\nvoid() h = { float i; \
                      for (i = 0; i < 2; i++) x += i; };\n";
        let mg3 = codebase(&codebases, "rerelease-mg3", source);
        let fake = oracle();
        let oracles = fake_oracles(&dir.join("work"), &fake);
        let [fteqcc, ours] =
            write_progs(&oracles, &codebases, "rerelease-mg3", &dir.join("out")).unwrap();
        assert_eq!(fs::read(fteqcc).unwrap(), b"oracle's");
        let fte = Profile::Fteqcc { autoproto: true };
        assert_eq!(fs::read(ours).unwrap(), compiled(&mg3, fte));
        let without = Profile::Fteqcc { autoproto: false };
        assert!(codegen::compile_codebase(&mg3, without).is_err());
        assert!(codegen::compile_codebase(&mg3, Profile::Vanilla).is_err());

        let rerelease = codebase(
            &codebases,
            "rerelease",
            "float x;\nvoid() g = { x += 1; };\n",
        );
        write_progs(&oracles, &codebases, "rerelease", &dir.join("out-rr")).unwrap();
        assert_eq!(
            fs::read(dir.join("out-rr").join("ours.dat")).unwrap(),
            compiled(&rerelease, without)
        );
    }

    /// The mode and codebases the command line `list` selects.
    fn parse(list: &[&str]) -> Result<(Mode, Vec<&'static str>), String> {
        crate::parse_cli::<Args, _>(list)
            .map_err(|e| e.to_string())
            .and_then(|args| select(&args))
    }

    /// The count and seed of a `--generated` command line `list`.
    fn generated_args(list: &[&str]) -> Result<(u64, u64), String> {
        let args = crate::parse_cli::<Args, _>(list).map_err(|e| e.to_string())?;
        let count = args.generated.ok_or("no --generated")?;
        Ok((count, args.seed.unwrap_or(1)))
    }

    // spec DIFF-12.29: `--fast` and no argument are the CI subset, `--full` the nightly run of
    // exactly one codebase, named; codebases named after the mode are the ones compared, each
    // one the mode compares.
    #[test]
    fn the_mode_is_fast_unless_full_is_asked_for() {
        assert_eq!(parse(&[]).unwrap(), (Mode::Fast, vec!["id-1.01"]));
        assert_eq!(parse(&["--fast"]).unwrap().0, Mode::Fast);
        assert!(parse(&["--full"]).is_err(), "a full run names a codebase");
        assert!(parse(&["--full", "rerelease-mg3", "id-1.01"]).is_err());
        assert_eq!(
            parse(&["--full", "rerelease-mg3"]).unwrap(),
            (Mode::Full, vec!["rerelease-mg3"])
        );
        assert_eq!(Mode::Fast.arg(), "fast");
        assert_eq!(Mode::Full.arg(), "full");
        assert_eq!(parse(&["id-1.01"]).unwrap(), (Mode::Fast, vec!["id-1.01"]));
        assert!(parse(&["--fast", "rerelease"]).is_err());
        assert!(parse(&["--slow"]).is_err());
        assert!(parse(&["--fast", "--full"]).is_err());
    }

    // spec DIFF-12.30: the CI subset compares id 1.01; the nightly run adds the rerelease
    // codebases at the bar, and a run may name any of the six, each one the FTEQCC oracle
    // compiles
    #[test]
    fn the_nightly_run_adds_the_rerelease() {
        assert_eq!(Mode::Fast.codebases(), ["id-1.01"]);
        let full = Mode::Full.codebases();
        assert_eq!(full.first(), Some(&"id-1.01"));
        assert_eq!(full.len(), 1 + RERELEASE.len() + MODS_AT_BAR.len());
        assert_eq!(RERELEASE.len(), 6, "the six rerelease codebases");
        for id in RERELEASE {
            assert_eq!(parse(&["--full", id]).unwrap(), (Mode::Full, vec![id]));
            assert!(parse(&["--fast", id]).is_err());
        }
        assert!(RERELEASE.iter().all(|id| rerelease::compiles(id)));
        assert!(VANILLA.iter().all(|id| !rerelease::compiles(id)));
    }

    // spec DIFF-12.18: the nightly run compares the corpus mods at the bar, and a run may name
    // any of them, each compiled by the FTEQCC oracle as a mod
    #[test]
    fn the_nightly_run_adds_the_mods_at_the_bar() {
        let full = Mode::Full.codebases();
        assert!(MODS_AT_BAR.iter().all(|id| full.contains(id)));
        assert!(MODS_AT_BAR.iter().all(|id| rerelease::MODS.contains(id)));
        let named = Mode::Full.named();
        for id in rerelease::MODS {
            assert_eq!(named.iter().filter(|n| **n == id).count(), 1, "{id}");
            assert!(!rerelease::compiles(id));
            assert_eq!(parse(&["--full", id]).unwrap(), (Mode::Full, vec![id]));
            assert!(parse(&["--fast", id]).is_err());
        }
        let pending: Vec<_> = rerelease::MODS
            .into_iter()
            .filter(|id| !MODS_AT_BAR.contains(id))
            .collect();
        assert!(pending.iter().all(|id| !full.contains(id)));
    }

    // spec DIFF-12.30, DIFF-12.18: the nightly workflow runs one job per codebase of the full
    // run's set, the matrix in .github/workflows/differential.yml naming exactly that set
    #[test]
    fn the_nightly_matrix_names_the_full_runs_codebases() {
        let workflow =
            fs::read_to_string(root().join(".github/workflows/differential.yml")).unwrap();
        let matrix: Vec<&str> = workflow
            .lines()
            .skip_while(|line| line.trim() != "codebase:")
            .skip(1)
            .take_while(|line| line.trim_start().starts_with("- "))
            .map(|line| line.trim().trim_start_matches("- "))
            .collect();
        assert_eq!(matrix, Mode::Full.codebases());
    }

    // spec DIFF-14.9: `--generated <count> [--seed <n>]` names how many programs and the seed,
    // 1 unless given
    #[test]
    fn the_generated_run_takes_a_count_and_a_seed() {
        assert_eq!(generated_args(&["--generated", "200"]).unwrap(), (200, 1));
        assert_eq!(
            generated_args(&["--generated", "10", "--seed", "4"]).unwrap(),
            (10, 4)
        );
        assert!(generated_args(&["--generated"]).is_err());
        assert!(generated_args(&["--generated", "x"]).is_err());
        assert!(generated_args(&["--generated", "10", "--seed"]).is_err());
        assert!(generated_args(&["--generated", "10", "--rounds", "4"]).is_err());
        assert!(generated_args(&["--generated", "10", "--seed", "4", "more"]).is_err());
    }

    // spec DIFF-14.3: the short run is a step of every pull request's Test Assets job and the
    // long one a job of the nightly workflow
    #[test]
    fn the_workflows_run_the_generated_differential() {
        let ci =
            fs::read_to_string(crate::workspace_root().join(".github/workflows/ci.yml")).unwrap();
        assert!(ci.contains("cargo xtask compiler-diff --generated 200 --seed 1"));
        let nightly =
            fs::read_to_string(crate::workspace_root().join(".github/workflows/differential.yml"))
                .unwrap();
        assert!(nightly.contains("cargo xtask compiler-diff --generated 10000 --seed 1"));
    }

    // spec DIFF-12.19: crmod7 and Arcane Dimensions run under vkQuake's profile, every other
    // codebase under id's
    #[test]
    fn the_extension_mods_run_under_vkquake() {
        assert_eq!(engine("crmod7"), "vkquake");
        assert_eq!(engine("arcane-dimensions"), "vkquake");
        for id in VANILLA.into_iter().chain(RERELEASE).chain(MODS_AT_BAR) {
            let expected = if VKQUAKE_MODS.contains(&id) {
                "vkquake"
            } else {
                "id"
            };
            assert_eq!(engine(id), expected, "{id}");
        }
    }
}
