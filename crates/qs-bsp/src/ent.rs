// SPDX-License-Identifier: GPL-3.0-or-later

//! `.ent` files: entity text that replaces a BSP's entity lump (BSP-19).
//!
//! Described from QuakeSpasm's `Mod_LoadEntities`, cited in `docs/spec/bsp.md` for
//! description only; nothing is ported. With [`crate::replace_entities`] it gives the two
//! ways to edit the entities of a shipped map that has no `.map` source (MAP-22).

use qc_progs::crc::crc_block;

use crate::lit::companion_name;

/// The extension of an entity file (BSP-19.1).
pub const BSP_ENT_EXTENSION: &str = ".ent";

/// The names QuakeSpasm tries, in order, for the entities of the BSP named `bsp_name`
/// whose entity lump is `entity_lump`: `<map>@<crc>.ent`, then `<map>.ent`, where `<crc>`
/// is the CRC of the lump without its last byte as four lower-case hexadecimal digits, 0
/// for an empty lump (BSP-19.1).
#[must_use]
pub fn ent_file_names(bsp_name: &str, entity_lump: &[u8]) -> [String; 2] {
    let crc = entity_lump
        .len()
        .checked_sub(1)
        .and_then(|len| entity_lump.get(..len))
        .map_or(0, crc_block);
    [
        companion_name(bsp_name, &format!("@{crc:04x}{BSP_ENT_EXTENSION}")),
        companion_name(bsp_name, BSP_ENT_EXTENSION),
    ]
}
