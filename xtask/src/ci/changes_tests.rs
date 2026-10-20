// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests of `scripts/ci-changes.sh`, which decides whether a CI run takes the
//! light or the full path (`docs/ci.md`, "Light changes"). The script runs only
//! on Linux in CI, so these run where `bash` and `git` are the Unix ones.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use crate::test_dirs::fresh;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn script() -> PathBuf {
    root().join("scripts/ci-changes.sh")
}

/// Runs the script with `args` in `dir`, `stdin` on its standard input;
/// returns its standard output. Every verdict exits 0.
fn run(dir: &Path, args: &[&str], stdin: &str) -> String {
    let mut child = Command::new("bash")
        .arg(script())
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Only `classify` reads its input; any other command may exit before the
    // write, which then fails with a broken pipe.
    let written = child.stdin.take().unwrap().write_all(stdin.as_bytes());
    let broken_pipe = written
        .as_ref()
        .is_err_and(|error| error.kind() == ErrorKind::BrokenPipe);
    assert!(written.is_ok() || broken_pipe, "{written:?}");
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn classify(paths: &[&str]) -> String {
    let mut stdin = paths.join("\n");
    stdin.push('\n');
    run(&root(), &["classify"], &stdin)
}

#[test]
fn documentation_plan_and_agent_files_are_light() {
    for path in [
        "docs/ci.md",
        "docs/roadmap.md",
        "docs/adr/0001-record-decisions.md",
        "docs/design/tokens.toml",
        "docs/design/mockups/editor.png",
        "docs/gates/p1.md",
        "plan/backlog/p2.yaml",
        "plan/coverage.md",
        "plan/check.py",
        ".claude/rules/testing.md",
        ".claude/skills/work-issue/scripts/land.sh",
        ".kiro/steering/project.md",
        "README.md",
        "CLAUDE.md",
        "AGENTS.md",
        "ATTRIBUTION.md",
    ] {
        assert_eq!(classify(&[path]), "light\n", "{path}");
    }
    assert_eq!(
        classify(&["docs/ci.md", "plan/backlog/p2.yaml", "CLAUDE.md"]),
        "light\n"
    );
}

#[test]
fn spec_statements_and_vectors_are_full() {
    for path in [
        "docs/spec/vm.md",
        "docs/spec/requirements.tsv",
        "docs/spec/vectors/vm/add.txt",
    ] {
        assert_eq!(classify(&[path]), "full\n", "{path}");
    }
}

#[test]
fn rust_files_are_full_even_under_a_light_directory() {
    for path in [
        "docs/example.rs",
        "plan/x.rs",
        ".claude/hooks/x.rs",
        "build.rs",
    ] {
        assert_eq!(classify(&[path]), "full\n", "{path}");
    }
}

#[test]
fn code_workflows_specs_and_configuration_are_full() {
    for path in [
        "crates/qc-vm/src/lib.rs",
        "crates/qc-vm/README.md",
        "apps/cli/README.md",
        "xtask/src/ci.rs",
        "corpus/manifest.toml",
        "oracles/README.md",
        "fuzz/Cargo.toml",
        ".github/workflows/ci.yml",
        ".github/pull_request_template.md",
        "specs/lsp/3.18/specification.md",
        "scripts/ci-changes.sh",
        "scripts/refresh-specs.sh",
        "Cargo.toml",
        "Cargo.lock",
        "deny.toml",
        "layers.toml",
        "ratchets.toml",
        "Makefile",
        "LICENSE",
        ".gitattributes",
        ".pre-commit-config.yaml",
    ] {
        assert_eq!(classify(&[path]), "full\n", "{path}");
    }
}

#[test]
fn a_path_no_rule_knows_is_full() {
    for path in [
        "newdir/file.md",
        "notes.txt",
        "Dockerfile",
        "\"docs/\\303\\251.md\"",
    ] {
        assert_eq!(classify(&[path]), "full\n", "{path}");
    }
}

#[test]
fn one_full_path_makes_the_whole_change_full() {
    assert_eq!(
        classify(&["docs/ci.md", "crates/qc-vm/src/lib.rs"]),
        "full\n"
    );
    assert_eq!(
        classify(&["crates/qc-vm/src/lib.rs", "docs/ci.md"]),
        "full\n"
    );
}

#[test]
fn an_empty_change_is_full() {
    assert_eq!(run(&root(), &["classify"], ""), "full\n");
    assert_eq!(run(&root(), &["classify"], "\n\n"), "full\n");
}

#[test]
fn a_last_line_without_a_newline_still_counts() {
    assert_eq!(run(&root(), &["classify"], "docs/ci.md"), "light\n");
    assert_eq!(
        run(&root(), &["classify"], "docs/ci.md\nCargo.lock"),
        "full\n"
    );
}

#[test]
fn an_unknown_or_missing_command_is_full() {
    assert_eq!(run(&root(), &[], "docs/ci.md\n"), "full\n");
    assert_eq!(run(&root(), &["classify-all"], "docs/ci.md\n"), "full\n");
    // More input than a pipe buffers, so the script exits before reading it.
    let large = "docs/ci.md\n".repeat(100_000);
    assert_eq!(run(&root(), &["classify-all"], &large), "full\n");
}

/// A scratch git repository with one commit holding `crates/a.rs` and
/// `docs/a.md`.
fn repository(name: &str) -> PathBuf {
    let dir = fresh(&format!("ci-changes/{name}"));
    fs::create_dir_all(dir.join("crates")).unwrap();
    fs::create_dir_all(dir.join("docs")).unwrap();
    fs::write(dir.join("crates/a.rs"), "fn a() {}\n").unwrap();
    fs::write(dir.join("docs/a.md"), "# A\n").unwrap();
    git(&dir, &["init", "--quiet"]);
    commit(&dir, "first");
    dir
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(["-c", "user.name=test", "-c", "user.email=test@example.com"])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn commit(dir: &Path, message: &str) -> String {
    git(dir, &["add", "--all"]);
    git(dir, &["commit", "--quiet", "--allow-empty", "-m", message]);
    git(dir, &["rev-parse", "HEAD"])
}

#[test]
fn a_diff_of_light_files_is_light_and_one_of_code_is_full() {
    let dir = repository("light-and-full");
    let base = git(&dir, &["rev-parse", "HEAD"]);
    fs::write(dir.join("docs/a.md"), "# A, edited\n").unwrap();
    let light = commit(&dir, "docs");
    assert_eq!(run(&dir, &["diff", &base, &light], ""), "light\n");
    fs::write(dir.join("crates/a.rs"), "fn b() {}\n").unwrap();
    let full = commit(&dir, "code");
    assert_eq!(run(&dir, &["diff", &light, &full], ""), "full\n");
    assert_eq!(run(&dir, &["diff", &base, &full], ""), "full\n");
}

#[test]
fn a_file_moved_from_code_to_docs_is_full() {
    let dir = repository("rename");
    let base = git(&dir, &["rev-parse", "HEAD"]);
    git(&dir, &["mv", "crates/a.rs", "docs/b.md"]);
    let head = commit(&dir, "move");
    assert_eq!(run(&dir, &["diff", &base, &head], ""), "full\n");
}

#[test]
fn a_diff_that_cannot_be_computed_is_full() {
    let dir = repository("unknown-base");
    let head = git(&dir, &["rev-parse", "HEAD"]);
    let zeros = "0000000000000000000000000000000000000000";
    assert_eq!(run(&dir, &["diff", zeros, &head], ""), "full\n");
    assert_eq!(run(&dir, &["diff", "", &head], ""), "full\n");
    assert_eq!(run(&dir, &["diff", &head], ""), "full\n");
    assert_eq!(run(&dir, &["diff"], ""), "full\n");
}

#[test]
fn an_empty_diff_is_full() {
    let dir = repository("empty");
    let base = git(&dir, &["rev-parse", "HEAD"]);
    let head = commit(&dir, "empty");
    assert_eq!(run(&dir, &["diff", &base, &head], ""), "full\n");
}

#[test]
fn a_diff_outside_a_repository_is_full() {
    let dir = fresh("ci-changes/no-repository");
    let ceiling = dir.parent().unwrap();
    let output = Command::new("bash")
        .arg(script())
        .args(["diff", "HEAD~1", "HEAD"])
        .current_dir(&dir)
        .env("GIT_CEILING_DIRECTORIES", ceiling)
        .output()
        .expect("bash runs");
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "full\n");
}

/// The jobs of `ci.yml`, by id, each with its lines.
fn workflow_jobs() -> Vec<(String, Vec<String>)> {
    let workflow = fs::read_to_string(root().join(".github/workflows/ci.yml")).unwrap();
    let mut jobs: Vec<(String, Vec<String>)> = Vec::new();
    let mut in_jobs = false;
    for line in workflow.lines() {
        if line == "jobs:" {
            in_jobs = true;
            continue;
        }
        if !in_jobs {
            continue;
        }
        let header = line
            .strip_prefix("  ")
            .and_then(|rest| rest.strip_suffix(':'));
        match header {
            Some(id) if !id.starts_with([' ', '#']) => jobs.push((id.to_owned(), Vec::new())),
            _ => {
                if let Some((_, lines)) = jobs.last_mut() {
                    lines.push(line.to_owned());
                }
            }
        }
    }
    jobs
}

/// The steps among a job's lines, each as its lines.
fn steps(lines: &[String]) -> Vec<Vec<String>> {
    let mut steps: Vec<Vec<String>> = Vec::new();
    let mut in_steps = false;
    for line in lines {
        if line == "    steps:" {
            in_steps = true;
        } else if in_steps && line.starts_with("      - ") {
            steps.push(vec![line.clone()]);
        } else if in_steps && let Some(step) = steps.last_mut() {
            step.push(line.clone());
        }
    }
    steps
}

/// A job that needs the classification runs even when the classification
/// fails: otherwise it would be skipped, and a skipped job reports success.
#[test]
fn every_job_that_needs_the_classification_runs_when_it_fails() {
    let jobs = workflow_jobs();
    let mut needing = 0_usize;
    for (id, lines) in &jobs {
        if !lines.iter().any(|line| line == "    needs: changes") {
            continue;
        }
        needing = needing.saturating_add(1);
        let condition = lines.iter().find(|line| line.starts_with("    if: "));
        assert!(
            condition.is_some_and(|line| line.contains("!cancelled()")),
            "{id}: {condition:?}"
        );
    }
    assert_eq!(needing, jobs.len().saturating_sub(3), "{jobs:?}");
}

/// On the light path a matrix job skips its steps, so every step of one must
/// wait for the full path, including a step added later.
#[test]
fn every_step_of_the_build_and_test_asset_jobs_waits_for_the_full_path() {
    let jobs = workflow_jobs();
    for id in ["build", "test-assets"] {
        let (_, lines) = jobs.iter().find(|(name, _)| name == id).unwrap();
        let steps = steps(lines);
        assert!(steps.len() >= 4, "{id}");
        for step in steps {
            assert!(
                step.iter()
                    .any(|line| line.starts_with("        if: env.CI_PATH == 'full'")),
                "{id}: {step:?}"
            );
        }
    }
}
