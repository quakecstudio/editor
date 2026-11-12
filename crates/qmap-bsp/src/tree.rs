// SPDX-License-Identifier: GPL-3.0-or-later

//! The BSP tree: surfaces split by their own planes until none is left, faces subdivided for
//! the lightmap and kept on the nodes, leaves given their contents (QBSP-7).

use qs_map::qbsp::QbspTexinfo;

use crate::math::{Vec3, dot, length, normalize, same};
use crate::planes::{Plane, PlaneId, Planes, Side};
use crate::winding::{ON_EPSILON, divide};
use crate::{CompileError, Contents, Face, Faults, Surface, Warning};

/// What the head node's bounds add to the entity's on each side (QBSP-7.1).
pub(crate) const SIDESPACE: f32 = 24.0;
/// How far short of the subdivision size each cut is made (QBSP-7.8).
const SUBDIVIDE_MARGIN: f32 = 16.0;
/// The mid-split choice's starting value (QBSP-7.3).
const MID_START: f32 = 6.0 * 8192.0 * 8192.0;
/// The full heuristic's starting split count and value (QBSP-7.9).
const SPLITS_START: usize = 99999;
const VALUE_START: f32 = 9e30;
/// `TEX_SPECIAL` (BSP-8.5).
const TEX_SPECIAL: i32 = 1;

/// A face kept on a node: what the leaves mark and the BSP writes (QBSP-7.4).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NodeFace {
    pub(crate) face: Face,
    /// Emptied by the outside fill (QBSP-9.4).
    pub(crate) removed: bool,
    /// The faces T-junction fixing cut from it, which a leaf marks with it (QBSP-11.3).
    pub(crate) pieces: Vec<usize>,
    /// Its number among the BSP's faces, once written.
    pub(crate) output: Option<usize>,
    /// Its surfedges, once written.
    pub(crate) edges: Vec<i32>,
}

/// A node or a leaf.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum NodeKind {
    Decision {
        plane: PlaneId,
        children: [usize; 2],
        /// Its faces, both sides, in qbsp's list order.
        faces: Vec<usize>,
    },
    Leaf {
        contents: Contents,
        /// The node faces from which its faces came (QBSP-7.7).
        marks: Vec<usize>,
    },
}

/// A node of the tree.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Node {
    pub(crate) kind: NodeKind,
    pub(crate) mins: Vec3,
    pub(crate) maxs: Vec3,
    /// The portals touching it (QBSP-8).
    pub(crate) portals: Vec<usize>,
    /// The entity standing in it (QBSP-9.1).
    pub(crate) occupied: bool,
}

/// A tree and its node faces.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Tree {
    pub(crate) nodes: Vec<Node>,
    pub(crate) faces: Vec<NodeFace>,
}

/// How the splitting surface is chosen (QBSP-7.3, QBSP-7.9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    /// The fast choice of the clipping hulls and of a first world tree.
    MidSplit,
    /// The full heuristic.
    Full,
}

/// What the tree is built with.
pub(crate) struct Builder<'a> {
    pub(crate) planes: &'a Planes,
    pub(crate) texinfo: &'a [QbspTexinfo],
    pub(crate) choice: Choice,
    pub(crate) subdivide: f32,
    pub(crate) faults: Faults,
    /// The warnings building gives (QBSP-14.3).
    pub(crate) warnings: core::cell::RefCell<Vec<Warning>>,
}

/// `FaceSide`: which side of `plane` a face is on, `None` when it has points on both
/// (QBSP-7.3).
fn face_side(face: &Face, plane: Plane, axial: Option<usize>) -> Option<Side> {
    let (mut front, mut back) = (false, false);
    for point in &face.points {
        // An axial plane compares the coordinate with its distance moved by the epsilon, in
        // double precision; any other, the float distance with the epsilon.
        let (ahead, behind) = match axial {
            Some(axis) => {
                let (p, dist) = (f64::from(point[axis]), f64::from(plane.dist));
                (p > dist + ON_EPSILON, p < dist - ON_EPSILON)
            }
            None => {
                let d = f64::from(dot(*point, plane.normal) - plane.dist);
                (d > ON_EPSILON, d < -ON_EPSILON)
            }
        };
        if ahead {
            if back {
                return None;
            }
            front = true;
        } else if behind {
            if front {
                return None;
            }
            back = true;
        }
    }
    match (front, back) {
        (false, _) => Some(Side::Back),
        (true, false) => Some(Side::Front),
        (true, true) => None,
    }
}

/// An axial plane's value over `mins` to `maxs` (QBSP-7.3).
fn axial_value(plane: Plane, axis: usize, mins: Vec3, maxs: Vec3) -> f32 {
    let dist = plane.dist * plane.normal[axis];
    let mut value = 0.0;
    for j in 0..3 {
        if j == axis {
            value += (maxs[axis] - dist) * (maxs[axis] - dist);
            value += (dist - mins[axis]) * (dist - mins[axis]);
        } else {
            value += 2.0 * (maxs[j] - mins[j]) * (maxs[j] - mins[j]);
        }
    }
    value
}

impl Builder<'_> {
    /// `ChooseMidPlaneFromList` (QBSP-7.3).
    fn mid_plane(&self, surfaces: &[Surface], mins: Vec3, maxs: Vec3) -> Option<usize> {
        let mut best_value = MID_START;
        let mut best = None;
        for (index, surface) in surfaces.iter().enumerate() {
            if surface.on_node {
                continue;
            }
            let kind = self.planes.kind(surface.plane);
            if !kind.is_axial() {
                continue;
            }
            let value = axial_value(self.planes.get(surface.plane), kind.axis(), mins, maxs);
            if value > best_value {
                continue;
            }
            best_value = value;
            best = Some(index);
        }
        best.or_else(|| surfaces.iter().position(|surface| !surface.on_node))
    }

    /// `ChoosePlaneFromList` with floors used (QBSP-7.9).
    fn best_plane(&self, surfaces: &[Surface], mins: Vec3, maxs: Vec3) -> Option<usize> {
        let mut best_splits = SPLITS_START;
        let mut best_value = VALUE_START;
        let mut best = None;
        for (index, surface) in surfaces.iter().enumerate() {
            if surface.on_node {
                continue;
            }
            let plane = self.planes.get(surface.plane);
            let kind = self.planes.kind(surface.plane);
            let axial = kind.is_axial().then_some(kind.axis());
            let mut splits = 0_usize;
            'count: for (other_index, other) in surfaces.iter().enumerate() {
                if other_index == index || other.on_node {
                    continue;
                }
                for face in &other.faces {
                    if face_side(face, plane, axial).is_none() {
                        splits = splits.saturating_add(1);
                        if splits >= best_splits {
                            break;
                        }
                    }
                }
                if splits > best_splits {
                    break 'count;
                }
            }
            if splits > best_splits {
                continue;
            }
            if splits < best_splits || (splits == best_splits && kind.is_axial()) {
                if let Some(axis) = axial {
                    let value = axial_value(plane, axis, mins, maxs);
                    if value > best_value && splits == best_splits {
                        continue;
                    }
                    best_value = value;
                }
                best_splits = splits;
                best = Some(index);
            }
        }
        best
    }

    /// `SelectPartition`: the surface to split by, `None` for a leaf (QBSP-7.2).
    fn select(&self, surfaces: &[Surface]) -> Option<usize> {
        let left: Vec<usize> = (0..surfaces.len())
            .filter(|&index| surfaces.get(index).is_some_and(|s| !s.on_node))
            .collect();
        match left.as_slice() {
            [] => return None,
            [only] => return Some(*only),
            _ => {}
        }
        let mut mins = [99999.0_f32; 3];
        let mut maxs = [-99999.0_f32; 3];
        for surface in surfaces {
            for axis in 0..3 {
                mins[axis] = mins[axis].min(surface.mins[axis]);
                maxs[axis] = maxs[axis].max(surface.maxs[axis]);
            }
        }
        match self.choice {
            Choice::MidSplit => self.mid_plane(surfaces, mins, maxs),
            Choice::Full => self.best_plane(surfaces, mins, maxs),
        }
    }

    /// Every face of a node's list subdivided in turn, the pieces each cut gives taking its
    /// place and being subdivided in their turn, as `LinkNodeFaces` walks the list (QBSP-7.8).
    fn subdivide_all(&self, faces: Vec<Face>) -> Result<Vec<Face>, CompileError> {
        let mut list = faces;
        let mut at = 0;
        while at < list.len() {
            let face = list.remove(at);
            let pieces = self.subdivide(face)?;
            for (offset, piece) in pieces.into_iter().enumerate() {
                list.insert(at.saturating_add(offset), piece);
            }
            at = at.saturating_add(1);
        }
        Ok(list)
    }

    /// `SubdivideFace`: a face cut while it spans more than the subdivision size along a
    /// texture axis: the piece it kept cutting, then the pieces cut off, the latest first
    /// (QBSP-7.8).
    fn subdivide(&self, face: Face) -> Result<Vec<Face>, CompileError> {
        let Some(texinfo) = self.texinfo.get(face.texinfo) else {
            return Ok(vec![face]);
        };
        if texinfo.flags & TEX_SPECIAL != 0 {
            return Ok(vec![face]);
        }
        let mut done: Vec<Face> = Vec::new();
        let mut current = face;
        for axis in 0..2 {
            loop {
                let vecs = texinfo.vecs.get(axis).copied().unwrap_or_default();
                let along = [vecs[0], vecs[1], vecs[2]];
                let (mut min, mut max) = (9999.0_f32, -9999.0_f32);
                for point in &current.points {
                    let v = dot(*point, along);
                    min = min.min(v);
                    max = max.max(v);
                }
                if max - min <= self.subdivide {
                    break;
                }
                let len = length(along);
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "spec QBSP-7.8: qbsp divides by its double length into a float"
                )]
                let dist = (f64::from(min + self.subdivide - SUBDIVIDE_MARGIN) / len) as f32;
                let plane = Plane {
                    normal: normalize(along),
                    dist,
                };
                let (front, back) = divide(&current.points, plane)?;
                let (Some(front), Some(back)) = (front, back) else {
                    return Err(CompileError::SubdivideFailed);
                };
                // The back stays to be cut on, the front follows it in the list.
                done.insert(
                    0,
                    Face {
                        points: front,
                        ..current.clone()
                    },
                );
                current = Face {
                    points: back,
                    ..current
                };
            }
        }
        let mut out = vec![current];
        out.extend(done);
        Ok(out)
    }

    /// The tree of `surfaces` within the head bounds `mins` to `maxs`, `SolidBSP` (QBSP-7).
    pub(crate) fn build(
        &self,
        surfaces: Vec<Surface>,
        mins: Vec3,
        maxs: Vec3,
    ) -> Result<Tree, CompileError> {
        let mut tree = Tree::default();
        let head = tree.leaf_node(mins, maxs);
        let mut work = vec![(surfaces, head)];
        while let Some((surfaces, node)) = work.pop() {
            let children = self.partition(&mut tree, surfaces, node)?;
            // qbsp recurses into the front child first: it is popped first.
            for child in children.into_iter().rev() {
                work.push(child);
            }
        }
        Ok(tree)
    }

    /// `PartitionSurfaces` at `node`: a leaf, or the split and the two lists to go on with.
    fn partition(
        &self,
        tree: &mut Tree,
        mut surfaces: Vec<Surface>,
        node: usize,
    ) -> Result<Vec<(Vec<Surface>, usize)>, CompileError> {
        let Some(split_index) = self.select(&surfaces) else {
            tree.link_leaf(node, surfaces, self.faults, &mut self.warnings.borrow_mut())?;
            return Ok(Vec::new());
        };
        let Some(split) = surfaces.get_mut(split_index) else {
            return Ok(Vec::new());
        };
        // `LinkNodeFaces`: each face subdivided, then copied to the node, the copy its
        // fragments' original.
        let mut subdivided = self.subdivide_all(core::mem::take(&mut split.faces))?;
        let mut node_faces = Vec::new();
        for face in &mut subdivided {
            let id = tree.faces.len();
            tree.faces.push(NodeFace {
                face: face.clone(),
                removed: false,
                pieces: Vec::new(),
                output: None,
                edges: Vec::new(),
            });
            face.original = Some(id);
            node_faces.insert(0, id);
        }
        split.faces = subdivided;
        let plane_id = split.plane;
        let plane = self.planes.get(plane_id);
        let kind = self.planes.kind(plane_id);
        let (mins, maxs) = tree
            .nodes
            .get(node)
            .map_or(([0.0; 3], [0.0; 3]), |n| (n.mins, n.maxs));
        // `DivideNodeBounds` (QBSP-7.5).
        let (mut front_mins, mut back_maxs) = (mins, maxs);
        if kind.is_axial() {
            front_mins[kind.axis()] = plane.dist;
            back_maxs[kind.axis()] = plane.dist;
        }
        let front = tree.leaf_node(front_mins, maxs);
        let back = tree.leaf_node(mins, back_maxs);
        if let Some(n) = tree.nodes.get_mut(node) {
            n.kind = NodeKind::Decision {
                plane: plane_id,
                children: [front, back],
                faces: node_faces,
            };
        }
        let mut front_list = Vec::new();
        let mut back_list = Vec::new();
        for surface in surfaces {
            let (f, b) = self.divide_surface(surface, plane)?;
            if let Some(f) = f {
                front_list.insert(0, f);
            }
            if let Some(b) = b {
                back_list.insert(0, b);
            }
        }
        Ok(vec![(front_list, front), (back_list, back)])
    }

    /// `DividePlane`: a surface's parts in front of and behind the splitting plane (QBSP-7.4).
    #[expect(
        clippy::float_cmp,
        reason = "spec QBSP-7.4: qbsp compares the two distances exactly"
    )]
    fn divide_surface(
        &self,
        mut surface: Surface,
        split: Plane,
    ) -> Result<(Option<Surface>, Option<Surface>), CompileError> {
        let own = self.planes.get(surface.plane);
        if same(own.normal, split.normal) {
            if own.dist == split.dist {
                let (front, back): (Vec<Face>, Vec<Face>) = surface
                    .faces
                    .drain(..)
                    .rev()
                    .partition(|face| face.side == Side::Front);
                let front_surface = Surface {
                    faces: front,
                    on_node: true,
                    ..surface.clone()
                };
                let back_surface = Surface {
                    faces: back,
                    on_node: true,
                    ..surface
                };
                return Ok((
                    (!front_surface.faces.is_empty()).then_some(front_surface),
                    (!back_surface.faces.is_empty()).then_some(back_surface),
                ));
            }
            return Ok(if own.dist > split.dist {
                (Some(surface), None)
            } else {
                (None, Some(surface))
            });
        }
        let mut front_list = Vec::new();
        let mut back_list = Vec::new();
        for face in surface.faces.drain(..) {
            let (front, back) = divide(&face.points, split)?;
            if let Some(points) = front {
                front_list.insert(
                    0,
                    Face {
                        points,
                        ..face.clone()
                    },
                );
            }
            if let Some(points) = back {
                back_list.insert(0, Face { points, ..face });
            }
        }
        if front_list.is_empty() {
            surface.faces = back_list;
            return Ok((None, Some(surface)));
        }
        if back_list.is_empty() {
            surface.faces = front_list;
            return Ok((Some(surface), None));
        }
        let mut back = Surface {
            faces: back_list,
            ..surface.clone()
        };
        surface.faces = front_list;
        back.recalculate();
        surface.recalculate();
        Ok((Some(surface), Some(back)))
    }
}

impl Tree {
    /// A new leaf with these bounds, made a node when split.
    fn leaf_node(&mut self, mins: Vec3, maxs: Vec3) -> usize {
        self.nodes.push(Node {
            kind: NodeKind::Leaf {
                contents: Contents::Solid,
                marks: Vec::new(),
            },
            mins,
            maxs,
            portals: Vec::new(),
            occupied: false,
        });
        self.nodes.len().saturating_sub(1)
    }

    /// `LinkConvexFaces`: the leaf's contents from its faces' fronts, and the node faces they
    /// came from (QBSP-7.6, QBSP-7.7).
    fn link_leaf(
        &mut self,
        node: usize,
        surfaces: Vec<Surface>,
        faults: Faults,
        warnings: &mut Vec<Warning>,
    ) -> Result<(), CompileError> {
        let mut mixed = false;
        let mut contents = None;
        let mut marks = Vec::new();
        for surface in surfaces {
            for face in surface.faces {
                match contents {
                    None => contents = Some(face.contents[0]),
                    Some(known) if known != face.contents[0] => match faults {
                        Faults::Stop => return Err(CompileError::MixedContents),
                        Faults::Warn => mixed = true,
                    },
                    Some(_) => {}
                }
                if let Some(original) = face.original {
                    marks.push(original);
                }
            }
        }
        if mixed {
            warnings.push(Warning::MixedContents);
        }
        if let Some(n) = self.nodes.get_mut(node) {
            n.kind = NodeKind::Leaf {
                contents: contents.unwrap_or(Contents::Solid),
                marks,
            };
        }
        Ok(())
    }

    /// A tree of one solid leaf: the model of an entity with no valid brush (QBSP-14.3).
    pub(crate) fn empty(mins: Vec3, maxs: Vec3) -> Self {
        let mut tree = Self::default();
        tree.leaf_node(mins, maxs);
        tree
    }

    /// The head node.
    pub(crate) const fn head(&self) -> usize {
        0
    }

    /// A leaf's contents, `None` for a decision node.
    pub(crate) fn contents(&self, node: usize) -> Option<Contents> {
        match self.nodes.get(node).map(|n| &n.kind) {
            Some(NodeKind::Leaf { contents, .. }) => Some(*contents),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
