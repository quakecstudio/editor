// SPDX-License-Identifier: GPL-3.0-or-later

//! Crash-safe logging for QuakeC Studio (QA-7).
//!
//! [`init`] installs a [`tracing`] subscriber that writes to `<logs>/quakec-studio.log`, or the
//! file a [`LogConfig`] names, in the per-OS log directory from [`qs_paths`], and a panic hook
//! that writes a crash record next to it. Every crate logs through the ordinary `tracing` macros.
//!
//! - **Crash-safe.** Each event is written to the operating system in one unbuffered append, so
//!   nothing is held in a buffer when the process dies. A panic writes
//!   `crash-<time>-<pid>.log` and syncs it to disk before unwinding continues.
//! - **Rotation and a size cap.** When the next event would take the log past
//!   [`LogConfig::max_file_bytes`], the file is rotated; at most [`LogConfig::max_files`] old
//!   files are kept, so the logs never exceed `(max_files + 1) * max_file_bytes`.
//! - **`RUST_LOG` directives.** `target=level` pairs and a bare default level, comma-separated,
//!   as in `RUST_LOG=info,qs_vm=trace`; `info` when the variable is unset or empty.
//! - **Source text is never logged.** Wrap user content in [`SourceText`], whose `Display` and
//!   `Debug` print only its length; a field named in [`SOURCE_FIELD_NAMES`] is redacted whatever
//!   its type; and the crash record never contains the panic message.

mod layer;
mod panic;
mod redact;
mod time;
mod writer;

use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::PathBuf;
use std::time::SystemTime;

use qs_paths::AppDirs;
use tracing::Subscriber;
use tracing::subscriber::set_global_default;
use tracing_subscriber::filter::{ParseError, Targets};
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::registry::Registry;

use crate::layer::FileLayer;
pub use crate::panic::install_panic_hook;
pub use crate::redact::{SOURCE_FIELD_NAMES, SourceText};
use crate::writer::RotatingFile;

/// The default log file's name without its extension, the IDE's.
pub const LOG_FILE_STEM: &str = "quakec-studio";

/// The directives used when `RUST_LOG` is unset or empty.
pub const DEFAULT_DIRECTIVES: &str = "info";

/// Where and how much to log.
#[derive(Debug, Clone)]
pub struct LogConfig {
    /// The directory for the log file and crash records.
    pub dir: PathBuf,
    /// The log file's name without its extension, [`LOG_FILE_STEM`] by default. Each process
    /// that logs to one directory takes its own, since two processes rotating one file would
    /// lose events.
    pub file_stem: String,
    /// The largest a log file may grow before it is rotated, in bytes.
    pub max_file_bytes: u64,
    /// How many rotated files are kept besides the current one.
    pub max_files: u32,
    /// `RUST_LOG`-style directives, such as `info,qs_vm=trace`.
    pub directives: String,
    /// The clock for timestamps; tests inject a fixed one.
    pub clock: fn() -> SystemTime,
}

impl LogConfig {
    /// Logs to `dir/quakec-studio.log` with the default limits: 5 MiB files, four rotated
    /// files kept, `info`.
    #[must_use]
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            file_stem: LOG_FILE_STEM.to_owned(),
            max_file_bytes: 5_242_880,
            max_files: 4,
            directives: DEFAULT_DIRECTIVES.to_owned(),
            clock: SystemTime::now,
        }
    }

    /// Logs to the application's log directory, with directives from `RUST_LOG`.
    #[must_use]
    pub fn for_app(dirs: &AppDirs) -> Self {
        Self::with_rust_log(dirs.logs().to_path_buf(), std::env::var_os("RUST_LOG"))
    }

    /// [`Self::for_app`] with the `RUST_LOG` value given rather than read from the environment,
    /// so a test can set the directives without touching process-global state.
    fn with_rust_log(dir: PathBuf, rust_log: Option<OsString>) -> Self {
        Self {
            directives: directives_from(rust_log),
            ..Self::new(dir)
        }
    }
}

/// The directives in a `RUST_LOG` value, or [`DEFAULT_DIRECTIVES`] when it is unset, empty or
/// not Unicode.
#[must_use]
pub fn directives_from(value: Option<OsString>) -> String {
    value
        .and_then(|value| value.into_string().ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_DIRECTIVES.to_owned())
}

/// Why logging could not be set up.
#[derive(Debug)]
pub enum LogError {
    /// The log directory or file could not be created or opened.
    Io {
        /// The directory.
        dir: PathBuf,
        /// The underlying error.
        source: io::Error,
    },
    /// The directives could not be parsed.
    Directives {
        /// The directives as given.
        directives: String,
        /// The parser's error.
        source: ParseError,
    },
    /// A global subscriber was already installed.
    AlreadyInstalled,
}

impl fmt::Display for LogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { dir, source } => {
                write!(f, "cannot open the log in {}: {source}", dir.display())
            }
            Self::Directives { directives, source } => {
                write!(f, "invalid log directives {directives:?}: {source}")
            }
            Self::AlreadyInstalled => f.write_str("a global log subscriber is already installed"),
        }
    }
}

impl std::error::Error for LogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Directives { source, .. } => Some(source),
            Self::AlreadyInstalled => None,
        }
    }
}

/// Builds the subscriber for `config` without installing it, opening the log file.
///
/// # Errors
///
/// [`LogError::Directives`] when the directives do not parse, and [`LogError::Io`] when the
/// directory or file cannot be created or opened.
pub fn subscriber(config: &LogConfig) -> Result<impl Subscriber + Send + Sync + use<>, LogError> {
    let filter: Targets =
        config
            .directives
            .parse()
            .map_err(|source: ParseError| LogError::Directives {
                directives: config.directives.clone(),
                source,
            })?;
    let file = RotatingFile::open(
        &config.dir,
        &config.file_stem,
        config.max_file_bytes,
        config.max_files,
    )
    .map_err(|source| LogError::Io {
        dir: config.dir.clone(),
        source,
    })?;
    Ok(Registry::default()
        .with(filter)
        .with(FileLayer::new(file, config.clock)))
}

/// Installs the subscriber as the global default and the crash-record panic hook.
///
/// # Errors
///
/// Any error from [`subscriber`], and [`LogError::AlreadyInstalled`] when a global subscriber
/// is already set; the panic hook is installed only on success.
pub fn init(config: &LogConfig) -> Result<(), LogError> {
    set_global_default(subscriber(config)?).map_err(|_| LogError::AlreadyInstalled)?;
    install_panic_hook(config.dir.clone(), config.clock);
    Ok(())
}

#[cfg(test)]
mod test_dir;

#[cfg(test)]
mod tests;
