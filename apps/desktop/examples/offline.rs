// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo run -p quakec-studio --example offline -- <project>`: the desktop app's session for
//! `cargo xtask offline` (P4-16, PRIN-9), which runs it with the network denied and traced.
//!
//! It opens the project in the app, headless through `egui_kittest`, with no model provider
//! set up, as on a first start. It then does what a modder does on a first afternoon: it draws
//! frames, opens each source file, builds, checks, runs the tests, searches, opens the command
//! palette, and asks the agent for a thread, which cannot start with no provider. It fails if
//! any of that fails, and the trace shows whether any of it touched the network.

#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "a check run by hand and in CI reports to the person who ran it"
)]

use std::path::PathBuf;
use std::process::ExitCode;

use eframe::egui::{Key, Modifiers};
use egui_kittest::Harness;
use quakec_studio::App;
use serde_json::{Value, json};

/// Opens a project in the app with no provider and works in it.
#[derive(clap::Parser, Debug)]
struct Args {
    /// The project directory.
    project: PathBuf,
}

fn main() -> ExitCode {
    let Args { project } = <Args as clap::Parser>::parse();
    match session(&project) {
        Ok(steps) => {
            println!("offline: the app's session ran {steps} steps with no provider");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("offline: {error}");
            ExitCode::FAILURE
        }
    }
}

/// The session; how many steps it ran.
fn session(project: &std::path::Path) -> Result<usize, String> {
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
    let mut app = App::checked_on_the_frame(&harness.ctx, None).map_err(|e| e.to_string())?;
    let shell = app.shell_mut();
    shell
        .open_project(project)
        .map_err(|e| format!("{}: {e}", project.display()))?;
    *harness.state_mut() = Some(app);
    harness.run_steps(10);
    let mut steps = 1_usize;
    let mut run = |harness: &mut Harness<'_, Option<App>>, id: &str, params: Value| {
        let shell = harness
            .state_mut()
            .as_mut()
            .map(App::shell_mut)
            .ok_or("the app is gone")?;
        let output = shell
            .execute(id, params)
            .map_err(|error| format!("{id}: {error}"))?;
        harness.run_steps(3);
        steps = steps.saturating_add(1);
        Ok::<Value, String>(output)
    };
    let files = run(&mut harness, "project.files", json!({}))?;
    for file in files.as_array().into_iter().flatten() {
        run(&mut harness, "file.open", json!({ "path": file }))?;
    }
    run(&mut harness, "project.build", json!({}))?;
    run(&mut harness, "project.check", json!({}))?;
    run(&mut harness, "project.test", json!({}))?;
    run(&mut harness, "project.search", json!({ "text": "self" }))?;
    run(&mut harness, "project.symbols", json!({}))?;
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::P);
    harness.run_steps(3);
    harness.key_press(Key::Escape);
    harness.run_steps(3);
    let ctx = harness.ctx.clone();
    let shell = harness
        .state_mut()
        .as_mut()
        .map(App::shell_mut)
        .ok_or("the app is gone")?;
    if shell.start_agent("Explain the project.", &ctx).is_ok() {
        return Err("a thread started with no provider set up".to_owned());
    }
    harness.run_steps(10);
    Ok(steps.saturating_add(2))
}
