// SPDX-License-Identifier: GPL-3.0-or-later

//! CSG: the faces of an entity's brushes that are not inside another brush, gathered by plane
//! into surfaces (QBSP-6).

use std::collections::BTreeMap;

use crate::brush::BrushSet;
use crate::merge::merge_into;
use crate::planes::{PlaneId, Planes, Side};
use crate::winding::divide;
use crate::{CompileError, Contents, Face, Surface};

/// Each face of `inside` cut by the stored plane `split`: the part on the side `outside_side`
/// faces goes to `outside`, the rest stays inside, `ClipInside` (QBSP-6.1, QBSP-6.2).
fn clip_inside(
    inside: Vec<Face>,
    outside: &mut Vec<Face>,
    split: PlaneId,
    outside_side: Side,
    precedence: Precedence,
    planes: &Planes,
) -> Result<Vec<Face>, CompileError> {
    let mut kept = Vec::new();
    let plane = planes.get(split);
    for face in inside {
        let (to_outside, to_inside) = if face.plane == split {
            if face.side != outside_side || precedence == Precedence::Later {
                // Always clip off a face facing the other way, and one a later brush covers.
                (None, Some(face))
            } else {
                (Some(face), None)
            }
        } else {
            let (front, back) = divide(&face.points, plane)?;
            let front = front.map(|points| Face {
                points,
                ..face.clone()
            });
            let back = back.map(|points| Face { points, ..face });
            match outside_side {
                Side::Front => (front, back),
                Side::Back => (back, front),
            }
        };
        if let Some(face) = to_outside {
            outside.insert(0, face);
        }
        if let Some(face) = to_inside {
            kept.insert(0, face);
        }
    }
    Ok(kept)
}

/// Whether the brush cutting another comes after it in CSG order (QBSP-6.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Precedence {
    Earlier,
    Later,
}

/// The surfaces of a brush set: every face not inside another brush, merged on its plane,
/// `CSGFaces` and `BuildSurfaces` (QBSP-6).
pub(crate) fn csg_faces(set: &BrushSet, planes: &Planes) -> Result<Vec<Surface>, CompileError> {
    let mut valid: BTreeMap<PlaneId, Vec<Face>> = BTreeMap::new();
    for (index, b1) in set.brushes.iter().enumerate() {
        // `CopyFacesToOutside` adds each face to the front of the list.
        let mut outside: Vec<Face> = Vec::new();
        for face in &b1.faces {
            outside.insert(
                0,
                Face {
                    contents: [Contents::Empty, b1.contents],
                    ..face.clone()
                },
            );
        }
        let mut precedence = Precedence::Earlier;
        for (other, b2) in set.brushes.iter().enumerate() {
            if other == index {
                precedence = Precedence::Later;
                continue;
            }
            if (0..3).any(|i| b1.mins[i] > b2.maxs[i] || b1.maxs[i] < b2.mins[i]) {
                continue;
            }
            let mut inside = core::mem::take(&mut outside);
            for face in &b2.faces {
                inside = clip_inside(
                    inside,
                    &mut outside,
                    face.plane,
                    face.side,
                    precedence,
                    planes,
                )?;
            }
            // `FreeInside`: a solid brush keeps its faces inside a liquid or sky, facing it.
            if b1.contents == Contents::Solid && b2.contents.is_liquid_or_sky() {
                for mut face in inside {
                    face.contents[0] = b2.contents;
                    outside.insert(0, face);
                }
            }
        }
        // `SaveOutside`.
        for face in outside {
            let list = valid.entry(face.plane).or_default();
            let mirror = (b1.contents != Contents::Solid).then(|| Face {
                side: face.side.other(),
                contents: [face.contents[1], face.contents[0]],
                points: face.points.iter().rev().copied().collect(),
                ..face.clone()
            });
            merge_into(list, face, planes);
            if let Some(mirror) = mirror {
                merge_into(list, mirror, planes);
            }
            list.reverse();
        }
    }
    // `BuildSurfaces` adds each plane's surface to the front of the list.
    let mut surfaces = Vec::new();
    for (plane, faces) in valid {
        if faces.is_empty() {
            continue;
        }
        surfaces.insert(0, Surface::new(plane, faces));
    }
    Ok(surfaces)
}

#[cfg(test)]
mod tests;
