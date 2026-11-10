// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::path::{Path, PathBuf};

use qc_project::FsLoader;
use qs_commands::{Registry, Session};
use qs_workbench::{Config, Workbench, register};
use serde_json::{Value, json};

use super::{AGENT_TOOLS, MODERN_VERSION, Server, ServerConfig, serve};

/// A fresh project on disk for test `name`: a `progs.src` and a `world.qc` of `world`.
fn project(name: &str, world: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("qs-mcp-{name}-{}", std::process::id()));
    if root.exists() {
        std::fs::remove_dir_all(&root).unwrap();
    }
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("progs.src"), "progs.dat\nworld.qc\n").unwrap();
    std::fs::write(root.join("world.qc"), world).unwrap();
    root
}

/// A server of the agent's tools over the workbench's commands, its project open, reading
/// and writing under the project only.
fn server(root: &Path) -> Server<Workbench<FsLoader>> {
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut session = Session::new(Workbench::new(FsLoader, Config::default()), registry);
    session
        .execute("project.open", json!({ "path": root }))
        .unwrap();
    let config = ServerConfig {
        project: root.to_path_buf(),
        read: vec![root.to_path_buf()],
        write: vec![root.to_path_buf()],
        name: "quakec-ide".to_owned(),
        version: "0".to_owned(),
    };
    Server::new(session, config, &AGENT_TOOLS)
}

/// The answer to `message`, parsed.
fn ask(server: &mut Server<Workbench<FsLoader>>, message: &Value) -> Value {
    serde_json::from_str(&server.handle_line(&message.to_string()).unwrap()).unwrap()
}

/// A modern request's `_meta`.
fn modern() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": MODERN_VERSION,
        "io.modelcontextprotocol/clientCapabilities": {},
        "io.modelcontextprotocol/clientInfo": { "name": "tests", "version": "0" },
    })
}

const WORLD: &str = "void() worldspawn = {};\n";

// No spec statement: MCP 2025-11-25 Lifecycle, a legacy client's initialize is answered with
// the version it asked for when served, else the latest legacy one, the tools capability and
// serverInfo; the initialized notification is not answered; tools/list and tools/call follow
// with no _meta
#[test]
fn a_legacy_client_initializes_and_calls() {
    let root = project("legacy", WORLD);
    let mut server = server(&root);
    let init = ask(
        &mut server,
        &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": { "name": "claude-code", "version": "2" } } }),
    );
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["capabilities"], json!({ "tools": {} }));
    assert_eq!(init["result"]["serverInfo"]["name"], "quakec-ide");
    let newer = ask(
        &mut server,
        &json!({ "jsonrpc": "2.0", "id": 2, "method": "initialize",
                 "params": { "protocolVersion": "2099-01-01" } }),
    );
    assert_eq!(newer["result"]["protocolVersion"], "2025-11-25");
    assert_eq!(
        server.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#),
        None
    );
    let list = ask(
        &mut server,
        &json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" }),
    );
    let tools = list["result"]["tools"].as_array().unwrap();
    let compile = tools.iter().find(|t| t["name"] == "compile").unwrap();
    assert_eq!(compile["inputSchema"]["type"], "object");
    assert!(
        compile["description"]
            .as_str()
            .unwrap()
            .contains("Compiles")
    );
    assert!(
        list["result"].get("resultType").is_none(),
        "legacy results carry none"
    );
    assert!(
        tools.iter().all(|t| t["name"] != "preview.play"),
        "a tool the registry lacks is not offered"
    );
    let called = ask(
        &mut server,
        &json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/call",
                 "params": { "name": "compile", "arguments": {} } }),
    );
    assert_eq!(called["result"]["isError"], false);
    assert_eq!(called["result"]["structuredContent"]["clean"], true);
    // A path relative to the project is read from it.
    let read = ask(
        &mut server,
        &json!({ "jsonrpc": "2.0", "id": 5, "method": "tools/call",
                 "params": { "name": "file.read", "arguments": { "path": "./world.qc" } } }),
    );
    assert_eq!(read["result"]["structuredContent"]["text"], WORLD);
    assert_eq!(
        read["result"]["structuredContent"]["path"],
        root.join("world.qc").to_string_lossy().as_ref()
    );
    let text = called["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("\"clean\": true"), "{text}");
}

// No spec statement: MCP 2026-07-28 Versioning and Discovery, a modern request names its
// version and capabilities in _meta and gets resultType complete; server/discover names the
// version served; another version is refused with -32022 listing it; a request without the
// capabilities is refused with -32602
#[test]
fn a_modern_client_is_served_per_request() {
    let root = project("modern", WORLD);
    let mut server = server(&root);
    let discover = ask(
        &mut server,
        &json!({ "jsonrpc": "2.0", "id": "d", "method": "server/discover",
                 "params": { "_meta": modern() } }),
    );
    assert_eq!(discover["id"], "d");
    assert_eq!(
        discover["result"]["supportedVersions"],
        json!([MODERN_VERSION])
    );
    assert_eq!(discover["result"]["resultType"], "complete");
    assert_eq!(
        discover["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "quakec-ide"
    );
    let symbols = ask(
        &mut server,
        &json!({ "jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {
            "_meta": modern(), "name": "project.symbols",
            "arguments": { "query": "world" } } }),
    );
    assert_eq!(symbols["result"]["resultType"], "complete");
    assert_eq!(symbols["result"]["isError"], false);
    assert!(
        symbols["result"].get("structuredContent").is_none(),
        "an array is not structured content"
    );
    let text = symbols["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("worldspawn"), "{text}");
    let old = ask(
        &mut server,
        &json!({ "jsonrpc": "2.0", "id": 6, "method": "tools/list", "params": { "_meta": {
            "io.modelcontextprotocol/protocolVersion": "1900-01-01",
            "io.modelcontextprotocol/clientCapabilities": {} } } }),
    );
    assert_eq!(old["error"]["code"], -32022);
    assert_eq!(old["error"]["data"]["supported"], json!([MODERN_VERSION]));
    assert_eq!(old["error"]["data"]["requested"], "1900-01-01");
    let bare = ask(
        &mut server,
        &json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/list", "params": { "_meta": {
            "io.modelcontextprotocol/protocolVersion": MODERN_VERSION } } }),
    );
    assert_eq!(bare["error"]["code"], -32602);
    let legacy_discover = ask(
        &mut server,
        &json!({ "jsonrpc": "2.0", "id": 8, "method": "server/discover" }),
    );
    assert!(
        legacy_discover.get("error").is_some(),
        "a legacy probe falls back on an error"
    );
}

// No spec statement: MCP 2026-07-28 Tools, Error Handling. An unknown tool is a protocol
// error; a command's failure, and a path outside the roots, are tool execution errors the
// model can read; JSON-RPC's own errors for what is not a request
#[test]
fn errors_are_reported_as_mcp_says() {
    let root = project("errors", "void() worldspawn = { nope = 1; };\n");
    let mut server = server(&root);
    let call = |name: &str, arguments: Value| {
        json!({ "jsonrpc": "2.0", "id": 9, "method": "tools/call",
                "params": { "name": name, "arguments": arguments } })
    };
    let unknown = ask(&mut server, &call("project.open", json!({})));
    assert_eq!(unknown["error"]["code"], -32602);
    assert!(
        unknown["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Unknown tool")
    );
    let failed = ask(
        &mut server,
        &call("symbol.hover", json!({ "path": "world.qc", "offset": "x" })),
    );
    assert_eq!(failed["result"]["isError"], true);
    let outside = ask(
        &mut server,
        &call(
            "edit.apply",
            json!({ "files": [{ "path": "../elsewhere/x.qc", "text": "" }] }),
        ),
    );
    assert_eq!(outside["result"]["isError"], true);
    assert!(
        outside["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("outside the roots"),
        "{outside}"
    );
    assert!(!root.join("../elsewhere/x.qc").exists());
    let compiled = ask(&mut server, &call("compile", json!({})));
    assert_eq!(compiled["result"]["structuredContent"]["clean"], false);
    let parse = ask_raw(&mut server, "{not json");
    assert_eq!(parse["error"]["code"], -32700);
    assert_eq!(parse["id"], Value::Null);
    let no_method = ask_raw(&mut server, r#"{"jsonrpc":"2.0","id":1}"#);
    assert_eq!(no_method["error"]["code"], -32600);
    let no_such = ask_raw(&mut server, r#"{"jsonrpc":"2.0","id":2,"method":"x/y"}"#);
    assert_eq!(no_such["error"]["code"], -32601);
}

fn ask_raw(server: &mut Server<Workbench<FsLoader>>, line: &str) -> Value {
    serde_json::from_str(&server.handle_line(line).unwrap()).unwrap()
}

// No spec statement: P4-13, a tool that writes needs the project under a write root: with the
// project only readable, compile is refused and a read tool still runs
#[test]
fn a_read_only_project_refuses_writing_tools() {
    let root = project("read-only", WORLD);
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut session = Session::new(Workbench::new(FsLoader, Config::default()), registry);
    session
        .execute("project.open", json!({ "path": root }))
        .unwrap();
    let config = ServerConfig {
        project: root.clone(),
        read: vec![root.clone()],
        write: Vec::new(),
        name: "quakec-ide".to_owned(),
        version: "0".to_owned(),
    };
    let mut server = Server::new(session, config, &AGENT_TOOLS);
    let call = |name: &str| {
        json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": { "name": name, "arguments": {} } })
    };
    assert_eq!(
        ask(&mut server, &call("compile"))["result"]["isError"],
        true
    );
    assert!(!root.join("build").exists(), "nothing was built");
    assert_eq!(
        ask(&mut server, &call("project.check"))["result"]["isError"],
        false
    );
}

// No spec statement: MCP 2026-07-28 stdio, one message per line in, one answer per line out,
// none for a notification, blank lines passed over
#[test]
fn stdio_is_one_message_per_line() {
    let root = project("stdio", WORLD);
    let mut server = server(&root);
    let input = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n\n\
                 {\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n\
                 {\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n";
    let mut out = Vec::new();
    serve(&mut server, input.as_bytes(), &mut out).unwrap();
    let text = String::from_utf8(out).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "{text}");
    let ping: Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(ping, json!({ "jsonrpc": "2.0", "id": 1, "result": {} }));
    assert!(server.tool_names().contains(&"compile"));
}
