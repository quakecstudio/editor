// SPDX-License-Identifier: GPL-3.0-or-later

//! Running other programs: compilers, cargo, curl and the oracles, the boundary of xtask. The
//! commands that run them take an [`Exec`], so that their tests can stand a [`Fake`] in for
//! the program and check both the command line xtask builds and what it makes of each answer.

use std::io::{self, Read};
use std::process::{Command, ExitStatus, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Runs a command.
pub(crate) trait Exec {
    /// Runs `command` to the end and returns its status and what it wrote.
    fn output(&self, command: &mut Command) -> io::Result<Output>;

    /// Runs `command` as [`Exec::output`] does, but kills it once it has run for `limit`, and
    /// then returns `None`.
    fn output_within(&self, command: &mut Command, limit: Duration) -> io::Result<Option<Output>>;

    /// Runs `command` to the end, its output where the command sends it.
    fn status(&self, command: &mut Command) -> io::Result<ExitStatus>;
}

/// The system: each command really runs.
pub(crate) struct System;

/// How often [`System`] looks whether a command given a time limit has ended.
const POLL: Duration = Duration::from_millis(1);

impl Exec for System {
    fn output(&self, command: &mut Command) -> io::Result<Output> {
        command.output()
    }

    fn output_within(&self, command: &mut Command, limit: Duration) -> io::Result<Option<Output>> {
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        // Each pipe is read on its own thread, so a command that writes much cannot block.
        let stdout = drain(child.stdout.take());
        let stderr = drain(child.stderr.take());
        let start = Instant::now();
        let status = wait_or_expire(
            || child.try_wait(),
            || start.elapsed() >= limit,
            || thread::sleep(POLL),
        )?;
        if status.is_none() {
            child.kill()?;
            child.wait()?;
        }
        let stdout = stdout.join().unwrap_or_default();
        let stderr = stderr.join().unwrap_or_default();
        Ok(status.map(|status| Output {
            status,
            stdout,
            stderr,
        }))
    }

    fn status(&self, command: &mut Command) -> io::Result<ExitStatus> {
        command.status()
    }
}

/// Asks `ended` for a command's status until it has one, or until `expired` says its time is
/// up, which gives `None`; `pause` waits between two asks. The clock is the caller's, so the
/// loop is tested with no real time.
fn wait_or_expire<T>(
    mut ended: impl FnMut() -> io::Result<Option<T>>,
    mut expired: impl FnMut() -> bool,
    mut pause: impl FnMut(),
) -> io::Result<Option<T>> {
    loop {
        if let Some(status) = ended()? {
            return Ok(Some(status));
        }
        if expired() {
            return Ok(None);
        }
        pause();
    }
}

/// Reads all of `pipe` on a thread of its own; a pipe that fails to read gives nothing.
fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe
            && pipe.read_to_end(&mut bytes).is_err()
        {
            bytes.clear();
        }
        bytes
    })
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
    use std::time::Duration;

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
        /// A program that never ends, so a time limit kills it.
        pub(crate) hangs: bool,
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

        fn output_within(
            &self,
            command: &mut Command,
            _limit: Duration,
        ) -> io::Result<Option<Output>> {
            let (status, reply) = self.run(command)?;
            Ok((!reply.hangs).then(|| Output {
                status,
                stdout: reply.stdout.into_bytes(),
                stderr: reply.stderr.into_bytes(),
            }))
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

    use super::*;
    use crate::test_dirs::fresh;

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
        let dir = fresh("exec/fake");
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

    #[test]
    fn a_command_within_its_limit_gives_its_output() {
        let output = System
            .output_within(
                Command::new("cargo").arg("--version"),
                Duration::from_secs(600),
            )
            .unwrap()
            .unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).starts_with("cargo "));
        assert!(
            System
                .output_within(
                    &mut Command::new("no-such-program-xtask"),
                    Duration::from_secs(1)
                )
                .is_err()
        );
    }

    // A limit already past kills a command that is still running at the first look, with no
    // wait on the clock.
    #[cfg(unix)]
    #[test]
    fn a_command_past_its_limit_is_killed() {
        let output = System
            .output_within(
                Command::new("sh").args(["-c", "exec sleep 60"]),
                Duration::ZERO,
            )
            .unwrap();
        assert!(output.is_none());
    }

    #[test]
    fn waiting_ends_with_the_status_or_at_the_limit() {
        let mut asks = 0;
        let mut pauses = 0;
        let ended = wait_or_expire(
            || {
                asks += 1;
                Ok((asks == 3).then_some(7))
            },
            || false,
            || pauses += 1,
        );
        assert_eq!(ended.unwrap(), Some(7));
        assert_eq!(pauses, 2);
        let mut looks = 0;
        let expired = wait_or_expire(
            || Ok(None::<i32>),
            || {
                looks += 1;
                looks == 2
            },
            || {},
        );
        assert_eq!(expired.unwrap(), None);
        assert_eq!(looks, 2);
        let failed = wait_or_expire(
            || Err::<Option<i32>, _>(io::Error::other("x")),
            || true,
            || {},
        );
        assert!(failed.is_err());
    }

    #[test]
    fn a_fake_may_hang() {
        let fake = Fake::new(|command: &Command| Reply {
            hangs: command.get_args().count() > 0,
            ..Reply::exit(2)
        });
        let limit = Duration::from_secs(1);
        assert!(
            fake.output_within(Command::new("tool").arg("x"), limit)
                .unwrap()
                .is_none()
        );
        let output = fake
            .output_within(&mut Command::new("tool"), limit)
            .unwrap()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
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
