// SPDX-License-Identifier: GPL-3.0-or-later

//! The directives the graph follows (PRJ-5).

use std::path::PathBuf;

use super::{GraphSpec, brought, build, diags, names, source, vias};
use crate::Via;

// spec PRJ-5.1: a directive is a # line, with blanks allowed before and after the #
#[test]
fn directive_lines_allow_blanks() {
    let files = [("a.qc", ""), ("b.qc", ""), ("c.qc", "")];
    let body = "#include \"a.qc\"\n  \t#  include \"b.qc\"\nx = 1; #include \"c.qc\"\n";
    assert_eq!(brought(body, &files), ["a.qc", "b.qc"]);
}

// spec PRJ-5.1: a # line inside a /* */ comment is not a directive
#[test]
fn directives_in_block_comments_are_ignored() {
    let files = [("a.qc", ""), ("b.qc", "")];
    let body = "/*\n#include \"a.qc\"\n*/\n#include \"b.qc\"\n";
    assert_eq!(brought(body, &files), ["b.qc"]);
    let body =
        "#define S\nx = 1; /* starts\n#include \"a.qc\"\n ends */ y = 2;\n#include \"b.qc\"\n";
    assert_eq!(brought(body, &files), ["b.qc"]);
    let body = "#ifdef X /* open\n#include \"a.qc\"\n*/\n#endif\n#include \"b.qc\"\n";
    assert_eq!(brought(body, &files), ["b.qc"]);
}

// spec PRJ-5.1: text in string literals never starts a comment
#[test]
fn strings_do_not_start_comments() {
    let files = [("a.qc", ""), ("b.qc", "")];
    let body = "#define S\ns = \"/* not a comment\";\n#include \"a.qc\"\nt = 'x/*';\nu = \"\\\"/*\";\n#include \"b.qc\"\n";
    assert_eq!(brought(body, &files), ["a.qc", "b.qc"]);
    let body = "#define S\ns = \"x\"; /* open\n#include \"a.qc\"\n*/\n#include \"b.qc\"\n";
    assert_eq!(brought(body, &files), ["b.qc"]);
    let body = "#define S\nx = 1; // /* not open\n#include \"a.qc\"\n";
    assert_eq!(brought(body, &files), ["a.qc"]);
}

// spec PRJ-5.1: # followed by anything but a name is not a directive, so a list entry may start with #
#[test]
fn hash_without_a_name_is_not_a_directive() {
    let graph = build(&GraphSpec {
        files: &[
            ("progs.src", "x.dat\n#1.qc\n# \"q.qc\"\n#_x\n"),
            ("#1.qc", ""),
            ("#", ""),
            ("q.qc", ""),
        ],
        ..GraphSpec::default()
    });
    assert_eq!(names(&graph), ["#1.qc", "#", "q.qc"]);
}

// spec PRJ-5.4: #define with no name defines nothing
#[test]
fn define_without_a_name() {
    let files = [("a.qc", "")];
    let body = "#define\n#ifdef\n#include \"a.qc\"\n#endif\n";
    assert!(brought(body, &files).is_empty());
}

// spec PRJ-5.1: a line ending in a backslash continues the directive
#[test]
fn backslash_continues_a_directive() {
    let files = [("a.qc", ""), ("b.qc", "")];
    let body = "#define LONG 1 \\\n#include \"a.qc\"\n#include \"b.qc\"\n";
    assert_eq!(brought(body, &files), ["b.qc"]);
}

// spec PRJ-5.1: pragma names are compared without regard to ASCII case
#[test]
fn pragma_names_ignore_case() {
    let graph = source("#pragma Progs_Dat \"out/x.dat\"\n", &[]);
    assert_eq!(graph.declared_output, Some(PathBuf::from("out/x.dat")));
}

// spec PRJ-5.1: directive names are compared exactly
#[test]
fn directive_names_are_exact() {
    let files = [("a.qc", "")];
    assert!(brought("#INCLUDE \"a.qc\"\n#includex \"a.qc\"\n", &files).is_empty());
}

// spec PRJ-5.1: a UTF-8 byte order mark at the start of a file is skipped
#[test]
fn byte_order_mark_is_skipped() {
    let files = [
        ("once.qh", "\u{feff}#pragma once\n"),
        ("a.qc", "\u{feff}#include \"once.qh\"\n"),
    ];
    let body = "\u{feff}#include \"a.qc\"\n#include \"once.qh\"\n";
    assert_eq!(brought(body, &files), ["a.qc", "once.qh"]);
}

// spec PRJ-5.2: #include "path" and #include <path> both compile the file where they stand
#[test]
fn include_quoted_and_angled() {
    let files = [("a.qc", ""), ("lib/b.qh", "")];
    let body = "#include \"a.qc\"\n#include <lib/b.qh>\n";
    assert_eq!(brought(body, &files), ["a.qc", "lib/b.qh"]);
}

// spec PRJ-5.2: the including file's directory first, then each include directory in order
#[test]
fn include_search_order() {
    let graph = build(&GraphSpec {
        files: &[
            (
                "src/main.src",
                "#include \"x.qh\"\n#include \"y.qh\"\n#include \"z.qh\"\n",
            ),
            ("src/x.qh", ""),
            ("one/x.qh", ""),
            ("one/y.qh", ""),
            ("two/y.qh", ""),
            ("two/z.qh", ""),
        ],
        src: "src/main.src",
        include_dirs: &["one", "two"],
        ..GraphSpec::default()
    });
    assert_eq!(
        names(&graph),
        ["src/main.src", "src/x.qh", "one/y.qh", "two/z.qh"]
    );
}

// spec PRJ-5.2: an include that names no readable file is E0111 and the line is skipped
// spec QCD-8.4: E0111 "cannot include PATH: REASON"
#[test]
fn missing_include_is_e0111() {
    let graph = source("#include \"gone.qh\"\n#include \"a.qc\"\n", &[("a.qc", "")]);
    assert_eq!(diags(&graph), ["E0111 main.src:1"]);
    assert_eq!(
        graph.diagnostics[0].message,
        "cannot include gone.qh: no such file"
    );
    assert_eq!(names(&graph), ["main.src", "a.qc"]);
}

// spec PRJ-5.2: a malformed #include is skipped without a diagnostic
#[test]
fn malformed_include_is_skipped() {
    let graph = source(
        "#include\n#include a.qc\n#include \"open\n",
        &[("a.qc", "")],
    );
    assert_eq!(names(&graph), ["main.src"]);
    assert!(graph.diagnostics.is_empty());
}

// spec PRJ-5.3: each word between #includelist and #endlist is a file, compiled in order
// spec PRJ-5.3: words split at blanks and , ; ( ) ], // ends the line, quotes are removed
#[test]
fn includelist_words() {
    let files = [
        ("a.qc", ""),
        ("b.qc", ""),
        ("c.qc", ""),
        ("d e.qc", ""),
        ("f.qc", ""),
        ("g.qc", ""),
    ];
    let body =
        "#includelist\na.qc, b.qc;(c.qc)]\n\"d\" // e.qc\n\"f.qc\"\n/* x.qc */ g.qc\n#endlist\n";
    let graph = source(body, &files);
    assert_eq!(
        names(&graph),
        ["main.src", "a.qc", "b.qc", "c.qc", "f.qc", "g.qc"]
    );
    assert_eq!(diags(&graph), ["E0402 main.src:3"]);
    let graph = source("#includelist\n/* a.qc\nb.qc */ c.qc\n#endlist\n", &files);
    assert_eq!(names(&graph), ["main.src", "c.qc"]);
    assert!(graph.diagnostics.is_empty());
}

// spec PRJ-5.3: directive lines inside a list are followed, so #ifdef can skip entries
#[test]
fn includelist_follows_directives() {
    let files = [("a.qc", ""), ("b.qc", ""), ("c.qc", ""), ("i.qh", "")];
    let body = "#define B\n#includelist\na.qc\n#ifndef B\nnot.qc\n#endif\n#ifdef B\nb.qc\n#endif\n#include \"i.qh\"\nc.qc\n#endlist\n";
    assert_eq!(brought(body, &files), ["a.qc", "b.qc", "i.qh", "c.qc"]);
}

// spec PRJ-5.3: a list word that names no readable file is E0402
#[test]
fn includelist_missing_file_is_e0402() {
    let graph = source("#includelist\na.qc\ngone.qc\n#endlist\n", &[("a.qc", "")]);
    assert_eq!(diags(&graph), ["E0402 main.src:3"]);
    assert_eq!(names(&graph), ["main.src", "a.qc"]);
}

// spec PRJ-5.3: a file that ends inside a list is E0406, and the list ends there
// spec QCD-5.6: E0406 "#includelist without #endlist"
#[test]
fn unterminated_includelist_is_e0406() {
    let graph = build(&GraphSpec {
        files: &[
            ("progs.src", "x.dat\na.qc\nb.qc\n"),
            ("a.qc", "\n#includelist\nc.qc\n"),
            ("b.qc", ""),
            ("c.qc", ""),
        ],
        ..GraphSpec::default()
    });
    assert_eq!(names(&graph), ["a.qc", "c.qc", "b.qc"]);
    assert_eq!(diags(&graph), ["E0406 a.qc:2"]);
    assert_eq!(
        graph.diagnostics[0].message,
        "#includelist without #endlist"
    );
}

// spec PRJ-5.4: #define and #undef decide the conditionals, function-like macros included
#[test]
fn define_and_undef() {
    let files = [("a.qc", ""), ("b.qc", ""), ("c.qc", "")];
    let body = "#define A\n#define F(x) x\n#ifdef A\n#include \"a.qc\"\n#endif\n#undef A\n#ifdef A\n#include \"b.qc\"\n#endif\n#ifdef F\n#include \"c.qc\"\n#endif\n";
    assert_eq!(brought(body, &files), ["a.qc", "c.qc"]);
}

// spec PRJ-5.4: the module's defines come first, and macros run on across files in order
#[test]
fn macros_run_on_across_files() {
    let graph = build(&GraphSpec {
        files: &[
            ("progs.src", "x.dat\nfirst.qc\nsecond.qc\n"),
            ("first.qc", "#ifdef GIVEN\n#define LATER\n#endif\n"),
            ("second.qc", "#ifdef LATER\n#include \"yes.qh\"\n#endif\n"),
            ("yes.qh", ""),
        ],
        defines: &["GIVEN"],
        ..GraphSpec::default()
    });
    assert_eq!(names(&graph), ["first.qc", "second.qc", "yes.qh"]);
}

// spec PRJ-5.5: #ifdef, #ifndef, #else and #endif nest, and skipped blocks are only counted
#[test]
fn conditionals_nest() {
    let files = [("a.qc", ""), ("b.qc", ""), ("c.qc", ""), ("d.qc", "")];
    let body = "#ifdef NO\n#ifndef NO\n#include \"a.qc\"\n#else\n#include \"a.qc\"\n#endif\n#else\n#ifndef NO\n#include \"b.qc\"\n#endif\n#endif\n#ifndef NO\n#include \"c.qc\"\n#else\n#include \"d.qc\"\n#endif\n";
    assert_eq!(brought(body, &files), ["b.qc", "c.qc"]);
}

// spec PRJ-5.5: #if and #elif evaluate expressions, and the first true branch is taken
#[test]
fn if_elif_else_chain() {
    let files = [("one.qc", ""), ("two.qc", ""), ("three.qc", "")];
    let chain = |cond: &str| {
        format!(
            "#define V 2\n#if {cond}\n#include \"one.qc\"\n#elif V == 2\n#include \"two.qc\"\n#elif 1\n#include \"three.qc\"\n#else\n#include \"three.qc\"\n#endif\n"
        )
    };
    assert_eq!(brought(&chain("V == 1"), &files), ["two.qc"]);
    assert_eq!(brought(&chain("V > 1"), &files), ["one.qc"]);
    assert_eq!(brought(&chain("garbage +"), &files), ["two.qc"]);
}

// spec PRJ-5.10: the graph reads a value inside 63 parentheses, and one inside 64 is false
#[test]
fn if_nesting_limit() {
    let files = [("yes.qc", ""), ("no.qc", "")];
    let body = |n: usize| {
        format!(
            "#if {}1{}\n#include \"yes.qc\"\n#else\n#include \"no.qc\"\n#endif\n",
            "(".repeat(n),
            ")".repeat(n)
        )
    };
    assert_eq!(brought(&body(63), &files), ["yes.qc"]);
    assert_eq!(brought(&body(64), &files), ["no.qc"]);
}

// spec FTEF-14.33: the graph of an FTEQCC module reads #if as FTEQCC does, a vanilla one with C's precedence
#[test]
fn if_reads_in_the_module_dialect() {
    let files = [("yes.qc", "")];
    let body = "#if 0 && 0 || 1\n#include \"yes.qc\"\n#endif\n";
    let mut all = vec![("main.src", body)];
    all.extend_from_slice(&files);
    let graph = |dialect| {
        build(&GraphSpec {
            files: &all,
            src: "main.src",
            dialect,
            ..GraphSpec::default()
        })
    };
    let vanilla = graph(qc_lexer::Dialect::Vanilla);
    assert_eq!(
        names(&vanilla).into_iter().skip(1).collect::<Vec<_>>(),
        ["yes.qc"]
    );
    let fteqcc = graph(qc_lexer::Dialect::Fteqcc);
    assert_eq!(names(&fteqcc).len(), 1);
}

// spec PRJ-5.5: #elifdef and #elifndef take their branch on whether the name is a macro
#[test]
fn elifdef_and_elifndef() {
    let files = [("a.qc", ""), ("b.qc", ""), ("c.qc", "")];
    let body = "#define S\n#ifdef C\n#include \"a.qc\"\n#elifdef S\n#include \"b.qc\"\n#elifndef S\n#include \"c.qc\"\n#endif\n#ifdef C\n#elifndef C\n#include \"c.qc\"\n#endif\n";
    assert_eq!(brought(body, &files), ["b.qc", "c.qc"]);
}

// spec PRJ-5.5: an #else, #elif or #endif without a block is skipped
#[test]
fn stray_else_and_endif_are_skipped() {
    let files = [("a.qc", "")];
    let body = "#endif\n#else\n#elif 0\n#include \"a.qc\"\n";
    assert_eq!(brought(body, &files), ["a.qc"]);
}

// spec PRJ-5.5: a block is closed in the file that opens it
#[test]
fn blocks_close_at_the_end_of_their_file() {
    let graph = build(&GraphSpec {
        files: &[
            ("progs.src", "x.dat\nopen.qc\nnext.qc\n"),
            ("open.qc", "#ifdef NEVER\n"),
            ("next.qc", "#include \"seen.qh\"\n"),
            ("seen.qh", ""),
        ],
        ..GraphSpec::default()
    });
    assert_eq!(names(&graph), ["open.qc", "next.qc", "seen.qh"]);
}

// spec PRJ-5.6: #output and #pragma progs_dat resolve against the holder's directory; the last wins
#[test]
fn declared_output() {
    let graph = build(&GraphSpec {
        files: &[
            (
                "src/main.src",
                "#output \"../a.dat\"\n#include \"sub/set.qh\"\n",
            ),
            (
                "src/sub/set.qh",
                "#pragma progs_dat \"../../build/b.dat\"\n",
            ),
        ],
        src: "src/main.src",
        ..GraphSpec::default()
    });
    assert_eq!(graph.declared_output, Some(PathBuf::from("build/b.dat")));
    let graph = source("#output \"a.dat\"\n", &[]);
    assert_eq!(graph.declared_output, Some(PathBuf::from("a.dat")));
}

// spec FTEF-12.10: a list-form progs.src's #pragma autoproto, in any case, asks for autoprototyping, `0` not; one in a source file is the preprocessor's
#[test]
fn list_form_autoproto_pragma() {
    let list = |text: &str| {
        build(&GraphSpec {
            files: &[("progs.src", text), ("a.qc", "#pragma autoproto\n")],
            ..GraphSpec::default()
        })
        .autoproto
    };
    assert_eq!(list("../progs.dat\n#pragma autoproto\na.qc\n"), Some(true));
    assert_eq!(
        list("../progs.dat\n#pragma AutoProto 1\na.qc\n"),
        Some(true)
    );
    assert_eq!(
        list("../progs.dat\n#pragma autoproto 0\na.qc\n"),
        Some(false)
    );
    assert_eq!(list("../progs.dat\na.qc\n"), None);
    assert_eq!(source("#pragma autoproto\n", &[]).autoproto, None);
}

// spec PRJ-5.7: #pragma sourcefile registers programs against the module source's directory, once each
#[test]
fn sourcefile_registers_programs() {
    let graph = build(&GraphSpec {
        files: &[
            (
                "src/main.src",
                "#pragma sourcefile a.src sub/b.src\n#include \"inc/x.qh\"\n",
            ),
            ("src/inc/x.qh", "#pragma sourcefile a.src c.src\n"),
        ],
        src: "src/main.src",
        ..GraphSpec::default()
    });
    assert_eq!(
        graph.sourcefiles,
        [
            PathBuf::from("src/a.src"),
            PathBuf::from("src/sub/b.src"),
            PathBuf::from("src/c.src"),
        ]
    );
}

// spec PRJ-5.8: #pragma includedir adds a directory, against the holder's, for the rest of the module
#[test]
fn includedir_pragma() {
    let graph = build(&GraphSpec {
        files: &[
            (
                "progs/main.src",
                "#pragma includedir \"../lib/\"\n#includelist\nx.qc\n#endlist\n",
            ),
            ("lib/x.qc", "#include \"y.qh\"\n"),
            ("lib/y.qh", ""),
        ],
        src: "progs/main.src",
        ..GraphSpec::default()
    });
    assert_eq!(names(&graph), ["progs/main.src", "lib/x.qc", "lib/y.qh"]);
}

// spec PRJ-5.9: #pragma once stops a later #include, list entry or #includelist entry
#[test]
fn pragma_once() {
    let graph = build(&GraphSpec {
        files: &[
            (
                "progs.src",
                "x.dat\nonce.qh\nuse.qc\nonce.qh\nplain.qh\nplain.qh\n",
            ),
            ("once.qh", "#pragma once\n"),
            (
                "use.qc",
                "#include \"once.qh\"\n#includelist\nonce.qh\n#endlist\n",
            ),
            ("plain.qh", ""),
        ],
        ..GraphSpec::default()
    });
    assert_eq!(names(&graph), ["once.qh", "use.qc", "plain.qh", "plain.qh"]);
}

// spec PRJ-5.9: #pragma once in a skipped block does nothing
#[test]
fn pragma_once_in_skipped_block() {
    let graph = build(&GraphSpec {
        files: &[
            ("progs.src", "x.dat\nh.qh\nh.qh\n"),
            ("h.qh", "#ifdef NO\n#pragma once\n#endif\n"),
        ],
        ..GraphSpec::default()
    });
    assert_eq!(names(&graph), ["h.qh", "h.qh"]);
}

// spec PRJ-5.3: a `//` touching a word ends it and the line, and a word that is only a pair of
// quotes is no file
#[test]
fn includelist_comment_after_a_word_and_empty_quotes() {
    let files = [("a.qc", ""), ("b.qc", ""), ("c.qc", "")];
    let body = "#includelist\na.qc// b.qc\n\"\" c.qc\n#endlist\n";
    let graph = source(body, &files);
    assert_eq!(names(&graph), ["main.src", "a.qc", "c.qc"]);
    assert!(diags(&graph).is_empty(), "{:?}", diags(&graph));
}

// spec PRJ-5.1: a directive whose last line ends in `\` at the end of the file ends there
#[test]
fn backslash_at_the_end_of_the_file() {
    let files = [("a.qc", "")];
    let graph = source("#include \"a.qc\"\n#define LONG 1 \\", &files);
    assert_eq!(names(&graph), ["main.src", "a.qc"]);
    assert!(diags(&graph).is_empty());
}

// spec PRJ-5.4: `#undef` with no name removes nothing
#[test]
fn undef_without_a_name() {
    let files = [("a.qc", "")];
    let body = "#define A\n#undef\n#ifdef A\n#include \"a.qc\"\n#endif\n";
    assert_eq!(brought(body, &files), ["a.qc"]);
}

// spec PRJ-5.6: `#output` and `#pragma progs_dat` with no path declare nothing, and leave an
// earlier declaration as it is
#[test]
fn output_without_a_path() {
    assert_eq!(source("#output\n", &[]).declared_output, None);
    assert_eq!(source("#output a.dat\n", &[]).declared_output, None);
    assert_eq!(source("#pragma progs_dat\n", &[]).declared_output, None);
    let graph = source("#output \"a.dat\"\n#pragma progs_dat\n#output\n", &[]);
    assert_eq!(graph.declared_output, Some(PathBuf::from("a.dat")));
}

/// The graph of a list-form module of `dialect` that compiles `src/a.qc` among `files`.
fn listed(files: &[(&str, &str)], dialect: qc_lexer::Dialect) -> crate::FileGraph {
    let mut all = vec![("progs.src", "progs.dat\nsrc/a.qc\n")];
    all.extend_from_slice(files);
    build(&GraphSpec {
        files: &all,
        dialect,
        ..GraphSpec::default()
    })
}

// spec PRJ-5.11: under FTEQCC a nested `#include` is found beside the file being compiled, not
// the file that holds it; the vanilla dialect keeps PRJ-5.2
#[test]
fn fteqcc_includes_are_found_beside_the_compiled_file() {
    let files = [
        ("src/a.qc", "#include \"x/b.qh\"\n"),
        ("src/x/b.qh", "#include \"c.qh\"\n"),
        ("src/c.qh", ""),
        ("src/x/c.qh", ""),
    ];
    let graph = listed(&files, qc_lexer::Dialect::Fteqcc);
    assert_eq!(names(&graph), ["src/a.qc", "src/x/b.qh", "src/c.qh"]);
    let graph = listed(&files, qc_lexer::Dialect::Vanilla);
    assert_eq!(names(&graph), ["src/a.qc", "src/x/b.qh", "src/x/c.qh"]);
    let graph = listed(&files[..2], qc_lexer::Dialect::Fteqcc);
    assert_eq!(diags(&graph), ["E0111 src/x/b.qh:1"]);
}

// spec PRJ-5.12: under GMQCC a nested `#include` is found beside the file that holds it, then
// beside the file being compiled; a missing one names the first
#[test]
fn gmqcc_includes_are_found_beside_the_including_file_first() {
    let gmqcc = qc_lexer::Dialect::Gmqcc;
    let files = [
        ("src/a.qc", "#include \"x/b.qh\"\n"),
        ("src/x/b.qh", "#include \"c.qh\"\n"),
        ("src/c.qh", ""),
        ("src/x/c.qh", ""),
    ];
    let graph = listed(&files, gmqcc);
    assert_eq!(names(&graph), ["src/a.qc", "src/x/b.qh", "src/x/c.qh"]);
    let graph = listed(&files[..3], gmqcc);
    assert_eq!(names(&graph), ["src/a.qc", "src/x/b.qh", "src/c.qh"]);
    let graph = listed(&files[..2], gmqcc);
    assert_eq!(diags(&graph), ["E0111 src/x/b.qh:1"]);
    // The message shows the path as the host writes it.
    let message = graph.diagnostics[0].message.replace('\\', "/");
    assert!(
        message.starts_with("cannot include src/x/c.qh"),
        "{message}"
    );
}

// spec PRJ-5.12: under GMQCC an `#includelist` entry, an include directory and `#pragma once`
// are the file holding them's, as the C preprocessor Xonotic is read with marks a header
#[test]
fn gmqcc_lists_dirs_and_once_use_the_holding_file() {
    let gmqcc = qc_lexer::Dialect::Gmqcc;
    let graph = listed(
        &[
            ("src/a.qc", "#include \"x/b.qh\"\n"),
            ("src/x/b.qh", "#includelist\nl.qc\n#endlist\n"),
            ("src/l.qc", ""),
            ("src/x/l.qc", ""),
        ],
        gmqcc,
    );
    assert_eq!(names(&graph), ["src/a.qc", "src/x/b.qh", "src/x/l.qc"]);
    let graph = listed(
        &[
            ("src/a.qc", "#include \"x/b.qh\"\n#include \"c.qh\"\n"),
            ("src/x/b.qh", "#pragma includedir y\n"),
            ("src/y/c.qh", ""),
            ("src/x/y/c.qh", ""),
        ],
        gmqcc,
    );
    assert_eq!(names(&graph), ["src/a.qc", "src/x/b.qh", "src/x/y/c.qh"]);
    let graph = listed(
        &[
            ("src/a.qc", "#include \"b.qh\"\n#include \"b.qh\"\n"),
            ("src/b.qh", "#pragma once\n"),
        ],
        gmqcc,
    );
    assert_eq!(names(&graph), ["src/a.qc", "src/b.qh"]);
}

// spec PRJ-5.12: Xonotic's layout opens under GMQCC: `server/progs.inc` includes
// `../lib/_all.inc`, which includes `compiler.qh` beside itself, in `lib/`
#[test]
fn gmqcc_opens_xonotics_layout() {
    let graph = build(&GraphSpec {
        files: &[
            ("server/progs.inc", "#include \"../lib/_all.inc\"\n"),
            ("lib/_all.inc", "#include \"compiler.qh\"\n"),
            ("lib/compiler.qh", ""),
        ],
        src: "server/progs.inc",
        dialect: qc_lexer::Dialect::Gmqcc,
        ..GraphSpec::default()
    });
    assert_eq!(
        names(&graph),
        ["server/progs.inc", "lib/_all.inc", "lib/compiler.qh"]
    );
    assert!(diags(&graph).is_empty(), "{:?}", diags(&graph));
}

// spec PRJ-5.11: under FTEQCC an `#includelist` entry in an included file, an include directory
// and `#pragma once` are the file being compiled's too
#[test]
fn fteqcc_lists_dirs_and_once_use_the_compiled_file() {
    let fte = qc_lexer::Dialect::Fteqcc;
    let graph = listed(
        &[
            ("src/a.qc", "#include \"x/b.qh\"\n"),
            ("src/x/b.qh", "#includelist\nl.qc\n#endlist\n"),
            ("src/l.qc", ""),
        ],
        fte,
    );
    assert_eq!(names(&graph), ["src/a.qc", "src/x/b.qh", "src/l.qc"]);
    let graph = listed(
        &[
            ("src/a.qc", "#include \"x/b.qh\"\n#include \"c.qh\"\n"),
            ("src/x/b.qh", "#pragma includedir y\n"),
            ("src/y/c.qh", ""),
        ],
        fte,
    );
    assert_eq!(names(&graph), ["src/a.qc", "src/x/b.qh", "src/y/c.qh"]);
    let graph = listed(
        &[
            (
                "src/a.qc",
                "#include \"b.qh\"\n#include \"b.qh\"\n#include \"a.qc\"\n",
            ),
            ("src/b.qh", "#pragma once\n"),
        ],
        fte,
    );
    assert_eq!(names(&graph), ["src/a.qc", "src/b.qh", "src/b.qh"]);
}

// spec PRJ-5.8: `#pragma includedir` with no path adds no directory
#[test]
fn includedir_without_a_path() {
    let graph = build(&GraphSpec {
        files: &[
            ("progs/main.src", "#pragma includedir\n#include \"y.qh\"\n"),
            ("y.qh", ""),
        ],
        src: "progs/main.src",
        ..GraphSpec::default()
    });
    assert_eq!(names(&graph), ["progs/main.src"]);
    assert_eq!(diags(&graph), ["E0111 progs/main.src:2"]);
}

/// The files a list-form module of `dialect` compiling `t.qc` brings in after it, with `files`.
fn after_t(files: &[(&str, &str)], dialect: qc_lexer::Dialect) -> Vec<String> {
    let mut all = vec![("progs.src", "progs.dat\nt.qc\n")];
    all.extend_from_slice(files);
    let graph = build(&GraphSpec {
        files: &all,
        dialect,
        ..GraphSpec::default()
    });
    names(&graph).into_iter().skip(1).collect()
}

// spec PRJ-5.13: under FTEQCC a directive's line ends where `QCC_PR_SkipToEndOfLine` ends it,
// names match by their first bytes and `FTEQCC` is defined; the vanilla dialect keeps PRJ-5.1
#[test]
fn fteqcc_files_are_read_with_the_directive_reader() {
    let fte = qc_lexer::Dialect::Fteqcc;
    let vanilla = qc_lexer::Dialect::Vanilla;
    let carried = [
        ("t.qc", "#undef N // c \\\n#include \"b.qh\"\n"),
        ("b.qh", ""),
    ];
    assert!(after_t(&carried, fte).is_empty());
    let prefixed = [
        (
            "t.qc",
            "#defined Q 1\n#ifdef Q\n#include \"b.qh\"\n#endif\n",
        ),
        ("b.qh", ""),
    ];
    assert_eq!(after_t(&prefixed, fte), ["b.qh"]);
    assert!(after_t(&prefixed, vanilla).is_empty());
    let predefined = [
        ("t.qc", "#ifdef FTEQCC\n#include \"b.qh\"\n#endif\n"),
        ("b.qh", ""),
    ];
    assert_eq!(after_t(&predefined, fte), ["b.qh"]);
    assert!(after_t(&predefined, vanilla).is_empty());
}

// spec FTEF-14.53: under FTEQCC and GMQCC an `#include` on a line a `\` joins to a line of code
// brings in no file, and one after a `//` comment's `\` or a blank line does
#[test]
fn joined_lines_bring_in_no_file() {
    for dialect in [qc_lexer::Dialect::Fteqcc, qc_lexer::Dialect::Gmqcc] {
        for (text, read) in [
            ("float g; \\\n#include \"b.qh\"\n", false),
            ("float g; \\\r\n  #include \"b.qh\"\n", false),
            ("float g; \\\r#include \"b.qh\"\n", false),
            (
                "#define N\nfloat g = \\\n#undef N\n#ifdef N\n#include \"b.qh\"\n#endif\n",
                true,
            ),
            ("float g; // c \\\n#include \"b.qh\"\n", true),
            ("float g; \\\n\n#include \"b.qh\"\n", true),
        ] {
            let files = [("t.qc", text), ("b.qh", "")];
            assert_eq!(after_t(&files, dialect) == ["b.qh"], read, "{text:?}");
        }
    }
}

/// Whether the preprocessor of `dialect` compiling `t.qc` with `files` beside it reads `b.qh`,
/// to which `files` gives the one name `marker`.
fn preprocessor_reads_b(files: &[(&str, &str)], dialect: qc_lexer::Dialect) -> bool {
    let mut pp = qc_preproc::Preprocessor::new(super::loader(files)).with_dialect(dialect);
    let out = pp.preprocess_file(std::path::Path::new("t.qc")).unwrap();
    out.tokens
        .iter()
        .any(|t| t.kind == qc_lexer::TokenKind::Name("marker".to_owned()))
}

// spec PRJ-5.15: under GMQCC the graph reads with the preprocessor's directive reader, so a
// carried line hides an `#include` and names match by their first bytes, but defines neither
// `FTEQCC` nor `__FTEQCC__`, as GMQCC and the C preprocessor do not; the graph lists `b.qh`
// exactly when the preprocessor reads it
#[test]
fn gmqcc_files_are_read_as_the_preprocessor_reads_them() {
    let gmqcc = qc_lexer::Dialect::Gmqcc;
    let fte = qc_lexer::Dialect::Fteqcc;
    let cases: [(&str, bool); 8] = [
        ("#ifdef FTEQCC\n#include \"b.qh\"\n#endif\n", false),
        ("#ifdef __FTEQCC__\n#include \"b.qh\"\n#endif\n", false),
        ("#ifndef FTEQCC\n#include \"b.qh\"\n#endif\n", true),
        ("#undef N // c \\\n#include \"b.qh\"\n", false),
        (
            "#ifdef FTEQCC\n#else // c \\\n#include \"b.qh\"\n#endif\n",
            false,
        ),
        ("#defined Q 1\n#ifdef Q\n#include \"b.qh\"\n#endif\n", true),
        ("#ifdef GMQCC\n#include \"b.qh\"\n#endif\n", false),
        ("#include \"b.qh\"\n", true),
    ];
    for (text, read) in cases {
        let files = [("t.qc", text), ("b.qh", "marker\n")];
        let listed = after_t(&files, gmqcc) == ["b.qh"];
        assert_eq!(listed, read, "{text:?}");
        assert_eq!(preprocessor_reads_b(&files, gmqcc), read, "{text:?}");
    }
    // FTEQCC defines its own name; GMQCC's is a module's define.
    let predefined = [
        ("t.qc", "#ifdef FTEQCC\n#include \"b.qh\"\n#endif\n"),
        ("b.qh", "marker\n"),
    ];
    assert!(preprocessor_reads_b(&predefined, fte));
    let graph = build(&GraphSpec {
        files: &[
            ("progs.src", "progs.dat\nt.qc\n"),
            ("t.qc", "#ifdef GMQCC\n#include \"b.qh\"\n#endif\n"),
            ("b.qh", ""),
        ],
        defines: &["GMQCC"],
        dialect: gmqcc,
        ..GraphSpec::default()
    });
    assert_eq!(names(&graph), ["t.qc", "b.qh"]);
}

// spec PRJ-5.13: under FTEQCC one count of open sides runs from file to file, so an `#else` in
// the including file ends a side the included file opened; the vanilla dialect closes the side
// at the end of its file (QC-9.12)
#[test]
fn fteqcc_sides_run_from_file_to_file() {
    let files = [
        (
            "t.qc",
            "#include \"o.qh\"\n#else\n#include \"b.qh\"\n#endif\n",
        ),
        ("o.qh", "#if 1\n"),
        ("b.qh", ""),
    ];
    assert_eq!(after_t(&files, qc_lexer::Dialect::Fteqcc), ["o.qh"]);
    assert_eq!(
        after_t(&files, qc_lexer::Dialect::Vanilla),
        ["o.qh", "b.qh"]
    );
}

// spec PRJ-5.13: under FTEQCC an included file's first line is a directive's only at its first
// byte, and a list entry's after spaces and tabs too, as each is included or compiled
#[test]
fn fteqcc_file_starts_follow_how_the_file_came_in() {
    let fte = qc_lexer::Dialect::Fteqcc;
    let included = [
        ("t.qc", "#include \"o.qh\"\n"),
        ("o.qh", "  #include \"b.qh\"\n"),
        ("b.qh", ""),
    ];
    assert_eq!(after_t(&included, fte), ["o.qh"]);
    let listed = [
        ("t.qc", "#includelist\no.qc\n#endlist\n"),
        ("o.qc", "  #include \"b.qh\"\n"),
        ("b.qh", ""),
    ];
    assert_eq!(after_t(&listed, fte), ["o.qc", "b.qh"]);
    assert_eq!(
        vias(&listed_graph(&listed)),
        [
            ("t.qc".to_owned(), Via::Listed),
            ("o.qc".to_owned(), Via::IncludeList),
            ("b.qh".to_owned(), Via::Include),
        ]
    );
}

/// The FTEQCC graph of a list-form module compiling `t.qc`, with `files`.
fn listed_graph(files: &[(&str, &str)]) -> crate::FileGraph {
    let mut all = vec![("progs.src", "progs.dat\nt.qc\n")];
    all.extend_from_slice(files);
    build(&GraphSpec {
        files: &all,
        dialect: qc_lexer::Dialect::Fteqcc,
        ..GraphSpec::default()
    })
}

// spec PRJ-5.13: under FTEQCC `#output`, the pragmas and an unclosed list reach the graph from
// the reader, and `#eof` ends the file
#[test]
fn fteqcc_outputs_pragmas_and_lists_reach_the_graph() {
    let graph = build(&GraphSpec {
        files: &[
            (
                "src/main.src",
                "#output \"../a.dat\"\n#pragma includedir inc\n#pragma sourcefile c.src\n\
                 #include \"x.qh\"\n#includelist\nl.qc\n",
            ),
            ("src/inc/x.qh", "#eof\n#include \"gone.qh\"\n"),
            ("src/l.qc", ""),
        ],
        src: "src/main.src",
        dialect: qc_lexer::Dialect::Fteqcc,
        ..GraphSpec::default()
    });
    assert_eq!(names(&graph), ["src/main.src", "src/inc/x.qh", "src/l.qc"]);
    assert_eq!(graph.declared_output, Some(PathBuf::from("a.dat")));
    assert_eq!(graph.sourcefiles, [PathBuf::from("src/c.src")]);
    assert_eq!(diags(&graph), ["E0406 src/main.src:5"]);
}

// spec PRJ-5.14: under FTEQCC an `#include` in the text `DONT_COMPILE_THIS_FILE` passes over
// brings in no file, and an `#includelist` there still compiles its entries
#[test]
fn fteqcc_text_that_is_not_compiled_includes_nothing() {
    let files = [
        (
            "t.qc",
            "#pragma DONT_COMPILE_THIS_FILE\n#include \"b.qh\"\n#includelist\nl.qc\n#endlist\n",
        ),
        ("b.qh", ""),
        ("l.qc", ""),
    ];
    assert_eq!(after_t(&files, qc_lexer::Dialect::Fteqcc), ["l.qc"]);
}

// spec FTEF-14.62: a module define that gives `FTEQCC` another body is FTEQCC's Q207, printed
// against the command line, which the graph does not report; the define holds
#[test]
fn a_define_of_fteqcc_s_own_macro_warns_nowhere() {
    let graph = build(&GraphSpec {
        files: &[
            ("progs.src", "#if FTEQCC == 1\n#include \"a.qc\"\n#endif\n"),
            ("a.qc", ""),
        ],
        defines: &["FTEQCC=1", "FTEQCC=1"],
        dialect: qc_lexer::Dialect::Fteqcc,
        ..GraphSpec::default()
    });
    assert!(graph.diagnostics.is_empty(), "{:?}", graph.diagnostics);
    assert_eq!(names(&graph), ["progs.src", "a.qc"]);
}
