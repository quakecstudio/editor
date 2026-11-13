// SPDX-License-Identifier: GPL-3.0-or-later

//! The visibility lump (VIS-5) and the ambient levels (VIS-6), written into a BSP: the rows
//! compressed one after another, each vis leaf's `visofs` and `ambient_level` set in place.

use qs_bsp::{Bsp, BspVersion, LumpId};

use crate::VisError;
use crate::flow::Leaves;

/// The longest run of zero bytes one count holds (VIS-5.1).
const MAX_RUN: u8 = 255;
/// `MAX_MAP_VISIBILITY` (VIS-5.2).
pub const VIS_MAX_MAP_VISIBILITY: usize = 0x10_0000;
/// The ambient distance with no emitter seen (VIS-6.2).
const VIS_AMBIENT_FAR: f32 = 1020.0;
/// The distance below which an ambient is at full level (VIS-6.3).
const VIS_AMBIENT_NEAR: f32 = 100.0;
/// The distance vis takes for every emitter it sees (VIS-6.2).
const SEEN_DISTANCE: f32 = 0.25;
/// How fast the level falls with distance (VIS-6.3).
const FALLOFF: f32 = 0.002;
/// The ambient kinds: water, sky, slime and lava (VIS-6).
const AMBIENTS: usize = 4;
/// The ambient slots, `AMBIENT_WATER` to `AMBIENT_LAVA` (BSP-12.4).
const AMBIENT_WATER: usize = 0;
const AMBIENT_SKY: usize = 1;
/// The slime kind, whose distance every level past the near one reads (VIS-6.3).
const SLIME: usize = 2;
const AMBIENT_LAVA: usize = 3;

/// An ambient sound a texture makes (VIS-6.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ambient {
    Water,
    Sky,
    Lava,
}

impl Ambient {
    /// The kind a texture `name` emits, compared over the bytes vis compares (VIS-6.1).
    fn of(name: &[u8]) -> Option<Self> {
        let starts = |prefix: &[u8]| {
            name.get(..prefix.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
        };
        // `*lava` is compared over 6 bytes, its NUL among them: only the name itself.
        if starts(b"*water") || starts(b"*slime") || starts(b"*04water") {
            Some(Self::Water)
        } else if starts(b"sky") {
            Some(Self::Sky)
        } else if name.eq_ignore_ascii_case(b"*lava") {
            Some(Self::Lava)
        } else {
            None
        }
    }

    /// Its slot in `ambient_level` (BSP-12.4).
    const fn slot(self) -> usize {
        match self {
            Self::Water => AMBIENT_WATER,
            Self::Sky => AMBIENT_SKY,
            Self::Lava => AMBIENT_LAVA,
        }
    }
}

/// `CompressRow`: a non-zero byte as it is, a run of zero bytes as a zero and its count, at most
/// 255 (VIS-5.1).
pub(crate) fn compress(row: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(row.len());
    let mut bytes = row.iter().peekable();
    while let Some(&byte) = bytes.next() {
        out.push(byte);
        if byte != 0 {
            continue;
        }
        let mut run = 1_u8;
        while run < MAX_RUN && bytes.next_if_eq(&&0).is_some() {
            run = run.saturating_add(1);
        }
        out.push(run);
    }
    out
}

/// Each vis leaf's four ambient levels: full for a kind it sees a face of, else none (VIS-6).
fn ambients(bsp: &Bsp<'_>, rows: &[Leaves]) -> Vec<[u8; AMBIENTS]> {
    // The kinds each vis leaf's faces emit.
    let emits: Vec<[bool; AMBIENTS]> = (0..rows.len())
        .map(|leaf| {
            let mut kinds = [false; AMBIENTS];
            let Some(found) = bsp.leafs.get(leaf.saturating_add(1)) else {
                return kinds;
            };
            let marks = bsp
                .marksurfaces
                .iter()
                .skip(found.firstmarksurface)
                .take(found.nummarksurfaces);
            for &face in marks {
                let name = bsp
                    .faces
                    .get(face)
                    .and_then(|f| bsp.texinfo.get(f.texinfo))
                    .and_then(|info| usize::try_from(info.miptex).ok())
                    .and_then(|slot| bsp.textures.get(slot))
                    .and_then(Option::as_ref)
                    .map(|texture| texture.name.as_slice());
                if let Some(kind) = name.and_then(Ambient::of)
                    && let Some(flag) = kinds.get_mut(kind.slot())
                {
                    *flag = true;
                }
            }
            kinds
        })
        .collect();
    rows.iter()
        .map(|row| {
            let mut dists = [VIS_AMBIENT_FAR; AMBIENTS];
            for (leaf, kinds) in emits.iter().enumerate() {
                if !row.has(leaf) {
                    continue;
                }
                for (dist, &emits) in dists.iter_mut().zip(kinds) {
                    if emits {
                        *dist = dist.min(SEEN_DISTANCE);
                    }
                }
            }
            let slime = dists.get(SLIME).copied().unwrap_or(VIS_AMBIENT_FAR);
            dists.map(|d| {
                if d < VIS_AMBIENT_NEAR {
                    u8::MAX
                } else {
                    level(1.0 - slime * FALLOFF)
                }
            })
        })
        .collect()
}

/// A volume from 0 to 1 as a byte, as C converts `vol * 255`, clamped at 0 (VIS-6.3).
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "spec VIS-6.3: vis stores vol * 255, vol clamped at 0, in a byte"
)]
fn level(volume: f32) -> u8 {
    (volume.max(0.0) * f32::from(u8::MAX)) as u8
}

/// The size of one leaf record in `version`, and where its ambient bytes start (BSP-12, BSP-17).
const fn leaf_layout(version: BspVersion) -> Option<(usize, usize)> {
    match version {
        BspVersion::Bsp29 => Some((LEAF_29, LEAF_29 - AMBIENTS)),
        BspVersion::Bsp2Psb => Some((LEAF_2PSB, LEAF_2PSB - AMBIENTS)),
        BspVersion::Bsp2 => Some((LEAF_BSP2, LEAF_BSP2 - AMBIENTS)),
        BspVersion::Valve => None,
    }
}

/// The bytes of a leaf record: BSP29's 16-bit bounds and marksurfaces, `2PSB`'s 16-bit bounds
/// and 32-bit marksurfaces, `BSP2`'s float bounds (BSP-12, BSP-17); its four ambient bytes end it.
const LEAF_29: usize = 28;
const LEAF_2PSB: usize = 32;
const LEAF_BSP2: usize = 44;
/// Where a leaf record's `visofs` starts, past `contents`.
const VISOFS_AT: usize = 4;
/// The bytes of `visofs`.
const VISOFS_SIZE: usize = 4;
/// Where the lump directory starts, past the version.
const DIRECTORY_AT: usize = 4;
/// The bytes of one directory entry: offset and length.
const ENTRY_SIZE: usize = 8;
/// Lumps start on four-byte boundaries.
const ALIGN: usize = 4;

/// `bytes` with the rows' visibility lump, each vis leaf's `visofs` and ambient levels; the new
/// lump is put at the end of the file and the directory pointed at it (VIS-5.2, VIS-6).
pub(crate) fn written(bytes: &[u8], bsp: &Bsp<'_>, rows: &[Leaves]) -> Result<Vec<u8>, VisError> {
    let leafs = rows.len();
    let mut lump = Vec::new();
    let mut offsets = Vec::with_capacity(leafs);
    for row in rows {
        offsets.push(lump.len());
        lump.extend(compress(&row.row(leafs)));
    }
    if lump.len() > VIS_MAX_MAP_VISIBILITY {
        return Err(VisError::VismapOverflow);
    }
    let levels = ambients(bsp, rows);
    let (record, ambient_at) = leaf_layout(bsp.version).ok_or(VisError::Format)?;
    let leaf_lump = bsp
        .lumps
        .get(LumpId::Leafs as usize)
        .ok_or(VisError::Format)?;
    let leaf_start = usize::try_from(leaf_lump.fileofs).map_err(|_| VisError::Format)?;
    let mut out = bytes.to_vec();
    for (leaf, (offset, ambient)) in offsets.iter().zip(&levels).enumerate() {
        let at = leaf
            .checked_add(1)
            .and_then(|n| n.checked_mul(record))
            .and_then(|n| n.checked_add(leaf_start))
            .ok_or(VisError::Format)?;
        let visofs = i32::try_from(*offset).map_err(|_| VisError::VismapOverflow)?;
        let field = out
            .get_mut(
                at.saturating_add(VISOFS_AT)
                    ..at.saturating_add(VISOFS_AT).saturating_add(VISOFS_SIZE),
            )
            .ok_or(VisError::Format)?;
        field.copy_from_slice(&visofs.to_le_bytes());
        let field = out
            .get_mut(
                at.saturating_add(ambient_at)
                    ..at.saturating_add(ambient_at).saturating_add(AMBIENTS),
            )
            .ok_or(VisError::Format)?;
        field.copy_from_slice(ambient);
    }
    while !out.len().is_multiple_of(ALIGN) {
        out.push(0);
    }
    let start = i32::try_from(out.len()).map_err(|_| VisError::Format)?;
    let length = i32::try_from(lump.len()).map_err(|_| VisError::Format)?;
    out.extend(lump);
    let entry =
        DIRECTORY_AT.saturating_add((LumpId::Visibility as usize).saturating_mul(ENTRY_SIZE));
    let field = out
        .get_mut(entry..entry.saturating_add(ENTRY_SIZE))
        .ok_or(VisError::Format)?;
    let [a, b, c, d] = start.to_le_bytes();
    let [e, f, g, h] = length.to_le_bytes();
    field.copy_from_slice(&[a, b, c, d, e, f, g, h]);
    Ok(out)
}
