// SPDX-License-Identifier: GPL-3.0-or-later

//! The ported-file header rule from `docs/porting.md` ("The rule
//! `cargo xtask licenses` enforces", rules H1 to H5).
//!
//! The registered upstreams and the copyright line each one requires are read
//! from the "Upstreams" table in that document, so registering an upstream is a
//! change to the document only.

pub(crate) const DOC: &str = "docs/porting.md";
const UPSTREAMS_HEADING: &str = "## Upstreams";
const PORTED_FROM: &str = "Ported from ";
const COMMIT_LEN: usize = 40;

/// An upstream registered in `docs/porting.md`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Upstream {
    /// `owner/repo`, as a `Ported from` line names it.
    pub(crate) repo: String,
    /// The copyright line a file ported from it must keep.
    pub(crate) copyright: String,
}

/// Reads the "Upstreams" table of `docs/porting.md`: the first backticked
/// `owner/repo` of each row and the backticked copyright line in its last cell.
pub(crate) fn upstreams(doc: &str) -> Result<Vec<Upstream>, String> {
    let mut upstreams = Vec::new();
    let mut in_section = false;
    for line in doc.lines() {
        if line.starts_with("## ") {
            in_section = line.trim_end() == UPSTREAMS_HEADING;
            continue;
        }
        if !in_section || !line.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = line.trim().trim_matches('|').split('|').collect();
        let (Some(first), Some(last)) = (cells.first(), cells.last()) else {
            continue;
        };
        let repo = backticked(first).find(|text| text.contains('/'));
        let copyright = backticked(last).find(|text| text.starts_with("Copyright"));
        if let (Some(repo), Some(copyright)) = (repo, copyright) {
            upstreams.push(Upstream {
                repo: repo.to_owned(),
                copyright: copyright.to_owned(),
            });
        }
    }
    if upstreams.is_empty() {
        return Err(format!(
            "{DOC} has no upstream rows under {UPSTREAMS_HEADING:?}"
        ));
    }
    Ok(upstreams)
}

/// The spans between pairs of backticks in `text`.
fn backticked(text: &str) -> impl Iterator<Item = &str> {
    text.split('`').skip(1).step_by(2)
}

/// The header of a source file: its leading comment lines (`//` or `#`), or
/// its first `/* */` block, ending at the first blank or non-comment line.
pub(crate) fn header(text: &str) -> Vec<&str> {
    let mut lines = text.lines();
    let mut header = Vec::new();
    let Some(first) = lines.next() else {
        return header;
    };
    if first.trim_start().starts_with("/*") {
        header.push(first);
        if first.contains("*/") {
            return header;
        }
        for line in lines {
            header.push(line);
            if line.contains("*/") {
                break;
            }
        }
        return header;
    }
    for line in std::iter::once(first).chain(lines) {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || !(trimmed.starts_with("//") || trimmed.starts_with('#')) {
            break;
        }
        header.push(line);
    }
    header
}

/// The problems rules H1 to H5 find in a file's header, each starting with the
/// rule's name. An empty result means the header passes.
pub(crate) fn check_header(header: &[&str], upstreams: &[Upstream]) -> Vec<String> {
    let mut problems = Vec::new();
    let citing: Vec<&str> = header
        .iter()
        .copied()
        .filter(|line| line.to_lowercase().contains("ported from"))
        .collect();
    let ported = !citing.is_empty()
        || header.iter().any(|line| {
            line.to_lowercase().contains("id software") || line.contains("id-Software/")
        });
    if !ported {
        return problems;
    }

    let mut cited = Vec::new();
    for line in &citing {
        match citation(line) {
            Some(repo) => cited.push(repo),
            None => problems.push(format!(
                "H2: {:?} is not `Ported from <owner>/<repo>@<40-hex commit>: <path>, \
                 <Function>[, <Function>...]`",
                line.trim()
            )),
        }
    }
    if citing.is_empty() {
        problems.push(
            "H3: the header mentions id's code but has no `Ported from <owner>/<repo>@<commit>: \
             <path>, <Function>` line"
                .to_owned(),
        );
    }

    // The copyright lines the cited upstreams require, each once, in the order of the
    // `Ported from` lines; two upstreams of one owner can share a line.
    let mut required: Vec<&Upstream> = Vec::new();
    for repo in cited {
        let Some(upstream) = upstreams.iter().find(|upstream| upstream.repo == repo) else {
            problems.push(format!(
                "H5: upstream {repo} is not registered in the Upstreams table of {DOC}"
            ));
            continue;
        };
        if !required.iter().any(|r| r.copyright == upstream.copyright) {
            required.push(upstream);
        }
    }
    problems.extend(check_copyrights(header, &required));
    problems.dedup();
    problems
}

/// Rule H4: the header carries each of `required`'s copyright lines, and they are its first
/// `Copyright` lines, in `required`'s order, so every upstream's line is above ours.
fn check_copyrights(header: &[&str], required: &[&Upstream]) -> Vec<String> {
    let copyrights: Vec<String> = header
        .iter()
        .filter(|line| line.contains("Copyright"))
        .map(|line| collapse_spaces(line))
        .collect();
    let mut problems = Vec::new();
    for upstream in required {
        let want = collapse_spaces(&upstream.copyright);
        if !copyrights.iter().any(|line| line.contains(&want)) {
            problems.push(format!(
                "H4: cites {} but the header lacks its copyright line `{}`",
                upstream.repo, upstream.copyright
            ));
        }
    }
    if !problems.is_empty() {
        return problems;
    }
    let misplaced = required
        .iter()
        .zip(&copyrights)
        .find(|(upstream, line)| !line.contains(&collapse_spaces(&upstream.copyright)));
    if let Some((upstream, _)) = misplaced {
        problems.push(format!(
            "H4: `{}` must come before any other Copyright line, the upstreams' lines in the \
             order of their `Ported from` lines",
            upstream.copyright
        ));
    }
    problems
}

/// The `owner/repo` a line cites if it matches rule H2:
/// `^(//|#|\s*\*)?\s*Ported from [\w.-]+/[\w.-]+@[0-9a-f]{40}: \S+, \w+(, \w+)*$`.
fn citation(line: &str) -> Option<&str> {
    let rest = if let Some(rest) = line.strip_prefix("//") {
        rest
    } else if let Some(rest) = line.strip_prefix('#') {
        rest
    } else {
        let trimmed = line.trim_start();
        trimmed.strip_prefix('*').unwrap_or(trimmed)
    };
    let rest = rest.trim_start().strip_prefix(PORTED_FROM)?;
    let (repo, rest) = rest.split_once('@')?;
    let (owner, name) = repo.split_once('/')?;
    if !is_repo_part(owner) || !is_repo_part(name) {
        return None;
    }
    let (commit, rest) = rest.split_at_checked(COMMIT_LEN)?;
    if !commit
        .chars()
        .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
    {
        return None;
    }
    let rest = rest.strip_prefix(": ")?;
    let (path, functions) = rest.split_once(' ')?;
    let path = path.strip_suffix(',')?;
    if path.is_empty() || path.contains(char::is_whitespace) {
        return None;
    }
    functions
        .split(", ")
        .all(|function| !function.is_empty() && function.chars().all(is_word))
        .then_some(repo)
}

fn is_repo_part(part: &str) -> bool {
    !part.is_empty() && part.chars().all(|c| is_word(c) || c == '.' || c == '-')
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn collapse_spaces(text: &str) -> String {
    text.split(' ')
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::string_slice,
        reason = "test code: a failed unwrap, index or slice is a failed test"
    )]

    use std::path::Path;

    use super::*;

    const COMMIT: &str = "bf4ac424ce754894ac8f1dae6a3981954bc9852d";

    fn porting_doc() -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(DOC);
        std::fs::read_to_string(path).unwrap()
    }

    fn id_upstreams() -> Vec<Upstream> {
        upstreams(&porting_doc()).unwrap()
    }

    fn rules(text: &str) -> Vec<String> {
        check_header(&header(text), &id_upstreams())
            .iter()
            .map(|problem| problem[..2].to_owned())
            .collect()
    }

    /// The rust blocks in the "File headers" section of `docs/porting.md`,
    /// each with the rule the prose before it says it fails (`None` = passes).
    fn doc_fixtures(doc: &str) -> Vec<(Option<String>, String)> {
        let section = doc
            .split("\n## ")
            .find(|section| section.starts_with("File headers"))
            .unwrap();
        let mut fixtures = Vec::new();
        let mut prose = "";
        let mut block: Option<String> = None;
        for line in section.lines() {
            match (&mut block, line.trim()) {
                (None, "```rust") => block = Some(String::new()),
                (Some(text), "```") => {
                    let expect = prose
                        .strip_prefix("Fails ")
                        .map(|rest| rest[..2].to_owned());
                    fixtures.push((expect, std::mem::take(text)));
                    block = None;
                }
                (Some(text), _) => {
                    text.push_str(line);
                    text.push('\n');
                }
                (None, trimmed) if !trimmed.is_empty() => prose = trimmed,
                (None, _) => {}
            }
        }
        fixtures
    }

    #[test]
    fn upstreams_come_from_the_porting_table() {
        let upstreams = id_upstreams();
        let repos: Vec<&str> = upstreams.iter().map(|u| u.repo.as_str()).collect();
        assert_eq!(
            repos,
            [
                "id-Software/Quake",
                "id-Software/Quake-Tools",
                "fte-team/fteqw"
            ]
        );
        let copyrights: Vec<&str> = upstreams.iter().map(|u| u.copyright.as_str()).collect();
        assert_eq!(
            copyrights,
            [
                "Copyright (C) 1996-1997 Id Software, Inc.",
                "Copyright (C) 1996-1997 Id Software, Inc.",
                "Copyright (c) 2004-2025 FTE's team and its contributors",
            ]
        );
        assert!(super::upstreams("# no table\n").is_err());
        let elsewhere = "## Other\n| `a/b` | `Copyright (C) x` |\n## Upstreams\n\
                         `c/d` | `Copyright (C) y` |\n| `e/f` | `Copyright (C) z` |\n";
        let found = super::upstreams(elsewhere).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].repo, "e/f");
    }

    // The fixtures are the passing and failing headers in docs/porting.md,
    // read from the document so the two cannot drift apart.
    #[test]
    fn porting_doc_headers_pass_and_fail_as_the_doc_says() {
        let fixtures = doc_fixtures(&porting_doc());
        let expected: Vec<Option<&str>> = fixtures
            .iter()
            .map(|(expect, _)| expect.as_deref())
            .collect();
        assert_eq!(
            expected,
            [None, None, Some("H4"), Some("H3"), Some("H2"), Some("H5")]
        );
        for (expect, text) in &fixtures {
            let want: Vec<String> = expect.iter().cloned().collect();
            assert_eq!(rules(text), want, "fixture:\n{text}");
        }
    }

    #[test]
    fn files_that_do_not_mention_id_are_not_ported() {
        assert!(rules("// SPDX-License-Identifier: GPL-3.0-or-later\n//! Our code.\n").is_empty());
        assert!(rules("").is_empty());
    }

    #[test]
    fn h1_triggers_on_any_case_and_on_the_github_owner() {
        assert_eq!(rules("// PORTED FROM somewhere\n"), ["H2"]);
        assert_eq!(rules("// Based on ID SOFTWARE code\n"), ["H3"]);
        assert_eq!(rules("// See id-Software/Quake\n"), ["H3"]);
        assert_eq!(
            rules("// SPDX\n\n// Based on id Software's code\n"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn h2_accepts_each_comment_style_and_several_functions() {
        let id = "Copyright (C) 1996-1997 Id Software, Inc.";
        for (open, close, mark) in [("", "", "//"), ("", "", "#"), ("/*\n", " */\n", " *")] {
            let text = format!(
                "{open}{mark} {id}\n{mark} Ported from id-Software/Quake@{COMMIT}: \
                 WinQuake/pr_exec.c, PR_ExecuteProgram, PR_EnterFunction\n{close}"
            );
            assert!(rules(&text).is_empty(), "{text}");
        }
    }

    #[test]
    fn h2_rejects_each_malformed_part() {
        for line in [
            format!("Ported from id-Software/Quake@{}: a.c, F", &COMMIT[1..]),
            format!(
                "Ported from id-Software/Quake@{}: a.c, F",
                COMMIT.to_uppercase()
            ),
            format!("Ported from id-Software@{COMMIT}: a.c, F"),
            format!("Ported from id-Software/Qu ake@{COMMIT}: a.c, F"),
            format!("Ported from id-Software/Quake@{COMMIT} a.c, F"),
            format!("Ported from id-Software/Quake@{COMMIT}: a.c F"),
            format!("Ported from id-Software/Quake@{COMMIT}: a.c,"),
            format!("Ported from id-Software/Quake@{COMMIT}: a.c, F,"),
            format!("Ported from id-Software/Quake@{COMMIT}: a.c, F G"),
            format!("Ported from id-Software/Quake@{COMMIT}: a.c, F "),
            format!("ported from id-Software/Quake@{COMMIT}: a.c, F"),
            format!("Ported from id-Software/Quake@{COMMIT}: , F"),
        ] {
            let text = format!("// Copyright (C) 1996-1997 Id Software, Inc.\n// {line}\n");
            assert_eq!(rules(&text), ["H2"], "{line}");
        }
    }

    #[test]
    fn h4_needs_the_upstream_line_first_with_spaces_collapsed() {
        let cite = format!("// Ported from id-Software/Quake@{COMMIT}: a.c, F\n");
        let ours = "// Copyright (C) 2026 QuakeC Studio contributors\n";
        let id = "// Copyright (C) 1996-1997  Id Software,   Inc.\n";
        assert!(rules(&format!("{id}{ours}{cite}")).is_empty());
        assert_eq!(rules(&format!("{ours}{id}{cite}")), ["H4"]);
        assert_eq!(rules(&format!("{ours}{cite}")), ["H4"]);
    }

    const FTE_COMMIT: &str = "f937b9d88f71fc4429db5fe56c6a98d922711b2e";
    const ID_LINE: &str = "// Copyright (C) 1996-1997 Id Software, Inc.\n";
    const FTE_LINE: &str = "// Copyright (c) 2004-2025 FTE's team and its contributors\n";
    const OURS: &str = "// Copyright (C) 2026 QuakeC Studio contributors\n";

    fn two_upstreams(copyrights: &str) -> String {
        format!(
            "// SPDX-License-Identifier: GPL-3.0-or-later\n{copyrights}//\n\
             // Ported from id-Software/Quake@{COMMIT}: WinQuake/pr_edict.c, ED_NewString\n\
             // Ported from fte-team/fteqw@{FTE_COMMIT}: engine/qclib/qcc_pr_lex.c, QCC_PR_Lex\n"
        )
    }

    #[test]
    fn h4_passes_two_upstreams_with_both_lines_in_citation_order() {
        assert!(rules(&two_upstreams(&format!("{ID_LINE}{FTE_LINE}{OURS}"))).is_empty());
    }

    #[test]
    fn h4_fails_two_upstreams_with_either_line_missing() {
        let missing_fte = check_header(
            &header(&two_upstreams(&format!("{ID_LINE}{OURS}"))),
            &id_upstreams(),
        );
        assert_eq!(missing_fte.len(), 1, "{missing_fte:?}");
        assert!(missing_fte[0].contains("fte-team/fteqw"), "{missing_fte:?}");
        let missing_id = check_header(
            &header(&two_upstreams(&format!("{FTE_LINE}{OURS}"))),
            &id_upstreams(),
        );
        assert_eq!(missing_id.len(), 1, "{missing_id:?}");
        assert!(
            missing_id[0].contains("id-Software/Quake"),
            "{missing_id:?}"
        );
    }

    #[test]
    fn h4_fails_two_upstreams_out_of_order_or_below_ours() {
        assert_eq!(
            rules(&two_upstreams(&format!("{FTE_LINE}{ID_LINE}{OURS}"))),
            ["H4"]
        );
        assert_eq!(
            rules(&two_upstreams(&format!("{ID_LINE}{OURS}{FTE_LINE}"))),
            ["H4"]
        );
    }

    #[test]
    fn two_id_upstreams_share_one_copyright_line() {
        let text = format!(
            "{ID_LINE}{OURS}// Ported from id-Software/Quake@{COMMIT}: WinQuake/pr_edict.c, F\n\
             // Ported from id-Software/Quake-Tools@{COMMIT}: qcc/pr_lex.c, G\n"
        );
        assert!(rules(&text).is_empty(), "{text}");
    }

    #[test]
    fn header_stops_at_the_first_blank_or_code_line() {
        let text = "#!/bin/sh\n# a\n#\n# b\n\n# not header\n";
        assert_eq!(header(text), ["#!/bin/sh", "# a", "#", "# b"]);
        assert_eq!(header("// a\nfn f() {}\n// b\n"), ["// a"]);
        assert_eq!(header("/* a\n * b\n */\n// c\n"), ["/* a", " * b", " */"]);
        assert_eq!(header("/* a */\n// c\n"), ["/* a */"]);
        assert!(header("fn f() {}\n").is_empty());
    }

    #[test]
    fn a_ported_line_outside_the_header_is_ignored() {
        let text = "// SPDX-License-Identifier: GPL-3.0-or-later\n\nfn f() {}\n// Ported from x\n";
        assert!(rules(text).is_empty());
    }
}
