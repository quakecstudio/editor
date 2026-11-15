// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use super::Query;
use super::domain::{Field, Fields, ObjectType};
use super::syntax::Expr;
use super::value::{Fails, Pattern, Value};

/// An object with no fields but its type, a brush.
struct Nothing;

impl Fields for Nothing {
    fn field(&self, field: Field) -> Value {
        match field {
            Field::Type => Value::Text(ObjectType::Brush.name().to_owned()),
            _ => Value::Undefined,
        }
    }
}

/// The value of the expression `text`, evaluated for no object.
fn value(text: &str) -> Result<Value, Fails> {
    Expr::parse(text).unwrap().evaluate(&Nothing)
}

fn is_true(text: &str) -> bool {
    value(text) == Ok(Value::Bool(true))
}

fn types(query: &str) -> Vec<ObjectType> {
    Query::parse(query)
        .domain()
        .into_iter()
        .map(|(ty, _)| ty)
        .collect()
}

// No spec statement: P7-08: TrenchBroom's "Text Search and Queries": input that is not an
// expression, that names an unknown field or function, or that is a single name or literal is
// text to search for, a string's quotes removed; `visible` and `locked` are queries on their
// own; surrounding whitespace is ignored and empty input finds nothing
#[test]
fn input_is_a_query_or_text_as_trenchbroom_decides() {
    for (input, text) in [
        ("func_*", "func_*"),
        ("red light", "red light"),
        ("light", "light"),
        ("42", "42"),
        ("-90", "-90"),
        ("\"red light\"", "red light"),
        ("\"visible\"", "visible"),
        ("light is red", "light is red"),
        ("clasname is \"light\"", "clasname is \"light\""),
        ("frobnicate(1)", "frobnicate(1)"),
        ("light%", "light%"),
        ("  light  ", "light"),
    ] {
        assert_eq!(Query::parse(input), Query::text(text), "{input}");
    }
    assert_eq!(Query::parse(""), Query(Expr::Literal(Value::Bool(false))));
    assert_eq!(Query::parse("   "), Query::text(""));
    assert_ne!(Query::parse("visible"), Query::text("visible"));
    assert_ne!(
        Query::parse("classname is \"light\""),
        Query::text("classname is \"light\"")
    );
    let deep = format!("{}1{}", "(".repeat(100), ")".repeat(100));
    assert_eq!(Query::parse(&deep), Query::text(&deep), "too deep to read");
    let not_deep = format!("{}1{}", "(".repeat(10), ")".repeat(10));
    assert!(Expr::parse(&not_deep).is_some());
}

// No spec statement: P7-08: a type name compared with `type` needs no quotes, alone or in an
// array, but only there
#[test]
fn type_names_compared_with_type_need_no_quotes() {
    for (bare, quoted) in [
        ("type is brush", "type is \"brush\""),
        ("type == brush", "type == \"brush\""),
        ("brush != type", "\"brush\" != type"),
        ("type in [brush, patch]", "type in [\"brush\", \"patch\"]"),
        ("[group] contains type", "[\"group\"] contains type"),
    ] {
        assert_eq!(Query::parse(bare), Query::parse(quoted), "{bare}");
    }
    assert_eq!(
        Query::parse("classname is brush"),
        Query::text("classname is brush"),
        "brush is a name elsewhere, which no field has"
    );
}

// No spec statement: P7-08: TrenchBroom's "Which Objects a Query Searches": a field limits the
// search to the types that have it, a part only reading `type` to those it is true for; `&&`
// meets and `||` joins them; nothing limiting searches every type; faces and layers only on
// their own
#[test]
fn a_query_searches_the_types_it_applies_to() {
    use ObjectType::{Brush, Entity, Face, Group, Layer, World};
    let objects = vec![World, Group, Entity, Brush];
    for (query, expected) in [
        ("classname is \"light\"", vec![World, Entity]),
        ("materials like \"*sky*\"", vec![Brush]),
        ("material like \"sky*\"", vec![Face]),
        ("normal.z > 0.7", vec![Face]),
        ("type is face && tags contains \"Clip\"", vec![Face]),
        ("type is layer && name like \"Combat*\"", vec![Layer]),
        ("name like \"Combat*\"", vec![Group]),
        ("type in [layer, face]", vec![]),
        ("classname is \"light\" && materials like \"*sky*\"", vec![]),
        (
            "!(classname is \"func_detail\") || materials like \"*sky*\"",
            vec![World, Entity, Brush],
        ),
        ("type != brush", vec![World, Group, Entity]),
        ("type is brush", vec![Brush]),
        ("visible", objects.clone()),
        ("1 == 1", objects.clone()),
        ("1 == 2", vec![]),
        (
            "bounds intersects bbox(vec(0, 0, 0), vec(1, 1, 1))",
            vec![Group, Entity, Brush],
        ),
        ("light", objects),
    ] {
        assert_eq!(types(query), expected, "{query}");
    }
}

// No spec statement: P7-08: negating a condition keeps it to the types it applies to:
// `!(classname is "func_detail")` finds the world and entities, never brushes, which lack a
// classname
#[test]
fn negation_keeps_to_the_types_the_condition_applies_to() {
    let query = Query::parse("!(classname is \"func_detail\")");
    let domain = query.domain();
    assert_eq!(
        domain.iter().map(|(ty, _)| *ty).collect::<Vec<_>>(),
        [ObjectType::World, ObjectType::Entity]
    );
    let or = Query::parse("!(classname is \"x\") || materials like \"*sky*\"");
    let brush = or
        .domain()
        .into_iter()
        .find(|(ty, _)| *ty == ObjectType::Brush)
        .unwrap()
        .1;
    assert!(
        !brush.holds(&Nothing),
        "the classname part never finds a brush"
    );
}

// No spec statement: P7-08: TrenchBroom's "Comparison Terms": undefined below everything, then
// null; a boolean, number or string converted as its table says; `1 == "asdf"` an error;
// arrays and vectors lexicographic
#[test]
fn comparisons_follow_the_manual_s_table() {
    for text in [
        "true > false",
        "true == \"true\"",
        "true == \"True\"",
        "true == \"asdf\"",
        "true != \"\"",
        "true != \"false\"",
        "true == 1",
        "true == 2",
        "true != 0",
        "1 == \"1\"",
        "1 == \"1.0\"",
        "1 < \"2\"",
        "\"asdf\" == \"asdf\"",
        "\"asdf\" < \"bsdf\"",
        "null == null",
        "null != undefined",
        "null > undefined",
        "null < -1",
        "null < \"asdf\"",
        "undefined == undefined",
        "undefined != 0",
        "undefined < -1",
        "\"asdf\" > undefined",
        "[1, 2, 3] == [1, 2, 3]",
        "[1, 2, 3] < [2, 2, 3]",
        "[1, 2] < [1, 2, 3]",
        "vec(1, 2, 3) == vec(1, 2, 3)",
        "vec(1, 2, 3) < vec(4, 5, 6)",
        "vec(1, 2, 3) == \"1 2 3\"",
        "bbox(vec(0, 0, 0), vec(1, 1, 1)) == bbox(vec(1, 1, 1), vec(0, 0, 0))",
        "1 is \"1\"",
        "1 in [1, 2] is true",
        "\"a\" is 'a'",
    ] {
        assert!(is_true(text), "{text}");
    }
    assert_eq!(value("1 == \"asdf\""), Err(Fails));
    assert_eq!(value("[1] == 1"), Err(Fails));
    assert_eq!(value("vec(1, 2, 3) == \"1 2\""), Err(Fails));
}

// No spec statement: P7-08: TrenchBroom's "Algebraic Terms", "Unary Operator Terms", "Logical
// Terms" and "Vec3": number strings convert, strings and arrays join, vectors add and scale,
// undefined propagates and null is an error; `&&` and `||` short-circuit
#[test]
fn arithmetic_unary_and_logic_follow_the_manual() {
    for (text, expected) in [
        ("2 * 3 + 4", Value::Number(10.0)),
        ("2 * (3 + 4)", Value::Number(14.0)),
        ("\"1.5\" + 1", Value::Number(2.5)),
        ("10 / 4", Value::Number(2.5)),
        ("\"a\" + \"b\"", Value::Text("ab".to_owned())),
        (
            "[1] + [2]",
            Value::Array(vec![Value::Number(1.0), Value::Number(2.0)]),
        ),
        (
            "vec(1, 2, 3) + vec(4, 5, 6)",
            Value::Vector([5.0, 7.0, 9.0]),
        ),
        ("vec(4, 5, 6) - vec(1, 2, 3)", Value::Vector([3.0; 3])),
        ("vec(1, 2, 3) * 2", Value::Vector([2.0, 4.0, 6.0])),
        (
            "vec(1, 2, 3) * vec(4, 5, 6)",
            Value::Vector([4.0, 10.0, 18.0]),
        ),
        ("\"1 2 3\" + vec(0, 0, 16)", Value::Vector([1.0, 2.0, 19.0])),
        ("vec(1, 1, 1) * \"2\"", Value::Vector([2.0; 3])),
        ("-vec(1, 2, 3)", Value::Vector([-1.0, -2.0, -3.0])),
        ("-'1'", Value::Number(-1.0)),
        ("+true", Value::Number(1.0)),
        ("-true", Value::Number(-1.0)),
        ("!true", Value::Bool(false)),
        ("!undefined", Value::Undefined),
        ("1 + undefined", Value::Undefined),
        ("\"a\" + undefined", Value::Undefined),
        ("true && undefined", Value::Undefined),
        ("false && undefined", Value::Bool(false)),
        ("true || undefined", Value::Bool(true)),
        ("false || undefined", Value::Undefined),
        ("undefined && true", Value::Undefined),
        ("null || true", Value::Bool(true)),
        ("false && 1", Value::Bool(false)),
        ("vec(1, 2, 3)[0]", Value::Number(1.0)),
        ("vec(1, 2, 3)[\"y\"]", Value::Number(2.0)),
        ("vec(1, 2, 3).z", Value::Number(3.0)),
        ("[7, 8, 9][-1]", Value::Number(9.0)),
        ("[7, 8, 9][1.7]", Value::Number(8.0)),
        (
            "bbox(vec(0, 0, 0), vec(1, 1, 1)).max",
            Value::Vector([1.0; 3]),
        ),
        (
            "bbox(vec(1, 1, 1), vec(0, 0, 0))[\"min\"]",
            Value::Vector([0.0; 3]),
        ),
        ("vec(0, 0, 0) distanceTo vec(3, 4, 0)", Value::Number(5.0)),
        ("distanceTo(\"0 0 0\", \"3 4 0\")", Value::Number(5.0)),
        ("vec(1, undefined, 3)", Value::Undefined),
        ("bbox(undefined, vec(1, 1, 1))", Value::Undefined),
        ("undefined[0]", Value::Undefined),
        ("properties.light", Value::Undefined),
    ] {
        assert_eq!(value(text), Ok(expected), "{text}");
    }
    for text in [
        "1 + null",
        "!null",
        "!1",
        "-[1]",
        "true && 1",
        "vec(1, 2, 3) + 1",
        "[7, 8, 9][3]",
        "[1][undefined]",
        "vec(1, 2, 3).w",
        "vec(1, 2)",
        "vec(\"a\", 2, 3)",
        "frobnicate(1)",
        "nope",
        "bbox(vec(0, 0, 0), vec(1, 1, 1)) intersects 1",
        "vec(0, 0, 0) distanceTo 1",
        "-bbox(vec(0, 0, 0), vec(1, 1, 1))",
    ] {
        assert_eq!(value(text), Err(Fails), "{text}");
    }
}

// No spec statement: P7-08: TrenchBroom's "Pattern Matching" and "Membership Testing": `like`
// ignores case, matches wholly with a wildcard and as a substring without, looks into arrays
// and maps and never fails; `contains` and `in` test arrays, map keys and boxes; `intersects`
// tests overlap
#[test]
fn like_contains_and_intersects_follow_the_manual() {
    for text in [
        "\"Hello World\" like \"hello*\"",
        "\"Hello World\" like \"*world\"",
        "\"Hello World\" like \"h?llo*\"",
        "\"func_detail\" like \"detail\"",
        "\"Hello World\" like \"LO wo\"",
        "\"func_detail\" like \"func_detail\"",
        "\"light2\" like \"light%\"",
        "\"light\" like \"light%*\"",
        "\"light12\" like \"light%*\"",
        "[\"a\", 1, \"trigger_once\"] like \"*trigger*\"",
        "[1, 2, 3] contains 2",
        "[1, 2, 3] contains \"2\"",
        "2 in [1, 2, 3]",
        "bbox(vec(0, 0, 0), vec(2, 2, 2)) contains vec(1, 1, 1)",
        "bbox(vec(0, 0, 0), vec(3, 3, 3)) contains bbox(vec(1, 1, 1), vec(2, 2, 2))",
        "vec(1, 1, 1) in bbox(vec(0, 0, 0), vec(2, 2, 2))",
        "bbox(vec(0, 0, 0), vec(2, 2, 2)) intersects bbox(vec(1, 1, 1), vec(3, 3, 3))",
        "contains([1], 1)",
    ] {
        assert!(is_true(text), "{text}");
    }
    for text in [
        "\"light12x\" like \"light%*\"",
        "\"lightx\" like \"light%\"",
        "1 like \"1\"",
        "\"a\" like 1",
        "undefined like \"*\"",
        "[1, 2] contains 3",
        "\"a\" contains \"a\"",
        "bbox(vec(0, 0, 0), vec(1, 1, 1)) contains vec(2, 0, 0)",
        "bbox(vec(0, 0, 0), vec(2, 2, 2)) contains bbox(vec(1, 1, 1), vec(3, 3, 3))",
        "bbox(vec(0, 0, 0), vec(1, 1, 1)) intersects bbox(vec(2, 2, 2), vec(3, 3, 3))",
    ] {
        assert!(!is_true(text), "{text}");
    }
    assert_eq!(value("undefined intersects 1"), Ok(Value::Undefined));
    let pairs = Value::Map(vec![("target".to_owned(), Value::Text("door1".to_owned()))]);
    assert!(pairs.like(&Value::Text("target".to_owned())), "by key");
    assert!(pairs.like(&Value::Text("door*".to_owned())), "by value");
    assert!(pairs.contains(&Value::Text("target".to_owned())));
    assert_eq!(
        pairs.index(&Value::Text("missing".to_owned())),
        Ok(Value::Undefined)
    );
}

// No spec statement: P7-08: a pattern's backslash makes the wildcard or backslash after it a
// character, and any other backslash is one; TrenchBroom's Liquid tag pattern `\**` matches a
// texture starting with `*`
#[test]
fn pattern_escapes_are_characters() {
    assert!(Pattern::new("quake\\*").matches("quake*"));
    assert!(!Pattern::new("quake\\*").matches("quakes"));
    assert!(Pattern::new("quake\\\\*").matches("quake\\gfx"));
    assert!(Pattern::new("id1\\gfx.wad").matches("ID1\\GFX.WAD"));
    assert!(Pattern::new("\\%").matches("%"));
    assert!(Pattern::new("\\?").matches("?"));
    assert!(Pattern::new("\\**").matches("*water1"));
    assert!(!Pattern::new("\\**").matches("water1"));
    assert!(Pattern::new("hint*").matches("HINTSKIP"));
    assert!(!Pattern::new("clip").matches("clip2"));
    assert!(Pattern::new("trail\\").matches("trail\\"));
    assert!(Pattern::new("").finds("anything"));
    assert!(!Pattern::new("").matches("anything"));
}
