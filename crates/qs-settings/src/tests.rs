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

use proptest::prelude::*;
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
    let (_, path, mut store) = store(&dir, &StoreSpec::default());
    let before = store
        .put(Scope::Project, Setting::Theme, Some(json!("light")))
        .unwrap();
    assert_eq!(before, None);
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "[appearance]\ntheme = \"light\"\n"
    );
    assert_eq!(
        Some(&SettingsFile::load(&path).unwrap()),
        store.file(Scope::Project)
    );
    let before = store.put(Scope::Project, Setting::Theme, None).unwrap();
    assert_eq!(before, Some(json!("light")));
    assert_eq!(fs::read_to_string(&path).unwrap(), "");
    let mut file = SettingsFile::default();
    file.put(Setting::Theme, Some(json!("light"))).unwrap();
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
}

// QA-10: a file whose directory cannot be written to is an error that changes nothing.
#[cfg(unix)]
#[test]
fn a_settings_file_that_cannot_be_written_is_an_error() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new("locked");
    let locked = dir.path().join("locked");
    fs::create_dir(&locked).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
    let path = locked.join("settings.toml");
    let mut store = SettingsStore::open(path.clone(), None).unwrap();
    let result = store.put(Scope::User, Setting::Theme, Some(json!("light")));
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    let error = result.unwrap_err();
    assert!(
        matches!(error, SettingsError::Write { path: ref p, .. } if *p == path),
        "{error:?}"
    );
    assert!(error.to_string().starts_with("cannot write "), "{error}");
    assert_eq!(store.theme(), Theme::Dark);
    assert!(!path.exists());
}

// QA-10 (P2-148): a change made to a file by hand after the store opened is kept when a
// command changes another setting in that file, and the store then reads it.
#[test]
fn a_hand_edit_after_the_store_opened_is_kept() {
    let dir = TempDir::new("hand-edit");
    let spec = StoreSpec {
        user: Some("# my comment\n[log]\nfilter = \"warn\"\n"),
        ..StoreSpec::default()
    };
    let (user, _, store) = store(&dir, &spec);
    let mut s = session(store);
    fs::write(&user, "# my comment\n[log]\nfilter = \"debug\"\n").unwrap();
    s.execute(
        "settings.set",
        json!({"key": "appearance.theme", "value": "light"}),
    )
    .unwrap();
    let both = "# my comment\n[log]\nfilter = \"debug\"\n\n[appearance]\ntheme = \"light\"\n";
    assert_eq!(fs::read_to_string(&user).unwrap(), both);
    assert_eq!(s.context().0.log_filter(), "debug");
    // Undo and redo change the theme alone.
    s.undo().unwrap();
    assert_eq!(
        fs::read_to_string(&user).unwrap(),
        "# my comment\n[log]\nfilter = \"debug\"\n"
    );
    s.redo().unwrap();
    assert_eq!(fs::read_to_string(&user).unwrap(), both);
    // A set to the value the file has by hand, not the one read at open, changes nothing.
    s.execute(
        "settings.set",
        json!({"key": "log.filter", "value": "debug"}),
    )
    .unwrap();
    assert_eq!(s.undo_depth(), 1);
    assert_eq!(fs::read_to_string(&user).unwrap(), both);
    // An undo puts back the value the file had when the command ran.
    fs::write(&user, "[log]\nfilter = \"trace\"\n").unwrap();
    s.execute(
        "settings.set",
        json!({"key": "log.filter", "value": "error"}),
    )
    .unwrap();
    s.undo().unwrap();
    assert_eq!(
        fs::read_to_string(&user).unwrap(),
        "[log]\nfilter = \"trace\"\n"
    );
}

/// A settings file written by hand, with a comment in each place one can go.
const COMMENTED: &str = "\
# Settings by hand.
[appearance]
# The theme I like.
theme = \"dark\" # for now

# Logging.
[log]
filter = \"warn\" # quiet
# The end.
";

// QA-10 (P2-148): comments survive settings.set, settings.reset, and an undo and redo of each;
// a set to the value the file has leaves the file as it was.
#[test]
fn comments_survive_set_reset_undo_and_redo() {
    let dir = TempDir::new("comments");
    let spec = StoreSpec {
        user: Some(COMMENTED),
        ..StoreSpec::default()
    };
    let (user, _, store) = store(&dir, &spec);
    let mut s = session(store);
    let text = || fs::read_to_string(&user).unwrap();
    s.execute(
        "settings.set",
        json!({"key": "log.filter", "value": "warn"}),
    )
    .unwrap();
    assert_eq!(text(), COMMENTED);
    let set = COMMENTED.replace("\"warn\" # quiet", "\"debug\" # quiet");
    s.execute(
        "settings.set",
        json!({"key": "log.filter", "value": "debug"}),
    )
    .unwrap();
    assert_eq!(text(), set);
    s.undo().unwrap();
    assert_eq!(text(), COMMENTED);
    s.redo().unwrap();
    assert_eq!(text(), set);
    // Reset removes the setting's own line and the comment above it, and keeps the table,
    // whose header has a comment.
    let theme_lines = "# The theme I like.\ntheme = \"dark\" # for now\n";
    let reset = set.replace(theme_lines, "");
    s.execute("settings.reset", json!({"key": "appearance.theme"}))
        .unwrap();
    assert_eq!(text(), reset);
    s.undo().unwrap();
    assert_eq!(text(), set.replace(theme_lines, "theme = \"dark\"\n"));
    s.redo().unwrap();
    assert_eq!(text(), reset);
    assert_eq!(
        Some(&SettingsFile::load(&user).unwrap()),
        s.context().0.file(Scope::User)
    );
}

// QA-10 (P2-148): a setting written as a dotted key or an inline table is changed where it is;
// a table left empty with no comment on its header is removed.
#[test]
fn a_setting_is_changed_where_the_file_writes_it() {
    let dir = TempDir::new("forms");
    let spec = StoreSpec {
        user: Some("appearance = { theme = \"dark\" }\nlog.filter = \"warn\" # quiet\n"),
        ..StoreSpec::default()
    };
    let (user, _, mut store) = store(&dir, &spec);
    let text = || fs::read_to_string(&user).unwrap();
    store
        .put(Scope::User, Setting::Theme, Some(json!("light")))
        .unwrap();
    store
        .put(Scope::User, Setting::LogFilter, Some(json!("debug")))
        .unwrap();
    assert_eq!(
        text(),
        "appearance = { theme = \"light\" }\nlog.filter = \"debug\" # quiet\n"
    );
    store.put(Scope::User, Setting::LogFilter, None).unwrap();
    store.put(Scope::User, Setting::Theme, None).unwrap();
    assert_eq!(text(), "appearance = {}\n");
    fs::write(
        &user,
        "[log]\nfilter = \"warn\"\n\n[appearance]\ntheme = \"dark\"\n",
    )
    .unwrap();
    store.put(Scope::User, Setting::LogFilter, None).unwrap();
    // The blank line that separated the removed table from the next goes with it.
    assert_eq!(text(), "[appearance]\ntheme = \"dark\"\n");
    // A file that starts with a blank line keeps it.
    fs::write(&user, "\n[log]\nfilter = \"warn\"\n").unwrap();
    store
        .put(Scope::User, Setting::Theme, Some(json!("light")))
        .unwrap();
    assert_eq!(
        text(),
        "\n[log]\nfilter = \"warn\"\n\n[appearance]\ntheme = \"light\"\n"
    );
}

// QA-10 (P2-148): a new table goes after the comments that end a file, so a file of comments
// keeps them at its top.
#[test]
fn a_new_table_goes_after_the_comments_that_end_the_file() {
    let dir = TempDir::new("trailing");
    let spec = StoreSpec {
        user: Some("# My settings.\n"),
        ..StoreSpec::default()
    };
    let (user, _, mut store) = store(&dir, &spec);
    store
        .put(Scope::User, Setting::Theme, Some(json!("light")))
        .unwrap();
    assert_eq!(
        fs::read_to_string(&user).unwrap(),
        "# My settings.\n[appearance]\ntheme = \"light\"\n"
    );
    store
        .put(Scope::User, Setting::LogFilter, Some(json!("warn")))
        .unwrap();
    assert_eq!(
        fs::read_to_string(&user).unwrap(),
        "# My settings.\n[appearance]\ntheme = \"light\"\n\n[log]\nfilter = \"warn\"\n"
    );
}

// QA-10 (P2-148): a comment on a last line with no line ending stays a comment: the new
// table starts on the next line (found by the qs_settings fuzz target).
#[test]
fn a_new_table_after_an_unterminated_comment_starts_its_own_line() {
    let dir = TempDir::new("unterminated");
    let spec = StoreSpec {
        user: Some("#"),
        ..StoreSpec::default()
    };
    let (user, _, mut store) = store(&dir, &spec);
    store
        .put(Scope::User, Setting::Theme, Some(json!("light")))
        .unwrap();
    assert_eq!(
        fs::read_to_string(&user).unwrap(),
        "#\n[appearance]\ntheme = \"light\"\n"
    );
    assert_eq!(
        SettingsFile::load(&user).unwrap().get(Setting::Theme),
        Some(json!("light"))
    );
}

// QA-10 (P2-148): a file with CRLF line endings keeps them, on every line.
#[test]
fn a_crlf_file_keeps_its_line_endings() {
    let dir = TempDir::new("crlf");
    let spec = StoreSpec {
        user: Some("# Mine.\r\n[log]\r\nfilter = \"warn\"\r\n"),
        ..StoreSpec::default()
    };
    let (user, _, mut store) = store(&dir, &spec);
    store
        .put(Scope::User, Setting::Theme, Some(json!("light")))
        .unwrap();
    assert_eq!(
        fs::read_to_string(&user).unwrap(),
        "# Mine.\r\n[log]\r\nfilter = \"warn\"\r\n\r\n[appearance]\r\ntheme = \"light\"\r\n"
    );
}

// QA-10 (P2-148): a value with a line break, set in a CRLF file, reads back as it was set: it
// is written escaped on one line (found by the qs_settings fuzz target).
#[test]
fn a_line_break_in_a_value_survives_a_crlf_file() {
    let dir = TempDir::new("crlf-value");
    let spec = StoreSpec {
        user: Some("\r\n"),
        ..StoreSpec::default()
    };
    let (user, _, mut store) = store(&dir, &spec);
    store
        .put(Scope::User, Setting::LogFilter, Some(json!("a'b\"c\n")))
        .unwrap();
    assert_eq!(
        SettingsFile::load(&user).unwrap().get(Setting::LogFilter),
        Some(json!("a'b\"c\n"))
    );
    assert!(!fs::read_to_string(&user).unwrap().contains("c\r\n"));
}

// QA-10: the table and name a setting is written under spell its key.
#[test]
fn a_setting_is_written_under_its_key() {
    for setting in Setting::ALL {
        let (table, name) = setting.place();
        assert_eq!(format!("{table}.{name}"), setting.key());
    }
}

// QA-10 (P2-148): a file that no longer parses, or that writes a table another way, is refused
// with `Parse` and left as it is, and the store keeps what it held.
#[test]
fn a_file_that_no_longer_parses_is_refused_not_overwritten() {
    let dir = TempDir::new("broken");
    let spec = StoreSpec {
        user: Some("[log]\nfilter = \"warn\"\n"),
        ..StoreSpec::default()
    };
    let (user, _, mut store) = store(&dir, &spec);
    for (text, value) in [
        ("[log\nfilter = \"debug\"\n", Some(json!("light"))),
        ("log.colour = 1\n", Some(json!("light"))),
        ("appearance = [\"dark\"]\n", Some(json!("light"))),
        ("appearance = [\"dark\"]\n", None),
    ] {
        fs::write(&user, text).unwrap();
        let error = store.put(Scope::User, Setting::Theme, value).unwrap_err();
        assert!(
            matches!(error, SettingsError::Parse { ref path, .. } if *path == user),
            "{text:?}: {error:?}"
        );
        assert_eq!(fs::read_to_string(&user).unwrap(), text);
        assert_eq!(store.log_filter(), "warn");
        assert_eq!(store.theme(), Theme::Dark);
    }
    let error = store
        .put(Scope::User, Setting::Theme, Some(json!("light")))
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        format!(
            "{}: `appearance` is not written as a table, so it cannot be changed",
            user.display()
        )
    );
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

// Readiness review (P2-R5, TEST-9): adversarial cases.

// QA-10: `null` is not a value of either setting; unsetting is `settings.reset`.
#[test]
fn settings_set_refuses_null_and_values_of_the_wrong_type() {
    let dir = TempDir::new("null");
    let (user, _, store) = store(&dir, &StoreSpec::default());
    let mut s = session(store);
    for (key, value) in [
        ("appearance.theme", json!(null)),
        ("appearance.theme", json!("Dark")),
        ("appearance.theme", json!(["dark"])),
        ("log.filter", json!(null)),
        ("log.filter", json!(3)),
        ("log.filter", json!({"info": true})),
    ] {
        let result = s.execute("settings.set", json!({"key": key, "value": value}));
        assert!(
            matches!(&result, Err(CommandError::Failed { error, .. })
                if error.to_string().starts_with(&format!("invalid value for {key}"))),
            "{key} {value}: {result:?}"
        );
    }
    assert!(!user.exists());
    assert_eq!(s.undo_depth(), 0);
}

// QA-10: the schema refuses a scope that is neither file, a missing value and unknown fields.
#[test]
fn settings_commands_refuse_malformed_arguments() {
    let dir = TempDir::new("args");
    let (user, _, store) = store(&dir, &StoreSpec::default());
    let mut s = session(store);
    for (id, params) in [
        (
            "settings.set",
            json!({"key": "log.filter", "value": "warn", "scope": "workspace"}),
        ),
        ("settings.set", json!({"key": "log.filter"})),
        (
            "settings.set",
            json!({"key": "log.filter", "value": "warn", "extra": 1}),
        ),
        (
            "settings.reset",
            json!({"key": "log.filter", "scope": "User"}),
        ),
        ("settings.get", json!({})),
        ("settings.list", json!({"key": "log.filter"})),
    ] {
        let result = s.execute(id, params.clone());
        assert!(
            matches!(result, Err(CommandError::InvalidParams { .. })),
            "{id} {params}: {result:?}"
        );
    }
    assert!(!user.exists());
}

// QA-10: a file that is not UTF-8, a TOML syntax error and a table where a value belongs are
// refused when the store opens, the project's as well as the user's.
#[test]
fn a_malformed_file_is_refused_when_the_store_opens() {
    let dir = TempDir::new("malformed");
    let path = dir.path().join("settings.toml");
    fs::write(&path, [0xff, 0xfe, b'\n']).unwrap();
    assert!(matches!(
        SettingsFile::load(&path),
        Err(SettingsError::Read { .. })
    ));
    for text in ["appearance.theme = ", "[appearance.theme]\n", "log = 1\n"] {
        fs::write(&path, text).unwrap();
        assert!(
            matches!(SettingsFile::load(&path), Err(SettingsError::Parse { .. })),
            "{text:?}"
        );
    }
    let user = dir.path().join("user.toml");
    assert!(matches!(
        SettingsStore::open(user, Some(path)),
        Err(SettingsError::Parse { .. })
    ));
}

// QA-10: an undo whose file cannot be written fails, changes nothing in memory, and stays in
// line to be tried again once the file can be written.
#[test]
fn an_undo_that_cannot_write_its_file_stays_in_line() {
    let dir = TempDir::new("unwritable");
    let (_, project, store) = store(&dir, &StoreSpec::default());
    let mut s = session(store);
    s.execute(
        "settings.set",
        json!({"key": "log.filter", "value": "warn", "scope": "project"}),
    )
    .unwrap();
    // Replace the project's settings directory with a file, so the file cannot be written.
    let settings_dir = project.parent().unwrap().to_path_buf();
    fs::remove_dir_all(&settings_dir).unwrap();
    fs::write(&settings_dir, "").unwrap();
    let error = s.undo().unwrap_err();
    assert!(
        matches!(error, CommandError::UndoFailed { .. }),
        "{error:?}"
    );
    assert_eq!(s.undo_depth(), 1);
    assert_eq!(s.context().0.log_filter(), "warn");
    fs::remove_file(&settings_dir).unwrap();
    s.undo().unwrap();
    assert_eq!(
        s.context().0.value(Setting::LogFilter),
        (json!("info"), Source::Default)
    );
    // The file went with its directory, so it already sets nothing and is not written.
    assert!(!project.exists());
}

// QA-10: a log filter is written and read back exactly, whatever characters it holds.
#[test]
fn a_log_filter_with_awkward_characters_round_trips() {
    let dir = TempDir::new("awkward");
    let (user, _, store) = store(&dir, &StoreSpec::default());
    let mut s = session(store);
    let filter = "info,qc_sema=debug,\"quoted\"\\back\ttab\u{1}\u{e9}";
    s.execute(
        "settings.set",
        json!({"key": "log.filter", "value": filter}),
    )
    .unwrap();
    let reread = SettingsStore::open(user, None).unwrap();
    assert_eq!(reread.log_filter(), filter);
}

fn theme() -> impl Strategy<Value = Option<Theme>> {
    prop_oneof![
        Just(None),
        Just(Some(Theme::Dark)),
        Just(Some(Theme::Light))
    ]
}

proptest! {
    // TEST-4, QA-10: a settings file is written and read back unchanged, whatever it sets.
    #[test]
    fn a_settings_file_round_trips(theme in theme(), filter in proptest::option::of(".*")) {
        let dir = TempDir::new("round-trip");
        let (path, _, mut store) = store(&dir, &StoreSpec::default());
        let file = SettingsFile {
            appearance: crate::Appearance { theme },
            log: crate::Log { filter: filter.clone() },
        };
        store
            .put(Scope::User, Setting::Theme, theme.map(|t| serde_json::to_value(t).unwrap()))
            .unwrap();
        store.put(Scope::User, Setting::LogFilter, filter.map(Value::from)).unwrap();
        prop_assert_eq!(store.file(Scope::User), Some(&file));
        prop_assert_eq!(SettingsFile::load(&path).unwrap(), file);
    }

    // TEST-4, QA-10 (P2-148): whatever changes a commented file is given, each comment not on
    // a removed setting's lines stays, and the file reads back as the store holds it.
    #[test]
    fn changes_keep_the_comments(
        changes in proptest::collection::vec(
            (proptest::bool::ANY, theme(), proptest::option::of(".*")),
            0..8,
        ),
    ) {
        let dir = TempDir::new("keep-comments");
        let spec = StoreSpec { user: Some(COMMENTED), ..StoreSpec::default() };
        let (path, _, mut store) = store(&dir, &spec);
        for (theme_change, theme, filter) in changes {
            if theme_change {
                let value = theme.map(|t| serde_json::to_value(t).unwrap());
                store.put(Scope::User, Setting::Theme, value).unwrap();
            } else {
                store.put(Scope::User, Setting::LogFilter, filter.map(Value::from)).unwrap();
            }
            let text = fs::read_to_string(&path).unwrap();
            for comment in ["# Settings by hand.\n", "# Logging.\n", "# The end.\n"] {
                prop_assert!(text.contains(comment), "{comment:?} gone from {text:?}");
            }
            prop_assert_eq!(Some(&SettingsFile::load(&path).unwrap()), store.file(Scope::User));
        }
    }

    // TEST-4, QA-10: the value a file is given is the value it then holds, for every setting.
    #[test]
    fn a_put_value_is_the_value_got(theme in theme(), filter in proptest::option::of(".*")) {
        let mut file = SettingsFile::default();
        let theme = theme.map(|t| serde_json::to_value(t).unwrap());
        let filter = filter.map(Value::from);
        file.put(Setting::Theme, theme.clone()).unwrap();
        file.put(Setting::LogFilter, filter.clone()).unwrap();
        prop_assert_eq!(file.get(Setting::Theme), theme);
        prop_assert_eq!(file.get(Setting::LogFilter), filter);
    }
}

// docs/testing.md, L5 note (schema test): each settings command is listed with an object
// schema whose `key` names exactly the settings and whose `scope` names exactly the two files.
#[test]
fn every_settings_command_has_its_schema() {
    let dir = TempDir::new("schema");
    let (_, _, store) = store(&dir, &StoreSpec::default());
    let s = session(store);
    let consts = |schema: &Value, def: &str| -> Vec<String> {
        schema["$defs"][def]["oneOf"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["const"].as_str().unwrap().to_owned())
            .collect()
    };
    let keys: Vec<String> = Setting::ALL.iter().map(|s| s.key().to_owned()).collect();
    for (id, required) in [
        ("settings.list", json!(null)),
        ("settings.get", json!(["key"])),
        ("settings.set", json!(["key", "value"])),
        ("settings.reset", json!(["key"])),
    ] {
        let info = s.registry().get(id).unwrap();
        let schema = info.params();
        assert_eq!(schema["type"], "object", "{id}");
        assert_eq!(schema["additionalProperties"], json!(false), "{id}");
        assert_eq!(schema["required"], required, "{id}");
        if id != "settings.list" {
            assert_eq!(
                schema["properties"]["key"]["$ref"], "#/$defs/Setting",
                "{id}"
            );
            assert_eq!(consts(schema, "Setting"), keys, "{id}");
        }
        if id == "settings.set" || id == "settings.reset" {
            assert_eq!(schema["properties"]["scope"]["default"], "user", "{id}");
            assert_eq!(consts(schema, "Scope"), ["user", "project"], "{id}");
        }
    }
}

// docs/testing.md, L5 note (undo test): the reading commands leave no undo step and write no
// file, and keep the redo history.
#[test]
fn reading_settings_leaves_the_history_and_the_files_alone() {
    let dir = TempDir::new("read");
    let (user, project, store) = store(&dir, &StoreSpec::default());
    let mut s = session(store);
    s.execute(
        "settings.set",
        json!({"key": "log.filter", "value": "warn"}),
    )
    .unwrap();
    s.undo().unwrap();
    fs::remove_file(&user).unwrap();
    s.execute("settings.list", json!({})).unwrap();
    s.execute("settings.get", json!({"key": "log.filter"}))
        .unwrap();
    assert_eq!((s.undo_depth(), s.redo_depth()), (0, 1));
    assert!(!user.exists() && !project.exists());
}
