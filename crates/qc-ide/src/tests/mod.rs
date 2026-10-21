// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests of the editor features.
//!
//! Each test cites the statement it pins (`// spec <ID>`). Fixtures come from the factory
//! [`host`], which takes a [`ProjectSpec`] (CONV-11) whose default is the ordinary case: a
//! vanilla project in `/p` whose `progs.src` lists its files in order.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap, expect or index is a failed test"
)]

mod completion;
mod diagnostics;
mod highlight;
mod hints;
mod host;
mod hover;
mod navigation;
mod rename;

use std::path::{Path, PathBuf};

use qc_project::{Dialect, MemoryLoader, Target};

use crate::{AnalysisHost, Config};

/// The project directory every fixture lives in.
const ROOT: &str = "/p";

/// A project for [`host`]: its QuakeC files in compilation order, any other files as they are
/// (a `quakec.toml` or another `.src`), and the configuration.
#[derive(Clone, Copy, Debug)]
struct ProjectSpec<'a> {
    /// `(name, text)` of each QuakeC file `progs.src` lists, in order.
    files: &'a [(&'a str, &'a str)],
    /// `(name, text)` of other files, written as they are; a `progs.src` here replaces the
    /// one made from `files`.
    extra: &'a [(&'a str, &'a str)],
    /// The dialect every module is read in.
    dialect: Option<Dialect>,
    /// The engine every module targets.
    target: Option<Target>,
}

impl Default for ProjectSpec<'_> {
    fn default() -> Self {
        Self {
            files: &[("a.qc", "")],
            extra: &[],
            dialect: None,
            target: None,
        }
    }
}

/// The path of file `name` of the fixture project.
fn path(name: &str) -> PathBuf {
    Path::new(ROOT).join(name)
}

/// The factory every fixture comes from: a host with `spec`'s project open.
fn host(spec: &ProjectSpec<'_>) -> AnalysisHost<MemoryLoader> {
    let mut loader = MemoryLoader::new();
    let mut src = String::from("../progs.dat\n");
    for (name, text) in spec.files {
        src.push_str(name);
        src.push('\n');
        loader.insert(path(name), text.as_bytes().to_vec());
    }
    loader.insert(path("progs.src"), src.into_bytes());
    for (name, text) in spec.extra {
        loader.insert(path(name), text.as_bytes().to_vec());
    }
    let config = Config {
        dialect: spec.dialect,
        target: spec.target,
        autoproto: false,
    };
    let mut host = AnalysisHost::new(loader, config);
    host.open(Path::new(ROOT)).expect("the project opens");
    host
}

/// A one-file vanilla project of `text`, as `a.qc`.
fn one(text: &str) -> AnalysisHost<MemoryLoader> {
    host(&ProjectSpec {
        files: &[("a.qc", text)],
        ..ProjectSpec::default()
    })
}

/// The byte offset of the `n`-th occurrence of `needle` in `text`, from 0.
fn nth(text: &str, needle: &str, n: usize) -> usize {
    text.match_indices(needle)
        .nth(n)
        .map(|(at, _)| at)
        .expect("the occurrence exists")
}
