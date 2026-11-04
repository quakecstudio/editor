// SPDX-License-Identifier: GPL-3.0-or-later

//! `quakec-ide`, QuakeC Studio's command line (GOAL-2, GOAL-4, ADR-0006): it builds and checks
//! a QuakeC project and runs any command of the IDE by its id, through the same command
//! registry the window, the palette and the agent use (PRIN-3), so the command line does
//! nothing the IDE does not.
//!
//! ```text
//! quakec-ide build [<project>] [--dialect <vanilla|fteqcc>]
//! quakec-ide check [<project>] [--dialect <vanilla|fteqcc>]
//! quakec-ide run <command> [<json>] [--project <project> [--dialect <vanilla|fteqcc>]]
//! quakec-ide commands
//! ```
//!
//! `build` opens the project (`project.open`), compiles each module and writes its progs.dat
//! and .lno (`project.build`), and prints each module's errors and warnings; `check` prints
//! the project's problems (`project.check`). `run` runs the command `<command>` with the JSON
//! object `<json>` as its parameters (`{}` when absent), after opening `--project` when given,
//! and prints what it returned as JSON. `commands` lists every command (`commands.list`). The
//! project is the current directory when none is named; `--dialect` reads a project with no
//! `quakec.toml` in that dialect (PRJ-2.8), vanilla otherwise. The arguments are read with
//! clap, which prints `--help` and `--version` and says what is wrong with a command line.

use std::io::Write;
use std::path::PathBuf;

use clap::error::ErrorKind;
use clap::{Parser, Subcommand, ValueEnum};

use qs_commands::{CommandError, Registry, Session};
use qs_workbench::{
    BUILD_PROJECT, CHECK_PROJECT, Config, Dialect, Gravity, HasWorkbench, Loader, ModuleBuilt,
    OPEN_PROJECT, Reported, Workbench,
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
    /// Run one command, after opening a project when one is named.
    Run {
        command: String,
        params: Value,
        project: Option<Open>,
    },
    /// List every command.
    Commands,
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
pub fn run<L: Loader + 'static>(
    args: &[String],
    loader: L,
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
        Request::Build(open) | Request::Check(open) => open.dialect,
        Request::Run { project, .. } => project.as_ref().and_then(|open| open.dialect),
        Request::Commands => None,
    };
    let config = Config {
        dialect,
        ..Config::default()
    };
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
}

impl core::fmt::Display for Stopped {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Command(error) => error.fmt(f),
            Self::Output(error) => {
                write!(f, "a command returned what it does not promise: {error}")
            }
            Self::Write(error) => write!(f, "cannot write the output: {error}"),
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
        let opened = call(OPEN_PROJECT, json!({ "path": open.path }))?;
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
        Request::Commands => {
            let output = session
                .execute("commands.list", json!({}))
                .map_err(Stopped::Command)?;
            print_json(out, &output)?;
            Ok(Exit::Success)
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
fn parse(args: &[String]) -> Result<Request, Refused> {
    let program = std::iter::once("quakec-ide".to_owned());
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
    })
}

#[cfg(test)]
mod tests;
