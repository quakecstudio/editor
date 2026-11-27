// SPDX-License-Identifier: GPL-3.0-or-later

//! The offscreen UI snapshot harness (P2-21, QA-3): each [`Script`] starts the app in a 1440 by
//! 900 window, runs its commands and keys, renders the window offscreen through wgpu with
//! `egui_kittest`, and compares the image with its golden PNG in `apps/desktop/snapshots/`.
//!
//! - `cargo nextest run -p quakec-studio snapshots` compares every script with its golden; a
//!   mismatch leaves `<name>.new.png` and `<name>.diff.png` beside it.
//! - `UPDATE_SNAPSHOTS=1` writes the goldens again (`make snapshots`).
//! - `QS_SCREENSHOTS=<dir>` also writes each image to `<dir>/<name>.png`, the docs' screenshots
//!   (`make screenshots`).
//!
//! The goldens are rendered on macOS. Text is rasterised by egui on the CPU and the same on
//! every OS; the GPU's anti-aliasing of edges is not, so each OS may differ in a few pixels
//! (`MAX_FAILED_PIXELS`), as egui's own snapshots allow.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test code: a failed unwrap or a mismatched snapshot is a failed test"
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::accesskit::Role;
use eframe::egui::{self, Event, Key, Modifiers};
use eframe::egui_wgpu;
use egui_kittest::kittest::Queryable as _;
use egui_kittest::{Harness, OsThreshold, SnapshotOptions};
use qs_map_edit::uv::{FaceIndex, FaceRef};
use qs_map_edit::{BrushIndex, EntityIndex};
use qs_render::Offscreen;
use serde_json::{Value, json};

use crate::App;
use crate::display::Display;
use crate::map_view::DEPTH_FORMAT;
use crate::map_view::tests::DepthRenderer;
use crate::strings::Text;
use crate::theme::{Contrast, Mode, Motion};

/// The window's size in points, the play-loop mockup's.
const SIZE: [f32; 2] = [1440.0, 900.0];

/// One step of a script.
#[derive(Clone, Debug)]
enum Step {
    /// Run a command by id with these parameters.
    Run(&'static str, Value),
    /// Press a key with modifiers.
    Key(Modifiers, Key),
    /// Type text.
    Type(&'static str),
    /// Run frames until every terminal tab's process has ended.
    WaitTerminals,
    /// Drop the file the function makes onto the lowest node with the label.
    Drop(fn() -> PathBuf, &'static str),
    /// Click the node with the label.
    Click(&'static str),
    /// Select these faces of the open map, each an entity, a brush and a face, as Alt-clicks
    /// would.
    Faces(&'static [[usize; 3]]),
    /// Show the look the system's display settings ask for (P7-12).
    Display(Display),
}

/// A named sequence of steps, whose final frame is the snapshot.
#[derive(Clone, Debug)]
struct Script {
    name: &'static str,
    steps: Vec<Step>,
}

/// Every script, each a golden image.
#[expect(
    clippy::too_many_lines,
    reason = "one list of every script, read from top to bottom as the goldens are"
)]
fn scripts() -> Vec<Script> {
    let toggle = |panel: &str| Step::Run("view.toggle_panel", json!({ "panel": panel }));
    vec![
        Script {
            name: "shell",
            steps: Vec::new(),
        },
        // L-28: the New Project dialog, from the Welcome page with the Project panel hidden,
        // so one button has the name.
        Script {
            name: "new-project",
            steps: vec![toggle("left"), Step::Click("New Project\u{2026}")],
        },
        // L-44: the command line lists the commands that match what is typed after a `/`.
        Script {
            name: "command-line",
            steps: vec![
                Step::Run(qs_layout::FOCUS_COMMAND_LINE, json!({})),
                Step::Type("/brush"),
            ],
        },
        Script {
            name: "shell-light",
            steps: vec![Step::Display(Display {
                mode: Mode::Light,
                ..Display::default()
            })],
        },
        Script {
            name: "shell-contrast",
            steps: vec![Step::Display(Display {
                contrast: Contrast::Increased,
                ..Display::default()
            })],
        },
        Script {
            name: "shell-light-contrast",
            steps: vec![Step::Display(Display {
                mode: Mode::Light,
                contrast: Contrast::Increased,
                motion: Motion::Reduced,
            })],
        },
        Script {
            name: "shell-zoom-200",
            steps: vec![Step::Run("view.zoom", json!({ "zoom": 2.0 }))],
        },
        Script {
            name: "shell-left-hidden",
            steps: vec![toggle("left")],
        },
        Script {
            name: "shell-editor-only",
            steps: vec![toggle("left"), toggle("right"), toggle("bottom")],
        },
        Script {
            name: "palette",
            steps: vec![
                Step::Key(Modifiers::COMMAND | Modifiers::SHIFT, Key::P),
                Step::Type("panel"),
            ],
        },
        Script {
            name: "palette-prompt",
            steps: vec![
                Step::Key(Modifiers::COMMAND | Modifiers::SHIFT, Key::P),
                Step::Type("view.toggle_panel"),
                Step::Key(Modifiers::NONE, Key::Enter),
            ],
        },
        Script {
            name: "project",
            steps: vec![
                Step::Run("project.open", json!({ "path": fixture() })),
                Step::Run(
                    "file.open",
                    json!({ "path": fixture().join("src/weapons.qc"), "offset": 269 }),
                ),
            ],
        },
        // P4-11, mockup 01: a field retyped where it is declared, and the next edit it
        // implies at the end of the line, with the pill that says which key takes it.
        Script {
            name: "next-edit",
            steps: vec![
                Step::Run("project.open", json!({ "path": fixture() })),
                Step::Run(
                    "file.open",
                    json!({ "path": fixture().join("defs.qc"), "offset": 186 }),
                ),
                Step::Run(qs_layout::FOCUS_EDITOR, json!({})),
                Step::Key(Modifiers::NONE, Key::Backspace),
                Step::Type("d"),
            ],
        },
        Script {
            name: "search",
            steps: vec![
                Step::Run("project.open", json!({ "path": fixture() })),
                Step::Run("view.project_search", json!({})),
                Step::Type("bounces"),
                Step::Key(Modifiers::NONE, Key::Enter),
            ],
        },
        Script {
            name: "terminal",
            steps: vec![
                Step::Run("project.open", json!({ "path": tasks_fixture() })),
                Step::Run("task.run", json!({ "name": "pack" })),
                Step::Run("task.run", json!({ "name": "check" })),
                Step::WaitTerminals,
            ],
        },
        Script {
            name: "map-editor",
            steps: vec![
                Step::Run("project.open", json!({ "path": keep_fixture() })),
                Step::Run(
                    "map.open",
                    json!({ "path": keep_fixture().join("maps/keep.map") }),
                ),
                Step::Run("view.map_shading", json!({ "shading": "textured" })),
                Step::Run("map.select", json!({ "entity": 2 })),
            ],
        },
        Script {
            name: "map-editor-zoom-200",
            steps: vec![
                Step::Run("view.zoom", json!({ "zoom": 2.0 })),
                Step::Run("project.open", json!({ "path": keep_fixture() })),
                Step::Run(
                    "map.open",
                    json!({ "path": keep_fixture().join("maps/keep.map") }),
                ),
                Step::Run("view.map_shading", json!({ "shading": "textured" })),
                Step::Run("map.select", json!({ "entity": 2 })),
            ],
        },
        Script {
            name: "map-lights",
            steps: vec![
                Step::Run("project.open", json!({ "path": keep_fixture() })),
                Step::Run(
                    "map.open",
                    json!({ "path": keep_fixture().join("maps/keep.map") }),
                ),
                Step::Run("view.map_shading", json!({ "shading": "textured" })),
                Step::Run("map.select", json!({ "entity": 3 })),
            ],
        },
        Script {
            name: "map-shapes",
            steps: vec![
                Step::Run("project.open", json!({ "path": keep_fixture() })),
                Step::Run(
                    "map.open",
                    json!({ "path": keep_fixture().join("maps/keep.map") }),
                ),
                Step::Run("view.map_shading", json!({ "shading": "textured" })),
                Step::Run(
                    "map.brush_shape",
                    json!({
                        "min": [-160, -96, 0], "max": [-32, 32, 128],
                        "shape": { "kind": "cylinder", "sides": 12, "hollow": 16 }
                    }),
                ),
                Step::Run(
                    "map.brush_shape",
                    json!({
                        "min": [32, -96, 0], "max": [160, -64, 128],
                        "shape": { "kind": "arch", "sides": 8, "wall": "filled" }
                    }),
                ),
                Step::Run(
                    "map.brush_shape",
                    json!({
                        "min": [32, 0, 0], "max": [160, 64, 96],
                        "shape": { "kind": "stairs", "climb": "east" }
                    }),
                ),
                Step::Run(
                    "map.brush_shape",
                    json!({
                        "min": [-96, 64, 0], "max": [0, 160, 96],
                        "shape": { "kind": "icospheroid", "subdivisions": 1 }
                    }),
                ),
            ],
        },
        Script {
            name: "map-panes",
            steps: vec![
                Step::Run("project.open", json!({ "path": keep_fixture() })),
                Step::Run(
                    "map.open",
                    json!({ "path": keep_fixture().join("maps/keep.map") }),
                ),
                Step::Run("view.map_shading", json!({ "shading": "textured" })),
                Step::Run("view.map_panes", json!({ "panes": "quad" })),
                Step::Run("map.select", json!({ "entity": 2 })),
            ],
        },
        Script {
            name: "map-leak",
            steps: vec![
                Step::Run("project.open", json!({ "path": keep_fixture() })),
                Step::Run(
                    "map.open",
                    json!({ "path": keep_fixture().join("maps/keep.map") }),
                ),
                Step::Run("view.map_shading", json!({ "shading": "textured" })),
                Step::Run("map.brush_delete", json!({ "entity": 0, "brush": 2 })),
                Step::Run(
                    "map.brush_texture",
                    json!({ "entity": 4, "brushes": [0], "texture": "nowhere" }),
                ),
                Step::Run("map.find_leaks", json!({})),
            ],
        },
        Script {
            name: "texture-import",
            steps: vec![
                Step::Run("project.open", json!({ "path": keep_fixture() })),
                Step::Run(
                    "map.open",
                    json!({ "path": keep_fixture().join("maps/keep.map") }),
                ),
                Step::Run("view.map_shading", json!({ "shading": "textured" })),
                Step::Drop(brick_png, "wall"),
            ],
        },
        Script {
            name: "map-issues",
            steps: vec![
                Step::Run("project.open", json!({ "path": keep_fixture() })),
                Step::Run(
                    "map.open",
                    json!({ "path": keep_fixture().join("maps/keep.map") }),
                ),
                Step::Run("view.map_shading", json!({ "shading": "textured" })),
                Step::Run(
                    "map.set_key",
                    json!({ "entity": 2, "key": "sped", "value": "200" }),
                ),
                Step::Run(
                    "map.brush_texture",
                    json!({ "entity": 4, "brushes": [0], "texture": "nowhere" }),
                ),
                Step::Click("Issues"),
                Step::Click("Check map"),
            ],
        },
        // UI-24: a new key offers, as it is typed, the keys the ogre's class reads, each with its
        // type, default and the class's comment.
        Script {
            name: "map-key-offers",
            steps: vec![
                Step::Run("project.open", json!({ "path": keep_fixture() })),
                Step::Run(
                    "map.open",
                    json!({ "path": keep_fixture().join("maps/keep.map") }),
                ),
                Step::Run("map.select", json!({ "entity": 2 })),
                Step::Click("New key"),
                Step::Type("s"),
            ],
        },
        // L-76: Fire in Properties fires the selected button, and the Logic tab lists what the
        // QuakeC did, the link it fired lit in the view.
        Script {
            name: "map-logic",
            steps: vec![
                Step::Run("project.open", json!({ "path": logic_fixture() })),
                Step::Run(
                    "map.open",
                    json!({ "path": logic_fixture().join("maps/logic.map") }),
                ),
                Step::Run("map.select", json!({ "entity": 1 })),
                Step::Click("Fire"),
            ],
        },
        // L-78: Count in the Balance tab lists the map's monsters, ammo, health, armour and
        // secrets a column a skill.
        Script {
            name: "map-balance",
            steps: vec![
                Step::Run("project.open", json!({ "path": balance_fixture() })),
                Step::Run(
                    "map.open",
                    json!({ "path": balance_fixture().join("maps/balance.map") }),
                ),
                Step::Click("Balance"),
                Step::Click("Count"),
            ],
        },
        Script {
            name: "map-uv",
            steps: vec![
                Step::Run("project.open", json!({ "path": keep_fixture() })),
                Step::Run(
                    "map.open",
                    json!({ "path": keep_fixture().join("maps/keep.map") }),
                ),
                Step::Run("view.map_shading", json!({ "shading": "textured" })),
                Step::Run(
                    "map.brush_texture",
                    json!({ "entity": 0, "brushes": [2], "rotate": 30.0, "scale": [0.5, 0.5] }),
                ),
                Step::Faces(&[[0, 2, 3], [0, 2, 5]]),
                Step::Run(
                    "map.face_uv",
                    json!({
                        "faces": [
                            { "entity": 0, "brush": 2, "face": 3 },
                            { "entity": 0, "brush": 2, "face": 5 }
                        ],
                        "edit": { "kind": "fit" }
                    }),
                ),
            ],
        },
        Script {
            name: "shell-resized",
            steps: vec![
                Step::Run(
                    "view.resize_panel",
                    json!({ "panel": "left", "size": 360.0 }),
                ),
                Step::Run(
                    "view.resize_panel",
                    json!({ "panel": "bottom", "size": 320.0 }),
                ),
            ],
        },
    ]
}

/// A file dropped on the window, by its path.
#[derive(Debug)]
struct Dropped(PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|error| error.to_string())
    }
}

/// A 32 by 32 PNG of red and yellow bricks, written to a temporary folder; its path.
fn brick_png() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("qs-snapshot-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap_or_else(|error| panic!("{}: {error}", dir.display()));
    let path = dir.join("brick.png");
    let image = image::RgbaImage::from_fn(32, 32, |x, y| {
        // Rows of bricks 8 high and 16 long, every other row shifted by half a brick.
        let shift = if (y >> 3) & 1 == 0 { 0 } else { 8 };
        let mortar = y & 7 == 0 || x.wrapping_add(shift) & 15 == 0;
        if mortar {
            image::Rgba([200, 190, 120, 255])
        } else {
            image::Rgba([150, 40, 30, 255])
        }
    });
    image
        .save_with_format(&path, image::ImageFormat::Png)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    path
}

/// The test project, with problems in two files.
fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/ember")
}

/// The project with a map, its entity classes from its QuakeC (P5-06).
fn keep_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/keep")
}

/// The project whose button fires a door through its QuakeC (L-76).
fn logic_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/logic")
}

/// The project whose map holds monsters and pickups that differ by skill (L-78).
fn balance_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/balance")
}

/// The project with two tasks, one that succeeds and one that fails (P4-36).
fn tasks_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/tasks")
}

impl Script {
    /// The most pixels each OS may differ from the macOS golden by. Linux's and Windows'
    /// software rasterizers filter textures a little differently, so a script that draws the
    /// map's 3D view textured may differ there by up to 6,000 pixels, half a percent of the
    /// window, as `map-textured` may (2,477 to 2,502 on the Linux runner on 2026-11-15); the
    /// rest by 400.
    fn max_failed_pixels(&self) -> OsThreshold<usize> {
        let textured = json!({ "shading": "textured" });
        let draws_textures = self
            .steps
            .iter()
            .any(|step| matches!(step, Step::Run("view.map_shading", p) if *p == textured));
        if draws_textures {
            OsThreshold::new(0).linux(6_000).windows(6_000)
        } else {
            OsThreshold::new(0).linux(400).windows(400)
        }
    }
}

/// Where the goldens are kept.
fn goldens() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("snapshots")
}

/// A harness running the app, made from the harness's context before the first frame so the
/// theme's fonts are in place when it draws, on a wgpu device with the window's depth buffer,
/// which the map editor's textured and lit views draw on (P7-13).
fn harness() -> Harness<'static, Option<App>> {
    let state = egui_kittest::wgpu::create_render_state(
        egui_kittest::wgpu::default_wgpu_setup(),
        egui_wgpu::RendererOptions {
            depth_stencil_format: Some(DEPTH_FORMAT),
            ..egui_wgpu::RendererOptions::PREDICTABLE
        },
    );
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_pixels_per_point(1.0)
        .renderer(DepthRenderer {
            offscreen: Offscreen::from_state(state.clone(), DEPTH_FORMAT),
        })
        .build_ui_state(
            |ui, app: &mut Option<App>| {
                if let Some(app) = app {
                    app.frame(ui);
                }
            },
            None,
        );
    let mut app = App::checked_on_the_frame(&harness.ctx, None).unwrap();
    app.shell_mut().set_render_state(Some(state));
    *harness.state_mut() = Some(app);
    harness
}

/// Runs `script` and returns the harness at its last frame.
fn run(script: &Script) -> Harness<'static, Option<App>> {
    let mut harness = harness();
    harness.run();
    for step in &script.steps {
        match step {
            Step::Run(id, params) => {
                let app = harness.state_mut().as_mut().unwrap();
                app.shell
                    .execute(id, params.clone())
                    .unwrap_or_else(|e| panic!("{}: {id}: {e}", script.name));
                // A task started runs and asks for frames until a WaitTerminals step.
                if id.starts_with("task.") {
                    harness.step();
                    continue;
                }
            }
            Step::Key(modifiers, key) => harness.key_press_modifiers(*modifiers, *key),
            Step::Type(text) => harness.event(Event::Text((*text).to_owned())),
            Step::Drop(make, onto) => {
                let over = harness
                    .query_all_by_label(onto)
                    .max_by(|a, b| a.rect().center().y.total_cmp(&b.rect().center().y))
                    .unwrap_or_else(|| panic!("{}: nothing labelled {onto}", script.name))
                    .rect()
                    .center();
                harness.hover_at(over);
                harness
                    .input_mut()
                    .dropped_files
                    .push(Arc::new(Dropped(make())));
            }
            Step::Click(label) => harness.get_by_label(label).click(),
            Step::Faces(faces) => {
                let faces: Vec<FaceRef> = faces
                    .iter()
                    .map(|&[entity, brush, face]| FaceRef {
                        entity: EntityIndex::new(entity),
                        brush: BrushIndex::new(brush),
                        face: FaceIndex::new(face),
                    })
                    .collect();
                let app = harness.state_mut().as_mut().unwrap();
                app.shell.select_map_faces(&faces);
            }
            Step::Display(display) => {
                let ctx = harness.ctx.clone();
                let app = harness.state_mut().as_mut().unwrap();
                app.show_display(&ctx, *display);
            }
            Step::WaitTerminals => {
                let running = |harness: &Harness<'static, Option<App>>| {
                    let shell = &harness.state().as_ref().unwrap().shell;
                    let terminals = shell.session().context().workbench.terminals();
                    terminals.tabs().iter().any(|tab| tab.exit().is_none())
                };
                // Each frame polls the tabs without waiting; the loop ends with the processes.
                // A running tab asks for frames, so each is one step, not a run to rest.
                while running(&harness) {
                    harness.step();
                    std::thread::yield_now();
                }
            }
        }
        harness.run();
    }
    harness
}

// QA-3: every script's last frame matches its golden image, within each OS's allowance.
#[test]
fn every_script_matches_its_golden() {
    let screenshots = std::env::var_os("QS_SCREENSHOTS").map(PathBuf::from);
    let mut failures = Vec::new();
    for script in scripts() {
        let mut harness = run(&script);
        if let Some(dir) = &screenshots {
            std::fs::create_dir_all(dir).unwrap();
            let image = harness.render().unwrap();
            image
                .save(dir.join(format!("{}.png", script.name)))
                .unwrap();
        }
        let options = SnapshotOptions::new()
            .output_path(goldens())
            .max_failed_pixels(script.max_failed_pixels());
        if let Err(error) = harness.try_snapshot_options(script.name, &options) {
            failures.push(format!("{}: {error}", script.name));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

// No spec statement: UI-24, ADR-0028: the toolbar's Pop Out and Full Screen are buttons, drawn
// framed so they are not read as the label beside them; Pop Out moves the game to its own
// window and becomes Pop In
#[test]
fn pop_out_is_a_button_that_says_what_it_does() {
    let mut harness = harness();
    harness.run();
    harness.get_by_role_and_label(Role::Button, Text::FullScreen.get());
    harness
        .get_by_role_and_label(Role::Button, Text::PopOut.get())
        .click();
    harness.run();
    harness.run();
    harness.get_by_role_and_label(Role::Button, Text::PopIn.get());
}

// QA-3: a script's steps change what is drawn, so a golden pins the state it ran to.
#[test]
fn a_script_changes_the_image() {
    let plain = run(&Script {
        name: "plain",
        steps: Vec::new(),
    })
    .render()
    .unwrap();
    let hidden = run(&Script {
        name: "hidden",
        steps: vec![Step::Run("view.toggle_panel", json!({ "panel": "left" }))],
    })
    .render()
    .unwrap();
    assert_eq!(plain.dimensions(), (1440, 900));
    assert_ne!(plain.as_raw(), hidden.as_raw());
    let typed = run(&Script {
        name: "typed",
        steps: vec![Step::Key(Modifiers::NONE, Key::Escape), Step::Type("x")],
    })
    .render()
    .unwrap();
    assert_eq!(
        plain.as_raw(),
        typed.as_raw(),
        "keys with no binding draw nothing new"
    );
}
