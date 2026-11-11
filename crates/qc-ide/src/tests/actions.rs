// SPDX-License-Identifier: GPL-3.0-or-later

//! The editor's diagnostics and the quick fixes (P4-34): each fix applied to its fixture removes
//! its diagnostic.

#![expect(
    clippy::panic,
    reason = "test code: a missing diagnostic or fix is a failed test"
)]

use std::path::Path;

use qc_diag::{Code, Span};
use qc_project::{MemoryLoader, Target};

use super::{ProjectSpec, host, path};
use crate::AnalysisHost;

/// `text` with every edit of `action` applied, the last first.
fn applied(text: &str, edits: &[(Span, String)]) -> String {
    let mut out = text.to_owned();
    let mut edits = edits.to_vec();
    edits.sort_by_key(|(span, _)| std::cmp::Reverse(span.start));
    for (span, new) in edits {
        out.replace_range(span.start..span.end, &new);
    }
    out
}

/// The codes of `name`'s diagnostics.
fn codes(host: &mut AnalysisHost<MemoryLoader>, name: &str) -> Vec<Code> {
    host.diagnostics(&path(name))
        .iter()
        .map(|d| d.code)
        .collect()
}

/// Applies the first quick fix for `code` in `name`, through the host's overlay, and gives the
/// new text of the file it edits.
fn fix(host: &mut AnalysisHost<MemoryLoader>, name: &str, code: Code) -> (String, String) {
    let diagnostic = host
        .diagnostics(&path(name))
        .into_iter()
        .find(|d| d.code == code)
        .unwrap_or_else(|| panic!("{name} has {code:?}: {:?}", codes(host, name)));
    let actions = host.code_actions(&path(name), diagnostic.span);
    let action = actions
        .into_iter()
        .find(|a| a.code == code)
        .unwrap_or_else(|| panic!("a quick fix for {code:?}"));
    let target = action.edit.path.clone();
    let before = String::from_utf8(host.text(&target).unwrap_or_default()).unwrap_or_default();
    let after = applied(&before, &action.edit.edits);
    host.set_overlay(&target, after.clone().into_bytes())
        .expect("the overlay is set");
    (action.title, after)
}

/// The fixture's file name in the target's path.
fn name_of(target: &Path) -> String {
    target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

// spec QCD-7.115: a local no expression names is W0321; its quick fix removes its declaration,
// alone on its line or one of a list, and the diagnostic goes
#[test]
fn an_unused_local_is_reported_and_removed() {
    let text = "void() f =\n{\n\tlocal float a;\n\tlocal float b, c;\n\tc = 1;\n};\n";
    let mut host = host(&ProjectSpec {
        files: &[("a.qc", text)],
        ..ProjectSpec::default()
    });
    let unused: Vec<String> = host
        .diagnostics(&path("a.qc"))
        .into_iter()
        .filter(|d| d.code == Code::W0321)
        .map(|d| d.message)
        .collect();
    assert_eq!(unused, ["Local a is never used", "Local b is never used"]);
    let (title, after) = fix(&mut host, "a.qc", Code::W0321);
    assert_eq!(title, "Remove the unused local a");
    assert_eq!(after, "void() f =\n{\n\tlocal float b, c;\n\tc = 1;\n};\n");
    let (_, after) = fix(&mut host, "a.qc", Code::W0321);
    assert_eq!(after, "void() f =\n{\n\tlocal float c;\n\tc = 1;\n};\n");
    assert!(!codes(&mut host, "a.qc").contains(&Code::W0321));
}

// spec QCD-7.116: a model or sound a call uses that no precache names is W0322, a precached one,
// the empty name and a brush model are not; its quick fix adds the precache to worldspawn
#[test]
fn a_missing_precache_is_reported_and_added_to_worldspawn() {
    let defs = "void(entity e, string m) setmodel = #3;\n\
                void(entity e, float chan, string samp, float vol, float atten) sound = #8;\n\
                void(string s) precache_model = #20;\nvoid(string s) precache_sound = #19;\n\
                entity self;\n";
    let world = "void() worldspawn =\n{\n\tprecache_model (\"progs/a.mdl\");\n};\n";
    let monster = "void() m =\n{\n\tsetmodel (self, \"progs/a.mdl\");\n\tsetmodel (self, \"progs/b.mdl\");\n\
                   \tsetmodel (self, \"\");\n\tsetmodel (self, \"*1\");\n\
                   \tsound (self, 0, \"ogre/ogwake.wav\", 1, 1);\n};\n";
    let mut host = host(&ProjectSpec {
        files: &[
            ("defs.qc", defs),
            ("world.qc", world),
            ("monster.qc", monster),
        ],
        ..ProjectSpec::default()
    });
    let missing: Vec<String> = host
        .diagnostics(&path("monster.qc"))
        .into_iter()
        .filter(|d| d.code == Code::W0322)
        .map(|d| d.message)
        .collect();
    assert_eq!(
        missing,
        [
            "\"progs/b.mdl\" is never precached",
            "\"ogre/ogwake.wav\" is never precached"
        ]
    );
    let (title, after) = fix(&mut host, "monster.qc", Code::W0322);
    assert_eq!(title, "Add precache_model (\"progs/b.mdl\") to worldspawn");
    assert_eq!(
        after,
        "void() worldspawn =\n{\n\tprecache_model (\"progs/b.mdl\");\n\tprecache_model (\"progs/a.mdl\");\n};\n"
    );
    let (title, _) = fix(&mut host, "monster.qc", Code::W0322);
    assert!(title.starts_with("Add precache_sound"), "{title}");
    assert!(!codes(&mut host, "monster.qc").contains(&Code::W0322));
}

// No spec statement: P4-34: an unknown name that is a builtin of the engine is declared with its
// number and signature from the engine table, and the error goes
#[test]
fn a_missing_builtin_declaration_is_added() {
    let text = "void() f =\n{\n\tlocal vector v;\n\tv = normalize ('1 0 0');\n};\n";
    let mut host = host(&ProjectSpec {
        files: &[("a.qc", text)],
        target: Some(Target::Quake),
        ..ProjectSpec::default()
    });
    assert!(codes(&mut host, "a.qc").contains(&Code::E0301));
    let (title, after) = fix(&mut host, "a.qc", Code::E0301);
    assert_eq!(title, "Declare the builtin normalize = #9");
    assert!(
        after.starts_with("vector(vector v) normalize = #9;\n"),
        "{after}"
    );
    assert!(!codes(&mut host, "a.qc").contains(&Code::E0301));
}

// No spec statement: P4-34: an unknown field after a `.` is changed to the field nearest it in
// spelling, within two edits, and the error goes
#[test]
fn a_misspelled_field_is_corrected() {
    let text = "entity self;\n.float health;\nvoid() f =\n{\n\tself.helth = 1;\n};\n";
    let mut host = host(&ProjectSpec {
        files: &[("a.qc", text)],
        ..ProjectSpec::default()
    });
    let errors = codes(&mut host, "a.qc");
    let code = *errors
        .iter()
        .find(|c| matches!(c, Code::E0301 | Code::E0318 | Code::E0319))
        .expect("an unknown field");
    let (title, after) = fix(&mut host, "a.qc", code);
    assert_eq!(title, "Change helth to health");
    assert!(after.contains("self.health = 1;"));
    assert!(!codes(&mut host, "a.qc").contains(&code));
    assert_eq!(name_of(&path("a.qc")), "a.qc");
}

// spec QCD-7.115: a vector local is used when it or any of its components is named, and its
// components are never reported on their own
#[test]
fn a_vector_local_counts_its_components() {
    let text = "void() f =\n{\n\tlocal vector v, w, u;\n\tv = '1 0 0';\n\tw_x = 1;\n};\n";
    let mut host = host(&ProjectSpec {
        files: &[("a.qc", text)],
        ..ProjectSpec::default()
    });
    let unused: Vec<String> = host
        .diagnostics(&path("a.qc"))
        .into_iter()
        .filter(|d| d.code == Code::W0321)
        .map(|d| d.message)
        .collect();
    assert_eq!(unused, ["Local u is never used"]);
}
