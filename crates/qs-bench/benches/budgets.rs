// SPDX-License-Identifier: GPL-3.0-or-later

//! One benchmark per budget in `docs/perf-budgets.md` (PRIN-5, ENG-13).
//!
//! `cargo xtask bench` runs this file and reads criterion's results from the
//! `budgets/<name>/new/` directories it writes. The benchmark names here must
//! match the budget table; an xtask test checks that they do.
//!
//! A budget marked `pending` in the table has a placeholder body here until
//! the issue named in its row measures the real operation and marks the row
//! `active`. Placeholders use a short measurement so CI stays quick.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "bench code: a setup that fails is a failed benchmark"
)]

use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::Duration;

use eframe::egui;
use quakec_studio::{App, bench};
use serde_json::json;

use criterion::measurement::WallTime;
use criterion::{BenchmarkGroup, Criterion, criterion_group, criterion_main};
use qc_codegen::compile_module;
use qc_sema::{Db, FileId, ModuleInput};

/// The criterion group every budget benchmark belongs to.
const GROUP: &str = "budgets";

/// Runs a placeholder for a budget whose measurement a later issue fills in.
fn placeholder(group: &mut BenchmarkGroup<'_, WallTime>, name: &str) {
    group.bench_function(name, |bencher| bencher.iter(|| black_box(name.len())));
}

/// The window's size in the headless frames, the editor_bench example's.
const WINDOW: [f32; 2] = [1440.0, 900.0];

/// One frame of `app` with `events`, headless: the UI laid out and painted, with no renderer
/// to take its output, as the desktop app's tests run it.
fn frame(ctx: &egui::Context, app: &mut App, events: Vec<egui::Event>) {
    frame_at(ctx, app, events, None);
}

/// [`frame`] at the input time `time` in seconds, which the editor reads its pauses by.
fn frame_at(ctx: &egui::Context, app: &mut App, events: Vec<egui::Event>, time: Option<f64>) {
    let input = egui::RawInput {
        time,
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::Vec2::from(WINDOW),
        )),
        events,
        ..egui::RawInput::default()
    };
    let mut output = ctx.run_ui(input, |ui| app.frame(ui));
    output.textures_delta.clear();
    black_box(output);
}

/// The app with the benchmark project open, the middle file in the editor with the caret at
/// its middle line, and the editor holding the keyboard: what the modder sees after opening.
fn opened(ctx: &egui::Context, dir: &Path, file: &Path) -> App {
    let mut app = App::new(ctx, None).expect("the app starts");
    let shell = app.shell_mut();
    shell.open_project(dir).expect("the project opens");
    let offset = bench::caret_offset(&fs::read_to_string(file).expect("the file reads"));
    shell
        .execute("file.open", json!({ "path": file, "offset": offset }))
        .expect("the file opens");
    frame(ctx, &mut app, Vec::new());
    app.shell().focus_editor(ctx);
    frame(ctx, &mut app, Vec::new());
    app
}

/// The benchmark project, written once into the target directory.
fn project() -> (PathBuf, PathBuf) {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("editor-bench-project");
    let file = bench::write_project(&dir).expect("the benchmark project writes");
    (dir, file)
}

/// Open a 50,000-line project and type the first character (P2-18): from creating the app to
/// the end of the frame that shows that character, headless, so without the renderer's time.
fn open_project_and_type(group: &mut BenchmarkGroup<'_, WallTime>) {
    let (dir, file) = project();
    group.bench_function("open_project_and_type", |bencher| {
        bencher.iter(|| {
            let ctx = egui::Context::default();
            let mut app = opened(&ctx, &dir, &file);
            frame(&ctx, &mut app, vec![egui::Event::Text("s".to_owned())]);
            black_box(app)
        });
    });
}

/// One keystroke in the code editor to the end of the next frame (P2-18), the project's text
/// highlighted, headless: each iteration types the next character of `bench::TYPED`. A run of
/// keystrokes never pauses long enough for the check after typing, which has no budget here.
fn keystroke(group: &mut BenchmarkGroup<'_, WallTime>) {
    let (dir, file) = project();
    let ctx = egui::Context::default();
    let mut app = opened(&ctx, &dir, &file);
    let mut typed = bench::TYPED.chars().cycle();
    group.bench_function("keystroke", |bencher| {
        bencher.iter(|| {
            let c = typed.next().expect("a cycle never ends");
            frame(&ctx, &mut app, vec![egui::Event::Text(c.to_string())]);
        });
    });
}

/// The variable naming id 1.01's QuakeC directory, which `cargo xtask bench` sets from the
/// asset cache.
const ID1_DIR: &str = "QS_BENCH_ID1";
/// The function the edit changes, and its first local, in `weapons.qc`.
const EDITED_FILE: &str = "weapons.qc";
const EDITED_LINE: &str = "void() W_FireAxe =\n{\n\tlocal\tvector\tsource;";
const EDITED_TO: &str = "void() W_FireAxe =\n{\n\tlocal\tvector\tsource, bench_edit;";

/// Edit one function in id 1.01's QuakeC and rebuild progs.dat (P2-11, PRIN-5).
///
/// id 1.01 is opened through its `progs.src` into one database and compiled once. Each
/// iteration then changes `W_FireAxe` in `weapons.qc`, adding or removing a local in turn, and
/// compiles the module again through the same database, which re-reads only the edited file:
/// the editor's rebuild after a save. The progs is written to bytes, as it would be to disk.
fn id1_recompile(group: &mut BenchmarkGroup<'_, WallTime>) {
    let dir = PathBuf::from(std::env::var_os(ID1_DIR).unwrap_or_else(|| {
        panic!("{ID1_DIR} is not set: run the benchmarks through cargo xtask bench")
    }));
    let project = qc_project::open(&dir).expect("id 1.01's progs.src opens");
    let module = project.modules.first().expect("id 1.01 has one module");
    let mut db = Db::new();
    let mut edited = None;
    for (index, file) in module.graph.files.iter().enumerate() {
        let text = fs::read(&file.path).expect("an id 1.01 file reads");
        if file
            .path
            .file_name()
            .is_some_and(|name| name == EDITED_FILE)
        {
            edited = Some((
                FileId::new(index),
                String::from_utf8_lossy(&text).into_owned(),
            ));
        }
        db.set_file(FileId::new(index), file.path.clone(), text);
    }
    let (file, original) = edited.expect("id 1.01 has weapons.qc");
    assert!(
        original.contains(EDITED_LINE),
        "W_FireAxe starts as expected"
    );
    let variants = [original.replacen(EDITED_LINE, EDITED_TO, 1), original];
    let input = ModuleInput::from_module(module);
    let name = |id: FileId| format!("{}.qc", id.index());
    compile_module(&mut db, &input, name).expect("id 1.01 compiles");
    let mut turns = variants.iter().cycle();
    group.bench_function("id1_recompile", |bencher| {
        bencher.iter(|| {
            let text = turns.next().expect("a cycle never ends");
            db.set_file_text(file, text.as_bytes().to_vec());
            let compiled =
                compile_module(&mut db, &input, name).expect("the edited id 1.01 compiles");
            black_box(qc_progs::write(&compiled.progs).expect("the progs writes"))
        });
    });
}

/// Save a QuakeC file to the change visible in a windowed engine (P3-12).
fn save_to_game(group: &mut BenchmarkGroup<'_, WallTime>) {
    placeholder(group, "save_to_game");
}

/// The frame that checks the project again once typing pauses (P2-166): one character typed,
/// then the frame after the editor's settle time, which sends the text to the project and
/// checks it, timed alone. The input time is set, not waited for.
fn check_after_typing(group: &mut BenchmarkGroup<'_, WallTime>) {
    let (dir, file) = project();
    let ctx = egui::Context::default();
    let mut app = opened(&ctx, &dir, &file);
    let mut time = 0.0_f64;
    let mut typed = bench::TYPED.chars().cycle();
    group.bench_function("check_after_typing", |bencher| {
        bencher.iter_custom(|iterations| {
            let mut spent = Duration::ZERO;
            for _ in 0..iterations {
                let c = typed.next().expect("a cycle never ends");
                time += 1.0;
                let text = vec![egui::Event::Text(c.to_string())];
                frame_at(&ctx, &mut app, text, Some(time));
                let start = std::time::Instant::now();
                frame_at(&ctx, &mut app, Vec::new(), Some(time + 0.4));
                spent += start.elapsed();
            }
            spent
        });
    });
}

fn budgets(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group(GROUP);
    group
        .sample_size(10)
        .warm_up_time(Duration::from_millis(100))
        .measurement_time(Duration::from_millis(500));
    open_project_and_type(&mut group);
    keystroke(&mut group);
    check_after_typing(&mut group);
    id1_recompile(&mut group);
    save_to_game(&mut group);
    group.finish();
}

criterion_group!(benches, budgets);
criterion_main!(benches);
