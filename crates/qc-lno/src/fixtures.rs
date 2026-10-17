// SPDX-License-Identifier: GPL-3.0-or-later

//! Factories for `.lno` test fixtures: an [`LnoSpec`] whose `Default` is the ordinary case
//! (the counts the spec's test vectors use and a two-statement line table), and helpers to
//! build the header bytes the vectors are written against.
//!
//! The ordinary counts are the ones `docs/spec/lno.md` uses throughout its test vectors:
//! `numglobaldefs` 3, `numglobals` 30, `numfielddefs` 1.

use crate::{LNO_MAGIC, LNO_VERSION, LnoFile};

/// The ordinary `numglobaldefs` of the spec's test vectors.
pub(crate) const DEFAULT_NUMGLOBALDEFS: u32 = 3;
/// The ordinary `numglobals` of the spec's test vectors.
pub(crate) const DEFAULT_NUMGLOBALS: u32 = 30;
/// The ordinary `numfielddefs` of the spec's test vectors.
pub(crate) const DEFAULT_NUMFIELDDEFS: u32 = 1;

/// A description of a `.lno` file for a test. `Default` is the ordinary case: the spec's
/// vector counts and the two-statement line table `[0, 5]` with no columns and no trailing
/// bytes. A test spells out only the axis it is about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LnoSpec {
    /// The progs header's `numglobaldefs`.
    pub(crate) numglobaldefs: u32,
    /// The progs header's `numglobals`.
    pub(crate) numglobals: u32,
    /// The progs header's `numfielddefs`.
    pub(crate) numfielddefs: u32,
    /// The line table.
    pub(crate) lines: Vec<i32>,
    /// The column table, or `None` for FTEQCC's layout.
    pub(crate) columns: Option<Vec<i32>>,
    /// Trailing bytes after the tables.
    pub(crate) trailing: Vec<u8>,
}

impl Default for LnoSpec {
    fn default() -> Self {
        Self {
            numglobaldefs: DEFAULT_NUMGLOBALDEFS,
            numglobals: DEFAULT_NUMGLOBALS,
            numfielddefs: DEFAULT_NUMFIELDDEFS,
            lines: vec![0, 5],
            columns: None,
            trailing: Vec::new(),
        }
    }
}

impl From<LnoSpec> for LnoFile {
    fn from(spec: LnoSpec) -> Self {
        Self {
            numglobaldefs: spec.numglobaldefs,
            numglobals: spec.numglobals,
            numfielddefs: spec.numfielddefs,
            lines: spec.lines,
            columns: spec.columns,
            trailing: spec.trailing,
        }
    }
}

/// The 24-byte header bytes for the spec's ordinary counts and the given `numstatements`,
/// `H(n)` in `docs/spec/lno.md`: `LNOF`, version 1, then the four counts.
pub(crate) fn header(numstatements: u32) -> Vec<u8> {
    header_with(
        DEFAULT_NUMGLOBALDEFS,
        DEFAULT_NUMGLOBALS,
        DEFAULT_NUMFIELDDEFS,
        numstatements,
    )
}

/// The 24-byte header bytes for the given counts: `LNOF`, version 1, then the four counts.
pub(crate) fn header_with(
    numglobaldefs: u32,
    numglobals: u32,
    numfielddefs: u32,
    numstatements: u32,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(24);
    out.extend_from_slice(&LNO_MAGIC);
    out.extend_from_slice(&LNO_VERSION.to_le_bytes());
    out.extend_from_slice(&numglobaldefs.to_le_bytes());
    out.extend_from_slice(&numglobals.to_le_bytes());
    out.extend_from_slice(&numfielddefs.to_le_bytes());
    out.extend_from_slice(&numstatements.to_le_bytes());
    out
}

/// Appends the little-endian bytes of each `i32` in `values` to `bytes`, as a line or column
/// table.
pub(crate) fn push_i32_table(bytes: &mut Vec<u8>, values: &[i32]) {
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}
