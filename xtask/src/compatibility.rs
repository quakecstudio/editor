// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask compatibility [--out <file>]`: the compatibility list, `docs/compatibility.md`
//! (P4-21, COMP-9), from the conformance suite.
//!
//! For each codebase of `corpus/manifest.toml` that may be used, fetched into the corpus cache
//! (`cargo xtask corpus fetch`):
//!
//! - **Build:** each module compiled with `qc-codegen`, as `codegen-corpus` compiles it, in the
//!   dialect the codebase's `quakec.toml` declares, or with none the vanilla dialect for id's
//!   1.01 and the FTEQCC dialect for every other codebase: `ok` with its statement count, or its
//!   first problem.
//! - **Behaviour:** whether the nightly compiler differential (`compiler-diff --full`) holds it to
//!   DIFF-12.5's bar against the reference compiler, from the lists it compares; otherwise what
//!   `corpus/compatibility.toml` records.
//! - **Issues:** the issues `corpus/compatibility.toml` links.
//!
//! The release workflow (`sign.yml`) runs it on every release tag and attaches the list.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use qc_project::Dialect;
use serde::Deserialize;

use crate::codegen_corpus;
use crate::compiler_diff;
use crate::corpus::manifest::Manifest;
use crate::differential::asset_root;

/// The list, relative to the workspace root.
const LIST: &str = "docs/compatibility.md";

/// `cargo xtask compatibility [--out <file>]`.
#[derive(clap::Parser, Debug)]
pub(crate) struct Args {
    /// Where to write the list; `docs/compatibility.md` by default.
    #[arg(long)]
    out: Option<PathBuf>,
}

/// `corpus/compatibility.toml`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Gaps {
    #[serde(default)]
    gap: Vec<Gap>,
}

/// What the list says of one codebase beyond what it measures.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Gap {
    codebase: String,
    behaviour: String,
    #[serde(default)]
    issues: Vec<u32>,
}

/// One codebase's row.
#[derive(Debug, PartialEq, Eq)]
struct Row {
    id: String,
    name: String,
    license: String,
    build: Build,
    behaviour: String,
    issues: Vec<u32>,
}

/// How a codebase builds with `qc-codegen`.
#[derive(Debug, PartialEq, Eq)]
enum Build {
    /// Not in the corpus cache.
    NotFetched,
    /// Fetched, but not a project the IDE opens on its own, and why.
    NotAProject(String),
    /// Each module, with its statement count or its first problem.
    Modules(Vec<codegen_corpus::ModuleBuild>),
}

impl Build {
    /// The cell of the list's Build column.
    fn cell(&self) -> String {
        match self {
            Self::NotFetched => "not fetched".to_owned(),
            Self::NotAProject(why) => format!("not opened on its own: {}", escape(why)),
            Self::Modules(modules) if modules.is_empty() => "no module".to_owned(),
            Self::Modules(modules) => modules
                .iter()
                .map(|(name, result)| match result {
                    Ok(statements) => format!("`{name}`: compiles, {statements} statements"),
                    Err(problem) => format!("`{name}`: does not compile: {}", escape(problem)),
                })
                .collect::<Vec<_>>()
                .join("<br>"),
        }
    }

    /// Whether every module compiled.
    fn ok(&self) -> bool {
        matches!(self, Self::Modules(modules)
            if !modules.is_empty() && modules.iter().all(|(_, result)| result.is_ok()))
    }
}

/// Writes the list; true when every fetched codebase's modules were read.
pub(crate) fn main(root: &Path, args: &Args) -> Result<bool, String> {
    let manifest = Manifest::load(&root.join("corpus/manifest.toml"))?;
    let gaps_path = root.join("corpus/compatibility.toml");
    let gaps: Gaps = toml::from_str(
        &fs::read_to_string(&gaps_path).map_err(|e| format!("{}: {e}", gaps_path.display()))?,
    )
    .map_err(|e| format!("{}: {e}", gaps_path.display()))?;
    let cache = asset_root()?;
    let codebases = cache.join("codebase");
    let compared = compiler_diff::nightly_codebases();
    let mut by_id: BTreeMap<&str, &Gap> = BTreeMap::new();
    for gap in &gaps.gap {
        if !manifest.codebase.iter().any(|c| c.id == gap.codebase) {
            return Err(format!(
                "corpus/compatibility.toml: {} is not a codebase of the manifest",
                gap.codebase
            ));
        }
        by_id.insert(gap.codebase.as_str(), gap);
    }
    let mut rows = Vec::new();
    for entry in manifest.codebase.iter().filter(|c| c.use_ != "never") {
        // A codebase built inside a tree of others (FreeCS in a Nuclide SDK) opens there.
        let in_tree = manifest.tree.iter().find_map(|tree| {
            tree.dirs
                .iter()
                .find(|(_, codebase)| **codebase == entry.id)
                .map(|(sub, _)| cache.join("tree").join(&tree.id).join(sub))
        });
        let dir = in_tree.unwrap_or_else(|| codebases.join(&entry.id));
        println!("compatibility: {}", entry.id);
        let build = if dir.is_dir() {
            let dialect = if entry.id == "id-1.01" {
                Dialect::Vanilla
            } else {
                Dialect::Fteqcc
            };
            match codegen_corpus::compile_codebase(&dir, dialect) {
                Ok(modules) => Build::Modules(
                    modules
                        .into_iter()
                        .map(|(name, result)| {
                            let why = |w: String| relative(&relative(&w, &dir), &cache);
                            (name, result.map_err(why))
                        })
                        .collect(),
                ),
                Err(why) => Build::NotAProject(why.rsplit(": ").next().unwrap_or(&why).to_owned()),
            }
        } else {
            Build::NotFetched
        };
        let gap = by_id.get(entry.id.as_str());
        let behaviour = match gap {
            Some(gap) => gap.behaviour.clone(),
            None if compared.contains(&entry.id.as_str()) => {
                "Matches the reference compiler's program in the nightly differential, at DIFF-12.5's bar."
                    .to_owned()
            }
            None => "Not compared yet.".to_owned(),
        };
        rows.push(Row {
            id: entry.id.clone(),
            name: entry._name.clone(),
            license: entry.license.clone(),
            build,
            behaviour,
            issues: gap.map(|g| g.issues.clone()).unwrap_or_default(),
        });
    }
    let out = args.out.clone().unwrap_or_else(|| root.join(LIST));
    fs::write(&out, render(&rows)).map_err(|e| format!("{}: {e}", out.display()))?;
    let built = rows.iter().filter(|row| row.build.ok()).count();
    let fetched = rows
        .iter()
        .filter(|row| row.build != Build::NotFetched)
        .count();
    println!(
        "compatibility: wrote {}: {built} of {fetched} fetched codebases compile, {} not fetched",
        out.display(),
        rows.len().saturating_sub(fetched)
    );
    Ok(true)
}

/// The list's text.
fn render(rows: &[Row]) -> String {
    let mut text = String::from(
        "# Compatibility\n\n\
         <!-- Generated by `cargo xtask compatibility` from the conformance corpus; do not edit by hand. -->\n\n\
         The QuakeC codebases of the conformance corpus (`corpus/manifest.toml`), and how each fares\n\
         with QuakeC Studio's compiler (COMP-9). **Build** is each module compiled with `qc-codegen`,\n\
         in the dialect the codebase's `quakec.toml` declares, or with none id's 1.01 in the\n\
         vanilla dialect and every other codebase in the FTEQCC dialect. **Behaviour**\n\
         is whether the nightly compiler differential runs our program and the reference compiler's\n\
         (id's qcc for 1.01, the pinned FTEQCC otherwise) on the same calls in the VM and finds them\n\
         the same, with at least 90% of the statements reached (DIFF-12.5). The release workflow\n\
         regenerates this list on every release; `corpus/compatibility.toml` records the known gaps.\n\n\
         | Codebase | License | Build | Behaviour | Issues |\n\
         | --- | --- | --- | --- | --- |\n",
    );
    for row in rows {
        let issues = if row.issues.is_empty() {
            String::new()
        } else {
            row.issues
                .iter()
                .map(|n| format!("[#{n}](https://github.com/quakecstudio/editor/issues/{n})"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        text.push_str(&format!(
            "| {} (`{}`) | {} | {} | {} | {} |\n",
            escape(&row.name),
            row.id,
            row.license,
            row.build.cell(),
            escape(&row.behaviour),
            issues
        ));
    }
    text
}

/// `problem` with the paths under `dir` written relative to it, so the list is the same
/// wherever the corpus cache is: a codebase's own files relative to it, and a file of another
/// codebase of its tree relative to the cache (`tree/freecs-tree/src/...`).
fn relative(problem: &str, dir: &Path) -> String {
    let prefix = format!("{}{}", dir.display(), std::path::MAIN_SEPARATOR);
    problem.replace(&prefix, "")
}

/// `text` safe in a table cell: `|` escaped and line breaks as spaces.
fn escape(text: &str) -> String {
    text.replace('|', "\\|").replace(['\r', '\n'], " ")
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use super::*;

    // No spec statement: P4-21, COMP-9: each row says how the codebase builds, how it behaves
    // and its issues, a cell's `|` escaped
    #[test]
    fn the_list_has_a_row_per_codebase() {
        let rows = [
            Row {
                id: "copper".to_owned(),
                name: "Copper".to_owned(),
                license: "GPL-2.0-or-later".to_owned(),
                build: Build::Modules(vec![
                    ("progs".to_owned(), Ok(1200)),
                    ("csprogs".to_owned(), Err("a.qc:3 E0001 x | y".to_owned())),
                ]),
                behaviour: "Matches.".to_owned(),
                issues: vec![193],
            },
            Row {
                id: "nzp".to_owned(),
                name: "NZP".to_owned(),
                license: "GPL-2.0-only".to_owned(),
                build: Build::NotFetched,
                behaviour: "Not compared yet.".to_owned(),
                issues: Vec::new(),
            },
        ];
        let text = render(&rows);
        assert!(
            text.contains(
                "| Copper (`copper`) | GPL-2.0-or-later | `progs`: compiles, 1200 statements<br>\
             `csprogs`: does not compile: a.qc:3 E0001 x \\| y | Matches. | \
             [#193](https://github.com/quakecstudio/editor/issues/193) |"
            ),
            "{text}"
        );
        assert!(
            text.contains("| NZP (`nzp`) | GPL-2.0-only | not fetched | Not compared yet. |  |")
        );
        assert!(!rows[0].build.ok() && !rows[1].build.ok());
        assert!(Build::Modules(vec![("progs".to_owned(), Ok(1))]).ok());
        assert!(!Build::Modules(Vec::new()).ok());
    }

    // No spec statement: P4-21: every codebase corpus/compatibility.toml names is one of the
    // manifest's, so a gap is never recorded against a codebase the list does not show
    #[test]
    fn every_gap_names_a_codebase_of_the_manifest() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let manifest = Manifest::load(&root.join("corpus/manifest.toml")).unwrap();
        let gaps: Gaps =
            toml::from_str(&fs::read_to_string(root.join("corpus/compatibility.toml")).unwrap())
                .unwrap();
        assert!(!gaps.gap.is_empty());
        for gap in &gaps.gap {
            assert!(
                manifest
                    .codebase
                    .iter()
                    .any(|c| c.id == gap.codebase && c.use_ != "never"),
                "{}",
                gap.codebase
            );
        }
    }

    // No spec statement: P4-21: a problem's path is relative to its codebase, so the list does
    // not depend on where the corpus cache is
    #[test]
    fn problems_name_paths_within_the_codebase() {
        let dir = Path::new("/cache/codebase/alkaline");
        let problem = format!("{}/weapons.qc:281 E0331 x", dir.display());
        assert_eq!(relative(&problem, dir), "weapons.qc:281 E0331 x");
        assert_eq!(relative("no path", dir), "no path");
    }

    #[test]
    fn other_arguments_are_a_usage_error() {
        for bad in [&["--out"][..], &["extra"]] {
            assert!(crate::parse_cli::<Args, _>(bad).is_err(), "{bad:?}");
        }
    }
}
