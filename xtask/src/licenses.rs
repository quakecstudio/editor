// SPDX-License-Identifier: GPL-3.0-or-later

//! Checks the license of every file in the repository.
//!
//! Every file is classified by name or extension, and a file of a type with no
//! rule fails, so a new kind of file cannot slip past unscanned:
//!
//! - **Source** (Rust, Python, shell, C, QuakeC, `Makefile`): starts with an
//!   `SPDX-License-Identifier` line (after a `#!` line, if any) naming an
//!   allowed license, and passes the ported-file header rule of
//!   `docs/porting.md` (rules H1 to H5, in [`crate::ported`]).
//! - **Text** (docs, config, data): covered by `LICENSE`; needs no header.
//! - **Asset** (images, fonts, sounds, game formats): its license is recorded in
//!   `ATTRIBUTION.md` and checked by `cargo xtask attribution`.
//! - **Cached** (a file listed in `specs/manifest.tsv`): a public specification
//!   cached verbatim under its own license, recorded in `specs/README.md`; it is
//!   not scanned, since its bytes must match the upstream file.
//!
//! Every source and text file is also scanned for SPDX tags in comments, and
//! each one must name an allowed license. `LICENSE` must be the GPL-3.0 text.

use std::collections::BTreeSet;
use std::path::Path;

use crate::{Report, attribution, ported, repo};

/// The licenses an SPDX tag in this repository may name (ADR-0002).
const ALLOWED: [&str; 1] = ["GPL-3.0-or-later"];
const SPDX_TAG: &str = "SPDX-License-Identifier:";
const LICENSE_FILE: &str = "LICENSE";
const LICENSE_MARKERS: [&str; 2] = ["GNU GENERAL PUBLIC LICENSE", "Version 3, 29 June 2007"];
const SPECS_MANIFEST: &str = "specs/manifest.tsv";

const SOURCE_EXTENSIONS: [&str; 9] = ["rs", "py", "sh", "c", "h", "qc", "ts", "lua", "scm"];
const SOURCE_NAMES: [&str; 1] = ["Makefile"];
const TEXT_EXTENSIONS: [&str; 12] = [
    "md", "toml", "yaml", "yml", "json", "jsonl", "lock", "html", "txt", "tsv", "csv", "hex",
];
const TEXT_NAMES: [&str; 3] = [LICENSE_FILE, ".gitignore", ".gitattributes"];
const COMMENT_OPENERS: [&str; 7] = ["//", "#", "<!--", "/*", "*", "--", ";"];
const COMMENT_CLOSERS: [&str; 2] = ["*/", "-->"];

#[derive(Debug, PartialEq, Eq)]
enum Kind {
    Source,
    Text,
    Asset,
    Cached,
    Unknown,
}

pub(crate) fn run(root: &Path) -> Result<Report, String> {
    check(root, &repo::files(root)?)
}

/// Checks `files` (paths relative to `root`).
pub(crate) fn check(root: &Path, files: &[String]) -> Result<Report, String> {
    let upstreams = ported::upstreams(&repo::read(root, ported::DOC)?)?;
    let cached = cached_specs(root, files)?;
    let mut report = Report::default();
    let (mut sources, mut texts, mut assets) = (0_usize, 0_usize, 0_usize);
    for path in files {
        let before = report.problems.len();
        let kind = if cached.contains(path) {
            Kind::Cached
        } else {
            classify(path)
        };
        match kind {
            Kind::Unknown => {
                report.problems.push(format!(
                    "{path}: no license rule for this file type; classify it in \
                     xtask/src/licenses.rs"
                ));
                continue;
            }
            Kind::Asset => {
                assets = assets.saturating_add(1);
                continue;
            }
            Kind::Cached => continue,
            Kind::Source | Kind::Text => {}
        }
        let text = match repo::read(root, path) {
            Ok(text) => text,
            Err(error) => {
                report.problems.push(error);
                continue;
            }
        };
        for (number, line) in text.lines().enumerate() {
            if let Some(expression) = spdx_expression(line)
                && !ALLOWED.contains(&expression)
            {
                report.problems.push(format!(
                    "{path}:{}: SPDX license {expression:?} is not allowed (allowed: {})",
                    number.saturating_add(1),
                    ALLOWED.join(", ")
                ));
            }
        }
        if kind == Kind::Text {
            if report.problems.len() == before {
                texts = texts.saturating_add(1);
            }
            continue;
        }
        let header = ported::header(&text);
        let first = header.iter().find(|line| !line.starts_with("#!"));
        if first.and_then(|line| spdx_expression(line)).is_none() {
            report.problems.push(format!(
                "{path}: does not start with an `{SPDX_TAG} {}` line",
                ALLOWED.join(" | ")
            ));
        }
        for problem in ported::check_header(&header, &upstreams) {
            report
                .problems
                .push(format!("{path}: ported-file header rule {problem}"));
        }
        if report.problems.len() == before {
            sources = sources.saturating_add(1);
        }
    }
    report.passed.push(format!(
        "{sources} source files start with an allowed SPDX line and pass the ported-file rule"
    ));
    report
        .passed
        .push(format!("{texts} text files carry no disallowed SPDX tag"));
    report.passed.push(format!(
        "{assets} asset files are left to cargo xtask attribution"
    ));
    report.passed.push(format!(
        "{} cached specification files are listed in {SPECS_MANIFEST}",
        cached.len()
    ));

    let license_ok = files.iter().any(|path| path == LICENSE_FILE)
        && repo::read(root, LICENSE_FILE)
            .is_ok_and(|text| LICENSE_MARKERS.iter().all(|marker| text.contains(marker)));
    report.record(license_ok, format!("{LICENSE_FILE} is the GPL-3.0 text"));
    Ok(report)
}

/// The paths `specs/manifest.tsv` lists (its first column), if the file is in `files`.
fn cached_specs(root: &Path, files: &[String]) -> Result<BTreeSet<String>, String> {
    if !files.iter().any(|path| path == SPECS_MANIFEST) {
        return Ok(BTreeSet::new());
    }
    let manifest = repo::read(root, SPECS_MANIFEST)?;
    Ok(manifest
        .lines()
        .skip(1)
        .filter_map(|line| line.split('\t').next())
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect())
}

fn classify(path: &str) -> Kind {
    let path = Path::new(path);
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if SOURCE_NAMES.contains(&name.as_ref()) || SOURCE_EXTENSIONS.contains(&extension.as_str()) {
        Kind::Source
    } else if TEXT_NAMES.contains(&name.as_ref()) || TEXT_EXTENSIONS.contains(&extension.as_str()) {
        Kind::Text
    } else if attribution::is_asset(path) {
        Kind::Asset
    } else {
        Kind::Unknown
    }
}

/// The license expression of a comment line carrying an SPDX tag.
fn spdx_expression(line: &str) -> Option<&str> {
    let mut rest = line.trim();
    let opener = COMMENT_OPENERS
        .iter()
        .find_map(|opener| rest.strip_prefix(opener))?;
    rest = opener.trim_start().strip_prefix(SPDX_TAG)?.trim();
    for closer in COMMENT_CLOSERS {
        rest = rest.strip_suffix(closer).unwrap_or(rest).trim_end();
    }
    Some(rest)
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

    const ID: &str = "Copyright (C) 1996-1997 Id Software, Inc.";
    const COMMIT: &str = "bf4ac424ce754894ac8f1dae6a3981954bc9852d";

    fn tag(license: &str) -> String {
        format!("SPDX-License-{} {license}", "Identifier:")
    }

    /// A scratch repository holding `files` plus the porting doc and LICENSE.
    fn fixture(name: &str, files: &[(&str, String)]) -> (PathBuf, Vec<String>) {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let real_root = manifest.parent().unwrap();
        let root = fresh(&format!("licenses/{name}"));
        let mut listed = Vec::new();
        let porting = fs::read_to_string(real_root.join(ported::DOC)).unwrap();
        let license = fs::read_to_string(real_root.join(LICENSE_FILE)).unwrap();
        let base = [(ported::DOC, porting), (LICENSE_FILE, license)];
        for (path, text) in base.iter().chain(files) {
            let full = root.join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, text).unwrap();
            listed.push((*path).to_owned());
        }
        (root, listed)
    }

    fn problems(name: &str, files: &[(&str, String)]) -> Vec<String> {
        let (root, listed) = fixture(name, files);
        check(&root, &listed).unwrap().problems
    }

    #[test]
    fn a_clean_repository_passes() {
        let rs = format!("// {}\n\nfn f() {{}}\n", tag("GPL-3.0-or-later"));
        let py = format!("#!/usr/bin/env python3\n# {}\n", tag("GPL-3.0-or-later"));
        let ported = format!(
            "// {}\n// {ID}\n// Ported from id-Software/Quake@{COMMIT}: a.c, F\n",
            tag("GPL-3.0-or-later")
        );
        let files = [
            ("src/a.rs", rs),
            ("tools/b.py", py),
            ("src/ported.rs", ported),
            ("Makefile", format!("# {}\n", tag("GPL-3.0-or-later"))),
            ("README.md", "# Readme\n".to_owned()),
            ("art/logo.png", String::new()),
        ];
        assert_eq!(problems("clean", &files), Vec::<String>::new());
    }

    // ADR-0017: a file ported from FTEQW keeps its README's copyright line (rule H4); without
    // it the header fails
    #[test]
    fn an_fteqw_port_keeps_fte_s_copyright_line() {
        let fte = "Copyright (c) 2004-2025 FTE's team and its contributors";
        let cite = "Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: \
                    engine/qclib/qcc_pr_lex.c, QCC_PR_CheckCompConst";
        let spdx = tag("GPL-3.0-or-later");
        let good = format!("// {spdx}\n// {fte}\n// Copyright (C) 2026 Us\n//\n// {cite}\n");
        assert_eq!(
            problems("fteqw-good", &[("src/a.rs", good)]),
            Vec::<String>::new()
        );
        let bad = format!("// {spdx}\n// Copyright (C) 2026 Us\n//\n// {cite}\n");
        let found = problems("fteqw-bad", &[("src/a.rs", bad)]);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("H4"), "{found:?}");
    }

    // ADR-0018: a file ported from QuakeSpasm keeps its upstream file's copyright lines, at
    // least one of them from the QuakeSpasm row (rule H4); with none of them the header fails
    #[test]
    fn a_quakespasm_port_keeps_its_file_s_copyright_lines() {
        let cite = "Ported from sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5: \
                    Quake/world.c, SV_LinkEdict";
        let spdx = tag("GPL-3.0-or-later");
        let good = format!(
            "// {spdx}\n// Copyright (C) 1996-2001 Id Software, Inc.\n\
             // Copyright (C) 2007-2008 Kristian Duske\n\
             // Copyright (C) 2010-2014 QuakeSpasm developers\n\
             // Copyright (C) 2026 QuakeC Studio contributors\n//\n// {cite}\n"
        );
        assert_eq!(
            problems("quakespasm-good", &[("src/a.rs", good)]),
            Vec::<String>::new()
        );
        let bad = format!(
            "// {spdx}\n// Copyright (C) 2007-2008 Kristian Duske\n\
             // Copyright (C) 2026 QuakeC Studio contributors\n//\n// {cite}\n"
        );
        let found = problems("quakespasm-bad", &[("src/a.rs", bad)]);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("H4"), "{found:?}");
    }

    #[test]
    fn the_repository_passes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let report = run(root).unwrap();
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        assert!(report.passed[0].starts_with(|c: char| c.is_ascii_digit() && c != '0'));
    }

    #[test]
    fn pass_counts_leave_out_failing_files() {
        let good = format!("// {}\n", tag("GPL-3.0-or-later"));
        let files = [
            ("a.rs", good.clone()),
            ("b.rs", "fn f() {}\n".to_owned()),
            ("c.md", format!("<!-- {} -->\n", tag("MIT"))),
            ("d.md", good),
        ];
        let (root, listed) = fixture("counts", &files);
        let passed = check(&root, &listed).unwrap().passed;
        assert!(passed[0].starts_with("1 source files"), "{passed:?}");
        // d.md, plus the porting doc and LICENSE every fixture holds.
        assert!(passed[1].starts_with("3 text files"), "{passed:?}");
    }

    #[test]
    fn a_source_file_without_an_spdx_line_fails() {
        let found = problems("missing", &[("src/a.rs", "fn f() {}\n".to_owned())]);
        assert_eq!(found.len(), 1);
        assert!(
            found[0].starts_with("src/a.rs: does not start with"),
            "{found:?}"
        );
    }

    #[test]
    fn an_spdx_line_below_other_comments_fails() {
        let text = format!("// Title\n// {}\n", tag("GPL-3.0-or-later"));
        let found = problems("late", &[("a.sh", text)]);
        assert_eq!(found.len(), 1, "{found:?}");
    }

    #[test]
    fn a_disallowed_license_fails_in_source_and_text() {
        let rs = format!("// {}\n", tag("MIT"));
        let md = format!("Text\n\n<!-- {} -->\n", tag("GPL-2.0-only"));
        let found = problems("disallowed", &[("a.rs", rs), ("b.md", md)]);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(
            found[0].starts_with("a.rs:1: SPDX license \"MIT\""),
            "{found:?}"
        );
        assert!(
            found[1].starts_with("b.md:3: SPDX license \"GPL-2.0-only\""),
            "{found:?}"
        );
    }

    #[test]
    fn a_cached_spec_is_classified_by_the_specs_manifest() {
        let manifest = "path\tspec\nspecs/mcp/index.mdx\tMCP\nspecs/mcp/schema.ts\tMCP\n";
        let files = [
            (SPECS_MANIFEST, manifest.to_owned()),
            ("specs/mcp/index.mdx", format!("{{/* {} */}}\n", tag("MIT"))),
            ("specs/mcp/schema.ts", "export {};\n".to_owned()),
            ("specs/mcp/unlisted.mdx", String::new()),
        ];
        let (root, listed) = fixture("cached", &files);
        let report = check(&root, &listed).unwrap();
        assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
        assert!(
            report.problems[0].starts_with("specs/mcp/unlisted.mdx: no license rule"),
            "{:?}",
            report.problems
        );
        assert!(
            report.passed[3].starts_with("2 cached specification files"),
            "{:?}",
            report.passed
        );
    }

    #[test]
    fn an_unclassified_file_type_fails() {
        let found = problems("unknown", &[("tools/run.exe", String::new())]);
        assert_eq!(found.len(), 1);
        assert!(found[0].contains("no license rule"), "{found:?}");
    }

    #[test]
    fn a_ported_header_violation_fails() {
        let text = format!(
            "// {}\n// Ported from id-Software/Quake@{COMMIT}: a.c, F\n",
            tag("GPL-3.0-or-later")
        );
        let found = problems("ported", &[("a.rs", text)]);
        assert_eq!(found.len(), 1);
        assert!(
            found[0].starts_with("a.rs: ported-file header rule H4"),
            "{found:?}"
        );
    }

    #[test]
    fn a_missing_or_wrong_license_file_fails() {
        let (root, mut listed) = fixture("license", &[]);
        listed.retain(|path| path != LICENSE_FILE);
        assert_eq!(check(&root, &listed).unwrap().problems.len(), 1);
        fs::write(root.join(LICENSE_FILE), "MIT License\n").unwrap();
        listed.push(LICENSE_FILE.to_owned());
        assert_eq!(check(&root, &listed).unwrap().problems.len(), 1);
    }

    #[test]
    fn a_missing_porting_doc_is_an_error() {
        let (root, listed) = fixture("no-doc", &[]);
        fs::remove_file(root.join(ported::DOC)).unwrap();
        assert!(check(&root, &listed).is_err());
    }

    #[test]
    fn spdx_tags_are_read_from_comments_only() {
        assert_eq!(spdx_expression(&format!("// {}", tag("MIT"))), Some("MIT"));
        assert_eq!(
            spdx_expression(&format!("/* {} */", tag("MIT"))),
            Some("MIT")
        );
        assert_eq!(
            spdx_expression(&format!("<!-- {} -->", tag("MIT"))),
            Some("MIT")
        );
        assert_eq!(spdx_expression(&format!(" * {}", tag("MIT"))), Some("MIT"));
        assert_eq!(
            spdx_expression(&format!("let s = \"{}\";", tag("MIT"))),
            None
        );
        assert_eq!(spdx_expression(&format!("-- {}", tag("MIT"))), Some("MIT"));
        assert_eq!(spdx_expression(&format!("; {}", tag("MIT"))), Some("MIT"));
        assert_eq!(spdx_expression("// nothing"), None);
    }

    #[test]
    fn files_are_classified_by_name_then_extension() {
        assert_eq!(classify("Makefile"), Kind::Source);
        assert_eq!(classify("x/y.RS"), Kind::Source);
        assert_eq!(classify("src/tests/sample.qc"), Kind::Source);
        assert_eq!(classify("editors/vscode/src/extension.ts"), Kind::Source);
        assert_eq!(classify("editors/neovim/lsp/quakec.lua"), Kind::Source);
        assert_eq!(
            classify("editors/zed/languages/quakec/highlights.scm"),
            Kind::Source
        );
        assert_eq!(classify("LICENSE"), Kind::Text);
        assert_eq!(classify(".gitignore"), Kind::Text);
        assert_eq!(classify("Cargo.lock"), Kind::Text);
        assert_eq!(classify("a/b.png"), Kind::Asset);
        assert_eq!(classify("a/b"), Kind::Unknown);
    }
}
