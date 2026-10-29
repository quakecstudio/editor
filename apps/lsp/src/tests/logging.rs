// SPDX-License-Identifier: GPL-3.0-or-later

//! The server's log (QA-7).

use std::ffi::OsString;

use qs_paths::{Platform, resolve};

use crate::{LOG_FILE_STEM, log_config};

#[test]
fn the_server_logs_to_its_own_file_in_the_log_directory() {
    let dirs = resolve(Platform::Linux, |name| {
        (name == "HOME").then(|| OsString::from("/home/q"))
    })
    .unwrap();
    let config = log_config(&dirs);
    assert_eq!(config.dir, dirs.logs());
    assert_eq!(config.file_stem, "quakec-lsp");
    assert_eq!(LOG_FILE_STEM, "quakec-lsp");
    assert_ne!(config.file_stem, qs_log::LOG_FILE_STEM);
    let ide = qs_log::LogConfig::for_app(&dirs);
    assert_eq!(
        (config.max_file_bytes, config.max_files, config.directives),
        (ide.max_file_bytes, ide.max_files, ide.directives)
    );
}
