// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::path::{Path, PathBuf};

use qc_project::FsLoader;
use qs_commands::{Registry, Session};
use qs_map::{Brush, Face};
use serde_json::{Value, json};

use super::{FaceIndex, MAP_FACE_ALIGN, MAP_FACE_UV};
use crate::map_edit::{BrushIndex, EntityIndex, MAP_BRUSH_TEXTURE, MAP_OPEN};
use crate::map_textures::TextureSources;
use crate::preview::{HasPreview, Preview, PreviewConfig};
use crate::{Config, HasWorkbench, Workbench, WorkbenchError, register, register_preview};

const NEAR: f64 = 1e-6;

struct Context {
    workbench: Workbench<FsLoader>,
    preview: Preview,
}

impl HasWorkbench for Context {
    type Loader = FsLoader;

    fn workbench(&mut self) -> &mut Workbench<FsLoader> {
        &mut self.workbench
    }

    fn preview(&mut self) -> Option<&mut Preview> {
        Some(&mut self.preview)
    }
}

impl HasPreview for Context {
    fn workbench_and_preview(&mut self) -> (&mut Workbench<FsLoader>, &mut Preview) {
        (&mut self.workbench, &mut self.preview)
    }
}

/// The desktop's keep project, with its WAD and map.
fn keep() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop/fixtures/keep")
}

/// A session with the keep map open.
fn session() -> Session<Context> {
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    register_preview(&mut registry).unwrap();
    let context = Context {
        workbench: Workbench::new(FsLoader, Config::default()),
        preview: Preview::new(PreviewConfig {
            host: None,
            app: Path::new("/nowhere/app").to_path_buf(),
            game: None,
            extra: Vec::new(),
        }),
    };
    let mut session = Session::new(context, registry);
    session
        .execute("project.open", json!({ "path": keep() }))
        .unwrap();
    session
        .execute(MAP_OPEN, json!({ "path": keep().join("maps/keep.map") }))
        .unwrap();
    session
}

/// The world's first brush.
fn first_brush(session: &Session<Context>) -> Brush {
    let map = session.context().workbench.map().unwrap();
    map.file().entities[0].brushes().next().unwrap().clone()
}

fn face_ref(face: usize) -> Value {
    json!({ "entity": 0, "brush": 0, "face": face })
}

/// The texels of `point` on `face`.
fn texels(face: &Face, point: [f64; 3]) -> [f64; 2] {
    face.texvecs()
        .unwrap()
        .map(|axis| point[0] * axis[0] + point[1] * axis[1] + point[2] * axis[2] + axis[3])
}

/// The size the keep WAD gives `texture`.
fn size(texture: &str) -> [f64; 2] {
    let wad = std::fs::read(keep().join("gfx/keep.wad")).unwrap();
    let sources = TextureSources::from_files(vec![("gfx/keep.wad".to_owned(), wad)], None, None);
    sources.size(texture).unwrap().map(f64::from)
}

// spec MAP-13.2: map.face_uv fits each face's texture at the size its WAD gives it, in one step
// that undoes
#[test]
fn fit_uses_the_wad_s_size_and_undoes() {
    let mut session = session();
    let before = first_brush(&session);
    session
        .execute(
            MAP_FACE_UV,
            json!({ "faces": [face_ref(1), face_ref(2)], "edit": { "kind": "fit" } }),
        )
        .unwrap();
    let after = first_brush(&session);
    let polygons = after.face_polygons();
    for index in [1, 2] {
        let face = &after.faces[index];
        let wanted = size(&String::from_utf8_lossy(face.texture()));
        let polygon = polygons[index].as_ref().unwrap();
        for axis in 0..2 {
            let at: Vec<f64> = polygon.iter().map(|&p| texels(face, p)[axis]).collect();
            let lo = at.iter().copied().fold(f64::INFINITY, f64::min);
            let hi = at.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            assert!(
                (hi - lo - wanted[axis]).abs() < NEAR,
                "{lo} {hi} {wanted:?}"
            );
        }
    }
    assert_eq!(
        after.faces[0], before.faces[0],
        "only the faces named change"
    );
    session.execute("edit.undo", json!({})).unwrap();
    assert_eq!(first_brush(&session), before);
}

// spec MAP-13.4: an edit that needs no size works on a texture no WAD holds; one that needs a
// size is refused for it, as is a face that is not there, and neither changes the map
#[test]
fn missing_textures_and_faces_are_refused() {
    let mut session = session();
    session
        .execute(
            MAP_BRUSH_TEXTURE,
            json!({ "entity": 0, "brushes": [0], "texture": "nowhere" }),
        )
        .unwrap();
    let before = first_brush(&session);
    let error = session
        .execute(
            MAP_FACE_UV,
            json!({ "faces": [face_ref(0)], "edit": { "kind": "justify", "to": "left" } }),
        )
        .unwrap_err();
    assert!(
        error.to_string().contains(
            &WorkbenchError::NoTextureSize {
                texture: "nowhere".to_owned()
            }
            .to_string()
        ),
        "{error}"
    );
    assert_eq!(first_brush(&session), before);
    session
        .execute(
            MAP_FACE_UV,
            json!({ "faces": [face_ref(0)], "edit": { "kind": "flip", "axis": "t" } }),
        )
        .unwrap();
    assert_ne!(first_brush(&session).faces[0], before.faces[0]);
    let flipped = first_brush(&session);
    let error = session
        .execute(
            MAP_FACE_UV,
            json!({ "faces": [face_ref(1), face_ref(99)], "edit": { "kind": "reset" } }),
        )
        .unwrap_err();
    let wanted = WorkbenchError::NoFace {
        entity: EntityIndex::new(0),
        brush: BrushIndex::new(0),
        face: FaceIndex::new(99),
    };
    assert!(error.to_string().contains(&wanted.to_string()), "{error}");
    assert_eq!(
        first_brush(&session),
        flipped,
        "a refused edit changes nothing"
    );
    let error = session
        .execute(
            MAP_FACE_ALIGN,
            json!({ "from": face_ref(99), "to": [face_ref(1)] }),
        )
        .unwrap_err();
    assert!(error.to_string().contains(&wanted.to_string()), "{error}");
}

// spec MAP-13.8: map.face_align carries a face's alignment across the corner to another, no
// seam on their shared edge, in one step that undoes
#[test]
fn alignment_carries_across_the_corner() {
    let mut session = session();
    session
        .execute(
            MAP_BRUSH_TEXTURE,
            json!({ "entity": 0, "brushes": [0], "shift": [5.0, 3.0], "rotate": 30.0,
                    "scale": [0.5, 2.0] }),
        )
        .unwrap();
    let before = first_brush(&session);
    let polygons = before.face_polygons();
    let (top, west) = (polygons[1].clone().unwrap(), polygons[2].clone().unwrap());
    let shared: Vec<[f64; 3]> = top
        .iter()
        .copied()
        .filter(|p| {
            west.iter()
                .any(|q| (0..3).all(|i| (p[i] - q[i]).abs() < NEAR))
        })
        .collect();
    assert_eq!(shared.len(), 2);
    let seam = |brush: &Brush| {
        shared.iter().any(|&point| {
            let (want, got) = (
                texels(&brush.faces[1], point),
                texels(&brush.faces[2], point),
            );
            (want[0] - got[0]).abs() > NEAR || (want[1] - got[1]).abs() > NEAR
        })
    };
    assert!(seam(&before), "the fixture starts with a seam");
    session
        .execute(
            MAP_FACE_ALIGN,
            json!({ "from": face_ref(1), "to": [face_ref(2)] }),
        )
        .unwrap();
    let after = first_brush(&session);
    assert!(!seam(&after));
    assert_eq!(after.faces[1], before.faces[1], "the source is kept");
    session.execute("edit.undo", json!({})).unwrap();
    assert_eq!(first_brush(&session), before);
}

// spec MAP-13.11: map.face_uv adjusts by what is given, the rest left as it is, on a texture no
// WAD holds
#[test]
fn adjust_needs_no_size() {
    let mut session = session();
    session
        .execute(
            MAP_BRUSH_TEXTURE,
            json!({ "entity": 0, "brushes": [0], "texture": "nowhere", "shift": [1.0, 2.0],
                    "rotate": 0.0, "scale": [1.0, 1.0] }),
        )
        .unwrap();
    session
        .execute(
            MAP_FACE_UV,
            json!({ "faces": [face_ref(3)], "edit": { "kind": "adjust", "shift": [4.0, 0.0] } }),
        )
        .unwrap();
    assert_eq!(
        *first_brush(&session).faces[3].texdef(),
        qs_map::TexDef::Standard {
            shift: [5.0, 2.0],
            rotate: 0.0,
            scale: [1.0, 1.0],
        }
    );
}
