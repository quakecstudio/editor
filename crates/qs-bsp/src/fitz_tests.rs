// SPDX-License-Identifier: GPL-3.0-or-later

//! The FitzQuake lineage's loader, the default (BSP-20): the vectors of
//! `docs/spec/vectors/bsp/` that it loads differently from GLQuake's, each giving the result
//! its README records for QuakeSpasm's `Mod_LoadBrushModel`.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap or index is a failed test; exact values are the point"
)]

use proptest::prelude::*;

use super::fixtures::{BspSpec, NodeSpec, TextureSpec, leaf, square, texture};
use super::lit::grey;
use super::tests::{ContentsLeaf, contents_leaf};
use super::*;

fn read_with(spec: &BspSpec, loader: BspLoader) -> Result<Bsp<'static>, BspError> {
    Bsp::read(Vec::leak(spec.build()), loader)
}

fn fitz(spec: &BspSpec) -> Result<Bsp<'static>, BspError> {
    read_with(spec, BspLoader::default())
}

fn gl(spec: &BspSpec) -> Result<Bsp<'static>, BspError> {
    read_with(spec, BspLoader::GlQuake)
}

fn with_texture(name: &'static [u8], flags: i32) -> BspSpec {
    let mut spec = BspSpec {
        textures: Some(vec![texture(name)]),
        ..BspSpec::default()
    };
    spec.texinfo[0].flags = flags;
    spec
}

fn sized_texture(width: u32, height: u32) -> BspSpec {
    BspSpec {
        textures: Some(vec![Some(TextureSpec {
            name: b"wall",
            width,
            height,
            pixel: 7,
        })]),
        ..BspSpec::default()
    }
}

/// The base with its square's side `side` and the light samples a face of that size needs.
fn square_face(side: u16) -> BspSpec {
    let samples = usize::from((side >> 4) + 1);
    BspSpec {
        vertexes: square(f32::from(side)),
        lighting: vec![0; samples * samples],
        ..BspSpec::default()
    }
}

/// The base with `count` nodes: node 0 has children `children`, and every other node has
/// leaf 0 on both sides.
fn many_nodes(count: usize, children: [i32; 2]) -> BspSpec {
    let mut spec = BspSpec::default();
    let mut node = spec.nodes[0].clone();
    node.numfaces = 0;
    node.children = [-1, -1];
    spec.nodes = vec![node; count];
    spec.nodes[0] = NodeSpec {
        children,
        ..BspSpec::default().nodes[0].clone()
    };
    spec
}

/// The vectors of `docs/spec/vectors/bsp/` written for BSP-20, by name.
pub(crate) fn fitz_vectors() -> Vec<(&'static str, BspSpec)> {
    let mut vectors = vec![
        ("extents-1984", square_face(1984)),
        ("extents-2016", square_face(2016)),
        ("texture-8x8", sized_texture(8, 8)),
        ("texture-zero", sized_texture(0, 16)),
        ("sky-upper", with_texture(b"SKY1", 0)),
        ("turb-lit", with_texture(b"*water", 0)),
        ("turb-lava", with_texture(b"*lava1", 1)),
        ("turb-slime", with_texture(b"!slime0", 1)),
        ("turb-tele", with_texture(b"*teleport", 1)),
        ("fence", with_texture(b"{grate", 0)),
        ("texinfo-flag-2", with_texture(b"wall", 2)),
        ("leafs-32768", leaves(32768)),
        ("nodes-32769", many_nodes(32769, [32768, -2])),
    ];
    let mut spec = BspSpec::default();
    spec.texinfo[0].vecs[0][3] = 2_000_000.0;
    vectors.push(("far-offset", spec));
    let mut spec = BspSpec::default();
    spec.faces[0].numedges = 0;
    vectors.push(("no-edges", spec));
    vectors.push(("double-sum", double_sum()));
    let mut spec = BspSpec::default();
    spec.texinfo[0].miptex = 2;
    vectors.push(("miptex-past", spec));
    let mut spec = BspSpec {
        textures: Some(vec![None]),
        ..BspSpec::default()
    };
    spec.faces[0].lightofs = -1;
    vectors.push(("missing-unlit", spec));
    let mut spec = BspSpec::default();
    spec.nodes[0].children = [-3, -1];
    vectors.push(("node-bad-leaf", spec));
    let mut spec = BspSpec::default();
    spec.clipnodes[0].1 = [5, -2];
    vectors.push(("clipnode-past", spec));
    let mut spec = BspSpec {
        version: BspVersion::Bsp2,
        ..BspSpec::default()
    };
    spec.nodes[0].children = [0, -1];
    vectors.push(("bsp2-child-0", spec));
    let mut spec = BspSpec {
        version: BspVersion::Bsp2,
        ..BspSpec::default()
    };
    spec.nodes[0].children = [-5, -1];
    vectors.push(("bsp2-bad-leaf", spec));
    vectors
}

/// The base with `count` leaves, the extra ones solid with no faces.
fn leaves(count: usize) -> BspSpec {
    let mut spec = BspSpec::default();
    spec.leafs.resize(count, leaf(-2, -1, 0, 0));
    spec
}

/// The base with vertex 0 at the origin and the others at (1, 1, 1), and an s axis whose
/// coordinate at (1, 1, 1), 16 + 2^-20 + 2^-26, rounds to 16 + 2^-19 from double precision
/// and to 16 when added in single precision: the `double-sum` vector.
fn double_sum() -> BspSpec {
    let mut spec = BspSpec {
        vertexes: vec![[0.0; 3], [1.0; 3], [1.0; 3], [1.0; 3]],
        ..BspSpec::default()
    };
    spec.texinfo[0].vecs[0] = [16.0, 2.0_f32.powi(-20), 2.0_f32.powi(-26), 0.0];
    spec
}

fn vector(name: &str) -> BspSpec {
    fitz_vectors()
        .into_iter()
        .find(|(vector, _)| *vector == name)
        .unwrap()
        .1
}

// spec BSP-20.1: the default loader is the FitzQuake lineage's; the base loads with it to the
// same faces, leaves, nodes, hulls and models as with GLQuake's.
#[test]
fn default_is_fitzquake() {
    assert_eq!(BspLoader::default(), BspLoader::FitzQuake);
    assert_eq!(BspLoader::FitzQuake.max_surface_extent(), 2000);
    let base = BspSpec::default();
    let fitz = fitz(&base).unwrap();
    let gl = gl(&base).unwrap();
    assert_eq!(fitz.faces, gl.faces);
    assert_eq!(fitz.texinfo, gl.texinfo);
    assert_eq!(fitz.leafs, gl.leafs);
    assert_eq!(fitz.nodes, gl.nodes);
    assert_eq!(fitz.clipnodes, gl.clipnodes);
    assert_eq!(fitz.hull0(), gl.hull0());
    assert_eq!(fitz.models, gl.models);
    assert_eq!(fitz.entities(), gl.entities());
    assert_eq!(fitz.leaf_pvs(1), gl.leaf_pvs(1));
}

// spec BSP-20.2: QuakeSpasm's other versions, Half-Life's 30 and Quake 64's, fail as BSP-1.2
// says with every loader.
#[test]
fn other_versions() {
    let q64 = i32::from_le_bytes(*b" 46Q");
    for version in [30, q64] {
        let spec = BspSpec {
            raw_version: Some(version),
            ..BspSpec::default()
        };
        assert_eq!(fitz(&spec), Err(BspError::WrongVersion { version }));
    }
}

// spec BSP-20.3: a texture of width or height 0 leaves its slot empty, so its texinfo's
// texture is missing (vector `texture-zero`); GLQuake loads it.
#[test]
fn zero_sized_texture() {
    let spec = vector("texture-zero");
    let bsp = fitz(&spec).unwrap();
    assert_eq!(bsp.textures, [None]);
    assert_eq!(
        (bsp.texinfo[0].texture, bsp.texinfo[0].flags),
        (None, BSP_TEX_MISSING)
    );
    assert_eq!(bsp.faces[0].flags, SURF_NOTEXTURE);
    let texture = gl(&spec).unwrap().textures[0].clone().unwrap();
    assert_eq!((texture.width, texture.height), (0, 16));
    assert_eq!(fitz(&sized_texture(16, 0)).unwrap().textures, [None]);
}

// spec BSP-20.4: a size that is not a multiple of 16 loads (`not-aligned`, `texture-8x8`),
// where GLQuake fails.
#[test]
fn unaligned_textures_load() {
    for (width, height) in [(24, 16), (8, 8), (16, 8)] {
        let spec = sized_texture(width, height);
        let texture = fitz(&spec).unwrap().textures[0].clone().unwrap();
        assert_eq!((texture.width, texture.height), (width, height));
        assert_eq!(
            texture.pixels.len(),
            usize::try_from(width * height).unwrap()
        );
        assert!(matches!(gl(&spec), Err(BspError::NotAligned { .. })));
    }
}

/// The base's bytes with the 32-bit value at `at` in the textures lump set to `value`.
fn patched_textures(at: usize, value: i32) -> &'static [u8] {
    let mut bytes = BspSpec::default().build();
    let lump = 236;
    bytes[lump + at..lump + at + 4].copy_from_slice(&value.to_le_bytes());
    Vec::leak(bytes)
}

// spec BSP-20.5: only level 0's width * height bytes are kept, whatever the offsets say, cut
// short at the end of the lump (`texture-past-lump`); a product over 2^31 - 1 fails.
#[test]
fn level_zero_only() {
    let bsp = fitz(&BspSpec::default()).unwrap();
    let wall = bsp.textures[0].as_ref().unwrap();
    assert_eq!(wall.pixels.len(), 256);
    assert_eq!(wall.mip(0).unwrap().len(), 256);
    assert!(wall.mip(1).is_none());
    let fitz = |bytes| Bsp::read(bytes, BspLoader::FitzQuake);
    // Mip offset 39 (in the header) and offsets past the lump are not read.
    for (at, value) in [(8 + 24, 39), (8 + 36, 9999)] {
        let bytes = patched_textures(at, value);
        assert!(fitz(bytes).is_ok());
        assert!(Bsp::read(bytes, BspLoader::GlQuake).is_err());
    }
    // Height 32: 512 bytes asked for, the 340 to the end of the lump kept.
    let bytes = patched_textures(8 + 20, 32);
    let texture = fitz(bytes).unwrap().textures[0].clone().unwrap();
    assert_eq!((texture.height, texture.pixels.len()), (32, 340));
    assert!(texture.mip(0).is_none());
    // A header past the lump still fails (BSP-4.6).
    assert_eq!(
        fitz(patched_textures(4, 389 - 40)).unwrap_err(),
        BspError::BadTexture { index: 0 }
    );
    // 65536 * 32768 is 2^31, past an int; 65536 * 65536 wraps to 0 as C's unsigned does.
    let mut bytes = patched_textures(8 + 16, 65536).to_vec();
    bytes[236 + 8 + 20..236 + 8 + 24].copy_from_slice(&32768_i32.to_le_bytes());
    assert_eq!(
        fitz(Vec::leak(bytes.clone())).unwrap_err(),
        BspError::BadTexture { index: 0 }
    );
    bytes[236 + 8 + 20..236 + 8 + 24].copy_from_slice(&65536_i32.to_le_bytes());
    let texture = fitz(Vec::leak(bytes)).unwrap().textures[0].clone().unwrap();
    assert!(texture.pixels.is_empty());
}

// spec BSP-20.6: `sky` in any case is a sky (`sky-upper`); GLQuake compares case included.
#[test]
fn sky_any_case() {
    for name in [&b"SKY1"[..], b"Sky1", b"sky"] {
        let spec = BspSpec {
            textures: Some(vec![Some(TextureSpec {
                name: Box::leak(name.to_vec().into_boxed_slice()),
                width: 16,
                height: 16,
                pixel: 7,
            })]),
            ..BspSpec::default()
        };
        let bsp = fitz(&spec).unwrap();
        assert!(bsp.textures[0].as_ref().unwrap().sky);
        assert_eq!(bsp.faces[0].flags, SURF_DRAWSKY | SURF_DRAWTILED);
    }
    let spec = vector("sky-upper");
    assert_eq!(gl(&spec).unwrap().faces[0].flags, 0);
    assert!(
        !fitz(&with_texture(b"sk", 0)).unwrap().textures[0]
            .as_ref()
            .unwrap()
            .sky
    );
    assert!(fitz_sky(b"sKy"));
    assert!(!fitz_sky(b"sk"));
}

// spec BSP-20.7: a miptex of nummiptex is the checkerboard with the flags kept (`bad-miptex`);
// one past it, or an empty slot, is missing and gets TEX_MISSING (`miptex-past`,
// `missing-texture`); a negative one fails; no mip bias is needed but one is still given.
#[test]
fn missing_textures() {
    assert_eq!(BSP_TEX_MISSING, 2);
    let mut spec = BspSpec::default();
    spec.texinfo[0].miptex = 1;
    spec.texinfo[0].flags = 4;
    let info = fitz(&spec).unwrap().texinfo[0];
    assert_eq!((info.texture, info.flags, info.mipadjust), (None, 4, 1));
    assert_eq!(gl(&spec).unwrap_err(), BspError::BadMiptex);
    let bsp = fitz(&vector("miptex-past")).unwrap();
    assert_eq!(
        (bsp.texinfo[0].texture, bsp.texinfo[0].flags),
        (None, BSP_TEX_MISSING)
    );
    let mut spec = BspSpec {
        textures: Some(vec![None]),
        ..BspSpec::default()
    };
    spec.texinfo[0].flags = 1;
    let info = fitz(&spec).unwrap().texinfo[0];
    assert_eq!((info.texture, info.flags), (None, 3));
    // No textures lump: slot 0 is the checkerboard, slot 1 missing.
    let mut spec = BspSpec {
        textures: None,
        ..BspSpec::default()
    };
    spec.texinfo[0].flags = 1;
    assert_eq!(fitz(&spec).unwrap().texinfo[0].flags, 1);
    spec.texinfo[0].miptex = 1;
    assert_eq!(fitz(&spec).unwrap().texinfo[0].flags, 3);
    spec.texinfo[0].miptex = -1;
    assert_eq!(fitz(&spec).unwrap_err(), BspError::BadMiptex);
}

// spec BSP-20.8: the bounds start at FLT_MAX and -FLT_MAX (`far-offset`, `no-edges`) and a NaN
// replaces neither (`nan-vertex`).
#[test]
fn bounds_from_flt_max() {
    let face = fitz(&vector("far-offset")).unwrap().faces[0];
    assert_eq!((face.texturemins, face.extents), ([-31616, 0], [64, 64]));
    assert_eq!(
        gl(&vector("far-offset")).unwrap_err(),
        BspError::BadSurfaceExtents
    );
    let face = fitz(&vector("no-edges")).unwrap().faces[0];
    assert_eq!((face.texturemins, face.extents), ([0, 0], [0, 0]));
    assert_eq!(face.lightmap_size, [1, 1]);
    assert_eq!(
        gl(&vector("no-edges")).unwrap_err(),
        BspError::BadSurfaceExtents
    );
    let mut spec = BspSpec::default();
    spec.vertexes[0] = [f32::NAN, 0.0, 0.0];
    let face = fitz(&spec).unwrap().faces[0];
    assert_eq!((face.texturemins, face.extents), ([64, 0], [0, 64]));
}

// spec BSP-20.9: coordinates are added in C's order in double precision (`double-sum`, and
// `x87-order`, where GLQuake's order gives another extent).
#[test]
#[expect(
    clippy::cast_possible_truncation,
    reason = "test code: rounding to single precision is the point"
)]
fn double_precision() {
    let s = [16.0, 2.0_f32.powi(-20), 2.0_f32.powi(-26), 0.0];
    assert_eq!(
        double_texture_coordinate([1.0; 3], s),
        16.0 + 2.0_f32.powi(-19)
    );
    assert_eq!(16.0_f32 + 2.0_f32.powi(-20) + 2.0_f32.powi(-26), 16.0);
    let face = fitz(&vector("double-sum")).unwrap().faces[0];
    assert_eq!((face.texturemins, face.extents), ([0, 0], [32, 16]));
    let mut spec = BspSpec {
        vertexes: vec![[1.0, 0.0, 1.0], [1.0; 3], [1.0; 3], [1.0; 3]],
        ..BspSpec::default()
    };
    spec.texinfo[0].vecs[0] = [
        2048.0,
        2.0_f32.powi(-42),
        -(1024.0 - 2.0_f32.powi(-14)),
        0.0,
    ];
    let face = fitz(&spec).unwrap().faces[0];
    assert_eq!((face.texturemins, face.extents), ([1024, 0], [0, 16]));
    assert_eq!(gl(&spec).unwrap().faces[0].extents, [16, 16]);
    let c_order = (2048.0_f64 + 2.0_f64.powi(-42) - (1024.0 - 2.0_f64.powi(-14))) as f32;
    assert_eq!(c_order, 1024.0);
}

// spec BSP-20.10: the limit is 2000, so 1984 loads and 2016 fails (`extents-1984`,
// `extents-2016`), against the stored 16-bit extent.
#[test]
fn extent_limit_2000() {
    assert_eq!(BSP_FITZ_MAX_SURFACE_EXTENT, 2000);
    let spec = vector("extents-1984");
    let face = fitz(&spec).unwrap().faces[0];
    assert_eq!(
        (face.extents, face.lightmap_size),
        ([1984, 1984], [125, 125])
    );
    assert_eq!(gl(&spec).unwrap_err(), BspError::BadSurfaceExtents);
    let spec = vector("extents-2016");
    for loader in [
        BspLoader::WinQuake,
        BspLoader::GlQuake,
        BspLoader::FitzQuake,
    ] {
        assert_eq!(
            read_with(&spec, loader).unwrap_err(),
            BspError::BadSurfaceExtents
        );
    }
    let mut spec = spec;
    spec.texinfo[0].flags = BSP_TEX_SPECIAL;
    assert_eq!(fitz(&spec).unwrap().faces[0].extents, [2016, 2016]);
    // 65536 + 1984 is stored as 1984 and loads.
    let mut spec = BspSpec::default();
    spec.texinfo[0].vecs[0] = [1055.0, 0.0, 0.0, 0.0];
    spec.faces[0].lightofs = -1;
    assert_eq!(fitz(&spec).unwrap().faces[0].extents, [1984, 64]);
}

/// Face 0's flags as the FitzQuake lineage and GLQuake give them.
fn flags(spec: &BspSpec) -> (u32, u32) {
    (
        fitz(spec).unwrap().faces[0].flags,
        gl(spec).unwrap().faces[0].flags,
    )
}

// spec BSP-20.11: liquids keep their extents and lightmap, are tiled only with TEX_SPECIAL and
// are told apart by name; `{` is a fence; a missing texture is flagged, and tiled when unlit.
#[test]
fn face_flags() {
    assert_eq!(
        [
            SURF_NOTEXTURE,
            SURF_DRAWFENCE,
            SURF_DRAWLAVA,
            SURF_DRAWSLIME,
            SURF_DRAWTELE,
            SURF_DRAWWATER
        ],
        [0x100, 0x200, 0x400, 0x800, 0x1000, 0x2000]
    );
    let bsp = fitz(&with_texture(b"*water", 1)).unwrap();
    let face = bsp.faces[0];
    assert_eq!(face.flags, SURF_DRAWTURB | SURF_DRAWTILED | SURF_DRAWWATER);
    assert_eq!((face.texturemins, face.extents), ([0, 0], [64, 64]));
    assert_eq!(face.lightmap_size, [5, 5]);
    assert_eq!(flags(&vector("turb-lit")), (0x2010, 0x30));
    assert_eq!(flags(&vector("turb-lava")), (0x430, 0x30));
    assert_eq!(fitz(&vector("turb-slime")).unwrap().faces[0].flags, 0x830);
    assert_eq!(flags(&vector("turb-tele")), (0x1030, 0x30));
    assert_eq!(flags(&with_texture(b"*Lava", 1)), (0x2030, 0x30));
    assert_eq!(flags(&vector("fence")), (SURF_DRAWFENCE, 0));
    assert_eq!(flags(&vector("texinfo-flag-2")), (SURF_NOTEXTURE, 0));
    assert_eq!(
        fitz(&vector("missing-unlit")).unwrap().faces[0].flags,
        SURF_NOTEXTURE | SURF_DRAWTILED
    );
    let mut spec = with_texture(b"sky1", BSP_TEX_MISSING);
    spec.faces[0].side = 1;
    assert_eq!(
        fitz(&spec).unwrap().faces[0].flags,
        SURF_PLANEBACK | SURF_DRAWSKY | SURF_DRAWTILED
    );
    // A lit liquid's samples must lie in the lump, as any lit face's (BSP-10.2).
    let mut spec = with_texture(b"*water", 0);
    spec.lighting.truncate(24);
    assert!(fitz(&spec).is_err());
}

// spec BSP-20.12: no face gets SURF_UNDERWATER (`water-leaf`, `water-leaf-turb`,
// `solid-leaf-face`, `contents-unknown`).
#[test]
fn no_underwater_flag() {
    for contents in [BSP_CONTENTS_WATER, BSP_CONTENTS_SOLID, -7] {
        let mut spec = BspSpec::default();
        spec.leafs[1].contents = contents;
        spec.leafs[0].nummarksurfaces = 1;
        assert_eq!(flags(&spec), (0, SURF_UNDERWATER));
    }
}

// spec BSP-20.13: a BSP29 file of more than 32767 leaves fails (`leafs-32768`); BSP2 has no
// limit.
#[test]
fn leaf_limit() {
    assert_eq!(BSP_FITZ_MAX_LEAFS, 32767);
    assert_eq!(fitz(&leaves(32767)).unwrap().leafs.len(), 32767);
    let spec = vector("leafs-32768");
    let error = fitz(&spec).unwrap_err();
    assert_eq!(error, BspError::TooManyLeafs { count: 32768 });
    assert_eq!(
        error.message("maps/test.bsp"),
        "Mod_LoadLeafs: 32768 leafs exceeds limit of 32767."
    );
    assert_eq!(gl(&spec).unwrap().leafs.len(), 32768);
    let spec = BspSpec {
        version: BspVersion::Bsp2,
        ..leaves(32768)
    };
    assert_eq!(fitz(&spec).unwrap().leafs.len(), 32768);
}

// spec BSP-20.14: a leaf's marksurfaces and the marksurfaces lump are read unsigned, as for
// every loader (BSP-12.5, BSP-13.1).
#[test]
fn unsigned_marksurfaces() {
    let mut spec = BspSpec {
        marksurfaces: vec![0; 32769],
        ..BspSpec::default()
    };
    spec.leafs[1].firstmarksurface = 32768;
    assert_eq!(fitz(&spec).unwrap().leafs[1].firstmarksurface, 32768);
    assert_eq!(fitz(&spec).unwrap().leafs, gl(&spec).unwrap().leafs);
    spec.marksurfaces = vec![0xFFFF];
    spec.leafs[1].firstmarksurface = 0;
    assert_eq!(fitz(&spec).unwrap_err(), BspError::BadMarksurface);
}

// spec BSP-20.15: a BSP29 node child is unsigned, a node below the count, else leaf
// 65535 - child, and leaf 0 when that leaf does not exist (`nodes-32769`, `node-bad-leaf`).
#[test]
fn bsp29_node_children() {
    let bsp = fitz(&vector("node-bad-leaf")).unwrap();
    assert_eq!(bsp.nodes[0].children, [-1, -1]);
    assert_eq!(
        bsp.hull0()[0].children,
        [BSP_CONTENTS_SOLID, BSP_CONTENTS_SOLID]
    );
    assert!(gl(&vector("node-bad-leaf")).is_err());
    let spec = vector("nodes-32769");
    let bsp = fitz(&spec).unwrap();
    assert_eq!(bsp.nodes[0].children, [32768, -2]);
    assert_eq!(bsp.hull0()[0].children, [32768, BSP_CONTENTS_EMPTY]);
    assert!(gl(&spec).is_err());
    let child = |stored, nodes| fitz_node_child(stored, BspVersion::Bsp29, nodes, 2);
    assert_eq!(child(-1, 1), -1);
    assert_eq!(child(-2, 1), -2);
    assert_eq!(child(-3, 1), -1);
    assert_eq!(child(-1, 65535), -1);
    assert_eq!(child(-2, 65535), 65534);
    assert_eq!(child(0, 1), 0);
    assert_eq!(child(1, 1), -1);
}

// spec BSP-20.16: a 2PSB child is a node from 0, a BSP2 child from 1, so a BSP2 child of 0 is
// leaf 0 (`bsp2-child-0`); a leaf that does not exist is leaf 0 (`bsp2-bad-leaf`).
#[test]
fn bsp2_node_children() {
    let mut spec = BspSpec {
        version: BspVersion::Bsp2Psb,
        ..BspSpec::default()
    };
    spec.nodes[0].children = [0, -1];
    assert_eq!(fitz(&spec).unwrap().nodes[0].children, [0, -1]);
    let bsp2 = fitz(&vector("bsp2-child-0")).unwrap();
    assert_eq!(bsp2.nodes[0].children, [-1, -1]);
    assert_eq!(
        gl(&vector("bsp2-child-0")).unwrap().nodes[0].children,
        [0, -1]
    );
    let bad = fitz(&vector("bsp2-bad-leaf")).unwrap();
    assert_eq!(bad.nodes[0].children, [-1, -1]);
    assert!(gl(&vector("bsp2-bad-leaf")).is_err());
    let child = |stored, version| fitz_node_child(stored, version, 2, 3);
    assert_eq!(child(1, BspVersion::Bsp2), 1);
    assert_eq!(child(2, BspVersion::Bsp2Psb), -1);
    assert_eq!(child(-3, BspVersion::Bsp2), -3);
    assert_eq!(child(i32::MIN, BspVersion::Bsp2Psb), -1);
}

// spec BSP-20.17: a BSP29 clipnode child is unsigned, less 65536 when not below the count, so
// no child fails (`clipnode-past`); BSP2's are signed and checked.
#[test]
fn bsp29_clipnode_children() {
    let spec = vector("clipnode-past");
    assert_eq!(fitz(&spec).unwrap().clipnodes[0].children, [5 - 65536, -2]);
    assert!(gl(&spec).is_err());
    assert_eq!(fitz_clipnode_child(-1, 1), -1);
    assert_eq!(fitz_clipnode_child(-32768, 32769), 32768);
    assert_eq!(fitz_clipnode_child(-32768, 32768), -32768);
    let spec = BspSpec {
        version: BspVersion::Bsp2,
        ..spec
    };
    assert!(fitz(&spec).is_err());
}

// spec BSP-20.18: the rest is as BSP-1 to BSP-17: id's errors, visibility rows and models.
#[test]
fn otherwise_as_glquake() {
    let spec = BspSpec {
        raw_version: Some(28),
        ..BspSpec::default()
    };
    assert_eq!(
        fitz(&spec).unwrap_err(),
        BspError::WrongVersion { version: 28 }
    );
    let spec = BspSpec {
        vertex_padding: 1,
        ..BspSpec::default()
    };
    assert_eq!(
        fitz(&spec).unwrap_err(),
        BspError::FunnyLumpSize {
            lump: LumpId::Vertexes
        }
    );
    let spec = BspSpec {
        textures: Some(vec![texture(b"+0a"), texture(b"+2a")]),
        ..BspSpec::default()
    };
    assert_eq!(
        fitz(&spec).unwrap_err().message("m"),
        "Missing frame 1 of +0a"
    );
    let mut spec = BspSpec::default();
    spec.models[0].visleafs = 16;
    spec.visibility = vec![0x00, 0x00, 0x07, 0x08, 0x00];
    assert_eq!(fitz(&spec).unwrap().leaf_pvs(1).unwrap(), [0x07, 0x08]);
}

// spec BSP-18.6: a `.lit` applies to a map the default loader read.
#[test]
fn lit_with_default_loader() {
    let bsp = fitz(&BspSpec::default()).unwrap();
    assert_eq!(grey(bsp.lighting).len(), 25);
}

// spec BSP-20.19: as in id's loaders, a node naming a leaf of contents 0 or more fails
// (`contents-0`, `contents-0-submodel`), including leaf 0 named in place of a leaf that does not
// exist (BSP-20.15) and a BSP2 node's leaf.
#[test]
fn leaf_contents_0_or_more() {
    for place in [ContentsLeaf::Named, ContentsLeaf::Submodel] {
        assert!(matches!(
            fitz(&contents_leaf(0, place)),
            Err(BspError::LeafContents { contents: 0, .. })
        ));
    }
    assert_eq!(
        fitz(&contents_leaf(0, ContentsLeaf::Unnamed))
            .unwrap()
            .leafs[2]
            .contents,
        0
    );
    let mut spec = BspSpec::default();
    spec.leafs[0].contents = 0;
    spec.nodes[0].children = [-3, -2];
    assert_eq!(
        fitz(&spec),
        Err(BspError::LeafContents {
            node: 0,
            leaf: 0,
            contents: 0
        })
    );
    let spec = BspSpec {
        version: BspVersion::Bsp2,
        ..contents_leaf(5, ContentsLeaf::Named)
    };
    assert_eq!(
        fitz(&spec),
        Err(BspError::LeafContents {
            node: 0,
            leaf: 1,
            contents: 5
        })
    );
}

// spec BSP-17.6: both BSP2 variants of the base load with the FitzQuake lineage's rules.
#[test]
fn bsp2_with_default_loader() {
    for version in [BspVersion::Bsp2Psb, BspVersion::Bsp2] {
        let spec = BspSpec {
            version,
            ..BspSpec::default()
        };
        let bsp = fitz(&spec).unwrap();
        assert_eq!(bsp.faces, gl(&spec).unwrap().faces);
        assert_eq!(bsp.nodes[0].children, [-2, -1]);
    }
}

proptest! {
    // spec BSP-1.6, BSP-20.1: any bytes, and the base with bytes changed, load or fail with
    // an error under the default loader, never a panic.
    #[test]
    fn arbitrary_bytes_default(bytes in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let mut base = BspSpec::default().build();
        let at = usize::from(bytes.first().copied().unwrap_or(0)) * 4 % base.len();
        for (offset, byte) in bytes.iter().skip(1).take(16).enumerate() {
            let index = (at + offset) % base.len();
            base[index] = *byte;
        }
        for input in [&bytes, &base] {
            if let Ok(bsp) = Bsp::read(input, BspLoader::default()) {
                for leaf in 0..bsp.leafs.len().min(64) {
                    let _row = bsp.leaf_pvs(leaf);
                }
                for face in &bsp.faces {
                    let _samples = bsp.face_samples(face);
                }
                let _hull = bsp.hull0();
            }
        }
    }
}
