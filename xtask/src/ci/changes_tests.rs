// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests of `scripts/ci-changes.sh`, which decides whether a CI run takes the
//! light or the full path (`docs/ci.md`, "Light changes"). The script runs only
//! on Linux in CI, so these run where `bash` and `git` are the Unix ones.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::fs;
use std::io::Write;
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
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
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
    for path in ["docs/example.rs", "plan/x.rs", ".claude/hooks/x.rs", "build.rs"] {
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
    for path in ["newdir/file.md", "notes.txt", "Dockerfile", "\"docs/\\303\\251.md\""] {
        assert_eq!(classify(&[path]), "full\n", "{path}");
    }
}

#[test]
fn one_full_path_makes_the_whole_change_full() {
    assert_eq!(classify(&["docs/ci.md", "crates/qc-vm/src/lib.rs"]), "full\n");
    assert_eq!(classify(&["crates/qc-vm/src/lib.rs", "docs/ci.md"]), "full\n");
}

#[test]
fn an_empty_change_is_full() {
    assert_eq!(run(&root(), &["classify"], ""), "full\n");
    assert_eq!(run(&root(), &["classify"], "\n\n"), "full\n");
}

#[test]
fn a_last_line_without_a_newline_still_counts() {
    assert_eq!(run(&root(), &["classify"], "docs/ci.md"), "light\n");
    assert_eq!(run(&root(), &["classify"], "docs/ci.md\nCargo.lock"), "full\n");
}

#[test]
fn an_unknown_or_missing_command_is_full() {
    assert_eq!(run(&root(), &[], "docs/ci.md\n"), "full\n");
    assert_eq!(run(&root(), &["classify-all"], "docs/ci.md\n"), "full\n");
}

/// A scratch git repository with one commit holding `crates/a.rs` and
/// `docs/a.md`.
fn repository(name: &str) -> PathBuf {
    let dir = root()
        .join("target")
        .join("xtask-tests")
        .join("ci-changes")
        .join(name);
    if dir.exists() {
        fs::remove_dir_all(&dir).unwrap();
    }
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
    let dir = root()
        .join("target")
        .join("xtask-tests")
        .join("ci-changes")
        .join("no-repository");
    fs::create_dir_all(&dir).unwrap();
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
