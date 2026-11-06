// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::path::{Path, PathBuf};

use qc_project::MemoryLoader;

use super::{ScenarioRunError, image_path};
use crate::preview::{Preview, PreviewConfig, PreviewError};
use crate::{Config, Workbench};

/// A fresh directory for the test `name`.
fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "qs-workbench-scenario-{name}-{}",
        std::process::id()
    ));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn preview() -> Preview {
    Preview::new(PreviewConfig {
        host: None,
        app: PathBuf::from("/nowhere/app"),
        game: Some(PathBuf::from("/nowhere/game")),
        extra: Vec::new(),
    })
}

// spec SCN-2.6: a screenshot is written beside its scenario as <scenario>-<name>.png
#[test]
fn a_screenshot_is_named_after_its_scenario() {
    assert_eq!(
        image_path(Path::new("/p/scenarios/bounce.scenario"), "hall"),
        PathBuf::from("/p/scenarios/bounce-hall.png")
    );
    assert_eq!(
        image_path(Path::new("bounce"), "a-1"),
        PathBuf::from("bounce-a-1.png")
    );
}

// spec SCN-2.6: scenario.run reads the file first, refusing one it cannot read or that does
// not read as SCN-1 says, with its path, then needs a project that builds
#[test]
fn a_scenario_is_read_before_the_project_builds() {
    let root = dir("read");
    let mut workbench = Workbench::new(MemoryLoader::new(), Config::default());
    let missing = root.join("missing.scenario");
    let error = preview()
        .run_scenario(&mut workbench, &missing)
        .unwrap_err();
    assert!(matches!(error, ScenarioRunError::Read(..)), "{error}");
    assert!(error.to_string().contains("missing.scenario"));
    let bad = root.join("bad.scenario");
    std::fs::write(&bad, "map a\nfly\n").unwrap();
    let error = preview().run_scenario(&mut workbench, &bad).unwrap_err();
    assert!(matches!(error, ScenarioRunError::Parse(..)), "{error}");
    assert!(
        error.to_string().ends_with("line 2: unknown line"),
        "{error}"
    );
    let good = root.join("good.scenario");
    std::fs::write(&good, "map start\nframes 1\n").unwrap();
    let error = preview().run_scenario(&mut workbench, &good).unwrap_err();
    assert!(
        matches!(error, ScenarioRunError::Preview(PreviewError::Build(_))),
        "{error}"
    );
}
