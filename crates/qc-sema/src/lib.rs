// SPDX-License-Identifier: GPL-3.0-or-later

//! The incremental QuakeC semantic model.
//!
//! This crate turns the lossless syntax trees of `qc-syntax` and the ordered file graph of
//! `qc-project` into a resolved view of a QuakeC module: what each name means, its type,
//! whether it is a field or a global, a prototype or a body, and where it is defined and
//! used. It is the engine behind go-to-definition, find-references and the semantic
//! diagnostics (GOAL-1, LANG-4).
//!
//! The model is *incremental* (the design doc, "Semantic model"): a [`Db`] holds each
//! file's text as an input and memoizes the derived queries built on it, so a keystroke in
//! one file re-parses and re-lowers only that file and re-resolves the module from the mix
//! of fresh and cached per-file results. [`Db::counts`] exposes how many times each query
//! actually ran, so an edit's effect on the query graph is observable.
//!
//! # Semantics
//!
//! Resolution follows id's `qcc` as `docs/spec/language.md` records it, not as other QuakeC
//! tools describe it:
//!
//! - Two scopes, global and the function being defined; blocks open no scope (QC-12.1).
//! - One definition list in declaration order; a name is the first global or current
//!   function definition with that name (QC-12.2).
//! - A local or parameter cannot shadow a global declared before it; it reuses the global
//!   (QC-12.3).
//! - Names resolve at their position in compilation order (QC-12.5, QC-23.3).
//! - A name may be declared repeatedly and initialised once; a second type is a type
//!   mismatch and a second initialiser is a redeclaration (QC-11.5).
//! - Every global function must be defined (QC-11.8).
//! - Fields and functions are global only (QC-11.9).
//! - A state header's next-think name is declared as a global `void()` if new (QC-13.6).
//! - A vector, or a vector field, also declares its `_x`, `_y` and `_z` components (QC-11.7).
//!
//! Under an FTEQCC profile the rules of `docs/spec/dialect-fteqcc-rerelease.md` FTE-22 replace
//! three of these: a local shadows a global, a constant may be initialised twice with the same
//! literal, and with autoprototyping ([`Db::set_autoproto`]) names do not wait for their
//! position.
//!
//! [`Db::analyze_module`] reads a module as the compiler reads it, from each file's
//! preprocessed expansion (QC-9), with every location mapped back to a span of a source file
//! through the expansion origins (QC-9.7); [`Db::parse`] stays the tree of the file as written.
//!
//! # Layer
//!
//! L3 semantics. It depends on `qc-syntax`, `qc-lexer`, `qc-preproc` and `qc-project` (L2) and `qc-diag` (L0, for the
//! one [`qc_diag::Diagnostic`], its [`qc_diag::Code`] and [`qc_diag::Span`]).
//!
//! # Spec sections
//!
//! Implements the semantic parts of QC-10 (types), QC-11 (declarations), QC-12 (scopes and
//! lookup), QC-13.5 (parameter order) and QC-13.6 (state-header names) of
//! `docs/spec/language.md`, and FTEF-19 (definitions as FTEQCC makes them) of
//! `docs/spec/dialect-fteqcc.md`, and reports the diagnostics of
//! `docs/spec/language-diagnostics.md` those name. The definitions are ported from id's
//! `PR_ParseDefs` and `PR_GetDef` and FTEQCC's `QCC_PR_ParseDefs` and `QCC_PR_GetDef`
//! (`docs/spec/inventory-l3.md`).

pub mod cast;
mod check;
pub mod db;
pub mod expand;
pub mod lower;
pub mod nav;
pub mod resolve;
pub mod symbol;
pub mod ty;

#[cfg(test)]
mod tests;

pub use check::TypedBody;
pub use db::{Db, ModuleAnalysis, QueryCounts};
pub use expand::{ModuleInput, OriginMap};
pub use lower::{
    BuiltinFunction, FileDecls, LiteralValue, RawBody, RawDecl, RawLocal, RawParam, RawUse, UseKind,
};
pub use resolve::{FileDiagnostic, ModuleIndex, ResolveOptions, resolve_module};
pub use symbol::{DefId, DefKind, Definition, FileId, Location, Reference};
pub use ty::{FunctionType, SimpleType, Type};
