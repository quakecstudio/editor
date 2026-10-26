// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask sema-corpus [--write] <dir>[=<progs.dat>]...`: the golden indexes and the
//! definitions differential of the semantic model (P2-54, TEST-6).
//!
//! For each codebase directory it opens the project and every module through `qc_sema::Db` as
//! the editor does (`Db::analyze_module`), under the vanilla profile for id 1.01 (a directory
//! named `id-1.01`) and the FTEQCC profile for any other, with autoprototyping for a codebase
//! FTEQCC builds with `-Fautoproto`. It writes the index as text, every definition with its
//! kind, type, place and number of references, and every diagnostic, and compares it with the
//! golden file `docs/spec/vectors/sema/<dir name>.txt`; `--write` rewrites the golden file
//! instead. A difference fails the check, with the first lines that differ.
//!
//! With `=<progs.dat>` after a directory, the progs an oracle compiler built from that codebase
//! (`cargo xtask corpus build` writes them to `<cache>/progs/<id>/progs.dat`) is the oracle for
//! its global definitions: every global, function and field of the index that the progs keeps
//! must be a global def of the same name and type there, and a field also a field def of the
//! inner type. For id 1.01, which id's `qcc` writes whole, every such definition must be in the
//! progs. A definition the progs does not agree with is a failure unless
//! `docs/spec/vectors/sema/<dir name>.differences.txt` lists it with its reason.
//!
//! The codebases come from `cargo xtask corpus fetch id-1.01 rerelease`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use qc_lexer::LineIndex;
use qc_progs::Progs;
use qc_project::{Module, open};
use qc_sema::{Db, DefKind, FileId, ModuleAnalysis, ModuleInput, SimpleType, Type};
use qc_types::{DefType, Etype};

use crate::oracles::rerelease::autoprototypes;

const USAGE: &str = "usage: cargo xtask sema-corpus [--write] <dir>[=<progs.dat>]...";

/// Where the golden files are, under the repository root.
const GOLDEN: &str = "docs/spec/vectors/sema";

/// The codebase resolved under the vanilla profile; every other is FTEQCC's.
const VANILLA: &str = "id-1.01";

/// One codebase to check and its oracle progs, if any.
struct Target {
    dir: PathBuf,
    progs: Option<PathBuf>,
}

pub(crate) fn main(root: &Path, args: &[String]) -> Result<bool, String> {
    let mut write = false;
    let mut targets = Vec::new();
    for arg in args {
        if arg == "--write" {
            write = true;
        } else if arg.starts_with("--") {
            return Err(format!("unknown flag {arg}\n{USAGE}"));
        } else {
            let (dir, progs) = match arg.split_once('=') {
                Some((dir, progs)) => (dir, Some(PathBuf::from(progs))),
                None => (arg.as_str(), None),
            };
            targets.push(Target {
                dir: PathBuf::from(dir),
                progs,
            });
        }
    }
    if targets.is_empty() {
        return Err(USAGE.to_owned());
    }
    let mut ok = true;
    for target in &targets {
        ok &= check(root, target, write)?;
    }
    println!(
        "sema-corpus: {}",
        if ok {
            "every index matches its golden file and its progs"
        } else {
            "failures found"
        }
    );
    Ok(ok)
}

/// A codebase's name: its directory's.
fn name_of(dir: &Path) -> Result<String, String> {
    dir.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .ok_or_else(|| format!("{}: no directory name", dir.display()))
}

fn check(root: &Path, target: &Target, write: bool) -> Result<bool, String> {
    let name = name_of(&target.dir)?;
    let project = open(&target.dir).map_err(|e| format!("{}: {e}", target.dir.display()))?;
    let dialect = if name == VANILLA {
        qc_syntax::Dialect::Vanilla
    } else {
        qc_syntax::Dialect::Fteqcc
    };
    let autoproto = autoprototypes(&name);
    let mut text = format!(
        "# cargo xtask sema-corpus: {name}, the {} profile{}\n",
        if name == VANILLA { "vanilla" } else { "FTEQCC" },
        if autoproto {
            " with autoprototyping"
        } else {
            ""
        }
    );
    let mut ok = true;
    let mut modules = 0_usize;
    let mut comparisons = Vec::new();
    for module in &project.modules {
        modules = modules.saturating_add(1);
        let (db, analysis) = analyze(module, dialect, autoproto)?;
        render(&mut text, &target.dir, module, &db, &analysis);
        if let Some(progs) = &target.progs {
            comparisons.push(differential(progs, &analysis, name == VANILLA)?);
        }
    }
    if target.progs.is_some() {
        let (passed, lines) = judge(&name, &comparisons, &listed_differences(root, &name)?);
        for line in lines {
            println!("{line}");
        }
        ok &= passed;
    }
    let golden = root.join(GOLDEN).join(format!("{name}.txt"));
    if write {
        fs::write(&golden, &text).map_err(|e| format!("{}: {e}", golden.display()))?;
        println!("sema-corpus: {name}: wrote {}", golden.display());
        return Ok(ok);
    }
    let want = fs::read_to_string(&golden).map_err(|e| format!("{}: {e}", golden.display()))?;
    if want == text {
        println!("ok    {name}: {modules} modules match {GOLDEN}/{name}.txt");
    } else {
        ok = false;
        report_difference(&name, &want, &text);
    }
    Ok(ok)
}

/// Opens one module through the semantic model, as `cargo xtask fteqcc-profile` does.
fn analyze(
    module: &Module,
    dialect: qc_syntax::Dialect,
    autoproto: bool,
) -> Result<(Db, ModuleAnalysis), String> {
    let mut db = Db::new();
    db.set_dialect(dialect);
    db.set_autoproto(autoproto);
    for (index, file) in module.graph.files.iter().enumerate() {
        let text = fs::read(&file.path).map_err(|e| format!("{}: {e}", file.path.display()))?;
        db.set_file(FileId(index), file.path.clone(), text);
    }
    let analysis = db.analyze_module(&ModuleInput::from_module(module));
    Ok((db, analysis))
}

/// The place of `span.start` in `file`: its path under `dir` and its 1-based line.
fn place(db: &Db, dir: &Path, file: FileId, start: usize) -> String {
    let path = db.file_path(file).map_or_else(
        || "?".to_owned(),
        |path| {
            path.strip_prefix(dir)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/")
        },
    );
    let line = LineIndex::new(db.file_text(file)).line(start);
    format!("{path}:{line}")
}

/// Appends one module's index to `text`: a header, every definition in declaration order with
/// the references to it, and every diagnostic.
fn render(text: &mut String, dir: &Path, module: &Module, db: &Db, analysis: &ModuleAnalysis) {
    let index = &analysis.index;
    let mut refs: HashMap<usize, usize> = HashMap::new();
    for reference in &index.refs {
        let count = refs.entry(reference.def.0).or_default();
        *count = count.saturating_add(1);
    }
    let diagnostics: Vec<_> = analysis.diagnostics().collect();
    text.push_str(&format!(
        "module {}: {} definitions, {} references, {} diagnostics\n",
        module.name,
        index.defs.len(),
        index.refs.len(),
        diagnostics.len()
    ));
    for (id, def) in index.defs.iter().enumerate() {
        text.push_str(&format!(
            "def {} {} {} {} refs {}\n",
            place(db, dir, def.location.file, def.location.span.start),
            kind_name(def.kind),
            def.name,
            type_name(&def.ty),
            refs.get(&id).copied().unwrap_or(0)
        ));
    }
    for d in diagnostics {
        text.push_str(&format!(
            "diag {} {} {}\n",
            place(db, dir, d.file, d.diagnostic.span.start),
            d.diagnostic.code.as_str(),
            d.diagnostic.message
        ));
    }
}

const fn kind_name(kind: DefKind) -> &'static str {
    match kind {
        DefKind::Global => "global",
        DefKind::Function => "function",
        DefKind::Field => "field",
        DefKind::Param => "param",
        DefKind::Local => "local",
    }
}

/// A type as QuakeC writes it, with no spaces: `float`, `.vector`, `void(float,...)`.
fn type_name(ty: &Type) -> String {
    match ty {
        Type::Unknown => "?".to_owned(),
        Type::Simple(simple) => simple.as_str().to_owned(),
        Type::Field(inner) => format!(".{}", type_name(inner)),
        Type::Function(func) => {
            let mut params: Vec<String> = func.params.iter().map(type_name).collect();
            if func.variadic {
                params.push("...".to_owned());
            }
            format!("{}({})", type_name(&func.ret), params.join(","))
        }
        Type::Named(name) => name.to_string(),
        Type::Pointer(inner) => format!("{}*", type_name(inner)),
    }
}

/// Prints the first lines in which `got` differs from the golden `want`.
fn report_difference(name: &str, want: &str, got: &str) {
    println!("FAIL  {name}: the index differs from {GOLDEN}/{name}.txt");
    let mut shown = 0_usize;
    let mut want_lines = want.lines();
    let mut got_lines = got.lines();
    let mut line = 0_usize;
    loop {
        line = line.saturating_add(1);
        let (a, b) = (want_lines.next(), got_lines.next());
        if a.is_none() && b.is_none() {
            break;
        }
        if a != b {
            println!("      line {line}: golden {a:?}, index {b:?}");
            shown = shown.saturating_add(1);
            if shown >= 10 {
                break;
            }
        }
    }
    println!(
        "      run `cargo xtask sema-corpus --write` on the codebase to update the golden file"
    );
}

/// The progs type a global definition of type `ty` has: a field is `ev_field`, a function
/// `ev_function`, and a simple type its own.
fn progs_type(ty: &Type) -> Option<Etype> {
    match ty {
        Type::Simple(SimpleType::Void) => Some(Etype::Void),
        Type::Simple(SimpleType::String) => Some(Etype::String),
        Type::Simple(SimpleType::Float) => Some(Etype::Float),
        Type::Simple(SimpleType::Vector) => Some(Etype::Vector),
        Type::Simple(SimpleType::Entity) => Some(Etype::Entity),
        Type::Field(_) => Some(Etype::Field),
        Type::Function(_) => Some(Etype::Function),
        Type::Pointer(_) => Some(Etype::Pointer),
        Type::Unknown | Type::Named(_) => None,
    }
}

/// The types of the defs of each name in a table of the progs. id's `qcc` writes a def for
/// every local too, so a global's name may have several defs of different types.
fn defs_by_name(
    progs: &Progs<'_>,
    defs: impl Iterator<Item = qc_progs::Def>,
) -> BTreeMap<String, Vec<DefType>> {
    let mut out: BTreeMap<String, Vec<DefType>> = BTreeMap::new();
    for def in defs {
        let Some(name) = progs.string(def.s_name) else {
            continue;
        };
        out.entry(String::from_utf8_lossy(name).into_owned())
            .or_default()
            .push(def.def_type());
    }
    out
}

/// Whether one of `found` is `want`.
fn has_type(found: &[DefType], want: Etype) -> bool {
    found.iter().any(|def| def.etype() == Some(want))
}

/// The type numbers of `found`, for a message.
fn numbers(found: &[DefType]) -> String {
    let numbers: Vec<String> = found.iter().map(|def| def.number().to_string()).collect();
    numbers.join(", ")
}

/// The listed differences of a codebase: `<kind> <name>: <reason>` per line, `#` comments.
fn listed_differences(root: &Path, name: &str) -> Result<BTreeMap<String, String>, String> {
    let path = root.join(GOLDEN).join(format!("{name}.differences.txt"));
    let Ok(text) = fs::read_to_string(&path) else {
        return Ok(BTreeMap::new());
    };
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, reason) = line
            .split_once(": ")
            .ok_or_else(|| format!("{}: {line:?} has no reason", path.display()))?;
        out.insert(key.to_owned(), reason.to_owned());
    }
    Ok(out)
}

/// One module's global definitions compared with the progs.
#[derive(Debug, Default)]
struct Comparison {
    /// Definitions the progs was checked for.
    compared: usize,
    /// Global definitions of the module, compared or not.
    considered: usize,
    /// `<kind> <name>` and what is wrong, for each definition the progs disagrees with.
    problems: Vec<(String, String)>,
}

/// Compares the global definitions of `analysis` with the defs of the progs at `progs`.
fn differential(
    progs: &Path,
    analysis: &ModuleAnalysis,
    whole: bool,
) -> Result<Comparison, String> {
    let bytes = fs::read(progs).map_err(|e| format!("{}: {e}", progs.display()))?;
    let progs = Progs::read(&bytes).map_err(|e| format!("{}: {e:?}", progs.display()))?;
    let globals = defs_by_name(&progs, progs.global_defs().iter());
    let fields = defs_by_name(&progs, progs.field_defs().iter());
    let mut comparison = Comparison::default();
    for def in &analysis.index.defs {
        if def.owner.is_some() || def.member_of.is_some() || def.generated {
            continue;
        }
        comparison.considered = comparison.considered.saturating_add(1);
        let problem = match (globals.get(&def.name), progs_type(&def.ty)) {
            (None, _) if !whole => continue,
            (None, _) => Some("is not a global def of the progs".to_owned()),
            (Some(_), None) => Some(format!(
                "has type {}, which no progs type is",
                type_name(&def.ty)
            )),
            (Some(found), Some(want)) if !has_type(found, want) => Some(format!(
                "is {}, and the progs' global defs of the name are of type {}",
                type_name(&def.ty),
                numbers(found)
            )),
            (Some(_), Some(_)) => field_problem(def, &fields),
        };
        comparison.compared = comparison.compared.saturating_add(1);
        if let Some(problem) = problem {
            let key = format!("{} {}", kind_name(def.kind), def.name);
            comparison.problems.push((key, problem));
        }
    }
    Ok(comparison)
}

/// Judges the comparisons of every module of codebase `name` against its `listed`
/// differences, which belong to the codebase rather than to one module: a problem fails unless
/// listed, and a listed difference fails when no module has it. Returns whether it passed and
/// the lines to print.
fn judge(
    name: &str,
    comparisons: &[Comparison],
    listed: &BTreeMap<String, String>,
) -> (bool, Vec<String>) {
    let mut lines = Vec::new();
    let mut used: BTreeSet<&str> = BTreeSet::new();
    let mut compared = 0_usize;
    let mut considered = 0_usize;
    for comparison in comparisons {
        compared = compared.saturating_add(comparison.compared);
        considered = considered.saturating_add(comparison.considered);
        for (key, problem) in &comparison.problems {
            if listed.contains_key(key) {
                used.insert(key);
            } else {
                lines.push(format!("FAIL  {name}: {key} {problem}"));
            }
        }
    }
    let stale: Vec<&str> = listed
        .keys()
        .map(String::as_str)
        .filter(|key| !used.contains(key))
        .collect();
    if !stale.is_empty() {
        lines.push(format!(
            "FAIL  {name}: {} listed differences no longer differ; remove them: {}",
            stale.len(),
            stale.join(", ")
        ));
    }
    let ok = lines.is_empty();
    if ok {
        lines.push(format!(
            "ok    {name}: {compared} of {considered} global definitions in {} modules are in \
             the progs and agree with it, {} listed differences",
            comparisons.len(),
            used.len()
        ));
    }
    (ok, lines)
}

/// For a field, the problem with its field def: it must be one, of the field's inner type.
fn field_problem(
    def: &qc_sema::Definition,
    fields: &BTreeMap<String, Vec<DefType>>,
) -> Option<String> {
    let Type::Field(inner) = &def.ty else {
        return None;
    };
    let want = progs_type(inner);
    match fields.get(&def.name) {
        None => Some("is not a field def of the progs".to_owned()),
        Some(found) if want.is_some_and(|want| !has_type(found, want)) => Some(format!(
            "is {}, and the progs' field defs of the name are of type {}",
            type_name(&def.ty),
            numbers(found)
        )),
        Some(_) => None,
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use qc_sema::lower::lower_file;

    use super::*;

    #[test]
    fn types_are_written_as_quakec_writes_them() {
        let parse = qc_syntax::parse(b"void(...) f;\nfloat(float a) g;\n.vector v;\n");
        let lowered = lower_file(&parse, qc_syntax::Dialect::Vanilla);
        let index = qc_sema::resolve_module(&[lowered], qc_sema::ResolveOptions::default());
        let written: Vec<_> = index.defs.iter().map(|d| type_name(&d.ty)).collect();
        assert_eq!(
            written,
            [
                "void(...)",
                "float(float)",
                ".vector",
                ".float",
                ".float",
                ".float"
            ]
        );
        assert_eq!(type_name(&Type::Unknown), "?");
        assert_eq!(type_name(&Type::Named("bool".into())), "bool");
        assert_eq!(
            type_name(&Type::Pointer(std::rc::Rc::new(Type::Named("s".into())))),
            "s*"
        );
    }

    #[test]
    fn progs_types_follow_the_def_kinds() {
        assert_eq!(
            progs_type(&Type::Simple(SimpleType::Float)),
            Some(Etype::Float)
        );
        assert_eq!(
            progs_type(&Type::Field(std::rc::Rc::new(Type::Simple(
                SimpleType::Float
            )))),
            Some(Etype::Field)
        );
        assert_eq!(progs_type(&Type::Unknown), None);
        assert_eq!(progs_type(&Type::Named("x".into())), None);
    }

    fn module(problems: &[&str]) -> Comparison {
        Comparison {
            compared: 2,
            considered: 3,
            problems: problems
                .iter()
                .map(|key| ((*key).to_owned(), "differs".to_owned()))
                .collect(),
        }
    }

    fn listed(keys: &[&str]) -> BTreeMap<String, String> {
        keys.iter()
            .map(|key| ((*key).to_owned(), "a reason".to_owned()))
            .collect()
    }

    #[test]
    fn a_difference_listed_for_one_of_two_modules_passes() {
        let (ok, lines) = judge(
            "two",
            &[module(&["global a"]), module(&[])],
            &listed(&["global a"]),
        );
        assert!(ok, "{lines:?}");
        assert_eq!(
            lines,
            [
                "ok    two: 4 of 6 global definitions in 2 modules are in the progs and agree with it, 1 listed differences"
            ]
        );
    }

    #[test]
    fn a_difference_two_modules_share_is_listed_once() {
        let (ok, lines) = judge(
            "two",
            &[module(&["global a"]), module(&["global a"])],
            &listed(&["global a"]),
        );
        assert!(ok, "{lines:?}");
    }

    #[test]
    fn an_unlisted_problem_or_a_difference_no_module_has_fails() {
        let (ok, lines) = judge("two", &[module(&["global b"]), module(&[])], &listed(&[]));
        assert!(!ok);
        assert_eq!(lines, ["FAIL  two: global b differs"]);
        let (ok, lines) = judge("two", &[module(&[]), module(&[])], &listed(&["global a"]));
        assert!(!ok);
        assert_eq!(
            lines,
            ["FAIL  two: 1 listed differences no longer differ; remove them: global a"]
        );
    }

    #[test]
    fn a_target_takes_its_progs_after_an_equals_sign_and_flags_are_checked() {
        let root = Path::new(".");
        assert!(main(root, &[]).is_err());
        assert!(
            main(root, &["--nope".to_owned()])
                .unwrap_err()
                .contains("--nope")
        );
        assert!(main(root, &["/no/such/dir".to_owned()]).is_err());
    }
}
