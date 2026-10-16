// SPDX-License-Identifier: GPL-3.0-or-later

//! The BSP vectors of `docs/spec/vectors/bsp/`, each giving the result its README records for
//! id's `Mod_LoadBrushModel`, and the statements the vectors do not reach.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap or index is a failed test; exact values are the point"
)]

use proptest::prelude::*;
use qc_progs::crc::crc_block;

use super::fixtures::{BspSpec, FaceSpec, TextureSpec, from_xxd, leaf, square, texture};
use super::lit::{companion_applies, companion_name, grey};
use super::*;

const BASE_XXD: &str = include_str!("../../../docs/spec/vectors/bsp/base.txt");

fn read(spec: &BspSpec) -> Result<Bsp<'static>, BspError> {
    let bytes: &'static [u8] = Vec::leak(spec.build());
    Bsp::read(bytes, BspLoader::WinQuake)
}

fn error(spec: &BspSpec) -> String {
    read(spec).unwrap_err().message("maps/test.bsp")
}

fn with_textures(names: &[&'static [u8]]) -> BspSpec {
    BspSpec {
        textures: Some(names.iter().map(|name| texture(name)).collect()),
        ..BspSpec::default()
    }
}

/// The base with its square's side `side` and `lighting` bytes of light.
fn sized(side: f32, lighting: usize) -> BspSpec {
    BspSpec {
        vertexes: square(side),
        lighting: vec![0; lighting],
        ..BspSpec::default()
    }
}

// spec BSP-1.1, BSP-1.3: the factory's default is the base vector byte for byte.
#[test]
fn factory_builds_the_base_vector() {
    let base = from_xxd(BASE_XXD);
    assert_eq!(base.len(), 1020);
    assert_eq!(BspSpec::default().build(), base);
    assert_eq!(BSP_HEADER_SIZE, 124);
    assert_eq!(BSP_HEADER_LUMPS, 15);
    let bsp = Bsp::read(&base, BspLoader::WinQuake).unwrap();
    assert_eq!(bsp.version, BspVersion::Bsp29);
    assert_eq!(
        bsp.lumps[BSP_LUMP_MODELS],
        Lump {
            fileofs: 892,
            filelen: 128
        }
    );
    assert_eq!(
        [
            BSP_LUMP_ENTITIES,
            BSP_LUMP_PLANES,
            BSP_LUMP_TEXTURES,
            BSP_LUMP_VERTEXES,
            BSP_LUMP_VISIBILITY,
            BSP_LUMP_NODES,
            BSP_LUMP_TEXINFO,
            BSP_LUMP_FACES,
            BSP_LUMP_LIGHTING,
            BSP_LUMP_CLIPNODES,
            BSP_LUMP_LEAFS,
            BSP_LUMP_MARKSURFACES,
            BSP_LUMP_EDGES,
            BSP_LUMP_SURFEDGES,
            BSP_LUMP_MODELS
        ],
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14]
    );
    assert_eq!(lump_name(14), "models");
    assert_eq!(lump_name(15), "unknown");
}

// spec BSP-2.1, BSP-3.1, BSP-4.3, BSP-5.1, BSP-7.1, BSP-7.2, BSP-8.1, BSP-9.1, BSP-9.2, BSP-9.10:
// spec BSP-9.6, BSP-9.7, BSP-11.1, BSP-12.1, BSP-13.1, BSP-14.1, BSP-14.2:
// the base vector loads as the README records.
#[test]
fn base_loads() {
    let bsp = read(&BspSpec::default()).unwrap();
    assert!(
        bsp.entities()
            .starts_with(b"{\n\"classname\" \"worldspawn\"")
    );
    assert_eq!(bsp.entities().len(), 91);
    assert_eq!(bsp.planes[0].normal, [0.0, 0.0, 1.0]);
    assert_eq!((bsp.planes[0].dist, bsp.planes[0].plane_type), (0.0, 2));
    let wall = bsp.textures[0].as_ref().unwrap();
    assert_eq!(
        (wall.name.as_slice(), wall.width, wall.height),
        (&b"wall"[..], 16, 16)
    );
    assert_eq!(wall.offsets, [40, 296, 360, 376]);
    assert_eq!(wall.pixels.len(), 340);
    assert!(wall.pixels.iter().all(|&pixel| pixel == 7));
    assert_eq!(wall.mip(3).unwrap().len(), 4);
    assert!(wall.mip(4).is_none());
    assert_eq!(bsp.vertexes, square(64.0));
    let node = bsp.nodes[0];
    assert_eq!((node.planenum, node.children), (0, [-2, -1]));
    assert_eq!((node.mins, node.maxs), ([0.0; 3], [64.0, 64.0, 0.0]));
    assert_eq!((node.firstface, node.numfaces), (0, 1));
    let face = bsp.faces[0];
    assert_eq!(face.texturemins, [0, 0]);
    assert_eq!(face.extents, [64, 64]);
    assert_eq!(face.lightmap_size, [5, 5]);
    assert_eq!(face.style_count(), 1);
    assert_eq!(face.flags, 0);
    assert_eq!(
        bsp.face_samples(&face).unwrap(),
        (0..25).collect::<Vec<u8>>()
    );
    // The last surfedge, -4, runs edge 4 (3, 0) backwards, so it gives vertex 0 again.
    let corners = square(64.0);
    assert_eq!(
        bsp.face_vertices(&face),
        [corners[0], corners[1], corners[2], corners[0]]
    );
    let texinfo = bsp.texinfo[0];
    assert_eq!(
        (texinfo.texture, texinfo.flags, texinfo.mipadjust),
        (Some(0), 0, 1)
    );
    assert_eq!(bsp.planes[0].signbits, 0);
    assert_eq!(
        bsp.clipnodes,
        [ClipNode {
            planenum: 0,
            children: [-1, -2]
        }]
    );
    assert_eq!(bsp.leafs.len(), 2);
    assert_eq!((bsp.leafs[1].contents, bsp.leafs[1].visofs), (-1, 0));
    assert_eq!(
        (bsp.leafs[1].firstmarksurface, bsp.leafs[1].nummarksurfaces),
        (0, 1)
    );
    assert_eq!(bsp.leafs[1].ambient_level, [0, 1, 2, 3]);
    assert_eq!(bsp.marksurfaces, [0]);
    assert_eq!(bsp.edges[0], [0, 0]);
    assert_eq!(bsp.surfedges, [1, 2, 3, -4]);
}

// spec BSP-6.2, BSP-6.4, BSP-11.2, BSP-11.3, BSP-12.3, BSP-15.1, BSP-15.2, BSP-15.3, BSP-15.4:
// hulls, models, radii and visibility rows of the base.
#[test]
fn base_models_and_hulls() {
    let bsp = read(&BspSpec::default()).unwrap();
    let world = bsp.models[0];
    assert_eq!((world.mins, world.maxs), ([-1.0; 3], [65.0, 65.0, 1.0]));
    assert!((world.radius - 91.9293).abs() < 1e-4);
    assert_eq!(world.visleafs, 1);
    let door = bsp.models[1];
    assert_eq!(door.maxs, [65.0, 65.0, 9.0]);
    assert!((door.radius - 92.3634).abs() < 1e-4);
    assert_eq!(model_name(1), "*1");
    assert_eq!(model_precache_index(0), 1);
    assert_eq!(model_precache_index(1), 2);
    assert_eq!(
        (BSP_HULL1_MINS, BSP_HULL1_MAXS),
        ([-16.0, -16.0, -24.0], [16.0, 16.0, 32.0])
    );
    assert_eq!(
        (BSP_HULL2_MINS, BSP_HULL2_MAXS),
        ([-32.0, -32.0, -24.0], [32.0, 32.0, 64.0])
    );
    assert_eq!(world.headnode[1], 0);
    assert_eq!(bsp.clipnodes.len(), 1);
    assert_eq!(BSP_MAX_MAP_HULLS, 4);
    assert_eq!(
        [hull_for_size(2.9), hull_for_size(3.0), hull_for_size(32.0)],
        [0, 1, 1]
    );
    assert_eq!(hull_for_size(32.5), 2);
    assert_eq!(bsp.leafs[0].contents, BSP_CONTENTS_SOLID);
    assert_eq!(bsp.vis_row_size(), 1);
    assert_eq!(bsp.leaf_pvs(0).unwrap(), [0xFF]);
    assert_eq!(bsp.leaf_pvs(1).unwrap(), [0x01]);
    assert!(bsp.leaf_pvs(2).is_err());
}

// spec BSP-7.5: hull 0 is the nodes with leaf children replaced by their contents.
#[test]
fn hull0() {
    let bsp = read(&BspSpec::default()).unwrap();
    assert_eq!(
        bsp.hull0(),
        [ClipNode {
            planenum: 0,
            children: [-1, -2]
        }]
    );
}

// spec BSP-1.2: version 30 fails; 2PSB and BSP2 are the other versions.
#[test]
fn version() {
    let spec = BspSpec {
        raw_version: Some(30),
        ..BspSpec::default()
    };
    assert_eq!(
        error(&spec),
        "Mod_LoadBrushModel: maps/test.bsp has wrong version number (30 should be 29)"
    );
    assert_eq!(BSP_VERSION, 29);
    assert!(
        read(&BspSpec {
            raw_version: Some(i32::from_le_bytes(*b"IDPO")),
            ..BspSpec::default()
        })
        .is_err()
    );
}

// spec BSP-1.4: a lump that is not a whole number of elements fails.
#[test]
fn funny_lump_size() {
    let spec = BspSpec {
        vertex_padding: 1,
        ..BspSpec::default()
    };
    assert_eq!(
        error(&spec),
        "MOD_LoadBmodel: funny lump size in maps/test.bsp"
    );
    let mut bytes = BspSpec::default().build();
    // Shorten the planes lump by one byte.
    bytes[16..20].copy_from_slice(&19_i32.to_le_bytes());
    assert_eq!(
        Bsp::read(&bytes, BspLoader::WinQuake),
        Err(BspError::FunnyLumpSize {
            lump: BSP_LUMP_PLANES
        })
    );
    // Empty fixed-size lumps hold no elements: no faces at all.
    let spec = BspSpec {
        faces: Vec::new(),
        marksurfaces: Vec::new(),
        leafs: vec![leaf(-2, -1, 0, 0), leaf(-1, 0, 0, 0)],
        nodes: vec![super::fixtures::NodeSpec {
            planenum: 0,
            children: [-2, -1],
            mins: [0; 3],
            maxs: [0; 3],
            firstface: 0,
            numfaces: 0,
        }],
        models: vec![super::fixtures::ModelSpec {
            mins: [0.0; 3],
            maxs: [0.0; 3],
            headnode: [0; 4],
            visleafs: 1,
            firstface: 0,
            numfaces: 0,
        }],
        ..BspSpec::default()
    };
    assert!(read(&spec).unwrap().faces.is_empty());
}

// spec BSP-1.5: of two faults, the one in the lump id loads first is reported.
#[test]
fn load_order() {
    let spec = BspSpec {
        vertex_padding: 1,
        textures: Some(vec![Some(TextureSpec {
            name: b"wall",
            width: 24,
            height: 16,
            pixel: 7,
        })]),
        ..BspSpec::default()
    };
    assert_eq!(
        read(&spec),
        Err(BspError::FunnyLumpSize {
            lump: BSP_LUMP_VERTEXES
        })
    );
    let spec = BspSpec {
        marksurfaces: vec![1],
        texinfo: vec![super::fixtures::TexinfoSpec {
            vecs: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]],
            miptex: 1,
            flags: 0,
        }],
        ..BspSpec::default()
    };
    assert_eq!(read(&spec), Err(BspError::BadMiptex));
}

// spec BSP-1.6: short files and lumps outside the file fail.
#[test]
fn bounds() {
    let base = BspSpec::default().build();
    assert_eq!(
        Bsp::read(&base[..123], BspLoader::WinQuake),
        Err(BspError::TooShort)
    );
    assert_eq!(
        Bsp::read(&base[..1019], BspLoader::WinQuake),
        Err(BspError::LumpOutOfBounds {
            lump: BSP_LUMP_MODELS
        })
    );
    for (at, value) in [(4, -1), (8, -1), (8, 2000), (4, i32::MAX)] {
        let mut bytes = base.clone();
        bytes[at..at + 4].copy_from_slice(&i32::to_le_bytes(value));
        assert_eq!(
            Bsp::read(&bytes, BspLoader::WinQuake),
            Err(BspError::LumpOutOfBounds { lump: 0 }),
            "{at} {value}"
        );
    }
    assert!(BspError::TooShort.message("x").contains("shorter"));
}

// spec BSP-1.8: indexes that name nothing fail.
#[test]
fn bad_indexes() {
    let changes: [fn(&mut FaceSpec); 7] = [
        |face| face.planenum = 1,
        |face| face.planenum = -1,
        |face| face.texinfo = 1,
        |face| face.firstedge = 1,
        |face| face.numedges = -1,
        |face| face.lightofs = 1,
        |face| face.lightofs = -2,
    ];
    for change in changes {
        let mut spec = BspSpec::default();
        change(&mut spec.faces[0]);
        assert_eq!(
            read(&spec),
            Err(BspError::BadIndex {
                lump: BSP_LUMP_FACES,
                element: 0
            })
        );
    }
    let mut spec = BspSpec::default();
    spec.edges[2] = [9, 2];
    assert!(matches!(
        read(&spec),
        Err(BspError::BadIndex {
            lump: BSP_LUMP_FACES,
            ..
        })
    ));
    let mut spec = BspSpec::default();
    spec.surfedges[0] = 5;
    assert!(read(&spec).is_err());
    let mut spec = BspSpec::default();
    spec.leafs[1].visofs = 1;
    assert_eq!(
        read(&spec),
        Err(BspError::BadIndex {
            lump: BSP_LUMP_LEAFS,
            element: 1
        })
    );
    let mut spec = BspSpec::default();
    spec.leafs[1].nummarksurfaces = 2;
    assert!(read(&spec).is_err());
    let mut spec = BspSpec::default();
    spec.nodes[0].children = [0, -3];
    assert_eq!(
        read(&spec),
        Err(BspError::BadIndex {
            lump: BSP_LUMP_NODES,
            element: 0
        })
    );
    let mut spec = BspSpec::default();
    spec.nodes[0].children = [1, -1];
    assert!(read(&spec).is_err());
    let mut spec = BspSpec::default();
    spec.nodes[0].numfaces = 2;
    assert!(read(&spec).is_err());
    let mut spec = BspSpec::default();
    spec.clipnodes[0] = (1, [-1, -2]);
    assert_eq!(
        read(&spec),
        Err(BspError::BadIndex {
            lump: BSP_LUMP_CLIPNODES,
            element: 0
        })
    );
    let mut spec = BspSpec::default();
    spec.clipnodes[0] = (0, [1, -2]);
    assert!(read(&spec).is_err());
    let mut spec = BspSpec::default();
    spec.models[1].headnode = [0, 1, 0, 0];
    assert_eq!(
        read(&spec),
        Err(BspError::BadIndex {
            lump: BSP_LUMP_MODELS,
            element: 1
        })
    );
    let mut spec = BspSpec::default();
    spec.models[1].headnode = [-1, -2, -2, 99];
    assert!(read(&spec).is_ok());
    let mut spec = BspSpec::default();
    spec.models[1].numfaces = 2;
    assert!(read(&spec).is_err());
    assert!(
        BspError::BadIndex {
            lump: 7,
            element: 0
        }
        .message("m")
        .contains("faces 0")
    );
}

// spec BSP-2.2, BSP-2.3, BSP-2.4: the entity text ends at a NUL or the lump's end; an empty
// lump loads; the door names its model *1.
#[test]
fn entities() {
    let bsp = read(&BspSpec {
        entities: Vec::new(),
        ..BspSpec::default()
    })
    .unwrap();
    assert!(bsp.entities().is_empty());
    let bsp = read(&BspSpec {
        entities: b"{\n}\n".to_vec(),
        ..BspSpec::default()
    })
    .unwrap();
    assert_eq!(bsp.entities(), b"{\n}\n");
    let bsp = read(&BspSpec::default()).unwrap();
    let text = String::from_utf8(bsp.entities().to_vec()).unwrap();
    assert!(text.contains("\"model\" \"*1\""));
    assert_eq!(bsp.entity_lump.len(), 92);
}

// spec BSP-3.2, BSP-3.3: the type is kept unchecked; signbits from negative components.
#[test]
fn planes() {
    let mut spec = BspSpec::default();
    spec.planes.push(([-0.6, 0.0, -0.8], 5.0, 9));
    spec.planes.push(([-0.0, 0.5, 1.0], 0.0, 0));
    let bsp = read(&spec).unwrap();
    assert_eq!((bsp.planes[1].signbits, bsp.planes[1].plane_type), (5, 9));
    assert_eq!(bsp.planes[2].signbits, 0);
    assert_eq!(BSP_PLANE_SIZE, 20);
    assert_eq!(
        [
            BSP_PLANE_X,
            BSP_PLANE_Y,
            BSP_PLANE_Z,
            BSP_PLANE_ANYX,
            BSP_PLANE_ANYY,
            BSP_PLANE_ANYZ
        ],
        [0, 1, 2, 3, 4, 5]
    );
}

// spec BSP-4.1, BSP-8.4: no textures lump: the checkerboard with flags 0.
#[test]
fn no_textures() {
    let mut spec = BspSpec {
        textures: None,
        ..BspSpec::default()
    };
    spec.texinfo[0].flags = 1;
    spec.texinfo[0].miptex = 99;
    let bsp = read(&spec).unwrap();
    assert!(bsp.textures.is_empty());
    assert_eq!((bsp.texinfo[0].texture, bsp.texinfo[0].flags), (None, 0));
}

// spec BSP-4.2, BSP-8.4: a slot of -1 is a missing texture.
#[test]
fn missing_texture() {
    let mut spec = BspSpec {
        textures: Some(vec![None]),
        ..BspSpec::default()
    };
    spec.texinfo[0].flags = 1;
    let bsp = read(&spec).unwrap();
    assert!(bsp.textures[0].is_none());
    assert_eq!((bsp.texinfo[0].texture, bsp.texinfo[0].flags), (None, 0));
}

// spec BSP-4.4: sizes must be multiples of 16.
#[test]
fn not_aligned() {
    let spec = BspSpec {
        textures: Some(vec![Some(TextureSpec {
            name: b"wall",
            width: 24,
            height: 16,
            pixel: 7,
        })]),
        ..BspSpec::default()
    };
    assert_eq!(error(&spec), "Texture wall is not 16 aligned");
    let spec = BspSpec {
        textures: Some(vec![Some(TextureSpec {
            name: b"wall",
            width: 16,
            height: 8,
            pixel: 7,
        })]),
        ..BspSpec::default()
    };
    assert!(matches!(read(&spec), Err(BspError::NotAligned { .. })));
}

// spec BSP-4.5, BSP-4.6: levels and directories that do not fit the lump fail.
#[test]
fn texture_bounds() {
    let base = BspSpec::default().build();
    let lump = 236;
    let set = |at: usize, value: i32| {
        let mut bytes = base.clone();
        bytes[lump + at..lump + at + 4].copy_from_slice(&value.to_le_bytes());
        Bsp::read(Vec::leak(bytes), BspLoader::WinQuake).map(|_| ())
    };
    assert_eq!(set(0, -1), Err(BspError::BadTexturesLump));
    assert_eq!(set(0, 97), Err(BspError::BadTexturesLump));
    assert_eq!(set(4, 389), Err(BspError::BadTexture { index: 0 }));
    assert_eq!(set(4, -2), Err(BspError::BadTexture { index: 0 }));
    // Mip level 0 offset 39 starts in the header; level 3 ending past the pixels.
    assert_eq!(set(8 + 24, 39), Err(BspError::BadTexture { index: 0 }));
    assert_eq!(set(8 + 36, 377), Err(BspError::BadTexture { index: 0 }));
    // A size whose pixels run past the lump.
    assert_eq!(set(8 + 20, 32), Err(BspError::BadTexture { index: 0 }));
    assert_eq!(BSP_MIPTEX_HEADER_SIZE, 40);
    assert_eq!(BSP_MIPLEVELS, 4);
    let short = BspSpec {
        textures: Some(Vec::new()),
        ..BspSpec::default()
    };
    let bytes = short.build();
    assert!(
        Bsp::read(&bytes, BspLoader::WinQuake).is_err(),
        "nummiptex 0 rejects every miptex"
    );
}

// spec BSP-4.7, BSP-9.5: `sky` textures, case included, and their faces.
#[test]
fn sky() {
    let bsp = read(&with_textures(&[b"sky1"])).unwrap();
    assert!(bsp.textures[0].as_ref().unwrap().sky);
    assert_eq!(bsp.faces[0].flags, SURF_DRAWSKY | SURF_DRAWTILED);
    assert_eq!(bsp.faces[0].flags, 0x24);
    let bsp = read(&with_textures(&[b"SKY1"])).unwrap();
    assert!(!bsp.textures[0].as_ref().unwrap().sky);
    assert_eq!(bsp.faces[0].flags, 0);
}

// spec BSP-9.5: `*` textures are liquids with huge extents.
#[test]
fn turb() {
    let mut spec = with_textures(&[b"*water"]);
    spec.texinfo[0].flags = 1;
    let bsp = read(&spec).unwrap();
    let face = bsp.faces[0];
    assert_eq!(face.flags, SURF_DRAWTURB | SURF_DRAWTILED);
    assert_eq!(face.flags, 0x30);
    assert_eq!((face.texturemins, face.extents), ([-8192; 2], [16384; 2]));
    assert_eq!(face.lightmap_size, [5, 5]);
}

// spec BSP-4.8, BSP-4.9: an animation with primary and alternate frames.
#[test]
fn anim() {
    let bsp = read(&with_textures(&[b"+0lava", b"+1lava", b"+Alava", b"wall"])).unwrap();
    let anim = |slot: usize| bsp.textures[slot].as_ref().unwrap().animation;
    assert_eq!(
        anim(0),
        Some(Animation {
            total: 4,
            min: 0,
            max: 2,
            next: 1,
            alternate: Some(2)
        })
    );
    assert_eq!(
        anim(1),
        Some(Animation {
            total: 4,
            min: 2,
            max: 4,
            next: 0,
            alternate: Some(2)
        })
    );
    assert_eq!(
        anim(2),
        Some(Animation {
            total: 2,
            min: 0,
            max: 2,
            next: 2,
            alternate: Some(0)
        })
    );
    assert_eq!(anim(3), None);
    assert_eq!(BSP_ANIM_CYCLE, 2);
    let bsp = read(&with_textures(&[b"+alava", b"+0lava"])).unwrap();
    assert_eq!(
        bsp.textures[0]
            .as_ref()
            .unwrap()
            .animation
            .unwrap()
            .alternate,
        Some(1)
    );
    assert_eq!(
        bsp.textures[1]
            .as_ref()
            .unwrap()
            .animation
            .unwrap()
            .alternate,
        Some(0)
    );
}

// spec BSP-4.9: a gap fails, naming the first texture of the animation.
#[test]
fn anim_missing() {
    assert_eq!(
        error(&with_textures(&[b"+0slime", b"+2slime"])),
        "Missing frame 1 of +0slime"
    );
    assert_eq!(
        error(&with_textures(&[b"+Bslime"])),
        "Missing frame 0 of +Bslime"
    );
}

// spec BSP-4.8: a frame that is not 0 to 9 or A to J fails.
#[test]
fn anim_bad() {
    assert_eq!(
        error(&with_textures(&[b"+xfoo"])),
        "Bad animating texture +xfoo"
    );
    assert_eq!(
        error(&with_textures(&[b"+0foo", b"+kfoo"])),
        "Bad animating texture +0foo"
    );
    assert_eq!(error(&with_textures(&[b"+"])), "Bad animating texture +");
}

// spec BSP-4.10: a later texture takes a frame both claim.
#[test]
fn anim_duplicate_frame() {
    let bsp = read(&with_textures(&[b"+0a", b"+0a", b"+1a"])).unwrap();
    assert_eq!(bsp.textures[0].as_ref().unwrap().animation, None);
    assert_eq!(bsp.textures[1].as_ref().unwrap().animation.unwrap().next, 2);
}

// spec BSP-8.1, BSP-9.10: texture offsets move the extents.
#[test]
fn texture_offset() {
    let mut spec = sized(64.0, 36);
    spec.texinfo[0].vecs[0][3] = -8.0;
    spec.texinfo[0].vecs[1][3] = 24.0;
    let face = read(&spec).unwrap().faces[0];
    assert_eq!((face.texturemins, face.extents), ([-16, 16], [80, 80]));
}

// spec BSP-8.6, BSP-9.10: half-length axes give mip bias 2 and smaller extents.
#[test]
fn mipadjust() {
    let mut spec = BspSpec::default();
    spec.texinfo[0].vecs = [[0.5, 0.0, 0.0, 0.0], [0.0, 0.5, 0.0, 0.0]];
    let bsp = read(&spec).unwrap();
    assert_eq!(bsp.texinfo[0].mipadjust, 2);
    assert_eq!(bsp.faces[0].extents, [32, 32]);
    for (scale, bias) in [(0.3_f32, 4), (0.4, 3), (0.98, 2), (0.99, 1), (2.0, 1)] {
        let mut spec = BspSpec::default();
        spec.texinfo[0].vecs = [[scale, 0.0, 0.0, 0.0], [0.0, scale, 0.0, 0.0]];
        spec.lighting = vec![0; 400];
        let bsp = read(&spec).unwrap();
        assert_eq!(bsp.texinfo[0].mipadjust, bias, "{scale}");
    }
}

// spec BSP-8.2, BSP-8.3: miptex indexes the slots; past them, or negative, fails.
#[test]
fn bad_miptex() {
    for miptex in [1, -1] {
        let mut spec = BspSpec::default();
        spec.texinfo[0].miptex = miptex;
        assert_eq!(error(&spec), "miptex >= loadmodel->numtextures");
    }
}

// spec BSP-8.5: TEX_SPECIAL is bit 0; other bits are kept.
#[test]
fn texinfo_flags() {
    assert_eq!(BSP_TEX_SPECIAL, 1);
    let mut spec = BspSpec::default();
    spec.texinfo[0].flags = 6;
    assert_eq!(read(&spec).unwrap().texinfo[0].flags, 6);
}

// spec BSP-9.4, BSP-10.2: 256 loads, 272 fails unless special.
#[test]
fn extents() {
    let face = read(&sized(256.0, 289)).unwrap().faces[0];
    assert_eq!(face.extents, [256, 256]);
    assert_eq!(error(&sized(272.0, 324)), "Bad surface extents");
    let mut spec = sized(272.0, 324);
    spec.texinfo[0].flags = 1;
    assert_eq!(read(&spec).unwrap().faces[0].extents, [272, 272]);
    assert_eq!(BSP_MAX_SURFACE_EXTENT, 256);
    assert_eq!(BSP_LIGHTMAP_SCALE, 16);
    // The samples of 18 by 18 must lie in the lump.
    let mut spec = sized(272.0, 323);
    spec.texinfo[0].flags = 1;
    assert_eq!(
        read(&spec),
        Err(BspError::BadIndex {
            lump: BSP_LUMP_FACES,
            element: 0
        })
    );
}

// spec BSP-9.17: GLQuake's loader accepts extents up to 512 where WinQuake's stops at 256:
// `extents-272` and `extents-512` load in GLQuake and fail in WinQuake, and `extents-528`
// fails in both unless special.
#[test]
fn gl_extents() {
    let gl = |spec: &BspSpec| Bsp::read(Vec::leak(spec.build()), BspLoader::GlQuake);
    for (side, extent, lighting) in [(256.0, 256, 289), (272.0, 272, 324), (512.0, 512, 1089)] {
        let face = gl(&sized(side, lighting)).unwrap().faces[0];
        assert_eq!(face.extents, [extent, extent]);
    }
    assert_eq!(
        gl(&sized(512.0, 1089)).unwrap().faces[0].lightmap_size,
        [33, 33]
    );
    assert_eq!(error(&sized(512.0, 1089)), "Bad surface extents");
    assert_eq!(
        gl(&sized(528.0, 1156)).unwrap_err(),
        BspError::BadSurfaceExtents
    );
    let mut spec = sized(528.0, 1156);
    spec.texinfo[0].flags = 1;
    assert_eq!(gl(&spec).unwrap().faces[0].extents, [528, 528]);
    assert_eq!(read(&spec).unwrap().faces[0].extents, [528, 528]);
    assert_eq!(BSP_GL_MAX_SURFACE_EXTENT, 512);
    assert_eq!(BspLoader::GlQuake.max_surface_extent(), 512);
    assert_eq!(BspLoader::WinQuake.max_surface_extent(), 256);
    // The stored 16-bit extent is checked (BSP-9.14): 65536 + 512 loads, 65536 + 528 fails.
    let mut spec = s_axis([1032.0, 0.0, 0.0, 0.0]);
    spec.faces[0].lightofs = -1;
    assert_eq!(gl(&spec).unwrap().faces[0].extents, [512, 64]);
    spec.texinfo[0].vecs[0][0] = 1032.25;
    assert_eq!(gl(&spec).unwrap_err(), BspError::BadSurfaceExtents);
}

/// The base with leaf 1's contents `contents`, and leaf 0 listing face 0 too when
/// `solid_lists`.
fn leaf_contents(contents: i32, solid_lists: bool) -> BspSpec {
    let mut spec = BspSpec::default();
    spec.leafs[1].contents = contents;
    if solid_lists {
        spec.leafs[0].nummarksurfaces = 1;
    }
    spec
}

/// Face 0's flags as WinQuake's and GLQuake's loaders give them.
fn flags_by_loader(spec: &BspSpec) -> (u32, u32) {
    let bytes: &'static [u8] = Vec::leak(spec.build());
    let flags = |loader| Bsp::read(bytes, loader).unwrap().faces[0].flags;
    (flags(BspLoader::WinQuake), flags(BspLoader::GlQuake))
}

// spec BSP-12.6: GLQuake's loader gives every face a leaf that is not empty lists
// SURF_UNDERWATER (0x80), WinQuake's never does: `water-leaf`, `water-leaf-turb`,
// `solid-leaf-face` and `contents-unknown`, against `base`, whose only face is in an empty leaf.
#[test]
fn gl_underwater() {
    assert_eq!(SURF_UNDERWATER, 0x80);
    assert_eq!(flags_by_loader(&BspSpec::default()), (0, 0));
    assert_eq!(
        flags_by_loader(&leaf_contents(BSP_CONTENTS_WATER, false)),
        (0, SURF_UNDERWATER)
    );
    let mut turb = leaf_contents(BSP_CONTENTS_WATER, false);
    turb.textures = Some(vec![texture(b"*water")]);
    turb.texinfo[0].flags = 1;
    assert_eq!(flags_by_loader(&turb), (0x30, 0xB0));
    // Listed by the solid leaf 0 and the empty leaf 1: the empty leaf does not clear it.
    assert_eq!(
        flags_by_loader(&leaf_contents(BSP_CONTENTS_EMPTY, true)),
        (0, SURF_UNDERWATER)
    );
    // Every value but -1, not only the CONTENTS_ values (`contents-unknown` is -7).
    for contents in [
        -2,
        BSP_CONTENTS_SLIME,
        BSP_CONTENTS_LAVA,
        BSP_CONTENTS_SKY,
        -7,
        i32::MIN,
    ] {
        assert_eq!(
            flags_by_loader(&leaf_contents(contents, false)),
            (0, SURF_UNDERWATER),
            "contents {contents}"
        );
    }
    // A face that no leaf lists keeps its flags, and so does one only an empty leaf lists.
    let mut spec = leaf_contents(BSP_CONTENTS_WATER, false);
    spec.leafs[1].nummarksurfaces = 0;
    assert_eq!(flags_by_loader(&spec), (0, 0));
    let mut spec = leaf_contents(BSP_CONTENTS_WATER, false);
    spec.faces.push(spec.faces[0].clone());
    spec.marksurfaces = vec![1, 0];
    spec.leafs[0] = leaf(BSP_CONTENTS_EMPTY, -1, 1, 1);
    spec.leafs[1].nummarksurfaces = 1;
    let bytes: &'static [u8] = Vec::leak(spec.build());
    let gl = Bsp::read(bytes, BspLoader::GlQuake).unwrap();
    assert_eq!((gl.faces[0].flags, gl.faces[1].flags), (0, SURF_UNDERWATER));
}

// spec BSP-12.6: GLQuake's flag applies to BSP2's leaves as to BSP29's.
#[test]
fn gl_underwater_bsp2() {
    for version in [BspVersion::Bsp2Psb, BspVersion::Bsp2] {
        let spec = BspSpec {
            version,
            ..leaf_contents(BSP_CONTENTS_WATER, false)
        };
        assert_eq!(flags_by_loader(&spec), (0, SURF_UNDERWATER), "{version:?}");
    }
}

// spec BSP-9.1, BSP-9.6, BSP-10.1: a face on its plane's back with no light samples.
#[test]
fn nolight_back() {
    let mut spec = BspSpec {
        lighting: Vec::new(),
        ..BspSpec::default()
    };
    spec.faces[0].side = 1;
    spec.faces[0].styles = [0, 1, 255, 255];
    spec.faces[0].lightofs = -1;
    let bsp = read(&spec).unwrap();
    let face = bsp.faces[0];
    assert_eq!(face.flags, SURF_PLANEBACK);
    assert_eq!(face.style_count(), 2);
    assert_eq!(bsp.face_samples(&face), None);
    assert!(bsp.lighting.is_empty());
    let mut spec = BspSpec::default();
    spec.faces[0].side = -7;
    assert_eq!(read(&spec).unwrap().faces[0].flags, SURF_PLANEBACK);
}

// spec BSP-9.8, BSP-14.3: negative counts and the surfedge that cannot be negated fail.
#[test]
fn face_fields() {
    let mut spec = BspSpec::default();
    spec.surfedges[0] = i32::MIN;
    assert_eq!(
        read(&spec),
        Err(BspError::BadIndex {
            lump: BSP_LUMP_FACES,
            element: 0
        })
    );
    let mut spec = BspSpec::default();
    spec.faces[0].texinfo = -1;
    assert!(read(&spec).is_err());
    assert_eq!(BSP_FACE_SIZE, 20);
    assert_eq!(BSP_MAXLIGHTMAPS, 4);
}

// spec BSP-9.13, BSP-9.14: a minimum out of the int range takes x86's integer indefinite
// value; the maximum stays -99999, so the extent is (-6249 - 2^31) * 16, whose low 16 bits are
// 31088, and only TEX_SPECIAL lets the face load.
#[test]
fn huge_extents() {
    let mut spec = BspSpec::default();
    spec.texinfo[0].vecs[0][3] = -1.0e30;
    spec.faces[0].lightofs = -1;
    assert_eq!(error(&spec), "Bad surface extents");
    spec.texinfo[0].flags = 1;
    let face = read(&spec).unwrap().faces[0];
    assert_eq!((face.texturemins, face.extents), ([0, 0], [31088, 64]));
    assert_eq!(x86_double_to_int(f64::NAN), i32::MIN);
    assert_eq!(x86_double_to_int(-3.7), -3);
    assert_eq!(x86_double_to_int(2_147_483_648.0), i32::MIN);
    assert_eq!(x86_double_to_int(-2_147_483_648.0), i32::MIN);
    assert_eq!(x86_double_to_int(2_147_483_647.9), i32::MAX);
}

/// The base with texinfo 0's s axis `s`, which spans the square's 64 units.
fn s_axis(s: [f32; 4]) -> BspSpec {
    let mut spec = BspSpec::default();
    spec.texinfo[0].vecs[0] = s;
    spec
}

// spec BSP-9.4, BSP-9.7, BSP-9.14: an extent of 65536 + 16 keeps its low 16 bits, 16, so
// the face passes the check and has a lightmap 2 samples wide (vector `extents-65552`).
#[test]
fn extents_65552() {
    let face = read(&s_axis([1024.25, 0.0, 0.0, 0.0])).unwrap().faces[0];
    assert_eq!((face.texturemins, face.extents), ([0, 0], [16, 64]));
    assert_eq!(face.lightmap_size, [2, 5]);
    // One step less, 65536, is 0.
    let face = read(&s_axis([1024.0, 0.0, 0.0, 0.0])).unwrap().faces[0];
    assert_eq!(face.extents, [0, 64]);
    // 65536 + 272 is 272, which fails as 272 does.
    assert_eq!(
        error(&s_axis([1028.25, 0.0, 0.0, 0.0])),
        "Bad surface extents"
    );
}

// spec BSP-9.14: a texturemins of 32768 is stored as -32768 (vector `texturemins-32768`);
// 32752 is kept.
#[test]
fn texturemins_32768() {
    let face = read(&s_axis([1.0, 0.0, 0.0, 32768.0])).unwrap().faces[0];
    assert_eq!((face.texturemins, face.extents), ([-32768, 0], [64, 64]));
    let face = read(&s_axis([1.0, 0.0, 0.0, 32752.0])).unwrap().faces[0];
    assert_eq!(face.texturemins, [32752, 0]);
    let face = read(&s_axis([1.0, 0.0, 0.0, -32784.0])).unwrap().faces[0];
    assert_eq!(face.texturemins, [32752, 0]);
}

// spec BSP-9.4, BSP-9.14: an extent of 32784 is stored as -32752, which the signed check
// passes, so the face loads (vector `extents-32784`); 32752 is kept and fails.
#[test]
fn extents_32784() {
    let face = read(&s_axis([512.25, 0.0, 0.0, 0.0])).unwrap().faces[0];
    assert_eq!(face.extents, [-32752, 64]);
    assert_eq!(
        error(&s_axis([511.75, 0.0, 0.0, 0.0])),
        "Bad surface extents"
    );
}

/// The base with texinfo 0's axes scaled by `s` and `t`.
fn st_axes(s: f32, t: f32) -> BspSpec {
    let mut spec = s_axis([s, 0.0, 0.0, 0.0]);
    spec.texinfo[0].vecs[1] = [0.0, t, 0.0, 0.0];
    spec
}

// spec BSP-9.16: a face with a negative stored extent on either axis has a lightmap of 0 by 0
// samples, so it has no samples and BSP-10.2 asks only that lightofs is at most the lump's
// size (vectors `extents-32784`, `extents-65520`, `extents-32784-t`, `extents-65504-st` and
// `extents-32784-st`).
#[test]
fn negative_extent_lightmap() {
    for (s, t, extents) in [
        (512.25, 1.0, [-32752, 64]),
        (1023.75, 1.0, [-16, 64]),
        (1.0, 512.25, [64, -32752]),
        (1023.5, 1023.5, [-32, -32]),
        (512.25, 512.25, [-32752, -32752]),
    ] {
        let bsp = read(&st_axes(s, t)).unwrap();
        let face = bsp.faces[0];
        assert_eq!(face.extents, extents);
        assert_eq!(face.lightmap_size, [0, 0], "{extents:?}");
        assert_eq!(bsp.face_samples(&face), Some(&[][..]));
        // The lump has 25 bytes: lightofs 25 is its end, 26 is past it.
        let mut spec = st_axes(s, t);
        spec.faces[0].lightofs = 25;
        let face = read(&spec).unwrap().faces[0];
        assert_eq!(face.lightmap_size, [0, 0]);
        spec.faces[0].lightofs = 26;
        assert_eq!(
            read(&spec),
            Err(BspError::BadIndex {
                lump: BSP_LUMP_FACES,
                element: 0
            })
        );
    }
    // An extent of 0 is not negative: one sample on that axis.
    let face = read(&st_axes(1024.0, 1.0)).unwrap().faces[0];
    assert_eq!((face.extents, face.lightmap_size), ([0, 64], [1, 5]));
    assert_eq!(lightmap_size([32767, 0]), [2048, 1]);
    assert_eq!(lightmap_size([0, -1]), [0, 0]);
    assert_eq!(lightmap_size([i16::MIN, 16]), [0, 0]);
}

// spec BSP-9.14: the stored value is the low 16 bits read as two's complement.
#[test]
fn low_16_bits() {
    for (value, low) in [
        (0, 0),
        (32767, 32767),
        (32768, -32768),
        (65535, -1),
        (65536, 0),
        (65552, 16),
        (-1, -1),
        (-32769, 32767),
        (i32::MIN, 0),
        (i32::MAX, -1),
    ] {
        assert_eq!(low_16(value), low, "{value}");
    }
}

/// The base with vertex 0 at (1, 0, 1), the others at (1, 1, 1), and texinfo 0's s axis
/// `s`: the face of the `x87-order` and `x87-pc64` vectors.
fn skewed(s: [f32; 4]) -> BspSpec {
    let mut spec = BspSpec {
        vertexes: vec![
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
        ],
        ..BspSpec::default()
    };
    spec.texinfo[0].vecs[0] = s;
    spec
}

// spec BSP-9.11: the products are exact and are added x, z, y at 53 bits, then rounded once
// to single precision; single precision or C's order of the additions gives extent 0.
#[test]
#[expect(
    clippy::cast_possible_truncation,
    reason = "test code: rounding to single precision is the point"
)]
fn x87_order() {
    let spec = skewed([
        2048.0,
        2.0_f32.powi(-42),
        -(1024.0 - 2.0_f32.powi(-14)),
        0.0,
    ]);
    let face = read(&spec).unwrap().faces[0];
    assert_eq!((face.texturemins, face.extents), ([1024, 0], [16, 16]));
    assert_eq!(face.lightmap_size, [2, 2]);
    let s = spec.texinfo[0].vecs[0];
    let top = x87_texture_coordinate([1.0, 1.0, 1.0], s);
    assert_eq!(top, 1024.0 + 2.0_f32.powi(-13));
    let single = 1.0 * s[0] + 1.0 * s[1] + 1.0 * s[2] + s[3];
    assert_eq!(single, 1024.0);
    let c_order = (2048.0_f64 + 2.0_f64.powi(-42) - (1024.0 - 2.0_f64.powi(-14))) as f32;
    assert_eq!(c_order, 1024.0);
}

// spec BSP-9.11: the sums round to 53 bits, not 64, so a sum that only 64 bits keep above
// the midpoint of two singles rounds to the even one.
#[test]
fn x87_pc53() {
    let spec = skewed([0.0, 2.0_f32.powi(-14), 2.0_f32.powi(-50), 1024.0]);
    let face = read(&spec).unwrap().faces[0];
    assert_eq!((face.texturemins, face.extents), ([1024, 0], [0, 16]));
    let s = spec.texinfo[0].vecs[0];
    assert_eq!(x87_texture_coordinate([1.0, 1.0, 1.0], s), 1024.0);
    assert_eq!(x87_texture_coordinate([1.0, 0.0, 1.0], s), 1024.0);
    // Infinities and NaN pass through as on the x87.
    assert_eq!(
        x87_texture_coordinate([f32::MAX, 0.0, 0.0], [2.0, 0.0, 0.0, 0.0]),
        f32::INFINITY
    );
    assert!(x87_texture_coordinate([f32::INFINITY, 0.0, 0.0], [0.0; 4]).is_nan());
}

// spec BSP-9.12: a NaN coordinate replaces the minimum and maximum, and the next coordinate
// replaces a NaN bound; vertex 0, read first and last, leaves both NaN.
#[test]
fn nan_vertex() {
    let mut spec = BspSpec::default();
    spec.vertexes[0] = [f32::NAN, 0.0, 0.0];
    let face = read(&spec).unwrap().faces[0];
    assert_eq!((face.texturemins, face.extents), ([0, 0], [0, 0]));
    assert_eq!(face.lightmap_size, [1, 1]);
    assert!(x87_replaces(f32::NAN, 5.0));
    assert!(x87_replaces(5.0, f32::NAN));
    assert!(x87_replaces(4.0, 5.0));
    assert!(!x87_replaces(5.0, 5.0));
    assert!(!x87_replaces(6.0, 5.0));
}

// spec BSP-13.2: a marksurface that names no face fails.
#[test]
fn bad_marksurface() {
    let spec = BspSpec {
        marksurfaces: vec![1],
        ..BspSpec::default()
    };
    assert_eq!(error(&spec), "Mod_ParseMarksurfaces: bad surface number");
    // Read as unsigned: 0xFFFF is 65535, not -1.
    let spec = BspSpec {
        marksurfaces: vec![0xFFFF],
        ..BspSpec::default()
    };
    assert_eq!(read(&spec), Err(BspError::BadMarksurface));
}

// spec BSP-12.2, BSP-12.4, BSP-12.5: contents unchecked, ambients, unsigned ranges.
#[test]
fn leaves() {
    let mut spec = BspSpec::default();
    spec.leafs[1].contents = 42;
    spec.marksurfaces = vec![0; 32769];
    spec.leafs[1].firstmarksurface = 32768;
    let bsp = read(&spec).unwrap();
    assert_eq!(bsp.leafs[1].contents, 42);
    assert_eq!(bsp.leafs[1].firstmarksurface, 32768);
    assert_eq!(
        [
            BSP_CONTENTS_EMPTY,
            BSP_CONTENTS_SOLID,
            BSP_CONTENTS_WATER,
            BSP_CONTENTS_SLIME,
            BSP_CONTENTS_LAVA,
            BSP_CONTENTS_SKY
        ],
        [-1, -2, -3, -4, -5, -6]
    );
    assert_eq!(BSP_NUM_AMBIENTS, 4);
    assert_eq!(BSP_LEAF_SIZE, 28);
}

// spec BSP-6.1, BSP-6.2, BSP-6.3, BSP-6.4, BSP-15.2: the vis variant's rows.
#[test]
fn vis() {
    let mut spec = BspSpec::default();
    spec.models[0].visleafs = 20;
    spec.leafs.push(leaf(-1, 4, 0, 0));
    for _ in 3..21 {
        spec.leafs.push(leaf(-1, -1, 0, 0));
    }
    spec.visibility = vec![0x05, 0x00, 0x01, 0x80, 0x00, 0x05];
    let bsp = read(&spec).unwrap();
    assert_eq!(bsp.vis_row_size(), 3);
    assert_eq!(bsp.leaf_pvs(0).unwrap(), [0xFF; 3]);
    assert_eq!(bsp.leaf_pvs(1).unwrap(), [0x05, 0x00, 0x80]);
    assert_eq!(bsp.leaf_pvs(2).unwrap(), [0x00; 3]);
    assert_eq!(bsp.leaf_pvs(3).unwrap(), [0xFF; 3]);
    assert_eq!(BSP_MAX_MAP_LEAFS, 8192);
}

// spec BSP-6.6: leaf 0's offset is not read; 0 into an empty lump sees everything.
#[test]
fn unread_vis_offsets() {
    let mut spec = BspSpec::default();
    spec.leafs[0].visofs = 99;
    assert_eq!(read(&spec).unwrap().leaf_pvs(0).unwrap(), [0xFF]);
    let mut spec = BspSpec {
        visibility: Vec::new(),
        ..BspSpec::default()
    };
    spec.leafs[0].visofs = 0;
    spec.leafs[1].visofs = 0;
    let bsp = read(&spec).unwrap();
    assert_eq!(bsp.leaf_pvs(1).unwrap(), [0xFF]);
    spec.leafs[1].visofs = 1;
    assert_eq!(
        read(&spec),
        Err(BspError::BadIndex {
            lump: BSP_LUMP_LEAFS,
            element: 1
        })
    );
}

// spec BSP-6.3, BSP-6.5: a run of 0 writes nothing; a set that ends early fails.
#[test]
fn vis_edges() {
    let mut spec = BspSpec::default();
    spec.models[0].visleafs = 16;
    spec.visibility = vec![0x00, 0x00, 0x07, 0x08, 0x00];
    let bsp = read(&spec).unwrap();
    assert_eq!(bsp.leaf_pvs(1).unwrap(), [0x07, 0x08]);
    spec.models[0].visleafs = 24;
    let bsp = read(&spec).unwrap();
    assert_eq!(bsp.leaf_pvs(1), Err(BspError::VisTruncated { leaf: 1 }));
    assert!(
        BspError::VisTruncated { leaf: 1 }
            .message("m")
            .contains("leaf 1")
    );
}

// spec BSP-7.3, BSP-7.4, BSP-7.6: node bounds and faces; no nodes fails.
#[test]
fn nodes() {
    let mut spec = BspSpec::default();
    spec.nodes[0].mins = [-5, -6, -7];
    let bsp = read(&spec).unwrap();
    assert_eq!(bsp.nodes[0].mins, [-5.0, -6.0, -7.0]);
    assert_eq!(BSP_NODE_SIZE, 24);
    let spec = BspSpec {
        nodes: Vec::new(),
        ..BspSpec::default()
    };
    assert_eq!(read(&spec), Err(BspError::NoNodes));
}

// spec BSP-15.5, BSP-15.6: origin is read; no models fails.
#[test]
fn models() {
    assert_eq!(
        read(&BspSpec::default()).unwrap().models[0].origin,
        [0.0; 3]
    );
    let spec = BspSpec {
        models: Vec::new(),
        ..BspSpec::default()
    };
    assert_eq!(error(&spec), "maps/test.bsp has no models");
    assert_eq!(BSP_MODEL_SIZE, 64);
}

// spec BSP-11.1, BSP-14.1, BSP-14.2: element sizes, and a reversed edge's v[1].
#[test]
fn edges() {
    assert_eq!(
        (BSP_CLIPNODE_SIZE, BSP_EDGE_SIZE, BSP_SURFEDGE_SIZE),
        (8, 4, 4)
    );
    assert_eq!((BSP_VERTEX_SIZE, BSP_TEXINFO_SIZE), (12, 40));
    let bsp = read(&BspSpec::default()).unwrap();
    let face = bsp.faces[0];
    assert_eq!(bsp.face_vertices(&face)[2], [64.0, 64.0, 0.0]);
    let spec = BspSpec {
        surfedges: vec![1, 2, 3, 4],
        ..BspSpec::default()
    };
    let bsp = read(&spec).unwrap();
    assert_eq!(bsp.face_vertices(&bsp.faces[0]), square(64.0));
}

// spec BSP-16.1, BSP-16.2: the compiler limits are constants the loader does not apply; a
// BSP29 child of -32768 is leaf 32767.
#[test]
fn limits() {
    assert_eq!(limits::BSP_MAX_MAP_MODELS, 256);
    assert_eq!(limits::BSP_MAX_MAP_PLANES, 32767);
    assert_eq!(limits::BSP_MAX_MAP_VERTS, 65535);
    assert_eq!(limits::BSP_MAX_MAP_EDGES, 256_000);
    assert_eq!(limits::BSP_MAX_MAP_SURFEDGES, 512_000);
    assert_eq!(limits::BSP_MAX_MAP_ENTSTRING, 65536);
    assert_eq!(limits::BSP_MAX_MAP_MIPTEX, 0x20_0000);
    assert_eq!(limits::BSP_MAX_MAP_LIGHTING, 0x10_0000);
    assert_eq!(limits::BSP_MAX_MAP_VISIBILITY, 0x10_0000);
    assert_eq!(limits::BSP_MAX_MAP_TEXTURES, 512);
    assert_eq!(limits::BSP_MAX_MAP_TEXINFO, 4096);
    assert_eq!(limits::BSP_MAX_MAP_NODES, 32767);
    assert_eq!(limits::BSP_MAX_MAP_CLIPNODES, 32767);
    assert_eq!(limits::BSP_MAX_MAP_FACES, 65535);
    assert_eq!(limits::BSP_MAX_MAP_MARKSURFACES, 65535);
    assert_eq!(limits::BSP_MAX_MAP_BRUSHES, 4096);
    assert_eq!(limits::BSP_MAX_MAP_ENTITIES, 1024);
    assert_eq!((limits::BSP_MAX_KEY, limits::BSP_MAX_VALUE), (32, 1024));
    let mut spec = BspSpec::default();
    spec.nodes[0].children = [-32768, -1];
    assert_eq!(
        read(&spec),
        Err(BspError::BadIndex {
            lump: BSP_LUMP_NODES,
            element: 0
        })
    );
    // Many vertexes load: the engine applies no limit.
    let mut spec = BspSpec::default();
    spec.vertexes.extend(core::iter::repeat_n([1.0; 3], 70_000));
    assert_eq!(read(&spec).unwrap().vertexes.len(), 70_004);
}

// spec BSP-17.1, BSP-17.2, BSP-17.3, BSP-17.4, BSP-17.6: both BSP2 variants of the base load
// to the same values, with 32-bit fields and, for BSP2, float bounds.
#[test]
fn bsp2() {
    assert_eq!(BSP_2PSB_VERSION, 0x4253_5032);
    assert_eq!(BSP_BSP2_VERSION, 0x3250_5342);
    let base = read(&BspSpec::default()).unwrap();
    for version in [BspVersion::Bsp2Psb, BspVersion::Bsp2] {
        let mut spec = BspSpec {
            version,
            ..BspSpec::default()
        };
        spec.marksurfaces = vec![0; 70_000];
        spec.leafs[1].firstmarksurface = 69_999;
        spec.nodes[0].children = [-2, -1];
        let bsp = read(&spec).unwrap();
        assert_eq!(bsp.version, version);
        assert_eq!(bsp.faces, base.faces);
        assert_eq!(bsp.nodes, base.nodes);
        assert_eq!(bsp.clipnodes, base.clipnodes);
        assert_eq!(bsp.edges, base.edges);
        assert_eq!(bsp.leafs[1].firstmarksurface, 69_999);
        assert_eq!(bsp.models, base.models);
        assert_eq!(bsp.leaf_pvs(1).unwrap(), [1]);
    }
    let mut spec = BspSpec {
        version: BspVersion::Bsp2,
        ..BspSpec::default()
    };
    spec.edges.push([0, 100_000]);
    spec.vertexes
        .extend(core::iter::repeat_n([0.0; 3], 100_000));
    spec.surfedges = vec![1, 2, 3, -5];
    spec.edges[3] = [2, 3];
    let bsp = read(&spec).unwrap();
    assert_eq!(bsp.edges[5], [0, 100_000]);
    assert_eq!(
        (
            BSP_2PSB_NODE_SIZE,
            BSP_BSP2_NODE_SIZE,
            BSP_2PSB_LEAF_SIZE,
            BSP_BSP2_LEAF_SIZE
        ),
        (32, 44, 32, 44)
    );
    assert_eq!(
        (
            BSP_BSP2_CLIPNODE_SIZE,
            BSP_BSP2_EDGE_SIZE,
            BSP_BSP2_FACE_SIZE,
            BSP_BSP2_MARKSURFACE_SIZE
        ),
        (12, 8, 28, 4)
    );
    // A BSP29 reader's sizes do not fit a BSP2 file.
    let spec = BspSpec {
        version: BspVersion::Bsp2,
        raw_version: Some(29),
        ..BspSpec::default()
    };
    assert!(read(&spec).is_err());
}

// spec BSP-18.1, BSP-18.2, BSP-18.3, BSP-18.5: .lit files.
#[test]
fn lit() {
    let mut file = b"QLIT".to_vec();
    file.extend_from_slice(&1_i32.to_le_bytes());
    file.extend_from_slice(&[1, 2, 3, 4, 5, 6]);
    let found = Lit::read(&file, 2).unwrap();
    assert_eq!(found.sample(1), Some([4, 5, 6]));
    assert_eq!(found.sample(2), None);
    assert_eq!(
        Lit::read(&file, 3).unwrap_err().message("maps/a.lit"),
        "Outdated .lit file (maps/a.lit should be 17 bytes, not 14)"
    );
    let mut old = file.clone();
    old[4..8].copy_from_slice(&0x0001_0001_i32.to_le_bytes());
    assert_eq!(
        Lit::read(&old, 2),
        Err(LitError::UnknownVersion {
            version: 0x0001_0001
        })
    );
    assert_eq!(
        LitError::UnknownVersion { version: 2 }.to_string(),
        "Unknown .lit file version (2)"
    );
    assert_eq!(Lit::read(b"QLI", 0), Err(LitError::Corrupt));
    assert_eq!(Lit::read(b"qlit\x01\0\0\0", 0), Err(LitError::Corrupt));
    assert_eq!(
        LitError::Corrupt.to_string(),
        "Corrupt .lit file (old version?), ignoring"
    );
    assert!(matches!(
        Lit::read(b"QLIT\x01", 0),
        Err(LitError::Outdated { .. })
    ));
    assert_eq!(Lit::read(b"QLIT\x01\0\0\0", 0).unwrap().rgb, b"");
    assert_eq!(
        (BSP_LIT_MAGIC, BSP_LIT_VERSION, BSP_LIT_HEADER_SIZE),
        (*b"QLIT", 1, 8)
    );
    assert_eq!(grey(&[0, 9]), [[0; 3], [9; 3]]);
    assert_eq!(companion_name("maps/e1m1.bsp", ".lit"), "maps/e1m1.lit");
}

// spec BSP-18.4, BSP-19.2: a .lit or .ent from a lower-priority game directory is ignored.
#[test]
fn companion_priority() {
    assert!(companion_applies(2, 2));
    assert!(companion_applies(2, 3));
    assert!(!companion_applies(2, 1));
}

// spec BSP-19.1: the .ent names QuakeSpasm tries, with the CRC of the lump less its NUL.
#[test]
fn ent_names() {
    let lump = b"{\n}\n\0";
    let crc = crc_block(b"{\n}\n");
    assert_eq!(
        ent_file_names("maps/e1m1.bsp", lump),
        [
            format!("maps/e1m1@{crc:04x}.ent"),
            "maps/e1m1.ent".to_owned()
        ]
    );
    assert_eq!(
        ent_file_names("maps/e1m1.bsp", b"")[0],
        "maps/e1m1@0000.ent"
    );
    assert_eq!(companion_name("maps.d/start", ".ent"), "maps.d/start.ent");
    assert_eq!(companion_name("a.b.bsp", ".ent"), "a.b.ent");
    assert_eq!(companion_name(".bsp", ".ent"), ".bsp.ent");
    assert_eq!(companion_name("", ".ent"), ".ent");
    assert_eq!(BSP_ENT_EXTENSION, ".ent");
}

// spec BSP-19.3: a BSP written with new entities reads back with them and every other lump
// unchanged.
#[test]
fn replaced_entities() {
    let base = BspSpec::default().build();
    let text = b"{\n\"classname\" \"worldspawn\"\n\"message\" \"edited\"\n}\n";
    let edited = replace_entities(&base, text, BspLoader::WinQuake).unwrap();
    let before = Bsp::read(&base, BspLoader::WinQuake).unwrap();
    let after = Bsp::read(&edited, BspLoader::WinQuake).unwrap();
    assert_eq!(after.entities(), text);
    assert_eq!(after.entity_lump.last(), Some(&0));
    assert_eq!(after.faces, before.faces);
    assert_eq!(after.textures, before.textures);
    assert_eq!(after.lighting, before.lighting);
    assert_eq!(after.models, before.models);
    for lump in after.lumps {
        assert_eq!(lump.fileofs.checked_rem(4), Some(0));
    }
    assert_eq!(
        replace_entities(&edited, &base_entities(), BspLoader::WinQuake).unwrap(),
        base
    );
    assert!(replace_entities(&base[..100], b"", BspLoader::WinQuake).is_err());
}

fn base_entities() -> Vec<u8> {
    let mut text = BspSpec::default().entities;
    text.pop();
    text
}

proptest! {
    // spec BSP-1.6, BSP-1.8: arbitrary bytes, alone or after a valid header, never panic the
    // reader or its accessors.
    #[test]
    fn arbitrary_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..2048)) {
        exercise(&bytes);
        let mut base = BspSpec::default().build();
        let at = usize::from(bytes.first().copied().unwrap_or(0)) * 4 % base.len();
        for (offset, byte) in bytes.iter().skip(1).take(16).enumerate() {
            let index = (at + offset) % base.len();
            base[index] = *byte;
        }
        exercise(&base);
    }
}

fn exercise(bytes: &[u8]) {
    for loader in [BspLoader::WinQuake, BspLoader::GlQuake] {
        exercise_with(bytes, loader);
    }
}

fn exercise_with(bytes: &[u8], loader: BspLoader) {
    if let Ok(bsp) = Bsp::read(bytes, loader) {
        for leaf in 0..bsp.leafs.len().min(64) {
            let _row = bsp.leaf_pvs(leaf);
        }
        for face in &bsp.faces {
            let _vertices = bsp.face_vertices(face);
            let _samples = bsp.face_samples(face);
        }
        let _hull = bsp.hull0();
        let _entities = bsp.entities();
        let _edited = replace_entities(bytes, b"{\n}\n", loader);
    }
}
