// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::string_slice,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::path::Path;

use eframe::egui::Color32;
use qs_editor_view::Styler;
use qs_workbench::{Config, FsLoader, Workbench};

use super::CodeStyler;
use crate::theme::{Mode, Theme, Token};

/// The styler of `text`, the one file of a project in a fresh directory named `name`.
fn styler(name: &str, text: &str) -> (CodeStyler, Theme) {
    let dir =
        std::env::temp_dir().join(format!("quakec-studio-style-{name}-{}", std::process::id()));
    let _ignored = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("progs.src"), "progs.dat\na.qc\n").unwrap();
    std::fs::write(dir.join("a.qc"), text).unwrap();
    let mut workbench = Workbench::new(FsLoader, Config::default());
    workbench.open_project(&dir).unwrap();
    workbench.open_file(&dir.join("a.qc"), 0).unwrap();
    let theme = Theme::load(Mode::Dark).unwrap();
    let styler = CodeStyler::new(workbench.open().unwrap(), &theme);
    let open = workbench.open().unwrap();
    assert!(styler.is_current(open));
    assert_eq!(styler.revision(), open.revision());
    workbench
        .update(&dir.join("a.qc"), format!("{text}\n").into_bytes())
        .unwrap();
    assert!(
        !styler.is_current(workbench.open().unwrap()),
        "an update makes it stale"
    );
    let _ignored = std::fs::remove_dir_all(Path::new(&dir));
    (styler, theme)
}

/// Line `line`'s spans as `(text, colour, italic)`.
fn spans<'t>(styler: &CodeStyler, text: &'t str, line: usize) -> Vec<(&'t str, Color32, bool)> {
    let line_text = text.split('\n').nth(line).unwrap();
    styler
        .spans(line, line_text)
        .into_iter()
        .map(|s| (&line_text[s.range.clone()], s.color, s.italic))
        .collect()
}

#[test]
fn keywords_literals_comments_and_names_take_their_syntax_colours() {
    let text = "float BOUNCE = 0.7;\n.float hops;\nentity self;\nfloat(float x) rint = #36;\n\
                void() f = { local float n; n = rint(BOUNCE); self.hops = n; };\n";
    let (styler, theme) = styler("names", text);
    let c = |token| theme.color(token);
    assert_eq!(
        spans(&styler, text, 0),
        [
            ("float", c(Token::CodeType), false),
            ("BOUNCE", c(Token::CodeConstant), false),
            ("0.7", c(Token::CodeConstant), false),
        ]
    );
    assert_eq!(
        spans(&styler, text, 1),
        [
            ("float", c(Token::CodeType), false),
            ("hops", c(Token::CodeType), false)
        ]
    );
    let body = spans(&styler, text, 4);
    assert!(
        body.contains(&("local", c(Token::CodeKeyword), false)),
        "{body:?}"
    );
    assert!(
        body.contains(&("rint", c(Token::CodeFunction), true)),
        "a builtin is italic"
    );
    assert!(body.contains(&("f", c(Token::CodeFunction), false)));
    assert!(
        body.contains(&("hops", c(Token::CodeType), false)),
        "a field"
    );
    assert!(
        !body.iter().any(|(t, ..)| *t == "n" || *t == "self"),
        "plain names: {body:?}"
    );
    assert!(styler.spans(99, "").is_empty(), "past the end");
    assert_ne!(styler.revision(), u64::MAX);
}

#[test]
fn a_block_comment_is_coloured_on_every_line_it_covers() {
    let text = "/* one\ntwo\nthree */ float x;\n\"s\";\n";
    let (styler, theme) = styler("comment", text);
    let comment = theme.color(Token::CodeComment);
    assert_eq!(spans(&styler, text, 0), [("/* one", comment, true)]);
    assert_eq!(spans(&styler, text, 1), [("two", comment, true)]);
    let third = spans(&styler, text, 2);
    assert_eq!(third[0], ("three */", comment, true));
    assert_eq!(third[1].0, "float");
}
