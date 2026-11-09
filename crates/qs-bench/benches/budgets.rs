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
use criterion::{BenchmarkGroup, Criterion, SamplingMode, criterion_group, criterion_main};
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
/// keystrokes never pauses long enough for the check after typing, which `check_after_typing`
/// times on its own.
///
/// Each sample is one keystroke, not an average (`docs/perf-budgets.md`, "Percentiles and
/// samples"): the benchmark takes [`KEYSTROKES`] samples in criterion's flat sampling mode over a
/// measurement time far shorter than one keystroke, so criterion gives every sample one
/// iteration (`iteration_counts`: `((time_per_sample / met).ceil() as u64).max(1)`), and the p99
/// is over that many single keystrokes. Should criterion still ask for more than one, each is
/// timed alone and the sample takes its slowest keystroke's time.
fn keystroke(group: &mut BenchmarkGroup<'_, WallTime>) {
    let (dir, file) = project();
    let ctx = egui::Context::default();
    let mut app = opened(&ctx, &dir, &file);
    let mut typed = bench::TYPED.chars().cycle();
    group
        .sampling_mode(SamplingMode::Flat)
        .sample_size(KEYSTROKES)
        .measurement_time(Duration::from_millis(1));
    group.bench_function("keystroke", |bencher| {
        bencher.iter_custom(|iterations| {
            let mut slowest = Duration::ZERO;
            for _ in 0..iterations {
                let c = typed.next().expect("a cycle never ends");
                let start = std::time::Instant::now();
                frame(&ctx, &mut app, vec![egui::Event::Text(c.to_string())]);
                slowest = slowest.max(start.elapsed());
            }
            slowest.saturating_mul(u32::try_from(iterations).unwrap_or(u32::MAX))
        });
    });
    group
        .sampling_mode(SamplingMode::Auto)
        .sample_size(SAMPLES)
        .measurement_time(MEASUREMENT);
}

/// The single keystrokes `keystroke` times: enough for a p99 over at least 100 (P3-36).
const KEYSTROKES: usize = 200;

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

/// The variable naming a game root holding `id1` (a Steam or GOG install, or LibreQuake's)
/// that `save_to_game` plays in the windowed preview host; with none, as on the CI runners,
/// which have no display and no game, the benchmark is a placeholder.
const GAME_DIR: &str = "QS_BENCH_GAME";
/// The variable naming the preview host to play in, which `cargo xtask bench` builds and sets
/// when [`GAME_DIR`] is set.
const HOST: &str = "QS_PREVIEW_HOST";
/// The map `save_to_game` plays.
const MAP: &str = "e1m1";
/// The size of the screenshot that waits for the next frame drawn: as small as the host takes.
const SHOT: (u32, u32) = (8, 5);

/// Save a QuakeC file to the change visible in a windowed engine (P3-12, ENG-13).
///
/// A copy of id 1.01's QuakeC is the open project, playing [`MAP`] in the windowed preview host
/// on the game [`GAME_DIR`] names. Each iteration makes the edit `id1_recompile` makes to
/// `W_FireAxe`, or takes it back, saves the file, and times `preview.reload`, which builds the
/// project and reloads its progs in the running game (BRIDGE-4.8), and a `screenshot`, which the
/// host answers once it has drawn the next frame (BRIDGE-4.9): compile, reload and the next
/// rendered frame, from the save.
fn save_to_game(group: &mut BenchmarkGroup<'_, WallTime>) {
    let Some(game) = std::env::var_os(GAME_DIR).map(PathBuf::from) else {
        // A reference machine holds the budget (`docs/perf-budgets.md`), so it measures it.
        let host = std::env::var("QS_BENCH_HOST").unwrap_or_default();
        assert!(
            !host.starts_with("ref-"),
            "{host} is a reference machine: set {GAME_DIR} to a game root for save_to_game"
        );
        placeholder(group, "save_to_game");
        return;
    };
    let source = PathBuf::from(std::env::var_os(ID1_DIR).unwrap_or_else(|| {
        panic!("{ID1_DIR} is not set: run the benchmarks through cargo xtask bench")
    }));
    let host = std::env::var_os(HOST).map_or_else(
        || panic!("{HOST} is not set: cargo xtask bench sets it with {GAME_DIR}"),
        PathBuf::from,
    );
    let project = std::env::temp_dir().join(format!("qs-bench-save-{}", std::process::id()));
    if project.exists() {
        fs::remove_dir_all(&project).expect("the old copy is removed");
    }
    fs::create_dir_all(&project).expect("the project directory is made");
    for entry in fs::read_dir(&source).expect("id 1.01's directory reads") {
        let path = entry.expect("an entry reads").path();
        if path.is_file() {
            let name = path.file_name().expect("a file has a name");
            fs::copy(&path, project.join(name)).expect("an id 1.01 file copies");
        }
    }
    let edited = project.join(EDITED_FILE);
    let original = fs::read_to_string(&edited).expect("weapons.qc reads");
    assert!(
        original.contains(EDITED_LINE),
        "W_FireAxe starts as expected"
    );
    let variants = [original.replacen(EDITED_LINE, EDITED_TO, 1), original];
    let mut workbench =
        qs_workbench::Workbench::new(qs_workbench::FsLoader, qs_workbench::Config::default());
    workbench.open_project(&project).expect("the copy opens");
    let mut preview = qs_workbench::Preview::new(qs_workbench::PreviewConfig {
        host: Some(host),
        app: PathBuf::new(),
        game: Some(game),
        extra: vec!["--nosound".to_owned()],
    });
    preview
        .play(&mut workbench, Some(MAP.to_owned()))
        .expect("the game plays in the windowed host");
    let mut turns = variants.iter().cycle();
    group.bench_function("save_to_game", |bencher| {
        bencher.iter_custom(|iterations| {
            let mut spent = Duration::ZERO;
            for _ in 0..iterations {
                let text = turns.next().expect("a cycle never ends");
                workbench
                    .update(&edited, text.as_bytes().to_vec())
                    .expect("the edit is taken");
                workbench.save(&edited).expect("the file saves");
                let start = std::time::Instant::now();
                preview
                    .reload(&mut workbench)
                    .expect("the game reloads")
                    .expect("a game runs");
                preview
                    .client()
                    .expect("a game runs")
                    .screenshot(Some(SHOT))
                    .expect("the next frame is drawn");
                spent = spent.saturating_add(start.elapsed());
            }
            spent
        });
    });
    preview.stop();
}

/// The variable naming the rerelease's game root, holding `id1` and `mg1`, whose hub the
/// `*_mg1` benchmarks play; with none they are placeholders.
const RERELEASE_DIR: &str = "QS_BENCH_RERELEASE";

/// One route of the game benchmarks (P3-29, ENG-17): a map played in the preview host's
/// `--timerefresh` mode, each measurement its own benchmark.
struct Route {
    /// The variable naming the game root.
    root: &'static str,
    /// The mod directory, if any.
    game: Option<&'static str>,
    /// The map.
    map: &'static str,
    /// The eye `timerefresh` turns at: in the map's first room, the player's height.
    eye: [f32; 3],
    /// Each benchmark's name: the frame, the server frame, its QuakeC and the map load.
    names: [&'static str; 4],
}

/// The routes: id1's e1m1 and the rerelease's mg1 hub, from their starts.
const ROUTES: [Route; 2] = [
    Route {
        root: GAME_DIR,
        game: None,
        map: "e1m1",
        eye: [480.0, -352.0, 110.0],
        names: [
            "frame_e1m1",
            "server_frame_e1m1",
            "quakec_e1m1",
            "map_load_e1m1",
        ],
    },
    Route {
        root: RERELEASE_DIR,
        game: Some("mg1"),
        map: "hub",
        eye: [0.0, -1920.0, 46.0],
        names: [
            "frame_mg1",
            "server_frame_mg1",
            "quakec_mg1",
            "map_load_mg1",
        ],
    },
];

/// What the preview host's `--timerefresh` measured, in seconds.
#[derive(serde::Deserialize)]
struct Timerefresh {
    load: f64,
    server_frame: Vec<f64>,
    quakec: Vec<f64>,
    frame: Vec<f64>,
}

/// `values`, measured by the preview host, as the samples of benchmark `name`: one sample a
/// value, each iteration of it the value, so the row's statistic is the values' own (P3-29).
fn recorded(group: &mut BenchmarkGroup<'_, WallTime>, name: &str, values: &[f64]) {
    assert!(!values.is_empty(), "the preview host measured no {name}");
    group.sample_size(values.len().max(10));
    let mut next = values.iter().cycle();
    group.bench_function(name, |bencher| {
        bencher.iter_custom(|iterations| {
            let value = *next.next().expect("a cycle of values never ends");
            // Each iteration takes exactly the value, so criterion's per-iteration time is it.
            // Replaying takes no time, so criterion's warm-up asks for many iterations: the
            // product is a double, not a Duration's integer nanoseconds.
            #[expect(
                clippy::cast_precision_loss,
                reason = "bench code: an iteration count past 2^53 rounds, far below a sample's resolution"
            )]
            let iterations = iterations as f64;
            Duration::from_secs_f64(value * iterations)
        });
    });
    group.sample_size(SAMPLES);
}

/// The frame time, server-frame time, QuakeC time per frame and map load of each route
/// (P3-29, ENG-17): the release preview host run once with `--timerefresh`, which loads the
/// map, times a second of server frames and their QuakeC, then draws id's `timerefresh` 16
/// times offscreen, each pass finished on the GPU and its mean frame one sample. Without the
/// route's game root its benchmarks are placeholders.
fn routes(group: &mut BenchmarkGroup<'_, WallTime>) {
    for route in &ROUTES {
        let Some(root) = std::env::var_os(route.root).map(PathBuf::from) else {
            let host = std::env::var("QS_BENCH_HOST").unwrap_or_default();
            assert!(
                !host.starts_with("ref-"),
                "{host} is a reference machine: set {} for {}",
                route.root,
                route.map
            );
            for name in route.names {
                placeholder(group, name);
            }
            continue;
        };
        let host = std::env::var_os(HOST).map_or_else(
            || {
                panic!(
                    "{HOST} is not set: cargo xtask bench sets it with {}",
                    route.root
                )
            },
            PathBuf::from,
        );
        let mut command = std::process::Command::new(host);
        command.arg("--basedir").arg(root);
        if let Some(game) = route.game {
            command.args(["--game", game]);
        }
        command.arg(route.map).arg("--timerefresh");
        command.args(route.eye.map(|n| n.to_string()));
        let output = command.output().expect("the preview host runs");
        assert!(
            output.status.success(),
            "timerefresh on {} failed: {}",
            route.map,
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).expect("the report is UTF-8");
        let json = stdout.lines().nth(1).expect("the report has its JSON line");
        let measured: Timerefresh = serde_json::from_str(json).expect("the JSON line reads");
        let [frame, server, quakec, load] = route.names;
        recorded(group, frame, &measured.frame);
        recorded(group, server, &measured.server_frame);
        recorded(group, quakec, &measured.quakec);
        recorded(group, load, &[measured.load]);
    }
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
                spent = spent.saturating_add(start.elapsed());
            }
            spent
        });
    });
}

/// The samples of each benchmark but those that set their own.
const SAMPLES: usize = 10;
/// The time criterion measures each benchmark for, but those that set their own.
const MEASUREMENT: Duration = Duration::from_millis(500);

fn budgets(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group(GROUP);
    group
        .sample_size(SAMPLES)
        .warm_up_time(Duration::from_millis(100))
        .measurement_time(MEASUREMENT);
    open_project_and_type(&mut group);
    keystroke(&mut group);
    check_after_typing(&mut group);
    id1_recompile(&mut group);
    save_to_game(&mut group);
    routes(&mut group);
    group.finish();
}

criterion_group!(benches, budgets);
criterion_main!(benches);
