// SPDX-License-Identifier: GPL-3.0-or-later

//! Member access as FTEQCC types it (`docs/spec/dialect-fteqcc.md` FTEF-24).

use std::path::PathBuf;

use qc_diag::Code;
use qc_preproc::CommandLine;
use qc_syntax::Dialect;

use crate::db::Db;
use crate::expand::ModuleInput;
use crate::symbol::FileId;

const FILE: FileId = FileId::new(0);

fn diags(source: &str) -> Vec<(Code, String)> {
    let mut db = Db::new();
    db.set_dialect(Dialect::Fteqcc);
    db.set_file(FILE, PathBuf::from("t.qc"), source.as_bytes().to_vec());
    let analysis = db.analyze_module(&ModuleInput {
        compiled: vec![FILE],
        command_line: CommandLine::new::<&str>(&[]),
        ..ModuleInput::default()
    });
    analysis
        .diagnostics()
        .map(|d| (d.diagnostic.code, d.diagnostic.message.clone()))
        .collect()
}

fn one(code: Code, message: &str) -> Vec<(Code, String)> {
    vec![(code, message.to_owned())]
}

const DEFS: &str = ".float health; .vector origin; float g; entity self;\n";

fn body(statements: &str) -> String {
    format!("{DEFS}void() f = {{ local float a; local vector v; local string s; {statements} }};\n")
}

// spec FTEF-24.1: an entity's field is the value it holds, its type the field's (each case of
// this file agrees with the pinned FTEQCC, 2026-10-27)
#[test]
fn an_entitys_field_is_its_value() {
    assert_eq!(
        diags(&body("a = self.health + 1; v = self.origin * 2;")),
        []
    );
    assert_eq!(
        diags(&body("a = self.origin * \"s\";")),
        [
            (
                Code::E0302,
                "type mismatch for * (vector and string)".to_owned()
            ),
            (
                Code::W0232,
                "Implicit conversion from vector to float".to_owned()
            ),
            (Code::E0213, "Cannot cast from vector to float".to_owned()),
        ]
    );
}

// spec FTEF-24.1: a member that names a definition that is no field is "NAME is not a field"
// spec QCD-4.24: E0318 "NAME is not a field"
#[test]
fn a_non_field_member_is_an_error() {
    assert_eq!(
        diags(&body("a = self.g;")),
        one(Code::E0318, "g is not a field")
    );
}

// spec FTEF-24.2: x, y and z (and r, g, b) are a vector's float components, on a `.vector`
// field `.float` fields; any other name is unsupported
// spec QCD-4.20: E0314 "unsupported vector swizzle '.NAME'"
#[test]
fn vector_components() {
    assert_eq!(diags(&body("a = v.x + v.y + v.z + v.r;")), []);
    assert_eq!(
        diags(&body("a = v.w;")),
        one(Code::E0314, "unsupported vector swizzle '.w'")
    );
    assert_eq!(
        diags(&body("a = v.q;")),
        one(Code::E0314, "unsupported vector swizzle '.q'")
    );
}

// spec FTEF-24.3: a constant string's length is an int; one that is no constant is "not a
// constant", and another name "unsupported string method"
// spec QCD-4.21: E0315 "unsupported string method NAME"
// spec QCD-4.22: E0316 "not a constant"
#[test]
fn string_length() {
    assert_eq!(diags(&body("a = \"abc\".length;")), []);
    assert_eq!(
        diags(&body("a = s.length;")),
        one(Code::E0316, "not a constant")
    );
    assert_eq!(
        diags(&body("a = \"abc\".size;")),
        one(Code::E0315, "unsupported string method size")
    );
}

// spec FTEF-24.4: a struct's member has the member's type
// spec FTEF-24.5: through a pointer to something other than a struct or union, indirection is
// an error
// spec QCD-4.23: E0317 "indirection in TYPE - not a struct or union"
#[test]
fn struct_members_and_pointers() {
    let source = "struct pt { float px; vector pv; };\n\
                  void() f = { local pt p; local float a; local float *q; \
                  a = p.px + 1; a = p.pv * \"s\"; };\n";
    assert_eq!(
        diags(source),
        [
            (
                Code::E0302,
                "type mismatch for * (vector and string)".to_owned()
            ),
            (
                Code::W0232,
                "Implicit conversion from vector to float".to_owned()
            ),
            (Code::E0213, "Cannot cast from vector to float".to_owned()),
        ]
    );
    assert_eq!(
        diags("void() f = { local float *q; local float a; a = q->x; };\n"),
        one(Code::E0317, "indirection in float - not a struct or union")
    );
}

// spec FTEF-24.6: a class's member is looked up in the base's class and its parents, whatever
// member of another class the name also is
#[test]
fn class_members_by_the_bases_class() {
    let source = "entity self;\nclass W;\nclass V { float Visible; };\n\
                  class W { nonvirtual float Visible(void); W m_next; virtual void Draw(void); };\n\
                  float W::Visible(void) { return 1; };\n\
                  void W::Draw(void) { W w = this; w = w.m_next; \
                  if (w && w.Visible() && w.m_next.Visible()) { } };\n";
    assert_eq!(diags(source), []);
}

// spec FTEF-24.8: an array's `length` is the array's, an int, whatever its elements are
#[test]
fn an_arrays_length() {
    let source = "string styles[] = { \"a\", \"b\" };\n\
                  void() f = { local float n; n = styles.length; n = n * 2; };\n";
    assert_eq!(diags(source), []);
}
