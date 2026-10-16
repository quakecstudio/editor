// SPDX-License-Identifier: GPL-3.0-or-later

//! The `tracing` layer that formats events, with redaction, into the log file.

use std::fmt;
use std::sync::{Mutex, PoisonError};
use std::time::SystemTime;

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

use crate::redact::{is_source_field, marker};
use crate::time::Utc;
use crate::writer::RotatingFile;

/// Writes one line per event: time, level, target, the span scope, the message and the fields.
///
/// A field named in [`SOURCE_FIELD_NAMES`](crate::SOURCE_FIELD_NAMES) is replaced by a redaction
/// marker before it is formatted into the line, and line breaks inside values are escaped so one
/// event is always one line.
pub(crate) struct FileLayer {
    file: Mutex<RotatingFile>,
    clock: fn() -> SystemTime,
}

impl FileLayer {
    pub(crate) fn new(file: RotatingFile, clock: fn() -> SystemTime) -> Self {
        Self {
            file: Mutex::new(file),
            clock,
        }
    }
}

impl<S> Layer<S> for FileLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else {
            return;
        };
        let mut fields = Fields::default();
        attrs.record(&mut fields);
        span.extensions_mut().insert(fields);
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else {
            return;
        };
        let mut extensions = span.extensions_mut();
        if let Some(fields) = extensions.get_mut::<Fields>() {
            values.record(fields);
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let metadata = event.metadata();
        let mut line = format!(
            "{} {:>5} {}:",
            Utc::from_system_time((self.clock)()).rfc3339(),
            metadata.level(),
            metadata.target()
        );
        if let Some(scope) = ctx.event_scope(event) {
            for span in scope.from_root() {
                line.push(' ');
                line.push_str(span.name());
                if let Some(fields) = span.extensions().get::<Fields>() {
                    push_span_fields(&mut line, fields);
                }
                line.push(':');
            }
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        if let Some(message) = &fields.message {
            line.push(' ');
            line.push_str(message);
        }
        line.push_str(&fields.pairs);
        line.push('\n');
        // Formatting happens before the lock, so a panic in a value's `Debug` cannot leave the
        // file locked for the panic hook's own event.
        let mut file = self.file.lock().unwrap_or_else(PoisonError::into_inner);
        // A failed write cannot be logged anywhere else; the next record tries again.
        drop(file.write_record(&line));
    }
}

fn push_span_fields(line: &mut String, fields: &Fields) {
    line.push('{');
    if let Some(message) = &fields.message {
        line.push_str(message);
    }
    line.push_str(fields.pairs.trim_start());
    line.push('}');
}

/// The formatted, already redacted fields of one event or span.
#[derive(Debug, Default)]
struct Fields {
    message: Option<String>,
    pairs: String,
}

impl Fields {
    fn push(&mut self, field: &Field, value: String) {
        let value = escape(&value);
        if field.name() == "message" {
            self.message = Some(value);
        } else {
            self.pairs.push(' ');
            self.pairs.push_str(field.name());
            self.pairs.push('=');
            self.pairs.push_str(&value);
        }
    }
}

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        let value = if is_source_field(field.name()) {
            marker(value.len())
        } else {
            format!("{value:?}")
        };
        self.push(field, value);
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        let formatted = format!("{value:?}");
        let value = if is_source_field(field.name()) {
            marker(formatted.len())
        } else {
            formatted
        };
        self.push(field, value);
    }
}

/// Escapes line breaks so a value cannot start a forged log line.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_breaks_lines() {
        assert_eq!(escape("a\nb\r\nc"), "a\\nb\\r\\nc");
        assert_eq!(escape("plain"), "plain");
    }
}
