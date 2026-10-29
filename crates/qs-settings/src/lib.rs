// SPDX-License-Identifier: GPL-3.0-or-later

//! QuakeC Studio's settings (QA-10): one file for the user and one for each project, each a
//! TOML table of the settings it sets, and the commands that read and change them.
//!
//! The model is Zed's and VS Code's: a setting has a default, the user's file may set it, and a
//! project's file may set it again for that project, which wins. The user's file is
//! `settings.toml` in the app's config directory ([`user_settings_path`], from `qs-paths`); a
//! project's is `.quakec-studio/settings.toml` in its directory ([`project_settings_path`]).
//! Every setting is a [`Setting`], documented in `docs/settings.md`, which [`reference`]
//! writes, and changeable by the commands of [`register`].

mod commands;
mod error;

#[cfg(test)]
mod tests;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use toml_edit::{Decor, DocumentMut, Item, TableLike};

pub use commands::{Entry, HasSettings, register};
pub use error::SettingsError;

/// The user's settings file in the app's config directory, `qs_paths::AppDirs::config`.
#[must_use]
pub fn user_settings_path(config_dir: &Path) -> PathBuf {
    config_dir.join("settings.toml")
}

/// A project's settings file in the project's directory.
#[must_use]
pub fn project_settings_path(project_dir: &Path) -> PathBuf {
    project_dir.join(".quakec-studio").join("settings.toml")
}

/// Which file a setting is read from or written to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// The user's settings, which hold in every project.
    User,
    /// The open project's settings, which win over the user's.
    Project,
}

/// Where a setting's value comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// Neither file sets it.
    Default,
    /// The user's file.
    User,
    /// The project's file.
    Project,
}

/// The colour theme, one of the two palettes of `docs/design/tokens.toml`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// `[color.dark]`.
    #[default]
    Dark,
    /// `[color.light]`.
    Light,
}

/// The log filter's default, `qs_log`'s when `RUST_LOG` is unset.
pub const DEFAULT_LOG_FILTER: &str = "info";

/// A setting, named by its key.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub enum Setting {
    /// `appearance.theme`.
    #[serde(rename = "appearance.theme")]
    Theme,
    /// `log.filter`.
    #[serde(rename = "log.filter")]
    LogFilter,
}

impl Setting {
    /// Every setting, in the order the reference lists them.
    pub const ALL: [Self; 2] = [Self::Theme, Self::LogFilter];

    /// The key the files and the commands name it by.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Theme => "appearance.theme",
            Self::LogFilter => "log.filter",
        }
    }

    /// The table a file keeps it in and its name in that table: `("appearance", "theme")`.
    const fn place(self) -> (&'static str, &'static str) {
        match self {
            Self::Theme => ("appearance", "theme"),
            Self::LogFilter => ("log", "filter"),
        }
    }

    /// What it does, for the reference and `settings.list`.
    #[must_use]
    pub const fn summary(self) -> &'static str {
        match self {
            Self::Theme => "The colour theme: `dark` or `light`.",
            Self::LogFilter => {
                "Which events the log records, as `RUST_LOG` directives (`info`, \
                 `info,qc_sema=debug`); `RUST_LOG` wins when it is set."
            }
        }
    }

    /// The value it has when no file sets it.
    #[must_use]
    pub fn default_value(self) -> Value {
        match self {
            Self::Theme => json(Theme::default()),
            Self::LogFilter => Value::from(DEFAULT_LOG_FILTER),
        }
    }
}

/// The value `value` serialises to; the settings' types always serialise.
fn json<T: Serialize>(value: T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// `[appearance]` of a settings file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Appearance {
    /// The colour theme.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<Theme>,
}

/// `[log]` of a settings file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Log {
    /// The log's `RUST_LOG`-style filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
}

/// One settings file: the settings it sets, each left out when it does not.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SettingsFile {
    /// Appearance.
    #[serde(default, skip_serializing_if = "is_default")]
    pub appearance: Appearance,
    /// Logging.
    #[serde(default, skip_serializing_if = "is_default")]
    pub log: Log,
}

/// Whether a table sets nothing, so the file leaves it out.
fn is_default<T: Default + PartialEq>(table: &T) -> bool {
    *table == T::default()
}

impl SettingsFile {
    /// The file at `path`, or one setting nothing when there is none.
    ///
    /// # Errors
    ///
    /// The file cannot be read, or is not a settings file: unknown keys and values of the
    /// wrong type are refused, so a typo is not silently ignored.
    pub fn load(path: &Path) -> Result<Self, SettingsError> {
        Self::parse(path, &read_text(path)?)
    }

    /// The settings `text`, the contents of the file at `path`, sets.
    fn parse(path: &Path, text: &str) -> Result<Self, SettingsError> {
        toml::from_str(text).map_err(|error| SettingsError::parse(path, &error))
    }

    /// What the file sets `setting` to, if anything.
    #[must_use]
    pub fn get(&self, setting: Setting) -> Option<Value> {
        match setting {
            Setting::Theme => self.appearance.theme.map(json),
            Setting::LogFilter => self.log.filter.clone().map(Value::from),
        }
    }

    /// Sets `setting` to `value`, or leaves it unset for `None`.
    ///
    /// # Errors
    ///
    /// `value` is not of the setting's type; the file is left as it was.
    pub fn put(&mut self, setting: Setting, value: Option<Value>) -> Result<(), SettingsError> {
        let invalid = |error: serde_json::Error| SettingsError::value(setting, &error);
        match setting {
            Setting::Theme => {
                self.appearance.theme = value
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(invalid)?;
            }
            Setting::LogFilter => {
                self.log.filter = value
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(invalid)?;
            }
        }
        Ok(())
    }
}

/// The text of the file at `path`; empty when there is none.
fn read_text(path: &Path) -> Result<String, SettingsError> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(SettingsError::read(path, error)),
    }
}

/// Writes `text` to the file at `path`, making its directory.
fn write_text(path: &Path, text: &str) -> Result<(), SettingsError> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|error| SettingsError::write(path, &error))?;
    }
    fs::write(path, text).map_err(|error| SettingsError::write(path, &error))
}

/// `text`, the file at `path`, with `setting` set to `value`, or removed for `None`, and the
/// rest of it as it was, comments included.
///
/// A value keeps the comment after it on its line. A table a removal leaves empty is removed
/// too, unless a comment is attached to its header.
fn edit(
    path: &Path,
    text: &str,
    setting: Setting,
    value: Option<&Value>,
) -> Result<String, SettingsError> {
    let mut document: DocumentMut = text
        .parse()
        .map_err(|error| SettingsError::parse_edit(path, &error))?;
    let (table_name, name) = setting.place();
    match value {
        Some(value) => {
            let mut value = toml_value(path, value)?;
            let item = document.entry(table_name).or_insert_with(toml_edit::table);
            let table = table_like(path, table_name, item)?;
            if let Some(old) = table.get_mut(name).and_then(Item::as_value_mut) {
                *value.decor_mut() = old.decor().clone();
                *old = value;
            } else {
                table.insert(name, Item::Value(value));
            }
        }
        None => {
            if let Some(item) = document.get_mut(table_name) {
                table_like(path, table_name, item)?.remove(name);
            }
            if let Some(Item::Table(table)) = document.get(table_name)
                && table.is_empty()
                && !has_comment(table.decor())
            {
                document.remove(table_name);
            }
        }
    }
    Ok(document.to_string())
}

/// The table `name` of the file at `path`, refused when it is written another way (as an
/// array, which TOML reads as the table's fields in order).
fn table_like<'a>(
    path: &Path,
    name: &str,
    item: &'a mut Item,
) -> Result<&'a mut dyn TableLike, SettingsError> {
    item.as_table_like_mut()
        .ok_or_else(|| SettingsError::not_a_table(path, name))
}

/// Whether the space around a table's header holds a comment.
fn has_comment(decor: &Decor) -> bool {
    [decor.prefix(), decor.suffix()]
        .into_iter()
        .flatten()
        .any(|raw| raw.as_str().is_none_or(|text| text.contains('#')))
}

/// `value`, a setting's value, as TOML.
fn toml_value(path: &Path, value: &Value) -> Result<toml_edit::Value, SettingsError> {
    let value = toml::Value::try_from(value).map_err(|error| SettingsError::write(path, &error))?;
    value
        .to_string()
        .parse()
        .map_err(|error| SettingsError::write(path, &error))
}

/// The user's settings and, with a project open, the project's, each with the file it is read
/// from and written to.
///
/// Each file is read when the store opens and again whenever a setting in it is changed, so a
/// change by hand made in between is kept: [`SettingsStore::put`] edits the file as it is then,
/// changing only the one setting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsStore {
    user_path: PathBuf,
    user: SettingsFile,
    project: Option<(PathBuf, SettingsFile)>,
}

impl SettingsStore {
    /// The settings in the user's file at `user_path` and, for an open project, the project's
    /// file at `project_path`.
    ///
    /// # Errors
    ///
    /// Either file cannot be read ([`SettingsFile::load`]).
    pub fn open(user_path: PathBuf, project_path: Option<PathBuf>) -> Result<Self, SettingsError> {
        let user = SettingsFile::load(&user_path)?;
        let project = match project_path {
            Some(path) => {
                let file = SettingsFile::load(&path)?;
                Some((path, file))
            }
            None => None,
        };
        Ok(Self {
            user_path,
            user,
            project,
        })
    }

    /// The file of `scope`; `None` for the project's with no project open.
    #[must_use]
    pub fn file(&self, scope: Scope) -> Option<&SettingsFile> {
        match scope {
            Scope::User => Some(&self.user),
            Scope::Project => self.project.as_ref().map(|(_, file)| file),
        }
    }

    /// The value `setting` has, and where it comes from: the project's file, else the user's,
    /// else the default.
    #[must_use]
    pub fn value(&self, setting: Setting) -> (Value, Source) {
        if let Some(value) = self.file(Scope::Project).and_then(|file| file.get(setting)) {
            return (value, Source::Project);
        }
        if let Some(value) = self.user.get(setting) {
            return (value, Source::User);
        }
        (setting.default_value(), Source::Default)
    }

    /// The colour theme in force.
    #[must_use]
    pub fn theme(&self) -> Theme {
        serde_json::from_value(self.value(Setting::Theme).0).unwrap_or_default()
    }

    /// The log filter in force.
    #[must_use]
    pub fn log_filter(&self) -> String {
        match self.value(Setting::LogFilter).0 {
            Value::String(filter) => filter,
            _ => DEFAULT_LOG_FILTER.to_owned(),
        }
    }

    /// Sets `setting` in the file of `scope`, or unsets it for `None`, and gives the value
    /// the file had for it.
    ///
    /// The file is read again and only that setting's line is changed, so the rest of the text,
    /// comments and changes made by hand included, stays as it is; the store then holds what
    /// the file now sets. A file that already has the value is not written.
    ///
    /// # Errors
    ///
    /// No project is open for [`Scope::Project`], the file cannot be read or is no longer a
    /// settings file ([`SettingsError::Parse`]), the value is not of the setting's type, or
    /// the file cannot be written. Neither the file nor the store changes when it fails.
    pub fn put(
        &mut self,
        scope: Scope,
        setting: Setting,
        value: Option<Value>,
    ) -> Result<Option<Value>, SettingsError> {
        let (path, file) = match scope {
            Scope::User => (&self.user_path, &mut self.user),
            Scope::Project => match &mut self.project {
                Some((path, file)) => (&*path, file),
                None => return Err(SettingsError::NoProject),
            },
        };
        let text = read_text(path)?;
        let mut changed = SettingsFile::parse(path, &text)?;
        let before = changed.get(setting);
        changed.put(setting, value)?;
        let after = changed.get(setting);
        if after != before {
            write_text(path, &edit(path, &text, setting, after.as_ref())?)?;
        }
        *file = changed;
        Ok(before)
    }
}

/// `docs/settings.md`: every setting with its key, default and summary.
#[must_use]
pub fn reference() -> String {
    let mut text = String::from(
        "# Settings\n\n\
         <!-- Generated from crates/qs-settings (`qs_settings::reference`); do not edit by hand. -->\n\n\
         The user's settings are in `settings.toml` in the app's config directory; a project's are\n\
         in `.quakec-studio/settings.toml` in the project's directory, and win over the user's.\n\
         Each file is a TOML table of the settings it sets: `appearance.theme = \"light\"`.\n\
         The `settings.list`, `settings.get`, `settings.set` and `settings.reset` commands read\n\
         and change them.\n\n\
         | Key | Default | What it does |\n| --- | --- | --- |\n",
    );
    for setting in Setting::ALL {
        text.push_str(&format!(
            "| `{}` | `{}` | {} |\n",
            setting.key(),
            setting.default_value(),
            setting.summary()
        ));
    }
    text
}
