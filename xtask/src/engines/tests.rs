// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::{Facts, Item, Key, TABLES, extract, is_extension_name, load, run};
use crate::download::Net;
use crate::oracles::sha256::hex_digest;
use crate::test_dirs::fresh;

/// A network serving fixed bodies by URL.
struct Canned(BTreeMap<String, Vec<u8>>);

impl Net for Canned {
    fn get(&self, url: &str) -> Result<Vec<u8>, String> {
        self.0.get(url).cloned().ok_or(format!("{url}: not found"))
    }

    fn download(&self, url: &str, _out: &Path) -> Result<(), String> {
        Err(format!("{url}: not used"))
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn facts(text: &str, defines: &[&str]) -> Facts {
    let defines: Vec<String> = defines.iter().map(|d| (*d).to_owned()).collect();
    extract(text, &defines).unwrap()
}

fn item(key: Key, name: &str, signature: &str, extensions: &[&str]) -> Item {
    Item {
        key,
        name: name.to_owned(),
        signature: signature.to_owned(),
        extensions: extensions.iter().map(|e| (*e).to_owned()).collect(),
    }
}

fn error(text: &str) -> String {
    extract(text, &[]).unwrap_err()
}

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
const SOURCE: &str = "float(string s) strlen = #114; // a note\n";
const LICENSE: &str = "license text\n";

/// A temporary workspace with one pinned source, the corpus manifest pinning the same
/// commit and files, and an empty tables file.
struct Workspace {
    dir: PathBuf,
    net: Canned,
}

/// What a test workspace holds. The default is the rerelease source [`SOURCE`], pinned at
/// [`COMMIT`] as the corpus manifest pins it, read once for server QuakeC.
struct WorkspaceSpec<'a> {
    name: &'a str,
    manifest_commit: &'a str,
    source: &'a str,
    tables: &'a str,
}

impl Default for WorkspaceSpec<'_> {
    fn default() -> Self {
        Self {
            name: "default",
            manifest_commit: COMMIT,
            source: SOURCE,
            tables: "[{ kind = \"ssqc\", defines = [] }]",
        }
    }
}

fn workspace(name: &str, manifest_commit: &str) -> Workspace {
    workspace_with(&WorkspaceSpec {
        name,
        manifest_commit,
        ..WorkspaceSpec::default()
    })
}

fn workspace_with(spec: &WorkspaceSpec<'_>) -> Workspace {
    let WorkspaceSpec {
        name,
        manifest_commit,
        source,
        tables,
    } = *spec;
    let dir = fresh(&format!("engines/{name}"));
    fs::create_dir_all(dir.join("crates/qc-engines/src")).unwrap();
    fs::create_dir_all(dir.join("corpus")).unwrap();
    let sha = hex_digest(source.as_bytes());
    let license_sha = hex_digest(LICENSE.as_bytes());
    fs::write(
        dir.join("crates/qc-engines/sources.toml"),
        format!(
            "[[source]]\nid = \"rerelease\"\nrepo = \"o/r\"\ncommit = \"{COMMIT}\"\n\
             path = \"defs.qc\"\nsha256 = \"{sha}\"\nlicense = \"GPL-2.0-or-later\"\n\
             license_file = \"COPYING.txt\"\nlicense_sha256 = \"{license_sha}\"\n\
             tables = {tables}\n"
        ),
    )
    .unwrap();
    fs::write(
        dir.join("corpus/manifest.toml"),
        format!(
            "[[source]]\nid = \"quake-rerelease-qc\"\nurl = \"https://github.com/o/r\"\n\
             commit = \"{manifest_commit}\"\n[source.checksums]\n\"defs.qc\" = \"{sha}\"\n\
             \"COPYING.txt\" = \"{license_sha}\"\n"
        ),
    )
    .unwrap();
    fs::write(dir.join(TABLES), "").unwrap();
    let base = format!("https://raw.githubusercontent.com/o/r/{COMMIT}");
    let net = Canned(BTreeMap::from([
        (format!("{base}/defs.qc"), source.as_bytes().to_vec()),
        (format!("{base}/COPYING.txt"), LICENSE.as_bytes().to_vec()),
    ]));
    Workspace { dir, net }
}

// spec ENGT-1.5: the checked-in tables record every pin of sources.toml, commit and SHA-256
#[test]
fn the_checked_in_tables_record_every_pin() {
    let tables = fs::read_to_string(root().join(TABLES)).unwrap();
    let pins = load(&root()).unwrap();
    // Every `[[source]]` of sources.toml is a pin, however many there are.
    let sources = fs::read_to_string(root().join("crates/qc-engines/sources.toml")).unwrap();
    let declared = sources
        .lines()
        .filter(|line| line.trim() == "[[source]]")
        .count();
    assert!(declared > 0);
    assert_eq!(pins.len(), declared);
    for pin in pins {
        for value in [
            &pin.commit,
            &pin.sha256,
            &pin.license_sha256,
            &pin.path,
            &pin.repo,
        ] {
            assert!(
                tables.contains(&format!("\"{value}\"")),
                "{} {value}",
                pin.id
            );
        }
    }
}

// spec ENGT-1.2: generation writes the tables, and the check passes on them and fails once
// they are edited
#[test]
fn generation_is_reproducible_and_checked() {
    let ws = workspace("reproducible", COMMIT);
    assert!(run(&ws.dir, false, &ws.net).unwrap());
    let written = fs::read_to_string(ws.dir.join(TABLES)).unwrap();
    assert!(written.contains("BuiltinKey::Number(114)"));
    assert!(written.contains(&hex_digest(SOURCE.as_bytes())));
    assert!(!written.contains("a note"));
    assert!(run(&ws.dir, true, &ws.net).unwrap());
    fs::write(ws.dir.join(TABLES), written.replace("114", "115")).unwrap();
    assert!(!run(&ws.dir, true, &ws.net).unwrap());
}

// spec ENGT-1.2: a source or license file whose SHA-256 is not the pinned one is an error, and
// nothing is written
#[test]
fn a_changed_source_is_an_error() {
    for file in ["defs.qc", "COPYING.txt"] {
        let mut ws = workspace(&format!("changed-{file}"), COMMIT);
        let url = format!("https://raw.githubusercontent.com/o/r/{COMMIT}/{file}");
        ws.net.0.insert(url, b"something else".to_vec());
        let err = run(&ws.dir, false, &ws.net).unwrap_err();
        assert!(err.contains("SHA-256"), "{err}");
        assert_eq!(fs::read_to_string(ws.dir.join(TABLES)).unwrap(), "");
    }
    let mut ws = workspace("missing", COMMIT);
    ws.net.0.clear();
    assert!(run(&ws.dir, false, &ws.net).is_err());
}

// spec ENGT-1.3: the rerelease pin must be the corpus manifest's
#[test]
fn the_rerelease_pin_agrees_with_the_corpus() {
    let ws = workspace("disagree", "fedcba9876543210fedcba9876543210fedcba98");
    let err = run(&ws.dir, false, &ws.net).unwrap_err();
    assert!(err.contains("quake-rerelease-qc"), "{err}");
    let real = load(&root()).unwrap();
    super::agree_with_corpus(&root(), &real).unwrap();
}

// spec ENGT-2.10: a source read for several module kinds gives a builtin table and an extension
// list for each, read with that kind's macros
#[test]
fn each_kind_gets_its_own_table() {
    let source = "\
#ifdef CSQC
float() client = #300;
#define EXT_CLIENT
#else
float() server = #78;
#endif
";
    let ws = workspace_with(&WorkspaceSpec {
        name: "kinds",
        source,
        tables: "[{ kind = \"ssqc\", defines = [\"SSQC\"] }, \
                 { kind = \"csqc\", defines = [\"CSQC\"] }]",
        ..WorkspaceSpec::default()
    });
    assert!(run(&ws.dir, false, &ws.net).unwrap());
    let written = fs::read_to_string(ws.dir.join(TABLES)).unwrap();
    let server = written.find("RERELEASE_SSQC_BUILTINS").unwrap();
    let client = written.find("RERELEASE_CSQC_BUILTINS").unwrap();
    let (server_part, client_part) = written.split_at(client);
    assert!(server < client);
    assert!(server_part.contains("BuiltinKey::Number(78)"));
    assert!(!server_part.contains("BuiltinKey::Number(300)"));
    assert!(client_part.contains("BuiltinKey::Number(300)"));
    assert!(!client_part.contains("BuiltinKey::Number(78)"));
    assert!(written.contains("RERELEASE_SSQC_EXTENSIONS: &[&str] = &[];"));
    assert!(client_part.contains("\"EXT_CLIENT\""));
}

// spec ENGT-2.10: a source has at least one table, each of a known module kind and no kind twice
#[test]
fn bad_tables_are_errors() {
    for (name, tables, needle) in [
        ("no-table", "[]", "no table"),
        (
            "unknown-kind",
            "[{ kind = \"server\", defines = [] }]",
            "unknown kind",
        ),
        (
            "twice",
            "[{ kind = \"csqc\", defines = [] }, { kind = \"csqc\", defines = [\"CSQC\"] }]",
            "two csqc tables",
        ),
        ("no-defines", "[{ kind = \"ssqc\" }]", "defines"),
    ] {
        let ws = workspace_with(&WorkspaceSpec {
            name,
            tables,
            ..WorkspaceSpec::default()
        });
        let err = run(&ws.dir, false, &ws.net).unwrap_err();
        assert!(err.contains(needle), "{name}: {err}");
        assert_eq!(fs::read_to_string(ws.dir.join(TABLES)).unwrap(), "");
    }
}

// spec ENGT-2.10: the pinned fteqw source is read for server, client and menu QuakeC, with
// NQSSQC and SSQC, CSQC and MENU defined; darkplaces and rerelease for server QuakeC,
// darkplaces-client for client and darkplaces-menu for menu QuakeC, with no macro; the C
// sources with none (ENGT-2.11)
#[test]
fn the_pinned_tables() {
    let pins = load(&root()).unwrap();
    let mut readings = Vec::new();
    for pin in &pins {
        for table in &pin.tables {
            readings.push(format!(
                "{} {} {}",
                pin.id,
                table.kind,
                table.defines.clone().unwrap_or_default().join(",")
            ));
            // ENGT-2.11: a QuakeC reading has no C array, and a C reading no macros.
            if pin.path.ends_with(".c") {
                assert!(table.defines.is_none(), "{} {}", pin.id, table.kind);
            } else {
                assert!(table.array.is_none(), "{} {}", pin.id, table.kind);
            }
        }
    }
    // The readings ENGT-2.10 names, each once; the C sources are covered above, so a new pin
    // does not change this list.
    for named in [
        "fteqw ssqc NQSSQC,SSQC",
        "fteqw csqc CSQC",
        "fteqw menuqc MENU",
        "darkplaces ssqc ",
        "darkplaces-client csqc ",
        "darkplaces-menu menuqc ",
        "rerelease ssqc ",
    ] {
        let found = readings.iter().filter(|r| *r == named).count();
        assert_eq!(found, 1, "{named}: {readings:?}");
    }
    for id in [
        "fteqw",
        "darkplaces",
        "darkplaces-client",
        "darkplaces-menu",
        "rerelease",
    ] {
        let kinds = readings
            .iter()
            .filter(|r| r.split(' ').next() == Some(id))
            .count();
        let named = if id == "fteqw" { 3 } else { 1 };
        assert_eq!(
            kinds, named,
            "{id} is read only for the kinds ENGT-2.10 names"
        );
    }
}

// spec ENGT-2.14, ENGT-2.15, ENGT-2.16, ENGT-2.17: every reading of a pinned C source has the
// columns its kind of array needs: a positional array none (ENGT-2.14), rows a name (ENGT-2.15),
// name patches a name, number, target and an earlier target_of (ENGT-2.16); and the readings
// the statements quote are read so: QuakeSpasm's positional pr_builtin with PF_Fixme absent,
// and vkquake-ext's extension names from qcextensions
#[test]
fn the_pinned_c_readings() {
    let pins = load(&root()).unwrap();
    let mut seen: Vec<(String, String)> = Vec::new();
    let mut quoted = Vec::new();
    for pin in &pins {
        for t in &pin.tables {
            let what = format!("{} {}", pin.id, t.kind);
            if pin.path.ends_with(".c") {
                assert!(t.array.is_some(), "{what}: a C reading names its array");
                if t.name.is_none() {
                    assert_eq!(
                        (t.function, t.number, t.signature, t.extension, t.target),
                        (None, None, None, None, None),
                        "{what}: a positional array has no columns (ENGT-2.14)"
                    );
                }
                if t.target.is_some() {
                    assert!(t.name.is_some() && t.number.is_some(), "{what}: ENGT-2.16");
                    let of = t.target_of.clone().unwrap_or_default();
                    assert!(
                        seen.contains(&(of.clone(), t.kind.clone())),
                        "{what}: target_of {of:?} is an earlier reading of its kind"
                    );
                }
                quoted.push(format!(
                    "{what} {} positional={} absent={} extensions={:?}",
                    t.array.clone().unwrap_or_default(),
                    t.name.is_none(),
                    t.absent.join(","),
                    t.extension_array
                ));
            }
            seen.push((pin.id.clone(), t.kind.clone()));
        }
    }
    for named in [
        "quakespasm ssqc pr_builtin positional=true absent=PF_Fixme extensions=None",
        "vkquake-ext ssqc extensionbuiltins positional=false absent=NULL,PF_NotImplemented \
         extensions=Some(\"qcextensions\")",
        "vkquake-ext csqc extensionbuiltins positional=false absent=NULL,PF_NotImplemented \
         extensions=Some(\"qcextensions\")",
    ] {
        assert!(quoted.iter().any(|q| q == named), "{named}: {quoted:?}");
    }
}

// spec ENGT-1.2: the command takes only --check
#[test]
fn the_command_takes_only_check() {
    let args = ["--write".to_owned()];
    assert!(super::main(&root(), &args).is_err());
    let args = ["--check".to_owned(), "x".to_owned()];
    assert!(super::main(&root(), &args).is_err());
}

// spec ENGT-2.1: only the key, name, signature and extensions of each builtin, and the
// extension names, are kept; comment text, code and other declarations are not
#[test]
fn only_facts_are_kept() {
    let text = "\
// A long explanation that must not be copied.
#define EXT_ONE /* what it does */
float MOVE_NORMAL = 0; // a constant, not a builtin
void() helper = { local float x; x = 1; };
float(float a) one = #91; /* Part of EXT_ONE
    more prose about it */
";
    assert_eq!(
        facts(text, &[]),
        Facts {
            builtins: vec![item(Key::Number(91), "one", "float(float a)", &["EXT_ONE"])],
            extensions: vec!["EXT_ONE".to_owned()],
        }
    );
}

// spec ENGT-2.2: line and block comments are not code, a block comment may span lines, and a
// `//` inside a string literal starts no comment
#[test]
fn comments_and_strings() {
    let text = "\
/* float(float a) hidden = #1;
float(float a) also_hidden = #2; */
//float(float a) commented = #3;
float(string s = \"//x\") shown = #4; float(float a) /* inline */ second = #5;
";
    let found = facts(text, &[]);
    let names: Vec<&str> = found.builtins.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["shown", "second"]);
    assert_eq!(found.builtins[0].signature, "float(string s = \"//x\")");
}

// spec ENGT-2.3: conditionals are evaluated over the predefined macros and the read #define
// and #undef lines, and only the parts read give facts
#[test]
fn conditionals_choose_what_is_read() {
    let text = "\
#ifdef SSQC
float(float a) server = #1;
#else
float(float a) client = #2;
#endif
#if defined(CSQC) || defined(MENU)
float(float a) menu = #3;
#endif
#if !defined(CSQC) && (defined SSQC)
#define LATER
#endif
#ifndef LATER
float(float a) not_later = #4;
#endif
#undef LATER
#ifndef LATER
float(float a) undefined_again = #5;
#endif
#if 0
#define NEVER
#endif
#ifdef NEVER
float(float a) never = #6;
#endif
#ifdef CSQC
#ifdef SSQC
float(float a) nested = #7;
#else
float(float a) nested_else = #8;
#endif
#endif
#if 1
float(float a) one = #9;
#endif
#pragma noref 1
";
    let found = facts(text, &["SSQC"]);
    let names: Vec<&str> = found.builtins.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["server", "undefined_again", "one"]);
    let client = facts(text, &["CSQC"]);
    let names: Vec<&str> = client.builtins.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "client",
            "menu",
            "not_later",
            "undefined_again",
            "nested_else",
            "one"
        ]
    );
}

// spec ENGT-2.3: an unknown directive, an unreadable #if, a stray #else or #endif and an
// unclosed block are errors
#[test]
fn bad_directives_are_errors() {
    assert!(error("#include \"x.qc\"\n").contains("unknown directive"));
    assert!(error("#if defined(A) + 1\n#endif\n").contains("#if"));
    assert!(error("#if (defined(A)\n#endif\n").contains("#if"));
    assert!(error("#if A & B\n#endif\n").contains("#if"));
    assert!(error("#if\n#endif\n").contains("#if"));
    assert!(error("#if defined(A) B\n#endif\n").contains("#if"));
    assert!(error("#else\n").contains("#else"));
    assert!(error("#endif\n").contains("#endif"));
    assert!(error("#ifdef A\n").contains("#endif"));
    assert!(error("#if 1\n#ifdef A\n#endif\n").contains("#endif"));
    assert!(error("#if A\n#endif\n").contains("cannot read A"));
}

// spec ENGT-2.4: both declaration forms, engine names, collapsed whitespace and the first of
// two declarations of one key
#[test]
fn builtin_declarations() {
    let text = "\
float(string s)   strlen =  #114;
float   log(float f) = #532;
void localsound( entity client, string sample ) = #0:ex_localsound;
float(float a) again = #114;
entity*(.__variant fld, int type=EV_STRING) find_list = #0:find_list;
";
    assert_eq!(
        facts(text, &[]).builtins,
        [
            item(Key::Number(114), "strlen", "float(string s)", &[]),
            item(Key::Number(532), "log", "float(float f)", &[]),
            item(
                Key::Name("ex_localsound".to_owned()),
                "localsound",
                "void( entity client, string sample )",
                &[]
            ),
            item(
                Key::Name("find_list".to_owned()),
                "find_list",
                "entity*(.__variant fld, int type=EV_STRING)",
                &[]
            ),
        ]
    );
}

// spec ENGT-2.4: a #0 with no name, a number that is not digits and an unreadable builtin are
// errors
#[test]
fn bad_builtins_are_errors() {
    for text in [
        "void() f = #0;",
        "void() f = #x;",
        "void() f = #1:name;",
        "void() f = #0:;",
        "void() f = #-1;",
        "float f = #1;",
        "void() = #1;",
        "f = #1;",
        "(float a) = #1;",
        "void() 1f = #1;",
    ] {
        assert!(error(text).contains("builtin"), "{text}");
    }
}

// spec ENGT-2.5: deprecation markers before the type are dropped with their argument
#[test]
fn deprecation_markers_are_dropped() {
    let text = "\
DEP void(string s) a = #1;
DEP_CSQC void(string s) b = #2;
FTEDEP(\"Redundant (really)\") string(string) c = #3;
__deprecated(\"Use x.\") __deprecated void() d = #4;
";
    let signatures: Vec<(String, String)> = facts(text, &[])
        .builtins
        .into_iter()
        .map(|b| (b.name, b.signature))
        .collect();
    assert_eq!(
        signatures,
        [
            ("a".to_owned(), "void(string s)".to_owned()),
            ("b".to_owned(), "void(string s)".to_owned()),
            ("c".to_owned(), "string(string)".to_owned()),
            ("d".to_owned(), "void()".to_owned()),
        ]
    );
}

// spec ENGT-2.6: extension names come from bodiless #define lines and from comment lines that
// hold only a name of the extension shape, each once in file order
#[test]
fn extension_names() {
    let text = "\
#define FTE_ONE
#define FTE_ONE
#define WITH_BODY 1
#define QWSSQC
#define lower_case_ext
#define MACRO_FN(x)
//DP_TWO
//  DP_THREE
//DP_TWO
//not a name
//NOUNDERSCORE
//ENDS_
";
    assert_eq!(
        facts(text, &[]).extensions,
        ["FTE_ONE", "DP_TWO", "DP_THREE"]
    );
    for good in ["FTE_QC_HASHTABLES", "A_1", "DP_QC_SINCOSSQRTPOW"] {
        assert!(is_extension_name(good), "{good}");
    }
    for bad in ["", "_DP_X", "1A_B", "ABC", "ABC_", "Abc_d", "A-B"] {
        assert!(!is_extension_name(bad), "{bad}");
    }
}

// spec ENGT-2.7: a builtin's extensions are its `Part of` list, else the heading above it,
// else none
#[test]
fn builtin_extensions() {
    let text = "\
float(float a) none = #1;
//DP_HEADING
float(float a) under_heading = #2;
float(float a) listed = #3; /* Part of FRIK_FILE, FTE_STRINGS, ??MVDSV_X */
float(float a) stops = #4; /* Part of ZQ_A. Called later, Not_A_NAME */
float(float a) prose = #5; /* Part of simple csqc, called after */
";
    let found: Vec<(String, Vec<String>)> = facts(text, &[])
        .builtins
        .into_iter()
        .map(|b| (b.name, b.extensions))
        .collect();
    let owned = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>();
    assert_eq!(
        found,
        [
            ("none".to_owned(), owned(&[])),
            ("under_heading".to_owned(), owned(&["DP_HEADING"])),
            (
                "listed".to_owned(),
                owned(&["FRIK_FILE", "FTE_STRINGS", "??MVDSV_X"])
            ),
            ("stops".to_owned(), owned(&["ZQ_A"])),
            ("prose".to_owned(), owned(&["DP_HEADING"])),
        ]
    );
}

// spec ENGT-2.9: a function body with no `;` after it ends at its closing brace, so the
// comment-line name after it is an extension heading and the builtin after it is read
#[test]
fn a_function_body_without_a_semicolon_ends_at_its_brace() {
    let text = "\
void() helper =
{
	print(\"a\\n\");
	if (1) { print(\"b\"); }
}

// DP_AFTER_BODY
float(float a) after = #7;
";
    let found = facts(text, &[]);
    assert_eq!(found.extensions, ["DP_AFTER_BODY"]);
    assert_eq!(
        found.builtins,
        [item(
            Key::Number(7),
            "after",
            "float(float a)",
            &["DP_AFTER_BODY"]
        )]
    );
}

// spec ENGT-2.9: braces inside parentheses end nothing, and a `;` inside a string literal ends
// no statement
#[test]
fn braces_in_parentheses_and_semicolons_in_strings() {
    let text = "\
float(enumflags:float{A=1,B=2} flags) braced = #8;
float(string s = \";\") quoted = #9;
float(string s = \"\\\";\") escaped = #10;
";
    let found: Vec<(Key, String)> = facts(text, &[])
        .builtins
        .into_iter()
        .map(|b| (b.key, b.signature))
        .collect();
    assert_eq!(
        found,
        [
            (
                Key::Number(8),
                "float(enumflags:float{A=1,B=2} flags)".to_owned()
            ),
            (Key::Number(9), "float(string s = \";\")".to_owned()),
            (Key::Number(10), "float(string s = \"\\\";\")".to_owned()),
        ]
    );
}

// spec ENGT-2.9: a comment-line name inside an unfinished statement, or inside a brace or
// parenthesis left open, is no extension name
#[test]
fn a_name_inside_a_statement_is_no_heading() {
    let text = "\
float(float a,
//DP_IN_PARENS
	float b) split = #11;
void() body =
{
	print(\"x\");
//DP_IN_BODY
};
float x =
//DP_IN_STATEMENT
	1;
";
    let found = facts(text, &[]);
    assert!(found.extensions.is_empty(), "{:?}", found.extensions);
    assert_eq!(
        found.builtins,
        [item(
            Key::Number(11),
            "split",
            "float(float a, float b)",
            &[]
        )]
    );
}
