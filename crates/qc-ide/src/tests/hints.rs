// SPDX-License-Identifier: GPL-3.0-or-later

//! Parameter-name inlay hints.

use qc_diag::Span;

use super::{nth, one, path};
use crate::ParameterHint;

// spec QC-13.5: each argument is named by the parameter at its position, from the function's
// declaration, a builtin's included
#[test]
fn hints_name_each_argument() {
    let text = "void(entity e, vector o) setorigin = #2;\nentity self;\n\
                void(float amount, entity o) hurt = { };\n\
                void() f = { setorigin(self, '0 0 0'); hurt( 5, self); };\n";
    let mut host = one(text);
    let hints = host.parameter_hints(&path("a.qc"), Span::new(0, text.len()));
    assert_eq!(
        hints,
        [
            ParameterHint {
                offset: nth(text, "self, '", 0),
                name: "e".to_owned()
            },
            ParameterHint {
                offset: nth(text, "'0 0 0'", 0),
                name: "o".to_owned()
            },
            ParameterHint {
                offset: nth(text, "5, self", 0),
                name: "amount".to_owned()
            },
            ParameterHint {
                offset: nth(text, "self); }", 0),
                name: "o".to_owned()
            },
        ]
    );
}

// spec QC-13.2: a prototype with no names gives none; the defining declaration's names are
// used
#[test]
fn hints_come_from_the_declaration_that_names_parameters() {
    let text = "void(float, float) g;\nvoid() f = { g(1, 2); };\nvoid(float x, float y) g = { };\n";
    let mut host = one(text);
    let hints = host.parameter_hints(&path("a.qc"), Span::new(0, text.len()));
    let names: Vec<&str> = hints.iter().map(|h| h.name.as_str()).collect();
    assert_eq!(names, ["x", "y"]);
}

// spec QC-13.5: an argument that is the parameter's own name, a call of something that is not
// a function, and a call outside the range get no hint
#[test]
fn hints_are_left_out_where_they_say_nothing() {
    let text = "void(float amount) hurt = { };\nfloat amount;\n\
                void() f = { hurt(amount); local void() v; v(); };\nvoid() g = { hurt(1); };\n";
    let mut host = one(text);
    let in_f = Span::new(nth(text, "void() f", 0), nth(text, "void() g", 0) - 1);
    assert!(host.parameter_hints(&path("a.qc"), in_f).is_empty());
    let all = host.parameter_hints(&path("a.qc"), Span::new(0, text.len()));
    assert_eq!(all.len(), 1);
    assert!(host.parameter_hints(&path("none.qc"), in_f).is_empty());
}
