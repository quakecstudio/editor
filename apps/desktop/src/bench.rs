// SPDX-License-Identifier: GPL-3.0-or-later

//! The code editor's benchmark project (P2-18, PRIN-5): 25 files of 2,000 lines of QuakeC and
//! a `defs.qc`, 50,000 lines in all, which the `editor_bench` example measures in a window and
//! `qs-bench`'s budgets measure headless.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The project's files of QuakeC, and each one's lines.
pub const FILES: usize = 25;
/// The lines of each file.
pub const LINES_PER_FILE: usize = 2_000;
/// The file the editor shows.
pub const MIDDLE_FILE: usize = 12;
/// The line of that file the caret starts on.
pub const MIDDLE_LINE: usize = 1_000;
/// The text typed, one character a frame.
pub const TYPED: &str = "self.speed = self.speed * 0.7; // slow down";

/// Writes the benchmark project into `dir`: `defs.qc` and [`FILES`] files of
/// [`LINES_PER_FILE`] lines of functions; the middle file's path.
///
/// # Errors
///
/// The error of a file that cannot be written, as text.
pub fn write_project(dir: &Path) -> Result<PathBuf, String> {
    let io = |e: std::io::Error| format!("{}: {e}", dir.display());
    std::fs::create_dir_all(dir).map_err(io)?;
    let mut src = String::from("progs.dat\ndefs.qc\n");
    let defs = "entity self;\nfloat time;\n.float health;\n.float speed;\n\
                void(entity e) remove = #15;\n";
    std::fs::write(dir.join("defs.qc"), defs).map_err(io)?;
    for file in 0..FILES {
        let mut text = String::new();
        let mut function = 0_usize;
        while text.lines().count() < LINES_PER_FILE {
            writeln!(
                text,
                "void() f{file}_{function} =\n{{\n\tlocal float x;\n\tx = self.health + time;\n\
                 \tif (x > 10)\n\t\tself.speed = x * 0.5;\n\telse\n\t\tself.speed = 0;\n\
                 \tself.health = self.health - 1;\n}};\n"
            )
            .map_err(|e| e.to_string())?;
            function = function.saturating_add(1);
        }
        let name = format!("f{file}.qc");
        std::fs::write(dir.join(&name), text).map_err(io)?;
        src.push_str(&name);
        src.push('\n');
    }
    std::fs::write(dir.join("progs.src"), src).map_err(io)?;
    Ok(dir.join(format!("f{MIDDLE_FILE}.qc")))
}

/// The byte offset of the start of [`MIDDLE_LINE`] in `text`, where the caret is put.
#[must_use]
pub fn caret_offset(text: &str) -> usize {
    text.match_indices('\n')
        .nth(MIDDLE_LINE)
        .map_or(0, |(at, _)| at.saturating_add(1))
}
