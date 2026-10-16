// SPDX-License-Identifier: GPL-3.0-or-later

//! `quakec.toml` (PRJ-2, PRJ-3).

use std::path::{Path, PathBuf};

use super::loader;
use crate::{
    Define, Dialect, ModuleKind, PRJ_CSQC_OUTPUT, PRJ_MENUQC_OUTPUT, PRJ_SSQC_OUTPUT, Project,
    ProjectError, Target, open_dir,
};

/// Opens directory `proj` holding `quakec.toml` with `manifest`, and `files` under it.
fn open_manifest(manifest: &str, files: &[(&str, &str)]) -> Result<Project, ProjectError> {
    let mut all = vec![("proj/quakec.toml", manifest)];
    all.extend_from_slice(files);
    open_dir(&mut loader(&all), Path::new("proj"))
}

fn manifest_error(manifest: &str) -> String {
    match open_manifest(manifest, &[]) {
        Err(ProjectError::Manifest { path, message }) => {
            assert_eq!(path, Path::new("proj/quakec.toml"));
            message
        }
        other => panic!("expected a manifest error for {manifest:?}, got {other:?}"),
    }
}

const ONE_MODULE: &str = "[[module]]\nkind = \"ssqc\"\nsrc = \"progs.src\"\n";

// spec PRJ-2.1: an optional [project] table and one or more [[module]] tables
// spec PRJ-2.2: a module without dialect or target takes the project's, and the defaults are vanilla and quake
#[test]
fn smallest_manifest() {
    let project = open_manifest(
        ONE_MODULE,
        &[("proj/progs.src", "x.dat a.qc"), ("proj/a.qc", "")],
    )
    .unwrap();
    assert_eq!(project.name, "proj");
    assert_eq!(project.mod_name, "proj");
    assert_eq!(project.dialect, Dialect::Vanilla);
    assert_eq!(project.target, Target::Quake);
    let [module] = project.modules.as_slice() else {
        panic!("one module");
    };
    assert_eq!(module.name, "ssqc");
    assert_eq!(module.dialect, Dialect::Vanilla);
    assert_eq!(module.target, Target::Quake);
    assert_eq!(module.src, Path::new("proj/progs.src"));
    assert_eq!(module.graph.files.len(), 1);
}

// spec PRJ-2.1: every key of [project] and [[module]] is read
// spec PRJ-2.2: dialect and target take each listed value, and a module's own wins
// spec PRJ-2.5: src, output and include_dirs resolve against the project directory
// spec PRJ-2.6: defines are NAME or NAME=VALUE
// spec PRJ-3.2: an explicit output is used, and the declared output is kept separately
#[test]
fn every_key() {
    let manifest = r#"
[project]
name = "Copper"
mod = "copper"
dialect = "fteqcc"
target = "fte"

[[module]]
kind = "csqc"
src = "cs/progs.src"
name = "client"
dialect = "gmqcc"
target = "dp"
output = "out/cl.dat"
defines = ["CSQC", "VERSION=2"]
include_dirs = ["lib"]

[[module]]
kind = "menuqc"
src = "menu.src"
target = "rerelease"
"#;
    let files = [
        ("proj/cs/progs.src", "../csprogs.dat\nmain.qc\n"),
        ("proj/cs/main.qc", "#include \"inc.qh\"\n"),
        ("proj/lib/inc.qh", ""),
        ("proj/menu.src", "#pragma once\n"),
    ];
    let project = open_manifest(manifest, &files).unwrap();
    assert_eq!(project.name, "Copper");
    assert_eq!(project.mod_name, "copper");
    assert_eq!(project.dialect, Dialect::Fteqcc);
    assert_eq!(project.target, Target::Fte);
    assert_eq!(project.manifest, Some(PathBuf::from("proj/quakec.toml")));
    let client = &project.modules[0];
    assert_eq!(client.name, "client");
    assert_eq!(client.kind, ModuleKind::Csqc);
    assert_eq!(client.dialect, Dialect::Gmqcc);
    assert_eq!(client.target, Target::Dp);
    assert_eq!(client.output, Path::new("proj/out/cl.dat"));
    assert_eq!(
        client.graph.declared_output,
        Some(PathBuf::from("proj/csprogs.dat"))
    );
    assert_eq!(
        client.defines,
        [
            Define {
                name: "CSQC".to_owned(),
                value: String::new()
            },
            Define {
                name: "VERSION".to_owned(),
                value: "2".to_owned()
            }
        ]
    );
    assert_eq!(client.include_dirs, [PathBuf::from("proj/lib")]);
    let files: Vec<&Path> = client
        .graph
        .files
        .iter()
        .map(|f| f.path.as_path())
        .collect();
    assert_eq!(
        files,
        [Path::new("proj/cs/main.qc"), Path::new("proj/lib/inc.qh")]
    );
    let menu = &project.modules[1];
    assert_eq!(menu.name, "menuqc");
    assert_eq!(menu.dialect, Dialect::Fteqcc);
    assert_eq!(menu.target, Target::Rerelease);
    assert_eq!(menu.output, Path::new("proj/build/copper/menu.dat"));
}

// spec PRJ-2.2: vanilla, fteqcc, gmqcc and quake, rerelease, fte, dp are the values
#[test]
fn every_dialect_and_target() {
    let dialects = [
        ("vanilla", Dialect::Vanilla),
        ("fteqcc", Dialect::Fteqcc),
        ("gmqcc", Dialect::Gmqcc),
    ];
    let targets = [
        ("quake", Target::Quake),
        ("rerelease", Target::Rerelease),
        ("fte", Target::Fte),
        ("dp", Target::Dp),
    ];
    for (text, dialect) in dialects {
        for (ttext, target) in targets {
            let manifest =
                format!("[project]\ndialect = \"{text}\"\ntarget = \"{ttext}\"\n{ONE_MODULE}");
            let project = open_manifest(&manifest, &[]).unwrap();
            assert_eq!((project.dialect, project.target), (dialect, target));
        }
    }
}

// spec PRJ-2.1: an unknown key, a wrong type, a value outside its list or invalid TOML is an error
#[test]
fn invalid_manifests() {
    let cases = [
        (format!("{ONE_MODULE}colour = \"red\"\n"), "colour"),
        (
            format!("[project]\nauthor = \"me\"\n{ONE_MODULE}"),
            "author",
        ),
        (format!("[extra]\n{ONE_MODULE}"), "extra"),
        (
            "[[module]]\nkind = \"ssqc\"\nsrc = 3\n".to_owned(),
            "line 3:",
        ),
        (
            "[[module]]\nkind = \"server\"\nsrc = \"p.src\"\n".to_owned(),
            "server",
        ),
        (format!("[project]\ndialect = \"c\"\n{ONE_MODULE}"), "c"),
        (
            format!("[project]\ntarget = \"quake2\"\n{ONE_MODULE}"),
            "quake2",
        ),
        ("[[module]]\nsrc = \"p.src\"\n".to_owned(), "kind"),
        ("[[module]]\nkind = \"ssqc\"\n".to_owned(), "src"),
        ("[[module]\n".to_owned(), "line 1:"),
    ];
    for (manifest, needle) in cases {
        let message = manifest_error(&manifest);
        assert!(message.contains(needle), "{manifest:?}: {message}");
    }
}

// spec PRJ-2.1: a manifest with no [[module]] is an error
#[test]
fn no_module_is_an_error() {
    assert_eq!(manifest_error(""), "no [[module]] is declared");
    assert_eq!(
        manifest_error("[project]\nname = \"x\"\n"),
        "no [[module]] is declared"
    );
}

// spec PRJ-2.3: the mod name is one directory name: not empty, not . or .., no / \ or :
#[test]
fn mod_names() {
    for bad in ["", ".", "..", "a/b", "a\\b", "c:d"] {
        let manifest = format!("[project]\nmod = {bad:?}\n{ONE_MODULE}");
        let message = manifest_error(&manifest);
        assert!(message.contains("not a single directory name"), "{bad}");
    }
    for good in ["mymod", "my mod", ".hidden", "a..b"] {
        let manifest = format!("[project]\nmod = {good:?}\n{ONE_MODULE}");
        assert_eq!(open_manifest(&manifest, &[]).unwrap().mod_name, good);
    }
}

// spec PRJ-2.4: module names are unique, and a module without name is named after its kind
#[test]
fn module_names_are_unique() {
    let twice = format!("{ONE_MODULE}{ONE_MODULE}");
    assert_eq!(manifest_error(&twice), "module name \"ssqc\" is used twice");
    let named = "[[module]]\nkind = \"ssqc\"\nsrc = \"a.src\"\nname = \"x\"\n\
                 [[module]]\nkind = \"csqc\"\nsrc = \"b.src\"\nname = \"x\"\n";
    assert_eq!(manifest_error(named), "module name \"x\" is used twice");
    let kinds = format!("{ONE_MODULE}[[module]]\nkind = \"csqc\"\nsrc = \"cs.src\"\n");
    let project = open_manifest(&kinds, &[]).unwrap();
    let names: Vec<&str> = project.modules.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["ssqc", "csqc"]);
}

// spec PRJ-2.5: an absolute src, output or include directory, or an output with .., is an error
#[test]
fn paths_stay_relative() {
    let base = "[[module]]\nkind = \"ssqc\"\n";
    let cases = [
        format!("{base}src = \"/abs/progs.src\"\n"),
        format!("{base}src = \"p.src\"\noutput = \"/abs/progs.dat\"\n"),
        format!("{base}src = \"p.src\"\noutput = \"build/../../x.dat\"\n"),
        format!("{base}src = \"p.src\"\ninclude_dirs = [\"/usr/include\"]\n"),
    ];
    for manifest in cases {
        let message = manifest_error(&manifest);
        assert!(message.starts_with("module ssqc: "), "{message}");
    }
    let parent_src = format!("{base}src = \"../shared/progs.src\"\n");
    let project = open_manifest(&parent_src, &[]).unwrap();
    assert_eq!(project.modules[0].src, Path::new("shared/progs.src"));
}

// spec PRJ-2.6: an entry that is not NAME or NAME=VALUE with a QuakeC name is an error
#[test]
fn bad_defines() {
    for bad in ["", "=1", "1X", "A-B", "A B=1"] {
        let manifest = format!("{ONE_MODULE}defines = [{bad:?}]\n");
        let message = manifest_error(&manifest);
        assert!(
            message.contains("is not NAME or NAME=VALUE"),
            "{bad}: {message}"
        );
    }
    for (good, name, value) in [("_X", "_X", ""), ("A1=", "A1", ""), ("B=x=y", "B", "x=y")] {
        assert_eq!(
            Define::parse(good),
            Some(Define {
                name: name.to_owned(),
                value: value.to_owned()
            })
        );
    }
}

// spec PRJ-2.7: #pragma sourcefile adds no modules to a project with quakec.toml
#[test]
fn sourcefile_adds_no_module() {
    let files = [
        ("proj/progs.src", "x.dat\n#pragma sourcefile cs.src\na.qc\n"),
        ("proj/a.qc", ""),
        ("proj/cs.src", "../csprogs.dat\n"),
    ];
    let project = open_manifest(ONE_MODULE, &files).unwrap();
    assert_eq!(project.modules.len(), 1);
    assert_eq!(
        project.modules[0].graph.sourcefiles,
        [PathBuf::from("proj/cs.src")]
    );
}

// spec PRJ-3.1: modules are separate programs: macros, include directories and once do not carry over
#[test]
fn modules_are_separate() {
    let manifest = "[[module]]\nkind = \"ssqc\"\nsrc = \"a.src\"\n\
                    [[module]]\nkind = \"csqc\"\nsrc = \"b.src\"\n";
    let files = [
        (
            "proj/a.src",
            "#define FROM_A\n#pragma includedir \"lib\"\n#include \"once.qh\"\n",
        ),
        ("proj/once.qh", "#pragma once\n"),
        (
            "proj/b.src",
            "#ifdef FROM_A\n#include \"a_only.qh\"\n#endif\n#include \"once.qh\"\n#include \"in_lib.qh\"\n",
        ),
        ("proj/a_only.qh", ""),
        ("proj/lib/in_lib.qh", ""),
    ];
    let project = open_manifest(manifest, &files).unwrap();
    let b: Vec<&Path> = project.modules[1]
        .graph
        .files
        .iter()
        .map(|f| f.path.as_path())
        .collect();
    assert_eq!(b, [Path::new("proj/b.src"), Path::new("proj/once.qh")]);
    let codes: Vec<&str> = project.modules[1]
        .graph
        .diagnostics
        .iter()
        .map(|d| d.code.as_str())
        .collect();
    assert_eq!(codes, ["E0111"]);
}

// spec PRJ-3.2: the default output is build/<mod>/ with the kind's file name
#[test]
fn default_outputs() {
    let manifest = "[project]\nmod = \"m\"\n\
                    [[module]]\nkind = \"ssqc\"\nsrc = \"a.src\"\n\
                    [[module]]\nkind = \"csqc\"\nsrc = \"b.src\"\n\
                    [[module]]\nkind = \"menuqc\"\nsrc = \"c.src\"\n";
    let project = open_manifest(manifest, &[]).unwrap();
    let outputs: Vec<PathBuf> = project.modules.iter().map(|m| m.output.clone()).collect();
    assert_eq!(
        outputs,
        [
            Path::new("proj/build/m").join(PRJ_SSQC_OUTPUT),
            Path::new("proj/build/m").join(PRJ_CSQC_OUTPUT),
            Path::new("proj/build/m").join(PRJ_MENUQC_OUTPUT),
        ]
    );
}

// spec PRJ-1.1: a quakec.toml that cannot be read is an error naming it
#[test]
fn unreadable_manifest_is_an_error() {
    struct Broken;
    impl crate::Loader for Broken {
        fn load(&mut self, _: &Path) -> std::io::Result<Vec<u8>> {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "denied",
            ))
        }
    }
    let err = open_dir(&mut Broken, Path::new("proj")).unwrap_err();
    let shown = err.to_string();
    assert!(
        shown.starts_with("proj") && shown.ends_with("quakec.toml: denied"),
        "{shown}"
    );
    assert!(std::error::Error::source(&err).is_some());
}
