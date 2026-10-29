// SPDX-License-Identifier: GPL-3.0-or-later

//! Feeds arbitrary bytes to the settings reader as a user settings file (QA-10, P2-148): through
//! `SettingsFile::load` and `SettingsStore::open`, then one `settings.set` or `settings.reset`
//! the first byte picks, run on a session over the store. Fails on a panic; on the store and
//! the reader disagreeing about the file; on a refused file being rewritten; and on an accepted
//! change that does not read back from the file as the store holds it.

#![no_main]

use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use qs_commands::{Registry, Session};
use qs_settings::{Scope, SettingsFile, SettingsStore, register};
use serde_json::{Value, json};

/// The settings file every input is written to, in a directory of this process's own.
fn path() -> &'static PathBuf {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("qs-settings-fuzz-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir.join("settings.toml")
    })
}

/// The command and parameters `selector` picks.
fn change(selector: u8) -> (&'static str, Value) {
    match selector % 6 {
        0 => (
            "settings.set",
            json!({"key": "appearance.theme", "value": "light"}),
        ),
        1 => (
            "settings.set",
            json!({"key": "appearance.theme", "value": "dark"}),
        ),
        2 => (
            "settings.set",
            json!({"key": "log.filter", "value": "debug"}),
        ),
        3 => (
            "settings.set",
            json!({"key": "log.filter", "value": "a'b\"c\n"}),
        ),
        4 => ("settings.reset", json!({"key": "appearance.theme"})),
        _ => ("settings.reset", json!({"key": "log.filter"})),
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&selector, text)) = data.split_first() else {
        return;
    };
    let path = path();
    fs::write(path, text).unwrap();
    let loaded = SettingsFile::load(path);
    let Ok(store) = SettingsStore::open(path.clone(), None) else {
        assert!(loaded.is_err());
        return;
    };
    assert_eq!(Some(loaded.as_ref().unwrap()), store.file(Scope::User));
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut session = Session::new(store, registry);
    let (id, params) = change(selector);
    if session.execute(id, params).is_err() {
        assert_eq!(fs::read(path).unwrap(), text);
        assert_eq!(
            Some(loaded.as_ref().unwrap()),
            session.context().file(Scope::User)
        );
        return;
    }
    let reread = SettingsFile::load(path).unwrap();
    assert_eq!(Some(&reread), session.context().file(Scope::User));
    if session.undo().is_ok() {
        assert_eq!(&SettingsFile::load(path).unwrap(), loaded.as_ref().unwrap());
    }
});
