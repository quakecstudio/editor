// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use super::*;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

/// One crate, `qs-x`, at or above the bar, set by readiness issue P1-R0.
const BASE: &str = r#"
[crates.qs-x]
readiness = "P1-R0"
lines = 95
branches = 85
mutation = 92
spec = 100
"#;

fn keys() -> BTreeSet<String> {
    ["P1-R0", "P1-R1", "P2-R1"].map(str::to_owned).into()
}

fn members() -> BTreeSet<String> {
    ["qs-x"].map(str::to_owned).into()
}

fn edit(from: &str, to: &str) -> String {
    assert!(BASE.contains(from), "{from}");
    BASE.replace(from, to)
}

fn change(new: &str) -> Vec<String> {
    check_change(&parse(BASE).unwrap(), &parse(new).unwrap(), &members())
}

fn score(passed: u64, total: u64) -> Score {
    Score {
        passed,
        total,
        failures: Vec::new(),
    }
}

// The acceptance test of P0-24: lowering a committed ratchet value without a
// readiness issue reference fails.

#[test]
fn lowering_a_minimum_without_a_new_readiness_issue_fails() {
    let problems = change(&edit("lines = 95", "lines = 91\nreason = \"slower code\""));
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].contains("without naming the readiness issue"),
        "{problems:?}"
    );
}

#[test]
fn lowering_a_minimum_without_a_reason_fails() {
    let problems = change(&edit("lines = 95", "lines = 91").replace("\"P1-R0\"", "\"P2-R1\""));
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].contains("lowered lines without a new reason"),
        "{problems:?}"
    );
}

#[test]
fn lowering_a_minimum_with_a_new_readiness_issue_and_reason_passes() {
    let new = edit(
        "lines = 95",
        "lines = 91\nreason = \"generated code moved in\"",
    )
    .replace("\"P1-R0\"", "\"P2-R1\"");
    assert_eq!(change(&new), Vec::<String>::new());
}

#[test]
fn an_old_reason_does_not_cover_a_new_lowering() {
    let old = edit("lines = 95", "lines = 93\nreason = \"first\"");
    let new =
        edit("lines = 95", "lines = 92\nreason = \"first\"").replace("\"P1-R0\"", "\"P2-R1\"");
    let problems = check_change(&parse(&old).unwrap(), &parse(&new).unwrap(), &members());
    assert_eq!(problems.len(), 1, "{problems:?}");
}

#[test]
fn raising_a_minimum_needs_a_readiness_issue_too() {
    let problems = change(&edit("lines = 95", "lines = 97"));
    assert_eq!(problems.len(), 1, "{problems:?}");
    let raised = edit("lines = 95", "lines = 97").replace("\"P1-R0\"", "\"P2-R1\"");
    assert_eq!(change(&raised), Vec::<String>::new());
}

#[test]
fn adding_an_exception_or_equivalent_mutant_is_a_lowering() {
    let new = edit(
        "spec = 100",
        "spec = 100\nexceptions = { lines = \"UI drawing\" }",
    )
    .replace("\"P1-R0\"", "\"P2-R1\"");
    let problems = change(&new);
    assert!(problems[0].contains("added an exception"), "{problems:?}");
    let new = edit(
        "spec = 100",
        "spec = 100\nequivalent_mutants = { \"src/lib.rs: replace f -> u8 with 0\" = \"unused\" }",
    )
    .replace("\"P1-R0\"", "\"P2-R1\"");
    assert_eq!(change(&new).len(), 1);
}

#[test]
fn an_unchanged_file_or_a_new_crate_passes() {
    assert_eq!(change(BASE), Vec::<String>::new());
    let new = format!(
        "{BASE}\n[crates.qs-y]\nreadiness = \"P1-R1\"\nlines = 90\nbranches = 80\nmutation = 90\nspec = 100\n"
    );
    assert_eq!(change(&new), Vec::<String>::new());
}

#[test]
fn a_ratchet_goes_away_only_with_its_crate() {
    let problems = change("");
    assert!(
        problems[0].contains("removed while the crate still exists"),
        "{problems:?}"
    );
    let gone = check_change(
        &parse(BASE).unwrap(),
        &Ratchets::default(),
        &BTreeSet::new(),
    );
    assert_eq!(gone, Vec::<String>::new());
}

/// A scratch git repository holding `versions` of `ratchets.toml`, one commit each.
fn repository(name: &str, versions: &[&str]) -> PathBuf {
    let dir = root()
        .join("target")
        .join("xtask-tests")
        .join("ratchet")
        .join(name);
    if dir.exists() {
        fs::remove_dir_all(&dir).unwrap();
    }
    fs::create_dir_all(&dir).unwrap();
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["-c", "user.name=test", "-c", "user.email=test@example.com"])
            .args(["-c", "commit.gpgsign=false", "-c", "core.autocrlf=false"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "--quiet"]);
    for (number, text) in versions.iter().enumerate() {
        fs::write(dir.join(FILE), text).unwrap();
        git(&["add", FILE]);
        git(&["commit", "--quiet", "-m", &format!("version {number}")]);
    }
    dir
}

#[test]
fn a_lowering_committed_in_git_history_fails() {
    let lowered = edit("lines = 95", "lines = 91\nreason = \"why\"");
    let dir = repository("lowered", &[BASE, &lowered]);
    let versions = versions(&dir).unwrap();
    assert_eq!(versions.len(), 2, "{versions:?}");
    let problems = history_problems(&versions, &members());
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].starts_with("ratchets.toml at "), "{problems:?}");
}

#[test]
fn an_uncommitted_lowering_fails_too() {
    let dir = repository("uncommitted", &[BASE]);
    fs::write(dir.join(FILE), edit("mutation = 92", "mutation = 90")).unwrap();
    let versions = versions(&dir).unwrap();
    assert_eq!(versions.last().unwrap().0, "working tree");
    let problems = history_problems(&versions, &members());
    assert_eq!(
        problems.len(),
        2,
        "same readiness key and no reason: {problems:?}"
    );
    assert!(problems[0].contains("at working tree"), "{problems:?}");
}

#[test]
fn a_history_of_proper_changes_passes() {
    let raised = edit("lines = 95", "lines = 97").replace("\"P1-R0\"", "\"P2-R1\"");
    let dir = repository("proper", &["# empty\n", BASE, &raised]);
    let versions = versions(&dir).unwrap();
    assert_eq!(versions.len(), 3);
    assert_eq!(
        history_problems(&versions, &members()),
        Vec::<String>::new()
    );
}

#[test]
fn an_unreadable_version_is_reported_and_skipped() {
    let versions = vec![
        ("a".to_owned(), Some(BASE.to_owned())),
        (
            "b".to_owned(),
            Some("[crates.qs-x]\nlines = \"high\"\n".to_owned()),
        ),
        ("c".to_owned(), Some(BASE.to_owned())),
        ("d".to_owned(), None),
    ];
    let problems = history_problems(&versions, &BTreeSet::new());
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].starts_with("b: "), "{problems:?}");
}

#[test]
fn the_file_is_checked_against_the_plan_and_the_bar() {
    assert_eq!(
        validate(&parse(BASE).unwrap(), &keys()),
        Vec::<String>::new()
    );
    let bad = parse(
        r#"
[crates.qs-x]
readiness = "P9-R9"
lines = 101
branches = 70
mutation = 92
spec = 100
reason = " "
exceptions = { flakes = "slow" }
"#,
    )
    .unwrap();
    let problems = validate(&bad, &keys());
    let expected = [
        "readiness \"P9-R9\" is not a readiness issue",
        "lines is 101, over 100",
        "branches minimum 70 is below the bar of 80% without an exception",
        "the flake check takes no exception",
        "every reason must say something",
    ];
    assert_eq!(problems.len(), expected.len(), "{problems:?}");
    for (problem, expected) in problems.iter().zip(expected) {
        assert!(problem.contains(expected), "{problem} / {expected}");
    }
    let excepted = parse(&edit(
        "branches = 85",
        "branches = 70\nexceptions = { branches = \"rendering code\" }",
    ))
    .unwrap();
    assert_eq!(validate(&excepted, &keys()), Vec::<String>::new());
}

#[test]
fn the_file_rejects_unknown_fields_and_checks() {
    assert!(parse("[crates.qs-x]\nreadiness = \"P1-R0\"\n").is_err());
    assert!(parse(&format!("{BASE}regions = 90\n")).is_err());
    assert!(
        parse(&edit(
            "spec = 100",
            "spec = 100\nexceptions = { regions = \"x\" }"
        ))
        .is_err()
    );
    assert!(parse("[other]\n").is_err());
    assert_eq!(parse("").unwrap(), Ratchets::default());
}

#[test]
fn the_committed_file_and_plan_are_readable() {
    let text = fs::read_to_string(root().join(FILE)).unwrap();
    parse(&text).unwrap();
    let keys = readiness_keys(&fs::read_to_string(root().join(READINESS_PLAN)).unwrap());
    assert!(keys.contains("P1-R0"), "{keys:?}");
    assert!(keys.iter().all(|key| key.contains("-R")), "{keys:?}");
    assert!(!keys.contains("P0-24"));
    let crates = layer_crates(&root()).unwrap();
    assert!(crates.contains("xtask"));
}

#[test]
fn the_bar_applies_unless_waived_and_a_minimum_above_it_applies_too() {
    let base = parse(BASE).unwrap();
    let qs_x = &base.crates["qs-x"];
    assert!(judge(Check::Lines, &score(90, 100), None).0);
    assert!(!judge(Check::Lines, &score(89, 100), None).0);
    assert!(
        !judge(Check::Lines, &score(94, 100), Some(qs_x)).0,
        "minimum 95"
    );
    assert!(judge(Check::Lines, &score(95, 100), Some(qs_x)).0);
    assert!(judge(Check::Branches, &score(80, 100), None).0);
    assert!(!judge(Check::Spec, &score(99, 100), None).0);

    let waived = parse(&edit(
        "branches = 85",
        "branches = 70\nexceptions = { branches = \"rendering code\" }",
    ))
    .unwrap();
    let waived = &waived.crates["qs-x"];
    let (ok, line) = judge(Check::Branches, &score(75, 100), Some(waived));
    assert!(ok, "{line}");
    assert!(line.contains("bar waived"), "{line}");
    assert!(
        !judge(Check::Branches, &score(69, 100), Some(waived)).0,
        "minimum 70"
    );
}

#[test]
fn flakes_need_every_run_and_enough_runs() {
    assert!(judge(Check::Flakes, &score(20, 20), None).0);
    let (ok, line) = judge(Check::Flakes, &score(5, 5), None);
    assert!(!ok);
    assert!(line.contains("only 5 runs"), "{line}");
    let failing = Score {
        passed: 19,
        total: 20,
        failures: vec!["run 4 of 20 failed".to_owned()],
    };
    let (ok, line) = judge(Check::Flakes, &failing, None);
    assert!(!ok);
    assert!(line.contains("run 4 of 20 failed"), "{line}");
}

#[test]
fn a_listed_equivalent_mutant_is_not_a_survivor() {
    let survivor = "src/lib.rs: replace f -> u8 with 0";
    let listed = parse(&edit(
        "spec = 100",
        &format!("spec = 100\nequivalent_mutants = {{ \"{survivor}\" = \"f is unused\" }}"),
    ))
    .unwrap();
    let score = Score {
        passed: 19,
        total: 20,
        failures: vec![survivor.to_owned()],
    };
    assert!(!judge(Check::Mutation, &score, None).0);
    assert!(judge(Check::Mutation, &score, listed.crates.get("qs-x")).0);
}

fn measurement(check: Check, os: &str, name: &str, score: Score) -> Measurement {
    Measurement {
        check,
        os: os.to_owned(),
        crates: [(name.to_owned(), score)].into(),
    }
}

fn passing(name: &str) -> Vec<Measurement> {
    let mut all = Vec::new();
    for check in RATCHETED {
        let passed = if check == Check::Spec { 100 } else { 97 };
        all.push(measurement(check, "linux", name, score(passed, 100)));
    }
    for os in FLAKE_OSES {
        all.push(measurement(Check::Flakes, os, name, score(20, 20)));
    }
    all
}

#[test]
fn a_crate_passes_only_with_every_check_measured() {
    let mut report = Report::default();
    let measured = group(&passing("qs-x"), &mut report);
    for (name, checks) in &measured {
        evaluate(name, checks, None, &mut report);
    }
    assert_eq!(report.problems, Vec::<String>::new());
    assert_eq!(report.passed.len(), 7);
    assert!(proposed_minimums(&measured).contains("[crates.qs-x]\n"));
    assert!(proposed_minimums(&measured).contains("lines = 97\n"));

    let mut partial = passing("qs-x");
    partial.retain(|m| m.check != Check::Spec && m.os != "windows");
    let mut report = Report::default();
    for (name, checks) in &group(&partial, &mut report) {
        evaluate(name, checks, None, &mut report);
    }
    assert_eq!(
        report.problems,
        [
            "qs-x spec: not measured",
            "qs-x flakes on windows: not measured"
        ]
    );
}

#[test]
fn a_crate_measured_twice_is_a_problem() {
    let mut twice = passing("qs-x");
    twice.push(measurement(Check::Lines, "linux", "qs-x", score(1, 1)));
    let mut report = Report::default();
    group(&twice, &mut report);
    assert_eq!(report.problems, ["qs-x lines on linux is measured twice"]);
}

#[test]
fn the_history_appends_one_run_each_time() {
    let dir = root()
        .join("target")
        .join("xtask-tests")
        .join("ratchet-history");
    if dir.exists() {
        fs::remove_dir_all(&dir).unwrap();
    }
    let path = dir.join("history.json");
    let mut report = Report::default();
    let measured = group(&passing("qs-x"), &mut report);
    record(&root(), &path, true, &measured).unwrap();
    record(&root(), &path, false, &measured).unwrap();
    let history: History = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(history.runs.len(), 2);
    assert!(history.runs[0].passed);
    assert!(!history.runs[1].passed);
    assert_eq!(history.runs[1].crates, measured);
    fs::write(&path, "{\"version\":99,\"runs\":[]}").unwrap();
    assert!(record(&root(), &path, true, &measured).is_err());
    fs::write(&path, "[").unwrap();
    assert!(record(&root(), &path, true, &measured).is_err());
}

#[test]
fn the_command_line_takes_a_history_and_measurement_files() {
    let args = |text: &str| -> Vec<String> { text.split_whitespace().map(str::to_owned).collect() };
    assert_eq!(
        parse_args(&args("--history h.json a.json b.json")).unwrap(),
        (
            Some(PathBuf::from("h.json")),
            vec![PathBuf::from("a.json"), PathBuf::from("b.json")]
        )
    );
    assert_eq!(parse_args(&[]).unwrap(), (None, Vec::new()));
    assert!(parse_args(&args("--history")).is_err());
    assert!(parse_args(&args("--fast")).is_err());
}

/// A scratch repository with the files `main` reads and `ratchets` committed
/// as `ratchets.toml`.
fn checkout(name: &str, ratchets: &str) -> PathBuf {
    let dir = repository(name, &[ratchets]);
    fs::create_dir_all(dir.join("plan/backlog")).unwrap();
    fs::write(
        dir.join(READINESS_PLAN),
        "groups:\n- issues:\n  - key: P1-R0\n",
    )
    .unwrap();
    fs::write(dir.join(LAYERS), "[crates]\nqs-x = \"L0\"\n").unwrap();
    dir
}

#[test]
fn the_command_passes_a_clean_file_and_fails_a_low_measurement() {
    let dir = checkout("main-clean", BASE);
    assert!(main(&dir, &[]).unwrap());

    let flakes = root().join("xtask/fixtures/readiness/flakes/recorded/flakes.json");
    let history = dir.join("history.json");
    let args = [
        "--history".to_owned(),
        history.display().to_string(),
        flakes.display().to_string(),
    ];
    assert!(!main(&dir, &args).unwrap());
    let history: History = serde_json::from_str(&fs::read_to_string(&history).unwrap()).unwrap();
    assert!(!history.runs[0].passed);
    assert!(history.runs[0].crates.contains_key("fx-flakes"));

    assert!(main(&dir, &["missing.json".to_owned()]).is_err());
}

#[test]
fn the_command_fails_a_lowered_file() {
    let dir = checkout("main-lowered", BASE);
    fs::write(dir.join(FILE), edit("lines = 95", "lines = 94")).unwrap();
    assert!(!main(&dir, &[]).unwrap());
}

#[test]
fn a_history_path_that_cannot_be_read_is_an_error() {
    let dir = root()
        .join("target")
        .join("xtask-tests")
        .join("ratchet-history-unreadable");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("history.json");
    // Not UTF-8: reading fails, and the run must not start a new history over it.
    fs::write(&path, [0xff, 0xfe]).unwrap();
    let error = record(&root(), &path, true, &Measured::new()).unwrap_err();
    assert!(error.contains("history.json"), "{error}");
    assert_eq!(fs::read(&path).unwrap(), [0xff, 0xfe]);
}
