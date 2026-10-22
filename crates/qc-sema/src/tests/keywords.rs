// SPDX-License-Identifier: GPL-3.0-or-later

//! FTEQCC's keywords as the whole profile reads them, through the preprocessor's marks, the
//! parser and the resolver (`docs/spec/dialect-fteqcc.md` FTEF-20).

use std::path::PathBuf;

use qc_diag::Code;
use qc_preproc::CommandLine;
use qc_syntax::Dialect;

use crate::db::Db;
use crate::expand::ModuleInput;
use crate::symbol::FileId;
use crate::ty::{SimpleType, Type};

/// The diagnostics of the FTEQCC files `sources`, compiled in order, read with the command line
/// `args`.
fn analysed_files(sources: &[&str], args: &[&str]) -> Vec<(Code, String)> {
    let mut db = Db::new();
    db.set_dialect(Dialect::Fteqcc);
    for (index, source) in sources.iter().enumerate() {
        db.set_file(
            FileId(index),
            PathBuf::from(format!("t{index}.qc")),
            source.as_bytes().to_vec(),
        );
    }
    let analysis = db.analyze_module(&ModuleInput {
        compiled: (0..sources.len()).map(FileId).collect(),
        command_line: CommandLine::new(args),
        ..ModuleInput::default()
    });
    analysis
        .diagnostics()
        .map(|d| (d.diagnostic.code, d.diagnostic.message.clone()))
        .collect()
}

/// The diagnostics of the FTEQCC file `source` read with the command line `args`.
fn analysed(source: &str, args: &[&str]) -> Vec<(Code, String)> {
    analysed_files(&[source], args)
}

/// The codes of the errors of `source` read with `args`.
fn errors(source: &str, args: &[&str]) -> Vec<Code> {
    analysed(source, args)
        .into_iter()
        .map(|(code, _)| code)
        .filter(|code| code.is_error())
        .collect()
}

/// The type of the global `name` of `source` read with `args`.
fn type_of(source: &str, args: &[&str], name: &str) -> Option<Type> {
    let mut db = Db::new();
    db.set_dialect(Dialect::Fteqcc);
    db.set_file(FileId(0), PathBuf::from("t.qc"), source.as_bytes().to_vec());
    let analysis = db.analyze_module(&ModuleInput {
        compiled: vec![FileId(0)],
        command_line: CommandLine::new(args),
        ..ModuleInput::default()
    });
    analysis
        .index
        .defs
        .iter()
        .find(|d| d.name == name)
        .map(|d| d.ty.clone())
}

fn keyword_warnings(source: &str, args: &[&str]) -> Vec<String> {
    analysed(source, args)
        .into_iter()
        .filter(|(code, _)| *code == Code::W0221)
        .map(|(_, message)| message)
        .collect()
}

// spec FTEF-20.1: a `#pragma keyword` changes the keyword from the token after it, for the
// parser and the semantic model alike, and the flags carry into the next file
#[test]
fn pragmas_change_keywords_from_the_next_token() {
    let off = "#pragma keyword disable for\nfloat for;\nvoid() g = { for = 1; };\n";
    assert_eq!(errors(off, &[]), []);
    assert_ne!(errors("float for;\nvoid() g = { for = 1; };\n", &[]), []);
    let again =
        "#pragma keyword disable for\n#pragma keyword enable for\nvoid() g = { for (;;) {} };\n";
    assert_eq!(errors(again, &[]), []);
    let carried = analysed_files(
        &["#pragma keyword enable long\n", "long l;\n__int64 l;\n"],
        &[],
    );
    assert!(
        carried.iter().all(|(code, _)| !code.is_error()),
        "{carried:?}"
    );
}

// spec FTEF-20.4: a definition named as a keyword that is on is warned of, "coexisting" with
// `kce` on and "disabling" with it off, which turns the keyword off
// spec QCD-7.67: W0221 has either text, at the name
#[test]
fn definitions_named_as_keywords() {
    assert_eq!(
        keyword_warnings("float for;\n", &[]),
        ["\"for\" keyword used as variable name - coexisting"]
    );
    assert_eq!(
        keyword_warnings("#pragma flag disable kce\nfloat for;\n", &[]),
        ["\"for\" keyword used as variable name - disabling"]
    );
    assert_eq!(
        keyword_warnings("float for;\n", &["-Fno-kce"]),
        ["\"for\" keyword used as variable name - disabling"]
    );
    assert_eq!(
        keyword_warnings("#pragma keyword disable for\nfloat for;\n", &[]),
        Vec::<String>::new()
    );
    assert_eq!(
        keyword_warnings("float thinktime;\n", &[]),
        Vec::<String>::new()
    );
    assert_eq!(
        keyword_warnings("#pragma keyword enable thinktime\nfloat thinktime;\n", &[]),
        ["\"thinktime\" keyword used as variable name - coexisting"]
    );
    assert_eq!(
        keyword_warnings("void() break = {};\n", &[]),
        Vec::<String>::new()
    );
    assert_eq!(keyword_warnings(".void() break;\n", &[]).len(), 1);
    assert_eq!(
        keyword_warnings("float FOR;\nfloat __for;\n", &[]),
        Vec::<String>::new()
    );
    assert_eq!(
        keyword_warnings("#pragma TARGET HEXEN2\nfloat float;\n", &[]),
        Vec::<String>::new()
    );
    assert_eq!(
        keyword_warnings("float float;\n", &["-Thexen2"]),
        Vec::<String>::new()
    );
    assert_eq!(keyword_warnings("float float;\n", &[]).len(), 1);
    // A redeclaration makes no new definition, nor a class's member.
    assert_eq!(keyword_warnings("float for;\nfloat for;\n", &[]).len(), 1);
    assert_eq!(
        keyword_warnings("class C { float vector; };\n", &[]),
        Vec::<String>::new()
    );
    // `kce` off: the keyword is a name after the definition.
    let lone = "#pragma flag disable kce\nfloat for;\nvoid() g = { for = 1; };\n";
    assert_eq!(errors(lone, &[]), []);
    let call = "#pragma flag disable kce\nfloat for;\nvoid() g = { for (;;) {} };\n";
    assert_ne!(errors(call, &[]), []);
    // The parameters of a function with a body are definitions, a prototype's are not.
    assert_eq!(
        errors(
            "#pragma flag disable kce\nvoid(float for) g = { for = 1; };\n",
            &[]
        ),
        []
    );
    assert_ne!(
        errors(
            "#pragma flag disable kce\nvoid(float for) h;\nvoid() g = { for (;;) {} };\n",
            &[]
        ),
        []
    );
}

// spec FTEF-20.5: `using` aliases a definition for its statement, "NAME is not defined" when
// there is none; `thinktime` needs `nextthink` and `time`; `state` declares its next think
// function
// spec QCD-4.13: E0313 "NAME is not defined", at the name
#[test]
fn keyword_statements_resolve_their_names() {
    assert_eq!(
        errors(
            "float x;\nvoid() g = { __using (x = y) { y = 1; } };\n",
            &[]
        ),
        []
    );
    assert_eq!(
        errors(
            "float x;\nvoid() g = { __using (x = y) { } y = 1; };\n",
            &[]
        ),
        [Code::E0301]
    );
    assert_eq!(
        errors("float x;\nvoid() g = { __using x = y; y = 1; };\n", &[]),
        []
    );
    assert_eq!(
        errors("float x;\nvoid() g = { y = 1; __using x = y; };\n", &[]),
        [Code::E0301]
    );
    assert_eq!(
        analysed("void() g = { __using (q) { } };\n", &[]),
        [(Code::E0313, "q is not defined".to_owned())]
    );
    let think = "entity self;\n.float nextthink;\nfloat time;\n\
                 void() g = { __thinktime self : 0.1; };\n";
    assert_eq!(errors(think, &[]), []);
    assert_eq!(
        errors(
            "entity self;\nvoid() g = { __thinktime self : 0.1; };\n",
            &[]
        ),
        [Code::E0301, Code::E0301]
    );
    assert_eq!(
        errors("void() g = { __state [1, h]; };\nvoid() h = {};\n", &[]),
        []
    );
    assert_eq!(errors("void() g = { __loop { break; } };\n", &[]), []);
    // A Hexen II target has `OP_THINKTIME`, and needs neither.
    assert_eq!(
        errors(
            "entity self;\nvoid() g = { thinktime self : 0.1; };\n",
            &["-Thexen2"]
        ),
        []
    );
    assert_eq!(
        errors("void() g = { asm { local float x; DONE; } };\n", &[]),
        []
    );
}

// spec FTEF-20.9: C type words name the type FTEQCC registers, read with the keywords and
// `ILP32` in force
#[test]
fn c_type_words_name_registered_types() {
    let named = |source: &str, args: &[&str]| type_of(source, args, "l");
    let int64 = Some(Type::Named("__int64".into()));
    let int32 = Some(Type::Named("__int32".into()));
    assert_eq!(named("#pragma keyword enable long\nlong l;\n", &[]), int64);
    assert_eq!(
        named("#pragma keyword enable long\nlong int l;\n", &[]),
        int32
    );
    assert_eq!(named("#pragma flag enable ILP32\n__long l;\n", &[]), int32);
    assert_eq!(named("long l;\n", &["-std=c"]), int32);
    assert_eq!(
        named("_Bool l;\n", &["-Kno-int"]),
        Some(Type::Named("__bfloat".into()))
    );
    assert_eq!(named("_Bool l;\n", &[]), Some(Type::Named("__bint".into())));
    assert_eq!(
        named("FLOAT l;\n", &["-Fcaseinsens"]),
        Some(Type::Simple(SimpleType::Float))
    );
    assert_eq!(
        errors("#pragma keyword enable long\nlong l;\n__int64 l;\n", &[]),
        []
    );
    assert_ne!(
        errors("#pragma flag enable ILP32\n__long l;\n__int64 l;\n", &[]),
        []
    );
}

// spec FTEF-20.11: a class's member words are read in any case and after `__`
#[test]
fn class_member_words_lower_as_the_parser_read_them() {
    let class = "class C { virtual: __STATIC void() m; float x; };\nvoid() C::m = { x = 1; };\n";
    assert_eq!(errors(class, &[]), []);
    let label = "class C { __VIRTUAL: void() m; };\nvoid() C::m = {};\n";
    assert_eq!(errors(label, &[]), []);
    let accessor =
        "accessor A : float { SET float(float v) = { }; GET float() = { return 0; } };\n";
    assert_eq!(errors(accessor, &["-Fcaseinsens"]), []);
}

// spec FTEF-20.11: `this` and `super` are intrinsics only with `keyword_class` on
#[test]
fn this_needs_the_class_keyword() {
    let outside = "void() g = { local entity e; e = this; };\n";
    assert_eq!(errors(outside, &[]), [Code::E0310]);
    let off = "#pragma keyword disable class\nvoid() g = { local entity e; e = this; };\n";
    assert_eq!(errors(off, &[]), [Code::E0301]);
    let super_off = "#pragma keyword disable class\nvoid() g = { local entity e; e = super; };\n";
    assert_eq!(errors(super_off, &[]), [Code::E0301]);
}

// spec FTEF-14.72: `-std=` sets every flag to its default and then a dialect's, in order with the
// `-K` options, and `-T` the target
#[test]
fn std_and_target_options() {
    let for_name = "float for;\nvoid() g = { for = 1; };\n";
    assert_eq!(errors(for_name, &["-std=qcc"]), []);
    assert_ne!(errors(for_name, &["-std=Qcc"]), []);
    assert_ne!(errors("long l;\n", &["-Klong", "-std=qcc"]), []);
    assert_eq!(
        errors("void() g = { for (;;) {} };\n", &["-std=qcc", "-Kfor"]),
        []
    );
    assert_eq!(
        errors("void() g = { loop { break; } };\n", &["-std=hcc"]),
        []
    );
    assert_eq!(
        errors("void() g = { loop { break; } };\n", &["-Thexen2"]),
        []
    );
    assert_ne!(errors("void() g = { loop { break; } };\n", &["-h2"]), []);
    assert_ne!(errors("void(float) g = {};\n", &["-std=qcc"]), []);
}
