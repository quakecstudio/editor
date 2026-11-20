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
    let ctx = egui::Context::default();
    let (root, app) = window_on(&ctx, name, script);
    (root, ctx, app)
}

/// [`window`] drawn on `ctx`.
fn window_on(ctx: &egui::Context, name: &str, script: &str) -> (PathBuf, App) {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("qs-external-{name}-{}", std::process::id()));
    let _gone = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("progs.src"), "progs.dat\nworld.qc\n").unwrap();
    fs::write(root.join("world.qc"), "void() worldspawn = {};\n").unwrap();
    fs::write(root.join("agent.sh"), script).unwrap();
    let mut app = App::checked_on_the_frame(ctx, None).unwrap();
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
    (root, app)
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

/// The app a harness draws.
fn app_of<'a>(harness: &'a mut egui_kittest::Harness<'static, Option<App>>) -> &'a mut App {
    harness.state_mut().as_mut().unwrap()
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
printf '%s\n' '{"jsonrpc":"2.0","id":13,"method":"fs/write_text_file","params":{"sessionId":"s1","path":"'"$1"'/../climbs-out.qc","content":"x"}}'
read -r line; printf '%s\n' "$line" > "$1/climbs.json"
printf '%s\n' '{"jsonrpc":"2.0","id":14,"method":"fs/write_text_file","params":{"sessionId":"s1","path":"'"$1"'/new.qc","content":"float fresh;\n"}}'
read -r line; printf '%s\n' "$line" > "$1/new.json"
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
    assert_eq!(hunks.len(), 2, "{hunks:?}");
    let of = |name: &str| hunks.iter().find(|hunk| hunk.path.ends_with(name)).unwrap();
    assert_eq!(of("world.qc").added, "float added;\n");
    // ACP v1, file-system.mdx: "The Client **MUST** create the file if it doesn't exist.": a
    // file that is not there is taken, staged as a new file, which accepting its hunk makes
    assert_eq!(
        read_json(&root, "new.json")["result"],
        serde_json::json!({})
    );
    assert_eq!(of("new.qc").added, "float fresh;\n");
    assert!(
        !root.join("new.qc").exists(),
        "made when its hunk is accepted"
    );
    // A path that climbs out of the project with `..` is outside it.
    let climbs = read_json(&root, "climbs.json");
    assert_eq!(climbs["error"]["message"], Text::AgentOutside.get());
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

// No spec statement: P4-14, the acceptance criterion: with code selected, the editor's context
// menu starts a thread that asks what the code does, naming the file and its lines and giving
// the code; the agent panel is shown
#[test]
fn the_editors_menu_asks_what_selected_code_does() {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use qs_layout::{DockPanel, Panel};

    let script = format!(
        "{HANDSHAKE}{}",
        r#"
printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"stopReason":"end_turn"}}'
"#
    );
    let mut harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .build_ui_state(
            |ui, app: &mut Option<App>| {
                if let Some(app) = app {
                    app.frame(ui);
                }
            },
            None,
        );
    let (root, mut app) = window_on(&harness.ctx, "explain", &script);
    app.shell_mut()
        .execute(
            "file.open",
            serde_json::json!({ "path": root.join("world.qc") }),
        )
        .unwrap();
    *harness.state_mut() = Some(app);
    harness.run();
    app_of(&mut harness)
        .shell_mut()
        .execute(qs_layout::FOCUS_EDITOR, serde_json::json!({}))
        .unwrap();
    harness.run();

    // With no selection the editor has no menu.
    let editor = egui::pos2(700.0, 300.0);
    let secondary_click = |harness: &mut Harness<'static, Option<App>>| {
        harness.event(egui::Event::PointerMoved(editor));
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos: editor,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
        harness.run();
    };
    secondary_click(&mut harness);
    assert!(harness.query_by_label(Text::ExplainMenu.get()).is_none());

    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.run();
    secondary_click(&mut harness);
    harness.get_by_label(Text::ExplainMenu.get()).click();
    // One frame: the agent wakes the window for as long as it talks.
    harness.step();

    while thread(app_of(&mut harness)).running() {
        assert!(app_of(&mut harness).shell_mut().wait_external_agent(0));
    }
    let prompt = read_json(&root, "prompt.json");
    let asked = prompt["params"]["prompt"][0]["text"].as_str().unwrap();
    assert!(asked.starts_with(Text::ExplainAsk.get()), "{asked}");
    assert!(asked.contains("world.qc, lines 1 to 2:"), "{asked}");
    assert!(asked.contains("void() worldspawn = {};"), "{asked}");
    let shown = app_of(&mut harness)
        .shell()
        .layout()
        .docks
        .shown(Panel::Right);
    assert_eq!(shown, Some(DockPanel::Agent));
}

// No spec statement: P4-14, AGT-3, an agent that writes the disk itself, as Claude Code's
// adapter does, leaves no hunk to reject: the checkpoint recorded before it started puts the
// project back as it was
#[test]
fn the_checkpoint_before_the_agent_restores_what_it_wrote() {
    let script = format!(
        "{HANDSHAKE}{}",
        r#"
printf 'float written_by_the_agent;\n' > "$1/world.qc"
printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"stopReason":"end_turn"}}'
"#
    );
    let (root, ctx, mut app) = window("checkpoint", &script);
    run(&ctx, &mut app, "Rewrite the world.");
    assert_eq!(
        fs::read_to_string(root.join("world.qc")).unwrap(),
        "float written_by_the_agent;\n"
    );
    let checkpoint = thread(&mut app).checkpoint().unwrap();
    app.shell_mut()
        .agent_action(AgentAction::Restore(checkpoint), &ctx);
    assert_eq!(
        fs::read_to_string(root.join("world.qc")).unwrap(),
        "void() worldspawn = {};\n"
    );
}

// No spec statement: P4-14, permission requests that come together each wait their turn, the
// oldest first, and each is answered
#[test]
fn permission_requests_that_come_together_are_each_answered() {
    let request = |id: u32, title: &str| {
        format!(
            r#"printf '%s\n' '{{"jsonrpc":"2.0","id":{id},"method":"session/request_permission","params":{{"sessionId":"s1","toolCall":{{"toolCallId":"c{id}","title":"{title}"}},"options":[{{"optionId":"yes","name":"Allow {title}","kind":"allow_once"}}]}}}}'"#
        )
    };
    let script = format!(
        "{HANDSHAKE}\n{}\n{}\n{}",
        request(20, "first"),
        request(21, "second"),
        r#"
read -r line; printf '%s\n' "$line" > "$1/a.json"
read -r line; printf '%s\n' "$line" > "$1/b.json"
printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"stopReason":"end_turn"}}'
"#
    );
    let (root, ctx, mut app) = window("two-asks", &script);
    let mut seen = Vec::new();
    app.shell_mut()
        .agent_action(AgentAction::Start("Go.".to_owned()), &ctx);
    while thread(&mut app).running() {
        assert!(app.shell_mut().wait_external_agent(0));
        let waiting = thread(&mut app);
        while let Some(options) = waiting.asking() {
            seen.extend(options);
            waiting.permit_for_test(0);
        }
    }
    assert_eq!(seen, ["Allow first", "Allow second"]);
    assert_eq!(read_json(&root, "a.json")["id"], 20);
    assert_eq!(read_json(&root, "b.json")["id"], 21);
}

// No spec statement: P4-14, an agent that does not answer `session/cancel` cannot hold its
// thread: the first Stop asks it to cancel, a second ends its process
#[test]
fn a_second_stop_ends_an_agent_that_does_not_answer() {
    let script = format!("{HANDSHAKE}\nwhile read -r line; do :; done\n");
    let (_root, ctx, mut app) = window("deaf", &script);
    app.shell_mut()
        .agent_action(AgentAction::Start("Go.".to_owned()), &ctx);
    while thread(&mut app).shown().1 != "Working" {
        assert!(app.shell_mut().wait_external_agent(0));
    }
    thread(&mut app).stop_for_test();
    assert_eq!(thread(&mut app).shown().1, "Stopping");
    assert!(thread(&mut app).running());
    thread(&mut app).stop_for_test();
    assert_eq!(thread(&mut app).shown().1, "Ended(Cancelled)");
    assert!(!thread(&mut app).running());
}

// No spec statement: P4-14, a new thread runs on the built-in agent when a model provider is
// set up and on the first external agent when none is, whichever the app reads first at its
// start
#[test]
fn the_agent_a_new_thread_runs_on_by_default() {
    use qs_models::{ProviderConfig, ProviderKind};

    let agents = || vec![claude_code()];
    let mut panel = AgentPanel::new(Box::new(|_| Err("no model".to_owned())));
    assert!(panel.external_agent().is_none());
    panel.set_external_agents(agents());
    assert_eq!(panel.external_agent(), Some(&claude_code()));
    // The provider read after the agents, as the app reads them.
    panel.set_config(ProviderConfig::new(ProviderKind::Ollama, "local"));
    assert!(panel.external_agent().is_none(), "the built-in agent");

    let mut panel = AgentPanel::new(Box::new(|_| Err("no model".to_owned())));
    panel.set_config(ProviderConfig::new(ProviderKind::Ollama, "local"));
    panel.set_external_agents(agents());
    assert!(panel.external_agent().is_none(), "the built-in agent");
    panel.set_external_agents(Vec::new());
    assert!(panel.external_agent().is_none());
}

// No spec statement: P4-14, the agent is found on the login shell's PATH, which an app
// started from the Finder does not have: a login shell is asked for it
#[test]
fn the_login_shells_path_is_asked_for() {
    let path = super::path_of_login_shell(Path::new("/bin/sh")).unwrap();
    assert!(
        path.split(':')
            .any(|folder| folder == "/bin" || folder == "/usr/bin"),
        "{path}"
    );
    assert!(super::path_of_login_shell(Path::new("/no/such/shell")).is_none());
}

// No spec statement: P4-14, with an external agent and no model provider the provider form
// can be left without saving one: Cancel goes back to the threads
#[test]
fn the_provider_form_can_be_left_without_saving() {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    let mut harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .build_ui_state(
            |ui, app: &mut Option<App>| {
                if let Some(app) = app {
                    app.frame(ui);
                }
            },
            None,
        );
    let (_root, mut app) = window_on(&harness.ctx, "form", "");
    app.shell_mut()
        .execute(
            qs_layout::SHOW_PANEL,
            serde_json::json!({ "panel": "agent", "when_shown": "keep" }),
        )
        .unwrap();
    *harness.state_mut() = Some(app);
    harness.run();
    harness.get_by_label(Text::AgentRun.get());
    harness.get_by_label(Text::AgentSettings.get()).click();
    harness.run();
    assert!(harness.query_by_label(Text::AgentRun.get()).is_none());
    harness.get_by_label(Text::AgentSave.get());
    harness.get_by_label(Text::PaletteCancel.get()).click();
    harness.run();
    harness.get_by_label(Text::AgentRun.get());
    assert!(
        app_of(&mut harness)
            .shell_mut()
            .agent_mut()
            .external_agent()
            .is_some()
    );
}
