// SPDX-License-Identifier: GPL-3.0-or-later

//! The FTEQCC preprocessor's directives and macros (`docs/spec/dialect-fteqcc.md`, FTEF-14).

use std::path::Path;

use super::{codes, one, one_fte, tokens};
use crate::{Code, MemoryLoader, Preprocessor};

/// The tokens of `text` preprocessed under the FTEQCC dialect, and its diagnostic codes.
fn fte(text: &str) -> (String, Vec<Code>) {
    let (_, out) = one_fte(text);
    (tokens(&out), codes(&out))
}

// spec FTEF-14.1: `#if` takes its side by its expression, as the project graph evaluates it,
// and an `#if` in a skipped side only nests.
#[test]
fn if_takes_a_side_by_its_expression() {
    let (toks, errs) = fte("#define A 2\n#if A == 2 && !defined(B)\nyes\n#else\nno\n#endif\n");
    assert_eq!((toks.as_str(), errs), ("yes", vec![]));
    let (toks, _) = fte("#if 0\n#if 1\ninner\n#endif\nouter\n#endif\nafter\n");
    assert_eq!(toks, "after");
    let (toks, _) = fte("#if defined(A) || 1\nyes\n#endif\n");
    assert_eq!(toks, "yes");
}

// spec FTEF-14.11: the preprocessor reads `#if` as FTEQCC's `ParsePrecompilerIf` does, `||` and
// `&&` at one level grouped to the right, from the bytes as written.
#[test]
fn if_follows_fteqcc_precedence() {
    let side = |cond: &str| fte(&format!("#if {cond}\nyes\n#else\nno\n#endif\n")).0;
    assert_eq!(side("1 || 0 && 0"), "yes");
    assert_eq!(side("0 && 0 || 1"), "no");
    assert_eq!(side("1 < 2 == 1"), "no");
    // `1||0` is one word, read with atoi.
    assert_eq!(side("0||1"), "no");
    assert_eq!(side("1 \\\n|| 0"), "yes");
}

// spec FTEF-14.1: the first true `#elif`, `#elifdef` or `#elifndef` is taken, or the `#else`,
// and only while no earlier side was.
#[test]
fn elif_chains_take_the_first_true_side() {
    let chain = |defines: &str| {
        let text = format!(
            "{defines}#if defined(A)\na\n#elif defined(B)\nb\n#elifdef C\nc\n#elifndef D\nd\n#else\ne\n#endif\n"
        );
        fte(&text).0
    };
    assert_eq!(chain("#define A\n#define B\n"), "a");
    assert_eq!(chain("#define B\n"), "b");
    assert_eq!(chain("#define C\n#define D\n"), "c");
    assert_eq!(chain(""), "d");
    assert_eq!(chain("#define D\n"), "e");
    // An `#elif` inside a skipped outer block takes nothing.
    let (toks, _) = fte("#if 0\n#if 0\n#elif 1\nx\n#endif\n#endif\ny\n");
    assert_eq!(toks, "y");
    // An `#elif` with no block is an error, as `#else` is.
    assert_eq!(fte("#elif 1\n").1, vec![Code::E0109]);
}

// spec FTEF-14.1: vanilla has no `#if` or `#elif`, and its `#else` flips the block.
#[test]
fn vanilla_has_no_if() {
    let (_, out) = one("#if 1\n#endif\n");
    assert_eq!(codes(&out), vec![Code::E0108, Code::E0109]);
    let (_, out) = one("#ifdef X\na\n#else\nb\n#else\nc\n#endif\n");
    assert_eq!(tokens(&out), "b");
}

// spec FTEF-14.2: a directive goes on past a break after `\`, or one inside a `/* */` comment,
// and a `//` comment ends it.
#[test]
fn directives_continue_after_a_backslash() {
    let (toks, errs) = fte("#define F(a) {\\\n\tg(a);\\\n}\nF(x)\n");
    assert_eq!((toks.as_str(), errs), ("{ g ( x ) ; }", vec![]));
    let (toks, _) = fte("#define G 1 /* a\ncomment */ 2\nG\n");
    assert_eq!(toks, "1 2");
    let (toks, _) = fte("#define H 1 // c \\\n2\nH\n");
    assert_eq!(toks, "2 1");
    let (toks, _) = fte("#define K 1\r\n2\nK\n");
    assert_eq!(toks, "2 1");
    let (toks, _) = fte("#define L 1 \\\r\n2\nL\n");
    assert_eq!(toks, "1 2");
    // Vanilla ends a directive at its line.
    let (_, out) = one("#define M 1\n2\nM\n");
    assert_eq!(tokens(&out), "2 1");
}

// spec QCD-7.20, QCD-8.11: `#warning` is W0103 and `#error` E0118, with FTEQCC's texts.
// spec FTEF-14.3: `#warning` and `#error` report their raw text on a taken side; `#message`
// reports nothing; a skipped side reports neither.
#[test]
fn warning_error_and_message() {
    let (_, out) = one_fte("#warning Toggle doesn't work\n#error stop 'here\n#message hi\nx\n");
    assert_eq!(codes(&out), vec![Code::W0103, Code::E0118]);
    assert_eq!(out.diagnostics[0].message, "#warning: Toggle doesn't work");
    assert_eq!(out.diagnostics[1].message, "#Error: stop 'here");
    assert_eq!(tokens(&out), "x");
    let (_, out) = one_fte("#if 0\n#warning no\n#error no\n#endif\n#warning\n");
    assert_eq!(codes(&out), vec![Code::W0103]);
    assert_eq!(out.diagnostics[0].message, "#warning: ");
}

// spec FTEF-14.4: an `#includelist` produces no tokens to its `#endlist`, whose lines are the
// graph's to compile; a stray `#endlist` is accepted; a list open at the end is closed there.
#[test]
fn includelists_produce_no_tokens() {
    let (toks, errs) = fte("a\n#includelist\nb.qc 'odd\n#ifdef X\nc.qc\n#endif\n#endlist\nd\n");
    assert_eq!((toks.as_str(), errs), ("a d", vec![]));
    assert_eq!(fte("#endlist\nx\n"), ("x".to_owned(), vec![]));
    assert_eq!(fte("#includelist\nb.qc\n"), (String::new(), vec![]));
}

// spec FTEF-14.5: `...` makes a macro variadic; `__VA_ARGS__` is the arguments after the named
// ones, commas and all, and may be empty.
#[test]
fn variadic_macros() {
    let (toks, errs) = fte("#define P(...) print(sprintf(__VA_ARGS__))\nP(\"%d\", a, (b, c))\n");
    assert_eq!(
        (toks.as_str(), errs),
        (r#"print ( sprintf ( "%d" , a , ( b , c ) ) )"#, vec![])
    );
    let (toks, errs) = fte("#define L(level, ...) log(level, __VA_ARGS__)\nL(1)\nL(2, x, y)\n");
    assert_eq!(
        (toks.as_str(), errs),
        ("log ( 1 , ) log ( 2 , x , y )", vec![])
    );
    // Fewer than the named parameters is the count error.
    assert_eq!(
        fte("#define L(level, kind, ...) x\nL(1)\n").1,
        vec![Code::E0116]
    );
    // A `...` that is not last is a bad parameter list; vanilla has no `...` parameter.
    assert_eq!(fte("#define B(..., a) x\n").1, vec![Code::E0115]);
    let (_, out) = one("#define V(...) x\n");
    assert_eq!(codes(&out), vec![Code::E0115]);
}

// spec FTEF-14.6: `#param` is the argument as a string and `a ## b` one token; a `#` before
// a word that is no parameter stays.
#[test]
fn stringify_and_paste() {
    let (toks, errs) = fte("#define S(a) #a\nS(info_null)\n");
    assert_eq!((toks.as_str(), errs), ("\"info_null\"", vec![]));
    let (toks, _) = fte("#define N(x) float x; float x ##_net;\nN(health)\n");
    assert_eq!(toks, "float health ; float health_net ;");
    let (toks, _) = fte("#define J(a, b) a ## b\nJ(1, 2)\n");
    assert_eq!(toks, "12");
    let (toks, _) = fte("#define J(a, b) a ## b\nJ(+, =)\n");
    assert_eq!(toks, "+=");
    let (toks, _) = fte("#define Q(a) #a\nQ(say \"hi\")\n");
    assert_eq!(toks, r#""say \"hi\"""#);
    let (toks, _) = fte("#define K #notparam\nK\n");
    assert_eq!(toks, "# notparam");
    // A `##` with nothing after it joins nothing.
    let (toks, _) = fte("#define E(a) a ##\nE(x)\n");
    assert_eq!(toks, "x");
}

// spec FTEF-3.4: `__NULL__` is `0i`.
// spec FTEF-14.7: the built-in constants expand when no macro has the name; a macro of the
// same name wins; vanilla has none.
#[test]
fn builtin_constants() {
    let (_, out) = one_fte("__NULL__ __FILE__ __LINE__\n__LINESTR__ __FUNC__ __func__\n");
    assert_eq!(tokens(&out), r#"0 "a.qc" 1 "2" "" """#);
    let (toks, _) = fte("__TIME__ __DATE__ __QCCVER__ __RAND__ __OTHER__\n");
    assert_eq!(toks, r#""" "" "" 0 __OTHER__"#);
    let (toks, _) = fte("#define __NULL__ none\n__NULL__\n");
    assert_eq!(toks, "none");
    let (_, out) = one("__NULL__\n");
    assert_eq!(tokens(&out), "__NULL__");
    // The spelling of a built-in is its text.
    let (_, out) = one_fte("__NULL__\n");
    assert_eq!(out.tokens[0].spelling.as_deref(), Some("0i"));
}

// spec FTEF-14.8: `#append` adds to a body, or defines; `#eof` ends the file; the build
// directives are accepted.
#[test]
fn append_eof_and_build_directives() {
    let (toks, _) = fte("#define A 1\n#append A 2\nA\n");
    assert_eq!(toks, "1 2");
    let (toks, _) = fte("#append B 3\nB\n");
    assert_eq!(toks, "3");
    let (toks, errs) = fte("x\n#eof\ny 'bad\n");
    assert_eq!((toks.as_str(), errs), ("x", vec![]));
    let directives = "#output \"p.dat\"\n#forcecrc 1\n#copyright \"c\"\n#pack 1\n#package x\n#merge \"m\"\n#datafile \"d\"\nz\n";
    assert_eq!(fte(directives), ("z".to_owned(), vec![]));
    let (_, out) = one("#output \"p.dat\"\n");
    assert_eq!(codes(&out), vec![Code::E0108]);
}

// spec FTEF-14.9: one argument more than the parameters is accepted and unused; two more is
// the count error; vanilla accepts neither.
#[test]
fn one_extra_macro_argument_is_accepted() {
    let (toks, errs) = fte("#define F(a, b) a b\nF(1, 2, 3)\n");
    assert_eq!((toks.as_str(), errs), ("1 2", vec![]));
    assert_eq!(
        fte("#define F(a, b) a b\nF(1, 2, 3, 4)\n").1,
        vec![Code::E0116]
    );
    assert_eq!(fte("#define F() k\nF(1)\n").0, "k");
    let (_, out) = one("#define F(a, b) a b\nF(1, 2, 3)\n");
    assert_eq!(codes(&out), vec![Code::E0116]);
}

// spec FTEF-14.4: an included file's list is skipped too, and its lexical diagnostics are not
// reported.
#[test]
fn an_included_list_is_skipped() {
    let mut loader = MemoryLoader::new();
    loader.insert("a.qc", "#include \"l.src\"\nafter\n".as_bytes());
    loader.insert("l.src", "#includelist\nx.qc 'y\n".as_bytes());
    let mut pp = Preprocessor::new(loader).with_dialect(qc_lexer::Dialect::Fteqcc);
    let out = pp.preprocess_file(Path::new("a.qc")).unwrap();
    assert_eq!((tokens(&out), codes(&out)), ("after".to_owned(), vec![]));
}

// spec FTEF-14.10: a block left open on a taken side is closed at the end of the file without
// a word; one left open while a side is skipped is reported; vanilla reports both.
#[test]
fn open_blocks_at_the_end_of_a_file() {
    assert_eq!(fte("#ifndef NOPE\nx\n"), ("x".to_owned(), vec![]));
    assert_eq!(fte("#ifdef NOPE\nx\n").1, vec![Code::E0110]);
    assert_eq!(fte("#if 1\n#else\nx\n").1, vec![Code::E0110]);
    let (_, out) = one("#ifndef NOPE\nx\n");
    assert_eq!(codes(&out), vec![Code::E0110]);
}

// spec FTEF-12.6: `#pragma noref N` is recorded at the token it precedes, so the definitions
// after it are `noref` or not.
#[test]
fn noref_pragmas_are_recorded() {
    let (_, out) = one_fte("a\n#pragma noref 1\nb c\n#pragma noref 0\nd\n#pragma noref\n");
    assert_eq!(out.noref, vec![(1, true), (3, false)]);
    let mut loader = MemoryLoader::new();
    loader.insert("a.qc", "#pragma noref 1\n".as_bytes());
    let mut pp = Preprocessor::new(loader);
    let vanilla = pp.preprocess_file(Path::new("a.qc")).unwrap();
    assert!(vanilla.noref.is_empty());
}

// spec FTEF-12.9: each `#pragma autoproto [N]` is recorded with whether it asks for autoprototyping, its name in any case
#[test]
fn autoproto_pragmas_are_recorded() {
    let (_, out) = one_fte("#pragma autoproto\na\n#pragma AutoProto 0\n#pragma autoproto 2\n");
    assert_eq!(out.autoproto, vec![(0, true), (1, false), (1, true)]);
    assert!(out.noref.is_empty());
    let (_, out) = one_fte("#PRAGMA noref 1\n#pragma NOREF 1\nx");
    assert_eq!(out.noref, vec![(0, true)]);
    let mut loader = MemoryLoader::new();
    loader.insert("a.qc", "#pragma autoproto\n".as_bytes());
    let mut pp = Preprocessor::new(loader);
    let vanilla = pp.preprocess_file(Path::new("a.qc")).unwrap();
    assert!(vanilla.autoproto.is_empty());
}

// spec FTEF-18.16: `$flush`, `$frame_reset`, `$framevalue`, `$framerestore` and `$modelname` set the number of the next frame name
#[test]
fn frame_commands_number_frames() {
    let frames = |text: &str| fte(text);
    // `$flush` numbers from 0 again; the earlier names keep their numbers.
    assert_eq!(
        frames("$frame a b\n$flush\n$frame c\nx = $a + $b + $c;"),
        ("x = 0 + 1 + 0 ;".to_owned(), vec![])
    );
    // `$frame_reset` also forgets them.
    assert_eq!(
        frames("$frame a\n$frame_reset\n$frame b\nx = $b; y = $a;"),
        ("x = 0 ; y = $a ;".to_owned(), vec![Code::E0106])
    );
    // `$framevalue N` numbers from N, an empty word from 0.
    assert_eq!(
        frames("$framevalue 10\n$frame a b\nx = $b;\n$framevalue\n$frame c\ny = $c;"),
        ("x = 11 ; y = 0 ;".to_owned(), vec![])
    );
    // `$framerestore NAME` numbers from NAME's number; an unknown NAME is QCD-2.6.
    assert_eq!(
        frames("$frame a b c\n$framerestore b\n$frame d\nx = $d;"),
        ("x = 1 ;".to_owned(), vec![])
    );
    assert_eq!(
        frames("$framerestore nothing\n$frame d\nx = $d;"),
        ("x = 0 ;".to_owned(), vec![Code::E0106])
    );
    // `$modelname NAME`: a name with no macro numbers from -1, the earlier model name becomes
    // a macro of the number reached, and a model name that is a macro numbers from it.
    assert_eq!(
        frames("$modelname m1\n$frame a b\n$modelname m2\nx = $a + $m1;"),
        ("x = -1 + 1 ;".to_owned(), vec![])
    );
    assert_eq!(
        frames("$frame m2 q r\n$modelname m2\n$frame s\nx = $s;"),
        ("x = 3 ;".to_owned(), vec![])
    );
}

// spec FTEF-18.17: each file numbers its frames from 0, and the limit is 8192 frame macros in the program, not 256 in a file
#[test]
fn frame_macros_limit_is_the_program() {
    let names: Vec<String> = (0..300).map(|i| format!("f{i}")).collect();
    let text = format!("$frame {}\nx = $f299;", names.join(" "));
    assert_eq!(fte(&text), ("x = 299 ;".to_owned(), vec![]));
    let files = [("a.qc", "$frame a b\n"), ("b.qc", "$frame c\nx = $c + $b;")];
    let (_, outputs) = super::fte_project(&files, &["a.qc", "b.qc"], false);
    assert_eq!(tokens(&outputs[1]), "x = 0 + 1 ;");
    let all: Vec<String> = (0..crate::FTE_MAX_FRAMES)
        .map(|i| format!("g{i}"))
        .collect();
    let fits = format!("$frame {}\n", all[..crate::FTE_MAX_FRAMES - 1].join(" "));
    assert_eq!(fte(&fits).1, vec![]);
    let full = format!("$frame {}\n", all.join(" "));
    assert_eq!(fte(&full).1, vec![Code::E0504]);
}

// spec FTEF-12.2, FTEF-12.3, FTEF-12.4, FTEF-12.5, FTEF-12.7: FTEQCC's other pragmas are
// accepted and produce no tokens.
#[test]
fn other_pragmas_are_accepted() {
    let pragmas = "#pragma TARGET FTE\n#pragma PROGS_DAT \"../progs.dat\"\n\
                   #pragma progs_dat \"x\"\n#pragma sourcefile client/progs.src\n\
                   #pragma flag enable assumeint\n#pragma forcecrc 54730\n#pragma once\n\
                   #pragma optimise 2\n#pragma unknown thing\nz\n";
    assert_eq!(fte(pragmas), ("z".to_owned(), vec![]));
}
