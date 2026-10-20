// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap, index or sum is a failed test"
)]

use super::*;
use crate::exec::{Fake, Reply};
use crate::ratchet::judge;
use crate::test_dirs::{claim, fresh};

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

/// The claims of a README with `sections` and `statements` and no disclaimed ones.
fn claims(sections: &[&str], statements: &[&str]) -> Claims {
    Claims {
        sections: sections.iter().map(|s| (*s).to_owned()).collect(),
        statements: statements.iter().map(|s| (*s).to_owned()).collect(),
        disclaimed: Vec::new(),
    }
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
    let error = spec_score(
        &claims(&["X-2"], &[]),
        &statements,
        &BTreeSet::new(),
        &BTreeSet::new(),
    )
    .unwrap_err();
    assert!(error.contains("X-2 has no active statements"), "{error}");
}

#[test]
fn a_baseline_header_is_read_and_a_bad_one_is_an_error() {
    assert_eq!(
        baseline_statements("id\nPROGS-5.1\n\nLNO-1.1\n").unwrap(),
        ["LNO-1.1".to_owned(), "PROGS-5.1".to_owned()]
            .into_iter()
            .collect()
    );
    assert_eq!(baseline_statements("id\n").unwrap(), BTreeSet::new());
    assert!(baseline_statements("").is_err());
    assert!(baseline_statements("statement\nPROGS-5.1\n").is_err());
}

#[test]
fn a_baseline_statement_is_not_counted_against_a_crate() {
    // X-1 owns X-1.1 (cited) and X-1.2 (not cited); X-1.2 is in the baseline,
    // so it is excluded and the crate is 100%, as `docs/spec/README.md` says a
    // statement written ahead of its code is not yet owned by the crate.
    let statements = [
        ("X-1.1".to_owned(), "X-1".to_owned()),
        ("X-1.2".to_owned(), "X-1".to_owned()),
    ];
    let cited = ["X-1.1".to_owned()].into_iter().collect();
    let baseline = ["X-1.2".to_owned()].into_iter().collect();
    let score = spec_score(&claims(&["X-1"], &[]), &statements, &baseline, &cited).unwrap();
    assert_eq!(score, self::score(1, 1));
    assert!(score.failures.is_empty(), "{:?}", score.failures);
    // Without the baseline the uncited statement is a failure.
    let score = spec_score(
        &claims(&["X-1"], &[]),
        &statements,
        &BTreeSet::new(),
        &cited,
    )
    .unwrap();
    assert_eq!(
        score,
        Score {
            passed: 1,
            total: 2,
            failures: vec!["X-1.2 is not cited".to_owned()],
        }
    );
    // A section whose every statement is in the baseline contributes nothing,
    // and does not error as a missing section would.
    let both_baselined = ["X-1.1".to_owned(), "X-1.2".to_owned()]
        .into_iter()
        .collect();
    let score = spec_score(
        &claims(&["X-1"], &[]),
        &statements,
        &both_baselined,
        &BTreeSet::new(),
    )
    .unwrap();
    assert_eq!(score, self::score(0, 0));
}

#[test]
fn a_range_of_sections_is_every_section_between() {
    let readme = "- **Spec sections:** [LNO-2 to LNO-4](../lno.md), PROGS-19 to PROGS-21 and\n  \
                  VM-7 to VM-6, which is no range; MAP-1 to WAD-2 neither.\n";
    assert_eq!(
        spec_sections(readme).unwrap(),
        [
            "LNO-2", "LNO-3", "LNO-4", "PROGS-19", "PROGS-20", "PROGS-21", "VM-7", "VM-6", "MAP-1",
            "WAD-2"
        ]
    );
    let statements = "- **Spec statements:** X-1.2 to X-1.4, Y-1.9 to Y-2.1, Z-1.5 to Z-1.3,\n  \
                      W-1.1 to V-1.2 and X-1.x.\n";
    assert_eq!(
        spec_statements(statements),
        [
            "X-1.2", "X-1.3", "X-1.4", "Y-1.9", "Y-2.1", "Z-1.5", "Z-1.3", "W-1.1", "V-1.2"
        ]
    );
}

#[test]
fn a_disclaimed_section_or_statement_is_not_the_crates() {
    let readme = "- **Spec sections:** PROGS-1 to PROGS-6 and PROGS-22.\n\
                  - **Not this crate's:** PROGS-5 (the compiler's), PROGS-16 to PROGS-18\n  \
                  (`qc-types`), and PROGS-6.2.\n";
    assert_eq!(
        spec_sections(readme).unwrap(),
        [
            "PROGS-1", "PROGS-2", "PROGS-3", "PROGS-4", "PROGS-6", "PROGS-22"
        ]
    );
    assert_eq!(disclaimed_statements(readme), ["PROGS-6.2"]);
    assert!(disclaimed_statements("- **Spec sections:** X-1.\n").is_empty());
    let statements = [
        ("X-1.1".to_owned(), "X-1".to_owned()),
        ("X-1.2".to_owned(), "X-1".to_owned()),
    ];
    let cited: BTreeSet<String> = ["X-1.1".to_owned()].into_iter().collect();
    let mut claims = claims(&["X-1"], &[]);
    claims.disclaimed = vec!["X-1.2".to_owned()];
    let score = spec_score(&claims, &statements, &BTreeSet::new(), &cited).unwrap();
    assert_eq!(score, self::score(1, 1));
}

#[test]
fn spec_statements_come_from_their_own_readme_line() {
    let readme = "- **Spec sections:** VM-12.\n- **Spec statements:** VM-13.2, VM-13.10 and\n  \
                  [VM-14.1](../vm.md). VM-13.2 again; VM-15 is a section.\n- **Shipped:** VM-16.1.\n";
    assert_eq!(spec_statements(readme), ["VM-13.2", "VM-13.10", "VM-14.1"]);
    assert_eq!(spec_sections(readme).unwrap(), ["VM-12"]);
    assert!(spec_statements("- **Spec sections:** VM-12.1.\n").is_empty());
}

#[test]
fn a_claimed_statement_is_counted_once_and_must_be_active_and_owned() {
    let statements = [
        ("X-1.1".to_owned(), "X-1".to_owned()),
        ("X-1.2".to_owned(), "X-1".to_owned()),
        ("Y-1.1".to_owned(), "Y-1".to_owned()),
        ("Y-1.2".to_owned(), "Y-1".to_owned()),
    ];
    let cited: BTreeSet<String> = ["X-1.1".to_owned(), "Y-1.1".to_owned()]
        .into_iter()
        .collect();
    // Y-1.2, uncited, is in the claimed statement's section but not claimed.
    let score = spec_score(
        &claims(&["X-1"], &["Y-1.1", "X-1.1"]),
        &statements,
        &["X-1.2".to_owned()].into_iter().collect(),
        &cited,
    )
    .unwrap();
    assert_eq!(score, self::score(2, 2));
    let uncited = spec_score(
        &claims(&[], &["Y-1.2"]),
        &statements,
        &BTreeSet::new(),
        &cited,
    )
    .unwrap();
    assert_eq!(uncited.failures, ["Y-1.2 is not cited"]);
    let missing = spec_score(
        &claims(&[], &["Z-1.1"]),
        &statements,
        &BTreeSet::new(),
        &cited,
    )
    .unwrap_err();
    assert!(
        missing.contains("Z-1.1 is not an active statement"),
        "{missing}"
    );
    let baselined = spec_score(
        &claims(&[], &["Y-1.2"]),
        &statements,
        &["Y-1.2".to_owned()].into_iter().collect(),
        &cited,
    )
    .unwrap_err();
    assert!(
        baselined.contains("Y-1.2 is claimed but is in"),
        "{baselined}"
    );
}

#[test]
fn a_crate_without_a_readme_line_or_index_cannot_be_measured() {
    let dir = fresh("readiness/spec-unmeasurable");
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

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let path = entry.unwrap().path();
        let target = to.join(path.file_name().unwrap());
        if path.is_dir() {
            copy_dir(&path, &target);
        } else {
            fs::copy(&path, &target).unwrap();
        }
    }
}

#[test]
fn measuring_writes_a_measurement_file() {
    let base = fresh("readiness/main");
    // A fresh copy of the spec fixture's small repository, which has no scratch directory
    // yet, as a fresh checkout has none: the tools need it to exist.
    let repo = base.join("repo");
    copy_dir(&root().join(FIXTURES).join("spec"), &repo);
    let out = base.join("spec.json");
    let mut command = args("spec --os linux --out");
    command.push(out.display().to_string());
    command.push("crates/fx-spec".to_owned());
    assert!(!scratch(&repo).exists());
    assert!(main(&repo, &command).unwrap());
    assert!(scratch(&repo).is_dir());
    let measurement: Measurement =
        serde_json::from_str(&fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(measurement.check, Check::Spec);
    assert_eq!(measurement.os, "linux");
    assert_eq!(measurement.crates["fx-spec"].passed, 1);
    let missing = out.with_file_name("missing.json");
    let mut command = args("spec --out");
    command.push(missing.display().to_string());
    command.push("no-such-crate".to_owned());
    assert!(main(&repo, &command).is_err());
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
    let fake = Fake::new(|_: &Command| Reply::exit(0));
    assert_eq!(
        measure(
            &root(),
            Check::Mutation,
            &target,
            &tools(&fake, "oracles-no-mutation")
        )
        .unwrap(),
        None
    );
    let out = fresh("readiness/oracles-mutation").join("mutation.json");
    let mut command = args("mutation --os linux --out");
    command.push(out.display().to_string());
    command.push(ORACLES.to_owned());
    // `main` makes the real scratch directory, and writes nothing in it here.
    claim(&scratch(&root()));
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
    let fake = Fake::new(|_: &Command| Reply::exit(0));
    let score = measure(&root(), Check::Spec, &target, &tools(&fake, "oracles-spec"))
        .unwrap()
        .unwrap();
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

// --- Measuring with fakes of the tools ---

/// The value after `flag` in `command`'s arguments.
fn after(command: &Command, flag: &str) -> Option<PathBuf> {
    let args: Vec<_> = command.get_args().collect();
    args.iter()
        .position(|arg| *arg == flag)
        .and_then(|index| args.get(index + 1))
        .map(PathBuf::from)
}

fn has_arg(command: &Command, arg: &str) -> bool {
    command.get_args().any(|a| a == arg)
}

fn fixture(check: &str) -> Target {
    resolve(&root(), &format!("{FIXTURES}/{check}")).unwrap()
}

/// Tools that run `exec`, writing in a new `readiness/<name>` of their own.
fn tools<'a>(exec: &'a dyn Exec, name: &str) -> Tools<'a> {
    Tools {
        exec,
        assets: None,
        scratch: fresh(&format!("readiness/{name}")),
    }
}

#[test]
fn coverage_reads_the_report_cargo_llvm_cov_writes() {
    let report = recorded("branches", "llvm-cov.json");
    let fake = Fake::new(|command: &Command| Reply {
        files: vec![(
            after(command, "--output-path").unwrap(),
            report.clone().into_bytes(),
        )],
        ..Reply::default()
    });
    let target = fixture("branches");
    let tools = tools(&fake, "coverage-reads");
    let score = coverage(&root(), &target, Check::Branches, &tools).unwrap();
    assert_eq!(score, self::score(2, 4));
    let lines = coverage(&root(), &target, Check::Lines, &tools).unwrap();
    assert_eq!(lines.passed, lines.total, "every line of the fixture runs");
    let seen = fake.seen.borrow();
    assert!(
        seen[0].starts_with(&format!("cargo +{NIGHTLY} llvm-cov")),
        "{}",
        seen[0]
    );
    assert!(seen[0].ends_with("--branch"), "{}", seen[0]);
    assert!(!seen[1].contains("--branch"), "{}", seen[1]);
}

/// The `PROPTEST_*` variables `command` sets, by name.
fn proptest_envs(command: &Command) -> BTreeMap<String, String> {
    command
        .get_envs()
        .filter_map(|(key, value)| {
            let key = key.to_string_lossy().into_owned();
            let value = value?.to_string_lossy().into_owned();
            key.starts_with("PROPTEST_").then_some((key, value))
        })
        .collect()
}

#[test]
fn coverage_runs_property_tests_on_a_fixed_seed() {
    let report = recorded("branches", "llvm-cov.json");
    let envs = std::cell::RefCell::new(Vec::new());
    let fake = Fake::new(|command: &Command| {
        envs.borrow_mut().push(proptest_envs(command));
        Reply {
            files: vec![(
                after(command, "--output-path").unwrap(),
                report.clone().into_bytes(),
            )],
            ..Reply::default()
        }
    });
    let target = fixture("branches");
    let tools = tools(&fake, "coverage-fixed-seed");
    coverage(&root(), &target, Check::Lines, &tools).unwrap();
    coverage(&root(), &target, Check::Branches, &tools).unwrap();
    let fixed = BTreeMap::from([
        ("PROPTEST_CASES".to_owned(), "256".to_owned()),
        ("PROPTEST_RNG_ALGORITHM".to_owned(), "cc".to_owned()),
        ("PROPTEST_RNG_SEED".to_owned(), "461".to_owned()),
    ]);
    assert_eq!(*envs.borrow(), [fixed.clone(), fixed]);
}

#[test]
fn an_inherited_proptest_setting_is_removed_and_others_kept() {
    let mut command = Command::new("cargo");
    let inherited = [
        "PROPTEST_MAX_DEFAULT_SIZE_RANGE",
        "PROPTEST_RNG_SEED",
        "PATH",
    ];
    fix_proptest(&mut command, inherited.map(OsString::from));
    let envs: BTreeMap<_, _> = command
        .get_envs()
        .map(|(key, value)| {
            let value = value.map(|value| value.to_string_lossy().into_owned());
            (key.to_string_lossy().into_owned(), value)
        })
        .collect();
    let set = |value: &str| Some(value.to_owned());
    assert_eq!(
        envs,
        BTreeMap::from([
            ("PROPTEST_CASES".to_owned(), set("256")),
            ("PROPTEST_MAX_DEFAULT_SIZE_RANGE".to_owned(), None),
            ("PROPTEST_RNG_ALGORITHM".to_owned(), set("cc")),
            ("PROPTEST_RNG_SEED".to_owned(), set("461")),
        ])
    );
}

#[test]
fn mutation_keeps_proptests_random_seed() {
    let outcomes = recorded("mutation", "outcomes.json");
    let envs = std::cell::RefCell::new(None);
    let fake = Fake::new(|command: &Command| {
        envs.replace(Some(proptest_envs(command)));
        Reply {
            files: vec![(
                after(command, "--output")
                    .unwrap()
                    .join("mutants.out/outcomes.json"),
                outcomes.clone().into_bytes(),
            )],
            ..Reply::default()
        }
    });
    let tools = tools(&fake, "mutation-random-seed");
    mutation(&root(), &fixture("mutation"), &tools).unwrap();
    assert_eq!(envs.borrow().as_ref(), Some(&BTreeMap::new()));
}

#[test]
fn coverage_fails_when_cargo_llvm_cov_does() {
    let fake = Fake::new(|_: &Command| Reply {
        code: 101,
        stderr: "test failed".to_owned(),
        ..Reply::default()
    });
    let name = "coverage-fails";
    let error = coverage(
        &root(),
        &fixture("lines"),
        Check::Lines,
        &tools(&fake, name),
    )
    .unwrap_err();
    assert!(
        error.contains("cargo llvm-cov failed") && error.contains("test failed"),
        "{error}"
    );
    let silent = Fake::new(|_: &Command| Reply::exit(0));
    assert!(
        coverage(
            &root(),
            &fixture("lines"),
            Check::Lines,
            &tools(&silent, name)
        )
        .is_err()
    );
    let cannot = Fake::new(|_: &Command| Reply {
        cannot_run: true,
        ..Reply::default()
    });
    let error = coverage(
        &root(),
        &fixture("lines"),
        Check::Lines,
        &tools(&cannot, name),
    )
    .unwrap_err();
    assert!(error.contains("cannot run cargo llvm-cov"), "{error}");
}

#[test]
fn mutation_reads_the_outcomes_cargo_mutants_writes() {
    let outcomes = recorded("mutation", "outcomes.json");
    let reply = |code: i32| {
        let outcomes = outcomes.clone();
        move |command: &Command| Reply {
            code,
            files: vec![(
                after(command, "--output")
                    .unwrap()
                    .join("mutants.out/outcomes.json"),
                outcomes.clone().into_bytes(),
            )],
            ..Reply::default()
        }
    };
    let target = fixture("mutation");
    for code in [0, 2, 3] {
        let fake = Fake::new(reply(code));
        let score = mutation(&root(), &target, &tools(&fake, "mutation-reads")).unwrap();
        assert_eq!(score.total, 2);
        let line = fake.seen.borrow()[0].clone();
        assert!(line.contains("--copy-vcs=true"), "{line}");
        assert!(line.contains("--jobs "), "{line}");
        // The fixture is its own workspace, without the `mutants` profile.
        assert!(!line.contains("--profile"), "{line}");
        assert!(!line.contains("--minimum-test-timeout"), "{line}");
    }
    let failed = Fake::new(reply(4));
    let error = mutation(&root(), &target, &tools(&failed, "mutation-reads")).unwrap_err();
    assert!(error.contains("cargo mutants failed"), "{error}");
}

#[test]
fn mutation_builds_a_member_incrementally_with_the_mutants_profile() {
    let outcomes = recorded("mutation", "outcomes.json");
    let incremental = std::cell::RefCell::new(None);
    let fake = Fake::new(|command: &Command| {
        incremental.replace(
            command
                .get_envs()
                .find(|(key, _)| *key == "CARGO_INCREMENTAL")
                .and_then(|(_, value)| value.map(|value| value.to_string_lossy().into_owned())),
        );
        Reply {
            files: vec![(
                after(command, "--output")
                    .unwrap()
                    .join("mutants.out/outcomes.json"),
                outcomes.clone().into_bytes(),
            )],
            ..Reply::default()
        }
    });
    let target = resolve(&root(), "qs-porting-example").unwrap();
    mutation(&root(), &target, &tools(&fake, "mutation-member")).unwrap();
    let line = fake.seen.borrow()[0].clone();
    assert!(line.contains("--profile mutants "), "{line}");
    assert!(line.contains("--minimum-test-timeout 10 "), "{line}");
    // Without incremental builds, every mutant rebuilds the crate in full.
    assert_eq!(incremental.borrow().as_deref(), Some("1"));
}

#[test]
fn flakes_build_once_then_run_the_tests_twenty_times() {
    let runs = std::cell::Cell::new(0_u32);
    let fake = Fake::new(|command: &Command| {
        if has_arg(command, "--no-run") {
            return Reply::exit(0);
        }
        runs.set(runs.get() + 1);
        Reply::exit(i32::from(runs.get() == 3))
    });
    let score = flakes(&root(), &fixture("flakes"), &tools(&fake, "flakes")).unwrap();
    assert_eq!(score.total, FLAKE_RUNS);
    assert_eq!(score.passed, FLAKE_RUNS - 1);
    assert_eq!(score.failures, ["run 3 of 20 failed"]);
    let broken = Fake::new(|_: &Command| Reply {
        code: 101,
        stderr: "error[E0425]".to_owned(),
        ..Reply::default()
    });
    let error = flakes(&root(), &fixture("flakes"), &tools(&broken, "flakes")).unwrap_err();
    assert!(error.contains("do not build"), "{error}");
}

#[test]
fn the_oracles_flake_check_builds_then_runs_cargo_xtask_oracles() {
    let fake = Fake::new(|_: &Command| Reply::exit(0));
    let score = oracle_flakes(&root(), &fake).unwrap();
    assert_eq!(score, self::score(FLAKE_RUNS, FLAKE_RUNS));
    let seen = fake.seen.borrow();
    assert!(seen[0].ends_with("oracles build"), "{}", seen[0]);
    assert!(seen[1].ends_with("oracles"), "{}", seen[1]);
    let broken = Fake::new(|_: &Command| Reply::exit(1));
    assert!(
        oracle_flakes(&root(), &broken)
            .unwrap_err()
            .contains("failed")
    );
}

/// A directory holding the corpus layout the oracles' coverage reads.
fn assets(name: &str, with_id101: bool) -> PathBuf {
    let dir = fresh(&format!("readiness/{name}"));
    let id101 = dir.join(ID101);
    fs::create_dir_all(&id101).unwrap();
    if with_id101 {
        fs::write(id101.join("progs.src"), "progs.dat\n").unwrap();
    }
    dir
}

/// A fake of the oracles' coverage tools: `cargo xtask oracles` writes a profile where
/// `LLVM_PROFILE_FILE` says (unless `profile` is false), and `llvm-cov export` prints
/// `report`.
fn coverage_tools(profile: bool, report: String) -> impl Fn(&Command) -> Reply {
    move |command: &Command| {
        let program = command.get_program().to_string_lossy().into_owned();
        let profile_file = command
            .get_envs()
            .find(|(key, _)| *key == "LLVM_PROFILE_FILE")
            .and_then(|(_, value)| value.map(PathBuf::from));
        if let Some(path) = profile_file.filter(|_| profile) {
            return Reply {
                files: vec![(path.with_file_name("vm-oracle-1.profraw"), b"raw".to_vec())],
                ..Reply::default()
            };
        }
        if has_arg(command, "export") || program.ends_with("llvm-cov") {
            return Reply {
                stdout: report.clone(),
                ..Reply::default()
            };
        }
        Reply::exit(0)
    }
}

#[test]
fn the_oracles_coverage_runs_every_check_then_exports_our_files() {
    let report = recorded("lines", "llvm-cov.json");
    let fake = Fake::new(coverage_tools(true, report));
    let tools = Tools {
        assets: Some(assets("assets", true).into_os_string()),
        ..self::tools(&fake, "oracles-coverage")
    };
    let score = c_coverage(&root(), Check::Lines, &tools).unwrap();
    assert_eq!(score, self::score(6, 18));
    let seen = fake.seen.borrow();
    assert!(seen[0].ends_with(" oracles"), "{}", seen[0]);
    assert!(seen[1].contains(" oracles id101 "), "{}", seen[1]);
    assert!(seen[2].contains("merge -sparse"), "{}", seen[2]);
    let export = &seen[3];
    assert!(export.contains("export -summary-only"), "{export}");
    assert!(
        export.contains("host.c") && !export.contains("pr_exec.c"),
        "{export}"
    );
}

#[test]
fn the_oracles_coverage_needs_id101_and_a_profile() {
    let name = "oracles-coverage-needs";
    let fake = Fake::new(coverage_tools(true, String::new()));
    let error = c_coverage(&root(), Check::Lines, &tools(&fake, name)).unwrap_err();
    assert!(error.contains("QS_TEST_ASSETS"), "{error}");
    let without = Tools {
        assets: Some(assets("no-id101", false).into_os_string()),
        ..tools(&fake, name)
    };
    let error = c_coverage(&root(), Check::Lines, &without).unwrap_err();
    assert!(error.contains("no id 1.01"), "{error}");
    let with = Some(assets("with-id101", true).into_os_string());
    let silent = Fake::new(coverage_tools(false, String::new()));
    let no_profile = Tools {
        assets: with.clone(),
        ..tools(&silent, name)
    };
    let error = c_coverage(&root(), Check::Lines, &no_profile).unwrap_err();
    assert!(error.contains("wrote no profile"), "{error}");
    let export_fails = Fake::new(|command: &Command| {
        if has_arg(command, "export") {
            return Reply::exit(1);
        }
        coverage_tools(true, String::new())(command)
    });
    let failing = Tools {
        assets: with,
        ..tools(&export_fails, name)
    };
    let error = c_coverage(&root(), Check::Branches, &failing).unwrap_err();
    assert!(error.contains("llvm-cov export failed"), "{error}");
}

#[test]
fn a_measurement_runs_the_check_for_each_crate_named() {
    let out = fresh("readiness/run-out").join("flakes.json");
    let fake = Fake::new(|_: &Command| Reply::exit(0));
    let mut command = args("flakes --os linux --out");
    command.push(out.display().to_string());
    command.push(format!("{FIXTURES}/flakes"));
    command.push(ORACLES.to_owned());
    assert!(run_check(&root(), &command, &tools(&fake, "run")).unwrap());
    let measurement: Measurement =
        serde_json::from_str(&fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(measurement.crates.len(), 2);
    assert_eq!(measurement.crates["oracles"].passed, FLAKE_RUNS);
}
