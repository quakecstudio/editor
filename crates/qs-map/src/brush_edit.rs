// SPDX-License-Identifier: GPL-3.0-or-later

//! Editing brushes (P6-08, MAP-3): a box from two corners, a clip by a plane, a subtraction
//! of one brush from another, the merge of brushes into their convex hull, and a vertex moved.
//! A brush is the convex solid behind every face's plane (MAP-3.4); every operation gives only
//! brushes whose every face has a polygon on that solid and whose solid is bounded, so a
//! compiler never meets a brush with a stray or duplicate face.

use crate::geometry::{
    MIN_CORNERS, ON_PLANE, POLYGON_REACH, Plane, dot, paraxial_axes, plane as plane_of, polygons_on,
};
use crate::{Brush, Entity, Face, Item, MapError, TexDef};

/// What a new face is painted with: its texture and its texture definition.
#[derive(Clone, Debug, PartialEq)]
pub struct Surface {
    /// The texture's name.
    pub texture: Vec<u8>,
    /// The texture's alignment.
    pub texdef: TexDef,
}

impl Surface {
    /// The surface of `face`.
    fn of(face: &Face) -> Self {
        Self {
            texture: face.texture().to_vec(),
            texdef: *face.texdef(),
        }
    }

    /// A face through `points`, facing along `normal`, painted with the surface. A standard
    /// texture definition fits any plane; Valve 220 axes that do not lie in this one are
    /// replaced by its paraxial axes (MAP-4.2), keeping their shifts, rotation and scale.
    fn face(&self, points: [[f64; 3]; 3], normal: [f64; 3]) -> Result<Face, BrushError> {
        let texdef = match self.texdef {
            TexDef::Valve {
                s,
                t,
                rotate,
                scale,
            } => {
                let [sx, sy, sz, s_shift] = s;
                let [tx, ty, tz, t_shift] = t;
                let lies_in = |axis: [f64; 3]| dot(axis, normal).abs() < IN_PLANE;
                if lies_in([sx, sy, sz]) && lies_in([tx, ty, tz]) {
                    self.texdef
                } else {
                    let [[psx, psy, psz], [ptx, pty, ptz]] = paraxial_axes(normal);
                    TexDef::Valve {
                        s: [psx, psy, psz, s_shift],
                        t: [ptx, pty, ptz, t_shift],
                        rotate,
                        scale,
                    }
                }
            }
            TexDef::Standard { .. } => self.texdef,
        };
        Ok(Face::new(points, &self.texture, texdef)?)
    }
}

/// Whether a brush's textures stay where they are in the world when it moves (MAP-11.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextureLock {
    /// Each face's shift follows the move, so the texture stays fixed in the world.
    #[default]
    Locked,
    /// The texture definitions stay as they are, so the texture moves with the brush.
    Free,
}

/// Why a brush could not be made.
#[derive(Debug)]
pub enum BrushError {
    /// The result has no volume, or a face with no polygon on it.
    Degenerate,
    /// No vertex of the brush is at the point given.
    NoVertex([f64; 3]),
    /// The moved vertex would lie inside the brush, so it would vanish.
    VertexInside([f64; 3]),
    /// Moving a vertex would leave this other vertex inside the brush or on a face.
    CornerLost([f64; 3]),
    /// The plane misses the brush, so the clip would change nothing.
    Missed,
    /// A face could not be written.
    Map(MapError),
}

impl core::fmt::Display for BrushError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Degenerate => {
                f.write_str("the brush would have no volume or a face with nothing on it")
            }
            Self::NoVertex(p) => write!(f, "the brush has no vertex at {p:?}"),
            Self::VertexInside(p) => write!(f, "a vertex moved to {p:?} would be inside the brush"),
            Self::CornerLost(p) => write!(f, "the move would take away the vertex at {p:?}"),
            Self::Missed => f.write_str("the plane misses the brush"),
            Self::Map(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for BrushError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Map(error) => Some(error),
            Self::Degenerate
            | Self::NoVertex(_)
            | Self::VertexInside(_)
            | Self::CornerLost(_)
            | Self::Missed => None,
        }
    }
}

impl From<MapError> for BrushError {
    fn from(error: MapError) -> Self {
        Self::Map(error)
    }
}

/// Coordinates are rounded to this many parts of a unit, so arithmetic noise is not written.
const PRECISION: f64 = 1e6;
/// How far apart two vertices must be to be two.
const SAME_POINT: f64 = 1e-3;
/// How near 1 the dot product of two normals is when they point the same way.
const SAME_NORMAL: f64 = 1.0 - 1e-9;
/// The largest dot product of a Valve 220 axis with a face's normal for the axis to lie in
/// the face's plane.
const IN_PLANE: f64 = 1e-6;

/// `value` rounded to [`PRECISION`], -0 written as 0.
fn round(value: f64) -> f64 {
    (value * PRECISION).round() / PRECISION + 0.0
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    dot(d, d).sqrt()
}

/// Whether `a` and `b` are one vertex: nearer than [`SAME_POINT`].
fn same_point(a: [f64; 3], b: [f64; 3]) -> bool {
    distance(a, b) < SAME_POINT
}

impl Plane<f64> {
    /// The same plane facing the other way.
    fn flipped(self) -> Self {
        Self {
            normal: self.normal.map(|c| -c),
            dist: -self.dist,
        }
    }

    /// How far `point` is in front of it.
    fn side(&self, point: [f64; 3]) -> f64 {
        dot(point, self.normal) - self.dist
    }

    /// Whether `other` is the same plane, facing the same way.
    fn same(&self, other: &Self) -> bool {
        dot(self.normal, other.normal) > SAME_NORMAL && (self.dist - other.dist).abs() < ON_PLANE
    }
}

/// Three corners of `polygon`, spread around it, ordered so the plane they give faces along
/// `normal`; `None` for a polygon of fewer than three corners.
fn face_points(polygon: &[[f64; 3]], normal: [f64; 3]) -> Option<[[f64; 3]; 3]> {
    let mut corners: Vec<[f64; 3]> = Vec::new();
    for corner in polygon.iter().map(|p| p.map(round)) {
        if !corners.iter().any(|&known| same_point(known, corner)) {
            corners.push(corner);
        }
    }
    if corners.len() < MIN_CORNERS {
        return None;
    }
    // The first corner, the one farthest from it, and the one making the widest triangle with
    // the two: three points well apart, so the plane they give is the polygon's.
    let a = *corners.first()?;
    let b = *corners
        .iter()
        .max_by(|p, q| distance(a, **p).total_cmp(&distance(a, **q)))?;
    let area = |c: [f64; 3]| {
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        dot(n, n)
    };
    let c = *corners
        .iter()
        .max_by(|p, q| area(**p).total_cmp(&area(**q)))?;
    let made = plane_of([a, b, c])?;
    Some(if dot(made.normal, normal) >= 0.0 {
        [a, b, c]
    } else {
        [c, b, a]
    })
}

impl Entity {
    /// The places in [`Entity::items`] of the entity's brushes, in order.
    #[must_use]
    pub fn brush_places(&self) -> Vec<usize> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| matches!(item, Item::Brush(_)))
            .map(|(place, _)| place)
            .collect()
    }
}

impl Brush {
    /// The planes of the brush's faces; `None` when a face's points are collinear.
    fn planes(&self) -> Option<Vec<Plane<f64>>> {
        self.faces.iter().map(Face::plane).collect()
    }

    /// Whether the brush is a bounded convex solid each face of which has a polygon on it,
    /// with no two faces on one plane (MAP-3.4).
    #[must_use]
    pub fn is_valid(&self) -> bool {
        let Some(planes) = self.planes() else {
            return false;
        };
        if planes.len() < MIN_CORNERS.saturating_add(1) {
            return false;
        }
        let polygons = polygons_on(&planes);
        for ((index, plane), polygon) in planes.iter().enumerate().zip(polygons) {
            if planes
                .iter()
                .skip(index.saturating_add(1))
                .any(|other| plane.same(other))
            {
                return false;
            }
            let bounded = polygon
                .iter()
                .flatten()
                .all(|c| c.abs() < POLYGON_REACH / 2.0);
            if polygon.len() < MIN_CORNERS || !bounded {
                return false;
            }
        }
        true
    }

    /// The brush's corners, each once.
    #[must_use]
    pub fn vertices(&self) -> Vec<[f64; 3]> {
        Self::corners(&self.polygons())
    }

    /// The corners of a brush whose face polygons are `polygons`, as [`Brush::polygons`] gives
    /// them, each once and rounded as [`Brush::vertices`] gives them.
    #[must_use]
    pub fn corners(polygons: &[Vec<[f64; 3]>]) -> Vec<[f64; 3]> {
        let mut out: Vec<[f64; 3]> = Vec::new();
        for &corner in polygons.iter().flatten() {
            let corner = corner.map(round);
            if !out.iter().any(|&known| same_point(known, corner)) {
                out.push(corner);
            }
        }
        out
    }

    /// The box from `min` to `max`, its six faces painted with `surface`.
    ///
    /// # Errors
    ///
    /// [`BrushError::Degenerate`] when the box has no volume, and [`BrushError::Map`] when the
    /// surface cannot be written.
    pub fn cuboid(min: [f64; 3], max: [f64; 3], surface: &Surface) -> Result<Self, BrushError> {
        let [x0, y0, z0] = min;
        let [x1, y1, z1] = max;
        if x0 >= x1 || y0 >= y1 || z0 >= z1 {
            return Err(BrushError::Degenerate);
        }
        let corners = [
            [x0, y0, z0],
            [x1, y0, z0],
            [x1, y1, z0],
            [x0, y1, z0],
            [x0, y0, z1],
            [x1, y0, z1],
            [x1, y1, z1],
            [x0, y1, z1],
        ];
        Self::hull(&corners, &[], surface)
    }

    /// The brush with only the faces that have a polygon on its solid.
    fn pruned(mut self) -> Self {
        if let Some(planes) = self.planes() {
            let keep: Vec<bool> = polygons_on(&planes)
                .iter()
                .map(|polygon| polygon.len() >= MIN_CORNERS)
                .collect();
            let mut flags = keep.into_iter();
            self.faces.retain(|_| flags.next().unwrap_or(false));
        }
        self
    }

    /// The brush with a face added on `plane`, the solid behind it kept, painted with
    /// `surface`; `None` when nothing of the solid is behind it.
    fn cut(&self, plane: Plane<f64>, surface: &Surface) -> Result<Option<Self>, BrushError> {
        let mut planes = self.planes().ok_or(BrushError::Degenerate)?;
        planes.push(plane);
        let polygon = polygons_on(&planes).pop().unwrap_or_default();
        let Some(points) = face_points(&polygon, plane.normal) else {
            return Ok(None);
        };
        let mut brush = self.clone();
        brush.faces.push(surface.face(points, plane.normal)?);
        let brush = brush.pruned();
        Ok(brush.is_valid().then_some(brush))
    }

    /// The parts of the brush behind `plane` and in front of it, the new face of each painted
    /// with `surface`. A brush wholly on one side is that side's part unchanged, the other
    /// `None`; a brush the plane crosses gives two parts.
    ///
    /// # Errors
    ///
    /// [`BrushError::Degenerate`] when the plane crosses the brush but a part would be too thin
    /// to be a brush, and [`BrushError::Map`] when the new face cannot be written.
    pub fn split(
        &self,
        plane: Plane<f64>,
        surface: &Surface,
    ) -> Result<(Option<Self>, Option<Self>), BrushError> {
        let vertices = self.vertices();
        if vertices.iter().all(|&v| plane.side(v) <= ON_PLANE) {
            return Ok((Some(self.clone()), None));
        }
        if vertices.iter().all(|&v| plane.side(v) >= -ON_PLANE) {
            return Ok((None, Some(self.clone())));
        }
        match (
            self.cut(plane, surface)?,
            self.cut(plane.flipped(), surface)?,
        ) {
            (Some(back), Some(front)) => Ok((Some(back), Some(front))),
            (None, _) | (_, None) => Err(BrushError::Degenerate),
        }
    }

    /// The brush with `cutter` taken out of it: the pieces of it in front of each of the
    /// cutter's planes, the new faces painted as the cutter's; the brush itself, unchanged, when
    /// the two do not overlap, and nothing when the cutter holds all of it.
    ///
    /// # Errors
    ///
    /// [`BrushError::Degenerate`] for a cutter that is no valid brush, and [`BrushError::Map`]
    /// when a new face cannot be written.
    pub fn subtract(&self, cutter: &Self) -> Result<Vec<Self>, BrushError> {
        if !cutter.is_valid() {
            return Err(BrushError::Degenerate);
        }
        let mut pieces = Vec::new();
        let mut rest = Some(self.clone());
        for face in &cutter.faces {
            let Some(remaining) = rest.take() else {
                break;
            };
            let plane = face.plane().ok_or(BrushError::Degenerate)?;
            let (inside, outside) = remaining.split(plane, &Surface::of(face))?;
            if let Some(outside) = outside {
                pieces.push(outside);
            }
            rest = inside;
            if rest.is_none() {
                // The brush lies wholly in front of this face of the cutter: no overlap.
                return Ok(vec![self.clone()]);
            }
        }
        Ok(pieces)
    }

    /// The convex hull of `points`, each face painted as the face of `from` whose normal is
    /// nearest its own, or with `surface` when `from` has none.
    fn hull(points: &[[f64; 3]], from: &[&Self], surface: &Surface) -> Result<Self, BrushError> {
        let mut unique: Vec<[f64; 3]> = Vec::new();
        for &point in points {
            if !unique.iter().any(|&known| same_point(known, point)) {
                unique.push(point);
            }
        }
        let points = unique.as_slice();
        let mut planes: Vec<Plane<f64>> = Vec::new();
        for (i, &a) in points.iter().enumerate() {
            for (j, &b) in points.iter().enumerate().skip(i.saturating_add(1)) {
                for &c in points.iter().skip(j.saturating_add(1)) {
                    let Some(candidate) = plane_of([a, b, c]) else {
                        continue;
                    };
                    let outward = if points.iter().all(|&p| candidate.side(p) <= ON_PLANE) {
                        candidate
                    } else if points.iter().all(|&p| candidate.side(p) >= -ON_PLANE) {
                        candidate.flipped()
                    } else {
                        continue;
                    };
                    if !planes.iter().any(|known| known.same(&outward)) {
                        planes.push(outward);
                    }
                }
            }
        }
        let faces: Vec<&Face> = from.iter().flat_map(|brush| &brush.faces).collect();
        let mut brush = Self::new();
        for (plane, polygon) in planes.iter().zip(polygons_on(&planes)) {
            let Some(points) = face_points(&polygon, plane.normal) else {
                continue;
            };
            let paint = faces
                .iter()
                .filter_map(|face| face.plane().map(|p| (dot(p.normal, plane.normal), face)))
                .max_by(|a, b| a.0.total_cmp(&b.0))
                .map_or_else(|| surface.clone(), |(_, face)| Surface::of(face));
            brush.faces.push(paint.face(points, plane.normal)?);
        }
        if brush.is_valid() {
            Ok(brush)
        } else {
            Err(BrushError::Degenerate)
        }
    }

    /// The convex hull of this brush and `others`, painted as their nearest faces.
    ///
    /// # Errors
    ///
    /// [`BrushError::Degenerate`] when the hull has no volume, and [`BrushError::Map`] when a
    /// face cannot be written.
    pub fn merge(&self, others: &[&Self]) -> Result<Self, BrushError> {
        let mut points = self.vertices();
        for other in others {
            points.extend(other.vertices());
        }
        let mut from = vec![self];
        from.extend_from_slice(others);
        let surface = self
            .faces
            .first()
            .map(Surface::of)
            .ok_or(BrushError::Degenerate)?;
        Self::hull(&points, &from, &surface)
    }

    /// The brush moved by `by`, every face's points with it; with `lock` [`TextureLock::Locked`]
    /// each face's shift is less the move along its texture axes, so every point of the face
    /// keeps its texture coordinate.
    ///
    /// Implements MAP-11.1 and MAP-11.2.
    ///
    /// # Errors
    ///
    /// [`BrushError::Map`] when a face cannot be written, its coordinates not finite.
    pub fn translated(&self, by: [f64; 3], lock: TextureLock) -> Result<Self, BrushError> {
        let mut moved = self.clone();
        for face in &mut moved.faces {
            let points = face
                .points()
                .map(|p| [p[0] + by[0], p[1] + by[1], p[2] + by[2]]);
            let texdef = match (lock, face.texvecs()) {
                (TextureLock::Locked, Some([s, t])) => {
                    let [bx, bdy, bz] = by;
                    let along = |[x, y, z, _]: [f64; 4]| x * bx + y * bdy + z * bz;
                    match *face.texdef() {
                        TexDef::Standard {
                            shift,
                            rotate,
                            scale,
                        } => TexDef::Standard {
                            shift: [shift[0] - along(s), shift[1] - along(t)],
                            rotate,
                            scale,
                        },
                        TexDef::Valve {
                            s: [sx, sy, sz, s_shift],
                            t: [tx, ty, tz, t_shift],
                            rotate,
                            scale,
                        } => TexDef::Valve {
                            s: [sx, sy, sz, s_shift - along(s)],
                            t: [tx, ty, tz, t_shift - along(t)],
                            rotate,
                            scale,
                        },
                    }
                }
                (TextureLock::Locked, None) | (TextureLock::Free, _) => *face.texdef(),
            };
            let texture = face.texture().to_vec();
            face.set(points, &texture, texdef)?;
        }
        Ok(moved)
    }

    /// The brush with its vertex at `from` moved to `to`: the convex hull of its vertices so
    /// moved, its faces painted as the nearest of its own.
    ///
    /// # Errors
    ///
    /// [`BrushError::NoVertex`] when no vertex is at `from`, [`BrushError::VertexInside`] when
    /// `to` would lie inside the brush, [`BrushError::CornerLost`] when another vertex would,
    /// and [`BrushError::Degenerate`] when the result has no volume.
    pub fn move_vertex(&self, from: [f64; 3], to: [f64; 3]) -> Result<Self, BrushError> {
        let mut points = self.vertices();
        let moved = points
            .iter_mut()
            .find(|p| same_point(**p, from))
            .ok_or(BrushError::NoVertex(from))?;
        *moved = to.map(round);
        let surface = self
            .faces
            .first()
            .map(Surface::of)
            .ok_or(BrushError::Degenerate)?;
        let brush = Self::hull(&points, &[self], &surface)?;
        let corners = brush.vertices();
        let kept = |point: [f64; 3]| corners.iter().any(|&v| same_point(v, point));
        if !kept(to) {
            return Err(BrushError::VertexInside(to));
        }
        if let Some(&lost) = points.iter().find(|&&p| !kept(p)) {
            return Err(BrushError::CornerLost(lost));
        }
        Ok(brush)
    }
}

#[cfg(test)]
mod tests;
