// SPDX-License-Identifier: GPL-3.0-or-later

//! The application directories resolved from this process's real environment.

#![expect(
    clippy::expect_used,
    reason = "test code: a failed expect is a failed test with a message"
)]

use qs_paths::{APP_DIR_NAME, APP_DIR_NAME_UNIX, Platform};

#[test]
fn current_environment_resolves_to_absolute_app_directories() {
    let dirs = qs_paths::current().expect("the test environment sets HOME or APPDATA");
    let name = match Platform::current() {
        Platform::MacOs | Platform::Windows => APP_DIR_NAME,
        Platform::Linux => APP_DIR_NAME_UNIX,
    };
    for dir in [dirs.config(), dirs.data(), dirs.cache(), dirs.logs()] {
        assert!(dir.is_absolute(), "{}", dir.display());
        assert!(
            dir.components().any(|part| part.as_os_str() == name),
            "{} does not contain {name}",
            dir.display()
        );
    }
}
