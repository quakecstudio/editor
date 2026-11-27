// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test code: a failed unwrap or a panic is a failed test"
)]

use super::*;
use crate::LIMIT_SOURCES;

/// The bound's most, `None` when it is not [`Bound::At`].
fn most(engine: Engine, limit: MapLimit, format: MapFormat) -> Option<usize> {
    match engine.bound(limit, format) {
        Bound::At(fact) => Some(fact.most),
        Bound::Unlimited | Bound::Unknown => None,
    }
}

/// The past of the bound.
fn past(engine: Engine, limit: MapLimit) -> Past {
    match engine.bound(limit, MapFormat::Bsp29) {
        Bound::At(fact) => fact.past,
        Bound::Unlimited | Bound::Unknown => panic!("{engine:?} {limit:?} has no bound"),
    }
}

// spec ENGT-5.7: id's limits are BSP-16's compiler limits, tools past them, its visleafs row,
// BSP-9.4's extent and LIMIT-6's server limits
#[test]
fn vanilla_has_ids_limits() {
    let v = Engine::Vanilla;
    let at = |limit| most(v, limit, MapFormat::Bsp29).unwrap();
    assert_eq!(at(MapLimit::Clipnodes), 32767);
    assert_eq!(at(MapLimit::Marksurfaces), 65535);
    assert_eq!(at(MapLimit::EntityText), 65536);
    assert_eq!(at(MapLimit::Entities), 1024);
    assert_eq!(at(MapLimit::Leafs), 8192);
    assert_eq!(at(MapLimit::Visleafs), 8192);
    assert_eq!(at(MapLimit::FaceExtent), 256);
    assert_eq!(at(MapLimit::Edicts), 600);
    assert_eq!(at(MapLimit::ModelPrecaches), 255);
    assert_eq!(at(MapLimit::SoundPrecaches), 255);
    assert_eq!(at(MapLimit::LightStyles), 64);
    assert_eq!(past(v, MapLimit::Clipnodes), Past::Tools);
    assert_eq!(past(v, MapLimit::Visleafs), Past::Undefined);
    assert_eq!(past(v, MapLimit::FaceExtent), Past::Refused);
    assert_eq!(past(v, MapLimit::Edicts), Past::Refused);
    assert_eq!(past(v, MapLimit::LightStyles), Past::Undefined);
    // Every limit of ENGT-5.4 has one of id's.
    for limit in MapLimit::ALL {
        assert!(
            matches!(v.bound(limit, MapFormat::Bsp29), Bound::At(_)),
            "{limit:?}"
        );
    }
}

// spec ENGT-5.7: every limit of KEX is unknown, for every format
#[test]
fn kex_limits_are_unknown() {
    for limit in MapLimit::ALL {
        for format in [MapFormat::Bsp29, MapFormat::Bsp2] {
            assert_eq!(Engine::Kex.bound(limit, format), Bound::Unknown);
        }
    }
    assert!(Engine::Kex.limit_facts().is_empty());
}

// spec ENGT-5.8: the limits each engine's pinned source gives
#[test]
fn the_pinned_limits() {
    let rows: [(Engine, MapLimit, usize, Past); 21] = [
        (Engine::QuakeSpasm, MapLimit::Edicts, 8192, Past::Refused),
        (Engine::Ironwail, MapLimit::Edicts, 16384, Past::Refused),
        (Engine::VkQuake, MapLimit::Edicts, 32000, Past::Refused),
        (Engine::Fteqw, MapLimit::Edicts, 131_072, Past::Refused),
        (Engine::DarkPlaces, MapLimit::Edicts, 32768, Past::Refused),
        (
            Engine::QuakeSpasm,
            MapLimit::ModelPrecaches,
            4095,
            Past::Refused,
        ),
        (
            Engine::VkQuake,
            MapLimit::ModelPrecaches,
            8191,
            Past::Refused,
        ),
        (
            Engine::Fteqw,
            MapLimit::ModelPrecaches,
            16383,
            Past::Refused,
        ),
        (
            Engine::DarkPlaces,
            MapLimit::ModelPrecaches,
            8191,
            Past::Refused,
        ),
        (
            Engine::Ironwail,
            MapLimit::SoundPrecaches,
            2047,
            Past::Refused,
        ),
        (Engine::Fteqw, MapLimit::SoundPrecaches, 4095, Past::Refused),
        (Engine::QuakeSpasm, MapLimit::LightStyles, 64, Past::Warned),
        (Engine::Fteqw, MapLimit::LightStyles, 65536, Past::Warned),
        (
            Engine::DarkPlaces,
            MapLimit::LightStyles,
            256,
            Past::Refused,
        ),
        (
            Engine::QuakeSpasm,
            MapLimit::FaceExtent,
            2000,
            Past::Refused,
        ),
        (
            Engine::DarkPlaces,
            MapLimit::FaceExtent,
            4080,
            Past::Refused,
        ),
        (Engine::QuakeSpasm, MapLimit::Faces, 32767, Past::Warned),
        (Engine::VkQuake, MapLimit::Clipnodes, 32767, Past::Warned),
        (Engine::Fteqw, MapLimit::Clipnodes, 65536, Past::Refused),
        (Engine::Ironwail, MapLimit::Leafs, 32767, Past::Refused),
        (Engine::QuakeSpasm, MapLimit::Visleafs, 8192, Past::Warned),
    ];
    for (engine, limit, want, want_past) in rows {
        assert_eq!(
            most(engine, limit, MapFormat::Bsp29),
            Some(want),
            "{engine:?} {limit:?}"
        );
        assert_eq!(past(engine, limit), want_past, "{engine:?} {limit:?}");
    }
    // ENGT-5.7: a limit an engine's source checks nothing for is none of its own.
    assert_eq!(
        Engine::Fteqw.bound(MapLimit::FaceExtent, MapFormat::Bsp29),
        Bound::Unlimited
    );
    assert_eq!(
        Engine::DarkPlaces.bound(MapLimit::Clipnodes, MapFormat::Bsp29),
        Bound::Unlimited
    );
    assert_eq!(
        Engine::QuakeSpasm.bound(MapLimit::EntityText, MapFormat::Bsp29),
        Bound::Unlimited
    );
}

// spec ENGT-5.5: a limit of a BSP29 loader does not bound a BSP2 map; one of every format does
#[test]
fn a_bsp29_check_does_not_bound_bsp2() {
    assert_eq!(
        most(Engine::QuakeSpasm, MapLimit::Clipnodes, MapFormat::Bsp29),
        Some(32767)
    );
    assert_eq!(
        Engine::QuakeSpasm.bound(MapLimit::Clipnodes, MapFormat::Bsp2),
        Bound::Unlimited
    );
    assert_eq!(
        most(Engine::QuakeSpasm, MapLimit::Visleafs, MapFormat::Bsp2),
        Some(8192)
    );
    assert_eq!(
        most(Engine::Vanilla, MapLimit::Clipnodes, MapFormat::Bsp2),
        Some(32767)
    );
}

// spec ENGT-5.6: the five engines with sources read BSP2, id's does not, and KEX is unknown
#[test]
fn which_engines_read_bsp2() {
    for engine in [
        Engine::QuakeSpasm,
        Engine::Ironwail,
        Engine::VkQuake,
        Engine::Fteqw,
        Engine::DarkPlaces,
    ] {
        assert_eq!(engine.bsp2(), Bsp2::Reads, "{engine:?}");
    }
    assert_eq!(Engine::Vanilla.bsp2(), Bsp2::DoesNotRead);
    assert_eq!(Engine::Kex.bsp2(), Bsp2::Unknown);
}

// spec ENGT-5.1: every generated limit names a file of LIMIT_SOURCES, the source's commit
#[test]
fn every_generated_limit_names_a_pinned_file() {
    for engine in Engine::ALL {
        for fact in engine.limit_facts() {
            if engine == Engine::Vanilla {
                assert_eq!(fact.source, "id");
                continue;
            }
            assert!(
                LIMIT_SOURCES
                    .iter()
                    .any(|s| s.id == fact.source && s.path == fact.path),
                "{engine:?} {fact:?}"
            );
            assert!(crate::SOURCES.iter().any(|s| {
                s.id == fact.source
                    && LIMIT_SOURCES
                        .iter()
                        .any(|l| l.id == s.id && l.commit == s.commit)
            }));
        }
    }
}

// spec ENGT-5.4: the lumps and the face extent are measured in a compile, the server's limits
// in a spawn
#[test]
fn where_each_limit_is_measured() {
    let spawned: Vec<MapLimit> = MapLimit::ALL
        .into_iter()
        .filter(|l| l.measured_by() == Measure::Spawn)
        .collect();
    assert_eq!(
        spawned,
        [
            MapLimit::Edicts,
            MapLimit::ModelPrecaches,
            MapLimit::SoundPrecaches,
            MapLimit::LightStyles
        ]
    );
    assert_eq!(MapLimit::Clipnodes.name(), "clipnodes");
}

// spec ENGT-5.7: the bound is the least of the limits that apply
#[test]
fn the_least_limit_bounds() {
    let facts = Engine::QuakeSpasm.limit_facts();
    for limit in MapLimit::ALL {
        let least = facts
            .iter()
            .filter(|f| f.limit == limit)
            .map(|f| f.most)
            .min();
        assert_eq!(most(Engine::QuakeSpasm, limit, MapFormat::Bsp29), least);
    }
    assert!(facts.iter().all(|f| f.most > 0));
    assert_eq!(facts.first().unwrap().source, "quakespasm");
}
