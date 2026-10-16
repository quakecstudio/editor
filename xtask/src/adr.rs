// SPDX-License-Identifier: GPL-3.0-or-later

//! Checks the architecture decision records in `docs/adr/` against ADR-0001.
//!
//! Every `NNNN-slug.md` file must open with `# ADR-NNNN: Title` matching its
//! number, carry the header fields and the sections of `0000-template.md` in
//! order, and have a valid status. The template must carry the same sections, so
//! the template and this check cannot drift apart. The index in `README.md`,
//! between the `adr:index` markers, must list every ADR in order with the title
//! and status its file states.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use crate::Report;

const DIR: &str = "docs/adr";
const TEMPLATE: &str = "0000-template.md";
const INDEX: &str = "README.md";
const INDEX_START: &str = "<!-- adr:index -->";
const INDEX_END: &str = "<!-- /adr:index -->";
const INDEX_HEADER: [&str; 2] = ["| ADR | Title | Status |", "| --- | --- | --- |"];
const HEADER_FIELDS: [&str; 5] = ["Status", "Date", "Decided by", "Issue", "Requirements"];
const SECTIONS: [&str; 6] = [
    "Question",
    "Context",
    "Options",
    "Decision",
    "Consequences",
    "Links",
];
const STATUSES: [&str; 4] = ["Proposed", "Accepted", "Rejected", "Deprecated"];
const SUPERSEDED_BY: &str = "Superseded by ADR-";

/// What an ADR file says about itself, as the index must repeat it.
#[derive(Debug, PartialEq, Eq)]
struct Adr {
    number: String,
    file: String,
    title: String,
    status: String,
}

impl Adr {
    fn index_row(&self) -> String {
        format!(
            "| [{}]({}) | {} | {} |",
            self.number, self.file, self.title, self.status
        )
    }
}

pub(crate) fn check(root: &Path) -> Result<Report, String> {
    let dir = root.join(DIR);
    let mut files = BTreeMap::new();
    for entry in fs::read_dir(&dir).map_err(|error| format!("{DIR}: {error}"))? {
        let entry = entry.map_err(|error| format!("{DIR}: {error}"))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let text =
            fs::read_to_string(entry.path()).map_err(|error| format!("{DIR}/{name}: {error}"))?;
        files.insert(name, text);
    }
    Ok(check_files(&files))
}

fn check_files(files: &BTreeMap<String, String>) -> Report {
    let mut report = Report::default();
    match files.get(TEMPLATE) {
        Some(text) => record_all(
            &mut report,
            &format!("{DIR}/{TEMPLATE} has the header fields and sections"),
            template_problems(text),
        ),
        None => report.record(false, format!("{DIR}/{TEMPLATE} is missing")),
    }

    let mut adrs = Vec::new();
    for (name, text) in files {
        if name == TEMPLATE || name == INDEX {
            continue;
        }
        let Some(number) = file_number(name) else {
            report.record(
                false,
                format!("{DIR}/{name}: an ADR file must be named NNNN-slug.md"),
            );
            continue;
        };
        let (adr, problems) = parse_adr(name, number, text);
        record_all(
            &mut report,
            &format!("{DIR}/{name} follows the template"),
            problems,
        );
        if let Some(adr) = adr {
            adrs.push(adr);
        }
    }

    record_all(
        &mut report,
        "ADR numbers are unique and superseding ADRs exist",
        numbering_problems(&adrs),
    );
    match files.get(INDEX) {
        Some(text) => record_all(
            &mut report,
            &format!("{DIR}/{INDEX} indexes every ADR"),
            index_problems(text, &adrs),
        ),
        None => report.record(false, format!("{DIR}/{INDEX} is missing")),
    }
    report
}

fn record_all(report: &mut Report, label: &str, problems: Vec<String>) {
    if problems.is_empty() {
        report.record(true, label.to_owned());
    }
    for problem in problems {
        report.record(false, problem);
    }
}

/// The number of an ADR file named `NNNN-slug.md`, other than the template's 0000.
fn file_number(name: &str) -> Option<&str> {
    let (number, slug) = name.strip_suffix(".md")?.split_once('-')?;
    let well_formed = is_adr_number(number)
        && number != "0000"
        && !slug.is_empty()
        && slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    well_formed.then_some(number)
}

fn is_adr_number(text: &str) -> bool {
    text.len() == 4 && text.chars().all(|c| c.is_ascii_digit())
}

fn parse_adr(name: &str, number: &str, text: &str) -> (Option<Adr>, Vec<String>) {
    let mut problems = Vec::new();
    let title = text
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("# ADR-"))
        .and_then(|rest| rest.split_once(": "));
    let title = match title {
        Some((n, title)) if n == number && !title.trim().is_empty() => Some(title.trim()),
        _ => {
            problems.push(format!(
                "{DIR}/{name}: the first line must be \"# ADR-{number}: <title>\""
            ));
            None
        }
    };

    let fields = header_fields(text);
    problems.extend(missing_fields(name, &fields));
    if let Some(date) = fields.get("Date")
        && !is_date(date)
    {
        problems.push(format!("{DIR}/{name}: Date {date:?} is not YYYY-MM-DD"));
    }
    let status = fields.get("Status").copied();
    if let Some(status) = status
        && !is_status(status)
    {
        problems.push(format!(
            "{DIR}/{name}: Status {status:?} is not one of {}, or {SUPERSEDED_BY}NNNN",
            STATUSES.join(", ")
        ));
    }
    problems.extend(section_problems(name, text));

    let adr = title.zip(status).map(|(title, status)| Adr {
        number: number.to_owned(),
        file: name.to_owned(),
        title: title.to_owned(),
        status: status.to_owned(),
    });
    (adr, problems)
}

fn template_problems(text: &str) -> Vec<String> {
    let mut problems = missing_fields(TEMPLATE, &header_fields(text));
    problems.extend(section_problems(TEMPLATE, text));
    problems
}

/// The `- **Field:** value` lines before the first section.
fn header_fields(text: &str) -> BTreeMap<&str, &str> {
    text.lines()
        .take_while(|line| !line.starts_with("## "))
        .filter_map(|line| line.strip_prefix("- **"))
        .filter_map(|rest| rest.split_once(":**"))
        .map(|(field, value)| (field, value.trim()))
        .collect()
}

fn missing_fields(name: &str, fields: &BTreeMap<&str, &str>) -> Vec<String> {
    HEADER_FIELDS
        .iter()
        .filter(|field| fields.get(*field).is_none_or(|value| value.is_empty()))
        .map(|field| format!("{DIR}/{name}: header field \"- **{field}:**\" is missing or empty"))
        .collect()
}

/// Each required section must appear exactly once, in template order; other
/// sections may sit between them.
fn section_problems(name: &str, text: &str) -> Vec<String> {
    let headings: Vec<&str> = text
        .lines()
        .filter_map(|line| line.strip_prefix("## "))
        .map(str::trim)
        .collect();
    let mut problems = Vec::new();
    for section in SECTIONS {
        let count = headings.iter().filter(|h| **h == section).count();
        if count != 1 {
            problems.push(format!(
                "{DIR}/{name}: section \"## {section}\" appears {count} times, not once"
            ));
        }
    }
    let order: Vec<&str> = headings
        .iter()
        .copied()
        .filter(|h| SECTIONS.contains(h))
        .collect();
    if problems.is_empty() && order != SECTIONS {
        problems.push(format!(
            "{DIR}/{name}: sections must come in the order {}",
            SECTIONS.join(", ")
        ));
    }
    problems
}

fn is_date(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    let lengths: Vec<usize> = parts.iter().map(|part| part.len()).collect();
    lengths == [4, 2, 2]
        && parts
            .iter()
            .all(|part| part.chars().all(|c| c.is_ascii_digit()))
}

fn is_status(text: &str) -> bool {
    STATUSES.contains(&text) || text.strip_prefix(SUPERSEDED_BY).is_some_and(is_adr_number)
}

fn numbering_problems(adrs: &[Adr]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut seen = BTreeSet::new();
    for adr in adrs {
        if !seen.insert(adr.number.as_str()) {
            problems.push(format!(
                "{DIR}/{}: ADR-{} is used by more than one file",
                adr.file, adr.number
            ));
        }
    }
    for adr in adrs {
        if let Some(by) = adr.status.strip_prefix(SUPERSEDED_BY)
            && !seen.contains(by)
        {
            problems.push(format!(
                "{DIR}/{}: superseded by ADR-{by}, which does not exist",
                adr.file
            ));
        }
    }
    problems
}

fn index_problems(text: &str, adrs: &[Adr]) -> Vec<String> {
    let Some(rows) = index_rows(text) else {
        return vec![format!(
            "{DIR}/{INDEX}: needs a table between {INDEX_START} and {INDEX_END} \
             starting with the header {:?}",
            INDEX_HEADER.join("\n")
        )];
    };
    let expected: Vec<String> = adrs.iter().map(Adr::index_row).collect();
    let mut problems = Vec::new();
    for row in &expected {
        if !rows.contains(&row.as_str()) {
            problems.push(format!("{DIR}/{INDEX}: index needs the row {row}"));
        }
    }
    for row in &rows {
        if !expected.contains(&(*row).to_owned()) {
            problems.push(format!(
                "{DIR}/{INDEX}: index row matches no ADR file, or has the wrong title or status: {row}"
            ));
        }
    }
    if problems.is_empty() && rows != expected {
        problems.push(format!(
            "{DIR}/{INDEX}: index rows must be in ADR number order"
        ));
    }
    problems
}

/// The table rows between the index markers, after the header, or `None`
/// when the markers or the header are missing.
fn index_rows(text: &str) -> Option<Vec<&str>> {
    let (_, rest) = text.split_once(INDEX_START)?;
    let (table, _) = rest.split_once(INDEX_END)?;
    let mut lines = table.lines().map(str::trim).filter(|line| !line.is_empty());
    let header = [lines.next()?, lines.next()?];
    (header == INDEX_HEADER).then(|| lines.collect())
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use super::*;

    const GOOD: &str = "# ADR-0001: Record decisions\n\n\
        - **Status:** Accepted\n\
        - **Date:** 2026-10-15\n\
        - **Decided by:** Justin Plock\n\
        - **Issue:** #7\n\
        - **Requirements:** PM-2\n\n\
        ## Question\n\nq\n\n## Context\n\nc\n\n## Options\n\no\n\n\
        ## Decision\n\nd\n\n## Why\n\nw\n\n## Consequences\n\nc\n\n## Links\n\nl\n";

    const INDEX_GOOD: &str = "# ADRs\n\n<!-- adr:index -->\n\
        | ADR | Title | Status |\n| --- | --- | --- |\n\
        | [0001](0001-record-decisions.md) | Record decisions | Accepted |\n\
        <!-- /adr:index -->\n";

    fn template() -> String {
        GOOD.replacen("# ADR-0001: Record decisions", "# ADR-NNNN: Title", 1)
    }

    fn files(adr: &str, index: &str) -> BTreeMap<String, String> {
        BTreeMap::from([
            (TEMPLATE.to_owned(), template()),
            ("0001-record-decisions.md".to_owned(), adr.to_owned()),
            (INDEX.to_owned(), index.to_owned()),
        ])
    }

    fn problems(files: &BTreeMap<String, String>) -> Vec<String> {
        check_files(files).problems
    }

    fn assert_flags(files: &BTreeMap<String, String>, needle: &str) {
        let problems = problems(files);
        assert!(
            problems.iter().any(|p| p.contains(needle)),
            "expected {needle:?} in {problems:?}"
        );
    }

    #[test]
    fn a_conforming_set_passes() {
        let report = check_files(&files(GOOD, INDEX_GOOD));
        assert_eq!(report.problems, Vec::<String>::new());
        assert_eq!(report.passed.len(), 4);
    }

    #[test]
    fn the_real_adrs_pass() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let report = check(root).unwrap();
        assert_eq!(report.problems, Vec::<String>::new());
        assert!(
            report
                .passed
                .iter()
                .any(|line| line.ends_with("0001-record-decisions.md follows the template")),
            "{:?}",
            report.passed
        );
    }

    #[test]
    fn file_names_must_be_numbered_slugs() {
        assert_eq!(file_number("0012-ui-framework.md"), Some("0012"));
        for name in [
            "0000-template.md",
            "12-short.md",
            "0012.md",
            "0012-.md",
            "0012-Upper.md",
            "0012-x.txt",
            "notes.md",
        ] {
            assert_eq!(file_number(name), None, "{name}");
        }
        let mut set = files(GOOD, INDEX_GOOD);
        set.insert("notes.md".to_owned(), String::new());
        assert_flags(&set, "notes.md: an ADR file must be named NNNN-slug.md");
    }

    #[test]
    fn the_title_must_match_the_file_number() {
        let adr = GOOD.replacen("ADR-0001", "ADR-0002", 1);
        assert_flags(
            &files(&adr, INDEX_GOOD),
            "first line must be \"# ADR-0001: <title>\"",
        );
        let adr = GOOD.replacen("# ADR-0001: Record decisions", "# Record decisions", 1);
        assert_flags(&files(&adr, INDEX_GOOD), "first line must be");
    }

    #[test]
    fn every_header_field_is_required() {
        for field in HEADER_FIELDS {
            let adr: String = GOOD
                .lines()
                .filter(|line| !line.starts_with(&format!("- **{field}:**")))
                .map(|line| format!("{line}\n"))
                .collect();
            assert_flags(
                &files(&adr, INDEX_GOOD),
                &format!("\"- **{field}:**\" is missing or empty"),
            );
        }
        let template = template().replacen("- **Issue:** #7\n", "", 1);
        let mut set = files(GOOD, INDEX_GOOD);
        set.insert(TEMPLATE.to_owned(), template);
        assert_flags(&set, "0000-template.md: header field \"- **Issue:**\"");
    }

    #[test]
    fn status_and_date_must_be_well_formed() {
        for status in [
            "Superseded by ADR-0001",
            "Proposed",
            "Rejected",
            "Deprecated",
        ] {
            assert!(is_status(status), "{status}");
        }
        for status in ["accepted", "Superseded by ADR-1", "Superseded", ""] {
            assert!(!is_status(status), "{status}");
        }
        let adr = GOOD.replacen("Accepted", "Approved", 1);
        assert_flags(
            &files(&adr, INDEX_GOOD),
            "Status \"Approved\" is not one of",
        );
        let adr = GOOD.replacen("2026-10-15", "15 Oct 2026", 1);
        assert_flags(
            &files(&adr, INDEX_GOOD),
            "Date \"15 Oct 2026\" is not YYYY-MM-DD",
        );
        assert!(!is_date("2026-1-15"));
    }

    #[test]
    fn sections_must_appear_once_and_in_order() {
        let adr = GOOD.replacen("## Context\n", "## Background\n", 1);
        assert_flags(&files(&adr, INDEX_GOOD), "\"## Context\" appears 0 times");
        let adr = format!("{GOOD}\n## Links\n\nagain\n");
        assert_flags(&files(&adr, INDEX_GOOD), "\"## Links\" appears 2 times");
        let adr = GOOD
            .replacen("## Options\n", "## TEMP\n", 1)
            .replacen("## Decision\n", "## Options\n", 1)
            .replacen("## TEMP\n", "## Decision\n", 1);
        assert_flags(&files(&adr, INDEX_GOOD), "sections must come in the order");
        let mut set = files(GOOD, INDEX_GOOD);
        set.insert(
            TEMPLATE.to_owned(),
            template().replacen("## Links\n", "", 1),
        );
        assert_flags(
            &set,
            "0000-template.md: section \"## Links\" appears 0 times",
        );
    }

    #[test]
    fn numbers_are_unique_and_supersessions_resolve() {
        let mut set = files(GOOD, INDEX_GOOD);
        set.insert("0001-duplicate.md".to_owned(), GOOD.to_owned());
        assert_flags(&set, "ADR-0001 is used by more than one file");
        let adr = GOOD.replacen("Accepted", "Superseded by ADR-0009", 1);
        let index = INDEX_GOOD.replacen("Accepted", "Superseded by ADR-0009", 1);
        assert_flags(
            &files(&adr, &index),
            "superseded by ADR-0009, which does not exist",
        );
    }

    #[test]
    fn the_index_must_match_the_files() {
        let index = INDEX_GOOD.replacen("| Accepted |", "| Proposed |", 1);
        let found = problems(&files(GOOD, &index));
        assert!(
            found
                .iter()
                .any(|p| p.contains("index needs the row | [0001]")),
            "{found:?}"
        );
        assert!(
            found
                .iter()
                .any(|p| p.contains("has the wrong title or status")),
            "{found:?}"
        );

        let index = INDEX_GOOD.replacen(
            "<!-- /adr:index -->",
            "| [0002](0002-x.md) | X | Accepted |\n<!-- /adr:index -->",
            1,
        );
        assert_flags(&files(GOOD, &index), "matches no ADR file");

        let mut set = files(GOOD, INDEX_GOOD);
        set.insert(
            "0002-second.md".to_owned(),
            GOOD.replacen("ADR-0001: Record decisions", "ADR-0002: Second", 1),
        );
        set.insert(
            INDEX.to_owned(),
            INDEX_GOOD.replacen(
                "| [0001]",
                "| [0002](0002-second.md) | Second | Accepted |\n| [0001]",
                1,
            ),
        );
        assert_flags(&set, "index rows must be in ADR number order");

        let index = INDEX_GOOD.replacen("<!-- /adr:index -->", "", 1);
        assert_flags(&files(GOOD, &index), "needs a table between");
        let index = INDEX_GOOD.replacen("| ADR | Title | Status |", "| Number | Title |", 1);
        assert_flags(&files(GOOD, &index), "needs a table between");
    }

    #[test]
    fn the_template_and_index_are_required() {
        let mut set = files(GOOD, INDEX_GOOD);
        set.remove(TEMPLATE);
        set.remove(INDEX);
        let found = problems(&set);
        assert!(
            found
                .iter()
                .any(|p| p.ends_with("0000-template.md is missing")),
            "{found:?}"
        );
        assert!(
            found.iter().any(|p| p.ends_with("README.md is missing")),
            "{found:?}"
        );
    }
}
