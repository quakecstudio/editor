// SPDX-License-Identifier: GPL-3.0-or-later

//! The extended dialect's pipeline, ported from ericw-tools' qbsp (ADR-0023,
//! `docs/design/extended-qbsp.md`, QBSP-15): brushes chopped and built into a tree of their own,
//! in double precision. Until every module is ported the extended dialect compiles with the
//! id dialect's pipeline (QBSP-14); the switch is made in `Compiler::compile_read`.

// The pipeline is wired into the compiler when its last module lands (the design doc's "Order
// and proof"); until then its items are reached only by its tests.
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the extended pipeline's modules land before it is called"
    )
)]

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
