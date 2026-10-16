// SPDX-License-Identifier: GPL-3.0-or-later

//! Matching a name to a file (PRJ-7).

use std::fs;
use std::path::Path;

use super::{GraphSpec, build, diags, names};
use crate::{Code, FileGraph, open};

/// A source-form module `src/main.src` whose root is `body`, with the other `files`, on a
/// file system that ignores case or not.
fn module(body: &str, files: &[(&str, &str)], ignore_case: bool) -> FileGraph {
    let mut all = vec![("src/main.src", body)];
    all.extend_from_slice(files);
    build(&GraphSpec {
        files: &all,
        src: "src/main.src",
        ignore_case,
        ..GraphSpec::default()
    })
}

/// The messages of the graph's diagnostics.
fn messages(graph: &FileGraph) -> Vec<String> {
    graph
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect()
}

/// FreeCS's case: `shared/defs.h` includes `"animations.h"` and the file is `Animations.h`.
const FREECS: [(&str, &str); 2] = [
    ("src/shared/defs.h", "// defs\n#include \"animations.h\"\n"),
    ("src/shared/Animations.h", ""),
];

// spec PRJ-7.1: where the file system ignores case, a name in the wrong case opens the file,
// and the entry keeps the name's own case
// spec PRJ-7.2: the name that differs in case from the file it opened is warning W0402 at the
// directive, naming the stored spelling
// spec QCD-7.16: W0402 is a warning naming the name and the stored name
#[test]
fn a_name_in_the_wrong_case_warns_where_case_is_ignored() {
    let graph = module("#include \"shared/defs.h\"\n", &FREECS, true);
    assert_eq!(
        names(&graph),
        [
            "src/main.src",
            "src/shared/defs.h",
            "src/shared/animations.h"
        ]
    );
    assert_eq!(diags(&graph), ["W0402 src/shared/defs.h:2"]);
    assert_eq!(
        messages(&graph),
        ["animations.h differs in case from the file it opens, Animations.h"]
    );
    assert!(!Code::W0402.is_error());
}

// spec PRJ-7.1: where the file system matches case, the same name is error E0111, as FTEQCC's
// fopen fails, and nothing is brought in
#[test]
fn a_name_in_the_wrong_case_fails_where_case_matters() {
    let graph = module("#include \"shared/defs.h\"\n", &FREECS, false);
    assert_eq!(names(&graph), ["src/main.src", "src/shared/defs.h"]);
    assert_eq!(diags(&graph), ["E0111 src/shared/defs.h:2"]);
}

// spec PRJ-7.1: a list entry in the wrong case opens where case is ignored and is E0402 where
// it is not, as id's qcc's open fails
// spec PRJ-7.2: a list entry in the wrong case warns at its line
#[test]
fn list_entries_in_the_wrong_case() {
    let files = [
        ("progs.src", "../progs.dat\nDefs.qc\na.qc\n"),
        ("defs.qc", ""),
        ("a.qc", ""),
    ];
    let spec = GraphSpec {
        files: &files,
        ..GraphSpec::default()
    };
    let graph = build(&GraphSpec {
        ignore_case: true,
        ..spec.clone()
    });
    assert_eq!(names(&graph), ["Defs.qc", "a.qc"]);
    assert_eq!(diags(&graph), ["W0402 progs.src:2"]);
    assert_eq!(
        messages(&graph),
        ["Defs.qc differs in case from the file it opens, defs.qc"]
    );
    let graph = build(&spec);
    assert_eq!(names(&graph), ["a.qc"]);
    assert_eq!(diags(&graph), ["E0402 progs.src:2"]);
}

// spec PRJ-7.1: an #includelist entry in the wrong case opens where case is ignored and is
// E0402 where it is not
// spec PRJ-7.2: an #includelist entry in the wrong case warns at its line
#[test]
fn includelist_entries_in_the_wrong_case() {
    let body = "#includelist\nA.QC\n#endlist\n";
    let graph = module(body, &[("src/a.qc", "")], true);
    assert_eq!(names(&graph), ["src/main.src", "src/A.QC"]);
    assert_eq!(diags(&graph), ["W0402 src/main.src:2"]);
    let graph = module(body, &[("src/a.qc", "")], false);
    assert_eq!(names(&graph), ["src/main.src"]);
    assert_eq!(diags(&graph), ["E0402 src/main.src:2"]);
}

// spec PRJ-7.2: every component the name spells is compared, directories included, and the
// warning gives the stored spelling of all of them, past any ..
#[test]
fn directory_components_of_the_name_are_compared() {
    let files = [("src/shared/defs.h", ""), ("lib/x.qh", "")];
    let body = "#include \"Shared/defs.h\"\n#include \"../LIB/X.qh\"\n";
    let graph = module(body, &files, true);
    assert_eq!(
        diags(&graph),
        ["W0402 src/main.src:1", "W0402 src/main.src:2"]
    );
    assert_eq!(
        messages(&graph),
        [
            "Shared/defs.h differs in case from the file it opens, shared/defs.h",
            "../LIB/X.qh differs in case from the file it opens, lib/x.qh",
        ]
    );
}

// spec PRJ-7.2: a name whose case matches its file is no warning, on either kind of file
// system
#[test]
fn a_name_in_the_right_case_does_not_warn() {
    let body = "#include \"shared/Animations.h\"\n";
    for ignore_case in [true, false] {
        let graph = module(body, &FREECS, ignore_case);
        assert_eq!(names(&graph).len(), 2, "{ignore_case}");
        assert!(graph.diagnostics.is_empty(), "{ignore_case}");
    }
}

// spec PRJ-7.2: the warning is given once for each entry brought in by a name in the wrong
// case, so a name used twice warns twice
#[test]
fn each_entry_brought_in_warns() {
    let body = "#include \"A.qc\"\n#include \"a.qc\"\n#include \"A.qc\"\n";
    let graph = module(body, &[("src/a.qc", "")], true);
    assert_eq!(names(&graph).len(), 4);
    assert_eq!(
        diags(&graph),
        ["W0402 src/main.src:1", "W0402 src/main.src:3"]
    );
}

// spec PRJ-7.2: the directory a name is resolved against is not compared, whether it is the
// including file's or an include directory
// spec PRJ-7.3: a module's src, include directories and #pragma sourcefile entries are not
// compared
#[test]
fn names_that_bring_in_no_file_are_not_compared() {
    let files = [
        (
            "src/main.src",
            "#pragma sourcefile CS.src\n#include \"a.qc\"\n#include \"x.qh\"\n",
        ),
        ("src/a.qc", ""),
        ("lib/x.qh", ""),
        ("src/cs.src", ""),
    ];
    let graph = build(&GraphSpec {
        files: &files,
        src: "SRC/Main.src",
        include_dirs: &["LIB"],
        ignore_case: true,
        ..GraphSpec::default()
    });
    assert_eq!(names(&graph), ["SRC/Main.src", "SRC/a.qc", "LIB/x.qh"]);
    assert!(graph.diagnostics.is_empty(), "{:?}", diags(&graph));
    assert_eq!(graph.sourcefiles.len(), 1);
}

/// A directory under the system temporary directory, removed when dropped.
struct TempDir(std::path::PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap_or_default();
    }
}

// spec PRJ-7.1: on the host file system, FreeCS's include opens where the file system ignores
// case and is E0111 where it matches case
// spec PRJ-7.2: on the host file system, the stored name is read from the directory, so the
// warning comes wherever the file opens
#[test]
fn the_host_file_system_decides_and_the_warning_follows() {
    let temp =
        TempDir(std::env::temp_dir().join(format!("qc-project-case-{}", std::process::id())));
    let shared = temp.0.join("shared");
    fs::create_dir_all(&shared).unwrap();
    fs::write(temp.0.join("progs.src"), "#include \"shared/defs.h\"\n").unwrap();
    fs::write(shared.join("defs.h"), "#include \"animations.h\"\n").unwrap();
    fs::write(shared.join("Animations.h"), "").unwrap();
    let ignores_case = shared.join("animations.h").exists();
    let project = open(&temp.0).unwrap();
    let graph = &project.modules[0].graph;
    let codes: Vec<Code> = graph.diagnostics.iter().map(|d| d.code).collect();
    if ignores_case {
        assert_eq!(codes, [Code::W0402]);
        assert_eq!(graph.files.len(), 3);
        assert_eq!(
            messages(graph),
            ["animations.h differs in case from the file it opens, Animations.h"]
        );
    } else {
        assert_eq!(codes, [Code::E0111]);
        assert_eq!(graph.files.len(), 2);
    }
    assert!(Path::new(&graph.diagnostics[0].file).ends_with("shared/defs.h"));
}
