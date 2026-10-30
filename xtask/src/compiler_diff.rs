// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask compiler-diff`: the compiler differential suite against id's `qcc` (P2-09,
//! COMP-4, COMP-7, `docs/spec/differential.md` DIFF-12).
//!
//! For each vanilla corpus codebase, this compiles it with the oracle `qcc` and with
//! `qc-codegen`, once with `--opt fold` off and once on (QC-27.20), writes the three progs under
//! `target/compiler-diff/<id>/`, and runs the `compiler-diff` binary of `qs-differential`,
//! which calls every function of `qcc`'s progs and of each of ours in `qc-vm` and compares
//! them (DIFF-12). It runs that binary with `cargo run`, as `cargo xtask differential` does,
//! because `qs-differential` is a `tests`-layer crate nothing may depend on (`layers.toml`).
//!
//! - `cargo xtask compiler-diff` and `--fast`: the CI subset, fewer rounds of calls.
//! - `cargo xtask compiler-diff --full`: the nightly run, more rounds.
//!
//! Both fail on any divergence and on a codebase whose `qcc` progs has less than 90% of its
//! statements executed (DIFF-12.5). The codebases come from the asset cache
//! (`$QS_TEST_ASSETS/codebase/<id>`, written by `cargo xtask corpus fetch`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use qc_codegen::Options;

use crate::differential::asset_root;
use crate::oracles::codegen::Profile;
use crate::oracles::{self, Oracles, codegen, id101, rerelease};

/// The vanilla corpus codebases, the ones id's `qcc` compiles (`corpus/manifest.toml`).
const VANILLA: [&str; 1] = ["id-1.01"];

/// The rerelease codebases, the ones the pinned FTEQCC compiles (`oracles/rerelease.rs`).
const RERELEASE: [&str; 6] = [
    "rerelease",
    "rerelease-hipnotic",
    "rerelease-rogue",
    "rerelease-mg1",
    "rerelease-ctf",
    "rerelease-mg3",
];

const USAGE: &str = "usage: cargo xtask compiler-diff [--fast | --full] [<codebase>...]";

/// Runs `cargo xtask compiler-diff [--fast | --full] [<codebase>...]`.
pub(crate) fn main(root: &Path, args: &[String]) -> Result<bool, String> {
    let (mode, selected) = parse(args)?;
    let oracles = oracles::build(root)?;
    let codebases = asset_root()?.join("codebase");
    let out = root.join("target").join("compiler-diff");
    let summary = out.join("summary.md");
    fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    write(&summary, SUMMARY_HEADER.as_bytes())?;
    let mut ok = true;
    for id in selected {
        let work = out.join(id);
        let progs = write_progs(&oracles, &codebases, id, &work)?;
        ok &= run_suite(root, mode, id, &progs, &summary)?;
    }
    Ok(ok)
}

/// The head of the results table, `target/compiler-diff/summary.md`; the binary adds a row
/// for each pair it compares (DIFF-12.8).
const SUMMARY_HEADER: &str = "| Codebase | Oracle | Ours | Divergences | Statements | Result |\n\
                              | --- | --- | --- | ---: | ---: | --- |\n";

/// How many rounds of calls the binary makes (DIFF-12.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// The CI subset, 32 rounds.
    Fast,
    /// The nightly run, 128 rounds.
    Full,
}

impl Mode {
    /// The codebases this mode compares: id 1.01 for the CI subset, and the rerelease too for
    /// the nightly run (DIFF-12.6, DIFF-12.8).
    fn codebases(self) -> Vec<&'static str> {
        match self {
            Self::Fast => VANILLA.to_vec(),
            Self::Full => VANILLA.into_iter().chain(RERELEASE).collect(),
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
/// to compare: those named after it, each one the mode compares, or with none every one.
fn parse(args: &[String]) -> Result<(Mode, Vec<&'static str>), String> {
    let (mode, named) = match args.split_first() {
        Some((flag, rest)) if flag == "--fast" => (Mode::Fast, rest),
        Some((flag, rest)) if flag == "--full" => (Mode::Full, rest),
        _ => (Mode::Fast, args),
    };
    let all = mode.codebases();
    if named.is_empty() {
        return Ok((mode, all));
    }
    let mut selected = Vec::new();
    for name in named {
        let id = all
            .iter()
            .find(|id| **id == name.as_str())
            .ok_or_else(|| format!("{name} is not a codebase this mode compares\n{USAGE}"))?;
        selected.push(*id);
    }
    Ok((mode, selected))
}

/// Compiles the codebase `id`, in `codebases/<id>`, with its oracle compiler (`qcc` for id
/// 1.01, the pinned FTEQCC for the rerelease) and with `qc-codegen` in the same dialect with
/// `--opt fold` off and on, writes the three progs in `work` and returns their paths, the
/// oracle's first.
fn write_progs(
    oracles: &Oracles,
    codebases: &Path,
    id: &str,
    work: &Path,
) -> Result<[PathBuf; 3], String> {
    fs::create_dir_all(work).map_err(|e| format!("{}: {e}", work.display()))?;
    let codebase = codebases.join(id);
    let (theirs, profile) = if rerelease::compiles(id) {
        let (dir, _) = rerelease::build(oracles, codebases, id)?;
        let path = dir.join("progs.dat");
        let theirs = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let autoproto = rerelease::autoprototypes(id);
        (theirs, Profile::Fteqcc { autoproto })
    } else {
        (id101::compile(oracles, &codebase)?.0, Profile::Vanilla)
    };
    let paths = [
        work.join("oracle.dat"),
        work.join("ours.dat"),
        work.join("ours-fold.dat"),
    ];
    let [oracle, ours, folded] = &paths;
    write(oracle, &theirs)?;
    for (path, fold) in [(ours, false), (folded, true)] {
        let compiled = codegen::compile_codebase(&codebase, profile, Options { fold })?;
        write(path, &codegen::bytes(&compiled)?)?;
    }
    Ok(paths)
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Runs the `compiler-diff` binary on the oracle's progs against each of ours, adding its rows
/// to `summary`.
fn run_suite(
    root: &Path,
    mode: Mode,
    id: &str,
    progs: &[PathBuf; 3],
    summary: &Path,
) -> Result<bool, String> {
    let [oracle, ours, folded] = progs;
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
            if rerelease::compiles(id) {
                "fteqcc"
            } else {
                "qcc"
            },
            "--summary",
        ])
        .arg(summary)
        .arg("--theirs")
        .arg(oracle)
        .arg("--ours")
        .arg(ours)
        .arg("--ours")
        .arg(folded)
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

    use qc_codegen::Options;

    use super::{Mode, RERELEASE, VANILLA, parse, write_progs};
    use crate::exec::{Fake, Reply};
    use crate::oracles::codegen::{self, Profile};
    use crate::oracles::rerelease;
    use crate::oracles::testing::{fake_oracles, fresh_dir};

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

    fn compiled(codebase: &Path, profile: Profile, fold: bool) -> Vec<u8> {
        codegen::bytes(&codegen::compile_codebase(codebase, profile, Options { fold }).unwrap())
            .unwrap()
    }

    // spec DIFF-12.6: the codebase is compiled with qcc and with qc-codegen with `--opt fold`
    // off and on, qcc's progs first.
    #[test]
    fn a_codebase_is_compiled_three_ways() {
        let dir = fresh_dir("compiler-diff");
        let codebases = dir.join("codebases");
        let id101 = codebase(
            &codebases,
            "id-1.01",
            "float x;\nvoid() main = { x = 1 + 2; };\n",
        );
        let fake = oracle();
        let oracles = fake_oracles(&dir.join("work"), &fake);
        let [qcc, ours, folded] =
            write_progs(&oracles, &codebases, "id-1.01", &dir.join("out")).unwrap();
        assert_eq!(fs::read(qcc).unwrap(), b"oracle's");
        assert_eq!(
            fs::read(&ours).unwrap(),
            compiled(&id101, Profile::Vanilla, false)
        );
        assert_eq!(
            fs::read(&folded).unwrap(),
            compiled(&id101, Profile::Vanilla, true)
        );
        assert_ne!(
            compiled(&id101, Profile::Vanilla, false),
            compiled(&id101, Profile::Vanilla, true),
            "1 + 2 folds"
        );
        assert!(write_progs(&oracles, &codebases, "nowhere", &dir.join("out2")).is_err());
    }

    // spec DIFF-12.8: a rerelease codebase is compiled with the pinned FTEQCC, the oracle, and
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
        let [fteqcc, ours, folded] =
            write_progs(&oracles, &codebases, "rerelease-mg3", &dir.join("out")).unwrap();
        assert_eq!(fs::read(fteqcc).unwrap(), b"oracle's");
        let fte = Profile::Fteqcc { autoproto: true };
        assert_eq!(fs::read(ours).unwrap(), compiled(&mg3, fte, false));
        assert_eq!(fs::read(folded).unwrap(), compiled(&mg3, fte, true));
        let without = Profile::Fteqcc { autoproto: false };
        assert!(codegen::compile_codebase(&mg3, without, Options::default()).is_err());
        assert!(codegen::compile_codebase(&mg3, Profile::Vanilla, Options::default()).is_err());

        let rerelease = codebase(
            &codebases,
            "rerelease",
            "float x;\nvoid() g = { x += 1; };\n",
        );
        write_progs(&oracles, &codebases, "rerelease", &dir.join("out-rr")).unwrap();
        assert_eq!(
            fs::read(dir.join("out-rr").join("ours.dat")).unwrap(),
            compiled(&rerelease, without, false)
        );
    }

    // spec DIFF-12.6: `--fast` and no argument are the CI subset, `--full` the nightly run;
    // codebases named after the mode are the ones compared, each one the mode compares.
    #[test]
    fn the_mode_is_fast_unless_full_is_asked_for() {
        let args = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(parse(&args(&[])).unwrap(), (Mode::Fast, vec!["id-1.01"]));
        assert_eq!(parse(&args(&["--fast"])).unwrap().0, Mode::Fast);
        assert_eq!(parse(&args(&["--full"])).unwrap().0, Mode::Full);
        assert_eq!(Mode::Fast.arg(), "fast");
        assert_eq!(Mode::Full.arg(), "full");
        assert_eq!(
            parse(&args(&["--full", "rerelease-mg3", "id-1.01"])).unwrap(),
            (Mode::Full, vec!["rerelease-mg3", "id-1.01"])
        );
        assert_eq!(
            parse(&args(&["id-1.01"])).unwrap(),
            (Mode::Fast, vec!["id-1.01"])
        );
        assert!(parse(&args(&["--fast", "rerelease"])).is_err());
        assert!(parse(&args(&["--slow"])).is_err());
        assert!(parse(&args(&["--fast", "--full"])).is_err());
    }

    // spec DIFF-12.8: the CI subset compares id 1.01; the nightly run adds the six rerelease
    // codebases, each one the FTEQCC oracle compiles.
    #[test]
    fn the_nightly_run_adds_the_rerelease() {
        assert_eq!(Mode::Fast.codebases(), ["id-1.01"]);
        let full = Mode::Full.codebases();
        assert_eq!(full.len(), 7);
        assert_eq!(full.first(), Some(&"id-1.01"));
        assert_eq!(full.get(1..), Some(RERELEASE.as_slice()));
        assert!(RERELEASE.iter().all(|id| rerelease::compiles(id)));
        assert!(VANILLA.iter().all(|id| !rerelease::compiles(id)));
    }
}
