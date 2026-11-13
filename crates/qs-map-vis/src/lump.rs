// SPDX-License-Identifier: GPL-3.0-or-later

//! The visibility lump (VIS-5) and the ambient levels (VIS-6), written into a BSP: the rows
//! compressed one after another, each vis leaf's `visofs` and `ambient_level` set in place.

use qs_bsp::{BSP_HEADER_LUMPS, BSP_HEADER_SIZE, Bsp, BspVersion, LumpId};

use crate::VisError;
use crate::flow::Leaves;

/// The longest run of zero bytes one count holds (VIS-5.1).
const MAX_RUN: u8 = 255;
/// `MAX_MAP_VISIBILITY` (VIS-5.2).
pub const VIS_MAX_MAP_VISIBILITY: usize = 0x10_0000;
/// The ambient kinds: water, sky, slime and lava (VIS-6).
const AMBIENTS: usize = 4;
/// The ambient slots, `AMBIENT_WATER` to `AMBIENT_LAVA` (BSP-12.4).
const AMBIENT_WATER: usize = 0;
const AMBIENT_SKY: usize = 1;
const AMBIENT_LAVA: usize = 3;
/// The level of a kind a leaf sees an emitter of: its distance is 0.25, below 100 (VIS-6.2,
/// VIS-6.3).
const HEARD: u8 = u8::MAX;
/// The level of any other: `1 - dists[2] * 0.002` with the slime distance always 1020, below 0
/// and so clamped to 0 (VIS-6.3).
const SILENT: u8 = 0;

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
/// vis's distances come to that: each is 0.25 or 1020, and the level past 100 reads the slime
/// one, which no face sets (VIS-6.2, VIS-6.3).
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
            let mut levels = [SILENT; AMBIENTS];
            for (leaf, kinds) in emits.iter().enumerate() {
                if !row.has(leaf) {
                    continue;
                }
                for (level, &emits) in levels.iter_mut().zip(kinds) {
                    if emits {
                        *level = HEARD;
                    }
                }
            }
            levels
        })
        .collect()
}

/// The size of one leaf record in `version`, its four ambient bytes last (BSP-12, BSP-17).
const fn leaf_size(version: BspVersion) -> Option<usize> {
    match version {
        BspVersion::Bsp29 => Some(LEAF_29),
        BspVersion::Bsp2Psb => Some(LEAF_2PSB),
        BspVersion::Bsp2 => Some(LEAF_BSP2),
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
/// The bytes before the lump directory: the version.
const VERSION_SIZE: usize = 4;
/// The bytes of one directory entry: offset and length.
const ENTRY_SIZE: usize = 8;
/// Lumps start on four-byte boundaries.
const ALIGN: usize = 4;
/// The lumps in the order `WriteBSPFile` writes them (VIS-5.6).
const ORDER: [LumpId; BSP_HEADER_LUMPS] = [
    LumpId::Planes,
    LumpId::Leafs,
    LumpId::Vertexes,
    LumpId::Nodes,
    LumpId::Texinfo,
    LumpId::Faces,
    LumpId::Clipnodes,
    LumpId::Marksurfaces,
    LumpId::Surfedges,
    LumpId::Edges,
    LumpId::Models,
    LumpId::Lighting,
    LumpId::Visibility,
    LumpId::Entities,
    LumpId::Textures,
];

/// `bytes` with the rows' visibility lump, each vis leaf's `visofs` and ambient levels, laid
/// out as `WriteBSPFile` writes a file: the header, then each lump in its order, padded to
/// four bytes (VIS-5.2, VIS-5.6, VIS-6).
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
    let record = leaf_size(bsp.version).ok_or(VisError::Format)?;
    let ambient_at = record.checked_sub(AMBIENTS).ok_or(VisError::Format)?;
    let stored = |id: LumpId| -> Result<&[u8], VisError> {
        let entry = bsp.lumps.get(id.index()).ok_or(VisError::Format)?;
        let start = usize::try_from(entry.fileofs).map_err(|_| VisError::Format)?;
        let len = usize::try_from(entry.filelen).map_err(|_| VisError::Format)?;
        bytes
            .get(start..start.saturating_add(len))
            .ok_or(VisError::Format)
    };
    let mut leaf_lump = stored(LumpId::Leafs)?.to_vec();
    for (leaf, (offset, ambient)) in offsets.iter().zip(&levels).enumerate() {
        let at = leaf
            .checked_add(1)
            .and_then(|n| n.checked_mul(record))
            .ok_or(VisError::Format)?;
        let visofs = i32::try_from(*offset).map_err(|_| VisError::VismapOverflow)?;
        let field = leaf_lump
            .get_mut(
                at.saturating_add(VISOFS_AT)
                    ..at.saturating_add(VISOFS_AT).saturating_add(VISOFS_SIZE),
            )
            .ok_or(VisError::Format)?;
        field.copy_from_slice(&visofs.to_le_bytes());
        let field = leaf_lump
            .get_mut(
                at.saturating_add(ambient_at)
                    ..at.saturating_add(ambient_at).saturating_add(AMBIENTS),
            )
            .ok_or(VisError::Format)?;
        field.copy_from_slice(ambient);
    }
    let mut out = vec![0; BSP_HEADER_SIZE];
    if let (Some(version), Some(head)) = (bytes.get(..VERSION_SIZE), out.get_mut(..VERSION_SIZE)) {
        head.copy_from_slice(version);
    }
    for id in ORDER {
        let data: &[u8] = match id {
            LumpId::Leafs => &leaf_lump,
            LumpId::Visibility => &lump,
            _ => stored(id)?,
        };
        let start = i32::try_from(out.len()).map_err(|_| VisError::Format)?;
        let len = i32::try_from(data.len()).map_err(|_| VisError::Format)?;
        out.extend_from_slice(data);
        // `AddLump` pads from the lump's array: past the new rows, `dvisdata` still holds the
        // input's visibility there; past any other lump, zeros (VIS-5.5).
        let old: &[u8] = if id == LumpId::Visibility {
            stored(id)?
        } else {
            &[]
        };
        let mut at = data.len();
        while !out.len().is_multiple_of(ALIGN) {
            out.push(old.get(at).copied().unwrap_or(0));
            at = at.saturating_add(1);
        }
        let entry = VERSION_SIZE.saturating_add(id.index().saturating_mul(ENTRY_SIZE));
        let field = out
            .get_mut(entry..entry.saturating_add(ENTRY_SIZE))
            .ok_or(VisError::Format)?;
        let [a, b, c, d] = start.to_le_bytes();
        let [e, f, g, h] = len.to_le_bytes();
        field.copy_from_slice(&[a, b, c, d, e, f, g, h]);
    }
    Ok(out)
}
