// SPDX-License-Identifier: GPL-3.0-or-later

//! The FTEQCC preprocessor's directives and macros (`docs/spec/dialect-fteqcc.md`, FTEF-14).

use std::path::Path;

use super::{codes, one, one_fte, tokens};
use crate::fte_directive::join_paths;
use crate::{Code, CompilerFlags, Dialect, MemoryLoader, PragmaDefault, Preprocessor, TokenKind};

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

// spec FTEF-14.33: the preprocessor reads `#if` as FTEQCC's `ParsePrecompilerIf` does, `||` and
// `&&` at one level grouped to the right, from the bytes as written, within the line: a `\`
// ends the expression, and the rest of the line is junk
#[test]
fn if_follows_fteqcc_precedence() {
    let side = |cond: &str| fte(&format!("#if {cond}\nyes\n#else\nno\n#endif\n"));
    assert_eq!(side("1 || 0 && 0").0, "yes");
    assert_eq!(side("0 && 0 || 1").0, "no");
    assert_eq!(side("1 < 2 == 1").0, "no");
    // `1||0` is one word, read with atoi.
    assert_eq!(side("0||1").0, "no");
    assert_eq!(side("1 \\\n|| 0"), ("yes".to_owned(), vec![Code::E0121]));
    assert_eq!(side("0 \\\n|| 1"), ("no".to_owned(), vec![Code::E0121]));
    // The errors FTEQCC reports are reported, and the expression is false.
    for cond in ["(1", "defined(A", ""] {
        assert_eq!(side(cond), ("no".to_owned(), vec![Code::E0121]), "{cond:?}");
    }
    assert_eq!(side("1 2"), ("yes".to_owned(), vec![Code::E0121]));
    let (_, out) = one_fte("#if (1\n#endif\n");
    assert_eq!(out.diagnostics[0].message, "unclosed bracket condition");
    // One nested past the limit is only false.
    let deep = format!("{}1{}", "(".repeat(70), ")".repeat(70));
    assert_eq!(side(&deep), ("no".to_owned(), vec![]));
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

// spec FTEF-14.35: a directive goes on past a break after `\`, or one inside a `/* */`
// comment, and a `//` comment ends its text
// spec FTEF-14.24: and the line a `#define` ends on goes on past a `\` and line break in that
// `//` comment, as `QCC_PR_SkipToEndOfLine` reads it, so the next line is not read
#[test]
fn directives_continue_after_a_backslash() {
    let (toks, errs) = fte("#define F(a) {\\\n\tg(a);\\\n}\nF(x)\n");
    assert_eq!((toks.as_str(), errs), ("{ g ( x ) ; }", vec![]));
    let (toks, _) = fte("#define G 1 /* a\ncomment */ 2\nG\n");
    assert_eq!(toks, "1 2");
    let (toks, _) = fte("#define H 1 // c \\\n2\nH\n");
    assert_eq!(toks, "1");
    let (toks, _) = fte("#define H 1 // c\n2\nH\n");
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
// spec FTEF-14.38: their text is the bytes right after the name, its space included, to the line
// break, `#error`'s to a tab too and `#warning`'s to a CR; `#message` reports nothing; a skipped
// side reports neither
#[test]
fn warning_error_and_message() {
    let (_, out) = one_fte("#warning Toggle doesn't work\n#error stop 'here\n#message hi\nx\n");
    assert_eq!(codes(&out), vec![Code::W0103, Code::E0118]);
    assert_eq!(out.diagnostics[0].message, "#warning:  Toggle doesn't work");
    assert_eq!(out.diagnostics[1].message, "#Error:  stop 'here");
    assert_eq!(tokens(&out), "x");
    let (_, out) = one_fte("#if 0\n#warning no\n#error no\n#endif\n#warning\n");
    assert_eq!(codes(&out), vec![Code::W0103]);
    assert_eq!(out.diagnostics[0].message, "#warning: ");
    let (_, out) = one_fte("#error a\tb\n#warning c\td\r\n#warningx\n#errorx\n");
    let messages: Vec<&str> = out.diagnostics.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(
        messages,
        ["#Error:  a", "#warning:  c\td", "#warning: x", "#Error: x"]
    );
    // At most 1023 bytes of the text are kept.
    let (_, out) = one_fte(&format!("#warning{}\n", "w".repeat(2000)));
    assert_eq!(out.diagnostics[0].message.len(), "#warning: ".len() + 1023);
}

// spec FTEF-14.41: an `#includelist` produces no tokens to its `#endlist`, whose words are the
// graph's to compile, and runs the directives at its lines' starts; `#endlist` outside a list is
// no directive; a list open at the end is E0406
#[test]
fn includelists_produce_no_tokens() {
    let (toks, errs) = fte("a\n#includelist\nb.qc 'odd\n#ifdef X\nc.qc\n#endif\n#endlist\nd\n");
    assert_eq!((toks.as_str(), errs), ("a d", vec![]));
    let (toks, errs) = fte("#includelist\n#define N 5\nb.qc, (c.qc)\n#endlist\nN\n");
    assert_eq!((toks.as_str(), errs), ("5", vec![]));
    // The line break after a word is white space, so the `#` after it starts a directive.
    let (toks, errs) = fte("#includelist\nb.qc\n#define N 5\n#endlist\nN\n");
    assert_eq!((toks.as_str(), errs), ("5", vec![]));
    // The words on the line are junk; the list starts after it.
    assert_eq!(
        fte("#includelist b.qc #endlist\nx\n"),
        (String::new(), vec![Code::E0121, Code::E0406])
    );
    assert_eq!(
        fte("#includelist\nb.qc #endlist // c\nx\n"),
        ("x".to_owned(), vec![])
    );
    assert_eq!(
        fte("#includelist\nb.qc #endlist x\ny\n"),
        ("y".to_owned(), vec![Code::E0121])
    );
    assert_eq!(fte("#endlist\nx\n"), ("# endlist x".to_owned(), vec![]));
    assert_eq!(
        fte("#includelist\nb.qc\n"),
        (String::new(), vec![Code::E0406])
    );
    assert_eq!(
        fte("#includelist\n#eof\nb.qc\n"),
        (String::new(), vec![Code::E0406])
    );
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

// spec FTEF-14.14, FTEF-14.31: `#param` is the argument as a string and `a ## b` one token;
// a `#` before a word that is no parameter stays.
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

// spec FTEF-14.39: `#append` adds to a body, or defines; `#eof` and `#package` end the text; the
// build directives read their lines as FTEQCC does, `#forcecrc` and `#pack` the byte after
// their name whatever it is
#[test]
fn append_eof_and_build_directives() {
    let (toks, _) = fte("#define A 1\n#append A 2\nA\n");
    assert_eq!(toks, "1 2");
    let (toks, _) = fte("#append B 3\nB\n");
    assert_eq!(toks, "3");
    let (toks, errs) = fte("x\n#eof\ny 'bad\n");
    assert_eq!((toks.as_str(), errs), ("x", vec![]));
    let (toks, errs) = fte("x\n#eof zz\ny\n");
    assert_eq!((toks.as_str(), errs), ("x", vec![]));
    let directives = "#output \"p.dat\"\n#forcecrc 1\n#copyright \"c\"\n#pack 1 \n#merge \"m\"\n#datafile \"d\" x\n#message m\nz\n";
    assert_eq!(fte(directives), ("z".to_owned(), vec![]));
    assert_eq!(fte("x\n#package\ny\n"), ("x".to_owned(), vec![]));
    assert_eq!(
        fte("x\n#package y\nz\n"),
        ("x".to_owned(), vec![Code::E0121])
    );
    // `#forcecrc` and `#pack` at a line's end take its break and read the next line as theirs.
    assert_eq!(
        fte("#forcecrc\nzz zz\ny\n"),
        ("y".to_owned(), vec![Code::E0121])
    );
    assert_eq!(fte("#forcecrc\n5\ny\n"), ("y".to_owned(), vec![]));
    // Its number takes a `.` but one before another `.` (`c == '.'&&pr_file_p[1]!='.'`), so
    // `1.5` is all number and `1..` leaves `..` on the line.
    assert_eq!(fte("#forcecrc 1.5\ny\n"), ("y".to_owned(), vec![]));
    assert_eq!(
        fte("#forcecrc 1..\ny\n"),
        ("y".to_owned(), vec![Code::E0121])
    );
    // A `0x` where the number starts is taken whole, and only there: `#forcecrc0x1f` is all
    // number, and the `x` of `#forcecrc1x5` ends it.
    assert_eq!(fte("#forcecrc0x1f\ny\n"), ("y".to_owned(), vec![]));
    assert_eq!(
        fte("#forcecrc1x5\ny\n"),
        ("y".to_owned(), vec![Code::E0121])
    );
    assert_eq!(fte("#pack\nzz\ny\n"), ("y".to_owned(), vec![Code::E0121]));
    assert_eq!(fte("#packid\ny\n"), (String::new(), vec![Code::E0121]));
    assert_eq!(fte("#pack id\ny\n"), ("y".to_owned(), vec![Code::E0121]));
    assert_eq!(fte("#pack 6 \ny\n"), ("y".to_owned(), vec![Code::E0121]));
    assert_eq!(fte("#pack 5 \ny\n"), ("y".to_owned(), vec![]));
    assert_eq!(fte("#output zz\ny\n"), ("y".to_owned(), vec![Code::E0121]));
    assert_eq!(fte("#datafile\ny\nz\n"), ("z".to_owned(), vec![]));
    assert_eq!(
        fte("#datafile \"d\" /* \n */ y\n"),
        ("* / y".to_owned(), vec![])
    );
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

// spec FTEF-14.41: an included file's list is skipped too, and its lexical diagnostics are not
// reported; one the file leaves open is E0406
#[test]
fn an_included_list_is_skipped() {
    let mut loader = MemoryLoader::new();
    loader.insert("a.qc", "#include \"l.src\"\nafter\n".as_bytes());
    loader.insert("l.src", "#includelist\nx.qc 'y\n#endlist\n".as_bytes());
    loader.insert("b.qc", "#include \"m.src\"\nafter\n".as_bytes());
    loader.insert("m.src", "#includelist\nx.qc 'y\n".as_bytes());
    let mut pp = Preprocessor::new(loader).with_dialect(qc_lexer::Dialect::Fteqcc);
    let out = pp.preprocess_file(Path::new("a.qc")).unwrap();
    assert_eq!((tokens(&out), codes(&out)), ("after".to_owned(), vec![]));
    let out = pp.preprocess_file(Path::new("b.qc")).unwrap();
    assert_eq!(
        (tokens(&out), codes(&out)),
        ("after".to_owned(), vec![Code::E0406])
    );
}

// spec FTEF-14.37: a block left open on a taken side is not reported, and an `#endif` in a file
// read after it closes it; one left open while a side is skipped is E0110; vanilla reports both
#[test]
fn open_blocks_at_the_end_of_a_file() {
    assert_eq!(fte("#ifndef NOPE\nx\n"), ("x".to_owned(), vec![]));
    assert_eq!(fte("#ifdef NOPE\nx\n").1, vec![Code::E0110]);
    assert_eq!(fte("#if 1\n#else\nx\n").1, vec![Code::E0110]);
    let (_, out) = one("#ifndef NOPE\nx\n");
    assert_eq!(codes(&out), vec![Code::E0110]);
    let mut loader = MemoryLoader::new();
    loader.insert("a.qc", "#if 1\na\n".as_bytes());
    loader.insert("b.qc", "#endif\nb\n#endif\n".as_bytes());
    let mut pp = Preprocessor::new(loader).with_dialect(qc_lexer::Dialect::Fteqcc);
    let a = pp.preprocess_file(Path::new("a.qc")).unwrap();
    let b = pp.preprocess_file(Path::new("b.qc")).unwrap();
    assert_eq!((tokens(&a), codes(&a)), ("a".to_owned(), vec![]));
    assert_eq!((tokens(&b), codes(&b)), ("b".to_owned(), vec![Code::E0109]));
}

/// Each mark of `out` that sets the pragma default `default`, with the token it holds from and
/// what it sets.
fn default_marks(out: &crate::FileOutput, default: PragmaDefault) -> Vec<(usize, bool)> {
    out.flags
        .iter()
        .skip(1)
        .filter_map(|&(at, edit)| {
            let on = edit.apply(CompilerFlags::default()).default_on(default);
            let off = edit
                .apply(CompilerFlags::default().with_default(default, true))
                .default_on(default);
            (on == off).then_some((at, on))
        })
        .collect()
}

// spec FTEF-12.6: `#pragma noref N` is recorded at the token it precedes, so the definitions
// after it are `noref` or not
// spec FTEF-14.70: its text is read with `atoi`, so one with no number, or a `(`, is 0
// spec FTEF-21.14: `#pragma nosave N` and `#pragma defaultstatic N` are recorded alike
#[test]
fn noref_pragmas_are_recorded() {
    let (_, out) = one_fte(
        "a\n#pragma noref 1\nb c\n#pragma noref 0\nd\n#pragma noref\ne\n#pragma noref(1)\n",
    );
    assert_eq!(
        default_marks(&out, PragmaDefault::Noref),
        vec![(1, true), (3, false), (4, false), (5, false)]
    );
    let (_, out) = one_fte("#pragma nosave 1\na\n#pragma defaultstatic 1\nb\n");
    assert_eq!(default_marks(&out, PragmaDefault::Nosave), vec![(0, true)]);
    assert_eq!(default_marks(&out, PragmaDefault::Static), vec![(1, true)]);
    assert_eq!(default_marks(&out, PragmaDefault::Noref), vec![]);
    let mut loader = MemoryLoader::new();
    loader.insert("a.qc", "#pragma noref 1\n".as_bytes());
    let mut pp = Preprocessor::new(loader);
    let vanilla = pp.preprocess_file(Path::new("a.qc")).unwrap();
    assert!(vanilla.flags.is_empty());
}

// spec FTEF-21.14: the defaults hold from one file to the next, and the second pass starts with
// `defaultstatic` off and `noref` and `nosave` as they were
#[test]
fn pragma_defaults_carry_over() {
    let mut loader = MemoryLoader::new();
    loader.insert(
        "a.qc",
        "#pragma noref 1\n#pragma nosave 1\n#pragma defaultstatic 1\n".as_bytes(),
    );
    loader.insert("b.qc", "x\n".as_bytes());
    let mut pp = Preprocessor::new(loader).with_dialect(Dialect::Fteqcc);
    pp.preprocess_file(Path::new("a.qc")).unwrap();
    let b = pp.preprocess_file(Path::new("b.qc")).unwrap();
    let first = b.flags[0].1.apply(CompilerFlags::default());
    for default in [
        PragmaDefault::Static,
        PragmaDefault::Noref,
        PragmaDefault::Nosave,
    ] {
        assert!(first.default_on(default), "{default:?}");
    }
    pp.restart();
    let again = pp.preprocess_file(Path::new("b.qc")).unwrap();
    let first = again.flags[0].1.apply(CompilerFlags::default());
    assert!(!first.default_on(PragmaDefault::Static));
    assert!(first.default_on(PragmaDefault::Noref));
    assert!(first.default_on(PragmaDefault::Nosave));
}

// spec FTEF-12.10: each `#pragma autoproto [N]` is recorded with whether it asks for autoprototyping, its name in any case
#[test]
fn autoproto_pragmas_are_recorded() {
    let (_, out) = one_fte("#pragma autoproto\na\n#pragma AutoProto 0\n#pragma autoproto 2\n");
    let pragmas: Vec<(usize, bool)> = out.autoproto.iter().map(|&(i, on, _)| (i, on)).collect();
    assert_eq!(pragmas, vec![(0, true), (1, false), (1, true)]);
    let starts: Vec<usize> = out.autoproto.iter().map(|p| p.2.span.start).collect();
    assert_eq!(starts, vec![0, 20, 40]);
    assert!(default_marks(&out, PragmaDefault::Noref).is_empty());
    // A directive's name is matched as written, so `#PRAGMA` is none, and its words are code.
    let (_, out) = one_fte("#PRAGMA noref 1\n#pragma NOREF 1\nx");
    assert_eq!(default_marks(&out, PragmaDefault::Noref), vec![(4, true)]);
    assert_eq!(codes(&out), vec![]);
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
        frames("$modelname m1\n$frame a b c\n$modelname m2\nx = $b + $m1;"),
        ("x = 0 + 2 ;".to_owned(), vec![])
    );
    assert_eq!(
        frames("$frame m2 q r\n$modelname m2\n$frame s\nx = $s;"),
        ("x = 3 ;".to_owned(), vec![])
    );
}

// spec FTEF-18.21: a frame macro not found by its name is found in any case, the last defined, with a warning
// spec QCD-7.24: W0104 "Case insensitive frame macro (using NAME)"
#[test]
fn frame_macros_fall_back_to_any_case() {
    let (_, out) = one_fte("$frame Walk1 walk2 WALK2\nx = $walk1 + $Walk2 + $walk2;");
    assert_eq!(tokens(&out), "x = 0 + 2 + 1 ;");
    assert_eq!(codes(&out), [Code::W0104, Code::W0104]);
    assert_eq!(
        out.diagnostics[1].message,
        "Case insensitive frame macro (using WALK2)"
    );
    assert!(!Code::W0104.is_error());
    let (_, out) = one_fte("$frame a b\n$framerestore B\n$frame c\nx = $c;");
    assert_eq!(
        (tokens(&out), codes(&out)),
        ("x = 1 ;".to_owned(), vec![Code::W0104])
    );
    // Vanilla matches the name exactly (QC-8.5).
    let (_, out) = one("$frame Walk1\nx = $walk1;");
    assert_eq!(codes(&out), [Code::E0106]);
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

// spec FTEF-12.2, FTEF-12.3, FTEF-12.5, FTEF-12.7: FTEQCC's other pragmas are accepted and
// produce no tokens
// spec FTEF-12.11: an unknown one is the Q120 warning "Unknown pragma 'NAME'"
#[test]
fn other_pragmas_are_accepted() {
    let pragmas = "#pragma TARGET FTE\n#pragma PROGS_DAT \"../progs.dat\"\n\
                   #pragma progs_dat \"x\"\n#pragma sourcefile client/progs.src\n\
                   #pragma flag enable assumeint\n#pragma forcecrc 54730\n#pragma once\n\
                   #pragma optimise 2\n#pragma unknown thing\nz\n";
    assert_eq!(fte(pragmas), ("z".to_owned(), vec![Code::W0113]));
    let (_, out) = one_fte("#pragma unknown thing\n");
    assert_eq!(out.diagnostics[0].message, "Unknown pragma 'unknown'");
}

// spec FTEF-18.29: a frame macro another reading of a file defined (an earlier file, an
// `#include`d one, the file that includes this one, or a macro's expansion) is "Stale macro
// used"; one this reading defined is not.
// spec QCD-7.25: W0105 "Stale macro used (NAME, defined in FILE)"
#[test]
fn a_frame_macro_from_another_reading_is_stale() {
    // Defined in this file, used in this file: not stale.
    assert_eq!(fte("$frame a\nx = $a;"), ("x = 0 ;".to_owned(), vec![]));
    let files = [
        ("a.qc", "$frame a\n#include \"b.qh\"\nx = $a + $b;"),
        ("b.qh", "$frame b\ny = $a + $b;\n"),
        ("c.qc", "#define F $a\nz = $a; w = F;"),
    ];
    let (pp, outputs) = super::fte_project(&files, &["a.qc", "c.qc"], false);
    // The include goes on numbering from where the file that includes it is.
    assert_eq!(tokens(&outputs[0]), "y = 0 + 1 ; x = 0 + 1 ;");
    assert_eq!(
        super::diagnostics(&pp, &outputs[0]),
        "W0105 Stale macro used (a, defined in a.qc) @b.qh:2\n\
         W0105 Stale macro used (b, defined in b.qh) @a.qc:3\n"
    );
    assert_eq!(
        super::diagnostics(&pp, &outputs[1]),
        "W0105 Stale macro used (a, defined in a.qc) @c.qc:2\n\
         W0105 Stale macro used (a, defined in a.qc) @c.qc:1\n"
    );
    assert!(!Code::W0105.is_error());
    // A use through `$framerestore` and the case-insensitive lookup is checked too.
    let files = [("a.qc", "$frame Walk\n"), ("b.qc", "$framerestore walk\n")];
    let (_, outputs) = super::fte_project(&files, &["a.qc", "b.qc"], false);
    assert_eq!(codes(&outputs[1]), [Code::W0104, Code::W0105]);
}

// spec FTEF-18.29: a frame name the same reading of a file defined before is "Duplicate macro
// defined" and keeps its first number; another reading's definition is replaced silently, and
// so is the model name `$modelname` defines.
// spec QCD-7.26: W0106 "Duplicate macro defined (NAME). Rename it."
#[test]
fn a_frame_name_defined_twice_in_one_reading_is_a_duplicate() {
    let (_, out) = one_fte("$frame a b a\nx = $a;");
    assert_eq!(tokens(&out), "x = 0 ;");
    assert_eq!(codes(&out), [Code::W0106]);
    assert_eq!(
        out.diagnostics[0].message,
        "Duplicate macro defined (a). Rename it."
    );
    assert!(!Code::W0106.is_error());
    let files = [("a.qc", "$frame a\n"), ("b.qc", "$frame b a\nx = $a;")];
    let (_, outputs) = super::fte_project(&files, &["a.qc", "b.qc"], false);
    assert_eq!(
        (tokens(&outputs[1]), codes(&outputs[1])),
        ("x = 1 ;".to_owned(), vec![])
    );
    assert_eq!(
        fte("$frame m\n$modelname m\n$modelname n\nx = $m;"),
        ("x = 1 ;".to_owned(), vec![])
    );
}

// spec FTEF-18.29: a new frame name of 64 bytes or more is "Name for frame macro NAME is too
// long", an error, and is not defined; 63 bytes are kept.
// spec QCD-2.8: E0119 "Name for frame macro NAME is too long"
#[test]
fn a_frame_name_of_64_bytes_is_too_long() {
    let fits = "f".repeat(63);
    assert_eq!(
        fte(&format!("$frame {fits}\nx = ${fits};")),
        ("x = 0 ;".to_owned(), vec![Code::W0101])
    );
    let long = "g".repeat(64);
    let (_, out) = one_fte(&format!("$frame {long} h\nx = $h;"));
    assert_eq!(tokens(&out), "x = 1 ;");
    // W0101 is the lexer's note that id's qcc could not look the name up (QC-27.6).
    assert_eq!(codes(&out), [Code::E0119, Code::W0101]);
    assert_eq!(
        out.diagnostics[0].message,
        format!("Name for frame macro {long} is too long")
    );
    assert!(Code::E0119.is_error());
    // Never defined, so a use of it is unknown.
    let (_, out) = one_fte(&format!("$frame {long}\nx = ${long};"));
    let errors: Vec<Code> = codes(&out).into_iter().filter(|c| c.is_error()).collect();
    assert_eq!(errors, [Code::E0119, Code::E0106]);
}

// spec FTEF-18.29: a `$modelname` of 64 bytes or more is cut to 63 with the warning
// "$modelname name too long"; one of 63 is kept whole.
// spec QCD-7.27: W0107 "$modelname name too long"
#[test]
fn a_long_model_name_is_cut() {
    let fits = "m".repeat(63);
    let text = format!("$modelname {fits}\n$frame a\n$modelname next\nx = ${fits};");
    assert_eq!(fte(&text), ("x = 0 ;".to_owned(), vec![]));
    let cut = "n".repeat(63);
    let text = format!("$modelname {cut}x\n$frame a\n$modelname next\ny = ${cut};");
    let (_, out) = one_fte(&text);
    assert_eq!(tokens(&out), "y = 0 ;");
    assert_eq!(codes(&out), [Code::W0107]);
    assert!(!Code::W0107.is_error());
}

// spec FTEF-14.58: a redefinition with the same body is F316's "Identical precompiler
// definition", ignored unless a pragma says otherwise, whatever its parameters, variadic or not
// spec FTEF-14.5: a variadic list is a parameter list as any other there
#[test]
fn a_redefinition_with_other_parameters_warns() {
    assert_eq!(fte("#define A 1\n#define A() 1\n").1, vec![]);
    assert_eq!(fte("#define V(a, ...) x\n#define V(a) x\n").1, vec![]);
    assert_eq!(
        fte("#define V(a, ...) x\n#define V(a, ...) y\n").1,
        vec![Code::W0115]
    );
    let (_, out) = one_fte("#pragma warning enable F316\n#define V(a, ...) x\n#define V(a) x\n");
    assert_eq!(codes(&out), vec![Code::W0116]);
    assert_eq!(
        out.diagnostics[0].message,
        "Identical precompiler definition of V"
    );
}

// spec FTEF-14.14: a pasted text that lexes as one token is that token, a number or an
// operator; one that lexes as more keeps its tokens, the second written against the first
#[test]
fn a_paste_is_lexed_again() {
    use qc_lexer::Punct;
    let (_, out) = one_fte("#define J(a, b) a ## b\nJ(1, 2) J(+, =) J(a, +)\n");
    let kinds: Vec<TokenKind> = out.tokens.iter().map(|t| t.kind.clone()).collect();
    assert_eq!(
        kinds,
        [
            TokenKind::Float(12.0),
            TokenKind::Punct(Punct::PlusEq),
            TokenKind::Name("a".to_owned()),
            TokenKind::Punct(Punct::Plus),
        ]
    );
    let plus = out.tokens.last().unwrap();
    assert!(plus.touches(&out.tokens[2]));
    assert_eq!(out.tokens[0].spelling.as_deref(), Some("12"));
}

// spec FTEF-14.31: `#param` is a string literal whose spelling escapes `"` and `\`
#[test]
fn a_stringified_argument_is_escaped() {
    let (_, out) = one_fte("#define Q(a) #a\nQ(say \"a\\\\b\")\n");
    assert_eq!(
        out.tokens[0].kind,
        TokenKind::String(br#"say "a\\b""#.to_vec())
    );
    assert_eq!(
        out.tokens[0].spelling.as_deref(),
        Some(r#""say \"a\\\\b\"""#)
    );
}

/// The first token of `text` as the file `path`, preprocessed under FTEQCC.
fn first_token_in(path: &str, text: &str) -> crate::PpToken {
    let mut loader = MemoryLoader::new();
    loader.insert(path, text.as_bytes());
    let mut pp = Preprocessor::new(loader).with_dialect(qc_lexer::Dialect::Fteqcc);
    let out = pp.preprocess_file(Path::new(path)).unwrap();
    out.tokens[0].clone()
}

/// The tokens and codes of `text` in the file `path` under FTEQCC.
fn fte_in(path: &str, text: &str) -> (Vec<TokenKind>, Vec<Code>) {
    let mut loader = MemoryLoader::new();
    loader.insert(path, text.as_bytes());
    let mut pp = Preprocessor::new(loader).with_dialect(qc_lexer::Dialect::Fteqcc);
    let out = pp.preprocess_file(Path::new(path)).unwrap();
    let kinds = out.tokens.iter().map(|t| t.kind.clone()).collect();
    (kinds, codes(&out))
}

// spec FTEF-14.74: `__FILE__` is `"` and the file's name as given, kept to 255 bytes, ended at
// its first `:`, and `"`, read as a string literal with nothing in the name escaped
#[test]
fn file_constant_is_fteqcc_s_text() {
    let string = |bytes: &[u8]| TokenKind::String(bytes.to_vec());
    assert_eq!(
        fte_in("a.qc", "__FILE__\n"),
        (vec![string(b"a.qc")], vec![])
    );
    assert_eq!(fte_in("c:a.qc", "__FILE__\n"), (vec![string(b"c")], vec![]));
    let bell =
        qc_lexer::fteqcc_string_bytes(b"\"a\\b.qc\"", qc_lexer::LexFlags::default()).unwrap();
    assert_eq!(
        fte_in("a\\b.qc", "__FILE__\n"),
        (vec![string(&bell)], vec![])
    );
    // The line break ends the string, and the rest of the name opens another.
    assert_eq!(
        fte_in("a\nb.qc", "__FILE__\n").1,
        vec![Code::E0102, Code::E0101]
    );
    let (kinds, errors) = fte_in("a\"b.qc", "__FILE__\n");
    assert_eq!(kinds.first(), Some(&string(b"a")));
    assert!(errors.iter().any(|c| c.is_error()), "{errors:?}");
    // 253 bytes fit with both quotes; one more leaves the string open.
    let fits = "n".repeat(253);
    assert_eq!(
        fte_in(&fits, "__FILE__\n"),
        (vec![string(fits.as_bytes())], vec![])
    );
    assert_eq!(fte_in(&"n".repeat(254), "__FILE__\n").1, vec![Code::E0101]);
    // The spelling of the string reads back as its bytes.
    assert_eq!(
        first_token_in("a.qc", "__FILE__\n").spelling.as_deref(),
        Some("\"a.qc\"")
    );
}

// spec FTEF-14.74: an argument copies `__FILE__` as its text, which a `#` then writes into a
// string as it stands
#[test]
fn a_stringified_file_constant_holds_its_text() {
    let tok = first_token_in("dx.qc", "#define S(a) #a\n#define T(b) S(b)\nT(__FILE__)\n");
    assert_eq!(tok.kind, TokenKind::String(br#""dx.qc""#.to_vec()));
}

// spec FTEF-14.74: an included file is named by the bytes between its quotes joined to the
// compiled file's name after its last `/` or `\`, also when another included file includes it,
// and alone when it starts with `/`; in a macro's chunk the name is the file's
#[test]
fn an_included_file_is_named_beside_the_compiled_file() {
    let mut loader = MemoryLoader::new();
    loader.insert(
        "src/a.qc",
        "#include \"x/b.qh\"\n#define F __FILE__\nF\n".as_bytes(),
    );
    loader.insert("src/x/b.qh", "__FILE__\n#include \"c.qh\"\n".as_bytes());
    loader.insert("src/c.qh", "__FILE__\n".as_bytes());
    let mut pp = Preprocessor::new(loader).with_dialect(qc_lexer::Dialect::Fteqcc);
    let out = pp.preprocess_file(Path::new("src/a.qc")).unwrap();
    let kinds: Vec<TokenKind> = out.tokens.iter().map(|t| t.kind.clone()).collect();
    assert_eq!(
        kinds,
        [
            TokenKind::String(b"src/x/b.qh".to_vec()),
            TokenKind::String(b"src/c.qh".to_vec()),
            TokenKind::String(b"src/a.qc".to_vec()),
        ]
    );
    assert_eq!(join_paths(b"/abs.qh", b"src\\a.qc"), b"/abs.qh");
    assert_eq!(join_paths(b"b.qh", b"src\\a.qc"), b"src\\b.qh");
    assert_eq!(join_paths(b"b.qh", b"a.qc"), b"b.qh");
}

// spec FTEF-14.73: a file is opened at its path and named by its name, which `__FILE__`
// writes and an `#include` is joined to, the joined name opened in the working directory; a
// file given by its path alone is named by it and opened as it is
#[test]
fn names_are_opened_in_the_working_directory() {
    let mut loader = MemoryLoader::new();
    loader.insert("/w/src/a.qc", "__FILE__\n#include \"x/b.qh\"\n".as_bytes());
    loader.insert("/w/src/x/b.qh", "__FILE__\n".as_bytes());
    let mut pp = Preprocessor::new(loader)
        .with_dialect(qc_lexer::Dialect::Fteqcc)
        .with_working_dir(Path::new("/w"));
    let out = pp
        .preprocess_file_named(Path::new("/w/src/a.qc"), b"src/a.qc")
        .unwrap();
    let kinds: Vec<TokenKind> = out.tokens.iter().map(|t| t.kind.clone()).collect();
    assert_eq!(
        kinds,
        [
            TokenKind::String(b"src/a.qc".to_vec()),
            TokenKind::String(b"src/x/b.qh".to_vec()),
        ]
    );
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    assert!(pp.preprocess_file(Path::new("src/a.qc")).is_err());
    let out = pp
        .preprocess_source_named(Path::new("/w/src/x/b.qh"), b"progs.src")
        .unwrap();
    assert_eq!(out.tokens[0].kind, TokenKind::String(b"progs.src".to_vec()));
    let out = pp.preprocess_file(Path::new("/w/src/x/b.qh")).unwrap();
    assert_eq!(
        out.tokens[0].kind,
        TokenKind::String(b"/w/src/x/b.qh".to_vec())
    );
}

// spec FTEF-14.70: `QCC_PR_AddIncludePath` names an empty directory `.` and puts a `/` after a
// name unless it ends in `/` or `\`
#[test]
fn include_directory_names() {
    assert_eq!(crate::include_dir_name(b""), b"./");
    assert_eq!(crate::include_dir_name(b"inc"), b"inc/");
    assert_eq!(crate::include_dir_name(b"inc/"), b"inc/");
    assert_eq!(crate::include_dir_name(b"inc\\"), b"inc\\");
    assert_eq!(crate::pragma_include_dir(b"y", b"src/a.qc"), b"src/y/");
    assert_eq!(crate::pragma_include_dir(b"y\\", b"src/a.qc"), b"src/y\\");
    assert_eq!(crate::pragma_include_dir(b"", b"src/a.qc"), b"src/");
}

// spec FTEF-14.74: `#pragma file(NAME)` and `#pragma file NAME` name the file from there, any
// case of `file`; a byte other than a space, tab or `(` after `file` makes another pragma
#[test]
fn pragma_file_names_the_file() {
    let names = |text: &str| -> Vec<TokenKind> { fte_in("t.qc", text).0 };
    let string = |bytes: &[u8]| TokenKind::String(bytes.to_vec());
    assert_eq!(
        names("__FILE__\n#pragma file(ex_a.qc) junk\n__FILE__\n"),
        [string(b"t.qc"), string(b"ex_a.qc")]
    );
    assert_eq!(names("#pragma FILE  x y \n__FILE__\n"), [string(b" x y")]);
    assert_eq!(names("#pragma file(n:m)\n__FILE__\n"), [string(b"n")]);
    assert_eq!(names("#pragma file(open\n__FILE__\n"), [string(b"open")]);
    assert_eq!(names("#pragma file\n__FILE__\n"), [string(b"")]);
    assert_eq!(names("#pragma file:x\n__FILE__\n"), [string(b"t.qc")]);
    // A pragma's text is bytes, so its `\` is no lexical error; the string FTEQCC then reads
    // takes `\\` as `\`.
    assert_eq!(
        fte_in("t.qc", "#pragma file(e\\\\x)\n__FILE__\n"),
        (vec![string(b"e\\x")], vec![])
    );
    // FTEF-14.63: the text is no lexical error, but `'` is no warning state FTEQCC knows.
    assert_eq!(fte_in("t.qc", "#pragma warning 'x\n").1, vec![Code::W0113]);
    assert_eq!(
        names("#pragma file(n)\n#define F __FILE__\nF\n"),
        [string(b"n")]
    );
}

// spec FTEF-14.74: a pragma's text runs to its line's break whatever a comment, a string or a
// `\` in it would run across, and reading goes on there
#[test]
fn a_pragma_s_text_ends_at_its_line_s_break() {
    for pragma in [
        "#pragma message x /* y\n",
        "#pragma message x \"y\n",
        "#pragma message x \\\n",
        "#pragma message x /* y */ z\n",
    ] {
        // `message` is no pragma FTEQCC knows (FTEF-12.11).
        let source = format!("{pragma}float f;\n");
        assert_eq!(
            fte(&source),
            ("float f ;".to_owned(), vec![Code::W0113]),
            "{source:?}"
        );
    }
    assert_eq!(fte("#pragma x /* a\nb */ float f;\n").0, "b * / float f ;");
    assert_eq!(fte("float g;\n#pragma x").0, "float g ;");
}

// spec FTEF-14.17: a built-in constant's text is a chunk of its own, so a function-like macro's
// name in it reads no `(` from the text after the constant
#[test]
fn a_name_in_a_built_in_constant_reads_on_in_its_text() {
    let (text, _) = fte("#pragma file(x\"M)\n#define M(a) a a\n__FILE__(7);\n");
    assert_eq!(text, "\"x\" M \"\" ( 7 ) ;");
}

// spec FTEF-18.16: `$framevalue N` numbers from `atoi`'s `int`, the low 32 bits of a 64-bit
// `strtol` (FTEF-14.55), as the pinned FTEQCC gives `x` 5, 1215752192 and 2147483648
#[test]
fn a_frame_value_past_an_int_keeps_its_low_32_bits() {
    assert_eq!(
        fte("$framevalue 4294967301\n$frame a\nx = $a;"),
        ("x = 5 ;".to_owned(), vec![])
    );
    assert_eq!(
        fte("$framevalue 99999999999\n$frame a\nx = $a;"),
        ("x = 1215752200 ;".to_owned(), vec![])
    );
    assert_eq!(
        fte("$framevalue -2147483649\n$frame a\nx = $a;"),
        ("x = 2147483600 ;".to_owned(), vec![])
    );
    assert_eq!(
        fte("$framevalue -2\n$frame a b c\nx = $c;"),
        ("x = 0 ;".to_owned(), vec![])
    );
}

// spec FTEF-18.30: a frame macro numbered below 0 is "Unknown frame macro" where a reference or a
// `$framerestore` expands it, as the pinned FTEQCC refuses `$framevalue -99999999999` and
// `$framevalue 2147483648` before `$frame a` and `x = $a;`
#[test]
fn a_negative_frame_macro_is_unknown_where_it_is_expanded() {
    let unknown = |text: &str| fte(text).1;
    for value in ["-1", "-5", "-99999999999", "2147483648", "-2147483648"] {
        assert_eq!(
            unknown(&format!("$framevalue {value}\n$frame a\nx = $a;")),
            vec![Code::E0106],
            "{value}"
        );
    }
    let (_, out) = one_fte("$framevalue -3\n$frame a\nx = $a;");
    assert_eq!(tokens(&out), "x = $a ;");
    assert_eq!(out.diagnostics[0].message, "Unknown frame macro $a");
    // 0 and above expand; the names after a negative one count up to them.
    assert_eq!(
        fte("$framevalue -2\n$frame a b c\nx = $c; y = $b;"),
        ("x = 0 ; y = $b ;".to_owned(), vec![Code::E0106])
    );
    // `$framerestore` of a negative macro is unknown and leaves the numbering as it was.
    assert_eq!(
        fte("$framevalue -4\n$frame a\n$framevalue 7\n$framerestore a\n$frame d\nx = $d;"),
        ("x = 7 ;".to_owned(), vec![Code::E0106])
    );
    // A `$modelname` lookup is `QCC_PR_FindMacro`'s alone, so a negative macro numbers from it.
    assert_eq!(
        fte("$framevalue -4\n$frame m\n$framevalue 3\n$modelname m\n$frame d e f g h\nx = $h;"),
        ("x = 0 ;".to_owned(), vec![])
    );
    // The lookup's warnings come before the refusal, as `QCC_PR_FindMacro` gives them.
    let (_, out) = one_fte("$framevalue -1\n$frame A\nx = $a;");
    assert_eq!(codes(&out), [Code::W0104, Code::E0106]);
}

// spec QC-8.5: the vanilla profile numbers frames from 0 and has no `$framevalue`, so no
// frame macro is negative
#[test]
fn vanilla_frame_macros_are_never_negative() {
    let mut loader = MemoryLoader::new();
    loader.insert("a.qc", "$frame a b\nx = $b;\n".as_bytes());
    let mut pp = Preprocessor::new(loader);
    let out = pp.preprocess_file(Path::new("a.qc")).unwrap();
    assert_eq!(tokens(&out), "x = 1 ;");
    assert_eq!(codes(&out), vec![]);
}

// spec FTEF-14.35: a directive goes on past a break inside a `/* */` comment, a `*` or `/` in
// the comment not ending it
#[test]
fn a_comment_with_stars_and_slashes_continues_a_directive() {
    assert_eq!(fte("#define G 1 /* x\n*y\n */ 2\nG\n").0, "1 2");
    assert_eq!(fte("#define G 1 /* a / b\n */ 2\nG\n").0, "1 2");
    assert_eq!(fte("#define G 1 /* a */\n2\nG\n").0, "2 1");
    assert_eq!(fte("#define G 1 /**/ 2\nG\n").0, "1 2");
    // `//*` is a `//` comment, which ends the directive, not a `/*` one.
    assert_eq!(fte("#define H 1 //* c\n2\nH\n").0, "2 1");
}

// spec FTEF-14.1: an `#elif` with no block is reported at the whole directive
// spec FTEF-14.37: and a block left open while a side is skipped at its `#if` line
#[test]
fn conditional_errors_span_the_directive() {
    let (_, out) = one_fte("x\n#elif 1\n");
    assert_eq!(codes(&out), vec![Code::E0109]);
    assert_eq!(out.diagnostics[0].span.span, qc_lexer::Span::new(2, 9));
    let (_, out) = one_fte("x\n#if 0\ny\n");
    assert_eq!(codes(&out), vec![Code::E0110]);
    assert_eq!(out.diagnostics[0].span.span, qc_lexer::Span::new(2, 7));
}

// spec FTEF-14.33: (P2-R2 review) `||` and `&&` group to the right, so a chain of either is
// read to its end with its value, and a chain or a run of `!` far longer than any real
// condition still takes one side, without exhausting the stack
#[test]
fn reviewer_long_logical_chains() {
    let side = |cond: &str| fte(&format!("#if {cond}\nyes\n#else\nno\n#endif\n")).0;
    assert_eq!(side(&format!("{}1", "0 || ".repeat(30))), "yes");
    assert_eq!(side(&format!("{}0", "1 && ".repeat(30))), "no");
    assert_eq!(side(&format!("{}1", "1 && ".repeat(30))), "yes");
    for cond in [
        format!("{}1", "0 || ".repeat(100_000)),
        format!("{}1", "!".repeat(100_000)),
        format!("{}1", "(".repeat(100_000)),
    ] {
        let taken = side(&cond);
        assert!(taken == "yes" || taken == "no", "{taken}");
    }
}

// spec FTEF-14.37: of three blocks left open at the end of a file, the FTEQCC profile reports
// only the one whose side is being skipped, where the vanilla profile reports each (QC-9.12)
#[test]
fn reviewer_open_blocks_at_the_end_of_a_file() {
    let text = "#ifndef X\n#ifndef Y\na\n#ifdef Z\nb\n";
    let (_, vanilla) = one(text);
    assert_eq!(tokens(&vanilla), "a");
    assert_eq!(codes(&vanilla), vec![Code::E0110; 3]);
    let (toks, errs) = fte(text);
    assert_eq!((toks.as_str(), errs), ("a", vec![Code::E0110]));
}

// spec QC-9.6: conditional blocks nest 100000 deep under both profiles: the innermost text is
// kept or dropped by every block around it, and each `#endif` closes one
#[test]
fn reviewer_deep_conditionals_nest_and_close() {
    let n = 100_000;
    for (open, kept) in [("#ifndef X\n", "a"), ("#ifdef X\n", "")] {
        let text = format!("{}a\n{}b\n", open.repeat(n), "#endif\n".repeat(n));
        let (_, vanilla) = one(&text);
        let (toks, errs) = fte(&text);
        let expected = if kept.is_empty() { "b" } else { "a b" };
        assert_eq!(tokens(&vanilla), expected);
        assert_eq!(codes(&vanilla), vec![]);
        assert_eq!((toks.as_str(), errs), (expected, vec![]));
    }
}

// spec QC-9.4: a chain of 1000 object-like macros, each the name of the one before, is
// rescanned to its end, one expansion each, under both profiles
#[test]
fn reviewer_a_chain_of_macros_expands_to_its_end() {
    let n = 1000;
    let mut text = "#define M0 1\n".to_owned();
    for i in 1..n {
        text.push_str(&format!("#define M{i} M{}\n", i - 1));
    }
    text.push_str(&format!("M{}\n", n - 1));
    for dialect in [qc_lexer::Dialect::Vanilla, qc_lexer::Dialect::Fteqcc] {
        let mut loader = MemoryLoader::new();
        loader.insert("a.qc", text.as_bytes());
        let mut pp = Preprocessor::new(loader).with_dialect(dialect);
        let out = pp.preprocess_file(Path::new("a.qc")).unwrap();
        assert_eq!(tokens(&out), "1");
        assert_eq!(codes(&out), vec![]);
        assert_eq!(pp.expansions().len(), n);
    }
}

// spec FTE-23.10: under FTEQCC a `#` right before a macro's name is the macro's expansion with
// no `#`: an object-like macro, an empty one, a function-like one with its arguments, a
// built-in constant, and a `#` a macro's body holds
#[test]
fn hash_before_a_macro_is_its_expansion() {
    for (text, want) in [
        ("#define N 5\nf = #N;\n", "f = 5 ;"),
        ("#define N \"x\"\nf = #N:#N\n", "f = \"x\" : \"x\""),
        ("#define E\nf = #E #5\n", "f = # 5"),
        ("#define M(x) x\nf = #M(#5)\n", "f = # 5"),
        ("#define M(x) x\nf = #M (7)\n", "f = 7"),
        ("f = #__LINE__\n", "f = 1"),
        ("#define A #N\n#define N 5\nf = A\n", "f = 5"),
    ] {
        assert_eq!(fte(text), (want.to_owned(), vec![]), "{text:?}");
    }
}

// spec QC-9.16: the tokens of one expansion keep their spans in the macro's body, so two that
// touch there touch in the output and a renderer can keep the body's spacing
#[test]
fn an_expansion_keeps_its_body_s_spans() {
    let (_, out) = one_fte("#define B #5 6\nf = B;\n");
    let body: Vec<_> = out
        .tokens
        .iter()
        .filter(|t| t.expansion.is_some())
        .map(|t| t.span.span)
        .collect();
    assert_eq!(body.len(), 3);
    assert_eq!(body[0].end, body[1].start);
    assert!(body[1].end < body[2].start);
    let expansions: Vec<_> = out.tokens.iter().map(|t| t.expansion).collect();
    assert_eq!(expansions[2], expansions[3]);
    assert_ne!(expansions[1], expansions[2]);
}

// spec FTE-23.10: a `#` stays when the word right after it is no macro FTEQCC expands there:
// no macro, a space or another byte first, a word longer than the name, a function-like macro
// with no arguments, or a `#` in the vanilla dialect
#[test]
fn hash_before_anything_else_stays() {
    for (text, want) in [
        ("f = #U\n", "f = # U"),
        ("#define N 5\nf = # N\n", "f = # 5"),
        ("#define N 5\nf = #N!\n", "f = # N !"),
        // The first `#` stays, before a `#`; the second goes.
        ("#define N 5\nf = ##N\n", "f = # 5"),
        ("#define N 5\nf = #\"N\"\n", "f = # \"N\""),
        ("#define M(x) x\nf = #M;\n", "f = # M ;"),
        ("#define N 5\nf = #5 #-N\n", "f = # 5 # - 5"),
    ] {
        assert_eq!(fte(text), (want.to_owned(), vec![]), "{text:?}");
    }
    let (_, out) = one("#define N 5\nf = #N;\n");
    assert_eq!(tokens(&out), "f = # 5 ;");
}

// spec FTE-23.10: a function-like macro after `#` with a bad or unclosed argument list keeps
// the `#` and reports its error once
#[test]
fn a_refused_macro_after_hash_reports_once() {
    let (toks, errs) = fte("#define M(x) x\nf = #M(1, 2, 3);\n");
    assert_eq!(errs, vec![Code::E0116]);
    assert_eq!(toks, "f = # M ( 1 , 2 , 3 ) ;");
    assert_eq!(fte("#define M(x) x\nf = #M(1\n").1, vec![Code::E0117]);
}
