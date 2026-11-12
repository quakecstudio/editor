// SPDX-License-Identifier: GPL-3.0-or-later

//! Merging the faces on a plane into larger convex faces (QBSP-10).

use crate::Face;
use crate::MAXEDGES;
use crate::math::{cross, cyclic, dot, normalize, same, sub};
use crate::planes::Planes;

/// How far an end of the shared edge may turn and still be straight (QBSP-10.1).
const CONTINUOUS_EPSILON: f64 = 0.001;

impl Face {
    /// The face of `f1` and `f2` joined along a shared edge, `TryMerge`; `None` when they cannot
    /// be (QBSP-10.1).
    pub(crate) fn try_merge(&self, f2: &Face, planes: &Planes) -> Option<Face> {
        if self.side != f2.side || self.texinfo != f2.texinfo || self.contents != f2.contents {
            return None;
        }
        let (n1, n2) = (self.points.len(), f2.points.len());
        let mut shared = None;
        'outer: for i in 0..n1 {
            let p1 = *self.points.get(i)?;
            let p2 = *self.points.get(cyclic(i, 1, n1))?;
            for j in 0..n2 {
                let p3 = *f2.points.get(j)?;
                let p4 = *f2.points.get(cyclic(j, 1, n2))?;
                if same(p1, p4) && same(p2, p3) {
                    shared = Some((i, j));
                    break 'outer;
                }
            }
        }
        let (i, j) = shared?;
        let normal = planes.facing(self.plane, self.side).normal;
        let p1 = *self.points.get(i)?;
        let p2 = *self.points.get(cyclic(i, 1, n1))?;
        let back = *self.points.get(cyclic(i, n1.saturating_sub(1), n1))?;
        let edge_normal = normalize(cross(normal, sub(p1, back)));
        let along = f64::from(dot(sub(*f2.points.get(cyclic(j, 2, n2))?, p1), edge_normal));
        if along > CONTINUOUS_EPSILON {
            return None;
        }
        let keep1 = along < -CONTINUOUS_EPSILON;
        let edge_normal = normalize(cross(normal, sub(*self.points.get(cyclic(i, 2, n1))?, p2)));
        let along = f64::from(dot(
            sub(*f2.points.get(cyclic(j, n2.saturating_sub(1), n2))?, p2),
            edge_normal,
        ));
        if along > CONTINUOUS_EPSILON {
            return None;
        }
        let keep2 = along < -CONTINUOUS_EPSILON;
        if n1.saturating_add(n2) > MAXEDGES {
            return None;
        }
        let mut points = Vec::with_capacity(n1.saturating_add(n2));
        let mut k = cyclic(i, 1, n1);
        while k != i {
            if !(k == cyclic(i, 1, n1) && !keep2) {
                points.push(*self.points.get(k)?);
            }
            k = cyclic(k, 1, n1);
        }
        let mut l = cyclic(j, 1, n2);
        while l != j {
            if !(l == cyclic(j, 1, n2) && !keep1) {
                points.push(*f2.points.get(l)?);
            }
            l = cyclic(l, 1, n2);
        }
        Some(Face {
            points,
            ..self.clone()
        })
    }
}

/// Adds `face` to the faces on its plane, merging it with the first it joins and adding the
/// merged face again, until none joins, `MergeFaceToList` and `FreeMergeListScraps`
/// (QBSP-10.2).
pub(crate) fn merge_into(list: &mut Vec<Face>, face: Face, planes: &Planes) {
    let mut face = face;
    loop {
        let found = list
            .iter()
            .enumerate()
            .find_map(|(index, other)| face.try_merge(other, planes).map(|merged| (index, merged)));
        match found {
            Some((index, merged)) => {
                list.remove(index);
                face = merged;
            }
            None => {
                // qbsp puts the face at the head of the list.
                list.insert(0, face);
                return;
            }
        }
    }
}

/// Merges the faces of a plane again, `MergePlaneFaces`: each merged into the list, which is
/// then reversed once (QBSP-10.3).
pub(crate) fn merge_all(faces: Vec<Face>, planes: &Planes) -> Vec<Face> {
    let mut merged = Vec::new();
    for face in faces {
        merge_into(&mut merged, face, planes);
    }
    merged.reverse();
    merged
}

#[cfg(test)]
mod tests;
