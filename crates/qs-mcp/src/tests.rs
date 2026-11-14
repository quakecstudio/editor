// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use qc_project::FsLoader;
use qs_commands::remote::Host;
use qs_commands::{Registry, Session};
use qs_workbench::{
    Config, HasPreview, HasWorkbench, Preview, PreviewConfig, Workbench, register, register_preview,
};
use serde_json::{Value, json};

use super::{AGENT_TOOLS, Access, Connected, Server, ServerSettings};

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
    let failed = Server::start::<Session<Workbench<FsLoader>>>(
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

// No spec statement: P4-13 and AGT-7, with --connect the tools run in the running app's
// session: a change the agent stages there is in the app's session, and the app's failures are
// tool errors
#[test]
fn a_connected_server_runs_in_the_apps_session() {
    let root = project("connected", WORLD);
    let socket = if cfg!(windows) {
        PathBuf::from(format!(r"\\.\pipe\qs-mcp-connected-{}", std::process::id()))
    } else {
        root.join("app.sock")
    };
    let host = Host::listen(&socket, || {}).unwrap();
    let open = root.clone();
    // The app's thread: its session, answering the server's list and its three calls.
    let app = std::thread::spawn(move || {
        let mut registry = Registry::new();
        register(&mut registry).unwrap();
        let mut session = Session::new(Workbench::new(FsLoader, Config::default()), registry);
        session
            .execute("project.open", json!({ "path": open }))
            .unwrap();
        for _ in 0..4 {
            assert!(host.answer_next(&mut session));
        }
        session.execute("edit.staged", json!({})).unwrap()
    });
    let settings = ServerSettings {
        project: root.clone(),
        read: vec![root.clone()],
        write: vec![root.clone()],
        name: "quakec-ide".to_owned(),
        version: "0".to_owned(),
    };
    let connected = Server::start(
        move || Connected::connect(&socket).map_err(|e| e.to_string()),
        settings,
        &AGENT_TOOLS,
    )
    .unwrap();
    assert!(connected.tool_names().contains(&"compile"));
    let [init, initialized] = opening("2025-11-25");
    let answers = exchange(
        connected,
        &[
            init,
            initialized,
            call(1, "compile", json!({})),
            call(
                2,
                "edit.apply",
                json!({ "files": [{ "path": "world.qc", "text": "void() worldspawn = { };\n" }] }),
            ),
            call(3, "vm.run", json!({ "function": "nowhere" })),
        ],
    );
    assert_eq!(answers["1"]["result"]["structuredContent"]["clean"], true);
    assert_eq!(answers["2"]["result"]["isError"], false, "{}", answers["2"]);
    assert_eq!(answers["3"]["result"]["isError"], true, "{}", answers["3"]);
    let staged = app.join().unwrap();
    assert!(
        staged.to_string().contains("world.qc"),
        "the edit is staged in the app's session: {staged}"
    );
}

/// A workbench and a preview with no game, for the map's commands that compile.
struct MapContext {
    workbench: Workbench<FsLoader>,
    preview: Preview,
}

impl HasWorkbench for MapContext {
    type Loader = FsLoader;

    fn workbench(&mut self) -> &mut Workbench<FsLoader> {
        &mut self.workbench
    }

    fn preview(&mut self) -> Option<&mut Preview> {
        Some(&mut self.preview)
    }
}

impl HasPreview for MapContext {
    fn workbench_and_preview(&mut self) -> (&mut Workbench<FsLoader>, &mut Preview) {
        (&mut self.workbench, &mut self.preview)
    }
}

/// A project at `root` with `maps/room.map`, a world with nothing in it, and a server of the
/// agent's tools over a session that can compile maps, writing under the project unless
/// `read_only`; the map's path.
fn map_server(name: &str, read_only: bool) -> (Server, PathBuf) {
    let root = project(name, WORLD);
    std::fs::create_dir_all(root.join("maps")).unwrap();
    let map = root.join("maps/room.map");
    std::fs::write(&map, "{\n\"classname\" \"worldspawn\"\n}\n").unwrap();
    let settings = ServerSettings {
        project: root.clone(),
        read: vec![root.clone()],
        write: if read_only {
            Vec::new()
        } else {
            vec![root.clone()]
        },
        name: "quakec-ide".to_owned(),
        version: "0".to_owned(),
    };
    let server = Server::start(
        move || {
            let mut registry = Registry::new();
            register(&mut registry).map_err(|e| e.to_string())?;
            register_preview(&mut registry).map_err(|e| e.to_string())?;
            let context = MapContext {
                workbench: Workbench::new(FsLoader, Config::default()),
                preview: Preview::new(PreviewConfig {
                    host: None,
                    app: PathBuf::from("/nowhere/app"),
                    game: None,
                    extra: Vec::new(),
                }),
            };
            let mut session = Session::new(context, registry);
            session
                .execute("project.open", json!({ "path": root }))
                .map_err(|e| e.to_string())?;
            Ok(session)
        },
        settings,
        &AGENT_TOOLS,
    )
    .unwrap();
    (server, map)
}

/// The map tools the server offers.
fn map_tools() -> impl Iterator<Item = &'static super::Tool> {
    AGENT_TOOLS
        .iter()
        .filter(|tool| tool.id.starts_with("map."))
}

// No spec statement: P6-36, MAP-16, MAP-29: tools/list offers the map's tools, each with its
// command's schema: open, inspect, place, keys, the brush, shape and CSG commands, compile and
// leaks
#[test]
fn the_map_tools_are_listed_with_their_schemas() {
    let (server, _) = map_server("map-list", false);
    let [init, initialized] = opening("2025-06-18");
    let answers = exchange(
        server,
        &[
            init,
            initialized,
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
        ],
    );
    let tools = answers["1"]["result"]["tools"].as_array().unwrap();
    for wanted in [
        "map.open",
        "map.inspect",
        "map.place",
        "map.set_key",
        "map.brush_create",
        "map.brush_shape",
        "map.brush_subtract",
        "map.brush_merge",
        "map.brush_intersect",
        "map.brush_hollow",
        "map.compile",
        "map.leaks",
    ] {
        assert!(map_tools().any(|tool| tool.id == wanted), "{wanted}");
    }
    for tool in map_tools() {
        let listed = tools.iter().find(|t| t["name"] == tool.id);
        assert!(listed.is_some(), "{} is listed", tool.id);
        let listed = listed.unwrap();
        assert_eq!(listed["inputSchema"]["type"], "object", "{}", tool.id);
    }
    let create = tools
        .iter()
        .find(|t| t["name"] == "map.brush_create")
        .unwrap();
    assert!(
        create["inputSchema"]["properties"]["min"].is_object(),
        "{create}"
    );
}

/// The six walls of a room around -128 to 128, 16 thick.
const WALLS: [([f64; 3], [f64; 3]); 6] = [
    ([-144.0, -144.0, -144.0], [144.0, 144.0, -128.0]),
    ([-144.0, -144.0, 128.0], [144.0, 144.0, 144.0]),
    ([-144.0, -144.0, -128.0], [-128.0, 144.0, 128.0]),
    ([128.0, -144.0, -128.0], [144.0, 144.0, 128.0]),
    ([-128.0, -144.0, -128.0], [128.0, -128.0, 128.0]),
    ([-128.0, 128.0, -128.0], [128.0, 144.0, 128.0]),
];

// No spec statement: P6-36, MAP-16, MAP-29: a sealed room built through MCP tool calls alone,
// a player start and six brushes, finds no leak and compiles sealed, its edits waiting for
// review
#[test]
fn a_room_built_over_mcp_compiles_sealed() {
    let (server, map) = map_server("map-room", false);
    let [init, initialized] = opening("2025-11-25");
    let mut messages = vec![
        init,
        initialized,
        call(1, "map.open", json!({ "path": map })),
        call(
            2,
            "map.place",
            json!({ "classname": "info_player_start", "origin": [0.0, 0.0, 24.0] }),
        ),
    ];
    for (n, (min, max)) in (3..).zip(WALLS) {
        messages.push(call(
            n,
            "map.brush_create",
            json!({ "min": min, "max": max, "texture": "wall" }),
        ));
    }
    messages.push(call(20, "map.leaks", json!({})));
    messages.push(call(21, "map.compile", json!({})));
    messages.push(call(22, "map.staged", json!({})));
    let answers = exchange(server, &messages);
    for id in 1..=8 {
        let answer = &answers[&id.to_string()]["result"];
        assert_eq!(answer["isError"], false, "{id}: {answer}");
    }
    let leaks = &answers["20"]["result"]["structuredContent"];
    assert_eq!(leaks["seal"], "sealed", "{leaks}");
    assert_eq!(leaks["leak"], json!([]));
    let compiled = &answers["21"]["result"];
    assert_eq!(compiled["isError"], false, "{compiled}");
    assert_eq!(compiled["structuredContent"]["seal"], "sealed");
    let staged = answers["22"]["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(staged.contains("info_player_start"), "{staged}");
}

// No spec statement: P6-36, P4-13: a read-only server refuses every map tool that writes, and
// still opens and inspects the map
#[test]
fn a_read_only_server_refuses_the_map_tools_that_write() {
    let (server, map) = map_server("map-read-only", true);
    let [init, initialized] = opening("2025-11-25");
    let writes: Vec<&str> = map_tools()
        .filter(|tool| tool.access == Access::Write)
        .map(|tool| tool.id)
        .collect();
    assert!(writes.contains(&"map.place") && writes.contains(&"map.compile"));
    let mut messages = vec![
        init,
        initialized,
        call(1, "map.open", json!({ "path": map })),
        call(2, "map.inspect", json!({})),
    ];
    for (n, id) in (3..).zip(&writes) {
        messages.push(call(n, id, json!({})));
    }
    let answers = exchange(server, &messages);
    assert_eq!(answers["1"]["result"]["isError"], false, "{}", answers["1"]);
    assert_eq!(answers["2"]["result"]["isError"], false, "{}", answers["2"]);
    for (n, id) in (3..).zip(&writes) {
        let answer = &answers[&n.to_string()]["result"];
        assert_eq!(answer["isError"], true, "{id}: {answer}");
        let text = answer["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("outside the roots"), "{id}: {text}");
    }
}
