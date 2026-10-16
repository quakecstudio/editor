// SPDX-License-Identifier: GPL-3.0-or-later

//! Running other programs: compilers, cargo, curl and the oracles, the boundary of xtask. The
//! commands that run them take an [`Exec`], so that their tests can stand a [`Fake`] in for
//! the program and check both the command line xtask builds and what it makes of each answer.

use std::io;
use std::process::{Command, ExitStatus, Output};

/// Runs a command.
pub(crate) trait Exec {
    /// Runs `command` to the end and returns its status and what it wrote.
    fn output(&self, command: &mut Command) -> io::Result<Output>;

    /// Runs `command` to the end, its output where the command sends it.
    fn status(&self, command: &mut Command) -> io::Result<ExitStatus>;
}

/// The system: each command really runs.
pub(crate) struct System;

impl Exec for System {
    fn output(&self, command: &mut Command) -> io::Result<Output> {
        command.output()
    }

    fn status(&self, command: &mut Command) -> io::Result<ExitStatus> {
        command.status()
    }
}

#[cfg(test)]
pub(crate) use fake::{Fake, Reply};

#[cfg(test)]
mod fake {
    use std::cell::RefCell;
    use std::fs;
    use std::io;
    use std::path::PathBuf;
    use std::process::{Command, ExitStatus, Output};

    use super::Exec;

    /// What a [`Fake`] program does: exits with `code` (or, on Unix, dies of signal `signal`),
    /// writes `stdout` and `stderr`, and writes each of `files` first.
    #[derive(Debug, Default, Clone)]
    pub(crate) struct Reply {
        pub(crate) code: i32,
        pub(crate) signal: Option<i32>,
        pub(crate) stdout: String,
        pub(crate) stderr: String,
        pub(crate) files: Vec<(PathBuf, Vec<u8>)>,
        /// A failure to start the program, in place of running it.
        pub(crate) cannot_run: bool,
    }

    impl Reply {
        /// A program that exits with `code` and writes nothing.
        pub(crate) fn exit(code: i32) -> Self {
            Self {
                code,
                ..Self::default()
            }
        }
    }

    /// A stand-in for every program a test runs: `answer` sees each command and says what it
    /// does. The command lines it saw are kept, in order.
    pub(crate) struct Fake<F: Fn(&Command) -> Reply> {
        answer: F,
        pub(crate) seen: RefCell<Vec<String>>,
    }

    impl<F: Fn(&Command) -> Reply> Fake<F> {
        pub(crate) fn new(answer: F) -> Self {
            Self {
                answer,
                seen: RefCell::new(Vec::new()),
            }
        }

        fn run(&self, command: &Command) -> io::Result<(ExitStatus, Reply)> {
            let mut line = command.get_program().to_string_lossy().into_owned();
            for arg in command.get_args() {
                line.push(' ');
                line.push_str(&arg.to_string_lossy());
            }
            self.seen.borrow_mut().push(line);
            let reply = (self.answer)(command);
            if reply.cannot_run {
                return Err(io::Error::new(io::ErrorKind::NotFound, "no such program"));
            }
            for (path, bytes) in &reply.files {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(path, bytes)?;
            }
            Ok((exit_status(reply.code, reply.signal), reply))
        }
    }

    impl<F: Fn(&Command) -> Reply> Exec for Fake<F> {
        fn output(&self, command: &mut Command) -> io::Result<Output> {
            let (status, reply) = self.run(command)?;
            Ok(Output {
                status,
                stdout: reply.stdout.into_bytes(),
                stderr: reply.stderr.into_bytes(),
            })
        }

        fn status(&self, command: &mut Command) -> io::Result<ExitStatus> {
            self.run(command).map(|(status, _)| status)
        }
    }

    #[cfg(unix)]
    fn exit_status(code: i32, signal: Option<i32>) -> ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        match signal {
            Some(signal) => ExitStatus::from_raw(signal),
            None => ExitStatus::from_raw(code.wrapping_shl(8)),
        }
    }

    #[cfg(windows)]
    fn exit_status(code: i32, _signal: Option<i32>) -> ExitStatus {
        use std::os::windows::process::ExitStatusExt;
        ExitStatus::from_raw(code.cast_unsigned())
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use std::path::Path;

    use super::*;

    #[test]
    fn the_system_runs_a_command() {
        let output = System
            .output(Command::new("cargo").arg("--version"))
            .unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).starts_with("cargo "));
        assert!(
            System
                .status(Command::new("cargo").arg("--version"))
                .unwrap()
                .success()
        );
        assert!(
            !System
                .status(Command::new("cargo").arg("no-such-command-xtask-exec"))
                .unwrap()
                .success()
        );
        assert!(
            System
                .output(&mut Command::new("no-such-program-xtask"))
                .is_err()
        );
    }

    #[test]
    fn a_fake_answers_writes_its_files_and_remembers_each_command() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/xtask-tests/exec-fake");
        let file = dir.join("out.txt");
        let fake = Fake::new(|command: &Command| {
            if command.get_args().count() == 0 {
                return Reply {
                    cannot_run: true,
                    ..Reply::default()
                };
            }
            Reply {
                code: 3,
                stdout: "out".to_owned(),
                stderr: "err".to_owned(),
                files: vec![(file.clone(), b"written".to_vec())],
                ..Reply::default()
            }
        });
        let output = fake.output(Command::new("tool").arg("a").arg("b")).unwrap();
        assert_eq!(output.status.code(), Some(3));
        assert_eq!(output.stdout, b"out");
        assert_eq!(output.stderr, b"err");
        assert_eq!(std::fs::read(&file).unwrap(), b"written");
        assert_eq!(
            fake.status(Command::new("tool").arg("c")).unwrap().code(),
            Some(3)
        );
        assert!(fake.output(&mut Command::new("tool")).is_err());
        assert_eq!(*fake.seen.borrow(), ["tool a b", "tool c", "tool"]);
        assert_eq!(Reply::exit(4).code, 4);
    }

    #[cfg(unix)]
    #[test]
    fn a_fake_can_die_of_a_signal() {
        let fake = Fake::new(|_: &Command| Reply {
            signal: Some(11),
            ..Reply::default()
        });
        let output = fake.output(&mut Command::new("tool")).unwrap();
        assert_eq!(output.status.code(), None);
    }
}
