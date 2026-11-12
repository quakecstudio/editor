// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask evals`: the agent's evals (P4-15, QA-5). Each task in `evals/<name>/` is a
//! modding job on a starter template (`task.toml`), a scenario that says what success is
//! (`success.scenario`), or for a map task what the map must hold (`[[map]]` in `task.toml`,
//! P5-08), a rubric for the reviewer (`rubric.md`) and a pass threshold.
//!
//! - `--check` proves each scenario defines success: on the task's starting point (the
//!   template with the task's `setup` edits) it fails, and with the reference `solution` edits
//!   it passes. It needs no model and runs in CI.
//! - `--run` gives each task to the agent on a model provider, as many times as `runs`, then
//!   plays the scenario on the agent's work; a task passes when at least `threshold` runs do.
//!   Each run stops once its estimated cost reaches `--max-usd`. The results are appended to
//!   `--history` as one JSON line per run.
//!
//! Both write a Markdown report (`--report`).

use std::fs;
use std::path::{Path, PathBuf};

use qc_project::FsLoader;
use qs_agent::{Message, Model, Reply, RunConfig, ToolSpec};
use qs_commands::{Registry, Session};
use qs_map::MapFile;
use qs_models::{MemoryKeys, ProviderConfig, ProviderKind, ProviderModel, Secret, Usage};
use qs_templates::{Folder, Template};
use qs_workbench::{
    Config, HasPreview, HasWorkbench, OPEN_PROJECT, Preview, PreviewConfig, Workbench,
};
use serde::Deserialize;
use serde_json::json;

/// `cargo xtask evals`.
#[derive(clap::Parser, Debug)]
pub(crate) struct Args {
    /// Check that each task's scenario fails on its start and passes with its solution.
    #[arg(long, conflicts_with = "run")]
    check: bool,
    /// Run each task with the agent on a model provider.
    #[arg(long)]
    run: bool,
    /// Only the task with this name.
    #[arg(long)]
    task: Option<String>,
    /// id's QuakeC 1.01, `qcc/v101qc`; `$QS_TEST_ASSETS/codebase/id-1.01` by default.
    #[arg(long)]
    id1: Option<PathBuf>,
    /// The rerelease's QuakeC, `quakec`; `$QS_TEST_ASSETS/codebase/rerelease` by default.
    #[arg(long)]
    rerelease: Option<PathBuf>,
    /// The game folder scenarios play over; `$QS_TEST_ASSETS/librequake-lite` by default.
    #[arg(long)]
    game: Option<PathBuf>,
    /// The provider: anthropic, openai-compatible or ollama. The key is read from
    /// `QS_EVAL_API_KEY`.
    #[arg(long, default_value = "anthropic")]
    provider: String,
    /// The model; the provider's default when absent.
    #[arg(long)]
    model: Option<String>,
    /// The provider's address, when not its default.
    #[arg(long)]
    base_url: Option<String>,
    /// The most one run may cost, in US dollars, at the prices below.
    #[arg(long, default_value_t = 2.0)]
    max_usd: f64,
    /// US dollars per million tokens read; Claude Opus 5.5's list price by default.
    #[arg(long, default_value_t = 4.0)]
    input_usd_per_mtok: f64,
    /// US dollars per million tokens written; Claude Opus 5.5's list price by default.
    #[arg(long, default_value_t = 20.0)]
    output_usd_per_mtok: f64,
    /// Where the report goes.
    #[arg(long, default_value = "target/evals/report.md")]
    report: PathBuf,
    /// A JSON-lines file each run's result is appended to.
    #[arg(long)]
    history: Option<PathBuf>,
}

/// One edit of a file of the project: the text `find`, which must be there once, becomes
/// `replace`.
#[derive(Debug, Deserialize)]
struct Edit {
    file: String,
    find: String,
    replace: String,
}

/// What a map task's map must hold for it to succeed (P5-08): `count` entities of `classname`
/// in the room `room` (MAP-9) of the map `file`.
#[derive(Debug, Deserialize)]
struct MapExpect {
    file: String,
    classname: String,
    room: String,
    count: usize,
}

impl MapExpect {
    /// Whether the map holds it: the map open in `session` when it is this one, the agent's
    /// unsaved edits included, else the file in `dir`.
    fn holds(&self, session: &mut Session<Context>, dir: &Path) -> Result<bool, String> {
        let path = dir.join(&self.file);
        let same = |open: &Path| {
            fs::canonicalize(open)
                .ok()
                .zip(fs::canonicalize(&path).ok())
                .is_some_and(|(a, b)| a == b)
        };
        let open = session
            .context()
            .workbench
            .map()
            .filter(|document| same(document.path()))
            .map(|document| document.file().clone());
        let file = match open {
            Some(file) => file,
            None => {
                let bytes = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                MapFile::parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))?
            }
        };
        let rooms = file.rooms();
        let count = file
            .entities
            .iter()
            .filter(|entity| entity.classname() == Some(self.classname.as_bytes()))
            .filter(|entity| {
                entity
                    .room(&rooms)
                    .is_some_and(|room| room.name.eq_ignore_ascii_case(&self.room))
            })
            .count();
        Ok(count == self.count)
    }
}

/// What a task's success is.
enum Success<'t> {
    /// Its scenario passes on the agent's work (`success.scenario`).
    Scenario(String),
    /// Its map holds every one of these (P5-08).
    Map(&'t [MapExpect]),
}

/// A task, as `task.toml` gives it.
#[derive(Debug, Deserialize)]
struct Task {
    title: String,
    template: Template,
    prompt: String,
    turns: usize,
    runs: u32,
    threshold: u32,
    #[serde(default)]
    setup: Vec<Edit>,
    #[serde(default)]
    solution: Vec<Edit>,
    /// Files of the task's directory copied into the project, such as a map, by their path
    /// from the directory.
    #[serde(default)]
    files: Vec<String>,
    /// For a map task, what the map must hold; with none, `success.scenario` says.
    #[serde(default)]
    map: Vec<MapExpect>,
}

/// The name a task's scenario takes in the project, and the test filter that finds it.
const SCENARIO: &str = "eval-success";

/// What the agent works on in an eval: the workbench and the preview, as the window has them.
struct Context {
    workbench: Workbench<FsLoader>,
    preview: Preview,
}

impl HasWorkbench for Context {
    type Loader = FsLoader;

    fn workbench(&mut self) -> &mut Workbench<FsLoader> {
        &mut self.workbench
    }

    fn preview(&mut self) -> Option<&mut Preview> {
        Some(&mut self.preview)
    }
}

impl HasPreview for Context {
    fn workbench_and_preview(&mut self) -> (&mut Workbench<FsLoader>, &mut Preview) {
        (&mut self.workbench, &mut self.preview)
    }
}

/// Where the inputs are.
struct Inputs {
    templates: Folder,
    game: PathBuf,
}

impl Inputs {
    fn of(args: &Args) -> Self {
        let assets = std::env::var_os("QS_TEST_ASSETS").map(PathBuf::from);
        let under = |path: &str| assets.as_ref().map(|assets| assets.join(path));
        Self {
            templates: Folder::new(
                args.id1.clone().or_else(|| under("codebase/id-1.01")),
                args.rerelease
                    .clone()
                    .or_else(|| under("codebase/rerelease")),
            ),
            game: args
                .game
                .clone()
                .or_else(|| under("librequake-lite"))
                .unwrap_or_default(),
        }
    }
}

/// Runs the evals; true when every task met its bar.
pub(crate) fn main(root: &Path, args: &Args) -> Result<bool, String> {
    if !args.check && !args.run {
        return Err("say --check or --run".to_owned());
    }
    let inputs = Inputs::of(args);
    let mut names: Vec<String> = fs::read_dir(root.join("evals"))
        .map_err(|e| format!("evals/: {e}"))?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().join("task.toml").is_file())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| args.task.as_ref().is_none_or(|only| only == name))
        .collect();
    names.sort();
    if names.is_empty() {
        return Err("no task found".to_owned());
    }
    let work = root.join("target/evals");
    let mut report = String::from("# Agent evals\n\n");
    let mut all = true;
    if args.check {
        report.push_str(
            "| Task | Fails at its start | Passes with its solution |\n| --- | --- | --- |\n",
        );
    } else {
        report.push_str(
            "| Task | Passed runs | Threshold | Met | Turns | Tokens in | Tokens out | Cost (USD) |\n\
             | --- | --- | --- | --- | --- | --- | --- | --- |\n",
        );
    }
    for name in &names {
        let dir = root.join("evals").join(name);
        let task: Task = toml::from_str(
            &fs::read_to_string(dir.join("task.toml")).map_err(|e| format!("{name}: {e}"))?,
        )
        .map_err(|e| format!("{name}/task.toml: {e}"))?;
        let success = if task.map.is_empty() {
            let scenario = fs::read_to_string(dir.join("success.scenario"))
                .map_err(|e| format!("{name}/success.scenario: {e}"))?;
            Success::Scenario(scenario)
        } else {
            Success::Map(&task.map)
        };
        if !dir.join("rubric.md").is_file() {
            return Err(format!("{name}: no rubric.md"));
        }
        if args.check {
            let (fails, passes) = check(&work, name, &dir, &task, &success, &inputs)?;
            all &= fails && passes;
            let mark = |ok: bool| if ok { "yes" } else { "**no**" };
            report.push_str(&format!(
                "| {name} | {} | {} |\n",
                mark(fails),
                mark(passes)
            ));
            println!(
                "{}  {name}: fails at its start {fails}, passes with its solution {passes}",
                if fails && passes { "ok  " } else { "FAIL" }
            );
        } else {
            let row = run(&work, (name, &dir), &task, &success, &inputs, args)?;
            all &= row.met;
            report.push_str(&format!(
                "| {name} | {}/{} | {} | {} | {} | {} | {} | {:.2} |\n",
                row.passed,
                task.runs,
                task.threshold,
                if row.met { "yes" } else { "**no**" },
                row.turns,
                row.usage.input_tokens,
                row.usage.output_tokens,
                row.cost
            ));
            println!(
                "{}  {name}: {}/{} runs passed, {:.2} USD",
                if row.met { "ok  " } else { "FAIL" },
                row.passed,
                task.runs,
                row.cost
            );
        }
    }
    if let Some(folder) = args.report.parent() {
        fs::create_dir_all(root.join(folder)).map_err(|e| e.to_string())?;
    }
    fs::write(root.join(&args.report), &report).map_err(|e| e.to_string())?;
    println!("evals: report written to {}", args.report.display());
    Ok(all)
}

/// Makes task `name`'s project for run `n` from its template, with the task's files from
/// `from` and its setup edits; its directory.
fn project(
    work: &Path,
    (name, from): (&str, &Path),
    n: u32,
    task: &Task,
    inputs: &Inputs,
) -> Result<PathBuf, String> {
    let dir = work.join(format!("{name}-{n}")).join("evalmod");
    if let Some(parent) = dir.parent()
        && parent.exists()
    {
        fs::remove_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    qs_templates::create(task.template, &dir, &inputs.templates).map_err(|e| e.to_string())?;
    for file in &task.files {
        let to = dir.join(file);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        fs::copy(from.join(file), &to).map_err(|e| format!("{name}/{file}: {e}"))?;
    }
    apply(&dir, &task.setup).map_err(|e| format!("{name} setup: {e}"))?;
    Ok(dir)
}

/// Makes each edit of `edits` to the project in `dir`.
fn apply(dir: &Path, edits: &[Edit]) -> Result<(), String> {
    for edit in edits {
        let file = dir.join(&edit.file);
        let text = fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        if text.matches(&edit.find).count() != 1 {
            return Err(format!("{}: the text to find is not there once", edit.file));
        }
        fs::write(&file, text.replacen(&edit.find, &edit.replace, 1))
            .map_err(|e| format!("{}: {e}", file.display()))?;
    }
    Ok(())
}

/// A session over the project in `dir`, the game at `game`.
fn session(dir: &Path, game: &Path) -> Result<Session<Context>, String> {
    let mut registry = Registry::new();
    qs_workbench::register(&mut registry).map_err(|e| e.to_string())?;
    qs_workbench::register_preview(&mut registry).map_err(|e| e.to_string())?;
    let mut preview = PreviewConfig::from_env(std::env::current_exe().unwrap_or_default());
    preview.game = Some(game.to_path_buf());
    let mut session = Session::new(
        Context {
            workbench: Workbench::new(FsLoader, Config::default()),
            preview: Preview::new(preview),
        },
        registry,
    );
    session
        .execute(OPEN_PROJECT, json!({ "path": dir }))
        .map_err(|e| e.to_string())?;
    Ok(session)
}

/// Whether the task's scenario passes on the project `session` has open, the agent's
/// unsaved edits included: it is written into the project's tests and run alone.
fn succeeds(
    session: &mut Session<Context>,
    dir: &Path,
    success: &Success<'_>,
    game: &Path,
) -> Result<bool, String> {
    let scenario = match success {
        Success::Scenario(scenario) => scenario,
        Success::Map(expects) => {
            for expect in *expects {
                if !expect.holds(session, dir)? {
                    return Ok(false);
                }
            }
            return Ok(true);
        }
    };
    let tests = dir.join("tests");
    fs::create_dir_all(&tests).map_err(|e| e.to_string())?;
    fs::write(tests.join(format!("{SCENARIO}.scenario")), scenario).map_err(|e| e.to_string())?;
    let run = session
        .execute(
            qs_workbench::RUN_TESTS,
            json!({ "filter": SCENARIO, "game": game }),
        )
        .map_err(|e| e.to_string());
    let Ok(run) = run else {
        // A project that does not build has not succeeded.
        return Ok(false);
    };
    let run: qs_workbench::TestRun = serde_json::from_value(run).map_err(|e| e.to_string())?;
    let ours = run.tests.iter().find(|test| test.name.contains(SCENARIO));
    match ours {
        Some(test) if test.verdict == qs_workbench::Verdict::Skipped => Err(format!(
            "the scenario was skipped: {}",
            test.message.as_deref().unwrap_or_default()
        )),
        Some(test) => Ok(test.verdict == qs_workbench::Verdict::Passed),
        None => Ok(false),
    }
}

/// `--check` for one task: whether its scenario fails at its start and passes with its
/// solution.
fn check(
    work: &Path,
    name: &str,
    from: &Path,
    task: &Task,
    success: &Success<'_>,
    inputs: &Inputs,
) -> Result<(bool, bool), String> {
    if task.solution.is_empty() {
        return Err(format!("{name}: no reference solution"));
    }
    let dir = project(work, (name, from), 0, task, inputs)?;
    let mut start = session(&dir, &inputs.game)?;
    let fails = !succeeds(&mut start, &dir, success, &inputs.game)?;
    apply(&dir, &task.solution).map_err(|e| format!("{name} solution: {e}"))?;
    let mut solved = session(&dir, &inputs.game)?;
    let passes = succeeds(&mut solved, &dir, success, &inputs.game)?;
    Ok((fails, passes))
}

/// One task's results over its runs.
struct Row {
    passed: u32,
    met: bool,
    turns: usize,
    usage: Usage,
    cost: f64,
}

/// A provider's model that stops once its estimated cost reaches the cap.
struct Capped {
    model: ProviderModel<qs_models::Http>,
    prices: (f64, f64),
    max_usd: f64,
}

impl Capped {
    fn cost(&self) -> f64 {
        self.model.usage().cost(self.prices.0, self.prices.1)
    }
}

impl Model for Capped {
    fn reply(
        &mut self,
        system: &str,
        tools: &[ToolSpec],
        transcript: &[Message],
    ) -> Result<Reply, String> {
        if self.cost() >= self.max_usd {
            return Err(format!(
                "the run reached its cost cap of {:.2} USD",
                self.max_usd
            ));
        }
        self.model.reply(system, tools, transcript)
    }
}

/// `--run` for one task: each run on a fresh project, the scenario played on the agent's work.
fn run(
    work: &Path,
    (name, from): (&str, &Path),
    task: &Task,
    success: &Success<'_>,
    inputs: &Inputs,
    args: &Args,
) -> Result<Row, String> {
    let kind = match args.provider.as_str() {
        "anthropic" => ProviderKind::Anthropic,
        "openai-compatible" => ProviderKind::OpenAiCompatible,
        "ollama" => ProviderKind::Ollama,
        other => return Err(format!("no provider {other}")),
    };
    let mut config = match (&args.model, kind) {
        (Some(model), _) => ProviderConfig::new(kind, model),
        (None, ProviderKind::Anthropic) => ProviderConfig::anthropic(),
        (None, _) => return Err("name the model with --model".to_owned()),
    };
    config.base_url.clone_from(&args.base_url);
    let keys = MemoryKeys::new();
    if let Ok(key) = std::env::var("QS_EVAL_API_KEY") {
        qs_models::KeyStore::set(&keys, &config.key_name(), &Secret::new(key))
            .map_err(|e| e.to_string())?;
    }
    let mut row = Row {
        passed: 0,
        met: false,
        turns: 0,
        usage: Usage::default(),
        cost: 0.0,
    };
    for n in 1..=task.runs {
        let dir = project(work, (name, from), n, task, inputs)?;
        let mut session = session(&dir, &inputs.game)?;
        let mut model = Capped {
            model: ProviderModel::connect(config.clone(), &keys).map_err(|e| e.to_string())?,
            prices: (args.input_usd_per_mtok, args.output_usd_per_mtok),
            max_usd: args.max_usd,
        };
        let run_config = RunConfig {
            turns: task.turns,
            test_params: json!({ "game": inputs.game }),
            ..RunConfig::new(&dir)
        };
        let report = qs_agent::run(&mut session, &mut model, &task.prompt, &run_config);
        let passed = succeeds(&mut session, &dir, success, &inputs.game)?;
        let usage = model.model.usage();
        let cost = model.cost();
        let turns = report
            .as_ref()
            .map(|r| {
                r.transcript
                    .iter()
                    .filter(|m| matches!(m, Message::Assistant { .. }))
                    .count()
            })
            .unwrap_or_default();
        if passed {
            row.passed = row.passed.saturating_add(1);
        }
        row.turns = row.turns.saturating_add(turns);
        row.usage.input_tokens = row.usage.input_tokens.saturating_add(usage.input_tokens);
        row.usage.output_tokens = row.usage.output_tokens.saturating_add(usage.output_tokens);
        row.cost += cost;
        if let Some(history) = &args.history {
            let line = json!({
                "task": name,
                "title": task.title,
                "run": n,
                "model": config.model,
                "passed": passed,
                "turns": turns,
                "usage": usage,
                "cost_usd": cost,
                "error": report.as_ref().err().map(ToString::to_string),
            });
            append(history, &line.to_string())?;
        }
    }
    row.met = row.passed >= task.threshold;
    Ok(row)
}

/// Appends `line` to the file `path`.
fn append(path: &Path, line: &str) -> Result<(), String> {
    use std::io::Write as _;
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    writeln!(file, "{line}").map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use std::path::Path;

    use qs_sim::scenario::Scenario;

    use super::{Edit, Task, apply};

    // No spec statement: P4-15, P5-08, every task in evals/ reads: a template, a prompt, a
    // threshold no greater than its runs, a reference solution, a scenario that reads or a map
    // to check, the files it names and a rubric
    #[test]
    fn every_task_is_whole() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../evals");
        let mut count = 0;
        for entry in std::fs::read_dir(&root).unwrap() {
            let dir = entry.unwrap().path();
            if !dir.join("task.toml").is_file() {
                continue;
            }
            count += 1;
            let task: Task =
                toml::from_str(&std::fs::read_to_string(dir.join("task.toml")).unwrap()).unwrap();
            assert!(
                !task.title.is_empty() && !task.prompt.is_empty(),
                "{}",
                dir.display()
            );
            assert!(task.threshold >= 1 && task.threshold <= task.runs && task.turns > 0);
            assert!(!task.solution.is_empty(), "{}: no solution", dir.display());
            if task.map.is_empty() {
                let scenario = std::fs::read_to_string(dir.join("success.scenario")).unwrap();
                Scenario::parse(&scenario).unwrap();
            }
            for file in &task.files {
                assert!(dir.join(file).is_file(), "{}: no {file}", dir.display());
            }
            assert!(dir.join("rubric.md").is_file());
        }
        assert!(count >= 5, "the tasks the issue names and more");
    }

    // No spec statement: P4-15, an edit applies only where its text is found exactly once
    #[test]
    fn an_edit_needs_its_text_once() {
        let dir = std::env::temp_dir().join(format!("qs-evals-apply-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.qc"), "x = 1;\nx = 1;\ny = 2;\n").unwrap();
        let edit = |find: &str| Edit {
            file: "a.qc".to_owned(),
            find: find.to_owned(),
            replace: "z = 3;".to_owned(),
        };
        assert!(apply(&dir, &[edit("x = 1;")]).is_err(), "twice");
        assert!(apply(&dir, &[edit("w = 0;")]).is_err(), "not at all");
        apply(&dir, &[edit("y = 2;")]).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("a.qc")).unwrap(),
            "x = 1;\nx = 1;\nz = 3;\n"
        );
    }
}
