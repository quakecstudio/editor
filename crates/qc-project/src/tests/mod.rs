// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::panic,
    reason = "test code: a failed index, unwrap or panic is a failed test"
)]

mod case;
mod cond;
mod directives;
mod forms;
mod manifest;
mod open;
mod properties;

use std::path::{Path, PathBuf};

use crate::{Code, Define, FileGraph, MemoryLoader, Via, build_graph};

/// A module to build a graph for: its files, its source, its defines and include
/// directories. By default a list-form `progs.src` naming one empty file.
#[derive(Clone, Debug)]
struct GraphSpec<'a> {
    files: &'a [(&'a str, &'a str)],
    src: &'a str,
    defines: &'a [&'a str],
    include_dirs: &'a [&'a str],
    /// Whether the files are on a file system that ignores case.
    ignore_case: bool,
}

impl Default for GraphSpec<'_> {
    fn default() -> Self {
        Self {
            files: &[("progs.src", "../progs.dat\na.qc\n"), ("a.qc", "")],
            src: "progs.src",
            defines: &[],
            include_dirs: &[],
            ignore_case: false,
        }
    }
}

fn loader(files: &[(&str, &str)]) -> MemoryLoader {
    fill(MemoryLoader::new(), files)
}

fn fill(mut loader: MemoryLoader, files: &[(&str, &str)]) -> MemoryLoader {
    for (path, text) in files {
        loader.insert(*path, text.as_bytes());
    }
    loader
}

fn build(spec: &GraphSpec<'_>) -> FileGraph {
    let empty = if spec.ignore_case {
        MemoryLoader::case_insensitive()
    } else {
        MemoryLoader::new()
    };
    let mut loader = fill(empty, spec.files);
    let defines: Vec<Define> = spec
        .defines
        .iter()
        .map(|d| Define::parse(d).unwrap())
        .collect();
    let dirs: Vec<PathBuf> = spec.include_dirs.iter().map(PathBuf::from).collect();
    build_graph(&mut loader, Path::new(spec.src), &defines, &dirs)
}

/// A path with `/` between its components, so tests read the same on every OS.
fn slash(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// The graph's files, as strings.
fn names(graph: &FileGraph) -> Vec<String> {
    graph.files.iter().map(|f| slash(&f.path)).collect()
}

/// The graph's files with how each came in.
fn vias(graph: &FileGraph) -> Vec<(String, Via)> {
    graph
        .files
        .iter()
        .map(|f| (slash(&f.path), f.via))
        .collect()
}

/// The graph's diagnostics as `code file:line`.
fn diags(graph: &FileGraph) -> Vec<String> {
    graph
        .diagnostics
        .iter()
        .map(|d| format!("{} {}:{}", d.code.as_str(), slash(&d.file), d.line))
        .collect()
}

/// A source-form module whose root is `body`, with the other `files`.
fn source<'a>(body: &'a str, files: &'a [(&'a str, &'a str)]) -> FileGraph {
    let mut all = vec![("main.src", body)];
    all.extend_from_slice(files);
    build(&GraphSpec {
        files: &all,
        src: "main.src",
        ..GraphSpec::default()
    })
}

/// The files a source-form root brings in, without the root.
fn brought(body: &str, files: &[(&str, &str)]) -> Vec<String> {
    names(&source(body, files)).into_iter().skip(1).collect()
}

#[test]
fn every_code_is_an_error() {
    for code in [
        Code::E0401,
        Code::E0402,
        Code::E0406,
        Code::E0407,
        Code::E0408,
    ] {
        assert!(code.is_error());
    }
}
