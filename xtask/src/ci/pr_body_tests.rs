// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests of `.claude/skills/work-issue/scripts/check-pr-body.sh`, which `land.sh` runs on a
//! pull request's body before queueing it: GitHub closes an issue named after a closing keyword
//! whatever the sentence around it says, so "does not close #84" closed #84 (PRs #477, #514).
//! Process tooling with no engine spec, so no statement is cited.

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// Runs the check on `body`; returns whether it accepted the body, and its standard output.
fn check(body: &str) -> (bool, String) {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(".claude/skills/work-issue/scripts/check-pr-body.sh");
    let mut child = Command::new("bash")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(body.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let code = output.status.code().unwrap();
    assert!(code == 0 || code == 1, "exit {code}");
    (code == 0, String::from_utf8(output.stdout).unwrap())
}

fn accepted(body: &str) -> bool {
    check(body).0
}

#[test]
fn the_bodies_that_closed_issues_by_mistake_are_refused() {
    for body in [
        "Lands the rest.\n\nThis PR does not close #84.\n\nCloses #470",
        "Makes them upstreams. This PR does not close #444, since the work is still to do.",
    ] {
        assert!(!accepted(body), "{body}");
    }
}

const KEYWORDS: [&str; 9] = [
    "close", "closes", "closed", "fix", "fixes", "fixed", "resolve", "resolves", "resolved",
];

const NEGATIONS: [&str; 30] = [
    "not",
    "no",
    "never",
    "nor",
    "without",
    "cannot",
    "doesn't",
    "doesn\u{2019}t",
    "won't",
    "can't",
    "isn't",
    "doesnt",
    "dont",
    "didnt",
    "wont",
    "isnt",
    "arent",
    "wasnt",
    "cant",
    "shouldnt",
    "partly",
    "partially",
    "in part",
    "later",
    "follow-up",
    "future",
    "will",
    "once",
    "until",
    "eventually",
];

#[test]
fn every_keyword_is_refused_after_every_negation_and_hedge_word() {
    let mut sentences = Vec::new();
    for keyword in KEYWORDS {
        for negation in NEGATIONS {
            sentences.push(format!("Here {negation} we {keyword} #84."));
            sentences.push(format!("{} #84 {negation}.", keyword.to_uppercase()));
        }
    }
    // One sentence per paragraph, so one run checks each on its own.
    let (ok, out) = check(&sentences.join("\n\n"));
    assert!(!ok);
    for sentence in &sentences {
        assert!(out.contains(&format!("  {sentence}\n")), "{sentence}");
    }
}

#[test]
fn colons_and_emphasis_between_keyword_and_reference_are_read() {
    for body in [
        "Not yet: CLOSES: #84",
        "**Does not close** #84",
        "It does not `close` #84",
        "It does not close: *#84*",
    ] {
        assert!(!accepted(body), "{body}");
    }
}

#[test]
fn other_repository_and_url_references_are_refused() {
    let (ok, out) = check("This does not fix owner/repo#5.");
    assert!(!ok);
    assert!(out.contains("Refs owner/repo#5"), "{out}");
    assert!(!accepted(
        "This won't resolve https://github.com/a/b/issues/7 yet."
    ));
}

#[test]
fn a_sentence_wrapped_over_lines_is_one_sentence() {
    assert!(!accepted("This PR does not\nclose #3, as it stays open."));
    assert!(!accepted("- This item does not\n  close #3"));
}

#[test]
fn a_line_starting_with_the_reference_continues_the_sentence() {
    for body in [
        "This PR does not close\n#84.",
        "This PR does not close\n- #84",
        "This PR does not close\n> #84",
        "This PR does not close\n| #84",
        "This PR does not close\n#84 or fix\n* owner/repo#5",
    ] {
        assert!(!accepted(body), "{body}");
    }
    // A heading needs its space; a list, quote or table line that starts with text splits.
    assert!(accepted("Not this.\n# 84 is a heading\nCloses #12"));
    assert!(accepted("Not this\n- Closes #12"));
    assert!(accepted("Not this\n> Closes #12"));
}
#[test]
fn the_refusal_prints_the_sentence_and_suggests_refs() {
    let (ok, out) = check("Intro.\n\nThis PR does not close #84.\n\nCloses #12");
    assert!(!ok);
    assert!(out.contains("  This PR does not close #84.\n"), "{out}");
    assert!(out.contains("Write \"Refs #84\""), "{out}");
    assert!(!out.contains("#12"), "{out}");

    let (_, indented) = check("   This PR does not\n   close #84   \n");
    assert!(
        indented.contains("\n  This PR does not close #84\n"),
        "{indented}"
    );
}

#[test]
fn each_reference_in_a_refused_sentence_gets_a_suggestion() {
    let (ok, out) = check("This does not close #1 or fix #2.");
    assert!(!ok);
    assert!(out.contains("Refs #1") && out.contains("Refs #2"), "{out}");
}

#[test]
fn ordinary_bodies_are_accepted() {
    for body in [
        "",
        "Closes #12",
        "Closes: #12\nFixes #13",
        "Refs #84",
        "This PR does not finish #84. Refs #84\n\nCloses #12",
        "Refs #84, which stays open.\n\nCloses #12",
        "Closes #12. Nothing else here is not fine.",
        "Closes #12; the docs are not touched.",
        "- Closes #12\n- not related",
        "## Not done\nCloses #12",
        "| Criterion | Not met |\nCloses #12",
        "Not this.\n> quoted\n```\nfixes #3 ok\n```",
        "This does not close the window.",
        "It does not fix anything; suffixes #4 and prefixes #5 are words.",
        "Fixes #12 (see notes)\n\nNo change to #13.",
    ] {
        assert!(accepted(body), "{body}");
    }
}

#[test]
fn crlf_bodies_split_as_lf_bodies_do() {
    assert!(accepted("Closes #12\r\n\r\nNot related."));
    assert!(!accepted("This PR does not\r\nclose #3."));
}
