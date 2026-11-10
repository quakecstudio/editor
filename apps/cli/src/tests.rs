// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap, match or index is a failed test"
)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use qc_edict::system::{SYSTEM_GLOBALS, defs_source};
use qc_types::Etype;
use qs_workbench::FsLoader;
use serde_json::Value;

use super::{Exit, Open, Refused, Request, parse, run, run_with_input};

fn args(list: &[&str]) -> Vec<OsString> {
    list.iter().map(OsString::from).collect()
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
    assert_eq!(
        parse(&args(&["test", "p", "--filter", "fire", "--game", "/q"])).unwrap(),
        Request::Test {
            project: open("p", None),
            params: serde_json::json!({ "filter": "fire", "game": "/q" }),
        }
    );
    assert_eq!(
        parse(&args(&[
            "mcp",
            "p",
            "--read",
            "/r",
            "--write",
            "/w",
            "--read-only"
        ]))
        .unwrap(),
        Request::Mcp(super::McpRequest {
            project: open("p", None),
            read: vec![PathBuf::from("/r")],
            write: vec![PathBuf::from("/w")],
            read_only: true,
            game: None,
        })
    );
    assert_eq!(
        parse(&args(&["test"])).unwrap(),
        Request::Test {
            project: open(".", None),
            params: serde_json::json!({ "filter": null, "game": null }),
        }
    );
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

/// Gives the project in `dir` id's system globals and fields, and a body for each function the
/// engine calls, so the VM takes its program.
fn engine_defs(dir: &Path) {
    let mut defs = defs_source();
    for var in SYSTEM_GLOBALS
        .iter()
        .filter(|var| var.etype == Etype::Function)
    {
        defs.push_str(&format!("void() {} = {{}};\n", var.name));
    }
    defs.push_str("float speed = 300;\n");
    std::fs::write(dir.join("defs.qc"), defs).unwrap();
}

// spec QCT-4.2, QCT-4.3: `test` prints each test's verdict and name, the failure's message and
// the counts, and exits 1 when one failed and 0 when none did; `run project.test` answers the
// same tests as JSON
#[test]
fn test_runs_the_projects_tests() {
    let spec = ProjectSpec {
        world: "void(string e) error = #10;\nvoid() test_passes = {};\n\
            void() test_world = { error(\"the world is flat\"); };\n",
    };
    let dir = project("test", &spec);
    engine_defs(&dir);
    let (exit, out, err) = cli(&["test", text(&dir)]);
    assert_eq!(exit, Exit::Problems, "{out}{err}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.first(), Some(&"passed  test_passes"), "{out}");
    assert_eq!(lines.get(1), Some(&"FAILED  test_world"), "{out}");
    assert!(
        lines
            .get(2)
            .is_some_and(|l| l.starts_with("        ") && l.contains("the world is flat")),
        "{out}"
    );
    assert_eq!(
        lines.last(),
        Some(&"1 passed, 1 failed, 0 skipped"),
        "{out}"
    );
    let (exit, json, err) = cli(&["run", "project.test", "--project", text(&dir)]);
    assert_eq!(exit, Exit::Success, "{err}");
    let run: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(run["tests"][1]["name"], "test_world");
    assert_eq!(run["tests"][1]["verdict"], "failed");
    let message = run["tests"][1]["message"].as_str().unwrap();
    assert!(out.contains(message.lines().next().unwrap()), "{out}");
    let (exit, out, _) = cli(&["test", text(&dir), "--filter", "passes"]);
    assert_eq!(exit, Exit::Success, "no test failed");
    assert_eq!(out, "passed  test_passes\n1 passed, 0 failed, 0 skipped\n");
    let broken = project(
        "test-broken",
        &ProjectSpec {
            world: "void() test_world = { nowhere = 1; };\n",
        },
    );
    let (exit, _, err) = cli(&["test", text(&broken)]);
    assert_eq!(exit, Exit::Failure);
    assert!(err.contains("nowhere"), "{err}");
}

// No spec statement: P4-13, AGT-7, `mcp` opens the project and serves the agent's tools on
// stdin and stdout: a legacy client initializes, lists the tools and compiles, one answer per
// line; a folder with no project is refused before serving
#[test]
fn mcp_serves_the_agents_tools() {
    let dir = project("mcp", &ProjectSpec::default());
    let input = format!(
        "{}\n{}\n{}\n{}\n",
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"compile","arguments":{}}}"#,
    );
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let exit = run_with_input(
        &args(&["mcp", text(&dir)]),
        FsLoader,
        input.as_bytes(),
        &mut out,
        &mut err,
    );
    let out = String::from_utf8(out).unwrap();
    assert_eq!(exit, Exit::Success, "{}", String::from_utf8_lossy(&err));
    let mut answers: Vec<Value> = out
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    // rmcp may answer concurrent requests in any order; each answer carries its id.
    answers.sort_by_key(|answer| answer["id"].as_u64());
    assert_eq!(answers.len(), 3, "{out}");
    assert_eq!(answers[0]["result"]["protocolVersion"], "2025-06-18");
    let names: Vec<&str> = answers[1]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    for tool in [
        "compile",
        "project.symbols",
        "engine.inspect",
        "scenario.run",
    ] {
        assert!(names.contains(&tool), "{tool} in {names:?}");
    }
    assert_eq!(answers[2]["result"]["structuredContent"]["clean"], true);
    assert!(dir.join("build").exists(), "compile wrote the program");
    let empty = dir.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let exit = run_with_input(
        &args(&["mcp", text(&empty)]),
        FsLoader,
        "".as_bytes(),
        &mut out,
        &mut err,
    );
    assert_eq!(exit, Exit::Failure);
    assert!(out.is_empty(), "nothing on stdout but MCP messages");
    assert!(String::from_utf8_lossy(&err).contains("no QuakeC project here"));
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

/// A writer that refuses every write, as a closed pipe does.
struct Closed;

impl std::io::Write for Closed {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "closed",
        ))
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

// P2-RA review: output that cannot be written is a failure, said on the error output, for
// every subcommand that prints.
#[test]
fn output_that_cannot_be_written_is_a_failure() {
    let dir = project("closed", &ProjectSpec::default());
    for list in [
        &["commands"][..],
        &["check", text(&dir)][..],
        &["build", text(&dir)][..],
        &["run", "commands.list"][..],
    ] {
        let mut err = Vec::new();
        let exit = run(&args(list), FsLoader, &mut Closed, &mut err);
        assert_eq!(exit, Exit::Failure, "{list:?}");
        let err = String::from_utf8(err).unwrap();
        assert!(err.contains("cannot write the output"), "{list:?}: {err}");
    }
}

// P2-RA review: a warning is printed and counted, and does not fail the build or the check.
#[test]
fn warnings_are_printed_and_counted_but_do_not_fail() {
    let dir = project(
        "warning",
        &ProjectSpec {
            world: "void() worldspawn = { local float speed; speed = 1; };\n",
        },
    );
    let (exit, out, err) = cli(&["check", text(&dir)]);
    assert_eq!(exit, Exit::Success, "{out}{err}");
    let place = format!("{}:1:", dir.join("world.qc").display());
    assert!(
        out.contains(&place) && out.contains(": warning W0201: "),
        "{out}"
    );
    assert!(out.ends_with("0 errors, 1 warnings\n"), "{out}");
    let (exit, out, _) = cli(&["build", text(&dir)]);
    assert_eq!(exit, Exit::Success, "{out}");
    assert!(
        out.contains(": warning W0201: ") && out.contains("built progs to "),
        "{out}"
    );
}

// PRJ-2.1: a project that does not open, a directory that does not exist or a project file
// that is wrong, is a failure that says why, not a crash or an empty success.
#[test]
fn a_project_that_does_not_open_is_a_failure() {
    let dir = project("broken-toml", &ProjectSpec::default());
    let missing = dir.join("no-such-dir");
    let (exit, out, err) = cli(&["build", text(&missing)]);
    assert_eq!(exit, Exit::Failure, "{out}{err}");
    assert!(out.is_empty() && !err.is_empty(), "{err}");
    std::fs::write(dir.join("quakec.toml"), "[[module]]\nkind = \"nope\"\n").unwrap();
    for verb in ["build", "check"] {
        let (exit, out, err) = cli(&[verb, text(&dir)]);
        assert_eq!(exit, Exit::Failure, "{verb}: {out}{err}");
        assert!(err.contains("quakec.toml"), "{verb}: {err}");
    }
}

// P2-RA review: the parameters of `run` are a JSON object; any other JSON is refused by the
// command with a failure.
#[test]
fn run_refuses_parameters_that_are_not_an_object() {
    for params in ["[1]", "3", "\"x\"", "null"] {
        let (exit, out, err) = cli(&["run", "commands.list", params]);
        assert_eq!(exit, Exit::Failure, "{params}: {out}");
        assert!(err.starts_with("quakec-ide: "), "{params}: {err}");
    }
}

// PRJ-2.8 and the README: `--dialect` reads a project with no `quakec.toml` in that dialect,
// so a project file's dialect, vanilla when it names none, wins over the option.
#[test]
fn the_project_file_dialect_wins_over_the_option() {
    let dir = project(
        "toml-dialect",
        &ProjectSpec {
            world: "void() worldspawn = { float x = 1; x += 2; };\n",
        },
    );
    std::fs::write(
        dir.join("quakec.toml"),
        "[[module]]\nkind = \"ssqc\"\nsrc = \"progs.src\"\n",
    )
    .unwrap();
    assert_eq!(
        cli(&["check", text(&dir), "--dialect", "fteqcc"]).0,
        Exit::Problems
    );
    std::fs::write(
        dir.join("quakec.toml"),
        "[project]\ndialect = \"fteqcc\"\n\n[[module]]\nkind = \"ssqc\"\nsrc = \"progs.src\"\n",
    )
    .unwrap();
    assert_eq!(
        cli(&["check", text(&dir), "--dialect", "vanilla"]).0,
        Exit::Success
    );
}

// No spec statement: a project path that is not UTF-8 is refused with exit status 2 and a
// message naming it, not a panic.
#[cfg(unix)]
#[test]
fn a_path_that_is_not_utf8_is_refused() {
    use std::os::unix::ffi::OsStrExt as _;
    let path = OsString::from(std::ffi::OsStr::from_bytes(b"a\xff"));
    for verb in ["build", "check"] {
        let list = [OsString::from(verb), path.clone()];
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let exit = run(&list, FsLoader, &mut out, &mut err);
        assert_eq!(exit, Exit::Failure, "{verb}");
        let err = String::from_utf8(err).unwrap();
        assert!(err.contains("a\u{fffd}: the path is not Unicode"), "{err}");
    }
}

// No spec statement (P2-RA re-review): `run --project` with a path that is not UTF-8 is
// refused as `build` and `check` refuse it, before the command runs.
#[cfg(unix)]
#[test]
fn run_refuses_a_project_path_that_is_not_utf8() {
    use std::os::unix::ffi::OsStrExt as _;
    let path = OsString::from(std::ffi::OsStr::from_bytes(b"/tmp/q\xfe"));
    let list = [
        OsString::from("run"),
        OsString::from("commands.list"),
        OsString::from("--project"),
        path,
    ];
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(run(&list, FsLoader, &mut out, &mut err), Exit::Failure);
    assert!(out.is_empty(), "the command must not run");
    let err = String::from_utf8(err).unwrap();
    assert!(err.contains("the path is not Unicode"), "{err}");
}

// No spec statement (P2-RA re-review): an option value or a command id that is not UTF-8 is
// a wrong command line, exit status 2 with clap's message, not a panic.
#[cfg(unix)]
#[test]
fn an_argument_that_is_not_utf8_is_a_wrong_command_line() {
    use std::os::unix::ffi::OsStrExt as _;
    let bad = OsString::from(std::ffi::OsStr::from_bytes(b"fte\xff"));
    let word = |s: &str| OsString::from(s);
    for list in [
        vec![word("check"), word("."), word("--dialect"), bad.clone()],
        vec![word("run"), bad.clone()],
        vec![word("run"), word("commands.list"), bad],
    ] {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let exit = run(&list, FsLoader, &mut out, &mut err);
        assert_eq!(exit, Exit::Failure, "{list:?}");
        assert!(!err.is_empty(), "{list:?}");
    }
}
