// SPDX-License-Identifier: GPL-3.0-or-later

//! The file watcher of P5-17 (MAP-32): about once a second, the open map's file and the
//! project's QuakeC are looked at, and the ones whose modified time moved are given to
//! `files.changed`, and so are ones made since they were looked for and missing. A time read for
//! the first time is only remembered.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

/// How often the files are looked at.
const EVERY: Duration = Duration::from_secs(1);

/// The modified times last seen, by file.
#[derive(Debug, Default)]
pub(crate) struct DiskWatch {
    seen: HashMap<PathBuf, SystemTime>,
    /// The files looked for and not there.
    missing: HashSet<PathBuf>,
    last: Option<Instant>,
}

impl DiskWatch {
    /// Whether the files are to be looked at again at `now`: once every [`EVERY`].
    pub(crate) fn due(&self, now: Instant) -> bool {
        self.last
            .is_none_or(|last| now.saturating_duration_since(last) >= EVERY)
    }

    /// How long until the files are looked at again, for the window to wake then.
    pub(crate) const fn every() -> Duration {
        EVERY
    }

    /// The files among `paths` whose modified time moved since they were last looked at, at
    /// most once every [`EVERY`] from `now`; empty in between.
    pub(crate) fn changed(&mut self, now: Instant, paths: Vec<PathBuf>) -> Vec<PathBuf> {
        if !self.due(now) {
            return Vec::new();
        }
        self.last = Some(now);
        let mut changed = Vec::new();
        for path in paths {
            let Ok(modified) = std::fs::metadata(&path).and_then(|meta| meta.modified()) else {
                self.missing.insert(path);
                continue;
            };
            let made = self.missing.remove(&path);
            match self.seen.insert(path.clone(), modified) {
                Some(before) if before != modified => changed.push(path),
                // A file that was not there before is one another program made.
                None if made => changed.push(path),
                Some(_) | None => {}
            }
        }
        changed
    }

    /// Remembers `path`'s modified time now, after the editor wrote it, so its own save is no
    /// change by another program.
    pub(crate) fn wrote(&mut self, path: PathBuf) {
        if let Ok(modified) = std::fs::metadata(&path).and_then(|meta| meta.modified()) {
            self.seen.insert(path, modified);
        }
    }
}

#[cfg(test)]
mod tests;
