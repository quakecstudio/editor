// SPDX-License-Identifier: GPL-3.0-or-later

//! The visibility compiler (P6-05, `docs/spec/vis.md`): from the portal file qs-map-bsp writes
//! (QBSP-12), each leaf's potentially visible set, compressed into the BSP's visibility lump,
//! and each leaf's ambient sound levels, as id's `vis` finds them.
//!
//! [`Vis::run`] takes the BSP and its portal file and gives the BSP with its visibility. The
//! fast mode is vis's `-fast`: each portal sees the leaves a flood through facing portals
//! reaches (VIS-3). The full mode runs the portal flow at a test level (VIS-4), on every
//! thread the machine has, in batches that give the same result on any of them (VIS-4.7).
//!
//! Nothing is ported: id's vis is not a portable upstream (`docs/porting.md`), so the code is
//! written from the spec, which was written from vis's source, read and run as an oracle.

mod flow;
mod lump;
mod portals;
mod winding;

use core::fmt;

use qs_bsp::{Bsp, BspLoader};

use crate::flow::{BYTE_BITS, Flow, Leaves};
use crate::portals::PortalFile;

pub use crate::flow::{VIS_FLOW_BATCH, VisCounts};
pub use crate::lump::VIS_MAX_MAP_VISIBILITY;

/// The full mode's test level: how many separating-plane clips each step of the flow makes
/// (VIS-4.4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// No separating planes.
    Zero,
    /// Planes from the source to the pass clip the target.
    One,
    /// Also those from the pass to the source, flipped: vis's default.
    #[default]
    Two,
    /// Also the target's planes clip the source.
    Three,
    /// Every clip.
    Four,
}

/// How much work vis does (VIS-1.3, VIS-1.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Each portal sees the leaves it might see: quick and loose.
    Fast,
    /// The portal flow at a test level.
    Full(Level),
}

impl Default for Mode {
    fn default() -> Self {
        Self::Full(Level::default())
    }
}

/// What stops vis, as its errors read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VisError {
    /// `LoadPortals: failed to read header` (VIS-2.1).
    Header,
    /// `LoadPortals: not a portal file` (VIS-2.1).
    NotPortalFile,
    /// `LoadPortals: reading portal <n>` (VIS-2.2).
    Portal {
        /// The portal's place in the file.
        number: usize,
    },
    /// `LoadPortals: portal <n> has too many points` (VIS-2.2).
    TooManyPoints {
        /// The portal's place in the file.
        number: usize,
    },
    /// `Leaf with too many portals` (VIS-2.4).
    TooManyLeafPortals,
    /// `CheckStack: leaf recursion` (VIS-4.5).
    LeafRecursion,
    /// `Leaf portals saw into leaf` (VIS-4.6).
    SawIntoLeaf {
        /// The leaf.
        leaf: usize,
    },
    /// `Vismap expansion overflow` (VIS-5.2).
    VismapOverflow,
    /// The BSP cannot be read, has fewer leaves than the portal file, or is a format vis does
    /// not write.
    Format,
    /// A flow's thread could not be run.
    Thread,
}

impl fmt::Display for VisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Header => f.write_str("LoadPortals: failed to read header"),
            Self::NotPortalFile => f.write_str("LoadPortals: not a portal file"),
            Self::Portal { number } => write!(f, "LoadPortals: reading portal {number}"),
            Self::TooManyPoints { number } => {
                write!(f, "LoadPortals: portal {number} has too many points")
            }
            Self::TooManyLeafPortals => f.write_str("Leaf with too many portals"),
            Self::LeafRecursion => f.write_str("CheckStack: leaf recursion"),
            Self::SawIntoLeaf { leaf } => write!(f, "Leaf portals saw into leaf {leaf}"),
            Self::VismapOverflow => f.write_str("Vismap expansion overflow"),
            Self::Format => f.write_str("the BSP cannot be given visibility"),
            Self::Thread => f.write_str("a vis thread could not be run"),
        }
    }
}

impl std::error::Error for VisError {}

/// The visibility compiler.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Vis {
    mode: Mode,
}

/// A visibility row, each vis leaf's set of leaves it may see, before compression.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Visible {
    rows: Vec<Vec<u8>>,
    counts: VisCounts,
}

impl Visible {
    /// Whether vis leaf `from` may see vis leaf `to`.
    #[must_use]
    pub fn sees(&self, from: usize, to: usize) -> bool {
        self.rows
            .get(from)
            .and_then(|row| row.get(to.checked_div(BYTE_BITS)?))
            .is_some_and(|byte| {
                let bit = to.checked_rem(BYTE_BITS).unwrap_or_default();
                byte & (1 << bit) != 0
            })
    }

    /// How many vis leaves there are.
    #[must_use]
    pub fn leafs(&self) -> usize {
        self.rows.len()
    }

    /// The flow's counts, as id's vis prints them; none in the fast mode (VIS-4.8).
    #[must_use]
    pub const fn counts(&self) -> VisCounts {
        self.counts
    }
}

impl Vis {
    /// A compiler of `mode`.
    #[must_use]
    pub const fn new(mode: Mode) -> Self {
        Self { mode }
    }

    /// Each vis leaf's visible leaves from the portal file `prt` (VIS-2 to VIS-4).
    ///
    /// # Errors
    ///
    /// The [`VisError`] vis stops with.
    pub fn visible(&self, prt: &str) -> Result<Visible, VisError> {
        let file = PortalFile::read(prt)?;
        let (rows, counts) = self.rows(&file)?;
        Ok(Visible {
            rows: rows.iter().map(|row| row.row(file.leafs)).collect(),
            counts,
        })
    }

    fn rows(&self, file: &PortalFile) -> Result<(Vec<Leaves>, VisCounts), VisError> {
        let threads = std::thread::available_parallelism().map_or(1, usize::from);
        let (seen, counts) = match self.mode {
            Mode::Fast => (Flow::new(file, Level::Zero).fast(), VisCounts::default()),
            Mode::Full(level) => Flow::new(file, level).full(threads)?,
        };
        Ok((Flow::rows(file, &seen)?, counts))
    }

    /// The BSP `bsp` with the visibility its portal file `prt` gives: the compressed rows, each
    /// vis leaf's `visofs` and its ambient levels (VIS-1 to VIS-6).
    ///
    /// # Errors
    ///
    /// The [`VisError`] vis stops with, or [`VisError::Format`] for a BSP that cannot be read
    /// or has fewer leaves than the portal file.
    pub fn run(&self, bsp: &[u8], prt: &str) -> Result<Vec<u8>, VisError> {
        let read = Bsp::read(bsp, BspLoader::FitzQuake).map_err(|_| VisError::Format)?;
        let file = PortalFile::read(prt)?;
        if read.leafs.len() <= file.leafs {
            return Err(VisError::Format);
        }
        let (rows, _) = self.rows(&file)?;
        lump::written(bsp, &read, &rows)
    }
}

/// A `double` stored as a `float`, as C converts on assignment.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec VIS-2.3: vis keeps every coordinate and normal as a float"
)]
pub(crate) const fn to_f32(value: f64) -> f32 {
    value as f32
}

#[cfg(test)]
mod tests;
