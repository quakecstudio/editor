// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the semantic model.
//!
//! Each test cites the spec statement it pins (`// spec <ID>`). Fixtures come from the
//! factories below: [`index_of`] resolves a one-file module, and [`module_index_of`]
//! resolves several files in compilation order. [`offset_of`] finds a cursor position by
//! the n-th occurrence of a name, for the navigation tests.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap, expect, panic or index is a failed test"
)]

mod dialect;
mod dialect_full;
mod dialect_resolution;
mod dialect_statements;
mod expansion;
mod incremental;
mod initialiser_uses;
mod navigation;
mod resolution;
mod reviewer;
mod types;

use crate::db::Db;
use crate::resolve::ModuleIndex;
use crate::symbol::FileId;

/// Resolves a single-file module from `source` and returns its index.
fn index_of(source: &str) -> ModuleIndex {
    module_index_of(&[source])
}

/// Resolves a single-file module from `source` under the FTEQCC dialect.
fn fte_index_of(source: &str) -> ModuleIndex {
    let mut db = Db::new();
    db.set_dialect(qc_syntax::Dialect::Fteqcc);
    let id = FileId(0);
    db.set_file_text(id, source.as_bytes().to_vec());
    db.module_index(&[id])
}

/// Resolves a module of several files, in the order given (compilation order, QC-23.3).
fn module_index_of(files: &[&str]) -> ModuleIndex {
    let mut db = Db::new();
    let ids: Vec<FileId> = files
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

/// The byte offset of the `n`-th occurrence (0-based) of `needle` in `source`.
fn nth_offset(source: &str, needle: &str, n: usize) -> usize {
    source
        .match_indices(needle)
        .nth(n)
        .map(|(index, _)| index)
        .expect("needle occurs enough times")
}
