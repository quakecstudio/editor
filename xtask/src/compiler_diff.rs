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
use crate::oracles::{self, Oracles, codegen, id101};

/// The vanilla corpus codebases, the ones id's `qcc` compiles (`corpus/manifest.toml`).
const VANILLA: [&str; 1] = ["id-1.01"];

/// Runs `cargo xtask compiler-diff [--fast | --full]`.
pub(crate) fn main(root: &Path, args: &[String]) -> Result<bool, String> {
    let mode = mode(args)?;
    let oracles = oracles::build(root)?;
    let assets = asset_root()?;
    let mut ok = true;
    for id in VANILLA {
        let codebase = assets.join("codebase").join(id);
        let work = root.join("target").join("compiler-diff").join(id);
        let progs = write_progs(&oracles, &codebase, &work)?;
        ok &= run_suite(root, mode, id, &progs)?;
    }
    Ok(ok)
}

/// The mode `args` name: `fast` for none or `--fast`, `full` for `--full`.
fn mode(args: &[String]) -> Result<&'static str, String> {
    match args {
        [] => Ok("fast"),
        [flag] if flag == "--fast" => Ok("fast"),
        [flag] if flag == "--full" => Ok("full"),
        _ => Err("usage: cargo xtask compiler-diff [--fast | --full]".to_owned()),
    }
}

/// Compiles `codebase` with `qcc` and with `qc-codegen` with `--opt fold` off and on, writes
/// the three progs in `work` and returns their paths, `qcc`'s first.
fn write_progs(oracles: &Oracles, codebase: &Path, work: &Path) -> Result<[PathBuf; 3], String> {
    fs::create_dir_all(work).map_err(|e| format!("{}: {e}", work.display()))?;
    let (theirs, _) = id101::compile(oracles, codebase)?;
    let paths = [
        work.join("qcc.dat"),
        work.join("ours.dat"),
        work.join("ours-fold.dat"),
    ];
    let [qcc, ours, folded] = &paths;
    write(qcc, &theirs)?;
    for (path, fold) in [(ours, false), (folded, true)] {
        let compiled = codegen::compile_codebase(codebase, Options { fold })?;
        write(path, &codegen::bytes(&compiled)?)?;
    }
    Ok(paths)
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Runs the `compiler-diff` binary on `qcc`'s progs against each of ours.
fn run_suite(root: &Path, mode: &str, id: &str, progs: &[PathBuf; 3]) -> Result<bool, String> {
    let [qcc, ours, folded] = progs;
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
            mode,
            "--codebase",
            id,
            "--theirs",
        ])
        .arg(qcc)
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
    use std::process::Command;

    use qc_codegen::Options;

    use super::{mode, write_progs};
    use crate::exec::{Fake, Reply};
    use crate::oracles::codegen;
    use crate::oracles::testing::{fake_oracles, fresh_dir};

    // spec DIFF-12.6: the codebase is compiled with qcc and with qc-codegen with `--opt fold`
    // off and on, qcc's progs first.
    #[test]
    fn a_codebase_is_compiled_three_ways() {
        let dir = fresh_dir("compiler-diff");
        let codebase = dir.join("id-1.01");
        fs::create_dir_all(&codebase).unwrap();
        fs::write(codebase.join("progs.src"), "../progs.dat\nworld.qc\n").unwrap();
        fs::write(
            codebase.join("world.qc"),
            "float x;\nvoid() main = { x = 1 + 2; };\n",
        )
        .unwrap();
        let fake = Fake::new(|command: &Command| {
            let src = command.get_current_dir().unwrap();
            Reply {
                files: vec![(src.parent().unwrap().join("progs.dat"), b"qcc's".to_vec())],
                ..Reply::default()
            }
        });
        let oracles = fake_oracles(&dir.join("work"), &fake);
        let [qcc, ours, folded] = write_progs(&oracles, &codebase, &dir.join("out")).unwrap();
        assert_eq!(fs::read(qcc).unwrap(), b"qcc's");
        let compiled = |fold| {
            codegen::bytes(&codegen::compile_codebase(&codebase, Options { fold }).unwrap())
                .unwrap()
        };
        assert_eq!(fs::read(&ours).unwrap(), compiled(false));
        assert_eq!(fs::read(&folded).unwrap(), compiled(true));
        assert_ne!(compiled(false), compiled(true), "1 + 2 folds");
        assert!(write_progs(&oracles, &dir.join("nowhere"), &dir.join("out2")).is_err());
    }

    // spec DIFF-12.6: `--fast` and no argument are the CI subset, `--full` the nightly run.
    #[test]
    fn the_mode_is_fast_unless_full_is_asked_for() {
        let args = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(mode(&args(&[])).unwrap(), "fast");
        assert_eq!(mode(&args(&["--fast"])).unwrap(), "fast");
        assert_eq!(mode(&args(&["--full"])).unwrap(), "full");
        assert!(mode(&args(&["--slow"])).is_err());
        assert!(mode(&args(&["--fast", "--full"])).is_err());
    }
}
