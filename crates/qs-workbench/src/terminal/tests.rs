// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::integer_division,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::path::PathBuf;

use qc_project::FsLoader;
use qs_commands::{Registry, Session};
use serde_json::json;

use super::{Exit, KEPT_OUTPUT, Running, TASK_LIST, TASK_RUN, TERMINAL_CLOSE, TERMINAL_NEW};
use super::{TERMINAL_POLL, TERMINAL_SEND, Terminal};
use crate::{Config, Workbench, register};

/// A project for test `name` with `tasks` in its `quakec.toml`: its directory.
fn project(name: &str, tasks: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("qs-terminal-{name}-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("quakec.toml"),
        format!("[[module]]\nkind = \"ssqc\"\nsrc = \"progs.src\"\n{tasks}"),
    )
    .unwrap();
    std::fs::write(root.join("progs.src"), "progs.dat\nworld.qc\n").unwrap();
    std::fs::write(root.join("world.qc"), "void() worldspawn = {};\n").unwrap();
    root
}

/// A session over the workbench with `root` open.
fn session(root: &PathBuf) -> Session<Workbench<FsLoader>> {
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut session = Session::new(Workbench::new(FsLoader, Config::default()), registry);
    session
        .execute("project.open", json!({ "path": root }))
        .unwrap();
    session
}

/// A command line that prints `text` and exits with `code`, in every OS's shell.
fn print_and_exit(text: &str, code: i32) -> String {
    format!("echo {text}&& exit {code}")
}

// spec PRJ-2.11: a task runs in the user's shell in the project directory and ends with the
// shell's exit status, its output kept
#[test]
fn a_task_runs_in_the_project_directory_and_its_exit_status_is_kept() {
    let root = project("status", "");
    let running = Running::Task {
        name: "fail".to_owned(),
        command: print_and_exit("hello", 3),
    };
    let mut tab = Terminal::start(running, &root).unwrap();
    assert_eq!(tab.title(), "fail");
    assert_eq!(tab.wait().unwrap(), Exit::Code(3));
    assert!(!Exit::Code(3).succeeded());
    assert!(tab.text().contains("hello"), "{:?}", tab.text());
    assert!(
        tab.text().starts_with("> echo hello"),
        "the command line is shown first"
    );
    let pwd = if cfg!(windows) { "cd" } else { "pwd" };
    let mut dir = Terminal::start(
        Running::Task {
            name: "dir".to_owned(),
            command: pwd.to_owned(),
        },
        &root,
    )
    .unwrap();
    assert_eq!(dir.wait().unwrap(), Exit::Code(0));
    let last = root.file_name().unwrap().to_string_lossy().into_owned();
    assert!(dir.text().contains(&last), "{:?}", dir.text());
}

// No spec statement: P4-36: a shell tab runs the lines written to it, echoed, and ends when its
// input does
#[test]
fn a_shell_tab_runs_the_lines_sent_to_it() {
    let root = project("shell", "");
    let mut tab = Terminal::start(Running::Shell, &root).unwrap();
    assert_eq!(tab.title(), root.file_name().unwrap().to_string_lossy());
    tab.send("echo from-the-shell").unwrap();
    tab.send("exit 5").unwrap();
    assert_eq!(tab.wait().unwrap(), Exit::Code(5));
    assert!(tab.text().contains("$ echo from-the-shell\n"));
    assert!(tab.text().contains("from-the-shell"));
    assert!(tab.send("echo late").is_err(), "the input is closed");
    let mut task = Terminal::start(
        Running::Task {
            name: "t".to_owned(),
            command: "echo x".to_owned(),
        },
        &root,
    )
    .unwrap();
    assert!(task.send("echo y").is_err(), "a task reads no input");
    task.wait().unwrap();
}

// No spec statement: P4-36: polling never blocks and notes the exit once the output is read;
// the kept output drops its oldest lines past the limit
#[test]
fn polling_takes_the_output_and_the_exit_and_the_output_is_bounded() {
    let root = project("poll", "");
    let mut tab = Terminal::start(
        Running::Task {
            name: "t".to_owned(),
            command: print_and_exit("done", 0),
        },
        &root,
    )
    .unwrap();
    let before = tab.text().to_owned();
    while tab.exit().is_none() {
        // Each poll returns at once; the loop ends when the process has, with no clock.
        tab.poll();
        std::thread::yield_now();
    }
    assert_eq!(tab.exit(), Some(Exit::Code(0)));
    assert!(tab.text().len() > before.len());
    assert!(!tab.poll(), "nothing more after the exit");
    let line = "x".repeat(99) + "\n";
    for _ in 0..(KEPT_OUTPUT / 100).saturating_add(20) {
        tab.push(line.as_bytes());
    }
    assert!(tab.text().len() <= KEPT_OUTPUT);
    assert!(tab.text().starts_with('x') && tab.text().ends_with('\n'));
    assert_eq!(tab.text().len() % 100, 0, "only whole lines are dropped");
}

// spec PRJ-2.10: task.list lists the project file's tasks; task.run runs one by name in a new
// tab, and an unknown name is refused
#[test]
fn the_task_commands_list_and_run_the_project_tasks() {
    let tasks = format!(
        "[[task]]\nname = \"pack\"\ncommand = \"{}\"\n",
        print_and_exit("packed", 0)
    );
    let root = project("commands", &tasks);
    let mut session = session(&root);
    let listed = session.execute(TASK_LIST, json!({})).unwrap();
    assert_eq!(listed[0]["name"], "pack");
    let opened = session
        .execute(TASK_RUN, json!({ "name": "pack" }))
        .unwrap();
    assert_eq!(opened, json!({ "tab": 0, "title": "pack" }));
    assert!(
        session
            .execute(TASK_RUN, json!({ "name": "nope" }))
            .is_err()
    );
    let shell = session.execute(TERMINAL_NEW, json!({})).unwrap();
    assert_eq!(shell["tab"], 1);
    session
        .execute(TERMINAL_SEND, json!({ "tab": 1, "line": "exit 0" }))
        .unwrap();
    assert!(
        session
            .execute(TERMINAL_SEND, json!({ "tab": 7, "line": "x" }))
            .is_err()
    );
    let tab = |session: &Session<Workbench<FsLoader>>| {
        let tabs = session.context().terminals().tabs();
        (tabs[0].exit(), tabs[0].text().to_owned())
    };
    while tab(&session).0.is_none() {
        session.execute(TERMINAL_POLL, json!({})).unwrap();
        std::thread::yield_now();
    }
    let (exit, text) = tab(&session);
    assert_eq!(exit, Some(Exit::Code(0)));
    assert!(text.contains("packed"), "{text:?}");
    session
        .execute(TERMINAL_CLOSE, json!({ "tab": 0 }))
        .unwrap();
    assert_eq!(session.context().terminals().tabs().len(), 1);
    assert!(
        session
            .execute(TERMINAL_CLOSE, json!({ "tab": 3 }))
            .is_err()
    );
}

// No spec statement: P4-36: with no project open there is nothing to run in
#[test]
fn with_no_project_the_terminal_commands_are_refused() {
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut session = Session::new(Workbench::new(FsLoader, Config::default()), registry);
    assert!(session.execute(TASK_LIST, json!({})).is_err());
    assert!(session.execute(TERMINAL_NEW, json!({})).is_err());
}
