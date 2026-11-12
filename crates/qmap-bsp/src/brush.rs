// SPDX-License-Identifier: GPL-3.0-or-later

//! Brushes as qbsp loads them for one hull (QBSP-4) and grows them for the clipping hulls
//! (QBSP-5): each face's polygon, the brush's contents and bounds.

use qs_map::qbsp::{QbspEntity, QbspFace, QbspMap};

use crate::math::{Vec3, add, cross, dot, length, normalize, rint, same, sub, within};
use crate::planes::{ANGLE_EPSILON, Plane, Planes};
use crate::winding::{KeepOn, ON_EPSILON, base_for_plane, clip};
use crate::{CompileError, Contents, Face, Faults, Hull, Limits, ModelKind};

/// How near a whole number a coordinate is set to it (QBSP-4.4).
const ZERO_EPSILON: f64 = 0.001;
/// The largest coordinate a face may have (QBSP-4.5).
const BOGUS_RANGE: f32 = 18000.0;
/// How far from 1 a plane's normal length may be when it is added (QBSP-5.3).
const NORMAL_LENGTH_SLACK: f64 = 0.001;
/// The prefixes of liquid textures and their contents (QBSP-4.1).
const LAVA: &[u8] = b"lava";
const SLIME: &[u8] = b"slime";
/// The prefix of a sky texture (QBSP-4.1).
const SKY: &[u8] = b"sky";
/// The clip texture (QBSP-4.1).
const CLIP: &[u8] = b"clip";

/// A plane of a brush being loaded, qbsp's `mface_t`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct BrushPlane {
    plane: Plane,
    texinfo: usize,
}

/// A loaded brush (QBSP-4).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Brush {
    pub(crate) faces: Vec<Face>,
    pub(crate) mins: Vec3,
    pub(crate) maxs: Vec3,
    pub(crate) contents: Contents,
}

/// An entity's brushes for one hull, in CSG order, and their bounds (QBSP-4.6).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct BrushSet {
    pub(crate) brushes: Vec<Brush>,
    pub(crate) mins: Vec3,
    pub(crate) maxs: Vec3,
}

/// What loading one brush gave.
enum Loaded {
    Brush(Brush),
    LeftOut,
    NoFaces,
}

/// `Q_strncasecmp` against a prefix.
fn starts_with_ignoring_case(name: &[u8], prefix: &[u8]) -> bool {
    name.get(..prefix.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
}

/// A brush's contents by its stored first face's texture, the last face in the file; `None`
/// when the hull leaves it out (QBSP-4.1, QBSP-4.2).
fn contents_of(name: &[u8], hull: Hull, model: ModelKind) -> Option<Contents> {
    if name.eq_ignore_ascii_case(CLIP) && hull == Hull::Draw {
        return None;
    }
    let contents = if name.first() == Some(&b'*') && model == ModelKind::World {
        let rest = name.get(1..).unwrap_or_default();
        if starts_with_ignoring_case(rest, LAVA) {
            Contents::Lava
        } else if starts_with_ignoring_case(rest, SLIME) {
            Contents::Slime
        } else {
            Contents::Water
        }
    } else if starts_with_ignoring_case(name, SKY)
        && model == ModelKind::World
        && hull == Hull::Draw
    {
        Contents::Sky
    } else {
        Contents::Solid
    };
    match (hull, contents) {
        (Hull::Draw, _) | (_, Contents::Solid | Contents::Sky) => Some(contents),
        _ => None,
    }
}

impl Brush {
    /// One brush of the map, `LoadBrush` (QBSP-4.1 to QBSP-4.5, QBSP-5).
    fn load(
        faces: &[QbspFace],
        map: &QbspMap,
        hull: Hull,
        model: ModelKind,
        planes: &mut Planes,
        limits: Limits,
        warnings: &mut Vec<crate::Warning>,
    ) -> Result<Loaded, CompileError> {
        // qbsp keeps a brush's faces with the last in the file first (QBSP-4.1).
        let mut stored: Vec<BrushPlane> = faces
            .iter()
            .rev()
            .map(|face| BrushPlane {
                plane: Plane {
                    normal: face.plane.normal,
                    dist: face.plane.dist,
                },
                texinfo: if hull == Hull::Draw { face.texinfo } else { 0 },
            })
            .collect();
        let name = faces
            .last()
            .and_then(|face| map.texinfo.get(face.texinfo))
            .and_then(|texinfo| map.miptex.get(texinfo.miptex))
            .map_or(&[][..], Vec::as_slice);
        let Some(contents) = contents_of(name, hull, model) else {
            return Ok(Loaded::LeftOut);
        };
        let Some(mut made) = create_faces(&stored, planes, limits, warnings)? else {
            return Ok(Loaded::NoFaces);
        };
        if let Some(size) = hull.size() {
            expand(&mut stored, &made, size, limits)?;
            match create_faces(&stored, planes, limits, warnings)? {
                Some(grown) => made = grown,
                None => return Ok(Loaded::NoFaces),
            }
        }
        let (faces, mins, maxs) = made;
        Ok(Loaded::Brush(Self {
            faces: faces
                .into_iter()
                .map(|mut face| {
                    face.contents = [Contents::Empty, contents];
                    face
                })
                .collect(),
            mins,
            maxs,
            contents,
        }))
    }
}

/// The faces of the solid behind every stored plane, `CreateBrushFaces`, in qbsp's list
/// order, and their bounds; `None` when no face is left (QBSP-4.3 to QBSP-4.5).
#[expect(
    clippy::type_complexity,
    reason = "the faces and the two corners of their bounds, which only the loader reads"
)]
fn create_faces(
    stored: &[BrushPlane],
    planes: &mut Planes,
    limits: Limits,
    warnings: &mut Vec<crate::Warning>,
) -> Result<Option<(Vec<Face>, Vec3, Vec3)>, CompileError> {
    let mut mins = [99999.0_f32; 3];
    let mut maxs = [-99999.0_f32; 3];
    let mut faces = Vec::new();
    for (i, face) in stored.iter().enumerate() {
        let mut winding = Some(base_for_plane(face.plane));
        for (j, other) in stored.iter().enumerate() {
            let Some(points) = winding.take() else {
                break;
            };
            if j == i {
                winding = Some(points);
                continue;
            }
            winding = clip(&points, other.plane.flipped(), KeepOn::Drop)?;
        }
        let Some(points) = winding else {
            continue;
        };
        let points: Vec<Vec3> = points
            .into_iter()
            .map(|point| {
                point.map(|c| {
                    let whole = rint(c);
                    if within(c, whole, ZERO_EPSILON) {
                        whole
                    } else {
                        c
                    }
                })
            })
            .collect();
        for point in &points {
            for axis in 0..3 {
                mins[axis] = mins[axis].min(point[axis]);
                maxs[axis] = maxs[axis].max(point[axis]);
            }
        }
        let (plane, side) = planes.find(face.plane)?;
        let made = Face {
            plane,
            side,
            texinfo: face.texinfo,
            contents: [Contents::Empty, Contents::Solid],
            points,
            original: None,
        };
        match (check_face(&made, planes), limits.faults) {
            (Ok(()), _) => {}
            (Err(error), Faults::Stop) => return Err(error),
            (Err(error), Faults::Warn) => warnings.push(crate::Warning::BadFace(error.to_string())),
        }
        // qbsp adds each face to the front of the brush's list.
        faces.insert(0, made);
    }
    if faces.is_empty() {
        return Ok(None);
    }
    Ok(Some((faces, mins, maxs)))
}

/// `CheckFace`: the faults that stop qbsp, point by point in its order (QBSP-4.5).
fn check_face(face: &Face, planes: &Planes) -> Result<(), CompileError> {
    let count = face.points.len();
    if count < 3 {
        return Err(CompileError::FacePoints(count));
    }
    let plane = planes.get(face.plane);
    let normal = planes.facing(face.plane, face.side).normal;
    for (i, &p1) in face.points.iter().enumerate() {
        if let Some(&c) = p1.iter().find(|c| c.abs() > BOGUS_RANGE) {
            return Err(CompileError::BogusRange(c));
        }
        let d = plane.distance(p1);
        if !(-ON_EPSILON..=ON_EPSILON).contains(&f64::from(d)) {
            return Err(CompileError::PointOffPlane);
        }
        let p2 = face
            .points
            .get((i.saturating_add(1)) % count)
            .copied()
            .unwrap_or(p1);
        let dir = sub(p2, p1);
        if length(dir) < ON_EPSILON {
            return Err(CompileError::DegenerateEdge);
        }
        let edge_normal = normalize(cross(normal, dir));
        // `edgedist += ON_EPSILON` adds in double and stores the float.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "spec QBSP-4.5: qbsp stores the double sum in a vec_t"
        )]
        let edge_dist = (f64::from(dot(p1, edge_normal)) + ON_EPSILON) as f32;
        for (j, &other) in face.points.iter().enumerate() {
            if j != i && dot(other, edge_normal) > edge_dist {
                return Err(CompileError::NonConvex);
            }
        }
    }
    Ok(())
}

/// `VectorLength` stored in a `vec_t`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec QBSP-5.3: qbsp keeps the double length in a float"
)]
fn float_length(v: Vec3) -> f32 {
    length(v) as f32
}

/// A brush's planes while it is grown, with its vertices and edges so far (QBSP-5).
struct Growth<'a> {
    stored: &'a mut Vec<BrushPlane>,
    limits: Limits,
    size: [Vec3; 2],
    points: Vec<Vec3>,
    corners: Vec<Vec3>,
    edges: Vec<[usize; 2]>,
}

impl Growth<'_> {
    /// `AddBrushPlane` (QBSP-5.3, QBSP-5.6).
    fn add_plane(&mut self, plane: Plane) -> Result<(), CompileError> {
        if Some(self.stored.len()) == self.limits.brush_faces
            || self.stored.len() >= self.limits.brush_planes
        {
            return Err(CompileError::TooManyBrushFaces);
        }
        // `float l = VectorLength (...)`: the length kept as a float.
        let len = f64::from(float_length(plane.normal));
        if !(1.0 - NORMAL_LENGTH_SLACK..=1.0 + NORMAL_LENGTH_SLACK).contains(&len) {
            return Err(CompileError::BadNormal);
        }
        if self.stored.iter().any(|known| {
            same(known.plane.normal, plane.normal)
                && within(known.plane.dist, plane.dist, ON_EPSILON)
        }) {
            return Ok(());
        }
        let texinfo = self.stored.first().map_or(0, |first| first.texinfo);
        self.stored.push(BrushPlane { plane, texinfo });
        Ok(())
    }

    /// `AddHullPoint`: the point's index, adding it and its eight corners when new.
    fn add_point(&mut self, point: Vec3) -> Result<usize, CompileError> {
        if let Some(index) = self.points.iter().position(|&known| same(known, point)) {
            return Ok(index);
        }
        if self.points.len() >= self.limits.hull_points {
            return Err(CompileError::TooManyHullPoints);
        }
        let [low, high] = self.size;
        for x in [low[0], high[0]] {
            for y in [low[1], high[1]] {
                for z in [low[2], high[2]] {
                    self.corners.push(add(point, [x, y, z]));
                }
            }
        }
        self.points.push(point);
        Ok(self.points.len().saturating_sub(1))
    }

    /// `TestAddPlane`: adds `plane`, turned so every corner is behind or on it, when all the
    /// corners are on one side and no plane has it either way (QBSP-5.4).
    fn test_add(&mut self, plane: Plane) -> Result<(), CompileError> {
        let flipped = plane.flipped();
        if self.stored.iter().any(|known| {
            (same(plane.normal, known.plane.normal)
                && within(plane.dist, known.plane.dist, ON_EPSILON))
                || (same(flipped.normal, known.plane.normal)
                    && f64::from((plane.dist + known.plane.dist).abs()) < ON_EPSILON)
        }) {
            return Ok(());
        }
        let (mut front, mut back) = (false, false);
        for &corner in &self.corners {
            let d = f64::from(plane.distance(corner));
            if d < -ON_EPSILON {
                if front {
                    return Ok(());
                }
                back = true;
            } else if d > ON_EPSILON {
                if back {
                    return Ok(());
                }
                front = true;
            }
        }
        self.add_plane(if front { flipped } else { plane })
    }

    /// `AddHullEdge`: the bevels around an edge, once per edge (QBSP-5.4).
    fn add_edge(&mut self, p1: Vec3, p2: Vec3) -> Result<(), CompileError> {
        let a = self.add_point(p1)?;
        let b = self.add_point(p2)?;
        if self
            .edges
            .iter()
            .any(|&[x, y]| (x == a && y == b) || (x == b && y == a))
        {
            return Ok(());
        }
        if self.edges.len() >= self.limits.hull_edges {
            return Err(CompileError::TooManyHullEdges);
        }
        self.edges.push([a, b]);
        let edge = normalize(sub(p1, p2));
        for axis in 0..3 {
            let (b, c) = ((axis + 1) % 3, (axis + 2) % 3);
            for d in 0..2 {
                for e in 0..2 {
                    let mut origin = p1;
                    origin[b] += self.size.get(d).map_or(0.0, |corner| corner[b]);
                    origin[c] += self.size.get(e).map_or(0.0, |corner| corner[c]);
                    let mut unit = [0.0; 3];
                    unit[axis] = 1.0;
                    let normal = cross(unit, edge);
                    let len = f64::from(float_length(normal));
                    if len < 1.0 - ANGLE_EPSILON || len > 1.0 + ANGLE_EPSILON {
                        continue;
                    }
                    let plane = Plane {
                        normal,
                        dist: dot(origin, normal),
                    };
                    self.test_add(plane)?;
                }
            }
        }
        Ok(())
    }
}

/// `ExpandBrush`: the stored planes pushed out by the hull's box, the bounds' axial planes and
/// the edge bevels added (QBSP-5.2 to QBSP-5.4).
fn expand(
    stored: &mut Vec<BrushPlane>,
    made: &(Vec<Face>, Vec3, Vec3),
    size: [Vec3; 2],
    limits: Limits,
) -> Result<(), CompileError> {
    let (faces, mins, maxs) = made;
    let mut growth = Growth {
        stored,
        limits,
        size,
        points: Vec::new(),
        corners: Vec::new(),
        edges: Vec::new(),
    };
    for face in faces {
        for &point in &face.points {
            growth.add_point(point)?;
        }
    }
    let [low, high] = size;
    for brush_plane in growth.stored.iter_mut() {
        let mut corner = [0.0; 3];
        for axis in 0..3 {
            let n = brush_plane.plane.normal[axis];
            if n > 0.0 {
                corner[axis] = high[axis];
            } else if n < 0.0 {
                corner[axis] = low[axis];
            }
        }
        brush_plane.plane.dist += dot(corner, brush_plane.plane.normal);
    }
    for axis in 0..3 {
        for sign in [-1.0_f32, 1.0] {
            let mut normal = [0.0; 3];
            normal[axis] = sign;
            let dist = if sign < 0.0 {
                -mins[axis] - low[axis]
            } else {
                maxs[axis] + high[axis]
            };
            growth.add_plane(Plane { normal, dist })?;
        }
    }
    for face in faces {
        let count = face.points.len();
        for (i, &p1) in face.points.iter().enumerate() {
            let p2 = face.points.get((i + 1) % count).copied().unwrap_or(p1);
            growth.add_edge(p1, p2)?;
        }
    }
    Ok(())
}

impl BrushSet {
    /// An entity's brushes for `hull`, `Brush_LoadEntity`: the liquids and sky first, then the
    /// solids, each as qbsp's lists leave them (QBSP-4.6).
    pub(crate) fn load(
        entity: &QbspEntity,
        map: &QbspMap,
        hull: Hull,
        model: ModelKind,
        planes: &mut Planes,
        warnings: &mut Vec<crate::Warning>,
        limits: Limits,
    ) -> Result<Self, CompileError> {
        let mut mins = [99999.0_f32; 3];
        let mut maxs = [-99999.0_f32; 3];
        let mut liquid = Vec::new();
        let mut solid = Vec::new();
        // qbsp keeps an entity's brushes with the last in the file first.
        for faces in entity.brushes.iter().rev() {
            match Brush::load(faces, map, hull, model, planes, limits, warnings)? {
                Loaded::Brush(brush) => {
                    for axis in 0..3 {
                        mins[axis] = mins[axis].min(brush.mins[axis]);
                        maxs[axis] = maxs[axis].max(brush.maxs[axis]);
                    }
                    if brush.contents == Contents::Solid {
                        solid.insert(0, brush);
                    } else {
                        liquid.insert(0, brush);
                    }
                }
                Loaded::NoFaces => warnings.push(crate::Warning::NoBrushFaces),
                Loaded::LeftOut => {}
            }
        }
        let mut brushes = solid;
        for brush in liquid {
            brushes.insert(0, brush);
        }
        Ok(Self {
            brushes,
            mins,
            maxs,
        })
    }
}

#[cfg(test)]
mod tests;
