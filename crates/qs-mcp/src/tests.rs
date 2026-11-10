// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use qc_project::FsLoader;
use qs_commands::{Registry, Session};
use qs_workbench::{Config, Workbench, register};
use serde_json::{Value, json};

use super::{AGENT_TOOLS, Server, ServerSettings};

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

/// A server of the agent's tools over the workbench's commands, the project at `root` open,
/// reading under it and writing under it unless `read_only`.
fn server(root: &Path, read_only: bool) -> Server {
    let open = root.to_path_buf();
    let settings = ServerSettings {
        project: root.to_path_buf(),
        read: vec![root.to_path_buf()],
        write: if read_only {
            Vec::new()
        } else {
            vec![root.to_path_buf()]
        },
        name: "quakec-ide".to_owned(),
        version: "0".to_owned(),
    };
    Server::start(
        move || {
            let mut registry = Registry::new();
            register(&mut registry).map_err(|e| e.to_string())?;
            let mut session = Session::new(Workbench::new(FsLoader, Config::default()), registry);
            session
                .execute("project.open", json!({ "path": open }))
                .map_err(|e| e.to_string())?;
            Ok(session)
        },
        settings,
        &AGENT_TOOLS,
    )
    .unwrap()
}

/// The answers `server` gives to `messages`, sent one per line, by their ids.
fn exchange(server: Server, messages: &[Value]) -> BTreeMap<String, Value> {
    let mut input = String::new();
    for message in messages {
        input.push_str(&message.to_string());
        input.push('\n');
    }
    let output = server.serve_bytes(input.into_bytes()).unwrap();
    String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter_map(|answer| Some((answer.get("id")?.to_string(), answer)))
        .collect()
}

/// A legacy client's opening: `initialize` with `version`, then `notifications/initialized`.
fn opening(version: &str) -> [Value; 2] {
    [
        json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
            "protocolVersion": version, "capabilities": {},
            "clientInfo": { "name": "claude-code", "version": "2" } } }),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    ]
}

/// A tools/call request with id `id`.
fn call(id: u32, name: &str, arguments: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": { "name": name, "arguments": arguments } })
}

/// A modern request's `_meta`.
fn modern() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
        "io.modelcontextprotocol/clientInfo": { "name": "tests", "version": "0" },
    })
}

const WORLD: &str = "void() worldspawn = {};\n";

// No spec statement: MCP 2025-11-25 Lifecycle, a legacy client's initialize gets the version it
// asked for, the tools capability and serverInfo; tools/list lists the tools the registry has
// with their schemas; tools/call answers a command's JSON as text and structured content; a
// path relative to the project is read from it
#[test]
fn a_legacy_client_initializes_and_calls() {
    let root = project("legacy", WORLD);
    let [init, initialized] = opening("2025-06-18");
    let answers = exchange(
        server(&root, false),
        &[
            init,
            initialized,
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
            call(2, "compile", json!({})),
            call(3, "file.read", json!({ "path": "./world.qc" })),
        ],
    );
    let init = &answers["0"]["result"];
    assert_eq!(init["protocolVersion"], "2025-06-18");
    assert!(init["capabilities"]["tools"].is_object(), "{init}");
    assert_eq!(init["serverInfo"]["name"], "quakec-ide");
    let tools = answers["1"]["result"]["tools"].as_array().unwrap();
    let compile = tools.iter().find(|t| t["name"] == "compile").unwrap();
    assert_eq!(compile["inputSchema"]["type"], "object");
    assert!(
        compile["description"]
            .as_str()
            .unwrap()
            .contains("Compiles")
    );
    assert!(
        tools.iter().all(|t| t["name"] != "preview.play"),
        "a tool the registry lacks is not offered"
    );
    let compiled = &answers["2"]["result"];
    assert_eq!(compiled["isError"], false);
    assert_eq!(compiled["structuredContent"]["clean"], true);
    assert!(
        compiled["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("\"clean\": true")
    );
    let read = &answers["3"]["result"]["structuredContent"];
    assert_eq!(read["text"], WORLD);
    assert_eq!(
        read["path"],
        root.join("world.qc").to_string_lossy().as_ref()
    );
}

// No spec statement: MCP 2026-07-28 Versioning and Discovery, a modern request carries its
// version in _meta, with no initialize: server/discover names 2026-07-28 among the versions
// served and the tools capability, and tools/call answers with resultType complete
#[test]
fn a_modern_client_is_served_per_request() {
    let root = project("modern", WORLD);
    let answers = exchange(
        server(&root, false),
        &[
            json!({ "jsonrpc": "2.0", "id": "d", "method": "server/discover",
                    "params": { "_meta": modern() } }),
            json!({ "jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {
                "_meta": modern(), "name": "project.symbols",
                "arguments": { "query": "world" } } }),
        ],
    );
    let discover = &answers["\"d\""]["result"];
    assert!(
        discover["supportedVersions"]
            .as_array()
            .unwrap()
            .contains(&json!("2026-07-28")),
        "{discover}"
    );
    assert!(discover["capabilities"]["tools"].is_object(), "{discover}");
    let symbols = &answers["5"]["result"];
    assert_eq!(symbols["resultType"], "complete", "{symbols}");
    assert_eq!(symbols["isError"], false);
    assert!(
        symbols["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("worldspawn")
    );
}

// No spec statement: MCP 2026-07-28 Tools, Error Handling. An unknown tool is a protocol error
// (-32602); a command's failure and a path outside the roots are tool execution errors with
// isError, and nothing outside the roots is written
#[test]
fn errors_are_reported_as_mcp_says() {
    let root = project("errors", "void() worldspawn = { nope = 1; };\n");
    let [init, initialized] = opening("2025-11-25");
    let answers = exchange(
        server(&root, false),
        &[
            init,
            initialized,
            call(1, "project.open", json!({})),
            call(
                2,
                "symbol.hover",
                json!({ "path": "world.qc", "offset": "x" }),
            ),
            call(
                3,
                "edit.apply",
                json!({ "files": [{ "path": "../elsewhere/x.qc", "text": "" }] }),
            ),
            call(4, "compile", json!({})),
        ],
    );
    assert_eq!(answers["1"]["error"]["code"], -32602, "{}", answers["1"]);
    assert!(
        answers["1"]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Unknown tool")
    );
    assert_eq!(answers["2"]["result"]["isError"], true);
    let outside = &answers["3"]["result"];
    assert_eq!(outside["isError"], true);
    assert!(
        outside["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("outside the roots"),
        "{outside}"
    );
    assert!(!root.join("../elsewhere/x.qc").exists());
    assert_eq!(answers["4"]["result"]["structuredContent"]["clean"], false);
}

// No spec statement: P4-13, a tool that writes needs the project under a write root: with the
// project only readable, compile is refused and nothing is built, while a read tool runs
#[test]
fn a_read_only_project_refuses_writing_tools() {
    let root = project("read-only", WORLD);
    let [init, initialized] = opening("2025-11-25");
    let answers = exchange(
        server(&root, true),
        &[
            init,
            initialized,
            call(1, "compile", json!({})),
            call(2, "project.check", json!({})),
        ],
    );
    assert_eq!(answers["1"]["result"]["isError"], true);
    assert!(!root.join("build").exists(), "nothing was built");
    assert_eq!(answers["2"]["result"]["isError"], false);
}

// No spec statement: P4-13, a session that cannot be made is reported before serving
#[test]
fn a_session_that_fails_is_reported() {
    let settings = ServerSettings {
        project: PathBuf::from("/nowhere"),
        read: Vec::new(),
        write: Vec::new(),
        name: "quakec-ide".to_owned(),
        version: "0".to_owned(),
    };
    let failed = Server::start::<Workbench<FsLoader>>(
        || Err("no project here".to_owned()),
        settings,
        &AGENT_TOOLS,
    );
    assert!(
        failed
            .err()
            .is_some_and(|e| e.to_string() == "no project here")
    );
    let root = project("names", WORLD);
    assert!(server(&root, false).tool_names().contains(&"compile"));
}
