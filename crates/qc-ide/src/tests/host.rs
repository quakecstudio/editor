// SPDX-License-Identifier: GPL-3.0-or-later

//! The analysis host: open files, files no module compiles, and project changes.

use std::path::Path;

use qc_project::MemoryLoader;

use super::{ProjectSpec, host, nth, one, path};
use crate::host::directive_lines;
use crate::{AnalysisHost, Config};

// spec QC-12.5: an open file's text replaces the text on disk until it is closed
#[test]
fn an_overlay_replaces_the_text_on_disk() {
    let mut host = one("float a;\n");
    let edited = "float b;\nvoid() f = { b = 1; };\n";
    host.set_overlay(&path("a.qc"), edited.as_bytes()).unwrap();
    assert_eq!(host.text(&path("a.qc")), Some(edited.as_bytes().to_vec()));
    assert!(
        host.definition(&path("a.qc"), nth(edited, "b", 1))
            .is_some()
    );
    host.remove_overlay(&path("a.qc")).unwrap();
    assert_eq!(host.text(&path("a.qc")), Some(b"float a;\n".to_vec()));
    assert_eq!(host.definition(&path("a.qc"), nth(edited, "b", 1)), None);
    // Closing a file that is not open changes nothing.
    host.remove_overlay(&path("a.qc")).unwrap();
}

// spec QC-12.5: a file no module compiles is analysed on its own while it is open
#[test]
fn an_open_file_no_module_compiles() {
    let mut host = one("");
    let text = "float x;\nvoid() f = { x = 1; };\n";
    host.set_overlay(&path("scratch.qc"), text.as_bytes())
        .unwrap();
    assert!(host.files().contains(&path("scratch.qc")));
    assert!(
        host.definition(&path("scratch.qc"), nth(text, "x", 1))
            .is_some()
    );
    host.remove_overlay(&path("scratch.qc")).unwrap();
    assert!(!host.files().contains(&path("scratch.qc")));
}

// spec PRJ-4.5: a module's source that cannot be read is an error; once the file is there,
// given as an editor's text, the module compiles it: it is no file on its own
#[test]
fn a_listed_file_made_later_joins_its_module() {
    let mut host = host(&ProjectSpec {
        files: &[("a.qc", "float a;\n")],
        extra: &[("progs.src", "../progs.dat\na.qc\nnew.qc\n")],
        ..ProjectSpec::default()
    });
    assert_eq!(
        host.project_modules()[0].names,
        ["a.qc"],
        "new.qc cannot be read yet"
    );
    host.set_overlay(
        &path("new.qc"),
        b"float b;\nvoid() f = { b = a; };\n".as_ref(),
    )
    .unwrap();
    let modules = host.project_modules();
    assert_eq!(modules.len(), 1);
    assert_eq!(modules[0].names, ["a.qc", "new.qc"]);
    // It is read in its module, after a.qc, whose name it uses.
    assert!(host.diagnostics(&path("new.qc")).is_empty());
    // A file no project file names is still analysed on its own.
    host.set_overlay(&path("loose.qc"), b"float c;\n".as_ref())
        .unwrap();
    assert_eq!(host.project_modules()[0].names, ["a.qc", "new.qc"]);
    assert!(host.files().contains(&path("loose.qc")));
}

// spec QC-12.5: only a file that may be QuakeC is analysed on its own: an open file of another
// kind, such as a script or a text, has no QuakeC problems
#[test]
fn an_open_file_that_is_no_quakec_is_not_analysed() {
    let mut host = one("");
    for name in ["tools/px.py", "notes.txt", "maps/room.map", "README"] {
        host.set_overlay(&path(name), b"import sys\n".as_ref())
            .unwrap();
        assert!(!host.files().contains(&path(name)), "{name}");
        assert!(host.diagnostics(&path(name)).is_empty(), "{name}");
        // Its text is still the editor's.
        assert_eq!(
            host.text(&path(name)),
            Some(b"import sys\n".to_vec()),
            "{name}"
        );
    }
    for name in ["scratch.qc", "defs.QH", "shared.h", "frames.inc"] {
        host.set_overlay(&path(name), b"float x\n".as_ref())
            .unwrap();
        assert!(host.files().contains(&path(name)), "{name}");
    }
}

// spec PRJ-4.1: editing the progs.src changes which files the module compiles
#[test]
fn editing_the_progs_src_reads_the_project_again() {
    let mut host = host(&ProjectSpec {
        files: &[("a.qc", "float a;\n")],
        extra: &[("b.qc", "float b;\n")],
        ..ProjectSpec::default()
    });
    assert!(!host.files().contains(&path("b.qc")));
    host.set_overlay(&path("progs.src"), b"../progs.dat\na.qc\nb.qc\n".as_ref())
        .unwrap();
    assert!(host.files().contains(&path("b.qc")));
    assert_eq!(host.text(&path("missing.qc")), None);
    host.remove_overlay(&path("progs.src")).unwrap();
    assert!(!host.files().contains(&path("b.qc")));
}

// spec PRJ-5.1: typing an `#include` line in an open file reads the module's files again, so
// a name the included file declares resolves, and deleting the line makes it unknown again,
// with no save between
#[test]
fn typing_an_include_reads_the_modules_files_again() {
    let mut host = host(&ProjectSpec {
        files: &[("a.qc", "void() f = { x = 1; };\n")],
        extra: &[("b.qh", "float x;\n")],
        ..ProjectSpec::default()
    });
    let plain = "void() f = { x = 1; };\n";
    let at = |text: &str| nth(text, "x", 0);
    assert_eq!(host.definition(&path("a.qc"), at(plain)), None);
    let included = "#include \"b.qh\"\nvoid() f = { x = 1; };\n";
    host.set_overlay(&path("a.qc"), included.as_bytes())
        .unwrap();
    assert!(host.files().contains(&path("b.qh")));
    assert!(host.definition(&path("a.qc"), at(included)).is_some());
    // An edit that leaves the directive lines alone keeps the graph.
    let renamed = "#include \"b.qh\"\nvoid() g = { x = 1; };\n";
    host.set_overlay(&path("a.qc"), renamed.as_bytes()).unwrap();
    assert!(host.definition(&path("a.qc"), at(renamed)).is_some());
    host.set_overlay(&path("a.qc"), plain.as_bytes()).unwrap();
    assert!(!host.files().contains(&path("b.qh")));
    assert_eq!(host.definition(&path("a.qc"), at(plain)), None);
    // Closing the file goes back to its text on disk, and the graph with it.
    host.set_overlay(&path("a.qc"), included.as_bytes())
        .unwrap();
    host.remove_overlay(&path("a.qc")).unwrap();
    assert!(!host.files().contains(&path("b.qh")));
}

// spec PRJ-1.2: a directory with no project is no error, and a bad project file is
#[test]
fn opening_a_directory() {
    let mut empty = AnalysisHost::new(MemoryLoader::new(), Config::default());
    empty.open(Path::new("/p")).unwrap();
    assert!(empty.files().is_empty());
    let mut loader = MemoryLoader::new();
    loader.insert(path("quakec.toml"), b"[project\n".to_vec());
    let mut bad = AnalysisHost::new(loader, Config::default());
    assert!(bad.open(Path::new("/p")).is_err());
    assert!(bad.files().is_empty());
    assert!(
        bad.set_overlay(&path("quakec.toml"), b"also bad [".as_ref())
            .is_err()
    );
}

// spec QC-12.5: reading the project again keeps a module whose files are unchanged, and reads
// again only the files that changed on disk
#[test]
fn reloading_keeps_unchanged_modules() {
    let mut host = one("float a;\nvoid() f = { a = 1; };\n");
    let before = host.diagnostics(&path("a.qc"));
    assert!(before.is_empty());
    let parses = host.modules[0].db.counts().parse;
    host.reload().unwrap();
    assert!(host.diagnostics(&path("a.qc")).is_empty());
    // The kept database parsed nothing again.
    assert_eq!(host.modules[0].db.counts().parse, parses);
}

// spec PRJ-3.2, QC-25.4: each project module is given to the build with its name, its output
// and each file named as its `progs.src` lists it; a file no module compiles is not built
#[test]
fn the_build_gets_each_project_module() {
    let mut host = host(&ProjectSpec {
        files: &[("a.qc", "float a;\n"), ("sub/b.qc", "float b;\n")],
        ..ProjectSpec::default()
    });
    host.set_overlay(&path("loose.qc"), b"float c;\n".as_ref())
        .unwrap();
    let modules = host.project_modules();
    assert_eq!(modules.len(), 1, "the loose file is not a module to build");
    let module = &modules[0];
    assert_eq!(module.name, "progs");
    assert_eq!(module.output, path("build/p/progs.dat"));
    assert_eq!(module.names, ["a.qc", "sub/b.qc"]);
    assert_eq!(module.input.compiled.len(), 2);
}

// spec PRJ-2.8: the editor's dialect reads a project with no `quakec.toml`; a project file's
// dialect, vanilla when it names none, wins over it
#[test]
fn a_project_files_dialect_wins_over_the_editors() {
    const FILES: &[(&str, &str)] = &[("a.qc", "void() f = { float x = 1; x += 2; };\n")];
    const NAMES_NONE: &[(&str, &str)] = &[(
        "quakec.toml",
        "[[module]]\nkind = \"ssqc\"\nsrc = \"progs.src\"\n",
    )];
    let errors = |extra: &'static [(&'static str, &'static str)]| {
        let mut host = host(&ProjectSpec {
            files: FILES,
            extra,
            dialect: Some(qc_project::Dialect::Fteqcc),
            ..ProjectSpec::default()
        });
        host.diagnostics(&path("a.qc"))
            .iter()
            .filter(|d| d.is_error())
            .count()
    };
    assert_eq!(errors(&[]), 0, "a bare project takes the editor's");
    assert!(errors(NAMES_NONE) > 0, "the project file's default wins");
}

// spec PRJ-2.9: the editor's target is a bare project's; a project file's target, id's engine
// when it names none, wins over it, so a builtin id's engine lacks is still W0311 there
#[test]
fn a_project_files_target_wins_over_the_editors() {
    const FILES: &[(&str, &str)] = &[(
        "a.qc",
        "void() print_it = #0:ex_bprint;\nvoid() f = { print_it(); };\n",
    )];
    const NAMES_NO_TARGET: &[(&str, &str)] = &[(
        "quakec.toml",
        "[project]\ndialect = \"fteqcc\"\n\n[[module]]\nkind = \"ssqc\"\nsrc = \"progs.src\"\n",
    )];
    let w0311 = |extra: &'static [(&'static str, &'static str)]| {
        let mut host = host(&ProjectSpec {
            files: FILES,
            extra,
            dialect: Some(qc_project::Dialect::Fteqcc),
            target: Some(qc_project::Target::Rerelease),
        });
        host.diagnostics(&path("a.qc"))
            .iter()
            .filter(|d| d.code == qc_diag::Code::W0311)
            .count()
    };
    assert_eq!(w0311(&[]), 0, "a bare project takes the editor's target");
    assert_eq!(w0311(NAMES_NO_TARGET), 1, "the project file's default wins");
}

// spec PRJ-5.1: the lines that may be directives are read after a byte order mark, with a `\`
// joining the next line; PRJ-5.3: every line of an `#includelist` names files
#[test]
fn the_lines_an_edit_rereads_the_graph_for() {
    let lines = |text: &str| directive_lines(text.as_bytes());
    assert_eq!(
        lines("\u{feff}#include \"b.qh\"\nfloat x;\n")[0],
        b"#include \"b.qh\""
    );
    assert_ne!(
        lines("#include \\\n\"b.qh\"\n"),
        lines("#include \\\n\"c.qh\"\n")
    );
    assert_ne!(
        lines("#includelist\na.qc\n#endlist\n"),
        lines("#includelist\na.qc\nb.qc\n#endlist\n")
    );
    assert_eq!(
        lines("float a;\n#define X\n"),
        lines("float b;\n#define X\n")
    );
}

// spec PRJ-5.3: typing an entry in an open file's `#includelist` adds the file to the module
#[test]
fn typing_an_includelist_entry_reads_the_modules_files_again() {
    let mut host = host(&ProjectSpec {
        files: &[("a.qc", "#includelist\n#endlist\n")],
        extra: &[("b.qc", "float y;\n")],
        dialect: Some(qc_project::Dialect::Fteqcc),
        ..ProjectSpec::default()
    });
    assert!(!host.files().contains(&path("b.qc")));
    host.set_overlay(&path("a.qc"), b"#includelist\nb.qc\n#endlist\n".as_ref())
        .unwrap();
    assert!(host.files().contains(&path("b.qc")));
}

// spec QC-12.5: a file made while the project file does not open is analysed on its own, as
// any file no module compiles
#[test]
fn a_file_made_while_the_project_does_not_open_is_analysed() {
    let mut host = host(&ProjectSpec {
        files: &[("a.qc", "float x;\n")],
        ..ProjectSpec::default()
    });
    assert!(
        host.set_overlay(&path("quakec.toml"), b"bad [".as_ref())
            .is_err()
    );
    let text = "float y;\nvoid() g = { y = 1; };\n";
    host.set_overlay(&path("scratch.qc"), text.as_bytes())
        .unwrap();
    assert!(host.files().contains(&path("scratch.qc")));
    assert!(
        host.definition(&path("scratch.qc"), nth(text, "y", 1))
            .is_some()
    );
}

// spec PRJ-1.2: an edit to a QuakeC file while the project file does not open keeps the
// analysis, and is no error
#[test]
fn a_directive_edit_while_the_project_does_not_open_keeps_the_analysis() {
    let mut host = host(&ProjectSpec {
        files: &[("a.qc", "float x;\nvoid() f = { x = 1; };\n")],
        extra: &[("b.qh", "float z;\n")],
        ..ProjectSpec::default()
    });
    assert!(
        host.set_overlay(&path("quakec.toml"), b"bad [".as_ref())
            .is_err()
    );
    let text = "#include \"b.qh\"\nfloat x;\nvoid() f = { x = 1; };\n";
    assert!(host.set_overlay(&path("a.qc"), text.as_bytes()).is_ok());
}
