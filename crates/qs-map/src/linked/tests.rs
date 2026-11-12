// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use crate::{ContainerId, EntityPlace, LayerSetting, LayerSwitch, LinkError, MapFile, Transform};

/// The manual's two linked groups (Linked Groups, "In the map file, these groups would be
/// stored as follows"), each with a brush and an army: group 1 moved by 128 along x, group 2
/// the source, whose army's `angle` is protected; then a layer that is left out of exports with
/// an entity in it.
const MAP: &str = "{\n\"classname\" \"worldspawn\"\n}\n\
{\n\"classname\" \"func_group\"\n\"_tb_type\" \"_tb_group\"\n\"_tb_name\" \"group\"\n\"_tb_id\" \"1\"\n\
\"_tb_linked_group_id\" \"{38b3b39d-a165-4999-985d-d40563ce51c1}\"\n\
\"_tb_transformation\" \"1 0 0 128 0 1 0 0 0 0 1 0 0 0 0 1\"\n\
{\n( 128 0 0 ) ( 128 1 0 ) ( 129 0 0 ) floor 0 0 0 1 1\n}\n}\n\
{\n\"classname\" \"monster_army\"\n\"origin\" \"128 0 0\"\n\"_tb_group\" \"1\"\n}\n\
{\n\"classname\" \"func_group\"\n\"_tb_type\" \"_tb_group\"\n\"_tb_name\" \"group\"\n\"_tb_id\" \"2\"\n\
\"_tb_linked_group_id\" \"{38b3b39d-a165-4999-985d-d40563ce51c1}\"\n\
\"_tb_transformation\" \"1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1\"\n\
{\n( 0 0 0 ) ( 0 1 0 ) ( 1 0 0 ) floor 0 0 0 1 1\n}\n}\n\
{\n\"classname\" \"monster_army\"\n\"origin\" \"0 0 0\"\n\"angle\" \"90\"\n\"_tb_group\" \"2\"\n\
\"_tb_protected_properties\" \"angle\"\n}\n\
{\n\"classname\" \"func_group\"\n\"_tb_type\" \"_tb_layer\"\n\"_tb_name\" \"Lights\"\n\"_tb_id\" \"3\"\n\
\"_tb_layer_omit_from_export\" \"1\"\n\"_tb_layer_hidden\" \"1\"\n}\n\
{\n\"classname\" \"light\"\n\"origin\" \"0 0 64\"\n\"_tb_layer\" \"3\"\n}\n";

fn map() -> MapFile {
    MapFile::parse(MAP.as_bytes()).unwrap()
}

/// The value of `key` on the entity at `place`, as text.
fn value(map: &MapFile, place: usize, key: &str) -> Option<String> {
    map.entities[place]
        .value(key.as_bytes())
        .map(|v| String::from_utf8_lossy(v).into_owned())
}

// spec MAP-9.5: groups with the same link id are linked; the transformation is read row by
// row, its translation the fourth, eighth and twelfth numbers, and written back the same way
#[test]
fn linked_groups_and_their_transformations_are_read() {
    let map = map();
    let groups = map.linked_groups();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].entity, EntityPlace::new(1));
    assert_eq!(groups[0].id, ContainerId::new(1));
    assert_eq!(groups[0].link, groups[1].link);
    let moved = groups[0].transform.unwrap();
    assert_eq!(moved, Transform::translation([128.0, 0.0, 0.0]));
    assert_eq!(groups[1].transform, Some(Transform::IDENTITY));
    assert_eq!(moved.apply([1.0, 2.0, 3.0]), [129.0, 2.0, 3.0]);
    assert_eq!(moved.text(), "1 0 0 128 0 1 0 0 0 0 1 0 0 0 0 1");
    assert_eq!(Transform::parse(b"1 0 0"), None);
    assert_eq!(Transform::parse(b"1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 x"), None);
    // A group with no link id is no linked group, whatever its transformation.
    let unlinked = MAP.replace(
        "\"_tb_linked_group_id\" \"{38b3b39d-a165-4999-985d-d40563ce51c1}\"\n",
        "",
    );
    assert!(
        MapFile::parse(unlinked.as_bytes())
            .unwrap()
            .linked_groups()
            .is_empty()
    );
    // One that does not read is the identity.
    let malformed = MAP.replace("1 0 0 128 0 1 0 0 0 0 1 0 0 0 0 1", "1 0 0 128");
    let malformed = MapFile::parse(malformed.as_bytes()).unwrap();
    assert_eq!(malformed.linked_groups()[0].transform, None);
    assert_eq!(Transform::parse(b"1 0 0 inf 0 1 0 0 0 0 1 0 0 0 0 1"), None);
    assert_eq!(Transform::parse(b"1 0 0 NaN 0 1 0 0 0 0 1 0 0 0 0 1"), None);
}

// spec MAP-9.7: the inverse undoes a transformation; a singular one has none
#[test]
fn a_transformation_is_inverted_and_composed() {
    let quarter = Transform::parse(b"0 -1 0 16 1 0 0 0 0 0 1 8 0 0 0 1").unwrap();
    let inverse = quarter.inverse().unwrap();
    assert_eq!(quarter.after(&inverse), Transform::IDENTITY);
    assert_eq!(
        inverse.apply(quarter.apply([3.0, 5.0, 7.0])),
        [3.0, 5.0, 7.0]
    );
    assert_eq!(quarter.facing(0.0), 90.0);
    assert!(quarter.turns() && !quarter.mirrors());
    assert!(!Transform::translation([8.0, 0.0, 0.0]).turns());
    let flat = Transform::parse(b"1 0 0 0 0 1 0 0 0 0 0 0 0 0 0 1").unwrap();
    assert_eq!(flat.inverse(), None);
}

// spec MAP-9.6: a group's contents are its own brushes, then the entities naming it, in order;
// the linked group of a member is its group's
#[test]
fn a_groups_members_are_the_entities_naming_it() {
    let map = map();
    assert_eq!(
        map.group_members(ContainerId::new(1)),
        [EntityPlace::new(2)]
    );
    assert_eq!(
        map.group_members(ContainerId::new(2)),
        [EntityPlace::new(4)]
    );
    assert_eq!(
        map.linked_group_of(EntityPlace::new(4)).unwrap().id,
        ContainerId::new(2)
    );
    assert_eq!(
        map.linked_group_of(EntityPlace::new(3)).unwrap().id,
        ContainerId::new(2)
    );
    assert_eq!(
        map.linked_group_of(EntityPlace::new(6)),
        None,
        "in a layer, not a group"
    );
}

// spec MAP-9.7: a change to the source is copied into the linked group, moved by its
// transformation times the source's inverse: the brush's points and the army's origin
// spec MAP-9.8: the protected angle keeps the value of the entity it replaces
#[test]
fn a_change_is_copied_into_the_linked_group() {
    let mut map = map();
    map.entities[4].set(b"origin", b"16 8 0").unwrap();
    map.entities[4].set(b"wait", b"2").unwrap();
    map.entities[4].set(b"angle", b"180").unwrap();
    map.update_linked(EntityPlace::new(3)).unwrap();
    assert_eq!(value(&map, 2, "origin").as_deref(), Some("144 8 0"));
    assert_eq!(value(&map, 2, "wait").as_deref(), Some("2"));
    assert_eq!(value(&map, 2, "_tb_group").as_deref(), Some("1"));
    assert_eq!(
        value(&map, 2, "angle"),
        None,
        "protected in the source, and the replaced army had none"
    );
    assert_eq!(value(&map, 2, "_tb_protected_properties"), None);
    let brush = map.entities[1].brushes().next().unwrap();
    assert_eq!(brush.faces[0].points()[0], [128.0, 0.0, 0.0]);
    // The source is as it was.
    assert_eq!(value(&map, 4, "origin").as_deref(), Some("16 8 0"));
    assert_eq!(value(&map, 4, "angle").as_deref(), Some("180"));
    // From the other side, the copy keeps the source's protected angle.
    map.entities[2].set(b"wait", b"5").unwrap();
    map.update_linked(EntityPlace::new(1)).unwrap();
    assert_eq!(value(&map, 4, "wait").as_deref(), Some("5"));
    assert_eq!(value(&map, 4, "origin").as_deref(), Some("16 8 0"));
    assert_eq!(value(&map, 4, "angle").as_deref(), Some("180"));
    assert_eq!(
        value(&map, 4, "_tb_protected_properties").as_deref(),
        Some("angle")
    );
}

// spec MAP-9.7: a turned group turns the copy's angle and points; members added to the source
// are added to the copy
#[test]
fn a_turned_copy_turns_and_takes_new_members() {
    let turned = MAP.replace(
        "1 0 0 128 0 1 0 0 0 0 1 0 0 0 0 1",
        "0 -1 0 0 1 0 0 0 0 0 1 0 0 0 0 1",
    );
    let mut map = MapFile::parse(turned.as_bytes()).unwrap();
    map.entities[4].remove(b"_tb_protected_properties");
    let mut extra = map.entities[4].clone();
    extra.set(b"origin", b"32 0 0").unwrap();
    map.entities.insert(5, extra);
    let moves = map.update_linked(EntityPlace::new(3)).unwrap();
    // The copied army replaces the old one in place; the new copy pushes the rest on by one.
    let to = |place: usize| moves.of(EntityPlace::new(place)).map(EntityPlace::get);
    assert_eq!(
        [to(1), to(2), to(3), to(4), to(5), to(7)],
        [1, 2, 4, 5, 6, 8].map(Some)
    );
    assert_eq!(to(99), None);
    assert_eq!(
        map.group_members(ContainerId::new(1)),
        [EntityPlace::new(2), EntityPlace::new(3)]
    );
    assert_eq!(value(&map, 2, "angle").as_deref(), Some("180"));
    assert_eq!(value(&map, 3, "origin").as_deref(), Some("0 32 0"));
    let brush = map.entities[1].brushes().next().unwrap();
    assert_eq!(brush.faces[0].points()[2], [0.0, 1.0, 0.0]);
}

// spec MAP-9.7: no update from an entity that is no linked group, from a group whose
// transformation has no inverse, or of a set holding a group; the map is left as it was
#[test]
fn an_update_that_cannot_be_made_changes_nothing() {
    let mut map = map();
    assert!(matches!(
        map.update_linked(EntityPlace::new(2)),
        Err(LinkError::NotLinked(place)) if place == EntityPlace::new(2)
    ));
    let singular = MAP.replace(
        "1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1",
        "0 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1",
    );
    let mut singular = MapFile::parse(singular.as_bytes()).unwrap();
    let before = singular.to_bytes();
    assert!(matches!(
        singular.update_linked(EntityPlace::new(3)),
        Err(LinkError::NotInvertible(place)) if place == EntityPlace::new(3)
    ));
    assert_eq!(singular.to_bytes(), before);
    map.entities[2].set(b"classname", b"func_group").unwrap();
    map.entities[2].set(b"_tb_type", b"_tb_group").unwrap();
    map.entities[2].set(b"_tb_id", b"9").unwrap();
    let before = map.to_bytes();
    assert!(matches!(
        map.update_linked(EntityPlace::new(3)),
        Err(LinkError::Nested(place)) if place == EntityPlace::new(1)
    ));
    assert_eq!(map.to_bytes(), before);
}

// spec MAP-9.9: a layer's settings are set by "1" and cleared by their absence; an export
// leaves out a layer marked so, and the entities in it
#[test]
fn layers_are_hidden_locked_and_left_out_of_exports() {
    let mut map = map();
    let settings = map.entities[5].layer_settings();
    assert!(settings.hidden && settings.omit_from_export && !settings.locked);
    map.entities[5]
        .set_layer_setting(LayerSetting::Locked, LayerSwitch::On)
        .unwrap();
    map.entities[5]
        .set_layer_setting(LayerSetting::Hidden, LayerSwitch::Off)
        .unwrap();
    assert_eq!(value(&map, 5, "_tb_layer_locked").as_deref(), Some("1"));
    assert_eq!(value(&map, 5, "_tb_layer_hidden"), None);
    let exported = map.without_omitted_layers();
    assert_eq!(exported.entities.len(), 5);
    assert!(
        exported
            .entities
            .iter()
            .all(|e| e.value(b"_tb_layer").is_none())
    );
    map.entities[5]
        .set_layer_setting(LayerSetting::OmitFromExport, LayerSwitch::Off)
        .unwrap();
    assert_eq!(map.without_omitted_layers().entities.len(), 7);
}

// spec MAP-9.5: the manual's map reads and writes back unchanged
#[test]
fn a_trenchbroom_map_with_linked_groups_and_layers_round_trips() {
    assert_eq!(map().to_bytes(), MAP.as_bytes());
}

// spec MAP-9.6: the group changed is the one whose brushes or entities differ; a change outside
// every linked group, or to a group's own keys, changes none
#[test]
fn the_changed_group_is_found() {
    let before = map();
    let mut after = map();
    assert_eq!(after.changed_linked_groups(&before), []);
    after.entities[6].set(b"light", b"300").unwrap();
    after.entities[3].set(b"_tb_name", b"renamed").unwrap();
    assert_eq!(after.changed_linked_groups(&before), []);
    after.entities[4].set(b"wait", b"1").unwrap();
    assert_eq!(after.changed_linked_groups(&before), [EntityPlace::new(3)]);
    let mut after = map();
    after.entities[1].items.pop();
    assert_eq!(after.changed_linked_groups(&before), [EntityPlace::new(1)]);
}

// spec MAP-9.5: a transformation is written only when it is not the identity
#[test]
fn a_transformation_is_written_unless_it_is_the_identity() {
    let mut map = map();
    let moved = Transform::translation([0.0, 16.0, 0.0]);
    map.entities[3].set_transform(&moved).unwrap();
    assert_eq!(map.entities[3].transform(), Some(moved));
    assert_eq!(
        value(&map, 3, "_tb_transformation").as_deref(),
        Some("1 0 0 0 0 1 0 16 0 0 1 0 0 0 0 1")
    );
    map.entities[3].set_transform(&Transform::IDENTITY).unwrap();
    assert_eq!(value(&map, 3, "_tb_transformation"), None);
    assert_eq!(map.entities[6].layer(), Some(ContainerId::new(3)));
    assert_eq!(map.entities[2].layer(), None);
}

// spec MAP-9.7: a copy keeps an angle of straight up or down; a turned angle stays below 360;
// -0 is written 0
#[test]
fn up_and_down_stay_and_turned_angles_wrap() {
    let quarter = Transform::parse(b"0 -1 0 0 1 0 0 0 0 0 1 0 0 0 0 1").unwrap();
    for (angle, turned) in [("-1", "-1"), ("-2", "-2"), ("350", "80"), ("0", "90")] {
        let text = format!(
            "{{\n\"classname\" \"worldspawn\"\n}}\n{{\n\"origin\" \"0 0 0\"\n\"angle\" \"{angle}\"\n}}\n"
        );
        let map = MapFile::parse(text.as_bytes()).unwrap();
        let copy = map.entities[1].transformed(&quarter).unwrap();
        assert_eq!(copy.value(b"angle"), Some(turned.as_bytes()), "{angle}");
    }
    // Every term of x is -0 here, so only the rounding's + 0.0 writes it as 0.
    let mirror = Transform::parse(b"-1 0 0 -0 0 1 0 0 0 0 1 0 0 0 0 1").unwrap();
    assert!(mirror.apply([0.0, -0.0, -0.0])[0].is_sign_positive());
}

// spec MAP-9.7: a scaled and sheared transformation that needs its rows swapped inverts
#[test]
fn a_general_transformation_inverts() {
    for text in [
        b"2 1 0 3 0 4 0 5 0 0 0.5 6 0 0 0 1".as_slice(),
        b"0 1 0 0 2 0 0 8 0 0 1 0 0 0 0 1".as_slice(),
    ] {
        let m = Transform::parse(text).unwrap();
        let inverse = m.inverse().unwrap();
        for point in [[1.0, 2.0, 3.0], [-4.0, 0.5, 9.0]] {
            assert_eq!(inverse.apply(m.apply(point)), point);
        }
    }
}

// spec MAP-9.7: only the groups of the changed group's set are updated
#[test]
fn a_group_of_another_set_is_left_alone() {
    let other = MAP.replacen("{38b3b39d-a165-4999-985d-d40563ce51c1}", "{other}", 1);
    let mut map = MapFile::parse(other.as_bytes()).unwrap();
    map.entities[4].set(b"wait", b"2").unwrap();
    let before = map.entities[2].clone();
    let moves = map.update_linked(EntityPlace::new(3)).unwrap();
    assert_eq!(map.entities[2], before);
    assert_eq!(moves.of(EntityPlace::new(2)), Some(EntityPlace::new(2)));
}

// spec MAP-9.1: a group whose id is 0 is no room; the ids and places read as they were written
#[test]
fn ids_and_places_read_as_written() {
    let zero = MAP.replace("\"_tb_id\" \"3\"", "\"_tb_id\" \"0\"");
    let names: Vec<String> = MapFile::parse(zero.as_bytes())
        .unwrap()
        .rooms()
        .list()
        .iter()
        .map(|room| room.name.clone())
        .collect();
    assert!(!names.iter().any(|name| name == "Lights"), "{names:?}");
    assert_eq!(ContainerId::new(2).get(), 2);
    assert_eq!(EntityPlace::new(5).to_string(), "5");
}

// No spec statement: P5-14: the errors say what failed and give the map's error as their
// source
#[test]
fn link_errors_say_what_failed() {
    use std::error::Error as _;
    assert_eq!(
        LinkError::NotLinked(EntityPlace::new(2)).to_string(),
        "entity 2 is no linked group"
    );
    assert_eq!(
        LinkError::NotInvertible(EntityPlace::new(3)).to_string(),
        "group 3's transformation is not invertible"
    );
    assert!(
        LinkError::Nested(EntityPlace::new(1))
            .to_string()
            .contains("holds a group")
    );
    assert!(LinkError::Nested(EntityPlace::new(1)).source().is_none());
    let mut entity = crate::Entity::new();
    let map_error = entity.set(b"bad\"key", b"v").unwrap_err();
    let error = LinkError::from(map_error);
    assert!(error.source().is_some());
    assert!(!error.to_string().is_empty());
}

// spec MAP-9.7: when the copies hold fewer entities than before, the entities after them move
// back, and a replaced member with no copy is gone
#[test]
fn the_moves_follow_a_shrinking_map() {
    let mut map = map();
    // The source loses its army: the copy loses its own, and everything after moves back one.
    map.entities.remove(4);
    let moves = map.update_linked(EntityPlace::new(3)).unwrap();
    assert_eq!(map.entities.len(), 5);
    let to = |place: usize| moves.of(EntityPlace::new(place)).map(EntityPlace::get);
    assert_eq!(to(2), None, "the copy's army is gone");
    assert_eq!(
        [to(1), to(3), to(4), to(5)],
        [Some(1), Some(2), Some(3), Some(4)]
    );
    assert_eq!(value(&map, 4, "classname").as_deref(), Some("light"));
}

// spec MAP-9.5: a group whose transformation does not read is never updated, and a change in it
// is not copied
#[test]
fn a_group_that_does_not_read_is_never_updated() {
    let malformed = MAP.replace("1 0 0 128 0 1 0 0 0 0 1 0 0 0 0 1", "1 0 0 128");
    let mut map = MapFile::parse(malformed.as_bytes()).unwrap();
    map.entities[4].set(b"wait", b"2").unwrap();
    let before = map.entities[2].clone();
    map.update_linked(EntityPlace::new(3)).unwrap();
    assert_eq!(map.entities[2], before);
    assert!(matches!(
        map.update_linked(EntityPlace::new(1)),
        Err(LinkError::Malformed(place)) if place == EntityPlace::new(1)
    ));
}

// spec MAP-9.7: a mirrored copy reverses each face's points, so the brush stays outside in, and
// mirrors its angle
#[test]
fn a_mirrored_copy_keeps_its_brushes_outside_in() {
    let mirrored = MAP.replace(
        "1 0 0 128 0 1 0 0 0 0 1 0 0 0 0 1",
        "-1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1",
    );
    let mut map = MapFile::parse(mirrored.as_bytes()).unwrap();
    map.entities[4].remove(b"_tb_protected_properties");
    map.entities[4].set(b"angle", b"30").unwrap();
    map.update_linked(EntityPlace::new(3)).unwrap();
    assert_eq!(value(&map, 2, "angle").as_deref(), Some("150"));
    let face = &map.entities[1].brushes().next().unwrap().faces[0];
    // ( 0 0 0 ) ( 0 1 0 ) ( 1 0 0 ) mirrored in x, its last two points swapped.
    assert_eq!(
        *face.points(),
        [[0.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
    );
}

// spec MAP-9.9: an entity in a group in a layer is in the layer, and an export leaves it out
// with the layer; a layer's own entity is in itself
#[test]
fn a_group_in_a_layer_takes_its_members_with_it() {
    let mut map = map();
    map.entities[3].set(b"_tb_layer", b"3").unwrap();
    let layers = map.layer_of_each();
    let three = Some(ContainerId::new(3));
    assert_eq!(layers, [None, None, None, three, three, three, three]);
    let exported = map.without_omitted_layers();
    assert_eq!(exported.entities.len(), 3);
    assert!(
        exported
            .linked_groups()
            .iter()
            .all(|g| g.id != ContainerId::new(2))
    );
}

// spec MAP-9.7: every changed set is copied, each from its group that changed, and where every
// entity went is followed across the updates; two groups of one set changed differently are
// refused, the map left as it was
#[test]
fn every_changed_set_is_copied_once() {
    let two_sets = format!(
        "{MAP}{{\n\"classname\" \"func_group\"\n\"_tb_type\" \"_tb_group\"\n\"_tb_name\" \"b\"\n\
         \"_tb_id\" \"4\"\n\"_tb_linked_group_id\" \"b\"\n}}\n\
         {{\n\"classname\" \"info_null\"\n\"origin\" \"0 0 0\"\n\"_tb_group\" \"4\"\n}}\n\
         {{\n\"classname\" \"func_group\"\n\"_tb_type\" \"_tb_group\"\n\"_tb_name\" \"b\"\n\
         \"_tb_id\" \"5\"\n\"_tb_linked_group_id\" \"b\"\n}}\n\
         {{\n\"classname\" \"info_null\"\n\"origin\" \"0 0 0\"\n\"_tb_group\" \"5\"\n}}\n"
    );
    let before = MapFile::parse(two_sets.as_bytes()).unwrap();
    let mut map = before.clone();
    map.entities[4].set(b"wait", b"2").unwrap();
    map.entities[8].set(b"wait", b"3").unwrap();
    let moves = map.update_changed(&before).unwrap();
    assert_eq!(value(&map, 2, "wait").as_deref(), Some("2"));
    assert_eq!(value(&map, 10, "wait").as_deref(), Some("3"));
    assert_eq!(moves.of(EntityPlace::new(10)), Some(EntityPlace::new(10)));
    // Both groups of the first set changed, differently.
    let mut map = before.clone();
    map.entities[4].set(b"wait", b"2").unwrap();
    map.entities[2].set(b"wait", b"5").unwrap();
    let edited = map.to_bytes();
    assert!(matches!(
        map.update_changed(&before),
        Err(LinkError::BothChanged(_))
    ));
    assert_eq!(map.to_bytes(), edited);
    // Nothing changed in a linked group: nothing moves.
    let mut map = before.clone();
    map.entities[6].set(b"light", b"1").unwrap();
    let moves = map.update_changed(&before).unwrap();
    assert_eq!(moves.of(EntityPlace::new(6)), Some(EntityPlace::new(6)));
}

// spec MAP-9.7: the determinant tells a mirror from a turn: below 0 mirrors, 0 and above do not
#[test]
fn the_determinant_tells_a_mirror() {
    for (text, determinant) in [
        (b"1 2 3 0 0 1 4 0 5 6 0 0 0 0 0 1".as_slice(), 1.0),
        (b"1 2 3 0 3 2 1 0 2 1 3 0 0 0 0 1".as_slice(), -12.0),
        (b"2 0 1 0 1 3 2 0 1 1 1 0 0 0 0 1".as_slice(), 0.0),
        (b"4 -2 1 9 3 5 -7 9 1 0 2 9 0 0 0 1".as_slice(), 61.0),
    ] {
        let m = Transform::parse(text).unwrap();
        assert_eq!(m.determinant(), determinant);
        assert_eq!(m.mirrors(), determinant < 0.0);
    }
    let quarter = Transform::parse(b"0 -1 0 0 1 0 0 0 0 0 1 0 0 0 0 1").unwrap();
    assert_eq!(quarter.facing(30.0), 120.0);
    // Pointing back along x, turned a quarter: -90 degrees, written 270.
    assert_eq!(quarter.facing(180.0), 270.0);
}

// spec MAP-9.6: the copies take the place of the entities they replace, even with another
// entity between the group and them, and follow the group's own entity when it held none
#[test]
fn copies_take_the_replaced_entities_place() {
    let mut map = map();
    // Group 1's army moves past the layer's light; group 2's army is the source.
    let army = map.entities.remove(2);
    map.entities.push(army);
    map.update_linked(EntityPlace::new(2)).unwrap();
    assert_eq!(
        value(&map, 6, "_tb_group").as_deref(),
        Some("1"),
        "{:?}",
        map.entities.len()
    );
    assert_eq!(value(&map, 5, "classname").as_deref(), Some("light"));
    let mut empty = self::map();
    // Group 1 holds nothing: the copy follows its own entity.
    empty.entities.remove(2);
    empty.update_linked(EntityPlace::new(2)).unwrap();
    assert_eq!(value(&empty, 2, "_tb_group").as_deref(), Some("1"));
    assert_eq!(value(&empty, 3, "_tb_id").as_deref(), Some("2"));
}

// spec MAP-9.2: an entity is in the layer its `_tb_layer` names before the group its
// `_tb_group` names, each only when the map has it; groups that name each other are in no
// container, so TrenchBroom's default layer holds them
#[test]
fn each_entity_has_one_parent_layer_first() {
    let mut map = map();
    map.entities[2].set(b"_tb_layer", b"3").unwrap();
    map.entities[4].set(b"_tb_layer", b"99").unwrap();
    let parents = map.parents();
    let at = |place: usize| parents[place].map(EntityPlace::get);
    assert_eq!(at(0), None);
    assert_eq!(at(1), None);
    assert_eq!(at(2), Some(5), "the layer before the group");
    assert_eq!(at(4), Some(3), "a layer the map has not is passed over");
    assert_eq!(at(5), None, "a layer's own entity");
    assert_eq!(at(6), Some(5));
    // Group 1 in group 2 and group 2 in group 1: a loop, and a member of group 1 under it.
    map.entities[1].set(b"_tb_group", b"2").unwrap();
    map.entities[3].set(b"_tb_group", b"1").unwrap();
    map.entities[2].remove(b"_tb_layer");
    let parents = map.parents();
    assert_eq!([parents[1], parents[3]], [None, None]);
    assert_eq!(parents[2], Some(EntityPlace::new(1)));
    // A group naming itself.
    map.entities[1].set(b"_tb_group", b"1").unwrap();
    assert_eq!(map.parents()[1], None);
}

// spec MAP-9.2: putting an entity in a layer or group writes the one key TrenchBroom reads
// spec MAP-9.5: a group is linked only when its link id is not blank
#[test]
fn an_entity_is_put_in_a_container() {
    let mut map = map();
    let entity = &mut map.entities[2];
    entity
        .put_in(Some((crate::RoomKind::Layer, ContainerId::new(3))))
        .unwrap();
    assert_eq!(entity.value(b"_tb_layer"), Some(b"3".as_slice()));
    assert_eq!(entity.value(b"_tb_group"), None);
    entity
        .put_in(Some((crate::RoomKind::Group, ContainerId::new(2))))
        .unwrap();
    assert_eq!(entity.value(b"_tb_layer"), None);
    assert_eq!(entity.value(b"_tb_group"), Some(b"2".as_slice()));
    entity.put_in(None).unwrap();
    assert_eq!(
        (entity.value(b"_tb_layer"), entity.value(b"_tb_group")),
        (None, None)
    );
    assert!(entity.mentions("monster") && entity.mentions("army") && !entity.mentions("ogre"));
    assert_eq!(entity.text(b"classname").as_deref(), Some("monster_army"));
    assert!(map.entities[1].is_linked_group());
    map.entities[1].set(b"_tb_linked_group_id", b"").unwrap();
    assert!(!map.entities[1].is_linked_group());
    assert!(!map.entities[5].is_linked_group(), "a layer");
}
