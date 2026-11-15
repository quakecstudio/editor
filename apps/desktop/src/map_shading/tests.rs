// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::indexing_slicing,
    clippy::absolute_paths,
    reason = "test code: a missing index is a failed test; the values compared are exact"
)]

use qs_layout::MapShading;

use super::{LitState, ShadedView, shade_of};
use crate::strings::Text;

// No spec statement: P7-13: floors and ceilings are drawn brightest, walls facing x darker and
// walls facing y darker again, so neighbouring faces read apart without a light
#[test]
fn faces_are_shaded_by_the_way_they_face() {
    let up = shade_of([0.0, 0.0, 1.0]).get();
    let down = shade_of([0.0, 0.0, -1.0]).get();
    let x = shade_of([1.0, 0.0, 0.0]).get();
    let y = shade_of([0.0, -1.0, 0.0]).get();
    assert_eq!((up, down), (255, 255));
    assert!(up > x && x > y, "{up} {x} {y}");
    assert_eq!(y, 209);
    assert_eq!(x, 232);
}

// No spec statement: P7-13: Lit says when there is no compile yet or the map changed since,
// and nothing in the other shadings; no faces are drawn before a mesh is made
#[test]
fn lit_says_when_its_compile_is_missing_or_old() {
    let mut view = ShadedView::default();
    assert_eq!(view.shading, MapShading::Lines);
    view.lit = Some(LitState::NoCompile);
    assert_eq!(view.note(), None);
    view.shading = MapShading::Lit;
    assert_eq!(view.note(), Some(Text::MapLitNone));
    view.lit = Some(LitState::Stale);
    assert_eq!(view.note(), Some(Text::MapLitStale));
    view.lit = Some(LitState::Current);
    assert_eq!(view.note(), None);
    assert!(!view.draws_faces());
    view.shading = MapShading::Textured;
    assert!(!view.draws_faces());
}

/// A brush of entity `entity` with one face: a 64-unit square on the floor at height `z`.
fn floor_brush(entity: usize, z: f64) -> crate::map_scene::BrushShape {
    use qs_map_edit::uv::FaceIndex;
    use qs_map_edit::{BrushIndex, EntityIndex};
    crate::map_scene::BrushShape {
        entity: EntityIndex::new(entity),
        brush: BrushIndex::new(0),
        lines: Vec::new(),
        vertices: Vec::new(),
        planes: Vec::new(),
        textures: vec!["floor".to_owned()],
        faces: vec![crate::map_scene::FaceShape {
            index: FaceIndex::new(0),
            plane: ([0.0, 0.0, 1.0], 0.0),
            polygon: vec![
                [0.0, 0.0, z],
                [64.0, 0.0, z],
                [64.0, 64.0, z],
                [0.0, 64.0, z],
            ],
            texture: "floor".to_owned(),
            texvecs: Some([[1.0, 0.0, 0.0, 0.0], [0.0, -1.0, 0.0, 0.0]]),
        }],
    }
}

// No spec statement: P7-13: the textured mesh leaves out the brushes of hidden entities, and
// asks for each texture once
#[test]
fn hidden_entities_are_left_out_of_the_textured_mesh() {
    use std::collections::BTreeSet;

    use qs_map_edit::EntityIndex;
    use qs_render::Look;

    let scene = crate::map_scene::Scene {
        brushes: vec![floor_brush(0, 0.0), floor_brush(1, 8.0)],
        ..crate::map_scene::Scene::default()
    };
    let mut asked = Vec::new();
    let mut image = |name: &str| {
        asked.push(name.to_owned());
        super::missing()
    };
    let all = scene.textured_mesh(&mut image, &BTreeSet::new(), Look::QuakeSpasm);
    assert_eq!(all.indices.len(), 12);
    assert_eq!(asked, ["floor"]);
    let hidden = BTreeSet::from([EntityIndex::new(1)]);
    let shown = scene.textured_mesh(&mut |_| super::missing(), &hidden, Look::QuakeSpasm);
    assert_eq!(shown.indices.len(), 6);
    assert!(shown.vertices.iter().all(|v| v.position[2] == 0.0));
    // A face is wound clockwise seen from where it faces, as the renderer draws it: from above,
    // its corners' turn points down.
    let [a, b, c] = [0, 1, 2].map(|i| shown.vertices[shown.indices[i] as usize].position);
    let turn = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    assert!(turn < 0.0, "{turn}");
}

// No spec statement: P7-13: a missing texture is Quake's black and pink checker, 64 texels a
// side in blocks of 32
#[test]
fn a_missing_texture_is_the_checker() {
    let image = super::missing();
    assert_eq!((image.width, image.height), (64, 64));
    let texel = |x: usize, y: usize| image.rgba[(y * 64 + x) * 4..][..4].to_vec();
    assert_eq!(texel(0, 0), [159, 91, 83, 255]);
    assert_eq!(texel(40, 0), [0, 0, 0, 255]);
    assert_eq!(texel(0, 40), [0, 0, 0, 255]);
    assert_eq!(texel(63, 63), [159, 91, 83, 255]);
}

// No spec statement: P7-13: the lit view draws the world and each brush entity's model at its
// origin, and leaves out triggers and models the BSP does not have
#[test]
fn the_lit_view_places_brush_models_and_leaves_out_triggers() {
    let text = b"{\n\"classname\" \"worldspawn\"\n}\n\
        {\n\"classname\" \"trigger_multiple\"\n\"model\" \"*1\"\n}\n\
        {\n\"classname\" \"func_door\"\n\"model\" \"*2\"\n\"origin\" \"8 0 0\"\n}\n\
        {\n\"classname\" \"func_wall\"\n\"model\" \"*9\"\n}\n";
    let mut bsp = qs_bsp::Bsp::empty(text);
    let world = bsp.models[0];
    bsp.models = vec![world; 3];
    let placements = super::placements(&bsp);
    assert_eq!(placements.len(), 2, "{placements:?}");
    assert_eq!(placements[0], qs_render::Placement::WORLD);
    assert_eq!(placements[1].model, 2);
    assert_eq!(placements[1].origin, [8.0, 0.0, 0.0]);
}

// spec QBSP-15.16: the textured view leaves out the faces the game never draws, by the name
// after the last `/` in any case, as the lit view does, and keeps their brushes' edges
#[test]
fn the_textured_view_leaves_out_unseen_textures() {
    use std::collections::BTreeSet;

    use qs_map_edit::EntityIndex;
    use qs_render::Look;

    let textured = |name: &str| {
        let mut face = floor_brush(0, 0.0).faces[0].clone();
        face.texture = name.to_owned();
        face
    };
    for unseen in [
        "CLIP",
        "skip",
        "Hint",
        "hintskip",
        "origin",
        "*waterskip",
        "null",
        "bevel",
        "tools/clip",
        "base/SKIP",
    ] {
        assert!(textured(unseen).unseen(), "{unseen}");
    }
    for seen in ["wall", "clipper", "*water0", "sky4", "trigger", "clip/wall"] {
        assert!(!textured(seen).unseen(), "{seen}");
    }
    let mut clip = floor_brush(0, 0.0);
    clip.faces[0].texture = "clip".to_owned();
    let scene = crate::map_scene::Scene {
        brushes: vec![clip, floor_brush(1, 8.0), floor_brush(2, 16.0)],
        ..crate::map_scene::Scene::default()
    };
    let mesh = scene.textured_mesh(
        &mut |_| super::missing(),
        &BTreeSet::from([EntityIndex::new(2)]),
        Look::QuakeSpasm,
    );
    assert_eq!(mesh.indices.len(), 6);
    // The clip brush and the trigger keep their edges over the faces.
    assert_eq!(
        scene.outlined(&BTreeSet::from([EntityIndex::new(2)])),
        [0, 2]
    );
    assert_eq!(scene.outlined(&BTreeSet::new()), [0]);
}
