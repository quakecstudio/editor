// SPDX-License-Identifier: GPL-3.0-or-later

//! Thread history (P4-31, UI-22): each thread kept in the project as it ended, to list,
//! search and reopen with its messages, the checkpoint it recorded and the hunks it staged, as
//! Zed's agent panel keeps its threads.
//!
//! A thread is a JSON file `<number>.json` in `.quakec-studio/threads/` under the project, the
//! numbers counting up from 1 in the order threads were kept, which is the order they are
//! listed in, the newest first.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Message;

/// The folder a project's threads are kept in, under its directory.
#[must_use]
pub fn threads_dir(project: &Path) -> PathBuf {
    project.join(".quakec-studio").join("threads")
}

/// A thread as kept.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThreadRecord {
    /// Its number in the folder, from 1.
    pub number: u32,
    /// Its title: the task's first line.
    pub title: String,
    /// Every message, the brief first.
    pub transcript: Vec<Message>,
    /// The checkpoint it recorded before it changed anything.
    pub checkpoint: Option<u32>,
    /// The ids of the hunks it staged, as `edit.staged` numbers them.
    #[serde(default)]
    pub staged: Vec<u64>,
    /// The summaries its compactions left.
    #[serde(default)]
    pub summaries: Vec<String>,
    /// Its estimated tokens when it ended.
    #[serde(default)]
    pub tokens: usize,
    /// How it ended, as the window says it.
    #[serde(default)]
    pub outcome: String,
}

impl ThreadRecord {
    /// Whether every word of `query` is in its title or one of its messages' text, ignoring
    /// case; an empty query matches every thread.
    #[must_use]
    pub fn matches(&self, query: &str) -> bool {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        if words.is_empty() {
            return true;
        }
        let mut text = self.title.to_lowercase();
        for message in &self.transcript {
            if let Message::User { text: said } | Message::Assistant { text: said, .. } = message {
                text.push('\n');
                text.push_str(&said.to_lowercase());
            }
        }
        words.iter().all(|word| text.contains(word.as_str()))
    }
}

/// The title of a thread on `task`: its first line.
#[must_use]
pub fn title_of(task: &str) -> String {
    task.lines().next().unwrap_or_default().trim().to_owned()
}

/// The threads kept in `dir`, the newest first; a file that is not a thread is skipped.
///
/// # Errors
///
/// The folder exists but cannot be read.
pub fn load(dir: &Path) -> io::Result<Vec<ThreadRecord>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut threads: Vec<ThreadRecord> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|e| e == "json"))
        .filter_map(|entry| fs::read_to_string(entry.path()).ok())
        .filter_map(|text| serde_json::from_str(&text).ok())
        .collect();
    threads.sort_by_key(|thread| std::cmp::Reverse(thread.number));
    Ok(threads)
}

/// Keeps `record` in `dir` under the next number, which it is given and returned.
///
/// # Errors
///
/// The folder cannot be made or the file written.
pub fn keep(dir: &Path, mut record: ThreadRecord) -> io::Result<u32> {
    fs::create_dir_all(dir)?;
    let next = load(dir)?
        .first()
        .map_or(1, |newest| newest.number.saturating_add(1));
    record.number = next;
    let text = serde_json::to_string_pretty(&record).map_err(io::Error::other)?;
    fs::write(dir.join(format!("{next}.json")), text)?;
    Ok(next)
}
