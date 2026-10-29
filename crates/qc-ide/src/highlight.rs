// SPDX-License-Identifier: GPL-3.0-or-later

//! Semantic highlighting: what each name written in a file is.
//!
//! Every definition, later declaration and use the semantic model resolved in the file is
//! classified by what it names (QC-12.1: a global, function or field, or a parameter or local
//! of one function). A location whose text is not the name, the use of a macro the name came
//! from (QC-9.7), is left to the editor's own colouring.

use std::path::Path;

use qc_diag::Span;
use qc_project::Loader;
use qc_sema::{DefKind, Definition};

use crate::host::{AnalysisHost, ModuleData};
use crate::syntax::name_at;

/// What a highlighted name is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HighlightKind {
    /// A file-level function (QC-11.8).
    Function,
    /// A field, `.T name` (QC-11.9).
    Field,
    /// A file-level variable or constant (QC-11.1).
    Global,
    /// A function parameter (QC-13.2).
    Parameter,
    /// A `local` (QC-11.10).
    Local,
}

impl HighlightKind {
    /// The kind of a name of a definition of `kind`.
    #[must_use]
    pub const fn of(kind: DefKind) -> Self {
        match kind {
            DefKind::Function => Self::Function,
            DefKind::Field => Self::Field,
            DefKind::Global => Self::Global,
            DefKind::Param => Self::Parameter,
            DefKind::Local => Self::Local,
        }
    }
}

/// One property a highlighted name may have.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Modifier {
    /// The name is declared here, not used.
    Declaration,
    /// A constant: under the vanilla profile an initialised global (QC-11.6).
    Readonly,
    /// A builtin the engine provides, declared `= #N` or by engine name (QC-13.3, ENGT-4.12).
    DefaultLibrary,
}

impl Modifier {
    /// Every modifier, in the order of their bits.
    pub const ALL: [Self; 3] = [Self::Declaration, Self::Readonly, Self::DefaultLibrary];

    /// The modifier's bit in a [`Modifiers`] set.
    const fn bit(self) -> u8 {
        match self {
            Self::Declaration => 1,
            Self::Readonly => 2,
            Self::DefaultLibrary => 4,
        }
    }
}

/// A set of [`Modifier`]s.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Modifiers(u8);

impl Modifiers {
    /// The set with `modifier` added.
    #[must_use]
    pub const fn with(self, modifier: Modifier) -> Self {
        Self(self.0 | modifier.bit())
    }

    /// Whether the set holds `modifier`.
    #[must_use]
    pub const fn contains(self, modifier: Modifier) -> bool {
        self.0 & modifier.bit() != 0
    }

    /// The modifiers in the set, in [`Modifier::ALL`] order.
    pub fn iter(self) -> impl Iterator<Item = Modifier> {
        Modifier::ALL.into_iter().filter(move |m| self.contains(*m))
    }
}

/// One highlighted name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Highlight {
    /// The name's span in the file.
    pub span: Span,
    /// What it names.
    pub kind: HighlightKind,
    /// Its properties.
    pub modifiers: Modifiers,
}

impl<L: Loader> AnalysisHost<L> {
    /// Every name of `path` the semantic model resolved, in source order, each once; read in
    /// the first module that compiles the file.
    pub fn highlights(&mut self, path: &Path) -> Vec<Highlight> {
        let Some(&(module, file)) = self.modules_of(path).first() else {
            return Vec::new();
        };
        let Some((state, data)) = self.module(module) else {
            return Vec::new();
        };
        let text = state.db.file_text(file);
        let vanilla = !state.dialect.has_fteqcc_extensions();
        let index = &data.analysis.index;
        let mut out = Vec::new();
        let declarations = index.defs.iter().map(|d| (d.location, d)).chain(
            index
                .redeclarations
                .iter()
                .filter_map(|r| index.def(r.def).map(|d| (r.location, d))),
        );
        for (location, def) in declarations {
            if location.file == file && name_at(text, location.span, &def.name) {
                let modifiers = modifiers(def, data, vanilla).with(Modifier::Declaration);
                out.push(highlight(location.span, def, modifiers));
            }
        }
        for reference in &index.refs {
            let Some(def) = index.def(reference.def) else {
                continue;
            };
            let location = reference.location;
            if location.file == file && name_at(text, location.span, &def.name) {
                out.push(highlight(location.span, def, modifiers(def, data, vanilla)));
            }
        }
        // A declaration and a use at one span (a local that reuses a global, QC-12.3) is
        // highlighted as the declaration, which comes first.
        out.sort_by_key(|h| h.span.start);
        out.dedup_by_key(|h| h.span);
        out
    }
}

/// The highlight of `def`'s name at `span`.
const fn highlight(span: Span, def: &Definition, modifiers: Modifiers) -> Highlight {
    Highlight {
        span,
        kind: HighlightKind::of(def.kind),
        modifiers,
    }
}

/// The modifiers every name of `def` has.
fn modifiers(def: &Definition, data: &ModuleData, vanilla: bool) -> Modifiers {
    let mut out = Modifiers::default();
    // QC-11.6: under the vanilla profile an initialised global is a constant. FTEQCC's `var`
    // (FTE-11.1) makes one variable, which the definition does not record, so no FTEQCC global
    // is marked.
    if vanilla && def.kind == DefKind::Global && def.initialized {
        out = out.with(Modifier::Readonly);
    }
    if def.kind == DefKind::Function && data.builtins.contains_key(&def.name) {
        out = out.with(Modifier::DefaultLibrary);
    }
    out
}
