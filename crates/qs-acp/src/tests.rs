// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::unreachable,
    reason = "test code: a failed unwrap or index, or an event of another kind, is a failed test"
)]

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::{
    Client, ClientInfo, Event, McpServer, OptionKind, Outcome, PlanEntry, SessionId, StopReason,
    ToolStatus,
};

/// A client writing to memory.
fn client() -> Client<Vec<u8>> {
    Client::new(Vec::new())
}

/// The messages the client has written, one per line, taken out of it.
fn written(client: &mut Client<Vec<u8>>) -> Vec<Value> {
    let text = String::from_utf8(core::mem::take(&mut client.out)).unwrap();
    assert!(text.is_empty() || text.ends_with('\n'), "{text:?}");
    text.lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// The events of one line from the agent.
fn told(client: &mut Client<Vec<u8>>, message: &Value) -> Vec<Event> {
    client.handle(&message.to_string()).unwrap()
}

fn info() -> ClientInfo {
    ClientInfo {
        name: "quakec-studio".to_owned(),
        title: "QuakeC Studio".to_owned(),
        version: "1.2.3".to_owned(),
    }
}

/// A client with a session: `initialize` and `session/new` asked and answered.
fn in_session() -> (Client<Vec<u8>>, SessionId) {
    let mut client = client();
    client.initialize(&info()).unwrap();
    told(
        &mut client,
        &json!({ "jsonrpc": "2.0", "id": 0, "result": { "protocolVersion": 1 } }),
    );
    client.new_session(Path::new("/mods/ember"), &[]).unwrap();
    let events = told(
        &mut client,
        &json!({ "jsonrpc": "2.0", "id": 1, "result": { "sessionId": "sess_1" } }),
    );
    let [Event::SessionReady(session)] = events.as_slice() else {
        unreachable!("{events:?}");
    };
    let session = session.clone();
    written(&mut client);
    (client, session)
}

/// Whether `value` is valid as the definition `name` of the protocol's schema.
fn valid_as(name: &str, value: &Value) -> bool {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../specs/acp/v1/schema/schema.json");
    let schema: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let wrapped = json!({ "$ref": format!("#/$defs/{name}"), "$defs": schema["$defs"] });
    jsonschema::validator_for(&wrapped).unwrap().is_valid(value)
}

// ACP v1, initialization.mdx: "Clients **MUST** initialize the connection by calling the
// `initialize` method with: The latest protocol version supported; The capabilities
// supported. They **SHOULD** also provide a name and version to the Agent."
#[test]
fn initialize_gives_the_version_and_who_asks() {
    let mut client = client();
    client.initialize(&info()).unwrap();
    let sent = written(&mut client);
    assert_eq!(
        sent,
        [json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": {
                "protocolVersion": 1,
                "clientCapabilities": {},
                "clientInfo": {
                    "name": "quakec-studio",
                    "title": "QuakeC Studio",
                    "version": "1.2.3",
                },
            },
        })]
    );
    assert!(valid_as("InitializeRequest", &sent[0]["params"]));
}

// ACP v1, initialization.mdx: "The Agent **MUST** respond with the chosen protocol version
// and the capabilities it supports. It **SHOULD** also provide a name and version"
#[test]
fn the_agents_answer_names_it() {
    let mut client = client();
    client.initialize(&info()).unwrap();
    let events = told(
        &mut client,
        &json!({ "jsonrpc": "2.0", "id": 0, "result": {
            "protocolVersion": 1,
            "agentCapabilities": { "loadSession": true },
            "agentInfo": { "name": "my-agent", "title": "My Agent", "version": "1.0.0" },
            "authMethods": [],
        }}),
    );
    assert_eq!(
        events,
        [Event::Initialized {
            agent: Some("My Agent 1.0.0".to_owned())
        }]
    );
    // An answer is taken once.
    let again = told(
        &mut client,
        &json!({ "jsonrpc": "2.0", "id": 0, "result": { "protocolVersion": 1 } }),
    );
    assert_eq!(again, []);
}

// ACP v1, initialization.mdx: "If the Client does not support the version specified by the
// Agent in the `initialize` response, the Client **SHOULD** close the connection and inform
// the user about it."
#[test]
fn another_version_is_reported() {
    let mut client = client();
    client.initialize(&info()).unwrap();
    let events = told(
        &mut client,
        &json!({ "jsonrpc": "2.0", "id": 0, "result": { "protocolVersion": 2 } }),
    );
    assert_eq!(events, [Event::WrongVersion(2)]);
}

// ACP v1, session-setup.mdx: "Clients create a new session by calling the `session/new`
// method with: The working directory for the session; A list of MCP servers the Agent
// should connect to"
#[test]
fn a_session_is_asked_for_with_its_directory_and_servers() {
    let mut client = client();
    let server = McpServer {
        name: "QuakeC Studio".to_owned(),
        command: PathBuf::from("/app/quakec-ide"),
        args: vec!["mcp".to_owned(), "--connect".to_owned()],
        env: vec![("RUST_LOG".to_owned(), "warn".to_owned())],
    };
    client
        .new_session(Path::new("/mods/ember"), &[server])
        .unwrap();
    let sent = written(&mut client);
    assert_eq!(
        sent[0]["params"],
        json!({
            "cwd": "/mods/ember",
            "mcpServers": [{
                "name": "QuakeC Studio",
                "command": "/app/quakec-ide",
                "args": ["mcp", "--connect"],
                "env": [{ "name": "RUST_LOG", "value": "warn" }],
            }],
        })
    );
    assert_eq!(sent[0]["method"], "session/new");
    assert!(valid_as("NewSessionRequest", &sent[0]["params"]));
    let events = told(
        &mut client,
        &json!({ "jsonrpc": "2.0", "id": 0, "result": { "sessionId": "sess_abc" } }),
    );
    let [Event::SessionReady(session)] = events.as_slice() else {
        unreachable!("{events:?}");
    };
    assert_eq!(session.as_str(), "sess_abc");
}

// ACP v1, prompt-turn.mdx: "The turn begins when the Client sends a `session/prompt`", and
// its updates: `agent_message_chunk`, `agent_thought_chunk`, `plan`; the turn ends with
// "the `session/prompt` response with a stop reason"
#[test]
fn a_turn_streams_and_ends() {
    let (mut client, session) = in_session();
    client.prompt(&session, "What does this code do?").unwrap();
    let sent = written(&mut client);
    assert_eq!(
        sent[0]["params"],
        json!({
            "sessionId": "sess_1",
            "prompt": [{ "type": "text", "text": "What does this code do?" }],
        })
    );
    assert!(valid_as("PromptRequest", &sent[0]["params"]));
    let update = |update: Value| {
        json!({ "jsonrpc": "2.0", "method": "session/update",
                "params": { "sessionId": "sess_1", "update": update } })
    };
    let chunk = |kind: &str, text: &str| {
        update(json!({ "sessionUpdate": kind, "content": { "type": "text", "text": text } }))
    };
    assert_eq!(
        told(&mut client, &chunk("agent_thought_chunk", "Reading.")),
        [Event::Thought("Reading.".to_owned())]
    );
    assert_eq!(
        told(&mut client, &chunk("agent_message_chunk", "It jumps.")),
        [Event::Message("It jumps.".to_owned())]
    );
    let plan = update(json!({ "sessionUpdate": "plan", "entries": [
        { "content": "Read demon.qc", "priority": "high", "status": "completed" },
    ]}));
    assert_eq!(
        told(&mut client, &plan),
        [Event::Plan(vec![PlanEntry {
            content: "Read demon.qc".to_owned(),
            status: "completed".to_owned(),
        }])]
    );
    // An update the panel does not show, a chunk that is not text and a notification that
    // is not an update give nothing, and are no error.
    for quiet in [
        update(json!({ "sessionUpdate": "usage_update", "used": 1, "size": 2 })),
        update(json!({ "sessionUpdate": "a_kind_added_later" })),
        update(json!({ "sessionUpdate": "agent_message_chunk",
                       "content": { "type": "image", "data": "", "mimeType": "image/png" } })),
        json!({ "jsonrpc": "2.0", "method": "$/cancel_request", "params": { "requestId": 9 } }),
    ] {
        assert_eq!(told(&mut client, &quiet), [], "{quiet}");
    }
    for (reason, stop) in [
        ("end_turn", StopReason::EndTurn),
        ("max_tokens", StopReason::MaxTokens),
        ("max_turn_requests", StopReason::MaxTurnRequests),
        ("refusal", StopReason::Refusal),
        ("cancelled", StopReason::Cancelled),
    ] {
        let (mut client, session) = in_session();
        client.prompt(&session, "go").unwrap();
        let ended = json!({ "jsonrpc": "2.0", "id": 2, "result": { "stopReason": reason } });
        assert_eq!(told(&mut client, &ended), [Event::TurnEnded(stop)]);
    }
}

// ACP v1, tool-calls.mdx: a `tool_call` reports a call, and `tool_call_update` "Only the
// fields being updated need to be included"
#[test]
fn tool_calls_are_reported_and_updated() {
    let (mut client, _) = in_session();
    let update = |update: Value| {
        json!({ "jsonrpc": "2.0", "method": "session/update",
                "params": { "sessionId": "sess_1", "update": update } })
    };
    let events = told(
        &mut client,
        &update(json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "call_001",
            "title": "Reading demon.qc",
            "kind": "read",
            "status": "pending",
            "locations": [{ "path": "/mods/ember/demon.qc", "line": 266 }],
        })),
    );
    let [Event::ToolCall(call)] = events.as_slice() else {
        unreachable!("{events:?}");
    };
    assert_eq!(call.id.as_str(), "call_001");
    assert_eq!(call.title.as_deref(), Some("Reading demon.qc"));
    assert_eq!(call.status, Some(ToolStatus::Pending));
    assert_eq!(call.paths, [PathBuf::from("/mods/ember/demon.qc")]);

    let events = told(
        &mut client,
        &update(json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "call_001",
            "status": "completed",
            "content": [
                { "type": "content", "content": { "type": "text", "text": "29 lines" } },
                { "type": "diff", "path": "/mods/ember/ai.qc", "oldText": "a", "newText": "b" },
                { "type": "terminal", "terminalId": "term_1" },
            ],
        })),
    );
    let [Event::ToolCallUpdate(call)] = events.as_slice() else {
        unreachable!("{events:?}");
    };
    assert_eq!(call.title, None, "not in the update");
    assert_eq!(call.status, Some(ToolStatus::Completed));
    assert_eq!(call.text, ["29 lines"]);
    assert_eq!(call.paths, [PathBuf::from("/mods/ember/ai.qc")]);
    for (status, known) in [
        ("in_progress", Some(ToolStatus::InProgress)),
        ("failed", Some(ToolStatus::Failed)),
        ("a_status_added_later", None),
    ] {
        let events = told(
            &mut client,
            &update(
                json!({ "sessionUpdate": "tool_call_update", "toolCallId": "c",
                            "status": status }),
            ),
        );
        let [Event::ToolCallUpdate(call)] = events.as_slice() else {
            unreachable!("{events:?}");
        };
        assert_eq!(call.status, known, "{status}");
    }
}

/// The agent's permission request with JSON-RPC id `id`, for call `call_001` of `sess_1`.
fn permission_request(id: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "session/request_permission",
        "params": {
            "sessionId": "sess_1",
            "toolCall": { "toolCallId": "call_001", "title": "Write ai.qc" },
            "options": [
                { "optionId": "allow-once", "name": "Allow once", "kind": "allow_once" },
                { "optionId": "allow-all", "name": "Always", "kind": "allow_always" },
                { "optionId": "reject-once", "name": "Reject", "kind": "reject_once" },
                { "optionId": "reject-all", "name": "Never", "kind": "reject_always" },
            ],
        },
    })
}

// ACP v1, tool-calls.mdx: "The Agent **MAY** request permission from the user before
// executing a tool call by calling the `session/request_permission` method", and "The Client
// responds with the user's decision"
#[test]
fn a_permission_request_waits_for_its_answer() {
    let (mut client, _) = in_session();
    let events = told(&mut client, &permission_request(&json!(5)));
    let [
        Event::PermissionAsked {
            request,
            tool,
            options,
        },
    ] = events.as_slice()
    else {
        unreachable!("{events:?}");
    };
    assert_eq!(tool.title.as_deref(), Some("Write ai.qc"));
    let kinds: Vec<OptionKind> = options.iter().map(|option| option.kind).collect();
    assert_eq!(
        kinds,
        [
            OptionKind::AllowOnce,
            OptionKind::AllowAlways,
            OptionKind::RejectOnce,
            OptionKind::RejectAlways
        ]
    );
    assert_eq!(options[0].name, "Allow once");
    assert!(
        written(&mut client).is_empty(),
        "nothing is answered unasked"
    );

    client
        .answer(request, &Outcome::Selected(options[0].id.clone()))
        .unwrap();
    let sent = written(&mut client);
    assert_eq!(
        sent,
        [json!({ "jsonrpc": "2.0", "id": 5, "result": {
            "outcome": { "outcome": "selected", "optionId": "allow-once" } } })]
    );
    assert!(valid_as("RequestPermissionResponse", &sent[0]["result"]));
    // A request is answered once.
    client.answer(request, &Outcome::Cancelled).unwrap();
    assert!(written(&mut client).is_empty());
}

// ACP v1, prompt-turn.mdx, Cancellation: "The Client **MUST** respond to all pending
// `session/request_permission` requests with the `cancelled` outcome."
#[test]
fn cancelling_answers_the_requests_that_wait() {
    let (mut client, session) = in_session();
    told(&mut client, &permission_request(&json!(5)));
    told(&mut client, &permission_request(&json!("req-b")));
    client.cancel(&session).unwrap();
    let sent = written(&mut client);
    assert_eq!(
        sent[0],
        json!({ "jsonrpc": "2.0", "method": "session/cancel",
                "params": { "sessionId": "sess_1" } })
    );
    assert!(valid_as("CancelNotification", &sent[0]["params"]));
    let cancelled = json!({ "outcome": { "outcome": "cancelled" } });
    let mut answered: Vec<(Value, Value)> = sent[1..]
        .iter()
        .map(|message| (message["id"].clone(), message["result"].clone()))
        .collect();
    answered.sort_by_key(|(id, _)| id.to_string());
    assert_eq!(
        answered,
        [
            (json!("req-b"), cancelled.clone()),
            (json!(5), cancelled.clone())
        ]
    );
    assert!(valid_as("RequestPermissionResponse", &cancelled));
    // None is left to answer again.
    client.cancel(&session).unwrap();
    assert_eq!(written(&mut client).len(), 1);
}

// ACP v1, initialization.mdx: "Clients and Agents **MUST** treat all capabilities omitted in
// the `initialize` request as **UNSUPPORTED**": a request for a method this client did not
// offer is answered as JSON-RPC 2.0 answers a method that is not there, code -32601
#[test]
fn a_method_not_offered_is_refused() {
    let (mut client, _) = in_session();
    for request in [
        json!({ "jsonrpc": "2.0", "id": 7, "method": "fs/read_text_file",
                "params": { "sessionId": "sess_1", "path": "/mods/ember/ai.qc" } }),
        // A permission request that is not the protocol's is no request to show.
        json!({ "jsonrpc": "2.0", "id": 7, "method": "session/request_permission",
                "params": { "sessionId": "sess_1" } }),
    ] {
        assert_eq!(told(&mut client, &request), []);
        assert_eq!(
            written(&mut client),
            [json!({ "jsonrpc": "2.0", "id": 7,
                     "error": { "code": -32601, "message": "Method not found" } })]
        );
    }
}

// No spec statement: an error answer to one of the client's requests names the method and
// gives the agent's reason; an answer that is not the protocol's is refused the same way
#[test]
fn a_refused_request_says_why() {
    let mut client = client();
    client.initialize(&info()).unwrap();
    client.new_session(Path::new("/mods/ember"), &[]).unwrap();
    let events = told(
        &mut client,
        &json!({ "jsonrpc": "2.0", "id": 1,
                 "error": { "code": -32000, "message": "Authentication required" } }),
    );
    assert_eq!(
        events,
        [Event::Refused {
            method: "session/new",
            message: "Authentication required".to_owned()
        }]
    );
    let events = told(
        &mut client,
        &json!({ "jsonrpc": "2.0", "id": 0, "result": { "version": "one" } }),
    );
    assert!(
        matches!(
            events.as_slice(),
            [Event::Refused {
                method: "initialize",
                ..
            }]
        ),
        "{events:?}"
    );
}

// ACP v1, transports.mdx: "Messages are delimited by newlines (`\n`), and **MUST NOT**
// contain embedded newlines."; a line that is no JSON-RPC message is an error to the caller
#[test]
fn each_message_is_one_line() {
    let (mut client, session) = in_session();
    client.prompt(&session, "line one\nline two\r\n").unwrap();
    let text = String::from_utf8(core::mem::take(&mut client.out)).unwrap();
    assert_eq!(text.matches('\n').count(), 1, "{text:?}");
    assert!(text.ends_with('\n'));
    assert!(client.handle("not json").is_err());
    assert_eq!(told(&mut client, &json!({ "jsonrpc": "2.0" })), []);
}

// ACP v1, transports.mdx, stdio: "The client launches the agent as a subprocess. The agent
// reads JSON-RPC messages from its standard input (`stdin`) and sends messages to its
// standard output (`stdout`)."; its output closing is the agent's end, told once
#[cfg(unix)]
#[test]
fn an_agent_runs_as_a_subprocess() {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::{Agent, AgentCommand};

    // An agent of one answer: it reads a line and answers `initialize`, then ends.
    let script = r#"read line; echo '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":1}}'; echo "to the log" >&2"#;
    let command = AgentCommand {
        program: PathBuf::from("/bin/sh"),
        args: vec!["-c".to_owned(), script.to_owned()],
        env: Vec::new(),
    };
    let (woken, wakes) = mpsc::channel();
    let mut agent = Agent::spawn(&command, Path::new("/"), move || {
        let _gone = woken.send(());
    })
    .unwrap();
    agent.client().initialize(&info()).unwrap();
    let mut events = Vec::new();
    // Each wake is a line or the end; a wake that never comes fails the test.
    while !events.contains(&Event::Exited) {
        wakes.recv_timeout(Duration::from_secs(30)).unwrap();
        events.extend(agent.poll());
    }
    assert_eq!(events, [Event::Initialized { agent: None }, Event::Exited]);
    assert!(agent.poll().is_empty(), "the end is told once");

    let missing = AgentCommand {
        program: PathBuf::from("/no/such/agent"),
        args: Vec::new(),
        env: Vec::new(),
    };
    let error = Agent::spawn(&missing, Path::new("/"), || {}).unwrap_err();
    assert!(error.to_string().contains("/no/such/agent"), "{error}");
}
