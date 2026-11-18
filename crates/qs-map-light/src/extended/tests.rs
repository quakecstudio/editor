// SPDX-License-Identifier: GPL-3.0-or-later

//! Rooms compiled and lit in the extended dialect: what each of ericw-tools' lighting
//! features does to a map (LIGHT-8 to LIGHT-20).

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::float_cmp,
    clippy::float_cmp_const,
    clippy::arithmetic_side_effects,
    clippy::modulo_arithmetic,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "test code: a failed unwrap or index is a failed test, and the numbers are small \
              and exact"
)]

use qs_bsp::{Bsp, BspLoader, Face};
use qs_map_bsp::Compiler;
use qs_wad::write::WadBuilder;

use super::bounce::{Emitter, Stage};
use super::dirt::Dirt;
use super::keys::{Dict, World};
use super::lights::Lights;
use super::math::Plane;
use super::phong::Smoothing;
use super::points::{Extents, Patch, Sample};
use super::rays::{Passed, Rays, Texels as _};
use super::scene::{Given, Map, MapFault};
use super::shade::{Answers, Shader};
use crate::face::FaceId;
use crate::tests::{cuboid, light_at, miptex};
use crate::{
    Extras, Jitter, LeafKind, Light, LightError, LightWarning, Lighted, Sampling, SidecarFile,
    SurfaceFlags, Vec3,
};

/// The textures the rooms are built of.
const TEXTURES: [&str; 7] = ["wall", "sky1", "sky4", "*water", "{fence", "glow", "trim"];

/// A room 256 by 256 by 128 inside, its ceiling of `ceiling`, with these keys on the world,
/// these other brushes in it and these other entities.
fn room(world_keys: &str, ceiling: &str, brushes: &str, entities: &str) -> Vec<u8> {
    let mut map = format!("{{\n\"classname\" \"worldspawn\"\n\"wad\" \"test.wad\"\n{world_keys}");
    map.push_str(&cuboid([-16, -16, -16], [272, 272, 0], "wall"));
    map.push_str(&cuboid([-16, -16, 128], [272, 272, 144], ceiling));
    for (min, max) in [
        ([-16, -16, 0], [0, 272, 128]),
        ([256, -16, 0], [272, 272, 128]),
        ([0, -16, 0], [256, 0, 128]),
        ([0, 256, 0], [256, 272, 128]),
    ] {
        map.push_str(&cuboid(min, max, "wall"));
    }
    map.push_str(brushes);
    map.push_str("}\n{\n\"classname\" \"info_player_start\"\n\"origin\" \"32 32 32\"\n}\n");
    map.push_str(entities);
    let mut wad = WadBuilder::new();
    for name in TEXTURES {
        wad.add(name.as_bytes(), qs_wad::WAD_TYP_MIPTEX, miptex(name))
            .unwrap();
    }
    Compiler::default()
        .compile(map.as_bytes(), Some(&wad.to_bytes().unwrap()))
        .unwrap()
        .bsp
}

/// A brush entity of `classname` with these keys and one brush.
fn brush_entity(
    classname: &str,
    keys: &str,
    min: [i32; 3],
    max: [i32; 3],
    texture: &str,
) -> String {
    format!(
        "{{\n\"classname\" \"{classname}\"\n{keys}{}}}\n",
        cuboid(min, max, texture)
    )
}

fn lit(bsp: &[u8]) -> Lighted {
    Light::default().run(bsp).unwrap()
}

/// The way a face faces.
fn normal_of(bsp: &Bsp<'_>, face: &Face) -> Vec3 {
    let normal = bsp.planes[face.planenum].normal;
    if face.side == 0 {
        normal
    } else {
        normal.map(|c| -c)
    }
}

/// The first lightmap of each lit face of the world that faces along `normal`.
fn lightmaps(bytes: &[u8], normal: Vec3) -> Vec<Vec<u8>> {
    let bsp = Bsp::read(bytes, BspLoader::FitzQuake).unwrap();
    let world = &bsp.models[0];
    bsp.faces[world.firstface..world.firstface + world.numfaces]
        .iter()
        .filter(|face| normal_of(&bsp, face) == normal)
        .filter_map(|face| {
            let at = usize::try_from(face.lightofs).ok()?;
            let size = face.lightmap_size[0] * face.lightmap_size[1];
            Some(bsp.lighting[at..at + size].to_vec())
        })
        .collect()
}

const UP: Vec3 = [0.0, 0.0, 1.0];
const DOWN: Vec3 = [0.0, 0.0, -1.0];

/// The floor's lightmap: the room's one face that faces up.
fn floor(bytes: &[u8]) -> Vec<u8> {
    lightmaps(bytes, UP).into_iter().flatten().collect()
}

/// The luxel of the world's face facing up that holds the floor point `x`, `y`; none when
/// the face there is unlit.
fn floor_at(bytes: &[u8], x: f32, y: f32) -> Option<u8> {
    let bsp = Bsp::read(bytes, BspLoader::FitzQuake).unwrap();
    let world = &bsp.models[0];
    for face in &bsp.faces[world.firstface..world.firstface + world.numfaces] {
        if normal_of(&bsp, face) != UP {
            continue;
        }
        let vecs = bsp.texinfo[face.texinfo].vecs;
        let luxel = |axis: usize| {
            let texel = vecs[axis][0] * x + vecs[axis][1] * y + vecs[axis][3];
            (texel - f32::from(face.texturemins[axis])) / 16.0
        };
        let (s, t) = (luxel(0), luxel(1));
        let [width, height] = face.lightmap_size;
        let inside = |c: f32, size: usize| c >= 0.0 && c.fract() == 0.0 && (c as usize) < size;
        if !inside(s, width) || !inside(t, height) {
            continue;
        }
        let start = usize::try_from(face.lightofs).ok()?;
        return Some(bsp.lighting[start + (t as usize) * width + s as usize]);
    }
    None
}

/// What lighting a compiled room is made of, for looking inside it.
struct Parts<'a> {
    map: Map<'a>,
    lights: Lights,
    rays: Rays,
}

fn parts<'a>(read: &'a Bsp<'a>, extras: Extras<'a>) -> Parts<'a> {
    let mut lights = Lights::read(read.entity_lump).unwrap();
    let map = Map::new(
        read,
        lights.world.clone(),
        &lights.dicts,
        extras.surfaces.to_vec(),
        &Given {
            switches: lights.shadow_switches(),
            leaves: extras.leaves,
            palette: extras.palette,
        },
    )
    .unwrap();
    let rays = Rays::new(map.casters());
    lights.set_up(&map, &rays, Jitter::Bsd);
    Parts { map, lights, rays }
}

/// The number of the world's face that faces along `normal`.
fn face_facing(read: &Bsp<'_>, normal: Vec3) -> usize {
    read.faces
        .iter()
        .position(|face| normal_of(read, face) == normal)
        .unwrap()
}

// spec LIGHT-8.1: the extended dialect lights a BSP as ericw-tools does, from the map's keys.
// spec LIGHT-11.2: a face whose texture's name starts with sky is sky, which sunlight comes
// from.
// spec LIGHT-14.7: a sample gets a sun's brightness when the ray along it reaches sky first.
#[test]
fn sunlight_reaches_what_sees_the_sky() {
    let sunny = lit(&room("\"_sunlight\" \"200\"\n", "sky1", "", ""));
    // Straight down on a floor: 200 whole, by the range of a half.
    assert!(floor(&sunny.bsp).iter().all(|&luxel| luxel == 100));
    assert!(sunny.lit.is_none(), "white light writes no .lit");
    let roofed = lit(&room("\"_sunlight\" \"200\"\n", "wall", "", ""));
    assert!(floor(&roofed.bsp).is_empty(), "no sky, no sunlight");
    // A sun from the side lights the wall it faces, and the floor by half its brightness
    // at a grazing angle, less the part of the floor the far wall shadows.
    let low = lit(&room(
        "\"_sunlight\" \"200\"\n\"_sun_mangle\" \"0 -45 0\"\n",
        "sky1",
        "",
        "",
    ));
    let floor = floor(&low.bsp);
    assert_eq!(
        *floor.iter().max().unwrap(),
        85,
        "200 * (0.5 + 0.5 * cos 45) / 2"
    );
    assert!(
        floor.contains(&0),
        "the wall toward the sun shadows the floor by it"
    );
}

// spec LIGHT-15.1: the dome lights from every way above the horizon.
#[test]
fn the_dome_lights_from_all_above() {
    let domed = lit(&room("\"_sunlight2\" \"130\"\n", "sky1", "", ""));
    let middle = floor_at(&domed.bsp, 112.0, 112.0).unwrap();
    assert!(
        middle > 20,
        "the middle of the floor sees most of the sky: {middle}"
    );
    assert!(
        floor_at(&domed.bsp, 16.0, 112.0).unwrap() < middle,
        "by a wall half the dome is hidden"
    );
    assert!(
        !lightmaps(&domed.bsp, [1.0, 0.0, 0.0]).is_empty(),
        "the walls see the dome too"
    );
}

// spec LIGHT-17.1: every sample of a face is raised to the minimum light, in its colour.
// spec LIGHT-18.1: a sample is scaled by _range, bent by _gamma and held to _maxlight.
// spec LIGHT-18.6: a .lit is written only when a sample has a colour.
#[test]
fn minimum_light_raises_every_sample() {
    let grey = lit(&room("\"light\" \"50\"\n", "wall", "", ""));
    assert!(floor(&grey.bsp).iter().all(|&luxel| luxel == 25));
    assert!(grey.lit.is_none());
    let ranged = lit(&room(
        "\"light\" \"50\"\n\"_range\" \"1\"\n",
        "wall",
        "",
        "",
    ));
    assert!(floor(&ranged.bsp).iter().all(|&luxel| luxel == 50));
    let capped = lit(&room(
        "\"light\" \"200\"\n\"_maxlight\" \"40\"\n",
        "wall",
        "",
        "",
    ));
    assert!(
        floor(&capped.bsp).iter().all(|&luxel| luxel == 40),
        "twice the maximum, by the range"
    );
    let bent = lit(&room(
        "\"light\" \"128\"\n\"_gamma\" \"2\"\n",
        "wall",
        "",
        "",
    ));
    // 64 over 255, to the half power, times 255.
    assert!(floor(&bent.bsp).iter().all(|&luxel| luxel == 127));
    let red = lit(&room(
        "\"light\" \"50\"\n\"_minlight_color\" \"1 0 0\"\n",
        "wall",
        "",
        "",
    ));
    let colours = red.lit.unwrap();
    assert_eq!(&colours[8..11], [25, 0, 0]);
    assert!(floor(&red.bsp).iter().all(|&luxel| luxel == 25));
}

// spec LIGHT-10.9: a sample gets a light's brightness at its distance, by the angle it meets
// the sample at.
// spec LIGHT-12.3: a sample lies a unit in front of its face.
// spec LIGHT-17.2: with _addmin the minimum light is added to a sample's light.
#[test]
fn a_light_falls_with_distance_and_angle() {
    let one = lit(&room("", "wall", "", &light_at("128 128 64", "")));
    let below = floor_at(&one.bsp, 128.0, 128.0).unwrap();
    // 300 less the 63 units to the sample a unit above the floor, whole, by a half.
    assert_eq!(below, 118);
    assert!(floor_at(&one.bsp, 32.0, 112.0).unwrap() < below);
    let raised = lit(&room(
        "\"light\" \"100\"\n",
        "wall",
        "",
        &light_at("128 128 64", ""),
    ));
    assert_eq!(
        floor_at(&raised.bsp, 128.0, 128.0),
        Some(118),
        "the light is over the minimum"
    );
    let added = lit(&room(
        "\"light\" \"100\"\n\"_addmin\" \"1\"\n",
        "wall",
        "",
        &light_at("128 128 64", ""),
    ));
    assert_eq!(floor_at(&added.bsp, 128.0, 128.0), Some(168));
    let inverse = lit(&room(
        "",
        "wall",
        "",
        &light_at("128 128 64", "\"delay\" \"2\"\n\"light\" \"50\"\n"),
    ));
    // 50 over (63 / 128) squared, by a half.
    assert_eq!(floor_at(&inverse.bsp, 128.0, 128.0), Some(103));
}

// spec LIGHT-8.4: lights under 0 take light away after the minimum light is given.
// spec LIGHT-17.3: a local minimum light raises the samples it sees to its brightness.
#[test]
fn passes_run_in_order() {
    let dimmed = lit(&room(
        "\"light\" \"100\"\n",
        "wall",
        "",
        &light_at("128 128 64", "\"light\" \"-80\"\n"),
    ));
    assert!(
        floor_at(&dimmed.bsp, 128.0, 128.0).unwrap() < 50,
        "the minimum is not put back after the light is taken"
    );
    assert_eq!(floor(&dimmed.bsp).iter().max(), Some(&50));
    let local = lit(&room(
        "",
        "wall",
        &cuboid([120, 0, 0], [136, 256, 64], "wall"),
        &light_at("64 128 96", "\"delay\" \"4\"\n\"light\" \"80\"\n"),
    ));
    let floor_light = floor(&local.bsp);
    assert!(
        floor_light.contains(&40),
        "what the light sees is raised to 80"
    );
    assert!(
        floor_light.iter().all(|&luxel| luxel == 40),
        "and no face it does not see has a lightmap"
    );
}

// spec LIGHT-8.2: the full mode takes four samples a luxel and writes a lightmap of the same
// size.
// spec LIGHT-12.2: in the fast mode a sample is at its luxel's own coordinates.
// spec LIGHT-12.1: a face's lightmap spans its texture coordinates in luxels of 16.
#[test]
fn the_full_mode_averages_four_samples() {
    let bsp = room("", "wall", "", &light_at("128 128 64", ""));
    let fast = Light::new(Sampling::Fast).run(&bsp).unwrap();
    let full = Light::new(Sampling::Extra).run(&bsp).unwrap();
    let (fast_floor, full_floor) = (floor(&fast.bsp), floor(&full.bsp));
    assert_eq!(full_floor.len(), fast_floor.len());
    for (a, b) in fast_floor.iter().zip(&full_floor) {
        assert!(a.abs_diff(*b) <= 6, "{a} against {b}");
    }
    let read = Bsp::read(&bsp, BspLoader::FitzQuake).unwrap();
    let made = parts(&read, Extras::default());
    let smoothing = Smoothing::new(&made.map);
    let floor_face = face_facing(&read, UP);
    let patch = Patch::new(
        &made.map,
        &smoothing,
        FaceId::new(floor_face),
        Sampling::Fast,
    )
    .unwrap();
    let face = &read.faces[floor_face];
    assert_eq!([patch.width, patch.height], face.lightmap_size);
    // A sample well inside the face lies on the 16 unit grid, a unit above the floor.
    let inner = patch.samples[patch.width + 1];
    assert_eq!(inner.point[2], 1.0);
    assert!(inner.point[0] % 16.0 == 0.0 && inner.point[1] % 16.0 == 0.0);
    assert_eq!(inner.normal, UP);
    let finer = Patch::new(
        &made.map,
        &smoothing,
        FaceId::new(floor_face),
        Sampling::Extra,
    )
    .unwrap();
    assert_eq!(
        [finer.width, finer.height],
        face.lightmap_size.map(|n| n * 2)
    );
    // The four samples of that luxel are a quarter of a luxel from it each way.
    let first = finer.samples[(2 * finer.width) + 2].point;
    assert_eq!(first[2], 1.0);
    assert_eq!((first[0] - inner.point[0]).abs(), 4.0);
    assert_eq!((first[1] - inner.point[1]).abs(), 4.0);
}

// spec LIGHT-12.6: a sample off its face snaps to the face's edge within a luxel, and is
// hidden farther.
// spec LIGHT-12.7: a sample in solid is moved half a unit on each axis to open space.
#[test]
fn samples_at_the_edges_are_moved_out_of_solid() {
    let bsp = room("", "wall", "", &light_at("128 128 64", ""));
    let read = Bsp::read(&bsp, BspLoader::FitzQuake).unwrap();
    let made = parts(&read, Extras::default());
    let smoothing = Smoothing::new(&made.map);
    let patch = Patch::new(
        &made.map,
        &smoothing,
        FaceId::new(face_facing(&read, UP)),
        Sampling::Fast,
    )
    .unwrap();
    // The samples along the walls are wanted on the walls' planes, which is in solid: each
    // is moved half a unit on every axis.
    assert!(patch.samples.iter().all(|sample| !sample.occluded));
    assert!(
        patch
            .samples
            .iter()
            .all(|sample| !made.map.in_any_solid(0, sample.point))
    );
    let moved = patch
        .samples
        .iter()
        .filter(|sample| sample.point[2] != 1.0)
        .count();
    assert!(moved > 0 && moved < patch.samples.len());
    assert!(
        patch
            .samples
            .iter()
            .all(|sample| (sample.point[2] - 1.0).abs() <= 0.5)
    );
    assert!(
        made.map.in_any_solid(0, [0.0, 0.0, 1.0]),
        "on the walls' planes"
    );
    assert!(
        made.map.in_any_solid(0, [-8.0, 64.0, 64.0]),
        "inside a wall"
    );
    assert!(!made.map.in_any_solid(0, [64.0, 64.0, 64.0]));
    // A wall's lightmap runs 16 units past its top and bottom: those rows snap to its edge.
    let wall = face_facing(&read, [1.0, 0.0, 0.0]);
    let wall_patch = Patch::new(&made.map, &smoothing, FaceId::new(wall), Sampling::Fast).unwrap();
    assert!(wall_patch.samples.iter().all(|sample| !sample.occluded));
    assert!(
        wall_patch
            .samples
            .iter()
            .all(|sample| (0.0..=128.0).contains(&sample.point[2]))
    );
}

// spec LIGHT-18.2: a face keeps at most four styles, the brightest first, and none whose
// brightest sample is under 1.
#[test]
fn the_brightest_four_styles_are_kept() {
    let lights: String = (1..=5)
        .map(|style| {
            light_at(
                "128 128 64",
                &format!(
                    "\"style\" \"{style}\"\n\"light\" \"{}\"\n",
                    100 + style * 20
                ),
            )
        })
        .collect();
    let five = lit(&room("", "wall", "", &lights));
    let read = Bsp::read(&five.bsp, BspLoader::FitzQuake).unwrap();
    let floor_face = &read.faces[face_facing(&read, UP)];
    assert_eq!(floor_face.styles, [5, 4, 3, 2]);
    let dim = lit(&room(
        "",
        "wall",
        "",
        &light_at("128 128 64", "\"light\" \"1\"\n"),
    ));
    assert!(floor(&dim.bsp).is_empty(), "nothing as bright as 1");
}

// spec LIGHT-8.7: an LMSHIFT lump's scale for each face is not read yet: a BSP that carries
// one is lit at the world's scale, and the lump is kept.
#[test]
fn a_face_scale_lump_is_not_read_yet() {
    let plain = room("", "wall", "", &light_at("128 128 64", ""));
    let faces = Bsp::read(&plain, BspLoader::FitzQuake).unwrap().faces.len();
    let mut shifted = plain.clone();
    let mut lumps = qs_bsp::Bspx::default();
    lumps.set(b"LMSHIFT", vec![3; faces]);
    lumps.append_to(&mut shifted).unwrap();
    let (plain, shifted) = (lit(&plain), lit(&shifted));
    assert_eq!(floor(&shifted.bsp), floor(&plain.bsp));
    assert_eq!(
        qs_bsp::Bspx::read(&shifted.bsp).get(b"LMSHIFT"),
        Some(&vec![3; faces][..])
    );
}

// spec LIGHT-9.7: a brush model takes its entity's settings; a model with no entity stops
// the run.
// spec LIGHT-11.1: a model casts shadow only with _shadow; the world always does.
#[test]
fn brush_models_take_their_entitys_keys() {
    let pillar = |keys: &str| {
        room(
            "",
            "wall",
            "",
            &format!(
                "{}{}",
                light_at("32 128 32", ""),
                brush_entity("func_wall", keys, [120, 96, 0], [136, 160, 128], "trim")
            ),
        )
    };
    // Behind the pillar from the light, on the floor.
    let behind = |lighted: &Lighted| floor_at(&lighted.bsp, 208.0, 112.0).unwrap_or(0);
    let plain = lit(&pillar(""));
    let shadowing = lit(&pillar("\"_shadow\" \"1\"\n"));
    assert!(behind(&plain) > 0, "a model without _shadow stops no light");
    assert_eq!(behind(&shadowing), 0);
    let world_pillar = lit(&room(
        "",
        "wall",
        &cuboid([120, 96, 0], [136, 160, 128], "trim"),
        &light_at("32 128 32", ""),
    ));
    let mut behind_world = lightmaps(&world_pillar.bsp, UP).into_iter().flatten();
    assert!(
        behind_world.any(|luxel| luxel == 0),
        "a brush of the world shadows the floor"
    );

    let bright = lit(&pillar("\"_minlight\" \"90\"\n"));
    let read = Bsp::read(&bright.bsp, BspLoader::FitzQuake).unwrap();
    let model = &read.models[1];
    for face in &read.faces[model.firstface..model.firstface + model.numfaces] {
        let start = usize::try_from(face.lightofs).unwrap();
        let size = face.lightmap_size[0] * face.lightmap_size[1];
        assert!(read.lighting[start..start + size].iter().all(|&l| l >= 45));
    }
    let bsp = pillar("");
    let read = Bsp::read(&bsp, BspLoader::FitzQuake).unwrap();
    let orphan = Map::new(
        &read,
        World::default(),
        &[Dict::default()],
        Vec::new(),
        &Given {
            switches: Vec::new(),
            leaves: &[],
            palette: None,
        },
    );
    assert_eq!(orphan.err(), Some(MapFault::NoEntityForModel(1)));
    assert_eq!(
        LightError::NoEntityForModel { model: 1 }.to_string(),
        "Couldn't find entity for model *1."
    );
}

// spec LIGHT-9.8: a texinfo's flags reach light beside the BSP: its minimum light, and
// whether it takes any.
// spec LIGHT-9.4: a world key whose value does not read is warned of.
#[test]
fn texinfo_flags_set_a_faces_light() {
    let bsp = room("\"_sunlight\" \"bright\"\n", "wall", "", "");
    let read = Bsp::read(&bsp, BspLoader::FitzQuake).unwrap();
    let flags = |flag: SurfaceFlags| vec![flag; read.texinfo.len()];
    let run = |surfaces: &[SurfaceFlags]| {
        Light::default()
            .run_with(
                &bsp,
                Extras {
                    surfaces,
                    ..Extras::default()
                },
            )
            .unwrap()
    };
    let raised = run(&flags(SurfaceFlags {
        minlight: Some(60.0),
        minlight_color: [255, 0, 0],
        ..SurfaceFlags::default()
    }));
    assert!(floor(&raised.bsp).iter().all(|&luxel| luxel == 30));
    assert_eq!(&raised.lit.unwrap()[8..11], [30, 0, 0]);
    assert_eq!(
        raised.warnings,
        [LightWarning::InvalidWorldKey {
            key: "_sunlight".to_owned()
        }]
    );
    assert_eq!(
        raised.warnings[0].to_string(),
        "WARNING: worldspawn key _sunlight has invalid value"
    );
    let excluded = run(&flags(SurfaceFlags {
        minlight: Some(60.0),
        no_minlight: true,
        ..SurfaceFlags::default()
    }));
    assert!(floor(&excluded.bsp).is_empty());
}

// spec LIGHT-9.9: a leaf's own contents, given beside the BSP, say whether a sample may lie
// in it, whatever the BSP's contents.
#[test]
fn leaf_kinds_say_where_samples_may_lie() {
    let bsp = room("", "wall", "", &light_at("128 128 64", ""));
    let read = Bsp::read(&bsp, BspLoader::FitzQuake).unwrap();
    let with = |kind: LeafKind| {
        let leaves = vec![kind; read.leafs.len()];
        let extras = Extras {
            leaves: &leaves,
            ..Extras::default()
        };
        let made = parts(&read, extras);
        (
            made.map.in_solid(0, [-8.0, 64.0, 64.0]),
            made.map.in_solid(0, [64.0, 64.0, 64.0]),
        )
    };
    assert_eq!(with(LeafKind::Native), (true, false));
    assert_eq!(with(LeafKind::Open), (false, false));
    assert_eq!(with(LeafKind::Closed), (true, true));
}

// spec LIGHT-10.4: lights are scattered before their targets are matched, so each copy is
// aimed.
// spec LIGHT-10.7: a light in solid is moved 2 units to open space.
// spec LIGHT-10.8: without visibility what a light sees is the box its rays reach, grown by
// a quarter.
#[test]
fn lights_are_set_up_in_order() {
    let bsp = room(
        "",
        "wall",
        "",
        &format!(
            "{}{}{}",
            light_at(
                "128 128 96",
                "\"target\" \"t\"\n\"_deviance\" \"4\"\n\"_samples\" \"3\"\n\"delay\" \"1\"\n"
            ),
            "{\n\"classname\" \"info_null\"\n\"targetname\" \"t\"\n\"origin\" \"128 128 0\"\n}\n",
            light_at("-1 128 64", ""),
        ),
    );
    let read = Bsp::read(&bsp, BspLoader::FitzQuake).unwrap();
    let made = parts(&read, Extras::default());
    assert_eq!(made.lights.lamps.len(), 4);
    let aimed = made
        .lights
        .lamps
        .iter()
        .filter(|lamp| lamp.spot.is_some())
        .count();
    assert_eq!(aimed, 3, "the scattered copies are spotlights too");
    let in_wall = &made.lights.lamps[1];
    assert_eq!(
        in_wall.origin(),
        [1.0, 128.0, 64.0],
        "moved 2 units out of the wall"
    );
    let sees = made
        .rays
        .visible_bounds(&made.map, [128.0, 128.0, 64.0], &mut Vec::new());
    // The room is 256 by 256 by 128: the box of it, grown by a quarter of its size.
    let near = |a: Vec3, b: Vec3| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.001);
    assert!(near(sees.mins, [-64.0, -64.0, -32.0]), "{:?}", sees.mins);
    assert!(near(sees.maxs, [320.0, 320.0, 160.0]), "{:?}", sees.maxs);
    assert!(made.lights.lamps.iter().all(|lamp| lamp.bounds.is_some()));
}

// spec LIGHT-11.2: a liquid face of the world casts no shadow.
#[test]
fn the_worlds_liquids_cast_no_shadow() {
    let pool = lit(&room(
        "",
        "wall",
        &cuboid([0, 0, 0], [256, 256, 32], "*water"),
        &light_at("128 128 96", ""),
    ));
    assert!(
        floor(&pool.bsp).iter().any(|&luxel| luxel > 0),
        "the light reaches the floor through the water"
    );
}

// spec LIGHT-13.2: a face is smoothed within its phong angle.
// spec LIGHT-13.3: faces that share a vertex and turn less than the angle are smoothed.
// spec LIGHT-13.4: a corner's normal is the faces' normals summed by area and angle.
// spec LIGHT-13.6: a face with a phong angle is curved.
#[test]
fn phong_smooths_the_corners_of_a_model() {
    let bsp = room(
        "",
        "wall",
        "",
        &format!(
            "{}{}",
            light_at("32 128 64", ""),
            brush_entity(
                "func_wall",
                "\"_phong\" \"1\"\n\"_phong_angle\" \"100\"\n",
                [112, 112, 0],
                [144, 144, 64],
                "trim"
            )
        ),
    );
    let read = Bsp::read(&bsp, BspLoader::FitzQuake).unwrap();
    let made = parts(&read, Extras::default());
    let smoothing = Smoothing::new(&made.map);
    let model = &read.models[1];
    let side = (model.firstface..model.firstface + model.numfaces)
        .find(|&face| normal_of(&read, &read.faces[face]) == [-1.0, 0.0, 0.0])
        .unwrap();
    let texinfo = read.faces[side].texinfo;
    assert_eq!(made.map.flags[texinfo].phong_angle, 100.0);
    let shape = &smoothing.shapes[side];
    // Each corner of the side is shared with a neighbouring side and the top or the bottom,
    // all at right angles: within 100 degrees, so its normal leans toward them.
    for normal in &shape.normals {
        assert!(normal[0] < -0.3 && normal[0] > -0.95, "{normal:?}");
        assert!(normal[1].abs() > 0.3);
    }
    let patch = Patch::new(&made.map, &smoothing, FaceId::new(side), Sampling::Fast).unwrap();
    assert!(patch.curved);
    let world_floor = Patch::new(
        &made.map,
        &smoothing,
        FaceId::new(face_facing(&read, UP)),
        Sampling::Fast,
    );
    assert!(!world_floor.unwrap().curved);
    // With an angle under a right angle nothing is smoothed.
    let sharp = room(
        "",
        "wall",
        "",
        &brush_entity(
            "func_wall",
            "\"_phong\" \"1\"\n\"_phong_angle\" \"45\"\n",
            [112, 112, 0],
            [144, 144, 64],
            "trim",
        ),
    );
    let read = Bsp::read(&sharp, BspLoader::FitzQuake).unwrap();
    let made = parts(&read, Extras::default());
    let smoothing = Smoothing::new(&made.map);
    let model = &read.models[1];
    for face in model.firstface..model.firstface + model.numfaces {
        let normal = normal_of(&read, &read.faces[face]);
        assert!(smoothing.shapes[face].normals.iter().all(|n| *n == normal));
    }
}

// spec LIGHT-16.1: a light with _surface gives its light to every face of that texture, in
// any case, and none itself.
// spec LIGHT-16.3: a copy is put 2 units above the middle of each part of the face, or
// _surface_offset.
#[test]
fn surface_lights_are_put_over_their_faces() {
    let panel = cuboid([96, 96, 0], [160, 160, 8], "glow");
    let bsp = room(
        "",
        "wall",
        &panel,
        &light_at("0 0 0", "\"_surface\" \"GLOW\"\n\"light\" \"150\"\n"),
    );
    let read = Bsp::read(&bsp, BspLoader::FitzQuake).unwrap();
    let made = parts(&read, Extras::default());
    let template = &made.lights.lamps[0];
    assert_eq!(template.keys.light.get(), 0.0);
    let copies: Vec<Vec3> = made.lights.lamps[1..]
        .iter()
        .map(super::lights::Lamp::origin)
        .collect();
    // The panel's top and its four sides, in as many parts as the BSP cut them into: each
    // copy is 2 units off the panel, over one of its faces.
    assert!(copies.len() >= 5);
    let over_top = copies.iter().filter(|copy| copy[2] == 10.0).count();
    assert!(over_top >= 1);
    assert!(copies.iter().all(|copy| {
        let off = |c: f32| c == 94.0 || c == 162.0;
        copy[2] == 10.0 || off(copy[0]) || off(copy[1])
    }));
    let lighted = lit(&bsp);
    assert!(
        lightmaps(&lighted.bsp, DOWN)
            .iter()
            .flatten()
            .any(|&luxel| luxel > 0),
        "the ceiling is lit from the panel"
    );
    let raised = room(
        "",
        "wall",
        &panel,
        &light_at(
            "0 0 0",
            "\"_surface\" \"glow\"\n\"_surface_offset\" \"16\"\n\"_surface_spotlight\" \"1\"\n",
        ),
    );
    let read = Bsp::read(&raised, BspLoader::FitzQuake).unwrap();
    let made = parts(&read, Extras::default());
    let top = made
        .lights
        .lamps
        .iter()
        .find(|lamp| lamp.origin()[2] == 24.0)
        .unwrap();
    assert_eq!(top.spot.unwrap().vec, UP);
}

// spec LIGHT-17.4: a brush model's hidden samples are raised to the light at its middle.
// spec LIGHT-17.5: the light at a point is what each light gives it with nothing in the way,
// scaled as a sample's is.
#[test]
fn a_models_hidden_samples_take_the_light_at_its_middle() {
    // A wall sunk half into the room's own wall: the samples of its end in the wall are
    // hidden.
    let bsp = room(
        "",
        "wall",
        "",
        &format!(
            "{}{}",
            light_at("128 64 64", ""),
            brush_entity("func_wall", "", [-8, 96, 0], [64, 160, 64], "trim")
        ),
    );
    let read = Bsp::read(&bsp, BspLoader::FitzQuake).unwrap();
    let made = parts(&read, Extras::default());
    let smoothing = Smoothing::new(&made.map);
    let shader = Shader {
        map: &made.map,
        rays: &made.rays,
        lights: &made.lights,
        dirt: None,
        surface_lit: &[],
    };
    let model = &read.models[1];
    let top = (model.firstface..model.firstface + model.numfaces)
        .find(|&face| normal_of(&read, &read.faces[face]) == UP)
        .unwrap();
    let mut patch = Patch::new(&made.map, &smoothing, FaceId::new(top), Sampling::Fast).unwrap();
    assert!(patch.samples.iter().any(|sample| sample.occluded));
    let hidden: Vec<usize> = (0..patch.samples.len())
        .filter(|&i| patch.samples[i].occluded)
        .collect();
    let mut stack = Vec::new();
    let mut lightmaps = shader.direct(&mut patch, &[], &mut stack);
    let emitters: Vec<Option<Emitter>> = Vec::new();
    shader.post(&mut patch, &mut lightmaps, &emitters, &mut stack);
    assert!(patch.samples.iter().all(|sample| !sample.occluded));
    let filled = lightmaps.all[0].samples[hidden[0]];
    // The model's middle is at 28 128 32 and the light 100, -64, 32 from it: 300 less that
    // distance, counted by the side of a cube it lights most, the one along x, by the range.
    let away = (100.0_f32 * 100.0 + 64.0 * 64.0 + 32.0 * 32.0).sqrt();
    let expected = (300.0 - away) * (1.0 + (100.0 / away - 1.0) * 0.5) * 0.5;
    assert!(
        (filled[0] - expected).abs() < 0.01,
        "{filled:?} against {expected}"
    );
    assert_eq!(filled[0], filled[1]);
}

// spec LIGHT-19.3: dirt darkens a sample by how much of the space about it is closed off.
#[test]
fn dirt_darkens_corners() {
    let clean = lit(&room("\"light\" \"100\"\n", "wall", "", ""));
    let dirty = lit(&room(
        "\"light\" \"100\"\n\"_dirt\" \"1\"\n",
        "wall",
        "",
        "",
    ));
    let (clean, dirty) = (floor(&clean.bsp), floor(&dirty.bsp));
    assert!(clean.iter().all(|&luxel| luxel == 50));
    assert!(dirty.iter().all(|&luxel| luxel <= 50));
    let (clean_bsp, dirty_bsp) = (
        room("\"light\" \"100\"\n", "wall", "", ""),
        room("\"light\" \"100\"\n\"_dirt\" \"1\"\n", "wall", "", ""),
    );
    let dirty_at = |x: f32, y: f32| floor_at(&lit(&dirty_bsp).bsp, x, y).unwrap();
    assert_eq!(floor_at(&lit(&clean_bsp).bsp, 16.0, 16.0), Some(50));
    let (corner, by_wall, open) = (
        dirty_at(16.0, 16.0),
        dirty_at(16.0, 112.0),
        dirty_at(112.0, 112.0),
    );
    assert!(
        corner < by_wall,
        "the corner is closed in more: {corner} {by_wall}"
    );
    assert!(by_wall < open, "{by_wall} {open}");
    let bsp = room("\"light\" \"100\"\n\"_dirt\" \"1\"\n", "wall", "", "");
    let read = Bsp::read(&bsp, BspLoader::FitzQuake).unwrap();
    let made = parts(&read, Extras::default());
    assert!(Dirt::new(&made.map.world, &made.lights.lamps, Jitter::Bsd).is_some());
}

// spec LIGHT-20.1: light bounces off the faces that cast shadow, _bounce times.
// spec LIGHT-20.5: bounced light reaches the samples nothing hides from the bouncing face.
#[test]
fn light_bounces_off_the_floor() {
    let spot = "{\n\"classname\" \"light\"\n\"origin\" \"128 128 120\"\n\"mangle\" \"0 -90 0\"\n\
                \"angle\" \"60\"\n\"light\" \"400\"\n}\n";
    let direct = lit(&room("", "wall", "", spot));
    let bounced = lit(&room("\"_bounce\" \"1\"\n", "wall", "", spot));
    let ceiling = |lighted: &Lighted| -> Vec<u8> {
        lightmaps(&lighted.bsp, DOWN)
            .into_iter()
            .flatten()
            .collect()
    };
    assert!(
        ceiling(&direct).is_empty(),
        "the spotlight shines only down"
    );
    let lit_ceiling = ceiling(&bounced);
    assert!(lit_ceiling.iter().any(|&luxel| luxel > 0));
    assert!(
        floor(&direct.bsp)
            .iter()
            .zip(floor(&bounced.bsp))
            .all(|(direct, bounced)| bounced >= *direct),
        "a bounce only adds light"
    );
    let twice = lit(&room("\"_bounce\" \"2\"\n", "wall", "", spot));
    let brighter: u32 = ceiling(&twice).iter().map(|&l| u32::from(l)).sum();
    let once: u32 = lit_ceiling.iter().map(|&l| u32::from(l)).sum();
    assert!(brighter >= once);
}

// spec LIGHT-20.6: a texture's colour is grey turned toward its own mean colour by
// _bouncecolorscale; grey with no palette.
#[test]
fn a_textures_colour_tints_what_it_bounces() {
    let bsp = room("\"_bouncecolorscale\" \"1\"\n", "wall", "", "");
    let read = Bsp::read(&bsp, BspLoader::FitzQuake).unwrap();
    let floor_face = face_facing(&read, UP);
    let unpaletted = parts(&read, Extras::default());
    assert_eq!(
        unpaletted.map.bounce_colours()[floor_face],
        [127.0 / 255.0; 3]
    );
    // The room's textures are all of index 1: a palette whose colour 1 is orange.
    let mut palette = vec![0_u8; 768];
    palette[3..6].copy_from_slice(&[255, 128, 0]);
    let paletted = parts(
        &read,
        Extras {
            palette: Some(&palette),
            ..Extras::default()
        },
    );
    let colour = paletted.map.bounce_colours()[floor_face];
    assert_eq!(colour, [1.0, 128.0 / 255.0, 0.0]);
    // spec LIGHT-11.6: a texel's colour is the palette's, and white with no palette.
    let on_floor = [128.0, 128.0, 0.0];
    assert_eq!(
        paletted.map.sample(floor_face, on_floor),
        [255, 128, 0, 255]
    );
    assert_eq!(
        unpaletted.map.sample(floor_face, on_floor),
        [255, 255, 255, 255]
    );
    let grey = room("", "wall", "", "");
    let read = Bsp::read(&grey, BspLoader::FitzQuake).unwrap();
    let made = parts(&read, Extras::default());
    assert_eq!(made.map.bounce_colours()[floor_face], [127.0 / 255.0; 3]);
}

// spec LIGHT-8.8: the quick light does everything but dirt and bounce.
#[test]
fn the_quick_light_skips_dirt_and_bounce() {
    let spot = "{\n\"classname\" \"light\"\n\"origin\" \"128 128 120\"\n\"mangle\" \"0 -90 0\"\n\
                \"angle\" \"60\"\n\"light\" \"400\"\n}\n";
    let keys = "\"_bounce\" \"1\"\n\"_dirt\" \"1\"\n\"light\" \"40\"\n";
    let bsp = room(keys, "wall", "", spot);
    let quick = Light::default()
        .with_finish(crate::Finish::Quick)
        .run(&bsp)
        .unwrap();
    let plain = lit(&room("\"light\" \"40\"\n", "wall", "", spot));
    assert_eq!(floor(&quick.bsp), floor(&plain.bsp));
    assert_eq!(
        lightmaps(&quick.bsp, DOWN),
        lightmaps(&plain.bsp, DOWN),
        "the ceiling has only the minimum light, with no dirt in its corners"
    );
    let full = lit(&bsp);
    assert_ne!(lightmaps(&full.bsp, DOWN), lightmaps(&plain.bsp, DOWN));
}

// spec LIGHT-10.10: with visibility in the BSP a light lights only the faces whose leaves
// see its leaf; a light in solid is never culled.
#[test]
fn a_light_is_culled_by_what_a_faces_leaves_see() {
    use super::vis::{is_unseen, leaf_at};
    let bsp = room("", "wall", "", &light_at("128 128 64", ""));
    let read = Bsp::read(&bsp, BspLoader::FitzQuake).unwrap();
    let inside = leaf_at(&read, [128.0, 128.0, 64.0]);
    let in_wall = leaf_at(&read, [-8.0, 128.0, 64.0]);
    assert!(inside.is_some_and(|leaf| leaf > 0));
    assert_eq!(in_wall, Some(0), "the solid leaf");
    let nothing = vec![0_u8; 8];
    let everything = vec![0xff_u8; 8];
    assert!(is_unseen(&read, &nothing, inside));
    assert!(!is_unseen(&read, &everything, inside));
    assert!(!is_unseen(&read, &nothing, in_wall));
    assert!(
        !is_unseen(&read, &[], inside),
        "a BSP with no visibility culls nothing"
    );
    assert!(!is_unseen(&read, &nothing, None));
    // The room has not been through vis: no face has leaves or anything seen.
    let made = parts(&read, Extras::default());
    assert!(made.map.visibility.is_none());
    assert!(made.lights.lamps.iter().all(|lamp| lamp.leaf.is_none()));
}

// spec LIGHT-12.8: a face whose lightmap would be 4096 luxels or more on an axis, or whose
// texture coordinates are not numbers, has no lightmap; one just under has.
#[test]
fn a_lightmap_past_any_a_map_has_is_not_made() {
    let plane = Plane {
        normal: UP,
        dist: 0.0,
    };
    let vecs = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]];
    let square = |side: f32| {
        vec![
            [0.0, 0.0, 0.0],
            [side, 0.0, 0.0],
            [side, side, 0.0],
            [0.0, side, 0.0],
        ]
    };
    let just_under = Extents::new(&square(4095.0 * 16.0), plane, vecs, 16.0).unwrap();
    assert_eq!((just_under.width(), just_under.height()), (4096, 4096));
    assert!(Extents::new(&square(4096.0 * 16.0), plane, vecs, 16.0).is_none());
    assert!(Extents::new(&square(f32::INFINITY), plane, vecs, 16.0).is_none());
    assert!(Extents::new(&square(f32::NAN), plane, vecs, 16.0).is_none());
    let huge = [[1e30, 0.0, 0.0, 0.0], [0.0, 1e30, 0.0, 0.0]];
    assert!(Extents::new(&square(64.0), plane, huge, 16.0).is_none());
}

// spec LIGHT-17.6: a mottled minimum light raises each sample to the minimum light plus the
// mottle at its point, from the model's key or else the world's.
#[test]
fn a_minimum_light_is_mottled() {
    let plain = lit(&room("\"light\" \"100\"\n", "wall", "", ""));
    let mottled = lit(&room(
        "\"light\" \"100\"\n\"_minlight_mottle\" \"1\"\n",
        "wall",
        "",
        "",
    ));
    let (plain, mottled) = (floor(&plain.bsp), floor(&mottled.bsp));
    assert!(plain.iter().all(|&luxel| luxel == 50));
    // 100 to under 148, halved as a lightmap holds it.
    assert!(mottled.iter().all(|&luxel| (50..74).contains(&luxel)));
    let (least, most) = (mottled.iter().min().unwrap(), mottled.iter().max().unwrap());
    assert!(most > least, "the mottle is uneven: {least} {most}");
    let other = lit(&room(
        "\"light\" \"100\"\n\"_minlightMottle\" \"1\"\n",
        "wall",
        "",
        "",
    ));
    assert_eq!(floor(&other.bsp), mottled);
    // Added minimum light takes no mottle (LIGHT-17.2).
    let added = lit(&room(
        "\"light\" \"100\"\n\"_minlight_mottle\" \"1\"\n\"_addmin\" \"1\"\n",
        "wall",
        "",
        "",
    ));
    assert!(floor(&added.bsp).iter().all(|&luxel| luxel == 50));
}

// spec LIGHT-19.4: with _dirtmode 1 the rays are drawn at random within the dirt angle, the
// same each time, and a corner is still closed in more than the open floor.
#[test]
fn dirt_mode_one_draws_its_rays_at_random() {
    let keys = "\"light\" \"100\"\n\"_dirt\" \"1\"\n\"_dirtmode\" \"1\"\n";
    let bsp = room(keys, "wall", "", "");
    let (first, again) = (lit(&bsp), lit(&bsp));
    assert_eq!(first.bsp, again.bsp);
    let dome = lit(&room(
        "\"light\" \"100\"\n\"_dirt\" \"1\"\n",
        "wall",
        "",
        "",
    ));
    assert_ne!(floor(&first.bsp), floor(&dome.bsp));
    let at = |x: f32, y: f32| floor_at(&first.bsp, x, y).unwrap();
    assert!(at(16.0, 16.0) < at(112.0, 112.0));
    assert!(floor(&first.bsp).iter().all(|&luxel| luxel <= 50));
}

// spec LIGHT-14.8: a sun light with _suntexture comes only through sky of that texture, named
// in any case; a name no texture has leaves it to every sky.
#[test]
fn a_sun_with_a_texture_comes_through_that_sky_alone() {
    // The ceiling is sky1; a block of sky4 hangs in one corner, so the map has both.
    let other_sky = cuboid([192, 192, 96], [256, 256, 128], "sky4");
    let sun = |texture: &str| {
        format!(
            "{{\n\"classname\" \"light\"\n\"_sun\" \"1\"\n\"mangle\" \"0 -90 0\"\n\
             \"light\" \"200\"\n{texture}}}\n"
        )
    };
    let at = |texture: &str, x: f32, y: f32| {
        floor_at(&lit(&room("", "sky1", &other_sky, &sun(texture))).bsp, x, y)
    };
    // Under the ceiling's sky, and under the block's.
    let (open, under_block) = ((64.0, 64.0), (224.0, 224.0));
    assert_eq!(at("", open.0, open.1), Some(100));
    assert_eq!(at("", under_block.0, under_block.1), Some(100));
    assert_eq!(at("\"_suntexture\" \"SKY1\"\n", open.0, open.1), Some(100));
    assert_eq!(
        at("\"_suntexture\" \"sky1\"\n", under_block.0, under_block.1),
        Some(0)
    );
    assert_eq!(at("\"_suntexture\" \"sky4\"\n", open.0, open.1), Some(0));
    assert_eq!(
        at("\"_suntexture\" \"sky4\"\n", under_block.0, under_block.1),
        Some(100)
    );
    assert_eq!(
        at("\"_suntexture\" \"nosuch\"\n", open.0, open.1),
        Some(100)
    );
}

// spec LIGHT-14.9: with four samples a luxel, a luxel well inside a sun's light or its shadow
// is answered once, and one on the shadow's edge at each sample, so the edge is a step
// between the two and not one or the other
#[test]
fn a_suns_shadow_has_a_soft_edge() {
    // A slab hangs under the sky ceiling over the middle of the floor; the sun is overhead.
    let slab = cuboid([96, 96, 96], [160, 160, 128], "wall");
    let sun = "{\n\"classname\" \"light\"\n\"_sun\" \"1\"\n\"mangle\" \"0 -90 0\"\n\
               \"light\" \"200\"\n}\n";
    let bsp = room("", "sky1", &slab, sun);
    let full = Light::new(Sampling::Extra).run(&bsp).unwrap().bsp;
    let at = |x: f32, y: f32| floor_at(&full, x, y).unwrap();
    let (open, under) = (at(32.0, 32.0), at(128.0, 128.0));
    assert_eq!((open, under), (100, 0));
    // Luxels well inside either are all of one value.
    for (x, y) in [(48.0, 48.0), (64.0, 128.0), (208.0, 208.0)] {
        assert_eq!(at(x, y), open, "{x} {y}");
    }
    for (x, y) in [(112.0, 128.0), (128.0, 144.0)] {
        assert_eq!(at(x, y), under, "{x} {y}");
    }
    // The luxel at the slab's side has samples on both sides of the shadow's edge.
    let edge = at(96.0, 128.0);
    assert!(edge > under && edge < open, "{edge}");
    // One sample a luxel has no such step: the luxel is lit or it is not.
    let fast = Light::new(Sampling::Fast).run(&bsp).unwrap().bsp;
    let hard = floor_at(&fast, 96.0, 128.0).unwrap();
    assert!(hard == open || hard == under, "{hard}");
}

// spec LIGHT-20.7: with four samples a luxel, the ray from a bouncing face is asked once for
// a luxel, to where its samples that are not hidden are taken together
#[test]
fn a_bounces_ray_is_asked_once_for_a_luxel() {
    let spot = "{\n\"classname\" \"light\"\n\"origin\" \"128 128 120\"\n\"mangle\" \"0 -90 0\"\n\
                \"angle\" \"60\"\n\"light\" \"400\"\n}\n";
    let bsp = room("\"_bounce\" \"1\"\n", "wall", "", spot);
    let read = Bsp::read(&bsp, BspLoader::FitzQuake).unwrap();
    let made = parts(&read, Extras::default());
    let smoothing = Smoothing::new(&made.map);
    let ceiling = FaceId::new(face_facing(&read, DOWN));
    let one = Patch::new(&made.map, &smoothing, ceiling, Sampling::Fast).unwrap();
    assert!(one.middles.is_empty(), "one sample a luxel is its own");
    let four = Patch::new(&made.map, &smoothing, ceiling, Sampling::Extra).unwrap();
    assert_eq!(four.per_luxel, 2);
    assert_eq!(four.middles.len() * 4, four.samples.len());
    for (luxel, middle) in four.middles.iter().enumerate() {
        let own: Vec<&Sample> = four
            .samples_of(luxel)
            .map(|index| &four.samples[index])
            .filter(|sample| !sample.occluded)
            .collect();
        assert_eq!(middle.occluded, own.is_empty(), "luxel {luxel}");
        if own.is_empty() {
            continue;
        }
        for axis in 0..3 {
            let seen = f32::from(u8::try_from(own.len()).unwrap());
            let mean = own.iter().map(|s| s.point[axis]).sum::<f32>() / seen;
            assert!((middle.point[axis] - mean).abs() < 1e-3, "luxel {luxel}");
        }
        assert_eq!(middle.normal, DOWN);
    }
    // The ceiling is lit by the bounce alone, the spotlight shining down: gathered where a
    // luxel's four samples meet, it is what one sample at the luxel's own place gathers.
    let ceiling_of = |sampling: Sampling| -> Vec<u8> {
        let lighted = Light::new(sampling).run(&bsp).unwrap();
        lightmaps(&lighted.bsp, DOWN)
            .into_iter()
            .flatten()
            .collect()
    };
    let (fast, full) = (ceiling_of(Sampling::Fast), ceiling_of(Sampling::Extra));
    assert_eq!(fast.len(), full.len());
    assert!(full.iter().any(|&luxel| luxel > 0));
    let apart = fast
        .iter()
        .zip(&full)
        .filter(|(a, b)| a.abs_diff(**b) > 1)
        .count();
    // But for the luxels along the walls, whose samples are not all on the ceiling.
    assert!(apart * 4 <= fast.len(), "{apart} of {}", fast.len());
}

// spec LIGHT-20.7: a luxel, or a block of them, is on the edge of a bounce's shadow when one
// of the eight around it was answered otherwise; one that asked nothing is on no edge and
// makes none.
#[test]
fn an_answer_is_on_an_edge_beside_another_answer() {
    let (lit, dark, unasked) = (Some(Some(Passed::default())), Some(None), None);
    // Three rows of three: lit but for the last of the top row, and one that asked nothing.
    let all = [lit, lit, dark, lit, lit, lit, lit, unasked, lit];
    let grid = Answers {
        all: &all,
        across: 3,
    };
    let on_edge: Vec<bool> = (0..all.len()).map(|at| grid.is_on_edge(at)).collect();
    assert_eq!(
        on_edge,
        [false, true, true, false, true, true, false, false, false]
    );
    // Past the end there is nothing to be on the edge of.
    assert!(!grid.is_on_edge(all.len()));
}

// spec LIGHT-11.10: a brush model with _shadow and no faces, all its brushes of a texture
// that is not drawn, casts the shadow of its brushes; without _shadow it casts none.
#[test]
fn a_model_with_no_faces_casts_its_brushes_shadow() {
    let light = light_at("128 128 120", "\"light\" \"300\"\n");
    let slab = |keys: &str| brush_entity("func_wall", keys, [112, 112, 64], [144, 144, 72], "skip");
    let shadowed = room(
        "",
        "wall",
        "",
        &format!("{light}{}", slab("\"_shadow\" \"1\"\n")),
    );
    let read = Bsp::read(&shadowed, BspLoader::FitzQuake).unwrap();
    assert_eq!(read.models[1].numfaces, 0, "a skip brush has no faces");
    let open = room("", "wall", "", &format!("{light}{}", slab("")));
    let (under_slab, beside) = ((128.0, 128.0), (16.0, 16.0));
    let lit_open = lit(&open).bsp;
    let lit_shadowed = lit(&shadowed).bsp;
    assert!(floor_at(&lit_open, under_slab.0, under_slab.1).unwrap() > 0);
    // Dark, or with no lightmap at all where no light reaches its face.
    assert_eq!(
        floor_at(&lit_shadowed, under_slab.0, under_slab.1).unwrap_or(0),
        0
    );
    assert_eq!(
        floor_at(&lit_shadowed, beside.0, beside.1),
        floor_at(&lit_open, beside.0, beside.1)
    );
}

// spec LIGHT-10.11: a light with _project_texture lights only what falls in its picture, in
// the texture's colour by its own; a texture the map has not is a warning and the light is a
// plain one; a field of view outside 1 to 179 stops the run.
#[test]
fn a_light_projects_a_texture() {
    let projector = |keys: &str| {
        light_at(
            "128 128 120",
            &format!("\"light\" \"300\"\n\"mangle\" \"0 -90 0\"\n\"angle\" \"120\"\n{keys}"),
        )
    };
    // The room's textures are all of index 1: a palette whose colour 1 is orange.
    let mut palette = vec![0_u8; 768];
    palette[3..6].copy_from_slice(&[255, 128, 0]);
    let run = |keys: &str| {
        let bsp = room("", "wall", "", &projector(keys));
        Light::default().run_with(
            &bsp,
            Extras {
                palette: Some(&palette),
                ..Extras::default()
            },
        )
    };
    let plain = run("").unwrap();
    let projected = run("\"_project_texture\" \"WALL\"\n\"_project_fov\" \"60\"\n").unwrap();
    // Straight under the light both light the floor; out by the corner, inside the
    // spotlight's cone and outside the picture's 60 degrees, only the plain light does.
    let (under, corner) = ((128.0, 128.0), (16.0, 16.0));
    assert!(floor_at(&plain.bsp, under.0, under.1).unwrap() > 0);
    assert!(floor_at(&plain.bsp, corner.0, corner.1).unwrap() > 0);
    assert!(floor_at(&projected.bsp, under.0, under.1).unwrap() > 0);
    assert_eq!(floor_at(&projected.bsp, corner.0, corner.1).unwrap_or(0), 0);
    assert!(plain.lit.is_none(), "a white light");
    // Orange: every lit luxel has red twice its green and no blue.
    let colours = projected.lit.unwrap();
    let lit: Vec<&[u8]> = colours[8..]
        .chunks(3)
        .filter(|rgb| rgb.iter().any(|&c| c > 0))
        .collect();
    assert!(!lit.is_empty());
    for rgb in lit {
        assert_eq!(rgb[2], 0, "{rgb:?}");
        assert!(
            rgb[0] > rgb[1] && rgb[0].abs_diff(rgb[1].saturating_mul(2)) <= 2,
            "{rgb:?}"
        );
    }
    assert!(projected.warnings.is_empty());
    let missing = run("\"_project_texture\" \"nosuch\"\n").unwrap();
    assert_eq!(
        missing.warnings,
        [LightWarning::ProjectTextureMissing {
            texture: "nosuch".to_owned()
        }]
    );
    assert_eq!(
        missing.warnings[0].to_string(),
        "WARNING: light has \"_project_texture\" \"nosuch\", but this texture was not found"
    );
    assert_eq!(floor(&missing.bsp), floor(&plain.bsp));
    let error = run("\"_project_texture\" \"wall\"\n\"_project_fov\" \"180\"\n").unwrap_err();
    assert!(matches!(error, LightError::ProjectFov { .. }), "{error}");
    assert!(error.to_string().starts_with("Unsupported fov"));
}

// spec LIGHT-16.4: a _surface light with _surface_radiosity 1, or with the world's
// _surflight_radiosity 1, puts no lights over its faces: each face gives light from a unit
// above its middle, its brightness its colour's brightest channel times the light times its
// area, its colour the light's or its texture's mean.
// spec LIGHT-16.5: and the face itself is raised to the light's brightness.
#[test]
fn a_radiosity_surface_light_lights_from_its_faces() {
    let panel = cuboid([96, 96, 0], [160, 160, 8], "glow");
    let template = |keys: &str| {
        light_at(
            "0 0 0",
            &format!("\"_surface\" \"GLOW\"\n\"light\" \"150\"\n{keys}"),
        )
    };
    let bsp = room(
        "",
        "wall",
        &panel,
        &template("\"_surface_radiosity\" \"1\"\n"),
    );
    let read = Bsp::read(&bsp, BspLoader::FitzQuake).unwrap();
    let mut made = parts(&read, Extras::default());
    assert_eq!(
        made.lights.lamps.len(),
        1,
        "no lights are put over the faces"
    );
    let (emitters, surface_lit) = made.lights.radiosity(&made.map, &made.rays).unwrap();
    let top: Vec<usize> = (0..read.faces.len())
        .filter(|&face| {
            emitters[face]
                .as_ref()
                .is_some_and(|emitter| emitter.normal == UP)
        })
        .collect();
    assert!(!top.is_empty());
    let mut area = 0.0;
    for &face in &top {
        let emitter = emitters[face].as_ref().unwrap();
        assert_eq!(emitter.pos[2], 9.0, "a unit above the panel's top");
        assert_eq!(emitter.emissions.len(), 1);
        let emission = emitter.emissions[0];
        // No palette: the texture's mean is grey, 127 of 255, times 150, and the colour is
        // made to have its brightest channel 1.
        assert_eq!(emission.stage, Stage::Direct);
        assert_eq!(emission.colour, [1.0; 3]);
        assert!(!emission.omnidirectional && emission.rescale);
        assert_eq!((emission.style, emission.atten), (0, 1.0));
        area += emission.intensity / (127.0 / 255.0 * 150.0);
        let lit = surface_lit[face].unwrap();
        assert_eq!((lit.light, lit.style, lit.minlight_scale), (150.0, 0, 1.0));
        assert_eq!(lit.colour, [127.0; 3]);
    }
    assert!((area - 64.0 * 64.0).abs() < 0.5, "{area}");
    let lighted = lit(&bsp);
    assert!(
        lightmaps(&lighted.bsp, DOWN)
            .iter()
            .flatten()
            .any(|&luxel| luxel > 0),
        "the ceiling is lit from the panel"
    );
    // The panel's top is lit by its own emitter, a unit above its middle, as every face is.
    assert!(floor_at(&lighted.bsp, 128.0, 128.0).unwrap() > 37);
    // The world's key says the same of a light with none; a light's own 0 puts lights.
    let by_world = room(
        "\"_surflight_radiosity\" \"1\"\n",
        "wall",
        &panel,
        &template(""),
    );
    assert_eq!(floor(&lit(&by_world).bsp), floor(&lighted.bsp));
    let by_lights = room(
        "\"_surflight_radiosity\" \"1\"\n",
        "wall",
        &panel,
        &template("\"_surface_radiosity\" \"0\"\n"),
    );
    let read = Bsp::read(&by_lights, BspLoader::FitzQuake).unwrap();
    let mut made = parts(&read, Extras::default());
    assert!(made.lights.lamps.len() > 1);
    let (emitters, surface_lit) = made.lights.radiosity(&made.map, &made.rays).unwrap();
    assert!(emitters.iter().all(Option::is_none) && surface_lit.iter().all(Option::is_none));
    // A dim light whose face is raised 64 times over: 10 * 64 by grey, 127 of 255, halved;
    // with a scale of 0 the face has only what its emitter gives it.
    let dim = |scale: &str| {
        let keys =
            format!("\"_surface_radiosity\" \"1\"\n\"_surflight_minlight_scale\" \"{scale}\"\n");
        let template = light_at(
            "0 0 0",
            &format!("\"_surface\" \"GLOW\"\n\"light\" \"10\"\n{keys}"),
        );
        lit(&room("", "wall", &panel, &template)).bsp
    };
    assert_eq!(floor_at(&dim("64"), 128.0, 128.0), Some(159));
    assert!(floor_at(&dim("0"), 128.0, 128.0).unwrap_or(0) < 40);
}

// spec LIGHT-12.9: the world's _lightmap_scale, or lightmap_scale, is the size of a luxel in
// texels for every face: half of 16 makes each lightmap about four times as large; 16 and
// none are the same, and a negative scale stops the run.
#[test]
fn the_worlds_lightmap_scale_sizes_the_luxels() {
    let lighting = |keys: &str| {
        let bsp = room(&format!("\"light\" \"100\"\n{keys}"), "wall", "", "");
        let lit = lit(&bsp).bsp;
        // The lighting lump's length, from the directory: a loader that takes every
        // lightmap for 16 texels to the luxel does not read a BSP lit at another scale.
        let entry = 4 + 8 * 8 + 4;
        usize::try_from(u32::from_le_bytes(
            lit[entry..entry + 4].try_into().unwrap(),
        ))
        .unwrap()
    };
    let plain = lighting("");
    assert_eq!(lighting("\"_lightmap_scale\" \"16\"\n"), plain);
    assert_eq!(lighting("\"_lightmap_scale\" \"0\"\n"), plain);
    let fine = lighting("\"_lightmap_scale\" \"8\"\n");
    assert!(fine > plain * 3 && fine < plain * 4, "{fine} of {plain}");
    assert_eq!(lighting("\"lightmap_scale\" \"8\"\n"), fine);
    let coarse = lighting("\"_lightmap_scale\" \"32\"\n");
    assert!(coarse < plain, "{coarse} of {plain}");
    let bsp = room("\"_lightmap_scale\" \"-8\"\n", "wall", "", "");
    let error = Light::default().run(&bsp).unwrap_err();
    assert_eq!(error, LightError::LightmapScale);
    assert_eq!(error.to_string(), "lightmap scale is 0 or negative");
}

// spec LIGHT-9.10: the light warns of an empty value, lightmap_scale, a falloff on a light
// that is not linear, an unknown delay, a sun with no target, a light left in solid and a
// _surface texture no face has, and lights the map all the same.
#[test]
fn the_light_warns_of_what_it_puts_right() {
    let entities = [
        light_at("128 128 64", "\"note\" \"\"\n\"lightmap_scale\" \"16\"\n"),
        light_at("64 64 64", "\"_falloff\" \"64\"\n\"delay\" \"2\"\n"),
        light_at("64 128 64", "\"delay\" \"9\"\n"),
        light_at("192 64 64", "\"_sun\" \"1\"\n"),
        light_at("128 128 -8", ""),
        light_at("192 192 64", "\"_surface\" \"nosuch\"\n"),
    ]
    .concat();
    let lighted = lit(&room("", "wall", "", &entities));
    let has = |wanted: &dyn Fn(&LightWarning) -> bool| lighted.warnings.iter().any(wanted);
    assert!(has(&|w| matches!(
        w,
        LightWarning::EmptyKeyValue { classname, key, value }
            if classname == "light" && key == "note" && value.is_empty()
    )));
    assert!(has(&|w| *w == LightWarning::LightmapScaleKey));
    assert!(has(&|w| matches!(
        w,
        LightWarning::FalloffNotLinear { classname, at } if classname == "light" && at == "64 64 64"
    )));
    assert!(has(&|w| matches!(
        w,
        LightWarning::UnknownDelay { at, .. } if at == "64 128 64"
    )));
    assert!(has(&|w| *w == LightWarning::SunMissingTarget));
    assert!(has(&|w| matches!(
        w,
        LightWarning::LightInSolid { at } if at == "128 128 -8"
    )));
    assert!(has(&|w| matches!(
        w,
        LightWarning::SurfaceTextureMissing { texture } if texture == "nosuch"
    )));
    for warning in &lighted.warnings {
        assert!(warning.to_string().starts_with("WARNING: "), "{warning}");
    }
    // A map with none of these has no warning.
    let plain = lit(&room("", "wall", "", &light_at("128 128 64", "")));
    assert_eq!(plain.warnings, []);
}

// spec LIGHT-9.10: a face lit in more styles than it holds is warned of once, however many
// faces it is true of.
#[test]
fn the_light_warns_once_of_styles_a_face_cannot_hold() {
    let mut entities = String::new();
    for (name, x) in [("a", 48), ("b", 88), ("c", 128), ("d", 168), ("e", 208)] {
        entities.push_str(&light_at(
            &format!("{x} 128 64"),
            &format!("\"targetname\" \"{name}\"\n"),
        ));
    }
    let many = lit(&room("", "wall", "", &entities));
    let count = |lighted: &Lighted, wanted: &LightWarning| {
        lighted.warnings.iter().filter(|w| *w == wanted).count()
    };
    assert_eq!(count(&many, &LightWarning::TooManyStyles), 1);
}

// spec LIGHT-9.11: texinfo flags for more texinfos than the BSP has, or leaf contents in
// another number than its leaves, are another BSP's: neither is taken, and each is warned of.
#[test]
fn files_beside_another_bsp_are_not_taken() {
    let bsp = room("", "wall", "", &light_at("128 128 64", ""));
    let read = Bsp::read(&bsp, BspLoader::FitzQuake).unwrap();
    let alone = lit(&bsp);
    // Flags that would put every face out, and leaves that would hide every sample.
    let dark = SurfaceFlags {
        light_ignore: true,
        ..SurfaceFlags::default()
    };
    let surfaces = vec![dark; read.texinfo.len() + 1];
    let leaves = vec![LeafKind::Closed; read.leafs.len() + 1];
    let extras = Extras {
        surfaces: &surfaces,
        leaves: &leaves,
        palette: None,
    };
    let lighted = Light::default().run_with(&bsp, extras).unwrap();
    assert_eq!(lighted.bsp, alone.bsp);
    assert_eq!(
        lighted.warnings,
        [
            LightWarning::SidecarMismatch(SidecarFile::Texinfo),
            LightWarning::SidecarMismatch(SidecarFile::Content),
        ]
    );
    // The BSP's own number of each is taken.
    let own = Extras {
        surfaces: &surfaces[..read.texinfo.len()],
        leaves: &[],
        palette: None,
    };
    let ignored = Light::default().run_with(&bsp, own).unwrap();
    assert_eq!(ignored.warnings, []);
    assert_ne!(ignored.bsp, alone.bsp);
}

// spec LIGHT-18.7: the BSP's BSPX lumps are written back after its own, but the lumps of an
// earlier lighting.
#[test]
fn bspx_lumps_are_kept_but_an_earlier_lightings() {
    let mut bsp = room("\"light\" \"100\"\n", "wall", "", "");
    let mut lumps = qs_bsp::Bspx::default();
    lumps.set(b"BRUSHLIST", vec![1, 2, 3, 4, 5]);
    for stale in [
        &b"LMOFFSET"[..],
        b"LMSTYLE",
        b"LMSTYLE16",
        b"DECOUPLED_LM",
        b"RGBLIGHTING",
        b"LIGHTINGDIR",
        b"LIGHTING_E5BGR9",
        b"FACENORMALS",
    ] {
        lumps.set(stale, vec![9; 8]);
    }
    lumps.append_to(&mut bsp).unwrap();
    let kept = qs_bsp::Bspx::read(&lit(&bsp).bsp);
    assert_eq!(kept.names().collect::<Vec<_>>(), [&b"BRUSHLIST"[..]]);
    assert_eq!(kept.get(b"BRUSHLIST"), Some(&[1, 2, 3, 4, 5][..]));
    // A BSP with none has none written.
    let plain = lit(&room("\"light\" \"100\"\n", "wall", "", "")).bsp;
    assert!(qs_bsp::Bspx::read(&plain).is_empty());
}
