// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask perf-baseline [--write]`: the clippy performance lints the audit left off
//! (`docs/audits/perf.md`, L-18) are held to `docs/audits/perf-baseline.tsv`, a count of each
//! lint's sites that only shrinks. The check runs clippy over the workspace with those lints
//! as warnings and fails when a lint has more sites than the file says. With fewer it passes
//! and says so: a platform compiles less of the workspace than the one the file was counted
//! on, and a lint whose sites were fixed has its line lowered with `--write`, which never
//! raises a count. `cargo xtask ci` runs the check after its own clippy step, whose build it
//! reuses.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use clap::Args as ClapArgs;
use serde::Deserialize;

use crate::{Report, print_outcome};

/// The baseline, from the repository's root.
pub(crate) const BASELINE: &str = "docs/audits/perf-baseline.tsv";

/// The lints held, each left off in the workspace's own lints with its reason in
/// `docs/audits/perf.md`.
const LINTS: [&str; 7] = [
    "clippy::format_collect",
    "clippy::format_push_string",
    "clippy::naive_bytecount",
    "clippy::needless_collect",
    "clippy::needless_pass_by_value",
    "clippy::string_add",
    "clippy::trivially_copy_pass_by_ref",
];

/// The baseline's header line.
const HEADER: &str = "lint\tsites";

/// The arguments of `cargo xtask perf-baseline`.
#[derive(ClapArgs, Clone, Debug, Default)]
pub(crate) struct Args {
    /// Lower each count in the baseline to the code's; a count is never raised.
    #[arg(long)]
    write: bool,
}

/// One line of clippy's `--message-format=json`.
#[derive(Deserialize)]
struct Line {
    reason: String,
    message: Option<Message>,
}

/// A diagnostic.
#[derive(Deserialize)]
struct Message {
    code: Option<Code>,
    spans: Vec<Span>,
}

/// A diagnostic's lint.
#[derive(Deserialize)]
struct Code {
    code: String,
}

/// Where a diagnostic points.
#[derive(Deserialize)]
struct Span {
    file_name: String,
    line_start: u32,
    column_start: u32,
    is_primary: bool,
}

/// How many sites each of [`LINTS`] has in clippy's JSON `output`, a site counted once however
/// many targets compile it; a lint with none is 0.
fn counted(output: &str) -> BTreeMap<String, usize> {
    let mut sites: BTreeSet<(String, String, u32, u32)> = BTreeSet::new();
    for line in output.lines() {
        let Ok(Line {
            reason,
            message: Some(message),
        }) = serde_json::from_str::<Line>(line)
        else {
            continue;
        };
        let Some(code) = message.code.map(|code| code.code) else {
            continue;
        };
        if reason != "compiler-message" || !LINTS.contains(&code.as_str()) {
            continue;
        }
        if let Some(span) = message.spans.into_iter().find(|span| span.is_primary) {
            sites.insert((code, span.file_name, span.line_start, span.column_start));
        }
    }
    let mut counts: BTreeMap<String, usize> =
        LINTS.iter().map(|lint| ((*lint).to_owned(), 0)).collect();
    for (lint, ..) in sites {
        if let Some(count) = counts.get_mut(&lint) {
            *count = count.saturating_add(1);
        }
    }
    counts
}

/// The counts a baseline's `text` holds.
fn read(text: &str) -> Result<BTreeMap<String, usize>, String> {
    let mut lines = text.lines();
    if lines.next() != Some(HEADER) {
        return Err(format!("{BASELINE}: the first line is not `{HEADER}`"));
    }
    let mut counts = BTreeMap::new();
    for line in lines.filter(|line| !line.is_empty()) {
        let entry = line
            .split_once('\t')
            .and_then(|(lint, sites)| Some((lint.to_owned(), sites.parse::<usize>().ok()?)));
        let Some((lint, sites)) = entry else {
            return Err(format!("{BASELINE}: `{line}` is not a lint and a count"));
        };
        counts.insert(lint, sites);
    }
    Ok(counts)
}

/// A baseline's text holding `counts`.
fn written(counts: &BTreeMap<String, usize>) -> String {
    let lines: Vec<String> = counts
        .iter()
        .map(|(lint, sites)| format!("{lint}\t{sites}\n"))
        .collect();
    format!("{HEADER}\n{}", lines.concat())
}

/// What the check says of the code's `found` counts against the `held` ones: a problem for a
/// lint with more sites than held or with no line.
fn compared(found: &BTreeMap<String, usize>, held: &BTreeMap<String, usize>) -> Report {
    let mut report = Report::default();
    for (lint, &sites) in found {
        match held.get(lint) {
            None => report.record(false, format!("{BASELINE}: no line for {lint}")),
            Some(&most) if sites > most => report.record(
                false,
                format!("{lint}: {sites} sites, {most} in {BASELINE}; fix the new ones"),
            ),
            // Fewer passes: a platform compiles less of the workspace than the one the
            // baseline was counted on.
            Some(&most) if sites < most => report.record(
                true,
                format!(
                    "{lint}: {sites} sites, {most} in {BASELINE}; when they were fixed, lower \
                     it with `cargo xtask perf-baseline --write`"
                ),
            ),
            Some(_) => report.record(true, format!("{lint}: {sites} sites, as {BASELINE} holds")),
        }
    }
    for lint in held.keys().filter(|lint| !found.contains_key(*lint)) {
        report.record(false, format!("{BASELINE}: {lint} is not a lint held"));
    }
    report
}

/// The counts of the workspace at `root`, by clippy with each of [`LINTS`] a warning.
fn clippy_counts(root: &Path) -> Result<BTreeMap<String, usize>, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .args([
            "clippy",
            "--locked",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--message-format=json",
            "--",
        ])
        .current_dir(root);
    for lint in LINTS {
        command.args(["-W", lint]);
    }
    let output = command.output().map_err(|e| format!("cargo clippy: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo clippy: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(counted(&String::from_utf8_lossy(&output.stdout)))
}

/// The check: the code's counts against the baseline's.
pub(crate) fn check(root: &Path) -> Result<Report, String> {
    let path = root.join(BASELINE);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(compared(&clippy_counts(root)?, &read(&text)?))
}

/// `cargo xtask perf-baseline`; whether the check passed, or the baseline was written.
pub(crate) fn main(root: &Path, args: &Args) -> Result<bool, String> {
    if !args.write {
        return Ok(print_outcome("perf-baseline", check(root)));
    }
    let path = root.join(BASELINE);
    let found = clippy_counts(root)?;
    // A count is lowered to the code's, never raised: with no file yet, the code's is taken.
    let held = match std::fs::read_to_string(&path) {
        Ok(text) => read(&text)?,
        Err(_) => found.clone(),
    };
    let lowered: BTreeMap<String, usize> = found
        .iter()
        .map(|(lint, &sites)| {
            let most = held.get(lint).copied().unwrap_or(sites);
            (lint.clone(), sites.min(most))
        })
        .collect();
    std::fs::write(&path, written(&lowered)).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("perf-baseline: wrote {BASELINE}");
    Ok(true)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "test code: a failed unwrap or index is a failed test"
    )]

    use super::*;

    /// A diagnostic line of clippy's JSON for `lint` at `file`, line `line`.
    fn diagnostic(lint: &str, file: &str, line: u32) -> String {
        format!(
            "{{\"reason\":\"compiler-message\",\"message\":{{\"code\":{{\"code\":\"{lint}\"}},\
             \"spans\":[{{\"file_name\":\"{file}\",\"line_start\":{line},\"column_start\":5,\
             \"is_primary\":true}}]}}}}"
        )
    }

    // No spec statement: L-18: a site is counted once however many targets report it, only
    // the lints held are counted, and a lint with no site is 0
    #[test]
    fn a_site_is_counted_once() {
        let output = [
            diagnostic("clippy::string_add", "a.rs", 3),
            diagnostic("clippy::string_add", "a.rs", 3),
            diagnostic("clippy::string_add", "a.rs", 9),
            diagnostic("clippy::needless_collect", "b.rs", 1),
            diagnostic("clippy::unwrap_used", "b.rs", 2),
            "{\"reason\":\"build-finished\",\"success\":true}".to_owned(),
            "not json".to_owned(),
        ]
        .join("\n");
        let counts = counted(&output);
        assert_eq!(counts.len(), LINTS.len());
        assert_eq!(counts["clippy::string_add"], 2);
        assert_eq!(counts["clippy::needless_collect"], 1);
        assert_eq!(counts["clippy::format_collect"], 0);
        assert!(!counts.contains_key("clippy::unwrap_used"));
    }

    // No spec statement: L-18: the baseline reads back as it is written, and a file that is
    // not one is refused
    #[test]
    fn a_baseline_reads_back() {
        let counts = counted(&diagnostic("clippy::string_add", "a.rs", 3));
        assert_eq!(read(&written(&counts)).unwrap(), counts);
        assert!(read("sites\n").is_err());
        assert!(read("lint\tsites\nclippy::string_add\tmany\n").is_err());
        assert!(read("lint\tsites\nclippy::string_add\n").is_err());
    }

    // No spec statement: L-18: the count only shrinks: more sites than held fail, fewer pass
    // and name the command that lowers the line; a lint with no line, or a line for no lint,
    // fails
    #[test]
    fn the_baseline_only_shrinks() {
        let held = counted(&diagnostic("clippy::string_add", "a.rs", 3));
        assert!(compared(&held, &held).problems.is_empty());
        assert_eq!(compared(&held, &held).passed.len(), LINTS.len());
        let more = counted(
            &[
                diagnostic("clippy::string_add", "a.rs", 3),
                diagnostic("clippy::string_add", "a.rs", 4),
            ]
            .join("\n"),
        );
        let grown = compared(&more, &held).problems;
        assert_eq!(grown.len(), 1);
        assert!(grown[0].contains("fix the new ones"), "{grown:?}");
        let shrunk = compared(&held, &more);
        assert!(shrunk.problems.is_empty());
        assert!(shrunk.passed.iter().any(|line| line.contains("--write")));
        let mut missing = held.clone();
        missing.remove("clippy::string_add");
        assert_eq!(compared(&held, &missing).problems.len(), 1);
        let mut extra = held.clone();
        extra.insert("clippy::other".to_owned(), 1);
        assert_eq!(compared(&held, &extra).problems.len(), 1);
    }
}
