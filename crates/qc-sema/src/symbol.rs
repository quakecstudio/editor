// SPDX-License-Identifier: GPL-3.0-or-later

//! Symbols: the definitions a module declares and the references to them.
//!
//! A [`Definition`] is one name `qcc` adds to its single definition list (QC-12.2): a
//! global, a function, a field, a parameter or a local. A [`Reference`] is one use of a
//! name in a value position that resolves to a definition. Both carry a [`Location`] so the
//! editor can jump to them (go-to-definition and find-references, GOAL-1).

use qc_lexer::Span;

use crate::ty::Type;

/// Where in the project a symbol or reference is: which file, and the byte span of its name.
///
/// The file is the index of a file in the module's ordered graph
/// (`qc_project::Module::graph`), so a location is comparable and cheap to copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Location {
    /// The file's index in the module's file graph.
    pub file: FileId,
    /// The byte span of the name token within that file.
    pub span: Span,
}

/// A file's index in a module's ordered file graph (`qc_project::Module::graph`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId(pub usize);

/// A stable handle to a [`Definition`] within one module's resolved index.
///
/// It is the definition's position in the module's definition list, which is built in
/// declaration order (QC-12.2), so a smaller id was declared earlier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DefId(pub usize);

/// What a definition is, in the two scopes `qcc` has (QC-12.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DefKind {
    /// A file-level variable or constant (QC-11.1).
    Global,
    /// A file-level function (QC-11.8: a global of function type is always a function).
    Function,
    /// A file-level field, `.T name` (QC-11.9: fields are global).
    Field,
    /// A function parameter, visible only inside that function (QC-12.1, QC-13.2).
    Param,
    /// A `local` inside a function body, visible to the end of the function (QC-11.10,
    /// QC-12.1).
    Local,
}

impl DefKind {
    /// Whether a definition of this kind belongs to the global scope (QC-12.1).
    #[must_use]
    pub fn is_global(self) -> bool {
        matches!(self, Self::Global | Self::Function | Self::Field)
    }
}

/// One definition in a module's definition list (QC-12.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Definition {
    /// The declared name.
    pub name: String,
    /// Its type (QC-10). A redeclaration keeps the first type (QC-11.5).
    pub ty: Type,
    /// What it is.
    pub kind: DefKind,
    /// Where its name is first written: the declaration go-to-definition jumps to.
    pub location: Location,
    /// For a local or parameter, the [`DefId`] of the function it belongs to (QC-12.1);
    /// `None` for a global, function or field.
    pub owner: Option<DefId>,
    /// Whether an initialiser has been seen (QC-11.5): a function body, a builtin number or
    /// a constant value. A second initialiser is an error, "NAME redeclared" (QCD-3.6).
    pub initialized: bool,
}

/// One use of a name that resolves to a definition (QC-12.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reference {
    /// Where the name is used.
    pub location: Location,
    /// The definition it resolves to.
    pub def: DefId,
}
