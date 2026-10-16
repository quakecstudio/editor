# qs-log

Crash-safe logging for QuakeC Studio (QA-7): a `tracing` subscriber that writes to the per-OS
log directory, rotates by size, caps how much is kept, writes a crash record on panic and keeps
source text out of every log.

- **Layer:** L0. It depends on `qs-paths` for the log directory, and on `tracing` and
  `tracing-subscriber` (`std` and `registry` features only).
- **Spec sections:** none; it implements requirement QA-7, not engine behaviour.
- **Shipped:** yes.

## Use

A binary calls `qs_log::init(&qs_log::LogConfig::for_app(&qs_paths::current()?))` once at
startup; every crate then logs with the ordinary `tracing` macros.

| What | Behaviour |
| --- | --- |
| Location | `<logs>/quakec-studio.log`, where `<logs>` is `qs_paths::AppDirs::logs()` (ADR-0011) |
| Crash safety | Each event is one unbuffered append, so a crash loses at most the event being written |
| Rotation | Before an event would take the file past 5 MiB, it becomes `quakec-studio.1.log`, older files move up one, and the fifth is deleted: at most 25 MiB in all. An event larger than a file is cut and marked `[truncated]` |
| Crash record | A panic writes `crash-<UTC time>-<pid>.log` (time, version, thread, location, backtrace), syncs it to disk, logs an `ERROR` line naming it, then runs the previous panic hook |
| Filtering | `RUST_LOG` directives: a default level and `target=level` pairs, comma-separated (`info,qs_vm=trace`); `info` when unset. Span and field filters are not supported |
| Line format | `2026-10-03T04:00:00.000Z  INFO target: span{fields}: message key=value`, one line per event, line breaks in values escaped |

## Keeping source text out

QA-7 says source text is never logged. Three rules enforce it:

1. **`SourceText`.** Wrap QuakeC, map text or any other user content in `qs_log::SourceText`
   before it can reach a log, an error or a panic. Its `Display` and `Debug` print only
   `<redacted N bytes>`, so it is safe in a message, a field and a span.
2. **Field names.** A field named `source`, `src`, `source_text`, `text`, `snippet`,
   `content` or `contents` (`SOURCE_FIELD_NAMES`) is redacted whatever its type, in events and
   spans.
3. **Panics.** The crash record and the log line for a panic never contain the panic message,
   only its length, because a panic message can hold anything.

Text interpolated into a message without `SourceText` (`info!("{}", text)`) is not detected;
the rules make the safe form the easy one. The previous panic hook, Rust's default unless
something else installed one, still prints the panic message to standard error, which is a
terminal, not a log file.

The test `source_text_is_redacted_through_every_api` in `src/tests.rs` logs a snippet through
each of these paths and asserts that no log file contains it.
