// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::float_cmp,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap or index is a failed test; exact inputs give exact floats; the atlas's offsets are small"
)]

use crate::look::Look;
use qs_bsp::{
    BSP_HEADER_LUMPS, BSP_MAX_MAP_HULLS, BSP_MIPTEX_HEADER_SIZE, Bsp, BspVersion, Face, Lit, Lump,
    Model, Node, Plane, SURF_DRAWSKY, SURF_DRAWTURB, Texinfo, Texture,
};

use qs_client::DynamicLight;

use super::{Changes, Extras, Image, MapMesh, MeshBuilder, MeshError, Palette, Placement, Surface};
use crate::light::light_point;
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
    /// The face's smallest texture coordinates.
    pub(crate) texturemins: [i16; 2],
}

impl Default for MapSpec {
    fn default() -> Self {
        Self {
            surfedges: vec![1, 2, 3, 4],
            flags: 0,
            styles: [0, 255, 255, 255],
            lightofs: 0,
            textured: true,
            texturemins: [0, 0],
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
        palette: None,
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
            texturemins: spec.texturemins,
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
    MapMesh::build(&bsp(spec, lighting), &palette(), Look::GlQuake).unwrap()
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

// spec REND-1.2: texture coordinates are the texinfo's s and t over the texture's size
#[test]
fn texture_coordinates_divide_by_the_textures_size() {
    let mesh = build(&MapSpec::default(), &samples(&[64]));
    assert_eq!(mesh.vertices[2].texture, [4.0, 2.0]);
    assert_eq!(mesh.batches.len(), 1);
    assert_eq!(mesh.batches[0].count, 6);
}

// spec REND-1.14: a face with no texture is drawn with the stand-in after the BSP's textures,
// its coordinates over `r_notexture_mip`'s 32, its image QuakeSpasm's checker in squares of 16
#[test]
fn a_face_with_no_texture_is_drawn_with_the_stand_in() {
    let untextured = MapSpec {
        textured: false,
        ..MapSpec::default()
    };
    let mesh = build(&untextured, &samples(&[64]));
    let stand_in = mesh.textures.len() - 1;
    assert_eq!(mesh.batches.len(), 1);
    assert_eq!(mesh.batches[0].texture, stand_in);
    assert_eq!(mesh.batches[0].count, 6);
    assert_eq!(mesh.vertices[2].texture, [2.0, 1.0]);
    let image = &mesh.textures[stand_in];
    assert_eq!((image.width, image.height), (32, 32));
    let texel = |x: usize, y: usize| &image.rgba[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4];
    let (pink, black) = ([159, 91, 83, 255], [0, 0, 0, 255]);
    assert_eq!((texel(0, 0), texel(15, 15)), (&pink[..], &pink[..]));
    assert_eq!((texel(16, 0), texel(0, 16)), (&black[..], &black[..]));
    assert_eq!(texel(31, 31), &pink[..]);
}

// spec REND-1.14: a face with no texture has no lightmap, whatever its flags or samples: it
// takes the full-bright texel and is drawn as a regular face with the stand-in
#[test]
fn a_face_with_no_texture_is_full_bright() {
    for (flags, lightofs) in [(0, 0), (0, -1), (SURF_DRAWTURB, 0), (SURF_DRAWSKY, 0)] {
        let spec = MapSpec {
            textured: false,
            flags,
            lightofs,
            ..MapSpec::default()
        };
        let mesh = build(&spec, &samples(&[64]));
        assert_eq!(mesh.lightmap.height, 1, "no texels placed");
        assert!(mesh.vertices.iter().all(|v| v.light == [0.5 / 1024.0, 0.5]));
        assert_eq!(mesh.batches.len(), 1);
        assert_eq!(mesh.batches[0].texture, mesh.textures.len() - 1);
        assert_eq!(mesh.batches[0].surface, Surface::Lit);
    }
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

// spec REND-1.3: a face's lightmap coordinates start at its texturemins and its place in the
// atlas, on a later row once a row is full
// spec REND-1.14: a face with no texture takes the full-bright texel however tall the atlas
#[test]
fn lightmap_coordinates_follow_texturemins_and_the_atlas_row() {
    let lighting = samples(&[64]);
    let world = bsp(
        &MapSpec {
            texturemins: [16, 16],
            ..MapSpec::default()
        },
        &lighting,
    );
    let untextured = bsp(
        &MapSpec {
            textured: false,
            ..MapSpec::default()
        },
        &lighting,
    );
    let palette = palette();
    let mut builder = MeshBuilder::new(&palette, Look::GlQuake);
    // After the full-bright texel, 204 faces 5 texels wide fill the 1024 texel row.
    for _ in 0..205 {
        builder.add(&world, &[Placement::WORLD]).unwrap();
    }
    builder.add(&untextured, &[Placement::WORLD]).unwrap();
    let mesh = builder.finish();
    assert_eq!((mesh.lightmap.width, mesh.lightmap.height), (1024, 6));
    let wrapped = &mesh.vertices[204 * 4];
    assert_eq!(
        wrapped.light,
        [
            (-16.0 + 8.0) / (1024.0 * 16.0),
            (-16.0 + 48.0 + 8.0) / (6.0 * 16.0)
        ]
    );
    let stand_in = &mesh.vertices[205 * 4..];
    assert_eq!(stand_in.len(), 4);
    assert!(
        stand_in
            .iter()
            .all(|v| v.light == [0.5 / 1024.0, 0.5 / 6.0])
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
    assert_eq!(
        MapMesh::build(&map, &palette(), Look::GlQuake),
        Err(MeshError::NoWorld)
    );
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
    let mesh = MapMesh::build(&map, &palette(), Look::GlQuake).unwrap();
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
    let mesh = MapMesh::build(&map, &palette(), Look::GlQuake).unwrap();
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
    let mut builder = MeshBuilder::new(&palette, Look::GlQuake);
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
    let mut builder = MeshBuilder::new(&palette, Look::GlQuake);
    builder.add(&world, &[Placement::WORLD]).unwrap();
    builder.add(&other, &[Placement::WORLD]).unwrap();
    let mesh = builder.finish();
    // REND-1.14: each BSP's textures, then its stand-in for a missing one.
    assert_eq!(mesh.textures.len(), 4);
    assert_eq!(mesh.animations[2].unwrap().next, 2);
    assert_eq!(
        mesh.batches.iter().map(|b| b.texture).collect::<Vec<_>>(),
        [0, 2]
    );
}

#[test]
fn a_placement_of_a_missing_model_is_refused() {
    let world = bsp(&MapSpec::default(), &[]);
    let palette = palette();
    let mut builder = MeshBuilder::new(&palette, Look::GlQuake);
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
    assert_eq!(
        MapMesh::build(&map, &palette(), Look::GlQuake)
            .unwrap()
            .animations,
        [None, None],
        "the texture's and the stand-in's (REND-1.14)"
    );
    map.textures[0].as_mut().unwrap().animation = Some(cycle(1));
    assert!(
        MapMesh::build(&map, &palette(), Look::GlQuake)
            .unwrap()
            .animations[0]
            .is_some()
    );
}

// spec REND-1.11: the world's faces come before every entity's, in batches of their own
#[test]
fn the_world_is_drawn_before_entities() {
    let world = bsp(&MapSpec::default(), &[]);
    let palette = palette();
    let mut builder = MeshBuilder::new(&palette, Look::GlQuake);
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
    assert!(
        mesh.relight(&LightStyles::unset()).is_empty(),
        "nothing changed"
    );
    assert!(
        mesh.relight(&style(5, b"a")).is_empty(),
        "style 5 is no face's"
    );
    assert!(!mesh.relight(&style(0, b"a")).is_empty());
    assert_ne!(mesh.lightmap, first);
    assert_eq!(&mesh.lightmap.rgba[4..8], [0, 0, 0, 255]);
    assert!(mesh.relight(&style(0, b"a")).is_empty(), "applied already");
    assert!(!mesh.relight(&LightStyles::unset()).is_empty());
    assert_eq!(mesh.lightmap, first);
    // A map with no lighting lump, or a face with no samples, reads no style.
    let mut full = build(&MapSpec::default(), &[]);
    assert!(full.relight(&style(0, b"a")).is_empty());
    let dark = MapSpec {
        lightofs: -1,
        ..MapSpec::default()
    };
    let mut dark = build(&dark, &samples(&[50]));
    assert!(dark.relight(&style(0, b"a")).is_empty());
    assert_eq!(&dark.lightmap.rgba[4..8], [0, 0, 0, 255]);
}

// spec REND-5.7: a relight answers the rectangle the faces it made again cover, its texels
// the atlas's
#[test]
fn a_relight_answers_the_rectangle_it_changed() {
    let mut mesh = build(&MapSpec::default(), &samples(&[50]));
    let patches = mesh.relight(&style(0, b"a"));
    assert_eq!(patches.len(), 1, "one face, one block");
    let patch = &patches[0];
    let face = &mesh.lightmaps.faces[0];
    assert_eq!(patch.at.map(|n| n as usize), face.at);
    assert_eq!(
        [patch.image.width as usize, patch.image.height as usize],
        face.size
    );
    let row = mesh.lightmap.width as usize * 4;
    let width = patch.image.width as usize * 4;
    let [x, y] = face.at;
    for (t, line) in patch.image.rgba.chunks(width).enumerate() {
        let start = (y + t) * row + x * 4;
        assert_eq!(line, &mesh.lightmap.rgba[start..start + width], "row {t}");
    }
}

// spec REND-5.7: faces in one 256-texel block grow one rectangle; a face in another block has
// its own; each rectangle is clipped to the atlas
#[test]
fn changes_grow_one_rectangle_a_block() {
    let atlas = Image {
        width: 512,
        height: 300,
        rgba: (0..512_u32 * 300 * 4)
            .map(|n| u8::try_from(n % 251).unwrap())
            .collect(),
    };
    let mut changes = Changes::default();
    changes.add([10, 20], [4, 2]);
    changes.add([100, 5], [3, 3]);
    changes.add([300, 0], [2, 2]);
    changes.add([0, 290], [600, 20]);
    let patches = changes.patches(&atlas);
    let rects: Vec<_> = patches
        .iter()
        .map(|p| (p.at, p.image.width, p.image.height))
        .collect();
    assert_eq!(
        rects,
        [([10, 5], 93, 17), ([0, 290], 512, 10), ([300, 0], 2, 2)]
    );
    for patch in &patches {
        let [x, y] = patch.at.map(|n| n as usize);
        let width = patch.image.width as usize * 4;
        assert_eq!(patch.image.rgba.len(), width * patch.image.height as usize);
        let start = y * 512 * 4 + x * 4;
        assert_eq!(patch.image.rgba[..width], atlas.rgba[start..start + width]);
    }
}

/// 16 by 16 pixels of index 7 with a fullbright 230 and 255 in the first row.
static FULLBRIGHT_PIXELS: [u8; 256] = {
    let mut pixels = [7; 256];
    pixels[1] = 230;
    pixels[2] = 255;
    pixels
};

/// The test map with [`FULLBRIGHT_PIXELS`] for its texture.
fn fullbright_bsp<'a>(spec: &MapSpec, lighting: &'a [u8]) -> Bsp<'a> {
    let mut bsp = bsp(spec, lighting);
    if let Some(Some(texture)) = bsp.textures.get_mut(0) {
        texture.pixels = &FULLBRIGHT_PIXELS;
    }
    bsp
}

// spec REND-13.4: in QuakeSpasm's look a texture with an index of 224 or above is split: its
// own texels black there, its glow those texels' colours and black elsewhere
#[test]
fn fullbright_texels_glow_in_quakespasm_s_look() {
    let lighting = samples(&[50]);
    let mesh = MapMesh::build(
        &fullbright_bsp(&MapSpec::default(), &lighting),
        &palette(),
        Look::QuakeSpasm,
    )
    .unwrap();
    let texture = &mesh.textures[0].rgba;
    let glow = mesh.glows[0].as_ref().unwrap();
    let colour = |i: u8| [i, i.wrapping_mul(2), i.wrapping_mul(3), 255];
    assert_eq!(
        texture[..12],
        [colour(7), [0, 0, 0, 255], [0, 0, 0, 255]].concat()
    );
    assert_eq!(
        glow.rgba[..12],
        [[0, 0, 0, 255], colour(230), colour(255)].concat()
    );
    assert_eq!((glow.width, glow.height), (16, 16));
}

// spec REND-13.4: GLQuake's look, a texture with no fullbright index and a liquid have no
// glow, and their texels are the palette's
#[test]
fn only_quakespasm_s_regular_textures_glow() {
    let lighting = samples(&[50]);
    let glq = MapMesh::build(
        &fullbright_bsp(&MapSpec::default(), &lighting),
        &palette(),
        Look::GlQuake,
    )
    .unwrap();
    assert_eq!(glq.glows, [None, None]);
    assert_eq!(glq.textures[0].rgba[4..8], [230, 204, 178, 255]);
    let plain = MapMesh::build(
        &bsp(&MapSpec::default(), &lighting),
        &palette(),
        Look::QuakeSpasm,
    )
    .unwrap();
    assert_eq!(plain.glows, [None, None]);
    let water = MapSpec {
        flags: SURF_DRAWTURB,
        ..MapSpec::default()
    };
    let liquid = MapMesh::build(
        &fullbright_bsp(&water, &lighting),
        &palette(),
        Look::QuakeSpasm,
    )
    .unwrap();
    assert_eq!(liquid.glows, [None, None]);
}

// spec REND-13.5: with overbright a texel's sum is shifted by 8, so 50's sum of 12800 is 50,
// where GLQuake's 7 makes it 100; the unsigned sum is capped at 255
#[test]
fn overbright_shifts_the_light_by_eight() {
    let lighting = samples(&[50]);
    let qs = MapMesh::build(
        &bsp(&MapSpec::default(), &lighting),
        &palette(),
        Look::QuakeSpasm,
    )
    .unwrap();
    assert_eq!(texel(&qs, 0, 0), [50, 50, 50, 255]);
    // "z" is 550: 50 * 550 >> 8 is 107; 255 * 550 >> 8 is 547, capped.
    assert_eq!(texel_for(&qs, &style(0, b"z"))[0], 107);
    let bright = samples(&[255]);
    let capped = MapMesh::build(
        &bsp(&MapSpec::default(), &bright),
        &palette(),
        Look::QuakeSpasm,
    )
    .unwrap();
    assert_eq!(texel_for(&capped, &style(0, b"z"))[0], 255);
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
    let mut builder = MeshBuilder::new(&palette, Look::GlQuake);
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
    let mut builder = MeshBuilder::new(&shared, Look::GlQuake);
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
    assert_eq!(&mesh.textures[2].rgba[..4], shared.rgba(7).as_slice());
    assert_ne!(own.rgba(7), shared.rgba(7));
}

fn light(origin: [f32; 3], radius: f32, minlight: f32) -> DynamicLight {
    DynamicLight {
        origin,
        radius,
        minlight,
    }
}

/// The face's texel `(s, t)` of `mesh`'s lightmap, which starts at column 1 of row 0.
fn texel(mesh: &MapMesh, s: usize, t: usize) -> [u8; 4] {
    let width = usize::try_from(mesh.lightmap.width).unwrap();
    let at = (t * width + 1 + s) * 4;
    mesh.lightmap.rgba[at..at + 4].try_into().unwrap()
}

// spec REND-7.2: a marked face adds `(rad - dist) * 256` to the texels within `rad -
// minlight` of the light's impact, `dist` being the greater axis distance plus half the
// lesser, in 16-unit texels; a light with `rad < minlight` adds nothing
#[test]
fn a_dynamic_light_brightens_the_texels_near_its_impact() {
    let mut mesh = build(&MapSpec::default(), &samples(&[50]));
    // 10 above the face with radius 20: rad 10, so only the impact's texel is within 10.
    assert!(
        !mesh
            .relight_with(&LightStyles::unset(), &[light([0.0, 0.0, 10.0], 20.0, 0.0)])
            .is_empty()
    );
    // 50 * 256 + 10 * 256 = 15360, >> 7 is 120.
    assert_eq!(texel(&mesh, 0, 0), [120, 120, 120, 255]);
    assert_eq!(texel(&mesh, 1, 0), [100, 100, 100, 255]);
    assert_eq!(texel(&mesh, 0, 1), [100, 100, 100, 255]);
    // Radius 60 at the same height: rad 50; texel (1, 0) is 16 away, (1, 1) is 16 + 8, (2, 0)
    // is 32, (0, 2) is 32, (2, 1) is 32 + 8.
    assert!(
        !mesh
            .relight_with(&LightStyles::unset(), &[light([0.0, 0.0, 10.0], 60.0, 0.0)])
            .is_empty()
    );
    let value = |add: u32| u8::try_from((50 * 256 + add * 256) >> 7).unwrap();
    assert_eq!(texel(&mesh, 0, 0)[0], value(50));
    assert_eq!(texel(&mesh, 1, 0)[0], value(34));
    assert_eq!(texel(&mesh, 1, 1)[0], value(26));
    assert_eq!(texel(&mesh, 2, 0)[0], value(18));
    assert_eq!(texel(&mesh, 2, 1)[0], value(10));
    assert_eq!(texel(&mesh, 3, 0)[0], value(2));
    assert_eq!(texel(&mesh, 4, 0)[0], 100, "64 away: nothing");
    // The light's `minlight` narrows the reach: rad 50 - 40 = 10 reaches only the impact.
    assert!(
        !mesh
            .relight_with(
                &LightStyles::unset(),
                &[light([0.0, 0.0, 10.0], 60.0, 40.0)]
            )
            .is_empty()
    );
    assert_eq!(texel(&mesh, 0, 0)[0], value(50));
    assert_eq!(texel(&mesh, 1, 0)[0], 100);
    // `rad < minlight` adds nothing at all.
    assert!(
        !mesh
            .relight_with(
                &LightStyles::unset(),
                &[light([0.0, 0.0, 10.0], 60.0, 55.0)]
            )
            .is_empty()
    );
    assert_eq!(texel(&mesh, 0, 0)[0], 100);
    // The sum is capped at 255 after the shift (REND-5.4).
    assert!(
        !mesh
            .relight_with(&LightStyles::unset(), &[light([0.0, 0.0, 1.0], 500.0, 0.0)])
            .is_empty()
    );
    assert_eq!(texel(&mesh, 0, 0)[0], 255);
}

// spec REND-7.7: the renderer takes 64 lights, QuakeSpasm's `MAX_DLIGHTS`: the 64th marks and
// lights the face, as id's 32 would not
#[test]
fn the_64th_light_is_pushed() {
    let mut mesh = build(&MapSpec::default(), &samples(&[50]));
    let mut lights = vec![light([0.0, 0.0, 1000.0], 1.0, 0.0); 63];
    lights.push(light([0.0, 0.0, 3.0], 5.0, 0.0));
    assert!(!mesh.relight_with(&LightStyles::unset(), &lights).is_empty());
    assert_ne!(texel(&mesh, 0, 0)[0], 100);
}

// spec REND-7.6: a light straddling the node's plane marks the 64 by 32 face only when its
// impact, clamped to the face's extents, is within its radius: `s*s + t*t + dist*dist <
// radius*radius`, strictly
#[test]
fn a_light_marks_only_a_face_it_reaches() {
    let mut mesh = build(&MapSpec::default(), &samples(&[50]));
    let marks = |mesh: &mut MapMesh, origin: [f32; 3]| {
        !mesh
            .relight_with(&LightStyles::unset(), &[light(origin, 5.0, 0.0)])
            .is_empty()
    };
    assert!(!marks(&mut mesh, [200.0, 0.0, 3.0]), "far along the plane");
    assert!(
        marks(&mut mesh, [66.0, 0.0, 3.0]),
        "2 past the edge: 4 + 9 < 25"
    );
    assert!(
        marks(&mut mesh, [68.0, 0.0, 3.0]),
        "taken off: the mark before"
    );
    assert!(
        !marks(&mut mesh, [68.0, 0.0, 3.0]),
        "4 past the edge: 16 + 9 is not < 25"
    );
    assert!(marks(&mut mesh, [32.0, 16.0, 3.0]), "over the face");
}

// spec REND-7.1: a light is pushed through the nodes: a face is marked only when the
// light's sphere straddles its node's plane; only the first 64 lights count, and a light of
// radius 0 is skipped
#[test]
fn lights_mark_the_faces_their_sphere_straddles() {
    let mut mesh = build(&MapSpec::default(), &samples(&[50]));
    // Wholly in front of the plane: only the front child, a leaf, is visited.
    assert!(
        mesh.relight_with(&LightStyles::unset(), &[light([0.0, 0.0, 10.0], 5.0, 0.0)])
            .is_empty()
    );
    assert_eq!(texel(&mesh, 0, 0)[0], 100);
    // Wholly behind it: the back child only.
    assert!(
        mesh.relight_with(&LightStyles::unset(), &[light([0.0, 0.0, -10.0], 5.0, 0.0)])
            .is_empty()
    );
    // Straddling it: marked, from behind as well as in front.
    assert!(
        !mesh
            .relight_with(&LightStyles::unset(), &[light([0.0, 0.0, -3.0], 5.0, 0.0)])
            .is_empty()
    );
    assert_eq!(texel(&mesh, 0, 0)[0], 104);
    // A light of radius 0 is skipped: the mark of the frame before is taken off.
    assert!(
        !mesh
            .relight_with(&LightStyles::unset(), &[light([0.0, 0.0, 3.0], 0.0, 0.0)])
            .is_empty()
    );
    assert_eq!(texel(&mesh, 0, 0)[0], 100);
    let mut far = vec![light([0.0, 0.0, 1000.0], 1.0, 0.0); 64];
    far.push(light([0.0, 0.0, 3.0], 5.0, 0.0));
    assert!(
        mesh.relight_with(&LightStyles::unset(), &far).is_empty(),
        "the 65th light is not pushed"
    );
    assert_eq!(texel(&mesh, 0, 0)[0], 100);
    // A map with no lighting lump stays full bright (REND-1.5), and a face with no samples is
    // lit by the lights alone.
    let mut full = build(&MapSpec::default(), &[]);
    assert!(
        !full
            .relight_with(&LightStyles::unset(), &[light([0.0, 0.0, 3.0], 5.0, 0.0)])
            .is_empty()
    );
    assert_eq!(texel(&full, 0, 0)[0], 255);
    let dark = MapSpec {
        lightofs: -1,
        ..MapSpec::default()
    };
    let mut dark = build(&dark, &samples(&[50]));
    assert!(
        !dark
            .relight_with(&LightStyles::unset(), &[light([0.0, 0.0, 3.0], 5.0, 0.0)])
            .is_empty()
    );
    assert_eq!(texel(&dark, 0, 0)[0], 4);
}

// spec REND-5.5: the atlas is made again when a light marks a face, and once more when
// the last light leaves
#[test]
fn relighting_follows_the_lights_as_they_come_and_go() {
    let mut mesh = build(&MapSpec::default(), &samples(&[50]));
    let first = mesh.lightmap.clone();
    let lights = [light([0.0, 0.0, 3.0], 5.0, 0.0)];
    assert!(!mesh.relight_with(&LightStyles::unset(), &lights).is_empty());
    assert!(
        !mesh.relight_with(&LightStyles::unset(), &lights).is_empty(),
        "every frame while lit"
    );
    assert!(
        !mesh.relight_with(&LightStyles::unset(), &[]).is_empty(),
        "taken off"
    );
    assert_eq!(mesh.lightmap, first);
    assert!(mesh.relight_with(&LightStyles::unset(), &[]).is_empty());
    assert!(
        !mesh.relight_with(&style(0, b"a"), &[]).is_empty(),
        "a style still counts"
    );
}

// spec REND-7.5: a brush model's own faces are marked from its own first node with the
// light's world origin, wherever the entity stands; an instanced model, `firstmodelsurface`
// 0, is not marked
#[test]
fn brush_models_are_lit_through_their_own_nodes() {
    let lighting = samples(&[50]);
    let mut bsp = bsp(&MapSpec::default(), &lighting);
    // A second face, node and model, the model's `firstface` 1.
    bsp.faces.push(bsp.faces[0]);
    bsp.nodes.push(Node {
        firstface: 1,
        ..bsp.nodes[0]
    });
    bsp.models.push(Model {
        headnode: [1, 0, 0, 0],
        firstface: 1,
        ..bsp.models[0]
    });
    let moved = Placement {
        model: 1,
        origin: [1000.0, 0.0, 0.0],
        angles: [0.0; 3],
    };
    let palette = palette();
    let mut builder = MeshBuilder::new(&palette, Look::GlQuake);
    builder.add(&bsp, &[Placement::WORLD, moved]).unwrap();
    // The world's BSP again as an instanced model, its model 0 with `firstface` 0.
    let instanced = Placement {
        model: 0,
        origin: [2000.0, 0.0, 0.0],
        angles: [0.0; 3],
    };
    builder.add(&bsp, &[instanced]).unwrap();
    let mut mesh = builder.finish();
    assert!(
        !mesh
            .relight_with(&LightStyles::unset(), &[light([0.0, 0.0, 3.0], 5.0, 0.0)])
            .is_empty()
    );
    let width = usize::try_from(mesh.lightmap.width).unwrap();
    let column = |column: usize| mesh.lightmap.rgba[(column) * 4];
    // The faces sit side by side in the atlas's first row: the world's at column 1, the
    // moved model's at 6 and the instanced model's at 11.
    assert_eq!(column(1), 104, "the world");
    assert_eq!(column(6), 104, "the moved model, lit as if at the origin");
    assert_eq!(column(11), 100, "the instanced model");
    assert!(width >= 16);
}

// spec REND-7.3: `R_LightPoint` is not changed by the dynamic lights
#[test]
fn light_point_ignores_the_dynamic_lights() {
    let lighting = samples(&[50]);
    let bsp = bsp(&MapSpec::default(), &lighting);
    let mut mesh = MapMesh::build(&bsp, &palette(), Look::GlQuake).unwrap();
    let before = light_point(&bsp, &LightStyles::unset(), [8.0, 8.0, 10.0]);
    assert!(
        !mesh
            .relight_with(&LightStyles::unset(), &[light([0.0, 0.0, 3.0], 5.0, 0.0)])
            .is_empty()
    );
    assert_eq!(texel(&mesh, 0, 0)[0], 104);
    let after = light_point(&bsp, &LightStyles::unset(), [8.0, 8.0, 10.0]);
    assert_eq!((before, after), (50, 50));
}
