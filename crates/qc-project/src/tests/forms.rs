// SPDX-License-Identifier: GPL-3.0-or-later

//! The two forms of `progs.src` and the shape of the graph (PRJ-4, PRJ-6, QC-23).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use qc_lexer::Dialect;

use super::{GraphSpec, build, diags, names, source, vias};
use crate::{Form, MemoryLoader, PRJ_MAX_DEPTH, PRJ_MAX_FILES, Via, build_graph};

// spec QC-23.1: the first token names the output, each later one a source file
// spec PRJ-4.12: in list form the output and the files are resolved against the progs.src's directory
#[test]
fn list_form_names_output_then_files() {
    let graph = build(&GraphSpec {
        files: &[
            ("src/progs.src", "../progs.dat\ndefs.qc sub/b.qc\n"),
            ("src/defs.qc", ""),
            ("src/sub/b.qc", ""),
        ],
        src: "src/progs.src",
        ..GraphSpec::default()
    });
    assert_eq!(graph.form, Some(Form::List));
    assert_eq!(graph.declared_output, Some(PathBuf::from("progs.dat")));
    assert_eq!(names(&graph), ["src/defs.qc", "src/sub/b.qc"]);
    assert!(graph.diagnostics.is_empty());
}

// spec QC-23.2: whitespace separates tokens, // comments run to the end of the line, quotes group
// spec QC-23.2: each of { } ( ) ' : is a token of its own
#[test]
fn list_tokens_split_as_com_parse() {
    let graph = build(&GraphSpec {
        files: &[
            (
                "progs.src",
                "out.dat // the output\n\"a b.qc\"\tc.qc// trailing\n(d.qc)\n",
            ),
            ("a b.qc", ""),
            ("c.qc", ""),
            ("(", ""),
            ("d.qc", ""),
            (")", ""),
        ],
        ..GraphSpec::default()
    });
    assert_eq!(names(&graph), ["a b.qc", "c.qc", "(", "d.qc", ")"]);
}

// spec QC-23.3: files are compiled in the order listed, and a file listed twice is compiled twice
// spec PRJ-6.2: a file brought in more than once appears each time
#[test]
fn list_order_and_repeats() {
    let graph = build(&GraphSpec {
        files: &[
            ("progs.src", "x.dat b.qc a.qc b.qc"),
            ("a.qc", ""),
            ("b.qc", ""),
        ],
        ..GraphSpec::default()
    });
    assert_eq!(names(&graph), ["b.qc", "a.qc", "b.qc"]);
}

// spec QC-23.5: an empty progs.src is fatal "No destination filename.  qcc -help for info."
// spec QCD-5.1: E0401 when progs.src has no tokens
// spec PRJ-4.12: a list with no token is E0401
// spec PRJ-6.5: a diagnostic about the whole source is at line 0
#[test]
fn empty_progs_src_is_e0401() {
    for text in ["", "  \n\t\n", "// only a comment\n", "/* a\nblock */"] {
        let graph = build(&GraphSpec {
            files: &[("progs.src", text)],
            ..GraphSpec::default()
        });
        assert_eq!(diags(&graph), ["E0401 progs.src:0"], "{text:?}");
        assert_eq!(
            graph.diagnostics[0].message,
            "No destination filename.  qcc -help for info."
        );
        assert!(graph.files.is_empty());
        assert_eq!(graph.declared_output, None);
    }
}

// spec QC-23.5: a source file that cannot be opened is an error
// spec QCD-5.2: E0402 "Error opening PATH: REASON" for a listed file that cannot be read
// spec PRJ-6.5: the diagnostic names the line of the list entry
#[test]
fn missing_listed_file_is_e0402() {
    let graph = build(&GraphSpec {
        files: &[
            ("progs.src", "x.dat\na.qc\nmissing.qc\nb.qc\n"),
            ("a.qc", ""),
            ("b.qc", ""),
        ],
        ..GraphSpec::default()
    });
    assert_eq!(diags(&graph), ["E0402 progs.src:3"]);
    assert_eq!(
        graph.diagnostics[0].message,
        "Error opening missing.qc: no such file"
    );
    assert_eq!(names(&graph), ["a.qc", "b.qc"]);
}

// spec PRJ-4.5: a source that cannot be read is E0402 and the module has no files
// spec QCD-5.2: E0402 when progs.src itself cannot be read
#[test]
fn unreadable_source_is_e0402() {
    let graph = build(&GraphSpec {
        files: &[],
        ..GraphSpec::default()
    });
    assert_eq!(graph.form, None);
    assert!(graph.files.is_empty());
    assert_eq!(diags(&graph), ["E0402 progs.src:0"]);
    assert_eq!(
        graph.diagnostics[0].message,
        "Error opening progs.src: no such file"
    );
}

// spec QC-23.6: files in the directory that progs.src does not list are not part of the program
// spec PRJ-6.2: files nothing brings in are not part of the module
#[test]
fn unlisted_files_are_not_part_of_the_module() {
    let graph = build(&GraphSpec {
        files: &[("progs.src", "x.dat a.qc"), ("a.qc", ""), ("amtest.qc", "")],
        ..GraphSpec::default()
    });
    assert_eq!(names(&graph), ["a.qc"]);
}

// spec PRJ-4.1: source form when the first token starts with #, list form otherwise
// spec PRJ-4.1: comments before the first token are skipped, and the extension does not matter
#[test]
fn form_comes_from_the_first_token() {
    let cases = [
        ("#define X\n", Some(Form::Source)),
        ("  // note\n/* a\n b */ #pragma once\n", Some(Form::Source)),
        ("#includelist\n#endlist\n", Some(Form::Source)),
        ("../progs.dat\n#pragma autoproto\n", Some(Form::List)),
        ("/* # */ out.dat\n", Some(Form::List)),
    ];
    for (text, form) in cases {
        for src in ["progs.src", "main.qc", "x.inc"] {
            let graph = build(&GraphSpec {
                files: &[(src, text)],
                src,
                ..GraphSpec::default()
            });
            assert_eq!(graph.form, form, "{src}: {text:?}");
        }
    }
}

/// The graph of a module whose `progs.src` is `progs`, beside an empty `a.qc` and `b.qc`, in
/// `dialect`.
fn loaded_graph(progs: &[u8], dialect: Dialect) -> crate::FileGraph {
    let mut loader = MemoryLoader::new();
    loader.insert("progs.src", progs);
    loader.insert("a.qc", b"".as_slice());
    loader.insert("b.qc", b"".as_slice());
    build_graph(&mut loader, Path::new("progs.src"), &[], &[], dialect)
}

/// `text` as UTF-16LE, UTF-16BE, UTF-32LE and UTF-32BE, each after its byte order mark.
fn wide(text: &str) -> [Vec<u8>; 4] {
    let utf16 = || text.encode_utf16();
    let utf32 = || text.chars().map(u32::from);
    [
        [0xff, 0xfe]
            .into_iter()
            .chain(utf16().flat_map(u16::to_le_bytes))
            .collect(),
        [0xfe, 0xff]
            .into_iter()
            .chain(utf16().flat_map(u16::to_be_bytes))
            .collect(),
        [0xff, 0xfe, 0, 0]
            .into_iter()
            .chain(utf32().flat_map(u32::to_le_bytes))
            .collect(),
        [0, 0, 0xfe, 0xff]
            .into_iter()
            .chain(utf32().flat_map(u32::to_be_bytes))
            .collect(),
    ]
}

// spec PRJ-4.6: under FTEQCC a `progs.src` written as UTF-16 or UTF-32 after its mark is decoded
// before its form is told and its words are read
#[test]
fn an_fteqcc_progs_src_is_decoded() {
    for dialect in [Dialect::Fteqcc, Dialect::Gmqcc] {
        for bytes in wide("#include \"b.qc\"\n") {
            let graph = loaded_graph(&bytes, dialect);
            assert_eq!(graph.form, Some(Form::Source), "{bytes:?}");
            assert_eq!(names(&graph), ["progs.src", "b.qc"]);
            assert!(graph.diagnostics.is_empty(), "{:?}", graph.diagnostics);
        }
        for bytes in wide("progs.dat\na.qc b.qc\n") {
            let graph = loaded_graph(&bytes, dialect);
            assert_eq!(graph.form, Some(Form::List), "{bytes:?}");
            assert_eq!(graph.declared_output, Some(PathBuf::from("progs.dat")));
            assert_eq!(names(&graph), ["a.qc", "b.qc"]);
            assert!(graph.diagnostics.is_empty(), "{:?}", graph.diagnostics);
        }
    }
}

// spec PRJ-4.6: under FTEQCC each NUL byte of a `progs.src` is a space that splits two words, and
// one UTF-8 byte order mark is skipped
#[test]
fn an_fteqcc_progs_src_reads_a_nul_as_a_space() {
    let fte = |bytes: &[u8]| loaded_graph(bytes, Dialect::Fteqcc);
    let graph = fte(b"progs.dat\na.qc\0b.qc\n");
    assert_eq!(names(&graph), ["a.qc", "b.qc"]);
    assert!(graph.diagnostics.is_empty(), "{:?}", graph.diagnostics);
    let graph = fte(b"progs.dat\0a.qc\n");
    assert_eq!(graph.declared_output, Some(PathBuf::from("progs.dat")));
    assert_eq!(names(&graph), ["a.qc"]);
    let graph = fte(b"\0#include \"b.qc\"\n");
    assert_eq!(graph.form, Some(Form::Source));
    assert_eq!(names(&graph), ["progs.src", "b.qc"]);
    let graph = fte(b"\xef\xbb\xbfprogs.dat\na.qc\n");
    assert_eq!(graph.declared_output, Some(PathBuf::from("progs.dat")));
    assert_eq!(names(&graph), ["a.qc"]);
    let graph = fte(b"\xef\xbb\xbf#include \"b.qc\"\n");
    assert_eq!(graph.form, Some(Form::Source));
    // A second mark is not skipped: the first word is not `#`'s.
    let graph = fte(b"\xef\xbb\xbf\xef\xbb\xbf#include \"b.qc\"\n");
    assert_eq!(graph.form, Some(Form::List));
}

// spec PRJ-4.1: the vanilla dialect reads `progs.src` as its bytes past a UTF-8 byte order mark,
// so a wide one is not decoded: its words are its bytes as `COM_Parse` splits them
// spec QC-23.8: a byte of 0x80 or more is white space to `COM_Parse`, so a UTF-16LE mark is
// passed over and the first word is the `#` before the first NUL
#[test]
fn a_vanilla_progs_src_is_its_bytes() {
    let vanilla = |bytes: &[u8]| loaded_graph(bytes, Dialect::Vanilla);
    let graph = vanilla(b"\xef\xbb\xbfprogs.dat\na.qc\n");
    assert_eq!(names(&graph), ["a.qc"]);
    let [le16, be16, le32, be32] = wide("#include \"b.qc\"\n");
    assert_eq!(vanilla(&le16).form, Some(Form::Source));
    // The source form's text ends at its first NUL, so it brings in nothing.
    assert_eq!(names(&vanilla(&le16)), ["progs.src"]);
    for bytes in [be16, le32, be32] {
        let graph = vanilla(&bytes);
        assert_eq!(graph.form, Some(Form::List), "{bytes:?}");
        assert_eq!(diags(&graph), ["E0401 progs.src:0"], "{bytes:?}");
    }
}

// spec QC-23.7: id's qcc ends the list at the first NUL between words or that ends a word or a
// `//` comment, and reads on past one in quotes, the name ending at it
// spec PRJ-4.11: under the vanilla dialect the graph tells the form of, and reads the list of, a
// `progs.src` only to that NUL
#[test]
fn a_vanilla_progs_src_ends_at_a_nul() {
    let vanilla = |bytes: &[u8]| loaded_graph(bytes, Dialect::Vanilla);
    for (bytes, files) in [
        (&b"progs.dat\0a.qc\n"[..], &[][..]),
        (b"progs.dat\na.qc\0b.qc\n", &["a.qc"]),
        (b"progs.dat\na.qc\n\0b.qc\n", &["a.qc"]),
        (b"progs.dat\na.qc \0 b.qc\n", &["a.qc"]),
        (b"progs.dat\n// c\0\nb.qc\n", &[]),
        (b"progs.dat\n// \"\0\nb.qc\n", &[]),
        (b"progs.dat\n\"a.qc\0x\" b.qc\n", &["a.qc", "b.qc"]),
        (b"progs.dat\n\"a.qc\0\nb.qc\" b.qc\n", &["a.qc", "b.qc"]),
        (b"\xef\xbb\xbfprogs.dat\na.qc\0b.qc\n", &["a.qc"]),
    ] {
        let graph = vanilla(bytes);
        assert_eq!(graph.form, Some(Form::List), "{bytes:?}");
        assert_eq!(graph.declared_output, Some(PathBuf::from("progs.dat")));
        assert_eq!(names(&graph), files, "{bytes:?}");
        assert!(graph.diagnostics.is_empty(), "{bytes:?}");
    }
    // A word that ends at a `(` or holds a `"` is followed by the NUL that ends the list: each
    // names one file more, which is missing.
    for (bytes, missing) in [
        (&b"progs.dat\na.qc(\0b.qc\n"[..], "("),
        (b"progs.dat\na.qc a\"b\0\" b.qc\n", "a\"b"),
    ] {
        let graph = vanilla(bytes);
        assert_eq!(names(&graph), ["a.qc"], "{bytes:?}");
        let messages: Vec<&str> = graph
            .diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .collect();
        assert_eq!(messages.len(), 1, "{bytes:?}");
        assert!(messages[0].contains(missing), "{messages:?}");
    }
    // A source that starts with a NUL is a list with no token, `#include` after it unread.
    for bytes in [
        &b"\0progs.dat\na.qc\n"[..],
        b"\0#include \"b.qc\"\n",
        b"  \0",
    ] {
        let graph = vanilla(bytes);
        assert_eq!(graph.form, Some(Form::List), "{bytes:?}");
        assert_eq!(diags(&graph), ["E0401 progs.src:0"], "{bytes:?}");
    }
    // The source form is told from the text before the NUL, and read only to it, as the lexer
    // reads it (QC-1.1).
    let graph = vanilla(b"#include \"a.qc\"\0\n#include \"b.qc\"\n");
    assert_eq!(graph.form, Some(Form::Source));
    assert_eq!(names(&graph), ["progs.src", "a.qc"]);
    // Under FTEQCC the NUL is a space (PRJ-4.6).
    assert_eq!(
        names(&loaded_graph(b"progs.dat\0a.qc\n", Dialect::Fteqcc)),
        ["a.qc"]
    );
}

// spec QC-1.1: a vanilla file the graph reads ends at its first NUL, so a directive after it is
// not followed
// spec PRJ-5.16: under FTEQCC the NUL is a space and the directive is followed
#[test]
fn a_vanilla_file_ends_at_its_first_nul() {
    let graph = loaded_graph(b"#include \"a.qc\"\n", Dialect::Vanilla);
    assert_eq!(names(&graph), ["progs.src", "a.qc"]);
    let mut loader = MemoryLoader::new();
    loader.insert("progs.src", b"progs.dat\na.qc\n".as_slice());
    loader.insert("a.qc", b"float x;\0\n#include \"b.qc\"\n".as_slice());
    loader.insert("b.qc", b"float y;\n".as_slice());
    let graph = build_graph(
        &mut loader,
        Path::new("progs.src"),
        &[],
        &[],
        Dialect::Vanilla,
    );
    assert_eq!(names(&graph), ["a.qc"]);
    let graph = build_graph(
        &mut loader,
        Path::new("progs.src"),
        &[],
        &[],
        Dialect::Fteqcc,
    );
    assert_eq!(names(&graph), ["a.qc", "b.qc"]);
}

// spec QC-23.8: id's `COM_Parse` reads a byte of 0x80 or more as white space, between words and
// at a word's end
#[test]
fn a_vanilla_list_splits_at_a_high_byte() {
    let vanilla = |bytes: &[u8]| loaded_graph(bytes, Dialect::Vanilla);
    assert_eq!(
        names(&vanilla(b"progs.dat\na.qc\xe9b.qc\n")),
        ["a.qc", "b.qc"]
    );
    assert_eq!(
        names(&vanilla("progs.dat\n\u{e9}a.qc\u{e9}\n".as_bytes())),
        ["a.qc"]
    );
    assert_eq!(
        vanilla(b"progs.dat\xffa.qc\n").declared_output,
        Some(PathBuf::from("progs.dat"))
    );
    // A quoted word keeps its high bytes.
    let graph = vanilla(b"progs.dat\n\"a\xe9.qc\"\n");
    assert_eq!(diags(&graph).len(), 1);
    // FTEQCC reads a high byte as part of the word.
    let graph = loaded_graph(b"progs.dat\na.qc\xe9b.qc\n", Dialect::Fteqcc);
    assert_eq!(diags(&graph).len(), 1);
}

// spec QC-23.9: a quoted word of a vanilla list runs on to its closing quote across lines, and
// one the text ends in is error E0101 at the line it opened on, where id's qcc reads past its
// buffer
#[test]
fn a_vanilla_quoted_word_runs_across_lines() {
    let vanilla = |bytes: &[u8]| loaded_graph(bytes, Dialect::Vanilla);
    let graph = vanilla(b"progs.dat\n\"a.qc\nb.qc\" a.qc\n");
    assert_eq!(names(&graph), ["a.qc"]);
    let missing: Vec<&str> = graph
        .diagnostics
        .iter()
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(missing.len(), 1);
    assert!(missing[0].contains("a.qc\nb.qc"), "{missing:?}");
    // A CRLF line keeps its CR in the name.
    let graph = vanilla(b"progs.dat\r\n\"a.qc\r\nb.qc\" a.qc\r\n");
    let missing: Vec<&str> = graph
        .diagnostics
        .iter()
        .map(|d| d.message.as_str())
        .collect();
    assert!(missing[0].contains("a.qc\r\nb.qc"), "{missing:?}");
    // A `#` line inside the quote is part of the name, no directive.
    let graph = vanilla(b"progs.dat\n\"a.qc\n#include \"b.qc\"\n");
    assert!(names(&graph).is_empty());
    let graph = vanilla(b"progs.dat\na.qc \"b.qc\n\nx\n");
    assert_eq!(names(&graph), ["a.qc"]);
    assert_eq!(diags(&graph), ["E0101 progs.src:2"]);
}

// spec PRJ-4.10: under FTEQCC and GMQCC a `#!` first line of a `progs.src` is passed over before
// its form is told, in either form, after a UTF-8 mark and in a wide encoding, with whatever it
// holds; a blank or a NUL before it, or the vanilla dialect, leaves it to be read
#[test]
fn an_fteqcc_progs_src_passes_over_a_script_line() {
    for dialect in [Dialect::Fteqcc, Dialect::Gmqcc] {
        let fte = |bytes: &[u8]| loaded_graph(bytes, dialect);
        for first in [
            "#!/usr/bin/fteqcc",
            "#!x \\",
            "#!x /*",
            "#!x \"",
            "#!",
            "#!b.qc",
        ] {
            let list = format!("{first}\nprogs.dat\na.qc\n");
            let source = format!("{first}\n#include \"b.qc\"\n");
            let lists = [
                list.as_bytes().to_vec(),
                format!("\u{feff}{list}").into_bytes(),
                list.replace('\n', "\r\n").into_bytes(),
            ];
            for bytes in lists.into_iter().chain(wide(&list)) {
                let graph = fte(&bytes);
                assert_eq!(graph.form, Some(Form::List), "{bytes:?}");
                assert_eq!(graph.declared_output, Some(PathBuf::from("progs.dat")));
                assert_eq!(names(&graph), ["a.qc"], "{bytes:?}");
            }
            let sources = [
                source.as_bytes().to_vec(),
                format!("\u{feff}{source}").into_bytes(),
            ];
            for bytes in sources.into_iter().chain(wide(&source)) {
                let graph = fte(&bytes);
                assert_eq!(graph.form, Some(Form::Source), "{bytes:?}");
                assert_eq!(names(&graph), ["progs.src", "b.qc"], "{bytes:?}");
            }
        }
        // The `#include` keeps its line, the second.
        let graph = fte(b"#!x\n#include \"missing.qc\"\n");
        assert_eq!(graph.diagnostics.first().map(|d| d.line), Some(2));
        // A source of that line alone is a list with no word: no output, no file, no error.
        for bytes in [&b"#!progs.dat a.qc"[..], b"#!x\n", b"#!x\n\n// c\n"] {
            let graph = fte(bytes);
            assert_eq!(graph.form, Some(Form::List), "{bytes:?}");
            assert!(graph.diagnostics.is_empty(), "{bytes:?}");
            assert!(graph.files.is_empty() && graph.declared_output.is_none());
        }
        // Only at the very start: after a blank or a NUL the line's `#` makes the source form.
        for bytes in [&b" #!x\nprogs.dat\na.qc\n"[..], b"\0#!x\nprogs.dat\na.qc\n"] {
            let graph = fte(bytes);
            assert_eq!(graph.form, Some(Form::Source), "{bytes:?}");
            assert_eq!(names(&graph), ["progs.src"], "{bytes:?}");
        }
    }
    // PRJ-4.1: the vanilla dialect reads the line, whose first token starts with `#`.
    let graph = loaded_graph(b"#!x\nprogs.dat\na.qc\n", Dialect::Vanilla);
    assert_eq!(graph.form, Some(Form::Source));
    assert_eq!(names(&graph), ["progs.src"]);
}

// spec PRJ-4.2: in list form, /* */ comments are skipped and # lines are directives
#[test]
fn list_form_comments_and_directives() {
    let graph = build(&GraphSpec {
        files: &[
            (
                "progs.src",
                "out.dat\n#pragma autoproto\n/* skipped.qc\nalso.qc */ a.qc\n#ifdef NOPE\nb.qc\n#endif\n",
            ),
            ("a.qc", ""),
            ("b.qc", ""),
        ],
        ..GraphSpec::default()
    });
    assert_eq!(names(&graph), ["a.qc"]);
    assert!(graph.diagnostics.is_empty());
}

// spec PRJ-4.4: in source form the file is compiled first, and what it brings in follows in place
// spec PRJ-6.1: each entry records how it came in and the entry that brought it in
#[test]
fn source_form_root_comes_first() {
    let graph = source(
        "#include \"a.qc\"\nvoid() x = {};\n#includelist\nb.qc\n#endlist\n",
        &[("a.qc", "#include \"c.qc\"\n"), ("b.qc", ""), ("c.qc", "")],
    );
    assert_eq!(graph.form, Some(Form::Source));
    assert_eq!(
        vias(&graph),
        [
            ("main.src".to_owned(), Via::Root),
            ("a.qc".to_owned(), Via::Include),
            ("c.qc".to_owned(), Via::Include),
            ("b.qc".to_owned(), Via::IncludeList),
        ]
    );
    let parents: Vec<Option<usize>> = graph.files.iter().map(|f| f.parent).collect();
    assert_eq!(parents, [None, Some(0), Some(1), Some(0)]);
    assert_eq!(graph.declared_output, None);
}

// spec PRJ-6.1: list entries have no parent, and what they include points at them
#[test]
fn list_entries_have_no_parent() {
    let graph = build(&GraphSpec {
        files: &[
            ("progs.src", "x.dat a.qc b.qc"),
            ("a.qc", "#include \"inc.qh\"\n"),
            ("b.qc", ""),
            ("inc.qh", ""),
        ],
        ..GraphSpec::default()
    });
    let shape: Vec<(String, Via, Option<usize>)> = graph
        .files
        .iter()
        .map(|f| (super::slash(&f.path), f.via, f.parent))
        .collect();
    assert_eq!(
        shape,
        [
            ("a.qc".to_owned(), Via::Listed, None),
            ("inc.qh".to_owned(), Via::Include, Some(0)),
            ("b.qc".to_owned(), Via::Listed, None),
        ]
    );
}

// spec PRJ-6.3: a file that brings itself in is read again, until the depth limit
// spec QCD-5.8: E0408 when more than PRJ_MAX_DEPTH files would be read at once
#[test]
fn self_include_stops_at_the_depth_limit() {
    let graph = source(
        "#include \"loop.qh\"\n",
        &[("loop.qh", "#include \"loop.qh\"\n")],
    );
    assert_eq!(graph.files.len(), PRJ_MAX_DEPTH);
    assert_eq!(diags(&graph), ["E0408 loop.qh:1"]);
    assert_eq!(
        graph.diagnostics[0].message,
        "files nested too deeply to read loop.qh"
    );
}

// spec PRJ-6.3: a chain of distinct files, each bringing in the next by `#includelist`, is
// read to PRJ_MAX_DEPTH files at once and no further, however long it is
// spec QCD-5.8: the file past the limit is E0408, reported once
#[test]
fn reviewer_a_long_includelist_chain_stops_at_the_depth_limit() {
    let n = 5000;
    let names: Vec<String> = (0..=n).map(|i| format!("b{i}.qc")).collect();
    let texts: Vec<String> = (0..n)
        .map(|i| format!("#includelist\nb{}.qc\n#endlist\n", i + 1))
        .collect();
    let mut files: Vec<(&str, &str)> = names
        .iter()
        .zip(&texts)
        .map(|(name, text)| (name.as_str(), text.as_str()))
        .collect();
    files.push((names[n].as_str(), ""));
    let graph = source("#includelist\nb0.qc\n#endlist\n", &files);
    assert_eq!(graph.files.len(), PRJ_MAX_DEPTH);
    let last = PRJ_MAX_DEPTH - 2;
    assert_eq!(diags(&graph), [format!("E0408 b{last}.qc:2")]);
}

// spec PRJ-6.3: a file being read is read again when brought in, as FTEQCC and cpp do
#[test]
fn reentry_reads_the_file_again() {
    let graph = source(
        "#include \"mod.qh\"\n",
        &[
            ("mod.qh", "#include \"a.qh\"\n#include \"b.qh\"\n"),
            ("a.qh", "#pragma once\n#include \"mod.qh\"\n"),
            ("b.qh", "#pragma once\n"),
        ],
    );
    assert_eq!(
        names(&graph),
        ["main.src", "mod.qh", "a.qh", "mod.qh", "b.qh"]
    );
    assert!(graph.diagnostics.is_empty());
}

// spec PRJ-6.4: more than PRJ_MAX_FILES entries is E0407 and the graph stops
// spec QCD-5.7: E0407 "too many files in the project graph"
#[test]
fn too_many_files_is_e0407() {
    let mut list = String::from("x.dat\n");
    for _ in 0..PRJ_MAX_FILES {
        list.push_str("a.qc\n");
    }
    list.push_str("a.qc\nb.qc\n");
    let graph = build(&GraphSpec {
        files: &[("progs.src", &list), ("a.qc", ""), ("b.qc", "")],
        ..GraphSpec::default()
    });
    assert_eq!(graph.files.len(), PRJ_MAX_FILES);
    let line = u32::try_from(PRJ_MAX_FILES).unwrap() + 2;
    assert_eq!(diags(&graph), [format!("E0407 progs.src:{line}")]);
    assert_eq!(
        graph.diagnostics[0].message,
        "too many files in the project graph"
    );
}

// spec PRJ-6.4: exactly PRJ_MAX_FILES entries is no error
#[test]
fn exactly_max_files_is_fine() {
    let mut list = String::from("x.dat\n");
    for _ in 0..PRJ_MAX_FILES {
        writeln!(list, "a.qc").unwrap();
    }
    let graph = build(&GraphSpec {
        files: &[("progs.src", &list), ("a.qc", "")],
        ..GraphSpec::default()
    });
    assert_eq!(graph.files.len(), PRJ_MAX_FILES);
    assert!(graph.diagnostics.is_empty());
}

// spec PRJ-6.5: diagnostics name the file and 1-based line of the directive that raised them
#[test]
fn diagnostics_name_file_and_line() {
    let graph = source(
        "\n\n#include \"inc.qh\"\n",
        &[("inc.qh", "// one\n#include \"gone.qh\"\n")],
    );
    assert_eq!(diags(&graph), ["E0111 inc.qh:2"]);
}

/// The graph of an FTEQCC module whose `progs.src` is `progs`, beside `a.qc` and `b.qc`, and
/// the messages of its diagnostics.
fn fte_list(progs: &[u8]) -> (crate::FileGraph, Vec<String>) {
    let graph = loaded_graph(progs, Dialect::Fteqcc);
    let messages = graph
        .diagnostics
        .iter()
        .map(|d| format!("{:?} {} @{}", d.code, d.message, d.line))
        .collect();
    (graph, messages)
}

// spec PRJ-4.9: FTEQCC reads a list's output with `QCC_PR_SimpleGetToken`, which stops at `,`,
// `;`, `(`, `)` and `]`, and each later word with `QCC_COM_Parse`, to which `,` is a word of its
// own and `"` ends a word; the pinned FTEQCC fails each of these at the file the word names
#[test]
fn an_fteqcc_list_reads_its_words_as_fteqcc_does() {
    let (graph, _) = fte_list(b",a.qc\n");
    assert!(graph.declared_output.is_none());
    assert_eq!(diags(&graph), ["E0402 progs.src:1"]);
    let (graph, messages) = fte_list(b"progs.dat,a.qc\n");
    assert_eq!(graph.declared_output, Some(PathBuf::from("progs.dat")));
    assert!(messages[0].contains("Error opening ,"), "{messages:?}");
    assert_eq!(names(&graph), ["a.qc"]);
    let (graph, messages) = fte_list(b"progs.dat;x\na.qc\n");
    assert!(messages[0].contains("Error opening ;x"), "{messages:?}");
    assert_eq!(names(&graph), ["a.qc"]);
    let (graph, _) = fte_list(b"progs.dat\n\"a.qc\" /* c */ b.qc a.qc\"b.qc\"\n");
    assert_eq!(names(&graph), ["a.qc", "b.qc", "a.qc", "b.qc"]);
    assert!(graph.diagnostics.is_empty());
    // A quoted word takes `\"`, `\\`, `\n`, `\r` and `\t`.
    let (_, messages) = fte_list(b"progs.dat\n\"a\\\".qc\"\n");
    assert!(messages[0].contains("Error opening a\".qc"), "{messages:?}");
    // A `//` comment runs to the line's end.
    let (graph, _) = fte_list(b"progs.dat\na.qc // b.qc\n");
    assert_eq!(names(&graph), ["a.qc"]);
}

// spec PRJ-4.9: the directives at the starts of an FTEQCC list's lines are run, as
// `QCC_PR_LexWhitespace(false)` before each word runs them
#[test]
fn an_fteqcc_list_runs_its_directives() {
    let (graph, _) = fte_list(b"progs.dat\n#define X\n#ifdef X\na.qc\n#else\nb.qc\n#endif\n");
    assert_eq!(names(&graph), ["a.qc"]);
    let (graph, _) = fte_list(b"progs.dat\n#include \"b.qc\"\na.qc\n");
    assert_eq!(names(&graph), ["b.qc", "a.qc"]);
}

// spec PRJ-4.9: an FTEQCC list with no word has no output and no file, and no error, as the
// pinned FTEQCC writes nothing ("nothing to write") and exits 0
#[test]
fn an_empty_fteqcc_list_is_no_error() {
    for progs in [&b""[..], b"\n", b"// c\n", b"/* c */\n"] {
        let (graph, messages) = fte_list(progs);
        assert_eq!(graph.form, Some(Form::List), "{progs:?}");
        assert!(messages.is_empty() && graph.files.is_empty(), "{progs:?}");
        assert!(graph.declared_output.is_none(), "{progs:?}");
    }
    // The vanilla dialect keeps id's error.
    let graph = loaded_graph(b"", Dialect::Vanilla);
    assert_eq!(diags(&graph), ["E0401 progs.src:0"]);
}

// spec FTEF-14.67: a `/* */` comment an FTEQCC list ends in is "EOF inside comment" at its line
// spec FTEF-14.66: an output past 1023 bytes is FTEQCC's "token exceeds 1023 chars"
#[test]
fn an_fteqcc_list_s_errors() {
    let (graph, messages) = fte_list(b"progs.dat\na.qc /* open\n\n");
    assert_eq!(names(&graph), ["a.qc"]);
    assert_eq!(messages, ["E0501 EOF inside comment @2"]);
    let long = format!("{}\na.qc\n", "p".repeat(1024));
    let (graph, messages) = fte_list(long.as_bytes());
    assert_eq!(messages, ["E0121 token exceeds 1023 chars @1"]);
    assert!(graph.files.is_empty());
    let fits = format!("{}\na.qc\n", "p".repeat(1023));
    assert_eq!(names(&fte_list(fits.as_bytes()).0), ["a.qc"]);
    // A word `QCC_COM_Parse` reads keeps 1023 bytes, the rest dropped.
    let word = format!("progs.dat\n{}\n", "w".repeat(1030));
    let (_, messages) = fte_list(word.as_bytes());
    let opened = format!("Error opening {} ", "w".repeat(1023));
    assert!(messages[0].contains(opened.trim_end()), "{messages:?}");
    assert!(!messages[0].contains(&"w".repeat(1024)), "{messages:?}");
}

// spec PRJ-6.6: the graph reports each file it cannot open and reads on, where both compilers
// stop at the first
#[test]
fn every_missing_file_is_reported() {
    for dialect in [Dialect::Vanilla, Dialect::Fteqcc] {
        let graph = loaded_graph(b"progs.dat\nx.qc\na.qc\ny.qc\nb.qc\n", dialect);
        assert_eq!(names(&graph), ["a.qc", "b.qc"], "{dialect:?}");
        assert_eq!(
            diags(&graph),
            ["E0402 progs.src:2", "E0402 progs.src:4"],
            "{dialect:?}"
        );
    }
}
