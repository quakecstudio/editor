// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "test code: a failed unwrap, index or lookup is a failed test"
)]

use egui::{
    CentralPanel, Context, Event, FontData, FontDefinitions, FontFamily, FullOutput, Key,
    Modifiers, Pos2, RawInput, Rect, Vec2, accesskit,
};

mod diff;
mod editing;
mod layout;
mod paint;

use crate::layout::{Plain, StyleRevision, StyledSpan, Styler};
use crate::{Buffer, EditState, EditorView, Extend, Motion, Position, Selection, ViewStyle, Wrap};

const FONT: &[u8] =
    include_bytes!("../../../../apps/desktop/assets/fonts/JetBrainsMono-Regular.ttf");

pub(super) fn pos(line: usize, col: usize) -> Position {
    Position::new(line, col)
}

fn carets(state: &EditState) -> Vec<Position> {
    state.selections().iter().map(|s| s.head).collect()
}

#[test]
fn a_buffer_splits_at_line_breaks_and_joins_back() {
    let buffer = Buffer::new("a\r\nb\n\nc");
    assert_eq!(buffer.line_count(), 4);
    assert_eq!(buffer.text(), "a\nb\n\nc");
    assert_eq!(Buffer::new("").line_count(), 1);
    assert_eq!(buffer.line(9), "");
    assert_eq!(buffer.line_id(9), None);
    assert_eq!(buffer.clamp(pos(9, 9)), pos(3, 1));
    assert_eq!(buffer.slice(pos(0, 0), pos(3, 1)), "a\nb\n\nc");
    assert_eq!(buffer.slice(pos(1, 0), pos(1, 1)), "b");
}

#[test]
fn a_replace_renews_only_the_lines_it_edits() {
    let mut buffer = Buffer::new("one\ntwo\nthree");
    let (first, last) = (buffer.line_id(0), buffer.line_id(2));
    let after = buffer.replace(pos(1, 1), pos(1, 2), "W\nX");
    assert_eq!(buffer.text(), "one\ntW\nXo\nthree");
    assert_eq!(after, pos(2, 1));
    assert_eq!(buffer.line_id(0), first, "the line above keeps its id");
    assert_eq!(
        buffer.line_id(3),
        last,
        "the line below moved and keeps its id"
    );
    assert_ne!(buffer.line_id(1), buffer.line_id(2));
    let after = buffer.replace(pos(2, 1), pos(0, 1), "");
    assert_eq!(
        buffer.text(),
        "oo\nthree",
        "a reversed range is the same range"
    );
    assert_eq!(after, pos(0, 1));
}

#[test]
fn moving_by_characters_steps_over_whole_characters_and_line_ends() {
    let buffer = Buffer::new("日本\nx");
    assert_eq!(buffer.next(pos(0, 0)), pos(0, 3));
    assert_eq!(buffer.next(pos(0, 6)), pos(1, 0));
    assert_eq!(buffer.next(pos(1, 1)), pos(1, 1));
    assert_eq!(buffer.previous(pos(1, 0)), pos(0, 6));
    assert_eq!(buffer.previous(pos(0, 3)), pos(0, 0));
    assert_eq!(buffer.previous(pos(0, 0)), pos(0, 0));
    assert_eq!(
        buffer.clamp(pos(0, 4)),
        pos(0, 3),
        "inside a character, down to its start"
    );
    assert_eq!(buffer.char_column(0, 6), 2);
    assert_eq!(buffer.byte_column(0, 1), 3);
    assert_eq!(buffer.byte_column(0, 9), 6);
}

#[test]
fn typing_at_several_carets_edits_each_line() {
    let mut state = EditState::new("a\nb\nc");
    state.add_caret(true);
    state.add_caret(true);
    state.add_caret(true);
    assert_eq!(state.selections().len(), 3, "no caret past the last line");
    state.insert("x");
    assert_eq!(state.buffer().text(), "xa\nxb\nxc");
    assert_eq!(carets(&state), [pos(0, 1), pos(1, 1), pos(2, 1)]);
    state.newline();
    assert_eq!(state.buffer().text(), "x\na\nx\nb\nx\nc");
    assert_eq!(carets(&state), [pos(1, 0), pos(3, 0), pos(5, 0)]);
    state.backspace();
    assert_eq!(state.buffer().text(), "xa\nxb\nxc");
    assert_eq!(carets(&state), [pos(0, 1), pos(1, 1), pos(2, 1)]);
    state.single_caret();
    assert_eq!(
        carets(&state),
        [pos(2, 1)],
        "the newest caret is the primary one"
    );
}

#[test]
fn several_carets_on_one_line_each_insert() {
    let mut state = EditState::new("ab");
    state.set_selections(&[Selection::caret(pos(0, 0)), Selection::caret(pos(0, 1))]);
    state.insert("--");
    assert_eq!(state.buffer().text(), "--a--b");
    assert_eq!(carets(&state), [pos(0, 2), pos(0, 5)]);
    state.insert("\n");
    assert_eq!(state.buffer().text(), "--\na--\nb");
    assert_eq!(carets(&state), [pos(1, 0), pos(2, 0)]);
}

#[test]
fn typing_replaces_each_selection() {
    let mut state = EditState::new("one two\nthree");
    state.set_selections(&[
        Selection {
            anchor: pos(0, 4),
            head: pos(1, 2),
        },
        Selection {
            anchor: pos(0, 0),
            head: pos(0, 3),
        },
    ]);
    assert_eq!(state.selected_text(), "one\ntwo\nth");
    state.insert("X");
    assert_eq!(state.buffer().text(), "X Xree");
    assert_eq!(carets(&state), [pos(0, 1), pos(0, 3)]);
    assert_eq!(
        state.primary().head,
        pos(0, 3),
        "the primary selection is the first given"
    );
}

#[test]
fn deleting_removes_selections_or_the_character_beside_each_caret() {
    let mut state = EditState::new("ab\ncd");
    state.set_selections(&[Selection::caret(pos(1, 0))]);
    state.backspace();
    assert_eq!(
        state.buffer().text(),
        "abcd",
        "backspace at a line's start joins it"
    );
    assert_eq!(carets(&state), [pos(0, 2)]);
    state.delete();
    assert_eq!(state.buffer().text(), "abd");
    state.set_selections(&[Selection::caret(pos(0, 0))]);
    state.backspace();
    assert_eq!(
        state.buffer().text(),
        "abd",
        "nothing before the text's start"
    );
    state.select_all();
    state.delete();
    assert_eq!(state.buffer().text(), "");
    state.delete();
    assert_eq!(state.buffer().text(), "", "nothing after the text's end");
}

#[test]
fn a_new_line_keeps_the_indent() {
    let mut state = EditState::new("\t  x = 1;");
    state.motion(Motion::End, Extend::Move);
    state.newline();
    assert_eq!(state.buffer().text(), "\t  x = 1;\n\t  ");
    assert_eq!(state.primary().head, pos(1, 3));
}

#[test]
fn moving_collapses_or_extends_a_selection() {
    let mut state = EditState::new("abcd");
    state.set_selections(&[Selection {
        anchor: pos(0, 1),
        head: pos(0, 3),
    }]);
    state.motion(Motion::Left, Extend::Move);
    assert_eq!(state.primary(), Selection::caret(pos(0, 1)));
    state.motion(Motion::Right, Extend::Select);
    state.motion(Motion::Right, Extend::Select);
    assert_eq!(state.selected_text(), "bc");
    state.motion(Motion::Right, Extend::Move);
    assert_eq!(state.primary(), Selection::caret(pos(0, 3)));
    state.motion(Motion::Home, Extend::Select);
    assert_eq!(state.selected_text(), "abc");
    state.motion(Motion::End, Extend::Move);
    assert_eq!(state.primary(), Selection::caret(pos(0, 4)));
    state.motion(Motion::TextStart, Extend::Move);
    assert_eq!(state.primary().head, pos(0, 0));
    state.motion(Motion::TextEnd, Extend::Move);
    assert_eq!(state.primary().head, pos(0, 4));
}

#[test]
fn vertical_moves_keep_the_column_they_started_at() {
    let mut state = EditState::new("abcdef\nab\nabcdef\nx");
    state.set_selections(&[Selection::caret(pos(0, 5))]);
    state.motion(Motion::Down, Extend::Move);
    assert_eq!(state.primary().head, pos(1, 2), "clamped to the short line");
    state.motion(Motion::Down, Extend::Move);
    assert_eq!(
        state.primary().head,
        pos(2, 5),
        "back to the column it started at"
    );
    state.motion(Motion::Down, Extend::Move);
    state.motion(Motion::Down, Extend::Move);
    assert_eq!(
        state.primary().head,
        pos(3, 1),
        "down from the last line is its end"
    );
    state.motion(Motion::PageUp(2), Extend::Move);
    assert_eq!(state.primary().head, pos(1, 2));
    state.motion(Motion::PageDown(9), Extend::Move);
    assert_eq!(state.primary().head, pos(3, 1));
    state.motion(Motion::Left, Extend::Move);
    state.motion(Motion::Up, Extend::Move);
    assert_eq!(
        state.primary().head,
        pos(2, 0),
        "a horizontal move resets the column"
    );
    state.set_selections(&[Selection::caret(pos(0, 3))]);
    state.motion(Motion::Up, Extend::Move);
    assert_eq!(
        state.primary().head,
        pos(0, 0),
        "up from the first line is its start"
    );
}

#[test]
fn overlapping_selections_merge() {
    let mut state = EditState::new("abcdef");
    state.set_selections(&[
        Selection {
            anchor: pos(0, 4),
            head: pos(0, 1),
        },
        Selection {
            anchor: pos(0, 3),
            head: pos(0, 5),
        },
        Selection::caret(pos(0, 5)),
    ]);
    assert_eq!(
        state.selections(),
        [Selection {
            anchor: pos(0, 5),
            head: pos(0, 1),
        }]
    );
    state.set_selections(&[]);
    assert_eq!(state.selections().len(), 1, "there is always a caret");
    state.set_selections(&[Selection::caret(pos(0, 1)), Selection::caret(pos(0, 1))]);
    assert_eq!(state.selections().len(), 1);
    state.add_caret(false);
    assert_eq!(state.selections().len(), 1, "no caret above the first line");
}

/// A styler that colours each line's first character, with a revision a test can bump.
struct FirstChar(u64);

impl Styler for FirstChar {
    fn revision(&self) -> StyleRevision {
        StyleRevision::new(self.0)
    }

    fn spans(&self, _line: usize, text: &str) -> Vec<StyledSpan> {
        let end = text.chars().next().map_or(0, char::len_utf8);
        vec![StyledSpan {
            range: 0..end,
            color: egui::Color32::RED,
            italic: false,
        }]
    }
}

/// A headless egui context with the bundled code font, and a view in an 800 by 400 window.
pub(super) struct Harness {
    pub(super) ctx: Context,
    pub(super) view: EditorView,
    pub(super) style: ViewStyle,
}

impl Harness {
    pub(super) fn new(text: &str) -> Self {
        let ctx = Context::default();
        let mut fonts = FontDefinitions::empty();
        fonts
            .font_data
            .insert("mono".to_owned(), FontData::from_static(FONT).into());
        for family in [FontFamily::Monospace, FontFamily::Proportional] {
            fonts.families.insert(family, vec!["mono".to_owned()]);
        }
        ctx.set_fonts(fonts);
        let view = EditorView::new(egui::Id::new("editor"), text);
        let mut harness = Self {
            ctx,
            view,
            style: ViewStyle::default(),
        };
        let id = harness.view.id();
        harness.ctx.memory_mut(|m| m.request_focus(id));
        harness.frame(Vec::new(), &Plain);
        harness
    }

    pub(super) fn frame(&mut self, events: Vec<Event>, styler: &dyn Styler) -> FullOutput {
        self.frame_with(events, Modifiers::NONE, styler)
    }

    pub(super) fn frame_with(
        &mut self,
        events: Vec<Event>,
        modifiers: Modifiers,
        styler: &dyn Styler,
    ) -> FullOutput {
        let mut all = vec![Event::ModifiersChanged(modifiers)];
        all.extend(events);
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 400.0))),
            events: all,
            focused: true,
            ..RawInput::default()
        };
        let (view, style) = (&mut self.view, &self.style);
        let mut output = self.ctx.run_ui(input, |ui| {
            CentralPanel::default().show(ui, |ui| {
                view.show(ui, style, styler);
            });
        });
        // A headless frame has no renderer to apply the font atlas to.
        output.textures_delta.clear();
        output
    }

    pub(super) fn key(&mut self, key: Key, modifiers: Modifiers) -> FullOutput {
        self.frame(
            vec![Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }],
            &Plain,
        )
    }

    fn type_text(&mut self, text: &str) -> FullOutput {
        self.frame(vec![Event::Text(text.to_owned())], &Plain)
    }
}

pub(super) fn numbered(lines: usize) -> String {
    (0..lines)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn only_the_visible_lines_are_laid_out() {
    let harness = Harness::new(&numbered(50_000));
    let laid_out = harness.view.cache().misses();
    assert!(laid_out > 5 && laid_out < 60, "{laid_out} lines laid out");
    assert_eq!(
        harness.view.cache().len(),
        usize::try_from(laid_out).unwrap()
    );
}

#[test]
fn an_edit_relays_out_only_the_lines_it_changed() {
    let mut harness = Harness::new(&numbered(30));
    let visible = harness.view.cache().misses();
    harness.frame(Vec::new(), &Plain);
    assert_eq!(
        harness.view.cache().misses(),
        visible,
        "an idle frame lays out nothing"
    );
    harness.type_text("x");
    assert_eq!(
        harness.view.cache().misses(),
        visible + 1,
        "the typed-in line"
    );
    harness.key(Key::Enter, Modifiers::NONE);
    assert_eq!(
        harness.view.cache().misses(),
        visible + 3,
        "the two halves of the split line; the lines it pushed down keep their layout"
    );
    harness.frame(Vec::new(), &FirstChar(1));
    assert_eq!(
        harness.view.cache().misses(),
        visible * 2 + 3,
        "a new styler revision lays out every visible line"
    );
    harness.frame(Vec::new(), &FirstChar(1));
    assert_eq!(harness.view.cache().misses(), visible * 2 + 3);
}

#[test]
fn keys_move_select_and_edit() {
    let mut harness = Harness::new("abc\ndef");
    harness.key(Key::ArrowRight, Modifiers::SHIFT);
    harness.key(Key::ArrowRight, Modifiers::SHIFT);
    assert_eq!(harness.view.state().selected_text(), "ab");
    harness.type_text("Z");
    assert_eq!(harness.view.state().buffer().text(), "Zc\ndef");
    harness.key(Key::ArrowDown, Modifiers::COMMAND | Modifiers::ALT);
    harness.type_text("!");
    assert_eq!(harness.view.state().buffer().text(), "Z!c\nd!ef");
    harness.key(Key::Escape, Modifiers::NONE);
    assert_eq!(harness.view.state().selections().len(), 1);
    harness.key(Key::A, Modifiers::COMMAND);
    assert_eq!(harness.view.state().selected_text(), "Z!c\nd!ef");
    harness.key(Key::Backspace, Modifiers::NONE);
    assert_eq!(harness.view.state().buffer().text(), "");
    harness.type_text("\u{7}");
    assert_eq!(
        harness.view.state().buffer().text(),
        "",
        "control text is not typed"
    );
    harness.frame(vec![Event::Paste("p\nq".to_owned())], &Plain);
    assert_eq!(harness.view.state().buffer().text(), "p\nq");
    harness.key(Key::Tab, Modifiers::NONE);
    assert_eq!(harness.view.state().buffer().text(), "p\nq\t");
}

#[test]
fn cut_takes_the_selection_to_the_clipboard() {
    let mut harness = Harness::new("hello");
    harness.key(Key::End, Modifiers::SHIFT);
    let copied = harness.frame(vec![Event::Copy], &Plain);
    assert_eq!(harness.view.state().buffer().text(), "hello");
    assert!(
        copied
            .platform_output
            .commands
            .iter()
            .any(|c| matches!(c, egui::OutputCommand::CopyText(t) if t == "hello"))
    );
    harness.frame(vec![Event::Cut], &Plain);
    assert_eq!(harness.view.state().buffer().text(), "");
}

#[test]
fn a_click_places_the_caret_and_alt_click_adds_one() {
    let mut harness = Harness::new(&numbered(10));
    let click = |pos: Pos2, modifiers: Modifiers| {
        vec![
            Event::PointerMoved(pos),
            Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers,
            },
            Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers,
            },
        ]
    };
    harness.frame_with(
        click(Pos2::new(700.0, 40.0), Modifiers::NONE),
        Modifiers::NONE,
        &Plain,
    );
    let first = harness.view.state().primary().head;
    assert_eq!(
        first.col,
        harness.view.state().buffer().line(first.line).len()
    );
    assert!(first.line > 0, "{first:?}");
    harness.frame_with(
        click(Pos2::new(700.0, 100.0), Modifiers::ALT),
        Modifiers::ALT,
        &Plain,
    );
    assert_eq!(harness.view.state().selections().len(), 2);
}

#[test]
fn the_wheel_scrolls_whole_lines() {
    let mut harness = Harness::new(&numbered(200));
    let wheel = |y: f32| {
        vec![
            Event::PointerMoved(Pos2::new(400.0, 200.0)),
            Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: Vec2::new(0.0, y),
                phase: egui::TouchPhase::Move,
                modifiers: Modifiers::NONE,
            },
        ]
    };
    for _ in 0..10 {
        harness.frame(wheel(-3.0), &Plain);
    }
    let scrolled = harness.view.first_line();
    assert!(scrolled > 0, "scrolled down");
    harness.key(Key::ArrowUp, Modifiers::COMMAND);
    assert_eq!(harness.view.first_line(), 0, "moving the caret reveals it");
    harness.view.scroll_to(9_999);
    assert_eq!(harness.view.first_line(), 199);
}

#[test]
fn ime_preedit_shows_and_commit_inserts() {
    let mut harness = Harness::new("ab");
    harness.key(Key::ArrowRight, Modifiers::NONE);
    let before = harness.frame(Vec::new(), &Plain);
    let caret = before.platform_output.ime.unwrap().cursor_rect;
    let preedit = |text: &str| {
        Event::Ime(egui::ImeEvent::Preedit {
            text: text.to_owned(),
            active_range_chars: None,
        })
    };
    let composing = harness.frame(vec![preedit("にほ")], &Plain);
    assert_eq!(harness.view.preedit(), Some("にほ"));
    assert_eq!(
        harness.view.state().buffer().text(),
        "ab",
        "composition is not text yet"
    );
    let moved = composing.platform_output.ime.unwrap().cursor_rect;
    assert!(
        moved.left() > caret.left(),
        "the candidate window follows the caret past the composition"
    );
    harness.frame(
        vec![Event::Ime(egui::ImeEvent::Commit("日本".to_owned()))],
        &Plain,
    );
    assert_eq!(harness.view.preedit(), None);
    assert_eq!(harness.view.state().buffer().text(), "a日本b");
    harness.frame(vec![preedit("x"), preedit("")], &Plain);
    assert_eq!(
        harness.view.preedit(),
        None,
        "an empty preedit dismisses the IME"
    );
    harness.frame(
        vec![Event::Ime(egui::ImeEvent::Commit(String::new()))],
        &Plain,
    );
    assert_eq!(harness.view.state().buffer().text(), "a日本b");
    harness.frame(
        vec![Event::Ime(egui::ImeEvent::DeleteSurrounding {
            before_chars: 1,
            after_chars: 1,
        })],
        &Plain,
    );
    assert_eq!(harness.view.state().buffer().text(), "a日");
}

#[test]
fn ime_composition_is_not_cached_and_escape_drops_it() {
    let mut harness = Harness::new("ab\ncd");
    let laid_out = harness.view.cache().misses();
    harness.frame(
        vec![Event::Ime(egui::ImeEvent::Preedit {
            text: "k".to_owned(),
            active_range_chars: None,
        })],
        &Plain,
    );
    assert_eq!(
        harness.view.cache().misses(),
        laid_out,
        "the composed line is laid out apart"
    );
    harness.key(Key::Escape, Modifiers::NONE);
    assert_eq!(harness.view.preedit(), None);
}

/// The AccessKit nodes of `output`, by id.
fn nodes(output: &FullOutput) -> Vec<(accesskit::NodeId, accesskit::Node)> {
    output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .clone()
}

/// The editor's node and its text runs, in its children's order.
fn editor_and_runs(
    output: &FullOutput,
    editor: accesskit::NodeId,
) -> (accesskit::Node, Vec<(accesskit::NodeId, accesskit::Node)>) {
    let nodes = nodes(output);
    let find = |id: accesskit::NodeId| nodes.iter().find(|(n, _)| *n == id).cloned().unwrap();
    let (_, node) = find(editor);
    let runs = node
        .children()
        .iter()
        .map(|id| find(*id))
        .filter(|(_, n)| n.role() == accesskit::Role::TextRun)
        .collect();
    (node, runs)
}

#[test]
fn the_accesskit_tree_holds_the_text_runs_caret_and_selection() {
    let mut harness = Harness::new("ab\nnihon 日本\nc");
    harness.ctx.enable_accesskit();
    harness.key(Key::ArrowDown, Modifiers::NONE);
    harness.key(Key::End, Modifiers::SHIFT);
    let output = harness.frame(Vec::new(), &Plain);
    let (node, runs) = editor_and_runs(&output, harness.view.id().accesskit_id());
    assert_eq!(node.role(), accesskit::Role::MultilineTextInput);
    let all_runs = nodes(&output)
        .iter()
        .filter(|(_, n)| n.role() == accesskit::Role::TextRun)
        .count();
    assert_eq!(
        all_runs,
        runs.len(),
        "every text run is a child of the editor"
    );
    let values: Vec<&str> = runs.iter().map(|(_, n)| n.value().unwrap()).collect();
    assert_eq!(values, ["ab\n", "nihon 日本\n", "c\n"]);
    for (_, run) in &runs {
        let total: usize = run
            .character_lengths()
            .iter()
            .map(|l| usize::from(*l))
            .sum();
        assert_eq!(total, run.value().unwrap().len());
    }
    let selection = node.text_selection().unwrap();
    assert_eq!(selection.anchor.node, runs[1].0);
    assert_eq!(selection.anchor.character_index, 0);
    assert_eq!(selection.focus.node, runs[1].0);
    assert_eq!(selection.focus.character_index, 8, "characters, not bytes");
}

#[test]
fn a_selection_off_screen_publishes_no_text_selection() {
    let mut harness = Harness::new(&numbered(500));
    harness.ctx.enable_accesskit();
    harness.view.scroll_to(300);
    let output = harness.frame(Vec::new(), &Plain);
    let (node, runs) = editor_and_runs(&output, harness.view.id().accesskit_id());
    assert!(node.text_selection().is_none());
    assert_eq!(runs[0].1.value(), Some("line 300\n"));
}

#[test]
fn touching_selections_stay_apart_and_a_caret_at_one_merges() {
    let sel = |a: usize, b: usize| Selection {
        anchor: pos(0, a),
        head: pos(0, b),
    };
    let mut state = EditState::new("abcdef");
    state.set_selections(&[sel(0, 2), sel(2, 4)]);
    assert_eq!(
        state.selections().len(),
        2,
        "two selections that only touch stay two"
    );
    state.set_selections(&[sel(0, 2), Selection::caret(pos(0, 2))]);
    assert_eq!(state.selections(), [sel(0, 2)]);
    state.set_selections(&[Selection::caret(pos(0, 2)), sel(2, 4)]);
    assert_eq!(state.selections(), [sel(2, 4)]);
    state.set_selections(&[sel(4, 1), sel(3, 5)]);
    assert_eq!(
        state.selections(),
        [sel(5, 1)],
        "merged backwards as the first ran"
    );
    state.set_selections(&[sel(1, 4), sel(3, 5)]);
    assert_eq!(state.selections(), [sel(1, 5)]);
}

#[test]
fn a_motion_keeps_the_primary_selection() {
    let mut state = EditState::new(
        "abc
abc",
    );
    state.set_selections(&[Selection::caret(pos(1, 1)), Selection::caret(pos(0, 1))]);
    state.motion(Motion::Right, Extend::Move);
    assert_eq!(state.primary().head, pos(1, 2));
    state.motion(Motion::Right, Extend::Select);
    assert_eq!(state.primary().anchor, pos(1, 2));
    assert_eq!(state.primary().head, pos(1, 3));
}

#[test]
fn a_caret_is_added_above() {
    let mut state = EditState::new(
        "abcd
ab
abcd",
    );
    state.set_selections(&[Selection::caret(pos(2, 3))]);
    state.add_caret(false);
    assert_eq!(
        carets(&state),
        [pos(1, 2), pos(2, 3)],
        "clamped to the shorter line"
    );
    assert_eq!(state.primary().head, pos(1, 2));
    state.add_caret(false);
    assert_eq!(carets(&state), [pos(0, 2), pos(1, 2), pos(2, 3)]);
}

#[test]
fn the_wheel_moves_whole_lines_and_keeps_the_rest() {
    let mut view = EditorView::new(egui::Id::new("v"), &numbered(100));
    view.scroll(-25.0, 10.0);
    assert_eq!(view.first_line(), 2, "two and a half lines down is two");
    view.scroll(-5.0, 10.0);
    assert_eq!(view.first_line(), 3, "the half kept makes the third");
    view.scroll(-4.0, 10.0);
    assert_eq!(view.first_line(), 3, "less than a line moves nothing");
    view.scroll(35.0, 10.0);
    assert_eq!(view.first_line(), 0, "up past the top stops at it");
    view.scroll(-10_000.0, 10.0);
    assert_eq!(
        view.first_line(),
        99,
        "down past the end stops at the last line"
    );
}

#[test]
fn keys_move_by_character_line_page_and_text() {
    let mut harness = Harness::new(&numbered(200));
    harness.key(Key::ArrowRight, Modifiers::NONE);
    harness.key(Key::ArrowRight, Modifiers::NONE);
    harness.key(Key::ArrowLeft, Modifiers::NONE);
    assert_eq!(harness.view.state().primary().head, pos(0, 1));
    harness.key(Key::Delete, Modifiers::NONE);
    assert_eq!(harness.view.state().buffer().line(0), "lne 0");
    harness.key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(harness.view.state().primary().head, pos(1, 1));
    harness.key(Key::ArrowUp, Modifiers::NONE);
    assert_eq!(harness.view.state().primary().head, pos(0, 1));
    harness.key(Key::End, Modifiers::NONE);
    assert_eq!(harness.view.state().primary().head, pos(0, 5));
    harness.key(Key::Home, Modifiers::NONE);
    assert_eq!(harness.view.state().primary().head, pos(0, 0));
    harness.key(Key::PageDown, Modifiers::NONE);
    let page = harness.view.state().primary().head.line;
    assert!(page > 5, "a page is the visible lines less one: {page}");
    assert_eq!(harness.view.first_line(), 0, "the caret is still on screen");
    harness.key(Key::PageDown, Modifiers::NONE);
    assert_eq!(harness.view.state().primary().head.line, page * 2);
    assert_eq!(
        harness.view.first_line(),
        page,
        "the caret is the last visible line"
    );
    harness.key(Key::PageUp, Modifiers::NONE);
    assert_eq!(harness.view.state().primary().head.line, page);
    harness.key(Key::ArrowDown, Modifiers::COMMAND);
    assert_eq!(
        harness.view.state().primary().head,
        harness.view.state().buffer().end()
    );
    assert_eq!(
        harness.view.state().selections().len(),
        1,
        "Cmd alone adds no caret"
    );
    assert_eq!(
        harness.view.first_line(),
        199 - page,
        "the end of the text is revealed"
    );
    harness.key(Key::ArrowUp, Modifiers::COMMAND);
    assert_eq!(harness.view.state().primary().head, pos(0, 0));
    assert_eq!(harness.view.first_line(), 0);
    harness.key(Key::ArrowDown, Modifiers::ALT);
    assert_eq!(
        harness.view.state().primary().head.line,
        1,
        "Alt alone is a plain move"
    );
    assert_eq!(harness.view.state().selections().len(), 1);
    harness.key(Key::ArrowUp, Modifiers::COMMAND | Modifiers::ALT);
    assert_eq!(
        harness.view.state().selections().len(),
        2,
        "a caret added above"
    );
    harness.key(Key::ArrowDown, Modifiers::ALT);
    assert_eq!(
        harness.view.state().selections().len(),
        2,
        "Alt alone adds no caret"
    );
    harness.key(Key::A, Modifiers::NONE);
    assert_eq!(
        harness.view.state().selected_text(),
        "",
        "A without Cmd selects nothing"
    );
}

#[test]
fn a_drag_selects_and_a_shift_click_extends() {
    let mut harness = Harness::new(&numbered(10));
    let at = |line: f32| Pos2::new(700.0, 20.0 * line + 15.0);
    let press = |p: Pos2| Event::PointerButton {
        pos: p,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    };
    let release = |p: Pos2| Event::PointerButton {
        pos: p,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    };
    harness.frame(vec![Event::PointerMoved(at(0.0)), press(at(0.0))], &Plain);
    for step in 1..=6 {
        let p = Pos2::new(
            700.0 - 60.0 * f32::from(u8::try_from(step).unwrap()),
            at(2.0).y,
        );
        harness.frame(vec![Event::PointerMoved(p)], &Plain);
    }
    harness.frame(vec![release(Pos2::new(340.0, at(2.0).y))], &Plain);
    let selection = harness.view.state().primary();
    assert_eq!(selection.anchor.line, 0);
    assert!(selection.head.line > selection.anchor.line, "{selection:?}");
    assert!(!selection.is_caret());
    let head = selection.head;
    harness
        .view
        .state_mut()
        .set_selections(&[Selection::caret(pos(0, 1)), Selection::caret(pos(5, 1))]);
    let shift_click = vec![
        Event::PointerMoved(at(2.0)),
        Event::PointerButton {
            pos: at(2.0),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::SHIFT,
        },
        Event::PointerButton {
            pos: at(2.0),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::SHIFT,
        },
    ];
    harness.frame_with(shift_click, Modifiers::SHIFT, &Plain);
    let state = harness.view.state();
    assert_eq!(
        state.primary().anchor,
        pos(0, 1),
        "the primary selection is extended"
    );
    assert_eq!(state.selections().len(), 2, "and the other caret kept");
    assert_ne!(head, pos(0, 0));
}

/// A press or a release of the primary button at `pos`.
fn button(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

// No spec statement: P7-16: a drag of the scroll bar's thumb scrolls the view and selects
// nothing, to the file's last line at the track's end and back to its first at the top
#[test]
fn a_drag_of_the_scroll_bars_thumb_scrolls_the_view() {
    let mut harness = Harness::new(&numbered(400));
    let bar = |y: f32| Pos2::new(786.0, y);
    harness.frame(vec![Event::PointerMoved(bar(14.0))], &Plain);
    harness.frame(vec![button(bar(14.0), true)], &Plain);
    harness.frame(vec![Event::PointerMoved(bar(24.0))], &Plain);
    let nudged = harness.view.first_line();
    assert!(nudged <= 2, "held where it was taken: {nudged}");
    for y in [60.0, 120.0, 200.0] {
        harness.frame(vec![Event::PointerMoved(bar(y))], &Plain);
    }
    let middle = harness.view.first_line();
    assert!(middle > 100 && middle < 300, "{middle}");
    harness.frame(vec![Event::PointerMoved(bar(900.0))], &Plain);
    assert_eq!(
        harness.view.first_line(),
        399,
        "the track's end is the last line"
    );
    harness.frame(vec![Event::PointerMoved(bar(-50.0))], &Plain);
    assert_eq!(harness.view.first_line(), 0);
    harness.frame(vec![Event::PointerMoved(bar(200.0))], &Plain);
    harness.frame(vec![button(bar(200.0), false)], &Plain);
    assert_eq!(harness.view.first_line(), middle);
    let selection = harness.view.state().primary();
    assert!(selection.is_caret(), "{selection:?}");
    assert_eq!(selection.head, pos(0, 0), "the caret stays where it was");
    // Released, the pointer moves the view no more.
    harness.frame(vec![Event::PointerMoved(bar(300.0))], &Plain);
    assert_eq!(harness.view.first_line(), middle);

    // A thumb taken part of the way down stays under the pointer where it was taken.
    harness.view.scroll_to(200);
    harness.frame(vec![Event::PointerMoved(bar(200.0))], &Plain);
    harness.frame(vec![button(bar(200.0), true)], &Plain);
    harness.frame(vec![Event::PointerMoved(bar(210.0))], &Plain);
    let held = harness.view.first_line();
    assert!((198..=202).contains(&held), "taken with no jump: {held}");
    harness.frame(vec![Event::PointerMoved(bar(220.0))], &Plain);
    let moved = harness.view.first_line();
    assert!((209..=214).contains(&moved), "ten points on: {moved}");
    harness.frame(vec![button(bar(220.0), false)], &Plain);
    // A drag begun beside the thumb takes it by its middle.
    harness.view.scroll_to(0);
    harness.frame(vec![Event::PointerMoved(bar(300.0))], &Plain);
    harness.frame(vec![button(bar(300.0), true)], &Plain);
    harness.frame(vec![Event::PointerMoved(bar(310.0))], &Plain);
    let taken = harness.view.first_line();
    assert!((316..=326).contains(&taken), "{taken}");
}

// No spec statement: P7-16: a click in the scroll bar beside the thumb pages toward it; a file
// the view shows whole has no scroll bar, and a click at its place is a click on the text
#[test]
fn a_click_beside_the_thumb_pages_and_a_short_file_has_no_bar() {
    let mut harness = Harness::new(&numbered(400));
    let bar = |y: f32| Pos2::new(786.0, y);
    let click = |harness: &mut Harness, at: Pos2| {
        harness.frame(vec![Event::PointerMoved(at)], &Plain);
        harness.frame(vec![button(at, true)], &Plain);
        harness.frame(vec![button(at, false)], &Plain);
    };
    click(&mut harness, bar(380.0));
    let paged = harness.view.first_line();
    assert!(paged > 5 && paged < 40, "a page down: {paged}");
    click(&mut harness, bar(380.0));
    assert_eq!(harness.view.first_line(), paged * 2);
    harness.view.scroll_to(200);
    harness.frame(Vec::new(), &Plain);
    click(&mut harness, bar(12.0));
    assert_eq!(harness.view.first_line(), 200 - paged, "a page up");
    assert_eq!(harness.view.state().primary().head, pos(0, 0));

    let mut short = Harness::new(&numbered(5));
    click(&mut short, Pos2::new(786.0, 70.0));
    assert_eq!(short.view.first_line(), 0);
    assert!(
        short.view.state().primary().head.line > 0,
        "the click placed the caret"
    );
}

// The width of one character and the left edge of the text, read from the caret at the
// start of a line and fifty characters along it: each character is drawn on a whole pixel,
// so one alone is a fraction off.
fn metrics(harness: &mut Harness) -> (f32, f32) {
    harness.key(Key::Home, Modifiers::NONE);
    let start = harness.view.caret_rect().unwrap().left();
    for _ in 0..50 {
        harness.key(Key::ArrowRight, Modifiers::NONE);
    }
    let along = harness.view.caret_rect().unwrap().left();
    harness.key(Key::Home, Modifiers::NONE);
    ((along - start) / 50.0, start)
}

// A column as a number to compare with a width in characters.
fn points(col: usize) -> f32 {
    f32::from(u16::try_from(col).unwrap())
}

// The right edge of the text in the harness's view: the window's, less the panel's margin
// and the scroll bar down it.
const TEXT_RIGHT: f32 = 780.0;

// No spec statement: P7-16: with wrap off a caret moved along a line wider than the view is
// scrolled into view across with four characters of the line past it, the wheel scrolls
// across, and a click lands on the character drawn under it; wrapped lines are not scrolled
// and end short of the scroll bar
#[test]
fn a_caret_on_a_wide_line_is_scrolled_into_view_across() {
    let wide = format!("{}\n{}\n", "0".repeat(400), "0".repeat(300));
    let mut harness = Harness::new(&wide);
    let (char_width, text_left) = metrics(&mut harness);
    assert!(
        char_width > 4.0 && text_left > 20.0,
        "{char_width} {text_left}"
    );
    assert!(harness.view.scrolled_across().abs() < f32::EPSILON);
    harness.key(Key::ArrowDown, Modifiers::NONE);
    harness.key(Key::End, Modifiers::NONE);
    let scrolled = harness.view.scrolled_across();
    let caret = harness.view.caret_rect().unwrap();
    let from_right = TEXT_RIGHT - caret.left();
    assert!(
        (from_right - 4.0 * char_width).abs() < 1.0,
        "four characters left of the edge: {caret:?}"
    );
    assert!(
        (scrolled - (300.0 * char_width - (caret.left() - text_left))).abs() < 1.0,
        "{scrolled}"
    );
    // A click near the view's left edge is on the character drawn there.
    let at = Pos2::new(text_left + 10.5 * char_width, 20.0);
    harness.frame(vec![Event::PointerMoved(at)], &Plain);
    harness.frame(vec![button(at, true)], &Plain);
    harness.frame(vec![button(at, false)], &Plain);
    let head = harness.view.state().primary().head;
    let under = (scrolled / char_width) + 10.5;
    assert_eq!(head.line, 0);
    assert!((points(head.col) - under).abs() <= 1.0, "{head:?} {under}");
    assert!(
        (harness.view.scrolled_across() - scrolled).abs() < f32::EPSILON,
        "a caret in view moves nothing"
    );
    // Moving left, the view follows once the caret is four characters from its left edge.
    for _ in 0..30 {
        harness.key(Key::ArrowLeft, Modifiers::NONE);
    }
    let caret = harness.view.caret_rect().unwrap();
    assert!(
        (caret.left() - text_left - 4.0 * char_width).abs() < 1.0,
        "{caret:?}"
    );
    assert!(harness.view.scrolled_across() < scrolled - 10.0 * char_width);

    // Moving right a character at a time, the caret keeps its four characters from the
    // right edge too.
    harness.key(Key::Home, Modifiers::NONE);
    for _ in 0..120 {
        harness.key(Key::ArrowRight, Modifiers::NONE);
        let caret = harness.view.caret_rect().unwrap();
        assert!(TEXT_RIGHT - caret.left() > 3.0 * char_width, "{caret:?}");
    }
    assert!(harness.view.scrolled_across() > 20.0 * char_width);

    let wheel = |x: f32| {
        vec![
            Event::PointerMoved(Pos2::new(400.0, 200.0)),
            Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: Vec2::new(x, 0.0),
                phase: egui::TouchPhase::Move,
                modifiers: Modifiers::NONE,
            },
        ]
    };
    let before = harness.view.scrolled_across();
    for _ in 0..20 {
        harness.frame(wheel(-30.0), &Plain);
    }
    assert!(
        harness.view.scrolled_across() > before + 100.0,
        "wheeled on"
    );
    harness.key(Key::Home, Modifiers::NONE);
    assert!(harness.view.scrolled_across().abs() < f32::EPSILON);

    harness.key(Key::End, Modifiers::NONE);
    harness.view.set_wrap(Wrap::Width);
    harness.frame(Vec::new(), &Plain);
    assert!(harness.view.scrolled_across().abs() < f32::EPSILON);
    // The wrapped line's rows end short of the bar down the view: its second row starts
    // after as many characters as fit left of the bar, and no more.
    let at = Pos2::new(text_left + 1.0, 36.0);
    harness.frame(vec![Event::PointerMoved(at)], &Plain);
    harness.frame(vec![button(at, true)], &Plain);
    harness.frame(vec![button(at, false)], &Plain);
    let head = harness.view.state().primary().head;
    let fit = ((TEXT_RIGHT - text_left - 2.0) / char_width).floor();
    assert_eq!(head.line, 0);
    assert!((points(head.col) - fit).abs() < 0.5, "{head:?} of {fit}");
}

// No spec statement: P7-16: a drag of the thumb of the bar across the view's bottom scrolls
// to the widest line's end at the track's end and no further, and back; the bar is the view's
// last twelve points, right of the gutter; a line out of sight is counted in the width; lines
// the view shows whole have no bar there, and a click at its place is a click on the text
#[test]
fn a_drag_of_the_bar_across_scrolls_to_the_widest_lines_end() {
    let wide = format!("{}\n", "0".repeat(400));
    let mut harness = Harness::new(&wide);
    let (char_width, text_left) = metrics(&mut harness);
    let bar = |x: f32| Pos2::new(x, 386.0);
    harness.frame(vec![Event::PointerMoved(bar(60.0))], &Plain);
    harness.frame(vec![button(bar(60.0), true)], &Plain);
    harness.frame(vec![Event::PointerMoved(bar(70.0))], &Plain);
    let nudged = harness.view.scrolled_across();
    assert!(
        nudged < 10.0 * char_width,
        "held where it was taken: {nudged}"
    );
    harness.frame(vec![Event::PointerMoved(bar(300.0))], &Plain);
    let middle = harness.view.scrolled_across();
    assert!(middle > 100.0, "{middle}");
    harness.frame(vec![Event::PointerMoved(bar(2_000.0))], &Plain);
    let end = harness.view.scrolled_across();
    // The line's 400 characters and room for a caret after them, less a view of them.
    let furthest = 401.0 * char_width - (TEXT_RIGHT - text_left);
    assert!((end - furthest).abs() < 1.5, "{end} of {furthest}");
    harness.frame(vec![Event::PointerMoved(bar(3_000.0))], &Plain);
    assert!((harness.view.scrolled_across() - end).abs() < f32::EPSILON);
    harness.frame(vec![Event::PointerMoved(bar(-500.0))], &Plain);
    assert!(harness.view.scrolled_across().abs() < f32::EPSILON);
    harness.frame(vec![button(bar(-500.0), false)], &Plain);
    assert_eq!(harness.view.state().primary().head, pos(0, 0));

    // Above the bar's twelve points, and in the gutter beside it, a click is not the bar's.
    let click = |harness: &mut Harness, at: Pos2| {
        harness.frame(vec![Event::PointerMoved(at)], &Plain);
        harness.frame(vec![button(at, true)], &Plain);
        harness.frame(vec![button(at, false)], &Plain);
    };
    click(&mut harness, Pos2::new(600.0, 376.0));
    assert!(harness.view.scrolled_across().abs() < f32::EPSILON);
    assert_eq!(
        harness.view.state().primary().head.line,
        1,
        "a click on text"
    );
    click(&mut harness, Pos2::new(text_left - 4.0, 386.0));
    assert!(harness.view.scrolled_across().abs() < f32::EPSILON);
    // In the track right of the thumb, a click pages on by a view less a point.
    click(&mut harness, bar(600.0));
    let paged = harness.view.scrolled_across();
    assert!(
        (paged - (TEXT_RIGHT - text_left - 1.0)).abs() < 1.5,
        "{paged}"
    );

    // A wide line out of sight is scrolled to as one in sight is.
    let mut text = numbered(100);
    text.push('\n');
    text.push_str(&"0".repeat(400));
    let mut far = Harness::new(&text);
    // Its gutter is a digit wider, so its text starts further right.
    let text_left = far.view.caret_rect().unwrap().left();
    let furthest = 401.0 * char_width - (TEXT_RIGHT - text_left);
    far.frame(vec![Event::PointerMoved(bar(60.0))], &Plain);
    far.frame(vec![button(bar(60.0), true)], &Plain);
    far.frame(vec![Event::PointerMoved(bar(2_000.0))], &Plain);
    far.frame(vec![button(bar(2_000.0), false)], &Plain);
    let reached = far.view.scrolled_across();
    assert!((reached - furthest).abs() < 1.5, "{reached} of {furthest}");
    // Once it is gone, there is nothing to scroll across.
    far.view.state_mut().set_selections(&[Selection {
        anchor: pos(99, 0),
        head: pos(100, 400),
    }]);
    far.view.state_mut().insert("");
    far.frame(Vec::new(), &Plain);
    assert!(far.view.scrolled_across().abs() < f32::EPSILON);
    let wheel = vec![
        Event::PointerMoved(Pos2::new(400.0, 200.0)),
        Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: Vec2::new(-300.0, 0.0),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
        },
    ];
    for _ in 0..5 {
        far.frame(wheel.clone(), &Plain);
    }
    assert!(
        far.view.scrolled_across().abs() < f32::EPSILON,
        "the width is measured again after an edit"
    );

    let mut short = Harness::new(&numbered(40));
    click(&mut short, bar(200.0));
    assert!(
        short.view.state().primary().head.line > 0,
        "a click on text"
    );
    assert!(short.view.scrolled_across().abs() < f32::EPSILON);
}
