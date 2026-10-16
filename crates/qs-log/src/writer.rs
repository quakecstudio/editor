// SPDX-License-Identifier: GPL-3.0-or-later

//! The log file: unbuffered appends, rotated by size, with a cap on the number of files.

use std::borrow::Cow;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Appended to a record cut short to fit in one file.
const TRUNCATED: &str = " [truncated]\n";

/// The current log file and its rotated predecessors.
///
/// Every record goes to the operating system in one unbuffered `write_all`, so a crash loses at
/// most the record being written. When the next record would take the current file past
/// `max_file_bytes`, `<stem>.log` becomes `<stem>.1.log`, each older `<stem>.<n>.log` moves up
/// one, and the one past `max_files` is deleted.
#[derive(Debug)]
pub(crate) struct RotatingFile {
    dir: PathBuf,
    stem: String,
    max_file_bytes: u64,
    max_files: u32,
    file: File,
    len: u64,
}

impl RotatingFile {
    /// Opens `<dir>/<stem>.log` for appending, creating `dir` if needed.
    pub(crate) fn open(
        dir: &Path,
        stem: &str,
        max_file_bytes: u64,
        max_files: u32,
    ) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        let path = file_path(dir, stem, 0);
        let file = append(&path)?;
        let len = file.metadata()?.len();
        Ok(Self {
            dir: dir.to_path_buf(),
            stem: stem.to_owned(),
            max_file_bytes,
            max_files,
            file,
            len,
        })
    }

    /// Writes one record, rotating first if it would not fit. A record longer than a whole file
    /// is cut at a character boundary and marked ` [truncated]`.
    pub(crate) fn write_record(&mut self, record: &str) -> io::Result<()> {
        let record = truncate(record, self.max_file_bytes);
        let size = u64::try_from(record.len()).unwrap_or(u64::MAX);
        if self.len > 0 && self.len.saturating_add(size) > self.max_file_bytes {
            self.rotate()?;
        }
        self.file.write_all(record.as_bytes())?;
        self.len = self.len.saturating_add(size);
        Ok(())
    }

    fn rotate(&mut self) -> io::Result<()> {
        let oldest = file_path(&self.dir, &self.stem, self.max_files);
        remove_if_present(&oldest)?;
        let mut index = self.max_files;
        while let Some(newer) = index.checked_sub(1) {
            let from = file_path(&self.dir, &self.stem, newer);
            if from.exists() {
                fs::rename(&from, file_path(&self.dir, &self.stem, index))?;
            }
            index = newer;
        }
        self.file = append(&file_path(&self.dir, &self.stem, 0))?;
        self.len = self.file.metadata()?.len();
        Ok(())
    }
}

/// `<stem>.log` for index 0, `<stem>.<index>.log` for the rotated files.
pub(crate) fn file_path(dir: &Path, stem: &str, index: u32) -> PathBuf {
    if index == 0 {
        dir.join(format!("{stem}.log"))
    } else {
        dir.join(format!("{stem}.{index}.log"))
    }
}

fn append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// Cuts `record` to at most `max` bytes, keeping whole characters and the truncation marker.
fn truncate(record: &str, max: u64) -> Cow<'_, str> {
    let max = usize::try_from(max).unwrap_or(usize::MAX);
    if record.len() <= max {
        return record.into();
    }
    let mut end = max.saturating_sub(TRUNCATED.len());
    while !record.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    let kept = record.get(..end).unwrap_or_default();
    format!("{kept}{TRUNCATED}").into()
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use super::*;
    use crate::test_dir::TestDir;

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    #[test]
    fn creates_the_directory_and_appends() {
        let dir = TestDir::new("writer-append");
        let logs = dir.path().join("nested").join("logs");
        let mut file = RotatingFile::open(&logs, "app", 1_000, 2).unwrap();
        file.write_record("one\n").unwrap();
        file.write_record("two\n").unwrap();
        assert_eq!(read(&logs.join("app.log")), "one\ntwo\n");
    }

    #[test]
    fn reopening_continues_the_existing_file_and_its_size() {
        let dir = TestDir::new("writer-reopen");
        RotatingFile::open(dir.path(), "app", 10, 1)
            .unwrap()
            .write_record("12345678\n")
            .unwrap();
        let mut file = RotatingFile::open(dir.path(), "app", 10, 1).unwrap();
        file.write_record("abc\n").unwrap();
        assert_eq!(read(&dir.path().join("app.1.log")), "12345678\n");
        assert_eq!(read(&dir.path().join("app.log")), "abc\n");
    }

    #[test]
    fn rotates_at_the_size_limit_and_keeps_max_files() {
        let dir = TestDir::new("writer-rotate");
        let mut file = RotatingFile::open(dir.path(), "app", 4, 2).unwrap();
        for record in ["a1\n", "b2\n", "c3\n", "d4\n"] {
            file.write_record(record).unwrap();
        }
        assert_eq!(read(&dir.path().join("app.log")), "d4\n");
        assert_eq!(read(&dir.path().join("app.1.log")), "c3\n");
        assert_eq!(read(&dir.path().join("app.2.log")), "b2\n");
        assert!(!dir.path().join("app.3.log").exists());
    }

    #[test]
    fn a_record_that_exactly_fills_the_file_does_not_rotate() {
        let dir = TestDir::new("writer-exact");
        let mut file = RotatingFile::open(dir.path(), "app", 6, 1).unwrap();
        file.write_record("ab\n").unwrap();
        file.write_record("cd\n").unwrap();
        assert_eq!(read(&dir.path().join("app.log")), "ab\ncd\n");
        assert!(!dir.path().join("app.1.log").exists());
        file.write_record("e\n").unwrap();
        assert_eq!(read(&dir.path().join("app.1.log")), "ab\ncd\n");
    }

    #[test]
    fn zero_max_files_keeps_only_the_current_file() {
        let dir = TestDir::new("writer-zero");
        let mut file = RotatingFile::open(dir.path(), "app", 3, 0).unwrap();
        file.write_record("ab\n").unwrap();
        file.write_record("cd\n").unwrap();
        assert_eq!(read(&dir.path().join("app.log")), "cd\n");
        assert!(!dir.path().join("app.1.log").exists());
    }

    #[test]
    fn an_oversized_record_is_truncated_to_one_file() {
        let dir = TestDir::new("writer-truncate");
        let mut file = RotatingFile::open(dir.path(), "app", 20, 1).unwrap();
        file.write_record(&"x".repeat(100)).unwrap();
        let text = read(&dir.path().join("app.log"));
        assert_eq!(text, format!("{}{TRUNCATED}", "x".repeat(7)));
        assert_eq!(text.len(), 20);
    }

    #[test]
    fn truncation_keeps_whole_characters() {
        let long = "\u{e9}".repeat(20);
        let cut = truncate(&long, 18);
        assert_eq!(cut, format!("{}{TRUNCATED}", "\u{e9}".repeat(2)));
        assert_eq!(truncate("short", 18), "short");
        assert_eq!(truncate("abc", 2), TRUNCATED);
    }

    #[test]
    fn open_fails_when_the_directory_is_a_file() {
        let dir = TestDir::new("writer-notdir");
        let blocker = dir.path().join("blocker");
        fs::write(&blocker, "").unwrap();
        assert!(RotatingFile::open(&blocker, "app", 10, 1).is_err());
    }

    #[test]
    fn file_names() {
        let dir = Path::new("logs");
        assert_eq!(file_path(dir, "app", 0), dir.join("app.log"));
        assert_eq!(file_path(dir, "app", 3), dir.join("app.3.log"));
    }
}
