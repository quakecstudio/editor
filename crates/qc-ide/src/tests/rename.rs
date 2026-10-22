// SPDX-License-Identifier: GPL-3.0-or-later

//! Rename.

use qc_diag::Span;

use super::{ProjectSpec, host, nth, one, path};
use crate::{Caution, EngineBinding, FileEdit, FileRange, RenameError};

/// The text of `text` with the edits of `edits` for one file applied.
fn applied(text: &str, edits: &FileEdit) -> String {
    let mut out = String::new();
    let mut at = 0;
    for (span, new) in &edits.edits {
        out.push_str(&text[at..span.start]);
        out.push_str(new);
        at = span.end;
    }
    out.push_str(&text[at..]);
    out
}

// spec QC-11.5: a rename edits every declaration and use of the definition, in every file
#[test]
fn rename_edits_every_declaration_and_use() {
    let defs = "void() think;\nfloat health;\n";
    let code = "void() think = { health = health - 1; };\n";
    let mut host = host(&ProjectSpec {
        files: &[("defs.qc", defs), ("code.qc", code)],
        ..ProjectSpec::default()
    });
    let edits = host
        .rename(&path("code.qc"), nth(code, "health", 1), "hp")
        .expect("the rename is allowed")
        .edits;
    assert_eq!(edits.len(), 2);
    assert_eq!(edits[0].path, path("code.qc"));
    assert_eq!(
        applied(code, &edits[0]),
        "void() think = { hp = hp - 1; };\n"
    );
    assert_eq!(applied(defs, &edits[1]), "void() think;\nfloat hp;\n");
    let functions = host
        .rename(&path("defs.qc"), nth(defs, "think", 0), "run")
        .unwrap()
        .edits;
    assert_eq!(
        applied(code, &functions[0]),
        "void() run = { health = health - 1; };\n"
    );
    assert_eq!(applied(defs, &functions[1]), "void() run;\nfloat health;\n");
}

// spec QC-11.7: renaming a vector renames the uses of its components with it
#[test]
fn rename_a_vector_with_its_components() {
    let text = "vector org;\nvoid() f = { org_x = 1; org = org; };\n";
    let mut host = one(text);
    let edits = host
        .rename(&path("a.qc"), nth(text, "org", 0), "pos")
        .unwrap()
        .edits;
    assert_eq!(
        applied(text, &edits[0]),
        "vector pos;\nvoid() f = { pos_x = 1; pos = pos; };\n"
    );
}

// spec QC-3.1: a new name must be a QuakeC name; the old name again is no edit
#[test]
fn rename_checks_the_new_name() {
    let text = "float a;\n";
    let mut host = one(text);
    for bad in ["", "1a", "a-b", "a b", "\u{e9}"] {
        assert_eq!(
            host.rename(&path("a.qc"), 6, bad),
            Err(RenameError::InvalidName(bad.to_owned()))
        );
    }
    assert_eq!(
        host.rename(&path("a.qc"), 6, "a").map(|r| r.edits),
        Ok(Vec::new())
    );
    assert!(host.rename(&path("a.qc"), 6, "_b9").is_ok());
}

// spec QC-12.3: a local named as a global declared before it reuses the global, so renaming a
// local to that name changes what the code means and is refused
#[test]
fn rename_refuses_a_name_that_changes_resolution() {
    let text = "float g;\nvoid() f = { local float t; t = 1; g = 2; };\n";
    let mut host = one(text);
    let refused = host.rename(&path("a.qc"), nth(text, "t;", 0), "g");
    assert!(
        matches!(refused, Err(RenameError::Conflict(_))),
        "{refused:?}"
    );
    // A free name is allowed.
    assert!(host.rename(&path("a.qc"), nth(text, "t;", 0), "u").is_ok());
}

// spec QC-3.2: a word the parser reads as a keyword where the name is used breaks the code,
// so the rename is refused
#[test]
fn rename_refuses_a_keyword_where_it_is_read_as_one() {
    let text = "float a;\nvoid() f = { a = 1; };\n";
    let mut host = one(text);
    let refused = host.rename(&path("a.qc"), nth(text, "a;", 0), "while");
    assert!(
        matches!(refused, Err(RenameError::Conflict(_))),
        "{refused:?}"
    );
}

// spec QC-9.7: a name written through a macro cannot be renamed there
#[test]
fn rename_refuses_a_use_through_a_macro() {
    let text = "#define H health\nfloat health;\nvoid() f = { H = 1; };\n";
    let mut host = host(&ProjectSpec {
        files: &[("a.qc", text)],
        dialect: Some(qc_project::Dialect::Fteqcc),
        ..ProjectSpec::default()
    });
    let refused = host.rename(&path("a.qc"), nth(text, "health;", 0), "hp");
    let macro_use = nth(text, "H =", 0);
    assert_eq!(
        refused,
        Err(RenameError::InMacro(FileRange {
            path: path("a.qc"),
            span: Span::new(macro_use, macro_use + 1),
        }))
    );
    assert_eq!(
        host.prepare_rename(&path("a.qc"), macro_use),
        Err(RenameError::InMacro(FileRange {
            path: path("a.qc"),
            span: Span::new(macro_use, macro_use + 1),
        }))
    );
}

// spec QC-12.5: prepare-rename gives the span of the name under the cursor, and nothing where
// no name resolves
#[test]
fn prepare_rename_gives_the_name_under_the_cursor() {
    let text = "float health;\nvoid() f = { health = 1; };\n";
    let mut host = one(text);
    let use_at = nth(text, "health", 1);
    assert_eq!(
        host.prepare_rename(&path("a.qc"), use_at + 2),
        Ok(FileRange {
            path: path("a.qc"),
            span: Span::new(use_at, use_at + "health".len()),
        })
    );
    assert_eq!(
        host.prepare_rename(&path("a.qc"), nth(text, "{", 0)),
        Err(RenameError::NoName)
    );
    assert_eq!(
        host.rename(&path("a.qc"), nth(text, "{", 0), "x"),
        Err(RenameError::NoName)
    );
}

// spec QC-23.3: a definition in a file two modules compile is renamed in both
#[test]
fn rename_across_modules() {
    let defs = "float shared;\n";
    let server = "void() s = { shared = 1; };\n";
    let client = "void() c = { shared = 2; };\n";
    let mut host = host(&ProjectSpec {
        files: &[("defs.qc", defs), ("server.qc", server)],
        extra: &[
            ("client.qc", client),
            ("csprogs.src", "../csprogs.dat\ndefs.qc\nclient.qc\n"),
        ],
        ..ProjectSpec::default()
    });
    let edits = host
        .rename(&path("defs.qc"), nth(defs, "shared", 0), "common")
        .unwrap()
        .edits;
    let paths: Vec<_> = edits.iter().map(|e| e.path.clone()).collect();
    assert_eq!(
        paths,
        [path("client.qc"), path("defs.qc"), path("server.qc")]
    );
    assert_eq!(applied(client, &edits[0]), "void() c = { common = 2; };\n");
}

// spec QC-12.5: each error says what stops the rename
#[test]
fn rename_errors_have_messages() {
    let at = FileRange {
        path: path("a.qc"),
        span: Span::new(3, 4),
    };
    assert_eq!(
        RenameError::NoName.to_string(),
        "there is no name to rename here"
    );
    assert!(
        RenameError::InvalidName("1".to_owned())
            .to_string()
            .starts_with("\"1\" is not")
    );
    assert_eq!(
        RenameError::Generated("spawnfunc_x".to_owned()).to_string(),
        "spawnfunc_x is made by the compiler"
    );
    assert_eq!(
        RenameError::InMacro(at.clone()).to_string(),
        format!(
            "the name is used through a macro in {} at byte 3",
            path("a.qc").display()
        )
    );
    assert_eq!(
        RenameError::Conflict(at).to_string(),
        format!(
            "the new name changes what the code means in {} at byte 3",
            path("a.qc").display()
        )
    );
}

// spec EDICT-2.2: a global declared before `end_sys_globals` is the engine's, found by its place
// and checked by the header CRC, so it is not renamed
// spec EDICT-3.2: so is a field declared before `end_sys_fields`
// spec EDICT-3.4: and a field the engine reads by name
#[test]
fn rename_refuses_the_engines_names() {
    let text = "entity self;\nvoid() main;\nvoid end_sys_globals;\n.float health;\n\
                void end_sys_fields;\n.float gravity;\nfloat mine;\n.float wait;\n";
    let mut host = one(text);
    let refused = |host: &mut crate::AnalysisHost<_>, needle: &str| {
        host.rename(&path("a.qc"), nth(text, needle, 0), "renamed")
            .err()
    };
    assert_eq!(
        refused(&mut host, "self"),
        Some(RenameError::EngineName(
            "self".to_owned(),
            EngineBinding::SystemGlobal
        ))
    );
    assert_eq!(
        refused(&mut host, "main"),
        Some(RenameError::EngineName(
            "main".to_owned(),
            EngineBinding::SystemGlobal
        ))
    );
    assert_eq!(
        refused(&mut host, "health"),
        Some(RenameError::EngineName(
            "health".to_owned(),
            EngineBinding::SystemField
        ))
    );
    assert_eq!(
        refused(&mut host, "gravity"),
        Some(RenameError::EngineName(
            "gravity".to_owned(),
            EngineBinding::FieldByName
        ))
    );
    assert_eq!(refused(&mut host, "mine"), None);
    assert_eq!(refused(&mut host, "wait"), None);
    assert!(
        RenameError::EngineName("self".to_owned(), EngineBinding::SystemGlobal)
            .to_string()
            .contains("before end_sys_globals")
    );
    assert!(
        RenameError::EngineName("health".to_owned(), EngineBinding::SystemField)
            .to_string()
            .contains("before end_sys_fields")
    );
    assert_eq!(
        RenameError::EngineName("gravity".to_owned(), EngineBinding::FieldByName).to_string(),
        "the engine reads the field gravity by its name"
    );
}

// spec EDICT-8.6: a function a map may name as a classname, and a field it may set as a key
// (EDICT-8.2), are renamed with a caution; a global is not
#[test]
fn rename_cautions_about_names_maps_use() {
    let text =
        ".float wait;\nvoid() monster_ogre = { };\nfloat count;\nvoid(entity e) remove = #15;\n";
    let mut host = one(text);
    let caution = |host: &mut crate::AnalysisHost<_>, needle: &str| {
        host.rename(&path("a.qc"), nth(text, needle, 0), "renamed")
            .unwrap()
            .caution
    };
    assert_eq!(
        caution(&mut host, "monster_ogre"),
        Some(Caution::MapFunction("monster_ogre".to_owned()))
    );
    assert_eq!(
        caution(&mut host, "wait"),
        Some(Caution::MapField("wait".to_owned()))
    );
    assert_eq!(caution(&mut host, "count"), None);
    // A builtin is bound by its number, which a rename keeps.
    assert_eq!(caution(&mut host, "remove"), None);
    assert!(
        Caution::MapFunction("f".to_owned())
            .to_string()
            .contains("classname")
    );
    assert!(
        Caution::MapField("w".to_owned())
            .to_string()
            .contains("as a key")
    );
}

// spec QC-11.7: a vector's component is renamed with its vector, not from one of its uses
#[test]
fn rename_refuses_a_component() {
    let text = "vector org;\nvoid() f = { org_x = 1; };\n";
    let mut host = one(text);
    let refused = RenameError::Component {
        component: "org_x".to_owned(),
        vector: "org".to_owned(),
    };
    assert_eq!(
        host.prepare_rename(&path("a.qc"), nth(text, "org_x", 0)),
        Err(refused.clone())
    );
    assert_eq!(
        host.rename(&path("a.qc"), nth(text, "org_x", 0), "pos_x"),
        Err(refused.clone())
    );
    assert_eq!(
        refused.to_string(),
        "org_x is a component of the vector org: rename org"
    );
}

// spec QC-23.3: a module that compiles a shared file but not the definition's file is checked
// too, though nothing asked about it before the rename
#[test]
fn rename_checks_a_module_it_has_not_read() {
    let defs = "float shared;\n";
    let common = "void() use_it = { shared = 1; };\n";
    let mut host = host(&ProjectSpec {
        files: &[("defs.qc", defs), ("common.qc", common)],
        extra: &[
            ("client_defs.qc", "float other;\n"),
            ("csprogs.src", "../csprogs.dat\nclient_defs.qc\ncommon.qc\n"),
        ],
        ..ProjectSpec::default()
    });
    // In the client module `shared` names nothing; renaming it to `other` there makes the use
    // name the client's `other`.
    let refused = host.rename(&path("defs.qc"), nth(defs, "shared", 0), "other");
    assert!(
        matches!(refused, Err(RenameError::Conflict(_))),
        "{refused:?}"
    );
}
