// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::collections::BTreeSet;
use std::path::PathBuf;

use sha2::{Digest as _, Sha256};

use super::{COPYING, Fetch, FetchError, Folder, Template, TemplateError, hex};

/// A fresh directory for test `name`.
fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("qs-templates-{name}-{}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    dir
}

/// A fetch answering every path with `text` for that path.
struct Fake;

impl Fetch for Fake {
    fn get(&self, _template: Template, path: &str) -> Result<Vec<u8>, FetchError> {
        Ok(match path {
            "progs.src" => b"../progs.dat\n\ndefs.qc\n".to_vec(),
            other => format!("// {other}\n").into_bytes(),
        })
    }
}

/// The table `Fake` matches.
fn table() -> Vec<(&'static str, String)> {
    ["progs.src", "defs.qc", "monsters/ogre.qc"]
        .into_iter()
        .map(|path| {
            (
                path,
                hex(&Sha256::digest(Fake.get(Template::Id1, path).unwrap())),
            )
        })
        .collect()
}

// No spec statement: P4-17, a new project holds the template's files, progs.src with tests.qc
// last, quakec.toml with the template's dialect and target, a .gitignore excluding build/, the
// sample unit and scenario tests, the GPLv2 text and a notice naming the upstream and commit
#[test]
fn a_new_project_holds_the_starter_and_ours() {
    let root = dir("new").join("axes");
    let table = table();
    let pairs: Vec<(&str, &str)> = table.iter().map(|(p, h)| (*p, h.as_str())).collect();
    let created = Template::Rerelease.make(&pairs, &root, &Fake).unwrap();
    assert_eq!(created.name, "axes");
    let read = |path: &str| std::fs::read_to_string(root.join(path)).unwrap();
    assert_eq!(read("monsters/ogre.qc"), "// monsters/ogre.qc\n");
    assert_eq!(read("progs.src"), "../progs.dat\n\ndefs.qc\n\ntests.qc\n");
    let toml = read("quakec.toml");
    assert!(toml.contains("name = \"axes\"") && toml.contains("dialect = \"fteqcc\""));
    assert!(toml.contains("target = \"rerelease\"") && toml.contains("src = \"progs.src\""));
    assert!(read(".gitignore").lines().any(|line| line == "build/"));
    assert!(read("tests.qc").contains("void() test_anglemod_wraps ="));
    assert!(read("tests/start.scenario").contains("expect field player health == 100"));
    assert_eq!(read("COPYING"), COPYING);
    let notice = read("NOTICE.md");
    assert!(
        notice.contains("id-Software/quake-rerelease-qc"),
        "{notice}"
    );
    assert!(notice.contains("634eefab09a77eb7b5f5ca7078ba3d8784a91142"));
    assert!(notice.contains("General Public License version 2"));
    assert_eq!(created.files.len(), 9, "{:?}", created.files);
}

// No spec statement: P4-17, LIC-5, a file whose SHA-256 is not the pinned one, a failed fetch, a
// directory that is not empty or cannot name a mod make no project, and nothing is written
// before every file is checked
#[test]
fn a_project_is_made_only_from_the_pinned_files() {
    let root = dir("pinned").join("m");
    let wrong = [("progs.src", "00")];
    let error = Template::Id1.make(&wrong, &root, &Fake).unwrap_err();
    assert!(matches!(&error, TemplateError::Checksum { path, .. } if path == "progs.src"));
    assert!(error.to_string().contains("is not the pinned file"));
    assert!(!root.exists(), "nothing was written");
    let missing = Folder::new(None, None);
    assert!(matches!(
        Template::Id1.create(&root, &missing),
        Err(TemplateError::Fetch(_))
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("x"), "").unwrap();
    assert!(matches!(
        Template::Id1.create(&root, &Fake),
        Err(TemplateError::NotEmpty(_))
    ));
    assert!(matches!(
        Template::Id1.create(std::path::Path::new("/"), &Fake),
        Err(TemplateError::BadName(_))
    ));
}

// No spec statement: P4-17, each template lists progs.src first and every file once with a
// SHA-256, and the GPLv2 text is the bytes both upstreams carry
#[test]
fn the_tables_and_the_license_are_whole() {
    for template in Template::ALL {
        let files = template.files();
        assert_eq!(files.first().map(|(path, _)| *path), Some("progs.src"));
        let paths: BTreeSet<&str> = files.iter().map(|(path, _)| *path).collect();
        assert_eq!(paths.len(), files.len(), "{template:?}");
        assert!(files.iter().all(|(_, sha)| sha.len() == 64));
        assert!(paths.contains("defs.qc") && paths.contains("ai.qc"));
    }
    assert_eq!(
        hex(&Sha256::digest(COPYING.as_bytes())),
        "32b1062f7da84967e7019d01ab805935caa7ab7321a7ced0e30ebe75e5df1670"
    );
}
