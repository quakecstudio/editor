// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask next-edit-corpus <codebase>...`: next-edit suggestions on a corpus codebase
//! (P4-11, UI-7). In each codebase a field of the mod's own is typed over where it is first
//! declared, through `file.update` as the editor sends its text, and `edit.next` must then
//! suggest every other place `symbol.references` found for the old name; each is accepted
//! with `edit.accept_next`, and the new name must then have as many references as the old one
//! had. Nothing is saved.
//!
//! The codebases come from `cargo xtask corpus fetch id-1.01 rerelease`.

use std::path::Path;

use qs_workbench::next_edit::{ACCEPT_NEXT_EDIT, NEXT_EDIT};
use qs_workbench::{
    Config, FsLoader, OPEN_PROJECT, READ_FILE, REFERENCES, UPDATE_FILE, Workbench, register,
};
use serde_json::{Value, json};

use crate::entity_corpus::dialect_of;

/// The file the field is declared in, the field, and what it is renamed to: a field id's
/// QuakeC and the rerelease's both declare after `end_sys_fields`.
const FILE: &str = "defs.qc";
const FIELD: &str = "attack_finished";
const RENAMED: &str = "attack_done";

/// `cargo xtask next-edit-corpus`.
#[derive(clap::Parser, Debug)]
pub(crate) struct Args {
    /// Each codebase directory.
    #[arg(required = true)]
    codebases: Vec<std::path::PathBuf>,
}

pub(crate) fn main(_root: &Path, args: &Args) -> Result<bool, String> {
    let mut ok = true;
    for dir in &args.codebases {
        match walk(dir) {
            Ok(line) => println!("ok    {}: {line}", dir.display()),
            Err(why) => {
                println!("FAIL  {}: {why}", dir.display());
                ok = false;
            }
        }
    }
    Ok(ok)
}

/// How many places an answer of `symbol.references` holds.
fn places(answer: &Value) -> usize {
    answer.as_array().map_or(0, Vec::len)
}

/// Renames [`FIELD`] in `dir` by typing over its declaration and accepting each suggestion.
fn walk(dir: &Path) -> Result<String, String> {
    let mut registry = qs_commands::Registry::new();
    register(&mut registry).map_err(|e| e.to_string())?;
    let config = Config {
        dialect: Some(dialect_of(dir)?),
        ..Config::default()
    };
    let mut session = qs_commands::Session::new(Workbench::new(FsLoader, config), registry);
    let mut run = |id: &str, params: Value| {
        session
            .execute(id, params)
            .map_err(|e| format!("{id}: {e}"))
    };
    run(OPEN_PROJECT, json!({ "path": dir }))?;
    let path = dir.join(FILE);
    let read = run(READ_FILE, json!({ "path": path }))?;
    let text = read
        .get("text")
        .and_then(Value::as_str)
        .ok_or("file.read answered no text")?;
    let offset = text
        .find(FIELD)
        .ok_or_else(|| format!("{FILE} declares no {FIELD}"))?;
    let before = places(&run(REFERENCES, json!({ "path": path, "offset": offset }))?);
    let typed = text.replacen(FIELD, RENAMED, 1);
    run(UPDATE_FILE, json!({ "path": path, "text": typed }))?;
    let next = run(NEXT_EDIT, json!({}))?;
    let left = next.get("left").and_then(Value::as_u64).unwrap_or(0);
    if usize::try_from(left).ok() != before.checked_sub(1) {
        return Err(format!(
            "{FIELD} is written in {before} places, and {left} were suggested once its \
             declaration was typed over"
        ));
    }
    let mut files = std::collections::BTreeSet::new();
    let mut accepted = 0_usize;
    while let Some(done) = run(ACCEPT_NEXT_EDIT, json!({}))?.as_object() {
        if let Some(file) = done.get("path").and_then(Value::as_str) {
            files.insert(file.to_owned());
        }
        accepted = accepted.saturating_add(1);
    }
    let after = places(&run(REFERENCES, json!({ "path": path, "offset": offset }))?);
    if accepted.checked_add(1) != Some(before) || after != before {
        return Err(format!(
            "{accepted} suggestions were accepted of {before} places, and {RENAMED} is then \
             written in {after}"
        ));
    }
    Ok(format!(
        "{FIELD} typed over as {RENAMED}: {accepted} places suggested and accepted, in {} files",
        files.len()
    ))
}
