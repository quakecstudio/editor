// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use qs_mdl::mdl::{MDL_EF_ROTATE, MDL_MAGIC, MDL_VERSION};

use qs_client::{DynamicLight, PoseBlend};

use super::{
    AliasEntity, AliasKind, AliasModel, AliasVertex, Shading, bob_rotate, entity_light,
    flood_fill_skin, rotate_for_entity, shade_dots,
};
use crate::mesh::Palette;

/// A skin entry of a test model: one skin, or a group of them.
pub(crate) enum SkinSpec {
    Single(Vec<u8>),
    Group(Vec<Vec<u8>>),
}

/// A frame entry of a test model: one pose, or a group of poses with their intervals.
pub(crate) enum FrameSpec {
    Single([u8; 3]),
    Group(Vec<[u8; 3]>, Vec<f32>),
}

/// A one-triangle model on a 4 by 4 skin; each pose moves all three vertices to one place.
pub(crate) struct MdlSpec {
    pub(crate) skins: Vec<SkinSpec>,
    pub(crate) frames: Vec<FrameSpec>,
    /// Each vertex's `onseam`.
    pub(crate) onseam: i32,
    pub(crate) facesfront: i32,
    pub(crate) flags: i32,
    /// The normal index of every vertex.
    pub(crate) normal: u8,
    /// The normal index of every vertex of each frame, where set, else `normal`.
    pub(crate) frame_normals: Vec<u8>,
}

impl Default for MdlSpec {
    fn default() -> Self {
        Self {
            skins: vec![SkinSpec::Single(vec![1; 16])],
            frames: vec![FrameSpec::Single([1, 2, 3])],
            onseam: 0,
            facesfront: 1,
            flags: 0,
            normal: 0,
            frame_normals: Vec::new(),
        }
    }
}

const SKIN: i32 = 4;

pub(crate) fn mdl(spec: &MdlSpec) -> Vec<u8> {
    let mut out = Vec::new();
    let int = |out: &mut Vec<u8>, v: i32| out.extend(v.to_le_bytes());
    let float = |out: &mut Vec<u8>, v: f32| out.extend(v.to_le_bytes());
    out.extend(MDL_MAGIC);
    int(&mut out, MDL_VERSION);
    // scale, scale_origin, radius, eye
    for v in [2.0, 2.0, 2.0, 10.0, 20.0, 30.0, 1.0, 0.0, 0.0, 0.0] {
        float(&mut out, v);
    }
    for v in [
        i32::try_from(spec.skins.len()).unwrap(),
        SKIN,
        SKIN,
        3,
        1,
        i32::try_from(spec.frames.len()).unwrap(),
        0,
        spec.flags,
    ] {
        int(&mut out, v);
    }
    float(&mut out, 1.0);
    for skin in &spec.skins {
        match skin {
            SkinSpec::Single(pixels) => {
                int(&mut out, 0);
                out.extend(pixels);
            }
            SkinSpec::Group(skins) => {
                int(&mut out, 1);
                int(&mut out, i32::try_from(skins.len()).unwrap());
                for _ in skins {
                    float(&mut out, 0.1);
                }
                for pixels in skins {
                    out.extend(pixels);
                }
            }
        }
    }
    for (s, t) in [(0, 0), (1, 2), (3, 3)] {
        int(&mut out, spec.onseam);
        int(&mut out, s);
        int(&mut out, t);
    }
    int(&mut out, spec.facesfront);
    for v in [0, 1, 2] {
        int(&mut out, v);
    }
    let pose = |out: &mut Vec<u8>, v: [u8; 3], normal: u8| {
        out.extend([0, 0, 0, 0, 255, 255, 255, 0]);
        out.extend([b'f'; 16]);
        for _ in 0..3 {
            out.extend(v);
            out.push(normal);
        }
    };
    for (i, frame) in spec.frames.iter().enumerate() {
        let normal = spec.frame_normals.get(i).copied().unwrap_or(spec.normal);
        match frame {
            FrameSpec::Single(v) => {
                int(&mut out, 0);
                pose(&mut out, *v, normal);
            }
            FrameSpec::Group(poses, intervals) => {
                int(&mut out, 1);
                int(&mut out, i32::try_from(poses.len()).unwrap());
                out.extend([0, 0, 0, 0, 255, 255, 255, 0]);
                for interval in intervals {
                    float(&mut out, *interval);
                }
                for v in poses {
                    pose(&mut out, *v, normal);
                }
            }
        }
    }
    out
}

/// A palette whose colour `i` is `(i, 1, 0)`, so a pixel's red is its index.
fn palette() -> Palette {
    let bytes: Vec<u8> = (0..=255_u8).flat_map(|i| [i, 1, 0]).collect();
    Palette::read(&bytes).unwrap()
}

fn read(spec: &MdlSpec, name: &str) -> AliasModel {
    AliasModel::read(name, &mdl(spec), &palette()).unwrap()
}

fn entity() -> AliasEntity {
    AliasEntity::new([0.0; 3], [0.0; 3], 0, 0, 100.0)
}

fn draw(model: &AliasModel, entity: &AliasEntity, time: f64) -> (Vec<AliasVertex>, usize) {
    let mut out = Vec::new();
    let image = model.draw(entity, time, &mut out);
    (out, image)
}

/// The red of each pixel of image `i`: its palette index.
fn indices(model: &AliasModel, i: usize) -> Vec<u8> {
    model.images()[i].rgba.chunks(4).map(|p| p[0]).collect()
}

// spec REND-4.1: the background joined to pixel (0, 0) takes the colour at its edge
#[test]
fn the_background_is_filled_with_its_edge_colour() {
    #[rustfmt::skip]
    let mut skin = vec![
        5, 5, 5, 5,
        5, 9, 9, 5,
        5, 9, 9, 5,
        5, 5, 5, 7,
    ];
    flood_fill_skin(&mut skin, (4, 4), 0);
    // The ring visits from (0, 0); each pixel takes the last other colour beside it, else 0.
    assert!(skin.iter().all(|&p| p != 5 && p != 255), "{skin:?}");
    assert_eq!(skin[5..7], [9, 9]);
    assert_eq!(skin[15], 7);
    assert_eq!(skin[1], 9);
    assert_eq!(skin[0], 0);
}

/// A flood fill case: the skin, its size and the skin after.
type Case = (Vec<u8>, (usize, usize), Vec<u8>);

// spec REND-4.1: each pixel taken from the ring takes the last neighbour colour, left, right,
// up then down, that is neither the background nor 255; values from a line-for-line model of
// id's C
#[test]
fn flood_fill_matches_ids_order() {
    #[rustfmt::skip]
    let cases: [Case; 5] = [
        (
            vec![5, 5, 5, 5, 5, 9, 5, 7, 5, 5, 5, 5],
            (4, 3),
            vec![0, 9, 9, 7, 0, 9, 9, 7, 0, 9, 9, 7],
        ),
        (vec![5; 8], (4, 2), vec![0; 8]),
        (
            vec![5, 5, 5, 5, 5, 5, 5, 5, 3],
            (3, 3),
            vec![0, 0, 0, 0, 0, 3, 0, 0, 3],
        ),
        // The right edge's step stays in its row.
        (vec![5, 5, 5, 5, 7, 5], (3, 2), vec![0, 7, 7, 0, 7, 7]),
        // A queued pixel, marked 255, is never taken as a neighbour colour.
        (
            vec![
                5, 9, 5, 5, 5, 5, 5,
                5, 5, 5, 7, 5, 5, 5,
                5, 5, 5, 5, 5, 5, 7,
                5, 5, 5, 5, 5, 9, 5,
                5, 5, 5, 5, 5, 7, 5,
                5, 5, 5, 7, 5, 9, 5,
            ],
            (7, 6),
            vec![
                9, 9, 7, 7, 7, 7, 7,
                9, 9, 7, 7, 7, 9, 7,
                9, 9, 7, 7, 7, 9, 7,
                9, 9, 7, 7, 7, 9, 5,
                9, 9, 7, 7, 7, 7, 5,
                9, 9, 7, 7, 7, 9, 5,
            ],
        ),
    ];
    for (mut skin, size, expected) in cases {
        flood_fill_skin(&mut skin, size, 0);
        assert_eq!(skin, expected, "{size:?}");
    }
}

// spec REND-4.1: nothing is filled when the first pixel is the filled colour or 255
#[test]
fn a_skin_starting_with_the_filled_colour_or_255_is_kept() {
    for first in [0, 255] {
        let mut skin = vec![first; 16];
        skin[5] = 9;
        let before = skin.clone();
        flood_fill_skin(&mut skin, (4, 4), 0);
        assert_eq!(skin, before);
    }
}

// spec REND-4.1: the filled colour is 255 only when entry 255 is pure red, else 0
#[test]
fn the_filled_colour_is_0_unless_entry_255_is_pure_red() {
    assert_eq!(palette().filled_colour(), 0);
    let mut bytes = vec![0_u8; 768];
    bytes[765] = 255;
    assert_eq!(Palette::read(&bytes).unwrap().filled_colour(), 255);
}

// spec REND-4.1: only the first skin's pixels are filled, before every upload
#[test]
fn only_the_first_skin_is_filled() {
    let mut second = vec![5; 16];
    second[5] = 9;
    let mut first = vec![5; 16];
    first[5] = 9;
    let model = read(
        &MdlSpec {
            skins: vec![SkinSpec::Single(first), SkinSpec::Single(second.clone())],
            ..MdlSpec::default()
        },
        "progs/x.mdl",
    );
    assert!(!indices(&model, 0).contains(&5));
    assert_eq!(indices(&model, 1), second);
}

// spec REND-4.2: a group's frames go in slots j&3, the rest repeat them, and the slot drawn
// is (int)(time*10) & 3; a skin the model lacks draws skin 0
#[test]
fn skin_groups_fill_four_slots() {
    let frames = (0..3).map(|i| vec![10 + i; 16]).collect();
    let model = read(
        &MdlSpec {
            skins: vec![SkinSpec::Group(frames)],
            ..MdlSpec::default()
        },
        "progs/x.mdl",
    );
    let slot = |time: f64, skin: usize| draw(&model, &AliasEntity { skin, ..entity() }, time).1;
    assert_eq!(
        [slot(0.0, 0), slot(0.15, 0), slot(0.25, 0), slot(0.35, 0)],
        [0, 1, 2, 0]
    );
    assert_eq!(slot(0.45, 0), 0);
    assert_eq!(slot(0.15, 7), 1);
}

// spec REND-4.3: (s + 0.5) / skinwidth, with s moved by half the skin for a seam vertex of a
// back-facing triangle
#[test]
fn skin_coordinates_are_texel_centres_and_seams_move_back() {
    let texture = |m: &AliasModel| -> Vec<[f32; 2]> {
        draw(m, &entity(), 0.0)
            .0
            .iter()
            .map(|v| v.texture)
            .collect()
    };
    let front = read(&MdlSpec::default(), "progs/x.mdl");
    assert_eq!(
        texture(&front),
        [[0.125, 0.125], [0.375, 0.625], [0.875, 0.875]]
    );
    let back = read(
        &MdlSpec {
            onseam: 0x20,
            facesfront: 0,
            ..MdlSpec::default()
        },
        "progs/x.mdl",
    );
    assert_eq!(texture(&back)[0], [0.625, 0.125]);
    let seam_front = read(
        &MdlSpec {
            onseam: 0x20,
            ..MdlSpec::default()
        },
        "progs/x.mdl",
    );
    assert_eq!(texture(&seam_front)[0], [0.125, 0.125]);
}

// spec REND-4.4: a group's pose is (int)(time / its first interval) % its poses; a frame the
// model lacks draws frame 0; a NaN interval holds the first pose
#[test]
fn frame_groups_step_by_their_first_interval() {
    let model = read(
        &MdlSpec {
            frames: vec![
                FrameSpec::Single([0, 0, 0]),
                FrameSpec::Group(vec![[1, 0, 0], [2, 0, 0], [3, 0, 0]], vec![0.5, 9.0, 9.0]),
                FrameSpec::Group(vec![[4, 0, 0], [5, 0, 0]], vec![f32::NAN, f32::NAN]),
            ],
            ..MdlSpec::default()
        },
        "progs/x.mdl",
    );
    let x = |frame: usize, time: f64| {
        draw(&model, &AliasEntity { frame, ..entity() }, time).0[0].position[0]
    };
    // x = v * 2 + 10.
    assert_eq!(
        [x(1, 0.0), x(1, 0.6), x(1, 1.1), x(1, 1.6)],
        [12.0, 14.0, 16.0, 12.0]
    );
    assert_eq!(x(0, 5.0), 10.0);
    assert_eq!(x(9, 0.0), 10.0);
    assert_eq!(x(2, 3.0), 18.0);
}

// spec REND-4.5: a vertex is v * scale + scale_origin, turned by -angles[0] about y and by the
// yaw about z, then moved to the origin
#[test]
fn vertices_are_scaled_turned_and_moved() {
    let model = read(&MdlSpec::default(), "progs/x.mdl");
    let at = |angles: [f32; 3]| {
        let e = AliasEntity {
            origin: [100.0, 0.0, 0.0],
            angles,
            ..entity()
        };
        draw(&model, &e, 0.0).0[0].position.map(f32::round)
    };
    assert_eq!(at([0.0; 3]), [112.0, 24.0, 36.0]);
    assert_eq!(at([0.0, 90.0, 0.0]), [76.0, 12.0, 36.0]);
    // Pitch 90 turns -90 about y, which takes x to +z.
    // (12, 24, 36) turned -90 about y is (-36, 24, 12).
    assert_eq!(at([90.0, 0.0, 0.0]), [64.0, 24.0, 12.0]);
    assert_eq!(
        rotate_for_entity([0.0; 3], [-90.0, 0.0, 0.0], [1.0, 0.0, 0.0]).map(f32::round),
        [0.0, 0.0, 1.0]
    );
    // Roll 90 about x takes z to -y and y to z.
    let rolled = |p| rotate_for_entity([0.0; 3], [0.0, 0.0, 90.0], p).map(f32::round);
    assert_eq!(rolled([0.0, 0.0, 1.0]), [0.0, -1.0, 0.0]);
    assert_eq!(rolled([0.0, 2.0, 1.0]), [0.0, -1.0, 2.0]);
    // Pitch 90 about y takes z to x.
    let pitched = rotate_for_entity([0.0; 3], [90.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    assert_eq!(pitched.map(f32::round), [1.0, 0.0, 0.0]);
}

// spec REND-4.7: ambient is capped at 128, ambient plus shade at 192, torches are 256, and
// the shade is over 200
#[test]
fn shade_light_caps_as_r_draw_alias_model() {
    assert_eq!(Shading::Lit.light(50.0), 0.25);
    assert_eq!(Shading::Lit.light(100.0), 0.46);
    assert_eq!(Shading::Lit.light(300.0), 0.32);
    assert_eq!(Shading::Lit.light(96.0), 0.48);
    assert_eq!(Shading::Lit.light(97.0), 0.475);
    assert_eq!(Shading::Torch.light(0.0), 1.28);
    let torch = read(&MdlSpec::default(), "progs/flame2.mdl");
    let plain = read(&MdlSpec::default(), "progs/flame3.mdl");
    let shade = |m: &AliasModel| {
        draw(
            m,
            &AliasEntity {
                light: 0.0,
                ..entity()
            },
            0.0,
        )
        .0[0]
            .shade
    };
    assert_eq!(shade(&torch), 1.0);
    assert_eq!(shade(&plain), 0.0);
}

// spec REND-4.8: the row is (int)(yaw * 16/360) & 15, the shade the entry times shadelight,
// clamped to 0 to 1
#[test]
fn shade_dots_follow_the_yaw_and_clamp() {
    assert_eq!(shade_dots(0.0)[0], 1.23);
    assert_eq!(shade_dots(22.5)[0], 1.26);
    assert_eq!(shade_dots(-22.5)[0], shade_dots(337.5)[0]);
    assert_eq!(shade_dots(-10.0)[0], 1.23);
    assert_eq!(shade_dots(45.0)[0], 1.34);
    assert_eq!(shade_dots(360.0 + 45.0)[0], 1.34);
    let model = read(&MdlSpec::default(), "progs/x.mdl");
    let shade = draw(
        &model,
        &AliasEntity {
            light: 50.0,
            ..entity()
        },
        0.0,
    )
    .0[0]
        .shade;
    assert_eq!(shade, 1.23 * 0.25);
    let bright = read(&MdlSpec::default(), "progs/flame.mdl");
    assert_eq!(draw(&bright, &entity(), 0.0).0[0].shade, 1.0);
}

// spec REND-4.12: two poses blend by the lerp fraction, positions and shade dots alike;
// equal poses draw the first
#[test]
fn poses_blend_by_the_fraction() {
    let model = read(
        &MdlSpec {
            frames: vec![FrameSpec::Single([1, 2, 3]), FrameSpec::Single([3, 6, 9])],
            frame_normals: vec![0, 1],
            ..MdlSpec::default()
        },
        "progs/x.mdl",
    );
    let blended = |previous, current, blend| {
        let e = AliasEntity {
            light: 50.0,
            blend: Some(PoseBlend {
                previous,
                current,
                blend,
            }),
            ..entity()
        };
        let vertex = draw(&model, &e, 0.0).0[0];
        (vertex.position, vertex.shade)
    };
    // Scale 2 and origin (10, 20, 30): (1, 2, 3) is (12, 24, 36) and (3, 6, 9) is (16, 32, 48).
    assert_eq!(blended(0, 1, 0.0), ([12.0, 24.0, 36.0], 1.23 * 0.25));
    assert_eq!(blended(0, 1, 1.0), ([16.0, 32.0, 48.0], 1.30 * 0.25));
    let (position, shade) = blended(0, 1, 0.5);
    assert_eq!(position, [14.0, 28.0, 42.0]);
    assert!(
        (shade - (1.23 * 0.5 + 1.30 * 0.5) * 0.25).abs() < 1e-6,
        "{shade}"
    );
    assert_eq!(blended(1, 1, 0.5), ([16.0, 32.0, 48.0], 1.30 * 0.25));
    // A pose the model does not have draws the other.
    assert_eq!(blended(0, 9, 0.5), ([12.0, 24.0, 36.0], 1.23 * 0.25));
    // No blend: the frame at the time, as GLQuake draws it (REND-4.4).
    let e = AliasEntity {
        frame: 1,
        ..entity()
    };
    assert_eq!(draw(&model, &e, 0.0).0[0].position, [16.0, 32.0, 48.0]);
}

// spec REND-4.11: the view model's floor of 24, then each live light's radius less its
// distance when positive, in float, then a player's floor of 8
#[test]
fn entity_light_adds_the_floors_and_the_lights() {
    let light = |origin, radius| DynamicLight {
        origin,
        radius,
        minlight: 0.0,
    };
    assert_eq!(entity_light(10, AliasKind::ViewModel, [0.0; 3], &[]), 24.0);
    assert_eq!(entity_light(30, AliasKind::ViewModel, [0.0; 3], &[]), 30.0);
    assert_eq!(entity_light(0, AliasKind::Player, [0.0; 3], &[]), 8.0);
    assert_eq!(entity_light(0, AliasKind::Other, [0.0; 3], &[]), 0.0);
    // 100 away from a light of radius 150.5: 50.5 added, kept as a float.
    let lights = [
        light([100.0, 0.0, 0.0], 150.5),
        light([0.0, 300.0, 0.0], 100.0),
    ];
    assert_eq!(entity_light(10, AliasKind::Other, [0.0; 3], &lights), 60.5);
    // The floors come in order: the view model's before the lights, the player's after.
    assert_eq!(
        entity_light(10, AliasKind::ViewModel, [0.0; 3], &lights),
        74.5
    );
    assert_eq!(entity_light(0, AliasKind::Player, [0.0; 3], &lights), 50.5);
    assert_eq!(
        entity_light(-100, AliasKind::Player, [0.0; 3], &lights),
        8.0
    );
}

// spec REND-4.9: an EF_ROTATE model's yaw is anglemod(100*time), in steps of 360/65536
#[test]
fn rotating_models_turn_100_degrees_a_second() {
    assert_eq!(bob_rotate(0.0), 0.0);
    let expected = 360.0 / 65536.0 * 18204.0;
    assert!((f64::from(bob_rotate(1.0)) - expected).abs() < 1e-4);
    assert!(f64::from(bob_rotate(3.6)) < 0.01);
    let rotating = MdlSpec {
        flags: MDL_EF_ROTATE,
        ..MdlSpec::default()
    };
    assert!(read(&rotating, "x").rotates());
    assert!(!read(&MdlSpec::default(), "x").rotates());
}

#[test]
fn a_file_that_is_not_a_model_is_refused() {
    let error = AliasModel::read("progs/x.mdl", b"IDPO", &palette()).unwrap_err();
    assert!(error.to_string().contains("progs/x.mdl"), "{error}");
}

/// A 4 by 4 skin of the palette indices the shirt and trouser ranges start, end and border.
const PLAYER_SKIN: [u8; 16] = [0, 16, 20, 31, 96, 100, 111, 112, 15, 32, 95, 50, 0, 0, 0, 0];

// spec REND-4.13: the shirt range from 16 and the trouser range from 96 move to the chosen
// rows, a row from 128 on reversed; every other index is kept
#[test]
fn player_colours_move_the_shirt_and_trouser_ranges() {
    let model = read(
        &MdlSpec {
            skins: vec![SkinSpec::Single(PLAYER_SKIN.to_vec())],
            ..MdlSpec::default()
        },
        "progs/player.mdl",
    );
    let red = |colors| -> Vec<u8> {
        let image = model.translated(0, colors, &palette()).unwrap();
        assert_eq!((image.width, image.height), (4, 4));
        image.rgba.chunks(4).map(|p| p[0]).collect()
    };
    assert_eq!(
        red(0x4d),
        [
            0, 64, 68, 79, 223, 219, 208, 112, 15, 32, 95, 50, 0, 0, 0, 0
        ]
    );
    assert_eq!(
        red(0xa2),
        [
            0, 175, 171, 160, 32, 36, 47, 112, 15, 32, 95, 50, 0, 0, 0, 0
        ]
    );
    assert_eq!(
        red(0x1),
        [0, 0, 4, 15, 16, 20, 31, 112, 15, 32, 95, 50, 0, 0, 0, 0]
    );
}

// spec REND-4.13: the translated skin is the entity's skin, or skin 0 for one the model does
// not have
#[test]
fn player_colours_translate_the_entitys_skin() {
    let model = read(
        &MdlSpec {
            skins: vec![
                SkinSpec::Single([0].into_iter().chain([16; 15]).collect()),
                SkinSpec::Group(vec![vec![17; 16], vec![18; 16]]),
            ],
            ..MdlSpec::default()
        },
        "progs/player.mdl",
    );
    // Pixel 1: skin 0 starts with the filled colour, so its flood fill keeps the rest.
    let first = |skin| model.translated(skin, 0x30, &palette()).unwrap().rgba[4];
    assert_eq!(first(0), 48);
    assert_eq!(first(1), 49, "a group's first image");
    assert_eq!(first(9), 48, "skin 0 for a skin out of range");
}
