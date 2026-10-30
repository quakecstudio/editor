// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo run -p quakec-studio --example problems_corpus -- <dir>`: P2-19's acceptance check,
//! that every diagnostic of a codebase is listed in the Problems tab and clickable.
//!
//! It opens the codebase in `<dir>` in the app, headless through `egui_kittest`, under the
//! FTEQCC profile the language server's corpus check uses (`cargo xtask lsp-corpus`), once
//! targeting the rerelease and once id's engine, whose missing builtins add warnings. For each
//! problem the workbench has, it finds the problem's row in the Problems tab by its text, clicks
//! it, and checks that the editor then shows the problem's file with the caret at the start of
//! its span. CI runs it on the rerelease in the Test Assets job.

#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "a check run by hand and in CI reports to the person who ran it"
)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use qs_editor_view::Position;
use qs_workbench::{Config, Dialect, Problem, Target};
use quakec_studio::App;
use quakec_studio::project::problem_text;
use serde_json::json;

/// The height of a Problems row, with room to spare, in points.
const ROW: f32 = 32.0;

fn main() -> ExitCode {
    let Some(dir) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: cargo run -p quakec-studio --example problems_corpus -- <dir>");
        return ExitCode::FAILURE;
    };
    let mut ok = true;
    for target in [Target::Rerelease, Target::Quake] {
        match check(&dir, target) {
            Ok(count) => println!(
                "problems-corpus: {} {target:?}: all {count} problems listed and clickable",
                dir.display()
            ),
            Err(error) => {
                eprintln!(
                    "problems-corpus: FAIL {} {target:?}: {error}",
                    dir.display()
                );
                ok = false;
            }
        }
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Opens `dir` targeting `target` and clicks every problem's row; the number of problems.
fn check(dir: &Path, target: Target) -> Result<usize, String> {
    let config = Config {
        dialect: Some(Dialect::Fteqcc),
        target: Some(target),
        autoproto: false,
    };
    let mut harness = Harness::builder().build_ui_state(
        |ui, app: &mut Option<App>| {
            if let Some(app) = app {
                app.frame(ui);
            }
        },
        None,
    );
    let mut app = App::with_analysis(&harness.ctx, None, config).map_err(|e| e.to_string())?;
    app.shell_mut()
        .open_project(dir)
        .map_err(|e| e.to_string())?;
    let problems: Vec<Problem> = app.shell().workbench().problems().to_vec();
    if problems.is_empty() {
        return Err("no problems, so nothing was checked".to_owned());
    }
    // Every row is drawn: the Problems tab is as tall as its rows, and the window taller.
    let rows = f32::from(u16::try_from(problems.len()).map_err(|e| e.to_string())?);
    let panel = rows.mul_add(ROW, 4.0 * ROW);
    app.shell_mut()
        .execute(
            "view.resize_panel",
            json!({ "panel": "bottom", "size": panel }),
        )
        .map_err(|e| e.to_string())?;
    harness.set_size(eframe::egui::Vec2::new(1440.0, panel + 600.0));
    *harness.state_mut() = Some(app);
    harness.run();
    let root = dir.to_path_buf();
    for problem in &problems {
        let label = problem_text(problem, Some(&root));
        harness
            .query_all_by_label(&label)
            .next()
            .ok_or_else(|| format!("not listed: {label}"))?
            .click();
        harness.run();
        let shell = harness.state().as_ref().ok_or("the app is gone")?.shell();
        let open = shell
            .workbench()
            .open()
            .ok_or_else(|| format!("nothing opened: {label}"))?;
        let expected = caret(open.text(), problem.diagnostic.span.start);
        match shell.caret() {
            Some((path, at))
                if path == problem.path
                    && open.offset() == problem.diagnostic.span.start
                    && at == expected => {}
            other => return Err(format!("{label}: opened {other:?}, not {expected:?}")),
        }
    }
    Ok(problems.len())
}

/// The editor's position of byte `offset` of `text`: its line, and its byte column in the line
/// as the editor holds it, with a CR before an LF dropped.
fn caret(text: &[u8], offset: usize) -> Position {
    let before = text.get(..offset).unwrap_or(text);
    let line = before.iter().filter(|&&b| b == b'\n').count();
    let start = before
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |newline| newline.saturating_add(1));
    Position::new(line, offset.saturating_sub(start))
}
