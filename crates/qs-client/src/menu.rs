// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/menu.c, M_ToggleMenu_f, M_Menu_Main_f, M_Main_Draw, M_Main_Key, M_Menu_SinglePlayer_f, M_SinglePlayer_Draw, M_SinglePlayer_Key, M_Menu_Help_f, M_Help_Draw, M_Help_Key, M_Menu_Quit_f, M_Quit_Draw, M_Quit_Key, M_Draw, M_Keydown, M_Print, M_PrintWhite, M_DrawTextBox
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/gl_draw.c, Draw_FadeScreen

//! The menus a local player uses (MENU): main, single player, help and the quit prompt, drawn
//! over a faded view and driven by keys.

use crate::console::KeyDest;
use crate::hud::{Anchor, HudItem, Placed};

/// `MAIN_ITEMS` (MENU-2.1).
const MAIN_ITEMS: usize = 5;
/// `SINGLEPLAYER_ITEMS` (MENU-3.1).
const SINGLEPLAYER_ITEMS: usize = 3;
/// `NUM_HELP_PAGES` (MENU-4.1).
pub const MENU_HELP_PAGES: usize = 6;
/// The cursor's six frames, `gfx/menudot1.lmp` to `gfx/menudot6.lmp` (MENU-2.1).
const DOTS: [&str; 6] = [
    "gfx/menudot1.lmp",
    "gfx/menudot2.lmp",
    "gfx/menudot3.lmp",
    "gfx/menudot4.lmp",
    "gfx/menudot5.lmp",
    "gfx/menudot6.lmp",
];
/// The help pages, `gfx/help0.lmp` to `gfx/help5.lmp` (MENU-4.1).
const HELP: [&str; MENU_HELP_PAGES] = [
    "gfx/help0.lmp",
    "gfx/help1.lmp",
    "gfx/help2.lmp",
    "gfx/help3.lmp",
    "gfx/help4.lmp",
    "gfx/help5.lmp",
];

/// `quitMessage`: eight messages of four lines (MENU-5.1).
const QUIT_MESSAGES: [[&str; 4]; 8] = [
    [
        "  Are you gonna quit    ",
        "  this game just like   ",
        "   everything else?     ",
        "                        ",
    ],
    [
        " Milord, methinks that  ",
        "   thou art a lowly     ",
        " quitter. Is this true? ",
        "                        ",
    ],
    [
        " Do I need to bust your ",
        "  face open for trying  ",
        "        to quit?        ",
        "                        ",
    ],
    [
        " Man, I oughta smack you",
        "   for trying to quit!  ",
        "     Press Y to get     ",
        "      smacked out.      ",
    ],
    [
        " Press Y to quit like a ",
        "   big loser in life.   ",
        "  Press N to stay proud ",
        "    and successful!     ",
    ],
    [
        "   If you press Y to    ",
        "  quit, I will summon   ",
        "  Satan all over your   ",
        "      hard drive!       ",
    ],
    [
        "  Um, Asmodeus dislikes ",
        " his children trying to ",
        " quit. Press Y to return",
        "   to your Tinkertoys.  ",
    ],
    [
        "  If you quit now, I'll ",
        "  throw a blanket-party ",
        "   for you next time!   ",
        "                        ",
    ],
];

/// `m_state`: the menu shown (MENU-1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MenuState {
    /// `m_none`.
    #[default]
    None,
    /// `m_main`.
    Main,
    /// `m_singleplayer`.
    SinglePlayer,
    /// `m_help`.
    Help,
    /// `m_quit`.
    Quit,
}

/// A key the menu reads (MENU-1.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuKey {
    /// Escape.
    Escape,
    /// The up arrow.
    Up,
    /// The down arrow.
    Down,
    /// The left arrow.
    Left,
    /// The right arrow.
    Right,
    /// Enter.
    Enter,
    /// Any other key, by its code.
    Char(u8),
}

/// What the menu asks of the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuEvent {
    /// `S_LocalSound` of the sound named (MENU-1.3).
    Sound(&'static str),
    /// A new game: `maxplayers 1` and `map start` (MENU-3.2).
    NewGame,
    /// `Host_Quit_f` (MENU-5.2).
    Quit,
}

/// The menu's state: what is shown, each menu's cursor and the quit prompt's (MENU).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Menu {
    state: MenuState,
    main_cursor: usize,
    singleplayer_cursor: usize,
    help_page: usize,
    /// `msgNumber`, the quit message shown.
    message: usize,
    /// `wasInMenus` and `m_quit_prevstate`: the menu the quit prompt came from.
    quit_from: Option<MenuState>,
    /// `m_entersound`: `misc/menu2.wav` after the next draw (MENU-1.3).
    enter_sound: bool,
}

impl Menu {
    /// No menu.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `m_state`.
    #[must_use]
    pub const fn state(&self) -> MenuState {
        self.state
    }

    /// `M_ToggleMenu_f` from `dest` (MENU-1.1); the destination after it. From the console it
    /// is the console's own toggle, which the caller makes.
    pub fn toggle(&mut self, dest: KeyDest) -> KeyDest {
        self.enter_sound = true;
        match dest {
            KeyDest::Menu if self.state != MenuState::Main => self.open(MenuState::Main),
            KeyDest::Menu => {
                self.state = MenuState::None;
                KeyDest::Game
            }
            KeyDest::Console => KeyDest::Console,
            KeyDest::Game => self.open(MenuState::Main),
        }
    }

    /// The menu `state` shown, as `M_Menu_Main_f` and its kin show theirs (MENU-1.1).
    fn open(&mut self, state: MenuState) -> KeyDest {
        self.state = state;
        self.enter_sound = true;
        KeyDest::Menu
    }

    /// `M_Menu_Quit_f`: the quit prompt over `dest`, its message `random & 7` (MENU-5.1).
    pub fn quit(&mut self, dest: KeyDest, random: i32) -> KeyDest {
        if self.state == MenuState::Quit {
            return KeyDest::Menu;
        }
        self.quit_from = (dest == KeyDest::Menu).then_some(self.state);
        self.message = usize::try_from(random & 7).unwrap_or(0);
        self.open(MenuState::Quit)
    }

    /// `M_Keydown`: `key` in the menu shown, `random` the next `rand ()` for a quit prompt;
    /// the destination after it and what it asks of the host (MENU-1.2 to MENU-5.2).
    pub fn key(&mut self, key: MenuKey, random: i32) -> (KeyDest, Vec<MenuEvent>) {
        let mut events = Vec::new();
        let dest = match self.state {
            MenuState::None => KeyDest::Game,
            MenuState::Main => self.main_key(key, random, &mut events),
            MenuState::SinglePlayer => self.singleplayer_key(key, &mut events),
            MenuState::Help => self.help_key(key),
            MenuState::Quit => self.quit_key(key, &mut events),
        };
        (dest, events)
    }

    /// `M_Main_Key` (MENU-2.2).
    fn main_key(&mut self, key: MenuKey, random: i32, events: &mut Vec<MenuEvent>) -> KeyDest {
        match key {
            MenuKey::Escape => {
                self.state = MenuState::None;
                KeyDest::Game
            }
            MenuKey::Down => {
                events.push(MenuEvent::Sound("misc/menu1.wav"));
                self.main_cursor = next(self.main_cursor, MAIN_ITEMS);
                KeyDest::Menu
            }
            MenuKey::Up => {
                events.push(MenuEvent::Sound("misc/menu1.wav"));
                self.main_cursor = previous(self.main_cursor, MAIN_ITEMS);
                KeyDest::Menu
            }
            MenuKey::Enter => {
                self.enter_sound = true;
                match self.main_cursor {
                    0 => self.open(MenuState::SinglePlayer),
                    3 => {
                        self.help_page = 0;
                        self.open(MenuState::Help)
                    }
                    4 => self.quit(KeyDest::Menu, random),
                    // Multiplayer and the options are not this menu's (MENU-2.3).
                    _ => KeyDest::Menu,
                }
            }
            MenuKey::Left | MenuKey::Right | MenuKey::Char(_) => KeyDest::Menu,
        }
    }

    /// `M_SinglePlayer_Key` (MENU-3.2).
    fn singleplayer_key(&mut self, key: MenuKey, events: &mut Vec<MenuEvent>) -> KeyDest {
        match key {
            MenuKey::Escape => self.open(MenuState::Main),
            MenuKey::Down => {
                events.push(MenuEvent::Sound("misc/menu1.wav"));
                self.singleplayer_cursor = next(self.singleplayer_cursor, SINGLEPLAYER_ITEMS);
                KeyDest::Menu
            }
            MenuKey::Up => {
                events.push(MenuEvent::Sound("misc/menu1.wav"));
                self.singleplayer_cursor = previous(self.singleplayer_cursor, SINGLEPLAYER_ITEMS);
                KeyDest::Menu
            }
            MenuKey::Enter => {
                self.enter_sound = true;
                if self.singleplayer_cursor == 0 {
                    self.state = MenuState::None;
                    events.push(MenuEvent::NewGame);
                    KeyDest::Game
                } else {
                    // Load and save are not this menu's yet (MENU-3.3).
                    KeyDest::Menu
                }
            }
            MenuKey::Left | MenuKey::Right | MenuKey::Char(_) => KeyDest::Menu,
        }
    }

    /// `M_Help_Key` (MENU-4.2).
    fn help_key(&mut self, key: MenuKey) -> KeyDest {
        match key {
            MenuKey::Escape => self.open(MenuState::Main),
            MenuKey::Up | MenuKey::Right => {
                self.enter_sound = true;
                self.help_page = next(self.help_page, MENU_HELP_PAGES);
                KeyDest::Menu
            }
            MenuKey::Down | MenuKey::Left => {
                self.enter_sound = true;
                self.help_page = previous(self.help_page, MENU_HELP_PAGES);
                KeyDest::Menu
            }
            MenuKey::Enter | MenuKey::Char(_) => KeyDest::Menu,
        }
    }

    /// `M_Quit_Key` (MENU-5.2).
    fn quit_key(&mut self, key: MenuKey, events: &mut Vec<MenuEvent>) -> KeyDest {
        match key {
            MenuKey::Escape | MenuKey::Char(b'n' | b'N') => match self.quit_from.take() {
                Some(state) => {
                    self.state = state;
                    self.enter_sound = true;
                    KeyDest::Menu
                }
                None => {
                    self.state = MenuState::None;
                    KeyDest::Game
                }
            },
            MenuKey::Char(b'y' | b'Y') => {
                events.push(MenuEvent::Quit);
                KeyDest::Console
            }
            MenuKey::Up
            | MenuKey::Down
            | MenuKey::Left
            | MenuKey::Right
            | MenuKey::Enter
            | MenuKey::Char(_) => KeyDest::Menu,
        }
    }

    /// `M_Draw` with `dest` the key destination at `host_time`, over the console's background
    /// when the console is down (`console_down`), else the faded view; and `misc/menu2.wav`
    /// when a menu was entered since the last draw (MENU-1.3).
    pub fn draw(
        &mut self,
        dest: KeyDest,
        host_time: f64,
        console_down: bool,
        screen_height: i32,
    ) -> (Vec<Placed>, Option<&'static str>) {
        if self.state == MenuState::None || dest != KeyDest::Menu {
            return (Vec::new(), None);
        }
        let mut out = Vec::new();
        out.push(Placed {
            anchor: Anchor::Screen,
            item: if console_down {
                HudItem::ConsoleBack {
                    lines: screen_height,
                    height: screen_height,
                }
            } else {
                HudItem::Fade
            },
        });
        self.draw_state(self.state, host_time, &mut out);
        let sound = core::mem::take(&mut self.enter_sound).then_some("misc/menu2.wav");
        (out, sound)
    }

    /// The menu `state` drawn into `out`.
    fn draw_state(&self, state: MenuState, host_time: f64, out: &mut Vec<Placed>) {
        match state {
            MenuState::None => {}
            MenuState::Main => {
                plaque(out, "gfx/ttl_main.lmp", "gfx/mainmenu.lmp");
                cursor(out, self.main_cursor, host_time);
            }
            MenuState::SinglePlayer => {
                plaque(out, "gfx/ttl_sgl.lmp", "gfx/sp_menu.lmp");
                cursor(out, self.singleplayer_cursor, host_time);
            }
            MenuState::Help => {
                if let Some(page) = HELP.get(self.help_page) {
                    pic(out, page, Some(0), 0);
                }
            }
            MenuState::Quit => {
                // MENU-5.1: drawn over the menu it came from, as `m_recursiveDraw` does.
                if let Some(from) = self.quit_from {
                    self.draw_state(from, host_time, out);
                }
                text_box(out, 56, 76, 24, 4);
                let lines = QUIT_MESSAGES.get(self.message).or(QUIT_MESSAGES.first());
                for (line, y) in lines.into_iter().flatten().zip([84, 92, 100, 108]) {
                    print(out, 64, y, line.as_bytes(), true);
                }
            }
        }
    }
}

/// `++cursor >= items` back to 0.
fn next(cursor: usize, items: usize) -> usize {
    let next = cursor.saturating_add(1);
    if next >= items { 0 } else { next }
}

/// `--cursor < 0` round to the last.
fn previous(cursor: usize, items: usize) -> usize {
    cursor.checked_sub(1).unwrap_or(items.saturating_sub(1))
}

/// `M_DrawPic` or `M_DrawTransPic` of the `.lmp` at `path`, `x` from the 320 wide menu's left,
/// centred when `None`.
fn pic(out: &mut Vec<Placed>, path: &str, x: Option<i32>, y: i32) {
    out.push(Placed {
        anchor: Anchor::Centered,
        item: HudItem::Lmp {
            path: path.to_owned(),
            x,
            y,
        },
    });
}

/// The plaque, the title centred and the menu's items, as the main and single player menus
/// draw them (MENU-2.1, MENU-3.1).
fn plaque(out: &mut Vec<Placed>, title: &str, items: &str) {
    pic(out, "gfx/qplaque.lmp", Some(16), 4);
    pic(out, title, None, 4);
    pic(out, items, Some(72), 32);
}

/// The spinning cursor at item `cursor`: `menudot` `(int)(host_time * 10) % 6 + 1`, at
/// `32 + cursor * 20` (MENU-2.1).
fn cursor(out: &mut Vec<Placed>, cursor: usize, host_time: f64) {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "spec MENU-2.1: `(int)(host_time * 10)`, toward zero, of a time from 0"
    )]
    let frame = ((host_time.max(0.0) * 10.0) as usize)
        .checked_rem(DOTS.len())
        .unwrap_or(0);
    let y = i32::try_from(cursor)
        .unwrap_or(0)
        .saturating_mul(20)
        .saturating_add(32);
    if let Some(dot) = DOTS.get(frame) {
        pic(out, dot, Some(54), y);
    }
}

/// `M_Print` (`red`, each character `+128`) or `M_PrintWhite` of `text` at `x`, `y`.
fn print(out: &mut Vec<Placed>, x: i32, y: i32, text: &[u8], red: bool) {
    let mut x = x;
    for &c in text {
        out.push(Placed {
            anchor: Anchor::Centered,
            item: HudItem::Char {
                x,
                y,
                ch: if red { c.wrapping_add(128) } else { c },
            },
        });
        x = x.saturating_add(8);
    }
}

/// `M_DrawTextBox`: the box of `gfx/box_*.lmp` around `width` by `lines` characters at `x`,
/// `y` (MENU-5.1).
fn text_box(out: &mut Vec<Placed>, x: i32, y: i32, width: i32, lines: i32) {
    let column = |out: &mut Vec<Placed>,
                  cx: i32,
                  top: &str,
                  middle: &dyn Fn(i32) -> &'static str,
                  bottom: &str| {
        pic(out, top, Some(cx), y);
        let mut cy = y;
        for n in 0..lines {
            cy = cy.saturating_add(8);
            pic(out, middle(n), Some(cx), cy);
        }
        pic(out, bottom, Some(cx), cy.saturating_add(8));
    };
    column(
        out,
        x,
        "gfx/box_tl.lmp",
        &|_| "gfx/box_ml.lmp",
        "gfx/box_bl.lmp",
    );
    let mut cx = x.saturating_add(8);
    let mut left = width;
    while left > 0 {
        // "if (n == 1) p = Draw_CachePic ("gfx/box_mm2.lmp");", kept for the rest.
        column(
            out,
            cx,
            "gfx/box_tm.lmp",
            &|n| {
                if n >= 1 {
                    "gfx/box_mm2.lmp"
                } else {
                    "gfx/box_mm.lmp"
                }
            },
            "gfx/box_bm.lmp",
        );
        left = left.saturating_sub(2);
        cx = cx.saturating_add(16);
    }
    column(
        out,
        cx,
        "gfx/box_tr.lmp",
        &|_| "gfx/box_mr.lmp",
        "gfx/box_br.lmp",
    );
}

#[cfg(test)]
mod tests;
