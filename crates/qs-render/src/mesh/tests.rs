// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::float_cmp,
    reason = "test code: a failed unwrap or index is a failed test; exact inputs give exact floats"
)]

use qs_bsp::{
    BSP_HEADER_LUMPS, BSP_MAX_MAP_HULLS, BSP_MIPTEX_HEADER_SIZE, Bsp, BspVersion, Face, Lit, Lump,
    Model, Node, Plane, SURF_DRAWSKY, SURF_DRAWTURB, Texinfo, Texture,
};

use super::{Extras, MapMesh, MeshBuilder, MeshError, Palette, Placement};
use crate::styles::LightStyles;

/// One face of a test map: a 64 by 32 rectangle in the z = 0 plane, lit by `lighting`.
pub(crate) struct MapSpec {
    /// The face's surfedges, around the rectangle.
    pub(crate) surfedges: Vec<i32>,
    /// The face's flags.
    pub(crate) flags: u32,
    /// The face's styles.
    pub(crate) styles: [u8; 4],
    /// Where its samples start in the lighting lump; -1 for none.
    pub(crate) lightofs: i32,
    /// Whether the texinfo names a texture.
    pub(crate) textured: bool,
}

impl Default for MapSpec {
    fn default() -> Self {
        Self {
            surfedges: vec![1, 2, 3, 4],
            flags: 0,
            styles: [0, 255, 255, 255],
            lightofs: 0,
            textured: true,
        }
    }
}

/// The face's lightmap: `(64>>4)+1` by `(32>>4)+1` texels (REND-1.3).
pub(crate) const LIGHTMAP: [usize; 2] = [5, 3];

/// A palette whose colour `i` is `(i, 2i, 3i)` wrapped.
pub(crate) fn palette() -> Palette {
    let bytes: Vec<u8> = (0..=255_u8)
        .flat_map(|i| [i, i.wrapping_mul(2), i.wrapping_mul(3)])
        .collect();
    Palette::read(&bytes).unwrap()
}

/// The texture's pixels, 16 by 16 of index 7, after its 40-byte header in the BSP.
const PIXELS: [u8; 256] = [7; 256];

/// The test map: its one face lies in the z = 0 plane of node 0, whose sides are leaves.
pub(crate) fn bsp<'a>(spec: &MapSpec, lighting: &'a [u8]) -> Bsp<'a> {
    let texture = Texture {
        name: b"wall".to_vec(),
        width: 16,
        height: 16,
        offsets: [u32::try_from(BSP_MIPTEX_HEADER_SIZE).unwrap(), 0, 0, 0],
        pixels: &PIXELS,
        sky: false,
        animation: None,
    };
    Bsp {
        version: BspVersion::Bsp29,
        lumps: [Lump::default(); BSP_HEADER_LUMPS],
        entity_lump: b"",
        planes: vec![Plane {
            normal: [0.0, 0.0, 1.0],
            dist: 0.0,
            plane_type: 2,
            signbits: 0,
        }],
        textures: vec![Some(texture)],
        // Edge 0 is unused; edges 1 to 4 go round the rectangle, and their reversed ends are
        // read for negative surfedges.
        vertexes: vec![
            [0.0, 0.0, 0.0],
            [64.0, 0.0, 0.0],
            [64.0, 32.0, 0.0],
            [0.0, 32.0, 0.0],
            [99.0, 99.0, 99.0],
        ],
        visibility: b"",
        nodes: vec![Node {
            planenum: 0,
            children: [-1, -2],
            mins: [0.0; 3],
            maxs: [64.0, 32.0, 0.0],
            firstface: 0,
            numfaces: 1,
        }],
        texinfo: vec![Texinfo {
            vecs: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]],
            miptex: 0,
            texture: spec.textured.then_some(0),
            flags: 0,
            mipadjust: 1,
        }],
        faces: vec![Face {
            planenum: 0,
            side: 0,
            firstedge: 0,
            numedges: spec.surfedges.len(),
            texinfo: 0,
            styles: spec.styles,
            lightofs: spec.lightofs,
            texturemins: [0, 0],
            extents: [64, 32],
            flags: spec.flags,
            lightmap_size: LIGHTMAP,
        }],
        lighting,
        clipnodes: Vec::new(),
        leafs: Vec::new(),
        marksurfaces: Vec::new(),
        edges: vec![[4, 4], [0, 1], [1, 2], [2, 3], [3, 0]],
        surfedges: spec.surfedges.clone(),
        models: vec![Model {
            mins: [0.0; 3],
            maxs: [64.0, 32.0, 0.0],
            origin: [0.0; 3],
            headnode: [0; BSP_MAX_MAP_HULLS],
            visleafs: 0,
            firstface: 0,
            numfaces: 1,
            radius: 0.0,
        }],
    }
}

/// Fifteen samples of `value` for each of `styles` styles.
pub(crate) fn samples(values: &[u8]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|&v| [v; LIGHTMAP[0] * LIGHTMAP[1]])
        .collect()
}

fn build(spec: &MapSpec, lighting: &[u8]) -> MapMesh {
    MapMesh::build(&bsp(spec, lighting), &palette()).unwrap()
}

// spec REND-1.1: the polygon is the surfedges' vertices in order, the first end of a positive
// edge and the second end of a negative one, drawn as a fan from the first
#[test]
fn a_face_is_a_fan_over_its_edges_vertices() {
    let mesh = build(&MapSpec::default(), &samples(&[64]));
    let corners: Vec<[f32; 3]> = mesh.vertices.iter().map(|v| v.position).collect();
    assert_eq!(
        corners,
        [
            [0.0, 0.0, 0.0],
            [64.0, 0.0, 0.0],
            [64.0, 32.0, 0.0],
            [0.0, 32.0, 0.0]
        ]
    );
    assert_eq!(mesh.indices, [0, 1, 2, 0, 2, 3]);
    // The same rectangle the other way round, by negative surfedges, and a surfedge of 0,
    // which takes edge 0's second vertex.
    let reversed = MapSpec {
        surfedges: vec![-4, -3, -2, -1, 0],
        ..MapSpec::default()
    };
    let mesh = build(&reversed, &samples(&[64]));
    let corners: Vec<[f32; 3]> = mesh.vertices.iter().map(|v| v.position).collect();
    assert_eq!(
        corners,
        [
            [0.0, 0.0, 0.0],
            [0.0, 32.0, 0.0],
            [64.0, 32.0, 0.0],
            [64.0, 0.0, 0.0],
            [99.0, 99.0, 99.0]
        ]
    );
    assert_eq!(mesh.indices.len(), 3 * 3);
}

// spec REND-1.2: texture coordinates are the texinfo's s and t over the texture's size; a face
// with no texture is not drawn
#[test]
fn texture_coordinates_divide_by_the_textures_size() {
    let mesh = build(&MapSpec::default(), &samples(&[64]));
    assert_eq!(mesh.vertices[2].texture, [4.0, 2.0]);
    assert_eq!(mesh.batches.len(), 1);
    assert_eq!(mesh.batches[0].count, 6);
    let untextured = MapSpec {
        textured: false,
        ..MapSpec::default()
    };
    let mesh = build(&untextured, &samples(&[64]));
    assert!(mesh.vertices.is_empty() && mesh.batches.is_empty());
}

// spec REND-1.3: lightmap coordinates are (s - texturemins + light_s*16 + 8) over the atlas's
// size times 16, the face's texels placed after the atlas's full-bright texel
#[test]
fn lightmap_coordinates_map_texels_to_the_surface() {
    let mesh = build(&MapSpec::default(), &samples(&[64]));
    let (width, height) = (
        f32::from(u16::try_from(mesh.lightmap.width).unwrap()),
        f32::from(u16::try_from(mesh.lightmap.height).unwrap()),
    );
    assert_eq!(mesh.lightmap.width, 1024);
    assert_eq!(mesh.lightmap.height, 3);
    // light_s is 1, after the full-bright texel at column 0.
    assert_eq!(
        mesh.vertices[0].light,
        [(16.0 + 8.0) / (width * 16.0), 8.0 / (height * 16.0)]
    );
    assert_eq!(
        mesh.vertices[2].light,
        [
            (64.0 + 16.0 + 8.0) / (width * 16.0),
            (32.0 + 8.0) / (height * 16.0)
        ]
    );
}

// spec REND-1.13: a texel is the sum of each style's sample times 256, shifted right by 7 and
// capped at 255, over the styles until one is 255
#[test]
fn light_texels_sum_the_styles() {
    let texel = |spec: &MapSpec, lighting: &[u8]| build(spec, lighting).lightmap.rgba[4];
    assert_eq!(texel(&MapSpec::default(), &samples(&[50])), 100);
    assert_eq!(texel(&MapSpec::default(), &samples(&[200])), 255);
    let two = MapSpec {
        styles: [0, 1, 255, 2],
        ..MapSpec::default()
    };
    assert_eq!(texel(&two, &samples(&[30, 40, 90])), 140);
    // The full-bright texel and the texel's grey in each channel.
    let mesh = build(&MapSpec::default(), &samples(&[50]));
    assert_eq!(
        &mesh.lightmap.rgba[..8],
        [255, 255, 255, 255, 100, 100, 100, 255]
    );
}

// spec REND-1.5: a face with no samples is black; in a map with no lighting lump it is 255
#[test]
fn an_unlit_face_is_black_and_an_unlit_map_full_bright() {
    let none = MapSpec {
        lightofs: -1,
        ..MapSpec::default()
    };
    assert_eq!(build(&none, &samples(&[50])).lightmap.rgba[4], 0);
    assert_eq!(build(&MapSpec::default(), &[]).lightmap.rgba[4], 255);
}

// spec REND-1.6: a sky or turbulent face has no lightmap and takes the full-bright texel
#[test]
fn sky_and_water_are_full_bright() {
    for flags in [SURF_DRAWSKY, SURF_DRAWTURB] {
        let spec = MapSpec {
            flags,
            ..MapSpec::default()
        };
        let mesh = build(&spec, &samples(&[10]));
        assert_eq!(mesh.lightmap.height, 1, "no texels placed");
        assert!(mesh.vertices.iter().all(|v| v.light == [0.5 / 1024.0, 0.5]));
    }
}

// spec REND-1.8: pixels are palette indices; a palette is 256 colours of three bytes
#[test]
fn textures_go_through_the_palette() {
    let mesh = build(&MapSpec::default(), &samples(&[64]));
    let image = &mesh.textures[0];
    assert_eq!((image.width, image.height), (16, 16));
    assert_eq!(&image.rgba[..4], [7, 14, 21, 255]);
    assert_eq!(Palette::read(&[0; 767]), Err(MeshError::Palette(767)));
    assert_eq!(Palette::read(&[0; 769]), Err(MeshError::Palette(769)));
}

#[test]
fn a_map_with_no_world_model_is_refused() {
    let lighting = samples(&[64]);
    let mut map = bsp(&MapSpec::default(), &lighting);
    map.models.clear();
    assert_eq!(MapMesh::build(&map, &palette()), Err(MeshError::NoWorld));
}

// spec REND-2.1: an animated texture is the frame for (int)(time*10) % anim_total, found by
// following next from the base while the frame is outside min..max
#[test]
fn animated_textures_step_ten_times_a_second() {
    use super::{Animation, frame};
    // Three frames in a cycle: textures 0, 1 and 2.
    let cycle = |min, next| {
        Some(Animation {
            total: 3,
            min,
            max: min + 1,
            next,
        })
    };
    let animations = [cycle(0, 1), cycle(1, 2), cycle(2, 0), None];
    assert_eq!(frame(&animations, 0, 0.0), 0);
    assert_eq!(frame(&animations, 0, 0.15), 1);
    assert_eq!(frame(&animations, 0, 0.25), 2);
    assert_eq!(frame(&animations, 0, 0.35), 0);
    assert_eq!(frame(&animations, 1, 0.25), 2);
    // A texture with no frames, one outside the list, and a broken cycle stay as they are.
    assert_eq!(frame(&animations, 3, 0.25), 3);
    assert_eq!(frame(&animations, 9, 0.25), 9);
    let broken = [cycle(0, 1), cycle(5, 1)];
    assert_eq!(frame(&broken, 0, 0.15), 0);
}

// spec REND-2.2: a turbulent face's texture coordinates are the raw dot products, without the
// texinfo's offset
#[test]
fn a_turbulent_face_keeps_its_raw_coordinates() {
    let spec = MapSpec {
        flags: SURF_DRAWTURB,
        ..MapSpec::default()
    };
    let lighting = samples(&[10]);
    let mut map = bsp(&spec, &lighting);
    map.texinfo[0].vecs[0][3] = 5.0;
    let mesh = MapMesh::build(&map, &palette()).unwrap();
    assert_eq!(mesh.vertices[2].texture, [64.0, 32.0]);
    assert_eq!(mesh.batches[0].surface, super::Surface::Turbulent);
}

// spec REND-2.3: a sky is two 128 by 128 layers: the solid one its texture's right half, the
// alpha one its left half with colour 0 transparent and coloured as the solid layer's average
#[test]
fn a_sky_is_a_solid_and_an_alpha_layer() {
    let spec = MapSpec {
        flags: SURF_DRAWSKY,
        ..MapSpec::default()
    };
    let lighting = samples(&[10]);
    let mut map = bsp(&spec, &lighting);
    // The left half alternates colours 0 and 4 in each row; the right half is all colour 2.
    let pixels: Vec<u8> = (0..128)
        .flat_map(|_| {
            (0..256).map(|x| {
                if x >= 128 {
                    2
                } else if x % 2 == 0 {
                    0
                } else {
                    4
                }
            })
        })
        .collect();
    let header = u32::try_from(BSP_MIPTEX_HEADER_SIZE).unwrap();
    if let Some(Some(texture)) = map.textures.first_mut() {
        texture.width = 256;
        texture.height = 128;
        texture.offsets = [header, 0, 0, 0];
        texture.pixels = Box::leak(pixels.into_boxed_slice());
    }
    let mesh = MapMesh::build(&map, &palette()).unwrap();
    let [solid, alpha] = mesh.skies[0].as_ref().unwrap();
    assert_eq!((solid.width, solid.height, alpha.width), (128, 128, 128));
    assert!(
        solid
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [2, 4, 6, 255])
    );
    // Colour 0 is transparent and the solid layer's average, colour 4 opaque.
    assert_eq!(&alpha.rgba[..8], [2, 4, 6, 0, 4, 8, 12, 255]);
    assert_eq!(mesh.batches[0].surface, super::Surface::Sky);
    // A texture of any other size has no layers.
    let mesh = build(&spec, &samples(&[10]));
    assert!(mesh.skies[0].is_none());
}

// spec REND-3.1: each palette byte becomes pow((c+1)/256, gamma)*255 + 0.5, kept within 0 to 255
#[test]
fn the_palette_is_corrected_by_gamma() {
    let bytes: Vec<u8> = (0..=255_u8).flat_map(|i| [i, 0, 255]).collect();
    let palette = Palette::read(&bytes).unwrap();
    let corrected = palette.with_gamma(super::GAMMA);
    // pow(1/256, 0.7)*255 + 0.5 = 5.08; pow(129/256, 0.7)*255 + 0.5 = 158.3; 256/256 stays 255.
    assert_eq!(corrected.rgba(0), [5, 5, 255, 255]);
    assert_eq!(corrected.rgba(128), [158, 5, 255, 255]);
    // At gamma 1, 129/256*255 + 0.5 = 128.996, which C's conversion truncates to 128.
    assert_eq!(palette.with_gamma(1.0).rgba(128), [128, 1, 255, 255]);
}

// spec REND-3.2: a texture is point-sampled up to powers of two, then each level is the >>2
// average of each 2 by 2 block, down to 1 by 1; a level of height 1 keeps the bytes it has
#[test]
fn mip_levels_follow_gl_resample_and_gl_mipmap() {
    use super::{Image, mip_levels};
    // 2 by 2 of grey 0, 10, 20 and 31: one 1 by 1 level of (0+10+20+31)>>2 = 15.
    let grey = |values: &[u8]| -> Vec<u8> { values.iter().flat_map(|&v| [v, v, v, 255]).collect() };
    let levels = mip_levels(&Image {
        width: 2,
        height: 2,
        rgba: grey(&[0, 10, 20, 31]),
    });
    assert_eq!(levels.len(), 2);
    assert_eq!(levels[1].rgba, [15, 15, 15, 255]);
    // 3 by 1 is resampled to 4 by 1 by frac stepping 3*0x10000/4 from its half: 0, 1, 1, 2.
    let levels = mip_levels(&Image {
        width: 3,
        height: 1,
        rgba: grey(&[1, 2, 3]),
    });
    assert_eq!((levels[0].width, levels[0].height), (4, 1));
    assert_eq!(levels[0].rgba, grey(&[1, 2, 2, 3]));
    // With height 1 no row is averaged: each level keeps the bytes the buffer starts with.
    assert_eq!(
        levels.iter().map(|l| l.width).collect::<Vec<_>>(),
        [4, 2, 1]
    );
    assert_eq!(levels[1].rgba, grey(&[1, 2]));
    assert_eq!(levels[2].rgba, grey(&[1]));
}

// spec REND-1.9: a brush entity's corners are turned by roll about x, pitch about y and yaw
// about z, then moved by its origin; its texture coordinates stay its own
#[test]
fn a_placed_model_moves_and_turns_its_corners() {
    let world = bsp(&MapSpec::default(), &[]);
    let palette = palette();
    let mut builder = MeshBuilder::new(&palette);
    let placed = Placement {
        model: 0,
        origin: [100.0, 200.0, 300.0],
        angles: [0.0, 90.0, 0.0],
    };
    builder.add(&world, &[Placement::WORLD, placed]).unwrap();
    let mesh = builder.finish();
    let (unmoved, moved) = mesh.vertices.split_at(4);
    // The entity's vertices are its model's; its matrix moves them.
    assert_eq!(moved[1].position, unmoved[1].position);
    assert_eq!(mesh.models, [Placement::WORLD, placed]);
    let m = mesh.models[1].matrix();
    let apply = |[x, y, z]: [f32; 3]| {
        [0, 1, 2].map(|r| (m[0][r] * x + m[1][r] * y + m[2][r] * z + m[3][r]).round())
    };
    // Yaw 90 takes (64, 0, 0) to (0, 64, 0).
    assert_eq!(apply(moved[1].position), [100.0, 264.0, 300.0]);
    assert_eq!(apply(moved[2].position), [68.0, 264.0, 300.0]);
    assert_eq!(
        apply(moved[2].position),
        placed.position(moved[2].position).map(f32::round)
    );
    assert_eq!(moved[1].texture, unmoved[1].texture);
    assert_eq!(moved[1].light, unmoved[1].light);
    // The first model added gives the bounds.
    assert_eq!(mesh.maxs, [64.0, 32.0, 0.0]);
    // The world's copy and the entity's are batches of their own (REND-1.11).
    assert_eq!(mesh.batches.len(), 2);
    assert_eq!(mesh.batches[0].count, 6);
}

// spec REND-1.9: pitch turns about y with the sign `R_DrawBrushModel`'s double negation leaves,
// and roll about x
#[test]
fn pitch_and_roll_turn_as_r_rotate_for_entity() {
    let pitched = Placement {
        model: 0,
        origin: [0.0; 3],
        angles: [90.0, 0.0, 0.0],
    };
    // glRotatef (90, 0, 1, 0) takes x to -z.
    let p = pitched.position([1.0, 0.0, 0.0]).map(f32::round);
    assert_eq!(p, [0.0, 0.0, -1.0]);
    let rolled = Placement {
        angles: [0.0, 0.0, 90.0],
        ..pitched
    };
    // glRotatef (90, 1, 0, 0) takes y to z.
    assert_eq!(
        rolled.position([0.0, 1.0, 0.0]).map(f32::round),
        [0.0, 0.0, 1.0]
    );
}

// spec REND-1.9: a second BSP's textures follow the first's, so its animations still name
// its own frames
#[test]
fn a_second_bsps_textures_follow_the_firsts() {
    let world = bsp(&MapSpec::default(), &[]);
    let mut other = bsp(&MapSpec::default(), &[]);
    other.textures[0].as_mut().unwrap().animation = Some(qs_bsp::Animation {
        total: 1,
        min: 0,
        max: 1,
        next: 0,
        alternate: None,
    });
    let palette = palette();
    let mut builder = MeshBuilder::new(&palette);
    builder.add(&world, &[Placement::WORLD]).unwrap();
    builder.add(&other, &[Placement::WORLD]).unwrap();
    let mesh = builder.finish();
    assert_eq!(mesh.textures.len(), 2);
    assert_eq!(mesh.animations[1].unwrap().next, 1);
    assert_eq!(
        mesh.batches.iter().map(|b| b.texture).collect::<Vec<_>>(),
        [0, 1]
    );
}

#[test]
fn a_placement_of_a_missing_model_is_refused() {
    let world = bsp(&MapSpec::default(), &[]);
    let palette = palette();
    let mut builder = MeshBuilder::new(&palette);
    let missing = Placement {
        model: 1,
        ..Placement::WORLD
    };
    assert!(matches!(
        builder.add(&world, &[missing]),
        Err(MeshError::NoWorld)
    ));
}

// spec REND-2.1: a cycle of no length is not animated
#[test]
fn an_empty_cycle_is_not_animated() {
    let mut map = bsp(&MapSpec::default(), &[]);
    let cycle = |total| qs_bsp::Animation {
        total,
        min: 0,
        max: 1,
        next: 0,
        alternate: None,
    };
    map.textures[0].as_mut().unwrap().animation = Some(cycle(0));
    assert_eq!(MapMesh::build(&map, &palette()).unwrap().animations, [None]);
    map.textures[0].as_mut().unwrap().animation = Some(cycle(1));
    assert!(MapMesh::build(&map, &palette()).unwrap().animations[0].is_some());
}

// spec REND-1.11: the world's faces come before every entity's, in batches of their own
#[test]
fn the_world_is_drawn_before_entities() {
    let world = bsp(&MapSpec::default(), &[]);
    let palette = palette();
    let mut builder = MeshBuilder::new(&palette);
    let lift = Placement {
        model: 0,
        origin: [0.0, 0.0, 5.0],
        angles: [0.0; 3],
    };
    builder.add(&world, &[Placement::WORLD, lift]).unwrap();
    let mesh = builder.finish();
    assert_eq!(mesh.batches.len(), 2);
    assert_eq!(mesh.batches[0].model, 0);
    assert_eq!(mesh.batches[1].model, 1);
    assert!(mesh.batches[0].first < mesh.batches[1].first);
    assert_eq!(mesh.models[1].matrix()[3], [0.0, 0.0, 5.0, 1.0]);
}

// spec REND-1.12: gl_zfix draws a brush entity 1/32 back on every axis, angles as they are
#[test]
fn zfix_moves_an_entity_back_a_32nd() {
    let lift = Placement {
        model: 3,
        origin: [10.0, 20.0, -66.0],
        angles: [0.0, 90.0, 0.0],
    };
    let fixed = lift.zfixed();
    assert_eq!(fixed.origin, [9.968_75, 19.968_75, -66.031_25]);
    assert_eq!(fixed.angles, lift.angles);
    assert_eq!(fixed.model, 3);
}

/// The texel at the face's corner, first of its row, in the image `mesh` made for `styles`.
fn texel_for(mesh: &MapMesh, styles: &LightStyles) -> [u8; 4] {
    let image = mesh.lightmaps.image(styles);
    image.rgba[4..8].try_into().unwrap()
}

/// Style `style`'s string is `string`; the others have none.
fn style(style: usize, string: &'static [u8]) -> LightStyles {
    LightStyles::at(move |s| (s == style).then_some(string), 0.0)
}

// spec REND-1.13: a texel sums each of its styles' samples times the style's value at the time,
// shifted right by 7 and capped at 255
#[test]
fn a_texel_sums_its_styles_at_their_values() {
    let spec = MapSpec {
        styles: [0, 3, 255, 2],
        ..MapSpec::default()
    };
    let mesh = build(&spec, &samples(&[30, 40, 90]));
    assert_eq!(
        texel_for(&mesh, &LightStyles::unset()),
        [140, 140, 140, 255]
    );
    // Style 3 at 'a' (0): only style 0's 30 * 256 >> 7 = 60.
    assert_eq!(texel_for(&mesh, &style(3, b"a")), [60, 60, 60, 255]);
    // Style 3 at 'z' (550): 30*2 + 40*550/128 = 60 + 171.
    assert_eq!(texel_for(&mesh, &style(3, b"z")), [231, 231, 231, 255]);
    // Style 2 is past the 255 that ends the list, so it is never read.
    assert_eq!(texel_for(&mesh, &style(2, b"z")), [140, 140, 140, 255]);
    let bright = build(&MapSpec::default(), &samples(&[200]));
    assert_eq!(texel_for(&bright, &style(0, b"z"))[0], 255, "capped");
}

// spec REND-5.4: the unsigned sum is read as a signed int: a negative one, from a letter
// before a, is drawn as its low byte after the shift
#[test]
fn a_negative_sum_keeps_its_low_byte() {
    let mesh = build(&MapSpec::default(), &samples(&[1]));
    // '0' is -1078: 1 * -1078 = -1078, >> 7 = -9 (arithmetic), low byte 247.
    assert_eq!(texel_for(&mesh, &style(0, b"0"))[0], 247);
}

// spec REND-5.5: the lightmap is made again only when a style some face reads changed
#[test]
fn relighting_waits_for_a_style_a_face_reads() {
    let mut mesh = build(&MapSpec::default(), &samples(&[50]));
    let first = mesh.lightmap.clone();
    assert!(!mesh.relight(&LightStyles::unset()), "nothing changed");
    assert!(!mesh.relight(&style(5, b"a")), "style 5 is no face's");
    assert!(mesh.relight(&style(0, b"a")));
    assert_ne!(mesh.lightmap, first);
    assert_eq!(&mesh.lightmap.rgba[4..8], [0, 0, 0, 255]);
    assert!(!mesh.relight(&style(0, b"a")), "applied already");
    assert!(mesh.relight(&LightStyles::unset()));
    assert_eq!(mesh.lightmap, first);
    // A map with no lighting lump, or a face with no samples, reads no style.
    let mut full = build(&MapSpec::default(), &[]);
    assert!(!full.relight(&style(0, b"a")));
    let dark = MapSpec {
        lightofs: -1,
        ..MapSpec::default()
    };
    let mut dark = build(&dark, &samples(&[50]));
    assert!(!dark.relight(&style(0, b"a")));
    assert_eq!(&dark.lightmap.rgba[4..8], [0, 0, 0, 255]);
}

/// A `.lit` file for `lighting`: each byte's triple `(b, 2b, 3b)`, wrapped.
fn lit_file(lighting: &[u8]) -> Vec<u8> {
    let mut file = b"QLIT".to_vec();
    file.extend(1_i32.to_le_bytes());
    for &b in lighting {
        file.extend([b, b.wrapping_mul(2), b.wrapping_mul(3)]);
    }
    file
}

// spec REND-6.1: with a .lit file a face's samples are its triples at lightofs * 3
// spec REND-6.2: each channel sums as a grey texel does, shifted and capped at 255
#[test]
fn a_lit_file_lights_each_channel() {
    let lighting = samples(&[50]);
    let file = lit_file(&lighting);
    let lit = Lit::read(&file, lighting.len()).unwrap();
    let map = bsp(&MapSpec::default(), &lighting);
    let palette = palette();
    let mut builder = MeshBuilder::new(&palette);
    let extras = Extras {
        lit: Some(&lit),
        ..Extras::default()
    };
    builder
        .add_with(&map, &extras, &[Placement::WORLD])
        .unwrap();
    let mesh = builder.finish();
    // 50, 100, 150 at 256 >> 7: 100, 200, 300 capped to 255.
    assert_eq!(&mesh.lightmap.rgba[4..8], [100, 200, 255, 255]);
    assert_eq!(texel_for(&mesh, &style(0, b"a")), [0, 0, 0, 255]);
    // REND-6.2: an unsigned sum shifts as unsigned, so a letter before a is bright, not
    // its low byte.
    assert_eq!(texel_for(&mesh, &style(0, b"0"))[0], 255);
}

// MAP-24: a texture with a palette of its own is read through it, the others through the
// mesh's
#[test]
fn a_texture_may_carry_its_own_palette() {
    let lighting = samples(&[50]);
    let map = bsp(&MapSpec::default(), &lighting);
    let shared = palette();
    let own_bytes: Vec<u8> = (0..=255_u8).flat_map(|i| [255 - i, 0, i]).collect();
    let own = Palette::read(&own_bytes).unwrap();
    let mut builder = MeshBuilder::new(&shared);
    let palettes = [Some(own.clone())];
    let extras = Extras {
        palettes: &palettes,
        ..Extras::default()
    };
    builder
        .add_with(&map, &extras, &[Placement::WORLD])
        .unwrap();
    builder.add(&map, &[Placement::WORLD]).unwrap();
    let mesh = builder.finish();
    // The texture's pixels are all index 7.
    assert_eq!(&mesh.textures[0].rgba[..4], own.rgba(7).as_slice());
    assert_eq!(&mesh.textures[1].rgba[..4], shared.rgba(7).as_slice());
    assert_ne!(own.rgba(7), shared.rgba(7));
}
