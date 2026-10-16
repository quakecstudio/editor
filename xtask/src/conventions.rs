// SPDX-License-Identifier: GPL-3.0-or-later

//! Checks the workspace configuration against `docs/conventions.md`.
//!
//! The document carries the configuration it requires in fenced blocks whose
//! info string names the file: ```` ```toml file=.rustfmt.toml exact ````. Every
//! key in such a block must have the same value in the file. With `exact`, a
//! table the block spells out with plain keys only must also have no other keys
//! in the file. The clippy lint table between the `conventions:clippy-deny`
//! markers lists every lint that is `deny`. The rest are structural rules from
//! the document that are not single values: pinned dependencies, inherited
//! package fields, lint inheritance and reasons on cargo-deny skips.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use toml::{Table, Value};

use crate::Report;

const DOC: &str = "docs/conventions.md";
const CLIPPY_TABLE_START: &str = "<!-- conventions:clippy-deny -->";
const CLIPPY_TABLE_END: &str = "<!-- /conventions:clippy-deny -->";
const MEMBER_DIRS: [&str; 2] = ["crates", "apps"];
const INHERITED_PACKAGE_FIELDS: [&str; 6] = [
    "version",
    "edition",
    "license",
    "rust-version",
    "repository",
    "publish",
];
const DEPENDENCY_TABLES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

/// A fenced TOML block in the conventions document.
#[derive(Debug, PartialEq, Eq)]
struct Block {
    line: usize,
    file: String,
    exact: bool,
    body: String,
}

pub(crate) fn check(root: &Path) -> Result<Report, String> {
    let doc = read(root, DOC)?;
    let cargo = read_toml(root, "Cargo.toml")?;
    let mut report = Report::default();

    let blocks = doc_blocks(&doc);
    if blocks.is_empty() {
        return Err(format!("{DOC} has no ```toml file=... blocks"));
    }
    let mut documented_clippy = BTreeSet::new();
    for block in &blocks {
        let want = parse_toml(&format!("{DOC}:{}", block.line), &block.body)?;
        if block.file == "Cargo.toml" {
            documented_clippy.extend(clippy_lints(&want).keys().cloned());
        }
        let actual = read_toml(root, &block.file)?;
        let mut problems = Vec::new();
        compare(&want, &actual, "", block.exact, &mut problems);
        let label = format!("{DOC}:{} matches {}", block.line, block.file);
        record_all(&mut report, &label, problems);
    }

    let denied = clippy_deny_table(&doc)?;
    documented_clippy.extend(denied.iter().cloned());
    record_all(
        &mut report,
        "every clippy lint in the docs table is deny in Cargo.toml",
        check_clippy_deny(&denied, &cargo),
    );
    record_all(
        &mut report,
        "every clippy lint in Cargo.toml is in the docs",
        clippy_lints(&cargo)
            .keys()
            .filter(|lint| !documented_clippy.contains(*lint))
            .map(|lint| format!("workspace.lints.clippy.{lint} is not in {DOC}"))
            .collect(),
    );
    record_all(
        &mut report,
        "workspace version is CalVer YYYY.MM.N",
        check_calver(&cargo),
    );
    let toolchain = read_toml(root, "rust-toolchain.toml")?;
    record_all(
        &mut report,
        "toolchain channel is exact and equals rust-version",
        check_toolchain(&toolchain, &cargo),
    );
    record_all(
        &mut report,
        "workspace dependencies are exact, default-features = false, git pinned to a rev",
        check_workspace_dependencies(&cargo),
    );
    let clippy_conf = read_toml(root, ".clippy.toml")?;
    record_all(
        &mut report,
        "every external dependency is in absolute-paths-allowed-crates",
        check_allowed_crates(&cargo, &clippy_conf),
    );
    let deny = read_toml(root, "deny.toml")?;
    record_all(
        &mut report,
        "every cargo-deny skip carries a reason",
        check_deny_skips(&deny),
    );
    for manifest in member_manifests(root)? {
        let crate_toml = read_toml(root, &manifest)?;
        record_all(
            &mut report,
            &format!("{manifest} inherits package fields, lints and dependencies"),
            check_member(&manifest, &crate_toml),
        );
    }
    Ok(report)
}

fn record_all(report: &mut Report, label: &str, problems: Vec<String>) {
    if problems.is_empty() {
        report.record(true, label.to_owned());
    }
    for problem in problems {
        report.record(false, problem);
    }
}

fn read(root: &Path, relative: &str) -> Result<String, String> {
    fs::read_to_string(root.join(relative)).map_err(|error| format!("{relative}: {error}"))
}

fn read_toml(root: &Path, relative: &str) -> Result<Table, String> {
    parse_toml(relative, &read(root, relative)?)
}

fn parse_toml(origin: &str, text: &str) -> Result<Table, String> {
    text.parse::<Table>()
        .map_err(|error| format!("{origin}: {error}"))
}

fn doc_blocks(doc: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut current: Option<Block> = None;
    for (line, text) in (1_usize..).zip(doc.lines()) {
        if let Some(block) = current.as_mut() {
            if text.trim_start().starts_with("```") {
                blocks.extend(current.take());
            } else {
                block.body.push_str(text);
                block.body.push('\n');
            }
            continue;
        }
        let Some(info) = text.trim_start().strip_prefix("```toml ") else {
            continue;
        };
        let mut file = None;
        let mut exact = false;
        for word in info.split_whitespace() {
            if let Some(name) = word.strip_prefix("file=") {
                file = Some(name.to_owned());
            } else if word == "exact" {
                exact = true;
            }
        }
        if let Some(file) = file {
            current = Some(Block {
                line,
                file,
                exact,
                body: String::new(),
            });
        }
    }
    blocks
}

/// Checks that every key in `want` has the same value in `actual`.
///
/// With `exact`, a table in `want` that holds no sub-tables must also have no
/// extra keys in `actual`.
fn compare(want: &Table, actual: &Table, prefix: &str, exact: bool, problems: &mut Vec<String>) {
    if exact && want.values().all(|value| !value.is_table()) {
        for key in actual.keys().filter(|key| !want.contains_key(*key)) {
            problems.push(format!("{prefix}{key}: in the file but not in {DOC}"));
        }
    }
    for (key, want_value) in want {
        let at = format!("{prefix}{key}");
        match (want_value, actual.get(key)) {
            (_, None) => problems.push(format!("{at}: in {DOC} but missing from the file")),
            (Value::Table(want_table), Some(Value::Table(actual_table))) => {
                compare(want_table, actual_table, &format!("{at}."), exact, problems);
            }
            (want_value, Some(actual_value)) => {
                if want_value != actual_value {
                    problems.push(format!(
                        "{at}: {DOC} says {want_value}, the file has {actual_value}"
                    ));
                }
            }
        }
    }
}

fn table<'a>(parent: &'a Table, path: &[&str]) -> Option<&'a Table> {
    let mut current = parent;
    for key in path {
        current = current.get(*key)?.as_table()?;
    }
    Some(current)
}

fn clippy_lints(cargo: &Table) -> Table {
    table(cargo, &["workspace", "lints", "clippy"])
        .cloned()
        .unwrap_or_default()
}

fn clippy_deny_table(doc: &str) -> Result<BTreeSet<String>, String> {
    let (Some(start), Some(end)) = (doc.find(CLIPPY_TABLE_START), doc.find(CLIPPY_TABLE_END))
    else {
        return Err(format!(
            "{DOC} has no {CLIPPY_TABLE_START} ... {CLIPPY_TABLE_END} lint table"
        ));
    };
    let Some(section) = doc.get(start..end) else {
        return Err(format!(
            "{DOC}: the lint table end marker comes before its start"
        ));
    };
    let lints: BTreeSet<String> = section
        .lines()
        .filter(|line| line.starts_with('|'))
        .flat_map(|line| line.split('`').skip(1).step_by(2))
        .filter(|word| {
            !word.is_empty()
                && word
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
        })
        .map(str::to_owned)
        .collect();
    if lints.is_empty() {
        return Err(format!("{DOC}: the clippy lint table names no lints"));
    }
    Ok(lints)
}

fn check_clippy_deny(denied: &BTreeSet<String>, cargo: &Table) -> Vec<String> {
    let actual = clippy_lints(cargo);
    let mut problems = Vec::new();
    for lint in denied {
        match actual.get(lint).and_then(Value::as_str) {
            Some("deny") => {}
            Some(level) => problems.push(format!(
                "workspace.lints.clippy.{lint}: {DOC} says deny, Cargo.toml has {level}"
            )),
            None => problems.push(format!(
                "workspace.lints.clippy.{lint}: denied in {DOC}, missing from Cargo.toml"
            )),
        }
    }
    for (lint, level) in &actual {
        if level.as_str() == Some("deny") && !denied.contains(lint) {
            problems.push(format!(
                "workspace.lints.clippy.{lint} is deny in Cargo.toml but not in the {DOC} table"
            ));
        }
    }
    problems
}

fn package_str<'a>(cargo: &'a Table, key: &str) -> Option<&'a str> {
    table(cargo, &["workspace", "package"])?.get(key)?.as_str()
}

fn check_calver(cargo: &Table) -> Vec<String> {
    let Some(version) = package_str(cargo, "version") else {
        return vec!["workspace.package.version is missing".to_owned()];
    };
    let parts: Vec<Option<u32>> = version.split('.').map(|part| part.parse().ok()).collect();
    let valid = match parts.as_slice() {
        [Some(year), Some(month), Some(_)] => {
            (2026..=9999).contains(year) && (1..=12).contains(month)
        }
        _ => false,
    };
    if valid {
        Vec::new()
    } else {
        vec![format!(
            "workspace.package.version {version} is not CalVer YYYY.MM.N"
        )]
    }
}

fn is_exact_rust_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

fn check_toolchain(toolchain: &Table, cargo: &Table) -> Vec<String> {
    let channel = table(toolchain, &["toolchain"])
        .and_then(|section| section.get("channel"))
        .and_then(Value::as_str);
    let Some(channel) = channel else {
        return vec!["rust-toolchain.toml: toolchain.channel is missing".to_owned()];
    };
    let mut problems = Vec::new();
    if !is_exact_rust_version(channel) {
        problems.push(format!(
            "rust-toolchain.toml: channel {channel} is not an exact x.y.z release"
        ));
    }
    match package_str(cargo, "rust-version") {
        Some(rust_version) if rust_version == channel => {}
        Some(rust_version) => problems.push(format!(
            "workspace.package.rust-version {rust_version} differs from the toolchain {channel}"
        )),
        None => problems.push("workspace.package.rust-version is missing".to_owned()),
    }
    problems
}

fn check_workspace_dependencies(cargo: &Table) -> Vec<String> {
    let Some(dependencies) = table(cargo, &["workspace", "dependencies"]) else {
        return Vec::new();
    };
    let mut problems = Vec::new();
    for (name, spec) in dependencies {
        let at = format!("workspace.dependencies.{name}");
        let Some(spec) = spec.as_table() else {
            problems.push(format!(
                "{at}: use a table with an exact version and default-features = false"
            ));
            continue;
        };
        if spec.contains_key("path") {
            continue;
        }
        if spec.contains_key("git") {
            if !spec.contains_key("rev") {
                problems.push(format!("{at}: a git source must be pinned to a rev"));
            }
        } else {
            match spec.get("version").and_then(Value::as_str) {
                Some(version)
                    if version
                        .strip_prefix('=')
                        .is_some_and(|rest| !rest.contains([',', '*'])) => {}
                Some(version) => problems.push(format!(
                    "{at}: version {version:?} is not an exact \"=x.y.z\" requirement"
                )),
                None => problems.push(format!("{at}: no version")),
            }
        }
        if spec.get("default-features").and_then(Value::as_bool) != Some(false) {
            problems.push(format!("{at}: default-features must be false"));
        }
    }
    problems
}

fn check_allowed_crates(cargo: &Table, clippy_conf: &Table) -> Vec<String> {
    let allowed: BTreeSet<&str> = clippy_conf
        .get("absolute-paths-allowed-crates")
        .and_then(Value::as_array)
        .map(|crates| crates.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let Some(dependencies) = table(cargo, &["workspace", "dependencies"]) else {
        return Vec::new();
    };
    dependencies
        .iter()
        .filter(|(_, spec)| {
            spec.as_table()
                .is_none_or(|spec| !spec.contains_key("path"))
        })
        .map(|(name, _)| name.replace('-', "_"))
        .filter(|name| !allowed.contains(name.as_str()))
        .map(|name| format!(".clippy.toml: absolute-paths-allowed-crates is missing {name}"))
        .collect()
}

fn check_deny_skips(deny: &Table) -> Vec<String> {
    let mut problems = Vec::new();
    for key in ["skip", "skip-tree"] {
        let entries = table(deny, &["bans"])
            .and_then(|bans| bans.get(key))
            .and_then(Value::as_array);
        for entry in entries.into_iter().flatten() {
            let reason = entry
                .as_table()
                .and_then(|entry| entry.get("reason"))
                .and_then(Value::as_str);
            if reason.is_none_or(|reason| reason.trim().is_empty()) {
                problems.push(format!("deny.toml: bans.{key} entry {entry} has no reason"));
            }
        }
    }
    problems
}

fn member_manifests(root: &Path) -> Result<Vec<String>, String> {
    let mut manifests = Vec::new();
    for dir in MEMBER_DIRS {
        let Ok(entries) = fs::read_dir(root.join(dir)) else {
            continue;
        };
        for entry in entries {
            let entry = entry.map_err(|error| format!("{dir}: {error}"))?;
            let name = entry.file_name();
            let manifest = format!("{dir}/{}/Cargo.toml", name.to_string_lossy());
            if root.join(&manifest).is_file() {
                manifests.push(manifest);
            }
        }
    }
    manifests.push("xtask/Cargo.toml".to_owned());
    manifests.sort();
    Ok(manifests)
}

fn inherits(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_table)
        .and_then(|spec| spec.get("workspace"))
        .and_then(Value::as_bool)
        == Some(true)
}

fn check_member(manifest: &str, crate_toml: &Table) -> Vec<String> {
    let mut problems = Vec::new();
    let package = table(crate_toml, &["package"]);
    for field in INHERITED_PACKAGE_FIELDS {
        if !inherits(package.and_then(|package| package.get(field))) {
            problems.push(format!(
                "{manifest}: package.{field} must be {{ workspace = true }}"
            ));
        }
    }
    if !inherits(crate_toml.get("lints")) {
        problems.push(format!("{manifest}: [lints] must be workspace = true"));
    }
    let mut sections: Vec<(String, &Table)> = Vec::new();
    for key in DEPENDENCY_TABLES {
        if let Some(deps) = table(crate_toml, &[key]) {
            sections.push((key.to_owned(), deps));
        }
    }
    if let Some(targets) = table(crate_toml, &["target"]) {
        for (cfg, target) in targets {
            for key in DEPENDENCY_TABLES {
                if let Some(deps) = target.as_table().and_then(|target| table(target, &[key])) {
                    sections.push((format!("target.{cfg}.{key}"), deps));
                }
            }
        }
    }
    for (section, deps) in sections {
        for (name, spec) in deps {
            if !inherits(Some(spec)) {
                problems.push(format!(
                    "{manifest}: {section}.{name} must use workspace = true"
                ));
            }
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "test code: a failed unwrap or index is a failed test"
    )]

    use super::*;

    fn toml(text: &str) -> Table {
        parse_toml("test", text).unwrap()
    }

    fn compared(want: &str, actual: &str, exact: bool) -> Vec<String> {
        let mut problems = Vec::new();
        compare(&toml(want), &toml(actual), "", exact, &mut problems);
        problems
    }

    #[test]
    fn doc_blocks_reads_file_and_exact_and_skips_other_fences() {
        let doc = "text\n```toml\nplain = 1\n```\n```toml file=a.toml exact\nx = 1\n```\n\
                   ```rust\nfn f() {}\n```\n  ```toml file=b.toml\n[t]\ny = 2\n```\n";
        let blocks = doc_blocks(doc);
        assert_eq!(
            blocks,
            vec![
                Block {
                    line: 5,
                    file: "a.toml".to_owned(),
                    exact: true,
                    body: "x = 1\n".to_owned()
                },
                Block {
                    line: 11,
                    file: "b.toml".to_owned(),
                    exact: false,
                    body: "[t]\ny = 2\n".to_owned()
                },
            ]
        );
    }

    #[test]
    fn compare_reports_missing_and_different_values() {
        let problems = compared("a = 1\nb = 2\n[t]\nc = 3\n", "a = 1\nb = 3\n[t]\n", false);
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems[0].starts_with("b: "));
        assert!(problems[1].starts_with("t.c: "));
    }

    #[test]
    fn compare_subset_allows_extra_keys() {
        assert!(compared("a = 1", "a = 1\nb = 2", false).is_empty());
    }

    #[test]
    fn compare_exact_rejects_extra_keys_in_leaf_tables_only() {
        assert_eq!(compared("a = 1", "a = 1\nb = 2", true).len(), 1);
        let nested = compared("[t]\na = 1\n", "x = 0\n[t]\na = 1\n", true);
        assert!(nested.is_empty(), "{nested:?}");
        assert_eq!(
            compared("[t]\na = 1\n", "[t]\na = 1\nz = 2\n", true).len(),
            1
        );
    }

    #[test]
    fn compare_arrays_must_match_exactly() {
        assert_eq!(compared("a = [1, 2]", "a = [1, 2, 3]", false).len(), 1);
    }

    #[test]
    fn clippy_table_collects_backticked_lint_names() {
        let doc = format!(
            "{CLIPPY_TABLE_START}\n| Group | Lints |\n| --- | --- |\n\
             | Panics | `unwrap_used`, `panic` |\n| Paths | `absolute_paths` (in `.clippy.toml`) |\n\
             {CLIPPY_TABLE_END}\n`outside`"
        );
        let lints = clippy_deny_table(&doc).unwrap();
        let expected: BTreeSet<String> = ["absolute_paths", "panic", "unwrap_used"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        assert_eq!(lints, expected);
    }

    #[test]
    fn clippy_table_missing_or_empty_is_an_error() {
        assert!(clippy_deny_table("no table").is_err());
        let empty = format!("{CLIPPY_TABLE_START}\n{CLIPPY_TABLE_END}");
        assert!(clippy_deny_table(&empty).is_err());
        let reversed = format!("{CLIPPY_TABLE_END}\n{CLIPPY_TABLE_START}");
        assert!(clippy_deny_table(&reversed).is_err());
    }

    #[test]
    fn clippy_deny_is_checked_both_ways() {
        let denied: BTreeSet<String> = ["unwrap_used", "panic"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let cargo = toml(
            "[workspace.lints.clippy]\nunwrap_used = \"deny\"\npanic = \"warn\"\n\
             todo = \"deny\"\nsimilar_names = \"allow\"\n",
        );
        let problems = check_clippy_deny(&denied, &cargo);
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems[0].contains("panic"));
        assert!(problems[1].contains("todo"));
    }

    #[test]
    fn calver_accepts_year_month_n_only() {
        let version = |v: &str| toml(&format!("[workspace.package]\nversion = \"{v}\""));
        assert!(check_calver(&version("2026.10.0")).is_empty());
        for bad in ["0.1.0", "2026.13.0", "2026.10", "2026.10.x"] {
            assert_eq!(check_calver(&version(bad)).len(), 1, "{bad}");
        }
        assert_eq!(check_calver(&toml("")).len(), 1);
    }

    #[test]
    fn toolchain_must_be_exact_and_match_rust_version() {
        let cargo = toml("[workspace.package]\nrust-version = \"1.99.0\"");
        let channel = |c: &str| toml(&format!("[toolchain]\nchannel = \"{c}\""));
        assert!(check_toolchain(&channel("1.99.0"), &cargo).is_empty());
        assert_eq!(check_toolchain(&channel("stable"), &cargo).len(), 2);
        assert_eq!(check_toolchain(&channel("1.98.0"), &cargo).len(), 1);
        assert_eq!(check_toolchain(&toml(""), &cargo).len(), 1);
    }

    #[test]
    fn workspace_dependencies_must_be_exact_and_without_default_features() {
        let cargo = toml(
            "[workspace.dependencies]\n\
             good = { version = \"=1.2.3\", default-features = false }\n\
             local = { path = \"crates/local\" }\n\
             pinned = { git = \"https://example.com/x\", rev = \"abc\", default-features = false }\n\
             caret = { version = \"1.2.3\", default-features = false }\n\
             star = { version = \"*\", default-features = false }\n\
             range = { version = \"=1.2, <2\", default-features = false }\n\
             defaults = { version = \"=1.0.0\" }\n\
             unpinned = { git = \"https://example.com/y\", default-features = false }\n\
             bare = \"1\"\n\
             noversion = { default-features = false }\n",
        );
        let problems = check_workspace_dependencies(&cargo);
        let flagged: BTreeSet<&str> = problems
            .iter()
            .map(|p| p.split([':', '.']).nth(2).unwrap())
            .collect();
        let expected: BTreeSet<&str> = [
            "bare",
            "caret",
            "defaults",
            "noversion",
            "range",
            "star",
            "unpinned",
        ]
        .into_iter()
        .collect();
        assert_eq!(flagged, expected, "{problems:?}");
    }

    #[test]
    fn external_dependencies_must_be_allowed_absolute_paths() {
        let cargo = toml(
            "[workspace.dependencies]\nserde-json = { version = \"=1.0.0\" }\n\
             toml = { version = \"=1.0.0\" }\nqs-x = { path = \"crates/qs-x\" }\n",
        );
        let conf = toml("absolute-paths-allowed-crates = [\"std\", \"toml\"]");
        assert_eq!(
            check_allowed_crates(&cargo, &conf),
            vec![".clippy.toml: absolute-paths-allowed-crates is missing serde_json"]
        );
    }

    #[test]
    fn deny_skips_need_a_reason() {
        let deny = toml(
            "[bans]\nskip = [{ crate = \"a\", reason = \"why\" }, { crate = \"b\" }]\n\
             skip-tree = [{ crate = \"c\", reason = \" \" }]\n",
        );
        assert_eq!(check_deny_skips(&deny).len(), 2);
        assert!(check_deny_skips(&toml("")).is_empty());
    }

    #[test]
    fn rust_versions_are_three_numbers() {
        assert!(is_exact_rust_version("1.99.0"));
        for bad in ["1.99", "1.99.0.1", "1..0", "1.x.0", ""] {
            assert!(!is_exact_rust_version(bad), "{bad}");
        }
    }

    #[test]
    fn the_repository_follows_its_conventions() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let report = check(root).unwrap();
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        for label in [
            "docs/conventions.md:",
            "every clippy lint in Cargo.toml is in the docs",
            "toolchain channel is exact and equals rust-version",
            "xtask/Cargo.toml inherits package fields, lints and dependencies",
            "crates/qs-paths/Cargo.toml inherits package fields, lints and dependencies",
        ] {
            assert!(
                report.passed.iter().any(|line| line.starts_with(label)),
                "{label}: {:?}",
                report.passed
            );
        }
    }

    #[test]
    fn every_problem_is_recorded_and_a_clean_check_passes() {
        let mut report = Report::default();
        record_all(&mut report, "clean", Vec::new());
        record_all(
            &mut report,
            "dirty",
            vec!["one".to_owned(), "two".to_owned()],
        );
        assert_eq!(report.passed, ["clean"]);
        assert_eq!(report.problems, ["one", "two"]);
    }

    #[test]
    fn members_inherit_fields_lints_and_dependencies() {
        let good = toml(
            "[package]\nname = \"x\"\nversion.workspace = true\nedition.workspace = true\n\
             license.workspace = true\nrust-version.workspace = true\n\
             repository.workspace = true\npublish.workspace = true\n\
             [dependencies]\na = { workspace = true, features = [\"f\"] }\n\
             [target.'cfg(windows)'.dependencies]\nb = { workspace = true }\n\
             [lints]\nworkspace = true\n",
        );
        assert!(check_member("x", &good).is_empty());

        let bad = toml(
            "[package]\nname = \"x\"\nversion = \"1.0.0\"\nedition.workspace = true\n\
             license.workspace = true\nrust-version.workspace = true\n\
             repository.workspace = true\npublish.workspace = true\n\
             [dev-dependencies]\na = \"1\"\n\
             [target.'cfg(unix)'.build-dependencies]\nb = { version = \"=1.0.0\" }\n",
        );
        let problems = check_member("x", &bad);
        assert_eq!(problems.len(), 4, "{problems:?}");
    }
}
