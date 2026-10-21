// SPDX-License-Identifier: GPL-3.0-or-later

//! A module read through the preprocessor (`Db::analyze_module`, P2-32): each file is parsed
//! and resolved from its expansion, and every location is a span of a source file.

use std::collections::HashMap;
use std::path::PathBuf;

use qc_diag::{Code, Span};
use qc_project::{FileGraph, Form, GraphFile, Via};
use qc_syntax::Dialect;

use crate::db::{Db, ModuleAnalysis};
use crate::expand::{Files, ModuleInput, OriginMap, preprocess};
use crate::lower::lower_file;
use crate::symbol::{FileId, Location};

/// A module of named files: every file is put in a database under its name, and the ones
/// named in `compiled` are compiled in that order. Returns the database and the input.
fn module(files: &[(&str, &str)], compiled: &[&str], dialect: Dialect) -> (Db, ModuleInput) {
    let mut db = Db::new();
    db.set_dialect(dialect);
    let mut input = ModuleInput::default();
    for (index, (name, text)) in files.iter().enumerate() {
        db.set_file(FileId(index), PathBuf::from(name), text.as_bytes().to_vec());
    }
    for name in compiled {
        let index = files.iter().position(|(n, _)| n == name).unwrap();
        input.compiled.push(FileId(index));
    }
    (db, input)
}

/// The codes of every diagnostic of an analysis.
fn codes(analysis: &ModuleAnalysis) -> Vec<Code> {
    analysis.diagnostics().map(|d| d.diagnostic.code).collect()
}

/// The text a location covers in the database.
fn text_at(db: &Db, at: Location) -> String {
    String::from_utf8_lossy(&db.file_text(at.file)[at.span.range()]).into_owned()
}

const DEFS: &str = "float coop;\nfloat x;\n#define INHIBIT_COOP if (coop) return;\n";
const USES: &str = "void() f =\n{\n\tINHIBIT_COOP\n\tx = 1;\n};\n";
const OTHER: &str = "void() g = { x = 2; };\n";

// spec QC-9.7: a file that uses a macro as a statement resolves from its expansion, and a
// name the macro body uses is a reference at the span of the macro use in the source.
#[test]
fn a_macro_used_as_a_statement_resolves_at_the_use() {
    let files = [("defs.qc", DEFS), ("uses.qc", USES)];
    let (mut db, input) = module(&files, &["defs.qc", "uses.qc"], Dialect::Fteqcc);
    let analysis = db.analyze_module(&input);
    assert!(codes(&analysis).is_empty(), "{:?}", codes(&analysis));
    let index = &analysis.index;
    let coop = index.defs.iter().position(|d| d.name == "coop").unwrap();
    let uses: Vec<_> = index
        .refs
        .iter()
        .filter(|r| r.def.0 == coop)
        .map(|r| r.location)
        .collect();
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].file, FileId(1));
    assert_eq!(text_at(&db, uses[0]), "INHIBIT_COOP");
    // An ordinary token is at its own span.
    let x = index.defs.iter().position(|d| d.name == "x").unwrap();
    let x_use = index.refs.iter().find(|r| r.def.0 == x).unwrap().location;
    assert_eq!(x_use.file, FileId(1));
    assert_eq!(x_use.span.start, USES.find("x = 1").unwrap());
    assert_eq!(text_at(&db, x_use), "x");
}

// spec QC-9.7: the same files resolved as written, with no preprocessor, have errors, so
// reading the expansion is what makes them resolve.
#[test]
fn the_files_as_written_do_not_resolve() {
    let files = [
        ("defs.qc", DEFS),
        ("uses.qc", "void() f = { INHIBIT_COOP; };\n"),
    ];
    let (mut db, input) = module(&files, &["defs.qc", "uses.qc"], Dialect::Fteqcc);
    let c = codes(&db.analyze_module(&input));
    assert!(!c.iter().any(|code| code.is_error()), "{c:?}");
    // As written, uses.qc calls `INHIBIT_COOP`, which only the macro defines, so resolving
    // the files without the preprocessor reports the name as unknown.
    let ids = [FileId(0), FileId(1)];
    let index = db.module_index(&ids);
    let unknown: Vec<String> = index
        .diagnostics
        .iter()
        .filter(|d| d.diagnostic.code.is_error() && d.file == FileId(1))
        .map(|d| d.diagnostic.message.clone())
        .collect();
    assert!(
        unknown.iter().any(|m| m.contains("INHIBIT_COOP")),
        "{unknown:?}"
    );
}

// spec QC-9.7: a semantic diagnostic on a name from a macro body is at the macro use.
#[test]
fn a_diagnostic_from_a_macro_body_is_at_the_use() {
    let files = [
        ("defs.qc", "#define BAD missing = 1;\n"),
        ("uses.qc", "void() f = { BAD };\n"),
    ];
    let (mut db, input) = module(&files, &["defs.qc", "uses.qc"], Dialect::Fteqcc);
    let analysis = db.analyze_module(&input);
    let unknown: Vec<_> = analysis
        .diagnostics()
        .filter(|d| d.diagnostic.code == Code::E0301)
        .collect();
    assert_eq!(unknown.len(), 1);
    let at = Location {
        file: unknown[0].file,
        span: unknown[0].diagnostic.span,
    };
    assert_eq!(at.file, FileId(1));
    assert_eq!(text_at(&db, at), "BAD");
}

// spec QC-9.7: a parse error in a file's expansion is reported at the source span it came
// from, in the file it is in.
#[test]
fn a_parse_error_is_at_its_source_span() {
    let files = [("bad.qc", "float a;\nvoid() f = { a = ; };\n")];
    let (mut db, input) = module(&files, &["bad.qc"], Dialect::Fteqcc);
    let analysis = db.analyze_module(&input);
    let errors: Vec<_> = analysis
        .syntax
        .iter()
        .filter(|d| d.diagnostic.code.is_error())
        .collect();
    assert!(!errors.is_empty());
    let source = files[0].1;
    for d in errors {
        assert_eq!(d.file, FileId(0));
        assert!(d.diagnostic.span.start >= source.find("a = ;").unwrap());
        assert!(d.diagnostic.span.end <= source.len());
    }
}

// spec QC-9.2: a definition in an included file is at its span in the included file, which
// the database finds by path; the include is not compiled on its own.
#[test]
fn an_included_definition_is_in_the_included_file() {
    let files = [
        (
            "main.qc",
            "#include \"inc.qh\"\nvoid() f = { common = 1; };\n",
        ),
        ("inc.qh", "float common;\n"),
    ];
    let (mut db, input) = module(&files, &["main.qc"], Dialect::Fteqcc);
    let analysis = db.analyze_module(&input);
    assert!(codes(&analysis).is_empty(), "{:?}", codes(&analysis));
    let common = analysis
        .index
        .defs
        .iter()
        .find(|d| d.name == "common")
        .unwrap();
    assert_eq!(common.location.file, FileId(1));
    assert_eq!(text_at(&db, common.location), "common");
}

// spec QC-9.11: an include the database has no file for is an error at the directive, in the
// file that holds it.
#[test]
fn an_include_the_database_lacks_is_an_error() {
    let files = [("main.qc", "#include \"gone.qh\"\nfloat a;\n")];
    let (mut db, input) = module(&files, &["main.qc"], Dialect::Fteqcc);
    let analysis = db.analyze_module(&input);
    let d = analysis
        .syntax
        .iter()
        .find(|d| d.diagnostic.code == Code::E0111)
        .unwrap();
    assert_eq!(d.file, FileId(0));
    assert_eq!(d.diagnostic.span.start, 0);
}

// spec QC-23.5: a compiled file the database has no path for, or whose path does not open,
// is "Error opening PATH" (QCD-5.2) and the module still resolves the rest.
#[test]
fn a_compiled_file_that_cannot_be_read_is_an_error() {
    let mut db = Db::new();
    db.set_file(FileId(0), PathBuf::from("a.qc"), b"float a;\n".to_vec());
    db.set_file(FileId(1), PathBuf::from("b.qc"), b"float b;\n".to_vec());
    // File 2 has no path, so it cannot be read.
    let input = ModuleInput {
        prelude: Vec::new(),
        compiled: vec![FileId(0), FileId(2), FileId(1)],
        autoproto: None,
        include_dirs: Vec::new(),
    };
    assert_eq!(
        db.file_path(FileId(1)),
        Some(PathBuf::from("b.qc").as_path())
    );
    assert_eq!(db.file_path(FileId(2)), None);
    let analysis = db.analyze_module(&input);
    assert_eq!(codes(&analysis), [Code::E0402]);
    assert_eq!(analysis.syntax[0].file, FileId(2));
    let names: Vec<_> = analysis
        .index
        .defs
        .iter()
        .map(|d| d.name.as_str())
        .collect();
    assert_eq!(names, ["a", "b"]);
}

// spec QC-9.6: the side of a conditional the build does not take is not parsed, so code in it
// is neither a definition nor an error.
#[test]
fn an_untaken_conditional_side_is_dropped() {
    let files = [(
        "a.qc",
        "#ifdef NOPE\nthis is not QuakeC\nfloat hidden;\n#else\nfloat shown;\n#endif\n",
    )];
    let (mut db, input) = module(&files, &["a.qc"], Dialect::Fteqcc);
    let analysis = db.analyze_module(&input);
    assert!(codes(&analysis).is_empty(), "{:?}", codes(&analysis));
    let names: Vec<_> = analysis
        .index
        .defs
        .iter()
        .map(|d| d.name.as_str())
        .collect();
    assert_eq!(names, ["shown"]);
}

// spec QC-9.8: the macros of the prelude, the source-form `progs.src` (PRJ-4.4), are defined
// for the compiled files, and the prelude's own tokens are not compiled.
#[test]
fn the_prelude_defines_macros_and_is_not_compiled() {
    let files = [
        ("progs.src", "#define FLAG\nfloat not_compiled;\n"),
        ("a.qc", "#ifdef FLAG\nfloat seen;\n#endif\n"),
    ];
    let (mut db, mut input) = module(&files, &["a.qc"], Dialect::Fteqcc);
    input.prelude.push(FileId(0));
    let analysis = db.analyze_module(&input);
    let names: Vec<_> = analysis
        .index
        .defs
        .iter()
        .map(|d| d.name.as_str())
        .collect();
    assert_eq!(names, ["seen"]);
}

// spec FTE-22.4: with autoprototyping the module is preprocessed twice, so a frame macro a
// later file defines is known to an earlier one.
#[test]
fn autoprototyping_preprocesses_twice() {
    let files = [
        ("a.qc", "float out;\nvoid() f = { out = $LATER; };\n"),
        ("b.qc", "$frame LATER\n"),
    ];
    let (mut db, input) = module(&files, &["a.qc", "b.qc"], Dialect::Fteqcc);
    let without = db.analyze_module(&input);
    assert!(codes(&without).contains(&Code::E0106));
    db.set_autoproto(true);
    let with = db.analyze_module(&input);
    // FTEF-18.29: the frame is the first pass's, so the use is stale, a warning.
    assert_eq!(codes(&with), [Code::W0105]);
}

/// The three-file module of the incremental tests, analysed once so every cache is full.
fn seeded() -> (Db, ModuleInput) {
    let files = [("defs.qc", DEFS), ("uses.qc", USES), ("other.qc", OTHER)];
    let (mut db, input) = module(&files, &["defs.qc", "uses.qc", "other.qc"], Dialect::Fteqcc);
    db.analyze_module(&input);
    (db, input)
}

// spec QC-9.7: editing a macro's definition re-parses and re-lowers the file that uses it
// and no other: the defining file's expansion and the other file's are unchanged.
#[test]
fn editing_a_macro_reparses_only_its_users() {
    let (mut db, input) = seeded();
    let before = db.counts();
    let edited = DEFS.replace("if (coop) return;", "if (coop) { return; }");
    db.set_file(FileId(0), PathBuf::from("defs.qc"), edited.into_bytes());
    let analysis = db.analyze_module(&input);
    let after = db.counts();
    assert!(codes(&analysis).is_empty(), "{:?}", codes(&analysis));
    assert_eq!(after.preprocess - before.preprocess, 1);
    assert_eq!(after.parse - before.parse, 1);
    assert_eq!(after.lower - before.lower, 1);
    assert_eq!(after.module_index - before.module_index, 1);
}

// spec QC-9.7: editing a file that defines no macro re-parses only that file.
#[test]
fn editing_a_plain_file_reparses_only_it() {
    let (mut db, input) = seeded();
    let before = db.counts();
    let edited = OTHER.replace("x = 2", "x = 3");
    db.set_file(FileId(2), PathBuf::from("other.qc"), edited.into_bytes());
    db.analyze_module(&input);
    let after = db.counts();
    assert_eq!(after.parse - before.parse, 1);
    assert_eq!(after.lower - before.lower, 1);
}

// spec QC-9.7: asking again with nothing changed runs nothing, and asking with another input
// preprocesses again.
#[test]
fn an_unchanged_module_is_served_from_cache() {
    let (mut db, input) = seeded();
    let before = db.counts();
    db.analyze_module(&input);
    assert_eq!(db.counts(), before);
    let fewer = ModuleInput {
        prelude: Vec::new(),
        compiled: input.compiled[..2].to_vec(),
        autoproto: None,
        include_dirs: Vec::new(),
    };
    db.analyze_module(&fewer);
    let after = db.counts();
    assert_eq!(after.preprocess - before.preprocess, 1);
    assert_eq!(after.parse, before.parse);
}

// spec QC-9.7: changing the dialect or autoprototyping re-analyses the module.
#[test]
fn a_profile_change_reanalyses() {
    let (mut db, input) = seeded();
    let before = db.counts();
    db.set_autoproto(true);
    db.analyze_module(&input);
    let after = db.counts();
    assert_eq!(after.preprocess - before.preprocess, 1);
    assert_eq!(after.parse, before.parse);
    db.set_dialect(Dialect::Vanilla);
    db.analyze_module(&input);
    assert_eq!(db.counts().parse - after.parse, 3);
}

// spec QC-9.7: a span of the rendered text maps to the source span of the token it starts in,
// through the token it ends in when both are in one file, and to nothing before any token.
#[test]
fn origin_maps_locate_spans() {
    let a = FileId(0);
    let b = FileId(1);
    let map = OriginMap::from_origins(vec![
        (
            2,
            Location {
                file: a,
                span: Span::new(10, 13),
            },
        ),
        (
            6,
            Location {
                file: a,
                span: Span::new(20, 21),
            },
        ),
        (
            8,
            Location {
                file: b,
                span: Span::new(0, 4),
            },
        ),
    ]);
    assert_eq!(map.locate(Span::new(0, 1)), None);
    assert_eq!(OriginMap::default().locate(Span::new(0, 0)), None);
    let one = map.locate(Span::new(2, 5)).unwrap();
    assert_eq!((one.file, one.span), (a, Span::new(10, 13)));
    let empty = map.locate(Span::new(3, 3)).unwrap();
    assert_eq!(empty.span, Span::new(10, 13));
    let two = map.locate(Span::new(2, 7)).unwrap();
    assert_eq!(two.span, Span::new(10, 21));
    let across = map.locate(Span::new(6, 9)).unwrap();
    assert_eq!((across.file, across.span), (a, Span::new(20, 21)));
}

// spec QC-9.7: a span that ends in an earlier source position than it starts (a macro use
// before the macro's own tokens) keeps the first token's origin.
#[test]
fn origin_maps_keep_the_first_origin_out_of_order() {
    let a = FileId(0);
    let map = OriginMap::from_origins(vec![
        (
            0,
            Location {
                file: a,
                span: Span::new(30, 32),
            },
        ),
        (
            3,
            Location {
                file: a,
                span: Span::new(5, 6),
            },
        ),
    ]);
    let at = map.locate(Span::new(0, 4)).unwrap();
    assert_eq!(at.span, Span::new(30, 32));
}

// spec PRJ-5.3: a graph's files are compiled in order, except an include, which the
// preprocessor reads where it is included, and a source-form root, which is the prelude.
#[test]
fn module_input_from_a_graph() {
    let file = |path: &str, via| GraphFile {
        path: PathBuf::from(path),
        via,
        parent: None,
    };
    let mut graph = FileGraph {
        form: Some(Form::Source),
        files: vec![
            file("progs.src", Via::Root),
            file("a.qc", Via::IncludeList),
            file("a.qh", Via::Include),
            file("b.qc", Via::IncludeList),
        ],
        ..FileGraph::default()
    };
    let input = ModuleInput::from_graph(&graph);
    assert_eq!(input.prelude, [FileId(0)]);
    assert_eq!(input.compiled, [FileId(1), FileId(3)]);
    graph.form = Some(Form::List);
    graph.files = vec![file("defs.qc", Via::Listed), file("world.qc", Via::Listed)];
    let input = ModuleInput::from_graph(&graph);
    assert!(input.prelude.is_empty());
    assert_eq!(input.compiled, [FileId(0), FileId(1)]);
    graph.files = vec![file("main.qc", Via::Root)];
    assert_eq!(ModuleInput::from_graph(&graph).compiled, [FileId(0)]);
}

// spec FTEF-12.10: the `#pragma autoproto` setting the graph read from a list-form
// `progs.src` is the module input's.
#[test]
fn module_input_keeps_the_graph_autoproto() {
    for autoproto in [None, Some(true), Some(false)] {
        let graph = FileGraph {
            autoproto,
            ..FileGraph::default()
        };
        assert_eq!(ModuleInput::from_graph(&graph).autoproto, autoproto);
    }
}

/// The text the parser reads for the one-file module `text`: its tokens rendered after
/// preprocessing in `dialect`.
fn rendered(text: &str, dialect: Dialect) -> String {
    let path = PathBuf::from("a.qc");
    let files = Files {
        texts: HashMap::from([(path.clone(), text.as_bytes())]),
        ids: HashMap::from([(path.clone(), FileId(0))]),
        paths: HashMap::from([(FileId(0), path)]),
    };
    let input = ModuleInput {
        compiled: vec![FileId(0)],
        ..ModuleInput::default()
    };
    let out = preprocess(&files, &input, dialect, false);
    String::from_utf8(out.files[0].text.clone()).unwrap()
}

// spec QC-9.7: a file is rendered a line for each source line its tokens came from, with a
// token that starts its line indented kept indented and tokens that touch in the source kept
// touching, so the parser's line-start recovery (QCD-1.4) sees the source's lines.
#[test]
fn rendering_keeps_lines_and_indentation() {
    assert_eq!(
        rendered("float x;\n  float y;\nfloat z;\n", Dialect::Vanilla),
        "float x;\n float y;\nfloat z;"
    );
}

// spec QC-8.5: a frame macro reference is rendered as its frame number, and every other float
// literal as written.
#[test]
fn rendering_writes_a_frame_reference_as_its_number() {
    assert_eq!(
        rendered(
            "$frame stand walk\nfloat f = $walk;\nfloat g = 1.50;\n",
            Dialect::Vanilla
        ),
        "float f = 1 ;\nfloat g = 1.50;"
    );
}

// spec QC-9.16: an expansion keeps its macro body's spacing; under FTEQCC an argument touches
// the body token its parameter touches; tokens from two macros, or a macro's and the text
// around it, stay apart
#[test]
fn rendering_keeps_a_macro_body_s_spacing() {
    for (text, want) in [
        (
            "#define LABEL done:\nvoid() f = { LABEL };\nvoid() g = { done: };\n",
            "void() f = { done: };\nvoid() g = { done: };",
        ),
        ("#define B #5\nvoid() f = B;\n", "void() f = #5 ;"),
        ("#define B #  5\nvoid() f = B;\n", "void() f = # 5 ;"),
        (
            "#define M(a) a:\nvoid() f = { M(lbl) };\n",
            "void() f = { lbl: };",
        ),
        (
            "#define M(a) a :\nvoid() f = { M(lbl) };\n",
            "void() f = { lbl : };",
        ),
        (
            "#define M lbl/**/:\nvoid() f = { M };\n",
            "void() f = { lbl: };",
        ),
        ("#define M(a) #a\nvoid() f = M(5);\n", "void() f = \"5\" ;"),
        ("#define M(a) -a\nvoid() f = #M(5);\n", "void() f = -5 ;"),
        ("#define M(a) a\nvoid() f = M(#5);\n", "void() f = #5 ;"),
        (
            "#define H #\n#define F 5\nvoid() f = H F;\n",
            "void() f = # 5 ;",
        ),
        ("#define B #\nvoid() f = B5;\n", "void() f = B5;"),
    ] {
        assert_eq!(rendered(text, Dialect::Fteqcc), want, "{text:?}");
    }
}

// spec QC-9.7: the declarations of a compiled file are those of its expansion, kept from the
// last analysis, and there are none before one
#[test]
fn the_expansion_s_declarations_are_kept() {
    let files = [("a.qc", "#define D float x;\nD\n")];
    let (mut db, input) = module(&files, &["a.qc"], Dialect::Fteqcc);
    assert!(db.expanded_decls(FileId(0)).is_none());
    db.analyze_module(&input);
    let names: Vec<_> = db
        .expanded_decls(FileId(0))
        .unwrap()
        .decls
        .iter()
        .map(|decl| decl.name.clone())
        .collect();
    assert_eq!(names, ["x"]);
    assert!(db.expanded_decls(FileId(1)).is_none());
}

// spec QC-9.16: a builtin initialiser or a label a macro makes reads as the same text written
// in the file would: the FTEQCC `#` before its number (FTE-23.1), a `-` before a digit
// (FTE-23.4), a label's `:` (FTEF-18.23); a body that writes them apart is read apart
#[test]
fn rules_on_touching_tokens_read_a_macro_s_tokens_as_written() {
    let binding = |text: &str| {
        let rendered = rendered(text, Dialect::Fteqcc);
        let lines = qc_syntax::DirectiveLines::ToLineBreak;
        let parse = qc_syntax::parse_with_lines(rendered.as_bytes(), Dialect::Fteqcc, lines);
        let errors: Vec<Code> = parse.errors().map(|d| d.code).collect();
        let function = lower_file(&parse, Dialect::Fteqcc)
            .decls
            .into_iter()
            .find(|decl| decl.name == "f")
            .and_then(|decl| decl.literal)
            .and_then(|literal| literal.builtin_function("f"))
            .map(|function| (function.number, function.name));
        (errors, function)
    };
    let bound = |number: i32, name: &str| (vec![], Some((number, name.to_owned())));
    assert_eq!(binding("#define B #5\nvoid() f = B;\n"), bound(5, "f"));
    assert_eq!(binding("#define B #-5\nvoid() f = B;\n"), bound(-5, "f"));
    assert_eq!(
        binding("#define B #\"ex_a\"\nvoid() f = B;\n"),
        bound(0, "ex_a")
    );
    assert_eq!(
        binding("#define B #5:ex_a\nvoid() f = B;\n"),
        bound(5, "ex_a")
    );
    assert_eq!(binding("#define B # 5\nvoid() f = B;\n").0, [Code::E0209]);
    assert_eq!(binding("#define B #- 5\nvoid() f = B;\n").0, [Code::E0209]);
    let files = [("a.qc", "#define L lbl:\nvoid() f = { L goto lbl; };\n")];
    let (mut db, input) = module(&files, &["a.qc"], Dialect::Fteqcc);
    let analysis = db.analyze_module(&input);
    assert!(
        analysis.diagnostics().next().is_none(),
        "{:?}",
        codes(&analysis)
    );
}

// spec QC-9.16: through `Db::analyze_module` under FTEQCC, `#define L(x) x:` then `L(lbl)` used
// as a statement is a label (FTEF-18.23), as are `L(lbl )`, a variadic `V(lbl)` and a body
// with a comment before its `:`; with the `:` apart in the body, after `L(lbl/**/)`, after
// `V(lbl )` or with an empty expansion before the `:` it is not
#[test]
fn an_argument_against_a_body_token_reads_as_written_there() {
    let errors = |text: &str| {
        let files = [("a.qc", text)];
        let (mut db, input) = module(&files, &["a.qc"], Dialect::Fteqcc);
        codes(&db.analyze_module(&input))
    };
    let label = |define: &str, use_: &str| {
        errors(&format!("{define}\nvoid() f = {{ {use_} goto lbl; }};\n"))
    };
    for (define, use_) in [
        ("#define L(x) x:", "L(lbl)"),
        ("#define L(x) x:", "L(lbl )"),
        ("#define L(x) x/**/:", "L(lbl)"),
        ("#define L(x,y) x y:", "L(,lbl)"),
        ("#define V(...) __VA_ARGS__:", "V(lbl)"),
    ] {
        assert_eq!(label(define, use_), [], "{define} {use_}");
    }
    for (define, use_) in [
        ("#define L(x) x :", "L(lbl)"),
        ("#define L(x) x:", "L(lbl/**/)"),
        ("#define L(x,y) x y:", "L(lbl,)"),
        ("#define V(...) __VA_ARGS__:", "V(lbl )"),
        ("#define E\n#define L lbl E:", "L"),
        ("#define F(a) a\n#define L lbl F():", "L"),
    ] {
        assert!(
            label(define, use_).contains(&Code::E0301),
            "{define} {use_}"
        );
    }
}

// spec FTEF-14.13: through `Db::analyze_module` under FTEQCC a macro's name followed by `!`
// stays a name, "Unknown value", in the source, in a body and after an argument; followed by
// `=`, white space or the end of an expansion it expands
#[test]
fn a_macro_followed_by_a_byte_that_does_not_end_its_word_is_unknown() {
    let errors = |define: &str, use_: &str| {
        let text = format!("#define N 5\n{define}\nvoid() g = {{ local float x; x = {use_}; }};\n");
        let files = [("a.qc", text.as_str())];
        let (mut db, input) = module(&files, &["a.qc"], Dialect::Fteqcc);
        codes(&db.analyze_module(&input))
    };
    for (define, use_) in [
        ("", "N!=5"),
        ("#define M N!=5", "M"),
        ("#define A(a) a!=5", "A(N)"),
        ("", "N\\\n"),
    ] {
        assert_eq!(errors(define, use_), [Code::E0301], "{define} {use_}");
    }
    for (define, use_) in [
        ("", "N==5"),
        ("", "N !=5"),
        ("#define M N !=5", "M"),
        ("#define A(a) a !=5", "A(N)"),
        ("#define M N", "M !=5"),
    ] {
        assert_eq!(errors(define, use_), [], "{define} {use_}");
    }
}

// spec FTEF-14.14: a name `##` makes is rendered as its own text, so it defines and resolves
// like any other.
#[test]
fn a_pasted_name_resolves() {
    let files = [(
        "a.qc",
        "#define NET(x) float x; float x ##_net;\nNET(health)\nvoid() f = { health_net = 1; };\n",
    )];
    let (mut db, input) = module(&files, &["a.qc"], Dialect::Fteqcc);
    let analysis = db.analyze_module(&input);
    assert!(codes(&analysis).is_empty(), "{:?}", codes(&analysis));
    let names: Vec<_> = analysis
        .index
        .defs
        .iter()
        .map(|d| d.name.as_str())
        .collect();
    assert!(names.contains(&"health_net"), "{names:?}");
}
