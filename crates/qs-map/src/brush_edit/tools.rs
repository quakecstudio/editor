// SPDX-License-Identifier: GPL-3.0-or-later

//! The brush tools beyond a box, a clip, a subtraction, a merge and a vertex moved (P6-18,
//! MAP-15): a face extruded, a brush scaled or sheared about a point, swept along a path,
//! hollowed and intersected, several vertices moved at once, an edge or a face split by a new
//! vertex, and an edge chamfered into one or more bevel faces. Each gives only valid brushes,
//! and each keeps every face's texture fixed in the world under texture lock.

use super::{BrushError, Surface, TextureLock, round, same_point};
use crate::geometry::{
    MAP_ON_EPSILON, MIN_CORNERS, ON_PLANE, Plane, Turn, centroid, cross, dot, normalize,
    plane as plane_of, polygons_on, texvecs,
};
use crate::{Brush, Face, TexDef};

/// A turn of a circle, in degrees.
const TURN_DEGREES: f64 = 360.0;
/// A rotation within this many degrees of a whole one, 2^-20, is written whole.
const WHOLE_DEGREES: f64 = 1.0 / 1_048_576.0;
/// The rotation that turns a standard face's base s axis onto its base t axis, in degrees.
const QUARTER: f64 = 90.0;
/// The smallest determinant of a linear map that keeps a brush's volume.
const FLAT: f64 = 1e-12;
/// The smallest length a texture axis keeps before it has none.
const NO_LENGTH: f64 = 1e-12;
/// The origin.
const ORIGIN: [f64; 3] = [0.0, 0.0, 0.0];
/// The linear map that changes nothing.
const IDENTITY: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
/// The most bevel faces a chamfer makes (MAP-15.9).
pub const MAP_MAX_CHAMFER_FACES: u32 = 16;

/// A face of a brush, by its place among the brush's faces, from 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BrushFace(usize);

impl BrushFace {
    /// The face at `index`.
    #[must_use]
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    /// Its place among the brush's faces.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

/// An axis of the world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorldAxis {
    /// x.
    X,
    /// y.
    Y,
    /// z.
    Z,
}

impl WorldAxis {
    /// Its place in a point's coordinates.
    const fn index(self) -> usize {
        match self {
            Self::X => 0,
            Self::Y => 1,
            Self::Z => 2,
        }
    }
}

/// An affine map of the world, `p' = linear * p + offset`, `linear` given by rows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    linear: [[f64; 3]; 3],
    offset: [f64; 3],
}

impl Affine {
    /// The scale by `factors` along x, y and z about `about` (MAP-15.2).
    #[must_use]
    pub fn scale(about: [f64; 3], factors: [f64; 3]) -> Self {
        let [fx, fy, fz] = factors;
        Self::about(about, [[fx, 0.0, 0.0], [0.0, fy, 0.0], [0.0, 0.0, fz]])
    }

    /// The shear that moves each point along `along` by `factor` times its distance from
    /// `about` along `by`, the two axes different (MAP-15.2); none when they are one.
    #[must_use]
    pub fn shear(about: [f64; 3], along: WorldAxis, by: WorldAxis, factor: f64) -> Option<Self> {
        if along == by {
            return None;
        }
        let mut linear = IDENTITY;
        if let Some(cell) = linear
            .get_mut(along.index())
            .and_then(|row| row.get_mut(by.index()))
        {
            *cell = factor;
        }
        Some(Self::about(about, linear))
    }

    /// The move by `by`.
    #[must_use]
    pub const fn translation(by: [f64; 3]) -> Self {
        Self {
            linear: IDENTITY,
            offset: by,
        }
    }

    /// The linear map `linear` with `about` kept where it is.
    fn about(about: [f64; 3], linear: [[f64; 3]; 3]) -> Self {
        let moved = Self {
            linear,
            offset: ORIGIN,
        }
        .apply(about);
        Self {
            linear,
            offset: [0, 1, 2].map(|i| {
                about.get(i).copied().unwrap_or_default()
                    - moved.get(i).copied().unwrap_or_default()
            }),
        }
    }

    /// `point` mapped.
    #[must_use]
    pub fn apply(&self, point: [f64; 3]) -> [f64; 3] {
        let [a, b, c] = self.linear;
        let [ox, oy, oz] = self.offset;
        [dot(a, point) + ox, dot(b, point) + oy, dot(c, point) + oz]
    }

    /// The linear part's determinant.
    fn determinant(&self) -> f64 {
        let [a, b, c] = self.linear;
        dot(a, cross(b, c))
    }

    /// The inverse of the linear part, transposed: what a direction a texture is projected
    /// along becomes so that each point keeps its texel; none for a map that flattens.
    fn inverse_transpose(&self) -> Option<[[f64; 3]; 3]> {
        let det = self.determinant();
        if det.abs() <= FLAT {
            return None;
        }
        let [a, b, c] = self.linear;
        // The cofactors, rows of the inverse transposed.
        Some([cross(b, c), cross(c, a), cross(a, b)].map(|row| row.map(|v| v / det)))
    }
}

/// A face's texture in the world: texel `i` of a point `p` on it is `dot(p, axes[i]) +
/// shift[i]` (MAP-4.2 to MAP-4.4, MAP-5.3).
#[derive(Clone, Copy, Debug, PartialEq)]
struct WorldTexture {
    axes: [[f64; 3]; 2],
    shift: [f64; 2],
}

impl WorldTexture {
    /// The texture of `face`; none when it has none (MAP-5.4).
    fn of(face: &Face) -> Option<Self> {
        let [[sx, sy, sz, ss], [tx, ty, tz, ts]] = face.texvecs()?;
        Some(Self {
            axes: [[sx, sy, sz], [tx, ty, tz]],
            shift: [ss, ts],
        })
    }

    /// The texture that gives each point `map` moves the texel its point had (MAP-15.1).
    fn mapped(self, map: &Affine) -> Option<Self> {
        let [r0, r1, r2] = map.inverse_transpose()?;
        let axes = self.axes.map(|v| [dot(r0, v), dot(r1, v), dot(r2, v)]);
        let shift = [0, 1].map(|i| {
            let axis = axes.get(i).copied().unwrap_or_default();
            self.shift.get(i).copied().unwrap_or_default() - dot(axis, map.offset)
        });
        Some(Self { axes, shift })
    }

    /// The texel of `point` along axis `i`.
    fn texel(&self, i: usize, point: [f64; 3]) -> f64 {
        dot(self.axes.get(i).copied().unwrap_or_default(), point)
            + self.shift.get(i).copied().unwrap_or_default()
    }

    /// A definition in `form`'s form that puts this texture on the plane `plane`: exactly in
    /// the Valve 220 form, and in the standard form the nearest rotation and scales, exact
    /// unless the texture is sheared on the plane (MAP-15.1).
    fn on(self, plane: &Plane<f64>, form: &TexDef) -> TexDef {
        match *form {
            TexDef::Valve { rotate, .. } => {
                // A face's axes have length: MAP-5.4 refuses a scale of 0.
                let [s, t] = self.axes.map(|axis| {
                    let length = dot(axis, axis).sqrt();
                    (axis.map(|c| c / length), 1.0 / length)
                });
                let ([sx, sy, sz], s_scale) = s;
                let ([tx, ty, tz], t_scale) = t;
                TexDef::Valve {
                    s: [sx, sy, sz, self.shift[0]],
                    t: [tx, ty, tz, self.shift[1]],
                    rotate,
                    scale: [s_scale, t_scale],
                }
            }
            TexDef::Standard { .. } => self.standard_on(plane),
        }
    }

    /// The standard definition nearest the texture on `plane`.
    fn standard_on(self, plane: &Plane<f64>) -> TexDef {
        let n = plane.normal;
        let base = |rotate: f64| {
            let [[sx, sy, sz, _], [tx, ty, tz, _]] = texvecs(n, [0.0; 2], rotate, [1.0; 2]);
            [[sx, sy, sz], [tx, ty, tz]]
        };
        let [s0, _] = base(0.0);
        let [s90, _] = base(QUARTER);
        // The axis the face is projected along: neither base axis has a part along it, and
        // the face's normal has, the base axes being those of the axis nearest it (MAP-4.2).
        let projection = normalize(cross(s0, s90));
        let flat = |axis: [f64; 3]| {
            let k = dot(axis, projection) / dot(n, projection);
            [0, 1, 2].map(|i| {
                axis.get(i).copied().unwrap_or_default() - k * n.get(i).copied().unwrap_or_default()
            })
        };
        let [s_axis, t_axis] = self.axes.map(flat);
        let (along, across) = (dot(s_axis, s0), dot(s_axis, s90));
        let length = along.hypot(across);
        let rotate = tidied(across.atan2(along).to_degrees()).rem_euclid(TURN_DEGREES);
        let [_, t_turned] = base(rotate);
        let t_length = dot(t_axis, t_turned);
        let scale = [
            if length <= NO_LENGTH {
                1.0
            } else {
                1.0 / length
            },
            if t_length.abs() <= NO_LENGTH {
                1.0
            } else {
                1.0 / t_length
            },
        ];
        let [[sx, sy, sz, _], [tx, ty, tz, _]] = texvecs(n, [0.0; 2], rotate, scale);
        let point = n.map(|c| c * plane.dist);
        let shift = [
            self.texel(0, point) - dot(point, [sx, sy, sz]),
            self.texel(1, point) - dot(point, [tx, ty, tz]),
        ];
        TexDef::Standard {
            shift,
            rotate,
            scale,
        }
    }
}

/// `value` as the nearest whole number when it is within [`WHOLE_DEGREES`] of one.
fn tidied(value: f64) -> f64 {
    let whole = value.round();
    if (value - whole).abs() < WHOLE_DEGREES {
        whole
    } else {
        value
    }
}

impl Face {
    /// The face moved by `map`: its points mapped, in the other order when the map mirrors,
    /// and under texture lock its texture kept where it was in the world (MAP-15.1).
    fn mapped(&self, map: &Affine, lock: TextureLock) -> Result<Self, BrushError> {
        let mut points = self.points().map(|p| map.apply(p).map(round));
        if map.determinant().is_sign_negative() {
            points.reverse();
        }
        let plane = plane_of(points).ok_or(BrushError::Degenerate)?;
        let texdef = match lock {
            TextureLock::Locked => WorldTexture::of(self)
                .and_then(|texture| texture.mapped(map))
                .map_or(*self.texdef(), |texture| texture.on(&plane, self.texdef())),
            TextureLock::Free => *self.texdef(),
        };
        let mut moved = self.clone();
        let texture = self.texture().to_vec();
        moved.set(points, &texture, texdef)?;
        Ok(moved)
    }
}

impl Brush {
    /// Whether the brush has volume: some vertex is well behind every face's plane.
    fn has_volume(&self) -> bool {
        let corners = self.vertices();
        self.faces
            .iter()
            .filter_map(Face::plane)
            .all(|plane| corners.iter().any(|&corner| plane.side(corner) < -ON_PLANE))
    }

    /// Whether a vertex of the brush lies in front of one of `planes` by qbsp's `ON_EPSILON`
    /// or more: a result that should lie within them but reaches out, as a sliver of a solid
    /// the clips' tolerance cannot place does (MAP-15.10).
    fn reaches_out(&self, planes: &[Plane<f64>]) -> bool {
        self.vertices()
            .iter()
            .any(|&v| planes.iter().any(|p| p.side(v) >= MAP_ON_EPSILON))
    }

    /// Whether the brush is valid and has volume.
    fn is_solid(&self) -> bool {
        self.is_valid() && self.has_volume()
    }

    /// Each face with a polygon on the solid of these faces, the first of any two on one
    /// plane; [`BrushError::Degenerate`] when they bound no valid brush.
    fn of_faces(faces: Vec<Face>) -> Result<Self, BrushError> {
        let mut planes: Vec<Plane<f64>> = Vec::new();
        let mut kept: Vec<Face> = Vec::new();
        for face in faces {
            let plane = face.plane().ok_or(BrushError::Degenerate)?;
            if !planes.iter().any(|known| known.same(&plane)) {
                planes.push(plane);
                kept.push(face);
            }
        }
        let polygons = polygons_on(&planes);
        let mut brush = Self::new();
        brush.faces = kept
            .into_iter()
            .zip(polygons)
            .filter(|(_, polygon)| polygon.len() >= MIN_CORNERS)
            .map(|(face, _)| face)
            .collect();
        if brush.is_solid() {
            Ok(brush)
        } else {
            Err(BrushError::Degenerate)
        }
    }

    /// The brush mapped by `map`, every face with it, its textures fixed in the world under
    /// `lock` (MAP-15.1, MAP-15.2).
    ///
    /// # Errors
    ///
    /// [`BrushError::Degenerate`] when the map flattens the brush, and [`BrushError::Map`] when
    /// a face cannot be written.
    pub fn transformed(&self, map: &Affine, lock: TextureLock) -> Result<Self, BrushError> {
        if map.determinant().abs() <= FLAT {
            return Err(BrushError::Degenerate);
        }
        let mut brush = self.clone();
        for face in &mut brush.faces {
            *face = face.mapped(map, lock)?;
        }
        if brush.is_solid() {
            Ok(brush)
        } else {
            Err(BrushError::Degenerate)
        }
    }

    /// The brush with face `face` moved `by` along its outward normal, out for a positive
    /// distance and in for a negative one; the other faces keep their planes and text, and a
    /// face the move leaves with no polygon goes (MAP-15.3).
    ///
    /// # Errors
    ///
    /// [`BrushError::Degenerate`] when there is no such face or the brush would have no
    /// volume, and [`BrushError::Map`] when the face cannot be written.
    pub fn extruded(
        &self,
        face: BrushFace,
        by: f64,
        lock: TextureLock,
    ) -> Result<Self, BrushError> {
        let moved = self.faces.get(face.get()).ok_or(BrushError::Degenerate)?;
        let normal = moved.plane().ok_or(BrushError::Degenerate)?.normal;
        if by.is_nan() || by.abs() < ON_PLANE {
            return Err(BrushError::Degenerate);
        }
        let shifted = moved.mapped(&Affine::translation(normal.map(|c| c * by)), lock)?;
        let mut faces = self.faces.clone();
        if let Some(slot) = faces.get_mut(face.get()) {
            *slot = shifted;
        }
        Self::of_faces(faces)
    }

    /// One brush for each step of `path` after its first: the hull of the brush moved to the
    /// step's start and its end, each relative to the path's first point, with the textures of
    /// the brush there under `lock` (MAP-15.4).
    ///
    /// # Errors
    ///
    /// [`BrushError::Degenerate`] for a path of fewer than two points or a step of no length,
    /// and [`BrushError::Map`] when a face cannot be written.
    pub fn swept(&self, path: &[[f64; 3]], lock: TextureLock) -> Result<Vec<Self>, BrushError> {
        let (&first, rest) = path.split_first().ok_or(BrushError::Degenerate)?;
        if rest.is_empty() {
            return Err(BrushError::Degenerate);
        }
        let offset = |p: [f64; 3]| [p[0] - first[0], p[1] - first[1], p[2] - first[2]];
        let surface = self
            .faces
            .first()
            .map(Surface::of)
            .ok_or(BrushError::Degenerate)?;
        let mut out = Vec::with_capacity(rest.len());
        let mut from = self.translated(offset(first), lock)?;
        let mut from_corners = from.vertices();
        for &to in rest {
            let next = self.translated(offset(to), lock)?;
            let next_corners = next.vertices();
            if from_corners == next_corners {
                return Err(BrushError::Degenerate);
            }
            let mut points = from_corners;
            points.extend_from_slice(&next_corners);
            out.push(Self::hull(&points, &[&from, &next], &surface)?);
            from = next;
            from_corners = next_corners;
        }
        Ok(out)
    }

    /// The brush's walls `wall` thick: the brush with the brush shrunk by `wall` on every face
    /// taken out, each inner face painted as the face it is under (MAP-15.5).
    ///
    /// # Errors
    ///
    /// [`BrushError::Degenerate`] for a wall that is not positive or that leaves no room
    /// inside, and [`BrushError::Map`] when a face cannot be written.
    pub fn hollowed(&self, wall: f64) -> Result<Vec<Self>, BrushError> {
        if wall.is_nan() || wall <= 0.0 {
            return Err(BrushError::Degenerate);
        }
        let mut inner = Vec::with_capacity(self.faces.len());
        for face in &self.faces {
            let normal = face.plane().ok_or(BrushError::Degenerate)?.normal;
            inner.push(face.mapped(
                &Affine::translation(normal.map(|c| -c * wall)),
                TextureLock::Locked,
            )?);
        }
        let inner = Self::of_faces(inner)?;
        if inner.faces.len() != self.faces.len() {
            return Err(BrushError::Degenerate);
        }
        let walls = self.subtract(&inner)?;
        let planes = self.planes().ok_or(BrushError::Degenerate)?;
        if walls.iter().any(|wall| wall.reaches_out(&planes)) {
            return Err(BrushError::Degenerate);
        }
        Ok(walls)
    }

    /// The solid every one of this brush and `others` holds, each face kept as the brush it
    /// comes from has it (MAP-15.6).
    ///
    /// # Errors
    ///
    /// [`BrushError::Degenerate`] when the brushes share no volume.
    pub fn intersected(&self, others: &[&Self]) -> Result<Self, BrushError> {
        let mut faces = self.faces.clone();
        for other in others {
            faces.extend(other.faces.iter().cloned());
        }
        let both = Self::of_faces(faces)?;
        let mut planes = self.planes().ok_or(BrushError::Degenerate)?;
        for other in others {
            planes.extend(other.planes().ok_or(BrushError::Degenerate)?);
        }
        if both.reaches_out(&planes) {
            return Err(BrushError::Degenerate);
        }
        Ok(both)
    }

    /// The brush with each vertex at a `from` of `moves` moved to its `to`, all at once: the
    /// convex hull of its vertices so moved, painted as the nearest of its faces (MAP-15.7).
    ///
    /// # Errors
    ///
    /// [`BrushError::NoVertex`] for a `from` that is no vertex, [`BrushError::VertexInside`]
    /// for a `to` that would not be a vertex, [`BrushError::CornerLost`] for another vertex
    /// that would not, and [`BrushError::Degenerate`] when the result has no volume.
    pub fn moved_vertices(&self, moves: &[([f64; 3], [f64; 3])]) -> Result<Self, BrushError> {
        let mut points = self.vertices();
        // Each vertex moves once, matched where it was before any move.
        let before = points.clone();
        let mut moved: Vec<usize> = Vec::with_capacity(moves.len());
        for &(from, to) in moves {
            let index = before
                .iter()
                .enumerate()
                .position(|(i, &p)| same_point(p, from) && !moved.contains(&i))
                .ok_or(BrushError::NoVertex(from))?;
            if let Some(point) = points.get_mut(index) {
                *point = to.map(round);
            }
            moved.push(index);
        }
        let placed: Vec<[f64; 3]> = moved
            .iter()
            .filter_map(|&i| points.get(i).copied())
            .collect();
        self.hull_keeping(&points, &placed)
    }

    /// The hull of `points` painted as this brush, refusing one that loses a point of
    /// `points`, with [`BrushError::VertexInside`] for one of `new` and
    /// [`BrushError::CornerLost`] for another.
    fn hull_keeping(&self, points: &[[f64; 3]], new: &[[f64; 3]]) -> Result<Self, BrushError> {
        let surface = self
            .faces
            .first()
            .map(Surface::of)
            .ok_or(BrushError::Degenerate)?;
        let brush = Self::hull(points, &[self], &surface)?;
        let corners = brush.vertices();
        let kept = |point: [f64; 3]| corners.iter().any(|&v| same_point(v, point));
        if let Some(&lost) = new.iter().find(|&&p| !kept(p)) {
            return Err(BrushError::VertexInside(lost));
        }
        if let Some(&lost) = points.iter().find(|&&p| !kept(p)) {
            return Err(BrushError::CornerLost(lost));
        }
        Ok(brush)
    }

    /// The faces whose polygons hold both `a` and `b`: the two of an edge.
    fn faces_of_edge(&self, a: [f64; 3], b: [f64; 3]) -> Vec<(usize, Vec<[f64; 3]>)> {
        self.face_polygons()
            .into_iter()
            .enumerate()
            .filter_map(|(i, polygon)| polygon.map(|polygon| (i, polygon)))
            .filter(|(_, polygon)| {
                let holds = |p: [f64; 3]| polygon.iter().any(|&q| same_point(q, p));
                holds(a) && holds(b)
            })
            .collect()
    }

    /// The point `by` out from the middle of face `face`'s polygon along its normal: where
    /// [`Brush::split_face`] raises a face to (MAP-15.8); none for no such face.
    #[must_use]
    pub fn face_out(&self, face: BrushFace, by: f64) -> Option<[f64; 3]> {
        let normal = self.faces.get(face.get())?.plane()?.normal;
        let polygon = self.face_polygons().into_iter().nth(face.get())??;
        let middle = centroid(&polygon);
        Some([0, 1, 2].map(|i| {
            middle.get(i).copied().unwrap_or_default()
                + normal.get(i).copied().unwrap_or_default() * by
        }))
    }

    /// The point `by` out from the middle of the edge from `a` to `b`, along the mean of its
    /// two faces' normals: where [`Brush::split_edge`] raises an edge to (MAP-15.8); none when
    /// the pair is no edge.
    #[must_use]
    pub fn edge_out(&self, a: [f64; 3], b: [f64; 3], by: f64) -> Option<[f64; 3]> {
        let sides = self.faces_of_edge(a, b);
        if sides.len() != 2 {
            return None;
        }
        let mut out = ORIGIN;
        for (i, _) in &sides {
            let normal = self.faces.get(*i)?.plane()?.normal;
            for (o, n) in out.iter_mut().zip(normal) {
                *o += n;
            }
        }
        let out = normalize(out);
        let middle = centroid(&[a, b]);
        Some([0, 1, 2].map(|i| {
            middle.get(i).copied().unwrap_or_default()
                + out.get(i).copied().unwrap_or_default() * by
        }))
    }

    /// The brush with its edge from `a` to `b` split by a new vertex at `to` (MAP-15.8).
    ///
    /// # Errors
    ///
    /// [`BrushError::NoVertex`] when `a` and `b` are no edge, [`BrushError::VertexInside`] when
    /// `to` would not be a vertex, [`BrushError::CornerLost`] when another vertex would not.
    pub fn split_edge(&self, a: [f64; 3], b: [f64; 3], to: [f64; 3]) -> Result<Self, BrushError> {
        if same_point(a, b) || self.faces_of_edge(a, b).len() < 2 {
            return Err(BrushError::NoVertex(b));
        }
        let mut points = self.vertices();
        let to = to.map(round);
        points.push(to);
        self.hull_keeping(&points, &[to])
    }

    /// The brush with face `face` split by a new vertex at `to`, in front of it (MAP-15.8).
    ///
    /// # Errors
    ///
    /// [`BrushError::Degenerate`] for no such face, [`BrushError::VertexInside`] when `to` is
    /// not in front of it or would not be a vertex, [`BrushError::CornerLost`] when another
    /// vertex would not.
    pub fn split_face(&self, face: BrushFace, to: [f64; 3]) -> Result<Self, BrushError> {
        let plane = self
            .faces
            .get(face.get())
            .and_then(Face::plane)
            .ok_or(BrushError::Degenerate)?;
        let to = to.map(round);
        if plane.side(to) <= ON_PLANE {
            return Err(BrushError::VertexInside(to));
        }
        let mut points = self.vertices();
        points.push(to);
        self.hull_keeping(&points, &[to])
    }

    /// The brush with its edge from `a` to `b` cut off by `faces` bevel faces, from `distance`
    /// along each of the edge's two faces, on an arc tangent to both, each painted as the
    /// first of them (MAP-15.9).
    ///
    /// # Errors
    ///
    /// [`BrushError::NoVertex`] when `a` and `b` are no edge, [`BrushError::Degenerate`] for
    /// a distance that is not positive, a count of faces from 1 to
    /// [`MAP_MAX_CHAMFER_FACES`] not given, or a bevel that cuts the brush away, and
    /// [`BrushError::Map`] when a face cannot be written.
    pub fn chamfered(
        &self,
        a: [f64; 3],
        b: [f64; 3],
        distance: f64,
        faces: u32,
    ) -> Result<Self, BrushError> {
        if distance.is_nan() || distance <= 0.0 || !(1..=MAP_MAX_CHAMFER_FACES).contains(&faces) {
            return Err(BrushError::Degenerate);
        }
        let sides = self.faces_of_edge(a, b);
        let ([(first, first_polygon), (second, second_polygon)], true) = (
            [0, 1].map(|i| sides.get(i).cloned().unwrap_or_default()),
            sides.len() == 2,
        ) else {
            return Err(BrushError::NoVertex(b));
        };
        let edge = normalize([b[0] - a[0], b[1] - a[1], b[2] - a[2]]);
        let normal_of = |i: usize| {
            self.faces
                .get(i)
                .and_then(Face::plane)
                .map(|p| p.normal)
                .ok_or(BrushError::Degenerate)
        };
        // Along each face, at right angles to the edge, into the face.
        let into = |normal: [f64; 3], polygon: &[[f64; 3]]| {
            let w = normalize(cross(normal, edge));
            let middle = centroid(polygon);
            if dot(w, [middle[0] - a[0], middle[1] - a[1], middle[2] - a[2]]) < 0.0 {
                w.map(|c| -c)
            } else {
                w
            }
        };
        let w1 = into(normal_of(first)?, &first_polygon);
        let w2 = into(normal_of(second)?, &second_polygon);
        // The bevel must start within each face: short of its far side from the edge.
        let width = |w: [f64; 3], polygon: &[[f64; 3]]| {
            polygon
                .iter()
                .map(|p| dot(w, [p[0] - a[0], p[1] - a[1], p[2] - a[2]]))
                .fold(0.0_f64, f64::max)
        };
        if distance >= width(w1, &first_polygon) - ON_PLANE
            || distance >= width(w2, &second_polygon) - ON_PLANE
        {
            return Err(BrushError::Degenerate);
        }
        let half = dot(w1, w2).clamp(-1.0, 1.0).acos() / 2.0;
        let towards = normalize([w1[0] + w2[0], w1[1] + w2[1], w1[2] + w2[2]]);
        let radius = distance * half.tan();
        let centre = [0, 1, 2].map(|i| {
            a.get(i).copied().unwrap_or_default()
                + towards.get(i).copied().unwrap_or_default() * distance / half.cos()
        });
        let start = [0, 1, 2].map(|i| {
            a.get(i).copied().unwrap_or_default()
                + w1.get(i).copied().unwrap_or_default() * distance
        });
        let end = [0, 1, 2].map(|i| {
            a.get(i).copied().unwrap_or_default()
                + w2.get(i).copied().unwrap_or_default() * distance
        });
        let sweep = core::f64::consts::PI - 2.0 * half;
        let out_of =
            |p: [f64; 3]| normalize([p[0] - centre[0], p[1] - centre[1], p[2] - centre[2]]);
        // The turn about the edge that takes the start toward the end.
        let axis = if dot(cross(out_of(start), out_of(end)), edge) >= 0.0 {
            edge
        } else {
            edge.map(|c| -c)
        };
        let steps = f64::from(faces);
        let arc: Vec<[f64; 3]> = (0..=faces)
            .map(|k| {
                if k == 0 {
                    return start;
                }
                if k == faces {
                    return end;
                }
                let turned = Turn::about(axis, sweep * f64::from(k) / steps).of(out_of(start));
                [0, 1, 2].map(|i| {
                    centre.get(i).copied().unwrap_or_default()
                        + turned.get(i).copied().unwrap_or_default() * radius
                })
            })
            .collect();
        let surface = self
            .faces
            .get(first)
            .map(Surface::of)
            .ok_or(BrushError::Degenerate)?;
        let mut brush = self.clone();
        for pair in arc.windows(2) {
            let (Some(&p), Some(&q)) = (pair.first(), pair.get(1)) else {
                continue;
            };
            let along = [p[0] + edge[0], p[1] + edge[1], p[2] + edge[2]];
            let mut plane = plane_of([p, q, along]).ok_or(BrushError::Degenerate)?;
            if plane.side(centre) > 0.0 {
                plane = plane.flipped();
            }
            brush = brush.cut(plane, &surface)?.ok_or(BrushError::Degenerate)?;
        }
        let corners = brush.vertices();
        if !brush.is_solid()
            || corners
                .iter()
                .any(|&v| same_point(v, a) || same_point(v, b))
        {
            return Err(BrushError::Degenerate);
        }
        if brush.reaches_out(&self.planes().ok_or(BrushError::Degenerate)?) {
            return Err(BrushError::Degenerate);
        }
        Ok(brush)
    }
}

#[cfg(test)]
mod tests;
