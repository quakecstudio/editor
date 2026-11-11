// SPDX-License-Identifier: GPL-3.0-or-later

//! Half-Life's version 30 (BSP-22): the vectors of `docs/spec/vectors/bsp/`'s "Half-Life's
//! version 30" section, each giving the result its README records for QuakeSpasm's
//! `Mod_LoadBrushModel`.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::integer_division,
    reason = "test code: a failed unwrap or index is a failed test; exact sizes are the point"
)]

use super::fixtures::{BspSpec, TextureSpec};
use super::*;

fn read(spec: &BspSpec, loader: BspLoader) -> Result<Bsp<'static>, BspError> {
    Bsp::read(Vec::leak(spec.build()), loader)
}

/// The vector `version-30`: the base as version 30, its lighting three bytes a sample.
fn valve() -> BspSpec {
    BspSpec {
        version: BspVersion::Valve,
        lighting: (0..75).collect(),
        ..BspSpec::default()
    }
}

/// [`valve`] with one texture `name` and `trailer` after its mip levels.
fn valve_texture(name: &'static [u8], trailer: &'static [u8], external: bool) -> BspSpec {
    BspSpec {
        textures: Some(vec![Some(TextureSpec {
            name,
            width: 16,
            height: 16,
            pixel: 7,
            trailer,
            external,
        })]),
        ..valve()
    }
}

// spec BSP-20.20: the FitzQuake lineage's loader reads version 30; WinQuake's and GLQuake's fail
// it, and every loader fails Quake 64's, with BSP-1.2's error
// spec BSP-22.1: version 30 is read with BSP29's layout: everything but the version, the
// textures' palettes and the samples' size is the base's
#[test]
fn version_30_reads_as_bsp29() {
    let spec = valve();
    let bsp = read(&spec, BspLoader::default()).unwrap();
    assert_eq!(bsp.version, BspVersion::Valve);
    let base = read(
        &BspSpec {
            lighting: (0..25).collect(),
            ..BspSpec::default()
        },
        BspLoader::default(),
    )
    .unwrap();
    assert_eq!(bsp.planes, base.planes);
    assert_eq!(bsp.vertexes, base.vertexes);
    assert_eq!(bsp.nodes, base.nodes);
    assert_eq!(bsp.leafs, base.leafs);
    assert_eq!(bsp.faces, base.faces);
    assert_eq!(bsp.texinfo, base.texinfo);
    assert_eq!(bsp.clipnodes, base.clipnodes);
    assert_eq!(bsp.models, base.models);
    assert_eq!(bsp.entities(), base.entities());
    for loader in [BspLoader::WinQuake, BspLoader::GlQuake] {
        assert_eq!(
            read(&spec, loader),
            Err(BspError::WrongVersion { version: 30 })
        );
    }
    let q64 = i32::from_le_bytes(*b" 46Q");
    for loader in [
        BspLoader::FitzQuake,
        BspLoader::WinQuake,
        BspLoader::GlQuake,
    ] {
        let spec = BspSpec {
            raw_version: Some(q64),
            ..BspSpec::default()
        };
        assert_eq!(
            read(&spec, loader),
            Err(BspError::WrongVersion { version: q64 })
        );
    }
}

// spec BSP-22.2: a texture's palette follows its four mip levels: the 16-bit count and that
// many RGB triples (`valve-palette`); the base, whose lump ends at the mips, has none; other
// versions have no palette
#[test]
fn a_texture_keeps_its_palette() {
    let palette: &[u8] = &[2, 0, 1, 2, 3, 4, 5, 6];
    let bsp = read(
        &valve_texture(b"wall", palette, false),
        BspLoader::default(),
    )
    .unwrap();
    let texture = bsp.textures[0].as_ref().unwrap();
    assert_eq!(texture.palette, Some(palette));
    assert_eq!(texture.pixels, &[7; 256][..]);
    let base = read(&valve(), BspLoader::default()).unwrap();
    assert_eq!(base.textures[0].as_ref().unwrap().palette, Some(&[][..]));
    let quake = read(&BspSpec::default(), BspLoader::default()).unwrap();
    assert_eq!(quake.textures[0].as_ref().unwrap().palette, None);
}

// spec BSP-22.2: a palette that runs past the lump is cut at its end (`valve-palette-past`),
// and a count the lump holds only one byte of leaves that byte
#[test]
fn a_palette_past_the_lump_is_cut() {
    let past: &[u8] = &[0, 1, 9, 9, 9];
    let bsp = read(&valve_texture(b"wall", past, false), BspLoader::default()).unwrap();
    assert_eq!(bsp.textures[0].as_ref().unwrap().palette, Some(past));
    let half: &[u8] = &[3];
    let bsp = read(&valve_texture(b"wall", half, false), BspLoader::default()).unwrap();
    assert_eq!(bsp.textures[0].as_ref().unwrap().palette, Some(half));
}

// spec BSP-22.3: the lighting is RGB and `lightofs` counts bytes: a face's samples are three
// bytes each (`valve-rgb`), and a lump too short for three bytes a sample fails BSP-10.2's
// check
#[test]
fn lighting_is_rgb() {
    let mut spec = valve();
    spec.faces[0].lightofs = 3;
    spec.lighting = (0..78).collect();
    let bsp = read(&spec, BspLoader::default()).unwrap();
    assert_eq!(bsp.lighting.len(), 78);
    let samples = bsp.face_samples(&bsp.faces[0]).unwrap();
    assert_eq!(samples, &(3..78).collect::<Vec<u8>>()[..]);
    assert_eq!(BspVersion::Valve.sample_bytes(), 3);
    assert_eq!(BspVersion::Bsp29.sample_bytes(), 1);
    spec.lighting = (0..77).collect();
    assert!(matches!(
        read(&spec, BspLoader::default()),
        Err(BspError::BadIndex {
            lump: LumpId::Faces,
            ..
        })
    ));
}

// spec BSP-22.4: a `sky` texture is no sky in version 30, but its face still gets the sky's
// flags from the name (`valve-sky`)
#[test]
fn a_sky_texture_is_no_sky() {
    let bsp = read(&valve_texture(b"sky1", &[], false), BspLoader::default()).unwrap();
    assert!(!bsp.textures[0].as_ref().unwrap().sky);
    assert_eq!(
        bsp.faces[0].flags & (SURF_DRAWSKY | SURF_DRAWTILED),
        SURF_DRAWSKY | SURF_DRAWTILED
    );
    let quake = BspSpec {
        textures: Some(vec![super::fixtures::texture(b"sky1")]),
        ..BspSpec::default()
    };
    assert!(
        read(&quake, BspLoader::default()).unwrap().textures[0]
            .as_ref()
            .unwrap()
            .sky
    );
}

// spec BSP-22.5: an external texture (first offset 0) is read from the BSP when no WAD has it:
// its pixels from after the header, cut at the lump's end, here none (`valve-external`)
#[test]
fn an_external_texture_reads_what_the_bsp_holds() {
    let bsp = read(&valve_texture(b"wall", &[], true), BspLoader::default()).unwrap();
    let texture = bsp.textures[0].as_ref().unwrap();
    assert_eq!(texture.offsets, [0; 4]);
    assert!(texture.pixels.is_empty());
    assert_eq!(texture.palette, Some(&[][..]));
}

/// A WAD3 holding `wall`: 16 by 16 of index 3, its palette `palette` (the vector `wad3-wall`).
fn wad3_wall(palette: &[u8]) -> Vec<u8> {
    let mut lump = b"WALL".to_vec();
    lump.resize(16, 0);
    for value in [16_u32, 16, 40, 296, 360, 376] {
        lump.extend_from_slice(&value.to_le_bytes());
    }
    lump.extend(core::iter::repeat_n(3_u8, 340));
    lump.extend_from_slice(palette);
    let mut out = b"WAD3".to_vec();
    out.extend_from_slice(&1_i32.to_le_bytes());
    out.extend_from_slice(&(12 + i32::try_from(lump.len()).unwrap()).to_le_bytes());
    let size = i32::try_from(lump.len()).unwrap().to_le_bytes();
    out.extend_from_slice(&lump);
    out.extend_from_slice(&12_i32.to_le_bytes());
    out.extend_from_slice(&size);
    out.extend_from_slice(&size);
    out.extend_from_slice(&[67, 0, 0, 0]);
    let mut name = b"wall".to_vec();
    name.resize(16, 0);
    out.extend_from_slice(&name);
    out
}

// spec WAD3-4.3: a version 30 BSP's external texture loads its pixels, palette, name and size
// from the WAD that has it, the last named first; one no WAD has, and every texture under
// WinQuake's loader, stays the BSP's own
#[test]
fn an_external_texture_comes_from_its_wad() {
    let palette: &[u8] = &[1, 0, 9, 8, 7];
    let first = Vec::leak(wad3_wall(&[1, 0, 1, 1, 1]));
    let last = Vec::leak(wad3_wall(palette));
    let wads = [
        qs_wad::MapWad::read(first).unwrap(),
        qs_wad::MapWad::read(last).unwrap(),
    ];
    let bytes = Vec::leak(valve_texture(b"wall", &[], true).build());
    let bsp = Bsp::read_with_wads(bytes, BspLoader::default(), &wads).unwrap();
    let texture = bsp.textures[0].as_ref().unwrap();
    assert_eq!(texture.name, b"WALL", "the WAD's own header's name");
    assert_eq!(texture.pixels, &[3; 256][..]);
    assert_eq!(
        texture.palette,
        Some(palette),
        "the last WAD named is searched first"
    );
    assert!(!texture.sky);
    let alone = Bsp::read_with_wads(bytes, BspLoader::default(), &[]).unwrap();
    assert!(alone.textures[0].as_ref().unwrap().pixels.is_empty());
    let inside = Vec::leak(valve_texture(b"wall", &[], false).build());
    let own = Bsp::read_with_wads(inside, BspLoader::default(), &wads).unwrap();
    assert_eq!(
        own.textures[0].as_ref().unwrap().pixels,
        &[7; 256][..],
        "offset not 0"
    );
    let quake = Vec::leak(
        BspSpec {
            textures: Some(vec![Some(TextureSpec {
                name: b"wall",
                width: 16,
                height: 16,
                pixel: 7,
                trailer: &[],
                external: true,
            })]),
            ..BspSpec::default()
        }
        .build(),
    );
    let fitz = Bsp::read_with_wads(quake, BspLoader::default(), &wads).unwrap();
    assert_eq!(
        fitz.textures[0].as_ref().unwrap().pixels,
        &[3; 256][..],
        "BSP29 too"
    );
    // WinQuake's loader reads no WAD: it needs the mip levels the BSP lacks (BSP-4.5).
    assert_eq!(
        Bsp::read_with_wads(quake, BspLoader::WinQuake, &wads).map(|_| ()),
        Err(BspError::BadTexture { index: 0 })
    );
}

/// A WAD file: `magic`, then each texture `(name, type, pixel, trailer)` 16 by 16, cut by
/// `cut` bytes at the end.
fn wad_file(magic: &[u8; 4], textures: &[(&[u8], u8, u8, &[u8])], cut: usize) -> Vec<u8> {
    let mut data = Vec::new();
    let mut directory = Vec::new();
    for (name, ty, pixel, trailer) in textures {
        let mut lump = name.to_vec();
        lump.resize(16, 0);
        for value in [16_u32, 16, 40, 296, 360, 376] {
            lump.extend_from_slice(&value.to_le_bytes());
        }
        lump.extend(core::iter::repeat_n(*pixel, 340));
        lump.extend_from_slice(trailer);
        let filepos = i32::try_from(12 + data.len()).unwrap();
        let size = i32::try_from(lump.len()).unwrap();
        directory.extend_from_slice(&filepos.to_le_bytes());
        directory.extend_from_slice(&size.to_le_bytes());
        directory.extend_from_slice(&size.to_le_bytes());
        directory.extend_from_slice(&[*ty, 0, 0, 0]);
        let mut entry_name = name.to_ascii_lowercase();
        entry_name.resize(16, 0);
        directory.extend_from_slice(&entry_name);
        data.extend_from_slice(&lump);
    }
    let mut out = magic.to_vec();
    out.extend_from_slice(&i32::try_from(textures.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&i32::try_from(12 + data.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&data);
    out.extend_from_slice(&directory);
    out.truncate(out.len() - cut);
    out
}

/// The WAD3 vectors' files (`docs/spec/vectors/wad3/`), by the name they have on the search
/// path.
fn wad3_files() -> Vec<(&'static str, Vec<u8>)> {
    let mut full = 256_u16.to_le_bytes().to_vec();
    for n in 0..=255_u8 {
        full.extend_from_slice(&[n, 255 - n, n / 2]);
    }
    let full = Vec::leak(full);
    let p = WAD_TYP_PAL;
    vec![
        ("brick.wad", wad_file(b"WAD3", &[(b"BRICK", p, 9, full)], 0)),
        (
            "few.wad",
            wad_file(
                b"WAD3",
                &[(b"few", p, 3, &[2, 0, 1, 2, 3, 4, 5, 6, 7, 7])],
                0,
            ),
        ),
        (
            "past.wad",
            wad_file(b"WAD3", &[(b"past", p, 5, &[0, 1, 1, 2, 3, 4, 5, 6])], 0),
        ),
        (
            "trunc.wad",
            wad_file(b"WAD3", &[(b"a", p, 1, &[0, 0]), (b"b", p, 2, &[0, 0])], 20),
        ),
        (
            "badid.wad",
            wad_file(b"WAD4", &[(b"wall", p, 4, &[1, 0, 1, 1, 1])], 0),
        ),
        (
            "gfx/gfxonly.wad",
            wad_file(b"WAD3", &[(b"wall", p, 6, &[1, 0, 6, 6, 6])], 0),
        ),
        (
            "first.wad",
            wad_file(b"WAD3", &[(b"wall", p, 1, &[1, 0, 1, 1, 1])], 0),
        ),
        (
            "second.wad",
            wad_file(b"WAD3", &[(b"wall", p, 2, &[1, 0, 2, 2, 2])], 0),
        ),
        ("plain.wad", wad_file(b"WAD2", &[(b"wall", 68, 8, &[])], 0)),
    ]
}

// spec WAD3-4.3: each WAD3 vector's version 30 BSP, its external textures read from the WADs its
// wad key names through the search path, gives what the vectors' README records for QuakeSpasm:
// each texture's name, pixels' count and first byte, and palette's length
#[test]
fn the_wad3_vectors_give_their_recorded_results() {
    let files = wad3_files();
    let open = |name: &[u8]| -> Option<&'static [u8]> {
        let name = String::from_utf8_lossy(name);
        files
            .iter()
            .find(|(file, _)| *file == name)
            .map(|(_, bytes)| &*Vec::leak(bytes.clone()))
    };
    type Expected = &'static [(&'static [u8], usize, Option<u8>, usize)];
    let vectors: [(&[&'static [u8]], &str, Expected); 8] = [
        (
            &[b"brick"],
            "\\\\half-life\\\\valve\\\\brick.wad",
            &[(b"BRICK", 256, Some(9), 770)],
        ),
        (&[b"few"], "few.wad", &[(b"few", 256, Some(3), 8)]),
        (&[b"past"], "past.wad", &[(b"past", 256, Some(5), 40)]),
        (
            &[b"a", b"b"],
            "trunc.wad",
            &[(b"a", 256, Some(1), 2), (b"b", 0, None, 0)],
        ),
        (&[b"wall"], "badid.wad", &[(b"wall", 0, None, 0)]),
        (
            &[b"wall"],
            "C:/maps/gfxonly.wad",
            &[(b"wall", 256, Some(6), 5)],
        ),
        (
            &[b"wall"],
            "first.wad;second.wad",
            &[(b"wall", 256, Some(2), 5)],
        ),
        (&[b"wall"], "plain.wad", &[(b"wall", 256, Some(8), 0)]),
    ];
    for (textures, key, expected) in vectors {
        let spec = BspSpec {
            entities: format!("{{\n\"classname\" \"worldspawn\"\n\"wad\" \"{key}\"\n}}\n")
                .into_bytes(),
            textures: Some(
                textures
                    .iter()
                    .map(|name| {
                        Some(TextureSpec {
                            name,
                            width: 16,
                            height: 16,
                            pixel: 7,
                            trailer: &[],
                            external: true,
                        })
                    })
                    .collect(),
            ),
            ..valve()
        };
        let bytes = Vec::leak(spec.build());
        let entities = Bsp::read(bytes, BspLoader::default())
            .unwrap()
            .entities()
            .to_vec();
        let mut opened = Vec::new();
        for [file, gfx] in wads::wad_files(&wads::wad_key(&entities).unwrap_or_default()) {
            if let Some(Ok(wad)) = open(&file).or_else(|| open(&gfx)).map(qs_wad::MapWad::read) {
                opened.push(wad);
            }
        }
        let bsp = Bsp::read_with_wads(bytes, BspLoader::default(), &opened).unwrap();
        let got: Vec<(&[u8], usize, Option<u8>, usize)> = bsp
            .textures
            .iter()
            .map(|t| {
                let t = t.as_ref().unwrap();
                (
                    t.name.as_slice(),
                    t.pixels.len(),
                    t.pixels.first().copied(),
                    t.palette.map_or(0, <[u8]>::len),
                )
            })
            .collect();
        assert_eq!(got, expected, "{key}");
    }
}

const WAD_TYP_PAL: u8 = qs_wad::WAD_TYP_MIPTEX_PALETTE;
