// SPDX-License-Identifier: GPL-3.0-or-later

//! The settings commands: `settings.list`, `settings.get`, `settings.set` and `settings.reset`.

use qs_commands::{BoxError, Command, NoParams, Outcome, RegisterError, Registry, UndoStep};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Scope, Setting, SettingsError, SettingsStore, Source};

/// A context whose settings the commands read and change.
pub trait HasSettings {
    /// The settings.
    fn settings(&mut self) -> &mut SettingsStore;
}

/// A store is its own settings, so a registry over the store alone runs the commands.
impl HasSettings for SettingsStore {
    fn settings(&mut self) -> &mut SettingsStore {
        self
    }
}

/// Registers the settings commands with `registry`.
///
/// # Errors
///
/// The registry already has a command of one of their ids.
pub fn register<C: HasSettings + 'static>(registry: &mut Registry<C>) -> Result<(), RegisterError> {
    registry.register(List)?;
    registry.register(Get)?;
    registry.register(Set)?;
    registry.register(Reset)
}

/// A setting as the commands return it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Entry {
    /// Its key.
    pub key: Setting,
    /// What it does.
    pub summary: &'static str,
    /// Its default.
    pub default: Value,
    /// The value in force.
    pub value: Value,
    /// Where that value comes from.
    pub source: Source,
}

impl Entry {
    fn of(store: &SettingsStore, setting: Setting) -> Self {
        let (value, source) = store.value(setting);
        Self {
            key: setting,
            summary: setting.summary(),
            default: setting.default_value(),
            value,
            source,
        }
    }
}

/// `settings.list`.
struct List;

impl<C: HasSettings + 'static> Command<C> for List {
    const ID: &'static str = "settings.list";
    const SUMMARY: &'static str =
        "Lists every setting with its default, its value and where the value comes from.";
    type Params = NoParams;
    type Output = Vec<Entry>;
    type Error = SettingsError;

    fn run(&self, context: &mut C, _: NoParams) -> Result<Outcome<Vec<Entry>, C>, SettingsError> {
        let store = context.settings();
        let entries = Setting::ALL.iter().map(|&s| Entry::of(store, s)).collect();
        Ok(Outcome::unchanged(entries))
    }
}

/// The parameters of `settings.get`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct GetParams {
    /// The setting's key.
    key: Setting,
}

/// `settings.get`.
struct Get;

impl<C: HasSettings + 'static> Command<C> for Get {
    const ID: &'static str = "settings.get";
    const SUMMARY: &'static str = "Gives one setting's value and where it comes from.";
    type Params = GetParams;
    type Output = Entry;
    type Error = SettingsError;

    fn run(&self, context: &mut C, params: GetParams) -> Result<Outcome<Entry, C>, SettingsError> {
        Ok(Outcome::unchanged(Entry::of(
            context.settings(),
            params.key,
        )))
    }
}

/// The parameters of `settings.set`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SetParams {
    /// The setting's key.
    key: Setting,
    /// Its new value, of the setting's type.
    value: Value,
    /// The file to set it in: the user's, by default, or the open project's.
    #[serde(default = "user")]
    scope: Scope,
}

/// The parameters of `settings.reset`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ResetParams {
    /// The setting's key.
    key: Setting,
    /// The file to remove it from: the user's, by default, or the open project's.
    #[serde(default = "user")]
    scope: Scope,
}

const fn user() -> Scope {
    Scope::User
}

/// `settings.set`.
struct Set;

impl<C: HasSettings + 'static> Command<C> for Set {
    const ID: &'static str = "settings.set";
    const SUMMARY: &'static str =
        "Sets a setting in the user's or the project's settings file and writes the file.";
    type Params = SetParams;
    type Output = Entry;
    type Error = SettingsError;

    fn run(&self, context: &mut C, params: SetParams) -> Result<Outcome<Entry, C>, SettingsError> {
        change(context, params.scope, params.key, Some(params.value))
    }
}

/// `settings.reset`.
struct Reset;

impl<C: HasSettings + 'static> Command<C> for Reset {
    const ID: &'static str = "settings.reset";
    const SUMMARY: &'static str =
        "Removes a setting from the user's or the project's settings file and writes the file.";
    type Params = ResetParams;
    type Output = Entry;
    type Error = SettingsError;

    fn run(
        &self,
        context: &mut C,
        params: ResetParams,
    ) -> Result<Outcome<Entry, C>, SettingsError> {
        change(context, params.scope, params.key, None)
    }
}

/// Puts `after` in the file of `scope`, undoably unless it was already so.
fn change<C: HasSettings + 'static>(
    context: &mut C,
    scope: Scope,
    setting: Setting,
    after: Option<Value>,
) -> Result<Outcome<Entry, C>, SettingsError> {
    let store = context.settings();
    let before = store.file(scope).and_then(|file| file.get(setting));
    store.put(scope, setting, after)?;
    let now = store.file(scope).and_then(|file| file.get(setting));
    let entry = Entry::of(store, setting);
    if now == before {
        return Ok(Outcome::unchanged(entry));
    }
    let step = Change {
        scope,
        setting,
        before,
        after: now,
    };
    Ok(Outcome::undoable(entry, step))
}

/// Undoes a change to one setting in one file, and makes it again.
struct Change {
    scope: Scope,
    setting: Setting,
    before: Option<Value>,
    after: Option<Value>,
}

impl<C: HasSettings + 'static> UndoStep<C> for Change {
    fn undo(&mut self, context: &mut C) -> Result<(), BoxError> {
        Ok(context
            .settings()
            .put(self.scope, self.setting, self.before.clone())?)
    }

    fn redo(&mut self, context: &mut C) -> Result<(), BoxError> {
        Ok(context
            .settings()
            .put(self.scope, self.setting, self.after.clone())?)
    }
}
