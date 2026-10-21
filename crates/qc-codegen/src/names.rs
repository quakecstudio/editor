// SPDX-License-Identifier: GPL-3.0-or-later

//! Which definition each name token of the compiled text is, as the semantic model resolved it.
//!
//! The compiler reads each file's expansion (QC-9.16), the text the parser and the semantic
//! model read. The module is resolved once more with no origin maps, so every definition and
//! reference is at a span of that text, and each name token is found by its own span. A
//! definition's name resolves to the definition made there; a vector's components are made at
//! the vector's name, after the vector, so the first definition at a span is the one written
//! there (QC-11.7). A name with no definition at its span resolves to the definition the
//! reference there names: a use (QC-12.5), a redeclaration (QC-11.5), a local or parameter
//! that is a global (QC-12.3).

use std::collections::HashMap;

use qc_diag::Span;
use qc_sema::{DefId, FileId, Location, ModuleIndex};

/// The definition at each name token's span.
#[derive(Debug, Default)]
pub(crate) struct Names {
    at: HashMap<Location, DefId>,
    globals: HashMap<String, DefId>,
}

impl Names {
    /// The names of `index`, a module resolved from its files' expansions.
    pub(crate) fn new(index: &ModuleIndex) -> Self {
        let mut at = HashMap::new();
        let mut globals = HashMap::new();
        for (id, def) in index.defs.iter().enumerate() {
            at.entry(def.location).or_insert(DefId(id));
            if def.kind.is_global() && def.owner.is_none() && def.member_of.is_none() {
                globals.entry(def.name.clone()).or_insert(DefId(id));
            }
        }
        for reference in &index.refs {
            at.entry(reference.location).or_insert(reference.def);
        }
        Self { at, globals }
    }

    /// The definition the name at `span` of compiled file `file` is.
    pub(crate) fn def(&self, file: FileId, span: Span) -> Option<DefId> {
        self.at.get(&Location { file, span }).copied()
    }

    /// The first global definition named `name` (QC-12.2): what a file-level declaration of a
    /// name already declared is, `PR_GetDef` with no scope (QC-11.5). The semantic model
    /// records no reference at such a declaration's name.
    pub(crate) fn global(&self, name: &str) -> Option<DefId> {
        self.globals.get(name).copied()
    }
}
