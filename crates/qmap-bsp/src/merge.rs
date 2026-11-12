// SPDX-License-Identifier: GPL-3.0-or-later

//! Merging the faces on a plane into larger convex faces (QBSP-10).

use crate::Face;
use crate::MAXEDGES;
use crate::math::{EQUAL_EPSILON, cross, dot, normalize, sub};
use crate::planes::Planes;

/// How far an end of the shared edge may turn and still be straight (QBSP-10.1).
const CONTINUOUS_EPSILON: f64 = 0.001;

/// The face of `f1` and `f2` joined along a shared edge, `TryMerge`; `None` when they cannot
/// be (QBSP-10.1).
pub(crate) fn try_merge(f1: &Face, f2: &Face, planes: &Planes) -> Option<Face> {
    if f1.side != f2.side || f1.texinfo != f2.texinfo || f1.contents != f2.contents {
        return None;
    }
    let (n1, n2) = (f1.points.len(), f2.points.len());
    let mut shared = None;
    'outer: for i in 0..n1 {
        let p1 = *f1.points.get(i)?;
        let p2 = *f1.points.get((i + 1) % n1)?;
        for j in 0..n2 {
            let p3 = *f2.points.get(j)?;
            let p4 = *f2.points.get((j + 1) % n2)?;
            if (0..3).all(|k| {
                f64::from((p1[k] - p4[k]).abs()) <= EQUAL_EPSILON
                    && f64::from((p2[k] - p3[k]).abs()) <= EQUAL_EPSILON
            }) {
                shared = Some((i, j));
                break 'outer;
            }
        }
    }
    let (i, j) = shared?;
    let normal = planes.facing(f1.plane, f1.side).normal;
    let p1 = *f1.points.get(i)?;
    let p2 = *f1.points.get((i + 1) % n1)?;
    let back = *f1.points.get((i + n1 - 1) % n1)?;
    let edge_normal = normalize(cross(normal, sub(p1, back)));
    let along = f64::from(dot(sub(*f2.points.get((j + 2) % n2)?, p1), edge_normal));
    if along > CONTINUOUS_EPSILON {
        return None;
    }
    let keep1 = along < -CONTINUOUS_EPSILON;
    let edge_normal = normalize(cross(normal, sub(*f1.points.get((i + 2) % n1)?, p2)));
    let along = f64::from(dot(
        sub(*f2.points.get((j + n2 - 1) % n2)?, p2),
        edge_normal,
    ));
    if along > CONTINUOUS_EPSILON {
        return None;
    }
    let keep2 = along < -CONTINUOUS_EPSILON;
    if n1 + n2 > MAXEDGES {
        return None;
    }
    let mut points = Vec::with_capacity(n1 + n2);
    let mut k = (i + 1) % n1;
    while k != i {
        if !(k == (i + 1) % n1 && !keep2) {
            points.push(*f1.points.get(k)?);
        }
        k = (k + 1) % n1;
    }
    let mut l = (j + 1) % n2;
    while l != j {
        if !(l == (j + 1) % n2 && !keep1) {
            points.push(*f2.points.get(l)?);
        }
        l = (l + 1) % n2;
    }
    Some(Face {
        points,
        ..f1.clone()
    })
}

/// Adds `face` to the faces on its plane, merging it with the first it joins and adding the
/// merged face again, until none joins, `MergeFaceToList` and `FreeMergeListScraps`
/// (QBSP-10.2).
pub(crate) fn merge_into(list: &mut Vec<Face>, face: Face, planes: &Planes) {
    let mut face = face;
    loop {
        let found = list.iter().enumerate().find_map(|(index, other)| {
            try_merge(&face, other, planes).map(|merged| (index, merged))
        });
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

/// [`merge_into`] then `FreeMergeListScraps`, which rebuilds the list by adding each face to
/// its front, reversing it: what CSG does with each face it keeps (QBSP-6.5).
pub(crate) fn merge_and_tidy(list: &mut Vec<Face>, face: Face, planes: &Planes) {
    merge_into(list, face, planes);
    list.reverse();
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
