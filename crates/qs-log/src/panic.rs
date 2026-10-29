// SPDX-License-Identifier: GPL-3.0-or-later

//! The panic hook: a crash record on disk, written before the process unwinds or aborts.

use std::backtrace::Backtrace;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::panic::{self, PanicHookInfo};
use std::path::{Path, PathBuf};
use std::process;
use std::thread;
use std::time::SystemTime;

use crate::redact::marker;
use crate::time::Utc;

/// Installs a panic hook that writes a crash record to `dir` and logs the panic, then runs the
/// hook that was installed before it (by default, Rust's message on standard error).
///
/// The record is `crash-<time>-<pid>.log`, written and flushed to disk before the previous hook
/// runs. It holds the time, the version, the thread, the panic's location in our Rust source and
/// a backtrace. The panic message is never written: it can hold any text, source included, so the
/// record gives only its length.
pub fn install_panic_hook(dir: PathBuf, clock: fn() -> SystemTime) {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map_or_else(|| "unknown".to_owned(), ToString::to_string);
        match write_crash_record(&dir, clock(), info, &location) {
            Ok(path) => tracing::error!(
                location = %location,
                record = %path.display(),
                "panic; crash record written"
            ),
            Err(err) => tracing::error!(
                location = %location,
                error = %err,
                "panic; the crash record could not be written"
            ),
        }
        previous(info);
    }));
}

fn write_crash_record(
    dir: &Path,
    now: SystemTime,
    info: &PanicHookInfo<'_>,
    location: &str,
) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let path = dir.join(format!(
        "crash-{}-{}.log",
        Utc::from_system_time(now).compact(),
        process::id()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    file.write_all(crash_record(now, info, location).as_bytes())?;
    file.sync_all()?;
    Ok(path)
}

/// The record's text: everything about the panic but its message, of which only the length.
fn crash_record(now: SystemTime, info: &PanicHookInfo<'_>, location: &str) -> String {
    let thread = thread::current();
    format!(
        "QuakeC Studio crash record\n\
         time: {}\n\
         version: {}\n\
         thread: {}\n\
         location: {location}\n\
         message: {}\n\
         backtrace:\n{}\n",
        Utc::from_system_time(now).rfc3339(),
        env!("CARGO_PKG_VERSION"),
        thread.name().unwrap_or("unnamed"),
        marker(info.payload_as_str().map_or(0, str::len)),
        Backtrace::force_capture()
    )
}
