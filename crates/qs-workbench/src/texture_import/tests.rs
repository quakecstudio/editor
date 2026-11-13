// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::path::{Path, PathBuf};

use qc_project::FsLoader;
use qs_commands::{Registry, Session};
use qs_wad::Wad;
use qs_wad::texture::Palette;
use serde_json::json;

use super::{DitherChoice, TEXTURE_IMPORT, TextureImported, project_wad, quantized};
use crate::map_edit::MAP_OPEN;
use crate::preview::{HasPreview, Preview, PreviewConfig};
use crate::{Config, HasWorkbench, Workbench, register, register_preview};

/// A workbench and a preview with no game.
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

/// The desktop's keep project, with its palette, WAD and map.
fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop/fixtures/keep")
}

/// A copy of the keep project in a folder of its own named `name`.
fn keep(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join(format!("qs-wb-import-{}", std::process::id()))
        .join(name);
    let _gone = std::fs::remove_dir_all(&dir);
    for file in [
        "progs.src",
        "defs.qc",
        "world.qc",
        "maps/keep.map",
        "gfx/keep.wad",
        "gfx/palette.lmp",
    ] {
        let to = dir.join(file);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(fixture().join(file), to).unwrap();
    }
    dir
}

/// A `width` by `height` PNG of one colour, written to `path`.
fn png(path: &Path, width: u32, height: u32, colour: [u8; 4]) {
    let image = image::RgbaImage::from_pixel(width, height, image::Rgba(colour));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    std::fs::write(path, bytes.into_inner()).unwrap();
}

fn session(project: Option<&Path>) -> Session<Context> {
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
    if let Some(project) = project {
        session
            .execute("project.open", json!({ "path": project }))
            .unwrap();
    }
    session
}

fn import(session: &mut Session<Context>, params: serde_json::Value) -> TextureImported {
    serde_json::from_value(session.execute(TEXTURE_IMPORT, params).unwrap()).unwrap()
}

/// The world's `wad` key in the open map.
fn wad_key(session: &Session<Context>) -> String {
    let map = session.context().workbench.map().unwrap();
    String::from_utf8_lossy(map.file().entities[0].value(b"wad").unwrap()).into_owned()
}

/// The lumps of the WAD at `path`, by name.
fn lumps(path: &Path) -> Vec<(String, Vec<u8>)> {
    let bytes = std::fs::read(path).unwrap();
    let wad = Wad::read(&bytes).unwrap();
    wad.lumps()
        .iter()
        .map(|lump| {
            (
                String::from_utf8_lossy(lump.name.as_bytes()).into_owned(),
                wad.data(lump).unwrap().to_vec(),
            )
        })
        .collect()
}

// spec WAD-7.1: an imported PNG is quantized into the project's WAD, as the preview shows it,
// and the open map's world names the WAD, in one step that undoes
#[test]
fn a_png_is_imported_into_the_project_s_wad() {
    let dir = keep("import");
    let image = dir.join("brick16.png");
    png(&image, 16, 16, [200, 40, 30, 255]);
    let mut session = session(Some(&dir));
    session
        .execute(MAP_OPEN, json!({ "path": dir.join("maps/keep.map") }))
        .unwrap();
    let before = wad_key(&session);
    let done = import(&mut session, json!({ "path": image }));
    assert_eq!(done.name, "brick16");
    assert_eq!(done.wad, project_wad(&dir));
    assert_eq!(done.wad, dir.join("textures/import.wad"));
    assert!(!done.replaced && done.map_names_wad);
    assert_eq!(wad_key(&session), format!("{before};textures/import.wad"));
    let written = lumps(&done.wad);
    assert_eq!(written.len(), 1);
    let palette = Palette::from_lmp(&std::fs::read(dir.join("gfx/palette.lmp")).unwrap()).unwrap();
    let shown = quantized(
        &std::fs::read(&image).unwrap(),
        "brick16",
        &palette,
        DitherChoice::None,
    )
    .unwrap();
    assert_eq!(palette.decode_miptex(&written[0].1).unwrap(), shown);
    session.execute("edit.undo", json!({})).unwrap();
    assert_eq!(wad_key(&session), before);
    // The WAD named already, a second import leaves the key alone and adds to the WAD.
    session.execute("edit.redo", json!({})).unwrap();
    let other = dir.join("stone.png");
    png(&other, 32, 16, [90, 90, 90, 255]);
    let second = import(&mut session, json!({ "path": other, "name": "+0stone" }));
    assert!(second.map_names_wad);
    assert_eq!(wad_key(&session), format!("{before};textures/import.wad"));
    let names: Vec<String> = lumps(&second.wad).into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["brick16", "+0stone"]);
}

// No spec statement: P6-16: importing a changed PNG under a name already in the WAD replaces
// that texture in its place
#[test]
fn a_changed_png_replaces_its_texture_in_place() {
    let dir = keep("reimport");
    let mut session = session(Some(&dir));
    let (a, b) = (dir.join("a.png"), dir.join("b.png"));
    png(&a, 16, 16, [200, 40, 30, 255]);
    png(&b, 16, 16, [30, 40, 200, 255]);
    import(&mut session, json!({ "path": a }));
    import(&mut session, json!({ "path": b }));
    let wad = project_wad(&dir);
    let before = lumps(&wad);
    png(&a, 16, 16, [30, 200, 40, 255]);
    let again = import(&mut session, json!({ "path": a }));
    assert!(again.replaced);
    assert!(!again.map_names_wad, "no map is open");
    let after = lumps(&wad);
    assert_eq!(
        after.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
        ["a", "b"],
        "kept in its place"
    );
    assert_ne!(after[0].1, before[0].1);
    assert_eq!(after[1].1, before[1].1);
}

// spec WAD-6.1: a name or size Quake's textures cannot take, a file that is no PNG, one that is
// not there, no palette and no project are each refused, and nothing is written
#[test]
fn what_cannot_be_a_texture_is_refused() {
    let dir = keep("refused");
    let mut session = session(Some(&dir));
    let odd = dir.join("odd.png");
    png(&odd, 17, 16, [1, 2, 3, 255]);
    let good = dir.join("good.png");
    png(&good, 16, 16, [1, 2, 3, 255]);
    let text = dir.join("text.png");
    std::fs::write(&text, b"no image here").unwrap();
    for (params, says) in [
        (json!({ "path": odd }), "17"),
        (
            json!({ "path": good, "name": "far too long a name" }),
            "far too long",
        ),
        (json!({ "path": text }), "text.png"),
        (json!({ "path": dir.join("gone.png") }), "gone.png"),
    ] {
        let error = session.execute(TEXTURE_IMPORT, params).unwrap_err();
        assert!(error.to_string().contains(says), "{error}");
    }
    assert!(!project_wad(&dir).exists());
    std::fs::remove_file(dir.join("gfx/palette.lmp")).unwrap();
    let error = session
        .execute(TEXTURE_IMPORT, json!({ "path": good }))
        .unwrap_err();
    assert!(error.to_string().contains("palette"), "{error}");
    let mut alone = session_alone();
    let error = alone
        .execute(TEXTURE_IMPORT, json!({ "path": good }))
        .unwrap_err();
    assert!(error.to_string().contains("project"), "{error}");
}

/// A session with no project open.
fn session_alone() -> Session<Context> {
    session(None)
}
