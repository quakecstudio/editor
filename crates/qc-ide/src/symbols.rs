// SPDX-License-Identifier: GPL-3.0-or-later

//! Symbols: the outline of a file and of the project, every global definition by name and
//! kind (UI-2's Symbols panel; LSP's document and workspace symbols).

use std::collections::BTreeSet;
use std::path::Path;

use qc_project::{Loader, normalize};
use qc_sema::DefKind;

use crate::FileRange;
use crate::host::AnalysisHost;

/// What a symbol is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SymbolKind {
    /// A function (QC-11.8).
    Function,
    /// A field, `.T name` (QC-11.9).
    Field,
    /// A global with an initialiser, which id's `qcc` makes a constant (QC-11.6).
    Constant,
    /// Any other global variable (QC-11.1).
    Global,
}

/// One global definition: its name, what it is, and where its name is first written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    /// The name.
    pub name: String,
    /// What it is.
    pub kind: SymbolKind,
    /// Where its name is first written.
    pub range: FileRange,
}

impl<L: Loader> AnalysisHost<L> {
    /// The global definitions whose first declaration is in `path`, in source order, each
    /// once though several modules compile the file. A class's members and what the compiler
    /// makes are left out.
    pub fn document_symbols(&mut self, path: &Path) -> Vec<Symbol> {
        let path = normalize(path);
        let mut out: Vec<Symbol> = self
            .workspace_symbols()
            .into_iter()
            .filter(|s| s.range.path == path)
            .collect();
        out.sort_by_key(|s| s.range.span.start);
        out
    }

    /// Every file's global definitions, by path and then source order, each once. A vector's
    /// components (QC-11.7), whose names are first written where the vector's is, are left
    /// out with it listed.
    pub fn workspace_symbols(&mut self) -> Vec<Symbol> {
        self.ensure_all();
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for index in 0..self.modules.len() {
            let Some((_, data)) = self.module(index) else {
                continue;
            };
            for def in &data.analysis.index.defs {
                if def.member_of.is_some() || def.generated {
                    continue;
                }
                let kind = match def.kind {
                    DefKind::Function => SymbolKind::Function,
                    DefKind::Field => SymbolKind::Field,
                    DefKind::Global if def.initialized => SymbolKind::Constant,
                    DefKind::Global => SymbolKind::Global,
                    DefKind::Param | DefKind::Local => continue,
                };
                let Some(range) = self.range(index, def.location) else {
                    continue;
                };
                if seen.insert((range.path.clone(), range.span.start)) {
                    out.push(Symbol {
                        name: def.name.clone(),
                        kind,
                        range,
                    });
                }
            }
        }
        out.sort_by(|a, b| {
            (&a.range.path, a.range.span.start).cmp(&(&b.range.path, b.range.span.start))
        });
        out
    }
}
