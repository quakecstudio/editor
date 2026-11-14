// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::indexing_slicing,
    reason = "test code: a failed index is a failed test"
)]

use super::{Contents, Game, NativeContents, TransWater, Unrepresentable};
use crate::extended::winding::Side;

const fn c(flags: u64) -> Contents {
    Contents::new(flags)
}

// spec QBSP-15.9: detail contents are made as ericw-tools makes them; solid means structural
// solid; solid and sky seal the map
#[test]
fn detail_contents_and_their_kinds() {
    let solid = c(Contents::SOLID);
    assert!(solid.is_solid() && solid.is_any_solid() && solid.seals_map());
    let detail = solid.detail_solid();
    assert!(detail.is_detail_solid() && detail.is_any_detail() && !detail.is_solid());
    assert!(!detail.seals_map() && detail.is_any_solid());
    let wall = solid.detail_wall();
    assert!(wall.is_detail_wall() && !wall.is_any_solid() && !wall.is_fence());
    let fence = solid.detail_fence();
    assert!(fence.is_detail_fence() && fence.is_fence() && !fence.is_opaque());
    let mist = solid.detail_illusionary();
    assert!(mist.is_detail_illusionary() && mist.is_fence());
    assert_eq!(
        mist.flags() & (Contents::MIRROR_INSIDE_SET | Contents::MIRROR_INSIDE),
        Contents::MIRROR_INSIDE_SET
    );
    assert!(c(Contents::SKY).seals_map() && c(Contents::SKY).is_sky());
    assert!(Contents::EMPTY.is_empty() && !Contents::EMPTY.is_opaque());
    assert!(c(Contents::PLAYERCLIP).is_clip() && c(Contents::MONSTERCLIP).is_clip());
    assert!(c(Contents::ORIGIN).is_origin());
    assert!(c(Contents::WATER).is_liquid());
    // ericw-tools tests the area portal bit on the visible contents, which never hold it, so an
    // area portal alone is not liquid there either.
    assert!(!c(Contents::AREAPORTAL).is_liquid());
    assert!(
        !c(Contents::SOLID | Contents::WATER).is_liquid(),
        "solid is stronger"
    );
    assert!(c(Contents::WATER | Contents::DETAIL).types_equal(c(Contents::WATER)));
    assert!(!c(Contents::WATER).types_equal(c(Contents::SLIME)));
    assert_eq!(
        c(Contents::WATER | Contents::TRANSLUCENT).content_type(),
        Contents::WATER
    );
}

// spec QBSP-15.9: the strongest visible contents are the lowest bit, then the clips' own indexes
#[test]
fn the_strongest_visible_contents() {
    assert_eq!(
        c(Contents::WATER | Contents::SKY).visible(),
        c(Contents::SKY)
    );
    assert_eq!(c(Contents::MIST).visible_index(), Some(9));
    assert_eq!(c(Contents::PLAYERCLIP).visible_index(), Some(10));
    assert_eq!(c(Contents::MONSTERCLIP).visible_index(), Some(11));
    assert_eq!(c(Contents::PROJECTILECLIP).visible_index(), Some(14));
    assert_eq!(c(Contents::ORIGIN).visible_index(), None);
    assert_eq!(c(Contents::DETAIL).visible(), Contents::EMPTY);
}

// spec QBSP-15.10: combining strips detail and translucency beside structural solid, sky or a
// visblocker; a cluster is solid only when both are
#[test]
fn contents_combine_and_cluster() {
    let water = c(Contents::WATER | Contents::DETAIL | Contents::TRANSLUCENT);
    assert_eq!(
        Contents::combine(water, c(Contents::SOLID)),
        c(Contents::WATER | Contents::SOLID)
    );
    assert_eq!(
        Contents::combine(water, c(Contents::SKY)),
        c(Contents::WATER | Contents::SKY)
    );
    assert_eq!(
        Contents::combine(water, c(Contents::ILLUSIONARY_VISBLOCKER)),
        c(Contents::WATER | Contents::ILLUSIONARY_VISBLOCKER)
    );
    assert_eq!(Contents::combine(water, Contents::EMPTY), water);
    let solid = c(Contents::SOLID);
    assert_eq!(solid.cluster(solid), solid);
    assert_eq!(solid.cluster(water), water);
}

// spec QBSP-15.11: a portal shows the strongest contents that change; a brush side gives a face
// for it on its back only when mirrored or not a window, aux or detail wall; solid cannot be seen
// through
#[test]
fn portals_show_what_changes() {
    let (solid, sky, water, empty) = (
        c(Contents::SOLID),
        c(Contents::SKY),
        c(Contents::WATER),
        Contents::EMPTY,
    );
    assert_eq!(Contents::portal_visible(solid, sky), empty);
    assert_eq!(Contents::portal_visible(water, empty), water);
    assert_eq!(Contents::portal_visible(water, water), empty);
    let suppressed = c(Contents::WATER | Contents::SUPPRESS_CLIPPING_SAME_TYPE);
    assert_eq!(Contents::portal_visible(suppressed, water), water);
    assert!(Contents::portal_generates_face(water, water, Side::Front));
    assert!(Contents::portal_generates_face(water, water, Side::Back));
    assert!(!Contents::portal_generates_face(water, solid, Side::Front));
    assert!(!Contents::portal_generates_face(empty, water, Side::Front));
    let window = c(Contents::WINDOW);
    assert!(!Contents::portal_generates_face(window, window, Side::Back));
    let mirrored = c(Contents::WINDOW | Contents::MIRROR_INSIDE_SET | Contents::MIRROR_INSIDE);
    assert!(Contents::portal_generates_face(
        window,
        mirrored,
        Side::Back
    ));
    let unmirrored = c(Contents::WATER | Contents::MIRROR_INSIDE_SET);
    assert!(!Contents::portal_generates_face(
        water,
        unmirrored,
        Side::Back
    ));
    assert!(water.is_two_sided() && !solid.is_two_sided());
    assert!(!unmirrored.is_two_sided() && mirrored.is_two_sided());
    assert!(!Contents::portal_can_see_through(solid, empty));
    assert!(Contents::portal_can_see_through(water, water));
    let detail_water = c(Contents::WATER | Contents::DETAIL);
    assert!(Contents::portal_can_see_through(detail_water, empty));
    assert!(!Contents::portal_can_see_through(water, empty));
    assert!(Contents::portal_can_see_through(
        detail_water,
        c(Contents::DETAIL)
    ));
}

// spec QBSP-15.12: Quake's native contents, written in order, the rest unwritable; the export
// remap; the hulls' sizes
#[test]
fn quake_writes_its_own_contents() {
    let quake = Game::Quake;
    for native in [
        NativeContents::Empty,
        NativeContents::Solid,
        NativeContents::Water,
        NativeContents::Slime,
        NativeContents::Lava,
        NativeContents::Sky,
    ] {
        assert_eq!(quake.to_native(quake.contents_of(native)), Ok(native));
    }
    assert_eq!(NativeContents::Sky.value(), -6);
    assert_eq!(NativeContents::Empty.value(), -1);
    assert_eq!(
        quake.to_native(c(Contents::WATER | Contents::LAVA)),
        Ok(NativeContents::Lava)
    );
    let wall = c(Contents::DETAIL_WALL);
    assert_eq!(quake.to_native(wall), Err(Unrepresentable(wall)));
    assert_eq!(quake.remap_for_export(wall), c(Contents::SOLID));
    assert_eq!(
        quake.remap_for_export(c(Contents::WINDOW)),
        c(Contents::SOLID)
    );
    assert_eq!(
        quake.remap_for_export(c(Contents::WATER | Contents::MIST)),
        c(Contents::WATER)
    );
    assert_eq!(
        quake.remap_for_export(c(Contents::ILLUSIONARY_VISBLOCKER)),
        Contents::EMPTY
    );
    let hulls = quake.hull_sizes();
    assert_eq!(hulls.len(), 3);
    assert_eq!(
        (hulls[1].mins, hulls[1].maxs),
        ([-16.0, -16.0, -32.0], [16.0, 16.0, 24.0])
    );
}

// spec QBSP-15.13: a face's contents from its texture's name, in any case
#[test]
fn texture_names_give_contents() {
    let quake = Game::Quake;
    let of = |name: &[u8]| quake.face_contents(name, TransWater::On);
    assert_eq!(of(b"ORIGIN"), c(Contents::ORIGIN));
    assert_eq!(of(b"hint"), Contents::EMPTY);
    assert_eq!(of(b"HintSkip"), Contents::EMPTY);
    assert_eq!(of(b"clip"), c(Contents::PLAYERCLIP));
    let liquid = Contents::DETAIL | Contents::TRANSLUCENT;
    assert_eq!(of(b"*LAVA1"), c(Contents::LAVA | liquid));
    assert_eq!(of(b"*slime0"), c(Contents::SLIME | liquid));
    assert_eq!(of(b"*04water1"), c(Contents::WATER | liquid));
    assert_eq!(
        quake.face_contents(b"*water", TransWater::Off),
        c(Contents::WATER)
    );
    assert_eq!(of(b"sky4"), c(Contents::SKY));
    assert_eq!(of(b"wall"), c(Contents::SOLID));
    assert_eq!(of(b"clipper"), c(Contents::SOLID));
}
