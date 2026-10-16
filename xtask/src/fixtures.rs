// SPDX-License-Identifier: GPL-3.0-or-later

//! Proves the lint and supply-chain configuration rejects what it targets.
//!
//! Each check builds a scratch workspace under `target/xtask/`, copies the
//! real configuration into it, adds one crate per fixture and runs the real
//! tool. A fixture that should fail must fail for its own reason only, and a
//! control fixture must pass, so a broken scratch setup cannot pass as a lint.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use toml::{Table, Value};

use crate::Report;
use crate::exec::Exec;

const CLIPPY_FIXTURES: &str = "xtask/fixtures/clippy";
const EXPECT_PREFIX: &str = "// expect: ";
const CLIPPY_LINT_ANCHOR: &str = "index.html#";

/// `cargo xtask clippy-fixtures`, with the programs `exec` runs.
pub(crate) fn clippy(root: &Path, exec: &dyn Exec) -> Result<Report, String> {
    let cargo = read_toml(&root.join("Cargo.toml"))?;
    let work = fresh_dir(root, "clippy-fixtures")?;

    let mut workspace = Table::new();
    workspace.insert("resolver".to_owned(), Value::from("3"));
    workspace.insert("members".to_owned(), Value::from(vec!["fx-*"]));
    for key in ["package", "lints"] {
        let Some(value) = cargo.get("workspace").and_then(|w| w.get(key)) else {
            return Err(format!("Cargo.toml has no [workspace.{key}]"));
        };
        workspace.insert(key.to_owned(), value.clone());
    }
    let mut manifest = Table::new();
    manifest.insert("workspace".to_owned(), Value::Table(workspace));
    write(&work.join("Cargo.toml"), &manifest.to_string())?;
    copy(&root.join(".clippy.toml"), &work.join(".clippy.toml"))?;

    let fixtures = clippy_fixtures(&root.join(CLIPPY_FIXTURES))?;
    for fixture in &fixtures {
        let crate_dir = work.join(format!("fx-{}", fixture.name));
        write(
            &crate_dir.join("Cargo.toml"),
            &lint_member_manifest(&format!("fx-{}", fixture.name)),
        )?;
        write(&crate_dir.join("src").join("lib.rs"), &fixture.source)?;
    }

    let mut report = Report::default();
    for fixture in &fixtures {
        let output = run(
            exec,
            cargo_command()
                .args(["clippy", "--quiet", "--all-targets", "--package"])
                .arg(format!("fx-{}", fixture.name))
                .arg("--manifest-path")
                .arg(work.join("Cargo.toml"))
                .args(["--", "-D", "warnings"])
                .env("CLIPPY_CONF_DIR", &work)
                .env("CARGO_TARGET_DIR", work.join("target")),
        )?;
        let fired = fired_lints(&output);
        let (ok, outcome) = match &fixture.expect {
            None => (
                output.status.success(),
                format!("passes clippy (fired: {fired:?})"),
            ),
            Some(lint) => (
                !output.status.success() && fired.len() == 1 && fired.contains(lint),
                format!("fails clippy with {lint} (fired: {fired:?})"),
            ),
        };
        if !ok {
            report
                .problems
                .push(String::from_utf8_lossy(&output.stderr).into_owned());
        }
        report.record(ok, format!("fixture {} {outcome}", fixture.name));
    }
    Ok(report)
}

/// One scratch workspace for `cargo deny`: a crate with this license and these
/// dependencies, checked with `checks`.
struct DenyCase {
    name: &'static str,
    license: &'static str,
    dependencies: &'static [&'static str],
    checks: &'static [&'static str],
    /// The cargo-deny error code that must be reported, or `None` to pass.
    expect: Option<&'static str>,
}

const DENY_CASES: [DenyCase; 4] = [
    DenyCase {
        name: "control",
        license: "GPL-3.0-or-later",
        dependencies: &[],
        checks: &["licenses", "bans", "sources"],
        expect: None,
    },
    DenyCase {
        name: "gpl2-only",
        license: "GPL-2.0-only",
        dependencies: &[],
        checks: &["licenses"],
        expect: Some("rejected"),
    },
    DenyCase {
        name: "openssl",
        license: "OpenSSL",
        dependencies: &[],
        checks: &["licenses"],
        expect: Some("rejected"),
    },
    DenyCase {
        name: "wildcard",
        license: "GPL-3.0-or-later",
        dependencies: &["itoa = \"*\""],
        checks: &["bans"],
        expect: Some("wildcard"),
    },
];

/// `cargo xtask deny-fixtures`, with the programs `exec` runs.
pub(crate) fn deny(root: &Path, exec: &dyn Exec) -> Result<Report, String> {
    let work = fresh_dir(root, "deny-fixtures")?;
    let config = fs::read_to_string(root.join("deny.toml"))
        .map_err(|error| format!("deny.toml: {error}"))?;
    let mut report = Report::default();
    for case in &DENY_CASES {
        let dir = work.join(case.name);
        write(
            &dir.join("Cargo.toml"),
            "[workspace]\nresolver = \"3\"\nmembers = [\"fx\"]\n",
        )?;
        // cargo-deny 0.20 moved `--config` from `check` to the top level, so the
        // config is copied next to the manifest, where every version finds it.
        write(&dir.join("deny.toml"), &config)?;
        write(
            &dir.join("fx").join("Cargo.toml"),
            &deny_member_manifest(case.license, case.dependencies),
        )?;
        write(&dir.join("fx").join("src").join("lib.rs"), "")?;
        let output = run(
            exec,
            Command::new(cargo_bin())
                .current_dir(&dir)
                .args(["deny", "--manifest-path"])
                .arg(dir.join("Cargo.toml"))
                .arg("check")
                .args(case.checks),
        )?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        let checks = case.checks.join(" ");
        let (ok, outcome) = match case.expect {
            None => (
                output.status.success(),
                format!("passes cargo deny check {checks}"),
            ),
            Some(code) => (
                !output.status.success() && stderr.contains(&format!("error[{code}]")),
                format!("fails cargo deny check {checks} with error[{code}]"),
            ),
        };
        if !ok {
            report.problems.push(stderr.into_owned());
        }
        report.record(ok, format!("{} ({}) {outcome}", case.name, case.license));
    }
    Ok(report)
}

#[derive(Debug, PartialEq, Eq)]
struct Fixture {
    name: String,
    /// The one lint the fixture must trigger, or `None` if it must pass.
    expect: Option<String>,
    source: String,
}

fn clippy_fixtures(dir: &Path) -> Result<Vec<Fixture>, String> {
    let entries = fs::read_dir(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    let mut fixtures = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|error| format!("{}: {error}", dir.display()))?
            .path();
        if path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        let source =
            fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().replace('_', "-"))
            .unwrap_or_default();
        fixtures.push(
            parse_fixture(name, source).map_err(|error| format!("{}: {error}", path.display()))?,
        );
    }
    fixtures.sort_by(|a, b| a.name.cmp(&b.name));
    if fixtures.is_empty() {
        return Err(format!("{} holds no fixtures", dir.display()));
    }
    Ok(fixtures)
}

fn parse_fixture(name: String, source: String) -> Result<Fixture, String> {
    let Some(expect) = source
        .lines()
        .find_map(|line| line.strip_prefix(EXPECT_PREFIX))
    else {
        return Err(format!(
            "no `{EXPECT_PREFIX}clippy::<lint>` or `{EXPECT_PREFIX}pass` line"
        ));
    };
    let expect = match expect.trim() {
        "pass" => None,
        other => match other.strip_prefix("clippy::") {
            Some(lint) if !lint.is_empty() => Some(lint.to_owned()),
            _ => return Err(format!("cannot read the expectation {other:?}")),
        },
    };
    Ok(Fixture {
        name,
        expect,
        source,
    })
}

/// The clippy lints named by the "for further information" links in `output`.
fn fired_lints(output: &Output) -> BTreeSet<String> {
    let stderr = String::from_utf8_lossy(&output.stderr);
    stderr
        .split(CLIPPY_LINT_ANCHOR)
        .skip(1)
        .filter_map(|rest| {
            let lint: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            (!lint.is_empty()).then_some(lint)
        })
        .collect()
}

/// A fixture crate that inherits the workspace's package fields and lints.
fn lint_member_manifest(name: &str) -> String {
    format!(
        "[package]\nname = \"{name}\"\nversion.workspace = true\nedition.workspace = true\n\
         license.workspace = true\nrust-version.workspace = true\npublish.workspace = true\n\
         \n[lints]\nworkspace = true\n"
    )
}

/// A fixture crate with its own license and dependencies.
fn deny_member_manifest(license: &str, dependencies: &[&str]) -> String {
    let mut text = format!(
        "[package]\nname = \"fx\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\
         license = \"{license}\"\npublish = false\n\n[dependencies]\n"
    );
    for dependency in dependencies {
        text.push_str(dependency);
        text.push('\n');
    }
    text
}

fn cargo_bin() -> OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"))
}

fn cargo_command() -> Command {
    let mut command = Command::new(cargo_bin());
    command.env_remove("RUSTFLAGS");
    command
}

fn run(exec: &dyn Exec, command: &mut Command) -> Result<Output, String> {
    exec.output(command)
        .map_err(|error| format!("cannot run {command:?}: {error}"))
}

fn fresh_dir(root: &Path, name: &str) -> Result<PathBuf, String> {
    let dir = root.join("target").join("xtask").join(name);
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    }
    fs::create_dir_all(&dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    Ok(dir)
}

fn read_toml(path: &Path) -> Result<Table, String> {
    fs::read_to_string(path)
        .map_err(|error| format!("{}: {error}", path.display()))?
        .parse::<Table>()
        .map_err(|error| format!("{}: {error}", path.display()))
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    fs::write(path, text).map_err(|error| format!("{}: {error}", path.display()))
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    fs::copy(from, to)
        .map(drop)
        .map_err(|error| format!("{}: {error}", from.display()))
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "test code: a failed unwrap, index or sum is a failed test"
    )]

    use super::*;
    use crate::exec::{Fake, Reply};

    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf()
    }

    /// The value after `flag` in `command`'s arguments.
    fn after(command: &Command, flag: &str) -> String {
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let index = args.iter().position(|arg| arg == flag).unwrap();
        args[index + 1].clone()
    }

    /// clippy as the configuration makes it: each fixture fires the lint it expects.
    fn clippy_that_agrees(command: &Command) -> Reply {
        let package = after(command, "--package");
        let name = package.strip_prefix("fx-").unwrap();
        let fixtures = clippy_fixtures(&root().join(CLIPPY_FIXTURES)).unwrap();
        let fixture = fixtures
            .iter()
            .find(|fixture| fixture.name == name)
            .unwrap();
        match &fixture.expect {
            None => Reply::exit(0),
            Some(lint) => Reply {
                code: 101,
                stderr: format!(
                    "error: help: see https://rust-lang.github.io/x/{CLIPPY_LINT_ANCHOR}{lint}\n"
                ),
                ..Reply::default()
            },
        }
    }

    // One test, not two: every run of the check rebuilds the same scratch workspace.
    #[test]
    fn the_clippy_fixtures_pass_only_when_each_fires_its_lint_alone() {
        let fake = Fake::new(clippy_that_agrees);
        let report = clippy(&root(), &fake).unwrap();
        assert_eq!(report.problems, Vec::<String>::new());
        assert_eq!(report.passed.len(), fake.seen.borrow().len());
        let work = root().join("target/xtask/clippy-fixtures");
        assert!(work.join(".clippy.toml").is_file());
        assert!(
            fs::read_to_string(work.join("Cargo.toml"))
                .unwrap()
                .contains("[workspace.lints")
        );

        let silent = Fake::new(|_: &Command| Reply::exit(0));
        let report = clippy(&root(), &silent).unwrap();
        assert!(!report.problems.is_empty());
        let two = Fake::new(|_: &Command| Reply {
            code: 101,
            stderr: format!("{CLIPPY_LINT_ANCHOR}unwrap_used {CLIPPY_LINT_ANCHOR}panic"),
            ..Reply::default()
        });
        let report = clippy(&root(), &two).unwrap();
        assert_eq!(
            report.passed.len(),
            0,
            "every fixture fails: {:?}",
            report.passed
        );
        let cannot = Fake::new(|_: &Command| Reply {
            cannot_run: true,
            ..Reply::default()
        });
        assert!(clippy(&root(), &cannot).is_err());
    }

    /// cargo deny as `deny.toml` makes it: each case reports its error code.
    fn deny_that_agrees(command: &Command) -> Reply {
        let dir = command.get_current_dir().unwrap();
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let case = DENY_CASES.iter().find(|case| case.name == name).unwrap();
        match case.expect {
            None => Reply::exit(0),
            Some(code) => Reply {
                code: 1,
                stderr: format!("error[{code}]: refused\n"),
                ..Reply::default()
            },
        }
    }

    #[test]
    fn the_deny_cases_pass_when_each_reports_its_error() {
        let fake = Fake::new(deny_that_agrees);
        let report = deny(&root(), &fake).unwrap();
        assert_eq!(report.problems, Vec::<String>::new());
        assert_eq!(report.passed.len(), DENY_CASES.len());
        let wildcard = root().join("target/xtask/deny-fixtures/wildcard/fx/Cargo.toml");
        assert!(
            fs::read_to_string(wildcard)
                .unwrap()
                .contains("itoa = \"*\"")
        );
        let passes = Fake::new(|_: &Command| Reply::exit(0));
        let report = deny(&root(), &passes).unwrap();
        assert_eq!(
            report.passed,
            ["control (GPL-3.0-or-later) passes cargo deny check licenses bans sources"]
        );
        let refuses = Fake::new(|_: &Command| Reply::exit(1));
        let report = deny(&root(), &refuses).unwrap();
        assert!(report.passed.is_empty(), "{:?}", report.passed);
    }

    #[test]
    fn a_lint_fixture_crate_inherits_the_workspace_lints() {
        let manifest: Table = lint_member_manifest("fx-a").parse().unwrap();
        let name = manifest
            .get("package")
            .and_then(|package| package.get("name"));
        assert_eq!(name.and_then(Value::as_str), Some("fx-a"));
        let lints = manifest
            .get("lints")
            .and_then(|lints| lints.get("workspace"));
        assert_eq!(lints.and_then(Value::as_bool), Some(true));
    }

    #[test]
    fn cargo_is_the_one_running_xtask() {
        let expected = std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
        assert_eq!(cargo_bin(), expected);
        assert!(!cargo_bin().is_empty());
    }

    #[test]
    fn fixture_expectation_is_a_clippy_lint_or_pass() {
        let lint = parse_fixture(
            "a".to_owned(),
            "// expect: clippy::unwrap_used\n".to_owned(),
        );
        assert_eq!(lint.unwrap().expect.as_deref(), Some("unwrap_used"));
        let pass = parse_fixture("b".to_owned(), "// x\n// expect: pass\n".to_owned());
        assert_eq!(pass.unwrap().expect, None);
        assert!(parse_fixture("c".to_owned(), "fn f() {}".to_owned()).is_err());
        assert!(parse_fixture("d".to_owned(), "// expect: unwrap_used".to_owned()).is_err());
        assert!(parse_fixture("e".to_owned(), "// expect: clippy::".to_owned()).is_err());
    }

    #[test]
    fn every_clippy_fixture_in_the_repository_parses() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join("clippy");
        let fixtures = clippy_fixtures(&dir).unwrap();
        let expected: BTreeSet<&str> = fixtures
            .iter()
            .filter_map(|fixture| fixture.expect.as_deref())
            .collect();
        for lint in [
            "unwrap_used",
            "indexing_slicing",
            "arithmetic_side_effects",
            "cast_possible_truncation",
            "print_stdout",
            "allow_attributes_without_reason",
        ] {
            assert!(expected.contains(lint), "no fixture expects {lint}");
        }
        assert!(fixtures.iter().any(|fixture| fixture.expect.is_none()));
    }
}
