// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: fixtures are known"
)]

use qc_preproc::is_opcode_name;

use crate::check::convert::{Choice, Operand, choose_opcode, conversion_cost};
use crate::check::etype::Etype;
use crate::check::opcodes::{OPCODES, candidates, index_of};

fn value(etype: Etype) -> Operand {
    Operand {
        etype,
        holds: None,
        null: false,
    }
}

fn chosen(spelling: &str, lhs: Operand, rhs: Operand) -> Choice {
    choose_opcode(&candidates(spelling), lhs, rhs, false).unwrap()
}

fn opname(choice: Choice) -> &'static str {
    OPCODES[choice.index()].opname
}

// spec FTEF-23.1: the table is pr_opcodes[] in order, 413 rows, each opname one #pragma opcode
// takes
#[test]
fn the_table_is_fteqccs_in_order() {
    assert_eq!(OPCODES.len(), 413);
    assert!(OPCODES.iter().all(|op| is_opcode_name(op.opname)));
    assert_eq!(index_of("DONE"), Some(0));
    assert_eq!(index_of("MUL_F"), Some(1));
    assert_eq!(index_of("ADD_F"), Some(6));
    assert_eq!(index_of("EQ_F"), Some(10));
}

// spec FTEF-23.1: an operator's candidates are its spelling's rows of a binary class, in table
// order
#[test]
fn candidates_are_the_spellings_rows_in_order() {
    let plus = candidates("+");
    assert_eq!(OPCODES[plus[0]].opname, "ADD_F");
    assert_eq!(OPCODES[plus[1]].opname, "ADD_V");
    assert!(plus.windows(2).all(|w| w[0] < w[1]));
    assert!(plus.iter().all(|&i| OPCODES[i].name == "+"));
    assert!(candidates("=").is_empty());
    assert!(candidates("nope").is_empty());
}

// spec FTEF-23.2: double 15, float 10, uint64 6, int64 3, uint 1, int 0, others unranked
#[test]
fn numeric_ranking() {
    let ranks = [
        (Etype::Double, Some(15)),
        (Etype::Float, Some(10)),
        (Etype::Uint64, Some(6)),
        (Etype::Int64, Some(3)),
        (Etype::Uint, Some(1)),
        (Etype::Integer, Some(0)),
        (Etype::Vector, None),
        (Etype::String, None),
        (Etype::Enum, None),
        (Etype::Variant, None),
    ];
    for (etype, rank) in ranks {
        assert_eq!(etype.numeric_ranking(), rank, "{etype:?}");
    }
}

// spec FTEF-23.3: identity 0, a field holding the type in a class 1, null 2, up and down the
// ranking, variant 9 and 10, float to vector 200, vector to float 201, else none
#[test]
fn conversion_costs() {
    let cost = |from: Operand, to| conversion_cost(from, to, false);
    assert_eq!(cost(value(Etype::Float), Etype::Float), Some(0));
    let field = Operand {
        holds: Some(Etype::Float),
        ..value(Etype::Field)
    };
    assert_eq!(conversion_cost(field, Etype::Float, true), Some(1));
    assert_eq!(conversion_cost(field, Etype::Float, false), None);
    let null = Operand {
        null: true,
        ..value(Etype::Integer)
    };
    assert_eq!(cost(null, Etype::String), Some(2));
    assert_eq!(cost(value(Etype::Integer), Etype::Float), Some(13));
    assert_eq!(cost(value(Etype::Float), Etype::Integer), Some(160));
    assert_eq!(cost(value(Etype::Uint), Etype::Int64), Some(5));
    assert_eq!(cost(value(Etype::String), Etype::Variant), Some(9));
    assert_eq!(cost(value(Etype::Variant), Etype::String), Some(10));
    assert_eq!(cost(value(Etype::Float), Etype::Vector), Some(200));
    assert_eq!(cost(value(Etype::Vector), Etype::Float), Some(201));
    assert_eq!(cost(value(Etype::String), Etype::Float), None);
    assert_eq!(cost(value(Etype::Entity), Etype::String), None);
}

// spec FTEF-23.4: the first candidate needing no conversion, else the fewest; none is a mismatch
// on the first
#[test]
fn choosing_an_opcode() {
    let f = value(Etype::Float);
    let v = value(Etype::Vector);
    let i = value(Etype::Integer);
    let s = value(Etype::String);
    assert_eq!(opname(chosen("+", f, f)), "ADD_F");
    assert_eq!(opname(chosen("+", v, v)), "ADD_V");
    assert_eq!(opname(chosen("*", f, v)), "MUL_FV");
    assert_eq!(opname(chosen("*", v, v)), "MUL_V");
    assert_eq!(opname(chosen("+", i, i)), "ADD_I");
    assert_eq!(opname(chosen("+", f, i)), "ADD_FI");
    assert_eq!(opname(chosen("==", s, s)), "EQ_S");
    assert_eq!(opname(chosen("+", s, f)), "ADD_SF");
    let mismatch = chosen("+", s, value(Etype::Entity));
    assert!(matches!(mismatch, Choice::Mismatch(_)));
    assert_eq!(opname(mismatch), "ADD_F");
    assert_eq!(choose_opcode(&[], f, f, false), None);
}
