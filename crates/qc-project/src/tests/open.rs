// SPDX-License-Identifier: GPL-3.0-or-later

//! Opening projects, bare ones in particular (PRJ-1, PRJ-3.3).

use std::fs;
use std::path::{Path, PathBuf};

use super::{loader, slash};
use crate::{Dialect, ModuleKind, Project, ProjectError, Target, open, open_dir, open_file};

fn modules(project: &Project) -> Vec<(String, ModuleKind, String)> {
    project
        .modules
        .iter()
        .map(|m| (m.name.clone(), m.kind, slash(&m.src)))
        .collect()
}

// spec PRJ-1.1: a directory holding quakec.toml opens it, and so does opening the file itself
#[test]
fn manifest_wins_over_progs_src() {
    let files = [
        (
            "p/quakec.toml",
            "[[module]]\nkind = \"csqc\"\nsrc = \"cs.src\"\n",
        ),
        ("p/progs.src", "x.dat a.qc"),
        ("p/cs.src", "x.dat"),
    ];
    let from_dir = open_dir(&mut loader(&files), Path::new("p")).unwrap();
    let from_file = open_file(&mut loader(&files), Path::new("p/quakec.toml")).unwrap();
    assert_eq!(from_dir, from_file);
    assert_eq!(from_dir.root, Path::new("p"));
    assert_eq!(
        modules(&from_dir),
        [("csqc".to_owned(), ModuleKind::Csqc, "p/cs.src".to_owned())]
    );
}

// spec PRJ-1.1: a quakec.toml that cannot be read is an error
#[test]
fn missing_manifest_file_is_an_error() {
    let err = open_file(&mut loader(&[]), Path::new("p/quakec.toml")).unwrap_err();
    assert!(matches!(err, ProjectError::Read { .. }), "{err:?}");
}

// spec PRJ-1.2: a bare directory opens progs.src, then csprogs.src, then registered sources
#[test]
fn bare_directory() {
    let files = [
        (
            "mymod/progs.src",
            "../progs.dat\n#pragma sourcefile menu.src\na.qc\n",
        ),
        ("mymod/a.qc", ""),
        (
            "mymod/csprogs.src",
            "../csprogs.dat\n#pragma sourcefile progs.src\nc.qc\n",
        ),
        ("mymod/c.qc", ""),
        ("mymod/menu.src", "#output \"../menu.dat\"\n"),
    ];
    let project = open_dir(&mut loader(&files), Path::new("mymod")).unwrap();
    assert_eq!(
        modules(&project),
        [
            (
                "progs".to_owned(),
                ModuleKind::Ssqc,
                "mymod/progs.src".to_owned()
            ),
            (
                "csprogs".to_owned(),
                ModuleKind::Csqc,
                "mymod/csprogs.src".to_owned()
            ),
            (
                "menu".to_owned(),
                ModuleKind::Menuqc,
                "mymod/menu.src".to_owned()
            ),
        ]
    );
    assert_eq!(project.manifest, None);
}

// spec PRJ-1.2: without csprogs.src a bare directory has the one module
#[test]
fn bare_directory_without_csprogs() {
    let files = [("d/progs.src", "x.dat a.qc"), ("d/a.qc", "")];
    let project = open_dir(&mut loader(&files), Path::new("d")).unwrap();
    assert_eq!(project.modules.len(), 1);
}

// spec PRJ-1.2: a directory with neither quakec.toml nor progs.src is an error
#[test]
fn empty_directory_is_an_error() {
    let err = open_dir(&mut loader(&[("d/other.qc", "")]), Path::new("d")).unwrap_err();
    assert!(matches!(&err, ProjectError::NoProject { dir } if dir == Path::new("d")));
    assert!(err.to_string().ends_with("no quakec.toml and no progs.src"));
    assert!(std::error::Error::source(&err).is_none());
}

// spec PRJ-1.3: any other file opens a bare project of it and what it registers
#[test]
fn bare_file() {
    let files = [
        (
            "g/src/progs.src",
            "#pragma sourcefile client/progs.src server/progs.src\n",
        ),
        (
            "g/src/client/progs.src",
            "#pragma progs_dat \"../../csprogs.dat\"\n#includelist\na.qc\n#endlist\n",
        ),
        ("g/src/client/a.qc", ""),
        ("g/src/server/progs.src", "#includelist\nb.qc\n#endlist\n"),
        ("g/src/server/b.qc", ""),
    ];
    let project = open_file(&mut loader(&files), Path::new("g/src/progs.src")).unwrap();
    assert_eq!(project.root, Path::new("g/src"));
    assert_eq!(project.name, "src");
    // spec PRJ-1.5: a source-form file that compiles nothing and only registers programs is not a module
    assert_eq!(
        modules(&project),
        [
            (
                "client/progs".to_owned(),
                ModuleKind::Csqc,
                "g/src/client/progs.src".to_owned()
            ),
            (
                "server/progs".to_owned(),
                ModuleKind::Ssqc,
                "g/src/server/progs.src".to_owned()
            ),
        ]
    );
}

// spec PRJ-1.5: a file that registers programs but compiles files of its own is a module
#[test]
fn program_list_with_files_is_a_module() {
    let files = [
        (
            "p/progs.src",
            "#pragma sourcefile cs.src\n#include \"a.qc\"\n",
        ),
        ("p/a.qc", ""),
        ("p/cs.src", "x.dat"),
    ];
    let project = open_file(&mut loader(&files), Path::new("p/progs.src")).unwrap();
    assert_eq!(project.modules.len(), 2);
    let files = [
        (
            "p/progs.src",
            "#pragma sourcefile cs.src\n#output \"x.dat\"\n",
        ),
        ("p/cs.src", "x.dat"),
    ];
    let project = open_file(&mut loader(&files), Path::new("p/progs.src")).unwrap();
    assert_eq!(project.modules.len(), 2);
}

// spec PRJ-1.3: a bare file that cannot be read opens, with the error in its graph (PRJ-4.5)
#[test]
fn bare_unreadable_file() {
    let project = open_file(&mut loader(&[]), Path::new("x/progs.src")).unwrap();
    assert_eq!(project.modules.len(), 1);
    assert_eq!(project.modules[0].graph.diagnostics.len(), 1);
}

// spec PRJ-1.4: a bare project is named after its directory, vanilla, quake, with no defines
// spec PRJ-1.4: module names drop .src but keep other extensions
#[test]
fn bare_defaults() {
    let files = [
        (
            "Mod Dir/progs.src",
            "#pragma sourcefile rules/x.qc\n#output \"p.dat\"\n",
        ),
        ("Mod Dir/rules/x.qc", "#output \"x.dat\"\n"),
    ];
    let project = open_file(&mut loader(&files), Path::new("Mod Dir/progs.src")).unwrap();
    assert_eq!(project.name, "Mod Dir");
    assert_eq!(project.mod_name, "Mod Dir");
    assert_eq!(
        (project.dialect, project.target),
        (Dialect::Vanilla, Target::Quake)
    );
    let names: Vec<&str> = project.modules.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["progs", "rules/x.qc"]);
    for module in &project.modules {
        assert!(module.defines.is_empty() && module.include_dirs.is_empty());
        assert_eq!(
            (module.dialect, module.target),
            (Dialect::Vanilla, Target::Quake)
        );
        assert_eq!(module.output, Path::new("Mod Dir/build/Mod Dir/progs.dat"));
    }
}

// spec PRJ-1.4: with no usable directory name the project is "project" and the mod "mod"
#[test]
fn bare_without_a_directory_name() {
    let project = open_file(
        &mut loader(&[("progs.src", "x.dat")]),
        Path::new("progs.src"),
    )
    .unwrap();
    assert_eq!(project.name, "project");
    assert_eq!(project.mod_name, "mod");
    assert_eq!(project.modules[0].output, Path::new("build/mod/progs.dat"));
}

// spec PRJ-3.3: a bare module is csqc for csprogs.dat, menuqc for menu.dat, any case, else ssqc
#[test]
fn kind_from_declared_output() {
    let cases = [
        ("../csprogs.dat", ModuleKind::Csqc),
        ("out/CSPROGS.DAT", ModuleKind::Csqc),
        ("menu.dat", ModuleKind::Menuqc),
        ("../Menu.Dat", ModuleKind::Menuqc),
        ("../progs.dat", ModuleKind::Ssqc),
        ("qwprogs.dat", ModuleKind::Ssqc),
        ("csprogs.dat.bak", ModuleKind::Ssqc),
    ];
    for (output, kind) in cases {
        let text = format!("{output}\n");
        let project = open_file(&mut loader(&[("m/p.src", &text)]), Path::new("m/p.src")).unwrap();
        assert_eq!(project.modules[0].kind, kind, "{output}");
    }
    let project = open_file(
        &mut loader(&[("m/p.src", "#define X\n")]),
        Path::new("m/p.src"),
    )
    .unwrap();
    assert_eq!(project.modules[0].kind, ModuleKind::Ssqc);
}

/// A directory under the system temporary directory, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("qc-project-{name}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap_or_default();
    }
}

// spec PRJ-1.1: open reads the file system: a directory and a file of it open the same project
// spec PRJ-1.3: opening a progs.src file directly opens a bare project of it
#[test]
fn open_from_the_file_system() {
    let temp = TempDir::new("open");
    let dir = temp.0.join("game");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("progs.src"), "../progs.dat\na.qc\n").unwrap();
    fs::write(dir.join("a.qc"), "").unwrap();
    let from_dir = open(&dir).unwrap();
    let from_file = open(&dir.join("progs.src")).unwrap();
    assert_eq!(from_dir, from_file);
    assert!(from_dir.root.is_absolute());
    assert_eq!(from_dir.name, "game");
    assert_eq!(from_dir.modules[0].graph.files.len(), 1);
    fs::write(
        dir.join("quakec.toml"),
        "[project]\nmod = \"g\"\n[[module]]\nkind = \"ssqc\"\nsrc = \"progs.src\"\n",
    )
    .unwrap();
    let with_manifest = open(&dir.join(".").join("..").join("game")).unwrap();
    assert_eq!(with_manifest.mod_name, "g");
    assert_eq!(with_manifest.root, from_dir.root);
}
