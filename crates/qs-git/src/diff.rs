// SPDX-License-Identifier: GPL-3.0-or-later

//! A file's diff as `git diff` writes it: its header and its hunks, and the patch of one hunk
//! that `git apply --cached` stages.

use crate::GitError;

/// One hunk of a diff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    /// The first line it covers in the old text, from 1.
    pub old_start: usize,
    /// How many lines of the old text it covers.
    pub old_lines: usize,
    /// The first line it covers in the new text, from 1.
    pub new_start: usize,
    /// How many lines of the new text it covers.
    pub new_lines: usize,
    /// Its text: the `@@` line and each line after it, with git's ` `, `-` and `+` marks.
    pub text: String,
}

/// A file's diff: the header lines before its first hunk, and its hunks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileDiff {
    /// `diff --git`, `index`, `---` and `+++` and any mode lines.
    pub header: String,
    /// The hunks, in order.
    pub hunks: Vec<Hunk>,
}

impl FileDiff {
    /// Reads `git diff`'s output for one file; an empty output is no change.
    ///
    /// # Errors
    ///
    /// [`GitError::Unexpected`] for a hunk line that is not `@@ -a,b +c,d @@`.
    pub fn parse(text: &str) -> Result<Self, GitError> {
        let mut diff = Self::default();
        for line in text.split_inclusive('\n') {
            if line.starts_with("@@") {
                let (old_start, old_lines, new_start, new_lines) = hunk_range(line)?;
                diff.hunks.push(Hunk {
                    old_start,
                    old_lines,
                    new_start,
                    new_lines,
                    text: line.to_owned(),
                });
            } else if let Some(hunk) = diff.hunks.last_mut() {
                hunk.text.push_str(line);
            } else {
                diff.header.push_str(line);
            }
        }
        Ok(diff)
    }

    /// The patch of hunk `index` alone: the header and that hunk.
    #[must_use]
    pub fn patch(&self, index: usize) -> Option<String> {
        let hunk = self.hunks.get(index)?;
        let mut patch = self.header.clone();
        patch.push_str(&hunk.text);
        if !patch.ends_with('\n') {
            patch.push('\n');
        }
        Some(patch)
    }
}

/// The ranges of a `@@ -a,b +c,d @@` line; a missing count is 1, as git writes a one-line
/// range.
fn hunk_range(line: &str) -> Result<(usize, usize, usize, usize), GitError> {
    let bad = || GitError::Unexpected(line.trim_end().to_owned());
    let mut words = line.split(' ');
    let (Some("@@"), Some(old), Some(new)) = (words.next(), words.next(), words.next()) else {
        return Err(bad());
    };
    let range = |text: &str| -> Option<(usize, usize)> {
        let (start, count) = text.split_once(',').unwrap_or((text, "1"));
        Some((start.parse().ok()?, count.parse().ok()?))
    };
    let (old_start, old_lines) = old.strip_prefix('-').and_then(range).ok_or_else(bad)?;
    let (new_start, new_lines) = new.strip_prefix('+').and_then(range).ok_or_else(bad)?;
    Ok((old_start, old_lines, new_start, new_lines))
}
