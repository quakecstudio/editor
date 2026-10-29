// SPDX-License-Identifier: GPL-3.0-or-later

//! Navigation: go-to-definition and find-references (GOAL-1).
//!
//! These read a resolved [`ModuleIndex`] (`crate::resolve`). Go-to-definition maps a byte
//! position in a file to the definition whose name, use or later declaration covers it;
//! find-references lists every use of a definition, and optionally its declarations. The
//! answers follow `qcc`'s resolution exactly (QC-12), so a name that reuses a global (QC-12.3)
//! navigates to that global, and a local of one function is never confused with a same-named
//! local of another (QC-12.4).

use crate::resolve::ModuleIndex;
use crate::symbol::{DefId, FileId, Location};

impl ModuleIndex {
    /// The definition at byte position `offset` in file `file`, if any (go-to-definition).
    ///
    /// A position inside a name use jumps to the definition it resolves to; a position
    /// inside a definition's own name resolves to that definition. Returns its [`DefId`] and
    /// the [`Location`] to jump to (the definition's name).
    #[must_use]
    pub fn definition_at(&self, file: FileId, offset: usize) -> Option<(DefId, Location)> {
        // A use, or a declaration after the first (QC-11.5), at this position names its
        // definition.
        if let Some(reference) = self
            .refs
            .iter()
            .chain(&self.redeclarations)
            .find(|r| r.location.file == file && covers(r.location.span, offset))
        {
            let def = self.def(reference.def)?;
            return Some((reference.def, def.location));
        }
        // Otherwise, a definition whose own name is at this position.
        self.defs
            .iter()
            .enumerate()
            .find(|(_, d)| d.location.file == file && covers(d.location.span, offset))
            .map(|(index, d)| (DefId(index), d.location))
    }

    /// Every reference to definition `def`, in source order (find-references).
    ///
    /// When `include_declaration` is true, the definition's own name location is first, then
    /// every later declaration of it (QC-11.5) in declaration order, then the uses.
    #[must_use]
    pub fn references(&self, def: DefId, include_declaration: bool) -> Vec<Location> {
        let mut out = Vec::new();
        if include_declaration && let Some(definition) = self.def(def) {
            out.push(definition.location);
            out.extend(
                self.redeclarations
                    .iter()
                    .filter(|r| r.def == def)
                    .map(|r| r.location),
            );
        }
        out.extend(
            self.refs
                .iter()
                .filter(|r| r.def == def)
                .map(|r| r.location),
        );
        out
    }

    /// The definition at byte position `offset`, then every reference to it (the editor's
    /// combined "find all references" from a cursor).
    #[must_use]
    pub fn references_at(
        &self,
        file: FileId,
        offset: usize,
        include_declaration: bool,
    ) -> Vec<Location> {
        match self.definition_at(file, offset) {
            Some((def, _)) => self.references(def, include_declaration),
            None => Vec::new(),
        }
    }
}

/// Whether a span covers a byte position: `[start, end)`, or exactly at `start` for an empty
/// span so a cursor at the first byte of a zero-width name still hits.
fn covers(span: qc_diag::Span, offset: usize) -> bool {
    if span.start == span.end {
        offset == span.start
    } else {
        offset >= span.start && offset < span.end
    }
}
