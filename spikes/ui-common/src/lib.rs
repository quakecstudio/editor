// SPDX-License-Identifier: GPL-3.0-or-later
//! Framework-free parts of the UI framework spike (P0-10): the document model, the QuakeC
//! highlighter, the scripted input, the frame statistics, the theme and the 3D scene. Both
//! prototypes use this crate, so they differ only in the framework.
//!
//! Not shipped, and outside the main workspace; see `spikes/README.md`.

pub mod a11y;
pub mod document;
pub mod harness;
pub mod lexer;
pub mod project;
pub mod scene;
pub mod script;
pub mod stats;
pub mod theme;
