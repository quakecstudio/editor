// SPDX-License-Identifier: GPL-3.0-or-later

//! What the eye is in, for the view's contents shift (VIEW-4.3).

use qs_bsp::{BSP_CONTENTS_EMPTY, BSP_CONTENTS_LAVA, BSP_CONTENTS_SLIME, BSP_CONTENTS_SOLID, Bsp};
use qs_client::Contents;

/// The contents of the leaf `eye` is in (BSP-21.1), as `V_SetContentsColor` tells them apart
/// (VIEW-4.3): empty and solid show no shift, lava and slime their own, any other water's.
pub(crate) fn contents_at(bsp: &Bsp<'_>, eye: [f32; 3]) -> Contents {
    let contents = bsp
        .point_in_leaf(eye)
        .and_then(|leaf| bsp.leafs.get(leaf))
        .map_or(BSP_CONTENTS_EMPTY, |leaf| leaf.contents);
    match contents {
        BSP_CONTENTS_EMPTY | BSP_CONTENTS_SOLID => Contents::Empty,
        BSP_CONTENTS_LAVA => Contents::Lava,
        BSP_CONTENTS_SLIME => Contents::Slime,
        _ => Contents::Water,
    }
}

#[cfg(test)]
mod tests;
