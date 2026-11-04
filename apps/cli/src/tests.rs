// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test code: a failed unwrap or match is a failed test"
)]

use std::path::{Path, PathBuf};

use qs_workbench::FsLoader;
use serde_json::Value;

use super::{Exit, Open, Refused, Request, parse, run};

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

/// What a test project holds; the default is one that builds.
struct ProjectSpec {
    world: &'static str,
}

impl Default for ProjectSpec {
    fn default() -> Self {
        Self {
            world: "void() worldspawn = { local float x; x = speed * 2; };\n",
        }
    }
}

/// A bare project in a fresh directory for the test `name`.
fn project(name: &str, spec: &ProjectSpec) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("quakec-ide-{name}-{}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    std::fs::create_dir_all(&dir).unwrap();
    for (file, text) in [
        ("progs.src", "progs.dat\ndefs.qc\nworld.qc\n"),
        ("defs.qc", "float speed = 300;\n"),
        ("world.qc", spec.world),
    ] {
        std::fs::write(dir.join(file), text).unwrap();
    }
    dir
}

/// Runs the command line `list` and gives how it ended, what it printed and its errors.
fn cli(list: &[&str]) -> (Exit, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let exit = run(&args(list), FsLoader, &mut out, &mut err);
    (
        exit,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

fn text(path: &Path) -> &str {
    path.to_str().unwrap()
}

#[test]
fn the_exit_statuses_are_0_1_and_2() {
    assert_eq!(
        [Exit::Success, Exit::Problems, Exit::Failure].map(Exit::number),
        [0, 1, 2]
    );
}

#[test]
fn each_command_line_is_read() {
    let open = |path: &str, dialect| Open {
        path: PathBuf::from(path),
        dialect,
    };
    assert_eq!(
        parse(&args(&["build"])).unwrap(),
        Request::Build(open(".", None))
    );
    assert_eq!(
        parse(&args(&["check", "mod", "--dialect", "fteqcc"])).unwrap(),
        Request::Check(open("mod", Some(qs_workbench::Dialect::Fteqcc)))
    );
    assert_eq!(
        parse(&args(&[
            "run",
            "symbol.hover",
            r#"{"a": 1}"#,
            "--project",
            "p"
        ]))
        .unwrap(),
        Request::Run {
            command: "symbol.hover".to_owned(),
            params: serde_json::json!({ "a": 1 }),
            project: Some(open("p", None)),
        }
    );
    assert_eq!(
        parse(&args(&["run", "commands.list"])).unwrap(),
        Request::Run {
            command: "commands.list".to_owned(),
            params: serde_json::json!({}),
            project: None,
        }
    );
    assert_eq!(parse(&args(&["commands"])).unwrap(), Request::Commands);
}

#[test]
fn a_wrong_command_line_is_refused() {
    for (list, says) in [
        (&[][..], "Usage: quakec-ide <COMMAND>"),
        (&["frobnicate"][..], "unrecognized subcommand 'frobnicate'"),
        (&["build", "a", "b"][..], "unexpected argument 'b'"),
        (
            &["build", "--project", "p"][..],
            "unexpected argument '--project'",
        ),
        (
            &["build", "--dialect"][..],
            "a value is required for '--dialect",
        ),
        (
            &["build", "--dialect", "gmqcc"][..],
            "invalid value 'gmqcc'",
        ),
        (&["check", "--fast"][..], "unexpected argument '--fast'"),
        (
            &["run"][..],
            "the following required arguments were not provided",
        ),
        (&["run", "a.b", "{"][..], "the parameters are not JSON"),
        (&["run", "a.b", "{}", "c"][..], "unexpected argument 'c'"),
        (
            &["run", "a.b", "--dialect", "fteqcc"][..],
            "--project <PROJECT>",
        ),
        (&["commands", "x"][..], "unexpected argument 'x'"),
        (
            &["commands", "--dialect", "fteqcc"][..],
            "unexpected argument '--dialect'",
        ),
    ] {
        let Err(Refused::Wrong(error)) = parse(&args(list)) else {
            panic!("{list:?} was not refused");
        };
        assert!(error.contains(says), "{list:?}: {error}");
        let (exit, out, err) = cli(list);
        assert_eq!(exit, Exit::Failure);
        assert!(out.is_empty());
        assert!(err.contains(says), "{err}");
    }
}

#[test]
fn help_and_version_are_printed_and_succeed() {
    let (exit, out, err) = cli(&["--help"]);
    assert_eq!(exit, Exit::Success);
    assert!(err.is_empty());
    for subcommand in ["build", "check", "run", "commands"] {
        assert!(out.contains(subcommand), "{out}");
    }
    let (exit, out, _) = cli(&["build", "--help"]);
    assert_eq!(exit, Exit::Success);
    assert!(out.contains("--dialect <DIALECT>"), "{out}");
    let (exit, out, _) = cli(&["--version"]);
    assert_eq!(exit, Exit::Success);
    assert_eq!(out, format!("quakec-ide {}\n", env!("CARGO_PKG_VERSION")));
}

#[test]
fn build_writes_the_program_and_says_where() {
    let dir = project("build", &ProjectSpec::default());
    let (exit, out, err) = cli(&["build", text(&dir)]);
    assert_eq!(exit, Exit::Success, "{out}{err}");
    let output = dir
        .join("build")
        .join(dir.file_name().unwrap())
        .join("progs.dat");
    assert!(output.is_file());
    assert!(output.with_extension("lno").is_file());
    assert!(
        out.contains(&format!("built progs to {}", output.display())),
        "{out}"
    );
}

#[test]
fn errors_are_printed_as_compilers_print_them() {
    let dir = project(
        "errors",
        &ProjectSpec {
            world: "void() worldspawn = {\n  missing = 1;\n};\n",
        },
    );
    let (exit, out, _) = cli(&["build", text(&dir)]);
    assert_eq!(exit, Exit::Problems);
    let place = format!("{}:2:3: error ", dir.join("world.qc").display());
    assert!(out.contains(&place), "{out}");
    assert!(
        out.contains("progs has errors; nothing was written"),
        "{out}"
    );
    let (exit, out, _) = cli(&["check", text(&dir)]);
    assert_eq!(exit, Exit::Problems);
    assert!(out.contains(&place), "{out}");
    assert!(out.ends_with("1 errors, 0 warnings\n"), "{out}");
}

#[test]
fn check_passes_a_project_with_no_error() {
    let dir = project("check", &ProjectSpec::default());
    let (exit, out, _) = cli(&["check", text(&dir)]);
    assert_eq!(exit, Exit::Success);
    assert_eq!(out, "0 errors, 0 warnings\n");
}

#[test]
fn the_dialect_option_reads_a_bare_project_as_fteqcc() {
    let dir = project(
        "dialect",
        &ProjectSpec {
            world: "void() worldspawn = { float x = 1; x += 2; };\n",
        },
    );
    assert_eq!(cli(&["build", text(&dir)]).0, Exit::Problems);
    assert_eq!(
        cli(&["build", text(&dir), "--dialect", "fteqcc"]).0,
        Exit::Success
    );
}

#[test]
fn run_runs_any_command_and_prints_its_json() {
    let dir = project("run", &ProjectSpec::default());
    let (exit, out, err) = cli(&["run", "project.check", "--project", text(&dir)]);
    assert_eq!(exit, Exit::Success, "{err}");
    assert_eq!(
        serde_json::from_str::<Value>(&out).unwrap(),
        serde_json::json!([])
    );
    let (exit, _, err) = cli(&["run", "no.such"]);
    assert_eq!(exit, Exit::Failure);
    assert!(err.contains("no.such"), "{err}");
    let (exit, _, err) = cli(&["run", "project.check"]);
    assert_eq!(exit, Exit::Failure);
    assert!(err.contains("no project is open"), "{err}");
    let empty = dir.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    for verb in ["build", "check"] {
        let (exit, _, err) = cli(&[verb, text(&empty)]);
        assert_eq!(exit, Exit::Failure);
        assert!(err.contains("no QuakeC project here"), "{err}");
    }
}

#[test]
fn commands_lists_every_command() {
    let (exit, out, _) = cli(&["commands"]);
    assert_eq!(exit, Exit::Success);
    let list: Value = serde_json::from_str(&out).unwrap();
    let ids: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    for id in [
        "commands.list",
        "project.open",
        "project.build",
        "project.check",
    ] {
        assert!(ids.contains(&id), "{ids:?}");
    }
}
