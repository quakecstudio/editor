// SPDX-License-Identifier: GPL-3.0-or-later

//! Hover text, as Markdown or plain text (LSP 3.18, `MarkupContent`).

use qc_ide::{BuiltinInfo, BuiltinRef, Hover};

/// The text of a hover: the declaration, its comment, and for a builtin which engines have it.
#[must_use]
pub(crate) fn hover(hover: &Hover, markdown: bool) -> String {
    let mut parts = Vec::new();
    parts.push(if markdown {
        format!("```quakec\n{}\n```", hover.declaration)
    } else {
        hover.declaration.clone()
    });
    if let Some(docs) = &hover.docs {
        parts.push(docs.clone());
    }
    if let Some(builtin) = &hover.builtin {
        parts.push(engines(builtin, markdown));
    }
    parts.join(if markdown { "\n\n---\n\n" } else { "\n\n" })
}

/// Which engines have a builtin, the target first.
fn engines(info: &BuiltinInfo, markdown: bool) -> String {
    let code = |text: &str| {
        if markdown {
            format!("`{text}`")
        } else {
            text.to_owned()
        }
    };
    let key = match &info.key {
        BuiltinRef::Number(n) => format!("#{n}"),
        BuiltinRef::Name(name) => format!("#0:{name}"),
    };
    let mut lines = vec![format!("Builtin {}.", code(&key))];
    let target = info.target.name();
    match info.on_target() {
        Some(entry) => {
            let shown = if entry.signature.is_empty() {
                entry.name.to_owned()
            } else {
                format!("{} {}", entry.signature, entry.name)
            };
            let mut line = format!("{target} has it as {}", code(&shown));
            if !entry.extensions.is_empty() {
                line.push_str(&format!(" (extensions {})", entry.extensions.join(", ")));
            }
            line.push('.');
            lines.push(line);
        }
        None => lines.push(format!("{target}, the target engine, does not have it.")),
    }
    let with: Vec<&str> = info
        .engines
        .iter()
        .filter(|(_, b)| b.is_some())
        .map(|(e, _)| e.name())
        .collect();
    let without: Vec<&str> = info
        .engines
        .iter()
        .filter(|(_, b)| b.is_none())
        .map(|(e, _)| e.name())
        .collect();
    if !with.is_empty() {
        lines.push(format!("Engines with it: {}.", with.join(", ")));
    }
    if !without.is_empty() {
        lines.push(format!("Engines without it: {}.", without.join(", ")));
    }
    lines.join(if markdown { "  \n" } else { "\n" })
}
