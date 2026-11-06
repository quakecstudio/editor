// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask compiler-diff --functions <codebase>...`: the per-function differential
//! (`docs/spec/differential.md` DIFF-15, P2-153).
//!
//! The functions of a corpus codebase the pinned FTEQCC compiles are cut, 64 at a time, into a
//! program for each group (DIFF-15.7): every declaration kept, the bodies of the group's
//! functions, of what the world's setup runs and of the functions all of them call kept, and
//! every other body emptied. Each program is compiled with the pinned FTEQCC, the fixed FTEQCC
//! and `qc-codegen` (DIFF-15.3), and the `compiler-diff` binary calls the group's functions
//! alone, adding the statements they executed to a list by function and offset (DIFF-15.9).
//! The statements of the whole codebase that list reaches are its coverage (DIFF-15.5), held to
//! the bar of DIFF-15.6. `--shard k/n` runs every n-th group from the k-th, so a codebase too
//! big for one CI job's six hours runs as several, and `--merge` holds the shards' lists together
//! to the bar (DIFF-15.10).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::Command;

use qc_progs::Progs;
use qc_progs::opcode::Opcode;
use qc_sema::{Db, FileId};
use qc_syntax::ast::{AstNode as _, Initializer};

use super::{FTEQCC_LIMIT, engine, run_binary, write, write_progs};
use crate::differential::asset_root;
use crate::oracles::codegen::{self, Profile};
use crate::oracles::{self, Oracles, rerelease};
use crate::readiness::Shard;

/// How many functions one cut program holds and calls (DIFF-15.7, DIFF-15.9).
const GROUP: usize = 64;

/// How many cut programs one run of the binary compares: as many as are compiled at once.
fn batch() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZero::get)
}

/// What an emptied body becomes (DIFF-15.7).
const STUB: &[u8] = b"{}";

/// The coverage the per-function runs of codebase `id` must reach (DIFF-15.6): DIFF-12.5's
/// 90%, and crmod7's accepted 87.7% (#661).
fn bar(id: &str) -> f64 {
    if id == "crmod7" { 0.877 } else { 0.9 }
}

/// One function body of a codebase's source (DIFF-15.7): the function's name, and the file and
/// bytes of its block, `{` to `}`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Body {
    name: Vec<u8>,
    path: PathBuf,
    /// The block's first byte, its `{`.
    start: usize,
    /// One past its last, its `}`.
    end: usize,
}

/// Runs the per-function differential over each codebase `ids` names (DIFF-15), or `shard`'s
/// groups of each (DIFF-15.10).
pub(super) fn run(root: &Path, ids: &[String], shard: Option<Shard>) -> Result<bool, String> {
    if ids.is_empty() {
        return Err("--functions takes one or more codebases".to_owned());
    }
    for id in ids {
        if rerelease::fteqcc_args(id).is_none() {
            return Err(format!("{id} is not a codebase the pinned FTEQCC compiles"));
        }
    }
    let oracles = oracles::build(root)?;
    let codebases = asset_root()?.join("codebase");
    let mut ok = true;
    for id in ids {
        ok &= codebase(root, &oracles, &codebases, id, shard)?;
    }
    Ok(ok)
}

/// Holds the statements the shards' `executed` files of codebase `ids`' one codebase list,
/// together, to its bar (DIFF-15.10); true when they reach it.
pub(super) fn merge(root: &Path, ids: &[String], executed: &[PathBuf]) -> Result<bool, String> {
    let [id] = ids else {
        return Err("--merge takes one codebase".to_owned());
    };
    let oracles = oracles::build(root)?;
    let codebases = asset_root()?.join("codebase");
    let work = root
        .join("target")
        .join("compiler-diff")
        .join("functions")
        .join(format!("{id}-merged"));
    let [whole_path, _] = write_progs(&oracles, &codebases, id, &work)?;
    let whole = fs::read(&whole_path).map_err(|e| format!("{}: {e}", whole_path.display()))?;
    let progs = Progs::read(&whole).map_err(|e| format!("{id}: {e}"))?;
    let mut text = String::new();
    for file in executed {
        text.push_str(&fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?);
        text.push('\n');
    }
    let (covered, total) = coverage(&progs, &text);
    let share = share(covered, total);
    let passed = share >= bar(id);
    eprintln!(
        "{}  {id}: {} shards together, {covered} of {total} statements ({:.1}%, the bar {:.1}%)",
        if passed { "ok  " } else { "FAIL" },
        executed.len(),
        share * 100.0,
        bar(id) * 100.0
    );
    Ok(passed)
}

/// `covered` of `total` as a fraction, 0 for none.
fn share(covered: u32, total: u32) -> f64 {
    if total == 0 {
        0.0
    } else {
        f64::from(covered) / f64::from(total)
    }
}

/// The profile `qc-codegen` compiles codebase `id` in, as the pinned FTEQCC reads it.
fn profile(id: &str) -> Profile {
    Profile::Fteqcc {
        autoproto: rerelease::autoprototypes(id),
    }
}

/// The per-function differential of codebase `id`: every function cut, compiled and compared,
/// and the coverage of the runs together; true when it passes (DIFF-15.6).
fn codebase(
    root: &Path,
    oracles: &Oracles,
    codebases: &Path,
    id: &str,
    shard: Option<Shard>,
) -> Result<bool, String> {
    let work = root
        .join("target")
        .join("compiler-diff")
        .join("functions")
        .join(id);
    if work.exists() {
        fs::remove_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    }
    let [whole_path, _] = write_progs(oracles, codebases, id, &work)?;
    let whole = fs::read(&whole_path).map_err(|e| format!("{}: {e}", whole_path.display()))?;
    let source = codebases.join(id);
    let (_, db) = codegen::compile_analysed(&source, profile(id))?;
    let (bodies, sources) = bodies(&db);
    let progs = Progs::read(&whole).map_err(|e| format!("{id}: {e}"))?;
    let graph = calls(&progs);
    // DIFF-15.8: every cut keeps what the world's setup runs, and what that calls.
    let mut setup = BTreeSet::new();
    for name in setup_functions(root, &whole_path, id)? {
        setup.extend(closure(&graph, &name));
    }
    let mut functions: Vec<&[u8]> = Vec::new();
    for body in &bodies {
        if graph.contains_key(&body.name) && !functions.contains(&body.name.as_slice()) {
            functions.push(&body.name);
        }
    }
    let executed = work.join("executed.tsv");
    let mut failures = Vec::new();
    let all = functions.chunks(GROUP).count();
    let groups: Vec<&[&[u8]]> = functions
        .chunks(GROUP)
        .enumerate()
        .filter(|(n, _)| shard.is_none_or(|shard| shard.holds(*n)))
        .map(|(_, group)| group)
        .collect();
    for (batch, names) in groups.chunks(batch()).enumerate() {
        let dir = work.join(format!("batch-{batch}"));
        let mut pairs = String::new();
        for (n, group, cut) in compile_batch(
            oracles,
            id,
            &source,
            &work,
            &dir,
            names,
            (&bodies, &sources, &graph, &setup),
        )? {
            let program = dir.join(n.to_string());
            let label = group
                .iter()
                .map(|name| String::from_utf8_lossy(name))
                .collect::<Vec<_>>()
                .join(",");
            match cut {
                Cut::Compiled => pairs.push_str(&format!(
                    "{label}\t{}\t{}\t{}\n",
                    program.join("theirs.dat").display(),
                    program.join("ours.dat").display(),
                    program.join("fixed.dat").display()
                )),
                Cut::Failed(why) => {
                    let first = group.first().map(|name| String::from_utf8_lossy(name));
                    let first = first.unwrap_or_default();
                    eprintln!("FAIL  {id}: the cut from {first}: {why}");
                    failures.push(format!("the cut from {first}: {why}"));
                }
            }
        }
        let list = dir.join("function-pairs.tsv");
        write(&list, pairs.as_bytes())?;
        let executed = executed.to_string_lossy().into_owned();
        ok_or_fail(
            &mut failures,
            run_binary(
                root,
                &[
                    "--function-pairs",
                    &list.to_string_lossy(),
                    "--executed",
                    &executed,
                    "--oracle",
                    "fteqcc",
                    "--engine",
                    engine(id),
                    "--codebase",
                    id,
                ],
            )?,
            batch,
        );
        fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let text = fs::read_to_string(&executed).unwrap_or_default();
    let (covered, total) = coverage(&progs, &text);
    let share = share(covered, total);
    if !failures.is_empty() {
        eprintln!("{id}: {} failures, each printed above", failures.len());
    }
    // DIFF-15.10: a shard's coverage is a part; the bar is held to the shards together.
    let passed = failures.is_empty() && (shard.is_some() || share >= bar(id));
    let held = match shard {
        Some(shard) => format!("shard {shard}, the bar left to --merge"),
        None => format!("the bar {:.1}%", bar(id) * 100.0),
    };
    eprintln!(
        "{}  {id}: {} cuts of {all} run, {covered} of {total} statements ({:.1}%, {held})",
        if passed { "ok  " } else { "FAIL" },
        groups.len(),
        share * 100.0,
    );
    Ok(passed)
}

/// The codebase's bodies, the source of each of its files, and its call graph.
type Codebase<'a> = (
    &'a [Body],
    &'a BTreeMap<PathBuf, Vec<u8>>,
    &'a BTreeMap<Vec<u8>, BTreeSet<Vec<u8>>>,
    &'a BTreeSet<Vec<u8>>,
);

/// One group's cut compiled: its place in its batch, its functions and the outcome.
type Compiled<'g, 'n> = (usize, &'g [&'n [u8]], Cut);

/// Cuts and compiles the program of each group of functions `groups` lists in `dir/<n>`, `n`
/// its place in the list (DIFF-15.7, DIFF-15.3), on as many threads as the machine runs at
/// once; the outcomes are in the list's order, each with its place and functions.
fn compile_batch<'g, 'n>(
    oracles: &Oracles,
    id: &str,
    source: &Path,
    work: &Path,
    dir: &Path,
    groups: &[&'g [&'n [u8]]],
    (bodies, sources, graph, setup): Codebase<'_>,
) -> Result<Vec<Compiled<'g, 'n>>, String> {
    let threads = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let results: Vec<Result<Vec<Compiled<'g, 'n>>, String>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|thread| {
                scope.spawn(move || {
                    let mut done = Vec::new();
                    for (n, group) in groups.iter().enumerate().skip(thread).step_by(threads) {
                        let mut keep = setup.clone();
                        for name in *group {
                            keep.extend(closure(graph, name));
                        }
                        let files = cut(bodies, sources, &keep);
                        let first = group.first().map(|name| String::from_utf8_lossy(name));
                        let log = work
                            .join("failed")
                            .join(format!("{}.log", first.unwrap_or_default()));
                        let program = dir.join(n.to_string());
                        let outcome = compile_cut(oracles, id, source, &program, &files, &log)?;
                        done.push((n, *group, outcome));
                    }
                    Ok(done)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| Err("a compile thread stopped".to_owned()))
            })
            .collect()
    });
    let mut all = Vec::with_capacity(groups.len());
    for result in results {
        all.extend(result?);
    }
    all.sort_by_key(|(n, _, _)| *n);
    Ok(all)
}

/// The functions the scenarios' setups run in the whole codebase's progs at `whole`, as the
/// `compiler-diff` binary lists them (DIFF-15.8).
fn setup_functions(root: &Path, whole: &Path, id: &str) -> Result<Vec<Vec<u8>>, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
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
            "--engine",
            engine(id),
            "--codebase",
            id,
            "--setup-functions",
        ])
        .arg(whole)
        .output()
        .map_err(|e| format!("cannot run the compiler differential: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "{id}: the setup's functions could not be listed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(output
        .stdout
        .split(|&b| b == b'\n')
        .filter(|line| !line.is_empty())
        .map(<[u8]>::to_vec)
        .collect())
}

/// Adds a failure for batch `batch` when its run of the binary found a divergence.
fn ok_or_fail(failures: &mut Vec<String>, compared: bool, batch: usize) {
    if !compared {
        failures.push(format!("batch {batch} diverged (see above)"));
    }
}

/// Every function body at file level of the codebase `db` analysed (DIFF-15.7), each located
/// in its source file, with the source of each file; a body whose block is not one span of one
/// file from `{` to `}` with its conditional directives balanced, and a `__wrap` or
/// `__accumulate` body, is left out, and so never emptied.
fn bodies(db: &Db) -> (Vec<Body>, BTreeMap<PathBuf, Vec<u8>>) {
    let mut bodies = BTreeSet::new();
    let mut sources = BTreeMap::new();
    for index in 0.. {
        let file = FileId::new(index);
        let Some(path) = db.file_path(file) else {
            break;
        };
        sources.insert(path.to_path_buf(), db.file_text(file).to_vec());
        let (Some(parse), Some(origins)) = (db.expanded_parse(file), db.expanded_origins(file))
        else {
            continue;
        };
        let Some(tree) = parse.tree() else {
            continue;
        };
        for declaration in tree.declarations() {
            // FTEQCC refuses a `__wrap` body that does not call `prior`, and an `__accumulate`
            // body is one piece of the function, so neither is emptied.
            let text = declaration.syntax().text();
            if [b"__wrap".as_slice(), b"__accumulate"]
                .iter()
                .any(|word| text.windows(word.len()).any(|w| w == *word))
            {
                continue;
            }
            for declarator in declaration.declarators() {
                if declarator.scope().is_some() {
                    continue;
                }
                let (Some(name), Some(Initializer::Body(body))) =
                    (declarator.name(), declarator.initializer())
                else {
                    continue;
                };
                let Some(block) = body.block() else {
                    continue;
                };
                let Some(location) = origins.locate(block.syntax().span()) else {
                    continue;
                };
                let (Some(path), text) = (db.file_path(location.file), db.file_text(location.file))
                else {
                    continue;
                };
                let (start, end) = (location.span.start, location.span.end);
                if text.get(start..end).is_some_and(is_whole_block) {
                    bodies.insert(Body {
                        name: name.text().to_vec(),
                        path: path.to_path_buf(),
                        start,
                        end,
                    });
                }
            }
        }
    }
    (bodies.into_iter().collect(), sources)
}

/// Whether `text` is a block that can be emptied (DIFF-15.7): it starts with `{`, ends with
/// `}`, and every `#if`, `#ifdef` and `#ifndef` line in it is closed by an `#endif` in it.
fn is_whole_block(text: &[u8]) -> bool {
    if !(text.starts_with(b"{") && text.ends_with(b"}")) {
        return false;
    }
    let mut depth = 0_i64;
    for line in text.split(|&b| b == b'\n') {
        let line = line.trim_ascii_start();
        let Some(directive) = line.strip_prefix(b"#") else {
            continue;
        };
        let directive = directive.trim_ascii_start();
        if directive.starts_with(b"if") {
            depth = depth.saturating_add(1);
        } else if directive.starts_with(b"endif") {
            depth = depth.saturating_sub(1);
            if depth < 0 {
                return false;
            }
        }
    }
    depth == 0
}

/// The functions each function of `progs` calls by name (DIFF-15.8): for each body, the
/// functions whose index the global a `CALL0` to `CALL8` of it calls holds; a call through a
/// variable or field, whose global holds no function, adds none.
fn calls(progs: &Progs<'_>) -> BTreeMap<Vec<u8>, BTreeSet<Vec<u8>>> {
    let functions = progs.functions();
    let name_of = |index: i32| {
        usize::try_from(index)
            .ok()
            .filter(|&i| i > 0)
            .and_then(|i| functions.get(i))
            .and_then(|f| progs.string(f.s_name))
    };
    let mut graph: BTreeMap<Vec<u8>, BTreeSet<Vec<u8>>> = BTreeMap::new();
    for (range, name) in body_ranges(progs) {
        let callees = graph.entry(name).or_default();
        for statement in range.filter_map(|s| progs.statements().get(s)) {
            let is_call = Opcode::from_number(statement.op)
                .is_some_and(|op| (Opcode::Call0..=Opcode::Call8).contains(&op));
            let held = usize::try_from(statement.a)
                .ok()
                .and_then(|a| progs.globals().get(a));
            if is_call && let Some(callee) = held.and_then(|slot| name_of(slot.as_i32())) {
                callees.insert(callee.to_vec());
            }
        }
    }
    graph
}

/// The body of each function of `progs` with one, its statements from its first to the next
/// function's first or the progs' end, with its name.
fn body_ranges(progs: &Progs<'_>) -> Vec<(Range<usize>, Vec<u8>)> {
    let mut starts: Vec<(usize, Vec<u8>)> = progs
        .functions()
        .iter()
        .filter_map(|f| {
            let start = usize::try_from(f.first_statement).ok().filter(|&s| s > 0)?;
            Some((start, progs.string(f.s_name)?.to_vec()))
        })
        .collect();
    starts.sort();
    let end = progs.statements().len();
    let mut ranges = Vec::with_capacity(starts.len());
    for (k, (start, name)) in starts.iter().enumerate() {
        let next = starts.get(k.saturating_add(1)).map_or(end, |(s, _)| *s);
        ranges.push((*start..next, name.clone()));
    }
    ranges
}

/// `root` and every function it calls, through any depth of calls (DIFF-15.8).
fn closure(graph: &BTreeMap<Vec<u8>, BTreeSet<Vec<u8>>>, root: &[u8]) -> BTreeSet<Vec<u8>> {
    let mut keep = BTreeSet::new();
    let mut next = vec![root.to_vec()];
    while let Some(name) = next.pop() {
        if let Some(callees) = graph.get(&name) {
            next.extend(callees.iter().filter(|c| !keep.contains(*c)).cloned());
        }
        keep.insert(name);
    }
    keep
}

/// The files of the program cut for the functions `keep` (DIFF-15.7): each file of `sources`
/// with a body of `bodies` whose function `keep` does not name, every such body emptied.
fn cut(
    bodies: &[Body],
    sources: &BTreeMap<PathBuf, Vec<u8>>,
    keep: &BTreeSet<Vec<u8>>,
) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut emptied: BTreeMap<&Path, Vec<(usize, usize)>> = BTreeMap::new();
    for body in bodies.iter().filter(|b| !keep.contains(&b.name)) {
        emptied
            .entry(&body.path)
            .or_default()
            .push((body.start, body.end));
    }
    let mut files = BTreeMap::new();
    for (path, mut spans) in emptied {
        let Some(text) = sources.get(path) else {
            continue;
        };
        spans.sort_unstable();
        let mut out = Vec::with_capacity(text.len());
        let mut at = 0;
        for (start, end) in spans {
            if start < at {
                continue;
            }
            out.extend_from_slice(text.get(at..start).unwrap_or_default());
            out.extend_from_slice(STUB);
            at = end;
        }
        out.extend_from_slice(text.get(at..).unwrap_or_default());
        files.insert(path.to_path_buf(), out);
    }
    files
}

/// How compiling one cut program went (DIFF-15.3).
enum Cut {
    /// The three progs are written beside it.
    Compiled,
    /// A compiler refused it, as this says.
    Failed(String),
}

/// Writes the cut program of `files` over a copy of the codebase `source` in `dir`, and
/// compiles it with the fixed FTEQCC, the pinned FTEQCC and `qc-codegen` into `fixed.dat`,
/// `theirs.dat` and `ours.dat` there (DIFF-15.3); what a compiler that refused it printed is
/// written to `log`.
fn compile_cut(
    oracles: &Oracles,
    id: &str,
    source: &Path,
    dir: &Path,
    files: &BTreeMap<PathBuf, Vec<u8>>,
    log_path: &Path,
) -> Result<Cut, String> {
    let src = dir.join("src");
    rerelease::copy_tree(source, &src)?;
    for (path, text) in files {
        let relative = path
            .strip_prefix(source)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        write(&src.join(relative), text)?;
    }
    let args = rerelease::fteqcc_args(id).ok_or_else(|| format!("{id}: no FTEQCC flags"))?;
    for (fteqcc, out) in [
        (&oracles.fteqcc_fixed, "fixed.dat"),
        (&oracles.fteqcc, "theirs.dat"),
    ] {
        let mut command = Command::new(fteqcc);
        command.args(&args).current_dir(&src);
        let output = oracles
            .exec
            .output_within(&mut command, FTEQCC_LIMIT)
            .map_err(|e| format!("cannot run fteqcc: {e}"))?;
        let built = dir.join("progs.dat");
        if !built.is_file() {
            let log = output.map_or_else(
                || "it ran out of time".to_owned(),
                |o| {
                    format!(
                        "{}{}",
                        String::from_utf8_lossy(&o.stdout),
                        String::from_utf8_lossy(&o.stderr)
                    )
                },
            );
            if let Some(parent) = log_path.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            write(log_path, log.as_bytes())?;
            let line = log
                .lines()
                .find(|l| l.contains("error"))
                .or_else(|| log.lines().last())
                .unwrap_or("");
            return Ok(Cut::Failed(format!(
                "{out}: FTEQCC rejected the cut program: {line}"
            )));
        }
        fs::rename(&built, dir.join(out)).map_err(|e| format!("{}: {e}", built.display()))?;
    }
    let compiled = match codegen::compile_codebase(&src, profile(id)) {
        Ok(compiled) => compiled,
        Err(error) => {
            write(log_path, error.as_bytes())?;
            return Ok(Cut::Failed(format!(
                "qc-codegen refused what FTEQCC compiled: {error}"
            )));
        }
    };
    write(&dir.join("ours.dat"), &codegen::bytes(&compiled)?)?;
    fs::remove_dir_all(&src).map_err(|e| format!("{}: {e}", src.display()))?;
    Ok(Cut::Compiled)
}

/// The statements of the whole codebase's progs `progs` that the per-function runs executed,
/// and how many it has (DIFF-15.5): a statement at an offset in the body of a function of one
/// name is covered when `executed`, `<name>\t<offset>` lines, lists that name and offset.
fn coverage(progs: &Progs<'_>, executed: &str) -> (u32, u32) {
    let mut listed: BTreeSet<(&[u8], usize)> = BTreeSet::new();
    for line in executed.lines() {
        if let Some((name, offset)) = line.rsplit_once('\t')
            && let Ok(offset) = offset.parse::<usize>()
        {
            listed.insert((name.as_bytes(), offset));
        }
    }
    let mut covered = 0_u32;
    for (range, name) in body_ranges(progs) {
        let len = range.end.saturating_sub(range.start);
        let reached = (0..len).filter(|o| listed.contains(&(name.as_slice(), *o)));
        covered = covered.saturating_add(u32::try_from(reached.count()).unwrap_or(u32::MAX));
    }
    let total = u32::try_from(progs.statements().len())
        .unwrap_or(u32::MAX)
        .saturating_sub(1);
    (covered, total)
}

#[cfg(test)]
mod tests;
