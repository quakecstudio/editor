// SPDX-License-Identifier: GPL-3.0-or-later

//! The diagnostics of assignments and postfix updates as FTEQCC gives them
//! (`docs/spec/dialect-fteqcc.md` FTEF-29). Each case's diagnostics are the pinned FTEQCC's
//! (2026-10-27).

use std::path::PathBuf;

use qc_diag::Code;
use qc_preproc::CommandLine;
use qc_syntax::Dialect;

use crate::db::Db;
use crate::expand::ModuleInput;
use crate::symbol::FileId;

const FILE: FileId = FileId(0);

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

fn body(statement: &str) -> Vec<(Code, String)> {
    diags(&format!(
        "const float k = 3;\n.float fl; .vector fv;\nentity self;\n\
         void() g = {{ local float a; local vector v; local string s; local entity e; \
         local float arr[4]; {statement} }};\n"
    ))
}

fn of(cases: &[(Code, &str)]) -> Vec<(Code, String)> {
    cases.iter().map(|&(c, m)| (c, m.to_owned())).collect()
}

// spec FTEF-29.1: an assignment to a read-only reference warns "Assignment to const"
// spec FTEF-29.3: and the store warns "Assignment to constant NAME"
// spec QCD-7.83: W0241 for an assignment to a constant
#[test]
fn assignment_to_a_constant() {
    assert_eq!(
        body("k = 1;"),
        of(&[
            (Code::W0241, "Assignment to const"),
            (Code::W0241, "Assignment to constant k"),
        ])
    );
}

// spec FTEF-29.3: an assignment's result is read-only and carries the value stored
#[test]
fn assignment_to_an_assignment() {
    assert_eq!(
        body("(a = 2) = 3;"),
        of(&[
            (Code::W0241, "Assignment to const"),
            (Code::W0241, "Assignment to constant IMMEDIATE"),
        ])
    );
}

// spec FTEF-29.2: `=` casts the value to the target's type, and a refused cast is fatal
#[test]
fn a_plain_store_casts() {
    assert_eq!(
        body("a = s;"),
        of(&[(Code::E0213, "Cannot cast from string to float")])
    );
}

// spec FTEF-29.2: a compound assignment with no opcode for the operands casts the value
#[test]
fn a_compound_store_casts() {
    assert_eq!(
        body("a += s;"),
        of(&[(Code::E0213, "Cannot cast from string to float")])
    );
}

// spec FTEF-29.4: a compound result of a type the left side cannot hold is a fatal error
// spec QCD-4.30: E0324 for an implicit type mismatch on assignment
#[test]
fn an_implicit_mismatch() {
    assert_eq!(
        body("a *= v;"),
        of(&[(
            Code::E0324,
            "Implicit type mismatch on assignment to a. Needed float, got vector."
        )])
    );
}

// spec FTEF-29.2: compound assignments with an opcode for their operands are quiet
#[test]
fn quiet_compound_stores() {
    for case in [
        "v *= v;",
        "v *= a;",
        "self.fl = 1;",
        "e.fv *= 2;",
        "a -= 1;",
        "a |= 4;",
    ] {
        assert_eq!(body(case), [], "{case}");
    }
}

// spec FTEF-29.3: an array named bare is a pointer to its elements, not an l-value
#[test]
fn an_array_head() {
    assert_eq!(
        body("arr = 1;"),
        of(&[(Code::E0213, "Cannot cast from float to float*")])
    );
}

// spec FTEF-29.6: an assignment that is a condition of if, while, do or for warns unless it is
// parenthesised
// spec QCD-7.84: W0242 for an assignment used as a truth value
#[test]
fn assignment_as_a_condition() {
    let suggest = of(&[(
        Code::W0242,
        "suggest parenthesis for assignment used as truth value",
    )]);
    for case in [
        "if (a = 1) a = 2;",
        "while (a = 1) a = 2;",
        "do a = 2; while (a = 1);",
        "for (; a = 1;) a = 2;",
        "for (a = 0; a = 1; a = 2) a = 3;",
        "if (a = 1 && a) a = 2;",
    ] {
        assert_eq!(body(case), suggest, "{case}");
    }
    for quiet in [
        "if ((a = 1)) a = 2;",
        "a = (a = 1) ? 2 : 3;",
        "if (!(a = 1)) a = 2;",
        "for (a = 1; ; a = 2) a = 3;",
    ] {
        assert_eq!(body(quiet), [], "{quiet}");
    }
}

// spec FTEF-29.1: an assignment to a post-increment's result is fatal
// spec QCD-4.28: E0322 for an assignment to a post-increment's result
#[test]
fn assignment_to_a_postincrement() {
    assert_eq!(
        body("a++ = 3;"),
        of(&[(Code::E0322, "Assignment to post-inc result")])
    );
}

// spec FTEF-29.5: a postfix update of a read-only operand is fatal
#[test]
fn postfix_of_a_constant() {
    assert_eq!(
        body("k++;"),
        of(&[(Code::E0322, "postincrement: lhs is readonly")])
    );
    assert_eq!(
        body("k--;"),
        of(&[(Code::E0322, "postdecrement: lhs is readonly")])
    );
    assert_eq!(body("a++; a--; self.fl++;"), []);
}

// spec FTEF-29.2: a compound assignment with no opcode for its types, even after the cast, is
// a fatal error
// spec QCD-4.29: E0323 for a type mismatch on assignment
#[test]
fn an_unsupported_compound() {
    assert_eq!(
        body("s += s;"),
        of(&[(
            Code::E0323,
            "Type mismatch on assignment. string += string is not supported"
        )])
    );
}

fn lax(statement: &str) -> Vec<(Code, String)> {
    diags(&format!(
        "#pragma flag enable lax\n.float fl; entity self;\n\
         void() g = {{ local float a; local vector v; local string s; {statement} }};\n"
    ))
}

// spec FTEF-29.4: under `lax` an implicit mismatch is a warning, and the value stored keeps its
// type, which the store warns of, naming a field `base.field`
// spec FTEF-29.7: the store of a value of another type warns "type mismatch"
// spec QCD-7.85: W0243 for an implicit mismatch under lax
// spec QCD-7.82: W0240 for the store's type mismatch
#[test]
fn a_lax_mismatch_is_stored() {
    assert_eq!(
        lax("a *= v;"),
        of(&[
            (
                Code::W0243,
                "Implicit type mismatch on assignment to a. Needed float, got vector."
            ),
            (Code::W0240, "type mismatch: vector vector to float a"),
        ])
    );
    assert_eq!(
        lax("self.fl *= v;"),
        of(&[
            (
                Code::W0243,
                "Implicit type mismatch on assignment to self.fl. Needed float, got vector."
            ),
            (Code::W0240, "type mismatch: vector vector to float self.fl"),
        ])
    );
    // A lax cast gives the value the target's type, so a plain store has no mismatch.
    assert_eq!(
        lax("a = s;"),
        of(&[(Code::W0229, "Implicit lax cast from string to float")])
    );
}

// spec FTEF-29.3: a pointer or null stored to an array named bare is not an l-value
// spec QCD-4.31: E0325 for a left operand that is no l-value
#[test]
fn an_array_is_no_l_value() {
    for case in ["arr = p;", "arr = 0;"] {
        let source = format!("void() g = {{ local float arr[4]; local float *p; {case} }};\n");
        assert_eq!(
            diags(&source),
            of(&[(
                Code::E0325,
                "left operand must be an l-value (did you mean arr[0]?)"
            )]),
            "{case}"
        );
    }
}

// spec FTEF-29.1: a function's or a field's definition is constant, so read-only, whether or
// not its value is known; a `var` one is not
#[test]
fn functions_and_fields_are_read_only() {
    let source = |statement: &str| {
        format!(
            ".float fld; .vector fv; void() vf = {{ }}; var void() var_f;\n\
             void() g = {{ {statement} }};\n"
        )
    };
    assert_eq!(
        diags(&source("vf = vf;")),
        of(&[
            (Code::W0241, "Assignment to const"),
            (Code::W0241, "Assignment to constant vf"),
        ])
    );
    assert_eq!(
        diags(&source("fld = fv;")),
        of(&[
            (Code::W0241, "Assignment to const"),
            (Code::E0213, "Cannot cast from .vector to .float"),
        ])
    );
    assert_eq!(diags(&source("var_f = vf;")), []);
}

// spec FTEF-29.3: an assignment's result is read-only and carries the value stored, named
// spec FTEF-29.5: a postfix's result is a temporary that messages name by its type
#[test]
fn an_assignments_value_is_read_only() {
    let assigned = |inner: &str| diags(&format!("float a, b; void() g = {{ ({inner}) = 2; }};\n"));
    let constant = |name: &str| {
        of(&[
            (Code::W0241, "Assignment to const"),
            (Code::W0241, &format!("Assignment to constant {name}")),
        ])
    };
    assert_eq!(assigned("a = b"), constant("b"));
    assert_eq!(assigned("a = b++"), constant("float"));
    assert_eq!(
        assigned("a++"),
        of(&[(Code::E0322, "Assignment to post-inc result")])
    );
}

fn fte(statement: &str) -> Vec<(Code, String)> {
    fte_with("", statement)
}

fn fte_lax(statement: &str) -> Vec<(Code, String)> {
    fte_with("#pragma flag enable lax\n", statement)
}

fn fte_with(pragmas: &str, statement: &str) -> Vec<(Code, String)> {
    diags(&format!(
        "#pragma target fte\n{pragmas}.float f; .int fi; .vector fv; entity e;\n\
         float *p; int *q; void *v; float arr[3]; string s; vector w; __variant x; int i;\n\
         void() fn = {{}}; void() g = {{ {statement} }};\n"
    ))
}

fn unsupported(left: &str, op: &str, right: &str) -> (Code, String) {
    (
        Code::E0323,
        format!("Type mismatch on assignment. {left} {op} {right} is not supported"),
    )
}

// spec FTEF-29.2: a compound assignment's types are named by their own names, the right side's
// the left side's when the cast changed it
// spec FTEF-24.11: a declared pointer is `ptr`, an array's address `T*`, a function
// `__function`, a field `.T`
#[test]
fn a_compound_mismatch_names_the_types_own_names() {
    let lax_cast = |from: &str, to: &str| {
        (
            Code::W0229,
            format!("Implicit lax cast from {from} to {to}"),
        )
    };
    let cast = |from: &str, to: &str| (Code::W0220, format!("Implicit cast from {from} to {to}"));
    assert_eq!(
        fte_lax("p *= w;"),
        [
            lax_cast("vector", "float*"),
            unsupported("ptr", "*=", "ptr")
        ]
    );
    assert_eq!(fte_lax("v += s;"), [unsupported("ptr", "+=", "ptr")]);
    assert_eq!(
        fte_lax("q += p;"),
        [cast("float*", "__int32*"), unsupported("ptr", "+=", "ptr")]
    );
    assert_eq!(fte_lax("arr += p;"), [unsupported("float*", "+=", "ptr")]);
    assert_eq!(
        fte_lax("arr += \"a\";"),
        [
            cast("string", "float*"),
            unsupported("float*", "+=", "float*")
        ]
    );
    assert_eq!(
        fte_lax("x += s;"),
        [unsupported("__variant", "+=", "__variant")]
    );
    let assignment_to_const = (Code::W0241, "Assignment to const".to_owned());
    assert_eq!(
        fte_lax("fn += fn;"),
        [
            assignment_to_const.clone(),
            unsupported("__function", "+=", "__function")
        ]
    );
    assert_eq!(
        fte_lax("f += f;"),
        [assignment_to_const, unsupported(".float", "+=", ".float")]
    );
}

// spec FTEF-29.2: an array named bare is its address in a compound assignment, which the store
// then refuses as no l-value
// spec FTEF-29.3: "left operand must be an l-value"
#[test]
fn a_compound_assignment_to_an_array_adds_to_its_address() {
    assert_eq!(
        fte("arr += 1;"),
        of(&[(
            Code::E0325,
            "left operand must be an l-value (did you mean arr[0]?)"
        )])
    );
}

// spec FTEF-29.2: a field updated in place by an opcode of its types is checked no further; with
// none for the value's type it is read, cast and stored
#[test]
fn a_field_is_updated_in_place_by_its_types_opcode() {
    for statement in [
        "e.f += 1;",
        "e.fi += 1.0;",
        "e.f += i;",
        "e.fv *= i;",
        "e.f |= i;",
    ] {
        assert_eq!(fte(statement), [], "{statement}");
    }
    assert_eq!(
        fte("e.f += \"a\";"),
        of(&[(Code::E0213, "Cannot cast from string to float")])
    );
    assert_eq!(
        fte("e.fv += 1.0;"),
        of(&[(Code::W0220, "Implicit cast from float to vector")])
    );
    // The standard target has no in-place opcode, so the field is read and added.
    assert_eq!(
        diags(".int fi; entity e; int i; void() g = { e.fi += i; };\n"),
        of(&[
            (
                Code::W0235,
                "AddInt function not defined: cannot emulate int+int"
            ),
            (
                Code::E0320,
                "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte"
            ),
        ])
    );
}
