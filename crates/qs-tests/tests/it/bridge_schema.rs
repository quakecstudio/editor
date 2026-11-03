// SPDX-License-Identifier: GPL-3.0-or-later

//! The bridge protocol's schemas (`docs/spec/bridge-protocol.md`, BRIDGE-1 to BRIDGE-7):
//! every schema under `docs/spec/bridge-schema/` is a JSON Schema 2020-12 document, every
//! example under `examples/` validates against its schema, and the methods the spec's tables
//! name are the schemas there.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::panic,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap, slice or panic is a failed test"
)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use jsonschema::draft202012::meta;
use jsonschema::{Draft, Retrieve, Uri, Validator};
use serde_json::{Value, json};

const BASE: &str = "https://quakecstudio.com/spec/bridge-schema/";

fn schema_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/spec/bridge-schema")
}

fn spec_text() -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/spec/bridge-protocol.md"),
    )
    .unwrap()
}

/// Every schema file's name without `.json`, sorted.
fn schema_names() -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(schema_dir())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .map(|path| path.file_stem().unwrap().to_str().unwrap().to_owned())
        .collect();
    names.sort();
    names
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap_or_else(|e| panic!("{path:?}: {e}")))
        .unwrap_or_else(|e| panic!("{path:?}: {e}"))
}

/// Resolves a `$ref` to another schema of the set from the files, as the spec's `$id`s name
/// them (BRIDGE-1.4).
struct Files;

impl Retrieve for Files {
    fn retrieve(
        &self,
        uri: &Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let name = uri
            .as_str()
            .strip_prefix(BASE)
            .ok_or_else(|| format!("{uri} is not a bridge schema"))?;
        let path = schema_dir().join(name);
        Ok(read_json(&path))
    }
}

fn validator(name: &str) -> Validator {
    let schema = read_json(&schema_dir().join(format!("{name}.json")));
    jsonschema::options()
        .with_draft(Draft::Draft202012)
        .with_retriever(Files)
        .build(&schema)
        .unwrap_or_else(|e| panic!("{name}.json: {e}"))
}

/// The methods a markdown table names in its second column as `` `method` ``, in the
/// section whose heading contains `heading`.
fn methods_in_table(spec: &str, heading: &str) -> BTreeSet<String> {
    let start = spec.find(heading).unwrap();
    let section = &spec[start..];
    let end = section[1..].find("\n## ").map_or(section.len(), |e| e + 1);
    section[..end]
        .lines()
        .filter(|line| line.trim_start().starts_with("| `"))
        .map(|line| {
            let cell = line.split('|').nth(1).unwrap().trim();
            cell.trim_matches('`').to_owned()
        })
        .filter(|method| method != "Method" && !method.is_empty())
        .collect()
}

// spec BRIDGE-1.4, BRIDGE-5.1: every schema of the set is a JSON Schema 2020-12 document
// that compiles with its references resolved
#[test]
fn every_schema_compiles() {
    let names = schema_names();
    assert!(names.len() >= 40, "{names:?}");
    for name in &names {
        let schema = read_json(&schema_dir().join(format!("{name}.json")));
        assert_eq!(
            schema["$schema"],
            json!("https://json-schema.org/draft/2020-12/schema"),
            "{name}"
        );
        assert_eq!(schema["$id"], json!(format!("{BASE}{name}.json")), "{name}");
        assert!(
            meta::is_valid(&schema),
            "{name}.json is not a valid 2020-12 schema"
        );
        validator(name);
    }
}

// spec BRIDGE-3.1, BRIDGE-4.1: every example validates against the schema its name starts
// with, and every schema has at least one example
#[test]
fn every_example_validates() {
    let mut covered = BTreeSet::new();
    let examples = schema_dir().join("examples");
    for entry in fs::read_dir(&examples).unwrap() {
        let path = entry.unwrap().path();
        let stem = path.file_stem().unwrap().to_str().unwrap().to_owned();
        let (name, number) = stem.rsplit_once('.').unwrap();
        number.parse::<u32>().unwrap();
        let instance = read_json(&path);
        let validator = validator(name);
        let errors: Vec<String> = validator
            .iter_errors(&instance)
            .map(|e| e.to_string())
            .collect();
        assert!(errors.is_empty(), "{stem}: {errors:?}");
        covered.insert(name.to_owned());
    }
    // value.json only holds shared `$defs`; nothing is an instance of it.
    covered.insert("value".to_owned());
    let names: BTreeSet<String> = schema_names().into_iter().collect();
    let missing: Vec<_> = names.difference(&covered).collect();
    assert!(missing.is_empty(), "schemas with no example: {missing:?}");
}

// spec BRIDGE-3.1: the events the spec's table names are the event schemas, each with a
// `time`
#[test]
fn the_events_table_matches_the_schemas() {
    let spec = spec_text();
    let events = methods_in_table(&spec, "## BRIDGE-3 Events");
    assert_eq!(events.len(), 9, "{events:?}");
    let from_files: BTreeSet<String> = schema_names()
        .into_iter()
        .filter_map(|name| name.strip_prefix("event.").map(|e| e.replace('.', "/")))
        .collect();
    assert_eq!(events, from_files);
    for event in &events {
        let schema =
            read_json(&schema_dir().join(format!("event.{}.json", event.replace('/', "."))));
        assert!(
            schema["properties"]["time"].is_object(),
            "{event} carries no time"
        );
    }
}

// spec BRIDGE-4.1, BRIDGE-2.3: every bridge client operation but launch maps to a command
// with a params and a result schema, and the capability-flagged ones are exactly the flags
// `initialize.result.json` declares
#[test]
fn the_commands_table_matches_the_schemas_and_capabilities() {
    let spec = spec_text();
    let start = spec.find("## BRIDGE-4 Commands").unwrap();
    let section = &spec[start..spec[start + 1..].find("\n## ").unwrap() + start + 1];
    let mut commands = BTreeSet::new();
    let mut flagged = BTreeSet::new();
    let mut launch_has_no_method = false;
    for line in section.lines().filter(|l| l.trim_start().starts_with("| ")) {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        if cells.len() < 5 || cells[1] == "Operation" || cells[1].starts_with("---") {
            continue;
        }
        if cells[1] == "launch" {
            launch_has_no_method = cells[2] == "none";
            continue;
        }
        for method in cells[2].split(',') {
            let method = method.trim().trim_matches('`');
            commands.insert(method.to_owned());
            if cells[3] == "yes" {
                flagged.insert(method.to_owned());
            }
        }
    }
    assert!(launch_has_no_method);
    let names: BTreeSet<String> = schema_names().into_iter().collect();
    for command in &commands {
        let file = command.replace('/', ".");
        assert!(
            names.contains(&format!("{file}.params")),
            "{command} has no params schema"
        );
        assert!(
            names.contains(&format!("{file}.result")),
            "{command} has no result schema"
        );
    }
    let from_files: BTreeSet<String> = names
        .iter()
        .filter_map(|n| n.strip_suffix(".params"))
        .filter(|n| !matches!(*n, "initialize" | "shutdown"))
        .map(|n| n.replace('.', "/"))
        .collect();
    assert_eq!(
        commands, from_files,
        "a command with a schema but no row, or the reverse"
    );
    let init = read_json(&schema_dir().join("initialize.result.json"));
    let declared: BTreeSet<String> = init["properties"]["capabilities"]["properties"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(flagged, declared);
    let listed = methods_in_table(&spec, "## BRIDGE-7 Errors");
    assert!(listed.contains("-32005"), "the unsupported code is listed");
}

// spec BRIDGE-1.4, BRIDGE-2.4: the envelope is a request, a notification or a response with
// exactly one of result and error, and a version is major.minor
#[test]
fn the_envelope_and_the_version_shape() {
    let message = validator("message");
    assert!(message.is_valid(&json!({"jsonrpc": "2.0", "id": 1, "method": "pause"})));
    assert!(message.is_valid(&json!({"jsonrpc": "2.0", "method": "console/line", "params": {}})));
    assert!(message.is_valid(&json!({"jsonrpc": "2.0", "id": 1, "result": null})));
    assert!(message.is_valid(
        &json!({"jsonrpc": "2.0", "id": "a", "error": {"code": -32001, "message": "no"}})
    ));
    assert!(!message.is_valid(&json!({"jsonrpc": "1.0", "id": 1, "method": "pause"})));
    assert!(!message.is_valid(
        &json!({"jsonrpc": "2.0", "id": 1, "result": 1, "error": {"code": 1, "message": "x"}})
    ));
    assert!(!message.is_valid(&json!({"jsonrpc": "2.0", "id": 1})));
    assert!(
        !message
            .is_valid(&json!({"jsonrpc": "2.0", "id": 1, "error": {"code": "x", "message": "x"}}))
    );
    assert!(
        message.is_valid(
            &json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "x"}})
        ),
        "a response to a request whose id could not be read"
    );
    assert!(!message.is_valid(&json!({"jsonrpc": "2.0", "id": null, "method": "pause"})));
    assert!(!message.is_valid(&json!({"jsonrpc": "2.0", "id": 1, "method": "pause", "result": 1})));
    assert!(!message.is_valid(&json!({"jsonrpc": "2.0", "method": "pause", "result": 1})));
    assert!(!message.is_valid(
        &json!({"jsonrpc": "2.0", "id": 1, "method": "pause", "error": {"code": 1, "message": "x"}})
    ));
    // spec BRIDGE-3.4: a location with a file has its line and statement too.
    let changed = validator("event.state.changed");
    let at = |location: Value| json!({"state": "paused", "time": 1.0, "location": location});
    assert!(changed.is_valid(&at(json!({"function": "f"}))));
    assert!(changed.is_valid(&at(
        json!({"function": "f", "file": "a.qc", "line": 1, "statement": 0})
    )));
    assert!(!changed.is_valid(&at(json!({"function": "f", "file": "a.qc", "line": 1}))));
    assert!(!changed.is_valid(&at(json!({"function": "f", "line": 1, "statement": 0}))));
    let init = validator("initialize.params");
    assert!(init.is_valid(&json!({"protocolVersion": "1.0", "client": {"name": "x"}})));
    assert!(!init.is_valid(&json!({"protocolVersion": "1", "client": {"name": "x"}})));
    assert!(!init.is_valid(&json!({"protocolVersion": "1.0"})));
}

/// Every object in a JSON document, depth first.
fn objects(value: &Value) -> Vec<&serde_json::Map<String, Value>> {
    let mut found = Vec::new();
    let mut stack = vec![value];
    while let Some(value) = stack.pop() {
        match value {
            Value::Object(map) => {
                found.push(map);
                stack.extend(map.values());
            }
            Value::Array(items) => stack.extend(items),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }
    found
}

// spec BRIDGE-6.1, BRIDGE-5.1: no schema, at any depth, forbids an unknown member, so a
// message from a newer minor version validates, and a value's shape follows its type
#[test]
fn unknown_members_pass_and_wrong_value_types_fail() {
    for name in schema_names() {
        let schema = read_json(&schema_dir().join(format!("{name}.json")));
        for object in objects(&schema) {
            for keyword in ["additionalProperties", "unevaluatedProperties"] {
                assert_ne!(
                    object.get(keyword),
                    Some(&json!(false)),
                    "{name} forbids unknown members with {keyword}"
                );
            }
        }
    }
    let set = validator("field.set.params");
    let mut params = json!({"edict": 1, "field": "health", "value": {"type": "float", "value": 1}});
    assert!(set.is_valid(&params));
    params["future"] = json!({"anything": true});
    assert!(set.is_valid(&params), "an unknown member is ignored");
    let value = validator("evaluate.result");
    assert!(value.is_valid(&json!({"value": {"type": "vector", "value": [1, 2, 3]}})));
    assert!(value.is_valid(&json!({"value": {"type": "void"}})));
    assert!(!value.is_valid(&json!({"value": {"type": "vector", "value": [1, 2]}})));
    assert!(!value.is_valid(&json!({"value": {"type": "entity", "value": -1}})));
    assert!(!value.is_valid(&json!({"value": {"type": "float", "value": "1"}})));
    assert!(!value.is_valid(&json!({"value": {"type": "colour", "value": 1}})));
}

// spec BRIDGE-7.1: every error code the spec lists is in JSON-RPC's range or the reserved
// implementation range, each once
#[test]
fn the_error_codes_are_in_range_and_distinct() {
    let codes = methods_in_table(&spec_text(), "## BRIDGE-7 Errors");
    assert_eq!(codes.len(), 12, "{codes:?}");
    for code in &codes {
        let code: i32 = code.parse().unwrap();
        assert!(
            matches!(code, -32700 | -32603..=-32600 | -32099..=-32000),
            "{code}"
        );
    }
}
