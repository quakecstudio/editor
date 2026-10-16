// SPDX-License-Identifier: GPL-3.0-or-later

//! The spec-citation ratchet (CONV-11, TEST-3).
//!
//! Every normative statement in `docs/spec/*.md` (a list item starting `- **PREFIX-n.m**`) must
//! be cited by a `// spec PREFIX-n.m` comment in a Rust file under `crates/` or `apps/`, or be
//! listed in `docs/spec/coverage-baseline.tsv`. The ratchet only turns one way: a statement
//! that loses its last citation fails, and a baseline entry that gains a citation fails until
//! it is removed from the baseline. `docs/spec/requirements.tsv` is the index of every
//! statement and must match the spec files. The format is in `docs/spec/README.md`.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code: a failed expect or panic is a failed test with a message"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const REQUIREMENTS_HEADER: &str = "id\tspec\tsection\tstatus\tstatement";
const BASELINE_HEADER: &str = "id";

/// One normative statement from a spec file.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Statement {
    id: String,
    spec: String,
    section: String,
    withdrawn: bool,
    text: String,
}

/// One `// spec` citation and where it is.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Citation {
    id: String,
    location: String,
}

/// `PREFIX-n`: an upper-case prefix (letters and digits, starting with a letter), a hyphen
/// and a number.
fn is_section_id(id: &str) -> bool {
    let Some((prefix, number)) = id.split_once('-') else {
        return false;
    };
    let mut prefix_chars = prefix.chars();
    prefix_chars.next().is_some_and(|c| c.is_ascii_uppercase())
        && prefix_chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        && !number.is_empty()
        && number.chars().all(|c| c.is_ascii_digit())
}

/// `PREFIX-n.m`: a section ID, a dot and a number.
fn is_statement_id(id: &str) -> bool {
    id.split_once('.').is_some_and(|(section, number)| {
        is_section_id(section) && !number.is_empty() && number.chars().all(|c| c.is_ascii_digit())
    })
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Parses the statements of one spec file. `spec` is the file name, used in the index and
/// in errors. Statements inside HTML comments are ignored, so a template can show one.
fn parse_spec(spec: &str, text: &str) -> Result<Vec<Statement>, String> {
    let mut statements: Vec<Statement> = Vec::new();
    let mut section: Option<String> = None;
    let mut in_comment = false;
    let mut open = false;
    for (line, number) in text.lines().zip(1_u32..) {
        let trimmed = line.trim_start();
        if in_comment {
            in_comment = !line.contains("-->");
            open = false;
            continue;
        }
        if trimmed.starts_with("<!--") {
            in_comment = !line.contains("-->");
            open = false;
            continue;
        }
        if let Some(heading) = line.strip_prefix("## ") {
            let id = heading.split_whitespace().next().unwrap_or_default();
            section = is_section_id(id).then(|| id.to_owned());
            open = false;
            continue;
        }
        if let Some(rest) = line.strip_prefix("- **") {
            let Some((id, body)) = rest.split_once("**") else {
                open = false;
                continue;
            };
            if !is_statement_id(id) {
                open = false;
                continue;
            }
            let Some(current) = section.as_deref() else {
                return Err(format!(
                    "{spec}:{number}: {id} is not under a `## <ID> <Title>` section"
                ));
            };
            if id.rsplit_once('.').map(|(owner, _)| owner) != Some(current) {
                return Err(format!("{spec}:{number}: {id} is under section {current}"));
            }
            let body = body.trim();
            statements.push(Statement {
                id: id.to_owned(),
                spec: spec.to_owned(),
                section: current.to_owned(),
                withdrawn: body.starts_with("**Withdrawn**"),
                text: body.to_owned(),
            });
            open = true;
            continue;
        }
        let continues = open && line.starts_with("  ") && !trimmed.is_empty();
        if continues {
            if let Some(last) = statements.last_mut() {
                last.text.push(' ');
                last.text.push_str(trimmed);
            }
        } else {
            open = false;
        }
    }
    for statement in &mut statements {
        statement.text = collapse_whitespace(&statement.text);
    }
    Ok(statements)
}

/// Parses the citations in one Rust file: lines whose first non-blank text is `// spec `,
/// followed by comma-separated statement IDs and an optional `: paraphrase`.
fn parse_citations(file: &str, text: &str) -> Result<Vec<Citation>, String> {
    let mut citations = Vec::new();
    for (line, number) in text.lines().zip(1_u32..) {
        let Some(rest) = line.trim_start().strip_prefix("// spec ") else {
            continue;
        };
        let ids = rest.split_once(':').map_or(rest, |(ids, _)| ids);
        for id in ids.split(',').map(str::trim) {
            if !is_statement_id(id) {
                return Err(format!(
                    "{file}:{number}: `{id}` is not a statement ID; cite statements as PREFIX-n.m"
                ));
            }
            citations.push(Citation {
                id: id.to_owned(),
                location: format!("{file}:{number}"),
            });
        }
    }
    Ok(citations)
}

/// Parses `coverage-baseline.tsv`: a header line, then one statement ID per line.
fn parse_baseline(text: &str) -> Result<BTreeSet<String>, String> {
    let mut lines = text.lines();
    if lines.next() != Some(BASELINE_HEADER) {
        return Err(format!(
            "coverage-baseline.tsv: the first line must be `{BASELINE_HEADER}`"
        ));
    }
    let mut ids = BTreeSet::new();
    for line in lines.filter(|line| !line.trim().is_empty()) {
        if !is_statement_id(line) || !ids.insert(line.to_owned()) {
            return Err(format!(
                "coverage-baseline.tsv: `{line}` is not a statement ID or is listed twice"
            ));
        }
    }
    Ok(ids)
}

/// The contents `requirements.tsv` must have for these statements.
fn render_requirements(statements: &[Statement]) -> String {
    let mut out = String::from(REQUIREMENTS_HEADER);
    out.push('\n');
    for statement in statements {
        let status = if statement.withdrawn {
            "withdrawn"
        } else {
            "active"
        };
        out.push_str(&format!(
            "{}\t{}\t{}\t{status}\t{}\n",
            statement.id, statement.spec, statement.section, statement.text
        ));
    }
    out
}

/// Everything wrong with the coverage of `statements` by `citations`, given the tolerated
/// `baseline`. Empty means the ratchet holds.
fn check_coverage(
    statements: &[Statement],
    citations: &[Citation],
    baseline: &BTreeSet<String>,
) -> Vec<String> {
    let mut errors = Vec::new();
    let mut by_id: BTreeMap<&str, &Statement> = BTreeMap::new();
    for statement in statements {
        if by_id.insert(&statement.id, statement).is_some() {
            errors.push(format!("{} is defined twice", statement.id));
        }
    }
    let mut cited: BTreeSet<&str> = BTreeSet::new();
    for citation in citations {
        match by_id.get(citation.id.as_str()) {
            None => errors.push(format!(
                "{}: cites {}, which no spec defines",
                citation.location, citation.id
            )),
            Some(statement) if statement.withdrawn => errors.push(format!(
                "{}: cites {}, which is withdrawn; cite the statement that replaced it",
                citation.location, citation.id
            )),
            Some(_) => {
                cited.insert(&citation.id);
            }
        }
    }
    for statement in statements.iter().filter(|s| !s.withdrawn) {
        let in_baseline = baseline.contains(&statement.id);
        let is_cited = cited.contains(statement.id.as_str());
        if !is_cited && !in_baseline {
            errors.push(format!(
                "{} ({}) is not cited by any test: add a test with `// spec {}`",
                statement.id, statement.spec, statement.id
            ));
        }
        if is_cited && in_baseline {
            errors.push(format!(
                "{} is cited now: remove it from docs/spec/coverage-baseline.tsv",
                statement.id
            ));
        }
    }
    for id in baseline {
        if by_id.get(id.as_str()).is_none_or(|s| s.withdrawn) {
            errors.push(format!(
                "{id} is in docs/spec/coverage-baseline.tsv but is not an active statement: remove it"
            ));
        }
    }
    errors
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn relative(path: &Path) -> String {
    path.strip_prefix(repo_root())
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// The spec files, sorted: every `docs/spec/*.md` except `README.md` and `_`-prefixed
/// templates.
fn spec_files() -> Vec<PathBuf> {
    let dir = repo_root().join("docs/spec");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("docs/spec is readable")
        .map(|entry| entry.expect("docs/spec entry is readable").path())
        .filter(|path| {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            path.extension().is_some_and(|ext| ext == "md")
                && name != "README.md"
                && !name.starts_with('_')
        })
        .collect();
    files.sort();
    files
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.expect("directory entry is readable").path();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if name.starts_with('.') || name == "target" {
            continue;
        }
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

fn repo_statements() -> Vec<Statement> {
    let mut statements = Vec::new();
    for path in spec_files() {
        let text = fs::read_to_string(&path).expect("spec file is readable");
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        statements.extend(parse_spec(&name, &text).unwrap_or_else(|e| panic!("{e}")));
    }
    statements
}

fn repo_citations() -> Vec<Citation> {
    let mut files = Vec::new();
    for dir in ["crates", "apps"] {
        rust_files(&repo_root().join(dir), &mut files);
    }
    files.sort();
    let mut citations = Vec::new();
    for path in files {
        let text = fs::read_to_string(&path).expect("Rust file is readable");
        citations
            .extend(parse_citations(&relative(&path), &text).unwrap_or_else(|e| panic!("{e}")));
    }
    citations
}

fn repo_baseline() -> BTreeSet<String> {
    let text = fs::read_to_string(repo_root().join("docs/spec/coverage-baseline.tsv"))
        .expect("docs/spec/coverage-baseline.tsv is readable");
    parse_baseline(&text).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn every_spec_statement_is_cited_or_in_the_baseline() {
    let errors = check_coverage(&repo_statements(), &repo_citations(), &repo_baseline());
    assert!(
        errors.is_empty(),
        "spec-citation ratchet:\n{}",
        errors.join("\n")
    );
}

#[test]
fn requirements_tsv_lists_every_statement() {
    let expected = render_requirements(&repo_statements());
    let actual = fs::read_to_string(repo_root().join("docs/spec/requirements.tsv"))
        .expect("docs/spec/requirements.tsv is readable");
    assert!(
        actual == expected,
        "docs/spec/requirements.tsv is out of date; replace it with:\n{expected}"
    );
}

#[test]
fn removing_a_spec_citation_fails_the_ratchet() {
    let statements = repo_statements();
    let mut citations = repo_citations();
    let baseline = repo_baseline();
    let target = statements
        .iter()
        .find(|s| !s.withdrawn && !baseline.contains(&s.id))
        .expect("at least one cited statement exists")
        .id
        .clone();
    citations.retain(|c| c.id != target);
    let errors = check_coverage(&statements, &citations, &baseline);
    assert!(
        errors
            .iter()
            .any(|e| e.starts_with(&format!("{target} ")) && e.contains("not cited")),
        "removing every citation of {target} did not fail the ratchet: {errors:?}"
    );
}

fn statement(id: &str, withdrawn: bool) -> Statement {
    Statement {
        id: id.to_owned(),
        spec: "t.md".to_owned(),
        section: id.rsplit_once('.').map_or(id, |(s, _)| s).to_owned(),
        withdrawn,
        text: "text".to_owned(),
    }
}

fn citation(id: &str) -> Citation {
    Citation {
        id: id.to_owned(),
        location: "x.rs:1".to_owned(),
    }
}

fn baseline(ids: &[&str]) -> BTreeSet<String> {
    ids.iter().map(|id| (*id).to_owned()).collect()
}

#[test]
fn ids_are_recognised() {
    for id in ["VM-12.3", "A-1.1", "BSP2-10.20"] {
        assert!(is_statement_id(id), "{id}");
    }
    for id in [
        "VM-12", "vm-12.3", "VM-12.", "VM-.3", "-1.1", "2VM-1.1", "VM-1.1a", "VM12.3",
    ] {
        assert!(!is_statement_id(id), "{id}");
    }
    assert!(is_section_id("VM-12"));
    assert!(!is_section_id("Overview"));
}

#[test]
fn parse_spec_joins_continuation_lines_and_marks_withdrawn() {
    let text = "# T\n\n## T-1 First\n\n- **T-1.1** One\n  continued here.\n- **T-1.2** **Withdrawn** 2026-10-15: gone; replaced by T-1.3.\n- **T-1.3** Three.\n\nProse.\n  Not a continuation.\n";
    let statements = parse_spec("t.md", text).expect("parses");
    let ids: Vec<&str> = statements.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["T-1.1", "T-1.2", "T-1.3"]);
    let first = statements.first().expect("one statement");
    assert_eq!(first.text, "One continued here.");
    assert_eq!(first.section, "T-1");
    assert!(statements.get(1).expect("two statements").withdrawn);
    assert_eq!(statements.get(2).expect("three statements").text, "Three.");
}

#[test]
fn parse_spec_ignores_statements_in_comments_and_non_statement_items() {
    let text = "## T-1 S\n\n<!-- A withdrawn statement stays in place:\n- **T-1.9** **Withdrawn**\n-->\n<!-- - **T-1.8** inline -->\n- **Note** not a statement.\n- plain item\n";
    assert_eq!(parse_spec("t.md", text), Ok(Vec::new()));
}

#[test]
fn parse_spec_rejects_statements_outside_their_section() {
    let outside = parse_spec("t.md", "# T\n\n- **T-1.1** Orphan.\n");
    assert!(outside.is_err_and(|e| e.contains("t.md:3") && e.contains("not under")));
    let wrong = parse_spec("t.md", "## T-2 S\n\n- **T-1.1** Wrong section.\n");
    assert!(wrong.is_err_and(|e| e.contains("under section T-2")));
    let plain_heading = parse_spec("t.md", "## T-1 S\n\n## Notes\n\n- **T-1.1** After it.\n");
    assert!(plain_heading.is_err());
}

#[test]
fn parse_citations_reads_lists_and_rejects_malformed_ids() {
    let text = "fn a() {}\n    // spec T-1.1, T-1.2: paraphrase\n// spec T-2.1\n/// spec T-9.9 in rustdoc is not a citation\n";
    let citations = parse_citations("x.rs", text).expect("parses");
    let found: Vec<(&str, &str)> = citations
        .iter()
        .map(|c| (c.id.as_str(), c.location.as_str()))
        .collect();
    assert_eq!(
        found,
        [
            ("T-1.1", "x.rs:2"),
            ("T-1.2", "x.rs:2"),
            ("T-2.1", "x.rs:3")
        ]
    );
    assert!(parse_citations("x.rs", "// spec T-1: section only\n").is_err());
}

#[test]
fn parse_baseline_needs_its_header_and_unique_ids() {
    assert_eq!(parse_baseline("id\n"), Ok(BTreeSet::new()));
    assert_eq!(parse_baseline("id\nT-1.1\n"), Ok(baseline(&["T-1.1"])));
    assert!(parse_baseline("").is_err());
    assert!(parse_baseline("T-1.1\n").is_err());
    assert!(parse_baseline("id\nT-1.1\nT-1.1\n").is_err());
    assert!(parse_baseline("id\nT-1\n").is_err());
}

#[test]
fn coverage_passes_when_every_active_statement_is_cited_or_tolerated() {
    let statements = [
        statement("T-1.1", false),
        statement("T-1.2", false),
        statement("T-1.3", true),
    ];
    let errors = check_coverage(&statements, &[citation("T-1.1")], &baseline(&["T-1.2"]));
    assert_eq!(errors, Vec::<String>::new());
}

#[test]
fn coverage_fails_for_an_uncited_statement() {
    let errors = check_coverage(&[statement("T-1.1", false)], &[], &baseline(&[]));
    assert_eq!(errors.len(), 1);
    assert!(
        errors
            .iter()
            .all(|e| e.contains("T-1.1") && e.contains("not cited"))
    );
}

#[test]
fn coverage_fails_for_a_cited_statement_still_in_the_baseline() {
    let errors = check_coverage(
        &[statement("T-1.1", false)],
        &[citation("T-1.1")],
        &baseline(&["T-1.1"]),
    );
    assert!(
        errors.iter().any(|e| e.contains("remove it from")),
        "{errors:?}"
    );
}

#[test]
fn coverage_fails_for_unknown_and_withdrawn_citations_and_stale_baseline() {
    let statements = [statement("T-1.1", false), statement("T-1.2", true)];
    let citations = [citation("T-1.1"), citation("T-9.9"), citation("T-1.2")];
    let errors = check_coverage(&statements, &citations, &baseline(&["T-1.2", "T-8.8"]));
    assert!(
        errors
            .iter()
            .any(|e| e.contains("T-9.9, which no spec defines")),
        "{errors:?}"
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("T-1.2, which is withdrawn")),
        "{errors:?}"
    );
    assert!(
        errors
            .iter()
            .any(|e| e.starts_with("T-1.2 is in docs/spec")),
        "{errors:?}"
    );
    assert!(
        errors
            .iter()
            .any(|e| e.starts_with("T-8.8 is in docs/spec")),
        "{errors:?}"
    );
}

#[test]
fn coverage_fails_for_a_duplicate_statement() {
    let statements = [statement("T-1.1", false), statement("T-1.1", false)];
    let errors = check_coverage(&statements, &[citation("T-1.1")], &baseline(&[]));
    assert!(
        errors.iter().any(|e| e.contains("defined twice")),
        "{errors:?}"
    );
}

#[test]
fn requirements_render_one_row_per_statement() {
    let rendered = render_requirements(&[statement("T-1.1", false), statement("T-1.2", true)]);
    assert_eq!(
        rendered,
        "id\tspec\tsection\tstatus\tstatement\nT-1.1\tt.md\tT-1\tactive\ttext\nT-1.2\tt.md\tT-1\twithdrawn\ttext\n"
    );
}
