// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use super::super::{Facts, Item, Key, Pin, Table, check_table};
use super::extract;

/// A positional reading of `array`, with `PF_Fixme` absent.
fn positional(array: &str) -> Table {
    Table {
        kind: "ssqc".to_owned(),
        array: Some(array.to_owned()),
        absent: vec!["PF_Fixme".to_owned()],
        ..Table::default()
    }
}

/// A reading of rows `{ name, ssqc, csqc, number, type, extension }`, with `NULL` absent.
fn rows() -> Table {
    Table {
        kind: "ssqc".to_owned(),
        array: Some("defs".to_owned()),
        absent: vec!["NULL".to_owned()],
        name: Some(0),
        function: Some(1),
        number: Some(3),
        signature: Some(4),
        extension: Some(5),
        ..Table::default()
    }
}

/// A reading of name patches `{ name, number, target }` against an earlier table.
fn patches() -> Table {
    Table {
        kind: "ssqc".to_owned(),
        array: Some("ex".to_owned()),
        name: Some(0),
        number: Some(1),
        target: Some(2),
        target_of: Some("base".to_owned()),
        ..Table::default()
    }
}

fn read(text: &str, table: &Table) -> Facts {
    extract(text, table, None).unwrap()
}

fn error(text: &str, table: &Table) -> String {
    extract(text, table, None).unwrap_err()
}

fn keys(facts: &Facts) -> Vec<Key> {
    facts.builtins.iter().map(|b| b.key.clone()).collect()
}

fn names(facts: &Facts) -> Vec<&str> {
    facts.builtins.iter().map(|b| b.name.as_str()).collect()
}

fn pin(path: &str, tables: Vec<Table>) -> Pin {
    Pin {
        id: "src".to_owned(),
        repo: "o/r".to_owned(),
        commit: "0".repeat(40),
        path: path.to_owned(),
        sha256: "0".repeat(64),
        license: "GPL-2.0-or-later".to_owned(),
        license_file: "LICENSE.txt".to_owned(),
        license_sha256: "0".repeat(64),
        tables,
    }
}

// spec ENGT-2.11: comments are dropped unread, so an element or a name inside one is not
// read, while a `//` inside a string literal is text
#[test]
fn comments_are_dropped_unread() {
    let text = "\
static builtin_t pr_builtin[] = {
\tPF_Fixme,
\tPF_one, // void() one = #1
\t/* PF_hidden,
\t   PF_also_hidden, */
\tPF_two,\t\t// PF_three,
};
";
    let facts = read(text, &positional("pr_builtin"));
    assert_eq!(names(&facts), ["PF_one", "PF_two"]);
    assert_eq!(keys(&facts), [Key::Number(1), Key::Number(2)]);
    let text = "x defs[] = { {\"a\", PF_a, NULL, 1, \"void(string s = \\\"//\\\")\"} };\n";
    let facts = read(text, &rows());
    assert_eq!(facts.builtins[0].signature, "void(string s = \\\"//\\\")");
}

// spec ENGT-2.11: a QuakeC source's reading needs defines and no C column, a C source's an
// array and no defines, and a file of any other kind is an error
#[test]
fn a_reading_fits_its_file() {
    let qc = Table {
        kind: "ssqc".to_owned(),
        defines: Some(Vec::new()),
        ..Table::default()
    };
    assert!(check_table(&pin("defs.qc", vec![qc.clone()]), &qc, &[]).is_ok());
    let c = positional("pr_builtin");
    assert!(check_table(&pin("pr_cmds.c", vec![c.clone()]), &c, &[]).is_ok());
    let mixed = Table {
        defines: Some(Vec::new()),
        ..positional("pr_builtin")
    };
    let cases = [
        ("defs.qc", mixed.clone(), "no C columns"),
        ("pr_cmds.c", mixed, "no defines"),
        (
            "pr_cmds.c",
            Table {
                array: None,
                ..positional("x")
            },
            "needs an array",
        ),
        ("defs.h", positional("x"), "neither QuakeC nor C"),
    ];
    for (path, table, needle) in cases {
        let err = check_table(&pin(path, vec![table.clone()]), &table, &[]).unwrap_err();
        assert!(err.contains(needle), "{path}: {err}");
    }
}

// spec ENGT-2.11: a directive continued by a final backslash is one directive
#[test]
fn a_directive_continues_over_a_backslash() {
    let text = "\
#define PAIR(x) \\
\tx, NULL
defs_t defs[] = { {\"one\", PAIR(PF_one), 1} };
";
    let facts = read(text, &rows());
    assert_eq!(keys(&facts), [Key::Number(1)]);
}

// spec ENGT-2.12: object-like and function-like macros expand, nested and in file order, a
// later #define replacing an earlier one and #undef removing one
#[test]
fn macros_expand() {
    let text = "\
#define PF_SSQC(x) x,NULL
#define PF_CSQC(x) NULL,x
#define PF_BOTH(x) x,x
#define NUM 7
#define NUM 8
#define D(t, d) t, d
#define GONE PF_gone
#undef GONE
defs_t defs[] = {
\t{\"a\", PF_SSQC(PF_a), 1},
\t{\"b\", PF_CSQC(PF_b), 2},
\t{\"c\", PF_BOTH(PF_c), NUM, D(\"float()\", \"described\")},
\t{\"d\", PF_SSQC(GONE), 3},
};
";
    let typed = Table {
        extension: None,
        ..rows()
    };
    let facts = read(text, &typed);
    assert_eq!(names(&facts), ["a", "c", "d"]);
    assert_eq!(facts.builtins[1].key, Key::Number(8));
    assert_eq!(facts.builtins[1].signature, "float()");
    let client = Table {
        function: Some(2),
        ..typed
    };
    assert_eq!(names(&read(text, &client)), ["b", "c"]);
}

// spec ENGT-2.12: a macro defined between `[] =` and `{` takes effect, and a self-referring
// macro is left alone after one expansion
#[test]
fn macros_inside_the_declaration() {
    let text = "\
static struct { const char *name; } list[] =
#define PF_NoSSQC NULL
#define PF_same PF_same
{
\t{\"a\", PF_NoSSQC, PF_x, 1},
\t{\"b\", PF_same, PF_x, 2},
};
";
    let table = Table {
        array: Some("list".to_owned()),
        ..rows()
    };
    let facts = read(text, &table);
    assert_eq!(names(&facts), ["b"]);
}

// spec ENGT-2.12: using a macro defined inside an #if block, or one whose body has # or ##, is
// an error; an unused one is not, and an #if inside the array is an error
#[test]
fn conditional_and_stringizing_macros_are_errors() {
    let conditional = "\
#ifdef SOMETHING
#define PF_X PF_y
#endif
#define NAME(n) #n
#define PASTE(a, b) a##b
defs_t defs[] = { {\"a\", PF_ok, NULL, 1} };
";
    assert_eq!(keys(&read(conditional, &rows())), [Key::Number(1)]);
    let used = conditional.replace("PF_ok", "PF_X");
    assert!(error(&used, &rows()).contains("#if"));
    let stringized = conditional.replace("PF_ok", "NAME(x)");
    assert!(error(&stringized, &rows()).contains("# or ##"));
    let pasted = conditional.replace("PF_ok", "PASTE(PF_, a)");
    assert!(error(&pasted, &rows()).contains("# or ##"));
    let inside = "defs_t defs[] = {\n#ifdef X\n{\"a\", PF_a, NULL, 1},\n#endif\n};\n";
    assert!(error(inside, &rows()).contains("#ifdef inside the array"));
    let deep = "#define A B\n#define B A\nb_t x[] = { PF_Fixme, A };\n";
    assert!(read(deep, &positional("x")).builtins.len() == 1);
    let arity = "#define F(a, b) a\ndefs_t defs[] = { {\"a\", F(PF_a), NULL, 1} };\n";
    assert!(error(arity, &rows()).contains("takes 2 arguments"));
}

// spec ENGT-2.13: elements split at top-level commas, a final comma ending none, and a column
// a row lacks is zero; a missing array or an initialiser with no end is an error
#[test]
fn elements_and_columns() {
    let text = "defs_t defs[] = { {\"a\", PF_a}, {\"b\", PF_b, NULL, 2,}, };\n";
    let facts = read(text, &rows());
    assert_eq!(
        facts.builtins,
        [
            Item {
                key: Key::Name("a".to_owned()),
                name: "a".to_owned(),
                signature: String::new(),
                extensions: Vec::new(),
            },
            Item {
                key: Key::Number(2),
                name: "b".to_owned(),
                signature: String::new(),
                extensions: Vec::new(),
            },
        ]
    );
    assert!(error("int other[] = { 1 };\n", &rows()).contains("no array defs"));
    assert!(error("defs_t defs[];\n", &rows()).contains("no array defs"));
    assert!(error("defs_t defs[] = { {\"a\", PF_a, NULL, 1}", &rows()).contains("no end"));
    assert!(error("defs_t defs[] = 3;\n", &rows()).contains("not followed by {"));
}

// spec ENGT-2.14: element N of a positional array is builtin #N named by its function, an
// absent function or element 0 is none, and an element that is not one name is an error
#[test]
fn positional_arrays() {
    let text = "builtin_t pr_builtin[] = { PF_zero, PF_one, PF_Fixme, PF_three };\n";
    let facts = read(text, &positional("pr_builtin"));
    assert_eq!(
        facts.builtins,
        [
            Item {
                key: Key::Number(1),
                name: "PF_one".to_owned(),
                signature: String::new(),
                extensions: Vec::new(),
            },
            Item {
                key: Key::Number(3),
                name: "PF_three".to_owned(),
                signature: String::new(),
                extensions: Vec::new(),
            },
        ]
    );
    assert!(facts.extensions.is_empty());
    let bad = "builtin_t pr_builtin[] = { PF_Fixme, &PF_one };\n";
    assert!(error(bad, &positional("pr_builtin")).contains("element 1"));
    let row = "builtin_t pr_builtin[] = { PF_Fixme, {PF_one} };\n";
    assert!(error(row, &positional("pr_builtin")).contains("element 1"));
}

// spec ENGT-2.15: a row's name joins its literals, NULL ends the list, an absent or missing
// function gives nothing, number 0 or none keys by name, and the first of a repeated key wins
#[test]
fn rows_give_keys_and_names() {
    let text = "\
defs_t defs[] = {
\t{\"str\" \"len\", PF_strlen, NULL, 114},
\t{\"absent\", NULL, PF_x, 5},
\t{\"named\", PF_named, NULL, 0},
\t{\"strlen2\", PF_strlen2, NULL, 114},
\t{\"nofunction\"},
\t{NULL, PF_after, NULL, 9},
\t{\"after\", PF_after, NULL, 10},
};
";
    let facts = read(text, &rows());
    assert_eq!(names(&facts), ["strlen", "named"]);
    assert_eq!(
        keys(&facts),
        [Key::Number(114), Key::Name("named".to_owned())]
    );
    let without_function = Table {
        function: None,
        ..rows()
    };
    assert_eq!(
        names(&read(text, &without_function)),
        ["strlen", "absent", "named", "nofunction"]
    );
}

// spec ENGT-2.15: the signature is the type string's last line, whitespace collapsed, and the
// extension column is the builtin's one extension
#[test]
fn rows_give_signatures_and_extensions() {
    let text = "\
defs_t defs[] = {
\t{\"findfloat\", PF_f, NULL, 98, \"#define findentity findfloat\\nentity(entity start,   .float fld)\"},
\t{\"multicast\", PF_m, NULL, 82, \"#define unicast(pl) x\\n\" \"void(vector where, float set)\", \"Sends it.\"},
\t{\"sin\", PF_s, NULL, 60, \"float(float a)\", DP_QC_SINCOSSQRTPOW},
};
";
    let typed = Table {
        extension: None,
        ..rows()
    };
    let facts = read(text, &typed);
    let signatures: Vec<&str> = facts
        .builtins
        .iter()
        .map(|b| b.signature.as_str())
        .collect();
    assert_eq!(
        signatures,
        [
            "entity(entity start, .float fld)",
            "void(vector where, float set)",
            "float(float a)"
        ]
    );
    assert!(facts.builtins.iter().all(|b| b.extensions.is_empty()));
    let sin = text
        .lines()
        .filter(|l| !l.contains("PF_f") && !l.contains("PF_m"));
    let sin = read(&sin.collect::<Vec<_>>().join("\n"), &rows());
    assert_eq!(sin.builtins[0].extensions, ["DP_QC_SINCOSSQRTPOW"]);
    let sentence = Table {
        signature: Some(5),
        extension: None,
        ..rows()
    };
    let described = "defs_t defs[] = { {\"a\", PF_a, NULL, 1, \"t\", \"Not a type.\"} };\n";
    assert!(error(described, &sentence).contains("type string"));
}

// spec ENGT-2.15: a row that cannot be read is an error
#[test]
fn unreadable_rows_are_errors() {
    for (row, needle) in [
        ("{\"a\", PF_a, NULL, -1}", "negative"),
        ("{\"a\", PF_a, NULL, 0x10}", "number"),
        ("{\"a\", PF_a, NULL, x}", "number"),
        ("{\"a\", PF_a, NULL, 1, \"float\"}", "type string"),
        (
            "{\"a\", PF_a, NULL, 1, \"float()\", lower_case}",
            "extension",
        ),
        ("{\"a\", PF_a, NULL, 1, ftos}", "type string"),
        ("{\"not a name\", PF_a, NULL, 1}", "no name"),
        ("{PF_a, PF_a, NULL, 1}", "no name"),
        ("{\"a\", &PF_a, NULL, 1}", "function"),
        ("\"a\"", "not a braced row"),
    ] {
        let text = format!("defs_t defs[] = {{ {row} }};\n");
        let err = error(&text, &rows());
        assert!(err.contains(needle), "{row}: {err}");
    }
}

// spec ENGT-2.16: a patch whose number is 0 keys `#0:NAME` with the target builtin's
// signature and extensions, and is nothing when the target is not a builtin
#[test]
fn name_patches() {
    let targets = Facts {
        builtins: vec![Item {
            key: Key::Number(23),
            name: "PF_bprint".to_owned(),
            signature: "void(string s)".to_owned(),
            extensions: vec!["EXT_A".to_owned()],
        }],
        extensions: Vec::new(),
    };
    let text = "\
static const exbuiltin_t ex[] = {
\t{ \"bprint\", -91, -23 },
\t{ \"ex_bprint\", 0, -23 },
\t{ \"ex_draw\", 0, -81 },
\t{ NULL, 0, 0 },
\t{ \"ex_after\", 0, -23 },
};
";
    let facts = extract(text, &patches(), Some(&targets)).unwrap();
    assert_eq!(
        facts.builtins,
        [Item {
            key: Key::Name("ex_bprint".to_owned()),
            name: "ex_bprint".to_owned(),
            signature: "void(string s)".to_owned(),
            extensions: vec!["EXT_A".to_owned()],
        }]
    );
    for bad in ["{ \"a\", 0, 23 }", "{ \"a\", 0, 0 }", "{ \"a\", x, -23 }"] {
        let text = format!("x ex[] = {{ {bad} }};\n");
        assert!(extract(&text, &patches(), Some(&targets)).is_err(), "{bad}");
    }
    assert!(extract(text, &patches(), None).is_err());
}

// spec ENGT-2.16: patches need name, number and target columns and a target_of naming an
// earlier source with a reading of their kind
#[test]
fn name_patches_need_an_earlier_target() {
    let base = pin("pr_cmds.c", vec![positional("pr_builtin")]);
    let base = Pin {
        id: "base".to_owned(),
        ..base
    };
    let table = patches();
    let source = pin("pr_edict.c", vec![table.clone()]);
    assert!(check_table(&source, &table, std::slice::from_ref(&base)).is_ok());
    assert!(check_table(&source, &table, &[]).is_err());
    let client = Table {
        kind: "csqc".to_owned(),
        ..patches()
    };
    assert!(check_table(&source, &client, std::slice::from_ref(&base)).is_err());
    for broken in [
        Table {
            target_of: None,
            ..patches()
        },
        Table {
            number: None,
            ..patches()
        },
        Table {
            target: None,
            ..patches()
        },
    ] {
        let err = check_table(&source, &broken, std::slice::from_ref(&base)).unwrap_err();
        assert!(err.contains("name patches"), "{err}");
    }
}

// spec ENGT-2.17: the extension names are the first strings of the extension array's rows,
// once each in file order, and a row that is not one is an error
#[test]
fn extension_arrays() {
    let text = "\
defs_t defs[] = { {\"a\", PF_a, NULL, 1} };
static struct { const char *name; } qcextensions[] = {
\t{\"DP_QC_SINCOSSQRTPOW\"},
\t//\t{\"DP_COMMENTED_OUT\"},
\t{\"FRIK_FILE\", PR_Can_Files},
\t{\"DP_QC_SINCOSSQRTPOW\"},
};
";
    let table = Table {
        extension_array: Some("qcextensions".to_owned()),
        ..rows()
    };
    assert_eq!(
        read(text, &table).extensions,
        ["DP_QC_SINCOSSQRTPOW", "FRIK_FILE"]
    );
    assert!(read(text, &rows()).extensions.is_empty());
    let bad = text.replace("FRIK_FILE", "frik_file");
    assert!(error(&bad, &table).contains("extension name"));
}
