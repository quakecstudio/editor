// SPDX-License-Identifier: GPL-3.0-or-later

//! Checks the crate layering in `layers.toml` (ARCH-1, `docs/layers.md`).
//!
//! Every workspace member must be in `layers.toml`, every entry there must be a
//! member, and a crate may depend only on crates in its own layer or a lower
//! one. Layers rank `L0` to `L7`, then `apps`. `tests` and `tooling` crates
//! sit outside the ranking: they may depend on any crate, and no crate may
//! depend on them. Normal, dev and build dependencies all count, including
//! target-specific ones.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use toml::{Table, Value};

use crate::Report;

pub(crate) const FILE: &str = "layers.toml";
const RANKED: [&str; 9] = ["L0", "L1", "L2", "L3", "L4", "L5", "L6", "L7", "apps"];
const UNRANKED: [&str; 2] = ["tests", "tooling"];
const DEPENDENCY_TABLES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

/// Where a layer sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    /// Position in [`RANKED`]: a crate may depend on its own rank or lower.
    Ranked(usize),
    /// `tests` or `tooling`: may depend on anything; nothing may depend on it.
    Unranked,
}

fn rank(layer: &str) -> Option<Rank> {
    if let Some(position) = RANKED.iter().position(|ranked| *ranked == layer) {
        Some(Rank::Ranked(position))
    } else if UNRANKED.contains(&layer) {
        Some(Rank::Unranked)
    } else {
        None
    }
}

/// A workspace member and the workspace crates it depends on.
#[derive(Debug)]
struct Member {
    manifest: String,
    dependencies: BTreeSet<String>,
}

pub(crate) fn check(root: &Path) -> Result<Report, String> {
    let layers = read_toml(root, FILE)?;
    let Some(crates) = layers.get("crates").and_then(Value::as_table) else {
        return Err(format!("{FILE} has no [crates] table"));
    };
    let members = members(root)?;
    let mut report = Report::default();

    let mut assigned = BTreeMap::new();
    for (name, layer) in crates {
        let Some(layer) = layer.as_str() else {
            report
                .problems
                .push(format!("{FILE}: {name} must map to a layer name"));
            continue;
        };
        let Some(rank) = rank(layer) else {
            report.problems.push(format!(
                "{FILE}: {name} is in unknown layer {layer:?} (layers: {}, {})",
                RANKED.join(", "),
                UNRANKED.join(", ")
            ));
            continue;
        };
        if !members.contains_key(name) {
            report.problems.push(format!(
                "{FILE}: {name} is not a workspace member; remove it"
            ));
        }
        assigned.insert(name.as_str(), (layer, rank));
    }

    for (name, member) in &members {
        let Some(&(layer, rank)) = assigned.get(name.as_str()) else {
            if crates.contains_key(name) {
                continue;
            }
            report.problems.push(format!(
                "{}: crate {name} is missing from {FILE}; add it to a layer",
                member.manifest
            ));
            continue;
        };
        let mut ok = true;
        for dependency in &member.dependencies {
            let Some(&(dep_layer, dep_rank)) = assigned.get(dependency.as_str()) else {
                continue;
            };
            let allowed = match (rank, dep_rank) {
                (_, Rank::Unranked) => false,
                (Rank::Unranked, Rank::Ranked(_)) => true,
                (Rank::Ranked(own), Rank::Ranked(other)) => other <= own,
            };
            if !allowed {
                ok = false;
                report.problems.push(format!(
                    "{name} ({layer}) depends on {dependency} ({dep_layer}): a crate may depend \
                     only on crates in its own layer or lower, and never on tests or tooling"
                ));
            }
        }
        if ok {
            report.passed.push(format!(
                "{name} ({layer}) depends on {} workspace crates, none above it",
                member.dependencies.len()
            ));
        }
    }
    Ok(report)
}

/// The workspace members by package name, with the member crates each depends on.
fn members(root: &Path) -> Result<BTreeMap<String, Member>, String> {
    let manifest = read_toml(root, "Cargo.toml")?;
    let workspace = manifest
        .get("workspace")
        .and_then(Value::as_table)
        .ok_or("Cargo.toml has no [workspace]")?;
    let excluded: BTreeSet<&str> = strings(workspace.get("exclude")).collect();
    let mut dirs = Vec::new();
    for pattern in strings(workspace.get("members")) {
        if let Some(parent) = pattern.strip_suffix("/*") {
            let path = root.join(parent);
            let entries =
                fs::read_dir(&path).map_err(|error| format!("{}: {error}", path.display()))?;
            for entry in entries {
                let entry = entry.map_err(|error| format!("{}: {error}", path.display()))?;
                if entry.path().join("Cargo.toml").is_file() {
                    dirs.push(format!("{parent}/{}", entry.file_name().to_string_lossy()));
                }
            }
        } else {
            dirs.push(pattern.to_owned());
        }
    }
    let renames = workspace
        .get("dependencies")
        .and_then(Value::as_table)
        .map(|deps| {
            deps.iter()
                .map(|(key, value)| (key.clone(), package_name(key, value)))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();

    let mut manifests = BTreeMap::new();
    for dir in dirs.iter().filter(|dir| !excluded.contains(dir.as_str())) {
        let path = format!("{dir}/Cargo.toml");
        let manifest = read_toml(root, &path)?;
        let Some(name) = manifest
            .get("package")
            .and_then(|package| package.get("name"))
            .and_then(Value::as_str)
        else {
            return Err(format!("{path} has no package name"));
        };
        manifests.insert(name.to_owned(), (path, manifest));
    }
    let mut members = BTreeMap::new();
    for (name, (path, manifest)) in &manifests {
        let mut tables: Vec<&Table> = DEPENDENCY_TABLES
            .iter()
            .filter_map(|key| manifest.get(*key).and_then(Value::as_table))
            .collect();
        if let Some(targets) = manifest.get("target").and_then(Value::as_table) {
            for target in targets.values() {
                tables.extend(
                    DEPENDENCY_TABLES
                        .iter()
                        .filter_map(|key| target.get(*key).and_then(Value::as_table)),
                );
            }
        }
        let mut dependencies = BTreeSet::new();
        for table in tables {
            for (key, value) in table {
                let inherited = value.get("workspace").and_then(Value::as_bool) == Some(true);
                let package = match (inherited, renames.get(key)) {
                    (true, Some(package)) => package.clone(),
                    (true, None) | (false, _) => package_name(key, value),
                };
                if manifests.contains_key(&package) {
                    dependencies.insert(package);
                }
            }
        }
        members.insert(
            name.clone(),
            Member {
                manifest: path.clone(),
                dependencies,
            },
        );
    }
    Ok(members)
}

/// The package a dependency entry names: its `package` key, or the key itself.
fn package_name(key: &str, value: &Value) -> String {
    value
        .get("package")
        .and_then(Value::as_str)
        .unwrap_or(key)
        .to_owned()
}

fn strings(value: Option<&Value>) -> impl Iterator<Item = &str> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
}

fn read_toml(root: &Path, path: &str) -> Result<Table, String> {
    fs::read_to_string(root.join(path))
        .map_err(|error| format!("{path}: {error}"))?
        .parse::<Table>()
        .map_err(|error| format!("{path}: {error}"))
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "test code: a failed unwrap or index is a failed test"
    )]

    use std::path::PathBuf;

    use super::*;

    /// One crate in a fixture workspace: name, directory, layer (`None` =
    /// missing from layers.toml) and extra manifest text (dependencies).
    struct Spec {
        name: &'static str,
        layer: Option<&'static str>,
        manifest: &'static str,
    }

    fn krate(name: &'static str, layer: &'static str, manifest: &'static str) -> Spec {
        Spec {
            name,
            layer: Some(layer),
            manifest,
        }
    }

    fn workspace(name: &str, specs: &[Spec], extra_layers: &str) -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("target")
            .join("xtask-tests")
            .join("layers")
            .join(name);
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        fs::create_dir_all(root.join("crates")).unwrap();
        fs::create_dir_all(root.join("fuzz")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            format!(
                "[workspace]\nmembers = [\"crates/*\"{}]\nexclude = [\"fuzz\"]\n\
                 [workspace.dependencies]\nrenamed-c = {{ path = \"crates/c\", package = \"c\" }}\n",
                if specs.iter().any(|spec| spec.name == "xtask") {
                    ", \"xtask\""
                } else {
                    ""
                }
            ),
        )
        .unwrap();
        let mut layers = String::from("[crates]\n");
        for spec in specs {
            let dir = if spec.name == "xtask" {
                root.join("xtask")
            } else {
                root.join("crates").join(spec.name)
            };
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join("Cargo.toml"),
                format!("[package]\nname = \"{}\"\n{}", spec.name, spec.manifest),
            )
            .unwrap();
            if let Some(layer) = spec.layer {
                layers.push_str(&format!("{} = \"{layer}\"\n", spec.name));
            }
        }
        layers.push_str(extra_layers);
        fs::write(root.join(FILE), layers).unwrap();
        root
    }

    fn problems(name: &str, specs: &[Spec]) -> Vec<String> {
        check(&workspace(name, specs, "")).unwrap().problems
    }

    const ON_C: &str = "[dependencies]\nc = { path = \"../c\" }\n";

    #[test]
    fn downward_and_same_layer_dependencies_pass() {
        let specs = [
            krate("a", "L0", ""),
            krate(
                "b",
                "L1",
                "[dependencies]\na = { path = \"../a\" }\nserde = \"1\"\n",
            ),
            krate("c", "L1", "[dependencies]\nb = { path = \"../b\" }\n"),
            krate("app", "apps", ON_C),
            krate(
                "it",
                "tests",
                "[dev-dependencies]\napp = { path = \"../app\" }\n",
            ),
            krate("xtask", "tooling", ""),
        ];
        assert_eq!(problems("ok", &specs), Vec::<String>::new());
    }

    #[test]
    fn an_l1_crate_depending_on_an_l3_crate_fails_naming_both() {
        let specs = [krate("vm", "L1", ON_C), krate("c", "L3", "")];
        let found = problems("upward", &specs);
        assert_eq!(found.len(), 1);
        assert!(
            found[0].starts_with("vm (L1) depends on c (L3)"),
            "{found:?}"
        );
    }

    #[test]
    fn every_dependency_kind_counts() {
        for (name, manifest) in [
            ("dev", "[dev-dependencies]\nc = { path = \"../c\" }\n"),
            ("build", "[build-dependencies]\nc = { path = \"../c\" }\n"),
            (
                "target",
                "[target.'cfg(unix)'.dependencies]\nc = { path = \"../c\" }\n",
            ),
            (
                "renamed",
                "[dependencies]\nrenamed-c = { workspace = true }\n",
            ),
            (
                "package",
                "[dependencies]\nother = { path = \"../c\", package = \"c\" }\n",
            ),
        ] {
            let specs = [krate("vm", "L1", manifest), krate("c", "L3", "")];
            assert_eq!(problems(name, &specs).len(), 1, "{name}");
        }
    }

    #[test]
    fn nothing_may_depend_on_tests_or_tooling() {
        let specs = [krate("app", "apps", ON_C), krate("c", "tooling", "")];
        assert_eq!(problems("on-tooling", &specs).len(), 1);
    }

    #[test]
    fn a_crate_missing_from_layers_toml_fails() {
        let specs = [
            krate("a", "L0", ""),
            Spec {
                name: "b",
                layer: None,
                manifest: "",
            },
        ];
        let found = problems("missing", &specs);
        assert_eq!(found.len(), 1);
        assert!(
            found[0].contains("crate b is missing from layers.toml"),
            "{found:?}"
        );
    }

    #[test]
    fn unknown_layers_and_stale_entries_fail() {
        let root = workspace("stale", &[krate("a", "L9", "")], "gone = \"L0\"\nnum = 3\n");
        let found = check(&root).unwrap().problems;
        assert_eq!(found.len(), 3, "{found:?}");
    }

    #[test]
    fn a_layers_file_without_crates_is_an_error() {
        let root = workspace("no-table", &[], "");
        fs::write(root.join(FILE), "[other]\n").unwrap();
        assert!(check(&root).is_err());
        fs::remove_file(root.join(FILE)).unwrap();
        assert!(check(&root).is_err());
    }

    #[test]
    fn excluded_directories_are_not_members() {
        let root = workspace("excluded", &[krate("a", "L0", "")], "");
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\nexclude = [\"crates/a\"]\n",
        )
        .unwrap();
        let found = check(&root).unwrap().problems;
        assert!(
            found[0].contains("a is not a workspace member"),
            "{found:?}"
        );
    }

    #[test]
    fn the_repository_layering_passes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        assert_eq!(check(root).unwrap().problems, Vec::<String>::new());
    }
}
