// SPDX-License-Identifier: GPL-3.0-or-later

//! Hover.

use qc_engines::{BuiltinRef, Engine};
use qc_project::{ModuleKind, Target};

use super::{ProjectSpec, host, nth, one, path};
use crate::DefKind;

// spec QC-11.1: the hover of a use shows the declaration it resolves to and the comment above
// it
#[test]
fn hover_shows_the_declaration_and_its_comment() {
    let text =
        "// How hurt it is.\n// Zero is dead.\nfloat  health;\nvoid() f = { health = 1; };\n";
    let mut host = one(text);
    let at = nth(text, "health", 1);
    let hover = host.hover(&path("a.qc"), at + 1).expect("a hover");
    assert_eq!(hover.name, "health");
    assert_eq!(hover.kind, DefKind::Global);
    assert_eq!(hover.declaration, "float health");
    assert_eq!(
        hover.docs.as_deref(),
        Some("How hurt it is.\nZero is dead.")
    );
    assert_eq!(hover.range.span.start, at);
    assert_eq!(hover.builtin, None);
}

// spec QC-11.1: a comment after the declaration on its line documents it when none is above;
// a blank line or code before a comment above ends it
#[test]
fn hover_reads_the_comment_after_the_declaration() {
    let text = "/* old */\n\nfloat a; /* the a */\nfloat b; // b's\nfloat c;\n";
    let mut host = one(text);
    let docs = |host: &mut crate::AnalysisHost<_>, name: &str| {
        host.hover(&path("a.qc"), nth(text, name, 0))
            .and_then(|h| h.docs)
    };
    assert_eq!(docs(&mut host, "a;").as_deref(), Some("the a"));
    assert_eq!(docs(&mut host, "b;").as_deref(), Some("b's"));
    assert_eq!(docs(&mut host, "c;"), None);
}

// spec QC-13.2: the hover of a function shows its parameters as declared, and of a local its
// `local` declaration
#[test]
fn hover_of_functions_parameters_and_locals() {
    let text = "float(entity targ, float amount) heal = { local vector v; return amount; };\n";
    let mut host = one(text);
    let function = host.hover(&path("a.qc"), nth(text, "heal", 0)).unwrap();
    assert_eq!(
        function.declaration,
        "float(entity targ, float amount) heal"
    );
    assert_eq!(function.kind, DefKind::Function);
    let param = host.hover(&path("a.qc"), nth(text, "amount", 1)).unwrap();
    assert_eq!(param.kind, DefKind::Param);
    assert_eq!(param.declaration, "float amount");
    let local = host.hover(&path("a.qc"), nth(text, "v;", 0)).unwrap();
    assert_eq!(local.declaration, "local vector v");
    assert!(host.hover(&path("a.qc"), nth(text, "{", 0)).is_none());
}

// spec ENGT-3.9: the hover of a builtin names the engines whose table has it, and the target's
// entry
#[test]
fn hover_of_a_builtin_names_its_engines() {
    let text = "void(entity e, vector o) setorigin = #2;\nvoid() print_it = #0:ex_bprint;\n";
    let mut host = host(&ProjectSpec {
        files: &[("a.qc", text)],
        dialect: Some(qc_project::Dialect::Fteqcc),
        target: Some(Target::Rerelease),
        ..ProjectSpec::default()
    });
    let hover = host
        .hover(&path("a.qc"), nth(text, "setorigin", 0))
        .unwrap();
    let info = hover.builtin.expect("a builtin");
    assert_eq!(info.key, BuiltinRef::Number(2));
    assert_eq!(info.kind, ModuleKind::Ssqc);
    assert_eq!(info.target, Engine::Kex);
    assert_eq!(info.on_target().map(|b| b.name), Some("setorigin"));
    assert_eq!(info.engines.len(), Engine::ALL.len());
    assert!(info.engines.iter().all(|(_, b)| b.is_some()));
    let named = host.hover(&path("a.qc"), nth(text, "print_it", 0)).unwrap();
    let info = named.builtin.unwrap();
    assert_eq!(info.key, BuiltinRef::Name("ex_bprint".to_owned()));
    let vanilla = info
        .engines
        .iter()
        .find(|(engine, _)| *engine == Engine::Vanilla)
        .unwrap();
    assert_eq!(vanilla.1, None);
    assert!(info.on_target().is_some());
}

// spec ENGT-3.11: a client module's builtins are looked up in the client tables, and only
// engines with one are listed
#[test]
fn hover_of_a_client_builtin_reads_the_client_tables() {
    let client = "void(entity e, vector o) setorigin = #2;\n";
    let mut host = host(&ProjectSpec {
        extra: &[
            ("client.qc", client),
            ("csprogs.src", "../csprogs.dat\nclient.qc\n"),
        ],
        dialect: Some(qc_project::Dialect::Fteqcc),
        target: Some(Target::Fte),
        ..ProjectSpec::default()
    });
    let info = host
        .hover(&path("client.qc"), nth(client, "setorigin", 0))
        .unwrap()
        .builtin
        .unwrap();
    assert_eq!(info.kind, ModuleKind::Csqc);
    assert!(
        info.engines
            .iter()
            .all(|(e, _)| e.has_table(ModuleKind::Csqc))
    );
    assert!(!info.engines.iter().any(|(e, _)| *e == Engine::Vanilla));
}

// spec QC-11.5: a function's documentation is the comment of the declaration with its body, over
// a section comment above its prototype; a banner of empty comment lines is no text
#[test]
fn hover_prefers_the_comment_of_the_body() {
    let text = "//\n// combat.qc\n//\nvoid() hurt;\n//\n//\nvoid() idle;\n\n\
                /*\nhurt\n\nTakes health.\n*/\nvoid() hurt = { };\n";
    let mut host = one(text);
    let hurt = host.hover(&path("a.qc"), nth(text, "hurt", 0)).unwrap();
    assert_eq!(hurt.docs.as_deref(), Some("hurt\n\nTakes health."));
    let idle = host.hover(&path("a.qc"), nth(text, "idle", 0)).unwrap();
    assert_eq!(idle.docs, None);
}
