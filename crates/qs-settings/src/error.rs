// SPDX-License-Identifier: GPL-3.0-or-later

//! What can go wrong reading and changing settings.

use std::error::Error;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use crate::Setting;

/// Why settings could not be read or changed.
#[derive(Debug)]
pub enum SettingsError {
    /// A settings file exists but cannot be read.
    Read {
        /// The file.
        path: PathBuf,
        /// Why.
        error: io::Error,
    },
    /// A settings file is not valid: not TOML, an unknown key, or a value of the wrong type.
    Parse {
        /// The file.
        path: PathBuf,
        /// What is wrong: what TOML's parser says, or which table is not written as one.
        message: String,
    },
    /// A settings file, or its directory, cannot be written.
    Write {
        /// The file.
        path: PathBuf,
        /// Why.
        message: String,
    },
    /// A value is not of its setting's type.
    Value {
        /// The setting.
        setting: Setting,
        /// What is wrong with the value.
        message: String,
    },
    /// The project's settings were asked for with no project open.
    NoProject,
}

impl SettingsError {
    pub(crate) fn read(path: &Path, error: io::Error) -> Self {
        Self::Read {
            path: path.to_path_buf(),
            error,
        }
    }

    pub(crate) fn parse(path: &Path, error: &toml::de::Error) -> Self {
        Self::Parse {
            path: path.to_path_buf(),
            message: error.message().to_owned(),
        }
    }

    pub(crate) fn parse_edit(path: &Path, error: &toml_edit::TomlError) -> Self {
        Self::Parse {
            path: path.to_path_buf(),
            message: error.message().to_owned(),
        }
    }

    pub(crate) fn not_a_table(path: &Path, name: &str) -> Self {
        Self::Parse {
            path: path.to_path_buf(),
            message: format!("`{name}` is not written as a table, so it cannot be changed"),
        }
    }

    pub(crate) fn write(path: &Path, error: &impl fmt::Display) -> Self {
        Self::Write {
            path: path.to_path_buf(),
            message: error.to_string(),
        }
    }

    pub(crate) fn value(setting: Setting, error: &serde_json::Error) -> Self {
        Self::Value {
            setting,
            message: error.to_string(),
        }
    }
}

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, error } => write!(f, "cannot read {}: {error}", path.display()),
            Self::Parse { path, message } => write!(f, "{}: {message}", path.display()),
            Self::Write { path, message } => {
                write!(f, "cannot write {}: {message}", path.display())
            }
            Self::Value { setting, message } => {
                write!(f, "invalid value for {}: {message}", setting.key())
            }
            Self::NoProject => f.write_str("no project is open"),
        }
    }
}

impl Error for SettingsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Read { error, .. } => Some(error),
            Self::Parse { .. } | Self::Write { .. } | Self::Value { .. } | Self::NoProject => None,
        }
    }
}
