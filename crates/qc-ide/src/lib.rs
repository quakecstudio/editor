// SPDX-License-Identifier: GPL-3.0-or-later

//! The editor features of QuakeC Studio, over the semantic model and with no protocol.
//!
//! This is the layer rust-analyzer calls `ide`: an [`AnalysisHost`] holds a project's modules,
//! each in its own incremental [`qc_sema::Db`], and the text of the files an editor has open,
//! and answers the questions an editor asks at a byte offset of a file: where a name is
//! defined, where it is used, how to rename it, what to show on hover, what to complete, how
//! to colour each name, which parameter names to show at a call, and what is wrong. The
//! language server (`apps/lsp`) translates these answers to the Language Server Protocol, and
//! the desktop editor will read them in process, so both give the same answers (LANG-7,
//! LANG-9).
//!
//! Every answer is in byte offsets of a file's text and paths, never in lines and columns:
//! positions are the front end's to convert, since the protocol and the editor count
//! columns differently.
//!
//! # Layer
//!
//! L3 semantics. It depends on `qc-sema` and `qc-engines` (L3), `qc-syntax` and `qc-project`
//! (L2) and `qc-diag` (L0).
//!
//! # Spec sections
//!
//! None of its own: it reads the resolution of `docs/spec/language.md` QC-11 and QC-12 (and
//! FTE-22 and FTEF-17 under FTEQCC) through `qc-sema`, the engine tables of
//! `docs/spec/engine-targets.md` through `qc-engines`, and names the statements its choices
//! rest on where it makes them.

mod completion;
mod diagnostics;
mod highlight;
mod hints;
mod host;
mod hover;
mod navigation;
mod rename;
mod syntax;

#[cfg(test)]
mod tests;

use std::path::PathBuf;

pub use completion::Completion;
pub use highlight::{Highlight, HighlightKind, Modifier, Modifiers};
pub use hints::ParameterHint;
pub use host::{AnalysisHost, Config};
pub use hover::{BuiltinInfo, Hover};
pub use qc_diag::{Code, Diagnostic, Severity, Span};
pub use qc_engines::{Builtin, BuiltinRef, Engine};
pub use qc_sema::DefKind;
pub use rename::{Caution, EngineBinding, FileEdit, Rename, RenameError};

/// A span of one file: where a definition or a use is, or a range to edit.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileRange {
    /// The file, as the project graph names it (`qc_project::normalize`d).
    pub path: PathBuf,
    /// The byte span in that file's text.
    pub span: Span,
}
