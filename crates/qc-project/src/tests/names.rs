// SPDX-License-Identifier: GPL-3.0-or-later

//! Each graph file's name, FTEQCC's, and the file FTEQCC opens for it (PRJ-6.7, PRJ-6.8).

use std::path::{Path, PathBuf};

use super::{GraphSpec, build, diags, names};
use crate::FileGraph;

/// The graph's names for its files.
fn given(graph: &FileGraph) -> Vec<&str> {
    graph.files.iter().map(|f| f.name.as_str()).collect()
}

/// An FTEQCC module of `files` whose `progs.src`, in `qc/`, lists `compiled`.
fn fteqcc(compiled: &str, files: &[(&str, &str)], include_dirs: &[&str]) -> FileGraph {
    let list = format!("progs.dat\n{compiled}\n");
    let mut all = vec![("qc/progs.src".to_owned(), list)];
    all.extend(
        files
            .iter()
            .map(|(name, text)| (format!("qc/{name}"), (*text).to_owned())),
    );
    let all: Vec<(&str, &str)> = all.iter().map(|(n, t)| (n.as_str(), t.as_str())).collect();
    build(&GraphSpec {
        files: &all,
        src: "qc/progs.src",
        include_dirs,
        dialect: qc_lexer::Dialect::Fteqcc,
        ..GraphSpec::default()
    })
}

// spec PRJ-6.7: a list entry is named as `progs.src` writes it, an `#include` joined to the name
// of the file being compiled, and the names are relative to the source's directory
#[test]
fn names_are_joined_to_the_compiled_file() {
    let graph = fteqcc(
        "src/a.qc",
        &[
            ("src/a.qc", "#include \"x/b.qh\"\n"),
            ("src/x/b.qh", "#include \"c.qh\"\n"),
            ("src/c.qh", ""),
        ],
        &[],
    );
    assert_eq!(given(&graph), ["src/a.qc", "src/x/b.qh", "src/c.qh"]);
    assert_eq!(
        names(&graph),
        ["qc/src/a.qc", "qc/src/x/b.qh", "qc/src/c.qh"]
    );
    assert_eq!(graph.dir, PathBuf::from("qc"));
}

// spec PRJ-6.7: the source is named by its file name, and a name keeps a `./` its path drops
#[test]
fn the_source_and_dotted_names() {
    let graph = build(&GraphSpec {
        files: &[
            ("qc/main.src", "#includelist\n./a.qc\n#endlist\n"),
            ("qc/a.qc", ""),
        ],
        src: "qc/main.src",
        dialect: qc_lexer::Dialect::Fteqcc,
        ..GraphSpec::default()
    });
    assert_eq!(given(&graph), ["main.src", "./a.qc"]);
    assert_eq!(names(&graph), ["qc/main.src", "qc/a.qc"]);
    let graph = fteqcc("src/./a.qc", &[("src/a.qc", "")], &[]);
    assert_eq!(given(&graph), ["src/./a.qc"]);
}

// spec PRJ-6.7: a file found in an include directory is named by the directory as given with a
// `/` after it, one a `#pragma includedir` adds by its name joined to the compiled file's
#[test]
fn include_directories_are_named_as_given() {
    for dir in ["inc", "inc/"] {
        let graph = fteqcc(
            "t.qc",
            &[("t.qc", "#include \"c.qh\"\n"), ("inc/c.qh", "")],
            &[dir],
        );
        assert_eq!(given(&graph), ["t.qc", "inc/c.qh"], "{dir}");
        assert_eq!(names(&graph), ["qc/t.qc", "qc/inc/c.qh"], "{dir}");
    }
    let graph = fteqcc(
        "src/a.qc",
        &[
            ("src/a.qc", "#pragma includedir y\n#include \"c.qh\"\n"),
            ("src/y/c.qh", ""),
        ],
        &[],
    );
    assert_eq!(given(&graph), ["src/a.qc", "src/y/c.qh"]);
}

// spec PRJ-6.8: FTEQCC opens a name in the source's directory, so an include directory given
// relative is in it
#[test]
fn a_relative_include_directory_is_in_the_source_directory() {
    let graph = fteqcc(
        "t.qc",
        &[("t.qc", "#include \"c.qh\"\n"), ("inc/c.qh", "")],
        &["inc"],
    );
    assert!(diags(&graph).is_empty(), "{:?}", diags(&graph));
    let vanilla = build(&GraphSpec {
        files: &[
            ("qc/progs.src", "progs.dat\nt.qc\n"),
            ("qc/t.qc", "#include \"c.qh\"\n"),
            ("inc/c.qh", ""),
        ],
        src: "qc/progs.src",
        include_dirs: &["inc"],
        ..GraphSpec::default()
    });
    assert_eq!(names(&vanilla), ["qc/t.qc", "inc/c.qh"]);
}

// spec PRJ-6.7: a `\` in a name is kept, and a name is joined after it; PRJ-6.8: the file opened
// is the name's, in which `\` separates only where the host's separator is
#[test]
fn backslash_names_are_joined_after_the_backslash() {
    let graph = fteqcc(
        "sub\\top.qc",
        &[
            ("sub\\top.qc", "#include \"tab.qh\"\n"),
            ("sub\\tab.qh", ""),
        ],
        &[],
    );
    assert_eq!(given(&graph), ["sub\\top.qc", "sub\\tab.qh"]);
    assert!(diags(&graph).is_empty(), "{:?}", diags(&graph));
    assert_eq!(
        graph.files[1].path,
        Path::new("qc").join("sub\\tab.qh"),
        "the file of the name"
    );
}

// spec PRJ-6.8: under FTEQCC `#pragma once` marks the compiled file's name, so a name that
// differs, as `./a.qc` does from `a.qc`, is read again; the other dialects mark the file
#[test]
fn fteqcc_once_compares_names() {
    let files = [
        ("progs.src", "progs.dat\na.qc\n./a.qc\na.qc\n"),
        ("a.qc", "#pragma once\n"),
    ];
    let graph = build(&GraphSpec {
        files: &files,
        dialect: qc_lexer::Dialect::Fteqcc,
        ..GraphSpec::default()
    });
    assert_eq!(given(&graph), ["a.qc", "./a.qc"]);
    let graph = build(&GraphSpec {
        files: &files,
        ..GraphSpec::default()
    });
    assert_eq!(given(&graph), ["a.qc"]);
}

// spec PRJ-6.7: the vanilla dialect names its files too, joined as FTEQCC joins them
#[test]
fn vanilla_names_are_joined_too() {
    let graph = build(&GraphSpec {
        files: &[
            ("qc/progs.src", "progs.dat\nsrc/a.qc\n"),
            ("qc/src/a.qc", "#include \"x/b.qh\"\n"),
            ("qc/src/x/b.qh", ""),
        ],
        src: "qc/progs.src",
        ..GraphSpec::default()
    });
    assert_eq!(given(&graph), ["src/a.qc", "src/x/b.qh"]);
}
