// SPDX-License-Identifier: GPL-3.0-or-later

//! The two forms of `progs.src` and the shape of the graph (PRJ-4, PRJ-6, QC-23).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use qc_lexer::Dialect;

use super::{GraphSpec, build, diags, names, source, vias};
use crate::{Form, MemoryLoader, PRJ_MAX_DEPTH, PRJ_MAX_FILES, Via, build_graph};

// spec QC-23.1: the first token names the output, each later one a source file
// spec PRJ-4.3: in list form the output and the files are resolved against the progs.src's directory
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
// spec PRJ-4.3: a list with no token is E0401
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
// so a wide one is not decoded and is no source form
#[test]
fn a_vanilla_progs_src_is_its_bytes() {
    let vanilla = |bytes: &[u8]| loaded_graph(bytes, Dialect::Vanilla);
    let graph = vanilla(b"\xef\xbb\xbfprogs.dat\na.qc\n");
    assert_eq!(names(&graph), ["a.qc"]);
    for bytes in wide("#include \"b.qc\"\n") {
        assert_eq!(vanilla(&bytes).form, Some(Form::List), "{bytes:?}");
    }
}

// spec QC-23.7: id's qcc ends the list at the first NUL between words or that ends a word or a
// `//` comment, and reads on past one in quotes, the name ending at it
// spec PRJ-4.7: under the vanilla dialect the graph tells the form of, and reads the list of, a
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
        (b"progs.dat\n\"a.qc\0\nb.qc\0a.qc\n", &["a.qc", "b.qc"]),
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
    // The source form is told from the text before the NUL, and then read as its bytes.
    let graph = vanilla(b"#include \"a.qc\"\0\n#include \"b.qc\"\n");
    assert_eq!(graph.form, Some(Form::Source));
    // Under FTEQCC the NUL is a space (PRJ-4.6).
    assert_eq!(
        names(&loaded_graph(b"progs.dat\0a.qc\n", Dialect::Fteqcc)),
        ["a.qc"]
    );
}

// spec PRJ-4.8: under FTEQCC and GMQCC a `#!` first line of a `progs.src` is passed over before
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
        // A source of that line alone is a list with no token.
        for bytes in [&b"#!progs.dat a.qc"[..], b"#!x\n", b"#!x\n\n// c\n"] {
            let graph = fte(bytes);
            assert_eq!(graph.form, Some(Form::List), "{bytes:?}");
            assert_eq!(diags(&graph), ["E0401 progs.src:0"], "{bytes:?}");
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
