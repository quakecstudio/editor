// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::path::{Path, PathBuf};

use qc_project::FsLoader;
use qs_commands::{Registry, Session};
use serde_json::json;

use super::{MatchCase, Query, REPLACE_IN_FILE, REPLACE_RANGE, Syntax, WholeWord};
use crate::tools::PROJECT_SEARCH;
use crate::{Config, OPEN_PROJECT, Workbench, WorkbenchError, register};

/// A project in a fresh directory named `name`, with `files` beside its `progs.src`.
fn project(name: &str, files: &[(&str, &str)]) -> (PathBuf, Session<Workbench<FsLoader>>) {
    let dir = std::env::temp_dir().join(format!("qs-wb-search-{name}-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut src = String::from("progs.dat\n");
    for (file, text) in files {
        std::fs::write(dir.join(file), text).unwrap();
        src.push_str(file);
        src.push('\n');
    }
    std::fs::write(dir.join("progs.src"), src).unwrap();
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut session = Session::new(Workbench::new(FsLoader, Config::default()), registry);
    session
        .execute(OPEN_PROJECT, json!({ "path": dir }))
        .unwrap();
    (dir, session)
}

fn query(text: &str, case: MatchCase, word: WholeWord, syntax: Syntax) -> Query {
    Query::new(text, case, word, syntax).unwrap()
}

// No spec statement: P4-33, UI-17: a search ignores ASCII case unless asked, finds a whole word
// only between word boundaries, and reads a regular expression; its matches are the file's
// byte ranges
#[test]
fn case_whole_words_and_regular_expressions() {
    let text = b"self.health = Self.healthy;";
    let plain = query(
        "health",
        MatchCase::Ignore,
        WholeWord::Anywhere,
        Syntax::Literal,
    );
    assert_eq!(plain.matches(text), [(5, 11), (19, 25)]);
    let word = query(
        "health",
        MatchCase::Ignore,
        WholeWord::Word,
        Syntax::Literal,
    );
    assert_eq!(word.matches(text), [(5, 11)]);
    let cased = query(
        "Self",
        MatchCase::Match,
        WholeWord::Anywhere,
        Syntax::Literal,
    );
    assert_eq!(cased.matches(text), [(14, 18)]);
    let any = query(
        "self",
        MatchCase::Ignore,
        WholeWord::Anywhere,
        Syntax::Literal,
    );
    assert_eq!(any.matches(text).len(), 2);
    let regex = query(
        r"heal\w+",
        MatchCase::Match,
        WholeWord::Anywhere,
        Syntax::Regex,
    );
    assert_eq!(regex.matches(text), [(5, 11), (19, 26)]);
    // A literal search reads the characters of a regular expression as themselves.
    let dots = query(
        "a.b",
        MatchCase::Match,
        WholeWord::Anywhere,
        Syntax::Literal,
    );
    assert_eq!(dots.matches(b"axb a.b"), [(4, 7)]);
}

// No spec statement: P4-33: a pattern that is not a regular expression is refused, and a
// pattern that matches nothing but the empty string finds nothing
#[test]
fn bad_and_empty_patterns() {
    let bad = Query::new("(", MatchCase::Match, WholeWord::Anywhere, Syntax::Regex);
    assert!(
        matches!(bad, Err(WorkbenchError::Pattern { .. })),
        "{bad:?}"
    );
    let empty = query("x*", MatchCase::Match, WholeWord::Anywhere, Syntax::Regex);
    assert!(empty.matches(b"abc").is_empty());
    let nothing = query("", MatchCase::Match, WholeWord::Anywhere, Syntax::Literal);
    assert!(nothing.matches(b"abc").is_empty());
    assert_eq!(nothing.replace(b"abc", "z"), (b"abc".to_vec(), 0));
}

// No spec statement: P4-33: bytes that are not UTF-8 are kept and searched around, and the
// offsets of a match after one are the file's own
#[test]
fn bytes_that_are_not_utf8_keep_their_offsets() {
    let text = b"caf\xe9 self \xff self";
    let self_ = query(
        "self",
        MatchCase::Match,
        WholeWord::Anywhere,
        Syntax::Literal,
    );
    assert_eq!(self_.matches(text), [(5, 9), (12, 16)]);
    let (replaced, count) = self_.replace(text, "other");
    assert_eq!(count, 2);
    assert_eq!(replaced, b"caf\xe9 other \xff other");
}

// No spec statement: P4-33: a regular expression's replacement expands its groups, a literal
// one is put as written, `$` and all
#[test]
fn replacements_expand_groups_only_for_regular_expressions() {
    let regex = query(
        r"(\w+)\.health",
        MatchCase::Match,
        WholeWord::Anywhere,
        Syntax::Regex,
    );
    assert_eq!(
        regex.replace(b"self.health; other.health", "$1.hp"),
        (b"self.hp; other.hp".to_vec(), 2)
    );
    let literal = query(
        "health",
        MatchCase::Match,
        WholeWord::Anywhere,
        Syntax::Literal,
    );
    assert_eq!(literal.replace(b"health", "$1"), (b"$1".to_vec(), 1));
}

// No spec statement: P4-33: project.search answers each match with its line and byte range,
// takes the whole-word and regex options, and refuses a bad pattern
#[test]
fn project_search_answers_byte_ranges() {
    let (dir, mut session) = project(
        "ranges",
        &[
            ("a.qc", "float healthy;\nfloat health;\r\n"),
            ("b.qc", "// health\n"),
        ],
    );
    let found = session
        .execute(
            PROJECT_SEARCH,
            json!({ "text": "health", "whole_word": true }),
        )
        .unwrap();
    let matches = found["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 2, "{found}");
    assert_eq!(
        Path::new(matches[0]["path"].as_str().unwrap()),
        dir.join("a.qc")
    );
    assert_eq!(matches[0]["line"], 2);
    assert_eq!(matches[0]["start"], 21);
    assert_eq!(matches[0]["end"], 27);
    assert_eq!(matches[0]["text"], "float health;");
    assert_eq!(matches[1]["line"], 1);
    let regex = session
        .execute(
            PROJECT_SEARCH,
            json!({ "text": "health(y)?;", "regex": true }),
        )
        .unwrap();
    assert_eq!(regex["matches"].as_array().unwrap().len(), 2);
    assert!(
        session
            .execute(PROJECT_SEARCH, json!({ "text": "(", "regex": true }))
            .is_err()
    );
}

// No spec statement: P4-33, the acceptance criterion: replacing across ten files saves each
// file and is one undoable change per file; undoing one puts back that file alone, saved
#[test]
fn replacing_across_ten_files_is_one_undoable_change_per_file() {
    let names: Vec<String> = (0..10).map(|n| format!("f{n}.qc")).collect();
    let files: Vec<(&str, &str)> = names
        .iter()
        .map(|name| {
            (
                name.as_str(),
                "float health;\nvoid() f = { health = 1; };\n",
            )
        })
        .collect();
    let (dir, mut session) = project("ten", &files);
    let found = session
        .execute(
            PROJECT_SEARCH,
            json!({ "text": "health", "whole_word": true }),
        )
        .unwrap();
    let mut paths: Vec<String> = found["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["path"].as_str().unwrap().to_owned())
        .collect();
    paths.dedup();
    assert_eq!(paths.len(), 10);
    for path in &paths {
        let done = session
            .execute(
                REPLACE_IN_FILE,
                json!({ "path": path, "text": "health", "replacement": "hp", "whole_word": true }),
            )
            .unwrap();
        assert_eq!(done["replaced"], 2);
    }
    let replaced = "float hp;\nvoid() f = { hp = 1; };\n";
    for name in &names {
        assert_eq!(std::fs::read_to_string(dir.join(name)).unwrap(), replaced);
        assert!(
            !session.context().is_unsaved(&dir.join(name)),
            "{name} saved"
        );
    }
    // Ten steps, the last file's first.
    session.undo().unwrap();
    let last = dir.join("f9.qc");
    assert_eq!(
        std::fs::read_to_string(&last).unwrap(),
        "float health;\nvoid() f = { health = 1; };\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("f8.qc")).unwrap(),
        replaced
    );
    for _ in 1..10 {
        session.undo().unwrap();
    }
    assert!(session.undo().is_err(), "ten steps, no more");
    session.redo().unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("f0.qc")).unwrap(),
        replaced
    );
    // A file with no match is left as it is, and nothing is recorded.
    let none = session
        .execute(
            REPLACE_IN_FILE,
            json!({ "path": dir.join("f0.qc"), "text": "missing", "replacement": "x" }),
        )
        .unwrap();
    assert_eq!(none["replaced"], 0);
}

// No spec statement: P4-33: file.replace_range changes one range of the editor's text, unsaved
// and undoable, and refuses a range outside the file
#[test]
fn replace_range_edits_an_excerpt_unsaved() {
    let (dir, mut session) = project("range", &[("a.qc", "float health;\n")]);
    let path = dir.join("a.qc");
    session
        .execute(
            REPLACE_RANGE,
            json!({ "path": path, "start": 6, "end": 12, "text": "hp" }),
        )
        .unwrap();
    assert!(session.context().is_unsaved(&path));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "float health;\n",
        "nothing saved"
    );
    let read = session
        .execute(crate::READ_FILE, json!({ "path": path }))
        .unwrap();
    assert!(read.to_string().contains("float hp;"), "{read}");
    session.undo().unwrap();
    let read = session
        .execute(crate::READ_FILE, json!({ "path": path }))
        .unwrap();
    assert!(read.to_string().contains("float health;"), "{read}");
    for (start, end) in [(6, 99), (8, 6)] {
        assert!(
            session
                .execute(
                    REPLACE_RANGE,
                    json!({ "path": path, "start": start, "end": end, "text": "x" }),
                )
                .is_err(),
            "{start}..{end}"
        );
    }
}
