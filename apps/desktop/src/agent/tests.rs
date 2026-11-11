// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use eframe::egui::{self, RawInput, Rect, Vec2};
use qs_agent::{Message, Model, Outcome, Reply, ToolCall, ToolSpec, history};
use qs_models::{MemoryKeys, ProviderConfig, ProviderKind};
use serde_json::json;

use super::{AgentAction, AgentPanel};
use crate::App;
use crate::shell::Shell;
use crate::strings::Text;

/// A model that waits for its first reply from the test, then answers with the replies the
/// test has sent, and once they run out with the last one again (the run asks again while
/// the work does not pass).
struct Gated {
    gate: mpsc::Receiver<Reply>,
    last: Option<Reply>,
}

impl Model for Gated {
    fn reply(&mut self, _: &str, _: &[ToolSpec], _: &[Message]) -> Result<Reply, String> {
        let next = match &self.last {
            None => self.gate.recv().ok(),
            Some(last) => Some(self.gate.try_recv().unwrap_or_else(|_| last.clone())),
        };
        self.last.clone_from(&next);
        next.ok_or_else(|| "the test hung up".to_owned())
    }
}

/// A project on disk for test `name`, opened in a window whose agent runs on gated models;
/// the gates, one per thread in the order they start.
fn window(name: &str, threads: usize) -> (PathBuf, egui::Context, App, Vec<mpsc::Sender<Reply>>) {
    let ctx = egui::Context::default();
    let (root, app, gates) = window_on(&ctx, name, threads);
    (root, ctx, app, gates)
}

/// [`window`] drawn on `ctx`.
fn window_on(
    ctx: &egui::Context,
    name: &str,
    threads: usize,
) -> (PathBuf, App, Vec<mpsc::Sender<Reply>>) {
    let root = std::env::temp_dir().join(format!("qs-agent-panel-{name}-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("progs.src"), "progs.dat\nworld.qc\n").unwrap();
    std::fs::write(root.join("world.qc"), "void() worldspawn = {};\n").unwrap();
    let (gates, models): (Vec<_>, Vec<_>) = (0..threads).map(|_| mpsc::channel()).unzip();
    let models = Arc::new(Mutex::new(models));
    let mut app = App::checked_on_the_frame(ctx, None).unwrap();
    *app.shell_mut().agent_mut() = AgentPanel::new(Box::new(move |_config| {
        let next = models.lock().unwrap().remove(0);
        let model: Box<dyn Model + Send> = Box::new(Gated {
            gate: next,
            last: None,
        });
        Ok(model)
    }));
    app.shell_mut()
        .agent_mut()
        .set_config(ProviderConfig::new(ProviderKind::Ollama, "local"));
    app.shell_mut().open_project(&root).unwrap();
    (root, app, gates)
}

/// One headless frame.
fn frame(ctx: &egui::Context, app: &mut App) {
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(
            egui::Pos2::ZERO,
            Vec2::new(1440.0, 900.0),
        )),
        ..RawInput::default()
    };
    let mut output = ctx.run_ui(input, |ui| app.frame(ui));
    output.textures_delta.clear();
}

/// Takes what thread `index` sends until it has a report.
fn finish(app: &mut App, index: usize) {
    while app.shell().agent_report(index).is_none() {
        assert!(app.shell_mut().wait_agent(index), "the model ended early");
    }
}

// No spec statement: P4-09, the acceptance criterion: two threads run at once without
// blocking the editor's frames. Both wait on their models while frames go on; each ends when
// its model answers, the other still waiting
#[test]
fn two_threads_run_at_once_without_blocking_frames() {
    let (_root, ctx, mut app, gates) = window("two", 2);
    app.shell_mut()
        .start_agent("Make the axe stronger.", &ctx)
        .unwrap();
    app.shell_mut()
        .start_agent("Add a rocket jump.", &ctx)
        .unwrap();
    for _ in 0..10 {
        frame(&ctx, &mut app);
    }
    assert_eq!(
        app.shell().agent().running(),
        2,
        "both wait, and frames went on"
    );
    gates[1]
        .send(Reply::Finish {
            summary: "jumped".to_owned(),
        })
        .unwrap();
    finish(&mut app, 1);
    frame(&ctx, &mut app);
    assert_eq!(app.shell().agent().running(), 1, "the first still waits");
    gates[0]
        .send(Reply::Finish {
            summary: "stronger".to_owned(),
        })
        .unwrap();
    finish(&mut app, 0);
    assert_eq!(app.shell().agent().running(), 0);
    let report = app.shell().agent_report(0).unwrap();
    assert!(
        report.checkpoint.is_some(),
        "a checkpoint was recorded first"
    );
    assert!(matches!(
        report.outcome,
        Outcome::Done { .. } | Outcome::NotDone { .. }
    ));
}

// No spec statement: P4-09, PRIN-4, a thread's edits are staged hunks: accepted from the panel
// they leave the staged list, and its checkpoint restores the project as it was
#[test]
fn hunks_are_reviewed_and_checkpoints_restored() {
    let (root, ctx, mut app, gates) = window("review", 1);
    app.shell_mut()
        .start_agent("Change the world.", &ctx)
        .unwrap();
    let world = root.join("world.qc").to_string_lossy().into_owned();
    gates[0]
        .send(Reply::Calls {
            text: "Editing.".to_owned(),
            calls: vec![ToolCall {
                id: "c1".to_owned(),
                name: "edit.apply".to_owned(),
                arguments: json!({ "files": [{ "path": world,
                    "text": "void() worldspawn = { local float x; };\n" }] }),
            }],
        })
        .unwrap();
    gates[0]
        .send(Reply::Finish {
            summary: "done".to_owned(),
        })
        .unwrap();
    finish(&mut app, 0);
    let staged = app.shell().workbench().staged_hunks();
    assert_eq!(staged.len(), 1, "{staged:?}");
    app.shell_mut()
        .agent_action(AgentAction::Reject(vec![staged[0].id]), &ctx);
    assert!(app.shell().workbench().staged_hunks().is_empty());
    std::fs::write(root.join("world.qc"), "// changed by hand\n").unwrap();
    let number = app.shell().agent_report(0).unwrap().checkpoint.unwrap();
    app.shell_mut()
        .agent_action(AgentAction::Restore(number), &ctx);
    assert_eq!(
        std::fs::read_to_string(root.join("world.qc")).unwrap(),
        "void() worldspawn = {};\n"
    );
}

// No spec statement: P4-07, P4-09, the provider form saves the key to the keychain and the rest
// to the configuration file, which holds no key; a thread cannot start before a provider is set
#[test]
fn the_provider_form_keeps_the_key_out_of_the_file() {
    let dir = std::env::temp_dir().join(format!("qs-agent-form-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    let file = super::config_path(&dir);
    let mut panel = AgentPanel::new(Box::new(|_| Err("no model".to_owned())));
    panel.set_config_file(file.clone());
    panel.setup.key = "sk-secret-key".to_owned();
    let keys = MemoryKeys::new();
    panel.save_setup(&keys);
    let kept = std::fs::read_to_string(&file).unwrap();
    assert!(
        kept.contains("claude-opus-5-5") && !kept.contains("sk-secret"),
        "{kept}"
    );
    use qs_models::KeyStore as _;
    assert!(keys.get("anthropic").unwrap().is_some());
    let mut reopened = AgentPanel::new(Box::new(|_| Err("no model".to_owned())));
    reopened.set_config_file(file);
    assert_eq!(reopened.config, Some(ProviderConfig::anthropic()));
}

/// A window like [`window`], drawn by a test harness, whose settings are kept under the
/// project and whose project's settings say `settings`; its one thread's gate.
fn harness(
    name: &str,
    settings: &str,
) -> (
    PathBuf,
    egui_kittest::Harness<'static, Option<App>>,
    mpsc::Sender<Reply>,
) {
    let mut harness = egui_kittest::Harness::builder()
        .with_size([1440.0, 900.0])
        .build_ui_state(
            |ui, app: &mut Option<App>| {
                if let Some(app) = app {
                    app.frame(ui);
                }
            },
            None,
        );
    let (root, mut app, mut gates) = window_on(&harness.ctx, name, 1);
    std::fs::create_dir_all(root.join(".quakec-studio")).unwrap();
    std::fs::write(root.join(".quakec-studio/settings.toml"), settings).unwrap();
    app.shell_mut()
        .set_settings_file(root.join("config").join("settings.toml"));
    *harness.state_mut() = Some(app);
    harness.run();
    (root, harness, gates.remove(0))
}

/// The harness's window.
fn shell<'a>(harness: &'a mut egui_kittest::Harness<'static, Option<App>>) -> &'a mut Shell {
    harness.state_mut().as_mut().unwrap().shell_mut()
}

// No spec statement: P4-30, the acceptance criterion: a call that asks waits for the answer in
// the thread, with Allow once, Always allow and Deny, while frames go on; Always allow runs it
// and keeps the rule in the project's settings, which the next thread reads
#[test]
fn a_call_that_asks_waits_in_the_thread_for_the_modder() {
    use egui_kittest::kittest::Queryable as _;
    let (root, mut harness, gate) = harness(
        "asks",
        "[agent]\ntool_permissions = { tools = { \"file.read\" = { default = \"ask\" } } }\n",
    );
    let ctx = harness.ctx.clone();
    shell(&mut harness)
        .start_agent("Read the world.", &ctx)
        .unwrap();
    gate.send(Reply::Calls {
        text: String::new(),
        calls: vec![ToolCall {
            id: "c0".to_owned(),
            name: "file.read".to_owned(),
            arguments: json!({ "path": "world.qc" }),
        }],
    })
    .unwrap();
    while shell(&mut harness).agent().asking(0).is_none() {
        assert!(shell(&mut harness).wait_agent(0), "the model ended early");
    }
    harness.run_steps(5);
    assert_eq!(
        shell(&mut harness).agent().running(),
        1,
        "it waits; frames go on"
    );
    let asks = format!("{} file.read", Text::AgentAsks.get());
    harness.get_by_label(&asks);
    assert!(
        harness.query_all_by_label("world.qc").count() >= 2,
        "the tree and the request"
    );
    harness.get_by_label("file.read's default is ask");
    harness.get_by_label("Allow once");
    harness.get_by_label("Deny");
    // The gated model repeats its last reply until it is sent another, so the finish is queued
    // before the call is answered: the run's next request takes it, and the run cannot spend
    // its turns repeating the call meanwhile.
    gate.send(Reply::Finish {
        summary: "read".to_owned(),
    })
    .unwrap();
    harness.get_by_label("Always allow").click();
    // The model's thread wakes the window as the run goes on, so step rather than settle.
    harness.run_steps(2);
    assert!(shell(&mut harness).agent().asking(0).is_none(), "answered");
    let kept = std::fs::read_to_string(root.join(".quakec-studio/settings.toml")).unwrap();
    assert!(kept.contains("world.qc"), "{kept}");
    assert_eq!(
        shell(&mut harness).agent().permissions().rules.tools["file.read"].allow,
        ["world.qc"]
    );
    let app = harness.state_mut().as_mut().unwrap();
    finish(app, 0);
    let report = app.shell().agent_report(0).unwrap();
    assert!(
        report.transcript.iter().any(|message| matches!(
            message,
            Message::Tool { content, is_error: false, .. } if content.to_string().contains("worldspawn")
        )),
        "the call ran"
    );
}

// No spec statement: P4-30: the profile picked in the panel is kept in the project's settings
// and decides the threads started after it
#[test]
fn the_profile_is_kept_in_the_projects_settings() {
    let (root, mut harness, _gate) = harness("profile", "");
    let ctx = harness.ctx.clone();
    shell(&mut harness).agent_action(AgentAction::Profile(qs_agent::AgentProfile::Ask), &ctx);
    let kept = std::fs::read_to_string(root.join(".quakec-studio/settings.toml")).unwrap();
    assert!(kept.contains("profile = \"ask\""), "{kept}");
    assert_eq!(
        shell(&mut harness).agent().permissions().profile,
        qs_agent::AgentProfile::Ask
    );
}

/// A model that keeps the first message of every request it is sent and finishes at once.
struct Recorder {
    seen: Arc<Mutex<Vec<String>>>,
}

impl Model for Recorder {
    fn reply(&mut self, _: &str, _: &[ToolSpec], messages: &[Message]) -> Result<Reply, String> {
        if let Some(Message::User { text }) = messages.first() {
            self.seen
                .lock()
                .map_err(|_| "the test panicked".to_owned())?
                .push(text.clone());
        }
        Ok(Reply::Finish {
            summary: "explained".to_owned(),
        })
    }
}

/// `app`'s agent on a recording model; what it is sent.
fn recording(app: &mut App) -> Arc<Mutex<Vec<String>>> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&seen);
    *app.shell_mut().agent_mut() = AgentPanel::new(Box::new(move |_config| {
        let model: Box<dyn Model + Send> = Box::new(Recorder {
            seen: Arc::clone(&kept),
        });
        Ok(model)
    }));
    app.shell_mut()
        .agent_mut()
        .set_config(ProviderConfig::new(ProviderKind::Ollama, "local"));
    seen
}

// No spec statement: P4-12, the acceptance criterion, UI-9: a thread started from an entity
// (Ask about this) holds its classname, its current think function and its fields
#[test]
fn a_thread_started_from_an_entity_holds_its_state() {
    let (_root, ctx, mut app, _gates) = window("ask", 0);
    let seen = recording(&mut app);
    let entity = qs_agent::EntityState {
        edict: 12,
        classname: "monster_army".to_owned(),
        fields: vec![
            ("health".to_owned(), "30".to_owned()),
            ("think".to_owned(), "army_stand1".to_owned()),
            ("nextthink".to_owned(), "1.5".to_owned()),
            ("netname".to_owned(), "@file:progs.src".to_owned()),
        ],
    };
    app.shell_mut().ask_about(&entity, &ctx).unwrap();
    finish(&mut app, 0);
    let first = seen.lock().unwrap()[0].clone();
    for part in [
        "#12",
        "classname monster_army",
        "think function is army_stand1, due at time 1.5",
        "- health = 30",
        "- netname = @file:progs.src",
    ] {
        assert!(first.contains(part), "{part} in {first}");
    }
    assert!(
        !first.contains("The file "),
        "a field is never read as a mention: {first}"
    );
}

// No spec statement: P4-12, UI-8: a task's @ mentions attach what they name, read when the
// thread starts; with no game running, @entity says nothing is selected
#[test]
fn a_task_attaches_what_its_mentions_name() {
    let (_root, ctx, mut app, _gates) = window("mentions", 0);
    let seen = recording(&mut app);
    app.shell_mut()
        .start_agent("Why does @file:world.qc fail? See @entity.", &ctx)
        .unwrap();
    finish(&mut app, 0);
    let first = seen.lock().unwrap()[0].clone();
    assert!(first.contains("void() worldspawn = {};"), "{first}");
    assert!(first.contains("Why does @file:world.qc fail?"), "{first}");
    assert!(
        first.contains("No entity is selected in the running game."),
        "{first}"
    );
}

// No spec statement: P4-31, the acceptance criterion: a thread kept when it ends is reopened from
// the history with its messages, the hunks it staged that still wait and its checkpoint
#[test]
fn a_thread_reopened_from_history_shows_messages_hunks_and_checkpoint() {
    use egui_kittest::kittest::Queryable as _;
    let (root, mut harness, gate) = harness("history", "");
    let world = root.join("world.qc").to_string_lossy().into_owned();
    let ctx = harness.ctx.clone();
    shell(&mut harness)
        .start_agent("Give the world a local.", &ctx)
        .unwrap();
    gate.send(Reply::Calls {
        text: "Adding the local.".to_owned(),
        calls: vec![ToolCall {
            id: "c1".to_owned(),
            name: "edit.apply".to_owned(),
            arguments: json!({ "files": [{ "path": world,
                "text": "void() worldspawn = { local float x; x = 1; };\n" }] }),
        }],
    })
    .unwrap();
    gate.send(Reply::Finish {
        summary: "Added it.".to_owned(),
    })
    .unwrap();
    while shell(&mut harness).agent_report(0).is_none() {
        assert!(shell(&mut harness).wait_agent(0), "the model ended early");
    }
    let kept = history::load(&history::threads_dir(&root)).unwrap();
    assert_eq!(kept.len(), 1, "the thread was kept when it ended");
    assert_eq!(kept[0].title, "Give the world a local.");
    assert_eq!(kept[0].staged.len(), 1);
    let checkpoint = kept[0].checkpoint.unwrap();
    shell(&mut harness).agent_action(AgentAction::OpenHistory, &ctx);
    harness.run();
    harness.get_by_label("Give the world a local.").click();
    harness.run();
    harness.run();
    harness.get_by_label("Adding the local.");
    harness.get_by_label(&format!("{} {checkpoint}", Text::AgentCheckpoint.get()));
    harness.get_by_label(Text::AgentRestore.get());
    harness.get_by_label(Text::AgentAccept.get());
}

// No spec statement: P4-31: while the editor follows the agent, a file a call names is opened;
// a message queued while a turn runs is read on the next turn; a thread that ends is counted
// for the notification
#[test]
fn following_queueing_and_the_finished_count() {
    let (root, ctx, mut app, gates) = window("follow", 1);
    app.shell_mut().agent_mut().set_follow_for_test();
    app.shell_mut()
        .start_agent("Read the world.", &ctx)
        .unwrap();
    app.shell_mut()
        .agent_mut()
        .queue_for_test(0, "Also say hello.");
    let world = root.join("world.qc");
    gates[0]
        .send(Reply::Calls {
            text: String::new(),
            calls: vec![ToolCall {
                id: "c1".to_owned(),
                name: "file.read".to_owned(),
                arguments: json!({ "path": world }),
            }],
        })
        .unwrap();
    assert!(app.shell_mut().wait_agent(0));
    frame(&ctx, &mut app);
    let (open, _) = app.shell().caret().unwrap();
    assert!(open.ends_with("world.qc"), "{open:?}");
    gates[0]
        .send(Reply::Finish {
            summary: "done".to_owned(),
        })
        .unwrap();
    finish(&mut app, 0);
    let transcript = &app.shell().agent_report(0).unwrap().transcript;
    assert!(transcript.contains(&Message::User {
        text: "Also say hello.".to_owned()
    }));
    assert_eq!(app.shell_mut().agent_mut().take_finished(), 1);
}
