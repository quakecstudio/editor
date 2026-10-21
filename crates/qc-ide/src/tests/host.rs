// SPDX-License-Identifier: GPL-3.0-or-later

//! The analysis host: open files, files no module compiles, and project changes.

use std::path::Path;

use qc_project::MemoryLoader;

use super::{ProjectSpec, host, nth, one, path};
use crate::{AnalysisHost, Config};

// spec QC-12.5: an open file's text replaces the text on disk until it is closed
#[test]
fn an_overlay_replaces_the_text_on_disk() {
    let mut host = one("float a;\n");
    let edited = "float b;\nvoid() f = { b = 1; };\n";
    host.set_overlay(&path("a.qc"), edited.as_bytes().to_vec())
        .unwrap();
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
    host.set_overlay(&path("scratch.qc"), text.as_bytes().to_vec())
        .unwrap();
    assert!(host.files().contains(&path("scratch.qc")));
    assert!(
        host.definition(&path("scratch.qc"), nth(text, "x", 1))
            .is_some()
    );
    host.remove_overlay(&path("scratch.qc")).unwrap();
    assert!(!host.files().contains(&path("scratch.qc")));
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
    host.set_overlay(&path("progs.src"), b"../progs.dat\na.qc\nb.qc\n".to_vec())
        .unwrap();
    assert!(host.files().contains(&path("b.qc")));
    assert_eq!(host.text(&path("missing.qc")), None);
    host.remove_overlay(&path("progs.src")).unwrap();
    assert!(!host.files().contains(&path("b.qc")));
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
        bad.set_overlay(&path("quakec.toml"), b"also bad [".to_vec())
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
