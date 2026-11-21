// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "test code: a failed unwrap, index or panic is a failed test"
)]

use std::path::{Path, PathBuf};

use qc_project::FsLoader;
use qs_commands::{Registry, Session};
use qs_mdl::mdl::make::{Pose, PoseEntry, Shape};
use qs_mdl::mdl::write::{Model, ModelFrameEntry, ModelSkin};
use qs_mdl::mdl::{MDL_DT_FACES_FRONT, MDL_ST_RAND, StVert, Triangle};
use serde_json::json;

use super::*;
use crate::map_compile::tests::Context;
use crate::preview::{Preview, PreviewConfig};
use crate::{Config, Workbench, register, register_preview};

/// A pyramid's corners with its tip at `tip`.
fn pyramid(tip: [f32; 3]) -> Vec<[f32; 3]> {
    vec![
        [-8.0, -8.0, 0.0],
        [8.0, -8.0, 0.0],
        [8.0, 8.0, 0.0],
        [-8.0, 8.0, 0.0],
        tip,
    ]
}

/// A model of a pyramid with the frames `frames`, each a name and its tip, and one skin of 8
/// by 4 whose pixels count up from 16: the first row of the palette's second row of shades.
fn model(frames: &[(&str, [f32; 3])]) -> Vec<u8> {
    let front = |vertindex: [usize; 3]| Triangle {
        facesfront: MDL_DT_FACES_FRONT,
        vertindex,
    };
    Shape {
        skin_width: 8,
        skin_height: 4,
        skins: vec![ModelSkin::Single((16..48).collect())],
        stverts: vec![
            StVert {
                onseam: 0,
                s: 1,
                t: 1
            };
            5
        ],
        triangles: vec![
            front([4, 1, 0]),
            front([4, 2, 1]),
            front([4, 3, 2]),
            front([4, 0, 3]),
            front([1, 2, 0]),
            front([2, 3, 0]),
        ],
        poses: frames
            .iter()
            .map(|(name, tip)| {
                PoseEntry::Single(Pose {
                    name: name.as_bytes().to_vec(),
                    positions: pyramid(*tip),
                })
            })
            .collect(),
        eye_position: [0.0, 0.0, 12.0],
        flags: 0,
        synctype: MDL_ST_RAND,
    }
    .build()
    .unwrap()
    .to_bytes()
    .unwrap()
}

/// A palette whose colour `n` is `(n, 255 - n, n / 2)`: every colour its own.
fn palette() -> Vec<u8> {
    (0..=u8::MAX).flat_map(|n| [n, !n, n >> 1]).collect()
}

/// A project in a folder of its own, with a palette and the model `progs/thing.mdl` of two
/// frames, and a session over it; and a game folder beside it when `game` holds files, each a
/// name and its bytes, in its `id1/pak0.pak`.
fn project(name: &str, game: &[(&str, Vec<u8>)]) -> (PathBuf, Session<Context>) {
    let dir = std::env::temp_dir().join(format!("qs-wb-models-{name}-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    let root = dir.join("mod");
    std::fs::create_dir_all(root.join("progs")).unwrap();
    std::fs::create_dir_all(root.join("gfx")).unwrap();
    std::fs::write(root.join("progs.src"), "progs.dat\nworld.qc\n").unwrap();
    std::fs::write(root.join("world.qc"), "void() worldspawn = {};\n").unwrap();
    std::fs::write(root.join("gfx/palette.lmp"), palette()).unwrap();
    std::fs::write(
        root.join("progs/thing.mdl"),
        model(&[("stand1", [0.0, 0.0, 16.0]), ("stand2", [0.0, 0.0, 12.0])]),
    )
    .unwrap();
    let base = (!game.is_empty()).then(|| {
        // The game keeps its files in a PAK, and is the full game, whose search path is read
        // (FS-4.2): id's pop table in gfx/pop.lmp.
        let base = dir.join("game");
        std::fs::create_dir_all(base.join("id1")).unwrap();
        let pop: Vec<u8> = qs_vfs::FS_POP_TABLE
            .iter()
            .flat_map(|value| value.to_be_bytes())
            .collect();
        let mut files: Vec<(&str, &[u8])> = vec![(qs_vfs::FS_POP_PATH, pop.as_slice())];
        files.extend(game.iter().map(|(file, bytes)| (*file, bytes.as_slice())));
        std::fs::write(
            base.join("id1/pak0.pak"),
            qs_pak::write_pak(&files).unwrap(),
        )
        .unwrap();
        base
    });
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    register_preview(&mut registry).unwrap();
    let context = Context::new(
        Workbench::new(FsLoader, Config::default()),
        Preview::new(PreviewConfig {
            host: None,
            app: Path::new("/nowhere/app").to_path_buf(),
            game: base,
            extra: Vec::new(),
        }),
    );
    let mut session = Session::new(context, registry);
    session
        .execute("project.open", json!({ "path": root }))
        .unwrap();
    (root, session)
}

fn read(path: &Path) -> Model {
    Model::read(&std::fs::read(path).unwrap()).unwrap()
}

fn names(model: &Model) -> Vec<String> {
    frames(model).map(name_of).collect()
}

// No spec statement: L-47, `model.inspect` answers a model's skins and the rows of the
// palette each is of, most pixels first, its vertices, triangles, the box its frames fill and its frames by name, as a $frame line lists them
#[test]
fn a_model_is_inspected() {
    let (root, mut session) = project("inspect", &[]);
    let seen = session
        .execute(
            MODEL_INSPECT,
            json!({ "path": root.join("progs/thing.mdl") }),
        )
        .unwrap();
    assert_eq!(seen["skins"], 1);
    // The skin's colours are 16 to 47: sixteen of row 1 and sixteen of row 2.
    assert_eq!(
        seen["skin_rows"],
        json!([[{ "row": 1, "pixels": 16 }, { "row": 2, "pixels": 16 }]])
    );
    assert_eq!(seen["skin_size"], json!([8, 4]));
    assert_eq!(
        (seen["vertices"].clone(), seen["triangles"].clone()),
        (json!(5), json!(6))
    );
    assert_eq!(
        seen["frames"],
        json!([{ "names": ["stand1"] }, { "names": ["stand2"] }])
    );
    assert_eq!(seen["frame_line"], "stand1 stand2");
    let bounds: [[f32; 3]; 2] = serde_json::from_value(seen["bounds"].clone()).unwrap();
    for (at, was) in bounds
        .iter()
        .flatten()
        .zip([-8.0, -8.0, 0.0, 8.0, 8.0, 16.0])
    {
        assert!((at - was).abs() < 0.1, "{bounds:?}");
    }
    let error = session
        .execute(MODEL_INSPECT, json!({ "path": root.join("progs.src") }))
        .unwrap_err()
        .to_string();
    assert!(error.contains("is not an alias model"), "{error}");
}

// No spec statement: L-47, `model.copy_from_game` makes a project's own file of a model of
// the game, refuses a file that is there and a name the game has none of
#[test]
fn a_games_model_is_copied_into_the_project() {
    let soldier = model(&[("walk1", [0.0, 0.0, 16.0])]);
    let (root, mut session) = project("copy", &[("progs/soldier.mdl", soldier.clone())]);
    let made = root.join("progs/brute.mdl");
    let answer = session
        .execute(
            MODEL_COPY,
            json!({ "name": "progs/soldier.mdl", "path": made }),
        )
        .unwrap();
    assert_eq!(answer["frames"], 1);
    assert_eq!(std::fs::read(&made).unwrap(), soldier);
    let again = session.execute(
        MODEL_COPY,
        json!({ "name": "progs/soldier.mdl", "path": made }),
    );
    assert!(again.unwrap_err().to_string().ends_with("is there already"));
    let error = session
        .execute(
            MODEL_COPY,
            json!({ "name": "progs/ogre.mdl", "path": root.join("progs/o.mdl") }),
        )
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("the game has no file progs/ogre.mdl"),
        "{error}"
    );
    assert!(!root.join("progs/o.mdl").exists());
}

// No spec statement: L-47, a skin goes out as a PNG in the game's palette and comes back the
// same; a PNG of another size is refused; a pixel becomes the palette's nearest colour
#[test]
fn a_skin_goes_out_as_a_png_and_comes_back() {
    let (root, mut session) = project("skin", &[]);
    let path = root.join("progs/thing.mdl");
    let png = root.join("skins/thing.png");
    let out = session
        .execute(MODEL_EXPORT_SKIN, json!({ "path": path, "png_path": png }))
        .unwrap();
    assert_eq!(out["size"], json!([8, 4]));
    let image = image::open(&png).unwrap().into_rgba8();
    assert_eq!((image.width(), image.height()), (8, 4));
    // The first pixel is the palette's colour 16.
    assert_eq!(image.get_pixel(0, 0).0, [16, 239, 8, 255]);
    let before = std::fs::read(&path).unwrap();
    session
        .execute(MODEL_SET_SKIN, json!({ "path": path, "png_path": png }))
        .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), before, "the same skin back");

    // A painted pixel takes the nearest colour of the palette: (201, 54, 100) is colour 201.
    let mut painted = image;
    painted.put_pixel(3, 2, image::Rgba([201, 54, 100, 255]));
    painted.save(&png).unwrap();
    session
        .execute(MODEL_SET_SKIN, json!({ "path": path, "png_path": png }))
        .unwrap();
    let ModelSkin::Single(pixels) = &read(&path).skins[0] else {
        panic!("single");
    };
    assert_eq!(pixels[2 * 8 + 3], 201);
    assert_eq!(pixels[0], 16);
    session.undo().unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), before);

    let small = root.join("skins/small.png");
    image::RgbaImage::new(4, 4).save(&small).unwrap();
    let error = session
        .execute(MODEL_SET_SKIN, json!({ "path": path, "png_path": small }))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("the image is 4 by 4: a skin of this model is 8 by 4"),
        "{error}"
    );
    let error = session
        .execute(
            MODEL_EXPORT_SKIN,
            json!({ "path": path, "skin": 1, "png_path": png }),
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("no single skin 1: it has 1"), "{error}");
}

// spec REND-4.13: a skin is recoloured by rows of the palette, a pixel keeping its shade, and
// between a row below 128 and one from 128 on the shade is counted from the other end
#[test]
fn a_skin_is_recoloured_by_rows() {
    let (root, mut session) = project("recolor", &[]);
    let path = root.join("progs/thing.mdl");
    let before = std::fs::read(&path).unwrap();
    // The skin holds rows 1 and 2, colours 16 to 47. Row 1 goes to row 4, row 2 to row 9.
    let answer = session
        .execute(
            MODEL_RECOLOR,
            json!({ "path": path, "rows": [[1, 4], [2, 9]] }),
        )
        .unwrap();
    assert_eq!(answer["pixels"], 32);
    let ModelSkin::Single(pixels) = &read(&path).skins[0] else {
        panic!("single");
    };
    let row_four: Vec<u8> = (64..80).collect();
    assert_eq!(pixels[..16], row_four);
    // Row 9 runs backwards: the first shade of row 2 is its last.
    let row_nine: Vec<u8> = (144..160).rev().collect();
    assert_eq!(pixels[16..], row_nine);
    session.undo().unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), before);
    // A row the skin has none of changes nothing.
    let none = session
        .execute(MODEL_RECOLOR, json!({ "path": path, "rows": [[7, 3]] }))
        .unwrap();
    assert_eq!(none["pixels"], 0);
    let error = session
        .execute(MODEL_RECOLOR, json!({ "path": path, "rows": [[1, 16]] }))
        .unwrap_err()
        .to_string();
    assert!(error.contains("16 is no row of the palette"), "{error}");
}

// spec MDL-5.3: a model is scaled and moved by its header's step and origin, its packed
// vertices as they were, so each vertex is where it was times the scale, or moved by the move
#[test]
fn a_model_is_scaled_and_moved() {
    let (root, mut session) = project("scale", &[]);
    let path = root.join("progs/thing.mdl");
    let was = read(&path);
    let answer = session
        .execute(MODEL_SCALE, json!({ "path": path, "by": [2.0, 2.0, 1.5] }))
        .unwrap();
    let now = read(&path);
    assert_eq!(now.frames, was.frames, "no vertex is packed again");
    for ((scaled, step), by) in now
        .header
        .scale
        .iter()
        .zip(was.header.scale)
        .zip([2.0, 2.0, 1.5])
    {
        assert!((scaled - step * by).abs() < 1e-6);
    }
    assert_eq!(now.header.scale_origin, [-16.0, -16.0, 0.0]);
    assert_eq!(now.header.eye_position, [0.0, 0.0, 18.0]);
    assert!((now.header.bounding_radius - was.header.bounding_radius * 2.0).abs() < 1e-4);
    let bounds: [[f32; 3]; 2] = serde_json::from_value(answer["bounds"].clone()).unwrap();
    for (at, want) in bounds
        .iter()
        .flatten()
        .zip([-16.0, -16.0, 0.0, 16.0, 16.0, 24.0])
    {
        assert!((at - want).abs() < 0.2, "{bounds:?}");
    }
    session
        .execute(MODEL_MOVE, json!({ "path": path, "by": [0.0, 0.0, -24.0] }))
        .unwrap();
    let moved = read(&path);
    assert_eq!(moved.header.scale_origin, [-16.0, -16.0, -24.0]);
    assert_eq!(moved.frames, was.frames);
    session.undo().unwrap();
    session.undo().unwrap();
    assert_eq!(read(&path), was);
    for by in [json!([0.0, 1.0, 1.0]), json!([-1.0, 1.0, 1.0])] {
        let error = session
            .execute(MODEL_SCALE, json!({ "path": path, "by": by }))
            .unwrap_err()
            .to_string();
        assert!(error.contains("a scale is a number above 0"), "{error}");
    }
}

// No spec statement: L-47, frames are renamed and taken out by name, and added from another
// model of the same vertices, each vertex packed into the model's own box; a frame that
// reaches outside the box, a model of other vertices and a name there is none of are refused
#[test]
fn frames_are_renamed_removed_and_added() {
    let (root, mut session) = project("frames", &[]);
    let path = root.join("progs/thing.mdl");
    session
        .execute(
            MODEL_RENAME_FRAMES,
            json!({ "path": path, "names": [["stand2", "idle1"]] }),
        )
        .unwrap();
    assert_eq!(names(&read(&path)), ["stand1", "idle1"]);
    let error = session
        .execute(
            MODEL_RENAME_FRAMES,
            json!({ "path": path, "names": [["nosuch", "x"]] }),
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("no frame named nosuch"), "{error}");
    let error = session
        .execute(
            MODEL_RENAME_FRAMES,
            json!({ "path": path, "names": [["idle1", "a_name_of_sixteen"]] }),
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("at most 15 bytes"), "{error}");

    // Another copy of the model, its tip lower and to one side: within this model's box.
    let other = root.join("progs/poses.mdl");
    std::fs::write(
        &other,
        model(&[("pain1", [4.0, -4.0, 8.0]), ("tall1", [0.0, 0.0, 40.0])]),
    )
    .unwrap();
    let added = session
        .execute(
            MODEL_ADD_FRAMES,
            json!({ "path": path, "from_path": other, "frames": ["pain1"] }),
        )
        .unwrap();
    assert_eq!(added["frames"], 3);
    let model_now = read(&path);
    assert_eq!(names(&model_now), ["stand1", "idle1", "pain1"]);
    let ModelFrameEntry::Single(pain) = &model_now.frames[2] else {
        panic!("single");
    };
    let mdl_bytes = std::fs::read(&path).unwrap();
    let mdl = qs_mdl::Mdl::read(&mdl_bytes).unwrap();
    let tip = mdl.position(pain.verts[4]);
    for (at, want) in tip.iter().zip([4.0, -4.0, 8.0]) {
        assert!((at - want).abs() < 0.1, "{tip:?}");
    }
    // The other model's tall frame reaches above this model's box.
    let error = session
        .execute(
            MODEL_ADD_FRAMES,
            json!({ "path": path, "from_path": other, "frames": ["tall1"] }),
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("tall1 reaches outside the box"), "{error}");
    assert_eq!(names(&read(&path)).len(), 3);

    session
        .execute(
            MODEL_REMOVE_FRAMES,
            json!({ "path": path, "frames": ["idle1"] }),
        )
        .unwrap();
    assert_eq!(names(&read(&path)), ["stand1", "pain1"]);
    session.undo().unwrap();
    assert_eq!(names(&read(&path)), ["stand1", "idle1", "pain1"]);
    session.redo().unwrap();
    assert_eq!(names(&read(&path)), ["stand1", "pain1"]);
    // A model cannot lose its last frame: the loader would refuse it.
    let error = session
        .execute(
            MODEL_REMOVE_FRAMES,
            json!({ "path": path, "frames": ["stand1", "pain1"] }),
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("the model cannot be written"), "{error}");
    assert_eq!(names(&read(&path)), ["stand1", "pain1"]);
}
