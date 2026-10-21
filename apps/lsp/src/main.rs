// SPDX-License-Identifier: GPL-3.0-or-later

//! `quakec-lsp`: the QuakeC language server, over standard input and output.

use std::io;
use std::process::ExitCode;

use qc_project::FsLoader;
use quakec_lsp::{Exit, run};

#[expect(
    clippy::print_stderr,
    reason = "a binary's crate root reports a broken connection to the person who ran it"
)]
fn main() -> ExitCode {
    let stdin = io::stdin();
    let stdout = io::stdout();
    match run(FsLoader, stdin.lock(), stdout.lock()) {
        Ok(Exit::Success) => ExitCode::SUCCESS,
        Ok(Exit::Failure) => ExitCode::FAILURE,
        Err(err) => {
            eprintln!("quakec-lsp: {err}");
            ExitCode::FAILURE
        }
    }
}
