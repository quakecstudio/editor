// SPDX-License-Identifier: GPL-3.0-or-later

//! What the background check keeps between runs: when it last checked and what it found, so
//! it asks at most once a day and a start in between still shows the release it knows of.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::{Feed, Found, Version};

/// The least time between two background checks.
pub const CHECK_INTERVAL: Duration = Duration::from_hours(24);

/// The file the background check's state is kept in, in the app's data directory
/// (`qs_paths::AppDirs::data`).
#[must_use]
pub fn state_path(data_dir: &Path) -> PathBuf {
    data_dir.join("update.json")
}

/// When the background check last had an answer, and the later release it named, if any.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    /// When the feed was last read, in seconds since the Unix epoch.
    #[serde(default)]
    checked: Option<u64>,
    /// The later release that read found.
    #[serde(default)]
    newer: Option<Feed>,
}

impl State {
    /// The state kept in the file at `path`; nothing checked yet when there is no file, or
    /// when it does not read as one, which a warning says.
    #[must_use]
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|error| tracing::warn!(%error, "the update check's state is not read"))
                .unwrap_or_default(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Self::default(),
            Err(error) => {
                tracing::warn!(%error, "the update check's state is not read");
                Self::default()
            }
        }
    }

    /// Writes the state to the file at `path`, making its directory.
    ///
    /// # Errors
    ///
    /// The directory or the file could not be written.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_string(self)?)
    }

    /// Whether a background check is due at `now`: none had an answer yet, the last was
    /// [`CHECK_INTERVAL`] ago or more, or the last is later than `now`, as after the clock was
    /// set back.
    #[must_use]
    pub fn due(&self, now: SystemTime) -> bool {
        let Some(checked) = self.checked else {
            return true;
        };
        seconds(now)
            .checked_sub(checked)
            .is_none_or(|since| since >= CHECK_INTERVAL.as_secs())
    }

    /// Records that a check at `now` found `found`.
    pub fn record(&mut self, now: SystemTime, found: &Found) {
        self.checked = Some(seconds(now));
        self.newer = match found {
            Found::Newer(feed) => Some(feed.clone()),
            Found::UpToDate => None,
        };
    }

    /// The release the last check found, while it is still later than `current`: an install
    /// updated since has it already.
    #[must_use]
    pub fn newer_than(&self, current: Version) -> Option<&Feed> {
        self.newer.as_ref().filter(|feed| feed.version() > current)
    }
}

/// `time` in whole seconds since the Unix epoch; 0 for a time before it.
fn seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}
