// SPDX-License-Identifier: GPL-3.0-or-later

use qs_bsp::{BSP_CONTENTS_LAVA, BSP_CONTENTS_SKY, BSP_CONTENTS_SLIME, BSP_CONTENTS_WATER};
use qs_client::Contents;

use super::contents_at;
use crate::game::tests::floor;

// spec VIEW-4.3: the shift follows the eye's leaf: empty and solid none, lava and slime
// their own, water and any other water's
#[test]
fn the_eye_is_in_its_leafs_contents() {
    // The floor's leaf 1 is above z = 0, leaf 0 the solid below.
    let mut bsp = floor(b"");
    let above = [0.0, 0.0, 10.0];
    assert_eq!(contents_at(&bsp, above), Contents::Empty);
    assert_eq!(
        contents_at(&bsp, [0.0, 0.0, -10.0]),
        Contents::Empty,
        "solid"
    );
    for (contents, seen) in [
        (BSP_CONTENTS_WATER, Contents::Water),
        (BSP_CONTENTS_SLIME, Contents::Slime),
        (BSP_CONTENTS_LAVA, Contents::Lava),
        (BSP_CONTENTS_SKY, Contents::Water),
    ] {
        if let Some(leaf) = bsp.leafs.get_mut(1) {
            leaf.contents = contents;
        }
        assert_eq!(contents_at(&bsp, above), seen, "{contents}");
    }
    bsp.nodes.clear();
    assert_eq!(contents_at(&bsp, above), Contents::Empty, "no tree");
}
