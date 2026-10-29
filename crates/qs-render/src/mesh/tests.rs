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
