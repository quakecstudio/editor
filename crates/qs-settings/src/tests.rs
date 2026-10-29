// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use qs_commands::{CommandError, Registry, Session};
use serde_json::{Value, json};

use crate::{
    HasSettings, Scope, Setting, SettingsError, SettingsFile, SettingsStore, Source, Theme,
    project_settings_path, reference, register, user_settings_path,
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A fresh directory under the system's temporary folder, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("qs-settings-{}-{name}-{n}", std::process::id()));
        if path.exists() {
            fs::remove_dir_all(&path).unwrap();
        }
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

/// What a test's settings start as: the text of each file, `None` for no file, and whether a
/// project is open.
struct StoreSpec {
    user: Option<&'static str>,
    project: Option<&'static str>,
    open_project: bool,
}

impl Default for StoreSpec {
    fn default() -> Self {
        Self {
            user: None,
            project: None,
            open_project: true,
        }
    }
}

/// The files of `spec` in `dir`, and the store reading them.
fn store(dir: &TempDir, spec: &StoreSpec) -> (PathBuf, PathBuf, SettingsStore) {
    let user = user_settings_path(&dir.path().join("config"));
    let project = project_settings_path(&dir.path().join("project"));
    for (path, text) in [(&user, spec.user), (&project, spec.project)] {
        if let Some(text) = text {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
    }
    let project_path = spec.open_project.then(|| project.clone());
    let store = SettingsStore::open(user.clone(), project_path).unwrap();
    (user, project, store)
}

struct Workspace(SettingsStore);

impl HasSettings for Workspace {
    fn settings(&mut self) -> &mut SettingsStore {
        &mut self.0
    }
}

fn session(store: SettingsStore) -> Session<Workspace> {
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    Session::new(Workspace(store), registry)
}

// QA-10: a setting's value is the project's, else the user's, else the default.
#[test]
fn the_project_wins_over_the_user_and_the_user_over_the_default() {
    let dir = TempDir::new("precedence");
    let (_, _, none) = store(&dir, &StoreSpec::default());
    assert_eq!(none.value(Setting::Theme), (json!("dark"), Source::Default));
    assert_eq!(none.theme(), Theme::Dark);
    assert_eq!(none.log_filter(), "info");
    let dir = TempDir::new("precedence-files");
    let spec = StoreSpec {
        user: Some("appearance.theme = \"light\"\n[log]\nfilter = \"debug\"\n"),
        project: Some("[appearance]\ntheme = \"dark\"\n"),
        ..StoreSpec::default()
    };
    let (_, _, both) = store(&dir, &spec);
    assert_eq!(both.value(Setting::Theme), (json!("dark"), Source::Project));
    assert_eq!(
        both.value(Setting::LogFilter),
        (json!("debug"), Source::User)
    );
    assert_eq!(both.log_filter(), "debug");
    // With no project open the project's file is not read.
    let closed = StoreSpec {
        open_project: false,
        ..spec
    };
    let dir = TempDir::new("precedence-closed");
    let (_, _, user_only) = store(&dir, &closed);
    assert_eq!(user_only.theme(), Theme::Light);
    assert!(user_only.file(Scope::Project).is_none());
}

// QA-10: a file with an unknown key or a value of the wrong type is refused, not ignored.
#[test]
fn a_bad_settings_file_is_an_error() {
    for (text, name) in [
        ("appearance.theme = \"blue\"\n", "value"),
        ("appearance.colour = \"dark\"\n", "key"),
        ("theme = \"dark\"\n", "top"),
        ("[appearance\n", "syntax"),
    ] {
        let dir = TempDir::new(name);
        let user = user_settings_path(dir.path());
        fs::write(&user, text).unwrap();
        let error = SettingsStore::open(user.clone(), None).unwrap_err();
        assert!(
            matches!(error, SettingsError::Parse { ref path, .. } if *path == user),
            "{text}"
        );
        assert!(
            error.to_string().starts_with(&user.display().to_string()),
            "{error}"
        );
    }
    // A settings path that is a directory cannot be read.
    let dir = TempDir::new("directory");
    let error = SettingsStore::open(dir.path().to_path_buf(), None).unwrap_err();
    assert!(matches!(error, SettingsError::Read { .. }), "{error:?}");
    assert!(std::error::Error::source(&error).is_some());
}

// QA-10: a file round-trips, writing only what it sets, and one setting nothing is empty.
#[test]
fn a_settings_file_writes_only_what_it_sets() {
    let dir = TempDir::new("save");
    let path = project_settings_path(dir.path());
    let mut file = SettingsFile::default();
    file.save(&path).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "");
    file.put(Setting::Theme, Some(json!("light"))).unwrap();
    file.save(&path).unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "[appearance]\ntheme = \"light\"\n"
    );
    assert_eq!(SettingsFile::load(&path).unwrap(), file);
    file.put(Setting::Theme, None).unwrap();
    file.put(Setting::LogFilter, Some(json!("warn"))).unwrap();
    assert_eq!(file.get(Setting::Theme), None);
    assert_eq!(file.get(Setting::LogFilter), Some(json!("warn")));
    // A value of the wrong type changes nothing.
    let before = file.clone();
    let error = file.put(Setting::LogFilter, Some(json!(3))).unwrap_err();
    assert!(matches!(
        error,
        SettingsError::Value {
            setting: Setting::LogFilter,
            ..
        }
    ));
    assert!(
        error
            .to_string()
            .starts_with("invalid value for log.filter"),
        "{error}"
    );
    assert_eq!(file, before);
    // A file that cannot be written: its directory is a file.
    let blocked = dir.path().join("blocked");
    fs::write(&blocked, "").unwrap();
    let error = file.save(&blocked.join("settings.toml")).unwrap_err();
    assert!(matches!(error, SettingsError::Write { .. }), "{error:?}");
}

// QA-10: every setting is changeable by command, in the user's or the project's file, and the
// change is written, undone and redone.
#[test]
fn settings_commands_change_the_files_and_undo() {
    let dir = TempDir::new("commands");
    let (user, project, store) = store(&dir, &StoreSpec::default());
    let mut s = session(store);
    let set = s
        .execute(
            "settings.set",
            json!({"key": "appearance.theme", "value": "light"}),
        )
        .unwrap();
    assert_eq!(set["value"], "light");
    assert_eq!(set["source"], "user");
    assert_eq!(
        fs::read_to_string(&user).unwrap(),
        "[appearance]\ntheme = \"light\"\n"
    );
    s.execute(
        "settings.set",
        json!({"key": "appearance.theme", "value": "dark", "scope": "project"}),
    )
    .unwrap();
    let got = s
        .execute("settings.get", json!({"key": "appearance.theme"}))
        .unwrap();
    assert_eq!(
        (&got["value"], &got["source"]),
        (&json!("dark"), &json!("project"))
    );
    assert!(project.exists());
    // Undo takes the project's value back, then the user's; redo makes them again.
    s.undo().unwrap();
    assert_eq!(
        s.context().0.value(Setting::Theme),
        (json!("light"), Source::User)
    );
    s.undo().unwrap();
    assert_eq!(
        s.context().0.value(Setting::Theme),
        (json!("dark"), Source::Default)
    );
    assert_eq!(fs::read_to_string(&user).unwrap(), "");
    s.redo().unwrap();
    assert_eq!(
        s.context().0.value(Setting::Theme),
        (json!("light"), Source::User)
    );
    // Reset removes it; resetting what is not set changes nothing and adds no undo step.
    let depth = s.undo_depth();
    s.execute("settings.reset", json!({"key": "appearance.theme"}))
        .unwrap();
    assert_eq!(s.undo_depth(), depth + 1);
    s.execute("settings.reset", json!({"key": "appearance.theme"}))
        .unwrap();
    assert_eq!(s.undo_depth(), depth + 1);
    s.undo().unwrap();
    assert_eq!(s.context().0.theme(), Theme::Light);
    // Setting a value it already has changes nothing either.
    s.execute(
        "settings.set",
        json!({"key": "appearance.theme", "value": "light"}),
    )
    .unwrap();
    assert_eq!(s.undo_depth(), depth);
    // The list has every setting.
    let list = s.execute("settings.list", json!({})).unwrap();
    let keys: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, ["appearance.theme", "log.filter"]);
    assert_eq!(list[1]["default"], "info");
}

// QA-10: a bad value, an unknown key and the project's file with no project open are errors
// that change nothing.
#[test]
fn settings_commands_refuse_bad_changes() {
    let dir = TempDir::new("refuse");
    let spec = StoreSpec {
        open_project: false,
        ..StoreSpec::default()
    };
    let (user, _, store) = store(&dir, &spec);
    let mut s = session(store);
    let failed = |result: Result<Value, CommandError>| match result {
        Err(CommandError::Failed { error, .. }) => error.to_string(),
        other => panic!("{other:?}"),
    };
    let bad = s.execute(
        "settings.set",
        json!({"key": "appearance.theme", "value": 1}),
    );
    assert!(failed(bad).starts_with("invalid value for appearance.theme"));
    let no_project = s.execute(
        "settings.set",
        json!({"key": "log.filter", "value": "warn", "scope": "project"}),
    );
    assert_eq!(failed(no_project), "no project is open");
    let unknown = s.execute("settings.get", json!({"key": "appearance.colour"}));
    assert!(
        matches!(unknown, Err(CommandError::InvalidParams { .. })),
        "{unknown:?}"
    );
    assert!(!user.exists());
    assert_eq!(s.undo_depth(), 0);
}

// QA-10: every setting is documented: docs/settings.md is the generated reference.
#[test]
fn the_settings_reference_is_up_to_date() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/settings.md");
    let written = fs::read_to_string(&path).unwrap();
    assert_eq!(
        written,
        reference(),
        "docs/settings.md is out of date; write qs_settings::reference() to it"
    );
    for setting in Setting::ALL {
        assert!(written.contains(setting.key()));
    }
}
