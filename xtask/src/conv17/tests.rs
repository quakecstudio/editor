// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::fs;
use std::path::{Path, PathBuf};

use super::{BASELINE, Rule, Violation, check, violations, write_baseline};
use crate::Report;

/// A workspace in a fresh directory named `name`: a library crate `lib` (L0) and an app
/// `app`, each with `src/lib.rs` holding the given source, and `layers.toml` naming them.
fn workspace(name: &str, lib: &str, app: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("xtask-conv17-{name}-{}", std::process::id()));
    let _gone = fs::remove_dir_all(&root);
    for (parent, krate, source) in [("crates", "lib", lib), ("apps", "app", app)] {
        let dir = root.join(parent).join(krate);
        fs::create_dir_all(dir.join("src/tests")).unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            format!("[package]\nname = \"{krate}\"\n"),
        )
        .unwrap();
        fs::write(dir.join("src/lib.rs"), source).unwrap();
        // Test code in every place the check leaves out.
        let test_code = "fn helper(flag: bool) -> Result<(), String> { Ok(()) }\n";
        for file in [
            "src/tests.rs",
            "src/fixtures.rs",
            "src/valve_tests.rs",
            "src/tests/it.rs",
        ] {
            fs::write(dir.join(file), test_code).unwrap();
        }
    }
    fs::write(
        root.join("layers.toml"),
        "[crates]\nlib = \"L0\"\napp = \"apps\"\n",
    )
    .unwrap();
    root
}

fn items(root: &Path) -> Vec<(Rule, String, String)> {
    violations(root)
        .unwrap()
        .into_iter()
        .map(|v| (v.rule, v.file, v.item))
        .collect()
}

const LIB: &str = "pub fn plain(x: u32) -> u32 { x }\n\
    pub fn flagged(x: u32, loud: bool) -> u32 { x }\n\
    pub fn stringly() -> Result<(), String> { Ok(()) }\n\
    pub fn typed() -> Result<(), std::io::Error> { Ok(()) }\n\
    pub struct Thing;\n\
    impl Thing { pub fn set(&mut self, on: bool) {} pub fn get(&self) -> bool { true } }\n\
    pub trait Act { fn act(&self, now: bool); }\n\
    pub mod inner { pub fn deep(a: bool) {} }\n\
    #[cfg(test)] mod tests { fn t(a: bool) -> Result<(), String> { Ok(()) } }\n\
    #[test] fn a_test() {}\n";

// CONV-17: a bool parameter in a shipped crate and a String error in a library crate are found,
// by file and qualified name; receivers, bool returns, other errors and test code are not
#[test]
fn violations_are_found_by_name_and_test_code_is_left_out() {
    let root = workspace(
        "found",
        LIB,
        "pub fn app_flag(x: bool) {}\npub fn app_error() -> Result<(), String> { Ok(()) }\n",
    );
    let lib = "crates/lib/src/lib.rs".to_owned();
    let app = "apps/app/src/lib.rs".to_owned();
    assert_eq!(
        items(&root),
        [
            (Rule::BoolArgument, app.clone(), "app_flag".to_owned()),
            (Rule::BoolArgument, lib.clone(), "Act::act".to_owned()),
            (Rule::BoolArgument, lib.clone(), "Thing::set".to_owned()),
            (Rule::BoolArgument, lib.clone(), "flagged".to_owned()),
            (Rule::BoolArgument, lib.clone(), "inner::deep".to_owned()),
            (Rule::StringError, lib, "stringly".to_owned()),
        ],
        "an app may return String errors"
    );
}

// CONV-17, P5-18: the check passes with every violation in the baseline, fails on one that is
// not, and fails on a baseline line whose violation is gone, so the baseline only shrinks
#[test]
fn the_baseline_only_shrinks() {
    let root = workspace("baseline", LIB, "pub fn fine() {}\n");
    let run = |root: &Path| {
        let mut report = Report::default();
        check(root, &mut report).unwrap();
        report.problems
    };
    let new = run(&root);
    assert_eq!(new.len(), 5, "{new:?}");
    assert!(
        new.iter().any(|p| p.contains("flagged takes a bool")),
        "{new:?}"
    );
    assert!(
        new.iter()
            .any(|p| p.contains("stringly returns Result<_, String>"))
    );
    assert_eq!(write_baseline(&root).unwrap(), 5);
    assert!(run(&root).is_empty());
    // Fixing `flagged` without removing its line fails.
    let fixed = LIB.replace("loud: bool", "loud: u8");
    fs::write(root.join("crates/lib/src/lib.rs"), fixed).unwrap();
    let stale = run(&root);
    assert_eq!(stale.len(), 1, "{stale:?}");
    assert!(stale[0].contains("no longer matches the code"), "{stale:?}");
    // A bad baseline line is an error, not a pass.
    fs::write(root.join(BASELINE), "bool-argument\tonly two\n").unwrap();
    let mut report = Report::default();
    assert!(check(&root, &mut report).is_err());
    let _unused = Violation {
        rule: Rule::StringError,
        file: String::new(),
        item: String::new(),
    };
}

// CONV-17: a free function whose first parameter is a type its crate defines is found, by value
// or by reference; one on a foreign or primitive type, a method and an associated function are
// not
#[test]
fn free_functions_on_a_crate_type_are_found() {
    let lib = "pub struct Map; pub enum Kind { A } pub type Alias = Map;\n\
        mod inner { pub struct Deep; }\n\
        fn size(map: &Map) -> usize { 0 }\n\
        fn kind_name(kind: Kind) -> u8 { 0 }\n\
        fn grow(map: &mut Map) {}\n\
        fn aliased(map: &Alias) {}\n\
        fn deep(d: &inner::Deep) {}\n\
        fn foreign(text: &str, map: &Map) {}\n\
        fn none() {}\n\
        impl Map { fn method(&self) {} fn new(other: &Map) -> Self { Map } }\n";
    let root = workspace("free", lib, "pub fn fine() {}\n");
    let found: Vec<String> = items(&root)
        .into_iter()
        .filter(|(rule, _, _)| *rule == Rule::FreeFnOnType)
        .map(|(_, _, item)| item)
        .collect();
    assert_eq!(found, ["aliased", "deep", "grow", "kind_name", "size"]);
}

// CONV-17: a visible field or a visible function's parameter named as an ID and typed as a raw
// integer, or an Option of one, is found; a private one, another name or a newtype is not
#[test]
fn bare_ids_are_found() {
    let lib = "pub struct EntityIndex(usize);\n\
        pub struct Place { pub entity: usize, pub edict_index: Option<u32>, pub x: f32,\n\
          hidden_id: u32, pub owner: EntityIndex }\n\
        pub fn select(entity: usize, count: usize) {}\n\
        fn private(entity: usize) {}\n\
        pub struct Panel; impl Panel { pub fn report(&self, index: usize) {} fn mine(&self, id: u8) {} }\n";
    let root = workspace("ids", lib, "pub fn fine() {}\n");
    let found: Vec<String> = items(&root)
        .into_iter()
        .filter(|(rule, _, _)| *rule == Rule::BareId)
        .map(|(_, _, item)| item)
        .collect();
    assert_eq!(
        found,
        [
            "Panel::report(index)",
            "Place.edict_index",
            "Place.entity",
            "select(entity)"
        ]
    );
}

// CONV-17: a numeric literal in a body other than 0, 1 and 2 is a magic number, once per
// function and value; constants, types, patterns, nested items and macro arguments are not read
#[test]
fn magic_numbers_are_found_once_per_function_and_value() {
    let lib = "const LIMIT: u32 = 4096;\n\
        pub fn scale(x: f32) -> f32 { let a: [u8; 16] = [0; 16]; x * 0.25 + 1.0 + 0.25 }\n\
        pub fn small(x: u32) -> u32 { x + 1 + 2 + 0 + LIMIT }\n\
        pub fn hex(x: u32) -> u32 { match x { 7 => 0x10, _ => 1_000u32 } }\n\
        pub fn macro_args() { let _ = format!(\"{}\", 99); }\n\
        pub fn nested() { const INNER: u8 = 77; fn helper() -> u8 { 5 } }\n\
        pub trait T { fn d(&self) -> u8 { 9 } }\n";
    let root = workspace("magic", lib, "pub fn fine() {}\n");
    let found: Vec<String> = items(&root)
        .into_iter()
        .filter(|(rule, _, _)| *rule == Rule::MagicNumber)
        .map(|(_, _, item)| item)
        .collect();
    // The `16` of `[u8; 16]` is a type's and not read; the `16` of `[0; 16]` is an expression's,
    // a size in code, and is.
    assert_eq!(
        found,
        ["T::d:9", "hex:1000", "hex:16", "scale:0.25", "scale:16"]
    );
}

// P5-18: the baseline command only removes: a fixed violation's line goes, a new violation is
// not added, and comments stay
#[test]
fn the_baseline_command_never_adds_a_violation() {
    let root = workspace("remove-only", LIB, "pub fn fine() {}\n");
    assert_eq!(write_baseline(&root).unwrap(), 5);
    let with_comment = format!(
        "# a reviewed exception\n{}",
        fs::read_to_string(root.join(BASELINE)).unwrap()
    );
    fs::write(root.join(BASELINE), with_comment).unwrap();
    // Fix one, add another.
    let changed = LIB
        .replace("loud: bool", "loud: u8")
        .replace("pub fn plain(x: u32)", "pub fn plain(x: u32, sneaky: bool)");
    fs::write(root.join("crates/lib/src/lib.rs"), changed).unwrap();
    assert_eq!(write_baseline(&root).unwrap(), 4);
    let text = fs::read_to_string(root.join(BASELINE)).unwrap();
    assert!(text.starts_with("# a reviewed exception\n"), "{text}");
    assert!(!text.contains("\tflagged"), "{text}");
    assert!(!text.contains("plain"), "the new violation is not written: {text}");
    let mut report = Report::default();
    check(&root, &mut report).unwrap();
    assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
    assert!(report.problems[0].contains("plain takes a bool"));
}
