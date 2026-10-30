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

use eframe::egui::{Event, Key, Modifiers};
use egui_kittest::{Harness, OsThreshold, SnapshotOptions};
use serde_json::{Value, json};

use crate::App;

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
}

/// A named sequence of steps, whose final frame is the snapshot.
#[derive(Clone, Debug)]
struct Script {
    name: &'static str,
    steps: Vec<Step>,
}

/// Every script, each a golden image.
fn scripts() -> Vec<Script> {
    let toggle = |panel: &str| Step::Run("view.toggle_panel", json!({ "panel": panel }));
    vec![
        Script {
            name: "shell",
            steps: Vec::new(),
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

/// The most pixels each OS may differ from the macOS golden by.
fn max_failed_pixels() -> OsThreshold<usize> {
    OsThreshold::new(0).linux(400).windows(400)
}

/// Where the goldens are kept.
fn goldens() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("snapshots")
}

/// A harness running the app, made from the harness's context before the first frame so the
/// theme's fonts are in place when it draws.
fn harness() -> Harness<'static, Option<App>> {
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_pixels_per_point(1.0)
        .wgpu()
        .build_ui_state(
            |ui, app: &mut Option<App>| {
                if let Some(app) = app {
                    app.frame(ui);
                }
            },
            None,
        );
    let app = App::new(&harness.ctx, None).unwrap();
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
            }
            Step::Key(modifiers, key) => harness.key_press_modifiers(*modifiers, *key),
            Step::Type(text) => harness.event(Event::Text((*text).to_owned())),
        }
        harness.run();
    }
    harness
}

// QA-3: every script's last frame matches its golden image, within each OS's allowance.
#[test]
fn every_script_matches_its_golden() {
    let options = SnapshotOptions::new()
        .output_path(goldens())
        .max_failed_pixels(max_failed_pixels());
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
        if let Err(error) = harness.try_snapshot_options(script.name, &options) {
            failures.push(format!("{}: {error}", script.name));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
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
