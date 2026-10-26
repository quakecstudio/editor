// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use super::*;
use crate::test_dirs::fresh;

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
    let dir = fresh(&format!("ratchet/{name}"));
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

fn git_in(dir: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=test", "-c", "user.email=test@example.com"])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn a_version_git_cannot_read_is_an_error() {
    let dir = repository("unreadable", &[BASE]);
    let blob = git_in(&dir, &["rev-parse", &format!("HEAD:{FILE}")]);
    let blob = blob.trim();
    let (fan, rest) = blob.split_at_checked(2).unwrap();
    let object = dir.join(".git/objects").join(fan).join(rest);
    fs::remove_file(&object).unwrap();
    let error = versions(&dir).unwrap_err();
    assert!(error.contains("git show"), "{error}");
}

#[test]
fn a_commit_that_deleted_the_file_is_a_version_without_it() {
    let dir = repository("deleted", &[BASE]);
    git_in(&dir, &["rm", "--quiet", FILE]);
    git_in(&dir, &["commit", "--quiet", "-m", "delete"]);
    let versions = versions(&dir).unwrap();
    let texts: Vec<bool> = versions.iter().map(|(_, text)| text.is_some()).collect();
    assert_eq!(texts, [true, false], "{versions:?}");
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
fn a_table_holds_a_review_s_four_minimums_or_only_exceptions() {
    let problems = |text: &str| validate(&parse(text).unwrap(), &keys());
    let readiness_alone = problems("[crates.qs-x]\nreadiness = \"P1-R0\"\n");
    assert_eq!(readiness_alone.len(), 2, "{readiness_alone:?}");
    assert!(readiness_alone[0].contains("a readiness issue but no minimums"));
    assert!(readiness_alone[1].contains("needs the minimums of a review, an exception"));
    let empty = problems("[crates.qs-x]\n");
    assert_eq!(empty.len(), 1, "{empty:?}");
    let partial = problems("[crates.qs-x]\nreadiness = \"P1-R0\"\nlines = 95\n");
    assert_eq!(partial.len(), 3, "{partial:?}");
    assert!(
        partial.iter().all(|p| p.contains("has no minimum")),
        "{partial:?}"
    );
    let unnamed = problems(&BASE.replace("readiness = \"P1-R0\"\n", ""));
    assert_eq!(unnamed.len(), 1, "{unnamed:?}");
    assert!(
        unnamed[0].contains("minimums need the readiness issue"),
        "{unnamed:?}"
    );
    let exceptions_only =
        "[crates.qs-c]\nreason = \"no tool\"\nexceptions = { mutation = \"C\" }\n";
    assert_eq!(problems(exceptions_only), Vec::<String>::new());
    // A crate with no review may hold only a justified survivor.
    let survivor_only = "[crates.qs-b]\nreason = \"entry point\"\n\
                         equivalent_mutants = { \"src/main.rs: replace main -> () with ()\" = \"unkillable\" }\n";
    assert_eq!(problems(survivor_only), Vec::<String>::new());
}

#[test]
fn an_exception_with_no_minimum_passes_unmeasured() {
    let ratchets =
        parse("[crates.qs-c]\nreason = \"no tool\"\nexceptions = { mutation = \"C\" }\n").unwrap();
    let mut measured = passing("qs-c");
    measured.retain(|m| m.check != Check::Mutation);
    let mut report = Report::default();
    for (name, checks) in &group(&measured, &mut report) {
        evaluate(name, checks, ratchets.crates.get(name), &mut report);
    }
    assert_eq!(report.problems, Vec::<String>::new());
    assert!(
        report
            .passed
            .iter()
            .any(|line| line == "qs-c mutation: not measured, waived by an exception: C"),
        "{:?}",
        report.passed
    );
    let mut report = Report::default();
    let held = parse(&edit(
        "mutation = 92",
        "mutation = 92\nexceptions = { mutation = \"C\" }",
    ))
    .unwrap();
    let mut unmeasured = passing("qs-x");
    unmeasured.retain(|m| m.check != Check::Mutation);
    for (name, checks) in &group(&unmeasured, &mut report) {
        evaluate(name, checks, held.crates.get(name), &mut report);
    }
    assert_eq!(
        report.problems,
        ["qs-x mutation: not measured"],
        "a minimum still needs a measurement"
    );
}

#[test]
fn a_table_of_exceptions_changes_with_a_reason_and_gains_a_review() {
    let before =
        parse("[crates.qs-c]\nreason = \"no tool\"\nexceptions = { mutation = \"C\" }\n").unwrap();
    let wider = parse(
        "[crates.qs-c]\nreason = \"no tool\"\nexceptions = { mutation = \"C\", branches = \"b\" }\n",
    )
    .unwrap();
    let problems = check_change(&before, &wider, &members());
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("without a new reason"), "{problems:?}");
    let reasoned = parse(
        "[crates.qs-c]\nreason = \"branch tool\"\nexceptions = { mutation = \"C\", branches = \"b\" }\n",
    )
    .unwrap();
    assert_eq!(
        check_change(&before, &reasoned, &members()),
        Vec::<String>::new()
    );
    let reviewed = parse(
        "[crates.qs-c]\nreadiness = \"P1-R1\"\nlines = 93\nbranches = 85\nmutation = 0\nspec = 100\n\
         reason = \"no tool\"\nexceptions = { mutation = \"C\" }\n",
    )
    .unwrap();
    assert_eq!(
        check_change(&before, &reviewed, &members()),
        Vec::<String>::new()
    );
    assert_eq!(validate(&reviewed, &keys()), Vec::<String>::new());
}

#[test]
fn the_file_rejects_unknown_fields_and_checks() {
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
        shard: None,
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
    // A directory that does not exist yet: recording makes it.
    let path = fresh("ratchet-history/appends").join("new/history.json");
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
    let dir = fresh("ratchet-history/unreadable");
    let path = dir.join("history.json");
    // Not UTF-8: reading fails, and the run must not start a new history over it.
    fs::write(&path, [0xff, 0xfe]).unwrap();
    let error = record(&root(), &path, true, &Measured::new()).unwrap_err();
    assert!(error.contains("history.json"), "{error}");
    assert_eq!(fs::read(&path).unwrap(), [0xff, 0xfe]);
}

fn shard(index: u64, count: u64, name: &str, score: Score) -> Measurement {
    Measurement {
        shard: Some(Shard::new(index, count).unwrap()),
        ..measurement(Check::Mutation, "linux", name, score)
    }
}

fn mutation_of(measurements: &[Measurement], report: &mut Report) -> Option<Score> {
    group(measurements, report)
        .get("qs-x")
        .and_then(|checks| checks.get(&Check::Mutation))
        .and_then(|by_os| by_os.get("linux"))
        .cloned()
}

#[test]
fn the_shards_of_a_crates_mutants_add_up_to_one_score() {
    let survivor = |name: &str| Score {
        passed: 0,
        total: 1,
        failures: vec![name.to_owned()],
    };
    // Out of order, with another crate's shard and another OS in between.
    let shards = [
        shard(2, 3, "qs-x", survivor("src/c.rs: replace c with 0")),
        shard(0, 1, "qs-y", score(5, 5)),
        shard(0, 3, "qs-x", score(10, 12)),
        Measurement {
            os: "macos".to_owned(),
            ..shard(0, 1, "qs-x", score(1, 1))
        },
        shard(1, 3, "qs-x", survivor("src/a.rs: replace a with 0")),
    ];
    let mut report = Report::default();
    let merged = mutation_of(&shards, &mut report).unwrap();
    assert_eq!(report.problems, Vec::<String>::new());
    assert_eq!((merged.passed, merged.total), (10, 14));
    assert_eq!(
        merged.failures,
        ["src/a.rs: replace a with 0", "src/c.rs: replace c with 0"]
    );
    // One shard of one is the whole measurement.
    let mut report = Report::default();
    let one = mutation_of(&[shard(0, 1, "qs-x", score(3, 4))], &mut report).unwrap();
    assert_eq!((one.passed, one.total), (3, 4));
    assert_eq!(report.problems, Vec::<String>::new());
}

#[test]
fn a_shard_missing_repeated_mixed_or_beside_a_whole_measurement_is_a_problem() {
    let cases: [(&str, Vec<Measurement>); 6] = [
        (
            "qs-x mutation on linux shard 1/3 is missing",
            vec![
                shard(0, 3, "qs-x", score(1, 1)),
                shard(2, 3, "qs-x", score(1, 1)),
            ],
        ),
        (
            "qs-x mutation on linux shard 2/3 is missing",
            vec![
                shard(0, 3, "qs-x", score(1, 1)),
                shard(1, 3, "qs-x", score(1, 1)),
            ],
        ),
        (
            "qs-x mutation on linux shard 1/2 is measured twice",
            vec![
                shard(0, 2, "qs-x", score(1, 1)),
                shard(1, 2, "qs-x", score(1, 1)),
                shard(1, 2, "qs-x", score(1, 1)),
            ],
        ),
        (
            "qs-x mutation on linux is measured in 2 shards and in 3 shards",
            vec![
                shard(0, 2, "qs-x", score(1, 1)),
                shard(1, 2, "qs-x", score(1, 1)),
                shard(2, 3, "qs-x", score(1, 1)),
            ],
        ),
        (
            "qs-x mutation on linux is measured twice",
            vec![
                shard(0, 1, "qs-x", score(1, 1)),
                measurement(Check::Mutation, "linux", "qs-x", score(1, 1)),
            ],
        ),
        (
            "qs-x lines on linux is measured in shards, which only the mutation check has",
            vec![Measurement {
                check: Check::Lines,
                ..shard(0, 1, "qs-x", score(1, 1))
            }],
        ),
    ];
    for (problem, measurements) in cases {
        let mut report = Report::default();
        assert_eq!(mutation_of(&measurements, &mut report), None, "{problem}");
        assert_eq!(report.problems, [problem]);
    }
}
