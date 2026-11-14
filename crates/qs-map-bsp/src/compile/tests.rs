// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::modulo_arithmetic,
    clippy::integer_division,
    reason = "test code: a failed unwrap or index is a failed test, and the numbers are small"
)]

use qs_bsp::{Bsp, BspLoader, BspVersion};
use qs_map::qbsp::QbspWarning;
use qs_wad::write::WadBuilder;

use super::{Compiled, Compiler};
use crate::fixtures::{
    BrushSpec, EntitySpec, Form, MapSpec, PLAYER, ROOM_MAXS, ROOM_MINS, Shape, map_text,
};
use crate::{CompileError, Dialect, Warning};

fn compile(spec: &MapSpec, dialect: Dialect) -> Result<Compiled, CompileError> {
    Compiler::default()
        .with_dialect(dialect)
        .compile(map_text(spec).as_bytes(), None)
}

fn read(compiled: &Compiled) -> Bsp<'_> {
    Bsp::read(&compiled.bsp, BspLoader::FitzQuake).unwrap()
}

/// A small box inside the default room, the `n`th along a grid.
fn crate_box(n: i32) -> BrushSpec {
    let x = ROOM_MINS[0] + 4 + (n % 30) * 8;
    let y = ROOM_MINS[1] + 4 + (n / 30) * 8;
    let z = ROOM_MINS[2];
    BrushSpec::cube([x, y, z], [x + 4, y + 4, z + 4], "crate")
}

// spec QBSP-14.9: a map the id dialect compiles, with no world brush entity, gives the same
// BSP, portal file and leak file in both dialects
// spec QBSP-1.7: the world's hulls are built, filled and written
// spec QBSP-9.4: a sealed room is filled from outside without a leak
// spec QBSP-12.2: the portal file starts with `PRT1`, the leaf count and the portal count
#[test]
fn a_sealed_room_compiles_the_same_in_both_dialects() {
    let spec = MapSpec::default();
    let id = compile(&spec, Dialect::Id).unwrap();
    let extended = compile(&spec, Dialect::Extended).unwrap();
    assert_eq!(id, extended);
    assert!(!id.warnings.contains(&Warning::Leak), "{:?}", id.warnings);
    let prt = String::from_utf8(id.portal_file.clone().unwrap()).unwrap();
    let mut lines = prt.lines();
    assert_eq!(lines.next(), Some("PRT1"));
    let leaves: usize = lines.next().unwrap().parse().unwrap();
    let portals: usize = lines.next().unwrap().parse().unwrap();
    assert_eq!(lines.count(), portals);
    assert!(leaves >= 1);
    let bsp = read(&id);
    assert_eq!(bsp.version, BspVersion::Bsp29);
    assert_eq!(bsp.models.len(), 1);
}

// spec QBSP-9.5: once an entity stands in empty space the leak file is written, and left empty
// spec QBSP-9.3: a room open to the outside leaks, so nothing is filled and no portal file is
// written
#[test]
fn a_room_with_a_hole_leaks() {
    let mut spec = MapSpec::default();
    spec.world.pop();
    let compiled = compile(&spec, Dialect::Id).unwrap();
    assert!(compiled.warnings.contains(&Warning::Leak));
    assert_eq!(compiled.portal_file, None);
    assert_eq!(compiled.leak_file, Some(Vec::new()));
}

// spec QBSP-9.8: a leak's trail runs from the entity reached out through the hole, its
// first point the entity's origin; a sealed map has none
#[test]
fn a_leak_s_trail_runs_from_the_entity_out() {
    let mut spec = MapSpec::default();
    spec.world.pop();
    let compiled = compile(&spec, Dialect::Id).unwrap();
    let trail = &compiled.leak_trail;
    assert!(
        (2..=3).contains(&trail.len()),
        "the shortest way out: {trail:?}"
    );
    assert_eq!(
        trail.first(),
        Some(&PLAYER.map(|c| f32::from(i16::try_from(c).unwrap())))
    );
    let last = trail.last().unwrap();
    assert!(
        last[1] > f32::from(i16::try_from(ROOM_MAXS[1]).unwrap()),
        "out through the +y hole: {trail:?}"
    );
    let sealed = compile(&MapSpec::default(), Dialect::Id).unwrap();
    assert!(sealed.leak_trail.is_empty());
}

// spec QBSP-9.8: an entity at exactly the origin, never placed (QBSP-9.1), is not the one the
// trail starts from
#[test]
fn a_leak_s_trail_skips_an_entity_at_the_origin() {
    let mut spec = MapSpec {
        entities: vec![
            EntitySpec::point("info_null", [0, 0, 0]),
            EntitySpec::point("info_player_start", PLAYER),
        ],
        ..MapSpec::default()
    };
    spec.world.pop();
    let compiled = compile(&spec, Dialect::Id).unwrap();
    assert_eq!(
        compiled.leak_trail.first(),
        Some(&PLAYER.map(|c| f32::from(i16::try_from(c).unwrap())))
    );
}

// spec QBSP-9.2: with no entity in empty space nothing is filled
// spec QBSP-9.1: an entity whose origin is exactly the origin is not placed
#[test]
fn no_occupant_fills_nothing() {
    for entities in [
        Vec::new(),
        vec![EntitySpec::point("info_player_start", [0, 0, 0])],
    ] {
        let spec = MapSpec {
            entities,
            ..MapSpec::default()
        };
        let compiled = compile(&spec, Dialect::Id).unwrap();
        assert!(compiled.warnings.contains(&Warning::NoOccupant));
        assert_eq!(compiled.portal_file, None);
        assert_eq!(compiled.leak_file, None);
    }
}

// spec QBSP-1.5: each entity with brushes after the world is a model, its `model` key `*<n>`
// spec QBSP-1.8: the entity lump is written from the entities as changed
#[test]
fn brush_entities_become_models() {
    let mut spec = MapSpec::default();
    spec.entities.push(EntitySpec::brushes(
        "func_wall",
        vec![BrushSpec::cube([-16, -16, -64], [16, 16, -32], "door")],
    ));
    let compiled = compile(&spec, Dialect::Id).unwrap();
    let bsp = read(&compiled);
    assert_eq!(bsp.models.len(), 2);
    let entities = String::from_utf8_lossy(bsp.entities()).into_owned();
    assert!(entities.contains("\"model\" \"*1\""), "{entities}");
    assert!(entities.contains("info_player_start"));
}

// spec QBSP-7.6: a water brush gives leaves of water contents
// spec QBSP-6.4: a liquid's faces are also seen from inside it
#[test]
fn water_makes_water_leaves() {
    let mut spec = MapSpec::default();
    spec.world
        .push(BrushSpec::cube([-64, -64, -64], [64, 64, -16], "*water"));
    let compiled = compile(&spec, Dialect::Id).unwrap();
    let bsp = read(&compiled);
    assert!(bsp.leafs.iter().any(|leaf| leaf.contents == -3));
}

// spec QBSP-14.1: the extended dialect reads Valve 220 faces, which id's parser refuses
#[test]
fn extended_reads_valve_220() {
    let spec = MapSpec {
        form: Form::Valve,
        ..MapSpec::default()
    };
    assert!(compile(&spec, Dialect::Id).is_err());
    let compiled = compile(&spec, Dialect::Extended).unwrap();
    assert!(compiled.portal_file.is_some());
}

// spec QBSP-14.1: a texture name longer than 15 bytes is cut to 15 in the extended dialect
#[test]
fn extended_cuts_long_texture_names() {
    let mut spec = MapSpec::default();
    spec.world.push(BrushSpec::cube(
        [-16, -16, -64],
        [16, 16, -32],
        "a_texture_name_of_twenty",
    ));
    assert!(matches!(
        compile(&spec, Dialect::Id),
        Err(CompileError::Map(_))
    ));
    assert!(compile(&spec, Dialect::Extended).is_ok());
}

// spec QBSP-14.2: a brush may have more faces than qbsp's MAX_FACES
// spec QBSP-5.6: in the id dialect a brush of too many faces stops the run
#[test]
fn extended_lifts_brush_limits() {
    let sides = 24;
    let corners: Vec<[i32; 2]> = (0..sides)
        .map(|i| {
            let angle = f64::from(i) * std::f64::consts::TAU / f64::from(sides);
            #[expect(
                clippy::cast_possible_truncation,
                reason = "test code: the corners are rounded to whole units"
            )]
            let corner = [
                (angle.cos() * 96.0).round() as i32,
                (angle.sin() * 96.0).round() as i32,
            ];
            corner
        })
        .collect();
    let mut spec = MapSpec::default();
    spec.world.push(BrushSpec {
        shape: Shape::Prism {
            corners,
            bottom: -64,
            top: -48,
        },
        texture: "floor".to_owned(),
    });
    assert!(compile(&spec, Dialect::Id).is_err());
    let compiled = compile(&spec, Dialect::Extended).unwrap();
    assert!(compiled.portal_file.is_some());
}

// spec QBSP-14.3: an entity none of whose brushes is loaded gets an empty model, where qbsp stops
// spec QBSP-1.6: in the id dialect it stops the run with `Entity with no valid brushes`
#[test]
fn extended_gives_an_empty_model_where_qbsp_stops() {
    let mut spec = MapSpec::default();
    spec.entities.push(EntitySpec::brushes(
        "func_wall",
        vec![BrushSpec::cube([-16, -16, -64], [16, 16, -32], "clip")],
    ));
    assert_eq!(
        compile(&spec, Dialect::Id),
        Err(CompileError::NoValidBrushes)
    );
    let compiled = compile(&spec, Dialect::Extended).unwrap();
    assert!(compiled.warnings.contains(&Warning::NoValidBrushes));
    let bsp = read(&compiled);
    assert_eq!(bsp.models.len(), 2);
    // The empty model's bounds are its origin, which the loader widens by a unit: never
    // backwards.
    let model = &bsp.models[1];
    assert_eq!((model.mins, model.maxs), ([-1.0; 3], [1.0; 3]));
}

// spec QBSP-14.4: no MAX_MAP_ limit applies in the extended dialect, and a file whose values fit
// BSP29 is written as BSP29
// spec QBSP-13.10: in the id dialect, a model past MAX_MAP_MODELS stops the run naming it
#[test]
fn extended_drops_the_model_limit() {
    let mut spec = MapSpec::default();
    for n in 0..256 {
        spec.entities
            .push(EntitySpec::brushes("func_wall", vec![crate_box(n)]));
    }
    assert_eq!(
        compile(&spec, Dialect::Id),
        Err(CompileError::Limit("nummodels == MAX_MAP_MODELS"))
    );
    let compiled = compile(&spec, Dialect::Extended).unwrap();
    let bsp = read(&compiled);
    assert_eq!(bsp.version, BspVersion::Bsp29);
    assert_eq!(bsp.models.len(), 257);
    assert!(
        !compiled
            .warnings
            .iter()
            .any(|w| matches!(w, Warning::Bsp2(_)))
    );
}

// spec QBSP-14.5: the extended dialect writes a key and value longer than qbsp's line
#[test]
fn extended_writes_long_entity_lines() {
    let message = "x".repeat(200);
    let spec = MapSpec {
        world_pairs: vec![("message".to_owned(), message.clone())],
        ..MapSpec::default()
    };
    assert!(matches!(
        compile(&spec, Dialect::Id),
        Err(CompileError::Map(_))
    ));
    let compiled = compile(&spec, Dialect::Extended).unwrap();
    let entities = String::from_utf8_lossy(read(&compiled).entities()).into_owned();
    assert!(entities.contains(&message));
}

// spec QBSP-9.1: the player stands in the empty leaf the fill keeps
#[test]
fn the_player_is_in_an_empty_leaf() {
    let compiled = compile(&MapSpec::default(), Dialect::Extended).unwrap();
    let bsp = read(&compiled);
    let origin = PLAYER.map(|c| f32::from(i16::try_from(c).unwrap()));
    let leaf = bsp.point_in_leaf(origin).unwrap();
    assert_eq!(bsp.leafs[leaf].contents, -1);
    let outside = ROOM_MAXS.map(|c| f32::from(i16::try_from(c + 64).unwrap()));
    let leaf = bsp.point_in_leaf(outside).unwrap();
    assert_eq!(bsp.leafs[leaf].contents, -2, "the outside is filled solid");
}

// spec QBSP-14.4: geometry past a 16-bit bound is written as BSP2 with a warning naming the lump
#[test]
fn far_geometry_is_written_as_bsp2() {
    let mut spec = MapSpec::default();
    spec.world
        .push(BrushSpec::cube([40_000, 0, 0], [40_064, 64, 64], "far"));
    let compiled = compile(&spec, Dialect::Extended).unwrap();
    assert_eq!(read(&compiled).version, BspVersion::Bsp2);
    assert!(
        compiled
            .warnings
            .iter()
            .any(|w| matches!(w, Warning::Bsp2(_)))
    );
}

// spec MAP-6.8: a WAD that is not a WAD2 stops the compile, as TEX_InitFromWad does
#[test]
fn a_bad_wad_stops_the_compile() {
    let map = map_text(&MapSpec::default());
    let result = Compiler::default().compile(map.as_bytes(), Some(b"not a wad"));
    assert!(matches!(result, Err(CompileError::Wad(_))), "{result:?}");
    assert!(result.unwrap_err().to_string().contains("isn't a wadfile"));
}

// spec QBSP-14.1: what the map reader prints reaches the compile's warnings
#[test]
fn the_readers_warnings_are_kept() {
    let mut spec = MapSpec::default();
    spec.world_pairs
        .push(("wad".to_owned(), "gfx/base.wad".to_owned()));
    let map = map_text(&spec);
    let wad = WadBuilder::new().to_bytes().unwrap();
    let compiled = Compiler::default()
        .compile(map.as_bytes(), Some(&wad))
        .unwrap();
    assert!(
        compiled
            .warnings
            .iter()
            .any(|w| matches!(w, Warning::Map(QbspWarning::TextureNotFound { .. }))),
        "{:?}",
        compiled.warnings
    );
}

// spec QBSP-14.1: the WAD path is read in the compiler's dialect, so a Valve 220 map names one
#[test]
fn wad_path_reads_the_dialects_map() {
    let spec = MapSpec {
        form: Form::Valve,
        world_pairs: vec![("wad".to_owned(), "gfx/base.wad".to_owned())],
        ..MapSpec::default()
    };
    let map = map_text(&spec);
    assert_eq!(
        Compiler::default().wad_path(map.as_bytes()).unwrap(),
        Some(b"gfx/base.wad".to_vec())
    );
    assert!(
        Compiler::default()
            .with_dialect(Dialect::Id)
            .wad_path(map.as_bytes())
            .is_err()
    );
}

// spec QBSP-14.7: the extended dialect compiles an external map instance as the map with the
// instance expanded (MAP-14), with the files its caller reads; the id dialect leaves it a point
// entity; a file that cannot be read stops the compile
#[test]
fn external_maps_are_expanded_in_the_extended_dialect() {
    let prefab = MapSpec {
        world: vec![BrushSpec::cube([-8, -8, 0], [8, 8, 32], "pillar")],
        entities: Vec::new(),
        ..MapSpec::default()
    };
    let prefab = map_text(&prefab);
    let mut instance = EntitySpec::point("misc_external_map", [0, 0, ROOM_MINS[2]]);
    instance.pairs = vec![
        ("_external_map".to_owned(), "pillar.map".to_owned()),
        ("_external_map_classname".to_owned(), "func_wall".to_owned()),
    ];
    let mut spec = MapSpec::default();
    spec.entities.push(instance);
    let text = map_text(&spec);
    let mut load = |name: &str| (name == "pillar.map").then(|| prefab.as_bytes().to_vec());
    let compiler = Compiler::default().with_dialect(Dialect::Extended);
    let compiled = compiler
        .compile_with_externals(text.as_bytes(), None, &mut load)
        .unwrap();
    assert_eq!(
        read(&compiled).models.len(),
        2,
        "the instance is a brush model"
    );
    let expanded = qs_map::MapFile::parse(text.as_bytes())
        .unwrap()
        .with_external_maps(&mut load)
        .unwrap()
        .to_bytes();
    assert_eq!(compiled, compiler.compile(&expanded, None).unwrap());
    let id = Compiler::default()
        .with_dialect(Dialect::Id)
        .compile_with_externals(text.as_bytes(), None, &mut load)
        .unwrap();
    assert_eq!(read(&id).models.len(), 1, "id's qbsp reads no instance");
    assert!(matches!(
        compiler.compile_with_externals(text.as_bytes(), None, &mut |_| None),
        Err(CompileError::ExternalMap(
            qs_map::ExternalMapError::Unreadable { .. }
        ))
    ));
}

// spec QBSP-14.8: in the extended dialect func_group and the detail kinds give their brushes to
// the world and leave the entity lump, in any case; func_detail_illusionary becomes a
// func_illusionary of its own and func_illusionary_visblocker goes; the id dialect keeps each
// a brush model of its own
#[test]
fn world_brush_entities_join_the_world_in_the_extended_dialect() {
    let pillar = |x: i32| BrushSpec::cube([x, -8, ROOM_MINS[2]], [x + 16, 8, 64], "pillar");
    let mut spec = MapSpec::default();
    for (n, classname) in [
        "func_group",
        "FUNC_DETAIL",
        "func_detail_wall",
        "func_detail_fence",
        "func_detail_illusionary",
        "func_illusionary_visblocker",
    ]
    .into_iter()
    .enumerate()
    {
        let x = -96 + 32 * i32::try_from(n).unwrap();
        spec.entities
            .push(EntitySpec::brushes(classname, vec![pillar(x)]));
    }
    let extended = compile(&spec, Dialect::Extended).unwrap();
    let bsp = read(&extended);
    assert_eq!(bsp.models.len(), 2, "the world and the illusionary");
    let lump = String::from_utf8_lossy(bsp.entities()).to_lowercase();
    for gone in [
        "func_group",
        "func_detail\"",
        "func_detail_wall",
        "func_detail_fence",
        "visblocker",
    ] {
        assert!(!lump.contains(gone), "{gone} left the lump: {lump}");
    }
    assert!(lump.contains("\"func_illusionary\""), "{lump}");
    // The four solid pillars are in the world's player hull, the other two not.
    let contents = |point: [f32; 3]| {
        let mut num = bsp.models[0].headnode[1];
        while num >= 0 {
            let node = &bsp.clipnodes[usize::try_from(num).unwrap()];
            let plane = &bsp.planes[usize::try_from(node.planenum).unwrap()];
            let d: f32 = plane
                .normal
                .iter()
                .zip(point)
                .map(|(n, p)| n * p)
                .sum::<f32>()
                - plane.dist;
            num = node.children[usize::from(d < 0.0)];
        }
        num
    };
    let solid: Vec<bool> = (0..6)
        .map(|n| {
            contents([f32::from(i16::try_from(-88 + 32 * n).unwrap()), 0.0, 0.0])
                == qs_bsp::BSP_CONTENTS_SOLID
        })
        .collect();
    assert_eq!(solid, [true, true, true, true, false, false]);
    let id = compile(&spec, Dialect::Id).unwrap();
    assert_eq!(
        read(&id).models.len(),
        7,
        "id's qbsp: each its own brush model"
    );
}
