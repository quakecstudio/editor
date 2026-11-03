// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::float_cmp_const,
    reason = "test code: a failed unwrap is a failed test; sizes are compared exactly as written"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use qs_commands::{CommandError, Registry, Session};
use serde_json::json;

use crate::{
    FOCUS_BOTTOM_PANEL, FOCUS_EDITOR, FOCUS_LEFT_PANEL, FOCUS_RIGHT_PANEL, Focus, LAYOUT_FILE,
    Layout, LayoutError, MIN_PANEL_SIZE, Panel, RESIZE_PANEL, TOGGLE_PANEL, layout_path, register,
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A fresh directory under the system's temporary folder, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("qs-layout-{}-{name}-{n}", std::process::id()));
        let _ignored = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ignored = fs::remove_dir_all(&self.0);
    }
}

fn session() -> Session<Layout> {
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    Session::new(Layout::default(), registry)
}

#[test]
fn every_panel_starts_shown_at_the_mockup_sizes() {
    let layout = Layout::default();
    for panel in Panel::ALL {
        assert!(layout.panel(panel).visible);
    }
    assert_eq!(
        Panel::ALL.map(|panel| layout.panel(panel).size),
        [240.0, 320.0, 230.0]
    );
    assert_eq!(
        layout_path(Path::new("/d")),
        Path::new("/d").join(LAYOUT_FILE)
    );
}

#[test]
fn toggling_a_panel_hides_and_shows_it_outside_the_history() {
    let mut session = session();
    let shown = session
        .execute("view.toggle_panel", json!({ "panel": "bottom" }))
        .unwrap();
    assert_eq!(shown, json!({ "panel": "bottom", "visible": false }));
    assert!(!session.context().bottom.visible);
    assert!(session.context().left.visible && session.context().right.visible);
    assert!(matches!(session.undo(), Err(CommandError::NothingToUndo)));
    session
        .execute("view.toggle_panel", json!({ "panel": "bottom" }))
        .unwrap();
    assert!(session.context().bottom.visible);
    session
        .execute("view.toggle_panel", json!({ "panel": "left" }))
        .unwrap();
    session
        .execute("view.toggle_panel", json!({ "panel": "right" }))
        .unwrap();
    assert!(!session.context().left.visible && !session.context().right.visible);
}

#[test]
fn a_panel_the_command_does_not_know_is_refused() {
    let mut session = session();
    for params in [
        json!({ "panel": "top" }),
        json!({}),
        json!({ "panel": "left", "x": 1 }),
    ] {
        assert!(matches!(
            session.execute("view.toggle_panel", params),
            Err(CommandError::InvalidParams { .. })
        ));
    }
    assert_eq!(*session.context(), Layout::default());
}

#[test]
fn the_commands_register_once() {
    let mut registry = Registry::<Layout>::new();
    register(&mut registry).unwrap();
    assert!(registry.get(TOGGLE_PANEL).is_some());
    assert!(registry.get(RESIZE_PANEL).is_some());
    for id in [
        FOCUS_EDITOR,
        FOCUS_LEFT_PANEL,
        FOCUS_RIGHT_PANEL,
        FOCUS_BOTTOM_PANEL,
    ] {
        assert!(registry.get(id).is_some(), "{id}");
    }
    assert!(register(&mut registry).is_err());
}

// QA-9: a focus command gives its area the keyboard, shows a hidden panel first, and is
// outside the history.
#[test]
fn focusing_an_area_shows_it_and_gives_it_the_keyboard() {
    let mut session = session();
    assert_eq!(session.context().focus, Focus::Editor);
    session
        .execute(TOGGLE_PANEL, json!({ "panel": "bottom" }))
        .unwrap();
    assert!(!session.context().bottom.visible);
    for (id, focus) in [
        (FOCUS_BOTTOM_PANEL, Focus::Panel(Panel::Bottom)),
        (FOCUS_LEFT_PANEL, Focus::Panel(Panel::Left)),
        (FOCUS_RIGHT_PANEL, Focus::Panel(Panel::Right)),
        (FOCUS_EDITOR, Focus::Editor),
    ] {
        let before = session.context().focus_requests;
        let focused = session.execute(id, json!({})).unwrap();
        assert_eq!(
            focused,
            json!({ "focus": serde_json::to_value(focus).unwrap() })
        );
        assert_eq!(session.context().focus, focus);
        assert_eq!(session.context().focus_requests, before + 1);
    }
    assert!(session.context().bottom.visible);
    assert!(matches!(session.undo(), Err(CommandError::NothingToUndo)));
    let error = session
        .execute(FOCUS_EDITOR, json!({ "panel": "left" }))
        .unwrap_err();
    assert!(
        matches!(error, CommandError::InvalidParams { .. }),
        "{error}"
    );
}

// QA-9: the focus is part of the layout file, and a file from before it defaults to the
// editor.
#[test]
fn the_focus_survives_a_restart() {
    let dir = TempDir::new("focus");
    let path = dir.path().join(LAYOUT_FILE);
    let layout = Layout {
        focus: Focus::Panel(Panel::Bottom),
        ..Layout::default()
    };
    layout.write(&path).unwrap();
    assert_eq!(
        Layout::read(&path).unwrap().focus,
        Focus::Panel(Panel::Bottom)
    );
    let requested = Layout {
        focus_requests: 3,
        ..layout
    };
    requested.write(&path).unwrap();
    assert_eq!(
        Layout::read(&path).unwrap(),
        layout,
        "the count is not written"
    );
    assert_eq!(requested.as_written(), layout);
    fs::write(&path, "[left]\nvisible = false\nsize = 200.0\n").unwrap();
    assert_eq!(Layout::read(&path).unwrap().focus, Focus::Editor);
}

#[test]
fn a_layout_survives_a_restart() {
    let dir = TempDir::new("round-trip");
    let path = layout_path(&dir.path().join("data"));
    assert_eq!(Layout::read(&path).unwrap(), Layout::default());
    let mut layout = Layout::default();
    layout.panel_mut(Panel::Right).visible = false;
    layout.panel_mut(Panel::Left).size = 300.5;
    layout.write(&path).unwrap();
    assert_eq!(Layout::read(&path).unwrap(), layout);
}

#[test]
fn a_partial_file_keeps_the_defaults_for_the_rest() {
    let dir = TempDir::new("partial");
    let path = dir.path().join(LAYOUT_FILE);
    fs::write(&path, "[left]\nvisible = false\nsize = 200.0\n").unwrap();
    let layout = Layout::read(&path).unwrap();
    assert!(!layout.left.visible);
    assert_eq!(layout.right, Layout::default().right);
}

#[test]
fn a_bad_layout_file_is_an_error() {
    let dir = TempDir::new("bad");
    let path = dir.path().join(LAYOUT_FILE);
    for text in [
        "[left\n",
        "[top]\nvisible = true\nsize = 1.0\n",
        "[left]\nvisible = 1\n",
    ] {
        fs::write(&path, text).unwrap();
        let error = Layout::read(&path).unwrap_err();
        assert!(
            matches!(error, LayoutError::Parse { .. }),
            "{text:?}: {error}"
        );
        assert!(
            error.to_string().contains("is not a layout file"),
            "{error}"
        );
        assert!(std::error::Error::source(&error).is_some());
    }
    // A directory where the file should be cannot be read, and cannot be written.
    let blocked = dir.path().join("blocked");
    fs::create_dir_all(&blocked).unwrap();
    let error = Layout::read(&blocked).unwrap_err();
    assert!(matches!(error, LayoutError::Read { .. }), "{error}");
    assert!(error.to_string().starts_with("cannot read "), "{error}");
    let error = Layout::default().write(&blocked).unwrap_err();
    assert!(matches!(error, LayoutError::Write { .. }), "{error}");
    assert!(error.to_string().starts_with("cannot write "), "{error}");
    assert!(std::error::Error::source(&error).is_some());
    // A file where its directory should be.
    let file = dir.path().join("file");
    fs::write(&file, "").unwrap();
    let error = Layout::default()
        .write(&file.join(LAYOUT_FILE))
        .unwrap_err();
    assert!(matches!(error, LayoutError::Write { .. }), "{error}");
}

#[test]
fn resizing_a_panel_sets_its_size_outside_the_history() {
    let mut session = session();
    session
        .execute(RESIZE_PANEL, json!({ "panel": "right", "size": 400.5 }))
        .unwrap();
    assert_eq!(session.context().right.size, 400.5);
    session
        .execute(
            RESIZE_PANEL,
            json!({ "panel": "bottom", "size": MIN_PANEL_SIZE }),
        )
        .unwrap();
    assert_eq!(session.context().bottom.size, MIN_PANEL_SIZE);
    assert!(matches!(session.undo(), Err(CommandError::NothingToUndo)));
}

#[test]
fn a_panel_too_small_or_not_a_size_is_refused() {
    let mut session = session();
    for size in [json!(119.9), json!(0.0), json!(-300.0), json!(1e40)] {
        let error = session
            .execute(RESIZE_PANEL, json!({ "panel": "left", "size": size }))
            .unwrap_err();
        assert!(
            matches!(error, CommandError::Failed { .. }),
            "{size}: {error}"
        );
        assert!(error.to_string().contains("at least 120 points"), "{error}");
    }
    assert_eq!(*session.context(), Layout::default());
}
