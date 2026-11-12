// SPDX-License-Identifier: GPL-3.0-or-later

//! The map compiler (P6-04, MAP-8): a `.map`, read as qbsp reads it (`qs_map::qbsp`), compiled
//! to a BSP29 file, its portal file and its leak file as id's qbsp builds them (QBSP-1 to
//! QBSP-13): brushes loaded and grown for the clipping hulls, CSG, the BSP tree, portals, the
//! outside fill, merged faces, T-junctions fixed, and the lumps written.

mod brush;
mod compile;
mod csg;
mod math;
mod merge;
mod output;
mod outside;
mod planes;
mod portals;
mod prt;
mod tjunc;
mod tree;
mod winding;

use std::fmt;

use qs_bsp::LumpId;
use qs_bsp::write::BspWriteError;
use qs_map::qbsp::QbspError;

pub use compile::{Compiled, Compiler};

use crate::math::Vec3;
use crate::planes::{PlaneId, Side};

/// The most points a face may have (QBSP-3.5).
pub(crate) const MAXEDGES: usize = 32;

/// What a leaf, or a side of a face, is (QBSP-4.1, BSP-12).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Contents {
    Empty,
    Solid,
    Water,
    Slime,
    Lava,
    Sky,
}

impl Contents {
    /// The `CONTENTS_` value.
    pub(crate) const fn value(self) -> i32 {
        match self {
            Self::Empty => -1,
            Self::Solid => -2,
            Self::Water => -3,
            Self::Slime => -4,
            Self::Lava => -5,
            Self::Sky => -6,
        }
    }

    /// Water, slime, lava or sky: qbsp's `contents <= CONTENTS_WATER` (QBSP-6.3).
    pub(crate) const fn is_liquid_or_sky(self) -> bool {
        matches!(self, Self::Water | Self::Slime | Self::Lava | Self::Sky)
    }
}

/// The hull being built (QBSP-1.4, QBSP-5.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Hull {
    /// Hull 0, the drawing hull.
    Draw,
    /// Hull 1, the player's.
    Player,
    /// Hull 2, the large monsters'.
    Large,
}

impl Hull {
    /// The box a brush is grown by; none for the drawing hull (QBSP-5.1).
    pub(crate) const fn size(self) -> Option<[Vec3; 2]> {
        match self {
            Self::Draw => None,
            Self::Player => Some([[-16.0, -16.0, -32.0], [16.0, 16.0, 24.0]]),
            Self::Large => Some([[-32.0, -32.0, -64.0], [32.0, 32.0, 24.0]]),
        }
    }

    /// The hull's number in a model's `headnode`.
    pub(crate) const fn number(self) -> usize {
        match self {
            Self::Draw => 0,
            Self::Player => 1,
            Self::Large => 2,
        }
    }
}

/// Whether an entity is the world, whose liquids, sky and outside qbsp treats apart
/// (QBSP-4.1, QBSP-1.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ModelKind {
    World,
    Entity,
}

/// A face: a convex polygon on a plane of the table (QBSP-4.3).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Face {
    pub(crate) plane: PlaneId,
    pub(crate) side: Side,
    pub(crate) texinfo: usize,
    /// The contents in front of it and behind it.
    pub(crate) contents: [Contents; 2],
    pub(crate) points: Vec<Vec3>,
    /// The node face it was cut from (QBSP-7.7).
    pub(crate) original: Option<usize>,
}

/// The faces of one plane (QBSP-6.6).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Surface {
    pub(crate) plane: PlaneId,
    pub(crate) faces: Vec<Face>,
    pub(crate) mins: Vec3,
    pub(crate) maxs: Vec3,
    /// Whether it has split a node (QBSP-7.2).
    pub(crate) on_node: bool,
}

impl Surface {
    /// A surface of `faces` on `plane`, its bounds theirs, `CalcSurfaceInfo` (QBSP-6.6).
    pub(crate) fn new(plane: PlaneId, faces: Vec<Face>) -> Self {
        let mut surface = Self {
            plane,
            faces,
            mins: [99999.0; 3],
            maxs: [-99999.0; 3],
            on_node: false,
        };
        surface.recalculate();
        surface
    }

    /// Its bounds again.
    pub(crate) fn recalculate(&mut self) {
        self.mins = [99999.0; 3];
        self.maxs = [-99999.0; 3];
        for &point in self.faces.iter().flat_map(|face| &face.points) {
            self.mins = math::min(self.mins, point);
            self.maxs = math::max(self.maxs, point);
        }
    }
}

/// Which compiler the map compiler is (QBSP-14).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Dialect {
    /// id's qbsp exactly: its parser and its fixed-size limits.
    Id,
    /// Valve 220 faces and exact points read, and qbsp's fixed-size tables unbounded
    /// (QBSP-14).
    #[default]
    Extended,
}

/// The limits a dialect compiles within (QBSP-2.5, QBSP-5.6, QBSP-14.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Limits {
    /// The face count at which adding a plane to a brush fails, compared with `==`.
    pub(crate) brush_faces: Option<usize>,
    /// The most planes a brush may have.
    pub(crate) brush_planes: usize,
    pub(crate) hull_points: usize,
    pub(crate) hull_edges: usize,
    /// The most planes in the table and the output.
    pub(crate) planes: usize,
    /// What a fault qbsp stops on does (QBSP-14.3).
    pub(crate) faults: Faults,
    /// Which limits bound the output's lumps (QBSP-14.4).
    pub(crate) tables: Tables,
}

/// Which limits bound the output's lumps (QBSP-14.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tables {
    /// qbsp's `MAX_MAP_` arrays, each refusing the compile when full.
    Qbsp,
    /// Only the file's fields: BSP29 when every value fits it, else `BSP2`.
    Format,
}

impl Tables {
    /// Whether a lump of `len` entries is full under qbsp's `max`.
    pub(crate) const fn full(self, len: usize, max: usize) -> bool {
        match self {
            Self::Qbsp => len >= max,
            Self::Format => false,
        }
    }
}

/// What a fault qbsp stops on does: a bad face, a leaf of mixed contents, an entity with no
/// valid brush (QBSP-14.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Faults {
    /// It stops the compile, as in qbsp.
    Stop,
    /// It is a warning, and the compile goes on.
    Warn,
}

impl Dialect {
    pub(crate) const fn limits(self) -> Limits {
        match self {
            Self::Id => Limits {
                brush_faces: Some(16),
                brush_planes: 128,
                hull_points: 32,
                hull_edges: 64,
                planes: 8192,
                faults: Faults::Stop,
                tables: Tables::Qbsp,
            },
            Self::Extended => Limits {
                brush_faces: None,
                brush_planes: usize::MAX,
                hull_points: usize::MAX,
                hull_edges: usize::MAX,
                planes: usize::MAX,
                faults: Faults::Warn,
                tables: Tables::Format,
            },
        }
    }
}

/// What qbsp prints and goes on after.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Warning {
    /// A brush with no face left (QBSP-4.3).
    NoBrushFaces,
    /// A node portal clipped away (QBSP-8.2).
    PortalClippedAway,
    /// No entity stands in empty space, so the outside is not filled (QBSP-9.2).
    NoOccupant,
    /// The outside reaches an entity: the map leaks (QBSP-9.3).
    Leak,
    /// A face qbsp's `CheckFace` refuses, kept (QBSP-14.3).
    BadFace(String),
    /// A leaf whose faces disagree on its contents, given the first's (QBSP-14.3).
    MixedContents,
    /// An entity with brushes none of which is valid, given an empty model (QBSP-14.3).
    NoValidBrushes,
    /// A value too wide for BSP29, so the file is written as `BSP2` (QBSP-14.4).
    Bsp2(LumpId),
}

/// What stops qbsp, as its errors read.
#[derive(Clone, Debug, PartialEq)]
pub enum CompileError {
    /// The map could not be read (MAP-1 to MAP-7).
    Map(QbspError),
    /// The map could not be read by the lossless parser (QBSP-14.1).
    Document(qs_map::MapError),
    /// `FindPlane: normalization error` (QBSP-2.3).
    Normalization,
    /// `numbrushplanes == MAX_MAP_PLANES` (QBSP-2.5).
    TooManyPlanes,
    /// A winding past 64 points, or a face past 32 (QBSP-3.5).
    TooManyPoints,
    /// `CheckFace: <n> points` (QBSP-4.5).
    FacePoints(usize),
    /// `CheckFace: BUGUS_RANGE: <value>` (QBSP-4.5).
    BogusRange(f32),
    /// `CheckFace: point off plane` (QBSP-4.5).
    PointOffPlane,
    /// `CheckFace: degenerate edge` (QBSP-4.5).
    DegenerateEdge,
    /// `CheckFace: non-convex` (QBSP-4.5).
    NonConvex,
    /// `AddBrushPlane: numbrushfaces == MAX_FACES` (QBSP-5.6).
    TooManyBrushFaces,
    /// `AddBrushPlane: bad normal` (QBSP-5.3).
    BadNormal,
    /// `MAX_HULL_POINTS` (QBSP-5.6).
    TooManyHullPoints,
    /// `MAX_HULL_EDGES` (QBSP-5.6).
    TooManyHullEdges,
    /// `Entity with no valid brushes` (QBSP-1.6).
    NoValidBrushes,
    /// `Mixed face contents in leafnode` (QBSP-7.6).
    MixedContents,
    /// `SubdivideFace: didn't split the polygon` (QBSP-7.8).
    SubdivideFailed,
    /// A `MAX_MAP_` limit passed while writing, named (QBSP-13.10).
    Limit(&'static str),
    /// The BSP could not be written (BSP-23.3).
    Write(BspWriteError),
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Map(error) => write!(f, "{error}"),
            Self::Document(error) => write!(f, "{error}"),
            Self::Normalization => f.write_str("FindPlane: normalization error"),
            Self::TooManyPlanes => f.write_str("numbrushplanes == MAX_MAP_PLANES"),
            Self::TooManyPoints => f.write_str("a polygon has too many points"),
            Self::FacePoints(count) => write!(f, "CheckFace: {count} points"),
            Self::BogusRange(value) => write!(f, "CheckFace: BUGUS_RANGE: {value}"),
            Self::PointOffPlane => f.write_str("CheckFace: point off plane"),
            Self::DegenerateEdge => f.write_str("CheckFace: degenerate edge"),
            Self::NonConvex => f.write_str("CheckFace: non-convex"),
            Self::TooManyBrushFaces => f.write_str("AddBrushPlane: numbrushfaces == MAX_FACES"),
            Self::BadNormal => f.write_str("AddBrushPlane: bad normal"),
            Self::TooManyHullPoints => f.write_str("MAX_HULL_POINTS"),
            Self::TooManyHullEdges => f.write_str("MAX_HULL_EDGES"),
            Self::NoValidBrushes => f.write_str("Entity with no valid brushes"),
            Self::MixedContents => f.write_str("Mixed face contents in leafnode"),
            Self::SubdivideFailed => f.write_str("SubdivideFace: didn't split the polygon"),
            Self::Limit(name) => write!(f, "{name}"),
            Self::Write(error) => write!(f, "{error}"),
        }
    }
}

impl core::error::Error for CompileError {}

impl From<QbspError> for CompileError {
    fn from(error: QbspError) -> Self {
        Self::Map(error)
    }
}
