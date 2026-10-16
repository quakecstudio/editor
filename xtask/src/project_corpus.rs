// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask project-corpus [--progs <progs.dat>] [--expect-warning <code>=<n>] <dir>...`:
//! opens each codebase as a
//! project with `qc-project` and checks every module's file graph (P1-23).
//!
//! A codebase with `quakec.toml` is opened through it. Otherwise every `.src` file that no
//! other `.src` brings in or registers is opened as a bare project. The check fails on any
//! error in a graph, and on a list-form `progs.src` with no directive lines whose order differs
//! from the list as `lex_corpus::progs_src_names` reads it. With `--progs`, the codebase must
//! have one module, and the source files of the functions in that `progs.dat`, in function
//! order, must appear in the module's graph in the same order: the program id's `qcc`
//! compiled from the same `progs.src` (`cargo xtask oracles id101`) is the oracle. With
//! `--files`, every file of every graph is printed, in order. With `--expect-warning`, exactly
//! `n` modules, over all the directories, must have a warning with that code; warnings are
//! otherwise printed and never fail the check.
//!
//! The codebases come from `cargo xtask corpus fetch <id>`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use qc_progs::Progs;
use qc_project::{Code, Form, Module, Project, Via, open};

use crate::lex_corpus::progs_src_names;

const USAGE: &str = "usage: cargo xtask project-corpus [--files] [--progs <progs.dat>] \
                     [--expect-warning <code>=<modules>] <dir>...";

/// Every `.src` file under `dir`, sorted.
fn src_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        let entries = fs::read_dir(&next).map_err(|e| format!("{}: {e}", next.display()))?;
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("src"))
            {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

/// The projects of a codebase: its `quakec.toml`, or one bare project per root `.src` file.
fn projects(dir: &Path) -> Result<Vec<Project>, String> {
    if dir.join(qc_project::PRJ_MANIFEST).is_file() {
        return Ok(vec![open(dir).map_err(|e| e.to_string())?]);
    }
    let src_root = |p: &Path| std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    let mut opened = Vec::new();
    let mut reached = BTreeSet::new();
    for src in src_files(dir)? {
        let project = open(&src).map_err(|e| e.to_string())?;
        let opened_src = qc_project::normalize(&src_root(&src));
        for module in &project.modules {
            if module.src != opened_src {
                reached.insert(module.src.clone());
            }
            for file in &module.graph.files {
                if file.via != Via::Root {
                    reached.insert(file.path.clone());
                }
            }
            reached.extend(module.graph.sourcefiles.iter().cloned());
        }
        opened.push((src, project));
    }
    Ok(opened
        .into_iter()
        .filter(|(src, _)| !reached.contains(&qc_project::normalize(&src_root(src))))
        .map(|(_, project)| project)
        .collect())
}

/// Whether a list-form source has a line that starts with `#`.
fn has_directives(src: &Path) -> bool {
    fs::read(src).is_ok_and(|text| {
        text.split(|b| *b == b'\n')
            .any(|line| line.trim_ascii_start().starts_with(b"#"))
    })
}

/// Checks a list-form module against the list as `progs_src_names` reads it.
fn check_list_order(module: &Module) -> Result<bool, String> {
    if module.graph.form != Some(Form::List) || has_directives(&module.src) {
        return Ok(true);
    }
    let dir = module.src.parent().unwrap_or_else(|| Path::new(""));
    let expected: Vec<PathBuf> = progs_src_names(&module.src)?
        .iter()
        .map(|name| qc_project::normalize(&dir.join(name)))
        .collect();
    let listed: Vec<PathBuf> = module
        .graph
        .files
        .iter()
        .filter(|f| f.via == Via::Listed)
        .map(|f| f.path.clone())
        .collect();
    if listed == expected {
        return Ok(true);
    }
    println!(
        "FAIL  {}: the graph lists {} files, the progs.src {}, or in another order",
        module.src.display(),
        listed.len(),
        expected.len()
    );
    Ok(false)
}

/// Checks that the source files of the functions in `progs`, in function order, appear in
/// the module's graph in that order.
fn check_progs_order(module: &Module, progs_path: &Path) -> Result<bool, String> {
    let bytes = fs::read(progs_path).map_err(|e| format!("{}: {e}", progs_path.display()))?;
    let progs = Progs::read(&bytes).map_err(|e| format!("{}: {e:?}", progs_path.display()))?;
    let mut order: Vec<String> = Vec::new();
    for function in progs.functions().iter() {
        let name = progs
            .string(function.s_file)
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .unwrap_or_default();
        if !name.is_empty() && order.last() != Some(&name) {
            order.push(name);
        }
    }
    let dir = module.src.parent().unwrap_or_else(|| Path::new(""));
    let graph: Vec<PathBuf> = module.graph.files.iter().map(|f| f.path.clone()).collect();
    let mut at = 0_usize;
    for name in &order {
        let path = qc_project::normalize(&dir.join(name));
        let Some(found) = graph
            .get(at..)
            .and_then(|rest| rest.iter().position(|p| *p == path))
        else {
            println!(
                "FAIL  {}: the functions of {name} come where the graph has no {name}",
                progs_path.display()
            );
            return Ok(false);
        };
        at = at.saturating_add(found);
    }
    println!(
        "ok    {}: the source files of its {} functions, {} in all, are in the graph's order",
        progs_path.display(),
        progs.functions().iter().count(),
        order.len()
    );
    Ok(true)
}

fn report(module: &Module) -> bool {
    let mut ok = true;
    for d in &module.graph.diagnostics {
        let label = if d.code.is_error() {
            ok = false;
            "FAIL"
        } else {
            "warn"
        };
        println!(
            "{label}  {}:{}: {} {}",
            d.file.display(),
            d.line,
            d.code.as_str(),
            d.message
        );
    }
    let form = match module.graph.form {
        Some(Form::List) => "list",
        Some(Form::Source) => "source",
        None => "unread",
    };
    let name = |i: Option<&qc_project::GraphFile>| {
        i.and_then(|f| f.path.file_name())
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    };
    println!(
        "ok    {}: module {} ({}, {form} form): {} files, {} to {}",
        module.src.display(),
        module.name,
        module.kind.as_str(),
        module.graph.files.len(),
        name(module.graph.files.first()),
        name(module.graph.files.last()),
    );
    ok
}

/// Parses `<code>=<modules>`, the code a warning.
fn parse_expected(arg: &str) -> Result<(Code, usize), String> {
    let bad = || format!("--expect-warning takes <warning code>=<modules>, not {arg}\n{USAGE}");
    let (code, count) = arg.split_once('=').ok_or_else(bad)?;
    let code = Code::ALL
        .into_iter()
        .find(|c| c.as_str() == code && !c.is_error())
        .ok_or_else(bad)?;
    Ok((code, count.parse().map_err(|_| bad())?))
}

/// Runs the check on the directories in `args`.
pub(crate) fn main(_root: &Path, args: &[String]) -> Result<bool, String> {
    let mut progs = None;
    let mut files = false;
    let mut expected: Option<(Code, usize)> = None;
    let mut warned = 0_usize;
    let mut dirs = Vec::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--progs" => progs = Some(PathBuf::from(rest.next().ok_or(USAGE)?)),
            "--files" => files = true,
            "--expect-warning" => expected = Some(parse_expected(rest.next().ok_or(USAGE)?)?),
            flag if flag.starts_with("--") => return Err(format!("unknown flag {flag}\n{USAGE}")),
            dir => dirs.push(PathBuf::from(dir)),
        }
    }
    if dirs.is_empty() || (progs.is_some() && dirs.len() != 1) {
        return Err(USAGE.to_owned());
    }
    let mut ok = true;
    for dir in &dirs {
        let projects = projects(dir)?;
        let modules: Vec<&Module> = projects.iter().flat_map(|p| &p.modules).collect();
        if modules.is_empty() {
            println!("FAIL  {}: no module found", dir.display());
            ok = false;
        }
        for module in &modules {
            if expected
                .is_some_and(|(code, _)| module.graph.diagnostics.iter().any(|d| d.code == code))
            {
                warned = warned.saturating_add(1);
            }
            ok &= report(module);
            ok &= check_list_order(module)?;
            if files {
                for file in &module.graph.files {
                    let via = match file.via {
                        Via::Root => "root",
                        Via::Listed => "listed",
                        Via::IncludeList => "includelist",
                        Via::Include => "include",
                    };
                    println!("file  {via} {}", file.path.display());
                }
            }
        }
        if let Some(progs) = &progs {
            let [module] = modules.as_slice() else {
                return Err(format!("--progs needs one module in {}", dir.display()));
            };
            ok &= check_progs_order(module, progs)?;
        }
        println!(
            "project-corpus: {}: {} modules, {} files",
            dir.display(),
            modules.len(),
            modules.iter().map(|m| m.graph.files.len()).sum::<usize>()
        );
    }
    if let Some((code, count)) = expected {
        let verdict = if warned == count { "ok  " } else { "FAIL" };
        println!(
            "{verdict}  {warned} modules have {}, {count} expected",
            code.as_str()
        );
        ok &= warned == count;
    }
    println!(
        "project-corpus: {}",
        if ok { "no failures" } else { "failures found" }
    );
    Ok(ok)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use std::fs;
    use std::path::{Path, PathBuf};

    use qc_progs::{Function, ProgsFile, write};

    use super::{main, parse_expected};
    use qc_project::Code;

    #[test]
    fn an_expected_warning_names_a_warning_code() {
        let warnings: Vec<Code> = Code::ALL.into_iter().filter(|c| !c.is_error()).collect();
        for code in &warnings {
            let parsed = parse_expected(&format!("{}=2", code.as_str())).unwrap();
            assert_eq!(parsed, (*code, 2));
        }
        for code in Code::ALL.into_iter().filter(|c| c.is_error()) {
            assert!(parse_expected(&format!("{}=1", code.as_str())).is_err());
        }
        assert!(parse_expected("no-such-code=1").is_err());
        assert!(parse_expected(warnings.first().unwrap().as_str()).is_err());
    }

    fn codebase(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/xtask-tests")
            .join(name);
        if dir.exists() {
            fs::remove_dir_all(&dir).unwrap();
        }
        fs::create_dir_all(&dir).unwrap();
        for (file, text) in files {
            fs::write(dir.join(file), text).unwrap();
        }
        dir
    }

    fn run(args: &[String]) -> Result<bool, String> {
        main(Path::new("."), args)
    }

    fn path(dir: &Path) -> String {
        dir.display().to_string()
    }

    /// A progs whose functions come from `files`, in that order.
    fn progs(dir: &Path, files: &[&str]) -> String {
        let mut strings = vec![0_u8];
        let mut functions = vec![Function::default()];
        for file in files {
            let s_file = i32::try_from(strings.len()).unwrap();
            strings.extend_from_slice(file.as_bytes());
            strings.push(0);
            functions.push(Function {
                s_file,
                ..Function::default()
            });
        }
        let bytes = write(&ProgsFile {
            functions,
            strings,
            ..ProgsFile::default()
        })
        .unwrap();
        let out = dir.join("progs.dat");
        fs::write(&out, bytes).unwrap();
        path(&out)
    }

    fn list() -> Vec<(&'static str, &'static str)> {
        vec![
            ("progs.src", "../progs.dat\ndefs.qc\nworld.qc\n"),
            ("defs.qc", "float x;\n"),
            ("world.qc", "void() main = {};\n"),
        ]
    }

    #[test]
    fn a_list_form_codebase_opens_in_its_order() {
        let dir = codebase("project-list", &list());
        assert!(run(&[path(&dir)]).unwrap());
        assert!(run(&["--files".to_owned(), path(&dir)]).unwrap());
        let progs = progs(&dir, &["defs.qc", "defs.qc", "world.qc"]);
        assert!(run(&["--progs".to_owned(), progs, path(&dir)]).unwrap());
    }

    #[test]
    fn functions_out_of_the_graph_s_order_fail() {
        let dir = codebase("project-progs-order", &list());
        let reversed = progs(&dir, &["world.qc", "defs.qc"]);
        assert!(!run(&["--progs".to_owned(), reversed, path(&dir)]).unwrap());
        let missing = codebase("project-progs-missing", &list());
        let elsewhere = progs(&missing, &["other.qc"]);
        assert!(!run(&["--progs".to_owned(), elsewhere, path(&missing)]).unwrap());
        let bad = missing.join("bad.dat");
        fs::write(&bad, b"not a progs").unwrap();
        assert!(run(&["--progs".to_owned(), path(&bad), path(&missing)]).is_err());
    }

    #[test]
    fn a_file_the_list_names_but_lacks_fails() {
        let dir = codebase(
            "project-missing-file",
            &[("progs.src", "../progs.dat\nnowhere.qc\n")],
        );
        assert!(!run(&[path(&dir)]).unwrap());
    }

    #[test]
    fn a_source_form_codebase_with_directives_is_not_checked_against_a_list() {
        let dir = codebase(
            "project-directives",
            &[
                (
                    "progs.src",
                    "#pragma progs_dat \"progs.dat\"\n#include \"world.qc\"\n",
                ),
                ("world.qc", "void() main = {};\n"),
            ],
        );
        assert!(run(&[path(&dir)]).unwrap());
    }

    #[test]
    fn a_codebase_without_a_module_or_with_bad_arguments_fails() {
        let empty = codebase("project-empty", &[("readme.txt", "x")]);
        assert!(!run(&[path(&empty)]).unwrap());
        assert!(run(&[]).unwrap_err().contains("usage"));
        assert!(
            run(&["--fast".to_owned()])
                .unwrap_err()
                .contains("unknown flag")
        );
        assert!(run(&["--progs".to_owned()]).is_err());
        let two = [
            "--progs".to_owned(),
            "p.dat".to_owned(),
            path(&empty),
            path(&empty),
        ];
        assert!(run(&two).unwrap_err().contains("usage"));
        assert!(run(&["--progs".to_owned(), "p.dat".to_owned(), path(&empty)]).is_err());
        assert!(run(&[path(&empty.join("nowhere"))]).is_err());
    }
}
