// SPDX-License-Identifier: GPL-3.0-or-later

//! Semantic highlighting.

use super::{ProjectSpec, host, nth, one, path};
use crate::{Highlight, HighlightKind, Modifier, Modifiers};

/// The highlight at the `n`-th `needle` of `text` among `all`.
fn find<'a>(all: &'a [Highlight], text: &str, needle: &str, n: usize) -> Option<&'a Highlight> {
    let start = nth(text, needle, n);
    all.iter().find(|h| h.span.start == start)
}

// spec QC-12.1: each name is highlighted as what it names, its declarations marked
#[test]
fn highlights_classify_each_name() {
    let text = "float g;\n.float hp;\nvoid(float p) f = { local float l; l = p + g; self.hp = l; f(); };\n";
    let mut host = one(text);
    let all = host.highlights(&path("a.qc"));
    let kind = |needle: &str, n: usize| find(&all, text, needle, n).map(|h| h.kind);
    assert_eq!(kind("g;", 0), Some(HighlightKind::Global));
    assert_eq!(kind("hp", 0), Some(HighlightKind::Field));
    assert_eq!(kind("p)", 0), Some(HighlightKind::Parameter));
    assert_eq!(kind("f =", 0), Some(HighlightKind::Function));
    assert_eq!(kind("l;", 0), Some(HighlightKind::Local));
    assert_eq!(kind("hp", 1), Some(HighlightKind::Field));
    let declared = |needle: &str, n: usize| {
        find(&all, text, needle, n).map(|h| h.modifiers.contains(Modifier::Declaration))
    };
    assert_eq!(declared("l;", 0), Some(true));
    assert_eq!(declared("l =", 0), Some(false));
    assert_eq!(declared("f()", 0), Some(false));
    // In source order, each once.
    assert!(all.windows(2).all(|w| w[0].span.start < w[1].span.start));
    // `self` is not declared here, so it is not highlighted.
    assert_eq!(kind("self", 0), None);
}

// Under the vanilla profile an initialised global is highlighted as a constant (QC-11.6).
#[test]
fn highlights_mark_vanilla_constants() {
    let text = "float FL_FLY = 1;\nfloat count;\n";
    let mut host = one(text);
    let all = host.highlights(&path("a.qc"));
    let readonly = |needle: &str| {
        find(&all, text, needle, 0).map(|h| h.modifiers.contains(Modifier::Readonly))
    };
    assert_eq!(readonly("FL_FLY"), Some(true));
    assert_eq!(readonly("count"), Some(false));
    let mut fte = host_fte(text);
    let all = fte.highlights(&path("a.qc"));
    assert_eq!(
        find(&all, text, "FL_FLY", 0).map(|h| h.modifiers.contains(Modifier::Readonly)),
        Some(false)
    );
}

/// The FTEQCC one-file project of `text`.
fn host_fte(text: &str) -> crate::AnalysisHost<qc_project::MemoryLoader> {
    host(&ProjectSpec {
        files: &[("a.qc", text)],
        dialect: Some(qc_project::Dialect::Fteqcc),
        ..ProjectSpec::default()
    })
}

// A builtin (QC-13.3) is highlighted as the engine's, a function with a body is not.
#[test]
fn highlights_mark_builtins() {
    let text = "void(entity e) remove = #15;\nvoid() f = { remove(world); };\nentity world;\n";
    let mut host = one(text);
    let all = host.highlights(&path("a.qc"));
    let library = |needle: &str, n: usize| {
        find(&all, text, needle, n).map(|h| h.modifiers.contains(Modifier::DefaultLibrary))
    };
    assert_eq!(library("remove", 0), Some(true));
    assert_eq!(library("remove", 1), Some(true));
    assert_eq!(library("f =", 0), Some(false));
}

// spec QC-9.7: a macro's use is not highlighted as the name it expands to
#[test]
fn highlights_skip_macro_uses() {
    let text = "#define H health\nfloat health;\nvoid() f = { H = 1; };\n";
    let mut host = host_fte(text);
    let all = host.highlights(&path("a.qc"));
    assert!(find(&all, text, "H =", 0).is_none());
    assert!(find(&all, text, "health;", 0).is_some());
    assert!(host.highlights(&path("none.qc")).is_empty());
}

// spec QC-12.1: a set of modifiers holds what is added, in a fixed order
#[test]
fn modifiers_are_a_set() {
    let set = Modifiers::default()
        .with(Modifier::DefaultLibrary)
        .with(Modifier::Declaration);
    assert!(set.contains(Modifier::Declaration));
    assert!(!set.contains(Modifier::Readonly));
    assert_eq!(
        set.iter().collect::<Vec<_>>(),
        [Modifier::Declaration, Modifier::DefaultLibrary]
    );
    assert_eq!(Modifiers::default().iter().count(), 0);
}
