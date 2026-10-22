// SPDX-License-Identifier: GPL-3.0-or-later

//! Hover text, as Markdown or plain text (LSP 3.18, `MarkupContent`).

use qc_ide::{BuiltinInfo, BuiltinRef, Hover};

/// A `MarkupKind`: how a hover's text is written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Markup {
    /// `plaintext`, which every client reads.
    #[default]
    PlainText,
    /// `markdown`, for a client whose preferred `contentFormat` it is.
    Markdown,
}

impl Markup {
    /// The `MarkupKind` that names it.
    pub(crate) const fn kind(self) -> &'static str {
        match self {
            Self::PlainText => "plaintext",
            Self::Markdown => "markdown",
        }
    }

    /// QuakeC source on lines of its own.
    fn source(self, text: &str) -> String {
        match self {
            Self::PlainText => text.to_owned(),
            Self::Markdown => format!("```quakec\n{text}\n```"),
        }
    }

    /// QuakeC source within a line.
    fn code(self, text: &str) -> String {
        match self {
            Self::PlainText => text.to_owned(),
            Self::Markdown => format!("`{text}`"),
        }
    }

    /// What separates the parts of a hover.
    const fn rule(self) -> &'static str {
        match self {
            Self::PlainText => "\n\n",
            Self::Markdown => "\n\n---\n\n",
        }
    }

    /// What ends a line within a part: Markdown joins lines without two trailing spaces.
    const fn line_break(self) -> &'static str {
        match self {
            Self::PlainText => "\n",
            Self::Markdown => "  \n",
        }
    }
}

/// The text of a hover: the declaration, its comment, and for a builtin which engines have it.
pub(crate) fn hover(hover: &Hover, markup: Markup) -> String {
    let mut parts = vec![markup.source(&hover.declaration)];
    if let Some(docs) = &hover.docs {
        parts.push(docs.clone());
    }
    if let Some(builtin) = &hover.builtin {
        parts.push(engines(builtin, markup));
    }
    parts.join(markup.rule())
}

/// Which engines have a builtin, the target first.
fn engines(info: &BuiltinInfo, markup: Markup) -> String {
    let key = match &info.key {
        BuiltinRef::Number(n) => format!("#{n}"),
        BuiltinRef::Name(name) => format!("#0:{name}"),
    };
    let mut lines = vec![format!("Builtin {}.", markup.code(&key))];
    let target = info.target.name();
    match info.on_target() {
        Some(entry) => {
            let shown = if entry.signature.is_empty() {
                entry.name.to_owned()
            } else {
                format!("{} {}", entry.signature, entry.name)
            };
            let mut line = format!("{target} has it as {}", markup.code(&shown));
            if !entry.extensions.is_empty() {
                line.push_str(&format!(" (extensions {})", entry.extensions.join(", ")));
            }
            line.push('.');
            lines.push(line);
        }
        None => lines.push(format!("{target}, the target engine, does not have it.")),
    }
    let names = |has: bool| -> Vec<&str> {
        info.engines
            .iter()
            .filter(|(_, b)| b.is_some() == has)
            .map(|(e, _)| e.name())
            .collect()
    };
    let (with, without) = (names(true), names(false));
    if !with.is_empty() {
        lines.push(format!("Engines with it: {}.", with.join(", ")));
    }
    if !without.is_empty() {
        lines.push(format!("Engines without it: {}.", without.join(", ")));
    }
    lines.join(markup.line_break())
}
