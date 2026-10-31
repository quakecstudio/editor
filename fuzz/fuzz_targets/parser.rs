// SPDX-License-Identifier: GPL-3.0-or-later

//! Fuzzes the parser (P1-22, P2-51, QA-2) with grammar-aware mutation, under the vanilla and
//! the FTEQCC profiles.
//!
//! The check, for each profile: parsing any bytes terminates without a panic or a stack
//! overflow, the tree's tokens tile the input and give it back byte for byte, every node lies
//! inside its parent, every diagnostic points inside the input and is reported once (QCD-1.5),
//! and parsing the same bytes twice gives the same tree.
//!
//! The mutator works on QuakeC tokens rather than bytes, so mutants stay close to programs
//! and reach the parser's grammar instead of stopping in the lexer. It lexes the input with
//! `qc-lexer` under the FTEQCC profile, whose tokens include the vanilla ones, and deletes,
//! duplicates, replaces, swaps or moves whole tokens and token runs, inserts QuakeC fragments
//! (keywords, types, punctuation, literals, frame macros, directives, comments, and the FTEQCC
//! dialect's operators, declarations and statements), and wraps token runs in brackets and
//! prefix operators to probe the nesting limit (QC-27.15, FTEF-18.32). One mutation in eight is libFuzzer's own byte mutation, so the lexer's edge
//! cases stay covered. An input longer than 4 KB is first cut to a window of whole tokens,
//! which keeps the run fast on whole corpus files. The cross-over splices a token run of one
//! input into the other.
//!
//! Seed it with QuakeC source: the nightly `Fuzz` workflow copies the `.qc` and `.qh` files of
//! the corpus (`cargo xtask corpus fetch id-1.01 rerelease copper arcane-dimensions`) into
//! `fuzz/corpus/parser/`.

#![no_main]

use libfuzzer_sys::{fuzz_crossover, fuzz_mutator, fuzz_target, fuzzer_mutate};
use qc_lexer::{Dialect, lex_with};
use qc_syntax::{Code, SyntaxNode, parse_with};

fuzz_target!(|data: &[u8]| {
    for dialect in [Dialect::Vanilla, Dialect::Fteqcc] {
        check(data, dialect);
    }
});

/// Parses `data` in `dialect` and checks the tree, the diagnostics and determinism.
fn check(data: &[u8], dialect: Dialect) {
    let parsed = parse_with(data, dialect);
    let root = parsed.syntax();
    assert_eq!(
        root.text(),
        data,
        "{dialect:?}: the tree does not give back the input"
    );
    check_tiling(&root, data);
    let mut seen = std::collections::HashSet::new();
    for diagnostic in parsed.diagnostics() {
        assert!(
            diagnostic.span.start <= diagnostic.span.end && diagnostic.span.end <= data.len(),
            "{dialect:?}: diagnostic outside the input: {diagnostic:?}"
        );
        // QCD-1.5: no two diagnostics with one code and one span, but for `W0227`, one per
        // operator, which two operators can give at one token.
        assert!(
            diagnostic.code == Code::W0227 || seen.insert((diagnostic.code, diagnostic.span)),
            "{dialect:?}: diagnostic reported twice: {diagnostic:?}"
        );
    }
    assert_eq!(
        parse_with(data, dialect),
        parsed,
        "{dialect:?}: parsing the same bytes twice differs"
    );
}

/// Checks that the tokens of `root` tile `data` in order and every node is inside its parent.
fn check_tiling(root: &SyntaxNode, data: &[u8]) {
    let mut at = 0;
    for token in root.tokens() {
        let span = token.span();
        assert_eq!(span.start, at, "a gap or overlap before {token:?}");
        assert_eq!(data.get(span.range()), Some(token.text()), "{token:?}");
        at = span.end;
    }
    assert_eq!(
        at,
        data.len(),
        "the tokens stop before the end of the input"
    );
    for node in root.descendants() {
        for child in node.children() {
            assert!(
                node.span().contains(child.span()),
                "{child:?} is outside {node:?}"
            );
        }
    }
}

/// Fragments the mutator inserts: every keyword and punctuation token of QC-3 and QC-7, the
/// shapes the grammar is built from, and the FTEQCC dialect's tokens and constructs
/// (`dialect-fteqcc-rerelease.md`, `dialect-fteqcc.md`).
const FRAGMENTS: &[&[u8]] = &[
    b"void",
    b"float",
    b"vector",
    b"string",
    b"entity",
    b"local",
    b"if",
    b"else",
    b"while",
    b"do",
    b"return",
    b"self",
    b"world",
    b"time",
    b"&&",
    b"||",
    b"<=",
    b">=",
    b"==",
    b"!=",
    b";",
    b",",
    b"!",
    b"*",
    b"/",
    b"(",
    b")",
    b"-",
    b"+",
    b"=",
    b"[",
    b"]",
    b"{",
    b"}",
    b"...",
    b".",
    b"<",
    b">",
    b"#",
    b"&",
    b"|",
    b"0",
    b"1",
    b"-1",
    b"0.5",
    b"1e10",
    b"2147483648",
    b"'0 0 0'",
    b"'1 -2 3.5'",
    b"'1 2'",
    b"\"\"",
    b"\"a\\n\\\"b\"",
    b"\"unterminated",
    b"$frame a b c\n",
    b"$walk1",
    b"$",
    b"$cd /x\n",
    b"#1",
    b"#0",
    b"#define X 1\n",
    b"#ifdef X\n",
    b"#endif\n",
    b"// c\n",
    b"/* c */",
    b"/*",
    b"*/",
    b"\n",
    b" ",
    b"\t",
    b"\r\n",
    b"x",
    b"_y2",
    b"x.y",
    b".float",
    b".vector v;",
    b".void() th;",
    b"void() f;",
    b"void(float a, ...) g;",
    b"float x = 1;",
    b"void() f = {};",
    b"void() f = #1;",
    b"void() f = [$a, g] {};",
    b"local float x;",
    b"if (x) return;",
    b"while (1) {}",
    b"do {} while (x);",
    b"f(1, 2)",
    b"x = y = z",
    b"a.b.c",
    b"((x))",
    b"!!x",
    b"-(x)",
    b"return x;",
    b"{ }",
    b"self.think = f;",
    b"++",
    b"--",
    b"+=",
    b"&~=",
    b"(+)",
    b"%",
    b"?",
    b":",
    b"::",
    b"->",
    b"..",
    b"~",
    b"^",
    b"<<",
    b">>=",
    b"0x1f",
    b"07",
    b"12i",
    b"1.0f",
    b".5",
    b"'a'",
    b"'\\n'",
    b"'8 8'",
    b"\"\\x41\\{65}\\u0041\\b\"",
    b"\\\n",
    b"const",
    b"var",
    b"nosave",
    b"static",
    b"__used",
    b"int",
    b"unsigned long",
    b"__variant",
    b"for (int i = 0; i < 3; i++)",
    b"switch (x) { case 1: break; case 2 .. 4: default: }",
    b"continue;",
    b"x ? y : z",
    b"x ?: z",
    b"typedef struct { float a; } T;",
    b"typedef enum { A, B } E;",
    b"struct s { float a; struct s *next; };",
    b"struct S { public: float a; private: float b; protected: float c; };",
    b"struct S : P { struct { float a; }; union { float b; vector v; }; float c : 3; };",
    b"struct S { enum E { A } e; struct T { float a; } *t; };",
    b"class C; class C x; class C { float a; } *p;",
    b"class C { protected: : float a; private: void() m = {}; };",
    b"typedef float t, *u, v[2];",
    b"typedef typedef struct S { float a; } T, *PT;",
    b"void() g = { struct { float a; } x; (struct S *)0; };",
    b"accessor A : float; typedef accessor B : float;",
    b"union u { float f; int i; };",
    b"enum class E : float { A, B = 2 };",
    b"enumflags { F1, F2 };",
    b"class C : D { float x; virtual void(void) f; public: };",
    b"accessor h : float { get float v = f; set float v = g; };",
    b"void C::f(void) { super::f(); };",
    b"float f(float a = 1, ...) { return a; };",
    b"void() f = #0:name;",
    b"float t[4] = { 1, 2, .x = 3 };",
    b"float *p, **q;",
    b"float(float)(string) ff;",
    b"..float ff;",
    b"(float)",
    b"(T *)",
    b"sizeof(int)",
    b"[1, 2, 3]",
    b"&x",
    b"*p",
    b"if not (x)",
    b"if !(x)",
    b"do {} __until (x);",
    b"goto l;",
    b"l:",
    b":l",
    b"return = x;",
    b"[++ 1 .. 4]",
    b"[--(w) 4 .. 1]",
    b"#if 1 && !defined(X)\n",
    b"#elif 0\n",
    b"#else\n",
    b"#warning w\n",
    b"#pragma noref 1\n",
    b"$framevalue 3\n",
    b"$flush\n",
];

/// Brackets the mutator wraps a token run in.
const WRAPS: &[(&[u8], &[u8])] = &[
    (b"(", b")"),
    (b"{", b"}"),
    (b"[", b"]"),
    (b"!", b""),
    (b"-", b""),
    (b"f(", b")"),
    (b"x.", b""),
    (b"if (x) ", b""),
    (b"void() f = {", b"};"),
    (b"++", b""),
    (b"~", b""),
    (b"(float)", b""),
    (b"x ? ", b" : y"),
    (b"struct { ", b" }"),
    (b"float", b"()"),
    (b"float*", b""),
];

/// A xorshift generator over libFuzzer's seed, so a mutation is reproducible from its seed.
struct Rng(u32);

impl Rng {
    fn new(seed: u32) -> Self {
        Self(seed | 1)
    }

    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }

    /// A number below `n`, or 0 when `n` is 0.
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        usize::try_from(self.next()).map_or(0, |value| value % n)
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        items.get(self.below(items.len()))
    }
}

/// The byte ranges of the tokens of `data`, in order, lexed under the FTEQCC profile.
fn token_spans(data: &[u8]) -> Vec<(usize, usize)> {
    lex_with(data, Dialect::Fteqcc)
        .tokens
        .iter()
        .map(|t| (t.span.start, t.span.end))
        .collect()
}

/// A run of whole tokens, as a byte range: from the start of one token to the end of a later
/// one, at most `max_tokens` long.
fn token_run(rng: &mut Rng, spans: &[(usize, usize)], max_tokens: usize) -> Option<(usize, usize)> {
    let first = rng.below(spans.len());
    let len = rng.below(max_tokens).saturating_add(1);
    let last = first
        .saturating_add(len)
        .saturating_sub(1)
        .min(spans.len().saturating_sub(1));
    Some((spans.get(first)?.0, spans.get(last)?.1))
}

/// `data` with `range` replaced by `with`.
fn splice(data: &[u8], (start, end): (usize, usize), with: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len().saturating_add(with.len()));
    out.extend_from_slice(data.get(..start).unwrap_or_default());
    out.extend_from_slice(with);
    out.extend_from_slice(data.get(end..).unwrap_or_default());
    out
}

/// One token-level mutation of `data`, or `None` when the one chosen does not apply.
fn mutate_tokens(rng: &mut Rng, data: &[u8]) -> Option<Vec<u8>> {
    let spans = token_spans(data);
    let fragment = *rng.pick(FRAGMENTS)?;
    if spans.is_empty() {
        return Some([data, fragment].concat());
    }
    let run = token_run(rng, &spans, 8)?;
    let text = data.get(run.0..run.1)?.to_vec();
    let at = rng.pick(&spans)?.0;
    match rng.below(7) {
        // Delete a token run.
        0 => Some(splice(data, run, b"")),
        // Duplicate a token run in place.
        1 => Some(splice(data, (run.0, run.0), &text)),
        // Replace a token run with a fragment.
        2 => Some(splice(data, run, fragment)),
        // Insert a fragment before a token.
        3 => Some(splice(data, (at, at), fragment)),
        // Copy a token run before another token.
        4 => Some(splice(data, (at, at), &text)),
        // Wrap a token run in brackets, many times over.
        5 => {
            let (open, close) = *rng.pick(WRAPS)?;
            let depth = if rng.below(4) == 0 { rng.below(400) } else { 1 };
            let mut wrapped = open.repeat(depth);
            wrapped.extend_from_slice(&text);
            wrapped.extend_from_slice(&close.repeat(depth));
            Some(splice(data, run, &wrapped))
        }
        // Swap two tokens.
        _ => {
            let (a, b) = (*rng.pick(&spans)?, *rng.pick(&spans)?);
            let (a, b) = if a.0 <= b.0 { (a, b) } else { (b, a) };
            if a.1 > b.0 {
                return None;
            }
            let mut out = Vec::with_capacity(data.len());
            out.extend_from_slice(data.get(..a.0)?);
            out.extend_from_slice(data.get(b.0..b.1)?);
            out.extend_from_slice(data.get(a.1..b.0)?);
            out.extend_from_slice(data.get(a.0..a.1)?);
            out.extend_from_slice(data.get(b.1..)?);
            Some(out)
        }
    }
}

fuzz_mutator!(|data: &mut [u8], size: usize, max_size: usize, seed: u32| {
    let mut rng = Rng::new(seed);
    let Some(input) = data.get(..size) else {
        return fuzzer_mutate(data, size, max_size);
    };
    let input = window(&mut rng, input);
    let mutant = if rng.below(8) == 0 {
        None
    } else {
        mutate_tokens(&mut rng, &input)
    };
    let (mutant, byte_level) = match mutant {
        Some(mutant) => (mutant, false),
        None => (input, true),
    };
    let len = mutant.len().min(max_size);
    let (Some(out), Some(src)) = (data.get_mut(..len), mutant.get(..len)) else {
        return fuzzer_mutate(data, size, max_size);
    };
    out.copy_from_slice(src);
    if byte_level {
        return fuzzer_mutate(data, len, max_size.min(WINDOW.saturating_mul(2)));
    }
    len
});

/// The most bytes of an input the mutator works on.
///
/// Corpus files run to 36 KB; the parser runs about five times as many 4 KB inputs a second
/// and finds new coverage faster on them, so a larger input is cut to a window first.
const WINDOW: usize = 4096;

/// `input`, or when it is longer than [`WINDOW`], a window of it from the start of one token
/// to the end of a later one.
fn window(rng: &mut Rng, input: &[u8]) -> Vec<u8> {
    if input.len() <= WINDOW {
        return input.to_vec();
    }
    let spans = token_spans(input);
    let Some(&(start, _)) = rng.pick(&spans) else {
        return input.get(..WINDOW).unwrap_or_default().to_vec();
    };
    let limit = start.saturating_add(WINDOW);
    let end = spans
        .iter()
        .map(|&(_, end)| end)
        .take_while(|&end| end <= limit)
        .last()
        .filter(|&end| end > start)
        .unwrap_or_else(|| limit.min(input.len()));
    input.get(start..end).unwrap_or_default().to_vec()
}

fuzz_crossover!(|data1: &[u8], data2: &[u8], out: &mut [u8], seed: u32| {
    let mut rng = Rng::new(seed);
    let data1 = window(&mut rng, data1);
    let spans1 = token_spans(&data1);
    let spans2 = token_spans(data2);
    let at = rng.pick(&spans1).map_or(data1.len(), |span| span.0);
    let run = token_run(&mut rng, &spans2, 64).unwrap_or((0, data2.len().min(WINDOW)));
    let child = splice(
        &data1,
        (at, at),
        data2.get(run.0..run.1).unwrap_or_default(),
    );
    let len = child.len().min(out.len());
    if let (Some(dest), Some(src)) = (out.get_mut(..len), child.get(..len)) {
        dest.copy_from_slice(src);
    }
    len
});
