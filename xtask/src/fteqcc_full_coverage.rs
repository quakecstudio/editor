// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask fteqcc-full-coverage <dir>...`: the coverage check for the full FTEQCC dialect
//! spec (P2-25, FTEF-1). It scans every `.qc`, `.qh` and `.h` file under each FTEQCC corpus
//! mod directory and lists every full-dialect construct the vanilla grammar lacks, grouped
//! into the classes of `docs/spec/dialect-fteqcc.md`. It fails if the source holds a class
//! that no section there names, so the spec and the corpus cannot drift apart.
//!
//! The FTEQCC corpus mods are the `mod` codebases that use the full FTEQCC dialect rather than
//! only the rerelease subset: Nuclide SDK, FreeCS and Rad-Therapy / FreeHL
//! (`corpus/manifest.toml`), fetched with `cargo xtask corpus fetch nuclide freecs freehl`.
//! The recorded run over the three is `docs/spec/vectors/dialect-fteqcc/coverage.txt`.
//!
//! This is the sibling of [`crate::fteqcc_coverage`], which checks the rerelease subset
//! (`docs/spec/dialect-fteqcc-rerelease.md`, FTE-*). The subset's constructs (`for`, `switch`,
//! `++`, `%`, `?:`, `const`, `var`, `nosave`, bare `enum`, inferred-size arrays,
//! `typedef struct`, `#pragma warning`) also appear in these mods; they belong to that file
//! and are not counted here. This tool counts only the constructs beyond the subset, which
//! `docs/spec/dialect-fteqcc.md` (FTEF-*) documents.
//!
//! Detection works on the token stream, on a signature of tokens for each class (a keyword the
//! full dialect introduces, or a punctuation sequence vanilla lacks). It is a syntactic
//! inventory, not a parse: the full dialect needs the FTEQCC profile (P2-26), which does not
//! exist yet, so the check cannot parse these files. That is why it classifies by signature
//! rather than by parser rejection, unlike the rerelease tool.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use qc_lexer::{Punct, TokenKind, lex};

/// Which full-dialect construct class a token sequence belongs to. Each maps to one section of
/// `docs/spec/dialect-fteqcc.md` beyond the rerelease subset. Every full-dialect construct
/// found in the FTEQCC corpus mods must be one of these.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    IntType,
    TypedLiteral,
    Pointer,
    SizedArray,
    StructUnion,
    Typedef,
    EnumFlags,
    Class,
    Accessor,
    Variant,
    Pragma,
}

impl Kind {
    /// Every class, for the "every class is exercised" check.
    const ALL: [Self; 11] = [
        Self::IntType,
        Self::TypedLiteral,
        Self::Pointer,
        Self::SizedArray,
        Self::StructUnion,
        Self::Typedef,
        Self::EnumFlags,
        Self::Class,
        Self::Accessor,
        Self::Variant,
        Self::Pragma,
    ];

    /// The spec section that documents this class.
    const fn section(self) -> &'static str {
        match self {
            Self::IntType => "FTEF-2",
            Self::TypedLiteral => "FTEF-3",
            Self::Pointer => "FTEF-4",
            Self::SizedArray => "FTEF-5",
            Self::StructUnion => "FTEF-6",
            Self::Typedef => "FTEF-7",
            Self::EnumFlags => "FTEF-8",
            Self::Class => "FTEF-9",
            Self::Accessor => "FTEF-10",
            Self::Variant => "FTEF-11",
            Self::Pragma => "FTEF-12",
        }
    }

    /// A short name for the report.
    const fn name(self) -> &'static str {
        match self {
            Self::IntType => "int/bool/double type",
            Self::TypedLiteral => "typed/hex literal",
            Self::Pointer => "pointer (-> & *)",
            Self::SizedArray => "sized array",
            Self::StructUnion => "struct/union",
            Self::Typedef => "typedef",
            Self::EnumFlags => "enumflags",
            Self::Class => "class/virtual",
            Self::Accessor => "accessor/get/set",
            Self::Variant => "__variant",
            Self::Pragma => "#pragma (beyond warning)",
        }
    }
}

const USAGE: &str = "usage: cargo xtask fteqcc-full-coverage <dir>...";

/// Runs the check on the FTEQCC corpus mod directories in `args`.
pub(crate) fn main(_root: &Path, args: &[String]) -> Result<bool, String> {
    let dirs: Vec<PathBuf> = args.iter().map(PathBuf::from).collect();
    if dirs.is_empty() {
        return Err(USAGE.to_owned());
    }
    let mut totals: BTreeMap<Kind, usize> = BTreeMap::new();
    let mut ok = true;
    for dir in &dirs {
        let files = sources(dir)?;
        if files.is_empty() {
            return Err(format!("no .qc, .qh or .h files under {}", dir.display()));
        }
        let mut found: BTreeMap<Kind, usize> = BTreeMap::new();
        for path in &files {
            let text = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
            for kind in classify(&text) {
                let entry = found.entry(kind).or_default();
                *entry = entry.saturating_add(1);
                let total = totals.entry(kind).or_default();
                *total = total.saturating_add(1);
            }
        }
        println!(
            "fteqcc-full-coverage: {}: {} files, {} full-dialect classes",
            dir.display(),
            files.len(),
            found.len()
        );
        for (kind, count) in &found {
            println!("  {:<24} {count:>5}  {}", kind.name(), kind.section());
        }
    }
    println!("fteqcc-full-coverage: total files with each class, by class:");
    for (kind, count) in &totals {
        let present = in_spec(kind.section());
        println!(
            "  {:<24} {count:>5}  {} ({})",
            kind.name(),
            kind.section(),
            if present {
                "in spec"
            } else {
                "MISSING FROM SPEC"
            }
        );
        if !present {
            ok = false;
        }
    }
    // Every class the spec names must have been exercised, or the spec claims a construct the
    // corpus does not use.
    for kind in Kind::ALL {
        if !totals.contains_key(&kind) {
            ok = false;
            println!(
                "FAIL  {} ({}) is in the spec but was not found in any corpus mod",
                kind.name(),
                kind.section()
            );
        }
    }
    println!(
        "fteqcc-full-coverage: {}",
        if ok {
            "every construct is in the spec"
        } else {
            "failures found"
        }
    );
    Ok(ok)
}

/// Whether the full FTEQCC dialect spec names `section` as one of its sections.
fn in_spec(section: &str) -> bool {
    let spec = Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/spec/dialect-fteqcc.md");
    let Ok(text) = fs::read_to_string(spec) else {
        return false;
    };
    text.lines()
        .any(|line| line.starts_with(&format!("## {section} ")))
}

/// The full-dialect construct classes present in `text`, each once.
fn classify(text: &[u8]) -> Vec<Kind> {
    let lexed = lex(text);
    let tokens = &lexed.tokens;
    let mut kinds: Vec<Kind> = Vec::new();
    let mut add = |kind: Kind| {
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
    };
    for (index, token) in tokens.iter().enumerate() {
        let next = index.checked_add(1).and_then(|n| tokens.get(n));
        let after = index.checked_add(2).and_then(|n| tokens.get(n));
        match &token.kind {
            // A keyword the full dialect introduces, anywhere it appears. These words are not
            // vanilla types or statements (QC-3, QC-10) and the rerelease subset does not use
            // them, so a match is a full-dialect construct. `struct`, `typedef` and `enum`
            // also start the rerelease subset's `typedef struct`/bare `enum` (FTE-14, FTE-15);
            // those are counted by the rerelease tool, but the general forms have their own
            // sections here (FTEF-6, FTEF-7), so a `struct`/`typedef`/`union` is a full-dialect
            // construct for this inventory too.
            TokenKind::Name(word) => {
                if let Some(kind) = keyword(word) {
                    add(kind);
                }
            }
            // `->`: a `-` immediately followed by `>` (no vanilla `->` token; QC-7). A pointer
            // dereference-and-select (FTEF-4.3).
            TokenKind::Punct(Punct::Minus)
                if next.is_some_and(|n| {
                    n.kind == TokenKind::Punct(Punct::Greater) && n.span.start == token.span.end
                }) =>
            {
                add(Kind::Pointer);
            }
            // A sized array declaration or type: `[` then a numeric literal then `]`
            // (FTEF-5.1). The rerelease subset uses only the inferred-size `[]` form (FTE-13),
            // which has no literal between the brackets, so this does not overlap it.
            TokenKind::Punct(Punct::LBracket)
                if next.is_some_and(|n| matches!(n.kind, TokenKind::Float(_)))
                    && after.is_some_and(|a| a.kind == TokenKind::Punct(Punct::RBracket)) =>
            {
                add(Kind::SizedArray);
            }
            // A typed or hex numeric literal: a number token immediately followed by a
            // suffix/`x` name with no gap (`12i`, `3.5d`, `0x20` lexes as Float(0) then
            // Name(x20)). Vanilla has only plain float literals (QC-4).
            TokenKind::Float(_) if touching_suffix(token, next) => {
                add(Kind::TypedLiteral);
            }
            // `#pragma <name>` with a name other than `warning` (which is the rerelease
            // subset's FTE-16): a full-dialect directive (FTEF-12).
            TokenKind::Punct(Punct::Hash)
                if token.line_start
                    && next.is_some_and(|n| n.kind == TokenKind::Name("pragma".to_owned()))
                    && after.is_some_and(|a| {
                        matches!(&a.kind, TokenKind::Name(w) if !w.eq_ignore_ascii_case("warning"))
                    }) =>
            {
                add(Kind::Pragma);
            }
            _ => {}
        }
    }
    kinds
}

/// Whether `token` (a number) is immediately followed by a one-or-two-letter type suffix or a
/// hex `x`, with no gap: a typed or hex literal (`12i`, `3.5d`, `1u`, `0x20`). A plain `float`
/// macro name after a space is not a suffix.
fn touching_suffix(token: &qc_lexer::Token, next: Option<&qc_lexer::Token>) -> bool {
    next.is_some_and(|n| {
        n.span.start == token.span.end && matches!(&n.kind, TokenKind::Name(w) if is_suffix(w))
    })
}

/// Whether `word` is a numeric type suffix (`i`, `u`, `l`, `d`, `f`, their combinations and
/// case variants) or begins a hex literal's tail (`x...`, `X...`), as `QCC_PR_LexNumber`
/// accepts.
fn is_suffix(word: &str) -> bool {
    if word.starts_with('x') || word.starts_with('X') {
        return true; // 0x.. lexes as Float(0) then Name(x..)
    }
    !word.is_empty()
        && word.len() <= 3
        && word
            .bytes()
            .all(|b| matches!(b.to_ascii_lowercase(), b'i' | b'u' | b'l' | b'd' | b'f'))
}

/// The construct class a keyword name belongs to, if it is a full-dialect keyword the vanilla
/// grammar lacks. Names only, since these words are never vanilla types or statements.
fn keyword(word: &str) -> Option<Kind> {
    match word {
        "int" | "integer" | "bool" | "double" | "long" | "short" | "char" | "signed"
        | "unsigned" => Some(Kind::IntType),
        "struct" | "union" => Some(Kind::StructUnion),
        "typedef" => Some(Kind::Typedef),
        "enumflags" => Some(Kind::EnumFlags),
        "class" | "virtual" | "nonvirtual" => Some(Kind::Class),
        "accessor" => Some(Kind::Accessor),
        "__variant" => Some(Kind::Variant),
        _ => None,
    }
}

/// Every `.qc`, `.qh` and `.h` file under `dir`, sorted.
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
                x.eq_ignore_ascii_case("qc")
                    || x.eq_ignore_ascii_case("qh")
                    || x.eq_ignore_ascii_case("h")
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

    use super::{Kind, classify, in_spec, is_suffix, main};
    use crate::test_dirs::fresh;

    fn codebase(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = fresh(&format!("fteqcc-full-coverage/{name}"));
        fs::create_dir_all(dir.join("sub")).unwrap();
        for (file, text) in files {
            fs::write(dir.join(file), text).unwrap();
        }
        dir
    }

    // spec FTEF-2.1: int/integer/bool/double and the C width keywords are detected.
    #[test]
    fn scalar_type_keywords_are_classified() {
        assert!(classify(b"int x;").contains(&Kind::IntType));
        assert!(classify(b"integer x;").contains(&Kind::IntType));
        assert!(classify(b"double d;").contains(&Kind::IntType));
        assert!(classify(b"unsigned long n;").contains(&Kind::IntType));
    }

    // spec FTEF-3.1, FTEF-3.2: a touching suffix or hex tail is a typed/hex literal; a spaced
    // name is not.
    #[test]
    fn typed_and_hex_literals_are_classified() {
        assert!(classify(b"x = 12i;").contains(&Kind::TypedLiteral));
        assert!(classify(b"x = 3.5d;").contains(&Kind::TypedLiteral));
        assert!(classify(b"x = 0x20;").contains(&Kind::TypedLiteral));
        assert!(!classify(b"x = 12 i;").contains(&Kind::TypedLiteral));
        assert!(is_suffix("i") && is_suffix("ll") && is_suffix("x20"));
        assert!(!is_suffix("index") && !is_suffix(""));
    }

    // spec FTEF-4.3: `->` is a pointer member access; a spaced `- >` is not.
    #[test]
    fn pointer_arrow_is_classified() {
        assert!(classify(b"a->b;").contains(&Kind::Pointer));
        assert!(!classify(b"a - > b;").contains(&Kind::Pointer));
    }

    // spec FTEF-5.1: `[N]` with a literal size is a sized array; bare `[]` is not.
    #[test]
    fn sized_array_is_classified() {
        assert!(classify(b"float a[3];").contains(&Kind::SizedArray));
        assert!(!classify(b"float a[];").contains(&Kind::SizedArray));
    }

    // spec FTEF-6.1, FTEF-7.1, FTEF-8.1, FTEF-9.1, FTEF-10.1, FTEF-11.1: the full-dialect
    // keywords are detected.
    #[test]
    fn full_dialect_keywords_are_classified() {
        assert!(classify(b"struct S { float f; };").contains(&Kind::StructUnion));
        assert!(classify(b"union U { float f; };").contains(&Kind::StructUnion));
        assert!(classify(b"typedef float real;").contains(&Kind::Typedef));
        assert!(classify(b"enumflags { A, B };").contains(&Kind::EnumFlags));
        assert!(classify(b"class Foo : Bar { virtual void M(); };").contains(&Kind::Class));
        assert!(classify(b"accessor strbuf : float;").contains(&Kind::Accessor));
        assert!(classify(b"__variant v;").contains(&Kind::Variant));
    }

    // spec FTEF-12.1: `#pragma` beyond `warning` is detected; `#pragma warning` (FTE-16) is
    // not counted here.
    #[test]
    fn full_dialect_pragma_is_classified() {
        assert!(classify(b"#pragma target fte\n").contains(&Kind::Pragma));
        assert!(classify(b"#pragma sourcefile foo.src\n").contains(&Kind::Pragma));
        assert!(!classify(b"#pragma warning disable F317\n").contains(&Kind::Pragma));
    }

    #[test]
    fn vanilla_source_has_no_full_dialect_construct() {
        assert!(classify(b"float a;\nvoid() f = { a = 1; };\n").is_empty());
    }

    #[test]
    fn the_spec_names_every_detected_section() {
        for section in ["FTEF-2", "FTEF-9", "FTEF-12"] {
            assert!(in_spec(section), "{section}");
        }
        assert!(!in_spec("FTEF-99"));
    }

    #[test]
    fn a_codebase_with_every_class_passes_and_bad_arguments_are_errors() {
        let dir = codebase(
            "fteqcc-full-coverage",
            &[(
                "all.qc",
                "typedef int handle;\n\
                 struct S { float f; };\n\
                 union U { int i; float f; };\n\
                 enumflags { A, B };\n\
                 accessor strbuf : float;\n\
                 __variant gv;\n\
                 #pragma target fte\n\
                 int arr[4];\n\
                 class Foo : Bar {\n\
                     virtual void Think(void);\n\
                     nonvirtual int Count(void);\n\
                 };\n\
                 void() f = {\n\
                     int n = 12i;\n\
                     float *p;\n\
                     gv = p->field;\n\
                 };\n",
            )],
        );
        assert!(main(Path::new("."), &[dir.display().to_string()]).unwrap());
        assert!(main(Path::new("."), &[]).unwrap_err().contains("usage"));
        let empty = codebase("fteqcc-full-coverage-empty", &[("readme.txt", "x")]);
        assert!(
            main(Path::new("."), &[empty.display().to_string()])
                .unwrap_err()
                .contains("no .qc")
        );
    }
}
