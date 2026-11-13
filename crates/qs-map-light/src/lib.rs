// SPDX-License-Identifier: GPL-3.0-or-later

//! The light compiler (P6-06, `docs/spec/light.md`): each face's lightmaps from the map's light
//! entities, as id's `light` finds them, written into the BSP's lighting lump with each face's
//! styles and offset, and coloured light in a `.lit` file beside it (LIGHT-6).
//!
//! [`Light::run`] takes the BSP and gives the lit BSP, the `.lit` when a light is coloured, and
//! the warnings `light` prints. The fast mode is `light`'s default, one sample per luxel; the
//! full mode is its `-extra`, four (LIGHT-1.3). Faces are lit on every thread the machine has
//! and written in face order, so the result is the one a single thread gives (LIGHT-1.5).
//!
//! Nothing is ported: id's `light` is not a portable upstream (`docs/porting.md`), so the code
//! is written from the spec, which was written from its source, read and run as an oracle.

mod entities;
mod face;
mod lump;
mod trace;

use core::fmt;

use qs_bsp::{Bsp, BspLoader, BspVersion};
pub use qs_map_bsp::Dialect;

use crate::entities::Entities;
pub use crate::face::FaceId;
use crate::face::Scene;
use crate::lump::Colour;
use crate::trace::Tree;

pub use crate::entities::{
    LIGHT_DEFAULT_LEVEL, LIGHT_FIRST_SWITCHED_STYLE, LIGHT_MAX_MAP_ENTITIES,
    LIGHT_MAX_MAP_ENTSTRING, LIGHT_MAX_SWITCHED,
};
pub use crate::lump::LIGHT_MAX_MAP_LIGHTING;

/// The style of an unused lightmap slot (LIGHT-5.1).
pub(crate) const NO_STYLE: u8 = 255;
/// The axes of a point, and the channels of a colour.
pub(crate) const AXES: usize = 3;
/// A point or direction, or a colour's red, green and blue.
pub(crate) type Vec3 = [f32; AXES];
/// A colour as bytes.
pub(crate) type Rgb = [u8; AXES];

/// How many samples a luxel takes (LIGHT-1.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sampling {
    /// One per luxel: `light`'s default.
    #[default]
    Fast,
    /// Four, each two by two averaged: `light -extra`.
    Extra,
}

/// `scaledist`, `scalecos` and `rangescale` (LIGHT-1.2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale {
    /// `-dist`: how much a light's distance counts.
    pub dist: f32,
    /// How much the angle of incidence counts; `light` has no option for it.
    pub cos: f32,
    /// `-range`: what a sample's light is scaled by before it is a byte.
    pub range: f32,
}

/// `LIGHT_SCALEDIST` (LIGHT-1.2).
pub const LIGHT_SCALEDIST: f32 = 1.0;
/// `LIGHT_SCALECOS` (LIGHT-1.2).
pub const LIGHT_SCALECOS: f32 = 0.5;
/// `LIGHT_RANGESCALE` (LIGHT-1.2).
pub const LIGHT_RANGESCALE: f32 = 0.5;

impl Default for Scale {
    fn default() -> Self {
        Self {
            dist: LIGHT_SCALEDIST,
            cos: LIGHT_SCALECOS,
            range: LIGHT_RANGESCALE,
        }
    }
}

/// What stops `light`, as its errors read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LightError {
    /// `LoadEntities: found <token> when expecting {` (LIGHT-2.7).
    ExpectedBrace {
        /// The token found.
        found: Vec<u8>,
    },
    /// `LoadEntities: MAX_MAP_ENTITIES`, in the id dialect (LIGHT-2.7, LIGHT-1.7).
    TooManyEntities,
    /// `LoadEntities: EOF without closing brace` (LIGHT-2.7).
    Unclosed,
    /// `LoadEntities: closing brace without data` (LIGHT-2.7).
    NoValue,
    /// `LoadEntities: not 3 values for origin` (LIGHT-2.3).
    Origin,
    /// `Bad light style <n> (must be 0-254)` (LIGHT-2.3).
    BadStyle {
        /// The style read.
        style: i32,
    },
    /// More than 32 names given switchable styles (LIGHT-2.4).
    TooManySwitched,
    /// `Texture axis perpendicular to face` (LIGHT-3.6).
    TextureAxis {
        /// The face.
        face: FaceId,
    },
    /// `Bad surface extents` (LIGHT-3.2).
    Extents {
        /// The face.
        face: FaceId,
    },
    /// `GetFileSpace: overrun` in the id dialect; in the extended one, a lightmap whose offset
    /// does not fit `lightofs` (LIGHT-1.5, LIGHT-1.7).
    Overrun,
    /// `Entity text too long`, in the id dialect (LIGHT-1.6, LIGHT-1.7).
    EntityTextTooLong,
    /// A key and value too long for `light`'s 128-byte line, in the id dialect (LIGHT-2.8).
    PairTooLong {
        /// The key.
        key: String,
    },
    /// The BSP cannot be read or is a format `light` does not write.
    Format,
    /// A thread could not be run.
    Thread,
}

impl fmt::Display for LightError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExpectedBrace { found } => write!(
                f,
                "LoadEntities: found {} when expecting {{",
                String::from_utf8_lossy(found)
            ),
            Self::TooManyEntities => f.write_str("LoadEntities: MAX_MAP_ENTITIES"),
            Self::Unclosed => f.write_str("LoadEntities: EOF without closing brace"),
            Self::NoValue => f.write_str("LoadEntities: closing brace without data"),
            Self::Origin => f.write_str("LoadEntities: not 3 values for origin"),
            Self::BadStyle { style } => write!(f, "Bad light style {style} (must be 0-254)"),
            Self::TooManySwitched => f.write_str("more than 32 switchable light styles"),
            Self::TextureAxis { face } => {
                write!(
                    f,
                    "Texture axis perpendicular to face (face {})",
                    face.index()
                )
            }
            Self::Extents { face } => {
                write!(f, "Bad surface extents (face {})", face.index())
            }
            Self::Overrun => f.write_str("GetFileSpace: overrun"),
            Self::EntityTextTooLong => f.write_str("Entity text too long"),
            Self::PairTooLong { key } => {
                write!(f, "the pair of key {key} is too long for id's light")
            }
            Self::Format => f.write_str("the BSP cannot be lit"),
            Self::Thread => f.write_str("a light thread could not be run"),
        }
    }
}

impl std::error::Error for LightError {}

/// What `light` warns of and carries on past.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LightWarning {
    /// `WARNING: entity at (x,y,z) (<class>) has unmatched target` (LIGHT-2.5).
    UnmatchedTarget {
        /// The entity's origin, each coordinate truncated.
        origin: [i32; 3],
        /// Its classname.
        classname: String,
    },
    /// `WARNING: Too many light styles on a face` (LIGHT-4.5).
    TooManyStyles,
    /// In the extended dialect, a face `light` would stop on, left unlit (LIGHT-1.7).
    Unlit {
        /// What `light` would have stopped with.
        error: LightError,
    },
}

impl fmt::Display for LightWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnmatchedTarget {
                origin: [x, y, z],
                classname,
            } => write!(
                f,
                "WARNING: entity at ({x},{y},{z}) ({classname}) has unmatched target"
            ),
            Self::TooManyStyles => f.write_str("WARNING: Too many light styles on a face"),
            Self::Unlit { error } => write!(f, "WARNING: {error}; the face is left unlit"),
        }
    }
}

/// A lit map: the BSP, its `.lit` when a light is coloured, and the warnings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lighted {
    /// The BSP with its lighting, styles and entities.
    pub bsp: Vec<u8>,
    /// The `.lit` file's bytes, when any light is not white (LIGHT-6.5).
    pub lit: Option<Vec<u8>>,
    /// What `light` warned of, in the order it prints them.
    pub warnings: Vec<LightWarning>,
    /// How many names were given switchable styles (`<n> switchable light styles`).
    pub switchable: usize,
}

/// The light compiler.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Light {
    sampling: Sampling,
    scale: Scale,
    dialect: Dialect,
}

impl Light {
    /// A compiler that samples by `sampling`, with `light`'s default scales.
    #[must_use]
    pub fn new(sampling: Sampling) -> Self {
        Self {
            sampling,
            scale: Scale::default(),
            dialect: Dialect::default(),
        }
    }

    /// The compiler with `scale` instead of the defaults.
    #[must_use]
    pub const fn with_scale(self, scale: Scale) -> Self {
        Self { scale, ..self }
    }

    /// The compiler in `dialect`: id's fixed tables, or only the format's limits (LIGHT-1.7).
    #[must_use]
    pub const fn with_dialect(self, dialect: Dialect) -> Self {
        Self { dialect, ..self }
    }

    /// The BSP `bsp` lit (LIGHT-1 to LIGHT-6).
    ///
    /// # Errors
    ///
    /// The [`LightError`] `light` stops with, or [`LightError::Format`] for a BSP that cannot
    /// be read.
    pub fn run(&self, bsp: &[u8]) -> Result<Lighted, LightError> {
        // The faces' old lighting is not read: `light` overwrites it (LIGHT-1.1).
        let unlit = lump::without_lighting(bsp)?;
        let read = Bsp::read(&unlit, BspLoader::FitzQuake).map_err(|_| LightError::Format)?;
        if read.version == BspVersion::Valve {
            return Err(LightError::Format);
        }
        let mut warnings = Vec::new();
        let entities = Entities::read(read.entity_lump, self.dialect, &mut warnings)?;
        let colour = if entities.all.iter().any(|e| e.is_light() && e.is_coloured()) {
            Colour::Coloured
        } else {
            Colour::White
        };
        let tree = Tree::new(&read);
        let scene = Scene {
            bsp: &read,
            tree: &tree,
            entities: &entities,
            sampling: self.sampling,
            scale: self.scale,
            colour,
            dialect: self.dialect,
        };
        let faces = scene.lit_faces()?;
        let mut lit = Vec::with_capacity(faces.len());
        for (face, face_warnings) in faces {
            warnings.extend(face_warnings);
            lit.push(face);
        }
        let text = entities.written(self.dialect)?;
        let out = lump::written(bsp, &read, &lit, &text, colour, self.dialect)?;
        Ok(Lighted {
            bsp: out.bsp,
            lit: out.lit,
            warnings,
            switchable: entities.switched(),
        })
    }
}

/// A `double` stored as a `float`, as C converts on assignment.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec LIGHT-1.1: light keeps every coordinate and amount as a float"
)]
pub(crate) const fn to_f32(value: f64) -> f32 {
    value as f32
}

#[cfg(test)]
mod tests;
