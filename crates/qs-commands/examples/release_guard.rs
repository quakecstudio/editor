// SPDX-License-Identifier: GPL-3.0-or-later

//! Proves the command guard works in a release build (P2-12, PRIN-7).
//!
//! Cargo builds test targets with `panic = "unwind"` whatever the profile says ("Tests,
//! benchmarks, build scripts, and proc macros ignore the `panic` setting", Cargo Book,
//! Profiles, `panic`), so only a binary shows what the release profile does. `cargo xtask ci`
//! runs this one with `cargo run --profile release --example release_guard`: a command panics,
//! and the program exits with success only if the guard caught it and the session then ran
//! another command. Under `panic = "abort"` the crate does not compile, so the step fails
//! either way.

use std::convert::Infallible;
use std::process::ExitCode;

use qs_commands::{Command, CommandError, NoParams, Outcome, Registry, Session};
use serde_json::json;

/// Panics, as a command with a bug would.
struct Boom;

impl Command<u32> for Boom {
    const ID: &'static str = "test.boom";
    const SUMMARY: &'static str = "Panics.";
    type Params = NoParams;
    type Output = ();
    type Error = Infallible;

    #[expect(
        clippy::panic,
        clippy::panic_in_result_fn,
        reason = "the example proves the guard catches this panic"
    )]
    fn run(&self, _: &mut u32, _: NoParams) -> Result<Outcome<(), u32>, Infallible> {
        panic!("a command with a bug")
    }
}

/// Counts its runs.
struct Bump;

impl Command<u32> for Bump {
    const ID: &'static str = "test.bump";
    const SUMMARY: &'static str = "Adds one.";
    type Params = NoParams;
    type Output = u32;
    type Error = Infallible;

    fn run(&self, count: &mut u32, _: NoParams) -> Result<Outcome<u32, u32>, Infallible> {
        *count = count.saturating_add(1);
        Ok(Outcome::unchanged(*count))
    }
}

fn main() -> ExitCode {
    let mut registry = Registry::new();
    if registry.register(Boom).is_err() || registry.register(Bump).is_err() {
        return ExitCode::FAILURE;
    }
    let mut session = Session::new(0, registry);
    let caught = matches!(
        session.execute("test.boom", json!({})),
        Err(CommandError::Panicked { .. })
    );
    let continued = session
        .execute("test.bump", json!({}))
        .is_ok_and(|out| out == json!(1));
    if caught && continued && cfg!(panic = "unwind") {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
