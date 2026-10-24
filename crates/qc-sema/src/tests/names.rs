// SPDX-License-Identifier: GPL-3.0-or-later

//! The name the preprocessor is given for each file of a project's graph (FTEF-14.75), read
//! through `Db::analyze_module` with the database holding each file by its resolved path, as
//! the editor's does. The cases are the named projects of `cargo xtask oracles builtins`
//! (`NAMED_PROJECTS` in `xtask/src/oracles/builtins/directives.rs`), whose names the pinned
//! FTEQCC gives.

use std::path::{Path, PathBuf};

use qc_project::{MemoryLoader, build_graph, normalize};

use crate::db::Db;
use crate::expand::ModuleInput;
use crate::symbol::FileId;

/// A file that binds `f` to the name `__FILE__` writes for it (FTEF-14.74).
const NAMED: &str = "void() f = :__FILE__;\n";

/// The directory the project's `progs.src` is in, a resolved path.
const DIR: &str = "/p/qc";

/// An FTEQCC project: the file its list-form `progs.src` names, the files in its directory by
/// their names, and its include directories. By default `t.qc` binds `f` to its name.
#[derive(Clone, Copy, Debug)]
struct ProjectSpec<'a> {
    compiled: &'a str,
    files: &'a [(&'a str, &'a str)],
    include_dirs: &'a [&'a str],
}

impl Default for ProjectSpec<'_> {
    fn default() -> Self {
        Self {
            compiled: "t.qc",
            files: &[("t.qc", NAMED)],
            include_dirs: &[],
        }
    }
}

/// How the semantic model is given the files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Given {
    /// By the graph's input, [`ModuleInput::from_graph`].
    Graph,
    /// By their paths alone, with no names, in the graph's directory, which a path is not
    /// opened in.
    Paths,
}

/// The engine name `f` is bound to in the project `spec` read as `given` says, and the errors.
fn bound_name(spec: &ProjectSpec<'_>, given: Given) -> (Option<String>, Vec<qc_diag::Code>) {
    let dir = Path::new(DIR);
    let at = |name: &str| normalize(&dir.join(name));
    let mut loader = MemoryLoader::new();
    loader.insert(
        at("progs.src"),
        format!("progs.dat\n{}\n", spec.compiled).into_bytes(),
    );
    for (name, text) in spec.files {
        loader.insert(at(name), text.as_bytes());
    }
    let include_dirs: Vec<PathBuf> = spec.include_dirs.iter().map(PathBuf::from).collect();
    let graph = build_graph(
        &mut loader,
        &at("progs.src"),
        &[],
        &include_dirs,
        qc_syntax::Dialect::Fteqcc,
    );
    assert!(graph.diagnostics.is_empty(), "{:?}", graph.diagnostics);
    let mut db = Db::new();
    db.set_dialect(qc_syntax::Dialect::Fteqcc);
    for (index, file) in graph.files.iter().enumerate() {
        let text = spec
            .files
            .iter()
            .find(|(name, _)| at(name) == file.path)
            .map(|(_, text)| text.as_bytes().to_vec())
            .unwrap();
        db.set_file(FileId(index), file.path.clone(), text);
    }
    let mut input = ModuleInput {
        include_dirs,
        ..ModuleInput::from_graph(&graph)
    };
    if given == Given::Paths {
        input.names.clear();
    }
    let errors = db
        .analyze_module(&input)
        .diagnostics()
        .filter(|d| d.diagnostic.code.is_error())
        .map(|d| d.diagnostic.code)
        .collect();
    let name = db
        .expanded_decls(FileId(0))
        .and_then(|decls| decls.decls.iter().find(|decl| decl.name == "f"))
        .and_then(|decl| decl.literal.as_ref())
        .and_then(|literal| literal.builtin_function("f"))
        .map(|function| function.name);
    (name, errors)
}

fn named(name: &str) -> (Option<String>, Vec<qc_diag::Code>) {
    (Some(name.to_owned()), Vec::new())
}

// spec FTEF-14.75: a listed file is named by its `progs.src` entry, not by the path the
// database holds it by
#[test]
fn a_listed_file_is_named_by_its_entry() {
    let spec = ProjectSpec {
        compiled: "src/a.qc",
        files: &[("src/a.qc", NAMED)],
        ..ProjectSpec::default()
    };
    assert_eq!(bound_name(&spec, Given::Graph), named("src/a.qc"));
    let (by_path, _) = bound_name(&spec, Given::Paths);
    assert!(by_path.is_some_and(|name| name != "src/a.qc"));
    assert_eq!(
        bound_name(&ProjectSpec::default(), Given::Graph),
        named("t.qc")
    );
}

// spec FTEF-14.75: an included file is named by its `#include` joined to the compiled file's
// name, beside it and in a directory a `#pragma includedir` adds
#[test]
fn an_included_file_is_named_from_the_compiled_file() {
    let nested = ProjectSpec {
        compiled: "src/a.qc",
        files: &[
            ("src/a.qc", "#include \"x/b.qh\"\n"),
            ("src/x/b.qh", "#include \"c.qh\"\n"),
            ("src/c.qh", NAMED),
        ],
        ..ProjectSpec::default()
    };
    assert_eq!(bound_name(&nested, Given::Graph), named("src/c.qh"));
    let beside = ProjectSpec {
        files: &[("src/a.qc", "#include \"x/b.qh\"\n"), ("src/x/b.qh", NAMED)],
        ..nested
    };
    assert_eq!(bound_name(&beside, Given::Graph), named("src/x/b.qh"));
    let pragma = ProjectSpec {
        files: &[
            ("src/a.qc", "#pragma includedir y\n#include \"c.qh\"\n"),
            ("src/y/c.qh", NAMED),
        ],
        ..nested
    };
    assert_eq!(bound_name(&pragma, Given::Graph), named("src/y/c.qh"));
    let dotted = ProjectSpec {
        compiled: "src/./a.qc",
        files: &[("src/a.qc", "#include \"b.qh\"\n"), ("src/b.qh", NAMED)],
        ..ProjectSpec::default()
    };
    assert_eq!(bound_name(&dotted, Given::Graph), named("src/./b.qh"));
}

// spec FTEF-14.75: a file found in an include directory is named by the directory as given
// with a `/` after it, which the preprocessor opens in the graph's directory
#[test]
fn a_file_in_an_include_directory_is_named_by_the_directory() {
    for dir in ["inc", "inc/"] {
        let spec = ProjectSpec {
            files: &[("t.qc", "#include \"c.qh\"\n"), ("inc/c.qh", NAMED)],
            include_dirs: &[dir],
            ..ProjectSpec::default()
        };
        assert_eq!(bound_name(&spec, Given::Graph), named("inc/c.qh"), "{dir}");
    }
}

// spec FTEF-14.75: a `\` in a listed name is kept, and an included file's name is joined after
// it, whichever separator the host has
#[test]
fn a_backslash_name_is_kept_and_joined_after() {
    let spec = ProjectSpec {
        compiled: "sub\\top.qc",
        files: &[("sub\\top.qc", NAMED)],
        ..ProjectSpec::default()
    };
    assert_eq!(bound_name(&spec, Given::Graph), named("sub\top.qc"));
    let spec = ProjectSpec {
        compiled: "sub\\top.qc",
        files: &[
            ("sub\\top.qc", "#include \"tab.qh\"\n"),
            ("sub\\tab.qh", NAMED),
        ],
        ..ProjectSpec::default()
    };
    assert_eq!(bound_name(&spec, Given::Graph), named("sub\tab.qh"));
}
