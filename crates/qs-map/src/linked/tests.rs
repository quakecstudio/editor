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
    assert_eq!(
        groups[0].transform,
        Transform::translation([128.0, 0.0, 0.0])
    );
    assert_eq!(groups[1].transform, Transform::IDENTITY);
    assert_eq!(
        groups[0].transform.apply([1.0, 2.0, 3.0]),
        [129.0, 2.0, 3.0]
    );
    assert_eq!(
        groups[0].transform.text(),
        "1 0 0 128 0 1 0 0 0 0 1 0 0 0 0 1"
    );
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
    assert_eq!(malformed.linked_groups()[0].transform, Transform::IDENTITY);
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
    assert!((quarter.yaw() - 90.0).abs() < 1e-9);
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
    map.update_linked(EntityPlace::new(3)).unwrap();
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
