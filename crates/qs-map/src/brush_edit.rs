// SPDX-License-Identifier: GPL-3.0-or-later

//! Editing brushes (P6-08, MAP-3): a box from two corners, a clip by a plane, a subtraction
//! of one brush from another, the merge of brushes into their convex hull, and a vertex moved.
//! A brush is the convex solid behind every face's plane (MAP-3.4); every operation gives only
//! brushes whose every face has a polygon on that solid and whose solid is bounded, so a
//! compiler never meets a brush with a stray or duplicate face.

use crate::geometry::{
    MIN_CORNERS, ON_PLANE, POLYGON_REACH, Plane, clip_behind, plane as plane_of,
};
use crate::{Brush, Face, MapError, TexDef};

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
            Self::Map(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for BrushError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Map(error) => Some(error),
            Self::Degenerate | Self::NoVertex(_) | Self::VertexInside(_) => None,
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

/// `value` rounded to [`PRECISION`], -0 written as 0.
fn round(value: f64) -> f64 {
    (value * PRECISION).round() / PRECISION + 0.0
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    dot(d, d).sqrt()
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

/// The polygon of the plane at `index` on the solid behind every one of `planes`.
fn polygon_on(planes: &[Plane<f64>], index: usize) -> Vec<[f64; 3]> {
    let Some(plane) = planes.get(index) else {
        return Vec::new();
    };
    let mut polygon = plane.polygon();
    for (other, clip) in planes.iter().enumerate() {
        if other != index && !polygon.is_empty() {
            polygon = clip_behind(&polygon, clip);
        }
    }
    polygon
}

/// Three corners of `polygon`, spread around it, ordered so the plane they give faces along
/// `normal`; `None` for a polygon of fewer than three corners.
fn face_points(polygon: &[[f64; 3]], normal: [f64; 3]) -> Option<[[f64; 3]; 3]> {
    let mut corners: Vec<[f64; 3]> = Vec::new();
    for corner in polygon.iter().map(|p| p.map(round)) {
        if !corners
            .iter()
            .any(|&known| distance(known, corner) < SAME_POINT)
        {
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
        for (index, plane) in planes.iter().enumerate() {
            if planes
                .iter()
                .skip(index.saturating_add(1))
                .any(|other| plane.same(other))
            {
                return false;
            }
            let polygon = polygon_on(&planes, index);
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
        let mut out: Vec<[f64; 3]> = Vec::new();
        for corner in self.polygons().into_iter().flatten() {
            let corner = corner.map(round);
            if !out
                .iter()
                .any(|&known| distance(known, corner) < SAME_POINT)
            {
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
            let keep: Vec<bool> = (0..planes.len())
                .map(|index| polygon_on(&planes, index).len() >= MIN_CORNERS)
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
        let polygon = polygon_on(&planes, planes.len().saturating_sub(1));
        let Some(points) = face_points(&polygon, plane.normal) else {
            return Ok(None);
        };
        let mut brush = self.clone();
        brush
            .faces
            .push(Face::new(points, &surface.texture, surface.texdef)?);
        let brush = brush.pruned();
        Ok(brush.is_valid().then_some(brush))
    }

    /// The parts of the brush behind `plane` and in front of it, the new face of each painted
    /// with `surface`; a part with nothing in it is `None`, and a brush wholly on one side is
    /// that side's part unchanged.
    ///
    /// # Errors
    ///
    /// [`BrushError::Map`] when the new face cannot be written.
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
        Ok((
            self.cut(plane, surface)?,
            self.cut(plane.flipped(), surface)?,
        ))
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
                // The cutter lies wholly behind this face's far side: no overlap.
                return Ok(vec![self.clone()]);
            }
        }
        Ok(pieces)
    }

    /// The convex hull of `points`, each face painted as the face of `from` whose normal is
    /// nearest its own, or with `surface` when `from` has none.
    fn hull(points: &[[f64; 3]], from: &[&Self], surface: &Surface) -> Result<Self, BrushError> {
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
        for (index, plane) in planes.iter().enumerate() {
            let polygon = polygon_on(&planes, index);
            let Some(points) = face_points(&polygon, plane.normal) else {
                continue;
            };
            let paint = faces
                .iter()
                .filter_map(|face| face.plane().map(|p| (dot(p.normal, plane.normal), face)))
                .max_by(|a, b| a.0.total_cmp(&b.0))
                .map_or_else(|| surface.clone(), |(_, face)| Surface::of(face));
            brush
                .faces
                .push(Face::new(points, &paint.texture, paint.texdef)?);
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

    /// The brush with its vertex at `from` moved to `to`: the convex hull of its vertices so
    /// moved, its faces painted as the nearest of its own.
    ///
    /// # Errors
    ///
    /// [`BrushError::NoVertex`] when no vertex is at `from`, [`BrushError::VertexInside`] when
    /// `to` would lie inside the brush, and [`BrushError::Degenerate`] when the result has no
    /// volume.
    pub fn move_vertex(&self, from: [f64; 3], to: [f64; 3]) -> Result<Self, BrushError> {
        let mut points = self.vertices();
        let moved = points
            .iter_mut()
            .find(|p| distance(**p, from) < SAME_POINT)
            .ok_or(BrushError::NoVertex(from))?;
        *moved = to.map(round);
        let surface = self
            .faces
            .first()
            .map(Surface::of)
            .ok_or(BrushError::Degenerate)?;
        let brush = Self::hull(&points, &[self], &surface)?;
        if !brush
            .vertices()
            .iter()
            .any(|&v| distance(v, to) < SAME_POINT)
        {
            return Err(BrushError::VertexInside(to));
        }
        Ok(brush)
    }
}

#[cfg(test)]
mod tests;
