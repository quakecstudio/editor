// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask project-corpus --fteqcc <dir>...`: runs the pinned FTEQCC on each module of a
//! codebase and checks that the files it reports compiling and including, in order, match the
//! module's file graph from `qc-project` (P1-33, LANG-3, COMP-8).
//!
//! FTEQCC is the build `cargo xtask oracles build` makes (P1-26). It is run once per module,
//! from a copy of the codebase, with the module's `progs.src` as its `-srcfile`; it writes its
//! own output into the copy, which is scratch, so nothing is written into the corpus cache.
//! With its default verbosity FTEQCC prints one line per list entry and per `#includelist`
//! entry, and nothing for an `#include`:
//!
//! - `QCC_FindBestInclude` (`engine/qclib/qcc_pr_lex.c`, fte-team/fteqw at the commit
//!   `oracles/sources.toml` pins, the commit `docs/spec/project-file.md` cites) is called with
//!   `includetype == 2` for a list entry (from `QCC_ContinueCompile` in
//!   `engine/qclib/qccmain.c`), `true` for an `#includelist` entry (`QCC_PR_Precompiler`),
//!   and `false` for an `#include`.
//! - It prints, guarded by `if (includetype && verbose >= VERBOSE_PROGRESS)` with
//!   `VERBOSE_PROGRESS == 0`, the default: `compiling %s\n` for a list entry (`includetype ==
//!   2`), `including %s\n` for an `#includelist` entry, and nothing for an `#include`
//!   (`includetype == 0`, which fails the guard).
//!
//! So the files FTEQCC reports, in order, are exactly the module's graph entries whose
//! [`Via`] is [`Via::Listed`] or [`Via::IncludeList`]. FTEQCC may stop early, on a compile
//! error or a crash, so what it prints must be a prefix of that sequence, in order; a file it
//! reports out of the graph's order, or one the graph does not have, is a failure (the spec's
//! PRJ-6 test vectors record the prefixes the corpus produces).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use qc_project::{Module, Via, normalize};

use crate::exec::Exec;

/// The output FTEQCC prints before a list entry (`QCC_FindBestInclude`, `includetype == 2`).
const COMPILING: &str = "compiling ";
/// The output FTEQCC prints before an `#includelist` entry (`QCC_FindBestInclude`,
/// `includetype == true`).
const INCLUDING: &str = "including ";
/// The prefix FTEQCC prints on its prototyping pass, when autoprototyping is in force (a
/// `#pragma autoproto` or a codebase, such as Copper, that enables it): `prototyping %s` for a
/// list entry and `prototyping include %s` for an `#includelist` entry. That pass visits the
/// same files, in the same order, as the following compiling pass, so these lines are skipped
/// and only the compiling pass ([`COMPILING`] and [`INCLUDING`]) is compared, once.
const PROTOTYPING: &str = "prototyping ";

/// The graph entries FTEQCC reports, the [`Via::Listed`] and [`Via::IncludeList`] ones, each
/// as its normalized path, in graph order.
fn reported_entries(module: &Module) -> Vec<PathBuf> {
    module
        .graph
        .files
        .iter()
        .filter(|f| matches!(f.via, Via::Listed | Via::IncludeList))
        .map(|f| f.path.clone())
        .collect()
}

/// Each file FTEQCC said it compiled or included, in order, as the normalized path it names
/// resolved against `root`, the copy of the codebase directory it ran from.
///
/// FTEQCC prints `compiling %s` for a list entry and `including %s` for an `#includelist`
/// entry (`QCC_FindBestInclude`), each a path relative to the root `.src`'s directory, which
/// is under `root`. Lines of the prototyping pass ([`PROTOTYPING`]) are skipped, so a codebase
/// that autoprototypes is not counted twice. Both the graph and this resolve against the same
/// copied tree, so the paths compare.
fn reported_by_fteqcc(progress: &str, root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for line in progress.lines() {
        let line = line.trim_end_matches('\r');
        if line.starts_with(PROTOTYPING) {
            continue;
        }
        let name = line
            .strip_prefix(COMPILING)
            .or_else(|| line.strip_prefix(INCLUDING));
        if let Some(name) = name
            && !name.is_empty()
        {
            files.push(normalize(&root.join(name)));
        }
    }
    files
}

/// Whether `reported` is a prefix, in order, of `expected`: FTEQCC stopped at or before the
/// end of the graph's [`Via::Listed`] and [`Via::IncludeList`] entries and named no file out
/// of their order. Returns the first index where they differ, if any.
fn prefix_mismatch(reported: &[PathBuf], expected: &[PathBuf]) -> Option<usize> {
    if reported.len() > expected.len() {
        return Some(expected.len());
    }
    reported.iter().zip(expected).position(|(a, b)| a != b)
}

/// Copies the tree at `from` into `to`, which need not exist.
fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for entry in fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let Some(name) = path.file_name() else {
            continue;
        };
        if path.is_dir() {
            copy_tree(&path, &to.join(name))?;
        } else {
            fs::copy(&path, to.join(name)).map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    Ok(())
}

/// Runs FTEQCC on the module whose source, in the copied tree, is `src`, from the copy root
/// `root`, and returns what it wrote to stderr, where this build's `externs->Printf` sends its
/// progress. FTEQCC writes its `progs.dat` and `.lno` into the copy, which is scratch. No `-o`
/// is passed: FTEQCC's `-o` does `verbose--` (`QCC_PR_CommandLinePrecompilerOptions` in
/// `engine/qclib/qccmain.c`), which drops verbosity below `VERBOSE_PROGRESS` and silences the
/// `compiling`/`including` lines this check reads. A non-zero exit (a compile error or a crash)
/// is not an error here, because the graph is checked against whatever FTEQCC reported before
/// it stopped.
fn run_fteqcc(exec: &dyn Exec, fteqcc: &Path, root: &Path, src: &Path) -> Result<String, String> {
    let relative = src.strip_prefix(root).unwrap_or(src);
    let output = exec
        .output(
            Command::new(fteqcc)
                .arg("-srcfile")
                .arg(relative)
                .current_dir(root),
        )
        .map_err(|e| format!("cannot run fteqcc: {e}"))?;
    Ok(String::from_utf8_lossy(&output.stderr).into_owned())
}

/// Checks one module's graph against FTEQCC. `copy_root` is the copy of the codebase
/// directory FTEQCC runs from, `dir` the original codebase directory the module's graph was
/// built against. Returns whether it passed.
fn check_module(
    exec: &dyn Exec,
    fteqcc: &Path,
    copy_root: &Path,
    dir: &Path,
    module: &Module,
) -> Result<bool, String> {
    // The module's source in the copied tree, relative to the copy root, so FTEQCC resolves
    // and prints the graph's files against the same tree.
    let relative = module.src.strip_prefix(dir).unwrap_or(&module.src);
    let copy_src = normalize(&copy_root.join(relative));
    let reported = run_fteqcc(exec, fteqcc, copy_root, &copy_src)?;
    // FTEQCC prints each path relative to the root `.src`'s directory, which is a path under
    // the copy root; resolve it against the copy root to compare.
    let reported = reported_by_fteqcc(&reported, copy_root);
    // Rebase the graph's entries, built against the original tree, onto the copied tree.
    let expected: Vec<PathBuf> = reported_entries(module)
        .iter()
        .map(|p| normalize(&copy_root.join(p.strip_prefix(dir).unwrap_or(p))))
        .collect();
    match prefix_mismatch(&reported, &expected) {
        None => {
            let how = if reported.len() == expected.len() {
                "all".to_owned()
            } else {
                format!("a prefix of {}", expected.len())
            };
            println!(
                "ok    {}: fteqcc reported {} files ({how}), in the graph's order",
                module.src.display(),
                reported.len(),
            );
            Ok(true)
        }
        Some(at) => {
            let shown = |files: &[PathBuf]| {
                files
                    .get(at)
                    .map_or_else(|| "<none>".to_owned(), |p| p.display().to_string())
            };
            println!(
                "FAIL  {}: at file {at}, fteqcc reported {}, the graph has {} ({} reported, \
                 {} in the graph's list and includelist)",
                module.src.display(),
                shown(&reported),
                shown(&expected),
                reported.len(),
                expected.len(),
            );
            Ok(false)
        }
    }
}

/// Checks every module of `modules`, whose sources are in `dir`, against FTEQCC run in a copy
/// of `dir`. Returns whether every module passed.
pub(super) fn check_dir(
    exec: &dyn Exec,
    fteqcc: &Path,
    work: &Path,
    dir: &Path,
    modules: &[&Module],
) -> Result<bool, String> {
    let name = dir.file_name().map_or_else(
        || "codebase".to_owned(),
        |n| n.to_string_lossy().into_owned(),
    );
    let copy = work.join("src").join(&name);
    if copy.exists() {
        fs::remove_dir_all(&copy).map_err(|e| format!("{}: {e}", copy.display()))?;
    }
    let dir = normalize(&std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf()));
    copy_tree(&dir, &copy)?;
    let mut ok = true;
    for module in modules {
        ok &= check_module(exec, fteqcc, &copy, &dir, module)?;
    }
    Ok(ok)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "test code: a failed unwrap or index is a failed test"
    )]

    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use qc_project::{MemoryLoader, Module, ModuleKind};

    use super::{check_dir, prefix_mismatch, reported_by_fteqcc, reported_entries};
    use crate::exec::{Fake, Reply};

    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
    }

    fn fresh(name: &str) -> PathBuf {
        let dir = root().join("target/xtask-tests/project-fteqcc").join(name);
        if dir.exists() {
            fs::remove_dir_all(&dir).unwrap();
        }
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A codebase directory and a scratch work directory beside it, never inside it (as the
    /// real work directory is under `target/oracles`, outside the corpus cache).
    fn codebase_and_work(name: &str) -> (PathBuf, PathBuf) {
        let base = fresh(name);
        let dir = base.join("code");
        let work = base.join("work");
        fs::create_dir_all(&dir).unwrap();
        (dir, work)
    }

    /// Opens the codebase `dir` with `qc-project` and returns its modules.
    fn modules(dir: &Path) -> Vec<Module> {
        qc_project::open(dir).unwrap().modules
    }

    /// The lines FTEQCC prints for the files named, as `compiling`, with the given prefix the
    /// directory of each name resolved against.
    fn compiling(names: &[&str]) -> String {
        names
            .iter()
            .map(|n| format!("compiling {n}\n"))
            .collect::<String>()
    }

    /// A fake FTEQCC that prints `progress` to stderr (where the real build sends it) and
    /// writes nothing. Records each command line.
    fn fteqcc(progress: String) -> Fake<impl Fn(&Command) -> Reply> {
        Fake::new(move |_: &Command| Reply {
            stderr: progress.clone(),
            ..Reply::default()
        })
    }

    // spec PRJ-6.1: the graph entries FTEQCC reports are the Listed and IncludeList ones.
    #[test]
    fn the_reported_entries_are_the_list_and_includelist_files() {
        let mut loader = MemoryLoader::new();
        loader.insert("/m/progs.src", "../progs.dat\na.qc\nb.qc\n");
        loader.insert("/m/a.qc", "void() a = {};\n");
        loader.insert("/m/b.qc", "void() b = {};\n");
        let graph = qc_project::build_graph(
            &mut loader,
            Path::new("/m/progs.src"),
            &[],
            &[],
            qc_lexer::Dialect::Vanilla,
        );
        let module = Module {
            name: "m".to_owned(),
            kind: ModuleKind::Ssqc,
            src: PathBuf::from("/m/progs.src"),
            dialect: qc_project::Dialect::Vanilla,
            target: qc_project::Target::Quake,
            output: PathBuf::from("/m/progs.dat"),
            defines: Vec::new(),
            include_dirs: Vec::new(),
            graph,
        };
        let reported = reported_entries(&module);
        assert_eq!(
            reported,
            [PathBuf::from("/m/a.qc"), PathBuf::from("/m/b.qc")]
        );
    }

    // spec PRJ-6.1: FTEQCC's compiling/including lines, in order, resolved against the root;
    // prototyping lines (the autoproto pass) are skipped so files are not counted twice.
    #[test]
    fn fteqcc_lines_are_parsed_and_resolved() {
        let progress = "Source file: progs.src\noutputfile: progs.dat\n\
                        prototyping a.qc\nprototyping sub/c.qc\n\
                        compiling a.qc\r\nincluding sub/c.qc\n\
                        ignored line\ncompiling ../up.qc\n";
        let files = reported_by_fteqcc(progress, Path::new("/m"));
        assert_eq!(
            files,
            [
                PathBuf::from("/m/a.qc"),
                PathBuf::from("/m/sub/c.qc"),
                PathBuf::from("/up.qc"),
            ]
        );
        assert!(reported_by_fteqcc("compiling \n", Path::new("/m")).is_empty());
    }

    #[test]
    fn a_prefix_is_accepted_and_a_divergence_is_found() {
        let p = |s: &str| PathBuf::from(s);
        let all = [p("a"), p("b"), p("c")];
        assert_eq!(prefix_mismatch(&all, &all), None);
        assert_eq!(prefix_mismatch(&[p("a"), p("b")], &all), None);
        assert_eq!(prefix_mismatch(&[], &all), None);
        assert_eq!(prefix_mismatch(&[p("a"), p("c")], &all), Some(1));
        assert_eq!(prefix_mismatch(&all, &[p("a"), p("b")]), Some(2));
    }

    // spec PRJ-6.1: a module whose FTEQCC order matches the graph passes; a swap fails.
    #[test]
    fn a_matching_order_passes_and_a_swap_fails() {
        let (dir, work) = codebase_and_work("match");
        fs::write(dir.join("progs.src"), "../progs.dat\na.qc\nb.qc\n").unwrap();
        fs::write(dir.join("a.qc"), "void() a = {};\n").unwrap();
        fs::write(dir.join("b.qc"), "void() b = {};\n").unwrap();
        let modules = modules(&dir);
        let refs: Vec<&Module> = modules.iter().collect();

        let good = fteqcc(compiling(&["a.qc", "b.qc"]));
        assert!(check_dir(&good, Path::new("fteqcc"), &work, &dir, &refs).unwrap());
        {
            let seen = good.seen.borrow();
            assert_eq!(seen.len(), 1);
            assert!(seen[0].contains("-srcfile progs.src"), "{}", seen[0]);
            assert!(!seen[0].contains("-o "), "{}", seen[0]);
        }
        // A prefix (FTEQCC stopped after the first file) still passes.
        let prefix = fteqcc(compiling(&["a.qc"]));
        assert!(check_dir(&prefix, Path::new("fteqcc"), &work, &dir, &refs).unwrap());
        // Reversed order fails.
        let swapped = fteqcc(compiling(&["b.qc", "a.qc"]));
        assert!(!check_dir(&swapped, Path::new("fteqcc"), &work, &dir, &refs).unwrap());
        // A file the graph does not list fails.
        let extra = fteqcc(compiling(&["a.qc", "b.qc", "c.qc"]));
        assert!(!check_dir(&extra, Path::new("fteqcc"), &work, &dir, &refs).unwrap());
    }

    // spec PRJ-6.1: includelist entries are reported as `including`; includes are not reported.
    #[test]
    fn an_includelist_is_checked_and_an_include_is_not() {
        let (dir, work) = codebase_and_work("includelist");
        fs::write(
            dir.join("progs.src"),
            "#pragma progs_dat \"progs.dat\"\n#include \"base.qc\"\n#includelist\nlisted.qc\n\
             #endlist\n",
        )
        .unwrap();
        fs::write(dir.join("base.qc"), "void() base = {};\n").unwrap();
        fs::write(dir.join("listed.qc"), "void() listed = {};\n").unwrap();
        let modules = modules(&dir);
        let refs: Vec<&Module> = modules.iter().collect();
        // The graph has base.qc (Via::Include, not reported) and listed.qc (Via::IncludeList).
        let reported = reported_entries(&modules[0]);
        assert_eq!(reported.len(), 1);
        // FTEQCC reports only the includelist entry.
        let fake = fteqcc("including listed.qc\n".to_owned());
        assert!(check_dir(&fake, Path::new("fteqcc"), &work, &dir, &refs).unwrap());
    }

    #[test]
    fn a_fteqcc_that_cannot_run_is_an_error() {
        let (dir, work) = codebase_and_work("cannot-run");
        fs::write(dir.join("progs.src"), "../progs.dat\na.qc\n").unwrap();
        fs::write(dir.join("a.qc"), "void() a = {};\n").unwrap();
        let modules = modules(&dir);
        let refs: Vec<&Module> = modules.iter().collect();
        let cannot = Fake::new(|_: &Command| Reply {
            cannot_run: true,
            ..Reply::default()
        });
        assert!(
            check_dir(&cannot, Path::new("fteqcc"), &work, &dir, &refs)
                .unwrap_err()
                .contains("cannot run")
        );
    }
}
