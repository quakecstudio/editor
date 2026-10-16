// SPDX-License-Identifier: GPL-3.0-or-later

//! The directives the graph follows (PRJ-5).

use std::path::PathBuf;

use super::{GraphSpec, brought, build, diags, names, source};

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
