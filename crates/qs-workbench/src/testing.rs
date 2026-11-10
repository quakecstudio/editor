// SPDX-License-Identifier: GPL-3.0-or-later

//! `project.test` (P4-26, QA-13): a project's tests, found and run as `docs/spec/qc-tests.md`
//! says. Unit tests are the program's `test_` functions, each called alone in the VM over the
//! mock engine as `vm.run` calls one (QCT-2); scenario tests are the `.scenario` files under
//! `tests/`, played in `qs-sim` as `scenario.run` plays one (QCT-3). Implements QCT-1 to QCT-4.

use std::path::{Path, PathBuf};

use qc_progs::Progs;
use qc_project::Loader;
use qs_commands::{Command, Outcome, RegisterError, Registry};
use qs_engine::{Flavour, prepare_run_root};
use qs_sim::scenario::{Report, Scenario};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::preview::scenario::{engine_words, play_on_game};
use crate::preview::{game_base, mod_folder};
use crate::vm_run::{VmRunParams, run};
use crate::{HasWorkbench, ModuleBuilt, Workbench, WorkbenchError};

/// The id of the command that runs a project's tests.
pub const RUN_TESTS: &str = "project.test";

/// The prefix of a unit test's name (QCT-1.1).
pub const UNIT_PREFIX: &str = "test_";

/// The directory, in the project, scenario tests are found under (QCT-1.2).
pub const SCENARIO_DIR: &str = "tests";

/// The parameters of `project.test`.
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunTestsParams {
    /// Only the tests whose name contains this (QCT-1.3).
    #[serde(default)]
    pub filter: Option<String>,
    /// The game folder scenario tests play over; the first install found when absent
    /// (QCT-3.1).
    #[serde(default)]
    pub game: Option<PathBuf>,
}

/// What kind of test one is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TestKind {
    /// A `test_` function called in the VM (QCT-2).
    Unit,
    /// A scenario file played in `qs-sim` (QCT-3).
    Scenario,
}

/// How a test came out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// It passed.
    Passed,
    /// It failed.
    Failed,
    /// It could not run, as QCT-3.3 says, and neither passed nor failed.
    Skipped,
}

/// One test's result (QCT-4.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TestResult {
    /// Its kind.
    pub kind: TestKind,
    /// Its name: the function's, or the scenario's path from the project (QCT-1.2).
    pub name: String,
    /// How it came out.
    pub verdict: Verdict,
    /// Why it failed or was skipped.
    pub message: Option<String>,
}

/// What `project.test` answers (QCT-4.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TestRun {
    /// No test failed.
    pub passed: bool,
    /// How many passed.
    pub passed_count: usize,
    /// How many failed.
    pub failed_count: usize,
    /// How many were skipped.
    pub skipped_count: usize,
    /// Each test, unit tests first (QCT-1.1, QCT-1.2).
    pub tests: Vec<TestResult>,
}

impl TestRun {
    /// The run of `tests`, counted.
    fn of(tests: Vec<TestResult>) -> Self {
        let count = |verdict| tests.iter().filter(|t| t.verdict == verdict).count();
        let failed_count = count(Verdict::Failed);
        Self {
            passed: failed_count == 0,
            passed_count: count(Verdict::Passed),
            failed_count,
            skipped_count: count(Verdict::Skipped),
            tests,
        }
    }
}

/// Why `project.test` answered no tests.
#[derive(Debug)]
pub enum TestRunError {
    /// No project is open, or its build failed to run.
    Build(WorkbenchError),
    /// No module built a program: each module's problems (QCT-4.1).
    NothingBuilt(Vec<ModuleBuilt>),
    /// The program the build wrote could not be read.
    Progs(String),
}

impl std::fmt::Display for TestRunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Build(error) => write!(f, "the project did not build: {error}"),
            Self::NothingBuilt(modules) => {
                f.write_str("no module of the project built a program")?;
                for problem in modules.iter().flat_map(|m| &m.problems) {
                    match &problem.place {
                        Some(place) => write!(
                            f,
                            "\n{}:{}: {}: {}",
                            place.path.display(),
                            place.location.line(),
                            problem.code,
                            problem.message
                        )?,
                        None => write!(f, "\n{}: {}", problem.code, problem.message)?,
                    }
                }
                Ok(())
            }
            Self::Progs(why) => write!(f, "the built program does not read: {why}"),
        }
    }
}

impl std::error::Error for TestRunError {}

/// The names of the program's unit tests, in the order of its function table (QCT-1.1).
fn unit_tests(program: &Progs<'_>) -> Vec<String> {
    program
        .functions()
        .iter()
        .filter(|function| function.first_statement >= 0 && function.numparms == 0)
        .filter_map(|function| program.string(function.s_name))
        .filter_map(|name| std::str::from_utf8(name).ok())
        .filter(|name| name.starts_with(UNIT_PREFIX))
        .map(str::to_owned)
        .collect()
}

/// Each `.scenario` file under `root/tests`, with its name, in name order (QCT-1.2).
fn scenario_tests(root: &Path) -> Vec<(String, PathBuf)> {
    let mut found = Vec::new();
    let mut dirs = vec![root.join(SCENARIO_DIR)];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|e| e == "scenario") {
                let name = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/");
                found.push((name, path));
            }
        }
    }
    found.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    found
}

/// A unit test's result from its call (QCT-2.2).
fn unit_result(name: String, progs: &Path, bytes: &[u8]) -> TestResult {
    let params = VmRunParams {
        function: name.clone(),
        ..VmRunParams::default()
    };
    let (verdict, message) = match run(progs.to_path_buf(), bytes, &params) {
        Ok(ran) => match ran.error {
            None => (Verdict::Passed, None),
            Some(error) => (Verdict::Failed, Some(error)),
        },
        Err(error) => (Verdict::Failed, Some(error.to_string())),
    };
    TestResult {
        kind: TestKind::Unit,
        name,
        verdict,
        message,
    }
}

/// A scenario test's verdict and message from its report (QCT-3.2).
pub(crate) fn scenario_verdict(report: &Report) -> (Verdict, Option<String>) {
    if report.passed {
        return (Verdict::Passed, None);
    }
    let mut message = String::new();
    for check in report.checks.iter().filter(|check| !check.passed) {
        message.push_str(&format!(
            "line {}: expect {}: found {}
",
            check.line, check.text, check.found
        ));
    }
    if let Some(error) = &report.error {
        message.push_str(&format!("frame {}: {error}\n", report.frames));
    }
    (Verdict::Failed, Some(message.trim_end().to_owned()))
}

impl<L: Loader> Workbench<L> {
    /// Builds the open project once and runs its tests, unit tests then scenario tests, those
    /// `params` keeps (QCT-1 to QCT-4).
    ///
    /// # Errors
    ///
    /// [`TestRunError`] when no project is open, no module builds, or the program does not
    /// read; a test that fails is answered in the run, not an error.
    pub fn run_tests(&mut self, params: &RunTestsParams) -> Result<TestRun, TestRunError> {
        let root = self
            .root()
            .map(Path::to_path_buf)
            .ok_or(TestRunError::Build(WorkbenchError::NoProject))?;
        let modules = self.build().map_err(TestRunError::Build)?;
        let Some(built) = modules.iter().find(|module| module.built) else {
            return Err(TestRunError::NothingBuilt(modules));
        };
        let progs = built.output.clone();
        let bytes = std::fs::read(&progs)
            .map_err(|e| TestRunError::Progs(format!("{}: {e}", progs.display())))?;
        let program = Progs::read(&bytes).map_err(|e| TestRunError::Progs(e.to_string()))?;
        let kept = |name: &str| params.filter.as_deref().is_none_or(|f| name.contains(f));
        let mut tests: Vec<TestResult> = unit_tests(&program)
            .into_iter()
            .filter(|name| kept(name))
            .map(|name| unit_result(name, &progs, &bytes))
            .collect();
        let scenarios: Vec<(String, PathBuf)> = scenario_tests(&root)
            .into_iter()
            .filter(|(name, _)| kept(name))
            .collect();
        if !scenarios.is_empty() {
            tests.extend(play_scenarios(
                &root,
                &progs,
                params.game.as_deref(),
                scenarios,
            ));
        }
        Ok(TestRun::of(tests))
    }
}

/// Each of `scenarios` played on the program `progs` of the project at `root`, over the game
/// `game` names (QCT-3).
fn play_scenarios(
    root: &Path,
    progs: &Path,
    game: Option<&Path>,
    scenarios: Vec<(String, PathBuf)>,
) -> Vec<TestResult> {
    let result = |name: String, (verdict, message): (Verdict, Option<String>)| TestResult {
        kind: TestKind::Scenario,
        name,
        verdict,
        message,
    };
    let skip_all = |why: String| {
        scenarios
            .iter()
            .map(|(name, _)| result(name.clone(), (Verdict::Skipped, Some(why.clone()))))
            .collect()
    };
    // QCT-3.3: the game, and a program where a run root can take it.
    let base = match game_base(game) {
        Ok(base) => base,
        Err(error) => return skip_all(error.to_string()),
    };
    let Some(mod_name) = mod_folder(root, progs) else {
        return skip_all(format!(
            "{} is not in the project's build/<mod>/ folder",
            progs.display()
        ));
    };
    let run_root = match prepare_run_root(root, Flavour::Original, &base, &mod_name) {
        Ok(run_root) => run_root,
        Err(error) => return skip_all(error.to_string()),
    };
    let mut out = Vec::new();
    for (name, path) in &scenarios {
        let played = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|text| Scenario::parse(&text).map_err(|e| e.to_string()))
            .and_then(|scenario| {
                play_on_game(&engine_words(&run_root, &scenario), &scenario)
                    .map_err(|e| e.to_string())
            });
        out.push(match played {
            Ok(report) => result(name.clone(), scenario_verdict(&report)),
            Err(why) => result(name.clone(), (Verdict::Failed, Some(why))),
        });
    }
    out
}

/// `project.test`: builds the project and runs its tests.
struct RunTests;

impl<C: HasWorkbench + 'static> Command<C> for RunTests {
    const ID: &'static str = RUN_TESTS;
    const SUMMARY: &'static str = "Builds the open project and runs its tests: each test_ \
        function called alone in the VM over a mock engine, and each scenario under tests/ \
        played in the server simulation, answering each test's verdict.";
    type Params = RunTestsParams;
    type Output = TestRun;
    type Error = TestRunError;

    fn run(
        &self,
        context: &mut C,
        params: RunTestsParams,
    ) -> Result<Outcome<TestRun, C>, TestRunError> {
        Ok(Outcome::outside_history(
            context.workbench().run_tests(&params)?,
        ))
    }
}

/// Registers `project.test`.
///
/// # Errors
///
/// [`RegisterError`] when its id is taken.
pub fn register_tests<C: HasWorkbench + 'static>(
    registry: &mut Registry<C>,
) -> Result<(), RegisterError> {
    registry.register(RunTests)
}

#[cfg(test)]
mod tests;
