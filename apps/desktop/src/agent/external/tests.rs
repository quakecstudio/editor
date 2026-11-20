// SPDX-License-Identifier: GPL-3.0-or-later

//! External threads against a scripted agent: a shell script that speaks the protocol's
//! lines in order, so the tests run it as the panel runs a real agent, as a subprocess.

#![cfg(unix)]
#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::fs;
use std::path::{Path, PathBuf};

use eframe::egui;
use qs_settings::ExternalAgent;

use super::{ExternalThread, claude_code};
use crate::App;
use crate::agent::{AgentAction, AgentPanel};
use crate::strings::Text;

/// A project on disk for test `name`, open in a window whose agent panel has one external
/// agent: `/bin/sh` running `script`, which is given the project's folder as `$1`.
fn window(name: &str, script: &str) -> (PathBuf, egui::Context, App) {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("qs-external-{name}-{}", std::process::id()));
    let _gone = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("progs.src"), "progs.dat\nworld.qc\n").unwrap();
    fs::write(root.join("world.qc"), "void() worldspawn = {};\n").unwrap();
    fs::write(root.join("agent.sh"), script).unwrap();
    let ctx = egui::Context::default();
    let mut app = App::checked_on_the_frame(&ctx, None).unwrap();
    *app.shell_mut().agent_mut() = AgentPanel::new(Box::new(|_| Err("no model".to_owned())));
    app.shell_mut()
        .agent_mut()
        .set_external_agents(vec![ExternalAgent {
            name: "Scripted".to_owned(),
            command: "/bin/sh".to_owned(),
            args: vec![
                root.join("agent.sh").to_string_lossy().into_owned(),
                root.to_string_lossy().into_owned(),
            ],
            env: std::collections::BTreeMap::new(),
        }]);
    app.shell_mut().open_project(&root).unwrap();
    (root, ctx, app)
}

/// The handshake every script begins with: `initialize`, `session/new` and the prompt, each
/// read and the first two answered.
const HANDSHAKE: &str = r#"
read -r line; printf '%s\n' "$line" > "$1/initialize.json"
printf '%s\n' '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":1}}'
read -r line; printf '%s\n' "$line" > "$1/session.json"
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"sessionId":"s1"}}'
read -r line; printf '%s\n' "$line" > "$1/prompt.json"
"#;

/// Starts a thread on `task` and takes what the agent says until the thread is over,
/// answering each permission request with its first option.
fn run(ctx: &egui::Context, app: &mut App, task: &str) {
    app.shell_mut()
        .agent_action(AgentAction::Start(task.to_owned()), ctx);
    while thread(app).running() {
        assert!(
            app.shell_mut().wait_external_agent(0),
            "the agent said nothing more"
        );
        let waiting = thread(app);
        if waiting.asking().is_some() {
            waiting.permit_for_test(0);
        }
    }
}

/// The first external thread.
fn thread(app: &mut App) -> &mut ExternalThread {
    app.shell_mut().agent_mut().external_thread(0).unwrap()
}

fn read_json(root: &Path, name: &str) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(root.join(name)).unwrap()).unwrap()
}

// No spec statement: P4-14, a thread of an external agent: the client says who it is and
// offers its file system, the session is made in the project, the task is the prompt, and
// what the agent says and does is shown, a tool call with its later changes folded in
#[test]
fn an_external_agent_runs_a_thread() {
    let script = format!(
        "{HANDSHAKE}{}",
        r#"
printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s1","update":{"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"Reading."}}}}'
printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s1","update":{"sessionUpdate":"tool_call","toolCallId":"c1","title":"Read world.qc","status":"pending"}}}'
printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s1","update":{"sessionUpdate":"tool_call_update","toolCallId":"c1","status":"completed"}}}'
printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"It spawns "}}}}'
printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"the world."}}}}'
printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"stopReason":"end_turn"}}'
"#
    );
    let (root, ctx, mut app) = window("thread", &script);
    run(&ctx, &mut app, "What does this code do?");

    let initialize = read_json(&root, "initialize.json");
    assert_eq!(initialize["params"]["protocolVersion"], 1);
    assert_eq!(
        initialize["params"]["clientCapabilities"]["fs"],
        serde_json::json!({ "readTextFile": true, "writeTextFile": true })
    );
    assert_eq!(
        initialize["params"]["clientInfo"]["title"],
        Text::AppName.get()
    );
    let session = read_json(&root, "session.json");
    assert_eq!(session["params"]["cwd"], root.to_string_lossy().as_ref());
    let prompt = read_json(&root, "prompt.json");
    assert_eq!(
        prompt["params"]["prompt"][0]["text"],
        "What does this code do?"
    );

    let panel = app.shell_mut().agent_mut();
    assert_eq!((panel.threads(), panel.running()), (1, 0));
    let (items, state) = panel.external_thread(0).unwrap().shown();
    assert_eq!(
        items,
        [
            "thought: Reading.",
            "tool: Read world.qc Some(Completed)",
            "said: It spawns the world.",
        ]
    );
    assert_eq!(state, "Ended(EndTurn)");
}

// No spec statement: P4-14, the acceptance criterion: an external agent's edit of a project
// file is staged as hunks to accept or reject, and nothing is written to disk; its read gets
// the text as the IDE has it, the staged text included; a write outside the project is refused
#[test]
fn an_external_agents_edit_is_staged_for_review() {
    let script = format!(
        "{HANDSHAKE}{}",
        r#"
printf '%s\n' '{"jsonrpc":"2.0","id":10,"method":"fs/write_text_file","params":{"sessionId":"s1","path":"'"$1"'/world.qc","content":"void() worldspawn = {};\nfloat added;\n"}}'
read -r line; printf '%s\n' "$line" > "$1/write.json"
printf '%s\n' '{"jsonrpc":"2.0","id":11,"method":"fs/read_text_file","params":{"sessionId":"s1","path":"'"$1"'/world.qc"}}'
read -r line; printf '%s\n' "$line" > "$1/read.json"
printf '%s\n' '{"jsonrpc":"2.0","id":12,"method":"fs/write_text_file","params":{"sessionId":"s1","path":"/outside-the-project.qc","content":"x"}}'
read -r line; printf '%s\n' "$line" > "$1/outside.json"
printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"stopReason":"end_turn"}}'
"#
    );
    let (root, ctx, mut app) = window("edit", &script);
    run(&ctx, &mut app, "Add a float.");

    assert_eq!(
        read_json(&root, "write.json")["result"],
        serde_json::json!({})
    );
    let hunks = app.shell().workbench().staged_hunks();
    assert_eq!(hunks.len(), 1, "{hunks:?}");
    assert!(hunks[0].path.ends_with("world.qc"));
    assert_eq!(hunks[0].added, "float added;\n");
    assert_eq!(
        fs::read_to_string(root.join("world.qc")).unwrap(),
        "void() worldspawn = {};\n",
        "nothing is written until the hunk is accepted"
    );
    assert_eq!(
        read_json(&root, "read.json")["result"]["content"],
        "void() worldspawn = {};\nfloat added;\n"
    );
    let outside = read_json(&root, "outside.json");
    assert_eq!(outside["error"]["message"], Text::AgentOutside.get());
    assert!(!Path::new("/outside-the-project.qc").exists());
}

// No spec statement: P4-14, the agent's permission request waits in its thread with the
// agent's own options, and the one chosen is its answer
#[test]
fn a_permission_request_is_answered_from_the_thread() {
    let script = format!(
        "{HANDSHAKE}{}",
        r#"
printf '%s\n' '{"jsonrpc":"2.0","id":20,"method":"session/request_permission","params":{"sessionId":"s1","toolCall":{"toolCallId":"c1","title":"Run make"},"options":[{"optionId":"yes","name":"Allow","kind":"allow_once"},{"optionId":"no","name":"Reject","kind":"reject_once"}]}}'
read -r line; printf '%s\n' "$line" > "$1/permission.json"
printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"stopReason":"end_turn"}}'
"#
    );
    let (root, ctx, mut app) = window("permission", &script);
    app.shell_mut()
        .agent_action(AgentAction::Start("Build it.".to_owned()), &ctx);
    while thread(&mut app).asking().is_none() {
        assert!(app.shell_mut().wait_external_agent(0));
    }
    let waiting = thread(&mut app);
    assert_eq!(waiting.asking().unwrap(), ["Allow", "Reject"]);
    waiting.permit_for_test(1);
    assert!(waiting.asking().is_none());
    while thread(&mut app).running() {
        assert!(app.shell_mut().wait_external_agent(0));
    }
    assert_eq!(
        read_json(&root, "permission.json")["result"]["outcome"],
        serde_json::json!({ "outcome": "selected", "optionId": "no" })
    );
}

// ACP v1, prompt-turn.mdx, Cancellation: Stop sends `session/cancel` and answers the
// permission request that waits with the `cancelled` outcome; the turn ends when the agent
// answers the prompt with the `cancelled` stop reason
#[test]
fn stop_cancels_the_turn() {
    let script = format!(
        "{HANDSHAKE}{}",
        r#"
printf '%s\n' '{"jsonrpc":"2.0","id":20,"method":"session/request_permission","params":{"sessionId":"s1","toolCall":{"toolCallId":"c1","title":"Run make"},"options":[{"optionId":"yes","name":"Allow","kind":"allow_once"}]}}'
read -r line; printf '%s\n' "$line" > "$1/first.json"
read -r line; printf '%s\n' "$line" > "$1/second.json"
printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"stopReason":"cancelled"}}'
"#
    );
    let (root, ctx, mut app) = window("stop", &script);
    app.shell_mut()
        .agent_action(AgentAction::Start("Build it.".to_owned()), &ctx);
    while thread(&mut app).asking().is_none() {
        assert!(app.shell_mut().wait_external_agent(0));
    }
    thread(&mut app).stop_for_test();
    while thread(&mut app).running() {
        assert!(app.shell_mut().wait_external_agent(0));
    }
    assert_eq!(read_json(&root, "first.json")["method"], "session/cancel");
    assert_eq!(
        read_json(&root, "second.json")["result"]["outcome"]["outcome"],
        "cancelled"
    );
    let (_, state) = thread(&mut app).shown();
    assert_eq!(state, "Ended(Cancelled)");
}

// No spec statement: P4-14, an agent that cannot be started, or that ends before its turn
// does, says so: under the task box, and in the thread
#[test]
fn an_agent_that_fails_says_why() {
    let (_root, ctx, mut app) = window("ends", "read -r line\n");
    app.shell_mut()
        .agent_action(AgentAction::Start("Go.".to_owned()), &ctx);
    while thread(&mut app).running() {
        assert!(app.shell_mut().wait_external_agent(0));
    }
    let (_, state) = thread(&mut app).shown();
    assert!(state.contains(Text::AgentExited.get()), "{state}");

    app.shell_mut()
        .agent_mut()
        .set_external_agents(vec![ExternalAgent {
            name: "Missing".to_owned(),
            command: "/no/such/agent".to_owned(),
            args: Vec::new(),
            env: std::collections::BTreeMap::new(),
        }]);
    app.shell_mut()
        .agent_action(AgentAction::Start("Go.".to_owned()), &ctx);
    assert_eq!(app.shell_mut().agent_mut().threads(), 1, "none was added");
}

// No spec statement: P4-14, Use Claude Code adds Claude Code's adapter, run by npx at a
// pinned version, to the external agents once, and new threads run on it
#[test]
fn use_claude_code_adds_the_adapter() {
    let (_root, ctx, mut app) = window("claude", "");
    app.shell_mut().agent_mut().set_external_agents(Vec::new());
    assert!(app.shell_mut().agent_mut().external_agent().is_none());
    for _ in 0..2 {
        app.shell_mut()
            .agent_action(AgentAction::UseClaudeCode, &ctx);
    }
    let panel = app.shell_mut().agent_mut();
    assert_eq!(panel.external_agents(), [claude_code()]);
    let chosen = panel.external_agent().unwrap();
    assert_eq!(chosen.name, Text::AgentClaudeCode.get());
    assert_eq!(chosen.command, "npx");
    assert!(chosen.args[1].starts_with("@agentclientprotocol/claude-agent-acp@"));
    // The project is still the window's own.
    assert!(app.shell().workbench().root().is_some());
}
