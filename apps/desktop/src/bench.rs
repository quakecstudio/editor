// SPDX-License-Identifier: GPL-3.0-or-later

//! The code editor's benchmark project (P2-18, PRIN-5): 25 files of 2,000 lines of QuakeC and
//! a `defs.qc`, 50,000 lines in all, which the `editor_bench` example measures in a window and
//! `qs-bench`'s budgets measure headless.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use eframe::egui::Ui;
use qs_workbench::entities::Entities;
use serde_json::json;

use crate::entities::{Outliner, outliner as draw_outliner};
use crate::theme::{Mode, Theme};

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

/// The live edicts the Entities outliner is measured with (P3-13: "Stays responsive with
/// 2,000 live edicts").
pub const LIVE_EDICTS: u32 = 2_000;

/// The classnames the live edicts cycle through, as a map's monsters, items and brushes mix.
const CLASSNAMES: [&str; 12] = [
    "monster_army",
    "monster_dog",
    "monster_ogre",
    "monster_knight",
    "item_health",
    "item_shells",
    "weapon_supershotgun",
    "func_door",
    "func_button",
    "trigger_once",
    "light",
    "info_notnull",
];

/// `count` live edicts from 1, their classnames cycling through a map's mix, as the bridge's
/// `edict/spawned` events list them, the middle one selected.
#[must_use]
pub fn live_entities(count: u32) -> Entities {
    let mut entities = Entities::new();
    let middle = count.div_euclid(2);
    let mut selected = "";
    for (edict, classname) in (1..=count).zip(CLASSNAMES.iter().cycle()) {
        if edict == middle {
            selected = classname;
        }
        let _ = entities.take_event(
            "edict/spawned",
            &json!({ "edict": edict, "classname": classname, "time": 0.0 }),
        );
    }
    entities.select(&json!({
        "edict": middle,
        "classname": selected,
        "fields": [],
    }));
    entities
}

/// One draw of the Entities outliner over `entities` with `filter` typed in it, in the dark
/// theme: what the left panel does each frame while the game runs.
pub fn outliner(ui: &mut Ui, entities: &Entities, filter: &str) {
    let Ok(theme) = Theme::load(Mode::Dark) else {
        return;
    };
    let mut state = Outliner::with_filter(filter);
    let _ = draw_outliner(ui, &theme, entities, &mut state);
}
