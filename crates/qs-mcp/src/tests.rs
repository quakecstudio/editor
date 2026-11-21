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
use qs_workbench::reference::Topic;
use qs_workbench::{
    Config, HasPreview, HasWorkbench, Preview, PreviewConfig, Workbench, register, register_preview,
};
use serde_json::{Value, json};

use super::{Access, Connected, Server, ServerSettings};

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
fn call(id: u32, name: &str, arguments: &Value) -> Value {
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
            call(2, "project.compile", &json!({})),
            call(3, "file.read", &json!({ "path": "./world.qc" })),
        ],
    );
    let init = &answers["0"]["result"];
    assert_eq!(init["protocolVersion"], "2025-06-18");
    assert!(init["capabilities"]["tools"].is_object(), "{init}");
    assert_eq!(init["serverInfo"]["name"], "quakec-ide");
    let tools = answers["1"]["result"]["tools"].as_array().unwrap();
    let compile = tools
        .iter()
        .find(|t| t["name"] == "project.compile")
        .unwrap();
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
            call(1, "project.no_such_command", &json!({})),
            call(
                2,
                "symbol.hover",
                &json!({ "path": "world.qc", "offset": "x" }),
            ),
            call(
                3,
                "edit.stage_change",
                &json!({ "files": [{ "path": "../elsewhere/x.qc", "text": "" }] }),
            ),
            call(4, "project.compile", &json!({})),
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
            call(1, "project.compile", &json!({})),
            call(2, "project.check", &json!({})),
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
    );
    assert!(
        failed
            .err()
            .is_some_and(|e| e.to_string() == "no project here")
    );
    let root = project("names", WORLD);
    assert!(
        server(&root, false)
            .tool_names()
            .contains(&"project.compile")
    );
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
        write: vec![root],
        name: "quakec-ide".to_owned(),
        version: "0".to_owned(),
    };
    let connected = Server::start(
        move || Connected::connect(&socket).map_err(|e| e.to_string()),
        settings,
    )
    .unwrap();
    assert!(connected.tool_names().contains(&"project.compile"));
    let [init, initialized] = opening("2025-11-25");
    let answers = exchange(
        connected,
        &[
            init,
            initialized,
            call(1, "project.compile", &json!({})),
            call(
                2,
                "edit.stage_change",
                &json!({ "files": [{ "path": "world.qc", "text": "void() worldspawn = { };\n" }] }),
            ),
            call(3, "vm.run", &json!({ "function": "nowhere" })),
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

impl qs_map_edit::HasMap for MapContext {
    type Project = Workbench<FsLoader>;
    type Live = Preview;

    fn map_store(&mut self) -> &mut qs_map_edit::MapStore {
        self.workbench.maps_mut()
    }

    fn map_project(&mut self) -> &mut Workbench<FsLoader> {
        &mut self.workbench
    }

    fn live_map(&mut self) -> Option<&mut Preview> {
        Some(&mut self.preview)
    }
}

impl HasWorkbench for MapContext {
    type Loader = FsLoader;

    fn workbench(&mut self) -> &mut Workbench<FsLoader> {
        &mut self.workbench
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
    )
    .unwrap();
    (server, map)
}

/// The map tools `server` offers.
fn map_tools(server: &Server) -> Vec<super::Tool> {
    server
        .tools()
        .filter(|tool| tool.id.starts_with("map."))
        .cloned()
        .collect()
}

// No spec statement: P6-36, MAP-16, MAP-29: tools/list offers the map's tools, each with its
// command's schema: open, inspect, place, keys, the brush, shape and CSG commands, compile and
// leaks
#[test]
fn the_map_tools_are_listed_with_their_schemas() {
    let (server, _) = map_server("map-list", false);
    let offered = map_tools(&server);
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
        "map.new",
        "map.save",
        "map.play_from_here",
        "map.list_entities",
        "map.place_entity",
        "map.set_key",
        "map.brush_create",
        "map.brush_shape",
        "map.brush_subtract",
        "map.brush_merge",
        "map.brush_intersect",
        "map.brush_hollow",
        "map.compile",
        "map.find_leaks",
    ] {
        assert!(tools.iter().any(|t| t["name"] == wanted), "{wanted}");
    }
    // Every command is a tool (PRIN-3): the review's own are among them.
    for decision in [
        "map.accept_hunks",
        "map.reject_hunks",
        "map.stage",
        "map.unstage",
    ] {
        assert!(tools.iter().any(|t| t["name"] == decision), "{decision}");
    }
    for tool in &offered {
        let listed = tools.iter().find(|t| t["name"] == *tool.id);
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

// No spec statement: PRIN-3, P6-44: every command of the session is a tool, none chosen by
// hand, each told to the client as reading only or not (MCP 2026-07-28,
// `ToolAnnotations.readOnlyHint`: "If true, the tool does not modify its environment."), and
// nothing claimed of `openWorldHint`, which the terminal and the tasks could not keep
#[test]
fn every_command_is_a_tool_with_its_access() {
    let (server, _) = map_server("every-tool", false);
    let offered: Vec<super::Tool> = server.tools().cloned().collect();
    let [init, initialized] = opening("2025-11-25");
    let answers = exchange(
        server,
        &[
            init,
            initialized,
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
            call(2, "commands.list", &json!({})),
        ],
    );
    let tools = answers["1"]["result"]["tools"].as_array().unwrap();
    let listed = answers["2"]["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    let commands: Vec<Value> = serde_json::from_str(listed).unwrap();
    assert_eq!(tools.len(), commands.len(), "a tool for each command");
    assert!(tools.len() > 150, "{}", tools.len());
    for command in &commands {
        let id = command["id"].as_str().unwrap();
        let tool = tools.iter().find(|t| t["name"] == id);
        assert!(tool.is_some(), "{id} is a tool");
        let annotations = &tool.unwrap()["annotations"];
        let reads = command["access"] == "read";
        assert_eq!(annotations["readOnlyHint"], reads, "{id}");
        assert!(annotations.get("openWorldHint").is_none(), "{id}");
    }
    let access = |id: &str| offered.iter().find(|tool| &*tool.id == id).unwrap().access;
    for id in [
        "map.list_entities",
        "file.read",
        "git.status",
        "project.symbols",
        "commands.list",
    ] {
        assert_eq!(access(id), Access::Read, "{id}");
    }
    for id in [
        "map.place_entity",
        "edit.stage_change",
        "preview.play",
        "git.commit",
        "engine.console",
    ] {
        assert_eq!(access(id), Access::Write, "{id}");
    }
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
// a player start and six brushes, finds no leak and compiles sealed. L-43, the acceptance
// criterion: its edits are made at once, so nothing waits for review, and undo takes the last
// one back
#[test]
fn a_room_built_over_mcp_compiles_sealed() {
    let (server, map) = map_server("map-room", false);
    let [init, initialized] = opening("2025-11-25");
    let mut messages = vec![
        init,
        initialized,
        call(1, "map.open", &json!({ "path": map })),
        call(30, "map.list_entities", &json!({})),
        call(31, "map.staged_hunks", &json!({})),
        call(
            2,
            "map.place_entity",
            &json!({ "classname": "info_player_start", "origin": [0.0, 0.0, 24.0] }),
        ),
    ];
    for (n, (min, max)) in (3..).zip(WALLS) {
        messages.push(call(
            n,
            "map.brush_create",
            &json!({ "min": min, "max": max, "texture": "wall" }),
        ));
    }
    messages.push(call(20, "map.find_leaks", &json!({})));
    messages.push(call(21, "map.compile", &json!({})));
    messages.push(call(22, "map.staged_hunks", &json!({})));
    messages.push(call(23, "map.summary", &json!({})));
    messages.push(call(24, "edit.undo", &json!({})));
    messages.push(call(25, "map.summary", &json!({})));
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
    let unread = answers["31"]["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(unread).unwrap(),
        json!([]),
        "a read opens no review"
    );
    let staged = answers["22"]["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(staged).unwrap(),
        json!([]),
        "nothing waits for review"
    );
    let built = &answers["23"]["result"]["structuredContent"];
    assert_eq!(built["world_brushes"], 6, "{built}");
    assert!(built["classes"].to_string().contains("info_player_start"));
    assert_eq!(answers["24"]["result"]["isError"], false);
    let undone = &answers["25"]["result"]["structuredContent"];
    assert_eq!(undone["world_brushes"], 5, "undo takes the last brush back");
}

// No spec statement: P6-44, MAP-16: a map is made, edited and saved through MCP tool calls
// alone: map.new opens no review and its file is under the write roots, and map.save writes
// what the tools put in it
#[test]
fn a_map_is_made_and_saved_over_mcp() {
    let (server, map) = map_server("map-new", false);
    let made = map.with_file_name("made.map");
    let [init, initialized] = opening("2025-11-25");
    let answers = exchange(
        server,
        &[
            init,
            initialized,
            call(
                1,
                "map.new",
                &json!({ "path": made, "keys": [["message", "made"]] }),
            ),
            call(2, "map.staged_hunks", &json!({})),
            call(
                3,
                "map.place_entity",
                &json!({ "classname": "info_player_start", "origin": [0.0, 0.0, 24.0] }),
            ),
            call(4, "map.save", &json!({})),
            call(5, "map.new", &json!({ "path": made })),
        ],
    );
    for id in 1..=4 {
        let answer = &answers[&id.to_string()]["result"];
        assert_eq!(answer["isError"], false, "{id}: {answer}");
    }
    let unread = answers["2"]["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert_eq!(serde_json::from_str::<Value>(unread).unwrap(), json!([]));
    let saved = std::fs::read_to_string(&made).unwrap();
    assert!(saved.contains("\"message\" \"made\""), "{saved}");
    assert!(saved.contains("info_player_start"), "{saved}");
    assert_eq!(
        answers["5"]["result"]["isError"], true,
        "a map there is not made again"
    );
}

// No spec statement: L-48: map.generate needs no map open, as map.new needs none: with no
// game to take textures from it says so, and makes nothing
#[test]
fn a_level_is_generated_with_no_map_open() {
    let (server, map) = map_server("map-generate", false);
    let [init, initialized] = opening("2025-11-25");
    let answers = exchange(
        server,
        &[
            init,
            initialized,
            call(1, "map.generate", &json!({ "seed": 3 })),
        ],
    );
    let answer = &answers["1"]["result"];
    assert_eq!(answer["isError"], true, "{answer}");
    let said = answer["content"][0]["text"].as_str().unwrap();
    assert!(said.contains("no texture to paint a wall with"), "{said}");
    assert!(!map.with_file_name("maps").exists());
}

// No spec statement: L-43: edit.repeat makes a map edit again, at once as the edit's own tool
// does: both are in the map and neither waits for review
#[test]
fn a_repeated_map_edit_is_made_at_once() {
    let (server, map) = map_server("map-repeat", false);
    let [init, initialized] = opening("2025-11-25");
    let answers = exchange(
        server,
        &[
            init,
            initialized,
            call(0, "map.open", &json!({ "path": map })),
            call(
                1,
                "map.place_entity",
                &json!({ "classname": "info_player_start", "origin": [0.0, 0.0, 24.0] }),
            ),
            call(2, "edit.repeat", &json!({})),
            call(3, "map.staged_hunks", &json!({})),
            call(4, "map.summary", &json!({})),
        ],
    );
    let text = |id: &str| {
        let answer = &answers[id]["result"];
        assert_eq!(answer["isError"], false, "{id}: {answer}");
        let text = answer["content"][0]["text"].as_str().unwrap();
        serde_json::from_str::<Value>(text).unwrap()
    };
    text("2");
    assert_eq!(text("3"), json!([]), "nothing waits");
    assert_eq!(text("4")["entities"], 3, "the world and two player starts");
}

// No spec statement: P6-36, P4-13: a read-only server refuses every map tool that writes,
// map.open included, which replaces the open map; a map tool that reads is not refused
#[test]
fn a_read_only_server_refuses_the_map_tools_that_write() {
    let (server, map) = map_server("map-read-only", true);
    let [init, initialized] = opening("2025-11-25");
    let offered = map_tools(&server);
    let writes: Vec<&str> = offered
        .iter()
        .filter(|tool| tool.access == Access::Write)
        .map(|tool| &*tool.id)
        .collect();
    for id in ["map.open", "map.place_entity", "map.compile"] {
        assert!(writes.contains(&id), "{id}");
    }
    let mut messages = vec![init, initialized, call(1, "map.staged_hunks", &json!({}))];
    for (n, id) in (3..).zip(&writes) {
        let arguments = if *id == "map.open" {
            json!({ "path": map })
        } else {
            json!({})
        };
        messages.push(call(n, id, &arguments));
    }
    let answers = exchange(server, &messages);
    let read = answers["1"]["result"].to_string();
    assert!(!read.contains("outside the roots"), "{read}");
    for (n, id) in (3..).zip(&writes) {
        let answer = &answers[&n.to_string()]["result"];
        assert_eq!(answer["isError"], true, "{id}: {answer}");
        let text = answer["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("outside the roots"), "{id}: {text}");
    }
}

// No spec statement: P6-36, P4-13: connected to the app, a map tool that writes is refused when
// the map the app has open lies outside the write roots, since it may write beside it, and
// nothing waits for review; so is edit.undo, which may take back an edit of that map
#[test]
fn a_map_outside_the_write_roots_is_not_written() {
    let root = project("map-outside", WORLD);
    let elsewhere = project("map-elsewhere", WORLD);
    let map = elsewhere.join("room.map");
    std::fs::write(&map, "{\n\"classname\" \"worldspawn\"\n}\n").unwrap();
    let socket = if cfg!(windows) {
        PathBuf::from(format!(
            r"\\.\pipe\qs-mcp-map-outside-{}",
            std::process::id()
        ))
    } else {
        root.join("app.sock")
    };
    let host = Host::listen(&socket, || {}).unwrap();
    let (open, opened) = (root.clone(), map);
    // The app: its map open outside the server's roots; it answers the list and the inspect.
    let app = std::thread::spawn(move || {
        let mut registry = Registry::new();
        register(&mut registry).unwrap();
        let mut session = Session::new(Workbench::new(FsLoader, Config::default()), registry);
        session
            .execute("project.open", json!({ "path": open }))
            .unwrap();
        session
            .execute("map.open", json!({ "path": opened }))
            .unwrap();
        // The list, then the inspect before the edit and the one before the undo.
        for _ in 0..3 {
            assert!(host.answer_next(&mut session));
        }
        session.execute("map.staged_hunks", json!({})).unwrap()
    });
    let settings = ServerSettings {
        project: root.clone(),
        read: vec![root.clone()],
        write: vec![root],
        name: "quakec-ide".to_owned(),
        version: "0".to_owned(),
    };
    let connected = Server::start(
        move || Connected::connect(&socket).map_err(|e| e.to_string()),
        settings,
    )
    .unwrap();
    let [init, initialized] = opening("2025-11-25");
    let answers = exchange(
        connected,
        &[
            init,
            initialized,
            call(
                1,
                "map.place_entity",
                &json!({ "classname": "light", "origin": [0.0, 0.0, 64.0] }),
            ),
            // The history's last change may be the person's edit of that map.
            call(2, "edit.undo", &json!({})),
        ],
    );
    for id in ["1", "2"] {
        let answer = &answers[id]["result"];
        assert_eq!(answer["isError"], true, "{id}: {answer}");
        assert!(answer.to_string().contains("outside the roots"), "{answer}");
    }
    assert_eq!(app.join().unwrap(), json!([]));
}

// No spec statement: L-43 (the review's findings): with no map open, undo is the session's to
// answer: the server asks nothing of a map and adds no refusal of its own
#[test]
fn undo_with_no_map_open_is_the_sessions_to_answer() {
    let root = project("undo-no-map", "void() worldspawn = {};\n");
    let [init, initialized] = opening("2025-11-25");
    let answers = exchange(
        server(&root, false),
        &[init, initialized, call(1, "edit.undo", &json!({}))],
    );
    let answer = answers["1"]["result"].to_string();
    assert!(!answer.contains("outside the roots"), "{answer}");
    assert!(!answer.contains("no map"), "{answer}");
}

// No spec statement: L-42: an agent whose client defers the tool list reads the server's
// instructions and the tools' names before it searches, so the instructions name every kind
// of tool the server's own session offers, the preview's among them, by the start of its
// name, and the kinds the running app adds
#[test]
fn the_instructions_name_every_kind_of_tool() {
    let (server, _) = map_server("kinds", false);
    let mut kinds: Vec<&str> = server
        .tool_names()
        .into_iter()
        .filter_map(|name| name.split_once('.').map(|(kind, _)| kind))
        .collect();
    kinds.sort_unstable();
    kinds.dedup();
    assert!(
        ["map", "project", "preview", "scenario", "texture"]
            .iter()
            .all(|kind| kinds.contains(kind)),
        "{kinds:?}"
    );
    // L-45: the instructions send an agent to the reference before it writes QuakeC, and
    // the reference is a tool the server has.
    assert!(super::INSTRUCTIONS.contains("project.reference"));
    for topic in Topic::ALL {
        let name = serde_json::to_value(topic).unwrap();
        let phrase = format!("topic {}", name.as_str().unwrap());
        assert!(super::INSTRUCTIONS.contains(&phrase), "{phrase}");
    }
    assert!(server.tool_names().contains(&"project.reference"));
    // The app's own kinds, which the server offers only with `--connect`.
    for kind in kinds
        .into_iter()
        .chain(["view", "editor", "settings", "help"])
    {
        assert!(
            super::INSTRUCTIONS.contains(&format!("{kind}.")),
            "the instructions do not name {kind}.*"
        );
    }
}
