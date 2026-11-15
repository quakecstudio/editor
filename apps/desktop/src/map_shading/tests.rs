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
    let mut scene = scene;
    for brush in &mut scene.brushes {
        brush.lines = vec![[[0.0; 3], [1.0, 0.0, 0.0]]];
    }
    let outlined = [0, 2];
    let drawn = |hidden: &BTreeSet<EntityIndex>| scene.outlined_lines(&outlined, hidden).count();
    assert_eq!(drawn(&BTreeSet::new()), 2);
    assert_eq!(
        drawn(&BTreeSet::from([EntityIndex::new(2)])),
        1,
        "a hidden one is not drawn"
    );
}

/// A model of one triangle, (0, 0, 0), (2, 4, 6) and (6, 6, 6) as its scale of 2 places them,
/// with one 4 by 4 skin of colour 1.
fn triangle_mdl() -> Vec<u8> {
    let mut out = b"IDPO".to_vec();
    let int = |out: &mut Vec<u8>, v: i32| out.extend(v.to_le_bytes());
    let float = |out: &mut Vec<u8>, v: f32| out.extend(v.to_le_bytes());
    int(&mut out, 6);
    // scale, scale_origin, radius, eye
    for v in [2.0, 2.0, 2.0, 0.0, 0.0, 0.0, 8.0, 0.0, 0.0, 0.0] {
        float(&mut out, v);
    }
    // skins, skin width and height, vertices, triangles, frames, sync type, flags
    for v in [1, 4, 4, 3, 1, 1, 0, 0] {
        int(&mut out, v);
    }
    float(&mut out, 1.0);
    int(&mut out, 0);
    out.extend([1_u8; 16]);
    for (s, t) in [(0, 0), (1, 2), (3, 3)] {
        for v in [0, s, t] {
            int(&mut out, v);
        }
    }
    for v in [1, 0, 1, 2] {
        int(&mut out, v);
    }
    int(&mut out, 0);
    out.extend([0, 0, 0, 0, 255, 255, 255, 0]);
    out.extend([b'f'; 16]);
    for vertex in [[0_u8, 0, 0], [1, 2, 3], [3, 3, 3]] {
        out.extend(vertex);
        out.push(0);
    }
    out
}

// No spec statement: P7-08: the textured and lit views draw each point entity's class model with
// its skin, as TrenchBroom does: its first frame at the entity's origin, turned by its angle;
// a model read once for every entity showing it; nothing for a hidden entity, a brush entity,
// a model whose file is missing or does not read, or with no palette
#[test]
fn point_entities_show_their_models_skinned() {
    use std::collections::{BTreeSet, HashMap};
    use std::sync::Arc;

    use qs_map_edit::EntityIndex;
    use qs_render::Degrees;
    use qs_wad::texture::Palette;

    let shape =
        |index: usize, model: &str, origin: [f32; 3], yaw: f32| crate::map_scene::EntityShape {
            index: EntityIndex::new(index),
            bounds: [origin, origin],
            origin,
            yaw: Degrees::new(yaw),
            color: None,
            model: Some(model.to_owned()),
            brush: false,
            classname: None,
            targetname: None,
            targets: Vec::new(),
        };
    let door = crate::map_scene::EntityShape {
        brush: true,
        ..shape(5, "progs/a.mdl", [0.0; 3], 0.0)
    };
    let scene = crate::map_scene::Scene {
        entities: vec![
            shape(1, "progs/a.mdl", [100.0, 0.0, 0.0], 0.0),
            shape(2, "progs/a.mdl", [0.0, 0.0, 0.0], 90.0),
            shape(3, "progs/a.mdl", [0.0; 3], 0.0),
            shape(4, "progs/gone.mdl", [0.0; 3], 0.0),
            shape(6, "progs/bad.mdl", [0.0; 3], 0.0),
            door,
        ],
        ..crate::map_scene::Scene::default()
    };
    let files: HashMap<String, Arc<[u8]>> = HashMap::from([
        ("progs/a.mdl".to_owned(), Arc::from(triangle_mdl())),
        ("progs/bad.mdl".to_owned(), Arc::from(b"IDPO".to_vec())),
    ]);
    let palette = Palette::from_lmp(&[64; 768]).ok();
    let hidden = BTreeSet::from([EntityIndex::new(3)]);
    let posed = scene.posed_models(&files, palette.as_ref(), &hidden);
    assert_eq!(posed.skins.len(), 1, "the model's one skin, read once");
    assert_eq!(posed.frame.draws.len(), 2, "the two entities shown");
    assert_eq!(posed.frame.vertices.len(), 6);
    assert!(
        posed
            .frame
            .draws
            .iter()
            .all(|draw| draw.skin == 0 && draw.count == 3)
    );
    assert!(
        posed
            .frame
            .vertices
            .iter()
            .all(|v| v.shade > 0.5 && v.shade <= 1.0),
        "lit as the faces are, not as dimly as the game's caps"
    );
    let positions: Vec<[f32; 3]> = posed.frame.vertices.iter().map(|v| v.position).collect();
    assert!(
        positions.contains(&[100.0, 0.0, 0.0]),
        "at its origin: {positions:?}"
    );
    assert!(positions.contains(&[102.0, 4.0, 6.0]), "{positions:?}");
    let turned = positions
        .iter()
        .skip(3)
        .any(|p| (p[0] + 4.0).abs() < 1e-3 && (p[1] - 2.0).abs() < 1e-3);
    assert!(turned, "the second turned a quarter: {positions:?}");
    assert!(
        scene
            .posed_models(&files, None, &hidden)
            .frame
            .draws
            .is_empty(),
        "no palette, no skins"
    );
}
