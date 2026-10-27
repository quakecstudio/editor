// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the semantic model.
//!
//! Each test cites the spec statement it pins (`// spec <ID>`). Fixtures come from the
//! factory [`resolve`], which takes a [`ModuleSpec`] (CONV-11); [`index_of`],
//! [`fte_index_of`] and [`module_index_of`] are its shorthands for one vanilla file, one
//! FTEQCC file and several vanilla files in compilation order. [`offset_of`] finds a cursor
//! position by the n-th occurrence of a name, for the navigation tests.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap, expect, panic or index is a failed test"
)]

mod casts;
mod definitions;
mod dialect;
mod dialect_builtins;
mod dialect_full;
mod dialect_resolution;
mod dialect_statements;
mod emulation;
mod expansion;
mod flags;
mod folding;
mod incremental;
mod initialiser_uses;
mod keywords;
mod member_access;
mod members;
mod names;
mod navigation;
mod operators;
mod passes;
mod properties;
mod recovery;
mod resolution;
mod reviewer;
mod settings;
mod type_declarations;
mod types;

use crate::db::Db;
use crate::resolve::ModuleIndex;
use crate::symbol::FileId;

/// A module for [`resolve`] to resolve: its files' text in compilation order (QC-23.3) and the
/// dialect it is read in. The default is the ordinary case, one empty file in the vanilla
/// dialect, so a test spells out only the axis it is about.
#[derive(Clone, Copy, Debug)]
struct ModuleSpec<'a> {
    files: &'a [&'a str],
    dialect: qc_syntax::Dialect,
}

impl Default for ModuleSpec<'_> {
    fn default() -> Self {
        Self {
            files: &[""],
            dialect: qc_syntax::Dialect::default(),
        }
    }
}

/// The factory every resolution fixture comes from: a fresh database holding `spec`'s files,
/// resolved as one module.
fn resolve(spec: &ModuleSpec<'_>) -> ModuleIndex {
    let mut db = Db::new();
    db.set_dialect(spec.dialect);
    let ids: Vec<FileId> = spec
        .files
        .iter()
        .enumerate()
        .map(|(index, text)| {
            let id = FileId(index);
            db.set_file_text(id, text.as_bytes().to_vec());
            id
        })
        .collect();
    db.module_index(&ids)
}

/// Resolves a single-file module from `source` and returns its index.
fn index_of(source: &str) -> ModuleIndex {
    resolve(&ModuleSpec {
        files: &[source],
        ..ModuleSpec::default()
    })
}

/// Resolves a single-file module from `source` under the FTEQCC dialect.
fn fte_index_of(source: &str) -> ModuleIndex {
    resolve(&ModuleSpec {
        files: &[source],
        dialect: qc_syntax::Dialect::Fteqcc,
    })
}

/// Resolves a module of several files, in the order given (compilation order, QC-23.3).
fn module_index_of(files: &[&str]) -> ModuleIndex {
    resolve(&ModuleSpec {
        files,
        ..ModuleSpec::default()
    })
}

/// The byte offset of the `n`-th occurrence (0-based) of `needle` in `source`.
fn nth_offset(source: &str, needle: &str, n: usize) -> usize {
    source
        .match_indices(needle)
        .nth(n)
        .map(|(index, _)| index)
        .expect("needle occurs enough times")
}
