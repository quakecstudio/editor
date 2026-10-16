// SPDX-License-Identifier: GPL-3.0-or-later

//! `#if` expressions (PRJ-5.5).

use std::collections::HashMap;

use crate::cond::{atoi, eval};

fn macros() -> HashMap<String, String> {
    [
        ("ONE", "1"),
        ("TWO", "2 // two"),
        ("EMPTY", ""),
        ("WORD", "abc"),
        ("NEG", "-3"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect()
}

// spec PRJ-5.5: integers, names, defined, parentheses, ! and the comparisons, with C's precedence
#[test]
fn expressions() {
    let m = macros();
    let cases = [
        ("1", true),
        ("0", false),
        ("ONE", true),
        ("EMPTY", false),
        ("UNDEFINED", false),
        ("defined EMPTY", true),
        ("defined(UNDEFINED)", false),
        ("defined ( ONE )", true),
        ("!defined(ONE)", false),
        ("!!TWO", true),
        ("TWO == 2", true),
        ("TWO != 2", false),
        ("ONE < TWO", true),
        ("TWO <= 2", true),
        ("ONE > TWO", false),
        ("TWO >= 3", false),
        ("TWO >= 2", true),
        ("ONE != 2", true),
        ("2 < 2", false),
        ("2 > 2", false),
        ("2 <= 1", false),
        ("1 >= 2", false),
        ("NEG < 0", true),
        ("0 || 1 && 0", false),
        ("1 || 0 && 0", true),
        ("(1 || 0) && 0", false),
        ("1 == 1 && 2 == 2", true),
        ("1 < 2 == 1", true),
        ("defined(ONE) && !defined(TWO) || TWO == 2", true),
        ("ONE // trailing comment", true),
        ("ONE /* block */", true),
    ];
    for (expr, want) in cases {
        assert_eq!(eval(expr, &m), want, "{expr}");
    }
}

// spec PRJ-5.5: a name whose macro text is not a number is 0, and so is a number with a suffix
#[test]
fn values() {
    let m = macros();
    assert!(!eval("WORD", &m));
    assert!(eval("WORD == 0", &m));
    assert!(!eval("0x10", &m));
    assert!(eval("12abc == 12", &m));
}

// spec PRJ-5.5: an expression that does not parse is false
#[test]
fn malformed_is_false() {
    let m = macros();
    for expr in [
        "",
        "(",
        "(1",
        "1)",
        "1 +",
        "&& 1",
        "defined",
        "defined(",
        "defined(ONE",
        "defined 1",
        "1 2",
        "@",
        "== 1",
        "!",
    ] {
        assert!(!eval(expr, &m), "{expr:?}");
    }
    let deep = format!("{}1{}", "(".repeat(100), ")".repeat(100));
    assert!(!eval(&deep, &m));
    let shallow = format!("{}1{}", "(".repeat(10), ")".repeat(10));
    assert!(eval(&shallow, &m));
    assert!(!eval(&"!".repeat(100), &m));
}

// spec PRJ-5.5: a macro's text is read as a decimal integer, as atoi reads it
#[test]
fn atoi_reads_like_c() {
    assert_eq!(atoi("42"), 42);
    assert_eq!(atoi("  -7x"), -7);
    assert_eq!(atoi("+5"), 5);
    assert_eq!(atoi(""), 0);
    assert_eq!(atoi("-"), 0);
    assert_eq!(atoi("x1"), 0);
    assert_eq!(atoi("99999999999999999999999"), i64::MAX);
    assert_eq!(atoi("-99999999999999999999999"), -i64::MAX);
}
