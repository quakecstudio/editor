// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use proptest::prelude::*;

use qc_lexer::lex;

use super::{
    Code, FileOutput, FileSpan, LIMIT_QCC_MAX_FRAMES, MemoryLoader, PpToken, Preprocessor,
    SourceMap, TokenKind,
};

/// A project: its files, and the ones compiled, in order. By default one file, `a.qc`,
/// is compiled.
struct ProjectSpec<'a> {
    files: &'a [(&'a str, &'a str)],
    compile: &'a [&'a str],
    /// The module's include directories (`Preprocessor::with_include_dirs`).
    include_dirs: &'a [&'a str],
}

impl Default for ProjectSpec<'_> {
    fn default() -> Self {
        Self {
            files: &[],
            compile: &["a.qc"],
            include_dirs: &[],
        }
    }
}

fn project(spec: &ProjectSpec<'_>) -> (Preprocessor<MemoryLoader>, Vec<FileOutput>) {
    let mut loader = MemoryLoader::new();
    for (path, text) in spec.files {
        loader.insert(*path, text.as_bytes());
    }
    let dirs: Vec<PathBuf> = spec.include_dirs.iter().map(PathBuf::from).collect();
    let mut pp = Preprocessor::new(loader).with_include_dirs(&dirs);
    let outputs = spec
        .compile
        .iter()
        .map(|p| pp.preprocess_file(Path::new(p)).unwrap())
        .collect();
    (pp, outputs)
}

/// Preprocesses `a.qc` with the given text.
fn one(text: &str) -> (Preprocessor<MemoryLoader>, FileOutput) {
    let (pp, mut outputs) = project(&ProjectSpec {
        files: &[("a.qc", text)],
        ..ProjectSpec::default()
    });
    (pp, outputs.remove(0))
}

/// Preprocesses `a.qc` under the FTEQCC dialect.
fn one_fte(text: &str) -> (Preprocessor<MemoryLoader>, FileOutput) {
    let mut loader = MemoryLoader::new();
    loader.insert("a.qc", text.as_bytes());
    let mut pp = Preprocessor::new(loader).with_dialect(qc_lexer::Dialect::Fteqcc);
    let out = pp.preprocess_file(Path::new("a.qc")).unwrap();
    (pp, out)
}

fn token_text(kind: &TokenKind) -> String {
    match kind {
        TokenKind::Name(name) => name.clone(),
        TokenKind::Float(value) => value.to_string(),
        TokenKind::Vector([x, y, z]) => format!("'{x} {y} {z}'"),
        TokenKind::String(bytes) => format!("\"{}\"", bytes.escape_ascii()),
        TokenKind::Punct(punct) => punct.as_str().to_owned(),
        TokenKind::FrameRef(word) => format!("${word}"),
        TokenKind::Frames(names) => {
            let names: Vec<&str> = names.iter().map(|n| n.name.as_str()).collect();
            format!("$frame {}", names.join(" "))
        }
        TokenKind::FrameCommand(command) => format!("${command:?}"),
    }
}

/// The tokens, space separated.
fn tokens(out: &FileOutput) -> String {
    out.tokens
        .iter()
        .map(|t| token_text(&t.kind))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `path:line`, the path with `/` separators on every OS.
fn where_(sources: &SourceMap, span: FileSpan) -> String {
    let file = sources.file(span.file).unwrap();
    let parts: Vec<String> = file
        .path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    format!("{}:{}", parts.join("/"), sources.line(span).unwrap())
}

/// The diagnostics, one per line: code, message, file and line.
fn diagnostics(pp: &Preprocessor<MemoryLoader>, out: &FileOutput) -> String {
    let mut text = String::new();
    for d in &out.diagnostics {
        writeln!(
            text,
            "{} {} @{}",
            d.code.as_str(),
            d.message,
            where_(pp.sources(), d.span)
        )
        .unwrap();
    }
    text
}

fn codes(out: &FileOutput) -> Vec<Code> {
    out.diagnostics.iter().map(|d| d.code).collect()
}

fn span_text(pp: &Preprocessor<MemoryLoader>, span: FileSpan) -> &str {
    std::str::from_utf8(pp.sources().text(span).unwrap()).unwrap()
}

// spec QC-9.1: vanilla `#` builtins are tokens, not directives, even at a line start
#[test]
fn vanilla_hash_is_a_token() {
    let text = "void() f = #1;\nvoid() g =\n#2;\nfloat a = 1; # b";
    let (pp, out) = one(text);
    assert_eq!(
        tokens(&out),
        "void ( ) f = # 1 ; void ( ) g = # 2 ; float a = 1 ; # b"
    );
    assert!(out.diagnostics.is_empty());
    let lexed: Vec<TokenKind> = lex(text.as_bytes())
        .tokens
        .into_iter()
        .map(|t| t.kind)
        .collect();
    let kinds: Vec<TokenKind> = out.tokens.iter().map(|t| t.kind.clone()).collect();
    assert_eq!(kinds, lexed);
    assert!(
        out.tokens
            .iter()
            .all(|t| t.expansion.is_none() && pp.origin(t) == t.span)
    );
}

// spec QC-9.1: a directive starts a line, outside any comment, `#` then optional blanks and a name
#[test]
fn directive_lines() {
    let (_, out) = one("  #  define A 1\nA /* x */ #define B 2\nB\n/*\n#define C 3\n*/ C");
    assert_eq!(tokens(&out), "1 # define B 2 B C");
    assert!(out.diagnostics.is_empty());
}

// spec QC-9.2: `#include` reads the file relative to the including file's directory
#[test]
fn include_relative_to_includer() {
    let files = [
        ("src/a.qc", "#include \"inc/b.qc\"\nfloat x;"),
        ("src/inc/b.qc", "float y;\n#include \"../c.qc\"\n"),
        ("src/c.qc", "float z;"),
    ];
    let (pp, outputs) = project(&ProjectSpec {
        files: &files,
        compile: &["src/a.qc"],
        ..ProjectSpec::default()
    });
    assert_eq!(tokens(&outputs[0]), "float y ; float z ; float x ;");
    let paths: Vec<String> = outputs[0]
        .tokens
        .iter()
        .map(|t| where_(pp.sources(), t.span))
        .filter(|w| w.ends_with(":1"))
        .collect();
    assert_eq!(paths[0], "src/inc/b.qc:1");
    assert_eq!(paths[3], "src/c.qc:1");
}

// spec QC-9.20: under the vanilla dialect an `#include` not beside the including file is found
// in the include directories, in order
#[test]
fn include_found_in_an_include_directory() {
    let files = [
        ("src/a.qc", "#include \"b.qh\"\nfloat x;"),
        ("one/b.qh", "float y;"),
        ("two/b.qh", "float z;"),
    ];
    let (pp, outputs) = project(&ProjectSpec {
        files: &files,
        compile: &["src/a.qc"],
        include_dirs: &["one", "two"],
    });
    assert_eq!(tokens(&outputs[0]), "float y ; float x ;");
    assert!(outputs[0].diagnostics.is_empty());
    let paths: Vec<String> = outputs[0]
        .tokens
        .iter()
        .map(|t| where_(pp.sources(), t.span))
        .collect();
    assert_eq!(paths[0], "one/b.qh:1");
    let (_, outputs) = project(&ProjectSpec {
        files: &files[..1],
        compile: &["src/a.qc"],
        include_dirs: &["one", "two"],
    });
    let codes: Vec<&str> = outputs[0]
        .diagnostics
        .iter()
        .map(|d| d.code.as_str())
        .collect();
    assert_eq!(codes, ["E0111"]);
    // The message shows the path as the host writes it.
    let message = outputs[0].diagnostics[0].message.replace('\\', "/");
    assert!(message.contains("src/b.qh"), "{message}");
}

// spec QC-9.20: a file beside the including file is preferred to one in an include directory,
// and an absolute name is not looked for in them
#[test]
fn include_beside_the_includer_is_preferred() {
    let files = [
        ("src/a.qc", "#include \"b.qh\"\nfloat x;"),
        ("src/b.qh", "float w;"),
        ("one/b.qh", "float y;"),
    ];
    let (_, outputs) = project(&ProjectSpec {
        files: &files,
        compile: &["src/a.qc"],
        include_dirs: &["one"],
    });
    assert_eq!(tokens(&outputs[0]), "float w ; float x ;");
    let files = [("a.qc", "#include \"/b.qh\"\n"), ("one/b.qh", "float y;")];
    let (_, outputs) = project(&ProjectSpec {
        files: &files,
        include_dirs: &["one"],
        ..ProjectSpec::default()
    });
    assert_eq!(outputs[0].diagnostics[0].code.as_str(), "E0111");
}

// spec QC-9.2: diagnostics in included text carry the included file's name and line
#[test]
fn include_diagnostics_name_the_included_file() {
    let files = [
        ("a.qc", "float a;\n#include \"b.qc\"\n%"),
        ("b.qc", "\n\nfloat b %;"),
    ];
    let (pp, outputs) = project(&ProjectSpec {
        files: &files,
        ..ProjectSpec::default()
    });
    assert_eq!(
        diagnostics(&pp, &outputs[0]),
        "E0105 Unknown punctuation @b.qc:3\nE0105 Unknown punctuation @a.qc:3\n"
    );
}

// spec QC-9.3: object-like and function-like macros; `(` must touch the name
#[test]
fn define_forms() {
    let (_, out) =
        one("#define E\n#define O  1 + 2  \n#define F(a, b) b a\n#define S (x)\nE O F(3, 4) S");
    assert_eq!(tokens(&out), "1 + 2 4 3 ( x )");
    assert!(out.diagnostics.is_empty());
}

// spec QC-9.3, QCD-7.2: an identical redefinition is fine; a different one warns and replaces
#[test]
fn redefinition() {
    let (pp, out) =
        one("#define A 1 + 2\n#define A 1   +   2\nA\n#define A 3\nA\n#define A(x) x\nA(4)");
    assert_eq!(tokens(&out), "1 + 2 3 4");
    assert!(pp.is_defined("A"));
    assert_eq!(
        diagnostics(&pp, &out),
        "W0102 macro A redefined @a.qc:4\nW0102 macro A redefined @a.qc:6\n"
    );
    assert!(!out.diagnostics[0].code.is_error());
    let (_, out) = one("#define A 1\n#define A 1 2");
    assert_eq!(
        codes(&out),
        vec![Code::W0102],
        "a longer body is a different body"
    );
}

// spec QC-9.4: replacement is rescanned, but never for the macro being expanded
#[test]
fn rescanning() {
    let (_, out) = one(
        "#define A B + A\n#define B C\n#define C 1\nA\n#define F(x) G(x) F(x)\n#define G(y) [y]\nF(2)",
    );
    assert_eq!(tokens(&out), "1 + A [ 2 ] F ( 2 )");
    let (_, out) = one("#define X Y\n#define Y X\nX Y");
    assert_eq!(tokens(&out), "X Y");
}

// spec QC-9.4: a function-like macro produced by expansion takes arguments after it
#[test]
fn rescanning_reaches_following_tokens() {
    let (_, out) = one("#define F G\n#define G(a) a a\nF(1)");
    assert_eq!(tokens(&out), "1 1");
}

/// `#define M0 1`, then `n - 1` macros each defined as the name of the one before, then a use
/// of the last.
fn macro_chain(n: usize) -> String {
    let mut text = "#define M0 1\n".to_owned();
    for (before, i) in (1..n).enumerate() {
        writeln!(text, "#define M{i} M{before}").unwrap();
    }
    writeln!(text, "M{}", n.saturating_sub(1)).unwrap();
    text
}

// spec QC-9.17: a chain of 10000 and of 100000 macros, each the name of the one before, is
// expanded to its end under both dialects on a 2 MiB thread, one expansion each, and so is a
// second use, after the first use's hide sets are dropped (with a hidden check that walks the
// whole set again, the second use takes quadratic time and the test times out)
#[test]
fn a_long_chain_of_macros_expands_to_its_end() {
    for n in [10_000, 100_000] {
        let mut text = macro_chain(n);
        writeln!(text, "M{}", n.saturating_sub(1)).unwrap();
        for dialect in [qc_lexer::Dialect::Vanilla, qc_lexer::Dialect::Fteqcc] {
            let text = text.clone();
            let (toks, errs, expansions) = std::thread::Builder::new()
                .stack_size(2 << 20)
                .spawn(move || {
                    let mut loader = MemoryLoader::new();
                    loader.insert("a.qc", text.as_bytes());
                    let mut pp = Preprocessor::new(loader).with_dialect(dialect);
                    let out = pp.preprocess_file(Path::new("a.qc")).unwrap();
                    (tokens(&out), codes(&out), pp.expansions().len())
                })
                .unwrap()
                .join()
                .unwrap();
            assert_eq!(
                (toks.as_str(), errs, expansions),
                ("1 1", vec![], n.saturating_mul(2))
            );
        }
    }
}

// spec QC-9.4: names in strings and comments are never replaced; a use before the define is not
#[test]
fn no_replacement_in_strings_comments_or_before_definition() {
    let (_, out) = one("A\n#define A 1\n\"A\" // A\n/* A */ A");
    assert_eq!(tokens(&out), "A \"A\" 1");
}

// spec QC-9.5: `#undef` removes a macro; removing an undefined one does nothing
#[test]
fn undef() {
    let (pp, out) = one("#define A 1\nA\n#undef A\nA\n#undef NOPE\n#undef A");
    assert_eq!(tokens(&out), "1 A");
    assert!(out.diagnostics.is_empty());
    assert!(!pp.is_defined("A"));
}

// spec QC-9.6: blocks are kept or skipped, `#else` flips, blocks nest
#[test]
fn conditionals() {
    let text = "#define A\n#ifdef A\na\n#ifndef A\nb\n#else\nc\n#ifdef B\nd\n#endif\n#endif\n#else\ne\n#endif\n#ifndef B\nf\n#endif";
    let (_, out) = one(text);
    assert_eq!(tokens(&out), "a c f");
    assert!(out.diagnostics.is_empty());
}

// spec QC-9.6: directives in a skipped block only count for nesting
#[test]
fn skipped_directives_only_nest() {
    let text = "#ifdef A\n#define B 1\n#bogus\n#include \"missing.qc\"\n#ifdef C\n#else\nx\n#endif\n#else\nB\n#endif";
    let (pp, out) = one(text);
    assert_eq!(tokens(&out), "B");
    assert!(out.diagnostics.is_empty());
    assert!(!pp.is_defined("B"));
}

// spec QC-9.6, QCD-8.2, QCD-8.3: a stray `#else` or `#endif`, or an unclosed block, is an error
#[test]
fn unbalanced_conditionals() {
    let (pp, out) = one("#endif\n#else\n#ifdef A\nx\n#ifndef A\ny");
    assert_eq!(
        diagnostics(&pp, &out),
        "E0109 #endif without #ifdef or #ifndef @a.qc:1\nE0109 #else without #ifdef or #ifndef @a.qc:2\nE0110 #ifdef or #ifndef without #endif @a.qc:3\nE0110 #ifdef or #ifndef without #endif @a.qc:5\n"
    );
    assert_eq!(tokens(&out), "");
}

// spec QC-9.6: lexical errors inside a skipped block are not reported
#[test]
fn skipped_lexical_errors() {
    let (_, out) = one("#ifdef FTE\nx = y % z; s = \"a\\tb\";\n#endif\n%");
    assert_eq!(codes(&out), vec![Code::E0105]);
    assert_eq!(out.diagnostics[0].span.span.start, 41);
    let (_, out) = one("#ifdef FTE\n%");
    assert_eq!(codes(&out), vec![Code::E0110]);
}

// spec QC-9.7: every expanded token keeps its definition span and its use span
#[test]
fn expansion_spans() {
    let text = "#define TWO 1 + 1\n#define SUM(a, b) (a + b)\nx = SUM(TWO, y);";
    let (pp, out) = one(text);
    assert_eq!(tokens(&out), "x = ( 1 + 1 + y ) ;");
    for token in &out.tokens {
        let span = pp.sources().text(token.span).unwrap();
        assert_eq!(
            span,
            token_text(&token.kind).as_bytes(),
            "the span holds the token's text"
        );
        let origin = pp.origin(token);
        let line = pp.sources().line(origin).unwrap();
        assert_eq!(line, 3, "every token's origin is on the line that uses it");
    }
    let expanded: Vec<&PpToken> = out
        .tokens
        .iter()
        .filter(|t| t.expansion.is_some())
        .collect();
    assert_eq!(expanded.len(), 7);
    for token in &expanded {
        assert_eq!(span_text(&pp, pp.origin(token)), "SUM(TWO, y)");
    }
    let one_token = expanded[1];
    let id = one_token.expansion.unwrap();
    let expansion = &pp.expansions()[id.0];
    assert_eq!(expansion.name, "TWO");
    assert_eq!(span_text(&pp, expansion.use_span), "TWO");
    assert_eq!(span_text(&pp, expansion.definition), "TWO");
    assert_eq!(
        pp.sources().line(one_token.span),
        Some(1),
        "its text is in TWO's definition"
    );
    let parent = &pp.expansions()[expansion.parent.unwrap().0];
    assert_eq!(parent.name, "SUM");
    assert_eq!(parent.parent, None);
    let y = expanded
        .iter()
        .find(|t| t.kind == TokenKind::Name("y".to_owned()))
        .unwrap();
    assert_eq!(
        pp.sources().line(y.span),
        Some(3),
        "an argument keeps its span at the use"
    );
}

// spec QC-9.7: expansions across files keep the definition's file
#[test]
fn expansion_spans_across_files() {
    let files = [
        ("a.qc", "#include \"defs.qh\"\nK"),
        ("defs.qh", "\n#define K 7"),
    ];
    let (pp, outputs) = project(&ProjectSpec {
        files: &files,
        ..ProjectSpec::default()
    });
    let token = &outputs[0].tokens[0];
    assert_eq!(where_(pp.sources(), token.span), "defs.qh:2");
    assert_eq!(where_(pp.sources(), pp.origin(token)), "a.qc:2");
}

// spec QC-9.8: macros run on across files; frame macros do not
#[test]
fn macros_span_files() {
    let files = [("a.qc", "#define A 1\n$frame f\n$f"), ("b.qc", "A $f")];
    let (pp, outputs) = project(&ProjectSpec {
        files: &files,
        compile: &["a.qc", "b.qc"],
        ..ProjectSpec::default()
    });
    assert_eq!(tokens(&outputs[0]), "0");
    assert_eq!(tokens(&outputs[1]), "1 $f");
    assert_eq!(
        diagnostics(&pp, &outputs[1]),
        "E0106 Unknown frame macro $f @b.qc:1\n"
    );
}

// spec QC-9.9, QCD-8.1: an unknown directive is an error and is dropped
#[test]
fn unknown_directive() {
    let (pp, out) = one("#pragma target fte\nx");
    assert_eq!(tokens(&out), "x");
    assert_eq!(
        diagnostics(&pp, &out),
        "E0108 unknown directive #pragma @a.qc:1\n"
    );
}

// spec FTE-16.1: `#pragma name args` is a compiler directive in an FTEQCC dialect, including
// the rerelease's `#pragma warning disable/enable F317`.
// spec FTE-16.3: a `#pragma` produces no tokens and no progs output.
#[test]
fn fteqcc_pragma_is_accepted_and_produces_no_tokens() {
    let (_pp, out) = one_fte("#pragma warning disable F317\nx\n#pragma warning enable F317\ny");
    assert_eq!(tokens(&out), "x y");
    assert!(out.diagnostics.is_empty());
    // An unrecognised pragma is still accepted and ignored (FTE-16.2), not an error.
    let (_pp, out) = one_fte("#pragma target fte\nx");
    assert_eq!(tokens(&out), "x");
    assert!(out.diagnostics.is_empty());
}

// spec FTE-16.1: the vanilla dialect still rejects `#pragma` as an unknown directive
// (QC-9.9), so the two profiles differ.
#[test]
fn vanilla_pragma_is_still_an_unknown_directive() {
    let (_pp, out) = one("#pragma warning disable F317\nx");
    assert_eq!(codes(&out), vec![Code::E0108]);
}

// spec QC-9.9, QCD-8.6, QCD-8.7, QCD-8.8: malformed operands are errors; extra tokens are ignored
#[test]
fn malformed_directives() {
    let text = "#include foo\n#define\n#define 1\n#undef\n#ifdef\n#endif\n#ifndef \"x\"\n#endif\n#define F(a b) a\n#define G(a,) a\n#define H(\n#define I(1) x";
    let (pp, out) = one(text);
    assert_eq!(
        diagnostics(&pp, &out),
        "E0113 #include expects a file name in quotes @a.qc:1\n\
         E0114 #define expects a macro name @a.qc:2\n\
         E0114 #define expects a macro name @a.qc:3\n\
         E0114 #undef expects a macro name @a.qc:4\n\
         E0114 #ifdef expects a macro name @a.qc:5\n\
         E0114 #ifndef expects a macro name @a.qc:7\n\
         E0115 bad parameter list for macro F @a.qc:9\n\
         E0115 bad parameter list for macro G @a.qc:10\n\
         E0115 bad parameter list for macro H @a.qc:11\n\
         E0115 bad parameter list for macro I @a.qc:12\n"
    );
    assert!(!pp.is_defined("F"));
    let (_, out) = one("#define A\n#ifdef A junk\nx\n#else junk\ny\n#endif junk\n#undef A junk\nA");
    assert_eq!(tokens(&out), "x A");
    assert!(out.diagnostics.is_empty());
}

// spec QC-9.10: a function-like macro name without `(` is an ordinary name
#[test]
fn function_like_without_parens() {
    let (_, out) = one("#define F(x) x\nF + F\n(1) F");
    assert_eq!(tokens(&out), "F + 1 F");
    assert!(out.diagnostics.is_empty());
}

// spec QC-9.10: arguments split at top-level commas; `F()` passes none
#[test]
fn arguments() {
    let (_, out) = one("#define Z() 0\n#define P(a, b) [a|b]\nZ() P((1, 2), f(3, 4)) P(, )");
    assert_eq!(tokens(&out), "0 [ ( 1 , 2 ) | f ( 3 , 4 ) ] [ | ]");
    assert!(out.diagnostics.is_empty());
}

// spec QC-9.10, QCD-8.9: a wrong argument count is an error and leaves the use unreplaced
#[test]
fn wrong_argument_count() {
    let (pp, out) = one("#define P(a, b) a\n#define Z() 0\nP(1) Z(2) P(1,\n2,3)");
    assert_eq!(tokens(&out), "P ( 1 ) Z ( 2 ) P ( 1 , 2 , 3 )");
    assert_eq!(
        diagnostics(&pp, &out),
        "E0116 macro P expects 2 arguments, found 1 @a.qc:3\nE0116 macro Z expects 0 arguments, found 1 @a.qc:3\nE0116 macro P expects 2 arguments, found 3 @a.qc:3\n"
    );
    let use_span = out.diagnostics[2].span;
    assert_eq!(span_text(&pp, use_span), "P(1,\n2,3)");
}

// spec QC-9.7: a use span runs to the `)` only when that `)` follows it in the same file
#[test]
fn use_span_ends_at_its_own_paren() {
    let files = [
        ("a.qc", "#define P(a) a\nP(1\n#include \"b.qc\"\n"),
        ("b.qc", "\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n)"),
    ];
    let (pp, outputs) = project(&ProjectSpec {
        files: &files,
        ..ProjectSpec::default()
    });
    assert_eq!(tokens(&outputs[0]), "1");
    let expansion = &pp.expansions()[0];
    assert_eq!(span_text(&pp, expansion.use_span), "P");
}

// spec QC-9.10, QCD-8.10: an argument list still open at the end of the file is an error
#[test]
fn unterminated_arguments() {
    let (pp, out) = one("#define P(a) a\nP(1, (2)");
    assert_eq!(tokens(&out), "P ( 1 , ( 2 )");
    assert_eq!(
        diagnostics(&pp, &out),
        "E0117 unterminated argument list for macro P @a.qc:2\n"
    );
}

// spec QC-9.11, QCD-8.4: an `#include` of a file that cannot be read is an error
#[test]
fn missing_include() {
    let (pp, out) = one("#include \"nope.qc\"\nx");
    assert_eq!(tokens(&out), "x");
    assert_eq!(
        diagnostics(&pp, &out),
        "E0111 cannot include nope.qc: no such file @a.qc:1\n"
    );
}

// spec QC-9.11, QCD-8.5: including a file already being included is an error
#[test]
fn recursive_include() {
    let files = [
        ("a.qc", "#include \"b.qc\"\na"),
        ("b.qc", "b\n#include \"./a.qc\"\n#include \"sub/../b.qc\""),
    ];
    let (pp, outputs) = project(&ProjectSpec {
        files: &files,
        ..ProjectSpec::default()
    });
    assert_eq!(tokens(&outputs[0]), "b a");
    assert_eq!(
        diagnostics(&pp, &outputs[0]),
        "E0112 recursive #include of a.qc @b.qc:2\nE0112 recursive #include of b.qc @b.qc:3\n"
    );
    let files = [("a.qc", "#include \"b.qc\"\n#include \"b.qc\"")];
    let files = [files[0], ("b.qc", "x")];
    let (_, outputs) = project(&ProjectSpec {
        files: &files,
        ..ProjectSpec::default()
    });
    assert_eq!(
        tokens(&outputs[0]),
        "x x",
        "including one file twice is not recursion"
    );
}

// spec QC-9.12: a block is closed in the file that opens it
#[test]
fn blocks_close_per_file() {
    let files = [
        ("a.qc", "#include \"b.qc\"\na\n#endif"),
        ("b.qc", "#ifdef X\nb"),
    ];
    let (pp, outputs) = project(&ProjectSpec {
        files: &files,
        ..ProjectSpec::default()
    });
    assert_eq!(tokens(&outputs[0]), "a");
    assert_eq!(
        diagnostics(&pp, &outputs[0]),
        "E0110 #ifdef or #ifndef without #endif @b.qc:1\nE0109 #endif without #ifdef or #ifndef @a.qc:3\n"
    );
    let files = [("a.qc", "#ifdef X\n"), ("b.qc", "b")];
    let (_, outputs) = project(&ProjectSpec {
        files: &files,
        compile: &["a.qc", "b.qc"],
        ..ProjectSpec::default()
    });
    assert_eq!(
        tokens(&outputs[1]),
        "b",
        "the next file starts outside the block"
    );
}

// spec QC-8.2, QC-8.7: frames number from 0 across `$frame` lines; a repeated name keeps its first index
#[test]
fn frame_numbers() {
    let (_, out) = one("$frame a b a\nfloat x;\nvoid() f = { x = $a; x = $b; };");
    assert_eq!(tokens(&out), "float x ; void ( ) f = { x = 0 ; x = 1 ; } ;");
    let (_, out) = one("$frame a b\n$frame c\nx = $c; y = $a;");
    assert_eq!(tokens(&out), "x = 2 ; y = 0 ;");
}

// spec QC-8.3: the frame list is cleared at the start of each source file, and an include shares it
#[test]
fn frames_per_source_file() {
    let files = [
        ("a.qc", "#include \"m.qc\"\n$b"),
        ("m.qc", "$frame a b"),
        ("b.qc", "$frame z\n$z $a"),
    ];
    let (pp, outputs) = project(&ProjectSpec {
        files: &files,
        compile: &["a.qc", "b.qc"],
        ..ProjectSpec::default()
    });
    assert_eq!(tokens(&outputs[0]), "1");
    assert_eq!(tokens(&outputs[1]), "0 $a");
    assert_eq!(
        diagnostics(&pp, &outputs[1]),
        "E0106 Unknown frame macro $a @b.qc:2\n"
    );
}

/// Preprocesses `compile` from `files` under the FTEQCC dialect; with `autoproto`, all of them
/// once first, then again after [`Preprocessor::restart`], returning the second pass.
fn fte_project(
    files: &[(&str, &str)],
    compile: &[&str],
    autoproto: bool,
) -> (Preprocessor<MemoryLoader>, Vec<FileOutput>) {
    let mut loader = MemoryLoader::new();
    for (path, text) in files {
        loader.insert(*path, text.as_bytes());
    }
    let mut pp = Preprocessor::new(loader).with_dialect(qc_lexer::Dialect::Fteqcc);
    let run = |pp: &mut Preprocessor<MemoryLoader>| -> Vec<FileOutput> {
        compile
            .iter()
            .map(|p| pp.preprocess_file(Path::new(p)).unwrap())
            .collect()
    };
    if autoproto {
        run(&mut pp);
        pp.restart();
    }
    let outputs = run(&mut pp);
    (pp, outputs)
}

// spec FTE-22.4: FTEQCC frame macros carry to later files; the numbering restarts in each
// file, a name another file defined takes the new value, and one the same file defined
// twice keeps its first.
#[test]
fn fteqcc_frames_carry_between_files() {
    let files = [
        ("a.qc", "$frame a b\nx = $b;"),
        ("b.qc", "x = $a; y = $b;\n$frame z b b\nx = $b; y = $z;"),
    ];
    let (pp, outputs) = fte_project(&files, &["a.qc", "b.qc"], false);
    assert_eq!(tokens(&outputs[0]), "x = 1 ;");
    assert_eq!(tokens(&outputs[1]), "x = 0 ; y = 1 ; x = 1 ; y = 0 ;");
    // FTEF-18.29: the uses before b.qc's own `$frame` are stale, and its second `b` is a
    // duplicate.
    assert_eq!(
        diagnostics(&pp, &outputs[1]),
        "W0105 Stale macro used (a, defined in a.qc) @b.qc:1\n\
         W0105 Stale macro used (b, defined in a.qc) @b.qc:1\n\
         W0106 Duplicate macro defined (b). Rename it. @b.qc:2\n"
    );
    // Vanilla clears the list per file (QC-8.3).
    let (_, vanilla) = project(&ProjectSpec {
        files: &files,
        compile: &["a.qc", "b.qc"],
        ..ProjectSpec::default()
    });
    assert_eq!(codes(&vanilla[1]), [Code::E0106, Code::E0106]);
    // Another file's definition replaces the value; the same file's second one does not.
    let files = [
        ("a.qc", "$frame a b c\nx = $c;"),
        ("b.qc", "$frame c\nx = $c;\n$frame d c\ny = $c;"),
    ];
    let (_, outputs) = fte_project(&files, &["a.qc", "b.qc"], false);
    assert_eq!(tokens(&outputs[0]), "x = 2 ;");
    assert_eq!(tokens(&outputs[1]), "x = 0 ; y = 0 ;");
}

// spec FTE-22.4: after the first autoprototyping pass its frame macros are still defined, so
// a file may use a frame a later file defines.
// spec FTE-22.3: the second pass forgets the first pass's `#define`s.
#[test]
fn autoprototyping_keeps_frames_and_forgets_macros() {
    let files = [
        ("a.qc", "x = $CORPSEFRAME;\n#ifdef LATER\nlater\n#endif\n"),
        ("b.qc", "$frame idle CORPSEFRAME\n#define LATER\n"),
    ];
    let (pp, outputs) = fte_project(&files, &["a.qc", "b.qc"], true);
    assert_eq!(tokens(&outputs[0]), "x = 1 ;");
    // FTEF-18.29: the frame is the first pass's reading of b.qc, so the use is stale.
    assert_eq!(
        diagnostics(&pp, &outputs[0]),
        "W0105 Stale macro used (CORPSEFRAME, defined in b.qc) @a.qc:1\n"
    );
    let (_, single) = fte_project(&files, &["a.qc", "b.qc"], false);
    assert_eq!(codes(&single[0]), [Code::E0106]);
}

// spec QC-8.5, QCD-2.6: a reference is the frame's index; an unknown one is an error
#[test]
fn frame_references() {
    let (pp, out) = one("$frame a b\nfloat x = $b;\ny = $c; $a");
    assert_eq!(tokens(&out), "float x = 1 ; y = $c ; 0");
    assert_eq!(out.tokens[3].kind, TokenKind::Float(1.0));
    assert_eq!(span_text(&pp, out.tokens[3].span), "$b");
    assert_eq!(
        diagnostics(&pp, &out),
        "E0106 Unknown frame macro $c @a.qc:3\n"
    );
    let (_, out) = one("$a\n$frame a");
    assert_eq!(
        codes(&out),
        vec![Code::E0106],
        "a frame is visible only after its line"
    );
}

// spec QC-8.6: `($b)` looks up `b)` and fails
#[test]
fn frame_reference_with_trailing_paren() {
    let (pp, out) = one("$frame a b\nfloat x;\nvoid() f = { x = ($b); };");
    assert_eq!(
        diagnostics(&pp, &out),
        "E0106 Unknown frame macro $b) @a.qc:3\n"
    );
}

// spec QC-8.8: `$modelname` and the other modelgen commands are undefined frame macros
#[test]
fn modelgen_commands_are_references() {
    for command in [
        "modelname",
        "framegroupstart",
        "framegroupend",
        "load",
        "spritename",
        "type",
    ] {
        let (pp, out) = one(&format!("float a;\n${command} foo\nfloat b;"));
        assert_eq!(
            diagnostics(&pp, &out),
            format!("E0106 Unknown frame macro ${command} @a.qc:2\n")
        );
    }
}

// spec QC-27.5, QCD-6.4: more than LIMIT_QCC_MAX_FRAMES frames in one file is an error
#[test]
fn too_many_frames() {
    let names: Vec<String> = (0..=LIMIT_QCC_MAX_FRAMES)
        .map(|i| format!("f{i}"))
        .collect();
    let fits = format!("$frame {}\n$f255", names[..LIMIT_QCC_MAX_FRAMES].join(" "));
    let (_, out) = one(&fits);
    assert!(out.diagnostics.is_empty());
    assert_eq!(tokens(&out), "255");
    let over = format!("$frame {}\n$frame extra\n$f256", names.join(" "));
    let (pp, out) = one(&over);
    assert_eq!(
        diagnostics(&pp, &out),
        "E0504 too many frames @a.qc:1\nE0106 Unknown frame macro $f256 @a.qc:3\n"
    );
}

#[test]
fn missing_source_file_is_an_io_error() {
    let mut pp = Preprocessor::new(MemoryLoader::new());
    let err = pp.preprocess_file(Path::new("nope.qc")).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn fs_loader_reads_files() {
    let dir = std::env::temp_dir().join(format!("qc-preproc-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("inc")).unwrap();
    std::fs::write(dir.join("a.qc"), "#include \"inc/b.qc\"\na").unwrap();
    std::fs::write(dir.join("inc/b.qc"), "b").unwrap();
    let mut pp = Preprocessor::new(super::FsLoader);
    let out = pp.preprocess_file(&dir.join("a.qc")).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(tokens(&out), "b a");
    assert_eq!(pp.sources().files().len(), 2);
}

// spec PRJ-7.2: the file system loader reads the stored name from the directory, the exact
// entry first and otherwise the one that differs only in case, whatever the host's case rules
#[test]
fn fs_loader_reads_stored_names() {
    use super::Loader as _;
    let dir = std::env::temp_dir().join(format!("qc-preproc-case-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("Sub")).unwrap();
    std::fs::write(dir.join("Sub/Animations.h"), "").unwrap();
    let mut loader = super::FsLoader;
    let stored = |loader: &mut super::FsLoader, path: &str| {
        loader
            .stored_name(&dir.join(path))
            .map(|n| n.to_string_lossy().into_owned())
    };
    let found = [
        stored(&mut loader, "Sub/Animations.h"),
        stored(&mut loader, "Sub/animations.h"),
        stored(&mut loader, "sub"),
        stored(&mut loader, "Sub/missing.h"),
        stored(&mut loader, "missing/a.h"),
    ];
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(
        found,
        [
            Some("Animations.h".to_owned()),
            Some("Animations.h".to_owned()),
            Some("Sub".to_owned()),
            None,
            None,
        ]
    );
}

// spec PRJ-7.1: a memory loader matches paths exactly, or, made case-insensitive, ignoring
// the case of each component
// spec PRJ-7.2: its stored name is the held spelling, and a case-sensitive loader has none
// for a name in the wrong case
#[test]
fn memory_loader_case_rules() {
    use super::Loader as _;
    let stored = |loader: &mut MemoryLoader, path: &str| {
        loader
            .stored_name(Path::new(path))
            .map(|n| n.to_string_lossy().into_owned())
    };
    let mut exact = MemoryLoader::new();
    exact.insert("Sub/Animations.h", "x");
    assert!(exact.load(Path::new("sub/animations.h")).is_err());
    assert_eq!(
        stored(&mut exact, "Sub/Animations.h").as_deref(),
        Some("Animations.h")
    );
    assert_eq!(stored(&mut exact, "Sub").as_deref(), Some("Sub"));
    assert_eq!(stored(&mut exact, "Sub/animations.h"), None);
    let mut folded = MemoryLoader::case_insensitive();
    folded.insert("Sub/Animations.h", "x");
    folded.insert("Sub/b.h", "y");
    assert_eq!(folded.load(Path::new("sub/ANIMATIONS.H")).unwrap(), b"x");
    assert_eq!(folded.load(Path::new("Sub/b.h")).unwrap(), b"y");
    assert!(folded.load(Path::new("sub/c.h")).is_err());
    assert!(folded.load(Path::new("sub")).is_err());
    assert_eq!(
        stored(&mut folded, "sub/animations.h").as_deref(),
        Some("Animations.h")
    );
    assert_eq!(stored(&mut folded, "SUB").as_deref(), Some("Sub"));
    assert_eq!(stored(&mut folded, "Sub/b.h").as_deref(), Some("b.h"));
    assert_eq!(stored(&mut folded, "sub/c.h"), None);
}

// spec PRJ-7.2: when a directory holds two entries that differ only in case, the stored name
// is the least of them, so the warning is the same whatever order the file system lists them
#[test]
fn fs_loader_picks_the_least_case_variant() {
    use std::ffi::{OsStr, OsString};

    use super::Loader as _;
    use super::source::stored_entry;
    // A case-sensitive directory's listing, in both orders: `D` (0x44) is less than `d`
    // (0x64), so the least is `Defs.qc`, and an entry named exactly wins.
    let listing =
        |names: &[&str]| -> Vec<OsString> { names.iter().map(|&n| OsString::from(n)).collect() };
    let stored = |name: &str, names: &[&str]| {
        stored_entry(OsStr::new(name), listing(names)).map(|n| n.to_string_lossy().into_owned())
    };
    for names in [["defs.qc", "Defs.qc"], ["Defs.qc", "defs.qc"]] {
        assert_eq!(stored("DEFS.qc", &names).as_deref(), Some("Defs.qc"));
        assert_eq!(stored("defs.qc", &names).as_deref(), Some("defs.qc"));
    }
    assert_eq!(stored("x.qc", &["defs.qc"]), None);
    // `FsLoader` reads the directory's listing, on any host.
    let dir = std::env::temp_dir().join(format!("qc-preproc-least-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Defs.qc"), "").unwrap();
    let found = super::FsLoader
        .stored_name(&dir.join("DEFS.qc"))
        .map(|n| n.to_string_lossy().into_owned());
    let missing = super::FsLoader.stored_name(&dir.join("other.qc"));
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(found.as_deref(), Some("Defs.qc"));
    assert_eq!(missing, None);
}

// spec PRJ-7.2: when a memory loader holds two keys whose first components differ only in
// case, the stored name is the least, whatever order the map iterates them
#[test]
fn memory_loader_picks_the_least_case_variant() {
    use super::Loader as _;
    let mut folded = MemoryLoader::case_insensitive();
    folded.insert("Defs.qc", "x");
    folded.insert("defs.qc", "y");
    let stored = folded
        .stored_name(Path::new("DEFS.qc"))
        .map(|n| n.to_string_lossy().into_owned());
    // `D` (0x44) is less than `d` (0x64), so the least is `Defs.qc`.
    assert_eq!(stored.as_deref(), Some("Defs.qc"));
}

// spec PRJ-7.2: a loader that does not list its directory cannot tell the stored name, so the
// default answer is none; the graph then never reports W0402 for it
#[test]
fn a_loader_without_a_directory_has_no_stored_name() {
    use super::Loader as _;
    /// A loader that serves one file and uses the trait's default `stored_name`.
    struct BareLoader;
    impl super::Loader for BareLoader {
        fn load(&mut self, _path: &Path) -> std::io::Result<Vec<u8>> {
            Ok(Vec::new())
        }
    }
    let mut loader = BareLoader;
    assert_eq!(loader.stored_name(Path::new("Anything.qc")), None);
    assert_eq!(loader.stored_name(Path::new("a/b/c.qc")), None);
}

proptest! {
    // Without directives or `$`, preprocessing changes nothing: the tokens are the lexer's.
    #[test]
    fn vanilla_text_is_unchanged(text in "[a-z0-9 ;,(){}=+*/<>!&|.\n\"'-]{0,80}") {
        let (pp, out) = one(&text);
        let lexed = lex(text.as_bytes());
        let kinds: Vec<TokenKind> = out.tokens.iter().map(|t| t.kind.clone()).collect();
        let expected: Vec<TokenKind> = lexed.tokens.into_iter().map(|t| t.kind).collect();
        prop_assert_eq!(kinds, expected);
        prop_assert_eq!(out.diagnostics.len(), lexed.diagnostics.len());
        for t in &out.tokens {
            prop_assert_eq!(pp.origin(t), t.span);
        }
    }

    // Every expanded token's origin is a span of the compiled file that starts with the
    // macro's name, and its own span holds its text.
    #[test]
    fn expanded_tokens_map_to_their_use(
        uses in proptest::collection::vec(prop_oneof![Just("A"), Just("F(x, 2)"), Just("G"), Just("y")], 0..12)
    ) {
        let text = format!("#define A 1 + B\n#define B (2)\n#define F(p, q) q p A\n#define G F(A, G)\n{}", uses.join(" "));
        let (pp, out) = one(&text);
        prop_assert!(out.diagnostics.is_empty());
        for token in &out.tokens {
            let text = pp.sources().text(token.span).unwrap();
            let expected = token_text(&token.kind);
            prop_assert_eq!(text, expected.as_bytes());
            let origin = pp.origin(token);
            prop_assert_eq!(pp.sources().line(origin), Some(5));
            let used = span_text(&pp, origin);
            prop_assert!(token.expansion.is_none() || ["A", "F(x, 2)", "G"].contains(&used));
        }
    }
}
mod directives;
mod fteqcc;
mod graph;
mod lines;
mod load;
mod reviewer;
mod spacing;
mod text;
mod warnings;
