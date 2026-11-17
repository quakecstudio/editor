// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask gmqcc-coverage <dir>...`: the coverage check for the GMQCC dialect spec (L-10,
//! GMQ-20). It scans every `.qc`, `.qh`, `.inc` and `.h` file under each GMQCC corpus directory
//! and lists each GMQCC construct the FTEQCC grammar lacks, grouped into the classes of
//! `docs/spec/dialect-gmqcc.md`. It fails if the source holds a class no section there names,
//! or if the spec names a class the corpus does not use.
//!
//! The GMQCC corpus is Xonotic (`xonotic` in `corpus/manifest.toml`), fetched with
//! `cargo xtask corpus fetch xonotic`. Xonotic writes several constructs through C
//! preprocessor macros (`#define ACCUMULATE [[accumulate]]`, `#define LABEL(id) :id`); the
//! lexer reads a directive's body as tokens, so a construct a macro holds is found where the
//! macro is defined. This is an inventory of which classes occur, by a signature of tokens for
//! each, not a parse; the GMQCC profile (L-02) is checked by `quakec-ide check` in CI.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use qc_lexer::{Dialect, Punct, Token, TokenKind, lex_with};

/// Which GMQCC construct class a token sequence belongs to. Each maps to one section of
/// `docs/spec/dialect-gmqcc.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Translatable,
    ReturnAssign,
    Nil,
    Variadic,
    Attribute,
    Noref,
    Power,
    Goto,
}

impl Kind {
    /// Every class, for the "every class is exercised" check.
    const ALL: [Self; 8] = [
        Self::Translatable,
        Self::ReturnAssign,
        Self::Nil,
        Self::Variadic,
        Self::Attribute,
        Self::Noref,
        Self::Power,
        Self::Goto,
    ];

    /// The spec section that documents this class.
    const fn section(self) -> &'static str {
        match self {
            Self::Translatable => "GMQ-6",
            Self::ReturnAssign => "GMQ-9",
            Self::Nil => "GMQ-12",
            Self::Variadic => "GMQ-13",
            Self::Attribute => "GMQ-16",
            Self::Noref => "GMQ-17",
            Self::Power => "GMQ-18",
            Self::Goto => "GMQ-19",
        }
    }

    /// A short name for the report.
    const fn name(self) -> &'static str {
        match self {
            Self::Translatable => "_(\"...\")",
            Self::ReturnAssign => "return = ...",
            Self::Nil => "nil",
            Self::Variadic => "...count, ...(i, T)",
            Self::Attribute => "[[attribute]]",
            Self::Noref => "noref",
            Self::Power => "**",
            Self::Goto => "goto, :label",
        }
    }
}

/// `cargo xtask gmqcc-coverage`.
#[derive(clap::Parser, Debug)]
pub(crate) struct Args {
    /// The GMQCC corpus directories.
    #[arg(required = true)]
    dirs: Vec<PathBuf>,
}

/// Runs the check on the GMQCC corpus directories in `args`.
pub(crate) fn main(_root: &Path, args: &Args) -> Result<bool, String> {
    let mut totals: BTreeMap<Kind, usize> = BTreeMap::new();
    for dir in &args.dirs {
        let files = sources(dir)?;
        if files.is_empty() {
            return Err(format!(
                "no .qc, .qh, .inc or .h files under {}",
                dir.display()
            ));
        }
        for path in &files {
            let text = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
            for kind in classify(&text) {
                let total = totals.entry(kind).or_default();
                *total = total.saturating_add(1);
            }
        }
        println!("gmqcc-coverage: {}: {} files", dir.display(), files.len());
    }
    let mut ok = true;
    println!("gmqcc-coverage: files with each class:");
    for (kind, count) in &totals {
        let present = in_spec(kind.section());
        println!(
            "  {:<22} {count:>5}  {} ({})",
            kind.name(),
            kind.section(),
            if present {
                "in spec"
            } else {
                "MISSING FROM SPEC"
            }
        );
        ok &= present;
    }
    for kind in Kind::ALL {
        if !totals.contains_key(&kind) {
            ok = false;
            println!(
                "FAIL  {} ({}) is in the spec but was not found in the corpus",
                kind.name(),
                kind.section()
            );
        }
    }
    println!(
        "gmqcc-coverage: {}",
        if ok {
            "every construct is in the spec"
        } else {
            "failures found"
        }
    );
    Ok(ok)
}

/// Whether the GMQCC dialect spec has a section headed `section`.
fn in_spec(section: &str) -> bool {
    let spec = Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/spec/dialect-gmqcc.md");
    let Ok(text) = fs::read_to_string(spec) else {
        return false;
    };
    text.lines()
        .any(|line| line.starts_with(&format!("## {section} ")))
}

/// The GMQCC construct classes present in `text`, each once.
fn classify(text: &[u8]) -> Vec<Kind> {
    // The GMQCC dialect's lexer keeps a lone `:`, which vanilla's drops.
    let lexed = lex_with(text, Dialect::Gmqcc);
    let tokens = &lexed.tokens;
    let mut kinds: Vec<Kind> = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        let next = index.checked_add(1).and_then(|n| tokens.get(n));
        let after = index.checked_add(2).and_then(|n| tokens.get(n));
        if let Some(kind) = signature(token, next, after)
            && !kinds.contains(&kind)
        {
            kinds.push(kind);
        }
    }
    kinds
}

/// The class whose signature starts at `token`, followed by `next` and `after`.
fn signature(token: &Token, next: Option<&Token>, after: Option<&Token>) -> Option<Kind> {
    let next_is = |kind: &TokenKind| next.is_some_and(|n| &n.kind == kind);
    let touching = next.is_some_and(|n| n.span.start == token.span.end);
    match &token.kind {
        // GMQ-6.1: the underscore intrinsic on a string constant.
        TokenKind::Name(word)
            if word == "_"
                && next_is(&TokenKind::Punct(Punct::LParen))
                && after.is_some_and(|a| matches!(a.kind, TokenKind::String(_))) =>
        {
            Some(Kind::Translatable)
        }
        // GMQ-9.1: `return` assigned as a local.
        TokenKind::Name(word) if word == "return" && next_is(&TokenKind::Punct(Punct::Eq)) => {
            Some(Kind::ReturnAssign)
        }
        TokenKind::Name(word) => match word.as_str() {
            // GMQ-12.1.
            "nil" => Some(Kind::Nil),
            // GMQ-17.1.
            "noref" => Some(Kind::Noref),
            // GMQ-19.1: a jump to a named label.
            "goto" => Some(Kind::Goto),
            _ => None,
        },
        // GMQ-13.1: `...` naming the count, or `...(` reading an argument.
        TokenKind::Punct(Punct::Ellipsis)
            if next.is_some_and(|n| {
                matches!(n.kind, TokenKind::Name(_) | TokenKind::Punct(Punct::LParen))
            }) =>
        {
            Some(Kind::Variadic)
        }
        // GMQ-16.1: `[[` then a name.
        TokenKind::Punct(Punct::LBracket)
            if touching
                && next_is(&TokenKind::Punct(Punct::LBracket))
                && after.is_some_and(|a| matches!(a.kind, TokenKind::Name(_))) =>
        {
            Some(Kind::Attribute)
        }
        // GMQ-18.1: `**`, two touching stars.
        TokenKind::Punct(Punct::Star) if touching && next_is(&TokenKind::Punct(Punct::Star)) => {
            Some(Kind::Power)
        }
        // GMQ-19.1: `:` touching a name, a label as GMQCC writes one.
        TokenKind::Punct(Punct::Colon)
            if touching && next.is_some_and(|n| matches!(n.kind, TokenKind::Name(_))) =>
        {
            Some(Kind::Goto)
        }
        _ => None,
    }
}

/// Every `.qc`, `.qh`, `.inc` and `.h` file under `dir`, sorted.
fn sources(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        let entries = fs::read_dir(&next).map_err(|e| format!("{}: {e}", next.display()))?;
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|x| {
                ["qc", "qh", "inc", "h"]
                    .iter()
                    .any(|e| x.eq_ignore_ascii_case(e))
            }) {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use std::fs;
    use std::path::{Path, PathBuf};

    use super::{Args, Kind, classify, in_spec, main};
    use crate::test_dirs::fresh;

    fn codebase(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = fresh(&format!("gmqcc-coverage/{name}"));
        fs::create_dir_all(dir.join("sub")).unwrap();
        for (file, text) in files {
            fs::write(dir.join(file), text).unwrap();
        }
        dir
    }

    // spec GMQ-20.2: each class is found by its signature, and FTEQCC's look-alikes are not.
    #[test]
    fn each_class_is_found_by_its_signature() {
        assert_eq!(classify(b"s = _(\"Hi\");"), vec![Kind::Translatable]);
        assert!(classify(b"s = _(x);").is_empty());
        assert_eq!(classify(b"return = 1;"), vec![Kind::ReturnAssign]);
        assert!(classify(b"return 1;").is_empty());
        assert_eq!(classify(b"e = nil;"), vec![Kind::Nil]);
        assert_eq!(
            classify(b"void f(float a, ...count);"),
            vec![Kind::Variadic]
        );
        assert_eq!(classify(b"x = ...(0, string);"), vec![Kind::Variadic]);
        assert!(classify(b"void f(...);").is_empty());
        assert_eq!(
            classify(b"#define A [[accumulate]]\n"),
            vec![Kind::Attribute]
        );
        assert!(classify(b"a[ [1] ];").is_empty());
        assert_eq!(classify(b"noref float x;"), vec![Kind::Noref]);
        assert_eq!(classify(b"y = x ** 2;"), vec![Kind::Power]);
        assert!(classify(b"y = x * *p;").is_empty());
        assert_eq!(classify(b"goto done;"), vec![Kind::Goto]);
        assert_eq!(classify(b"#define LABEL(id) :id\n"), vec![Kind::Goto]);
        assert!(classify(b"x = a ? b : c;").is_empty());
    }

    #[test]
    fn vanilla_source_has_no_gmqcc_construct() {
        assert!(classify(b"float a;\nvoid() f = { a = 1; return; };\n").is_empty());
    }

    // spec GMQ-20.1: every class detected is a section of the spec.
    #[test]
    fn the_spec_names_every_class() {
        for kind in Kind::ALL {
            assert!(in_spec(kind.section()), "{}", kind.section());
        }
        assert!(!in_spec("GMQ-99"));
    }

    // spec GMQ-20.1: a corpus with every class passes; one missing a class fails, and an empty
    // directory or no argument is an error.
    #[test]
    fn a_corpus_passes_only_with_every_class() {
        let all = "#define ACCUMULATE [[accumulate]]\n\
                   #define LABEL(id) :id\n\
                   noref float unused;\n\
                   entity e = nil;\n\
                   float f(float a, ...count) {\n\
                       string s = ...(0, string);\n\
                       s = _(\"Hello\");\n\
                       return = a ** 2;\n\
                       goto done;\n\
                       LABEL(done)\n\
                       return;\n\
                   }\n";
        let dir = codebase("all", &[("all.qc", all)]);
        let run = |dir: &Path| {
            crate::run_cli::<Args, _>(&[dir.display().to_string()], |a| main(Path::new("."), a))
        };
        assert!(run(&dir).unwrap());
        let some = codebase("some", &[("some.qh", "entity e = nil;\n")]);
        assert!(!run(&some).unwrap());
        let empty = codebase("empty", &[("readme.txt", "x")]);
        assert!(run(&empty).unwrap_err().contains("no .qc"));
        assert!(
            crate::run_cli::<Args, &str>(&[], |a| main(Path::new("."), a))
                .unwrap_err()
                .contains("Usage")
        );
    }
}
