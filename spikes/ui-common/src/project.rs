// SPDX-License-Identifier: GPL-3.0-or-later
//! Loads the benchmark project: every `.qc` file under a directory, in path order.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// A loaded project: its files and one buffer holding the first `limit` lines of all of them.
#[derive(Debug)]
pub struct Project {
    pub files: Vec<(PathBuf, usize)>,
    pub total_lines: usize,
    /// The concatenated text the editor opens.
    pub text: String,
}

/// Reads every `.qc` file under `root`, sorted by path, and concatenates them until `limit`
/// lines are collected. The editor opens that one buffer: a 50,000-line buffer is a harsher
/// test of layout and scrolling than the same lines spread over many files, which no single
/// QuakeC file reaches.
///
/// # Errors
/// Fails when the directory cannot be read or holds no `.qc` file.
pub fn load(root: &Path, limit: usize) -> io::Result<Project> {
    let mut paths = Vec::new();
    collect(root, &mut paths)?;
    paths.sort();
    if paths.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no .qc files under {}", root.display()),
        ));
    }
    let mut files = Vec::new();
    let mut text = String::new();
    let mut total_lines = 0usize;
    for path in paths {
        let bytes = fs::read(&path)?;
        let content = String::from_utf8_lossy(&bytes);
        let lines = content.lines().count();
        files.push((
            path.strip_prefix(root).unwrap_or(&path).to_path_buf(),
            lines,
        ));
        for line in content.lines() {
            if total_lines >= limit {
                break;
            }
            text.push_str(line);
            text.push('\n');
            total_lines = total_lines.saturating_add(1);
        }
    }
    Ok(Project {
        files,
        total_lines,
        text,
    })
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect(&path, out)?;
        } else if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("qc"))
        {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concatenates_qc_files_in_path_order_up_to_the_limit() -> io::Result<()> {
        let dir = std::env::temp_dir().join(format!("ui-common-project-{}", std::process::id()));
        fs::create_dir_all(dir.join("sub"))?;
        fs::write(dir.join("b.qc"), "b1\nb2\n")?;
        fs::write(dir.join("sub/a.qc"), "a1\n")?;
        fs::write(dir.join("notes.txt"), "ignored\n")?;
        let project = load(&dir, 2)?;
        assert_eq!(project.text, "b1\nb2\n");
        assert_eq!(project.files.len(), 2);
        let all = load(&dir, 10)?;
        assert_eq!(all.total_lines, 3);
        fs::remove_dir_all(&dir)?;
        let missing = load(&dir, 10);
        assert!(missing.is_err());
        Ok(())
    }
}
