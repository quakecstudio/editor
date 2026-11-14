// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::integer_division,
    reason = "test code: a failed unwrap or index is a failed test; small counts"
)]

use super::{
    CON_HISTORY, CON_TEXTSIZE, Connection, Console, ConsoleHold, ConsoleKey, InputLine, KeyDest,
    Typed,
};
use crate::hud::{HudItem, Placed};

/// A console for a 640-wide screen: lines of 78 characters (CON-1.1).
fn wide() -> Console {
    let mut console = Console::new();
    console.resize(640);
    console
}

/// The text of `console`'s line `n`, its trailing spaces dropped.
fn text(console: &Console, n: i64) -> String {
    String::from_utf8_lossy(console.line(n))
        .trim_end()
        .to_owned()
}

/// No completion.
fn none(_: &[u8]) -> Option<Vec<u8>> {
    None
}

/// The characters `items` draw, in order, at the row `y`.
fn row_at(items: &[Placed], y: i32) -> String {
    items
        .iter()
        .filter_map(|placed| match placed.item {
            HudItem::Char { y: at, ch, .. } if at == y => Some(char::from(ch)),
            _ => None,
        })
        .collect::<String>()
        .trim_end()
        .to_owned()
}

// spec CON-1.1: before the video is up a line is 38 characters; then `(width >> 3) - 2`, the
// lines `CON_TEXTSIZE` over it, the last lines kept, the newest at the bottom
#[test]
fn the_width_follows_the_screen_and_keeps_the_text() {
    let mut console = Console::new();
    assert_eq!(console.line_width(), 38);
    assert_eq!(
        console.current(),
        i64::try_from(CON_TEXTSIZE / 38).unwrap() - 1
    );
    console.print(b"first\nsecond\n", 1.0);
    console.resize(640);
    assert_eq!(console.line_width(), 78);
    let current = console.current();
    assert_eq!(current, i64::try_from(CON_TEXTSIZE / 78).unwrap() - 1);
    assert_eq!(text(&console, current), "second");
    assert_eq!(text(&console, current - 1), "first");
    console.resize(8);
    assert_eq!(console.line_width(), 38, "a width below 1 is 38");
    assert_eq!(text(&console, console.current()), "");
}

// spec CON-1.2: a leading 1 or 2 sets the high bit of what follows; 1 asks for the talk sound
#[test]
fn a_marker_byte_colours_the_text() {
    let mut console = wide();
    assert!(console.print(b"\x01hi\n", 1.0));
    assert_eq!(
        console.line(console.current())[..2],
        [b'h' | 128, b'i' | 128]
    );
    assert!(!console.print(b"\x02yo\n", 1.0));
    assert_eq!(console.line(console.current())[0], b'y' | 128);
    assert!(!console.print(b"plain\n", 1.0));
    assert_eq!(console.line(console.current())[0], b'p');
}

// spec CON-1.3: a word that does not fit in what is left of the line starts a new one, the word
// measured from each character up to a line
#[test]
fn words_wrap_to_the_next_line() {
    let mut console = Console::new();
    // 33 and a space leave 4 of 38: "wrapped" does not fit; 30 would, ending at the edge.
    let line = "a".repeat(33);
    console.print(format!("{line} wrapped").as_bytes(), 1.0);
    let current = console.current();
    assert_eq!(text(&console, current - 1), line);
    assert_eq!(text(&console, current), "wrapped");
    let mut long = Console::new();
    long.print("b".repeat(40).as_bytes(), 1.0);
    // The word is measured at each character: whole lines while 38 or more are left, then at
    // column 3 the 37 left do not fit, and wrap.
    let current = long.current();
    assert_eq!(text(&long, current - 1), "bbb");
    assert_eq!(text(&long, current), "b".repeat(37));
}

// spec CON-1.4: a new line is cleared; `\n` ends a line; `\r` makes the next text overwrite it
#[test]
fn newlines_and_carriage_returns() {
    let mut console = wide();
    console.print(b"one\ntwo", 1.0);
    let current = console.current();
    assert_eq!(text(&console, current - 1), "one");
    assert_eq!(text(&console, current), "two");
    console.print(b"\rthree", 1.0);
    assert_eq!(
        console.current(),
        current,
        "the next text is on the same line"
    );
    assert_eq!(text(&console, current), "three");
    console.print(b"\rfour\n", 1.0);
    assert_eq!(text(&console, current), "four");
}

// spec CON-1.5: clear fills the text with spaces; printing scrolls back to the bottom
#[test]
fn clear_and_scrollback() {
    let mut console = wide();
    console.print(b"text\n", 1.0);
    console.key(ConsoleKey::PageUp, 480, 1.0, &none);
    console.print(b"more\n", 1.0);
    let lines = console.draw(240, 480, 1.0, InputLine::Hidden);
    assert!(
        lines
            .iter()
            .any(|p| row_at(std::slice::from_ref(p), 216) == "m")
    );
    console.clear();
    assert_eq!(text(&console, console.current()), "");
}

// spec CON-2.1: each new line's time is kept, four at most
// spec CON-2.2: the last four lines show for three seconds, from the top, 8 apart
#[test]
fn notify_lines_show_for_three_seconds() {
    let mut console = wide();
    console.print(b"old\n", 1.0);
    console.print(b"a\nb\nc\nd\n", 5.0);
    let shown = console.notify(6.0);
    assert_eq!(row_at(&shown, 0), "a");
    assert_eq!(row_at(&shown, 24), "d");
    assert!(shown.iter().all(|p| match p.item {
        HudItem::Char { x, .. } => x >= 8,
        _ => false,
    }));
    assert!(
        console.notify(8.5).is_empty(),
        "more than three seconds later"
    );
}

// spec CON-2.3: toggling clears the notify times
// spec CON-4.8: from the game the console opens; from the console the game, connected, else
// the menu; the typed line is cleared
#[test]
fn toggling_the_console() {
    let mut console = wide();
    console.print(b"line\n", 1.0);
    assert_eq!(
        console.toggle(KeyDest::Game, Connection::Connected),
        KeyDest::Console
    );
    assert!(console.notify(1.5).is_empty());
    console.key(ConsoleKey::Char(b'x'), 480, 1.0, &none);
    assert_eq!(
        console.toggle(KeyDest::Console, Connection::Connected),
        KeyDest::Game
    );
    assert_eq!(console.typed(), b"");
    assert_eq!(
        console.toggle(KeyDest::Console, Connection::Disconnected),
        KeyDest::Menu
    );
    assert_eq!(
        console.toggle(KeyDest::Menu, Connection::Disconnected),
        KeyDest::Console
    );
}

// spec CON-3.1: forced up it fills the screen at once; else it slides to half the screen with
// the console the destination, and back to none, 300 pixels a second
#[test]
fn the_console_slides() {
    let mut console = wide();
    assert_eq!(
        console.slide(480, ConsoleHold::Forced, KeyDest::Game, 0.1),
        480
    );
    assert_eq!(
        console.slide(480, ConsoleHold::Sliding, KeyDest::Game, 0.1),
        450
    );
    assert_eq!(
        console.slide(480, ConsoleHold::Sliding, KeyDest::Game, 2.0),
        0
    );
    assert_eq!(
        console.slide(480, ConsoleHold::Sliding, KeyDest::Console, 0.5),
        150
    );
    assert_eq!(
        console.slide(480, ConsoleHold::Sliding, KeyDest::Console, 1.0),
        240
    );
    assert_eq!(
        console.slide(480, ConsoleHold::Sliding, KeyDest::Menu, 0.0),
        240
    );
}

// spec CON-3.2: the background first, with the console's height and the screen's
// spec CON-3.3: `(lines - 16) >> 3` rows, the last 16 above the bottom, from x 8
#[test]
fn the_console_draws_its_rows() {
    let mut console = wide();
    console.print(b"alpha\nbeta\n", 1.0);
    let items = console.draw(240, 480, 1.0, InputLine::Hidden);
    assert_eq!(
        items[0].item,
        HudItem::ConsoleBack {
            lines: 240,
            height: 480,
            forced: ConsoleHold::Sliding
        }
    );
    // 28 rows from y 0: the newest, beta, at 27 * 8 = 216.
    assert_eq!(row_at(&items, 216), "beta");
    assert_eq!(row_at(&items, 208), "alpha");
    assert!(console.draw(0, 480, 1.0, InputLine::Shown).is_empty());
}

// spec CON-3.4: the input line 16 above the bottom, its cursor 10 or 11 by
// `(int)(realtime * 4) & 1`, scrolled when the position passes the width
#[test]
fn the_input_line_blinks_its_cursor() {
    let mut console = wide();
    for c in b"map" {
        console.key(ConsoleKey::Char(*c), 480, 1.0, &none);
    }
    let even = console.draw(240, 480, 1.0, InputLine::Shown);
    let input: Vec<u8> = even
        .iter()
        .filter_map(|p| match p.item {
            HudItem::Char { y: 224, ch, .. } => Some(ch),
            _ => None,
        })
        .collect();
    assert_eq!(&input[..5], b"]map\x0a");
    let odd = console.draw(240, 480, 1.3, InputLine::Shown);
    assert!(odd.iter().any(|p| p.item
        == HudItem::Char {
            x: 40,
            y: 224,
            ch: 11
        }));
    let mut long = wide();
    for _ in 0..90 {
        long.key(ConsoleKey::Char(b'z'), 480, 1.0, &none);
    }
    let shown = long.draw(240, 480, 1.0, InputLine::Shown);
    let first = shown
        .iter()
        .find_map(|p| match p.item {
            HudItem::Char { x: 8, y: 224, ch } => Some(ch),
            _ => None,
        })
        .unwrap();
    assert_eq!(first, b'z', "scrolled past the prompt");
}

// spec CON-4.1: 32 lines of history, each starting with the prompt, the position at 1
// spec CON-4.2: Enter runs the line, prints it with its prompt and starts the next
#[test]
fn enter_runs_the_line() {
    let mut console = wide();
    for c in b"god" {
        console.key(ConsoleKey::Char(*c), 480, 1.0, &none);
    }
    assert_eq!(
        console.key(ConsoleKey::Enter, 480, 1.0, &none),
        Typed::Run(b"god".to_vec())
    );
    assert_eq!(text(&console, console.current()), "]god");
    assert_eq!(console.typed(), b"");
    for n in 0..CON_HISTORY + 2 {
        console.key(
            ConsoleKey::Char(b'a' + u8::try_from(n % 26).unwrap()),
            480,
            1.0,
            &none,
        );
        console.key(ConsoleKey::Enter, 480, 1.0, &none);
    }
    assert_eq!(console.typed(), b"", "the ring of 32 wraps");
}

// spec CON-4.3: Tab completes the command or variable, then a space
#[test]
fn tab_completes() {
    let mut console = wide();
    for c in b"no" {
        console.key(ConsoleKey::Char(*c), 480, 1.0, &none);
    }
    let complete = |typed: &[u8]| (typed == b"no").then(|| b"noclip".to_vec());
    console.key(ConsoleKey::Tab, 480, 1.0, &complete);
    assert_eq!(console.typed(), b"noclip ");
    console.key(ConsoleKey::Tab, 480, 1.0, &complete);
    assert_eq!(console.typed(), b"noclip ", "nothing completes it now");
}

// spec CON-4.4: Back moves the position back, not before the prompt; the next key overwrites
#[test]
fn back_moves_the_position() {
    let mut console = wide();
    for c in b"ab" {
        console.key(ConsoleKey::Char(*c), 480, 1.0, &none);
    }
    console.key(ConsoleKey::Back, 480, 1.0, &none);
    console.draw(240, 480, 1.0, InputLine::Shown);
    assert_eq!(
        console.typed(),
        b"a",
        "drawn, the line ends at the position"
    );
    console.key(ConsoleKey::Char(b'c'), 480, 1.0, &none);
    assert_eq!(console.typed(), b"ac");
    for _ in 0..5 {
        console.key(ConsoleKey::Back, 480, 1.0, &none);
    }
    console.key(ConsoleKey::Char(b'x'), 480, 1.0, &none);
    assert_eq!(console.typed(), b"x");
}

// spec CON-4.5: up steps back through lines with text, down forward, and past the last the
// line is empty
#[test]
fn history_recalls_earlier_lines() {
    let mut console = wide();
    for line in [b"one".as_slice(), b"two"] {
        for c in line {
            console.key(ConsoleKey::Char(*c), 480, 1.0, &none);
        }
        console.key(ConsoleKey::Enter, 480, 1.0, &none);
    }
    console.key(ConsoleKey::Enter, 480, 1.0, &none);
    console.key(ConsoleKey::Up, 480, 1.0, &none);
    assert_eq!(console.typed(), b"two");
    console.key(ConsoleKey::Up, 480, 1.0, &none);
    assert_eq!(console.typed(), b"one");
    console.key(ConsoleKey::Down, 480, 1.0, &none);
    assert_eq!(console.typed(), b"two");
    console.key(ConsoleKey::Down, 480, 1.0, &none);
    // The prompt and the position reset, the line cut there (CON-5.5).
    assert_eq!(console.typed(), b"");
    assert_eq!(
        console.key(ConsoleKey::Down, 480, 1.0, &none),
        Typed::Nothing
    );
}

// spec CON-4.6: page up and down scroll 2 lines within 0 and the limit; Home and End go to
// either
#[test]
fn the_scrollback_moves_within_its_limits() {
    let mut console = wide();
    for n in 0..40 {
        console.print(format!("line {n}\n").as_bytes(), 1.0);
    }
    let bottom = row_at(&console.draw(240, 480, 1.0, InputLine::Hidden), 216);
    console.key(ConsoleKey::PageUp, 480, 1.0, &none);
    assert_eq!(
        row_at(&console.draw(240, 480, 1.0, InputLine::Hidden), 216),
        "line 37"
    );
    console.key(ConsoleKey::PageDown, 480, 1.0, &none);
    console.key(ConsoleKey::PageDown, 480, 1.0, &none);
    assert_eq!(
        row_at(&console.draw(240, 480, 1.0, InputLine::Hidden), 216),
        bottom
    );
    console.key(ConsoleKey::Home, 480, 1.0, &none);
    let top = console.draw(240, 480, 1.0, InputLine::Hidden);
    assert_eq!(row_at(&top, 216), "", "back past every line printed");
    console.key(ConsoleKey::End, 480, 1.0, &none);
    assert_eq!(
        row_at(&console.draw(240, 480, 1.0, InputLine::Hidden), 216),
        bottom
    );
}

// spec CON-4.7: printable keys are stored while the position is below 255; others do nothing
#[test]
fn only_printable_keys_are_typed() {
    let mut console = wide();
    console.key(ConsoleKey::Char(7), 480, 1.0, &none);
    console.key(ConsoleKey::Char(200), 480, 1.0, &none);
    assert_eq!(console.typed(), b"");
    for _ in 0..300 {
        console.key(ConsoleKey::Char(b'q'), 480, 1.0, &none);
    }
    assert_eq!(console.typed().len(), 254);
}

/// QuakeSpasm's console of `console` at half a 480-high screen, at `realtime`.
fn quakespasm(console: &mut Console, realtime: f64) -> Vec<Placed> {
    console.draw_quakespasm(240, 480, realtime, InputLine::Shown, b"QuakeC Studio 1")
}

// spec CON-5.1: laid out at the screen's height and moved up by what is not shown: the
// background first, `(lines + 7) / 8` rows up from the bottom
// spec CON-5.2: the text ends 16 above the bottom; scrolled back, a blank row and `^` every
// fourth column take the last two
#[test]
fn quakespasm_lays_out_its_rows_from_the_bottom() {
    let mut console = wide();
    for n in 0..40 {
        console.print(format!("line {n}\n").as_bytes(), 1.0);
    }
    let items = quakespasm(&mut console, 1.0);
    assert_eq!(
        items[0].item,
        HudItem::ConsoleBack {
            lines: 240,
            height: 480,
            forced: ConsoleHold::Sliding
        }
    );
    // 30 rows, 28 of text: moved up by 240, the newest at 480 - 16 - 8 - 240.
    assert_eq!(row_at(&items, 216), "line 39");
    assert_eq!(row_at(&items, 0), "line 12");
    assert!(
        console
            .draw_quakespasm(0, 480, 1.0, InputLine::Shown, b"v")
            .is_empty()
    );
    console.key(ConsoleKey::PageUp, 480, 1.0, &none);
    let back = quakespasm(&mut console, 1.0);
    assert_eq!(row_at(&back, 200), "line 35");
    assert_eq!(row_at(&back, 208), "");
    let arrows = row_at(&back, 216);
    assert!(arrows.starts_with("^   ^   ^"), "{arrows}");
    assert_eq!(arrows.matches('^').count(), 78usize.div_ceil(4));
}

// spec CON-5.3: the input line 16 above the bottom, not cut off at the position, a block cursor
// when `(int)(realtime * 4) & 1` is 0
#[test]
fn quakespasm_draws_the_whole_input_line() {
    let mut console = wide();
    for c in b"map" {
        console.key(ConsoleKey::Char(*c), 480, 1.0, &none);
    }
    console.key(ConsoleKey::Back, 480, 1.0, &none);
    let lit = quakespasm(&mut console, 1.0);
    // Backspace cut the line (CON-5.5); the cursor sits after it, at the position.
    assert_eq!(row_at(&lit, 224).trim_end_matches('\x0b'), "]ma");
    assert!(lit.iter().any(|p| p.item
        == HudItem::Char {
            x: 32,
            y: 224,
            ch: 11
        }));
    let dark = quakespasm(&mut console, 1.3);
    assert!(
        !dark
            .iter()
            .any(|p| matches!(p.item, HudItem::Char { ch: 11, .. }))
    );
    assert_eq!(row_at(&dark, 224), "]ma");
    assert_eq!(console.typed(), b"ma", "drawing leaves the line as it is");
}

// spec CON-5.4: the version at the bottom right, its last character at column linewidth + 1
#[test]
fn quakespasm_draws_its_version_bottom_right() {
    let mut console = wide();
    let items = quakespasm(&mut console, 1.3);
    assert_eq!(row_at(&items, 232).trim_start(), "QuakeC Studio 1");
    let last = items
        .iter()
        .filter_map(|p| match p.item {
            HudItem::Char { x, y: 232, .. } => Some(x),
            _ => None,
        })
        .max()
        .unwrap();
    assert_eq!(last, (78 + 1) * 8);
}

// spec CON-6.3: a leading `/` or `\` is dropped from the line run, not from the line echoed
#[test]
fn a_leading_slash_is_dropped() {
    for (typed, run) in [
        (&b"/help"[..], &b"help"[..]),
        (b"\\cmdlist", b"cmdlist"),
        (b"//x", b"/x"),
        (b"map e1m1", b"map e1m1"),
    ] {
        let mut console = wide();
        for c in typed {
            console.key(ConsoleKey::Char(*c), 480, 1.0, &none);
        }
        assert_eq!(
            console.key(ConsoleKey::Enter, 480, 1.0, &none),
            Typed::Run(run.to_vec())
        );
        let echoed = format!("]{}", String::from_utf8_lossy(typed));
        assert_eq!(text(&console, console.current()), echoed);
    }
}

// spec CON-4.9: in a menu the backquote does nothing; elsewhere it is the console's toggle
#[test]
fn the_backquote_is_the_menus_in_a_menu() {
    let mut console = wide();
    assert_eq!(
        console.backquote(KeyDest::Menu, Connection::Connected),
        KeyDest::Menu
    );
    assert_eq!(
        console.backquote(KeyDest::Game, Connection::Connected),
        KeyDest::Console
    );
    assert_eq!(
        console.backquote(KeyDest::Console, Connection::Connected),
        KeyDest::Game
    );
}

// spec CON-5.5: Backspace and the down arrow back to the edit line cut the line at the position
#[test]
fn backspace_and_down_cut_the_line() {
    let mut console = wide();
    for c in b"map e1m1" {
        console.key(ConsoleKey::Char(*c), 480, 1.0, &none);
    }
    for _ in 0..4 {
        console.key(ConsoleKey::Back, 480, 1.0, &none);
    }
    assert_eq!(console.typed(), b"map ");
    assert_eq!(
        console.key(ConsoleKey::Enter, 480, 1.0, &none),
        Typed::Run(b"map ".to_vec())
    );
    for c in b"god" {
        console.key(ConsoleKey::Char(*c), 480, 1.0, &none);
    }
    console.key(ConsoleKey::Up, 480, 1.0, &none);
    assert_eq!(console.typed(), b"map ");
    console.key(ConsoleKey::Down, 480, 1.0, &none);
    assert_eq!(
        console.typed(),
        b"",
        "back on the edit line, its prompt alone"
    );
}

// spec CON-5.6: QuakeSpasm's text size holds the lines a wide console shows; the scrollback
// never goes below 0
#[test]
fn quakespasm_keeps_more_text_and_scrolls_back_from_zero() {
    let mut small = Console::new();
    small.resize(2560);
    // 16384 / 318 = 51 lines on a 1440-high screen of 180 rows: no scrolling back at all.
    small.key(ConsoleKey::PageUp, 1440, 1.0, &none);
    small.print(b"one\ntwo\n", 1.0);
    assert_eq!(text(&small, small.current()), "two");
    small.key(ConsoleKey::Home, 1440, 1.0, &none);
    let shown = small.draw(720, 1440, 1.0, InputLine::Hidden);
    assert!(
        row_at(&shown, 720 - 16 - 8).contains("two"),
        "not scrolled past the text"
    );
    let mut large = Console::with_text_size(super::CON_TEXTSIZE_QUAKESPASM);
    large.resize(2560);
    for n in 0..300 {
        large.print(format!("line {n}\n").as_bytes(), 1.0);
    }
    large.key(ConsoleKey::PageUp, 1440, 1.0, &none);
    let back = large.draw(720, 1440, 1.0, InputLine::Hidden);
    assert_eq!(row_at(&back, 720 - 16 - 8), "line 297");
}
