// SPDX-License-Identifier: GPL-3.0-or-later

//! `quakec-lsp`: the QuakeC language server, over standard input and output.

use std::io;
use std::process::ExitCode;

use qc_project::FsLoader;
use quakec_lsp::{Exit, log_config, run};

#[expect(
    clippy::print_stderr,
    reason = "a binary's crate root reports a broken connection, or logging it could not start, \
              to the person who ran it"
)]
fn main() -> ExitCode {
    // QA-7: a log of its own and a crash record on panic; the server runs without them.
    let logging = qs_paths::current()
        .map_err(|err| err.to_string())
        .and_then(|dirs| qs_log::init(&log_config(&dirs)).map_err(|err| err.to_string()));
    if let Err(err) = logging {
        eprintln!("quakec-lsp: logging is off: {err}");
    }
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
