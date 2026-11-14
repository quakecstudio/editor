// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use qs_client::PoseBlend;

use super::{Md5Model, normal_dot, shade_vector};
use crate::alias::{AliasEntity, AliasError, AliasVertex, Shading};
use crate::mesh::tests::palette;
use crate::picture::tests::{files, solid};

/// A test MD5 model: two joints, `root` at the origin and `arm` 10 along x from it, and
/// one triangle whose corners weigh on them.
pub(crate) struct Md5Spec {
    /// The root joint's quaternion x, y and z in the mesh.
    pub(crate) root_orientation: [f32; 3],
    /// Each frame's components: the root's x, then the arm's x relative to the root; with
    /// the root's z quaternion when `rotate_root` is set. `None` for no animation file.
    pub(crate) frames: Option<Vec<Vec<f32>>>,
    /// Whether the animation's root carries its quaternion's z as a third component.
    pub(crate) rotate_root: bool,
    /// The animation's joints, 2 to go with the mesh.
    pub(crate) anim_joints: u32,
}

impl Default for Md5Spec {
    fn default() -> Self {
        Self {
            root_orientation: [0.0; 3],
            frames: Some(vec![vec![0.0, 10.0], vec![5.0, 10.0]]),
            rotate_root: false,
            anim_joints: 2,
        }
    }
}

/// The mesh file: vertex 0 on the root at (0, 0, 0), vertex 1 on it at (0, 10, 0), and
/// vertex 2 half on the arm at (0, 0, 10) and half on the root at its origin.
pub(crate) fn md5mesh(spec: &Md5Spec) -> Vec<u8> {
    let [x, y, z] = spec.root_orientation;
    format!(
        "MD5Version 10\ncommandline \"\"\nnumJoints 2\nnumMeshes 1\njoints {{\n\
         \"root\" -1 ( 0 0 0 ) ( {x} {y} {z} )\n\"arm\" 0 ( 10 0 0 ) ( 0 0 0 )\n}}\n\
         mesh {{\nshader \"ogre\"\nnumverts 3\nvert 0 ( 0 0 ) 0 1\nvert 1 ( 1 0 ) 1 1\n\
         vert 2 ( 0 1 ) 2 2\nnumtris 1\ntri 0 0 1 2\nnumweights 4\n\
         weight 0 0 1 ( 0 0 0 )\nweight 1 0 1 ( 0 10 0 )\nweight 2 1 0.5 ( 0 0 10 )\n\
         weight 3 0 0.5 ( 0 0 0 )\n}}\n"
    )
    .into_bytes()
}

/// The animation file of `spec`, `None` without frames.
pub(crate) fn md5anim(spec: &Md5Spec) -> Option<Vec<u8>> {
    let frames = spec.frames.as_ref()?;
    let (root_flags, arm_start, components) = if spec.rotate_root {
        (1 | 32, 2, 3)
    } else {
        (1, 1, 2)
    };
    let mut text = format!(
        "MD5Version 10\ncommandline \"\"\nnumFrames {}\nnumJoints {}\nframeRate 10\n\
         numAnimatedComponents {components}\nhierarchy {{\n\"root\" -1 {root_flags} 0\n",
        frames.len(),
        spec.anim_joints,
    );
    if spec.anim_joints > 1 {
        text.push_str(&format!("\"arm\" 0 1 {arm_start}\n"));
    }
    text.push_str("}\nbounds {\n}\nbaseframe {\n}\n");
    for (i, values) in frames.iter().enumerate() {
        let values: Vec<String> = values.iter().map(f32::to_string).collect();
        text.push_str(&format!("frame {i} {{ {} }}\n", values.join(" ")));
    }
    Some(text.into_bytes())
}

/// The model of `spec`, with `skins` as its other files.
fn model_with(spec: &Md5Spec, skins: Vec<(String, Vec<u8>)>) -> Result<Md5Model, AliasError> {
    let mut paths = skins;
    if let Some(anim) = md5anim(spec) {
        paths.push(("progs/ogre.md5anim".to_owned(), anim));
    }
    let read = files(paths);
    Md5Model::read(
        "progs/ogre.md5mesh",
        &md5mesh(spec),
        &read,
        &palette(),
        Shading::Lit,
    )
}

fn model(spec: &Md5Spec) -> Md5Model {
    model_with(spec, Vec::new()).unwrap()
}

/// The triangle drawn for `entity`, and the runs.
fn draw(model: &Md5Model, entity: &AliasEntity) -> (Vec<AliasVertex>, Vec<(usize, usize)>) {
    let mut out = Vec::new();
    let runs = model.draw(entity, 0.0, &mut out);
    (out, runs)
}

fn entity(frame: usize) -> AliasEntity {
    AliasEntity::new([0.0; 3], [0.0; 3], frame, 0, 100.0)
}

fn positions(vertices: &[AliasVertex]) -> Vec<[f32; 3]> {
    vertices.iter().map(|v| v.position).collect()
}

fn near(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
}

// spec REND-12.2: a vertex is the sum over its weights of the bias times the weight's
// position through its joint; each animation frame places a joint on its parent
#[test]
fn vertices_follow_their_weighted_joints() {
    let model = model(&Md5Spec::default());
    assert_eq!(model.frame_count(), 2);
    let (zero, runs) = draw(&model, &entity(0));
    assert_eq!(runs, [(3, 0)]);
    assert_eq!(
        positions(&zero),
        [[0.0, 0.0, 0.0], [0.0, 10.0, 0.0], [5.0, 0.0, 5.0]]
    );
    let (one, _) = draw(&model, &entity(1));
    assert_eq!(
        positions(&one),
        [[5.0, 0.0, 0.0], [5.0, 10.0, 0.0], [10.0, 0.0, 5.0]]
    );
    let (missing, _) = draw(&model, &entity(9));
    assert_eq!(
        positions(&missing),
        positions(&zero),
        "frame 0 for a frame out of range"
    );
    assert_eq!(
        zero.iter().map(|v| v.texture).collect::<Vec<_>>(),
        [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]
    );
}

// spec REND-12.2: a joint's orientation turns its children and its weights; a quaternion's
// w is negative (MD5-2.6), so (0, 0, sin 45) turns 90 degrees clockwise about z
#[test]
fn joints_turn_their_children() {
    let s = core::f32::consts::FRAC_1_SQRT_2;
    let turned = model(&Md5Spec {
        frames: Some(vec![vec![0.0, s, 10.0]]),
        rotate_root: true,
        ..Md5Spec::default()
    });
    let (vertices, _) = draw(&turned, &entity(0));
    // Vertex 1 (0, 10, 0) on the root turns to (10, 0, 0); vertex 2 is half of the arm's
    // (0, -10, 0) plus (0, 0, 10), and half the root's origin.
    assert!(
        near(vertices[1].position, [10.0, 0.0, 0.0]),
        "{:?}",
        vertices[1].position
    );
    assert!(
        near(vertices[2].position, [0.0, -5.0, 5.0]),
        "{:?}",
        vertices[2].position
    );
    let bind = model(&Md5Spec {
        root_orientation: [0.0, 0.0, s],
        frames: None,
        ..Md5Spec::default()
    });
    let (vertices, _) = draw(&bind, &entity(0));
    assert!(
        near(vertices[1].position, [10.0, 0.0, 0.0]),
        "the mesh's joints are absolute"
    );
    assert!(
        near(vertices[2].position, [5.0, 0.0, 5.0]),
        "the arm is not turned by its parent"
    );
}

// spec REND-12.2: without an animation file the model has one frame, the mesh's own pose
#[test]
fn a_mesh_without_an_animation_draws_its_own_pose() {
    let model = model(&Md5Spec {
        frames: None,
        ..Md5Spec::default()
    });
    assert_eq!(model.frame_count(), 1);
    let (vertices, _) = draw(&model, &entity(3));
    assert_eq!(
        positions(&vertices),
        [[0.0, 0.0, 0.0], [0.0, 10.0, 0.0], [5.0, 0.0, 5.0]]
    );
}

// spec REND-12.3: two frames blend by the lerp fraction, positions and shades alike
#[test]
fn frames_blend_by_the_fraction() {
    let model = model(&Md5Spec::default());
    let mut halfway = entity(0);
    halfway.blend = Some(PoseBlend {
        previous: 0,
        current: 1,
        blend: 0.5,
    });
    let (vertices, _) = draw(&model, &halfway);
    assert_eq!(vertices[0].position, [2.5, 0.0, 0.0]);
    assert_eq!(vertices[2].position, [7.5, 0.0, 5.0]);
}

// spec REND-12.5: the light falls along QuakeSpasm's `shadevector`, the yaw in 16 steps,
// and a vertex's shade is `1 + dot`, or `1 + dot * 13/44` facing away, times shadelight
#[test]
fn vertices_are_shaded_by_their_normals() {
    let s = core::f32::consts::FRAC_1_SQRT_2;
    assert!(near(shade_vector(0.0), [s, 0.0, s]));
    assert!(near(shade_vector(22.4), [s, 0.0, s]), "below one step");
    assert!(
        near(shade_vector(-1.0), [s, 0.0, s]),
        "(int) truncates -0.04 to step 0"
    );
    // -30 degrees is -1.33 steps, (int) -1, & 15 step 15: 337.5 degrees.
    let [x, y, z] = shade_vector(-30.0);
    assert!(
        near([x, y, z], [0.653_281_5, 0.270_598_05, s]),
        "{x} {y} {z}"
    );
    assert_eq!(normal_dot([0.0, 0.0, 1.0], [0.0, 0.0, 1.0]), 2.0);
    assert!((normal_dot([0.0, 0.0, -1.0], [0.0, 0.0, 1.0]) - 31.0 / 44.0).abs() < 1e-6);
    // The triangle's normal points out of its clockwise side, (-1, 0, 1) normalised: across
    // yaw 0's light, shade 1, and straight into yaw 180's, 2; times light 100's
    // shadelight, capped at 192 - 100 over 200 (REND-4.7).
    let model = model(&Md5Spec::default());
    let (lit, _) = draw(&model, &entity(0));
    assert!((lit[0].shade - 0.46).abs() < 1e-4, "{}", lit[0].shade);
    let (away, _) = draw(
        &model,
        &AliasEntity::new([0.0; 3], [0.0, 180.0, 0.0], 0, 0, 100.0),
    );
    assert!((away[0].shade - 0.92).abs() < 1e-4, "{}", away[0].shade);
}

// spec REND-12.4: skin `n` frame `f` is `progs/<shader>_<nn>_<ff>`, frames until one is
// missing, skins until one has no frame 0; fewer than four frames repeat the first two
#[test]
fn skins_come_from_the_shader_name() {
    let skin = |name: &str, shade: u8| {
        (
            format!("progs/{name}.tga"),
            solid([1, 1], [shade, 0, 0, 255]),
        )
    };
    let model = model_with(
        &Md5Spec::default(),
        vec![
            skin("ogre_00_00", 1),
            skin("ogre_00_01", 2),
            skin("ogre_00_02", 3),
            skin("ogre_01_00", 4),
            skin("ogre_03_00", 5),
        ],
    )
    .unwrap();
    let red: Vec<u8> = model.images().iter().map(|image| image.rgba[0]).collect();
    assert_eq!(
        red,
        [1, 2, 3, 4],
        "skin 2 is missing, so skin 3 is not looked for"
    );
    let image_at = |skin: usize, time: f64| {
        let mut out = Vec::new();
        let entity = AliasEntity::new([0.0; 3], [0.0; 3], 0, skin, 100.0);
        model.draw(&entity, time, &mut out)[0].1
    };
    let slots: Vec<usize> = [0.0, 0.1, 0.2, 0.3]
        .iter()
        .map(|&t| image_at(0, t))
        .collect();
    assert_eq!(slots, [0, 1, 0, 1], "three frames are drawn as two");
    assert_eq!(image_at(1, 0.3), 3);
    assert_eq!(
        image_at(7, 0.0),
        0,
        "skin 0 for a skin the model does not have"
    );
    let bare = model_with(&Md5Spec::default(), Vec::new()).unwrap();
    assert_eq!(bare.images().len(), 1);
    assert_eq!(bare.images()[0].rgba, [255; 4], "no skin found is white");
}

// spec REND-12.4: four frames fill the four slots
#[test]
fn four_skin_frames_fill_the_slots() {
    let skins = (0..4)
        .map(|f| {
            (
                format!("progs/ogre_00_0{f}.tga"),
                solid([1, 1], [f, 0, 0, 255]),
            )
        })
        .collect();
    let model = model_with(&Md5Spec::default(), skins).unwrap();
    let slots: Vec<usize> = [0.0, 0.1, 0.2, 0.3]
        .iter()
        .map(|&t| model.draw(&entity(0), t, &mut Vec::new())[0].1)
        .collect();
    assert_eq!(slots, [0, 1, 2, 3]);
}

// spec REND-12.1: a mesh or an animation that does not read, or an animation that does not
// go with the mesh, is an error naming the file
#[test]
fn a_bad_mesh_or_animation_is_refused() {
    let read = files(Vec::new());
    let error = Md5Model::read("progs/x.md5mesh", b"IDPO", &read, &palette(), Shading::Lit);
    assert!(matches!(error, Err(AliasError::Md5(ref m)) if m.starts_with("progs/x.md5mesh: ")));
    let mismatched = model_with(
        &Md5Spec {
            anim_joints: 1,
            frames: Some(vec![vec![0.0]]),
            ..Md5Spec::default()
        },
        Vec::new(),
    );
    assert!(
        matches!(mismatched, Err(AliasError::Md5(_))),
        "{mismatched:?}"
    );
    let broken = files(vec![(
        "progs/ogre.md5anim".to_owned(),
        b"MD5Version 9".to_vec(),
    )]);
    let error = Md5Model::read(
        "progs/ogre.md5mesh",
        &md5mesh(&Md5Spec::default()),
        &broken,
        &palette(),
        Shading::Lit,
    );
    assert!(matches!(error, Err(AliasError::Md5(_))));
}

// spec REND-12.2: an animation asking for more frames than its file has bytes is refused,
// as every frame is made when the model is read
#[test]
fn an_animation_with_too_many_frames_is_refused() {
    let anim = md5anim(&Md5Spec::default())
        .map(|text| {
            String::from_utf8(text)
                .unwrap()
                .replace("numFrames 2", "numFrames 4000000000")
        })
        .unwrap();
    let read = files(vec![("progs/ogre.md5anim".to_owned(), anim.into_bytes())]);
    let error = Md5Model::read(
        "progs/ogre.md5mesh",
        &md5mesh(&Md5Spec::default()),
        &read,
        &palette(),
        Shading::Lit,
    );
    assert!(
        matches!(error, Err(AliasError::Md5(ref m)) if m == "progs/ogre.md5anim has too many frames"),
        "{error:?}"
    );
}
