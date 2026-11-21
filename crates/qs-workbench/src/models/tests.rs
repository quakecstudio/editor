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

    // spec WAD-7.4: a painted colour nearest a fullbright one, (230, 25, 115) of colour 230,
    // takes the nearest ordinary colour, 223, and does not glow; a colour that is exactly a
    // fullbright one, colour 240, is that one, and stays so when the skin goes out and back.
    let mut painted = image::open(&png).unwrap().into_rgba8();
    painted.put_pixel(0, 0, image::Rgba([230, 26, 114, 255]));
    painted.put_pixel(1, 0, image::Rgba([240, 15, 120, 255]));
    painted.save(&png).unwrap();
    session
        .execute(MODEL_SET_SKIN, json!({ "path": path, "png_path": png }))
        .unwrap();
    let glowing = std::fs::read(&path).unwrap();
    let ModelSkin::Single(pixels) = &read(&path).skins[0] else {
        panic!("single");
    };
    assert_eq!(pixels[..2], [223, 240]);
    session
        .execute(MODEL_EXPORT_SKIN, json!({ "path": path, "png_path": png }))
        .unwrap();
    session
        .execute(MODEL_SET_SKIN, json!({ "path": path, "png_path": png }))
        .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), glowing);

    // A PNG that cannot be written is said to be that, not to be no PNG.
    std::fs::write(root.join("a-file"), b"").unwrap();
    let error = session
        .execute(
            MODEL_EXPORT_SKIN,
            json!({ "path": path, "png_path": root.join("a-file/skin.png") }),
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("cannot be written"), "{error}");

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
    // Frames are renamed at once, by the names the file has: two change names with each
    // other, and a pair whose frame is another pair's new name is refused, nothing written.
    session
        .execute(
            MODEL_RENAME_FRAMES,
            json!({ "path": path, "names": [["stand1", "idle1"], ["idle1", "stand1"]] }),
        )
        .unwrap();
    assert_eq!(names(&read(&path)), ["idle1", "stand1"]);
    let error = session
        .execute(
            MODEL_RENAME_FRAMES,
            json!({ "path": path, "names": [["idle1", "a"], ["a", "b"]] }),
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("no frame named a"), "{error}");
    session
        .execute(
            MODEL_RENAME_FRAMES,
            json!({ "path": path, "names": [["stand1", "idle1"], ["idle1", "stand1"]] }),
        )
        .unwrap();
    assert_eq!(names(&read(&path)), ["stand1", "idle1"]);

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

/// A model of one flat square in the plane z = 0, its far corner at `corner`, with the
/// frames `frames`.
fn flat(frames: &[(&str, [f32; 2])]) -> Vec<u8> {
    let mut made = Model::read(&model(&[("stand1", [0.0, 0.0, 16.0])])).unwrap();
    let shape = Shape {
        skin_width: made.header.skinwidth,
        skin_height: made.header.skinheight,
        skins: std::mem::take(&mut made.skins),
        stverts: made.stverts.clone(),
        triangles: made.triangles.clone(),
        poses: frames
            .iter()
            .map(|(name, [x, y])| {
                PoseEntry::Single(Pose {
                    name: name.as_bytes().to_vec(),
                    positions: pyramid([*x, *y, 0.0]),
                })
            })
            .collect(),
        eye_position: [0.0; 3],
        flags: 0,
        synctype: MDL_ST_RAND,
    };
    shape.build().unwrap().to_bytes().unwrap()
}

// spec MDL-7.2: a model flat on an axis has a step of 0 there; a frame of another copy that
// lies in its plane is added, each vertex at byte 0 on that axis, and one that leaves the
// plane is outside its box. A move that is no number is refused as a move.
#[test]
fn a_flat_model_takes_frames_in_its_plane() {
    let (root, mut session) = project("flat", &[]);
    let path = root.join("progs/flat.mdl");
    let other = root.join("progs/flat-poses.mdl");
    std::fs::write(&path, flat(&[("lie1", [0.0, 0.0])])).unwrap();
    std::fs::write(&other, flat(&[("lie2", [4.0, 4.0])])).unwrap();
    assert!(read(&path).header.scale[2].abs() <= 0.0);
    let added = session
        .execute(
            MODEL_ADD_FRAMES,
            json!({ "path": path, "from_path": other, "frames": ["lie2"] }),
        )
        .unwrap();
    assert_eq!(added["frames"], 2);
    let ModelFrameEntry::Single(lie) = &read(&path).frames[1] else {
        panic!("single");
    };
    assert!(lie.verts.iter().all(|vert| vert.v[2] == 0));

    let error = session
        .execute(
            MODEL_ADD_FRAMES,
            json!({
                "path": path,
                "from_path": root.join("progs/thing.mdl"),
                "frames": ["stand1"],
            }),
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("stand1 reaches outside the box"), "{error}");

    let error = session
        .execute(MODEL_MOVE, json!({ "path": path, "by": [1e39, 0.0, 0.0] }))
        .unwrap_err()
        .to_string();
    assert!(error.contains("a move is a number on each axis"), "{error}");
}

/// A crate one unit a side as an `.obj` file with y up, its faces counterclockwise from
/// outside, with texture vertices when `textured`.
fn crate_obj(textured: bool) -> String {
    let mut text = String::from("# a crate\no crate\n");
    for z in [0, 1] {
        for (x, y) in [(0, 0), (1, 0), (1, 1), (0, 1)] {
            text.push_str(&format!("v {x} {y} {z}\n"));
        }
    }
    if textured {
        text.push_str("vt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\n");
    }
    // Back, front, bottom, right, top, left.
    for face in [
        [4, 3, 2, 1],
        [5, 6, 7, 8],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 4, 8, 7],
        [4, 1, 5, 8],
    ] {
        text.push('f');
        for (corner, vertex) in face.into_iter().enumerate() {
            if textured {
                text.push_str(&format!(" {vertex}/{}", corner + 1));
            } else {
                text.push_str(&format!(" {vertex}"));
            }
        }
        text.push('\n');
    }
    text
}

// spec MDL-9.1, MDL-9.3: an .obj with texture vertices is imported as a model that keeps
// them, on the PNG given as its skin, scaled and with y made up; the model loads, has the
// file's one pose, and is where the file's positions are
#[test]
fn an_obj_is_imported_with_its_own_skin() {
    let (root, mut session) = project("import-obj", &[]);
    let source = root.join("meshes/crate.obj");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(&source, crate_obj(true)).unwrap();
    // A picture of 8 by 4, each pixel a colour of the palette: colour n is (n, 255 - n, n / 2).
    let png = root.join("meshes/crate.png");
    let mut picture = image::RgbaImage::new(8, 4);
    for (x, y, pixel) in picture.enumerate_pixels_mut() {
        let n = u8::try_from(y * 8 + x).unwrap() + 32;
        *pixel = image::Rgba([n, !n, n >> 1, 255]);
    }
    picture.save(&png).unwrap();
    let path = root.join("progs/crate.mdl");
    let made = session
        .execute(
            MODEL_IMPORT,
            json!({ "source": source, "path": path, "skin_png": png, "scale": 32.0 }),
        )
        .unwrap();
    assert_eq!(made["layout"], "own");
    assert_eq!(made["skin_size"], json!([8, 4]));
    assert_eq!(made["triangles"], 12);
    assert_eq!(made["frame_line"], "base");
    let model = read(&path);
    assert_eq!(model.skins, [ModelSkin::Single((32..64).collect())]);
    // With y up, the file's z of 0 to 1 is Quake's x of -32 to 0, and its y is z.
    let seen = session
        .execute(MODEL_INSPECT, json!({ "path": path }))
        .unwrap();
    let bounds: [[f32; 3]; 2] = serde_json::from_value(seen["bounds"].clone()).unwrap();
    for (at, want) in bounds
        .iter()
        .flatten()
        .zip([-32.0, -32.0, 0.0, 0.0, 0.0, 32.0])
    {
        assert!((at - want).abs() < 0.2, "{bounds:?}");
    }
    // A file that is there is not written over.
    let error = session
        .execute(MODEL_IMPORT, json!({ "source": source, "path": path }))
        .unwrap_err()
        .to_string();
    assert!(error.contains("is there already"), "{error}");
}

// spec MDL-9.4: an .obj without texture vertices is imported laid out front and back, its
// skin one colour of the size the layout needs; a PNG is refused for it; its skin can then
// go out, be painted and come back
#[test]
fn an_obj_without_coordinates_is_laid_out() {
    let (root, mut session) = project("import-flat", &[]);
    let source = root.join("crate.obj");
    std::fs::write(&source, crate_obj(false)).unwrap();
    let path = root.join("progs/crate.mdl");
    let made = session
        .execute(
            MODEL_IMPORT,
            json!({ "source": source, "path": path, "up": "z", "scale": 16.0, "colour": 20 }),
        )
        .unwrap();
    assert_eq!(made["layout"], "front_and_back");
    // 16 units wide and high: 8 pixels a unit and 4 around, two halves.
    assert_eq!(made["skin_size"], json!([264, 132]));
    assert_eq!(
        (made["vertices"].clone(), made["triangles"].clone()),
        (json!(8), json!(12))
    );
    let model = read(&path);
    assert_eq!(model.skins, [ModelSkin::Single(vec![20; 264 * 132])]);
    // The face that looks back along x is on the skin's back half, the rest on the front.
    let back = model
        .triangles
        .iter()
        .filter(|triangle| triangle.facesfront == 0)
        .count();
    assert_eq!(back, 2);

    let png = root.join("skin.png");
    session
        .execute(MODEL_EXPORT_SKIN, json!({ "path": path, "png_path": png }))
        .unwrap();
    session
        .execute(MODEL_SET_SKIN, json!({ "path": path, "png_path": png }))
        .unwrap();
    let other = root.join("progs/other.mdl");
    let error = session
        .execute(
            MODEL_IMPORT,
            json!({ "source": source, "path": other, "skin_png": png }),
        )
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("the mesh has no texture coordinates"),
        "{error}"
    );
    assert!(!other.exists());
}

// spec MDL-9.2: an .iqe with an animation is imported with a frame for each of its frames,
// named for QuakeC, each vertex where its joints take it
#[test]
fn an_iqe_is_imported_with_its_animation() {
    let (root, mut session) = project("import-iqe", &[]);
    let source = root.join("arm.iqe");
    std::fs::write(
        &source,
        "# Inter-Quake Export\n\
         joint \"root\" -1\n\tpq 0 0 0 0 0 0 1\n\
         joint \"arm\" 0\n\tpq 8 0 0 0 0 0 1\n\
         mesh \"arm\"\n\
         vp 0 0 0\n\tvb 0 1\n\
         vp 0 4 0\n\tvb 0 1\n\
         vp 0 0 4\n\tvb 0 1\n\
         vp 16 0 0\n\tvb 1 1\n\
         fm 0 1 2\nfm 0 2 3\nfm 0 3 1\nfm 1 3 2\n\
         animation \"wave\"\n\
         frame\npq 0 0 0 0 0 0 1\npq 8 0 0 0 0 0 1\n\
         frame\npq 0 0 0 0 0 0 1\npq 8 0 0 0 0 0.70710678 0.70710678\n",
    )
    .unwrap();
    let path = root.join("progs/arm.mdl");
    let made = session
        .execute(MODEL_IMPORT, json!({ "source": source, "path": path }))
        .unwrap();
    assert_eq!(made["frame_line"], "wave1 wave2");
    assert_eq!(made["layout"], "front_and_back");
    let model = read(&path);
    let bytes = std::fs::read(&path).unwrap();
    let mdl = qs_mdl::Mdl::read(&bytes).unwrap();
    let tip = |entry: usize| {
        let ModelFrameEntry::Single(frame) = &model.frames[entry] else {
            panic!("single");
        };
        mdl.position(frame.verts[3])
    };
    // The arm's vertex, 8 along the arm, is turned a quarter about the arm's joint.
    for (at, want) in tip(0).iter().zip([16.0, 0.0, 0.0]) {
        assert!((at - want).abs() < 0.1, "{:?}", tip(0));
    }
    for (at, want) in tip(1).iter().zip([8.0, 8.0, 0.0]) {
        assert!((at - want).abs() < 0.1, "{:?}", tip(1));
    }
}

// No spec statement: L-47, model.import says what it cannot read: a file that is no mesh
// it reads, a line of an .obj or an .iqe that is wrong, a scale that is none, a file that is
// not there; and it makes nothing then
#[test]
fn an_import_that_fails_says_why() {
    let (root, mut session) = project("import-refused", &[]);
    let write = |name: &str, text: &str| {
        let path = root.join(name);
        std::fs::write(&path, text).unwrap();
        path
    };
    let path = root.join("progs/made.mdl");
    let mut refused = |params: serde_json::Value| {
        let error = session
            .execute(MODEL_IMPORT, params)
            .unwrap_err()
            .to_string();
        assert!(!path.exists(), "{error}");
        error
    };
    let fbx = write("thing.fbx", "");
    let error = refused(json!({ "source": fbx, "path": path }));
    assert!(error.contains("is no .obj or .iqe file"), "{error}");
    let obj = write("bad.obj", "v 0 0 0\nf 1 2 3\n");
    let error = refused(json!({ "source": obj, "path": path }));
    assert!(
        error.contains("bad.obj: line 2: a face refers to a vertex"),
        "{error}"
    );
    let iqe = write(
        "bad.iqe",
        "# Inter-Quake Export\njoint a -1\npa 0 0 0 0 0 0\n",
    );
    let error = refused(json!({ "source": iqe, "path": path }));
    assert!(
        error.contains("bad.iqe: line 3: a pa pose is not read"),
        "{error}"
    );
    let empty = write("empty.obj", "v 0 0 0\n");
    let error = refused(json!({ "source": empty, "path": path }));
    assert!(error.contains("the mesh has no triangle"), "{error}");
    let good = write("good.obj", &crate_obj(false));
    let error = refused(json!({ "source": good, "path": path, "scale": 0.0 }));
    assert!(error.contains("a scale is a number above 0"), "{error}");
    let error = refused(json!({ "source": root.join("none.obj"), "path": path }));
    assert!(error.contains("cannot be read"), "{error}");
}
