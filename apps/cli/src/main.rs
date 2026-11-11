// SPDX-License-Identifier: GPL-3.0-or-later

//! `quakec-ide`: QuakeC Studio's command line.

use std::ffi::OsString;
use std::io;
use std::process::ExitCode;

use mimalloc::MiMalloc;
use qs_workbench::FsLoader;

/// mimalloc as the global allocator (P4-18): musl's own is slow under heavy allocation, and
/// these tools ship as static musl binaries on Linux.
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn main() -> ExitCode {
    // An argument that is not UTF-8, a path of the host's, is read as it is given.
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let code = quakec_ide::run(&args, FsLoader, &mut io::stdout(), &mut io::stderr());
    ExitCode::from(code.number())
}
