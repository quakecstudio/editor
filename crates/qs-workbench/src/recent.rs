// SPDX-License-Identifier: GPL-3.0-or-later

//! The projects opened most recently, kept in the data directory between runs, for the Project
//! section to offer before a project is open, as Zed's recent projects are offered.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The recent projects file in the data directory `data_dir`.
#[must_use]
pub fn recent_path(data_dir: &Path) -> PathBuf {
    data_dir.join(RECENT_FILE)
}

/// The file's name.
const RECENT_FILE: &str = "recent-projects.json";

/// How many projects the list keeps.
pub const RECENT_LIMIT: usize = 10;

/// The project directories opened most recently, the latest first.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentProjects {
    projects: Vec<PathBuf>,
}

impl RecentProjects {
    /// The list in `file`; an empty one when the file is missing or unreadable, which a first
    /// run, or a file from another version, is.
    #[must_use]
    pub fn load(file: &Path) -> Self {
        let Ok(text) = std::fs::read(file) else {
            return Self::default();
        };
        match serde_json::from_slice(&text) {
            Ok(recent) => recent,
            Err(error) => {
                tracing::warn!(%error, file = %file.display(), "recent projects not read");
                Self::default()
            }
        }
    }

    /// Writes the list to `file`, making its directory.
    ///
    /// # Errors
    ///
    /// The I/O error when the file cannot be written.
    pub fn save(&self, file: &Path) -> std::io::Result<()> {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(file, text)
    }

    /// The projects, the latest first.
    #[must_use]
    pub fn projects(&self) -> &[PathBuf] {
        &self.projects
    }

    /// Puts `project` first, once, keeping at most [`RECENT_LIMIT`]; `true` when the list
    /// changed.
    pub fn opened(&mut self, project: &Path) -> bool {
        if self.projects.first().is_some_and(|first| first == project) {
            return false;
        }
        self.projects.retain(|p| p != project);
        self.projects.insert(0, project.to_path_buf());
        self.projects.truncate(RECENT_LIMIT);
        true
    }

    /// Takes `project` off the list, as when it can no longer be opened.
    pub fn forget(&mut self, project: &Path) {
        self.projects.retain(|p| p != project);
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use std::path::{Path, PathBuf};

    use super::{RECENT_LIMIT, RecentProjects, recent_path};

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("qs-recent-{name}-{}", std::process::id()));
        let _ignored = std::fs::remove_dir_all(&dir);
        dir
    }

    // No spec statement: opening a project puts it first, once, the list holding at most
    // ten; forgetting one takes it off
    #[test]
    fn the_latest_project_comes_first_once() {
        let mut recent = RecentProjects::default();
        assert!(recent.opened(Path::new("/a")));
        assert!(recent.opened(Path::new("/b")));
        assert!(!recent.opened(Path::new("/b")), "already first");
        assert!(recent.opened(Path::new("/a")));
        assert_eq!(
            recent.projects(),
            [PathBuf::from("/a"), PathBuf::from("/b")]
        );
        for n in 0..RECENT_LIMIT {
            recent.opened(&PathBuf::from(format!("/p{n}")));
        }
        assert_eq!(recent.projects().len(), RECENT_LIMIT);
        assert_eq!(recent.projects().first(), Some(&PathBuf::from("/p9")));
        assert!(
            !recent.projects().contains(&PathBuf::from("/a")),
            "the oldest dropped"
        );
        recent.forget(Path::new("/p9"));
        assert_eq!(recent.projects().first(), Some(&PathBuf::from("/p8")));
    }

    // No spec statement: the list survives a save and a load, in a directory made for it; a
    // missing or malformed file loads as an empty list
    #[test]
    fn the_list_is_saved_and_loaded() {
        let file = recent_path(&dir("save").join("data"));
        assert_eq!(RecentProjects::load(&file), RecentProjects::default());
        let mut recent = RecentProjects::default();
        recent.opened(Path::new("/games/ember"));
        recent.save(&file).unwrap();
        assert_eq!(RecentProjects::load(&file), recent);
        std::fs::write(&file, b"{ not json").unwrap();
        assert_eq!(RecentProjects::load(&file), RecentProjects::default());
        assert!(file.ends_with("recent-projects.json"));
    }
}
