// SPDX-License-Identifier: GPL-3.0-or-later

//! The lit BSP as `WriteBSPFile` lays it out: the header, then every lump in its order, each
//! padded to four bytes, with the new lighting and entity lumps and each face's styles and
//! `lightofs` (LIGHT-1.5, LIGHT-1.6, LIGHT-5); and the `.lit` file beside it (BSP-18.1,
//! LIGHT-6.4).

use qs_bsp::{
    BSP_BSP2_FACE_SIZE, BSP_FACE_SIZE, BSP_HEADER_LUMPS, BSP_HEADER_SIZE, BSP_MAXLIGHTMAPS, Bsp,
    BspVersion, LumpId,
};

use qs_map_bsp::Dialect;

use crate::face::Lit;
use crate::{AXES, LightError, Rgb};

/// `MAX_MAP_LIGHTING` (LIGHT-1.5).
pub const LIGHT_MAX_MAP_LIGHTING: usize = 0x10_0000;
/// Lightmaps and lumps start on four-byte boundaries (LIGHT-1.5).
const ALIGN: usize = 4;
/// The style of an unused slot, and the `lightofs` of an unlit face (LIGHT-5.1).
const NO_STYLE: u8 = 255;
const UNLIT: i32 = -1;
/// Where a BSP29 face's styles start; its `lightofs` follows them.
const STYLES_29: usize = 12;
/// Where a `BSP2` face's styles start.
const STYLES_BSP2: usize = 20;
/// The bytes before the lump directory: the version.
const VERSION_SIZE: usize = 4;
/// The bytes of a directory entry.
const ENTRY_SIZE: usize = 8;
/// `.lit`'s header: `QLIT` and version 1 (BSP-18.1).
const LIT_IDENT: &[u8; 4] = b"QLIT";
const LIT_VERSION: i32 = 1;
/// The lumps in the order `WriteBSPFile` writes them.
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

/// Whether any light is coloured, which makes the grey byte the brightest channel and writes a
/// `.lit` (LIGHT-6.4, LIGHT-6.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Colour {
    /// Every light is white: the grey bytes are id's.
    White,
    /// Some light has a colour.
    Coloured,
}

/// The files a lit map is written as.
pub(crate) struct Written {
    pub(crate) bsp: Vec<u8>,
    pub(crate) lit: Option<Vec<u8>>,
}

/// Each face's place in the lighting lump: its lightmaps one after another, each face from a
/// four-byte boundary (LIGHT-1.5), with the grey bytes, or each sample's brightest channel
/// when the map is coloured (LIGHT-6.4). The bytes skipped to reach a boundary are never
/// written, so they keep what the input's lighting lump held there (LIGHT-1.8).
struct Lighting {
    grey: Vec<u8>,
    rgb: Vec<Rgb>,
    offsets: Vec<Option<usize>>,
}

impl Lighting {
    fn new(
        faces: &[Option<Lit>],
        old: &[u8],
        colour: Colour,
        dialect: Dialect,
    ) -> Result<Self, LightError> {
        let mut out = Self {
            grey: Vec::new(),
            rgb: Vec::new(),
            offsets: Vec::with_capacity(faces.len()),
        };
        for face in faces {
            let Some(lit) = face else {
                out.offsets.push(None);
                continue;
            };
            while !out.grey.len().is_multiple_of(ALIGN) {
                out.grey.push(stale(old, out.grey.len()));
                out.rgb.push([0; AXES]);
            }
            out.offsets.push(Some(out.grey.len()));
            if colour == Colour::Coloured {
                out.grey
                    .extend(lit.rgb.iter().map(|c| c.iter().copied().max().unwrap_or(0)));
            } else {
                out.grey.extend_from_slice(&lit.grey);
            }
            out.rgb.extend_from_slice(&lit.rgb);
            if dialect == Dialect::Id && out.grey.len() > LIGHT_MAX_MAP_LIGHTING {
                return Err(LightError::Overrun);
            }
        }
        Ok(out)
    }
}

/// The BSP `bytes`, read as `bsp`, with `faces`' lighting and the entity text `entities`, and
/// the `.lit` when the map is coloured.
pub(crate) fn written(
    bytes: &[u8],
    bsp: &Bsp<'_>,
    faces: &[Option<Lit>],
    entities: &[u8],
    colour: Colour,
    dialect: Dialect,
) -> Result<Written, LightError> {
    let lump = |id: LumpId| -> Result<&[u8], LightError> {
        let entry = bsp.lumps.get(id.index()).ok_or(LightError::Format)?;
        let start = usize::try_from(entry.fileofs).map_err(|_| LightError::Format)?;
        let len = usize::try_from(entry.filelen).map_err(|_| LightError::Format)?;
        bytes
            .get(start..start.saturating_add(len))
            .ok_or(LightError::Format)
    };
    let lighting = Lighting::new(faces, lump(LumpId::Lighting)?, colour, dialect)?;
    let (record, styles_at) = match bsp.version {
        BspVersion::Bsp29 => (BSP_FACE_SIZE, STYLES_29),
        BspVersion::Bsp2Psb | BspVersion::Bsp2 => (BSP_BSP2_FACE_SIZE, STYLES_BSP2),
        BspVersion::Valve => return Err(LightError::Format),
    };
    let mut face_lump = lump(LumpId::Faces)?.to_vec();
    for (index, (face, offset)) in faces.iter().zip(&lighting.offsets).enumerate() {
        let mut styles = [NO_STYLE; BSP_MAXLIGHTMAPS];
        if let Some(lit) = face {
            for (slot, &style) in styles.iter_mut().zip(&lit.styles) {
                *slot = style;
            }
        }
        let lightofs = match offset {
            Some(at) => i32::try_from(*at).map_err(|_| LightError::Overrun)?,
            None => UNLIT,
        };
        let at = index
            .checked_mul(record)
            .and_then(|n| n.checked_add(styles_at))
            .ok_or(LightError::Format)?;
        let field = face_lump
            .get_mut(at..at.saturating_add(BSP_MAXLIGHTMAPS).saturating_add(ALIGN))
            .ok_or(LightError::Format)?;
        let [a, b, c, d] = styles;
        let [e, f, g, h] = lightofs.to_le_bytes();
        field.copy_from_slice(&[a, b, c, d, e, f, g, h]);
    }
    let mut out = vec![0; BSP_HEADER_SIZE];
    if let (Some(version), Some(head)) = (bytes.get(..VERSION_SIZE), out.get_mut(..VERSION_SIZE)) {
        head.copy_from_slice(version);
    }
    for id in ORDER {
        let data: &[u8] = match id {
            LumpId::Faces => &face_lump,
            LumpId::Lighting => &lighting.grey,
            LumpId::Entities => entities,
            _ => lump(id)?,
        };
        let start = i32::try_from(out.len()).map_err(|_| LightError::Format)?;
        let len = i32::try_from(data.len()).map_err(|_| LightError::Format)?;
        out.extend_from_slice(data);
        // `AddLump` writes the lump to a multiple of four from its array: past a lump `light`
        // rewrote, the bytes its input held there; past any other, zeros (LIGHT-1.8).
        let old: &[u8] = match id {
            LumpId::Lighting | LumpId::Entities => lump(id)?,
            _ => &[],
        };
        let mut at = data.len();
        while !out.len().is_multiple_of(ALIGN) {
            out.push(stale(old, at));
            at = at.saturating_add(1);
        }
        let entry = VERSION_SIZE.saturating_add(id.index().saturating_mul(ENTRY_SIZE));
        let field = out
            .get_mut(entry..entry.saturating_add(ENTRY_SIZE))
            .ok_or(LightError::Format)?;
        let [a, b, c, d] = start.to_le_bytes();
        let [e, f, g, h] = len.to_le_bytes();
        field.copy_from_slice(&[a, b, c, d, e, f, g, h]);
    }
    let lit = (colour == Colour::Coloured).then(|| {
        let mut lit = LIT_IDENT.to_vec();
        lit.extend_from_slice(&LIT_VERSION.to_le_bytes());
        lit.extend(lighting.rgb.iter().flatten());
        lit
    });
    Ok(Written { bsp: out, lit })
}

/// The byte the input's lump `old` held at `at`, or 0 past it: what `light`'s array still holds
/// where it writes nothing (LIGHT-1.8).
fn stale(old: &[u8], at: usize) -> u8 {
    old.get(at).copied().unwrap_or(0)
}
