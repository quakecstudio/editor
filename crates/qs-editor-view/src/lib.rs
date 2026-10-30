// SPDX-License-Identifier: GPL-3.0-or-later

//! The code editor widget (P2-27, ADR-0003): the spike's editor grown into a shipped egui crate.
//!
//! - [`buffer`]: the text as lines, each with a [`LineId`] an edit renews, so the layout cache
//!   relays out only the edited lines.
//! - [`selection`]: selections and multiple carets, the movements and the edits made at each.
//! - [`layout`]: the [`Styler`] hook that colours lines and the [`LayoutCache`].
//! - [`view`]: [`EditorView`], which draws only the visible lines, turns egui's keyboard,
//!   pointer, clipboard and IME events into edits, places the IME candidate window at the
//!   primary caret and publishes AccessKit text runs with the caret and selection.
//!
//! # Layer
//!
//! L7 UI. It depends on `egui` alone; the app gives it its fonts and colours.

pub mod buffer;
pub mod layout;
pub mod selection;
pub mod view;

pub use buffer::{Buffer, LineId, Position};
pub use layout::{LayoutCache, Plain, StyledSpan, Styler};
pub use selection::{EditState, Extend, Motion, Selection};
pub use view::{EditorView, ViewStyle};

#[cfg(test)]
mod tests;
