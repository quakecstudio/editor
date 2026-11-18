// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask notices --out <file> [--target <triple>]`: writes the third-party notices every
//! release package carries beside `LICENSE` and `NOTICE` (ADR-0029, LIC-6). It lists each crate
//! the shipped binaries, those of `apps/`, link on the target, with its version, its license
//! expression, its authors and the text of each license file it ships; a crate that ships none
//! is given the standard text of each license its expression names, from `packaging/licenses/`.
//! Then the assets the app bundles under another license, named in `ATTRIBUTION.md`, with their
//! license texts. A text several crates ship word for word is written once. `--check` renders
//! the notices for every target and fails when a linked crate states no license, when a license
//! has no standard text, or when a file cannot be read: a package must not ship what it cannot
//! attribute. `cargo xtask ci` runs the check in-process ([`check`]).

use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::attribution::{DOC as ATTRIBUTION, entries};
use crate::{Report, print_outcome};

/// `cargo xtask notices`.
#[derive(clap::Parser, Debug)]
pub(crate) struct Args {
    /// Where to write the notices.
    #[arg(long, required_unless_present = "check")]
    out: Option<PathBuf>,
    /// The target the binaries are built for; every target when absent.
    #[arg(long)]
    target: Option<String>,
    /// Check that the notices can be written for every target instead of writing them.
    #[arg(long)]
    check: bool,
}

/// The directory of the shipped binaries' crates, relative to the workspace root.
const APPS: &str = "apps";

/// The standard license texts, `<SPDX id>.txt`, relative to the workspace root (SPDX's
/// license-list-data, `packaging/licenses/README.md`).
const STANDARD_TEXTS: &str = "packaging/licenses";

/// The names a crate's license files and directories start with, compared without ASCII case.
const LICENSE_FILES: [&str; 6] = [
    "LICENSE",
    "LICENCE",
    "COPYING",
    "NOTICE",
    "UNLICENSE",
    "COPYRIGHT",
];

/// Runs `cargo xtask notices` with the arguments `args`.
pub(crate) fn main(root: &Path, args: &Args) -> Result<bool, String> {
    if args.check {
        return Ok(print_outcome("notices", check(root)));
    }
    let crates = linked_crates(root, &metadata(root, args.target.as_deref())?);
    if let Some(problem) = unlicensed(&crates) {
        return Err(problem);
    }
    let text = render(root, &crates, &bundled_assets(root)?)?;
    let out = args.out.as_deref().ok_or("--out is required")?;
    fs::write(out, text).map_err(|e| format!("{}: {e}", out.display()))?;
    eprintln!(
        "notices: wrote {} for {} crates",
        out.display(),
        crates.len()
    );
    Ok(true)
}

/// Checks that the notices can be written for the binaries on every target: each linked crate
/// states a license, each license a crate with no license file names has a standard text, and
/// every file the notices read can be read.
pub(crate) fn check(root: &Path) -> Result<Report, String> {
    let crates = linked_crates(root, &metadata(root, None)?);
    let mut report = Report::default();
    match unlicensed(&crates) {
        Some(problem) => report.record(false, problem),
        None => report.record(
            true,
            format!("{} linked crates state their licenses", crates.len()),
        ),
    }
    match bundled_assets(root).and_then(|assets| render(root, &crates, &assets)) {
        Ok(text) => report.record(true, format!("the notices render, {} bytes", text.len())),
        Err(problem) => report.record(false, format!("the notices do not render: {problem}")),
    }
    Ok(report)
}

/// What `cargo metadata` says of the workspace.
#[derive(Debug, Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
    resolve: Resolve,
}

/// One package of the graph.
#[derive(Clone, Debug, Deserialize)]
struct Package {
    id: String,
    name: String,
    version: String,
    #[serde(default)]
    authors: Vec<String>,
    license: Option<String>,
    license_file: Option<String>,
    manifest_path: PathBuf,
    #[serde(default)]
    targets: Vec<Target>,
}

impl Package {
    /// Whether the package is a procedural macro, run by the compiler and linked into nothing.
    fn is_proc_macro(&self) -> bool {
        self.targets
            .iter()
            .any(|t| t.kind.iter().any(|k| k == "proc-macro"))
    }

    /// Whether the package states its license, by an expression or a file.
    const fn states_license(&self) -> bool {
        self.license.is_some() || self.license_file.is_some()
    }

    /// The license files the package ships: those its manifest names, and each file at the top
    /// of its directory, or in a directory there, whose name starts with one of
    /// [`LICENSE_FILES`], in path order.
    fn license_files(&self) -> Vec<PathBuf> {
        let Some(dir) = self.manifest_path.parent() else {
            return Vec::new();
        };
        let licensed = |path: &Path| {
            let name = path
                .file_name()
                .map_or_default(|n| n.to_string_lossy().to_ascii_uppercase());
            LICENSE_FILES.iter().any(|p| name.starts_with(p))
        };
        let mut files: BTreeSet<PathBuf> = BTreeSet::new();
        if let Some(named) = &self.license_file {
            files.insert(dir.join(named));
        }
        for path in read_dir(dir).into_iter().filter(|p| licensed(p)) {
            if path.is_file() {
                files.insert(path);
            } else if path.is_dir() {
                // A REUSE `LICENSES/` directory: its files are the texts.
                files.extend(read_dir(&path).into_iter().filter(|p| p.is_file()));
            }
        }
        files.into_iter().collect()
    }
}

/// The entries of the directory `dir`, none when it cannot be read.
fn read_dir(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir).map_or_default(|entries| entries.flatten().map(|e| e.path()).collect())
}

/// One of a package's targets.
#[derive(Clone, Debug, Deserialize)]
struct Target {
    kind: Vec<String>,
}

/// The resolved graph.
#[derive(Debug, Deserialize)]
struct Resolve {
    nodes: Vec<Node>,
}

/// A package's resolved dependencies.
#[derive(Debug, Deserialize)]
struct Node {
    id: String,
    deps: Vec<NodeDep>,
}

/// One resolved dependency and the kinds it is one of.
#[derive(Debug, Deserialize)]
struct NodeDep {
    pkg: String,
    dep_kinds: Vec<DepKind>,
}

/// A dependency's kind: none for a normal one, `dev` or `build` otherwise.
#[derive(Debug, Deserialize)]
struct DepKind {
    kind: Option<String>,
}

/// `cargo metadata` of the workspace at `root`, locked, for the binaries built for `target`,
/// or for every target.
fn metadata(root: &Path, target: Option<&str>) -> Result<Metadata, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .args(["metadata", "--format-version", "1", "--locked"])
        .current_dir(root);
    if let Some(target) = target {
        command.args(["--filter-platform", target]);
    }
    let output = command
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    serde_json::from_slice(&output.stdout).map_err(|e| format!("cargo metadata: {e}"))
}

/// The third-party crates the binaries of `root`'s `apps/` link: every package reachable from
/// them by normal dependencies, in name and version order. A build or dev dependency and a
/// procedural macro, with what only it depends on, ship nothing.
fn linked_crates(root: &Path, metadata: &Metadata) -> Vec<Package> {
    let apps = root.join(APPS);
    let members: BTreeSet<&str> = metadata
        .workspace_members
        .iter()
        .map(String::as_str)
        .collect();
    let by_id: HashMap<&str, &Package> = metadata
        .packages
        .iter()
        .map(|p| (p.id.as_str(), p))
        .collect();
    let nodes: HashMap<&str, &Node> = metadata
        .resolve
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n))
        .collect();
    let mut pending: Vec<&str> = metadata
        .packages
        .iter()
        .filter(|p| members.contains(p.id.as_str()) && p.manifest_path.starts_with(&apps))
        .map(|p| p.id.as_str())
        .collect();
    let mut seen: BTreeSet<&str> = pending.iter().copied().collect();
    while let Some(id) = pending.pop() {
        let Some(node) = nodes.get(id) else {
            continue;
        };
        for dep in &node.deps {
            let normal = dep.dep_kinds.iter().any(|k| k.kind.is_none());
            let linked = by_id
                .get(dep.pkg.as_str())
                .is_some_and(|p| !p.is_proc_macro());
            if normal && linked && seen.insert(dep.pkg.as_str()) {
                pending.push(dep.pkg.as_str());
            }
        }
    }
    let mut crates: Vec<Package> = seen
        .into_iter()
        .filter(|id| !members.contains(id))
        .filter_map(|id| by_id.get(id).map(|p| (*p).clone()))
        .collect();
    crates.sort_by(|a, b| (&a.name, &a.version).cmp(&(&b.name, &b.version)));
    crates
}

/// The problem with `crates` that stops the notices: the crates that state no license.
fn unlicensed(crates: &[Package]) -> Option<String> {
    let missing: Vec<&str> = crates
        .iter()
        .filter(|c| !c.states_license())
        .map(|c| c.name.as_str())
        .collect();
    (!missing.is_empty()).then(|| format!("no license stated by {}", missing.join(", ")))
}

/// The SPDX identifiers the license expression `expression` names, its exceptions among them,
/// in order: `MIT OR Apache-2.0` names `MIT` and `Apache-2.0`.
fn license_ids(expression: &str) -> Vec<String> {
    expression
        .split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == '/')
        .filter(|word| !word.is_empty() && !matches!(*word, "OR" | "AND" | "WITH"))
        .map(|word| word.trim_end_matches('+').to_owned())
        .collect()
}

/// The licenses of `expression` a binary copy is distributed under, whose texts it carries: of a
/// choice between licenses, `A OR B OR C`, the first that `has_text` says has a standard text
/// (the first alternative when none has); otherwise every license it names.
fn chosen_ids(expression: &str, has_text: &dyn Fn(&str) -> bool) -> Vec<String> {
    let ids = license_ids(expression);
    let choice = !expression.contains(['(', ')']) && !expression.contains(" AND ");
    if !choice || ids.len() < 2 {
        return ids;
    }
    let chosen = ids
        .iter()
        .find(|id| has_text(id))
        .or_else(|| ids.first())
        .cloned();
    chosen.into_iter().collect()
}

/// The standard text of the license `id`, relative to the workspace root.
fn standard_path(id: &str) -> String {
    format!("{STANDARD_TEXTS}/{id}.txt")
}

/// One asset the app bundles under a license other than the project's, with the files that
/// hold its license text.
#[derive(Debug, PartialEq, Eq)]
struct Asset {
    path: String,
    author: String,
    license: String,
    texts: Vec<String>,
}

/// The assets `ATTRIBUTION.md` lists under `apps/` or `crates/`, which the binaries bundle,
/// that carry a license other than the project's, each with the license files its source names
/// ("license in `PATH`").
fn bundled_assets(root: &Path) -> Result<Vec<Asset>, String> {
    let text =
        fs::read_to_string(root.join(ATTRIBUTION)).map_err(|e| format!("{ATTRIBUTION}: {e}"))?;
    assets_of(&text)
}

/// [`bundled_assets`] of the attribution document `text`.
fn assets_of(text: &str) -> Result<Vec<Asset>, String> {
    let mut assets = Vec::new();
    for entry in entries(text)? {
        let bundled = entry.path.starts_with("apps/") || entry.path.starts_with("crates/");
        if !bundled || entry.license.starts_with("GPL-") {
            continue;
        }
        let texts = entry
            .source
            .split("license in `")
            .skip(1)
            .filter_map(|rest| rest.split('`').next())
            .map(str::to_owned)
            .collect();
        assets.push(Asset {
            path: entry.path,
            author: entry.author,
            license: entry.license,
            texts,
        });
    }
    Ok(assets)
}

/// The notices of `crates` and `assets`: a heading per crate with its license expression and
/// authors, then the text of each license file, or a pointer to the crate that shipped the same
/// text first, or for a crate with none the standard texts its expression names; then each
/// asset; and last the standard texts and the assets' license texts, under `root`, each once.
fn render(root: &Path, crates: &[Package], assets: &[Asset]) -> Result<String, String> {
    let mut out = String::from(
        "QuakeC Studio: third-party notices\n\n\
         The binaries link the crates below and bundle the assets after them. Each is listed \
         with its license and the license text it ships; a crate that ships no text is given \
         the standard text of its license, at the end.\n",
    );
    let mut first: HashMap<String, String> = HashMap::new();
    let mut standard: BTreeSet<String> = BTreeSet::new();
    for package in crates {
        let label = format!("{} {}", package.name, package.version);
        let license = package.license.as_deref().unwrap_or("see its license file");
        _ = writeln!(out, "\n==== {label}\nLicense: {license}");
        if !package.authors.is_empty() {
            _ = writeln!(out, "Authors: {}", package.authors.join(", "));
        }
        let files = package.license_files();
        if files.is_empty() {
            for id in chosen_ids(license, &|id| root.join(standard_path(id)).is_file()) {
                _ = writeln!(out, "License text: the standard text of {id}, below");
                standard.insert(id);
            }
        }
        for file in files {
            let text = fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            let text = String::from_utf8_lossy(&text).into_owned();
            let name = file
                .strip_prefix(
                    package
                        .manifest_path
                        .parent()
                        .unwrap_or_else(|| Path::new("")),
                )
                .unwrap_or(&file)
                .display()
                .to_string();
            match first.get(&text) {
                Some(earlier) => {
                    _ = writeln!(out, "{name}: the same text as {earlier}'s");
                }
                None => {
                    _ = writeln!(out, "\n---- {name}\n{}", text.trim_end());
                    first.insert(text, label.clone());
                }
            }
        }
    }
    let mut texts: BTreeSet<String> = BTreeSet::new();
    for asset in assets {
        _ = writeln!(
            out,
            "\n==== {}\nAuthor: {}\nLicense: {}",
            asset.path, asset.author, asset.license
        );
        for text in &asset.texts {
            _ = writeln!(out, "License text: {text}");
            texts.insert(text.clone());
        }
    }
    for id in standard {
        let path = standard_path(&id);
        let text = fs::read_to_string(root.join(&path))
            .map_err(|e| format!("{path}: no standard text of {id}: {e}"))?;
        _ = writeln!(out, "\n---- the standard text of {id}\n{}", text.trim_end());
    }
    for path in texts {
        let text = fs::read_to_string(root.join(&path)).map_err(|e| format!("{path}: {e}"))?;
        _ = writeln!(out, "\n---- {path}\n{}", text.trim_end());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "test code: a failed unwrap or index is a failed test"
    )]

    use super::*;

    fn package(id: &str, manifest: &str, license: Option<&str>) -> Package {
        Package {
            id: id.to_owned(),
            name: id.to_owned(),
            version: "1.0.0".to_owned(),
            authors: vec![format!("{id}'s author")],
            license: license.map(str::to_owned),
            license_file: None,
            manifest_path: PathBuf::from(manifest),
            targets: vec![Target {
                kind: vec!["lib".to_owned()],
            }],
        }
    }

    /// A graph of one app that depends on `a` normally, on `b` only to build and on the macro
    /// `m`, `a` depending on `c` and `m` on `n`; `d` is no one's dependency, and `t` is a
    /// workspace member under another `apps` directory than the workspace's.
    fn graph() -> Metadata {
        let dep = |pkg: &str, kind: Option<&str>| NodeDep {
            pkg: pkg.to_owned(),
            dep_kinds: vec![DepKind {
                kind: kind.map(str::to_owned),
            }],
        };
        let mut macro_crate = package("m", "/r/m/Cargo.toml", Some("MIT"));
        macro_crate.targets = vec![Target {
            kind: vec!["proc-macro".to_owned()],
        }];
        Metadata {
            packages: vec![
                package("app", "/w/apps/app/Cargo.toml", Some("GPL-3.0-or-later")),
                package(
                    "t",
                    "/elsewhere/apps/t/Cargo.toml",
                    Some("GPL-3.0-or-later"),
                ),
                package("a", "/r/a/Cargo.toml", Some("MIT")),
                package("b", "/r/b/Cargo.toml", Some("MIT")),
                package("c", "/r/c/Cargo.toml", None),
                package("d", "/r/d/Cargo.toml", Some("MIT")),
                macro_crate,
                package("n", "/r/n/Cargo.toml", Some("MIT")),
            ],
            workspace_members: vec!["app".to_owned(), "t".to_owned()],
            resolve: Resolve {
                nodes: vec![
                    Node {
                        id: "app".to_owned(),
                        deps: vec![dep("a", None), dep("b", Some("build")), dep("m", None)],
                    },
                    Node {
                        id: "t".to_owned(),
                        deps: vec![dep("d", None)],
                    },
                    Node {
                        id: "a".to_owned(),
                        deps: vec![dep("c", None)],
                    },
                    Node {
                        id: "m".to_owned(),
                        deps: vec![dep("n", None)],
                    },
                ],
            },
        }
    }

    #[test]
    fn the_linked_crates_are_the_apps_normal_dependencies_but_macros() {
        let crates = linked_crates(Path::new("/w"), &graph());
        let names: Vec<&str> = crates.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["a", "c"]);
    }

    #[test]
    fn a_crate_with_no_license_stops_the_notices() {
        let crates = linked_crates(Path::new("/w"), &graph());
        assert_eq!(unlicensed(&crates).unwrap(), "no license stated by c");
        assert_eq!(unlicensed(&crates[..1]), None);
    }

    #[test]
    fn license_ids_are_the_expressions_identifiers() {
        assert_eq!(license_ids("MIT OR Apache-2.0"), ["MIT", "Apache-2.0"]);
        assert_eq!(
            license_ids("(MIT OR Apache-2.0) AND Unicode-3.0"),
            ["MIT", "Apache-2.0", "Unicode-3.0"]
        );
        assert_eq!(
            license_ids("Apache-2.0 WITH LLVM-exception"),
            ["Apache-2.0", "LLVM-exception"]
        );
        assert_eq!(license_ids("MIT/Apache-2.0"), ["MIT", "Apache-2.0"]);
    }

    #[test]
    fn of_a_choice_the_first_license_with_a_text_is_chosen() {
        let has = |id: &str| id == "MIT" || id == "Apache-2.0";
        assert_eq!(
            chosen_ids("LGPL-2.1-or-later OR MIT OR Apache-2.0", &has),
            ["MIT"]
        );
        assert_eq!(chosen_ids("MIT OR Apache-2.0", &has), ["MIT"]);
        assert_eq!(chosen_ids("Zlib OR BSL-1.0", &has), ["Zlib"]);
        assert_eq!(
            chosen_ids("(MIT OR Apache-2.0) AND Unicode-3.0", &has),
            ["MIT", "Apache-2.0", "Unicode-3.0"]
        );
        assert_eq!(chosen_ids("MIT AND ISC", &has), ["MIT", "ISC"]);
    }

    /// A scratch directory of this test's own.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("qs-notices-{name}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_shared_license_text_is_written_once() {
        let dir = scratch("shared");
        for name in ["x", "y"] {
            fs::create_dir_all(dir.join(name)).unwrap();
            fs::write(dir.join(name).join("LICENSE-MIT"), "MIT text\n").unwrap();
        }
        let crates = ["x", "y"].map(|name| {
            let manifest = dir.join(name).join("Cargo.toml");
            package(name, manifest.to_str().unwrap(), Some("MIT"))
        });
        let text = render(&dir, &crates, &[]).unwrap();
        assert_eq!(text.matches("MIT text").count(), 1);
        assert!(
            text.contains("LICENSE-MIT: the same text as x 1.0.0's"),
            "{text}"
        );
        assert!(text.contains("Authors: y's author"), "{text}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_crate_with_no_license_file_is_given_the_standard_text() {
        let dir = scratch("standard");
        fs::create_dir_all(dir.join("z")).unwrap();
        fs::create_dir_all(dir.join(STANDARD_TEXTS)).unwrap();
        fs::write(dir.join(STANDARD_TEXTS).join("MIT.txt"), "the MIT text\n").unwrap();
        let manifest = dir.join("z").join("Cargo.toml");
        let both = [package(
            "z",
            manifest.to_str().unwrap(),
            Some("MIT AND Apache-2.0"),
        )];
        // Apache-2.0 has no standard text here, which stops the notices of both licenses.
        let missing = render(&dir, &both, &[]).unwrap_err();
        assert!(
            missing.contains("no standard text of Apache-2.0"),
            "{missing}"
        );
        // Of a choice, the license with a text is the one written.
        let crates = [package(
            "z",
            manifest.to_str().unwrap(),
            Some("Apache-2.0 OR MIT"),
        )];
        let text = render(&dir, &crates, &[]).unwrap();
        assert!(!text.contains("the standard text of Apache-2.0"), "{text}");
        assert!(
            text.contains("License text: the standard text of MIT, below"),
            "{text}"
        );
        assert!(
            text.contains("---- the standard text of MIT\nthe MIT text"),
            "{text}"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_licenses_directory_is_read() {
        let dir = scratch("reuse");
        fs::create_dir_all(dir.join("r").join("LICENSES")).unwrap();
        fs::write(
            dir.join("r").join("LICENSES").join("MIT.txt"),
            "reuse MIT\n",
        )
        .unwrap();
        let manifest = dir.join("r").join("Cargo.toml");
        let crates = [package("r", manifest.to_str().unwrap(), Some("MIT"))];
        let text = render(&dir, &crates, &[]).unwrap();
        assert!(text.contains("reuse MIT"), "{text}");
        assert!(!text.contains("License text: the standard text"), "{text}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn only_bundled_assets_of_another_license_are_listed() {
        let table = "| Path | Author | Source | License |\n| --- | --- | --- | --- |\n\
                     | `apps/desktop/assets/fonts/F.ttf` | IBM Corp. | release; license in \
                     `apps/desktop/assets/fonts/F-OFL.txt` | OFL-1.1 |\n\
                     | `docs/design/m.png` | us | drawn | CC0-1.0 |\n\
                     | `apps/desktop/assets/i.png` | us | drawn | GPL-3.0-or-later |\n";
        let assets = assets_of(table).unwrap();
        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].license, "OFL-1.1");
        assert_eq!(assets[0].texts, ["apps/desktop/assets/fonts/F-OFL.txt"]);
    }
}
