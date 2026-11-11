// SPDX-License-Identifier: GPL-3.0-or-later

//! Half-Life's version 30 (BSP-22): the vectors of `docs/spec/vectors/bsp/`'s "Half-Life's
//! version 30" section, each giving the result its README records for QuakeSpasm's
//! `Mod_LoadBrushModel`.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
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
