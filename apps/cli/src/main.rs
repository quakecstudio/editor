// SPDX-License-Identifier: GPL-3.0-or-later

//! `quakec-ide`: QuakeC Studio's command line.

use std::io;
use std::process::ExitCode;

use qs_workbench::FsLoader;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = quakec_ide::run(&args, FsLoader, &mut io::stdout(), &mut io::stderr());
    ExitCode::from(code.number())
}
