// SPDX-License-Identifier: GPL-3.0-or-later

//! The MDL vectors of `docs/spec/vectors/mdl/`, each giving the result its README records for
//! id's `Mod_LoadAliasModel`, and the statements the vectors do not reach.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap, index, panic or overflow is a failed test"
)]

use proptest::prelude::*;

use super::fixtures::{EntrySpec, MdlSpec, SkinSpec, frame, from_xxd};
use super::*;

const BASE_XXD: &str = include_str!("../../../../docs/spec/vectors/mdl/base.txt");

fn read(spec: &MdlSpec) -> Result<Mdl<'static>, MdlError> {
    let bytes: &'static [u8] = Vec::leak(spec.build());
    Mdl::read(bytes)
}

fn error(spec: &MdlSpec) -> String {
    read(spec).unwrap_err().message("progs/test.mdl")
}

fn single_frame(verts: usize) -> Vec<EntrySpec> {
    vec![EntrySpec::Single(frame(
        b"f",
        [0; 4],
        [0; 4],
        vec![[0; 4]; verts],
    ))]
}

fn with_verts(count: usize) -> MdlSpec {
    MdlSpec {
        stverts: vec![[0, 0, 0]; count],
        frames: single_frame(count),
        ..MdlSpec::default()
    }
}

// spec MDL-1.1, MDL-1.2, MDL-1.6, MDL-1.7, MDL-2.1, MDL-2.2, MDL-3.1, MDL-4.1, MDL-5.1, MDL-5.2:
// spec MDL-5.4:
// the base vector loads as the README records.
#[test]
fn base() {
    let bytes = from_xxd(BASE_XXD);
    assert_eq!(bytes.len(), 284);
    assert_eq!(MdlSpec::default().build(), bytes);
    let mdl = Mdl::read(&bytes).unwrap();
    let header = mdl.header;
    assert_eq!(
        (header.flags, header.synctype),
        (MDL_EF_ROTATE, MDL_ST_RAND)
    );
    assert_eq!((header.skinwidth, header.skinheight), (4, 2));
    assert_eq!(header.scale_origin, [-8.0; 3]);
    assert_eq!(header.eye_position, [0.0, 0.0, 22.0]);
    assert_eq!(mdl.size().to_bits(), 1.0_f32.to_bits());
    assert_eq!(mdl.bounds(), ([-16.0; 3], [16.0; 3]));
    assert_eq!(mdl.skins, [MdlSkin::Single(&[1, 2, 3, 4, 5, 6, 7, 8][..])]);
    assert_eq!(
        mdl.stverts[1],
        StVert {
            onseam: 0x20,
            s: 1,
            t: 2
        }
    );
    assert_eq!(
        mdl.triangles,
        [Triangle {
            facesfront: 1,
            vertindex: [0, 1, 2]
        }]
    );
    assert_eq!(mdl.frames.len(), 2);
    let MdlFrameEntry::Single(stand) = &mdl.frames[0] else {
        panic!("frame 0 is single");
    };
    assert_eq!(stand.name, b"stand1");
    assert_eq!(
        stand.vertex(1),
        Some(TriVertX {
            v: [1, 2, 3],
            lightnormalindex: 1
        })
    );
    assert_eq!(stand.vertex(3), None);
    assert_eq!(stand.vertex_count(), 3);
    assert_eq!(mdl.frames[1].name(), b"run2");
    assert_eq!(
        mdl.frames[1].bbox(),
        (
            TriVertX {
                v: [1; 3],
                lightnormalindex: 0
            },
            TriVertX {
                v: [9; 3],
                lightnormalindex: 0
            }
        )
    );
    let MdlFrameEntry::Group {
        intervals,
        frames,
        frame_type,
        ..
    } = &mdl.frames[1]
    else {
        panic!("frame 1 is a group");
    };
    assert_eq!(*frame_type, MDL_FRAME_GROUP);
    assert_eq!(intervals, &[0.1, 0.2]);
    assert_eq!(frames[0].name, b"run1");
    assert_eq!(frames[0].bboxmin.v, [0; 3]);
    assert_eq!(mdl.frames[0].bbox().1.v, [16; 3]);
    assert!(mdl.bad_normals().is_empty());
    assert!(mdl.gl_limits().is_empty());
}

// spec MDL-1.2: only `IDPO` is an alias model; a short header fails.
#[test]
fn magic() {
    assert_eq!(MDL_MAGIC, *b"IDPO");
    let spec = MdlSpec {
        magic: *b"IDSP",
        ..MdlSpec::default()
    };
    assert_eq!(read(&spec), Err(MdlError::WrongId));
    assert_eq!(Mdl::read(b"IDP"), Err(MdlError::WrongId));
    let bytes = MdlSpec::default().build();
    assert_eq!(
        Mdl::read(&bytes[..83]),
        Err(MdlError::Truncated { offset: 0 })
    );
}

// spec MDL-1.3: version 6 only.
#[test]
fn version() {
    assert_eq!(MDL_VERSION, 6);
    let spec = MdlSpec {
        version: 5,
        ..MdlSpec::default()
    };
    assert_eq!(
        error(&spec),
        "progs/test.mdl has wrong version number (5 should be 6)"
    );
}

// spec MDL-1.4: the header checks and their order.
#[test]
fn header_checks() {
    assert_eq!(MDL_MAX_SKIN_HEIGHT, 480);
    assert_eq!(MDL_MAX_VERTS, 2000);
    let tall = |height| MdlSpec {
        skinheight: height,
        counting_skin: true,
        ..MdlSpec::default()
    };
    assert!(read(&tall(480)).is_ok());
    assert_eq!(
        error(&tall(481)),
        "model progs/test.mdl has a skin taller than 480"
    );
    let spec = MdlSpec {
        numverts: Some(0),
        numtris: Some(0),
        ..MdlSpec::default()
    };
    assert_eq!(error(&spec), "model progs/test.mdl has no vertices");
    assert!(read(&with_verts(2000)).is_ok());
    assert_eq!(
        error(&with_verts(2001)),
        "model progs/test.mdl has too many vertices"
    );
    let spec = MdlSpec {
        numtris: Some(0),
        ..MdlSpec::default()
    };
    assert_eq!(error(&spec), "model progs/test.mdl has no triangles");
    let spec = MdlSpec {
        skinwidth: 6,
        numskins: Some(0),
        ..MdlSpec::default()
    };
    assert_eq!(
        error(&spec),
        "Mod_LoadAliasModel: skinwidth not multiple of 4"
    );
    let spec = MdlSpec {
        numskins: Some(0),
        ..MdlSpec::default()
    };
    assert_eq!(error(&spec), "Mod_LoadAliasModel: Invalid # of skins: 0");
    let spec = MdlSpec {
        numframes: Some(0),
        ..MdlSpec::default()
    };
    assert_eq!(error(&spec), "Mod_LoadAliasModel: Invalid # of frames: 0");
    // order: a tall skin is reported before missing vertices.
    let spec = MdlSpec {
        skinwidth: 6,
        skinheight: 481,
        numverts: Some(0),
        numtris: Some(0),
        ..MdlSpec::default()
    };
    assert_eq!(read(&spec), Err(MdlError::SkinTooTall));
    // order-2: the width is reported before the skins and frames.
    let spec = MdlSpec {
        skinwidth: 6,
        numskins: Some(0),
        numframes: Some(0),
        ..MdlSpec::default()
    };
    assert_eq!(read(&spec), Err(MdlError::SkinWidth));
    // The frame count is checked after the triangles are read.
    let spec = MdlSpec {
        numframes: Some(0),
        triangles: vec![[1, 0, 1, 9]],
        ..MdlSpec::default()
    };
    assert_eq!(read(&spec), Err(MdlError::BadVertexIndex { triangle: 0 }));
}

// spec MDL-1.5: negative or overflowing skin sizes fail.
#[test]
fn skin_size() {
    for (width, height) in [(-4, 2), (4, -2), (0x4000_0000, 8)] {
        let spec = MdlSpec {
            skinwidth: width,
            skinheight: height,
            skins: Vec::new(),
            numskins: Some(1),
            ..MdlSpec::default()
        };
        assert_eq!(
            read(&spec),
            Err(MdlError::BadSkinSize { width, height }),
            "{width} {height}"
        );
    }
    assert!(
        MdlError::BadSkinSize {
            width: -4,
            height: 2
        }
        .message("m")
        .contains("-4 by 2")
    );
    // spec MDL-1.5: the guard rejects only a negative width or height, so a zero width and
    // zero height is accepted (skin size 0). This distinguishes `< 0` from `== 0` and `<= 0`
    // on both fields: those mutants would reject a model the loader accepts.
    let zero = MdlSpec {
        skinwidth: 0,
        skinheight: 0,
        skins: vec![SkinSpec::Single(0)],
        counting_skin: false,
        ..MdlSpec::default()
    };
    assert_eq!(read(&zero).unwrap().header.skinwidth, 0);
    // spec MDL-1.5: either field being negative is rejected (the guard is `width < 0 || height
    // < 0`). A negative width (a multiple of 4, so the earlier width check passes) with a zero
    // height is rejected; `&&` would wrongly accept it, since the product 0 is a valid size.
    let neg_width = MdlSpec {
        skinwidth: -4,
        skinheight: 0,
        skins: Vec::new(),
        numskins: Some(1),
        ..MdlSpec::default()
    };
    assert_eq!(
        read(&neg_width),
        Err(MdlError::BadSkinSize {
            width: -4,
            height: 0
        })
    );
}

// spec MDL-1.6, MDL-1.9: unchecked fields; the size ratio; the effect bits.
#[test]
fn unchecked_fields() {
    let spec = MdlSpec {
        flags: 0x7FFF_FFFF,
        ..MdlSpec::default()
    };
    assert_eq!(read(&spec).unwrap().header.flags, 0x7FFF_FFFF);
    assert_eq!(MDL_SIZE_RATIO.to_bits(), (1.0_f64 / 11.0).to_bits());
    // spec MDL-1.6: size() is the header size times 1/11; a header size of 22 gives 2.0,
    // not the base vector's 1.0.
    let scaled = MdlSpec {
        size: Some(22.0),
        ..MdlSpec::default()
    };
    assert_eq!(read(&scaled).unwrap().size(), 2.0);
    assert_eq!(read(&MdlSpec::default()).unwrap().size(), 1.0);
    assert_eq!(
        [
            MDL_EF_ROCKET,
            MDL_EF_GRENADE,
            MDL_EF_GIB,
            MDL_EF_ROTATE,
            MDL_EF_TRACER,
            MDL_EF_ZOMGIB,
            MDL_EF_TRACER2,
            MDL_EF_TRACER3
        ],
        [1, 2, 4, 8, 16, 32, 64, 128]
    );
    assert_eq!((MDL_ST_SYNC, MDL_ST_RAND), (0, 1));
    assert_eq!(MDL_HEADER_SIZE, 84);
}

// spec MDL-1.8: GLQuake's limits are reported.
#[test]
fn gl_limits() {
    assert_eq!(
        read(&with_verts(1025)).unwrap().gl_limits(),
        [GlLimit::Vertices]
    );
    assert!(read(&with_verts(1024)).unwrap().gl_limits().is_empty());
    let spec = MdlSpec {
        skins: vec![SkinSpec::Single(0); 33],
        ..MdlSpec::default()
    };
    assert_eq!(read(&spec).unwrap().gl_limits(), [GlLimit::Skins]);
    // spec MDL-1.8: exactly MDL_GL_MAX_SKINS skins is not over the limit.
    let spec = MdlSpec {
        skins: vec![SkinSpec::Single(0); 32],
        ..MdlSpec::default()
    };
    assert!(read(&spec).unwrap().gl_limits().is_empty());
    let spec = MdlSpec {
        triangles: vec![[1, 0, 1, 2]; 2049],
        ..MdlSpec::default()
    };
    assert_eq!(read(&spec).unwrap().gl_limits(), [GlLimit::Triangles]);
    // spec MDL-1.8: exactly MDL_GL_MAX_TRIS triangles is not over the limit.
    let spec = MdlSpec {
        triangles: vec![[1, 0, 1, 2]; 2048],
        ..MdlSpec::default()
    };
    assert!(read(&spec).unwrap().gl_limits().is_empty());
    let poses = vec![frame(b"p", [0; 4], [0; 4], vec![[0; 4]; 3]); 257];
    let spec = MdlSpec {
        frames: vec![EntrySpec::Group(
            1,
            257,
            [0; 4],
            [0; 4],
            vec![0.1; 257],
            poses,
        )],
        ..MdlSpec::default()
    };
    assert_eq!(read(&spec).unwrap().gl_limits(), [GlLimit::Poses]);
    // spec MDL-1.8: exactly MDL_GL_MAX_POSES poses is not over the limit.
    let poses = vec![frame(b"p", [0; 4], [0; 4], vec![[0; 4]; 3]); 256];
    let spec = MdlSpec {
        frames: vec![EntrySpec::Group(
            1,
            256,
            [0; 4],
            [0; 4],
            vec![0.1; 256],
            poses,
        )],
        ..MdlSpec::default()
    };
    assert!(read(&spec).unwrap().gl_limits().is_empty());
    assert_eq!(
        (
            MDL_GL_MAX_VERTS,
            MDL_GL_MAX_SKINS,
            MDL_GL_MAX_TRIS,
            MDL_GL_MAX_POSES
        ),
        (1024, 32, 2048, 256)
    );
}

// spec MDL-2.1, MDL-2.3: skin groups of any non-zero type.
#[test]
fn skin_group() {
    let spec = MdlSpec {
        counting_skin: false,
        skins: vec![
            SkinSpec::Group(1, vec![0.5, 1.0], vec![11, 12]),
            SkinSpec::Group(9, vec![0.25], vec![13]),
        ],
        ..MdlSpec::default()
    };
    let mdl = read(&spec).unwrap();
    assert_eq!(
        mdl.skins[0],
        MdlSkin::Group {
            skin_type: 1,
            intervals: vec![0.5, 1.0],
            skins: vec![&[11; 8][..], &[12; 8]]
        }
    );
    assert!(
        matches!(&mdl.skins[1], MdlSkin::Group { skin_type: 9, skins, .. } if skins == &[&[13_u8; 8][..]])
    );
    assert_eq!((MDL_SKIN_SINGLE, MDL_SKIN_GROUP), (0, 1));
}

// spec MDL-2.3: an interval of 0 fails; NaN loads.
#[test]
fn skin_intervals() {
    let spec = MdlSpec {
        skins: vec![SkinSpec::Group(1, vec![0.5, 0.0], vec![1, 2])],
        ..MdlSpec::default()
    };
    assert_eq!(error(&spec), "Mod_LoadAliasSkinGroup: interval<=0");
    let spec = MdlSpec {
        skins: vec![SkinSpec::Group(1, vec![-1.0], vec![1])],
        ..MdlSpec::default()
    };
    assert_eq!(read(&spec), Err(MdlError::SkinInterval));
    let spec = MdlSpec {
        skins: vec![SkinSpec::Group(1, vec![f32::NAN], vec![1])],
        ..MdlSpec::default()
    };
    assert!(read(&spec).is_ok());
}

// spec MDL-2.4, MDL-2.5: an empty skin group, and skins past the end, fail.
#[test]
fn skin_group_bounds() {
    let spec = MdlSpec {
        skins: vec![SkinSpec::Group(1, Vec::new(), Vec::new())],
        ..MdlSpec::default()
    };
    assert_eq!(
        read(&spec),
        Err(MdlError::InvalidSkinGroupCount { count: 0 })
    );
    let bytes = MdlSpec::default().build();
    for len in [84, 86, 90, 95] {
        assert!(
            matches!(Mdl::read(&bytes[..len]), Err(MdlError::Truncated { .. })),
            "{len}"
        );
    }
}

// spec MDL-3.2: a seam vertex of a back-facing triangle moves right by half the width.
#[test]
fn seam() {
    let mdl = read(&MdlSpec::default()).unwrap();
    let front = mdl.triangles[0];
    assert_eq!(mdl.skin_s(&front, 1), Some(1));
    let back = Triangle {
        facesfront: 0,
        ..front
    };
    assert_eq!(mdl.skin_s(&back, 1), Some(3));
    assert_eq!(mdl.skin_s(&back, 0), Some(0));
    assert_eq!(mdl.skin_s(&back, 3), None);
    assert_eq!(MDL_ALIAS_ONSEAM, 0x20);
}

// spec MDL-3.3: s and t are kept as read, beyond 16 bits.
#[test]
fn wide_texture_coordinates() {
    let spec = MdlSpec {
        stverts: vec![[0, 70_000, -70_000], [0, 0, 0], [0, 0, 0]],
        ..MdlSpec::default()
    };
    let mdl = read(&spec).unwrap();
    assert_eq!((mdl.stverts[0].s, mdl.stverts[0].t), (70_000, -70_000));
    assert_eq!(MDL_STVERT_SIZE, 12);
}

// spec MDL-4.2, MDL-4.3: facesfront is any value; vertex numbers must exist.
#[test]
fn triangles() {
    let spec = MdlSpec {
        triangles: vec![[0x10, 2, 1, 0]],
        ..MdlSpec::default()
    };
    assert_eq!(
        read(&spec).unwrap().triangles[0].facesfront,
        MDL_DT_FACES_FRONT
    );
    for bad in [3, -1] {
        let spec = MdlSpec {
            triangles: vec![[1, 0, bad, 2]],
            ..MdlSpec::default()
        };
        assert_eq!(read(&spec), Err(MdlError::BadVertexIndex { triangle: 0 }));
    }
    assert_eq!(MDL_TRIANGLE_SIZE, 16);
}

// spec MDL-5.1, MDL-5.5: frame groups of any non-zero type; an interval of 0 fails, NaN loads.
#[test]
fn frame_groups() {
    let group = |interval: f32| MdlSpec {
        frames: vec![EntrySpec::Group(
            7,
            1,
            [0; 4],
            [0; 4],
            vec![interval],
            vec![frame(b"g", [0; 4], [0; 4], vec![[0; 4]; 3])],
        )],
        ..MdlSpec::default()
    };
    assert_eq!(error(&group(0.0)), "Mod_LoadAliasGroup: interval<=0");
    let mdl = read(&group(f32::NAN)).unwrap();
    assert!(matches!(
        mdl.frames[0],
        MdlFrameEntry::Group { frame_type: 7, .. }
    ));
    assert_eq!((MDL_FRAME_SINGLE, MDL_FRAME_GROUP), (0, 1));
    assert_eq!((MDL_FRAME_HEADER_SIZE, MDL_GROUP_HEADER_SIZE), (24, 12));
}

// spec MDL-5.2: a 16-byte name with no NUL is read whole.
#[test]
fn long_frame_name() {
    let spec = MdlSpec {
        frames: vec![EntrySpec::Single(frame(
            b"abcdefghijklmnop",
            [0; 4],
            [0; 4],
            vec![[0; 4]; 3],
        ))],
        ..MdlSpec::default()
    };
    assert_eq!(read(&spec).unwrap().frames[0].name(), b"abcdefghijklmnop");
}

// spec MDL-5.3: positions from scale and origin; normal indexes past the table are reported.
#[test]
fn positions_and_normals() {
    let mdl = read(&MdlSpec::default()).unwrap();
    assert_eq!(
        mdl.position(TriVertX {
            v: [2, 4, 6],
            lightnormalindex: 0
        }),
        [-6.0, -4.0, -2.0]
    );
    // spec MDL-5.3: position is v * scale + origin; with scale 2 and origin -8, v = 2 gives
    // 2 * 2 - 8 = -4, which v / scale would not (2 / 2 - 8 = -7).
    let scaled = MdlSpec {
        scale: Some([2.0, 2.0, 2.0]),
        ..MdlSpec::default()
    };
    assert_eq!(
        read(&scaled).unwrap().position(TriVertX {
            v: [2, 2, 2],
            lightnormalindex: 0
        }),
        [-4.0, -4.0, -4.0]
    );
    let spec = MdlSpec {
        frames: vec![EntrySpec::Single(frame(
            b"f",
            [0; 4],
            [0; 4],
            vec![[0, 0, 0, 161], [0, 0, 0, 162], [0, 0, 0, 255]],
        ))],
        ..MdlSpec::default()
    };
    assert_eq!(read(&spec).unwrap().bad_normals(), [(0, 0, 1), (0, 0, 2)]);
    assert_eq!(MDL_NUMVERTEXNORMALS, 162);
}

// spec MDL-5.6: an empty frame group and frames past the end fail.
#[test]
fn frame_group_bounds() {
    let spec = MdlSpec {
        frames: vec![EntrySpec::Group(
            1,
            0,
            [0; 4],
            [0; 4],
            Vec::new(),
            Vec::new(),
        )],
        ..MdlSpec::default()
    };
    assert_eq!(
        read(&spec),
        Err(MdlError::InvalidFrameGroupCount { count: 0 })
    );
    let bytes = MdlSpec::default().build();
    assert!(matches!(
        Mdl::read(&bytes[..283]),
        Err(MdlError::Truncated { .. })
    ));
    assert!(MdlError::FrameInterval.to_string().contains("interval"));
}

// spec MDL-5.7: bytes after the last frame are ignored.
#[test]
fn trailing_bytes() {
    let mut bytes = MdlSpec::default().build();
    bytes.extend_from_slice(&[0xFF; 9]);
    assert_eq!(Mdl::read(&bytes).unwrap().frames.len(), 2);
}

proptest! {
    // spec MDL-1.5, MDL-2.5, MDL-5.6: arbitrary bytes, alone or over the base, never panic.
    #[test]
    fn arbitrary_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        let _model = Mdl::read(&bytes);
        let mut base = MdlSpec::default().build();
        for (offset, byte) in bytes.iter().enumerate().take(64) {
            let index = (offset * 7 + usize::from(*byte)) % base.len();
            base[index] = *byte;
        }
        if let Ok(model) = Mdl::read(&base) {
            let _normals = model.bad_normals();
            let _limits = model.gl_limits();
        }
    }
}
