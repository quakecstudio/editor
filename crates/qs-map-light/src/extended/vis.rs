// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: light/ltface.cc, Mod_LeafPvs, CalcPvs, VisCullEntity, SurfaceLight_VisCull
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: common/bsputils.cc, DecompressVis, DecompressAllVis, DecompressedVisSize, Pvs_LeafVisible
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: light/trace.cc, Light_PointInLeaf

//! Culling by the BSP's visibility (LIGHT-10.10): when a BSP has been through vis, a light
//! lights only the faces whose leaves can see its leaf.

use std::collections::HashMap;

use qs_bsp::Bsp;

use crate::Vec3;

/// The contents of a leaf light is never culled into or out of (BSP-12): solid, sky, and the
/// liquids, which vis may call opaque though light passes them.
const CONTENTS_SOLID: i32 = -2;
const CONTENTS_WATER: i32 = -3;
const CONTENTS_SLIME: i32 = -4;
const CONTENTS_LAVA: i32 = -5;
const CONTENTS_SKY: i32 = -6;
/// The leaves a byte of a visibility row holds.
const BITS: usize = 8;
/// A row that sees every leaf.
const EVERYTHING: u8 = 0xff;

/// The visibility of a BSP that has any, each leaf's row decompressed once.
pub(crate) struct Visibility {
    /// The bytes of a row: a bit for each leaf of the world but the first.
    row: usize,
    /// The rows by where they start in the lump: leaves of one cluster share a row.
    rows: HashMap<i32, Vec<u8>>,
    /// The leaves each face is in, by the leaves' face lists.
    face_leaves: Vec<Vec<usize>>,
}

/// Whether a leaf's contents are a liquid's.
const fn is_liquid(contents: i32) -> bool {
    matches!(contents, CONTENTS_WATER | CONTENTS_SLIME | CONTENTS_LAVA)
}

impl Visibility {
    /// `DecompressAllVis`: the visibility of `bsp`; none when it has not been through vis.
    pub(crate) fn new(bsp: &Bsp<'_>) -> Option<Self> {
        if bsp.visibility.is_empty() {
            return None;
        }
        let leaves = bsp
            .models
            .first()
            .and_then(|world| usize::try_from(world.visleafs).ok())
            .unwrap_or(0);
        let row = leaves
            .saturating_add(BITS.saturating_sub(1))
            .checked_div(BITS)?;
        let mut rows: HashMap<i32, Vec<u8>> = HashMap::new();
        for leaf in &bsp.leafs {
            let Ok(at) = usize::try_from(leaf.visofs) else {
                continue;
            };
            if rows.contains_key(&leaf.visofs) {
                continue;
            }
            let Some(packed) = bsp.visibility.get(at..) else {
                continue;
            };
            rows.insert(leaf.visofs, decompressed(packed, row));
        }
        let mut face_leaves = vec![Vec::new(); bsp.faces.len()];
        for (index, leaf) in bsp.leafs.iter().enumerate() {
            let marks = bsp
                .marksurfaces
                .get(
                    leaf.firstmarksurface
                        ..leaf.firstmarksurface.saturating_add(leaf.nummarksurfaces),
                )
                .unwrap_or_default();
            for &face in marks {
                if let Some(of_face) = face_leaves.get_mut(face) {
                    of_face.push(index);
                }
            }
        }
        Some(Self {
            row,
            rows,
            face_leaves,
        })
    }

    /// The leaves the world's face `face` is in.
    pub(crate) fn leaves_of(&self, face: usize) -> &[usize] {
        self.face_leaves.get(face).map_or(&[], Vec::as_slice)
    }

    /// `CalcPvs`: what the leaves `leaves` can see between them; everything when one of them
    /// is a liquid, or has no row.
    pub(crate) fn seen_from(&self, bsp: &Bsp<'_>, leaves: &[usize]) -> Vec<u8> {
        let mut seen = vec![0; self.row];
        for &leaf in leaves {
            let Some(leaf) = bsp.leafs.get(leaf) else {
                continue;
            };
            let row = self.rows.get(&leaf.visofs);
            if is_liquid(leaf.contents) {
                return vec![EVERYTHING; self.row];
            }
            for (at, slot) in seen.iter_mut().enumerate() {
                *slot |= row.map_or(EVERYTHING, |row| row.get(at).copied().unwrap_or(0));
            }
        }
        seen
    }
}

/// `DecompressVis`: `row` bytes of a row from its run-length packing, a 0 followed by how
/// many bytes of 0 it stands for; what is not there stays 0.
fn decompressed(packed: &[u8], row: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(row);
    let mut bytes = packed.iter().copied();
    while out.len() < row {
        match bytes.next() {
            Some(0) => {
                let Some(run) = bytes.next().filter(|run| *run != 0) else {
                    break;
                };
                let end = out.len().saturating_add(usize::from(run)).min(row);
                out.resize(end, 0);
            }
            Some(byte) => out.push(byte),
            None => break,
        }
    }
    out.resize(row, 0);
    out
}

/// `Light_PointInLeaf`: the leaf of the world's tree `point` is in.
pub(crate) fn leaf_at(bsp: &Bsp<'_>, point: Vec3) -> Option<usize> {
    let mut at: i32 = 0;
    loop {
        let Ok(node) = usize::try_from(at) else {
            return usize::try_from(at.saturating_add(1).saturating_neg()).ok();
        };
        let node = bsp.nodes.get(node)?;
        let plane = bsp.planes.get(node.planenum)?;
        let n = plane.normal;
        let dist = n[0] * point[0] + n[1] * point[1] + n[2] * point[2] - plane.dist;
        let [front, back] = node.children;
        at = if dist < 0.0 { back } else { front };
    }
}

/// `VisCullEntity`: whether a light in the leaf `leaf` cannot reach what sees `seen`: its
/// leaf is not among them. A light in solid, sky or a liquid, or in no leaf, is never culled.
pub(crate) fn is_unseen(bsp: &Bsp<'_>, seen: &[u8], leaf: Option<usize>) -> bool {
    let Some(number) = leaf else {
        return false;
    };
    if seen.is_empty() {
        return false;
    }
    let Some(leaf) = bsp.leafs.get(number) else {
        return false;
    };
    if is_liquid(leaf.contents) || matches!(leaf.contents, CONTENTS_SOLID | CONTENTS_SKY) {
        return false;
    }
    // `Pvs_LeafVisible`: the first leaf is the solid one, which has no bit.
    let Some(bit) = number.checked_sub(1) else {
        return true;
    };
    let byte = bit.checked_div(BITS).and_then(|at| seen.get(at)).copied();
    let mask = 1_u8.checked_shl(u32::try_from(bit.checked_rem(BITS).unwrap_or(0)).unwrap_or(0));
    !matches!((byte, mask), (Some(byte), Some(mask)) if byte & mask != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec LIGHT-10.10: a row of visibility is run-length packed: a 0 and a count stand for
    // that many bytes of 0.
    #[test]
    fn a_row_is_unpacked_from_its_runs() {
        assert_eq!(decompressed(&[5, 0, 3, 7], 5), [5, 0, 0, 0, 7]);
        assert_eq!(decompressed(&[1, 2], 4), [1, 2, 0, 0], "a short row");
        assert_eq!(decompressed(&[1, 0, 9], 3), [1, 0, 0], "a run past the row");
        assert_eq!(
            decompressed(&[0, 0, 4], 2),
            [0, 0],
            "a run of nothing ends it"
        );
    }
}
