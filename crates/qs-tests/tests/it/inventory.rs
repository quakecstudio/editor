// SPDX-License-Identifier: GPL-3.0-or-later

//! The L2 upstream inventory (`docs/spec/inventory-l2.md`, P2-116): every row of its tables is
//! `ported` or `not modelled`, every statement ID a row names is an active statement
//! (or a section) of `docs/spec/requirements.tsv`, a `not modelled` row names the statement that
//! says why, and every case family a row names is a `fn` or `const` of the builtin differential
//! (`xtask/src/oracles/builtins.rs` and `builtins/*.rs`), or the row says `none` and why.

#![expect(
    clippy::expect_used,
    reason = "test code: a failed expect is a failed test with a message"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// What a row may cite: each statement's status by ID, the section IDs, and the text of the
/// differential's sources.
struct Known {
    statements: BTreeMap<String, String>,
    sections: BTreeSet<String>,
    differential: String,
}

impl Known {
    fn read(root: &Path) -> Self {
        let tsv = fs::read_to_string(root.join("docs/spec/requirements.tsv"))
            .expect("docs/spec/requirements.tsv");
        let mut statements = BTreeMap::new();
        let mut sections = BTreeSet::new();
        for line in tsv.lines().skip(1) {
            let mut fields = line.split('\t');
            let (Some(id), Some(_), Some(section), Some(status)) =
                (fields.next(), fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            statements.insert(id.to_owned(), status.to_owned());
            sections.insert(section.to_owned());
        }
        let oracles = root.join("xtask/src/oracles");
        let mut differential =
            fs::read_to_string(oracles.join("builtins.rs")).expect("builtins.rs");
        let mut family = fs::read_dir(oracles.join("builtins"))
            .expect("xtask/src/oracles/builtins")
            .map(|entry| entry.expect("an entry").path())
            .filter(|path| path.file_name().is_some_and(|name| name != "tests.rs"))
            .collect::<Vec<_>>();
        family.sort();
        for path in family {
            differential.push_str(&fs::read_to_string(&path).expect("a family"));
        }
        Self {
            statements,
            sections,
            differential,
        }
    }

    /// Why the statement or section `id` cannot be cited, if it cannot.
    fn refuse(&self, id: &str) -> Option<String> {
        match self.statements.get(id).map(String::as_str) {
            Some("active") => None,
            Some(status) => Some(format!("{id} is {status}")),
            None if self.sections.contains(id) => None,
            None => Some(format!("{id} is no statement or section")),
        }
    }

    /// Whether `name` is a `fn` or `const` of the differential.
    fn is_family(&self, name: &str) -> bool {
        [
            format!("fn {name}("),
            format!("const {name}:"),
            format!("fn {name}<"),
        ]
        .iter()
        .any(|form| self.differential.contains(form.as_str()))
    }
}

/// The cells of a table row, without the outer pipes.
fn cells(line: &str) -> Vec<String> {
    let inner = line.trim().trim_start_matches('|').trim_end_matches('|');
    inner
        .split(" | ")
        .map(|cell| cell.trim().to_owned())
        .collect()
}

/// The statement IDs a Statements cell names: each item, or both ends of an `A to B` range.
fn statement_ids(cell: &str) -> Vec<String> {
    let mut ids = Vec::new();
    for item in cell.split(", ") {
        for part in item.split(" to ") {
            let part = part.trim();
            if !part.is_empty() && part != "none" {
                ids.push(part.to_owned());
            }
        }
    }
    ids
}

/// The backticked names a Differential cell names.
fn family_names(cell: &str) -> Vec<String> {
    cell.split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

/// Every breach of the inventory's rules in `text`, the inventory `name`.
fn breaches(name: &str, text: &str, known: &Known) -> Vec<String> {
    let mut found = Vec::new();
    let mut header: Option<Vec<String>> = None;
    for (index, line) in text.lines().enumerate() {
        let at = format!("{name}:{}", index.saturating_add(1));
        if !line.starts_with('|') {
            header = None;
            continue;
        }
        let row = cells(line);
        let Some(columns) = &header else {
            header = Some(row);
            continue;
        };
        if row
            .iter()
            .all(|cell| cell.chars().all(|c| c == '-' || c == ' '))
        {
            continue;
        }
        if row.len() != columns.len() {
            found.push(format!(
                "{at}: {} cells, the header has {}",
                row.len(),
                columns.len()
            ));
            continue;
        }
        let cell = |title: &str| {
            columns
                .iter()
                .position(|column| column == title)
                .and_then(|i| row.get(i))
                .map(String::as_str)
        };
        let status = cell("Status").unwrap_or("");
        if status != "ported" && status != "not modelled" {
            found.push(format!("{at}: status {status:?}"));
        }
        if let Some(statements) = cell("Statements") {
            let ids = statement_ids(statements);
            if ids.is_empty() && status == "not modelled" {
                found.push(format!("{at}: not modelled with no statement"));
            }
            for id in ids {
                if let Some(why) = known.refuse(&id) {
                    found.push(format!("{at}: {why}"));
                }
            }
        }
        if let Some(differential) = cell("Differential") {
            if let Some(reason) = differential.strip_prefix("none") {
                if !reason.starts_with(": ") {
                    found.push(format!("{at}: a differential of none needs its reason"));
                }
            } else {
                let names = family_names(differential);
                if names.is_empty() {
                    found.push(format!("{at}: no case family"));
                }
                for family in names {
                    if !known.is_family(&family) {
                        found.push(format!("{at}: {family} is no case family"));
                    }
                }
            }
        }
    }
    found
}

#[test]
fn every_inventory_row_is_ported_or_not_modelled_and_proven() {
    let root = repo_root();
    let known = Known::read(&root);
    let text = fs::read_to_string(root.join("docs/spec/inventory-l2.md"))
        .expect("docs/spec/inventory-l2.md");
    let found = breaches("inventory-l2.md", &text, &known);
    assert!(
        found.is_empty(),
        "inventory breaches:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_inventory_check_finds_each_breach() {
    let known = Known::read(&repo_root());
    let table = |row: &str| {
        format!(
            "| Function | Rust | Statements | Differential | Status |\n| --- | --- | --- | --- | --- |\n{row}\n"
        )
    };
    let ok = table("| `f` | none | QC-1.1, FTEF-14.35 to FTEF-14.37 | `VALUES` | ported |");
    assert!(breaches("t", &ok, &known).is_empty());
    for (row, breach) in [
        ("| `f` | none | QC-1.1 | `VALUES` | unported |", "status"),
        (
            "| `f` | none | QC-1.99 | `VALUES` | ported |",
            "QC-1.99 is no statement",
        ),
        (
            "| `f` | none | FTEF-12.8 | `VALUES` | ported |",
            "FTEF-12.8 is withdrawn",
        ),
        (
            "| `f` | none | QC-1.1 | `NOT_A_FAMILY` | ported |",
            "is no case family",
        ),
        (
            "| `f` | none | QC-1.1 | none | not modelled |",
            "needs its reason",
        ),
        (
            "| `f` | none | none | none: x | not modelled |",
            "not modelled with no statement",
        ),
        ("| `f` | none | QC-1.1 | `VALUES` |", "cells"),
    ] {
        let found = breaches("t", &table(row), &known);
        assert!(found.iter().any(|f| f.contains(breach)), "{row}: {found:?}");
    }
    // A section ID is citable.
    assert!(
        breaches(
            "t",
            &table("| `f` | none | QC-6 | `VALUES` | ported |"),
            &known
        )
        .is_empty()
    );
}
