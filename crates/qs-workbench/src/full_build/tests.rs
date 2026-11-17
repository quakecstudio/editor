// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use qs_bsp::{Bsp, BspLoader};
use qs_map::{Brush, Item, MapFile, Surface, TexDef};
use qs_map_bsp::{Compiled, Compiler, Dialect};
use qs_map_light::Sidecars;

use super::{FullBuild, FullBuildError, Start, brush_models};

/// A sealed room 256 across with a player start, a light and a door, compiled as Play from here
/// compiles it, before its light.
pub(crate) fn compiled_room() -> Compiled {
    let mut file = MapFile::parse(
        b"{\n\"classname\" \"worldspawn\"\n}\n\
          {\n\"classname\" \"info_player_start\"\n\"origin\" \"-64 0 24\"\n}\n\
          {\n\"classname\" \"light\"\n\"origin\" \"0 0 96\"\n}\n\
          {\n\"classname\" \"func_door\"\n}\n",
    )
    .unwrap();
    let surface = Surface {
        texture: b"wall".to_vec(),
        texdef: TexDef::Standard {
            shift: [0.0; 2],
            rotate: 0.0,
            scale: [1.0; 2],
        },
    };
    let (a, b) = (128.0, 144.0);
    let walls = [
        ([-b, -b, -b], [b, b, -a]),
        ([-b, -b, a], [b, b, b]),
        ([-b, -b, -a], [-a, b, a]),
        ([a, -b, -a], [b, b, a]),
        ([-a, -b, -a], [a, -a, a]),
        ([-a, a, -a], [a, b, a]),
    ];
    for (min, max) in walls {
        let brush = Brush::cuboid(min, max, &surface).unwrap();
        file.entities[0].items.push(Item::Brush(brush));
    }
    let door = Brush::cuboid([32.0, -16.0, -128.0], [48.0, 16.0, 0.0], &surface).unwrap();
    file.entities[3].items.push(Item::Brush(door));
    Compiler::default()
        .with_dialect(Dialect::Extended)
        .compile(&file.to_bytes(), None)
        .unwrap()
}

/// A folder of a test's own, taken away when it is dropped.
pub(crate) struct Scratch(pub(crate) std::path::PathBuf);

impl Scratch {
    pub(crate) fn new(name: &str) -> Self {
        static MADE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = MADE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("qs-wb-{name}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.0));
    }
}

/// The start of a full compile of [`compiled_room`] for the map `map` played from `target`.
pub(crate) fn start(map: &str, target: std::path::PathBuf) -> Start {
    let compiled = compiled_room();
    Start {
        map: map.to_owned(),
        target,
        portals: compiled.portal_file.unwrap(),
        bsp: compiled.bsp,
        sidecars: Sidecars::new(compiled.surfaces, compiled.leaves),
        edits: 0,
    }
}

// No spec statement: P7-02: the full compile gives the fast compile's BSP full vis and full
// light, written beside the played BSP under a name of its own, which it leaves as it was
#[test]
fn a_full_build_sees_and_lights_the_map() {
    let dir = Scratch::new("full");
    let target = dir.0.join("room.bsp");
    std::fs::write(&target, b"the fast compile").unwrap();
    let build = FullBuild::start(start("room", target.clone()));
    assert!(build.current());
    let ready = build.wait().unwrap();
    assert_eq!(ready.bsp, dir.0.join("room.full.bsp"));
    let bytes = std::fs::read(&ready.bsp).unwrap();
    let bsp = Bsp::read(&bytes, BspLoader::default()).unwrap();
    assert!(!bsp.visibility.is_empty(), "full vis");
    assert!(bsp.lighting.iter().any(|&b| b > 0), "full light");
    assert_eq!(ready.lit, None, "white light writes no .lit");
    assert_eq!(ready.models, [(3, "*1".to_owned())]);
    assert_eq!(std::fs::read(&target).unwrap(), b"the fast compile");
    std::fs::write(&target, b"another compile").unwrap();
    assert!(!build.current(), "another compile written over it");
}

// spec LIGHT-9.8: the flags the compile gives each texinfo reach the full compile's light: a
// minimum light on every face leaves few luxels dark
#[test]
fn a_full_build_lights_with_the_compiles_flags() {
    let dark = |name: &str, minlight: Option<f32>| {
        let dir = Scratch::new(name);
        let mut start = start("room", dir.0.join("room.bsp"));
        for flags in &mut start.sidecars.surfaces {
            flags.minlight = minlight;
        }
        let ready = FullBuild::start(start).wait().unwrap();
        let bytes = std::fs::read(&ready.bsp).unwrap();
        let bsp = Bsp::read(&bytes, BspLoader::default()).unwrap();
        let dark = bsp.lighting.iter().filter(|&&b| b < 100).count();
        (dark, bsp.lighting.len())
    };
    let plain = dark("full-plain", None);
    let raised = dark("full-minlight", Some(200.0));
    // Luxels under 100, of all: nearly every one of the plain room, few of the raised.
    assert!(plain.0 * 2 > plain.1, "{plain:?}");
    assert!(raised.0 * 10 < raised.1, "{raised:?}");
}

// No spec statement: P7-02: a full compile of a map with no portals to see through stops with
// vis's reason
#[test]
fn a_full_build_of_broken_portals_says_why() {
    let dir = Scratch::new("full-broken");
    let mut broken = start("room", dir.0.join("room.bsp"));
    broken.portals = b"not a portal file".to_vec();
    let error = FullBuild::start(broken).wait().unwrap_err();
    assert!(matches!(error, FullBuildError::Vis(_)), "{error}");
}

// spec BRIDGE-4.18: each brush entity of the new build, by its place in the entity lump, and
// its brush model, for map/swap
#[test]
fn brush_models_are_read_from_the_entity_lump() {
    let compiled = compiled_room();
    assert_eq!(brush_models(&compiled.bsp).unwrap(), [(3, "*1".to_owned())]);
    assert!(matches!(
        brush_models(b"not a bsp"),
        Err(FullBuildError::Bsp(_))
    ));
}
