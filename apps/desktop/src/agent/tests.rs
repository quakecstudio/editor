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
use qs_agent::{Message, Model, Outcome, Reply, ToolCall, ToolSpec};
use qs_models::{MemoryKeys, ProviderConfig, ProviderKind};
use serde_json::json;

use super::{AgentAction, AgentPanel};
use crate::App;

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
    let root = std::env::temp_dir().join(format!("qs-agent-panel-{name}-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("progs.src"), "progs.dat\nworld.qc\n").unwrap();
    std::fs::write(root.join("world.qc"), "void() worldspawn = {};\n").unwrap();
    let (gates, models): (Vec<_>, Vec<_>) = (0..threads).map(|_| mpsc::channel()).unzip();
    let models = Arc::new(Mutex::new(models));
    let ctx = egui::Context::default();
    let mut app = App::checked_on_the_frame(&ctx, None).unwrap();
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
    (root, ctx, app, gates)
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
