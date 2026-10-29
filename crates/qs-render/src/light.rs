// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/gl_rlight.c, R_LightPoint, RecursiveLightPoint

//! The light at a point of the world, which an alias model is shaded by (REND-4.6).

use qs_bsp::{Bsp, SURF_DRAWTILED};

use crate::mesh::UNSET_STYLE;

/// `end[2] = p[2] - 2048`: how far below the point the light is looked for (REND-4.6).
const LIGHT_DEPTH: f32 = 2048.0;
/// `r >>= 8` in `RecursiveLightPoint` (REND-4.6).
const LIGHT_SHIFT: u32 = 8;
/// The light of a map with no lighting lump (REND-4.6).
const FULL_BRIGHT: i32 = 255;
/// The world units of one lightmap sample, `ds >>= 4`.
const SAMPLE_SHIFT: u32 = 4;

/// `R_LightPoint`: the light of the first lit world surface straight below `point`, within
/// 2048 units; 0 when there is none, 255 in a map with no lighting lump (REND-4.6).
#[must_use]
pub fn light_point(bsp: &Bsp<'_>, point: [f32; 3]) -> i32 {
    if bsp.lighting.is_empty() {
        return FULL_BRIGHT;
    }
    let [x, y, z] = point;
    let end = [x, y, z - LIGHT_DEPTH];
    // A tree is never deeper than its node count; a cycle in a damaged one ends there.
    recursive_light_point(bsp, 0, point, end, bsp.nodes.len()).map_or(0, |r| r.max(0))
}

/// `RecursiveLightPoint`: `None` for id's -1, nothing hit.
fn recursive_light_point(
    bsp: &Bsp<'_>,
    child: i32,
    start: [f32; 3],
    end: [f32; 3],
    depth: usize,
) -> Option<i32> {
    // "didn't hit anything": a leaf.
    let node = bsp.nodes.get(usize::try_from(child).ok()?)?;
    let depth = depth.checked_sub(1)?;
    let plane = bsp.planes.get(node.planenum)?;
    let front = dot(start, plane.normal) - plane.dist;
    let back = dot(end, plane.normal) - plane.dist;
    let [first, second] = node.children;
    let (near, far) = if front < 0.0 {
        (second, first)
    } else {
        (first, second)
    };
    if (back < 0.0) == (front < 0.0) {
        return recursive_light_point(bsp, near, start, end, depth);
    }
    let frac = front / (front - back);
    let mut mid = start;
    for ((m, s), e) in mid.iter_mut().zip(start).zip(end) {
        *m = s + (e - s) * frac;
    }
    if let Some(r) = recursive_light_point(bsp, near, start, mid, depth)
        && r >= 0
    {
        return Some(r);
    }
    let faces = bsp
        .faces
        .get(node.firstface..node.firstface.saturating_add(node.numfaces))
        .unwrap_or_default();
    for face in faces {
        if face.flags & SURF_DRAWTILED != 0 {
            continue;
        }
        let Some(tex) = bsp.texinfo.get(face.texinfo) else {
            continue;
        };
        let [svec, tvec] = tex.vecs;
        let s = truncate(dot(mid, vec3(svec)) + svec[3]);
        let t = truncate(dot(mid, vec3(tvec)) + tvec[3]);
        let [smin, tmin] = face.texturemins.map(i32::from);
        if s < smin || t < tmin {
            continue;
        }
        let (Some(ds), Some(dt)) = (s.checked_sub(smin), t.checked_sub(tmin)) else {
            continue;
        };
        let [sext, text] = face.extents.map(i32::from);
        if ds > sext || dt > text {
            continue;
        }
        let Some(samples) = bsp.face_samples(face) else {
            return Some(0);
        };
        let width = usize::try_from((sext >> SAMPLE_SHIFT).checked_add(1)?).ok()?;
        let height = usize::try_from((text >> SAMPLE_SHIFT).checked_add(1)?).ok()?;
        let at = usize::try_from(dt >> SAMPLE_SHIFT)
            .ok()?
            .checked_mul(width)?
            .checked_add(usize::try_from(ds >> SAMPLE_SHIFT).ok()?)?;
        let size = width.checked_mul(height)?;
        let mut r = 0_u32;
        for map in 0..face.style_count() {
            let sample = samples.get(map.checked_mul(size)?.checked_add(at)?)?;
            r = r.saturating_add(u32::from(*sample).saturating_mul(UNSET_STYLE));
        }
        return i32::try_from(r >> LIGHT_SHIFT).ok();
    }
    recursive_light_point(bsp, far, mid, end, depth)
}

fn dot([a, b, c]: [f32; 3], [x, y, z]: [f32; 3]) -> f32 {
    a * x + b * y + c * z
}

/// A texinfo vector's direction, without its offset.
const fn vec3([x, y, z, _]: [f32; 4]) -> [f32; 3] {
    [x, y, z]
}

/// C's float to int conversion, toward zero (REND-4.6); a value past `i32` is out of every
/// surface's extents, so it is pinned to the nearer end.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec REND-4.6: id assigns the float to an int, which truncates toward zero"
)]
fn truncate(value: f32) -> i32 {
    value as i32
}

#[cfg(test)]
mod tests;
