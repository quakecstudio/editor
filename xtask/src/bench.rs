// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask bench`: runs the performance-budget benchmarks, checks them and
//! records them in the JSON history (PRIN-5, ENG-13, TEST-8).
//!
//! The budgets are the table in `docs/perf-budgets.md`; the benchmarks are in
//! `crates/qs-bench/benches/budgets.rs`. Each active budget fails the run when
//! it is over budget or more than 10% slower than the last passing run on the
//! same host. Pending budgets run but are not checked.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::repo;

/// The document that holds the budget table.
pub(crate) const BUDGETS_DOC: &str = "docs/perf-budgets.md";
/// The benchmark source that must define every budget's benchmark.
pub(crate) const BENCH_SOURCE: &str = "crates/qs-bench/benches/budgets.rs";
const TABLE_START: &str = "<!-- perf-budgets -->";
const TABLE_END: &str = "<!-- /perf-budgets -->";
/// The criterion group the benchmarks belong to.
const GROUP: &str = "budgets";
const DEFAULT_HISTORY: &str = "target/bench/history.json";
const CRITERION_HOME: &str = "target/xtask/bench/criterion";
/// A run more than this much slower than the baseline is a regression.
const REGRESSION_RATIO: f64 = 1.10;
const HISTORY_VERSION: u32 = 1;

const USAGE: &str = "usage: cargo xtask bench [--history <path>] [--accept]\n       \
                     cargo xtask bench history [--history <path>]";

/// Which summary of the samples a budget is checked against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Statistic {
    Median,
    P95,
    P99,
}

impl Statistic {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "median" => Some(Self::Median),
            "p95" => Some(Self::P95),
            "p99" => Some(Self::P99),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Median => "median",
            Self::P95 => "p95",
            Self::P99 => "p99",
        }
    }

    fn of(self, result: &BenchResult) -> f64 {
        match self {
            Self::Median => result.median_ns,
            Self::P95 => result.p95_ns,
            Self::P99 => result.p99_ns,
        }
    }
}

/// Whether a budget's benchmark measures the real operation yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Status {
    /// A placeholder: it runs but is not checked.
    Pending,
    /// The real measurement: checked against the budget and the baseline.
    Active,
}

/// One row of the budget table.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Budget {
    pub(crate) name: String,
    pub(crate) budget_ns: f64,
    pub(crate) statistic: Statistic,
    pub(crate) status: Status,
}

/// The JSON history file.
#[derive(Debug, Default, Serialize, Deserialize)]
struct History {
    version: u32,
    runs: Vec<Run>,
}

/// One recorded run of the benchmarks.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Run {
    /// Seconds since the Unix epoch, UTC.
    unix_time: u64,
    commit: Option<String>,
    host: Host,
    /// No active budget was over budget or regressed (or `--accept` was given).
    passed: bool,
    /// The run was recorded with `--accept`.
    accepted: bool,
    results: Vec<BenchResult>,
}

/// The machine a run was recorded on.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Host {
    /// Runs compare only with runs that have the same key.
    key: String,
    os: String,
    arch: String,
    cpu: Option<String>,
    cores: Option<usize>,
}

/// One benchmark's result in a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BenchResult {
    benchmark: String,
    status: Status,
    statistic: Statistic,
    budget_ns: f64,
    samples: usize,
    median_ns: f64,
    p95_ns: f64,
    p99_ns: f64,
}

/// The part of criterion's `sample.json` the harness reads.
#[derive(Debug, Deserialize)]
struct Sample {
    iters: Vec<f64>,
    times: Vec<f64>,
}

/// The parsed command line.
#[derive(Debug, PartialEq)]
struct Options {
    history: Option<PathBuf>,
    accept: bool,
    show_history: bool,
}

/// Runs `cargo xtask bench` with the arguments after `bench`.
pub(crate) fn main(root: &Path, args: &[String]) -> Result<bool, String> {
    let options = parse_args(args)?;
    let history_path = options
        .history
        .clone()
        .unwrap_or_else(|| root.join(DEFAULT_HISTORY));
    if options.show_history {
        let history = load_history(&history_path)?;
        print!("{}", format_history(&history));
        return Ok(true);
    }
    let budgets = parse_budgets(&repo::read(root, BUDGETS_DOC)?)?;
    let criterion_home = root.join(CRITERION_HOME);
    run_benchmarks(root, &criterion_home)?;
    let mut results = Vec::new();
    for budget in &budgets {
        let sample = read_sample(&criterion_home, &budget.name)?;
        results.push(summarise(budget, &sample)?);
    }
    let mut history = load_history(&history_path)?;
    let host = host();
    let baseline = baseline(&history, &host.key);
    let outcome = evaluate(&results, baseline);
    let passed = outcome.passed(options.accept);
    print!("{}", format_results(&host, &results, baseline));
    let github = std::env::var_os("GITHUB_ACTIONS").is_some_and(|value| value == "true");
    print!("{}", outcome.report(options.accept, github));
    history.runs.push(Run {
        unix_time: unix_time(),
        commit: commit(root),
        host,
        passed,
        accepted: options.accept,
        results,
    });
    save_history(&history_path, &history)?;
    println!("bench: recorded in {}", history_path.display());
    Ok(passed)
}

/// The last passing run recorded on the host with this key.
fn baseline<'a>(history: &'a History, host_key: &str) -> Option<&'a Run> {
    history
        .runs
        .iter()
        .rev()
        .find(|run| run.passed && run.host.key == host_key)
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        history: None,
        accept: false,
        show_history: false,
    };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "history" if !options.show_history => options.show_history = true,
            "--accept" if !options.accept => options.accept = true,
            "--history" if options.history.is_none() => {
                let Some(path) = args.next() else {
                    return Err(format!("--history needs a path\n{USAGE}"));
                };
                options.history = Some(PathBuf::from(path));
            }
            other => return Err(format!("unexpected argument {other:?}\n{USAGE}")),
        }
    }
    if options.show_history && options.accept {
        return Err(format!("--accept only applies to a run\n{USAGE}"));
    }
    Ok(options)
}

/// Reads the budget table between the `perf-budgets` markers.
pub(crate) fn parse_budgets(doc: &str) -> Result<Vec<Budget>, String> {
    let Some((_, rest)) = doc.split_once(TABLE_START) else {
        return Err(format!("{BUDGETS_DOC} has no {TABLE_START} marker"));
    };
    let Some((table, _)) = rest.split_once(TABLE_END) else {
        return Err(format!("{BUDGETS_DOC} has no {TABLE_END} marker"));
    };
    let mut budgets: Vec<Budget> = Vec::new();
    for line in table.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        let [name, _requirement, budget, statistic, _owner, status] = cells.as_slice() else {
            return Err(format!("{BUDGETS_DOC}: a row needs 6 cells: {line}"));
        };
        if *name == "Benchmark" || name.starts_with("---") {
            continue;
        }
        let Some(name) = name
            .strip_prefix('`')
            .and_then(|name| name.strip_suffix('`'))
            .filter(|name| is_benchmark_name(name))
        else {
            return Err(format!(
                "{BUDGETS_DOC}: {name:?} is not a benchmark name in backticks"
            ));
        };
        if budgets.iter().any(|existing| existing.name == name) {
            return Err(format!("{BUDGETS_DOC}: {name} is listed twice"));
        }
        let budget_ns = parse_duration(budget)
            .ok_or_else(|| format!("{BUDGETS_DOC}: {name}: {budget:?} is not a duration"))?;
        let statistic = Statistic::parse(statistic).ok_or_else(|| {
            format!("{BUDGETS_DOC}: {name}: statistic {statistic:?} is not median, p95 or p99")
        })?;
        let status = match *status {
            "pending" => Status::Pending,
            "active" => Status::Active,
            other => {
                return Err(format!(
                    "{BUDGETS_DOC}: {name}: status {other:?} is not pending or active"
                ));
            }
        };
        budgets.push(Budget {
            name: name.to_owned(),
            budget_ns,
            statistic,
            status,
        });
    }
    if budgets.is_empty() {
        return Err(format!("{BUDGETS_DOC}: the budget table is empty"));
    }
    Ok(budgets)
}

fn is_benchmark_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Parses `8.33 ms` into nanoseconds.
fn parse_duration(text: &str) -> Option<f64> {
    let (number, unit) = text.split_once(' ')?;
    let value: f64 = number.parse().ok()?;
    let scale = match unit {
        "ns" => 1.0,
        "us" => 1e3,
        "ms" => 1e6,
        "s" => 1e9,
        _ => return None,
    };
    let nanos = value * scale;
    (nanos.is_finite() && nanos > 0.0).then_some(nanos)
}

fn run_benchmarks(root: &Path, criterion_home: &Path) -> Result<(), String> {
    if criterion_home.exists() {
        fs::remove_dir_all(criterion_home)
            .map_err(|error| format!("cannot clear {}: {error}", criterion_home.display()))?;
    }
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .args([
            "bench",
            "--locked",
            "--package",
            "qs-bench",
            "--bench",
            GROUP,
        ])
        .env("CRITERION_HOME", criterion_home)
        .current_dir(root)
        .status()
        .map_err(|error| format!("cannot run cargo bench: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("cargo bench failed: {status}"))
    }
}

fn read_sample(criterion_home: &Path, name: &str) -> Result<Sample, String> {
    let path = criterion_home
        .join(GROUP)
        .join(name)
        .join("new")
        .join("sample.json");
    let text = fs::read_to_string(&path).map_err(|error| {
        format!(
            "no result for budget {name} ({error}); add a benchmark named {name} to {BENCH_SOURCE}"
        )
    })?;
    serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))
}

/// The median, p95 and p99 time per iteration over a benchmark's samples.
fn summarise(budget: &Budget, sample: &Sample) -> Result<BenchResult, String> {
    let name = &budget.name;
    if sample.iters.len() != sample.times.len() || sample.iters.is_empty() {
        return Err(format!(
            "{name}: criterion recorded {} iteration counts and {} times",
            sample.iters.len(),
            sample.times.len()
        ));
    }
    let mut per_iteration = Vec::with_capacity(sample.times.len());
    for (iters, time) in sample.iters.iter().zip(&sample.times) {
        let value = time / iters;
        if !value.is_finite() || value < 0.0 {
            return Err(format!(
                "{name}: criterion recorded an invalid sample ({time} ns over {iters} iterations)"
            ));
        }
        per_iteration.push(value);
    }
    per_iteration.sort_by(f64::total_cmp);
    let percentile = |rank| {
        nearest_rank(&per_iteration, rank)
            .ok_or_else(|| format!("{name}: cannot take the {rank}th percentile"))
    };
    Ok(BenchResult {
        benchmark: name.clone(),
        status: budget.status,
        statistic: budget.statistic,
        budget_ns: budget.budget_ns,
        samples: per_iteration.len(),
        median_ns: percentile(50)?,
        p95_ns: percentile(95)?,
        p99_ns: percentile(99)?,
    })
}

/// The nearest-rank percentile of sorted values: the smallest value with at
/// least `percent`% of the values at or below it.
fn nearest_rank(sorted: &[f64], percent: usize) -> Option<f64> {
    let rank = percent.checked_mul(sorted.len())?.div_ceil(100);
    sorted.get(rank.checked_sub(1)?).copied()
}

/// What checking a run found.
#[derive(Debug, Default)]
struct Outcome {
    /// Active budgets over budget: always fail the run.
    failures: Vec<String>,
    /// Active budgets more than 10% slower than the baseline.
    regressions: Vec<String>,
}

impl Outcome {
    /// Whether the run passes; `accept` lets regressions through, never failures.
    fn passed(&self, accept: bool) -> bool {
        self.failures.is_empty() && (accept || self.regressions.is_empty())
    }

    /// One line per problem, plus a GitHub Actions error annotation for each
    /// one that fails the run when `github` is set.
    fn report(&self, accept: bool, github: bool) -> String {
        let mut out = String::new();
        for (line, fails) in self
            .failures
            .iter()
            .map(|line| (line, true))
            .chain(self.regressions.iter().map(|line| (line, !accept)))
        {
            if !fails {
                out.push_str(&format!("ACCEPTED  {line}\n"));
                continue;
            }
            if github {
                out.push_str(&format!("::error title=Performance budget::{line}\n"));
            }
            out.push_str(&format!("FAIL  {line}\n"));
        }
        out
    }
}

fn evaluate(results: &[BenchResult], baseline: Option<&Run>) -> Outcome {
    let mut outcome = Outcome::default();
    for result in results {
        if result.status == Status::Pending {
            continue;
        }
        let statistic = result.statistic;
        let value = statistic.of(result);
        if value > result.budget_ns {
            outcome.failures.push(format!(
                "{}: {} {} is over the budget of {}",
                result.benchmark,
                statistic.name(),
                format_ns(value),
                format_ns(result.budget_ns)
            ));
        }
        let previous = baseline.and_then(|run| {
            run.results.iter().find(|previous| {
                previous.benchmark == result.benchmark && previous.status == Status::Active
            })
        });
        if let Some(previous) = previous {
            let before = statistic.of(previous);
            if value > before * REGRESSION_RATIO {
                outcome.regressions.push(format!(
                    "{}: {} {} is more than 10% slower than {} in the last passing run",
                    result.benchmark,
                    statistic.name(),
                    format_ns(value),
                    format_ns(before)
                ));
            }
        }
    }
    outcome
}

fn format_results(host: &Host, results: &[BenchResult], baseline: Option<&Run>) -> String {
    let mut out = format!("\n==> bench results on {}\n", host.key);
    for result in results {
        let statistic = result.statistic;
        let previous = baseline
            .and_then(|run| {
                run.results
                    .iter()
                    .find(|previous| previous.benchmark == result.benchmark)
            })
            .map_or_else(
                || "none".to_owned(),
                |previous| format_ns(statistic.of(previous)),
            );
        let status = match result.status {
            Status::Pending => "pending",
            Status::Active => "active",
        };
        out.push_str(&format!(
            "{:<24} {status:<8} {:<6} {:>10}  budget {:>10}  last {:>10}  ({} samples)\n",
            result.benchmark,
            statistic.name(),
            format_ns(statistic.of(result)),
            format_ns(result.budget_ns),
            previous,
            result.samples
        ));
    }
    out
}

fn format_history(history: &History) -> String {
    if history.runs.is_empty() {
        return "bench: no runs recorded\n".to_owned();
    }
    let mut out = String::new();
    for run in &history.runs {
        let commit = run
            .commit
            .as_deref()
            .map_or("unknown", |commit| commit.get(..12).unwrap_or(commit));
        let verdict = match (run.passed, run.accepted) {
            (true, true) => "accepted",
            (true, false) => "passed",
            (false, _) => "FAILED",
        };
        out.push_str(&format!(
            "{} {commit} {} {verdict}\n",
            utc_date(run.unix_time),
            run.host.key
        ));
        for result in &run.results {
            out.push_str(&format!(
                "    {:<24} {:<7} median {:>10}  p95 {:>10}  p99 {:>10}\n",
                result.benchmark,
                match result.status {
                    Status::Pending => "pending",
                    Status::Active => "active",
                },
                format_ns(result.median_ns),
                format_ns(result.p95_ns),
                format_ns(result.p99_ns)
            ));
        }
    }
    out
}

/// Formats nanoseconds with a unit that keeps the number readable.
fn format_ns(nanos: f64) -> String {
    if nanos < 1e3 {
        format!("{nanos:.2} ns")
    } else if nanos < 1e6 {
        format!("{:.2} us", nanos / 1e3)
    } else if nanos < 1e9 {
        format!("{:.2} ms", nanos / 1e6)
    } else {
        format!("{:.3} s", nanos / 1e9)
    }
}

fn load_history(path: &Path) -> Result<History, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(History {
                version: HISTORY_VERSION,
                runs: Vec::new(),
            });
        }
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let history: History =
        serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?;
    if history.version != HISTORY_VERSION {
        return Err(format!(
            "{}: history version {} is not {HISTORY_VERSION}",
            path.display(),
            history.version
        ));
    }
    Ok(history)
}

fn save_history(path: &Path, history: &History) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let mut text = serde_json::to_string_pretty(history)
        .map_err(|error| format!("cannot serialise the history: {error}"))?;
    text.push('\n');
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, text).map_err(|error| format!("{}: {error}", temporary.display()))?;
    fs::rename(&temporary, path).map_err(|error| format!("{}: {error}", path.display()))
}

fn host() -> Host {
    let os = std::env::consts::OS.to_owned();
    let arch = std::env::consts::ARCH.to_owned();
    let cpu = cpu_model();
    let key = host_key(
        std::env::var("QS_BENCH_HOST").ok(),
        &os,
        &arch,
        cpu.as_deref(),
    );
    Host {
        key,
        os,
        arch,
        cpu,
        cores: std::thread::available_parallelism()
            .ok()
            .map(std::num::NonZero::get),
    }
}

/// `QS_BENCH_HOST` when it is set and not blank, otherwise the OS,
/// architecture and CPU model.
fn host_key(named: Option<String>, os: &str, arch: &str, cpu: Option<&str>) -> String {
    if let Some(named) = named.filter(|named| !named.trim().is_empty()) {
        return named;
    }
    let cpu = cpu.unwrap_or("unknown-cpu");
    let cpu: String = cpu
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .to_ascii_lowercase();
    format!("{os}-{arch}-{cpu}")
}

/// The CPU's model name, from the OS.
fn cpu_model() -> Option<String> {
    let model = match std::env::consts::OS {
        "macos" => command_output("sysctl", &["-n", "machdep.cpu.brand_string"]),
        "linux" => fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|info| cpuinfo_model(&info)),
        "windows" => std::env::var("PROCESSOR_IDENTIFIER").ok(),
        _ => None,
    }?;
    let model = model.trim();
    (!model.is_empty()).then(|| model.to_owned())
}

/// The first `model name` in Linux's `/proc/cpuinfo`.
fn cpuinfo_model(cpuinfo: &str) -> Option<String> {
    cpuinfo
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(key, _)| key.trim() == "model name")
        .map(|(_, value)| value.to_owned())
}

fn commit(root: &Path) -> Option<String> {
    let root = root.to_str()?;
    command_output("git", &["-C", root, "rev-parse", "HEAD"]).map(|sha| sha.trim().to_owned())
}

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// Formats seconds since the Unix epoch as a UTC date and time.
fn utc_date(unix_time: u64) -> String {
    civil(unix_time).unwrap_or_else(|| format!("@{unix_time}"))
}

/// Howard Hinnant's `civil_from_days`, for days on or after 1970-01-01.
fn civil(unix_time: u64) -> Option<String> {
    let days = unix_time.checked_div(86_400)?;
    let seconds = unix_time.checked_rem(86_400)?;
    let z = days.checked_add(719_468)?;
    let era = z.checked_div(146_097)?;
    let doe = z.checked_sub(era.checked_mul(146_097)?)?;
    let yoe = doe
        .checked_sub(doe.checked_div(1_460)?)?
        .checked_add(doe.checked_div(36_524)?)?
        .checked_sub(doe.checked_div(146_096)?)?
        .checked_div(365)?;
    let doy = doe.checked_sub(
        yoe.checked_mul(365)?
            .checked_add(yoe.checked_div(4)?)?
            .checked_sub(yoe.checked_div(100)?)?,
    )?;
    let mp = doy.checked_mul(5)?.checked_add(2)?.checked_div(153)?;
    let day = doy
        .checked_sub(mp.checked_mul(153)?.checked_add(2)?.checked_div(5)?)?
        .checked_add(1)?;
    let month = if mp < 10 {
        mp.checked_add(3)?
    } else {
        mp.checked_sub(9)?
    };
    let year = yoe
        .checked_add(era.checked_mul(400)?)?
        .checked_add(u64::from(month <= 2))?;
    let hour = seconds.checked_div(3_600)?;
    let minute = seconds.checked_rem(3_600)?.checked_div(60)?;
    let second = seconds.checked_rem(60)?;
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z"
    ))
}

#[cfg(test)]
mod tests;
