// SPDX-License-Identifier: GPL-3.0-or-later

//! T-junctions: every vertex of a face that lies on another face's edge is added to that edge,
//! and a face left with too many points is cut (QBSP-11).

use crate::math::{Axis, EQUAL_EPSILON, Vec3, cyclic, dot, ma, negate, normalize, same, sub};
use crate::tree::{NodeFace, NodeKind, Tree};
use crate::{CompileError, Face};

/// How near two points on a line are one (QBSP-11.1).
const T_EPSILON: f64 = 0.01;
/// The most points a face keeps (QBSP-11.3).
const MAXPOINTS: usize = 28;
/// The most lines and points (QBSP-11.4).
const MAXWEDGES: usize = 0x10000;
const MAXWVERTS: usize = 0x20000;
/// The hash's buckets.
const NUM_HASH: usize = 1024;
/// A line's sentinel, past every point on it.
const SENTINEL: f32 = 99999.0;
/// How straight two edges of a face must be to be one side when cutting it (QBSP-11.3).
const STRAIGHT_LOW: f64 = 0.9999;
const STRAIGHT_HIGH: f64 = 1.000_01;

/// A line: its direction, its point nearest the origin, and the distances along it of the
/// points on it, sorted, the sentinel last.
struct Line {
    dir: Vec3,
    origin: Vec3,
    points: Vec<f32>,
}

/// Every line, hashed by its origin's x and y as qbsp's `HashVec` does.
struct Lines {
    lines: Vec<Line>,
    buckets: Vec<Vec<usize>>,
    min: Vec3,
    scales: [f32; 3],
    verts: usize,
}

/// `CanonicalVector`: the direction made unit, its first component beyond `EQUAL_EPSILON`
/// positive, smaller components zeroed.
fn canonical(v: Vec3) -> Vec3 {
    let mut v = normalize(v);
    for axis in Axis::ALL {
        let c = f64::from(axis.of(v));
        if c > EQUAL_EPSILON {
            return v;
        }
        if c < -EQUAL_EPSILON {
            return negate(v);
        }
        *axis.of_mut(&mut v) = 0.0;
    }
    v
}

impl Lines {
    /// `InitHash` over the box `-maxs` to `maxs`.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        reason = "spec QBSP-11.1: qbsp's hash sizes, floats truncated to ints and back"
    )]
    fn new(maxs: Vec3) -> Self {
        let min = maxs.map(|c| -c);
        let size = sub(maxs, min);
        let volume = size[0] * size[1];
        let scale = f64::from(volume / NUM_HASH as f32).sqrt() as f32;
        let newsize = [(size[0] / scale) as i32, (size[1] / scale) as i32];
        Self {
            lines: Vec::new(),
            buckets: vec![Vec::new(); NUM_HASH],
            min,
            scales: [
                newsize[0] as f32 / size[0],
                newsize[1] as f32 / size[1],
                newsize[1] as f32,
            ],
            verts: 0,
        }
    }

    /// `HashVec`.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "spec QBSP-11.1: qbsp converts the float to an unsigned bucket"
    )]
    fn bucket(&self, v: Vec3) -> usize {
        let h = self.scales[0] * (v[0] - self.min[0]) * self.scales[2]
            + self.scales[1] * (v[1] - self.min[1]);
        if h < 0.0 {
            return NUM_HASH - 1;
        }
        (h as usize).min(NUM_HASH - 1)
    }

    /// `FindEdge`: the line through `p1` and `p2` and their distances along it, sorted.
    fn find(&mut self, p1: Vec3, p2: Vec3) -> Result<(usize, f32, f32), CompileError> {
        let dir = canonical(sub(p2, p1));
        let (mut t1, mut t2) = (dot(p1, dir), dot(p2, dir));
        let origin = ma(p1, -f64::from(t1), dir);
        if t1 > t2 {
            core::mem::swap(&mut t1, &mut t2);
        }
        let bucket = self.bucket(origin);
        if let Some(&found) = self.buckets.get(bucket).and_then(|ids| {
            ids.iter().rev().find(|&&id| {
                self.lines
                    .get(id)
                    .is_some_and(|line| same(line.origin, origin) && same(line.dir, dir))
            })
        }) {
            return Ok((found, t1, t2));
        }
        if self.lines.len() >= MAXWEDGES {
            return Err(CompileError::Limit("FindEdge: numwedges == MAXWEDGES"));
        }
        self.lines.push(Line {
            dir,
            origin,
            points: vec![SENTINEL],
        });
        let id = self.lines.len().saturating_sub(1);
        if let Some(ids) = self.buckets.get_mut(bucket) {
            ids.push(id);
        }
        Ok((id, t1, t2))
    }

    /// `AddVert`.
    fn add(&mut self, line: usize, t: f32) -> Result<(), CompileError> {
        let Some(line) = self.lines.get_mut(line) else {
            return Ok(());
        };
        let mut at = 0;
        while let Some(&known) = line.points.get(at) {
            if f64::from((known - t).abs()) < T_EPSILON {
                return Ok(());
            }
            if known > t {
                break;
            }
            at = at.saturating_add(1);
        }
        if self.verts >= MAXWVERTS {
            return Err(CompileError::Limit("AddVert: numwverts == MAXWVERTS"));
        }
        self.verts = self.verts.saturating_add(1);
        line.points.insert(at, t);
        Ok(())
    }
}

/// The node faces in qbsp's traversal order: each node's list, then its front child's, then
/// its back child's.
fn node_order(tree: &Tree) -> Vec<usize> {
    let mut order = Vec::new();
    let mut work = vec![tree.head()];
    while let Some(node) = work.pop() {
        if let Some(NodeKind::Decision { children, .. }) = tree.nodes.get(node).map(|n| &n.kind) {
            order.push(node);
            work.push(children[1]);
            work.push(children[0]);
        }
    }
    order
}

/// `FixFaceEdges`'s first part: the face's points with every point of its edges' lines added
/// between each edge's ends.
fn fix_points(points: &[Vec3], lines: &mut Lines) -> Result<Vec<Vec3>, CompileError> {
    let mut points = points.to_vec();
    'restart: loop {
        let count = points.len();
        for i in 0..count {
            let j = cyclic(i, 1, count);
            let (Some(&a), Some(&b)) = (points.get(i), points.get(j)) else {
                continue;
            };
            let (id, t1, t2) = lines.find(a, b)?;
            let Some(line) = lines.lines.get(id) else {
                continue;
            };
            let next = line
                .points
                .iter()
                .find(|&&t| f64::from(t) >= f64::from(t1) + T_EPSILON)
                .copied();
            if let Some(t) = next
                && f64::from(t) < f64::from(t2) - T_EPSILON
            {
                let point = ma(line.origin, f64::from(t), line.dir);
                // qbsp inserts at `j`: the start of the list for the closing edge.
                points.insert(j, point);
                continue 'restart;
            }
        }
        return Ok(points);
    }
}

/// `SplitFaceForTjunc`: a face of too many points cut into pieces along runs of straight
/// edges; the pieces cut off, in the order cut, and what is left (QBSP-11.3).
fn split_for_tjunc(points: Vec<Vec3>) -> Result<(Vec<Vec<Vec3>>, Vec<Vec3>), CompileError> {
    let mut f = points;
    let mut cut = Vec::new();
    loop {
        if f.len() <= MAXPOINTS {
            return Ok((cut, f));
        }
        let mut rotations = 0_usize;
        loop {
            let n = f.len();
            let straight = |a: Vec3, b: Vec3, dir: Vec3| {
                let v = f64::from(dot(normalize(sub(a, b)), dir));
                (STRAIGHT_LOW..=STRAIGHT_HIGH).contains(&v)
            };
            let first = *f.first().ok_or(CompileError::TooManyPoints)?;
            let last = *f.last().ok_or(CompileError::TooManyPoints)?;
            let dir = normalize(sub(last, first));
            let mut lastcorner = n.saturating_sub(1);
            while lastcorner > 0 {
                let (Some(&a), Some(&b)) = (f.get(lastcorner.saturating_sub(1)), f.get(lastcorner))
                else {
                    break;
                };
                if !straight(a, b, dir) {
                    break;
                }
                lastcorner = lastcorner.saturating_sub(1);
            }
            let second = *f.get(1).ok_or(CompileError::TooManyPoints)?;
            let dir = normalize(sub(second, first));
            let mut firstcorner = 1;
            while firstcorner < n.saturating_sub(1) {
                let (Some(&a), Some(&b)) =
                    (f.get(firstcorner.saturating_add(1)), f.get(firstcorner))
                else {
                    break;
                };
                if !straight(a, b, dir) {
                    break;
                }
                firstcorner = firstcorner.saturating_add(1);
            }
            if firstcorner.saturating_add(2) >= MAXPOINTS {
                rotations = rotations.saturating_add(1);
                if rotations > n {
                    return Err(CompileError::TooManyPoints);
                }
                f.rotate_left(1);
                continue;
            }
            let take = if n.saturating_sub(firstcorner) <= MAXPOINTS {
                firstcorner.saturating_add(2)
            } else if lastcorner.saturating_add(2) < MAXPOINTS
                && n.saturating_sub(lastcorner) <= MAXPOINTS
            {
                lastcorner.saturating_add(2)
            } else {
                MAXPOINTS
            };
            cut.push(f.get(..take).unwrap_or_default().to_vec());
            let mut rest = vec![first];
            rest.extend_from_slice(f.get(take.saturating_sub(1)..).unwrap_or_default());
            f = rest;
            break;
        }
    }
}

/// `tjunc` over the world's final tree, whose brushes reach `maxs` from the origin
/// (QBSP-11).
pub(crate) fn fix(tree: &mut Tree, maxs: Vec3) -> Result<(), CompileError> {
    let mut lines = Lines::new(maxs);
    let order = node_order(tree);
    for &node in &order {
        if let Some(NodeKind::Decision { faces, .. }) = tree.nodes.get(node).map(|n| &n.kind) {
            for &id in faces {
                let Some(face) = tree.faces.get(id) else {
                    continue;
                };
                let count = face.face.points.len();
                for i in 0..count {
                    let (Some(&a), Some(&b)) = (
                        face.face.points.get(i),
                        face.face.points.get(cyclic(i, 1, count)),
                    ) else {
                        continue;
                    };
                    let (line, t1, t2) = lines.find(a, b)?;
                    lines.add(line, t1)?;
                    lines.add(line, t2)?;
                }
            }
        }
    }
    for node in order {
        let Some(NodeKind::Decision { faces, .. }) = tree.nodes.get(node).map(|n| n.kind.clone())
        else {
            continue;
        };
        let mut newlist = Vec::new();
        for id in faces {
            let Some(points) = tree.faces.get(id).map(|f| f.face.points.clone()) else {
                continue;
            };
            let fixed = fix_points(&points, &mut lines)?;
            if fixed.len() <= MAXPOINTS {
                if let Some(face) = tree.faces.get_mut(id) {
                    face.face.points = fixed;
                }
                newlist.insert(0, id);
                continue;
            }
            let (pieces, rest) = split_for_tjunc(fixed)?;
            let template = tree.faces.get(id).map(|f| f.face.clone());
            let Some(template) = template else {
                continue;
            };
            let mut chain = Vec::new();
            for piece in pieces {
                let piece_id = tree.faces.len();
                tree.faces.push(NodeFace {
                    face: Face {
                        points: piece,
                        ..template.clone()
                    },
                    removed: false,
                    pieces: Vec::new(),
                    output: None,
                    edges: Vec::new(),
                });
                chain.insert(0, piece_id);
                newlist.insert(0, piece_id);
            }
            if let Some(face) = tree.faces.get_mut(id) {
                face.face.points = rest;
                face.pieces = chain;
            }
            newlist.insert(0, id);
        }
        if let Some(NodeKind::Decision { faces, .. }) =
            tree.nodes.get_mut(node).map(|n| &mut n.kind)
        {
            *faces = newlist;
        }
    }
    Ok(())
}
