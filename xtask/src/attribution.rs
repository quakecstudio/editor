// SPDX-License-Identifier: GPL-3.0-or-later

//! Checks that every asset file in the repository is listed in `ATTRIBUTION.md`.
//!
//! An asset is a file of a type that cannot carry an SPDX header: images,
//! fonts, sounds and Quake's own formats. `ATTRIBUTION.md` holds one table with
//! the columns `Path | Author | Source | License`, one row per asset, with the
//! path in backticks and the license as an SPDX identifier from
//! [`ASSET_LICENSES`]. Every asset must have a row, every row must name a file
//! in the repository, no path may appear twice, and author and source must not
//! be empty.

use std::collections::BTreeSet;
use std::path::Path;

use crate::{Report, repo};

pub(crate) const DOC: &str = "ATTRIBUTION.md";
const COLUMNS: [&str; 4] = ["Path", "Author", "Source", "License"];

/// File types that are assets.
const ASSET_EXTENSIONS: [&str; 24] = [
    "png", "jpg", "jpeg", "gif", "bmp", "tga", "pcx", "webp", "svg", "ico", "icns", "ttf", "otf",
    "woff", "woff2", "wav", "ogg", "mp3", "flac", "mdl", "bsp", "wad", "lmp", "spr",
];

/// The licenses an asset may carry: GPL-3.0-or-later itself and licenses whose
/// works may be distributed with a GPL-3.0-or-later program.
pub(crate) const ASSET_LICENSES: [&str; 7] = [
    "GPL-3.0-or-later",
    "GPL-2.0-or-later",
    "CC0-1.0",
    "CC-BY-4.0",
    "CC-BY-SA-4.0",
    "MIT",
    "Apache-2.0",
];

/// One row of the attribution table.
#[derive(Debug, PartialEq, Eq)]
struct Entry {
    line: usize,
    path: String,
    author: String,
    source: String,
    license: String,
}

pub(crate) fn is_asset(path: &Path) -> bool {
    path.extension()
        .map(|extension| extension.to_string_lossy().to_lowercase())
        .is_some_and(|extension| ASSET_EXTENSIONS.contains(&extension.as_str()))
}

pub(crate) fn run(root: &Path) -> Result<Report, String> {
    check(root, &repo::files(root)?)
}

/// Checks the assets among `files` (paths relative to `root`) against `ATTRIBUTION.md`.
pub(crate) fn check(root: &Path, files: &[String]) -> Result<Report, String> {
    let entries = entries(&repo::read(root, DOC)?)?;
    let mut report = Report::default();
    let all: BTreeSet<&str> = files.iter().map(String::as_str).collect();
    let mut listed = BTreeSet::new();
    for entry in &entries {
        let at = format!("{DOC}:{} {}", entry.line, entry.path);
        if !listed.insert(entry.path.as_str()) {
            report.problems.push(format!("{at}: listed twice"));
        }
        if !all.contains(entry.path.as_str()) {
            report
                .problems
                .push(format!("{at}: no such file in the repository"));
        }
        if entry.author.is_empty() || entry.source.is_empty() {
            report
                .problems
                .push(format!("{at}: author or source is empty"));
        }
        if !ASSET_LICENSES.contains(&entry.license.as_str()) {
            report.problems.push(format!(
                "{at}: license {:?} is not one of {}",
                entry.license,
                ASSET_LICENSES.join(", ")
            ));
        }
    }
    let mut assets = 0_usize;
    for path in files.iter().filter(|path| is_asset(Path::new(path))) {
        assets = assets.saturating_add(1);
        if !listed.contains(path.as_str()) {
            report
                .problems
                .push(format!("{path}: asset file not listed in {DOC}"));
        }
    }
    report.passed.push(format!(
        "{assets} asset files checked against {} {DOC} rows",
        entries.len()
    ));
    Ok(report)
}

/// The rows of the `Path | Author | Source | License` table.
fn entries(doc: &str) -> Result<Vec<Entry>, String> {
    let mut entries = Vec::new();
    let mut lines = doc.lines().enumerate();
    let found = lines.by_ref().any(|(_, line)| cells(line) == COLUMNS);
    if !found {
        return Err(format!("{DOC} has no `| {} |` table", COLUMNS.join(" | ")));
    }
    for (index, line) in lines {
        if !line.trim_start().starts_with('|') {
            break;
        }
        let number = index.saturating_add(1);
        let row = cells(line);
        if row
            .iter()
            .all(|cell| cell.chars().all(|c| c == '-' || c == ':'))
        {
            continue;
        }
        let [path, author, source, license] = row.as_slice() else {
            return Err(format!("{DOC}:{number}: a row needs exactly 4 cells"));
        };
        let Some(path) = path
            .strip_prefix('`')
            .and_then(|path| path.strip_suffix('`'))
            .filter(|path| !path.is_empty() && !path.contains('`'))
        else {
            return Err(format!("{DOC}:{number}: the path must be in backticks"));
        };
        entries.push(Entry {
            line: number,
            path: path.to_owned(),
            author: (*author).to_owned(),
            source: (*source).to_owned(),
            license: (*license).to_owned(),
        });
    }
    Ok(entries)
}

fn cells(line: &str) -> Vec<&str> {
    let line = line.trim();
    let Some(inner) = line
        .strip_prefix('|')
        .and_then(|line| line.strip_suffix('|'))
    else {
        return Vec::new();
    };
    inner.split('|').map(str::trim).collect()
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "test code: a failed unwrap or index is a failed test"
    )]

    use std::fs;
    use std::path::PathBuf;

    use super::*;
    use crate::test_dirs::fresh;

    const HEAD: &str =
        "# Attribution\n\n| Path | Author | Source | License |\n| --- | --- | --- | --- |\n";

    /// A scratch repository with `doc` as `ATTRIBUTION.md` and empty `files`.
    fn fixture(name: &str, doc: &str, files: &[&str]) -> (PathBuf, Vec<String>) {
        let root = fresh(&format!("attribution/{name}"));
        fs::write(root.join(DOC), doc).unwrap();
        let mut listed = vec![DOC.to_owned()];
        for file in files {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "").unwrap();
            listed.push((*file).to_owned());
        }
        (root, listed)
    }

    fn problems(name: &str, rows: &str, files: &[&str]) -> Vec<String> {
        let (root, listed) = fixture(name, &format!("{HEAD}{rows}"), files);
        check(&root, &listed).unwrap().problems
    }

    const ROW: &str = "| `art/a.png` | Someone | Drawn for this project | CC0-1.0 |\n";

    #[test]
    fn listed_assets_pass_and_non_assets_need_no_row() {
        assert_eq!(
            problems("ok", ROW, &["art/a.png", "src/a.rs"]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn an_unlisted_asset_fails() {
        let found = problems("unlisted", ROW, &["art/a.png", "art/b.WAV"]);
        assert_eq!(
            found,
            ["art/b.WAV: asset file not listed in ATTRIBUTION.md"]
        );
    }

    #[test]
    fn a_row_for_a_missing_file_fails() {
        let found = problems("stale", ROW, &[]);
        assert_eq!(found.len(), 1);
        assert!(
            found[0].ends_with("no such file in the repository"),
            "{found:?}"
        );
    }

    #[test]
    fn a_duplicate_row_fails() {
        let found = problems("twice", &format!("{ROW}{ROW}"), &["art/a.png"]);
        assert_eq!(found.len(), 1);
        assert!(found[0].ends_with("listed twice"), "{found:?}");
    }

    #[test]
    fn an_empty_author_or_source_fails() {
        let rows = "| `art/a.png` |  | x | CC0-1.0 |\n| `art/b.png` | x |  | CC0-1.0 |\n";
        let found = problems("empty", rows, &["art/a.png", "art/b.png"]);
        assert_eq!(found.len(), 2, "{found:?}");
    }

    #[test]
    fn a_disallowed_license_fails() {
        let rows = "| `art/a.png` | x | y | CC-BY-NC-4.0 |\n";
        let found = problems("license", rows, &["art/a.png"]);
        assert_eq!(found.len(), 1);
        assert!(found[0].contains("CC-BY-NC-4.0"), "{found:?}");
    }

    #[test]
    fn a_malformed_table_is_an_error() {
        for (name, doc) in [
            ("no-table", "# Attribution\n".to_owned()),
            ("cells", format!("{HEAD}| `a.png` | x | y |\n")),
            ("backticks", format!("{HEAD}| a.png | x | y | MIT |\n")),
            ("inner-tick", format!("{HEAD}| `a`b.png` | x | y | MIT |\n")),
            ("empty-path", format!("{HEAD}| `` | x | y | MIT |\n")),
        ] {
            let (root, listed) = fixture(name, &doc, &[]);
            assert!(check(&root, &listed).is_err(), "{name}");
        }
    }

    #[test]
    fn a_missing_attribution_file_is_an_error() {
        let (root, listed) = fixture("missing", "", &[]);
        fs::remove_file(root.join(DOC)).unwrap();
        assert!(check(&root, &listed).is_err());
    }

    #[test]
    fn the_table_ends_at_the_first_non_row_line() {
        let doc = format!("{HEAD}{ROW}\n| `x.png` | a | b | MIT |\n");
        assert_eq!(entries(&doc).unwrap().len(), 1);
    }

    #[test]
    fn the_repository_passes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let report = run(root).unwrap();
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        assert!(!report.passed.is_empty());
    }

    #[test]
    fn the_repository_attribution_table_parses() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let doc = fs::read_to_string(root.join(DOC)).unwrap();
        assert!(!entries(&doc).unwrap().is_empty());
    }
}
