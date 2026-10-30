// SPDX-License-Identifier: GPL-3.0-or-later

//! The layout job of a line and the cache of laid-out lines.

use egui::text::LayoutJob;
use egui::{Color32, FontFamily, FontId, Stroke};

use super::{Harness, numbered};
use crate::layout::{LayoutCache, Plain, Preedit, StyledSpan, Styler, line_job};

const PLAIN: Color32 = Color32::GRAY;

fn span(start: usize, end: usize, color: Color32, italic: bool) -> StyledSpan {
    StyledSpan {
        range: start..end,
        color,
        italic,
    }
}

/// Each section of `job`: its text, colour, whether italic, and whether underlined.
fn sections(job: &LayoutJob) -> Vec<(&str, Color32, bool, bool)> {
    job.sections
        .iter()
        .map(|s| {
            let text = job
                .text
                .get(s.byte_range.start.0..s.byte_range.end.0)
                .unwrap_or("?");
            (
                text,
                s.format.color,
                s.format.italics,
                s.format.underline != Stroke::NONE,
            )
        })
        .collect()
}

fn font() -> FontId {
    FontId::new(13.0, FontFamily::Monospace)
}

#[test]
fn an_empty_line_is_one_empty_section_in_the_font_as_tall_as_any_line() {
    let empty = line_job("", &[], &font(), PLAIN, None);
    assert_eq!(sections(&empty), [("", PLAIN, false, false)]);
    assert_eq!(empty.sections[0].format.font_id, font());
    let ctx = Harness::new("").ctx;
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let empty_height = ui.fonts_mut(|f| f.layout_job(empty.clone())).size().y;
        let text = line_job("x", &[], &font(), PLAIN, None);
        let text_height = ui.fonts_mut(|f| f.layout_job(text)).size().y;
        assert!(empty_height > 0.0);
        assert_eq!(empty_height, text_height);
    });
    output.textures_delta.clear();
    let typing = line_job("", &[], &font(), PLAIN, Some(Preedit { col: 0, text: "k" }));
    assert_eq!(sections(&typing), [("k", PLAIN, false, true)]);
}

#[test]
fn a_line_is_its_styled_runs_and_the_gaps_between_them_in_plain() {
    let spans = [
        span(2, 4, Color32::RED, true),
        span(6, 8, Color32::BLUE, false),
    ];
    let job = line_job("abcdefghij", &spans, &font(), PLAIN, None);
    assert_eq!(
        sections(&job),
        [
            ("ab", PLAIN, false, false),
            ("cd", Color32::RED, true, false),
            ("ef", PLAIN, false, false),
            ("gh", Color32::BLUE, false, false),
            ("ij", PLAIN, false, false),
        ]
    );
    assert!(job.sections.iter().all(|s| s.format.font_id == font()));
}

#[test]
fn runs_are_clamped_to_the_line_and_past_each_other() {
    let spans = [
        span(0, 3, Color32::RED, false),
        span(2, 5, Color32::BLUE, false),
        span(9, 20, Color32::GREEN, false),
    ];
    let job = line_job("abcdef", &spans, &font(), PLAIN, None);
    assert_eq!(
        sections(&job),
        [
            ("abc", Color32::RED, false, false),
            ("de", Color32::BLUE, false, false),
            ("f", PLAIN, false, false),
        ]
    );
    let job = line_job(
        "ab",
        &[span(0, 2, Color32::RED, false)],
        &font(),
        PLAIN,
        None,
    );
    assert_eq!(
        sections(&job),
        [("ab", Color32::RED, false, false)],
        "no empty tail"
    );
}

#[test]
fn composition_text_is_spliced_in_underlined() {
    let spans = [span(0, 4, Color32::RED, false)];
    let at = |col: usize| {
        let preedit = Preedit {
            col, text: "にほ"
        };
        let job = line_job("abcdef", &spans, &font(), PLAIN, Some(preedit));
        sections(&job)
            .into_iter()
            .map(|(text, color, _, underline)| (text.to_owned(), color, underline))
            .collect::<Vec<_>>()
    };
    let red = |t: &str| (t.to_owned(), Color32::RED, false);
    let plain = |t: &str| (t.to_owned(), PLAIN, false);
    let composed = ("にほ".to_owned(), PLAIN, true);
    assert_eq!(at(2), [red("ab"), composed.clone(), red("cd"), plain("ef")]);
    assert_eq!(at(0), [composed.clone(), red("abcd"), plain("ef")]);
    assert_eq!(at(4), [red("abcd"), composed.clone(), plain("ef")]);
    assert_eq!(at(6), [red("abcd"), plain("ef"), composed.clone()]);
    assert_eq!(
        at(60),
        [red("abcd"), plain("ef"), composed.clone()],
        "past the end"
    );
    let job = line_job("", &[], &font(), PLAIN, Some(Preedit { col: 0, text: "x" }));
    assert_eq!(sections(&job), [("x", PLAIN, false, true)]);
    let underline = job.sections.first().map(|s| s.format.underline);
    assert_eq!(underline, Some(Stroke::new(1.0, PLAIN)));
}

#[test]
fn the_plain_styler_colours_nothing() {
    assert_eq!(Plain.revision(), 0);
    assert!(Plain.spans(0, "float x;").is_empty());
}

#[test]
fn the_cache_keeps_only_the_lines_drawn_in_the_last_frame() {
    assert!(LayoutCache::default().is_empty());
    let mut harness = Harness::new(&numbered(1_000));
    let drawn = harness.view.cache().len();
    assert!(!harness.view.cache().is_empty());
    harness.view.scroll_to(500);
    harness.frame(Vec::new(), &Plain);
    assert_eq!(
        harness.view.cache().len(),
        drawn,
        "the lines scrolled away are dropped"
    );
    let misses = harness.view.cache().misses();
    harness.view.scroll_to(0);
    harness.frame(Vec::new(), &Plain);
    assert_eq!(
        harness.view.cache().misses(),
        misses + u64::try_from(drawn).unwrap_or(u64::MAX),
        "so they are laid out again when they come back"
    );
}
