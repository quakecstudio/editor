// SPDX-License-Identifier: GPL-3.0-or-later

//! Reads and writes `.map` files, the text a level editor saves and a BSP compiler reads
//! (`docs/spec/map.md`, MAP-1 to MAP-7).
//!
//! - [`MapFile`] is the lossless document: [`MapFile::parse`] keeps every byte, comments and
//!   formatting included, so [`MapFile::to_bytes`] gives back the file it read, and an edit
//!   changes only the text it touches. It reads both the standard and the Valve 220 forms,
//!   and [`MapFile::check`] reports what id's qbsp, ericw-tools or the game would do with
//!   it otherwise.
//! - [`qbsp`] models what id's qbsp reads: its errors, planes, texinfo, entity lump and
//!   texture slots.
//!
//! The entity lump of a BSP is the same syntax without brushes (BSP-2), so [`MapFile`] also
//! reads and edits a BSP's entities or a `.ent` file, for maps with no `.map` source.
//!
//! Nothing is ported: id's qbsp is not a portable upstream (`docs/porting.md`), so the code
//! is written from the spec, which was checked by running qbsp on its vectors.

mod check;
pub mod cnum;
mod document;
pub mod geometry;
pub mod lexer;
pub mod qbsp;

pub use check::{Diagnostic, DiagnosticKind, MAP_MAX_SOUNDS};
pub use document::{
    Brush, Compiler, Entity, Face, Item, MapError, MapFile, Pair, TexDef, is_tool_key,
};
