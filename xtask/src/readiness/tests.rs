// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use super::*;
use crate::ratchet::judge;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

const FIXTURES: &str = "xtask/fixtures/readiness";

fn recorded(check: &str, file: &str) -> String {
    fs::read_to_string(
        root()
            .join(FIXTURES)
            .join(check)
            .join("recorded")
            .join(file),
    )
    .unwrap()
}

fn score(passed: u64, total: u64) -> Score {
    Score {
        passed,
        total,
        failures: Vec::new(),
    }
}

// The acceptance test of P0-24: a fixture crate below each threshold fails the
// matching check. The tool output was recorded from the fixture by the
// commands in xtask/fixtures/readiness/README.md; the spec fixture is measured
// live because that check needs no tool.

#[test]
fn the_lines_fixture_fails_the_line_check() {
    let score = parse_llvm_cov(&recorded("lines", "llvm-cov.json"), Check::Lines).unwrap();
    assert_eq!(score, self::score(6, 18));
    let (ok, line) = judge(Check::Lines, &score, None);
    assert!(!ok, "{line}");
    assert!(line.contains("33.3% (6 of 18), needs 90%"), "{line}");
}

#[test]
fn the_branches_fixture_fails_the_branch_check_only() {
    let text = recorded("branches", "llvm-cov.json");
    let branches = parse_llvm_cov(&text, Check::Branches).unwrap();
    assert_eq!(branches, score(2, 4));
    let (ok, line) = judge(Check::Branches, &branches, None);
    assert!(!ok, "{line}");
    let lines = parse_llvm_cov(&text, Check::Lines).unwrap();
    assert!(judge(Check::Lines, &lines, None).0, "every line runs");
}

#[test]
fn the_mutation_fixture_fails_the_mutation_check() {
    let score = parse_mutants(&recorded("mutation", "outcomes.json")).unwrap();
    assert_eq!(score.passed, 0);
    assert_eq!(score.total, 2);
    assert_eq!(
        score.failures,
        [
            "src/lib.rs: replace add -> u32 with 0",
            "src/lib.rs: replace add -> u32 with 1"
        ]
    );
    let (ok, line) = judge(Check::Mutation, &score, None);
    assert!(!ok, "{line}");
    assert!(
        line.contains("survivor not killed or listed as equivalent"),
        "{line}"
    );
}

#[test]
fn the_spec_fixture_fails_the_spec_check() {
    let dir = root().join(FIXTURES).join("spec/crates/fx-spec");
    let score = spec(&dir).unwrap();
    assert_eq!(score.passed, 1);
    assert_eq!(
        score.total, 2,
        "FX-1.3 is withdrawn and FX-2 is not the crate's"
    );
    assert_eq!(score.failures, ["FX-1.2 is not cited"]);
    let (ok, line) = judge(Check::Spec, &score, None);
    assert!(!ok, "{line}");
    assert!(line.contains("FX-1.2 is not cited"), "{line}");
}

#[test]
fn the_flakes_fixture_fails_the_flake_check() {
    let measurement: Measurement =
        serde_json::from_str(&recorded("flakes", "flakes.json")).unwrap();
    assert_eq!(measurement.check, Check::Flakes);
    let score = &measurement.crates["fx-flakes"];
    assert_eq!(score.total, FLAKE_RUNS);
    assert!(score.passed < score.total);
    let (ok, line) = judge(Check::Flakes, score, None);
    assert!(!ok, "{line}");
}

#[test]
fn every_fixture_is_a_standalone_crate_with_a_readme() {
    for dir in [
        "lines",
        "branches",
        "mutation",
        "flakes",
        "spec/crates/fx-spec",
    ] {
        let path = format!("{FIXTURES}/{dir}");
        let target = resolve(&root(), &path).unwrap();
        assert_eq!(
            target.kind,
            Kind::Standalone,
            "{path} must be its own workspace"
        );
        assert!(target.name.starts_with("fx-"), "{}", target.name);
        assert!(
            target.dir.join("Cargo.lock").is_file(),
            "{path}: --locked needs Cargo.lock"
        );
        let readme = fs::read_to_string(target.dir.join("README.md")).unwrap();
        assert!(spec_sections(&readme).is_some(), "{path}");
    }
}

#[test]
fn a_real_crate_is_measured_for_spec_citations() {
    let target = resolve(&root(), "qs-porting-example").unwrap();
    assert_eq!(target.kind, Kind::Member);
    let score = spec(&target.dir).unwrap();
    assert_eq!(score, self::score(7, 7));
}

#[test]
fn the_workflow_installs_the_pinned_nightly() {
    let workflow = fs::read_to_string(root().join(".github/workflows/readiness.yml")).unwrap();
    assert!(
        workflow.contains(&format!("NIGHTLY: {NIGHTLY}\n")),
        "the Readiness workflow must pin NIGHTLY: {NIGHTLY}"
    );
    let date = NIGHTLY.strip_prefix("nightly-").unwrap();
    assert_eq!(date.len(), 10);
    assert!(
        date.chars().all(|c| c.is_ascii_digit() || c == '-'),
        "{date}"
    );
}

#[test]
fn checks_have_names_and_the_adr_0009_thresholds() {
    for check in Check::ALL {
        assert_eq!(Check::parse(check.name()), Some(check));
    }
    assert_eq!(Check::parse("regions"), None);
    let thresholds: Vec<u64> = Check::ALL.iter().map(|check| check.threshold()).collect();
    assert_eq!(thresholds, [90, 80, 90, 100, 100]);
}

#[test]
fn llvm_cov_reports_that_cannot_be_read_are_errors() {
    assert!(parse_llvm_cov("not json", Check::Lines).is_err());
    assert!(parse_llvm_cov(r#"{"data":[]}"#, Check::Lines).is_err());
    let lines = r#"{"data":[{"totals":{"lines":{"count":4,"covered":5}}}]}"#;
    assert!(
        parse_llvm_cov(lines, Check::Lines)
            .unwrap_err()
            .contains("5 of 4")
    );
    let lines = r#"{"data":[{"totals":{"lines":{"count":4}}}]}"#;
    assert!(parse_llvm_cov(lines, Check::Lines).is_err());
    let lines = r#"{"data":[{"totals":{"lines":{"count":4,"covered":3}}}]}"#;
    assert_eq!(parse_llvm_cov(lines, Check::Lines).unwrap(), score(3, 4));
    assert!(
        parse_llvm_cov(lines, Check::Branches).is_err(),
        "no branch totals"
    );
    for check in [Check::Mutation, Check::Spec, Check::Flakes] {
        assert!(parse_llvm_cov(lines, check).is_err());
    }
}

fn outcome(name: &str, summary: &str) -> String {
    format!(r#"{{"scenario":{{"Mutant":{{"name":"{name}"}}}},"summary":"{summary}"}}"#)
}

#[test]
fn timeouts_count_as_caught_and_unviable_mutants_do_not_count() {
    let text = format!(
        r#"{{"outcomes":[{{"scenario":"Baseline","summary":"Success"}},{},{},{},{}]}}"#,
        outcome("a.rs:1:1: one", "CaughtMutant"),
        outcome("a.rs:2:1: two", "Timeout"),
        outcome("a.rs:3:1: three", "Unviable"),
        outcome("a.rs:4:1: four", "MissedMutant"),
    );
    let score = parse_mutants(&text).unwrap();
    assert_eq!(score.passed, 2);
    assert_eq!(score.total, 3);
    assert_eq!(score.failures, ["a.rs: four"]);
}

#[test]
fn mutants_outcomes_that_cannot_be_read_are_errors() {
    assert!(parse_mutants("[").is_err());
    assert!(parse_mutants("{}").is_err());
    assert!(parse_mutants(r#"{"outcomes":[{"scenario":"Baseline"}]}"#).is_err());
    let text = format!(r#"{{"outcomes":[{}]}}"#, outcome("a.rs:1:1: x", "Exploded"));
    assert!(parse_mutants(&text).unwrap_err().contains("Exploded"));
}

#[test]
fn a_mutant_key_drops_the_line_and_column_only() {
    assert_eq!(
        mutant_key("crates/x/src/lib.rs:24:5: replace f -> u8 with 0"),
        "crates/x/src/lib.rs: replace f -> u8 with 0"
    );
    assert_eq!(mutant_key("src/lib.rs:7: delete !"), "src/lib.rs: delete !");
    assert_eq!(mutant_key("no separator"), "no separator");
    assert_eq!(mutant_key("src/a:b.rs: x"), "src/a:b.rs: x");
}

#[test]
fn spec_sections_come_from_the_readme_line() {
    let readme = "# x\n\n- **Layer:** L1.\n- **Spec sections:** [VM-12](../vm.md#vm-12-x)\n  \
                  (statements VM-12.1 to VM-12.3), VM-13 and VM-12.\n- **Shipped:** no; LIMITS-1.\n";
    assert_eq!(spec_sections(readme).unwrap(), ["VM-12", "VM-13"]);
    assert_eq!(
        spec_sections("- **Spec sections:** none.\n").unwrap(),
        Vec::<String>::new()
    );
    assert_eq!(spec_sections("# x\n\nNo sections here.\n"), None);
    assert!(!is_section_id("vm-12"));
    assert!(!is_section_id("VM-"));
    assert!(!is_section_id("VM12"));
    assert!(!is_section_id("1VM-2"));
    assert!(is_section_id("VM2-12"));
}

#[test]
fn citations_are_comment_lines_naming_statements() {
    let text =
        "    // spec VM-1.1, VM-1.2: what it does\n// spec VM-2.1\nlet x = 1; // spec VM-3.1\n";
    assert_eq!(citations(text), ["VM-1.1", "VM-1.2", "VM-2.1"]);
}

#[test]
fn a_bad_index_or_an_empty_section_is_an_error() {
    assert!(active_statements("id\tspec\n").is_err());
    assert!(active_statements("id\tspec\tsection\tstatus\tstatement\nX-1.1\n").is_err());
    let statements = active_statements(
        "id\tspec\tsection\tstatus\tstatement\nX-1.1\tx.md\tX-1\tactive\ta\nX-1.2\tx.md\tX-1\twithdrawn\tb\n",
    )
    .unwrap();
    assert_eq!(statements, [("X-1.1".to_owned(), "X-1".to_owned())]);
    let error = spec_score(&["X-2".to_owned()], &statements, &BTreeSet::new()).unwrap_err();
    assert!(error.contains("X-2 has no active statements"), "{error}");
}

#[test]
fn a_crate_without_a_readme_line_or_index_cannot_be_measured() {
    let dir = root()
        .join("target")
        .join("xtask-tests")
        .join("readiness-spec");
    if dir.exists() {
        fs::remove_dir_all(&dir).unwrap();
    }
    fs::create_dir_all(&dir).unwrap();
    assert!(spec(&dir).unwrap_err().contains("README.md"));
    fs::write(dir.join("README.md"), "# x\n").unwrap();
    assert!(spec(&dir).unwrap_err().contains("Spec sections"));
    fs::write(dir.join("README.md"), "- **Spec sections:** none.\n").unwrap();
    // The repository's own index is above target/, so a crate with no
    // sections scores 100%.
    assert_eq!(spec(&dir).unwrap(), score(0, 0));
}

#[test]
fn percentages_round_down_and_nothing_to_measure_is_complete() {
    assert_eq!(percent(&score(2, 3)), "66.6%");
    assert_eq!(percent(&score(0, 0)), "100.0%");
    assert_eq!(floor_percent(&score(899, 1000)), 89);
    assert_eq!(floor_percent(&score(0, 0)), 100);
    assert!(reaches(&score(9, 10), 90));
    assert!(!reaches(&score(899, 1000), 90));
    assert!(reaches(&score(0, 0), 100));
    assert!(reaches(&score(u64::MAX, u64::MAX), 100));
    assert_eq!(describe(&score(1, 4)), "25.0% (1 of 4)");
}

#[test]
fn every_flake_run_happens_even_after_a_failure() {
    let mut calls = 0_u64;
    let score = repeat(5, || {
        calls = calls.saturating_add(1);
        Ok(calls != 2)
    })
    .unwrap();
    assert_eq!(calls, 5);
    assert_eq!(score.passed, 4);
    assert_eq!(score.failures, ["run 2 of 5 failed"]);
    assert!(repeat(5, || Err("cannot run".to_owned())).is_err());
}

fn args(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_owned).collect()
}

#[test]
fn the_command_line_names_a_check_an_output_and_crates() {
    let options = parse_args(&args("flakes --os linux --out m.json a b")).unwrap();
    assert_eq!(
        options,
        Options {
            check: Check::Flakes,
            out: PathBuf::from("m.json"),
            os: "linux".to_owned(),
            crates: vec!["a".to_owned(), "b".to_owned()],
        }
    );
    assert_eq!(
        parse_args(&args("lines --out m.json a")).unwrap().os,
        std::env::consts::OS
    );
    for bad in [
        "",
        "regions --out m.json a",
        "lines a",
        "lines --out m.json",
        "lines --out",
        "lines --os",
        "lines --out m.json --fast a",
    ] {
        assert!(parse_args(&args(bad)).is_err(), "{bad:?}");
    }
}

#[test]
fn crates_resolve_by_name_or_by_directory_inside_the_repository() {
    let root = root();
    let error = resolve(&root, "no-such-crate").unwrap_err();
    assert!(error.contains("not a workspace crate"), "{error}");
    assert!(resolve(&root, "../outside").is_err());
    assert!(resolve(&root, "xtask/fixtures/no-such-dir").is_err());
    assert!(
        resolve(&root, "docs/spec")
            .unwrap_err()
            .contains("no Cargo.toml")
    );
    let by_dir = resolve(&root, "crates/qs-paths").unwrap();
    let by_name = resolve(&root, "qs-paths").unwrap();
    assert_eq!(by_dir.name, "qs-paths");
    assert_eq!(by_dir.dir, by_name.dir.canonicalize().unwrap());
    assert_eq!(by_dir.kind, Kind::Member);
}

#[test]
fn measuring_writes_a_measurement_file() {
    let out = root()
        .join("target")
        .join("xtask-tests")
        .join("readiness-main")
        .join("spec.json");
    let mut command = args("spec --os linux --out");
    command.push(out.display().to_string());
    command.push(format!("{FIXTURES}/spec/crates/fx-spec"));
    // The tools need the scratch directory to exist; a fresh checkout has none.
    if scratch(&root()).exists() {
        fs::remove_dir_all(scratch(&root())).unwrap();
    }
    assert!(main(&root(), &command).unwrap());
    assert!(scratch(&root()).is_dir());
    let measurement: Measurement =
        serde_json::from_str(&fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(measurement.check, Check::Spec);
    assert_eq!(measurement.os, "linux");
    assert_eq!(measurement.crates["fx-spec"].passed, 1);
    let missing = out.with_file_name("missing.json");
    let mut command = args("spec --out");
    command.push(missing.display().to_string());
    command.push("no-such-crate".to_owned());
    assert!(main(&root(), &command).is_err());
    assert!(
        !missing.exists(),
        "nothing is written when a crate cannot be measured"
    );
}

#[test]
fn the_oracles_resolve_to_the_c_tools_layer() {
    for name in ["oracles", "oracles/"] {
        let target = resolve(&root(), name).unwrap();
        assert_eq!(target.name, ORACLES);
        assert_eq!(target.kind, Kind::Oracles);
        assert_eq!(target.dir, root().join("oracles"));
    }
}

#[test]
fn the_oracles_c_is_every_source_and_header_under_oracles() {
    let sources = c_sources(&root().join(ORACLES)).unwrap();
    let names: Vec<String> = sources
        .iter()
        .map(|path| {
            path.strip_prefix(root())
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    for want in [
        "oracles/vm/host.c",
        "oracles/vm/trace.c",
        "oracles/vm/scenario.c",
        "oracles/vm/oracle.h",
        "oracles/qcc/compat.h",
    ] {
        assert!(names.iter().any(|name| name == want), "{want}: {names:?}");
    }
    assert!(
        names
            .iter()
            .all(|name| name.ends_with(".c") || name.ends_with(".h")),
        "{names:?}"
    );
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
    assert!(c_sources(&root().join("no-such-dir")).is_err());
}

#[test]
fn the_oracles_have_no_mutation_score_and_say_why() {
    let target = resolve(&root(), ORACLES).unwrap();
    assert_eq!(measure(&root(), Check::Mutation, &target).unwrap(), None);
    let out = root()
        .join("target")
        .join("xtask-tests")
        .join("readiness-oracles")
        .join("mutation.json");
    let mut command = args("mutation --os linux --out");
    command.push(out.display().to_string());
    command.push(ORACLES.to_owned());
    assert!(main(&root(), &command).unwrap());
    let measurement: Measurement =
        serde_json::from_str(&fs::read_to_string(&out).unwrap()).unwrap();
    assert!(measurement.crates.is_empty(), "{measurement:?}");
    assert!(NO_C_MUTATION.contains("ratchets.toml"));
}

#[test]
fn the_oracles_spec_sections_come_from_their_readme() {
    let target = resolve(&root(), ORACLES).unwrap();
    let readme = fs::read_to_string(target.dir.join("README.md")).unwrap();
    let sections = spec_sections(&readme).unwrap();
    for want in ["DIFF-1", "DIFF-8", "DIFF-10", "TRACE-1", "TRACE-5"] {
        assert!(sections.iter().any(|s| s == want), "{want}: {sections:?}");
    }
    for compared_by_the_runner in ["DIFF-9", "DIFF-11"] {
        assert!(
            !sections.iter().any(|s| s == compared_by_the_runner),
            "{sections:?}"
        );
    }
    let score = measure(&root(), Check::Spec, &target).unwrap().unwrap();
    assert!(score.total > 0, "{score:?}");
}

#[test]
fn citations_are_read_from_xtask_too() {
    assert_eq!(CITING_TREES, ["crates", "apps", "xtask"]);
    let mut cited = BTreeSet::new();
    for file in rust_files(&root().join("xtask").join("src")).unwrap() {
        cited.extend(citations(&fs::read_to_string(file).unwrap()));
    }
    assert!(cited.contains("PROGS-22.1"), "{cited:?}");
}
