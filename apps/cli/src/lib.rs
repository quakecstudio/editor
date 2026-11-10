// SPDX-License-Identifier: GPL-3.0-or-later

//! `quakec-ide`, QuakeC Studio's command line (GOAL-2, GOAL-4, ADR-0006): it builds and checks
//! a QuakeC project and runs any command of the IDE by its id, through the same command
//! registry the window, the palette and the agent use (PRIN-3), so the command line does
//! nothing the IDE does not.
//!
//! ```text
//! quakec-ide build [<project>] [--dialect <vanilla|fteqcc>]
//! quakec-ide check [<project>] [--dialect <vanilla|fteqcc>]
//! quakec-ide test [<project>] [--dialect <vanilla|fteqcc>] [--filter <text>] [--game <folder>]
//! quakec-ide run <command> [<json>] [--project <project> [--dialect <vanilla|fteqcc>]]
//! quakec-ide mcp [<project>] [--dialect <vanilla|fteqcc>] [--read <dir>]... [--write <dir>]...
//!                [--read-only] [--game <folder>]
//! quakec-ide commands
//! ```
//!
//! `build` opens the project (`project.open`), compiles each module and writes its progs.dat
//! and .lno (`project.build`), and prints each module's errors and warnings; `check` prints
//! the project's problems (`project.check`). `test` runs the project's tests (`project.test`,
//! QCT-4.3) and prints each one's verdict. `run` runs the command `<command>` with the JSON
//! object `<json>` as its parameters (`{}` when absent), after opening `--project` when given,
//! and prints what it returned as JSON. `commands` lists every command (`commands.list`). The
//! project is the current directory when none is named; `--dialect` reads a project with no
//! `quakec.toml` in that dialect (PRJ-2.8), vanilla otherwise. The arguments are read with
//! clap, which prints `--help` and `--version` and says what is wrong with a command line.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::PathBuf;

use clap::error::ErrorKind;
use clap::{Parser, Subcommand, ValueEnum};

use qs_commands::{CommandError, Registry, Session};
use qs_mcp::{AGENT_TOOLS, Server, ServerSettings};
use qs_workbench::{
    BUILD_PROJECT, CHECK_PROJECT, Config, Dialect, Gravity, HasPreview, HasWorkbench, Loader,
    ModuleBuilt, OPEN_PROJECT, Preview, PreviewConfig, RUN_TESTS, Reported, TestRun, Verdict,
    Workbench, register_preview,
};
use serde_json::{Value, json};

/// How a run of the command line ended, its process exit status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    /// It did what was asked: the build wrote every module, the check found no error, the
    /// command ran.
    Success,
    /// The build or the check found errors.
    Problems,
    /// The command line was wrong, the project did not open, or a command failed.
    Failure,
}

impl Exit {
    /// The process exit status: 0, 1 and 2.
    #[must_use]
    pub const fn number(self) -> u8 {
        match self {
            Self::Success => 0,
            Self::Problems => 1,
            Self::Failure => 2,
        }
    }
}

/// The command line as clap reads it.
#[derive(Debug, Parser)]
#[command(
    name = "quakec-ide",
    version,
    about = "QuakeC Studio's command line: build and check a QuakeC project, and run any \
             command of the IDE by its id"
)]
struct Args {
    #[command(subcommand)]
    verb: Verb,
}

/// The subcommands.
#[derive(Debug, Subcommand)]
enum Verb {
    /// Compile each module of a project and write its progs.dat and .lno.
    Build(ProjectArgs),
    /// List every error and warning of a project.
    Check(ProjectArgs),
    /// Build a project and run its tests: its test_ functions and its tests/*.scenario files.
    Test {
        /// The project.
        #[command(flatten)]
        project: ProjectArgs,
        /// Only the tests whose name contains this text.
        #[arg(long)]
        filter: Option<String>,
        /// The game folder scenario tests play over; the first install found otherwise.
        #[arg(long)]
        game: Option<PathBuf>,
    },
    /// Run any command by its id with JSON parameters, and print what it returns as JSON.
    Run {
        /// The command's id, as `quakec-ide commands` lists it.
        command: String,
        /// The parameters, a JSON object; `{}` when absent.
        params: Option<String>,
        /// A project to open first.
        #[arg(long)]
        project: Option<PathBuf>,
        /// The dialect a project with no quakec.toml is read in.
        #[arg(long, value_enum, requires = "project")]
        dialect: Option<DialectArg>,
    },
    /// List every command with its id, summary and parameter schema, as JSON.
    Commands,
    /// Serve the agent's tools over MCP on stdin and stdout, for Claude Code and other agents.
    Mcp {
        /// The project.
        #[command(flatten)]
        project: ProjectArgs,
        /// A directory the tools may read under, besides the project; repeatable.
        #[arg(long)]
        read: Vec<PathBuf>,
        /// A directory the tools may write under, besides the project; repeatable.
        #[arg(long)]
        write: Vec<PathBuf>,
        /// The project is read only: no tool that builds, edits or plays may run.
        #[arg(long)]
        read_only: bool,
        /// The game folder the preview and scenarios play over; the first install otherwise.
        #[arg(long)]
        game: Option<PathBuf>,
    },
}

/// The project a subcommand opens.
#[derive(Debug, clap::Args)]
struct ProjectArgs {
    /// The project directory: the one holding quakec.toml, or progs.src and csprogs.src.
    #[arg(default_value = ".")]
    project: PathBuf,
    /// The dialect a project with no quakec.toml is read in; vanilla otherwise.
    #[arg(long, value_enum)]
    dialect: Option<DialectArg>,
}

/// The dialects `--dialect` names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum DialectArg {
    /// id's QuakeC.
    Vanilla,
    /// FTEQCC's QuakeC.
    Fteqcc,
}

impl DialectArg {
    /// The project dialect it names.
    const fn dialect(self) -> Dialect {
        match self {
            Self::Vanilla => Dialect::Vanilla,
            Self::Fteqcc => Dialect::Fteqcc,
        }
    }
}

/// What the command line asks for.
#[derive(Clone, Debug, PartialEq)]
enum Request {
    /// Build the project.
    Build(Open),
    /// Check the project.
    Check(Open),
    /// Run the project's tests with these `project.test` parameters.
    Test { project: Open, params: Value },
    /// Run one command, after opening a project when one is named.
    Run {
        command: String,
        params: Value,
        project: Option<Open>,
    },
    /// List every command.
    Commands,
    /// Serve the agent's tools over MCP.
    Mcp(McpRequest),
}

/// What `mcp` serves.
#[derive(Clone, Debug, PartialEq, Eq)]
struct McpRequest {
    project: Open,
    read: Vec<PathBuf>,
    write: Vec<PathBuf>,
    read_only: bool,
    game: Option<PathBuf>,
}

/// A project to open, and the dialect to read it in when its project file does not say.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Open {
    path: PathBuf,
    dialect: Option<Dialect>,
}

/// What the command line's commands work on.
#[derive(Debug)]
struct Cli<L> {
    workbench: Workbench<L>,
}

impl<L: Loader> HasWorkbench for Cli<L> {
    type Loader = L;

    fn workbench(&mut self) -> &mut Workbench<L> {
        &mut self.workbench
    }
}

/// Runs the command line `args`, reading files through `loader`, writing what it prints to
/// `out` and its errors to `err`.
pub fn run<L: Loader + Send + 'static>(
    args: &[OsString],
    loader: L,
    out: &mut impl Write,
    err: &mut impl Write,
) -> Exit {
    run_on(args, loader, McpInput::Stdio, out, err)
}

/// [`run`], `mcp` reading the client's messages from `input` in place of stdin and writing its
/// answers to `out` once `input` ends.
pub fn run_with_input<L: Loader + Send + 'static>(
    args: &[OsString],
    loader: L,
    mut input: impl Read,
    out: &mut impl Write,
    err: &mut impl Write,
) -> Exit {
    let mut bytes = Vec::new();
    if let Err(error) = input.read_to_end(&mut bytes) {
        writeln!(err, "quakec-ide: {error}").unwrap_or(());
        return Exit::Failure;
    }
    run_on(args, loader, McpInput::Bytes(bytes), out, err)
}

/// [`run`], with `mcp` reading from `input`.
fn run_on<L: Loader + Send + 'static>(
    args: &[OsString],
    loader: L,
    input: McpInput,
    out: &mut impl Write,
    err: &mut impl Write,
) -> Exit {
    let request = match parse(args) {
        Ok(request) => request,
        Err(Refused::Shown(text)) => {
            // Nothing more can be told when the output itself fails.
            write!(out, "{text}").unwrap_or(());
            return Exit::Success;
        }
        Err(Refused::Wrong(text)) => {
            write!(err, "{text}").unwrap_or(());
            return Exit::Failure;
        }
    };
    let dialect = match &request {
        Request::Build(open) | Request::Check(open) | Request::Test { project: open, .. } => {
            open.dialect
        }
        Request::Run { project, .. } => project.as_ref().and_then(|open| open.dialect),
        Request::Commands => None,
        Request::Mcp(mcp) => mcp.project.dialect,
    };
    let config = Config {
        dialect,
        ..Config::default()
    };
    if let Request::Mcp(mcp) = &request {
        return match serve_mcp(mcp, loader, config, input, out) {
            Ok(()) => Exit::Success,
            Err(error) => {
                writeln!(err, "quakec-ide: {error}").unwrap_or(());
                Exit::Failure
            }
        };
    }
    let mut registry = Registry::new();
    if let Err(error) = qs_workbench::register(&mut registry) {
        // Nothing more can be told when the error output itself fails.
        writeln!(err, "quakec-ide: {error}").unwrap_or(());
        return Exit::Failure;
    }
    let mut session = Session::new(
        Cli {
            workbench: Workbench::new(loader, config),
        },
        registry,
    );
    match execute(&mut session, &request, out) {
        Ok(exit) => exit,
        Err(error) => {
            // Nothing more can be told when the error output itself fails.
            writeln!(err, "quakec-ide: {error}").unwrap_or(());
            Exit::Failure
        }
    }
}

/// Why a request did not finish.
#[derive(Debug)]
enum Stopped {
    /// A command failed.
    Command(CommandError),
    /// What a command returned is not what it promises.
    Output(serde_json::Error),
    /// Writing the output failed.
    Write(std::io::Error),
    /// The directory holds no QuakeC project.
    NoProject(PathBuf),
    /// A path that is not Unicode, which no command's JSON parameters can name.
    NotUnicode(PathBuf),
    /// The commands could not be registered.
    Register(String),
    /// The MCP server stopped.
    Mcp(qs_mcp::McpError),
}

impl core::fmt::Display for Stopped {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Command(error) => error.fmt(f),
            Self::Output(error) => {
                write!(f, "a command returned what it does not promise: {error}")
            }
            Self::Write(error) => write!(f, "cannot write the output: {error}"),
            Self::Register(error) => write!(f, "the commands could not be registered: {error}"),
            Self::Mcp(error) => error.fmt(f),
            Self::NotUnicode(path) => write!(
                f,
                "{}: the path is not Unicode, and the IDE's commands take paths as text",
                path.display()
            ),
            Self::NoProject(path) => write!(
                f,
                "{}: no QuakeC project here: no quakec.toml, progs.src or csprogs.src",
                path.display()
            ),
        }
    }
}

/// Runs `request` in `session`, printing to `out`.
fn execute<L: Loader + 'static>(
    session: &mut Session<Cli<L>>,
    request: &Request,
    out: &mut impl Write,
) -> Result<Exit, Stopped> {
    let mut call = |id: &str, params: Value| session.execute(id, params).map_err(Stopped::Command);
    // A directory with no project opens, as the window opens one, but there is nothing to
    // build or check in it.
    let mut open = |open: &Open| {
        // A command's parameters are JSON, whose strings are Unicode: a path that is not is
        // refused here, before the registry is asked.
        let path = open
            .path
            .to_str()
            .ok_or_else(|| Stopped::NotUnicode(open.path.clone()))?;
        let opened = call(OPEN_PROJECT, json!({ "path": path }))?;
        match opened.get("files").and_then(Value::as_u64) {
            Some(0) => Err(Stopped::NoProject(open.path.clone())),
            _ => Ok(()),
        }
    };
    match request {
        Request::Build(project) => {
            open(project)?;
            let built: Vec<ModuleBuilt> = serde_json::from_value(
                session
                    .execute(BUILD_PROJECT, json!({}))
                    .map_err(Stopped::Command)?,
            )
            .map_err(Stopped::Output)?;
            let mut exit = Exit::Success;
            for module in &built {
                print_problems(out, &module.problems)?;
                if module.built {
                    writeln!(out, "built {} to {}", module.name, module.output.display())
                } else {
                    exit = Exit::Problems;
                    writeln!(out, "{} has errors; nothing was written", module.name)
                }
                .map_err(Stopped::Write)?;
            }
            Ok(exit)
        }
        Request::Check(project) => {
            open(project)?;
            let problems: Vec<Reported> = serde_json::from_value(
                session
                    .execute(CHECK_PROJECT, json!({}))
                    .map_err(Stopped::Command)?,
            )
            .map_err(Stopped::Output)?;
            print_problems(out, &problems)?;
            let errors = problems
                .iter()
                .filter(|p| p.gravity == Gravity::Error)
                .count();
            let warnings = problems.len().saturating_sub(errors);
            writeln!(out, "{errors} errors, {warnings} warnings").map_err(Stopped::Write)?;
            Ok(if errors == 0 {
                Exit::Success
            } else {
                Exit::Problems
            })
        }
        Request::Test { project, params } => {
            open(project)?;
            let run: TestRun = serde_json::from_value(
                session
                    .execute(RUN_TESTS, params.clone())
                    .map_err(Stopped::Command)?,
            )
            .map_err(Stopped::Output)?;
            print_tests(out, &run)?;
            Ok(if run.passed {
                Exit::Success
            } else {
                Exit::Problems
            })
        }
        Request::Run {
            command,
            params,
            project,
        } => {
            if let Some(project) = project {
                open(project)?;
            }
            let output = session
                .execute(command, params.clone())
                .map_err(Stopped::Command)?;
            print_json(out, &output)?;
            Ok(Exit::Success)
        }
        // `mcp` is served before a session of the command line's own is made.
        Request::Mcp(_) => Ok(Exit::Success),
        Request::Commands => {
            let output = session
                .execute("commands.list", json!({}))
                .map_err(Stopped::Command)?;
            print_json(out, &output)?;
            Ok(Exit::Success)
        }
    }
}

/// Writes each test of `run` to `out` as `<verdict> <name>`, the message of one that failed or
/// was skipped indented below it, then the counts (QCT-4.3).
fn print_tests(out: &mut impl Write, run: &TestRun) -> Result<(), Stopped> {
    for test in &run.tests {
        let verdict = match test.verdict {
            Verdict::Passed => "passed ",
            Verdict::Failed => "FAILED ",
            Verdict::Skipped => "skipped",
        };
        writeln!(out, "{verdict} {}", test.name).map_err(Stopped::Write)?;
        for line in test.message.iter().flat_map(|message| message.lines()) {
            writeln!(out, "        {line}").map_err(Stopped::Write)?;
        }
    }
    writeln!(
        out,
        "{} passed, {} failed, {} skipped",
        run.passed_count, run.failed_count, run.skipped_count
    )
    .map_err(Stopped::Write)
}

/// What `mcp` serves: the workbench and the preview, as the window has them.
struct McpContext<L> {
    workbench: Workbench<L>,
    preview: Preview,
}

impl<L: Loader> HasWorkbench for McpContext<L> {
    type Loader = L;

    fn workbench(&mut self) -> &mut Workbench<L> {
        &mut self.workbench
    }
}

impl<L: Loader> HasPreview for McpContext<L> {
    fn workbench_and_preview(&mut self) -> (&mut Workbench<L>, &mut Preview) {
        (&mut self.workbench, &mut self.preview)
    }
}

/// Where `mcp` reads the client's messages from.
enum McpInput {
    /// stdin, answering on stdout, until stdin ends.
    Stdio,
    /// These bytes, answering into `run`'s output once every call is answered: for tests.
    Bytes(Vec<u8>),
}

/// Opens `mcp`'s project on the session's own thread and serves the agent's tools on `input`
/// until it ends.
fn serve_mcp<L: Loader + Send + 'static>(
    mcp: &McpRequest,
    loader: L,
    config: Config,
    input: McpInput,
    out: &mut impl Write,
) -> Result<(), Stopped> {
    let project = std::path::absolute(&mcp.project.path)
        .map_err(|_| Stopped::NoProject(mcp.project.path.clone()))?;
    let path = project
        .to_str()
        .ok_or_else(|| Stopped::NotUnicode(project.clone()))?
        .to_owned();
    let mut preview_config = PreviewConfig::from_env(std::env::current_exe().unwrap_or_default());
    if let Some(game) = &mcp.game {
        preview_config.game = Some(game.clone());
    }
    let no_project = Stopped::NoProject(project.clone()).to_string();
    let make = move || {
        let mut registry = Registry::new();
        qs_workbench::register(&mut registry)
            .and_then(|()| register_preview(&mut registry))
            .map_err(|error| Stopped::Register(error.to_string()).to_string())?;
        let mut session = Session::new(
            McpContext {
                workbench: Workbench::new(loader, config),
                preview: Preview::new(preview_config),
            },
            registry,
        );
        let opened = session
            .execute(OPEN_PROJECT, json!({ "path": path }))
            .map_err(|error| Stopped::Command(error).to_string())?;
        if opened.get("files").and_then(Value::as_u64) == Some(0) {
            return Err(no_project);
        }
        Ok(session)
    };
    let mut read = vec![project.clone()];
    read.extend(mcp.read.iter().cloned());
    let mut write = mcp.write.clone();
    if !mcp.read_only {
        write.push(project.clone());
    }
    let settings = ServerSettings {
        project,
        read,
        write,
        name: "quakec-ide".to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
    };
    let server = Server::start(make, settings, &AGENT_TOOLS).map_err(Stopped::Mcp)?;
    match input {
        McpInput::Stdio => server.serve_stdio().map_err(Stopped::Mcp),
        McpInput::Bytes(bytes) => {
            let answers = server.serve_bytes(bytes).map_err(Stopped::Mcp)?;
            out.write_all(&answers).map_err(Stopped::Write)
        }
    }
}

/// Writes `value` to `out` as indented JSON and a line break.
fn print_json(out: &mut impl Write, value: &Value) -> Result<(), Stopped> {
    let text = serde_json::to_string_pretty(value).map_err(Stopped::Output)?;
    writeln!(out, "{text}").map_err(Stopped::Write)
}

/// Writes each of `problems` to `out` as `<path>:<line>:<column>: <gravity> <code>: <message>`,
/// as compilers print them.
fn print_problems(out: &mut impl Write, problems: &[Reported]) -> Result<(), Stopped> {
    for problem in problems {
        let gravity = match problem.gravity {
            Gravity::Error => "error",
            Gravity::Warning => "warning",
        };
        let place = problem.place.as_ref().map_or_else(String::new, |place| {
            format!(
                "{}:{}:{}: ",
                place.path.display(),
                place.location.line(),
                place.location.column()
            )
        });
        writeln!(
            out,
            "{place}{gravity} {}: {}",
            problem.code, problem.message
        )
        .map_err(Stopped::Write)?;
    }
    Ok(())
}

/// Why the command line ran no request.
#[derive(Debug, PartialEq, Eq)]
enum Refused {
    /// It asked for the help or the version, this text.
    Shown(String),
    /// It is wrong, as this text says, with the usage.
    Wrong(String),
}

/// The request `args`, the arguments after the program's name, make.
fn parse(args: &[OsString]) -> Result<Request, Refused> {
    let program = std::iter::once(OsString::from("quakec-ide"));
    let parsed = Args::try_parse_from(program.chain(args.iter().cloned())).map_err(|error| {
        let text = error.render().to_string();
        match error.kind() {
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => Refused::Shown(text),
            _ => Refused::Wrong(text),
        }
    })?;
    let open = |project: PathBuf, dialect: Option<DialectArg>| Open {
        path: project,
        dialect: dialect.map(DialectArg::dialect),
    };
    Ok(match parsed.verb {
        Verb::Build(args) => Request::Build(open(args.project, args.dialect)),
        Verb::Check(args) => Request::Check(open(args.project, args.dialect)),
        Verb::Test {
            project,
            filter,
            game,
        } => Request::Test {
            project: open(project.project, project.dialect),
            params: json!({ "filter": filter, "game": game }),
        },
        Verb::Run {
            command,
            params,
            project,
            dialect,
        } => {
            let params = match params {
                Some(text) => serde_json::from_str(&text).map_err(|e| {
                    Refused::Wrong(format!(
                        "error: the parameters are not JSON: {e}\n\nFor more information, try \
                         'quakec-ide run --help'.\n"
                    ))
                })?,
                None => json!({}),
            };
            Request::Run {
                command,
                params,
                project: project.map(|path| open(path, dialect)),
            }
        }
        Verb::Commands => Request::Commands,
        Verb::Mcp {
            project,
            read,
            write,
            read_only,
            game,
        } => Request::Mcp(McpRequest {
            project: open(project.project, project.dialect),
            read,
            write,
            read_only,
            game,
        }),
    })
}

#[cfg(test)]
mod tests;
