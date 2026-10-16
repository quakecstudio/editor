// SPDX-License-Identifier: GPL-3.0-or-later

//! A fresh directory per test, removed when the test ends.

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process;

pub(crate) struct TestDir(PathBuf);

impl TestDir {
    /// `<temp>/qs-log-<pid>-<name>`, emptied first in case an earlier run left it behind.
    pub(crate) fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("qs-log-{}-{name}", process::id()));
        if path.exists() {
            fs::remove_dir_all(&path).unwrap();
        }
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}
