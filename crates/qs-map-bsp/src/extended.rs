// SPDX-License-Identifier: GPL-3.0-or-later

//! The extended dialect's pipeline, ported from ericw-tools' qbsp (ADR-0023,
//! `docs/design/extended-qbsp.md`, QBSP-15): brushes chopped and built into a tree of their own,
//! in double precision. `Compiler` compiles the extended dialect through `process` alone
//! (QBSP-14.10).

pub(crate) mod brush;
pub(crate) mod chop;
pub(crate) mod faces;
pub(crate) mod game;
pub(crate) mod map;
pub(crate) mod output;
pub(crate) mod outside;
pub(crate) mod portals;
pub(crate) mod process;
pub(crate) mod prt;
pub(crate) mod tjunc;
pub(crate) mod tree;
pub(crate) mod vec;
pub(crate) mod winding;
