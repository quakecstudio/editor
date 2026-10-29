// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::float_cmp,
    reason = "test code: a failed unwrap or index is a failed test; exact inputs give exact floats"
)]

use qs_bsp::{
    BSP_HEADER_LUMPS, BSP_MAX_MAP_HULLS, BSP_MIPTEX_HEADER_SIZE, Bsp, BspVersion, Face, Lump,
    Model, SURF_DRAWSKY, SURF_DRAWTURB, Texinfo, Texture,
};

use super::{MapMesh, MeshError, Palette};

/// One face of a test map: a 64 by 32 rectangle in the z = 0 plane, lit by `lighting`.
struct MapSpec {
    /// The face's surfedges, around the rectangle.
    surfedges: Vec<i32>,
    /// The face's flags.
    flags: u32,
    /// The face's styles.
    styles: [u8; 4],
    /// Where its samples start in the lighting lump; -1 for none.
    lightofs: i32,
    /// Whether the texinfo names a texture.
    textured: bool,
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
const LIGHTMAP: [usize; 2] = [5, 3];

/// A palette whose colour `i` is `(i, 2i, 3i)` wrapped.
fn palette() -> Palette {
    let bytes: Vec<u8> = (0..=255_u8)
        .flat_map(|i| [i, i.wrapping_mul(2), i.wrapping_mul(3)])
        .collect();
    Palette::read(&bytes).unwrap()
}

/// The texture's pixels, 16 by 16 of index 7, after its 40-byte header in the BSP.
const PIXELS: [u8; 256] = [7; 256];

fn bsp<'a>(spec: &MapSpec, lighting: &'a [u8]) -> Bsp<'a> {
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
        planes: Vec::new(),
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
        nodes: Vec::new(),
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
fn samples(values: &[u8]) -> Vec<u8> {
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

// spec REND-1.4: a texel is the sum of each style's sample times 256, shifted right by 7 and
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
