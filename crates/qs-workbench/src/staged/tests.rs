// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap or index is a failed test; counts of a few lines"
)]

use std::path::{Path, PathBuf};

use proptest::prelude::*;
use qc_project::{FsLoader, MemoryLoader};
use qs_commands::{Registry, Session};
use serde_json::json;

use super::HunkId;
use crate::{
    ACCEPT_HUNKS, APPLY_EDIT, Config, Decision, REJECT_HUNKS, STAGED_EDITS, StagedHunk, Workbench,
    WorkbenchError, register,
};

const ROOT: &str = "/p";

/// What a test project holds; the default is one that builds.
struct ProjectSpec {
    defs: String,
    world: String,
}

impl Default for ProjectSpec {
    fn default() -> Self {
        Self {
            defs: "float speed = 300;\n".to_owned(),
            world: "void() worldspawn = {\n\tlocal float x;\n\tx = speed * 2;\n};\n".to_owned(),
        }
    }
}

impl ProjectSpec {
    fn files(&self) -> [(&'static str, &str); 3] {
        [
            ("progs.src", "progs.dat\ndefs.qc\nworld.qc\n"),
            ("defs.qc", self.defs.as_str()),
            ("world.qc", self.world.as_str()),
        ]
    }
}

fn path(name: &str) -> PathBuf {
    Path::new(ROOT).join(name)
}

/// A workbench over `spec` in memory, its project open and checked on the frame.
fn in_memory(spec: &ProjectSpec) -> Workbench<MemoryLoader> {
    let mut loader = MemoryLoader::new();
    for (name, text) in spec.files() {
        loader.insert(path(name), text.as_bytes().to_vec());
    }
    let mut workbench = Workbench::new(loader, Config::default());
    workbench.open_project(Path::new(ROOT)).unwrap();
    workbench
}

/// A fresh directory for the test `name`, holding `spec`'s files on disk.
fn on_disk(name: &str, spec: &ProjectSpec) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("qs-workbench-staged-{name}-{}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    std::fs::create_dir_all(&dir).unwrap();
    for (file, text) in spec.files() {
        std::fs::write(dir.join(file), text).unwrap();
    }
    dir
}

fn ids(hunks: &[StagedHunk]) -> Vec<HunkId> {
    hunks.iter().map(|hunk| hunk.id).collect()
}

const WORLD: &str = "void() worldspawn = {\n\tlocal float x;\n\tx = speed * 3;\n};\n\
    void() staged_function = {};\n";

// No spec statement: P4-04, AGT-3. A change to two files is staged as hunks; until they are
// accepted the files on disk are untouched, and a build compiles the staged text
#[test]
fn staged_hunks_are_compiled_but_not_written() {
    let dir = on_disk("compiled", &ProjectSpec::default());
    let mut workbench = Workbench::new(FsLoader, Config::default());
    workbench.open_project(&dir).unwrap();
    let hunks = workbench
        .stage(vec![
            (dir.join("world.qc"), WORLD.as_bytes().to_vec()),
            (dir.join("defs.qc"), b"float speed = 320;\n".to_vec()),
        ])
        .unwrap();
    assert_eq!(hunks.len(), 3, "{hunks:?}");
    assert_eq!(hunks[0].path, dir.join("defs.qc"), "in path order");
    assert_eq!(hunks[0].removed, "float speed = 300;\n");
    assert_eq!(hunks[0].added, "float speed = 320;\n");
    assert_eq!(hunks[1].line, 3);
    assert_eq!(
        hunks[2].line, 5,
        "a hunk that only adds lines goes before line 5"
    );
    assert_eq!(hunks[2].removed, "");
    let on_disk_world = || std::fs::read_to_string(dir.join("world.qc")).unwrap();
    assert_eq!(
        on_disk_world(),
        ProjectSpec::default().world,
        "nothing is written to disk"
    );
    let built = workbench.build().unwrap();
    assert!(built[0].built, "{:?}", built[0].problems);
    let progs = std::fs::read(&built[0].output).unwrap();
    assert!(
        progs.windows(15).any(|w| w == b"staged_function"),
        "the build compiled the staged text"
    );
    assert_eq!(
        workbench.decide(&ids(&hunks), Decision::Accept).unwrap(),
        []
    );
    assert_eq!(
        workbench.text(&dir.join("world.qc")).unwrap(),
        WORLD.as_bytes()
    );
    assert!(workbench.is_unsaved(&dir.join("world.qc")));
    assert_eq!(
        on_disk_world(),
        ProjectSpec::default().world,
        "accepting gives the editor's text, which file.save writes"
    );
    workbench.save(&dir.join("world.qc")).unwrap();
    assert_eq!(on_disk_world(), WORLD);
}

// No spec statement: P4-04. The problems are those of the staged text before any hunk is
// accepted, and rejecting the hunk takes them away again
#[test]
fn the_semantic_model_reads_the_staged_text() {
    let mut workbench = in_memory(&ProjectSpec::default());
    assert!(workbench.problems().is_empty());
    let broken = "void() worldspawn = {\n\tlocal float x;\n\tx = nowhere * 2;\n};\n";
    let hunks = workbench
        .stage(vec![(path("world.qc"), broken.as_bytes().to_vec())])
        .unwrap();
    assert!(
        workbench
            .problems()
            .iter()
            .any(|problem| problem.path == path("world.qc")),
        "{:?}",
        workbench.problems()
    );
    workbench.decide(&ids(&hunks), Decision::Reject).unwrap();
    assert!(
        workbench.problems().is_empty(),
        "{:?}",
        workbench.problems()
    );
}

// No spec statement: P4-04, the acceptance criterion. Rejecting every hunk, one at a time,
// leaves the file byte-identical, read from disk again rather than an unsaved text
#[test]
fn rejecting_every_hunk_leaves_the_file_as_it_was() {
    let spec = ProjectSpec {
        world: "void() worldspawn = {\r\n};\r\n// no final line feed".to_owned(),
        ..ProjectSpec::default()
    };
    let mut workbench = in_memory(&spec);
    let hunks = workbench
        .stage(vec![(
            path("world.qc"),
            b"// first\nvoid() worldspawn = {\r\n\tx;\n};\r\n".to_vec(),
        )])
        .unwrap();
    assert!(hunks.len() >= 2, "{hunks:?}");
    assert!(workbench.is_unsaved(&path("world.qc")));
    let left = workbench.decide(&[hunks[0].id], Decision::Reject).unwrap();
    assert_eq!(left, hunks[1..]);
    workbench.decide(&ids(&left), Decision::Reject).unwrap();
    assert_eq!(
        workbench.text(&path("world.qc")).unwrap(),
        spec.world.as_bytes()
    );
    assert!(!workbench.is_unsaved(&path("world.qc")));
    assert!(workbench.staged_hunks().is_empty());
}

// No spec statement: P4-04. A file with an editor's text before it was staged keeps that text
// when every hunk is rejected, still unsaved
#[test]
fn rejecting_keeps_an_editors_unsaved_text() {
    let mut workbench = in_memory(&ProjectSpec::default());
    let typed = b"void() worldspawn = {};\n".to_vec();
    workbench.update(&path("world.qc"), typed.clone()).unwrap();
    let hunks = workbench
        .stage(vec![(path("world.qc"), b"void() other = {};\n".to_vec())])
        .unwrap();
    workbench.decide(&ids(&hunks), Decision::Reject).unwrap();
    assert_eq!(workbench.text(&path("world.qc")).unwrap(), typed);
    assert!(workbench.is_unsaved(&path("world.qc")));
}

// No spec statement: P4-04. A number that is not a waiting hunk is refused before anything is
// decided; a file with no change has no hunks; an edit in the editor drops the file's hunks;
// a new file is staged as one hunk
#[test]
fn edges_of_staging() {
    let mut workbench = in_memory(&ProjectSpec::default());
    let hunks = workbench
        .stage(vec![
            (path("world.qc"), WORLD.as_bytes().to_vec()),
            (path("defs.qc"), ProjectSpec::default().defs.into_bytes()),
        ])
        .unwrap();
    assert!(hunks.iter().all(|hunk| hunk.path == path("world.qc")));
    assert!(
        !workbench.is_unsaved(&path("defs.qc")),
        "an unchanged file is left alone"
    );
    let unknown = HunkId(9_999);
    assert!(matches!(
        workbench.decide(&[hunks[0].id, unknown], Decision::Accept),
        Err(WorkbenchError::NoHunk { id }) if id == unknown
    ));
    assert_eq!(workbench.staged_hunks(), hunks, "nothing was decided");
    workbench.decide(&[hunks[0].id], Decision::Accept).unwrap();
    assert!(
        matches!(
            workbench.decide(&[hunks[0].id], Decision::Reject),
            Err(WorkbenchError::NoHunk { .. })
        ),
        "a decided hunk is not waiting"
    );
    workbench
        .update(&path("world.qc"), b"void() worldspawn = {};\n".to_vec())
        .unwrap();
    assert!(
        workbench.staged_hunks().is_empty(),
        "the editor's text took their place"
    );
    let new_file = workbench
        .stage(vec![(path("new.qc"), b"float added;\n".to_vec())])
        .unwrap();
    assert_eq!(new_file.len(), 1);
    assert_eq!(new_file[0].line, 1);
    workbench.decide(&ids(&new_file), Decision::Reject).unwrap();
    assert!(!workbench.is_unsaved(&path("new.qc")));
}

// No spec statement: P4-04. Staging a staged file again stages it against its text from
// before, with fresh hunks
#[test]
fn staging_again_starts_from_the_text_before() {
    let mut workbench = in_memory(&ProjectSpec::default());
    workbench
        .stage(vec![(path("defs.qc"), b"float speed = 1;\n".to_vec())])
        .unwrap();
    let again = workbench
        .stage(vec![(path("defs.qc"), b"float speed = 2;\n".to_vec())])
        .unwrap();
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].removed, "float speed = 300;\n");
    assert_eq!(again[0].added, "float speed = 2;\n");
}

// No spec statement: P4-04. The edit commands run through the registry, as the agent runs
// them, outside the undo history
#[test]
fn the_edit_commands() {
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut session = Session::new(in_memory(&ProjectSpec::default()), registry);
    let mut run = |id: &str, params: serde_json::Value| -> Vec<StagedHunk> {
        serde_json::from_value(session.execute(id, params).unwrap()).unwrap()
    };
    let staged = run(
        APPLY_EDIT,
        json!({ "files": [{ "path": path("world.qc"), "text": WORLD }] }),
    );
    assert_eq!(staged.len(), 2);
    assert_eq!(run(STAGED_EDITS, json!({})), staged);
    let left = run(ACCEPT_HUNKS, json!({ "hunks": [staged[0].id] }));
    assert_eq!(left, staged[1..]);
    assert!(run(REJECT_HUNKS, json!({ "hunks": [staged[1].id] })).is_empty());
    assert!(
        session
            .execute(REJECT_HUNKS, json!({ "hunks": [staged[1].id] }))
            .is_err()
    );
    assert!(
        session
            .execute(APPLY_EDIT, json!({ "files": [], "x": 1 }))
            .is_err()
    );
    assert_eq!(session.undo_depth(), 0);
    assert!(
        session.context().is_unsaved(&path("world.qc")),
        "one hunk was accepted"
    );
}

/// `before` with the accepted hunks of `hunks` put in place, by their lines.
fn expected(before: &str, hunks: &[StagedHunk], accepted: &[bool]) -> String {
    let lines: Vec<&str> = before.split_inclusive('\n').collect();
    let mut out = String::new();
    let mut at = 0;
    for (hunk, &accept) in hunks.iter().zip(accepted) {
        let start = hunk.line - 1;
        out.extend(lines[at..start].iter().copied());
        out.push_str(if accept { &hunk.added } else { &hunk.removed });
        at = start + hunk.removed.split_inclusive('\n').count();
    }
    out.extend(lines[at..].iter().copied());
    out
}

/// A text of up to 12 lines of a few words, with or without a final line feed.
fn text() -> impl Strategy<Value = String> {
    (
        proptest::collection::vec(prop_oneof!["a", "b", "c", "\td;", ""], 0..12),
        any::<bool>(),
    )
        .prop_map(|(lines, last_feed)| {
            let mut text = lines.join("\n");
            if last_feed && !text.is_empty() {
                text.push('\n');
            }
            text
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    // No spec statement: P4-04. For any change and any choice of hunks, decided one at a time
    // in either order, the text is the text before with exactly the accepted hunks; rejecting
    // all of them gives it back byte for byte, and accepting all gives the proposed text
    #[test]
    fn any_choice_of_hunks(
        before in text(),
        after in text(),
        choices in proptest::collection::vec(any::<bool>(), 12),
        reverse in any::<bool>(),
    ) {
        let spec = ProjectSpec { defs: before.clone(), ..ProjectSpec::default() };
        let mut workbench = in_memory(&spec);
        let hunks = workbench
            .stage(vec![(path("defs.qc"), after.clone().into_bytes())])
            .unwrap();
        let accepted: Vec<bool> = choices.iter().copied().take(hunks.len()).collect();
        prop_assume!(accepted.len() == hunks.len());
        let mut order: Vec<usize> = (0..hunks.len()).collect();
        if reverse {
            order.reverse();
        }
        for index in order {
            let decision = if accepted[index] { Decision::Accept } else { Decision::Reject };
            workbench.decide(&[hunks[index].id], decision).unwrap();
        }
        prop_assert!(workbench.staged_hunks().is_empty());
        let text = String::from_utf8(workbench.text(&path("defs.qc")).unwrap()).unwrap();
        prop_assert_eq!(&text, &expected(&before, &hunks, &accepted));
        if accepted.iter().all(|accept| !accept) {
            prop_assert_eq!(&text, &before);
            prop_assert!(!workbench.is_unsaved(&path("defs.qc")));
        }
        if accepted.iter().all(|accept| *accept) {
            prop_assert_eq!(&text, &after);
        }
    }
}
