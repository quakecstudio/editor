// SPDX-License-Identifier: GPL-3.0-or-later

//! Diagnostics.

use qc_diag::Code;
use qc_project::Target;

use super::{ProjectSpec, host, nth, path};

// spec QC-12.5: a name that resolves to nothing is reported where it is used, and a clean file
// has an empty list
#[test]
fn diagnostics_of_each_file() {
    let defs = "float health;\n";
    let code = "void() f = { health = armor; };\n";
    let mut host = host(&ProjectSpec {
        files: &[("defs.qc", defs), ("code.qc", code)],
        ..ProjectSpec::default()
    });
    let all = host.all_diagnostics();
    assert_eq!(all.get(&path("defs.qc")).map(Vec::len), Some(0));
    let found = &all[&path("code.qc")];
    assert_eq!(found.len(), 1);
    assert!(found[0].code.is_error());
    assert_eq!(found[0].span.start, nth(code, "armor", 0));
    assert_eq!(host.diagnostics(&path("code.qc")), *found);
    assert!(host.diagnostics(&path("none.qc")).is_empty());
}

// spec ENGT-4.1: a call of a builtin the target engine lacks is W0311
#[test]
fn diagnostics_include_the_target_engines_builtins() {
    let text = "void() print_it = #0:ex_bprint;\nvoid() f = { print_it(); };\n";
    let spec = ProjectSpec {
        files: &[("a.qc", text)],
        dialect: Some(qc_project::Dialect::Fteqcc),
        target: Some(Target::Quake),
        ..ProjectSpec::default()
    };
    let mut vanilla = host(&spec);
    let found = vanilla.diagnostics(&path("a.qc"));
    assert_eq!(
        found.iter().map(|d| d.code).collect::<Vec<_>>(),
        [Code::W0311]
    );
    let mut rerelease = host(&ProjectSpec {
        target: Some(Target::Rerelease),
        ..spec
    });
    assert!(rerelease.diagnostics(&path("a.qc")).is_empty());
}

// spec PRJ-6.5: a problem the project graph finds is reported on its line of the progs.src
#[test]
fn diagnostics_of_the_project_graph() {
    let mut host = host(&ProjectSpec {
        extra: &[("progs.src", "../progs.dat\na.qc\nmissing.qc\n")],
        ..ProjectSpec::default()
    });
    let found = host.diagnostics(&path("progs.src"));
    assert_eq!(found.len(), 1, "{found:?}");
    let src = "../progs.dat\na.qc\nmissing.qc\n";
    assert_eq!(found[0].span.start, nth(src, "missing.qc", 0));
    assert_eq!(
        found[0].span.end,
        nth(src, "missing.qc", 0) + "missing.qc".len()
    );
}

// spec QC-23.3: a file two modules compile has each problem once
#[test]
fn diagnostics_of_a_shared_file_are_not_repeated() {
    let defs = "float x = y;\n";
    let spec = ProjectSpec {
        files: &[("defs.qc", defs)],
        ..ProjectSpec::default()
    };
    let once = host(&spec).diagnostics(&path("defs.qc"));
    assert!(!once.is_empty());
    let mut both = host(&ProjectSpec {
        extra: &[("csprogs.src", "../csprogs.dat\ndefs.qc\n")],
        ..spec
    });
    assert_eq!(both.diagnostics(&path("defs.qc")), once);
}
