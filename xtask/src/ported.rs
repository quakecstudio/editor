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
/// The holder our own copyright line names; every upstream's line must be above it.
const OUR_HOLDER: &str = "QuakeC Studio contributors";

/// An upstream registered in `docs/porting.md`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Upstream {
    /// `owner/repo`, as a `Ported from` line names it.
    pub(crate) repo: String,
    /// The copyright lines its row lists, at least one of which a file ported from it keeps.
    pub(crate) copyrights: Vec<String>,
}

/// Reads the "Upstreams" table of `docs/porting.md`: the first backticked
/// `owner/repo` of each row and the backticked copyright lines in its last cell.
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
        let copyrights: Vec<String> = backticked(last)
            .filter(|text| text.starts_with("Copyright"))
            .map(str::to_owned)
            .collect();
        if let Some(repo) = repo
            && !copyrights.is_empty()
        {
            upstreams.push(Upstream {
                repo: repo.to_owned(),
                copyrights,
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
        if !required.iter().any(|r| r.copyrights == upstream.copyrights) {
            required.push(upstream);
        }
    }
    problems.extend(check_copyrights(header, &required));
    problems.dedup();
    problems
}

/// Rule H4: the header carries at least one of each of `required`'s copyright lines; the
/// header's first `Copyright` line is one of the first upstream's, and each upstream's first
/// line comes in `required`'s order and above ours.
fn check_copyrights(header: &[&str], required: &[&Upstream]) -> Vec<String> {
    let copyrights: Vec<String> = header
        .iter()
        .filter(|line| line.contains("Copyright"))
        .map(|line| collapse_spaces(line))
        .collect();
    let mut firsts = Vec::new();
    let mut problems = Vec::new();
    for upstream in required {
        let first = copyrights.iter().position(|line| {
            upstream
                .copyrights
                .iter()
                .any(|want| line.contains(&collapse_spaces(want)))
        });
        match first {
            Some(at) => firsts.push((upstream, at)),
            None => problems.push(format!(
                "H4: cites {} but the header lacks its copyright line {}",
                upstream.repo,
                listed(upstream)
            )),
        }
    }
    if !problems.is_empty() {
        return problems;
    }
    let ours = copyrights
        .iter()
        .position(|line| line.contains(OUR_HOLDER))
        .unwrap_or(copyrights.len());
    // The first upstream's line opens the Copyright lines; a later upstream's may follow other
    // lines the earlier upstream files carry.
    let mut previous = 0;
    for (n, (upstream, at)) in firsts.into_iter().enumerate() {
        if at < previous || at >= ours || (n == 0 && at != 0) {
            problems.push(format!(
                "H4: {} must come before any other Copyright line, the upstreams' lines in the \
                 order of their `Ported from` lines and above ours",
                listed(upstream)
            ));
            break;
        }
        previous = at;
    }
    problems
}

/// An upstream's copyright lines as an error names them: `a`, or (one of `a`, `b`).
fn listed(upstream: &Upstream) -> String {
    let lines: Vec<String> = upstream
        .copyrights
        .iter()
        .map(|line| format!("`{line}`"))
        .collect();
    match lines.as_slice() {
        [one] => one.clone(),
        many => format!("(one of {})", many.join(", ")),
    }
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
                "fte-team/fteqw",
                "sezero/quakespasm",
                "andrei-drexler/ironwail",
                "Novum/vkQuake",
            ]
        );
        let firsts: Vec<&str> = upstreams.iter().map(|u| u.copyrights[0].as_str()).collect();
        assert_eq!(
            firsts,
            [
                "Copyright (C) 1996-1997 Id Software, Inc.",
                "Copyright (C) 1996-1997 Id Software, Inc.",
                "Copyright (c) 2004-2025 FTE's team and its contributors",
                QS_ID,
                QS_ID,
                QS_ID,
            ]
        );
        let counts: Vec<usize> = upstreams.iter().map(|u| u.copyrights.len()).collect();
        assert_eq!(counts, [1, 1, 1, 3, 5, 6]);
        assert!(upstreams[4].copyrights.contains(&SPIKE.to_owned()));
        assert!(super::upstreams("## Upstreams\n| `a/b` | `not a copyright` |\n").is_err());
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
            [
                None,
                None,
                None,
                Some("H4"),
                Some("H3"),
                Some("H2"),
                Some("H5")
            ]
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

    const QS: &str = "sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5";
    const IRONWAIL: &str = "andrei-drexler/ironwail@08d578136ff43d7d1ef38e636dfbfd3e844be7cd";
    const VKQUAKE: &str = "Novum/vkQuake@27789d7333a46674474d871137bb71e056719628";
    const ID_COPYRIGHT: &str = "Copyright (C) 1996-1997 Id Software, Inc.";
    const QS_ID: &str = "Copyright (C) 1996-2001 Id Software, Inc.";
    const FITZ: &str = "Copyright (C) 2002-2009 John Fitzgibbons and others";
    const QS_DEVS: &str = "Copyright (C) 2010-2014 QuakeSpasm developers";
    const DUSKE: &str = "Copyright (C) 2007-2008 Kristian Duske";
    const SPIKE: &str = "Copyright (C) 2016 Spike";

    /// A header with the given Copyright lines (`ours` among them where the test puts it) and
    /// one `Ported from` line per citation.
    fn lineage(copyrights: &[&str], cites: &[&str]) -> String {
        let mut text = "// SPDX-License-Identifier: GPL-3.0-or-later\n".to_owned();
        for line in copyrights {
            text.push_str(&format!("// {line}\n"));
        }
        text.push_str("//\n");
        for cite in cites {
            text.push_str(&format!(
                "// Ported from {cite}: Quake/sv_phys.c, SV_Physics\n"
            ));
        }
        text
    }

    const OURS_LINE: &str = "Copyright (C) 2026 QuakeC Studio contributors";

    // ADR-0018: a port from the QuakeSpasm lineage keeps its upstream file's lines, including
    // ones its row does not list, above ours
    #[test]
    fn h4_passes_a_lineage_port_with_its_file_s_lines() {
        let text = lineage(&[QS_ID, FITZ, DUSKE, QS_DEVS, OURS_LINE], &[QS]);
        assert!(rules(&text).is_empty(), "{text}");
        let spaced = lineage(&["Copyright (C) 2016      Spike", OURS_LINE], &[IRONWAIL]);
        assert!(rules(&spaced).is_empty(), "{spaced}");
        let pr_ext = lineage(&[ID_COPYRIGHT, SPIKE, OURS_LINE], &[VKQUAKE]);
        assert!(rules(&pr_ext).is_empty(), "{pr_ext}");
    }

    // ADR-0018: two lineage upstreams may share lines; id's line and a lineage row's lines
    // come in citation order
    #[test]
    fn h4_orders_lineage_upstreams_by_citation() {
        let shared = lineage(&[QS_ID, FITZ, QS_DEVS, SPIKE, OURS_LINE], &[QS, IRONWAIL]);
        assert!(rules(&shared).is_empty(), "{shared}");
        let id_cite = format!("id-Software/Quake@{COMMIT}");
        let id_first = lineage(&[ID_COPYRIGHT, QS_ID, QS_DEVS, OURS_LINE], &[&id_cite, QS]);
        assert!(rules(&id_first).is_empty(), "{id_first}");
        let reversed = lineage(&[QS_ID, QS_DEVS, ID_COPYRIGHT, OURS_LINE], &[&id_cite, QS]);
        assert_eq!(rules(&reversed), ["H4"]);
        let fte_cite = format!("fte-team/fteqw@{FTE_COMMIT}");
        let fte = FTE_LINE.trim_start_matches("// ").trim_end();
        let three = [id_cite.as_str(), &fte_cite, QS];
        let in_order = lineage(&[ID_COPYRIGHT, fte, QS_ID, OURS_LINE], &three);
        assert!(rules(&in_order).is_empty(), "{in_order}");
        let swapped = lineage(&[ID_COPYRIGHT, QS_ID, fte, OURS_LINE], &three);
        assert_eq!(rules(&swapped), ["H4"]);
    }

    // ADR-0018: none of the row's lines, an unlisted line first, or the row's lines below ours
    // all fail H4
    #[test]
    fn h4_fails_a_lineage_port_without_its_lines_in_place() {
        let missing = check_header(
            &header(&lineage(&[DUSKE, OURS_LINE], &[QS])),
            &id_upstreams(),
        );
        assert_eq!(missing.len(), 1, "{missing:?}");
        assert!(
            missing[0].starts_with("H4: cites sezero/quakespasm"),
            "{missing:?}"
        );
        assert!(missing[0].contains("(one of `"), "{missing:?}");
        assert_eq!(rules(&lineage(&[DUSKE, QS_ID, OURS_LINE], &[QS])), ["H4"]);
        assert_eq!(rules(&lineage(&[OURS_LINE, QS_ID], &[QS])), ["H4"]);
        assert_eq!(
            rules(&lineage(
                &[
                    QS_ID,
                    OURS_LINE,
                    FTE_LINE.trim_start_matches("// ").trim_end()
                ],
                &[QS, &format!("fte-team/fteqw@{FTE_COMMIT}")]
            )),
            ["H4"]
        );
    }

    #[test]
    fn listed_names_one_line_plainly_and_several_as_a_choice() {
        let one = Upstream {
            repo: "a/b".to_owned(),
            copyrights: vec!["Copyright x".to_owned()],
        };
        assert_eq!(listed(&one), "`Copyright x`");
        let two = Upstream {
            repo: "a/b".to_owned(),
            copyrights: vec!["Copyright x".to_owned(), "Copyright y".to_owned()],
        };
        assert_eq!(listed(&two), "(one of `Copyright x`, `Copyright y`)");
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
