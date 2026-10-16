// SPDX-License-Identifier: GPL-3.0-or-later

//! Structure of the VM test vectors in `docs/spec/vectors/vm/` (format in its README).
//!
//! `qc-vm` runs the vectors; this module checks, before that crate exists, that the files
//! stay well formed, that every vector cites statements that exist, and that every vanilla
//! opcode has at least one vector (the acceptance criterion of P1-02).

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code: a failed expect or panic is a failed test with a message"
)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// The vanilla opcode names in number order, from `docs/spec/progs.md` PROGS-8.1.
const OPCODES: [&str; 66] = [
    "DONE",
    "MUL_F",
    "MUL_V",
    "MUL_FV",
    "MUL_VF",
    "DIV_F",
    "ADD_F",
    "ADD_V",
    "SUB_F",
    "SUB_V",
    "EQ_F",
    "EQ_V",
    "EQ_S",
    "EQ_E",
    "EQ_FNC",
    "NE_F",
    "NE_V",
    "NE_S",
    "NE_E",
    "NE_FNC",
    "LE",
    "GE",
    "LT",
    "GT",
    "LOAD_F",
    "LOAD_V",
    "LOAD_S",
    "LOAD_ENT",
    "LOAD_FLD",
    "LOAD_FNC",
    "ADDRESS",
    "STORE_F",
    "STORE_V",
    "STORE_S",
    "STORE_ENT",
    "STORE_FLD",
    "STORE_FNC",
    "STOREP_F",
    "STOREP_V",
    "STOREP_S",
    "STOREP_ENT",
    "STOREP_FLD",
    "STOREP_FNC",
    "RETURN",
    "NOT_F",
    "NOT_V",
    "NOT_S",
    "NOT_ENT",
    "NOT_FNC",
    "IF",
    "IFNOT",
    "CALL0",
    "CALL1",
    "CALL2",
    "CALL3",
    "CALL4",
    "CALL5",
    "CALL6",
    "CALL7",
    "CALL8",
    "STATE",
    "GOTO",
    "AND",
    "OR",
    "BITAND",
    "BITOR",
];

const KEYS: [&str; 11] = [
    "spec",
    "globals",
    "fields",
    "strings",
    "numglobals",
    "server",
    "entry",
    "steps",
    "after",
    "oracle",
    "functions",
];

/// One vector, with only what these checks need.
#[derive(Debug, Default)]
struct Vector {
    name: String,
    spec: Vec<String>,
    ops: Vec<String>,
    expect: Vec<String>,
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Parses one vector file. Errors name the file and line.
fn parse(file: &str, text: &str) -> Result<Vec<Vector>, String> {
    let mut vectors: Vec<Vector> = Vec::new();
    let mut block = "";
    for (raw, number) in text.lines().zip(1_u32..) {
        let at = || format!("{file}:{number}");
        let line = if raw.starts_with("strings:") {
            raw
        } else {
            raw.split('#').next().unwrap_or_default()
        };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(name) = line.strip_prefix("== ") {
            vectors.push(Vector {
                name: name.trim().to_owned(),
                ..Vector::default()
            });
            block = "";
            continue;
        }
        let Some(vector) = vectors.last_mut() else {
            return Err(format!("{}: text before the first `== name` line", at()));
        };
        if let Some(body) = line.strip_prefix("  ") {
            match block {
                "code" => {
                    let op = body.split_whitespace().next().unwrap_or_default();
                    vector.ops.push(op.to_owned());
                }
                "expect" => vector.expect.push(body.trim().to_owned()),
                "functions" => {}
                _ => return Err(format!("{}: indented line outside a block", at())),
            }
            continue;
        }
        let Some((key, rest)) = line.split_once(':') else {
            return Err(format!("{}: expected `key: value`", at()));
        };
        match key {
            "code" | "expect" => block = key,
            "spec" => {
                vector
                    .spec
                    .extend(rest.split(',').map(|id| id.trim().to_owned()));
                block = "";
            }
            _ if KEYS.contains(&key) => block = if key == "functions" { "functions" } else { "" },
            _ => return Err(format!("{}: unknown key `{key}`", at())),
        }
    }
    Ok(vectors)
}

fn active_statements() -> BTreeSet<String> {
    let path = repo_root().join("docs/spec/requirements.tsv");
    let text = fs::read_to_string(&path).expect("read docs/spec/requirements.tsv");
    text.lines()
        .skip(1)
        .filter_map(|row| {
            let mut columns = row.split('\t');
            let id = columns.next()?;
            let status = columns.nth(2)?;
            (status == "active").then(|| id.to_owned())
        })
        .collect()
}

fn vm_vectors() -> Vec<Vector> {
    let dir = repo_root().join("docs/spec/vectors/vm");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("read docs/spec/vectors/vm")
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "txt"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no vector files in {}", dir.display());
    files
        .iter()
        .flat_map(|path| {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            let text = fs::read_to_string(path).expect("read vector file");
            parse(name, &text).unwrap_or_else(|error| panic!("{error}"))
        })
        .collect()
}

/// P1-02 acceptance: every vanilla opcode has at least one VM vector.
#[test]
fn every_vanilla_opcode_has_a_vector() {
    let used: BTreeSet<String> = vm_vectors().into_iter().flat_map(|v| v.ops).collect();
    let missing: Vec<&str> = OPCODES
        .iter()
        .copied()
        .filter(|op| !used.contains(*op))
        .collect();
    assert!(missing.is_empty(), "opcodes without a vector: {missing:?}");
}

#[test]
fn every_vector_is_well_formed() {
    let statements = active_statements();
    let vectors = vm_vectors();
    let mut names = BTreeSet::new();
    for vector in &vectors {
        let name = &vector.name;
        assert!(
            names.insert(name.clone()),
            "vector `{name}` is defined twice"
        );
        assert!(
            !vector.spec.is_empty(),
            "vector `{name}` cites no statement"
        );
        for id in &vector.spec {
            assert!(
                statements.contains(id),
                "vector `{name}` cites `{id}`, not an active statement"
            );
        }
        assert!(!vector.ops.is_empty(), "vector `{name}` has no code");
        for op in &vector.ops {
            assert!(
                OPCODES.contains(&op.as_str()) || op.parse::<u16>().is_ok(),
                "vector `{name}` uses unknown opcode `{op}`"
            );
        }
        let end = vector.expect.last().map(String::as_str).unwrap_or_default();
        assert!(
            end == "done" || end == "error" || end.starts_with("error \""),
            "vector `{name}` must end its expect block with `done` or `error`, not `{end}`"
        );
    }
}

#[test]
fn parse_rejects_malformed_files() {
    assert!(
        parse("t", "spec: VM-4.1\n").is_err(),
        "text before the first vector"
    );
    assert!(parse("t", "== a\nbogus: 1\n").is_err(), "unknown key");
    assert!(
        parse("t", "== a\n  ADD_F 1 2 3\n").is_err(),
        "indented line outside a block"
    );
    assert!(
        parse("t", "== a\nno colon here\n").is_err(),
        "line without a key"
    );
    let parsed = parse(
        "t",
        "== a\nspec: VM-4.1, VM-4.4\ncode:\n  ADD_F 1 2 3 # c\nexpect:\n  done\n",
    )
    .expect("a well-formed vector parses");
    let [vector] = parsed.as_slice() else {
        panic!("expected one vector, got {parsed:?}");
    };
    assert_eq!(vector.spec, ["VM-4.1", "VM-4.4"]);
    assert_eq!(vector.ops, ["ADD_F"]);
    assert_eq!(vector.expect, ["done"]);
}
