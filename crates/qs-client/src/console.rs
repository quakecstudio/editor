// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2002-2009 John Fitzgibbons and others
// Copyright (C) 2010-2014 QuakeSpasm developers
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/console.c, Con_CheckResize, Con_Linefeed, Con_Print, Con_Clear_f, Con_ClearNotify, Con_ToggleConsole_f, Con_DrawNotify, Con_DrawConsole, Con_DrawInput
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/keys.c, Key_Init, Key_Console
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/gl_screen.c, SCR_SetUpToDrawConsole, SCR_DrawConsole
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: QW/client/keys.c, Key_Console
// Ported from sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5: Quake/console.c, Con_DrawConsole, Con_DrawInput

//! The client's console (CON): the text printed into it, the notify lines, the console drawn
//! over the view and the line typed into it.

use crate::hud::{Anchor, HudItem, Placed};

/// `CON_TEXTSIZE`: the bytes of the console's text (CON-1.1).
pub const CON_TEXTSIZE: usize = 16384;
/// QuakeSpasm's `CON_TEXTSIZE`, `(1024 * 1024)` (CON-5.6).
pub const CON_TEXTSIZE_QUAKESPASM: usize = 1024 * 1024;
/// `NUM_CON_TIMES`: the lines a notify time is kept for (CON-2.1).
pub const CON_NUM_TIMES: usize = 4;
/// `con_notifytime`'s default: how long a notify line shows, in seconds (CON-2.2).
pub const CON_NOTIFY_TIME: f64 = 3.0;
/// `con_cursorspeed`: the cursor's blinks a second, twice over (CON-3.4).
pub const CON_CURSOR_SPEED: f64 = 4.0;
/// `scr_conspeed`'s default: how fast the console slides, in pixels a second (CON-3.1).
pub const CON_SPEED: f32 = 300.0;
/// `MAXCMDLINE`: the bytes of a typed line, its prompt included (CON-4.1).
pub const CON_MAX_LINE: usize = 256;
/// `key_lines[32]`: the lines of history (CON-4.1).
pub const CON_HISTORY: usize = 32;

/// The width a console line has before the video is up (CON-1.1).
const UNSET_WIDTH: usize = 38;
/// The prompt each typed line starts with (CON-4.1).
const PROMPT: u8 = b']';

/// Where keys go: `key_dest` (CON-4.8).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KeyDest {
    /// The game: keys are bindings.
    #[default]
    Game,
    /// The console's input line.
    Console,
    /// The menu.
    Menu,
}

/// A key the console's input line reads (CON-4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsoleKey {
    /// Enter: run the line (CON-4.2).
    Enter,
    /// Tab: complete the command or variable (CON-4.3).
    Tab,
    /// Backspace or the left arrow (CON-4.4).
    Back,
    /// The up arrow: an earlier line (CON-4.5).
    Up,
    /// The down arrow: a later line (CON-4.5).
    Down,
    /// Page up or the wheel up (CON-4.6).
    PageUp,
    /// Page down or the wheel down (CON-4.6).
    PageDown,
    /// Home (CON-4.6).
    Home,
    /// End (CON-4.6).
    End,
    /// Any other key, by its code (CON-4.7).
    Char(u8),
}

/// What the console did with a key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Typed {
    /// Nothing for the host to do.
    Nothing,
    /// A line to run as a console command, without its prompt (CON-4.2).
    Run(Vec<u8>),
}

/// The console: its text, notify times, slide and input line (CON-1 to CON-4).
#[derive(Clone, Debug, PartialEq)]
pub struct Console {
    text: Vec<u8>,
    linewidth: usize,
    totallines: usize,
    /// `con_current`, the line the next text is printed on.
    current: i64,
    /// `con_x`, the column it is printed at.
    x: usize,
    /// `cr`: the last line ended with `\r` (CON-1.4).
    cr: bool,
    backscroll: i64,
    times: [f64; CON_NUM_TIMES],
    /// `scr_con_current`, the drawn height in pixels (CON-3.1).
    height: f32,
    /// `con_forcedup`, as the last slide had it (CON-3.1).
    forced: bool,
    lines: Vec<[u8; CON_MAX_LINE]>,
    edit: usize,
    history: usize,
    pos: usize,
}

impl Default for Console {
    fn default() -> Self {
        Self::new()
    }
}

impl Console {
    /// The console before the video is up: lines of 38 characters, all spaces, and every input
    /// line just its prompt (CON-1.1, CON-4.1).
    #[must_use]
    pub fn new() -> Self {
        Self::with_text_size(CON_TEXTSIZE)
    }

    /// The console with `size` bytes of text: [`CON_TEXTSIZE`], or QuakeSpasm's
    /// [`CON_TEXTSIZE_QUAKESPASM`] (CON-5.6).
    #[must_use]
    pub fn with_text_size(size: usize) -> Self {
        let mut line = [0; CON_MAX_LINE];
        set(&mut line, 0, PROMPT);
        let size = size.max(UNSET_WIDTH);
        let totallines = size.checked_div(UNSET_WIDTH).unwrap_or(1);
        Self {
            text: vec![b' '; size],
            linewidth: UNSET_WIDTH,
            totallines,
            current: i64_of(totallines).saturating_sub(1),
            x: 0,
            cr: false,
            backscroll: 0,
            times: [0.0; CON_NUM_TIMES],
            height: 0.0,
            forced: false,
            lines: vec![line; CON_HISTORY],
            edit: 0,
            history: 0,
            pos: 1,
        }
    }

    /// `con_linewidth` (CON-1.1).
    #[must_use]
    pub const fn line_width(&self) -> usize {
        self.linewidth
    }

    /// `Con_CheckResize` for a screen `width` pixels wide (CON-1.1).
    pub fn resize(&mut self, width: i32) {
        let width = usize::try_from((width >> 3).saturating_sub(2)).unwrap_or(0);
        if width == self.linewidth {
            return;
        }
        if width < 1 {
            self.linewidth = UNSET_WIDTH;
            self.totallines = self.text.len().checked_div(UNSET_WIDTH).unwrap_or(1);
            self.text.fill(b' ');
        } else {
            let (old_width, old_total) = (self.linewidth, self.totallines);
            self.linewidth = width;
            self.totallines = self.text.len().checked_div(width).unwrap_or(1);
            let lines = old_total.min(self.totallines);
            let chars = old_width.min(width);
            let size = self.text.len();
            let old = core::mem::replace(&mut self.text, vec![b' '; size]);
            for i in 0..lines {
                let from = self
                    .current
                    .saturating_sub(i64_of(i))
                    .rem_euclid(i64_of(old_total));
                let from = usize::try_from(from).unwrap_or(0).saturating_mul(old_width);
                let to = self
                    .totallines
                    .saturating_sub(1)
                    .saturating_sub(i)
                    .saturating_mul(width);
                for j in 0..chars {
                    if let (Some(&c), Some(slot)) = (
                        old.get(from.saturating_add(j)),
                        self.text.get_mut(to.saturating_add(j)),
                    ) {
                        *slot = c;
                    }
                }
            }
            self.times = [0.0; CON_NUM_TIMES];
        }
        self.backscroll = 0;
        self.current = i64_of(self.totallines).saturating_sub(1);
    }

    /// `Con_Print`: `text` printed at `realtime`; whether it asks for `misc/talk.wav`
    /// (CON-1.2 to CON-1.5).
    pub fn print(&mut self, text: &[u8], realtime: f64) -> bool {
        self.backscroll = 0;
        let (mask, talk, text) = match text.split_first() {
            Some((1, rest)) => (128, true, rest),
            Some((2, rest)) => (128, false, rest),
            _ => (0, false, text),
        };
        let mut rest = text;
        while let Some((&c, after)) = rest.split_first() {
            // The word starting here, up to a line long (CON-1.3).
            let word = rest
                .iter()
                .take(self.linewidth)
                .take_while(|&&b| b > b' ')
                .count();
            if word != self.linewidth && self.x.saturating_add(word) > self.linewidth {
                self.x = 0;
            }
            rest = after;
            if self.cr {
                self.current = self.current.saturating_sub(1);
                self.cr = false;
            }
            if self.x == 0 {
                self.linefeed();
                // CON-2.1: the line's time, for the notify lines.
                if let Some(time) = usize::try_from(self.current)
                    .ok()
                    .and_then(|line| self.times.get_mut(line % CON_NUM_TIMES))
                {
                    *time = realtime;
                }
            }
            match c {
                b'\n' => self.x = 0,
                b'\r' => {
                    self.x = 0;
                    self.cr = true;
                }
                _ => {
                    let at = self.row(self.current).saturating_add(self.x);
                    if let Some(slot) = self.text.get_mut(at) {
                        *slot = c | mask;
                    }
                    self.x = self.x.saturating_add(1);
                    if self.x >= self.linewidth {
                        self.x = 0;
                    }
                }
            }
        }
        talk
    }

    /// `Con_Linefeed`: the next line cleared and made the current one (CON-1.4).
    fn linefeed(&mut self) {
        self.x = 0;
        self.current = self.current.saturating_add(1);
        let start = self.row(self.current);
        let end = start.saturating_add(self.linewidth);
        if let Some(line) = self.text.get_mut(start..end) {
            line.fill(b' ');
        }
    }

    /// Where line `n` starts in the text: `(n % con_totallines) * con_linewidth`.
    fn row(&self, n: i64) -> usize {
        let line = n.rem_euclid(i64_of(self.totallines));
        usize::try_from(line)
            .unwrap_or(0)
            .saturating_mul(self.linewidth)
    }

    /// Line `n`'s characters.
    #[must_use]
    pub fn line(&self, n: i64) -> &[u8] {
        let start = self.row(n);
        self.text
            .get(start..start.saturating_add(self.linewidth))
            .unwrap_or_default()
    }

    /// `con_current`.
    #[must_use]
    pub const fn current(&self) -> i64 {
        self.current
    }

    /// `Con_Clear_f` (CON-1.5).
    pub fn clear(&mut self) {
        self.text.fill(b' ');
    }

    /// `Con_ToggleConsole_f` from `dest`, the client `connected` or not: the destination
    /// after it (CON-4.8).
    pub fn toggle(&mut self, dest: KeyDest, connected: bool) -> KeyDest {
        let next = match dest {
            KeyDest::Console if connected => {
                self.with_edit(|line| set(line, 1, 0));
                self.pos = 1;
                KeyDest::Game
            }
            KeyDest::Console => KeyDest::Menu,
            KeyDest::Game | KeyDest::Menu => KeyDest::Console,
        };
        self.times = [0.0; CON_NUM_TIMES];
        next
    }

    /// The backquote from `dest` (CON-4.8, CON-4.9): the console's toggle, but nothing in a
    /// menu, whose keys are its own.
    pub fn backquote(&mut self, dest: KeyDest, connected: bool) -> KeyDest {
        match dest {
            KeyDest::Menu => KeyDest::Menu,
            KeyDest::Game | KeyDest::Console => self.toggle(dest, connected),
        }
    }

    /// `SCR_SetUpToDrawConsole`'s height: the console forced up, else half the screen with the
    /// console the destination, else none, the drawn height moving toward it by
    /// [`CON_SPEED`] a second over `frametime` (CON-3.1); the height to draw.
    pub fn slide(
        &mut self,
        screen_height: i32,
        forced: bool,
        dest: KeyDest,
        frametime: f32,
    ) -> i32 {
        let screen = f32_of(screen_height);
        self.forced = forced;
        if forced {
            self.height = screen;
            return screen_height;
        }
        let target = match dest {
            KeyDest::Console => f32_of(screen_height >> 1),
            KeyDest::Game | KeyDest::Menu => 0.0,
        };
        let step = CON_SPEED * frametime;
        if target < self.height {
            self.height = (self.height - step).max(target);
        } else if target > self.height {
            self.height = (self.height + step).min(target);
        }
        int_of(self.height)
    }

    /// `Con_DrawNotify`: the notify lines at `realtime` (CON-2.2).
    #[must_use]
    pub fn notify(&self, realtime: f64) -> Vec<Placed> {
        let mut out = Vec::new();
        let mut v = 0;
        let first = self
            .current
            .saturating_sub(i64_of(CON_NUM_TIMES))
            .saturating_add(1);
        for i in first..=self.current {
            let Ok(index) = usize::try_from(i) else {
                continue;
            };
            let time = self
                .times
                .get(index % CON_NUM_TIMES)
                .copied()
                .unwrap_or(0.0);
            // `if (time == 0) continue;`: a line never timed.
            let unset = time <= 0.0;
            if unset || realtime - time > CON_NOTIFY_TIME {
                continue;
            }
            chars(&mut out, self.line(i), 8, v);
            v = v.saturating_add(8);
        }
        out
    }

    /// `Con_DrawConsole`: the console `lines` pixels high on a screen `screen_height` high at
    /// `realtime`, the input line with it when `input`, which then ends the line at its
    /// position (CON-3.2 to CON-3.4).
    pub fn draw(
        &mut self,
        lines: i32,
        screen_height: i32,
        realtime: f64,
        input: bool,
    ) -> Vec<Placed> {
        if lines <= 0 {
            return Vec::new();
        }
        let mut out = vec![Placed {
            anchor: Anchor::Screen,
            item: HudItem::ConsoleBack {
                lines,
                height: screen_height,
                forced: self.forced,
            },
        }];
        let rows = lines.saturating_sub(16) >> 3;
        let mut y = lines.saturating_sub(16).saturating_sub(rows << 3);
        let first = self
            .current
            .saturating_sub(i64::from(rows))
            .saturating_add(1);
        for i in first..=self.current {
            let j = i.saturating_sub(self.backscroll).max(0);
            chars(&mut out, self.line(j), 8, y);
            y = y.saturating_add(8);
        }
        if input {
            self.draw_input(&mut out, lines, realtime);
        }
        out
    }

    /// QuakeSpasm's `Con_DrawConsole` and `Con_DrawInput`: the console `lines` pixels high on a
    /// screen `height` high, laid out at full height and moved up by what is not shown
    /// (`CANVAS_CONSOLE`): `(lines + 7) / 8 - 2` rows of text ending 16 above the bottom, a row
    /// of `^` under them while scrolled back, the input line 16 above the bottom with a block
    /// cursor on even beats of `realtime * 4`, and `version` at the bottom right (CON-5).
    pub fn draw_quakespasm(
        &mut self,
        lines: i32,
        height: i32,
        realtime: f64,
        input: bool,
        version: &[u8],
    ) -> Vec<Placed> {
        if lines <= 0 {
            return Vec::new();
        }
        let shift = lines.saturating_sub(height);
        let mut out = vec![Placed {
            anchor: Anchor::Screen,
            item: HudItem::ConsoleBack {
                lines,
                height,
                forced: self.forced,
            },
        }];
        let all = lines.saturating_add(7) >> 3;
        let mut y = height.saturating_sub(all.saturating_mul(8));
        let rows = all.saturating_sub(2);
        let back = if self.backscroll > 0 { 2 } else { 0 };
        let first = self
            .current
            .saturating_sub(i64::from(rows))
            .saturating_add(1);
        for i in first..=self.current.saturating_sub(back) {
            let j = i.saturating_sub(self.backscroll).max(0);
            chars(&mut out, self.line(j), 8, y.saturating_add(shift));
            y = y.saturating_add(8);
        }
        if self.backscroll > 0 {
            y = y.saturating_add(8);
            let arrows: Vec<u8> = (0..self.linewidth)
                .map(|x| if x % 4 == 0 { b'^' } else { b' ' })
                .collect();
            chars(&mut out, &arrows, 8, y.saturating_add(shift));
        }
        // CON-3.4: the input line only while the keys are the console's.
        if input {
            let input_y = height.saturating_sub(16).saturating_add(shift);
            let start = if self.pos >= self.linewidth {
                self.pos.saturating_add(1).saturating_sub(self.linewidth)
            } else {
                0
            };
            let line = *self.edit_line();
            let typed: Vec<u8> = line
                .iter()
                .skip(start)
                .take_while(|&&c| c != 0)
                .take(self.linewidth)
                .copied()
                .collect();
            chars(&mut out, &typed, 8, input_y);
            #[expect(
                clippy::cast_possible_truncation,
                reason = "spec CON-5.3: `(int)(realtime*con_cursorspeed)`, toward zero"
            )]
            let beat = ((realtime * CON_CURSOR_SPEED) as i64) & 1;
            if beat == 0 {
                let column =
                    i32::try_from(self.pos.saturating_sub(start).saturating_add(1)).unwrap_or(0);
                out.push(Placed {
                    anchor: Anchor::Screen,
                    item: HudItem::Char {
                        x: column.saturating_mul(8),
                        y: input_y,
                        ch: 11,
                    },
                });
            }
        }
        let width = i32::try_from(self.linewidth).unwrap_or(0);
        let length = i32::try_from(version.len()).unwrap_or(0);
        let x = width
            .saturating_sub(length)
            .saturating_add(2)
            .saturating_mul(8);
        let mut at = x;
        for &c in version {
            out.push(Placed {
                anchor: Anchor::Screen,
                item: HudItem::Char {
                    x: at,
                    y: height.saturating_sub(8).saturating_add(shift),
                    ch: c,
                },
            });
            at = at.saturating_add(8);
        }
        out
    }

    /// `Con_DrawInput`: the edit line 16 above the console's bottom with its cursor, then
    /// "`key_lines[edit_line][key_linepos] = 0;`", which ends the line there (CON-3.4).
    fn draw_input(&mut self, out: &mut Vec<Placed>, lines: i32, realtime: f64) {
        let mut text = *self.edit_line();
        #[expect(
            clippy::cast_possible_truncation,
            reason = "spec CON-3.4: `(int)(realtime*con_cursorspeed)`, toward zero"
        )]
        let blink = ((realtime * CON_CURSOR_SPEED) as i64) & 1;
        if let Some(cursor) = text.get_mut(self.pos) {
            *cursor = if blink == 0 { 10 } else { 11 };
        }
        for c in text
            .iter_mut()
            .skip(self.pos.saturating_add(1))
            .take(self.linewidth.saturating_sub(self.pos.saturating_add(1)))
        {
            *c = b' ';
        }
        let start = if self.pos >= self.linewidth {
            self.pos.saturating_add(1).saturating_sub(self.linewidth)
        } else {
            0
        };
        let shown = text
            .get(start..start.saturating_add(self.linewidth))
            .unwrap_or_default();
        chars(out, shown, 8, lines.saturating_sub(16));
        let pos = self.pos;
        self.with_edit(|line| set(line, pos, 0));
    }

    /// The edit line.
    fn edit_line(&self) -> &[u8; CON_MAX_LINE] {
        self.lines.get(self.edit).unwrap_or(&[0; CON_MAX_LINE])
    }

    /// Runs `f` on the edit line.
    fn with_edit(&mut self, f: impl FnOnce(&mut [u8; CON_MAX_LINE])) {
        if let Some(line) = self.lines.get_mut(self.edit) {
            f(line);
        }
    }

    /// The typed text, without its prompt, up to its end.
    #[must_use]
    pub fn typed(&self) -> &[u8] {
        let line = self.edit_line();
        let len = line.iter().skip(1).take_while(|&&b| b != 0).count();
        line.get(1..len.saturating_add(1)).unwrap_or_default()
    }

    /// `Key_Console`: `key` typed into the line on a screen `screen_height` high, `complete`
    /// finding the command or variable a prefix names; what to run (CON-4.2 to CON-4.7).
    pub fn key(
        &mut self,
        key: ConsoleKey,
        screen_height: i32,
        realtime: f64,
        complete: &dyn Fn(&[u8]) -> Option<Vec<u8>>,
    ) -> Typed {
        match key {
            ConsoleKey::Enter => {
                let run = self.typed().to_vec();
                let mut echo = vec![PROMPT];
                echo.extend_from_slice(&run);
                echo.push(b'\n');
                self.print(&echo, realtime);
                self.edit = self.edit.saturating_add(1) % CON_HISTORY;
                self.history = self.edit;
                self.with_edit(|line| {
                    set(line, 0, PROMPT);
                    set(line, 1, 0);
                });
                self.pos = 1;
                // CON-6.3: QuakeWorld's leading `\` or `/`, dropped.
                let run = match run.split_first() {
                    Some((b'/' | b'\\', rest)) => rest.to_vec(),
                    _ => run,
                };
                return Typed::Run(run);
            }
            ConsoleKey::Tab => {
                if let Some(name) = complete(self.typed()) {
                    let end = name.len().min(CON_MAX_LINE.saturating_sub(3));
                    let name = name.get(..end).unwrap_or_default();
                    let pos = name.len().saturating_add(1);
                    self.with_edit(|line| {
                        if let Some(slot) = line.get_mut(1..pos) {
                            slot.copy_from_slice(name);
                        }
                        set(line, pos, b' ');
                        set(line, pos.saturating_add(1), 0);
                    });
                    self.pos = pos.saturating_add(1);
                }
            }
            ConsoleKey::Back => {
                if self.pos > 1 {
                    self.pos = self.pos.saturating_sub(1);
                }
                // CON-5.5: the line ends at the position, as id's next draw ends it.
                let pos = self.pos;
                self.with_edit(|line| set(line, pos, 0));
            }
            ConsoleKey::Up => {
                loop {
                    self.history = self.history.wrapping_add(CON_HISTORY - 1) % CON_HISTORY;
                    if self.history == self.edit || self.has_text(self.history) {
                        break;
                    }
                }
                if self.history == self.edit {
                    self.history = self.edit.saturating_add(1) % CON_HISTORY;
                }
                self.recall();
            }
            ConsoleKey::Down => {
                if self.history == self.edit {
                    return Typed::Nothing;
                }
                loop {
                    self.history = self.history.saturating_add(1) % CON_HISTORY;
                    if self.history == self.edit || self.has_text(self.history) {
                        break;
                    }
                }
                if self.history == self.edit {
                    // CON-5.5: the prompt alone, as id's next draw leaves it.
                    self.with_edit(|line| {
                        set(line, 0, PROMPT);
                        set(line, 1, 0);
                    });
                    self.pos = 1;
                } else {
                    self.recall();
                }
            }
            ConsoleKey::PageUp => {
                self.backscroll = self
                    .backscroll
                    .saturating_add(2)
                    .min(self.most_back(screen_height));
            }
            ConsoleKey::PageDown => self.backscroll = self.backscroll.saturating_sub(2).max(0),
            ConsoleKey::Home => self.backscroll = self.most_back(screen_height),
            ConsoleKey::End => self.backscroll = 0,
            ConsoleKey::Char(c) => {
                if (32..=127).contains(&c) && self.pos < CON_MAX_LINE.saturating_sub(1) {
                    let pos = self.pos;
                    self.with_edit(|line| {
                        set(line, pos, c);
                        set(line, pos.saturating_add(1), 0);
                    });
                    self.pos = pos.saturating_add(1);
                }
            }
        }
        Typed::Nothing
    }

    /// Whether history line `n` has anything after its prompt (CON-4.5).
    fn has_text(&self, n: usize) -> bool {
        self.lines
            .get(n)
            .and_then(|line| line.get(1))
            .is_some_and(|&b| b != 0)
    }

    /// The history line copied into the edit line, the position at its end (CON-4.5).
    fn recall(&mut self) {
        let from = self
            .lines
            .get(self.history)
            .copied()
            .unwrap_or([0; CON_MAX_LINE]);
        self.with_edit(|line| *line = from);
        self.pos = from.iter().position(|&b| b == 0).unwrap_or(CON_MAX_LINE);
    }

    /// `con_totallines - (vid.height >> 3) - 1`, the farthest back the console scrolls
    /// (CON-4.6).
    fn most_back(&self, screen_height: i32) -> i64 {
        // CON-5.6: never below 0, which a console narrower than its screen is tall would go.
        i64_of(self.totallines)
            .saturating_sub(i64::from(screen_height >> 3))
            .saturating_sub(1)
            .max(0)
    }
}

/// `line[at] = byte`, when `at` is within the line.
fn set(line: &mut [u8; CON_MAX_LINE], at: usize, byte: u8) {
    if let Some(slot) = line.get_mut(at) {
        *slot = byte;
    }
}

/// `line`'s characters drawn from x 8, each 8 to the right, at `y` (CON-2.2, CON-3.3).
fn chars(out: &mut Vec<Placed>, line: &[u8], x0: i32, y: i32) {
    let mut x = x0;
    for &ch in line {
        out.push(Placed {
            anchor: Anchor::Screen,
            item: HudItem::Char { x, y, ch },
        });
        x = x.saturating_add(8);
    }
}

/// A count as a line number.
fn i64_of(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// A pixel count as a float, exactly for any screen.
fn f32_of(n: i32) -> f32 {
    f32::from(i16::try_from(n).unwrap_or(i16::MAX))
}

/// `scr_con_current` as the int `Con_DrawConsole` takes, toward zero.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec CON-3.1: the float height passed as C's int, toward zero"
)]
fn int_of(height: f32) -> i32 {
    height as i32
}

#[cfg(test)]
mod tests;
