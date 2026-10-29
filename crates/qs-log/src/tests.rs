// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap is a failed test, the crash hook needs a panic, and \
              the fixed clock is a small offset from the epoch"
)]

use std::fs;
use std::panic;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use tracing::subscriber::with_default;
use tracing::{debug, error, info, info_span, trace, warn};

use qs_paths::{Platform, resolve};

use super::*;
use crate::test_dir::TestDir;

/// QuakeC source with two distinctive words; neither may appear in any log file.
const SNIPPET: &str = "void() secret_fn = { bprint(\"hunter2\\n\"); };";
const SECRETS: [&str; 2] = ["secret_fn", "hunter2"];

fn fixed_clock() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_791_000_000)
}

fn config(dir: &Path, directives: &str) -> LogConfig {
    LogConfig {
        directives: directives.to_owned(),
        clock: fixed_clock,
        ..LogConfig::new(dir.to_path_buf())
    }
}

/// Every file in `dir`, concatenated, so rotated files and crash records are checked too.
fn all_logs(dir: &Path) -> String {
    let mut names: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    names.sort();
    names
        .iter()
        .map(|path| fs::read_to_string(path).unwrap())
        .collect()
}

fn assert_redacted(text: &str) {
    for secret in SECRETS {
        assert!(!text.contains(secret), "{secret:?} leaked into:\n{text}");
    }
}

fn current_log(dir: &Path) -> String {
    fs::read_to_string(dir.join("quakec-studio.log")).unwrap()
}

// Acceptance criterion: a source snippet logged through every API is redacted.
#[test]
fn source_text_is_redacted_through_every_api() {
    let dir = TestDir::new("every-api");
    let marker = format!("<redacted {} bytes>", SNIPPET.len());
    let subscriber = subscriber(&config(dir.path(), "trace")).unwrap();
    with_default(subscriber, || {
        info!("message {}", SourceText(SNIPPET));
        info!("message debug {:?}", SourceText(SNIPPET));
        info!(line = %SourceText(SNIPPET), "display field");
        info!(line = ?SourceText(SNIPPET), "debug field");
        info!(line = %SourceText(SNIPPET.to_owned()), "owned");
        trace!("trace {}", SourceText(SNIPPET));
        debug!("debug {}", SourceText(SNIPPET));
        warn!("warn {}", SourceText(SNIPPET));
        error!("error {}", SourceText(SNIPPET));
        for name in SOURCE_FIELD_NAMES {
            let callsite_name = *name;
            match callsite_name {
                "source" => info!(source = SNIPPET, as_str = true),
                "src" => info!(src = %SNIPPET, as_display = true),
                "source_text" => info!(source_text = ?SNIPPET, as_debug = true),
                "text" => info!(text = SNIPPET),
                "snippet" => info!(snippet = SNIPPET),
                "content" => info!(content = SNIPPET),
                "contents" => info!(contents = SNIPPET),
                other => panic!("no test logs the field {other}"),
            }
        }
        let span = info_span!("parse", source = SNIPPET, file = %SourceText(SNIPPET), text = 0);
        let _entered = span.enter();
        span.record("text", SNIPPET);
        info!("inside the span");
    });
    let log = current_log(dir.path());
    assert_redacted(&all_logs(dir.path()));
    assert!(log.contains(&format!("message {marker}")), "{log}");
    assert!(
        log.contains(&format!("display field line={marker}\n")),
        "{log}"
    );
    assert!(log.contains(&format!("source={marker}")), "{log}");
    assert!(log.contains(&format!("contents={marker}")), "{log}");
    assert!(
        log.contains(&format!("parse{{source={marker} file={marker} text=")),
        "{log}"
    );
    // The `text` field is recorded after the span is created (span.record("text", SNIPPET)),
    // so its redacted value must appear in the span context; without on_record it would not.
    assert!(
        log.contains(&format!(
            "parse{{source={marker} file={marker} text=<redacted 1 bytes> text={marker}}}"
        )),
        "{log}"
    );
    assert!(log.contains("inside the span"), "{log}");
    assert_eq!(log.lines().count(), 17, "{log}");
}

// Acceptance criterion, crash path: a panic whose message is source text leaves a crash record
// and a log line, neither of which contains it. Also covers `init`, which installs the hook.
#[test]
fn panic_hook_writes_a_redacted_crash_record() {
    let dir = TestDir::new("panic");
    let config = config(dir.path(), "info");
    init(&config).unwrap();
    assert!(matches!(init(&config), Err(LogError::AlreadyInstalled)));

    let owned = panic::catch_unwind(|| panic!("{SNIPPET}"));
    assert!(owned.is_err());
    let borrowed = panic::catch_unwind(|| panic::panic_any(SNIPPET));
    assert!(borrowed.is_err());
    drop(panic::take_hook());

    let records: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("crash-20261003T040000.000Z-"))
        })
        .collect();
    // Both panics share the fixed clock and the pid, so the second record cannot be created
    // and the hook logs that instead.
    assert_eq!(records.len(), 1, "{records:?}");
    let record = fs::read_to_string(records.first().unwrap()).unwrap();
    assert_redacted(&all_logs(dir.path()));
    assert!(
        record.starts_with("QuakeC Studio crash record\n"),
        "{record}"
    );
    assert!(
        record.contains("time: 2026-10-03T04:00:00.000Z\n"),
        "{record}"
    );
    assert!(
        record.contains(&format!("message: <redacted {} bytes>\n", SNIPPET.len())),
        "{record}"
    );
    assert!(record.contains("tests.rs:"), "{record}");
    assert!(record.contains("backtrace:\n"), "{record}");
    let log = current_log(dir.path());
    assert!(log.contains("panic; crash record written"), "{log}");
    assert!(
        log.contains("panic; the crash record could not be written"),
        "{log}"
    );
}

#[test]
fn directives_filter_by_level_and_target() {
    let dir = TestDir::new("directives");
    let subscriber = subscriber(&config(dir.path(), "warn,noisy=error,chatty=debug")).unwrap();
    with_default(subscriber, || {
        info!("dropped by the default level");
        warn!("kept at the default level");
        warn!(target: "noisy", "dropped by the target's level");
        error!(target: "noisy", "kept for the target");
        debug!(target: "chatty", "kept below the default level");
    });
    let log = current_log(dir.path());
    assert!(!log.contains("dropped"), "{log}");
    assert_eq!(log.lines().count(), 3, "{log}");
    assert!(log.contains(" ERROR noisy: kept for the target"), "{log}");
    assert!(log.contains(" DEBUG chatty: kept below"), "{log}");
}

#[test]
fn lines_have_time_level_target_and_escaped_breaks() {
    let dir = TestDir::new("format");
    with_default(subscriber(&config(dir.path(), "info")).unwrap(), || {
        info!(target: "qs", path = "a\nb", "two\nlines");
    });
    assert_eq!(
        current_log(dir.path()),
        "2026-10-03T04:00:00.000Z  INFO qs: two\\nlines path=\"a\\nb\"\n"
    );
}

#[test]
fn events_rotate_into_numbered_files() {
    let dir = TestDir::new("rotate");
    let config = LogConfig {
        max_file_bytes: 60,
        max_files: 1,
        ..config(dir.path(), "info")
    };
    with_default(subscriber(&config).unwrap(), || {
        info!(target: "qs", "first");
        info!(target: "qs", "second");
        info!(target: "qs", "third");
    });
    assert!(current_log(dir.path()).contains("third"));
    assert!(
        fs::read_to_string(dir.path().join("quakec-studio.1.log"))
            .unwrap()
            .contains("second")
    );
    assert!(!dir.path().join("quakec-studio.2.log").exists());
}

#[test]
fn a_named_file_is_written_and_rotated() {
    let dir = TestDir::new("named");
    let config = LogConfig {
        file_stem: "quakec-lsp".to_owned(),
        max_file_bytes: 60,
        max_files: 1,
        ..config(dir.path(), "info")
    };
    with_default(subscriber(&config).unwrap(), || {
        info!(target: "qs", "first");
        info!(target: "qs", "second");
    });
    let read = |name: &str| fs::read_to_string(dir.path().join(name)).unwrap();
    assert!(read("quakec-lsp.log").contains("second"));
    assert!(read("quakec-lsp.1.log").contains("first"));
    assert!(!dir.path().join("quakec-studio.log").exists());
    assert_eq!(
        LogConfig::new(dir.path().to_path_buf()).file_stem,
        LOG_FILE_STEM
    );
}

#[test]
fn bad_directives_are_an_error() {
    let dir = TestDir::new("bad-directives");
    let Err(err) = subscriber(&config(dir.path(), "qs_vm=loud")) else {
        panic!("loud is not a level");
    };
    assert!(matches!(err, LogError::Directives { .. }));
    assert!(
        err.to_string()
            .starts_with("invalid log directives \"qs_vm=loud\": "),
        "{err}"
    );
    // The parser's error is the source, and the message ends with it.
    let source = std::error::Error::source(&err).unwrap().to_string();
    assert!(err.to_string().ends_with(&source), "{err} / {source}");
}

#[test]
fn an_unusable_directory_is_an_error() {
    let dir = TestDir::new("bad-dir");
    let blocker = dir.path().join("blocker");
    fs::write(&blocker, "").unwrap();
    let Err(err) = subscriber(&config(&blocker, "info")) else {
        panic!("a file is not a directory");
    };
    assert!(matches!(err, LogError::Io { .. }));
    assert!(
        err.to_string().starts_with("cannot open the log in "),
        "{err}"
    );
    assert!(std::error::Error::source(&err).is_some());
    assert_eq!(
        LogError::AlreadyInstalled.to_string(),
        "a global log subscriber is already installed"
    );
}

#[test]
fn rust_log_falls_back_to_info() {
    assert_eq!(directives_from(None), "info");
    assert_eq!(directives_from(Some(OsString::from(""))), "info");
    assert_eq!(directives_from(Some(OsString::from("  "))), "info");
    assert_eq!(
        directives_from(Some(OsString::from("qs_vm=trace"))),
        "qs_vm=trace"
    );
}

#[test]
fn app_config_uses_the_per_os_log_directory() {
    let dirs = resolve(Platform::Linux, |name| {
        (name == "HOME").then(|| OsString::from("/home/q"))
    })
    .unwrap();
    let config = LogConfig::for_app(&dirs);
    assert_eq!(config.dir, dirs.logs());
    assert_eq!(config.max_file_bytes, 5_242_880);
    assert_eq!(config.max_files, 4);
    // for_app takes its directives from RUST_LOG: a set value is used, an unset one falls back
    // to the default. with_rust_log is for_app with the value given, so the test does not touch
    // process-global environment state.
    let custom = LogConfig::with_rust_log(dirs.logs().to_path_buf(), Some(OsString::from("debug")));
    assert_eq!(custom.directives, "debug");
    let unset = LogConfig::with_rust_log(dirs.logs().to_path_buf(), None);
    assert_eq!(unset.directives, DEFAULT_DIRECTIVES);
}
