// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use super::*;
use crate::parse_cli;
use crate::test_dirs::fresh;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

/// A scratch directory for one test, emptied first.
fn scratch(name: &str) -> PathBuf {
    fresh(&format!("bench/{name}"))
}

fn table(rows: &str) -> String {
    format!(
        "# Budgets\n\n{TABLE_START}\n| Benchmark | Requirement | Budget | Statistic | Filled in by | Status |\n\
         | --- | --- | --- | --- | --- | --- |\n{rows}{TABLE_END}\n"
    )
}

fn parse_error(rows: &str) -> String {
    parse_budgets(&table(rows)).unwrap_err()
}

/// A result with every statistic equal to `ns`, for the axis a test sets.
fn result(name: &str, status: Status, statistic: Statistic, ns: f64) -> BenchResult {
    BenchResult {
        benchmark: name.to_owned(),
        status,
        statistic,
        budget_ns: 1e6,
        samples: 10,
        median_ns: ns,
        p95_ns: ns,
        p99_ns: ns,
    }
}

fn run(key: &str, passed: bool, results: Vec<BenchResult>) -> Run {
    Run {
        unix_time: 0,
        commit: None,
        host: Host {
            key: key.to_owned(),
            os: "linux".to_owned(),
            arch: "x86_64".to_owned(),
            cpu: None,
            cores: None,
        },
        passed,
        accepted: false,
        results,
    }
}

#[test]
fn the_repository_budget_table_names_every_budget() {
    let budgets = parse_budgets(&repo::read(&root(), BUDGETS_DOC).unwrap()).unwrap();
    let rows: Vec<(&str, f64, Statistic)> = budgets
        .iter()
        .map(|budget| (budget.name.as_str(), budget.budget_ns, budget.statistic))
        .collect();
    assert_eq!(
        rows,
        [
            ("open_project_and_type", 1e9, Statistic::P95),
            ("keystroke", 8.33e6, Statistic::P99),
            ("check_after_typing", 1e8, Statistic::P95),
            ("id1_recompile", 2e8, Statistic::Median),
            ("save_to_game", 1e9, Statistic::P95),
            ("frame_e1m1", 383e3, Statistic::P99),
            ("server_frame_e1m1", 1e6, Statistic::P99),
            ("quakec_e1m1", 5e5, Statistic::P99),
            ("map_load_e1m1", 1e9, Statistic::Median),
            ("frame_mg1", 3.62e6, Statistic::P99),
            ("server_frame_mg1", 1e6, Statistic::P99),
            ("quakec_mg1", 5e5, Statistic::P99),
            ("map_load_mg1", 1e9, Statistic::Median),
        ]
    );
}

// The route benchmarks read a game, so each must be in GAME_BUDGETS, judged pending when its
// game is not set; a route left out would pass on a placeholder.
#[test]
fn every_game_budget_is_listed_with_its_game() {
    let budgets = parse_budgets(&repo::read(&root(), BUDGETS_DOC).unwrap()).unwrap();
    let source = repo::read(&root(), BENCH_SOURCE).unwrap();
    let routes = ["frame_", "server_frame_", "quakec_", "map_load_"];
    for budget in &budgets {
        let routed = routes.iter().any(|prefix| budget.name.starts_with(prefix));
        let listed = GAME_BUDGETS.iter().any(|(name, _)| *name == budget.name);
        assert_eq!(
            routed || budget.name == "save_to_game",
            listed,
            "{}",
            budget.name
        );
    }
    for (name, variable) in GAME_BUDGETS {
        assert!(
            budgets.iter().any(|budget| budget.name == name),
            "{name} has no row"
        );
        assert!(
            source.contains(&format!("\"{variable}\"")),
            "{variable} is not read"
        );
    }
}

#[test]
fn every_budget_has_a_named_benchmark() {
    let budgets = parse_budgets(&repo::read(&root(), BUDGETS_DOC).unwrap()).unwrap();
    let source = repo::read(&root(), BENCH_SOURCE).unwrap();
    assert!(source.contains("benchmark_group(GROUP)"));
    assert!(source.contains(&format!("const GROUP: &str = \"{GROUP}\";")));
    for budget in &budgets {
        assert!(
            source.contains(&format!("\"{}\"", budget.name)),
            "{BENCH_SOURCE} has no benchmark named {}",
            budget.name
        );
    }
}

#[test]
fn a_row_parses_into_a_budget() {
    let budgets =
        parse_budgets(&table("| `a_b2` | PRIN-5 | 250 us | p99 | #1 | active |\n")).unwrap();
    assert_eq!(
        budgets,
        [Budget {
            name: "a_b2".to_owned(),
            budget_ns: 250_000.0,
            statistic: Statistic::P99,
            status: Status::Active,
        }]
    );
}

#[test]
fn a_malformed_table_is_an_error() {
    assert!(
        parse_budgets("no markers")
            .unwrap_err()
            .contains("no <!-- perf-budgets -->")
    );
    assert!(
        parse_budgets(TABLE_START)
            .unwrap_err()
            .contains("no <!-- /perf-budgets -->")
    );
    assert!(parse_error("").contains("empty"));
    assert!(parse_error("| `a` | PRIN-5 | 1 s | p95 |\n").contains("6 cells"));
    assert!(parse_error("| a | PRIN-5 | 1 s | p95 | #1 | active |\n").contains("backticks"));
    assert!(parse_error("| `A-b` | PRIN-5 | 1 s | p95 | #1 | active |\n").contains("backticks"));
    assert!(parse_error("| `a` | PRIN-5 | 1 sec | p95 | #1 | active |\n").contains("duration"));
    assert!(parse_error("| `a` | PRIN-5 | 1 s | p90 | #1 | active |\n").contains("statistic"));
    assert!(parse_error("| `a` | PRIN-5 | 1 s | p95 | #1 | done |\n").contains("status"));
    let twice = "| `a` | PRIN-5 | 1 s | p95 | #1 | active |\n".repeat(2);
    assert!(parse_error(&twice).contains("listed twice"));
}

#[test]
fn durations_parse_in_every_unit_and_reject_the_rest() {
    assert_eq!(parse_duration("5 ns"), Some(5.0));
    assert_eq!(parse_duration("1.5 us"), Some(1_500.0));
    assert_eq!(parse_duration("8.33 ms"), Some(8_330_000.0));
    assert_eq!(parse_duration("1 s"), Some(1e9));
    for bad in ["1s", "1 min", "x ms", "0 ms", "-1 ms", "inf s", "NaN s", ""] {
        assert_eq!(parse_duration(bad), None, "{bad}");
    }
}

#[test]
fn nearest_rank_takes_the_smallest_value_covering_the_percentile() {
    let values: Vec<f64> = (1..=10).map(f64::from).collect();
    assert_eq!(nearest_rank(&values, 50), Some(5.0));
    assert_eq!(nearest_rank(&values, 95), Some(10.0));
    assert_eq!(nearest_rank(&values, 1), Some(1.0));
    let hundred: Vec<f64> = (1..=100).map(f64::from).collect();
    assert_eq!(nearest_rank(&hundred, 95), Some(95.0));
    assert_eq!(nearest_rank(&hundred, 99), Some(99.0));
    assert_eq!(nearest_rank(&[], 50), None);
    assert_eq!(nearest_rank(&values, 0), None);
}

#[test]
fn samples_summarise_per_iteration() {
    let budget = Budget {
        name: "a".to_owned(),
        budget_ns: 1e6,
        statistic: Statistic::Median,
        status: Status::Pending,
    };
    let sample = Sample {
        iters: vec![1.0, 2.0, 4.0, 1.0],
        times: vec![10.0, 40.0, 40.0, 30.0],
    };
    let summary = summarise(&budget, &sample).unwrap();
    assert_eq!(summary.samples, 4);
    assert_eq!(summary.median_ns, 10.0);
    assert_eq!(summary.p95_ns, 30.0);
    assert_eq!(summary.p99_ns, 30.0);
    assert_eq!(summary.status, Status::Pending);

    let mismatched = Sample {
        iters: vec![1.0],
        times: vec![],
    };
    assert!(
        summarise(&budget, &mismatched)
            .unwrap_err()
            .contains("1 iteration counts and 0 times")
    );
    let empty = Sample {
        iters: vec![],
        times: vec![],
    };
    assert!(summarise(&budget, &empty).is_err());
    let zero = Sample {
        iters: vec![0.0],
        times: vec![5.0],
    };
    assert!(
        summarise(&budget, &zero)
            .unwrap_err()
            .contains("invalid sample")
    );
    let instant = Sample {
        iters: vec![1.0],
        times: vec![0.0],
    };
    assert_eq!(summarise(&budget, &instant).unwrap().median_ns, 0.0);
    let negative = Sample {
        iters: vec![1.0],
        times: vec![-5.0],
    };
    assert!(
        summarise(&budget, &negative)
            .unwrap_err()
            .contains("invalid sample")
    );
}

#[test]
fn criterion_samples_are_read_from_the_group_directory() {
    let home = scratch("criterion");
    let dir = home.join(GROUP).join("keystroke").join("new");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("sample.json"),
        r#"{"sampling_mode":"Linear","iters":[1.0,2.0],"times":[5.0,8.0]}"#,
    )
    .unwrap();
    let sample = read_sample(&home, "keystroke").unwrap();
    assert_eq!(sample.times, [5.0, 8.0]);
    let missing = read_sample(&home, "save_to_game").unwrap_err();
    assert!(
        missing.contains("add a benchmark named save_to_game"),
        "{missing}"
    );
    fs::write(dir.join("sample.json"), "{").unwrap();
    assert!(read_sample(&home, "keystroke").is_err());
}

#[test]
fn pending_budgets_are_never_checked() {
    let slow = result("a", Status::Pending, Statistic::Median, 1e12);
    let baseline = run(
        "h",
        true,
        vec![result("a", Status::Pending, Statistic::Median, 1.0)],
    );
    let outcome = evaluate(&[slow], Some(&baseline), Machine::Reference);
    assert!(outcome.failures.is_empty() && outcome.regressions.is_empty());
}

#[test]
fn an_active_budget_over_its_budget_fails() {
    let at_budget = result("a", Status::Active, Statistic::P95, 1e6);
    assert!(
        evaluate(&[at_budget], None, Machine::Reference)
            .failures
            .is_empty()
    );
    let over = result("a", Status::Active, Statistic::P95, 1.01e6);
    let failures = evaluate(&[over], None, Machine::Reference).failures;
    assert_eq!(failures, ["a: p95 1.01 ms is over the budget of 1.00 ms"]);
}

#[test]
fn the_budget_statistic_is_the_one_checked() {
    let mut mixed = result("a", Status::Active, Statistic::Median, 5e5);
    mixed.p99_ns = 5e6;
    assert!(
        evaluate(&[mixed.clone()], None, Machine::Reference)
            .failures
            .is_empty()
    );
    mixed.statistic = Statistic::P99;
    assert_eq!(
        evaluate(&[mixed], None, Machine::Reference).failures.len(),
        1
    );
}

#[test]
fn a_regression_over_ten_percent_is_reported() {
    let baseline = run(
        "h",
        true,
        vec![result("a", Status::Active, Statistic::Median, 1000.0)],
    );
    let within = result("a", Status::Active, Statistic::Median, 1100.0);
    assert!(
        evaluate(&[within], Some(&baseline), Machine::Reference)
            .regressions
            .is_empty()
    );
    let over = result("a", Status::Active, Statistic::Median, 1101.0);
    let outcome = evaluate(&[over], Some(&baseline), Machine::Reference);
    assert!(outcome.failures.is_empty());
    assert_eq!(
        outcome.regressions,
        ["a: median 1.10 us is more than 10% slower than 1.00 us in the last passing run"]
    );
}

#[test]
fn a_benchmark_pending_in_the_baseline_is_not_compared() {
    let baseline = run(
        "h",
        true,
        vec![result("a", Status::Pending, Statistic::Median, 1.0)],
    );
    let now_active = result("a", Status::Active, Statistic::Median, 1000.0);
    assert!(
        evaluate(&[now_active], Some(&baseline), Machine::Reference)
            .regressions
            .is_empty()
    );
    let other = run(
        "h",
        true,
        vec![result("b", Status::Active, Statistic::Median, 1.0)],
    );
    let unrelated = result("a", Status::Active, Statistic::Median, 1000.0);
    assert!(
        evaluate(&[unrelated], Some(&other), Machine::Reference)
            .regressions
            .is_empty()
    );
}

#[test]
fn the_baseline_is_the_last_passing_run_on_the_same_host() {
    let history = History {
        version: HISTORY_VERSION,
        runs: vec![
            run(
                "h",
                true,
                vec![result("first", Status::Active, Statistic::Median, 1.0)],
            ),
            run("h", false, vec![]),
            run("other", true, vec![]),
        ],
    };
    let found = baseline(&history, "h").unwrap();
    assert_eq!(found.results[0].benchmark, "first");
    assert!(baseline(&history, "nobody").is_none());
}

#[test]
fn arguments_parse_and_bad_ones_are_rejected() {
    assert_eq!(
        parse_cli::<Args, &str>(&[]).unwrap(),
        Args {
            show: None,
            history: None,
            accept: false,
        }
    );
    assert_eq!(
        parse_cli::<Args, _>(&["--accept", "--history", "h.json"]).unwrap(),
        Args {
            show: None,
            history: Some(PathBuf::from("h.json")),
            accept: true,
        }
    );
    assert_eq!(
        parse_cli::<Args, _>(&["history"]).unwrap().show,
        Some(Show::History)
    );
    assert!(
        parse_cli::<Args, _>(&["--history"]).is_err(),
        "needs a path"
    );
    assert!(parse_cli::<Args, _>(&["--accept", "--accept"]).is_err());
    assert!(
        parse_cli::<Args, _>(&["history", "--accept"]).is_err(),
        "--accept only applies to a run"
    );
    assert!(parse_cli::<Args, _>(&["--fast"]).is_err());
    assert!(parse_cli::<Args, _>(&["history", "history"]).is_err());
    assert!(parse_cli::<Args, _>(&["--history", "a", "--history", "b"]).is_err());
}

#[test]
fn accept_lets_regressions_through_but_never_failures() {
    let regressed = Outcome {
        failures: vec![],
        regressions: vec!["r".to_owned()],
    };
    assert!(!regressed.passed(false));
    assert!(regressed.passed(true));
    let over = Outcome {
        failures: vec!["f".to_owned()],
        regressions: vec![],
    };
    assert!(!over.passed(false));
    assert!(!over.passed(true));
    assert!(Outcome::default().passed(false));
}

#[test]
fn problems_are_reported_with_annotations_in_github_actions() {
    let outcome = Outcome {
        failures: vec!["f".to_owned()],
        regressions: vec!["r".to_owned()],
    };
    assert_eq!(outcome.report(false, false), "FAIL  f\nFAIL  r\n");
    assert_eq!(
        outcome.report(false, true),
        "::error title=Performance budget::f\nFAIL  f\n::error title=Performance budget::r\nFAIL  r\n"
    );
    assert_eq!(
        outcome.report(true, true),
        "::error title=Performance budget::f\nFAIL  f\nACCEPTED  r\n"
    );
    assert_eq!(Outcome::default().report(false, true), "");
}

#[test]
fn a_failing_cargo_bench_is_an_error() {
    let missing = scratch("failing-cargo-bench").join("no-such-root");
    let error = run_benchmarks(&missing, &missing.join("criterion"), &missing).unwrap_err();
    assert!(error.contains("cannot run cargo bench"), "{error}");
}

#[test]
fn the_run_records_the_commit_and_time() {
    // A copy of the tree without .git (as cargo-mutants makes) has no commit.
    if root().join(".git").exists() {
        let commit = commit(&root()).unwrap();
        assert_eq!(commit.len(), 40, "{commit}");
        assert!(commit.chars().all(|c| c.is_ascii_hexdigit()), "{commit}");
    }
    let outside = scratch("outside-git");
    assert_eq!(commit(&outside.join("missing")), None);
    assert!(
        command_output("git", &["--version"])
            .unwrap()
            .starts_with("git version")
    );
    assert_eq!(command_output("git", &["no-such-subcommand"]), None);
    assert_eq!(command_output("no-such-program-xtask-bench", &[]), None);
    assert!(unix_time() > 1_700_000_000);
}

#[test]
fn the_cpu_model_is_read_from_the_os() {
    let cpuinfo = "processor\t: 0\nvendor_id\t: AuthenticAMD\nmodel name\t: AMD EPYC 7763\n\
                   processor\t: 1\nmodel name\t: other\n";
    assert_eq!(cpuinfo_model(cpuinfo).unwrap().trim(), "AMD EPYC 7763");
    assert_eq!(cpuinfo_model("processor\t: 0\nCPU part\t: 0xd0c\n"), None);
    let model = cpu_model();
    assert_eq!(model, cpu_model_on(std::env::consts::OS, read_cpu_source));
    // Each OS has its own source and not the others'. `PROCESSOR_IDENTIFIER` is an
    // environment variable anyone may set on any OS, so only its presence on Windows is the
    // OS's; which source each OS reads is `each_os_reads_its_own_cpu_source`'s.
    let os = std::env::consts::OS;
    assert_eq!(read_cpu_source(CpuSource::Sysctl).is_some(), os == "macos");
    assert_eq!(read_cpu_source(CpuSource::CpuInfo).is_some(), os == "linux");
    if os == "windows" {
        assert!(read_cpu_source(CpuSource::ProcessorIdentifier).is_some());
    }
    if let Some(model) = &model {
        assert!(!model.is_empty() && model.trim() == model, "{model:?}");
    }
    // Linux on ARM has no "model name" line in /proc/cpuinfo.
    if matches!(std::env::consts::OS, "macos" | "windows") {
        assert!(model.is_some());
    }
}

#[test]
fn each_os_reads_its_own_cpu_source() {
    let read = |source: CpuSource| {
        Some(match source {
            CpuSource::Sysctl => " Apple M2 \n".to_owned(),
            CpuSource::CpuInfo => "model name\t: AMD EPYC 7763\n".to_owned(),
            CpuSource::ProcessorIdentifier => "Intel64 Family 6".to_owned(),
        })
    };
    assert_eq!(cpu_model_on("macos", read).as_deref(), Some("Apple M2"));
    assert_eq!(
        cpu_model_on("linux", read).as_deref(),
        Some("AMD EPYC 7763")
    );
    assert_eq!(
        cpu_model_on("windows", read).as_deref(),
        Some("Intel64 Family 6")
    );
    assert_eq!(cpu_model_on("freebsd", read), None);
    assert_eq!(cpu_model_on("macos", |_| Some("  ".to_owned())), None);
    assert_eq!(cpu_model_on("windows", |_| None), None);
}

#[test]
fn only_a_true_github_actions_variable_means_github() {
    assert!(on_github(Some(std::ffi::OsStr::new("true"))));
    assert!(!on_github(Some(std::ffi::OsStr::new("false"))));
    assert!(!on_github(None));
}

#[test]
fn the_history_round_trips_and_a_missing_file_is_empty() {
    let dir = scratch("history");
    let path = dir.join("nested").join("history.json");
    let empty = load_history(&path).unwrap();
    assert_eq!(empty.version, HISTORY_VERSION);
    assert!(empty.runs.is_empty());
    let history = History {
        version: HISTORY_VERSION,
        runs: vec![run(
            "h",
            true,
            vec![result("a", Status::Active, Statistic::P95, 1.5)],
        )],
    };
    save_history(&path, &history).unwrap();
    let loaded = load_history(&path).unwrap();
    assert_eq!(loaded.runs.len(), 1);
    assert_eq!(loaded.runs[0].results[0].p95_ns, 1.5);
    assert_eq!(loaded.runs[0].results[0].statistic, Statistic::P95);
    assert!(!dir.join("nested").join("history.json.tmp").exists());
}

#[test]
fn a_bad_history_file_is_an_error() {
    let dir = scratch("bad-history");
    let path = dir.join("history.json");
    fs::write(&path, "not json").unwrap();
    assert!(load_history(&path).is_err());
    fs::write(&path, r#"{"version":2,"runs":[]}"#).unwrap();
    assert!(load_history(&path).unwrap_err().contains("version 2"));
    assert!(load_history(&dir).is_err());
}

#[test]
fn bench_history_prints_the_recorded_runs() {
    let dir = scratch("print");
    let path = dir.join("history.json");
    let path_arg = path.to_str().unwrap();
    assert!(
        main(
            &root(),
            &parse_cli::<Args, _>(&["history", "--history", path_arg]).unwrap()
        )
        .unwrap()
    );
    let mut first = run(
        "ref-mac",
        true,
        vec![result("keystroke", Status::Active, Statistic::P99, 2e6)],
    );
    first.unix_time = 1_700_000_000;
    first.commit = Some("0123456789abcdef".to_owned());
    let mut second = run("ref-mac", false, vec![]);
    second.accepted = true;
    let history = History {
        version: HISTORY_VERSION,
        runs: vec![first, second],
    };
    save_history(&path, &history).unwrap();
    assert!(
        main(
            &root(),
            &parse_cli::<Args, _>(&["history", "--history", path_arg]).unwrap()
        )
        .unwrap()
    );
    let printed = format_history(&history);
    assert!(
        printed.contains("2023-11-14T22:13:20Z 0123456789ab ref-mac passed"),
        "{printed}"
    );
    assert!(printed.contains("keystroke"), "{printed}");
    assert!(printed.contains("p99    2.00 ms"), "{printed}");
    assert!(
        printed.contains("1970-01-01T00:00:00Z unknown ref-mac FAILED"),
        "{printed}"
    );
    assert_eq!(
        format_history(&History::default()),
        "bench: no runs recorded\n"
    );
    assert!(parse_cli::<Args, _>(&["--bogus"]).is_err());
    let both = parse_cli::<Args, _>(&["--accept", "history"]).unwrap();
    assert!(
        main(&root(), &both)
            .unwrap_err()
            .contains("only applies to a run")
    );
}

#[test]
fn results_print_against_the_baseline() {
    let host = run("h", true, vec![]).host;
    let now = [result("a", Status::Active, Statistic::Median, 2e3)];
    let printed = format_results(&host, &now, None);
    assert!(printed.contains("on h"), "{printed}");
    assert!(printed.contains("last       none"), "{printed}");
    let baseline = run(
        "h",
        true,
        vec![result("a", Status::Active, Statistic::Median, 1e3)],
    );
    let printed = format_results(&host, &now, Some(&baseline));
    assert!(
        printed.contains("median    2.00 us  budget    1.00 ms  last    1.00 us"),
        "{printed}"
    );
}

#[test]
fn dates_format_in_utc() {
    assert_eq!(utc_date(0), "1970-01-01T00:00:00Z");
    assert_eq!(utc_date(951_782_400), "2000-02-29T00:00:00Z");
    assert_eq!(utc_date(1_700_000_000), "2023-11-14T22:13:20Z");
    assert_eq!(utc_date(4_107_542_399), "2100-02-28T23:59:59Z");
}

#[test]
fn nanoseconds_format_with_a_readable_unit() {
    assert_eq!(format_ns(12.0), "12.00 ns");
    assert_eq!(format_ns(1_500.0), "1.50 us");
    assert_eq!(format_ns(1_000.0), "1.00 us");
    assert_eq!(format_ns(1e6), "1.00 ms");
    assert_eq!(format_ns(1e9), "1.000 s");
    assert_eq!(format_ns(8_330_000.0), "8.33 ms");
    assert_eq!(format_ns(1.25e9), "1.250 s");
}

#[test]
fn the_default_host_key_names_the_os_arch_and_cpu() {
    assert_eq!(
        host_key(None, "macos", "aarch64", Some("Apple  M2 Pro")),
        "macos-aarch64-apple-m2-pro"
    );
    assert_eq!(
        host_key(Some(" ".to_owned()), "linux", "x86_64", None),
        "linux-x86_64-unknown-cpu"
    );
    assert_eq!(
        host_key(Some("ref-mac".to_owned()), "macos", "aarch64", None),
        "ref-mac"
    );
    let host = host();
    assert_eq!(host.os, std::env::consts::OS);
    assert!(!host.key.is_empty());
}

// No spec statement: a host that is no reference machine, a CI runner, is judged by the 10%
// regression rule alone; on a reference machine a result over its budget fails.
#[test]
fn only_a_reference_machine_judges_the_budget() {
    let over = result("keystroke", Status::Active, Statistic::P99, 2e7);
    assert_eq!(Machine::of("ref-mac"), Machine::Reference);
    assert_eq!(Machine::of("ref-linux"), Machine::Reference);
    assert_eq!(Machine::of("linux-x86_64-amd-epyc"), Machine::Other);
    assert_eq!(
        evaluate(std::slice::from_ref(&over), None, Machine::Reference)
            .failures
            .len(),
        1
    );
    assert!(evaluate(&[over], None, Machine::Other).failures.is_empty());
}

// No spec statement: a budget that needs a game is judged only by a run that played one; any
// other budget, and that one with a game, keeps its status
#[test]
fn a_game_budget_is_pending_without_a_game() {
    let row = |name: &str| Budget {
        name: name.to_owned(),
        budget_ns: 1e9,
        statistic: Statistic::P95,
        status: Status::Active,
    };
    assert_eq!(
        judged_here(&row("save_to_game"), |_| false).status,
        Status::Pending
    );
    assert_eq!(
        judged_here(&row("save_to_game"), |_| true).status,
        Status::Active
    );
    // Each route needs its own game: the mg1 hub the rerelease's.
    let only_game = |variable: &str| variable == GAME_DIR;
    assert_eq!(
        judged_here(&row("frame_e1m1"), only_game).status,
        Status::Active
    );
    assert_eq!(
        judged_here(&row("frame_mg1"), only_game).status,
        Status::Pending
    );
    assert_eq!(
        judged_here(&row("map_load_mg1"), |v: &str| v == RERELEASE_DIR).status,
        Status::Active
    );
    assert_eq!(
        judged_here(&row("keystroke"), |_| false).status,
        Status::Active
    );
}
