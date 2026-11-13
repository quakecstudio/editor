// SPDX-License-Identifier: GPL-3.0-or-later

//! External map instances (MAP-14): a `misc_external_map` entity names another `.map`, whose
//! brushes ericw-tools' qbsp copies in place, scaled, turned and moved by the entity's keys.
//! [`MapFile::with_external_maps`] makes that expansion, so qs-map-bsp compiles the map as
//! ericw-tools does and the editor can draw each instance where it stands.

use core::fmt;

use crate::cnum::{c_atof, c_atoi, c_strtod};
use crate::document::{Entity, Item, MapError, MapFile, TexDef};
use crate::geometry::to_f32;

/// The classname of an external map instance, compared in any case (MAP-14.1).
pub const EXTERNAL_MAP_CLASS: &str = "misc_external_map";
/// The key naming the external map's file (MAP-14.1).
pub const EXTERNAL_MAP_KEY: &str = "_external_map";
/// The key giving the classname the instance becomes (MAP-14.1).
pub const EXTERNAL_CLASSNAME_KEY: &str = "_external_map_classname";
/// The key giving the instance's pitch, yaw and roll in degrees (MAP-14.4).
const ANGLES_KEY: &[u8] = b"_external_map_angles";
/// The key giving the instance's yaw alone, read when the angles are all 0 (MAP-14.4).
const ANGLE_KEY: &[u8] = b"_external_map_angle";
/// The key giving the instance's scale, one number or three (MAP-14.5).
const SCALE_KEY: &[u8] = b"_external_map_scale";
/// The key that leaves an entity's brushes out of the compile (MAP-14.2).
const OMIT_KEY: &[u8] = b"_omitbrushes";
/// ericw-tools' `QBSP_EQUAL_EPSILON`, its `DIST_EPSILON`: angles within it of 0 are none
/// (MAP-14.4).
const EQUAL_EPSILON: f32 = 0.0001;
/// The numbers of a whole vector.
const VECTOR_NUMBERS: usize = 3;

/// Why an external map instance cannot be expanded.
#[derive(Clone, Debug, PartialEq)]
pub enum ExternalMapError {
    /// The instance names no file or no classname (MAP-14.1).
    MissingKey {
        /// The instance's place among the map's entities.
        entity: usize,
        /// The key.
        key: &'static str,
    },
    /// The instance has brushes of its own: it must be a point entity (MAP-14.1).
    HasBrushes {
        /// The instance's place among the map's entities.
        entity: usize,
    },
    /// The file cannot be read (MAP-14.2).
    Unreadable {
        /// The file as the instance names it.
        file: String,
    },
    /// The file is not a map whose first entity is the world (MAP-14.2).
    NotWorld {
        /// The file as the instance names it.
        file: String,
    },
    /// The file has no brush (MAP-14.2).
    NoBrushes {
        /// The file as the instance names it.
        file: String,
    },
    /// A scale of 0 along an axis, which leaves no brush (MAP-14.5).
    ZeroScale {
        /// The instance's place among the map's entities.
        entity: usize,
    },
    /// The file does not parse, or a moved face cannot be written.
    Map {
        /// The file as the instance names it.
        file: String,
        /// Why.
        error: MapError,
    },
}

impl fmt::Display for ExternalMapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingKey { entity, key } => {
                write!(f, "external map entity {entity} has no {key}")
            }
            Self::HasBrushes { entity } => {
                write!(
                    f,
                    "external map entity {entity} has brushes; it must be a point entity"
                )
            }
            Self::Unreadable { file } => write!(f, "cannot read the external map {file}"),
            Self::NotWorld { file } => {
                write!(
                    f,
                    "the external map {file} does not start with its worldspawn"
                )
            }
            Self::NoBrushes { file } => write!(f, "the external map {file} has no brush"),
            Self::ZeroScale { entity } => {
                write!(f, "external map entity {entity} has a scale of 0")
            }
            Self::Map { file, error } => write!(f, "{file}: {error}"),
        }
    }
}

impl std::error::Error for ExternalMapError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Map { error, .. } => Some(error),
            Self::MissingKey { .. }
            | Self::HasBrushes { .. }
            | Self::Unreadable { .. }
            | Self::NotWorld { .. }
            | Self::NoBrushes { .. }
            | Self::ZeroScale { .. } => None,
        }
    }
}

/// A key's value read as `sscanf`'s `"%f %f %f"` does: the floats read, in order, and how many
/// (MAP-14.3).
fn scan_floats(text: &[u8]) -> ([f32; 3], usize) {
    let mut out = [0.0_f32; VECTOR_NUMBERS];
    let mut rest = text;
    let mut read: usize = 0;
    for slot in &mut out {
        let (value, used) = c_strtod(rest);
        if used == 0 {
            break;
        }
        *slot = to_f32(value);
        read = read.saturating_add(1);
        rest = rest.get(used..).unwrap_or_default();
    }
    (out, read)
}

/// A 3 by 3 matrix, by rows.
type Matrix = [[f64; 3]; 3];

/// `m` times `v`.
fn apply(m: &Matrix, v: [f64; 3]) -> [f64; 3] {
    m.map(|row| row[0] * v[0] + row[1] * v[1] + row[2] * v[2])
}

/// `a` times `b`.
fn product(a: &Matrix, b: &Matrix) -> Matrix {
    a.map(|row| {
        [0, 1, 2].map(|j| {
            let column = b.map(|r| r.get(j).copied().unwrap_or_default());
            row[0] * column[0] + row[1] * column[1] + row[2] * column[2]
        })
    })
}

/// How an instance moves its brushes: scale, then turn, then move (MAP-14.6).
struct Placement {
    scale: [f64; 3],
    turn: Matrix,
    origin: [f64; 3],
}

impl Placement {
    /// The placement `entity`'s keys give (MAP-14.3 to MAP-14.5).
    fn of(entity: &Entity, place: usize) -> Result<Self, ExternalMapError> {
        let value = |key: &[u8]| entity.value(key).unwrap_or_default();
        let (origin, _) = scan_floats(value(b"origin"));
        let (mut angles, _) = scan_floats(value(ANGLES_KEY));
        if angles.iter().all(|a| a.abs() <= EQUAL_EPSILON) {
            angles[1] = to_f32(c_atof(value(ANGLE_KEY)));
        }
        let (scale, read) = scan_floats(value(SCALE_KEY));
        let scale = if read >= VECTOR_NUMBERS {
            scale
        } else if scale[0] == 0.0 {
            [1.0; VECTOR_NUMBERS]
        } else {
            [scale[0]; VECTOR_NUMBERS]
        };
        if scale.contains(&0.0) {
            return Err(ExternalMapError::ZeroScale { entity: place });
        }
        let [pitch, yaw, roll] = angles.map(|a| f64::from(a).to_radians());
        let (sp, cp) = pitch.sin_cos();
        let (sy, cy) = yaw.sin_cos();
        let (sr, cr) = roll.sin_cos();
        let about_z = [[cy, -sy, 0.0], [sy, cy, 0.0], [0.0, 0.0, 1.0]];
        let about_y = [[cp, 0.0, sp], [0.0, 1.0, 0.0], [-sp, 0.0, cp]];
        let about_x = [[1.0, 0.0, 0.0], [0.0, cr, -sr], [0.0, sr, cr]];
        Ok(Self {
            scale: scale.map(f64::from),
            turn: product(&product(&about_z, &about_y), &about_x),
            origin: origin.map(f64::from),
        })
    }

    /// `point` placed.
    fn point(&self, point: [f64; 3]) -> [f64; 3] {
        let [x, y, z] = point;
        let [sx, sy, sz] = self.scale;
        let [tx, ty, tz] = apply(&self.turn, [x * sx, y * sy, z * sz]);
        let [ox, oy, oz] = self.origin;
        [tx + ox, ty + oy, tz + oz]
    }

    /// A texture axis and offset placed, so each point keeps its texel (MAP-14.6).
    fn axis(&self, [x, y, z, offset]: [f64; 4]) -> [f64; 4] {
        let unscaled = [x / self.scale[0], y / self.scale[1], z / self.scale[2]];
        let [ax, ay, az] = apply(&self.turn, unscaled);
        let moved = ax * self.origin[0] + ay * self.origin[1] + az * self.origin[2];
        [ax, ay, az, offset - moved]
    }
}

impl MapFile {
    /// The map with each `misc_external_map` entity given the brushes of the map it names,
    /// placed by its keys, its classname its `_external_map_classname` and its origin
    /// `0 0 0`, as ericw-tools' qbsp makes it; `load` reads a file the entity names. The
    /// moved faces are written in the Valve 220 form, with the placed texture axes.
    ///
    /// Implements MAP-14.
    ///
    /// # Errors
    ///
    /// An [`ExternalMapError`] for an instance that lacks a key, has brushes or a scale of 0,
    /// or names a file that cannot be read, does not parse, does not start with its world or
    /// has no brush.
    pub fn with_external_maps(
        &self,
        load: &mut dyn FnMut(&str) -> Option<Vec<u8>>,
    ) -> Result<Self, ExternalMapError> {
        let mut out = self.clone();
        for (place, entity) in out.entities.iter_mut().enumerate() {
            let is_instance = entity
                .classname()
                .is_some_and(|c| c.eq_ignore_ascii_case(EXTERNAL_MAP_CLASS.as_bytes()));
            if !is_instance {
                continue;
            }
            let key = |key: &'static str| {
                entity
                    .value(key.as_bytes())
                    .filter(|value| !value.is_empty())
                    .map(|value| String::from_utf8_lossy(value).into_owned())
                    .ok_or(ExternalMapError::MissingKey { entity: place, key })
            };
            let file = key(EXTERNAL_MAP_KEY)?;
            let classname = key(EXTERNAL_CLASSNAME_KEY)?;
            if entity.has_brushes() {
                return Err(ExternalMapError::HasBrushes { entity: place });
            }
            let placement = Placement::of(entity, place)?;
            let bytes =
                load(&file).ok_or_else(|| ExternalMapError::Unreadable { file: file.clone() })?;
            let map_error = |error| ExternalMapError::Map {
                file: file.clone(),
                error,
            };
            let external = Self::parse(&bytes).map_err(map_error)?;
            let is_world = external
                .entities
                .first()
                .and_then(Entity::classname)
                .is_some_and(|c| c.eq_ignore_ascii_case(b"worldspawn"));
            if !is_world {
                return Err(ExternalMapError::NotWorld { file });
            }
            let mut brushes = Vec::new();
            for source in &external.entities {
                let omitted = source
                    .value(OMIT_KEY)
                    .is_some_and(|value| c_atoi(value) != 0);
                if omitted {
                    continue;
                }
                brushes.extend(source.brushes().filter(|b| !b.faces.is_empty()).cloned());
            }
            if brushes.is_empty() {
                return Err(ExternalMapError::NoBrushes { file });
            }
            for brush in &mut brushes {
                for face in &mut brush.faces {
                    let points = face.points().map(|p| placement.point(p));
                    // A face with no texture axes, one with no plane, keeps its definition:
                    // both compilers drop it (MAP-3.6).
                    let texdef = face
                        .texvecs()
                        .map_or(*face.texdef(), |[s, t]| TexDef::Valve {
                            s: placement.axis(s),
                            t: placement.axis(t),
                            rotate: 0.0,
                            scale: [1.0, 1.0],
                        });
                    let texture = face.texture().to_vec();
                    face.set(points, &texture, texdef).map_err(map_error)?;
                }
            }
            entity
                .set(b"classname", classname.as_bytes())
                .map_err(map_error)?;
            entity.set(b"origin", b"0 0 0").map_err(map_error)?;
            entity.items.extend(brushes.into_iter().map(Item::Brush));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests;
