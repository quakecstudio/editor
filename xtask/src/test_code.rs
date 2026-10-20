// SPDX-License-Identifier: GPL-3.0-or-later

//! Which lines of a Rust tree are test code, so that only a `// spec` line in a test counts as a
//! citation (TEST-3: every statement "has a test citing it"; the rule is in
//! `docs/spec/README.md`, "Citing statements").
//!
//! Test code is:
//!
//! - a file under a `tests` directory, or named `tests.rs`;
//! - an item marked `#[test]`, or `#[cfg(...)]` naming `test` without `not` (`cfg(test)`,
//!   `cfg(all(unix, test))`), such as an inline `mod tests { ... }`, and a file whose inner
//!   `#![cfg(test)]` marks all of it;
//! - the file of a module declared `#[cfg(test)] mod name;`, and every file under its
//!   directory, such as a crate's `fixtures.rs` or `vectors.rs`;
//! - xtask's oracle scenarios ([`DIFFERENTIAL_TESTS`]), which are differential tests run
//!   against id's code through the built oracle.
//!
//! The spec-citation ratchet in `qs-tests` includes this file with `#[path]` so that it and
//! `cargo xtask readiness` read citations the same way: nothing may depend on xtask
//! (`docs/layers.md`), and two copies would drift.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Directories, relative to the repository root, whose every file is test code although no
/// `#[cfg(test)]` marks it.
pub(crate) const DIFFERENTIAL_TESTS: [&str; 1] = ["xtask/src/oracles/scenarios"];

/// xtask's fixture repositories, relative to the repository root. Their files cite the
/// statements of their own small `docs/spec/`, not the repository's, so neither the
/// spec-citation ratchet nor `cargo xtask readiness` reads them as the repository's citations.
pub(crate) const FIXTURES: &str = "xtask/fixtures";

/// The error for a `// spec` line outside test code: line `line` of `file`, whose text after
/// `// spec ` is `rest`. Such a line cites nothing (TEST-3 asks for a test), so the
/// spec-citation ratchet and `cargo xtask readiness` both report it rather than drop it, and
/// it cannot pass for coverage.
pub(crate) fn shipped_citation(file: &str, line: u32, rest: &str) -> String {
    format!(
        "{file}:{line}: a `// spec` line outside test code cites nothing; name the statement in \
         a plain comment (`// {rest}`) or cite it from a test"
    )
}

/// The test code of one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TestCode {
    /// The whole file is test code.
    File,
    /// These lines (1-based) are test code.
    Lines(BTreeSet<u32>),
}

impl TestCode {
    /// Whether `line` (1-based) is test code.
    pub(crate) fn contains(&self, line: u32) -> bool {
        match self {
            Self::File => true,
            Self::Lines(lines) => lines.contains(&line),
        }
    }
}

/// The test code of each of `files`, in order. Each is a path relative to the repository root
/// and the file's text; the modules a file declares under `#[cfg(test)]` are looked up among
/// the others.
pub(crate) fn test_code(files: &[(PathBuf, String)]) -> Vec<TestCode> {
    let scans: Vec<Scan> = files.iter().map(|(_, text)| scan(text)).collect();
    let mut test_dirs: Vec<PathBuf> = DIFFERENTIAL_TESTS.iter().map(PathBuf::from).collect();
    let mut test_files: BTreeSet<PathBuf> = BTreeSet::new();
    for ((path, _), scan) in files.iter().zip(&scans) {
        let dir = module_dir(path);
        for name in &scan.test_modules {
            test_files.insert(dir.join(format!("{name}.rs")));
            test_dirs.push(dir.join(name));
        }
    }
    files
        .iter()
        .zip(scans)
        .map(|((path, _), scan)| {
            let whole = path
                .components()
                .rev()
                .skip(1)
                .any(|c| c.as_os_str() == "tests")
                || path.file_name().is_some_and(|name| name == "tests.rs")
                || test_files.contains(path)
                || test_dirs.iter().any(|dir| path.starts_with(dir))
                || scan.whole_file;
            if whole {
                TestCode::File
            } else {
                TestCode::Lines(scan.test_lines)
            }
        })
        .collect()
}

/// The directory a file's `mod name;` declarations live in: its own for `lib.rs`, `main.rs`
/// and `mod.rs`, otherwise the directory named after it.
fn module_dir(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    match path.file_stem().and_then(|stem| stem.to_str()) {
        Some("lib" | "main" | "mod") | None => parent.to_path_buf(),
        Some(stem) => parent.join(stem),
    }
}

/// What one file's text says about its test code.
#[derive(Debug, Default)]
struct Scan {
    /// Lines inside an item marked as test code.
    test_lines: BTreeSet<u32>,
    /// The names of modules declared `#[cfg(test)] mod name;`.
    test_modules: Vec<String>,
    /// Whether an inner `#![cfg(test)]` makes the whole file test code.
    whole_file: bool,
}

/// Whether an attribute, given as its words, makes what it marks test code: `#[test]`, or a
/// `cfg` that names `test` with no `not`, such as `cfg(test)` or `cfg(all(unix, test))`.
fn is_test_attribute(words: &[String]) -> bool {
    match words {
        [only] => only == "test",
        [cfg, rest @ ..] if cfg == "cfg" => {
            rest.iter().any(|word| word == "test") && !rest.iter().any(|word| word == "not")
        }
        _ => false,
    }
}

/// The item a test attribute marks, while its header is read.
#[derive(Debug, Default)]
struct Pending {
    /// The words of the item's header so far.
    words: Vec<String>,
    /// Open `(` and `[` in the header, so that a `;` inside a type does not end it.
    nesting: usize,
}

impl Pending {
    /// Whether a `,` at the header's own level ends it: it does for a field, a variant or an
    /// argument the attribute marks, but not in the generics or `where` clause of a `fn` or
    /// `impl` header.
    fn ends_at_comma(&self) -> bool {
        self.nesting == 0 && !self.words.iter().any(|word| word == "fn" || word == "impl")
    }
}

/// Reads `text` as Rust tokens, enough to skip comments, strings and character literals and to
/// match braces, and returns its test items and test module declarations.
fn scan(text: &str) -> Scan {
    let chars: Vec<char> = text.chars().collect();
    let mut cursor = Cursor {
        chars: &chars,
        at: 0,
        line: 1,
        depth: 0,
        test_depth: None,
        scan: Scan::default(),
    };
    let mut pending: Option<Pending> = None;
    while let Some(c) = cursor.peek(0) {
        if cursor.starts_with("//") {
            while cursor.peek(0).is_some_and(|c| c != '\n') {
                cursor.bump();
            }
        } else if cursor.starts_with("/*") {
            cursor.block_comment();
        } else if c == '"' {
            cursor.bump();
            cursor.string();
        } else if c == '\'' {
            cursor.char_or_lifetime();
        } else if c.is_alphabetic() || c == '_' {
            let word = cursor.word();
            if matches!(word.as_str(), "r" | "br") && matches!(cursor.peek(0), Some('#' | '"')) {
                cursor.raw_string();
            } else if let Some(pending) = pending.as_mut() {
                pending.words.push(word);
            }
        } else if cursor.starts_with("#[") || cursor.starts_with("#![") {
            let inner = cursor.starts_with("#!");
            let words = cursor.attribute();
            if is_test_attribute(&words) {
                if inner && cursor.depth == 0 {
                    cursor.scan.whole_file = true;
                } else if !inner {
                    pending = Some(Pending::default());
                }
            }
        } else {
            cursor.bump();
            match c {
                '{' => {
                    cursor.depth = cursor.depth.saturating_add(1);
                    if pending.take().is_some() && cursor.test_depth.is_none() {
                        cursor.test_depth = Some(cursor.depth);
                        cursor.scan.test_lines.insert(cursor.line);
                    }
                }
                '}' => {
                    // The attribute marked the last field or variant of this block.
                    pending = None;
                    if cursor.test_depth == Some(cursor.depth) {
                        cursor.test_depth = None;
                    }
                    cursor.depth = cursor.depth.saturating_sub(1);
                }
                ',' if pending.as_ref().is_some_and(Pending::ends_at_comma) => pending = None,
                '(' | '[' => {
                    if let Some(pending) = pending.as_mut() {
                        pending.nesting = pending.nesting.saturating_add(1);
                    }
                }
                ')' | ']' => {
                    if let Some(pending) = pending.as_mut() {
                        pending.nesting = pending.nesting.saturating_sub(1);
                    }
                }
                ';' if pending.as_ref().is_some_and(|p| p.nesting == 0) => {
                    let words = pending.take().map(|p| p.words).unwrap_or_default();
                    if let Some(at) = words.iter().position(|word| word == "mod")
                        && let Some(name) = words.get(at.saturating_add(1))
                        && cursor.test_depth.is_none()
                    {
                        cursor.scan.test_modules.push(name.clone());
                    }
                }
                _ => {}
            }
        }
    }
    cursor.scan
}

/// A position in a file's characters, with the line and brace depth it is at.
struct Cursor<'a> {
    chars: &'a [char],
    at: usize,
    line: u32,
    depth: usize,
    /// The brace depth of the test item the cursor is in, if any.
    test_depth: Option<usize>,
    scan: Scan,
}

impl Cursor<'_> {
    fn peek(&self, ahead: usize) -> Option<char> {
        self.chars.get(self.at.saturating_add(ahead)).copied()
    }

    fn starts_with(&self, prefix: &str) -> bool {
        prefix
            .chars()
            .enumerate()
            .all(|(ahead, c)| self.peek(ahead) == Some(c))
    }

    /// Moves past one character, counting lines and marking those in a test item.
    fn bump(&mut self) {
        if self.peek(0) == Some('\n') {
            self.line = self.line.saturating_add(1);
            if self.test_depth.is_some() {
                self.scan.test_lines.insert(self.line);
            }
        }
        self.at = self.at.saturating_add(1);
    }

    fn word(&mut self) -> String {
        let mut word = String::new();
        while let Some(c) = self.peek(0).filter(|c| c.is_alphanumeric() || *c == '_') {
            word.push(c);
            self.bump();
        }
        word
    }

    /// A block comment, which nests.
    fn block_comment(&mut self) {
        let mut open: usize = 0;
        while self.peek(0).is_some() {
            if self.starts_with("/*") {
                open = open.saturating_add(1);
                self.bump();
            } else if self.starts_with("*/") {
                open = open.saturating_sub(1);
                self.bump();
            }
            self.bump();
            if open == 0 {
                return;
            }
        }
    }

    /// The rest of a string after its opening quote.
    fn string(&mut self) {
        while let Some(c) = self.peek(0) {
            self.bump();
            match c {
                '\\' => self.bump(),
                '"' => return,
                _ => {}
            }
        }
    }

    /// A raw string after its `r` or `br`: hashes, a quote, then up to a quote and as many
    /// hashes. A raw identifier such as `r#type` is left to be read as a word.
    fn raw_string(&mut self) {
        let mut hashes: usize = 0;
        while self.peek(hashes) == Some('#') {
            hashes = hashes.saturating_add(1);
        }
        if self.peek(hashes) != Some('"') {
            return;
        }
        for _ in 0..=hashes {
            self.bump();
        }
        let close: String = std::iter::once('"')
            .chain(std::iter::repeat_n('#', hashes))
            .collect();
        while self.peek(0).is_some() && !self.starts_with(&close) {
            self.bump();
        }
        for _ in 0..close.len() {
            self.bump();
        }
    }

    /// A character literal such as `'{'` or `'\''`, or the quote of a lifetime such as `'a`.
    fn char_or_lifetime(&mut self) {
        self.bump();
        if self.peek(0) == Some('\\') {
            while let Some(c) = self.peek(0) {
                self.bump();
                if c == '\\' {
                    self.bump();
                } else if c == '\'' {
                    return;
                }
            }
        } else if self.peek(1) == Some('\'') {
            self.bump();
            self.bump();
        }
    }

    /// An attribute, up to its matching `]`, and the words in it outside its strings.
    fn attribute(&mut self) -> Vec<String> {
        let mut words = Vec::new();
        let mut open: usize = 0;
        while let Some(c) = self.peek(0) {
            if c == '"' {
                self.bump();
                self.string();
                continue;
            }
            if c.is_alphabetic() || c == '_' {
                words.push(self.word());
                continue;
            }
            self.bump();
            match c {
                '[' => open = open.saturating_add(1),
                ']' => {
                    open = open.saturating_sub(1);
                    if open == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
        words
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::expect_used,
        reason = "test code: a failed expect is a failed test"
    )]

    use super::*;

    fn lines(text: &str) -> Vec<u32> {
        scan(text).test_lines.into_iter().collect()
    }

    fn file(path: &str, text: &str) -> (PathBuf, String) {
        (PathBuf::from(path), text.to_owned())
    }

    #[test]
    fn an_inline_test_module_is_test_code_and_the_code_around_it_is_not() {
        let text = "// spec A-1.1: not a test\nfn f() {}\n#[cfg(test)]\nmod tests {\n    // spec A-1.1: a test\n    fn g() { if x { } }\n}\n// spec A-1.1: after\n";
        assert_eq!(lines(text), [4, 5, 6, 7]);
    }

    #[test]
    fn a_test_function_outside_a_test_module_is_test_code() {
        let text = "fn f() {}\n#[test]\nfn t() {\n    // spec A-1.1\n}\nfn h() {}\n";
        assert_eq!(lines(text), [3, 4, 5]);
    }

    #[test]
    fn braces_in_strings_chars_and_comments_do_not_end_a_test_module() {
        let text = "#[cfg(all(test, unix))]\nmod tests {\n    const A: &str = \"}\\\"}\";\n    const B: &str = r#\"}\n\"}\"#;\n    const C: char = '}';\n    const D: char = '\\'';\n    fn e<'a>(x: &'a str) {}\n    /* } /* } */ } */\n    // }\n    // spec A-1.1\n}\nfn after() {}\n";
        assert_eq!(lines(text), (2..=12).collect::<Vec<u32>>());
    }

    #[test]
    fn a_test_field_or_variant_does_not_mark_the_next_block() {
        let field = "struct S {\n    #[cfg(test)]\n    hook: u8,\n    x: u8,\n}\nimpl S {\n    // spec A-1.1\n}\n";
        assert_eq!(lines(field), Vec::<u32>::new());
        let last =
            "struct S {\n    #[cfg(test)]\n    hook: u8\n}\nimpl S {\n    // spec A-1.1\n}\n";
        assert_eq!(lines(last), Vec::<u32>::new());
        let variant =
            "enum E {\n    #[cfg(test)]\n    A,\n    B,\n}\nfn f() {\n    // spec A-1.1\n}\n";
        assert_eq!(lines(variant), Vec::<u32>::new());
    }

    #[test]
    fn a_generic_test_function_keeps_its_header_past_commas() {
        let text = "#[test]\nfn t<A, B>() where A: Copy, B: Copy {\n    // spec A-1.1\n}\n";
        assert_eq!(lines(text), [2, 3, 4]);
    }

    #[test]
    fn any_cfg_naming_test_without_not_is_test_code() {
        let text = "#[cfg(all(unix, test))]\nmod a {\n    // spec A-1.1\n}\n#[cfg( test )]\nmod b {\n}\n#[cfg(not(test))]\nmod c {\n}\n#[cfg(feature = \"test\")]\nmod d {\n}\n#[cfg_attr(test, derive(Debug))]\nstruct E {\n}\n";
        assert_eq!(lines(text), [2, 3, 4, 6, 7]);
    }

    #[test]
    fn a_bracket_in_an_attribute_string_does_not_end_the_attribute() {
        let text =
            "#[expect(clippy::x, reason = \"a ] b [\")]\n#[test]\nfn t() {\n    // spec A-1.1\n}\n";
        assert_eq!(lines(text), [3, 4, 5]);
    }

    #[test]
    fn an_inner_cfg_test_marks_the_whole_file() {
        let files = [
            file("crates/a/src/helpers.rs", "#![cfg(test)]\n// spec A-1.1\n"),
            file(
                "crates/a/src/other.rs",
                "#![expect(dead_code, reason = \"x\")]\n// spec A-1.1\n",
            ),
        ];
        let code = test_code(&files);
        assert_eq!(code.first(), Some(&TestCode::File));
        assert_eq!(code.get(1), Some(&TestCode::Lines(BTreeSet::new())));
    }

    #[test]
    fn a_raw_identifier_is_not_a_raw_string() {
        let text =
            "#[cfg(test)]\nmod tests {\n    fn r#type() {}\n    // spec A-1.1\n}\nfn f() {}\n";
        assert_eq!(lines(text), [2, 3, 4, 5]);
    }

    #[test]
    fn a_semicolon_in_a_test_function_signature_does_not_end_its_header() {
        let text = "#[test]\nfn t() -> [u8; 4] {\n    // spec A-1.1\n    [0; 4]\n}\n";
        assert_eq!(lines(text), [2, 3, 4, 5]);
    }

    #[test]
    fn a_test_attribute_on_a_use_marks_nothing() {
        let text = "#[cfg(test)]\nuse std::fmt;\nfn f() {\n    // spec A-1.1\n}\n";
        assert_eq!(lines(text), Vec::<u32>::new());
        assert!(scan(text).test_modules.is_empty());
    }

    #[test]
    fn test_module_files_and_their_directories_are_test_code() {
        let files = [
            file(
                "crates/a/src/lib.rs",
                "mod read;\n#[cfg(test)]\nmod vectors;\n#[cfg(test)]\npub(crate) mod fixtures;\n",
            ),
            file("crates/a/src/read.rs", "#[cfg(test)]\nmod read_tests;\n"),
            file("crates/a/src/vectors.rs", "// spec A-1.1\n"),
            file("crates/a/src/vectors/more.rs", "// spec A-1.1\n"),
            file("crates/a/src/fixtures.rs", "// spec A-1.1\n"),
            file("crates/a/src/read/read_tests.rs", "// spec A-1.1\n"),
            file("crates/a/src/read/tests/x.rs", "// spec A-1.1\n"),
            file("crates/a/src/write/tests.rs", "// spec A-1.1\n"),
            file("crates/a/tests/it.rs", "// spec A-1.1\n"),
            file("xtask/src/oracles/scenarios/cases.rs", "// spec A-1.1\n"),
        ];
        let code = test_code(&files);
        let mut iter = code.iter();
        assert_eq!(iter.next(), Some(&TestCode::Lines(BTreeSet::new())));
        assert_eq!(iter.next(), Some(&TestCode::Lines(BTreeSet::new())));
        for (code, (path, _)) in iter.zip(files.iter().skip(2)) {
            assert_eq!(code, &TestCode::File, "{}", path.display());
        }
    }

    #[test]
    fn a_shipped_module_and_a_file_beside_a_test_module_are_not_test_code() {
        let files = [
            file(
                "crates/a/src/lib.rs",
                "mod ops;\n#[cfg(test)]\nmod tests;\n",
            ),
            file("crates/a/src/ops.rs", "// spec A-1.1\n"),
            file("crates/a/src/testsuite.rs", "// spec A-1.1\n"),
            file("xtask/src/oracles/vectors.rs", "// spec A-1.1\n"),
        ];
        let code = test_code(&files);
        for (code, (path, _)) in code.iter().zip(&files) {
            assert!(!code.contains(1), "{}", path.display());
        }
        assert!(
            TestCode::File.contains(1),
            "a whole test file contains each line"
        );
        assert!(
            code.first().is_some_and(|code| !code.contains(3)),
            "a module declaration is not itself a test line"
        );
        assert_eq!(
            module_dir(Path::new("crates/a/src/x/mod.rs")),
            PathBuf::from("crates/a/src/x")
        );
        assert_eq!(
            module_dir(Path::new("crates/a/src/x.rs")),
            PathBuf::from("crates/a/src/x")
        );
        let first = code.first().expect("one result per file");
        assert_eq!(first, &TestCode::Lines(BTreeSet::new()));
    }
}
