// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask notices --out <file>`: writes the third-party notices every release package
//! carries beside `LICENSE` and `NOTICE` (ADR-0029, LIC-6). It lists each crate the shipped
//! binaries link, the binaries of `apps/`, with its version, its license expression and the
//! text of each license file it ships, and the license texts of the assets the app bundles,
//! named in `ATTRIBUTION.md`. A license text several crates ship word for word is written once.
//! `--check` writes nothing and fails when a linked crate states no license: a package must not
//! ship what it cannot attribute. `cargo xtask ci` runs the check in-process ([`check`]).

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::attribution::DOC as ATTRIBUTION;
use crate::{Report, print_outcome};

/// `cargo xtask notices`.
#[derive(clap::Parser, Debug)]
pub(crate) struct Args {
    /// Where to write the notices.
    #[arg(long, required_unless_present = "check")]
    out: Option<PathBuf>,
    /// Check that every linked crate states a license instead of writing.
    #[arg(long)]
    check: bool,
}

/// The directory of the shipped binaries' crates, relative to the workspace root.
const APPS: &str = "apps";

/// The names a crate's license files start with, compared without ASCII case.
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
    let crates = linked_crates(&metadata(root)?);
    let missing: Vec<&str> = crates
        .iter()
        .filter(|c| c.license.is_none() && c.license_file.is_none())
        .map(|c| c.name.as_str())
        .collect();
    if !missing.is_empty() {
        return Err(format!("no license stated by {}", missing.join(", ")));
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

/// Checks that every crate the shipped binaries link states a license.
pub(crate) fn check(root: &Path) -> Result<Report, String> {
    let crates = linked_crates(&metadata(root)?);
    let mut report = Report::default();
    for package in &crates {
        let stated = package.license.is_some() || package.license_file.is_some();
        report.record(
            stated,
            if stated {
                format!("{} {} states its license", package.name, package.version)
            } else {
                format!("{} {} states no license", package.name, package.version)
            },
        );
    }
    report.record(
        !crates.is_empty(),
        format!("{} crates linked by the binaries of {APPS}/", crates.len()),
    );
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
    license: Option<String>,
    license_file: Option<String>,
    manifest_path: PathBuf,
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

/// `cargo metadata` of the workspace at `root`, offline and locked.
fn metadata(root: &Path) -> Result<Metadata, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--locked"])
        .current_dir(root)
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

/// The third-party crates the binaries of `apps/` link: every package reachable from them by
/// normal dependencies, on any target, in name and version order. A build or dev dependency
/// ships nothing.
fn linked_crates(metadata: &Metadata) -> Vec<Package> {
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
        .filter(|p| members.contains(p.id.as_str()) && is_app(&p.manifest_path))
        .map(|p| p.id.as_str())
        .collect();
    let mut seen: BTreeSet<&str> = pending.iter().copied().collect();
    while let Some(id) = pending.pop() {
        let Some(node) = nodes.get(id) else {
            continue;
        };
        for dep in &node.deps {
            let normal = dep.dep_kinds.iter().any(|k| k.kind.is_none());
            if normal && seen.insert(dep.pkg.as_str()) {
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

/// Whether the manifest at `path` is one of the workspace's binaries, under `apps/`.
fn is_app(path: &Path) -> bool {
    path.components().any(|c| c.as_os_str() == APPS)
}

/// The license files a crate ships: those its manifest names, and each file at the top of its
/// directory whose name starts with one of [`LICENSE_FILES`], in name order.
fn license_files(package: &Package) -> Vec<PathBuf> {
    let Some(dir) = package.manifest_path.parent() else {
        return Vec::new();
    };
    let mut files: BTreeSet<PathBuf> = BTreeSet::new();
    if let Some(named) = &package.license_file {
        files.insert(dir.join(named));
    }
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_ascii_uppercase();
            if entry.path().is_file() && LICENSE_FILES.iter().any(|p| name.starts_with(p)) {
                files.insert(entry.path());
            }
        }
    }
    files.into_iter().collect()
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
    Ok(assets_of(&text))
}

/// [`bundled_assets`] of the attribution table `text`.
fn assets_of(text: &str) -> Vec<Asset> {
    let mut assets = Vec::new();
    for line in text.lines() {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        let [_, path, author, source, license, ..] = cells.as_slice() else {
            continue;
        };
        let path = path.trim_matches('`');
        let bundled = path.starts_with("apps/") || path.starts_with("crates/");
        if !bundled || license.starts_with("GPL-") {
            continue;
        }
        let texts = source
            .split("license in `")
            .skip(1)
            .filter_map(|rest| rest.split('`').next())
            .map(str::to_owned)
            .collect();
        assets.push(Asset {
            path: path.to_owned(),
            author: (*author).to_owned(),
            license: (*license).to_owned(),
            texts,
        });
    }
    assets
}

/// The notices of `crates` and `assets`: a heading per crate with its license expression, then
/// the text of each license file, or a pointer to the crate that shipped the same text first;
/// then each asset, and last the license texts the assets' rows name, under `root`, each once.
fn render(root: &Path, crates: &[Package], assets: &[Asset]) -> Result<String, String> {
    let mut out = String::from(
        "QuakeC Studio: third-party notices\n\n\
         The binaries link the crates below and bundle the assets after them. Each is listed \
         with its license and the license text it ships.\n",
    );
    let mut first: HashMap<String, String> = HashMap::new();
    for package in crates {
        let label = format!("{} {}", package.name, package.version);
        let license = package.license.as_deref().unwrap_or("see its license file");
        out.push_str(&format!("\n==== {label}\nLicense: {license}\n"));
        for file in license_files(package) {
            let text = fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            let name = file
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into());
            match first.get(&text) {
                Some(earlier) => {
                    out.push_str(&format!("{name}: the same text as {earlier}'s\n"));
                }
                None => {
                    out.push_str(&format!("\n---- {name}\n{}\n", text.trim_end()));
                    first.insert(text, label.clone());
                }
            }
        }
    }
    let mut texts: BTreeSet<&str> = BTreeSet::new();
    for asset in assets {
        out.push_str(&format!(
            "\n==== {}\nAuthor: {}\nLicense: {}\n",
            asset.path, asset.author, asset.license
        ));
        for text in &asset.texts {
            out.push_str(&format!("License text: {text}\n"));
            texts.insert(text);
        }
    }
    for path in texts {
        let text = fs::read_to_string(root.join(path)).map_err(|e| format!("{path}: {e}"))?;
        out.push_str(&format!("\n---- {path}\n{}\n", text.trim_end()));
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

    /// A graph of one app that depends on `a` normally and on `b` only to build, `a`
    /// depending on `c`; `d` is no one's dependency.
    fn graph() -> Metadata {
        let package = |id: &str, manifest: &str, license: Option<&str>| Package {
            id: id.to_owned(),
            name: id.to_owned(),
            version: "1.0.0".to_owned(),
            license: license.map(str::to_owned),
            license_file: None,
            manifest_path: PathBuf::from(manifest),
        };
        let dep = |pkg: &str, kind: Option<&str>| NodeDep {
            pkg: pkg.to_owned(),
            dep_kinds: vec![DepKind {
                kind: kind.map(str::to_owned),
            }],
        };
        Metadata {
            packages: vec![
                package("app", "/w/apps/app/Cargo.toml", Some("GPL-3.0-or-later")),
                package("a", "/r/a/Cargo.toml", Some("MIT")),
                package("b", "/r/b/Cargo.toml", Some("MIT")),
                package("c", "/r/c/Cargo.toml", None),
                package("d", "/r/d/Cargo.toml", Some("MIT")),
            ],
            workspace_members: vec!["app".to_owned()],
            resolve: Resolve {
                nodes: vec![
                    Node {
                        id: "app".to_owned(),
                        deps: vec![dep("a", None), dep("b", Some("build"))],
                    },
                    Node {
                        id: "a".to_owned(),
                        deps: vec![dep("c", None)],
                    },
                ],
            },
        }
    }

    #[test]
    fn the_linked_crates_are_the_apps_normal_dependencies() {
        let names: Vec<String> = linked_crates(&graph())
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(names, ["a", "c"]);
    }

    #[test]
    fn a_crate_with_no_license_is_found() {
        let crates = linked_crates(&graph());
        let missing: Vec<&str> = crates
            .iter()
            .filter(|c| c.license.is_none() && c.license_file.is_none())
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(missing, ["c"]);
    }

    #[test]
    fn a_shared_license_text_is_written_once() {
        let dir = std::env::temp_dir().join(format!("qs-notices-{}", std::process::id()));
        for name in ["x", "y"] {
            fs::create_dir_all(dir.join(name)).unwrap();
            fs::write(dir.join(name).join("LICENSE-MIT"), "MIT text\n").unwrap();
        }
        let package = |name: &str| Package {
            id: name.to_owned(),
            name: name.to_owned(),
            version: "1.0.0".to_owned(),
            license: Some("MIT".to_owned()),
            license_file: None,
            manifest_path: dir.join(name).join("Cargo.toml"),
        };
        let text = render(&dir, &[package("x"), package("y")], &[]).unwrap();
        assert_eq!(text.matches("MIT text").count(), 1);
        assert!(
            text.contains("LICENSE-MIT: the same text as x 1.0.0's"),
            "{text}"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn only_bundled_assets_of_another_license_are_listed() {
        let table = "| Path | Author | Source | License |\n| --- | --- | --- | --- |\n\
                     | `apps/desktop/assets/fonts/F.ttf` | IBM Corp. | release; license in \
                     `apps/desktop/assets/fonts/F-OFL.txt` | OFL-1.1 |\n\
                     | `docs/design/m.png` | us | drawn | CC0-1.0 |\n\
                     | `apps/desktop/assets/i.png` | us | drawn | GPL-3.0-or-later |\n";
        let assets = assets_of(table);
        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].license, "OFL-1.1");
        assert_eq!(assets[0].texts, ["apps/desktop/assets/fonts/F-OFL.txt"]);
    }
}
