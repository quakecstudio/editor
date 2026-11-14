// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2002-2009 John Fitzgibbons and others
// Copyright (C) 2007-2008 Kristian Duske
// Copyright (C) 2010-2014 QuakeSpasm developers
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/menu.c, M_ToggleMenu_f, M_Menu_Main_f, M_Main_Draw, M_Main_Key, M_Menu_SinglePlayer_f, M_SinglePlayer_Draw, M_SinglePlayer_Key, M_ScanSaves, M_Menu_Load_f, M_Menu_Save_f, M_Load_Draw, M_Save_Draw, M_Load_Key, M_Save_Key, M_Menu_Options_f, M_AdjustSliders, M_DrawSlider, M_DrawCheckbox, M_Options_Draw, M_Options_Key, M_Menu_Keys_f, M_UnbindCommand, M_Keys_Draw, M_Keys_Key, M_Menu_Help_f, M_Help_Draw, M_Help_Key, M_Menu_Quit_f, M_Quit_Draw, M_Quit_Key, M_Draw, M_Keydown, M_Print, M_PrintWhite, M_DrawTextBox
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/gl_draw.c, Draw_FadeScreen
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/gl_screen.c, SCR_ModalMessage, SCR_DrawNotifyString
// Ported from sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5: Quake/gl_screen.c, SCR_DrawNotifyString

//! The menus a local player uses (MENU): main, single player, load and save, options, help and
//! the quit prompt, drawn over a faded view and driven by keys.

use crate::console::KeyDest;
use crate::hud::{Anchor, HudItem, Placed};

/// `MAIN_ITEMS` (MENU-2.1).
const MAIN_ITEMS: usize = 5;
/// `SINGLEPLAYER_ITEMS` (MENU-3.1).
const SINGLEPLAYER_ITEMS: usize = 3;
/// `MAX_SAVEGAMES`: the load and save menus' slots (MENU-6.1).
pub const MENU_SAVEGAMES: usize = 12;
/// `SAVEGAME_COMMENT_LENGTH`: a slot's characters (MENU-6.1).
const SAVEGAME_COMMENT_LENGTH: usize = 39;
/// `OPTIONS_ITEMS` of a build other than Windows' (MENU-7.1).
const OPTIONS_ITEMS: usize = 13;
/// The options' video item, which a host with no video menu skips (MENU-7.1).
const OPTIONS_VIDEO: usize = 12;
/// `SLIDER_RANGE` (MENU-7.3).
const SLIDER_RANGE: i32 = 10;
/// `%79s`: the most `fscanf` reads of a save's comment (MENU-6.1).
const SAVEGAME_SCAN_LENGTH: usize = 79;
/// A menu character's width and height in pixels.
const CHAR_SIZE: i32 = 8;
/// The menu's width, which its layout is in.
const MENU_WIDTH: i32 = 320;
/// The top of every menu's title picture.
const TITLE_Y: i32 = 4;
/// The plaque's left edge (MENU-2.1, MENU-7.3).
const PLAQUE_X: i32 = 16;
/// The main and single player menus' items picture's left edge (MENU-2.1, MENU-3.1).
const ITEMS_X: i32 = 72;
/// The first item's row: the items picture's top and the lists' first line (MENU-2.1,
/// MENU-6.1, MENU-7.3).
const MENU_TOP: i32 = 32;
/// The spinning cursor: frames a second, its left edge and its rows' spacing (MENU-2.1).
const DOT_RATE: f64 = 10.0;
const DOT_X: i32 = 54;
const DOT_SPACING: i32 = 20;
/// The line cursor's blinks a second, twice over, and its first character: 12 and 13
/// (MENU-6.1, MENU-7.3, MENU-8.1).
const BLINK_RATE: f64 = 4.0;
const LINE_CURSOR: u8 = 12;
/// The load and save menus' slot names' left edge and the line cursor's (MENU-6.1).
const SLOTS_X: i32 = 16;
const SLOTS_CURSOR_X: i32 = 8;
/// The options' labels' left edge, the line cursor's, and the sliders' and checkboxes'
/// (MENU-7.3).
const OPTIONS_LABEL_X: i32 = 16;
const OPTIONS_CURSOR_X: i32 = 200;
const OPTIONS_VALUE_X: i32 = 220;
/// The slider's characters: its left end, its track, its right end and its knob (MENU-7.3).
const SLIDER_LEFT: u8 = 128;
const SLIDER_TRACK: u8 = 129;
const SLIDER_RIGHT: u8 = 130;
const SLIDER_KNOB: u8 = 131;
/// The options' rows: screen size, brightness, mouse speed, the two volumes, always run,
/// invert mouse, lookspring and lookstrafe (MENU-7.3, MENU-7.4).
const ROW_SCREEN_SIZE: usize = 3;
const ROW_BRIGHTNESS: usize = 4;
const ROW_MOUSE_SPEED: usize = 5;
const ROW_MUSIC_VOLUME: usize = 6;
const ROW_SOUND_VOLUME: usize = 7;
const ROW_ALWAYS_RUN: usize = 8;
const ROW_INVERT_MOUSE: usize = 9;
const ROW_LOOKSPRING: usize = 10;
const ROW_LOOKSTRAFE: usize = 11;
/// `scr_viewsize`'s step and range (MENU-7.4).
const VIEWSIZE_STEP: f64 = 10.0;
const VIEWSIZE_MIN: f32 = 30.0;
const VIEWSIZE_MAX: f32 = 120.0;
/// `v_gamma`'s step and least (MENU-7.4).
const GAMMA_STEP: f64 = 0.05;
const GAMMA_MIN: f32 = 0.5;
/// `sensitivity`'s step and range (MENU-7.4).
const SENSITIVITY_STEP: f64 = 0.5;
const SENSITIVITY_MIN: f32 = 1.0;
const SENSITIVITY_MAX: f32 = 11.0;
/// The volumes' step (MENU-7.4).
const VOLUME_STEP: f64 = 0.1;
/// `cl_forwardspeed` walking and running: always run is a speed above walking (MENU-7.4).
const WALK_SPEED: f32 = 200.0;
const RUN_SPEED: f32 = 400.0;
/// `M_Print`'s red: each character `+128`.
const RED: u8 = 128;
/// `SCR_DrawNotifyString`: its top, `200 * 0.35`, and its lines' most characters
/// (MENU-3.2).
const NOTIFY_Y: i32 = 70;
const NOTIFY_LINE: usize = 40;
/// `random & 7`: the quit prompt's messages (MENU-5.1).
const QUIT_MESSAGE_MASK: i32 = 7;
/// The quit prompt's box: its left edge, top, width and lines, and its text's left edge
/// (MENU-5.1).
const QUIT_BOX_X: i32 = 56;
const QUIT_BOX_Y: i32 = 76;
const QUIT_BOX_WIDTH: i32 = 24;
const QUIT_BOX_LINES: i32 = 4;
const QUIT_TEXT_X: i32 = 64;
/// The keys menu: the prompt's left edge while grabbing and else, the first binding's row, the
/// labels' and bindings' left edges, and the cursor's (MENU-8.1).
const KEYS_GRAB_X: i32 = 12;
const KEYS_HINT_X: i32 = 18;
const KEYS_TOP: i32 = 48;
const KEYS_LABEL_X: i32 = 16;
const KEYS_BOUND_X: i32 = 140;
const KEYS_CURSOR_X: i32 = 130;
/// The columns from a first binding's end to the second: a space, `or` and a space.
const KEYS_OR_COLUMNS: i32 = 4;

/// `M_Print`'s red characters or `M_PrintWhite`'s white.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ink {
    /// `M_Print`.
    Red,
    /// `M_PrintWhite`.
    White,
}
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
    /// `SCR_ModalMessage`'s question before a new game while a game runs (MENU-3.2).
    NewGame,
    /// `m_load`.
    Load,
    /// `m_save`.
    Save,
    /// `m_options`.
    Options,
    /// `m_keys`.
    Keys,
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
    /// Backspace or Delete, which clear a binding in the keys menu (MENU-8.2).
    Delete,
    /// Any other key, by its code.
    Char(u8),
}

/// `bindnames`: the commands the keys menu binds and their labels, in its order (MENU-8.1).
pub const MENU_BINDINGS: [(&str, &str); 18] = [
    ("+attack", "attack"),
    ("impulse 10", "change weapon"),
    ("+jump", "jump / swim up"),
    ("+forward", "walk forward"),
    ("+back", "backpedal"),
    ("+left", "turn left"),
    ("+right", "turn right"),
    ("+speed", "run"),
    ("+moveleft", "step left"),
    ("+moveright", "step right"),
    ("+strafe", "sidestep"),
    ("+lookup", "look up"),
    ("+lookdown", "look down"),
    ("centerview", "center view"),
    ("+mlook", "mouse look"),
    ("+klook", "keyboard look"),
    ("+moveup", "swim up"),
    ("+movedown", "swim down"),
];

/// What the menu asks of the host.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuEvent {
    /// `S_LocalSound` of the sound named (MENU-1.3).
    Sound(&'static str),
    /// `Cbuf_AddText` of a command line, run by the console (MENU-3.2, MENU-5.2, MENU-6.3,
    /// MENU-7.2).
    Run(String),
    /// `M_UnbindCommand`: every key whose binding starts with the command made unbound
    /// (MENU-8.2).
    Unbind(&'static str),
    /// `Cvar_SetValue` of a setting (MENU-7.4).
    Set {
        /// The setting's name.
        name: &'static str,
        /// Its new value.
        value: f32,
    },
}

/// The settings the options menu shows and changes, as the host holds them (MENU-7).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MenuSettings {
    /// `viewsize`.
    pub viewsize: f32,
    /// `gamma`.
    pub gamma: f32,
    /// `sensitivity`.
    pub sensitivity: f32,
    /// `bgmvolume`.
    pub bgmvolume: f32,
    /// `volume`.
    pub volume: f32,
    /// `cl_forwardspeed`, which Always Run reads.
    pub forwardspeed: f32,
    /// `m_pitch`, negative when the mouse is inverted.
    pub m_pitch: f32,
    /// `lookspring`.
    pub lookspring: f32,
    /// `lookstrafe`.
    pub lookstrafe: f32,
}

impl MenuSettings {
    /// The settings, each read by its name from `setting`.
    #[must_use]
    pub fn read(setting: &dyn Fn(&str) -> f32) -> Self {
        Self {
            viewsize: setting("viewsize"),
            gamma: setting("gamma"),
            sensitivity: setting("sensitivity"),
            bgmvolume: setting("bgmvolume"),
            volume: setting("volume"),
            forwardspeed: setting("cl_forwardspeed"),
            m_pitch: setting("m_pitch"),
            lookspring: setting("lookspring"),
            lookstrafe: setting("lookstrafe"),
        }
    }
}

/// The key the keys menu waits for, as `M_Keys_Key` takes it with `bind_grab` set (MENU-8.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grabbed<'a> {
    /// Escape, which binds nothing.
    Escape,
    /// The backquote, which binds nothing.
    Backquote,
    /// Any other key, by its `Key_KeynumToString` name.
    Key(&'a str),
}

/// What the menu reads of the game as a key goes in (MENU-3.2, MENU-6.1, MENU-7.4).
pub struct MenuGame<'a> {
    /// The next `rand ()`, for a quit prompt's message.
    pub random: i32,
    /// `sv.active`: a game runs.
    pub server_active: bool,
    /// What `M_Menu_Save_f` asks: a game runs, it is not at an intermission, and it has one
    /// player.
    pub can_save: bool,
    /// The settings now.
    pub settings: MenuSettings,
    /// `M_ScanSaves`' files: the text of `s<n>.sav` for each slot, `None` where there is
    /// none.
    pub saves: &'a dyn Fn() -> [Option<Vec<u8>>; MENU_SAVEGAMES],
    /// How many keys, none to two, `M_FindKeysForCommand` finds bound to a command (MENU-8.2).
    pub keys_bound: &'a dyn Fn(&str) -> usize,
}

/// What a menu's drawing reads: the key destination, the clocks and the screen (MENU-1.3).
#[derive(Clone, Debug, PartialEq)]
pub struct MenuFrame {
    /// `key_dest`.
    pub dest: KeyDest,
    /// `host_time`, which turns the spinning cursor.
    pub host_time: f64,
    /// `realtime`, which blinks the line cursors.
    pub realtime: f64,
    /// Whether the console is down, its background under the menu instead of the fade.
    pub console_down: bool,
    /// `con_forcedup`, which makes that background opaque in QuakeSpasm's look (REND-13.8).
    pub console_forced: bool,
    /// The screen's height.
    pub screen_height: i32,
    /// The settings the options menu shows.
    pub settings: MenuSettings,
    /// `M_FindKeysForCommand`'s answer for each of [`MENU_BINDINGS`], the keys' names, for the
    /// keys menu (MENU-8.1); empty draws every command as unbound.
    pub bound: Vec<[Option<String>; 2]>,
}

/// `M_ScanSaves`' reading of a save's text: `fscanf (f, "%i\n", &version)`, then
/// `fscanf (f, "%79s\n", name)`, the first 39 characters kept and each `_` made a space
/// (MENU-6.1).
#[must_use]
pub fn save_comment(text: &[u8]) -> Vec<u8> {
    let rest = text.trim_ascii_start();
    let digits = rest
        .iter()
        .enumerate()
        .take_while(|&(i, &c)| c.is_ascii_digit() || (i == 0 && (c == b'-' || c == b'+')))
        .count();
    let rest = rest.get(digits..).unwrap_or_default().trim_ascii_start();
    rest.iter()
        .take_while(|c| !c.is_ascii_whitespace())
        .take(SAVEGAME_SCAN_LENGTH)
        .take(SAVEGAME_COMMENT_LENGTH)
        .map(|&c| if c == b'_' { b' ' } else { c })
        .collect()
}

/// The menu's state: what is shown, each menu's cursor and the quit prompt's (MENU).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Menu {
    state: MenuState,
    main_cursor: usize,
    singleplayer_cursor: usize,
    /// `load_cursor`, the load and save menus' slot.
    load_cursor: usize,
    /// `m_filenames` and `loadable`: each slot's comment, `None` for an unused slot.
    slots: Vec<Option<Vec<u8>>>,
    options_cursor: usize,
    /// `keys_cursor` and `bind_grab`: the keys menu's command and whether it waits for a key.
    keys_cursor: usize,
    grab: bool,
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

    /// `M_Menu_Main_f`, the `menu_main` command (CON-6.1): the main menu.
    pub fn main(&mut self) -> KeyDest {
        self.open(MenuState::Main)
    }

    /// `M_Menu_SinglePlayer_f`, the `menu_singleplayer` command (CON-6.1).
    pub fn single_player(&mut self) -> KeyDest {
        self.open(MenuState::SinglePlayer)
    }

    /// `M_Menu_Help_f`, the `help` command (CON-6.1): help on its first page (MENU-4.1).
    pub fn help(&mut self) -> KeyDest {
        self.help_page = 0;
        self.open(MenuState::Help)
    }

    /// The menu `state` shown, as `M_Menu_Main_f` and its kin show theirs (MENU-1.1).
    fn open(&mut self, state: MenuState) -> KeyDest {
        self.state = state;
        self.enter_sound = true;
        KeyDest::Menu
    }

    /// `M_Menu_Quit_f`: the quit prompt over `dest`, its message `random & 7` (MENU-5.1).
    pub fn quit(&mut self, dest: KeyDest, random: i32) -> KeyDest {
        // "`if (m_state == m_quit) return;`": the key destination stays as it was.
        if self.state == MenuState::Quit {
            return dest;
        }
        self.quit_from = (dest == KeyDest::Menu).then_some(self.state);
        self.message = usize::try_from(random & QUIT_MESSAGE_MASK).unwrap_or(0);
        self.open(MenuState::Quit)
    }

    /// `M_Menu_Load_f`: the load menu, its slots scanned (MENU-6.1).
    pub fn load(&mut self, game: &MenuGame<'_>) -> KeyDest {
        self.scan(game);
        self.open(MenuState::Load)
    }

    /// `M_Menu_Save_f`: the save menu, its slots scanned, only when `game.can_save`; else
    /// nothing changes (MENU-6.1).
    pub fn save(&mut self, dest: KeyDest, game: &MenuGame<'_>) -> KeyDest {
        if !game.can_save {
            return dest;
        }
        self.scan(game);
        self.open(MenuState::Save)
    }

    /// `M_Menu_Options_f`: the options (MENU-7.1).
    pub fn options(&mut self) -> KeyDest {
        self.open(MenuState::Options)
    }

    /// `M_ScanSaves` (MENU-6.1).
    fn scan(&mut self, game: &MenuGame<'_>) {
        self.slots = (game.saves)()
            .into_iter()
            .map(|text| text.map(|text| save_comment(&text)))
            .collect();
    }

    /// `M_Keydown`: `key` in the menu shown, with what it reads of `game`; the destination
    /// after it and what it asks of the host (MENU-1.2 to MENU-7.4).
    pub fn key(&mut self, key: MenuKey, game: &MenuGame<'_>) -> (KeyDest, Vec<MenuEvent>) {
        let mut events = Vec::new();
        let dest = match self.state {
            MenuState::None => KeyDest::Game,
            MenuState::Main => self.main_key(key, game, &mut events),
            MenuState::SinglePlayer => self.singleplayer_key(key, game, &mut events),
            MenuState::NewGame => self.new_game_key(key, &mut events),
            MenuState::Load | MenuState::Save => self.slot_key(key, &mut events),
            MenuState::Options => self.options_key(key, &game.settings, &mut events),
            MenuState::Keys => self.keys_key(key, game, &mut events),
            MenuState::Help => self.help_key(key),
            MenuState::Quit => self.quit_key(key, &mut events),
        };
        (dest, events)
    }

    /// `M_Main_Key` (MENU-2.2).
    fn main_key(
        &mut self,
        key: MenuKey,
        game: &MenuGame<'_>,
        events: &mut Vec<MenuEvent>,
    ) -> KeyDest {
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
                    2 => self.options(),
                    3 => self.help(),
                    4 => self.quit(KeyDest::Menu, game.random),
                    // Multiplayer is not this menu's (MENU-2.3).
                    _ => KeyDest::Menu,
                }
            }
            MenuKey::Left | MenuKey::Right | MenuKey::Delete | MenuKey::Char(_) => KeyDest::Menu,
        }
    }

    /// `M_SinglePlayer_Key` (MENU-3.2).
    fn singleplayer_key(
        &mut self,
        key: MenuKey,
        game: &MenuGame<'_>,
        events: &mut Vec<MenuEvent>,
    ) -> KeyDest {
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
                match self.singleplayer_cursor {
                    0 if game.server_active => {
                        self.state = MenuState::NewGame;
                        KeyDest::Menu
                    }
                    0 => self.state.new_game(events),
                    1 => self.load(game),
                    _ => self.save(KeyDest::Menu, game),
                }
            }
            MenuKey::Left | MenuKey::Right | MenuKey::Delete | MenuKey::Char(_) => KeyDest::Menu,
        }
    }

    /// `SCR_ModalMessage`'s answer before a new game: `y` starts it, `n` or Escape go back to
    /// the single player menu, any other key waits (MENU-3.2).
    fn new_game_key(&mut self, key: MenuKey, events: &mut Vec<MenuEvent>) -> KeyDest {
        match key {
            MenuKey::Char(b'y' | b'Y') => self.state.new_game(events),
            MenuKey::Escape | MenuKey::Char(b'n' | b'N') => {
                self.state = MenuState::SinglePlayer;
                KeyDest::Menu
            }
            MenuKey::Up
            | MenuKey::Down
            | MenuKey::Left
            | MenuKey::Right
            | MenuKey::Enter
            | MenuKey::Delete
            | MenuKey::Char(_) => KeyDest::Menu,
        }
    }

    /// `M_Load_Key` and `M_Save_Key` (MENU-6.2, MENU-6.3).
    fn slot_key(&mut self, key: MenuKey, events: &mut Vec<MenuEvent>) -> KeyDest {
        match key {
            MenuKey::Escape => self.open(MenuState::SinglePlayer),
            MenuKey::Up | MenuKey::Left => {
                events.push(MenuEvent::Sound("misc/menu1.wav"));
                self.load_cursor = previous(self.load_cursor, MENU_SAVEGAMES);
                KeyDest::Menu
            }
            MenuKey::Down | MenuKey::Right => {
                events.push(MenuEvent::Sound("misc/menu1.wav"));
                self.load_cursor = next(self.load_cursor, MENU_SAVEGAMES);
                KeyDest::Menu
            }
            MenuKey::Enter if self.state == MenuState::Load => {
                events.push(MenuEvent::Sound("misc/menu2.wav"));
                if !matches!(self.slots.get(self.load_cursor), Some(Some(_))) {
                    return KeyDest::Menu;
                }
                self.state = MenuState::None;
                events.push(MenuEvent::Run(format!("load s{}", self.load_cursor)));
                KeyDest::Game
            }
            MenuKey::Enter => {
                self.state = MenuState::None;
                events.push(MenuEvent::Run(format!("save s{}", self.load_cursor)));
                KeyDest::Game
            }
            MenuKey::Delete | MenuKey::Char(_) => KeyDest::Menu,
        }
    }

    /// `M_Options_Key` (MENU-7.2): Escape to the main menu; Enter on an item that is not a
    /// setting, else the setting up; the arrows move or change it; the video item skipped.
    fn options_key(
        &mut self,
        key: MenuKey,
        settings: &MenuSettings,
        events: &mut Vec<MenuEvent>,
    ) -> KeyDest {
        let dest = match key {
            MenuKey::Escape => return self.open(MenuState::Main),
            MenuKey::Enter => {
                self.enter_sound = true;
                return match self.options_cursor {
                    0 => self.open(MenuState::Keys),
                    1 => {
                        self.state = MenuState::None;
                        KeyDest::Console
                    }
                    2 => {
                        events.push(MenuEvent::Run("exec default.cfg".to_owned()));
                        KeyDest::Menu
                    }
                    _ => {
                        adjust(self.options_cursor, 1, settings, events);
                        KeyDest::Menu
                    }
                };
            }
            MenuKey::Up => {
                events.push(MenuEvent::Sound("misc/menu1.wav"));
                self.options_cursor = previous(self.options_cursor, OPTIONS_ITEMS);
                KeyDest::Menu
            }
            MenuKey::Down => {
                events.push(MenuEvent::Sound("misc/menu1.wav"));
                self.options_cursor = next(self.options_cursor, OPTIONS_ITEMS);
                KeyDest::Menu
            }
            MenuKey::Left => {
                adjust(self.options_cursor, -1, settings, events);
                KeyDest::Menu
            }
            MenuKey::Right => {
                adjust(self.options_cursor, 1, settings, events);
                KeyDest::Menu
            }
            MenuKey::Delete | MenuKey::Char(_) => KeyDest::Menu,
        };
        // "if (options_cursor == 12 && vid_menudrawfn == NULL)": this host has no video menu.
        if self.options_cursor == OPTIONS_VIDEO {
            self.options_cursor = if key == MenuKey::Up {
                OPTIONS_VIDEO.saturating_sub(1)
            } else {
                0
            };
        }
        dest
    }

    /// Whether the keys menu waits for a key to bind, `bind_grab` (MENU-8.3): the host then
    /// sends the next key going down to [`Menu::grab`].
    #[must_use]
    pub const fn grabbing(&self) -> bool {
        self.grab
    }

    /// `M_Keys_Key` with `bind_grab` set (MENU-8.3): `misc/menu1.wav`; Escape and the backquote
    /// bind nothing, any other key binds the command, by its name `Key_KeynumToString` gives.
    pub fn grab(&mut self, key: Grabbed<'_>) -> Vec<MenuEvent> {
        let mut events = vec![MenuEvent::Sound("misc/menu1.wav")];
        if let (Grabbed::Key(name), Some((command, _))) = (key, MENU_BINDINGS.get(self.keys_cursor))
        {
            events.push(MenuEvent::Run(format!("bind \"{name}\" \"{command}\"")));
        }
        self.grab = false;
        events
    }

    /// `M_Keys_Key` (MENU-8.2): Escape back to the options; the arrows move round the
    /// commands; Enter waits for a key, first unbinding a command two keys hold; Backspace and
    /// Delete unbind it.
    fn keys_key(
        &mut self,
        key: MenuKey,
        game: &MenuGame<'_>,
        events: &mut Vec<MenuEvent>,
    ) -> KeyDest {
        let command = MENU_BINDINGS.get(self.keys_cursor).map(|(c, _)| *c);
        match key {
            MenuKey::Escape => return self.options(),
            MenuKey::Left | MenuKey::Up => {
                events.push(MenuEvent::Sound("misc/menu1.wav"));
                self.keys_cursor = previous(self.keys_cursor, MENU_BINDINGS.len());
            }
            MenuKey::Down | MenuKey::Right => {
                events.push(MenuEvent::Sound("misc/menu1.wav"));
                self.keys_cursor = next(self.keys_cursor, MENU_BINDINGS.len());
            }
            MenuKey::Enter => {
                events.push(MenuEvent::Sound("misc/menu2.wav"));
                if let Some(command) = command
                    && (game.keys_bound)(command) == 2
                {
                    events.push(MenuEvent::Unbind(command));
                }
                self.grab = true;
            }
            MenuKey::Delete => {
                events.push(MenuEvent::Sound("misc/menu2.wav"));
                if let Some(command) = command {
                    events.push(MenuEvent::Unbind(command));
                }
            }
            MenuKey::Char(_) => {}
        }
        KeyDest::Menu
    }

    /// `M_Keys_Draw` (MENU-8.1).
    fn draw_keys(&self, out: &mut Vec<Placed>, frame: &MenuFrame) {
        pic(out, "gfx/ttl_cstm.lmp", None, TITLE_Y);
        if self.grab {
            let prompt = b"Press a key or button for this action";
            print(out, KEYS_GRAB_X, MENU_TOP, prompt, Ink::Red);
        } else {
            let hint = b"Enter to change, backspace to clear";
            print(out, KEYS_HINT_X, MENU_TOP, hint, Ink::Red);
        }
        let mut y = KEYS_TOP;
        for (i, (_, label)) in MENU_BINDINGS.iter().enumerate() {
            print(out, KEYS_LABEL_X, y, label.as_bytes(), Ink::Red);
            match frame.bound.get(i) {
                Some([Some(first), second]) => {
                    print(out, KEYS_BOUND_X, y, first.as_bytes(), Ink::Red);
                    if let Some(second) = second {
                        let x = KEYS_BOUND_X.saturating_add(
                            i32::try_from(first.len())
                                .unwrap_or(0)
                                .saturating_mul(CHAR_SIZE),
                        );
                        print(out, x.saturating_add(CHAR_SIZE), y, b"or", Ink::Red);
                        let x = x.saturating_add(KEYS_OR_COLUMNS.saturating_mul(CHAR_SIZE));
                        print(out, x, y, second.as_bytes(), Ink::Red);
                    }
                }
                _ => print(out, KEYS_BOUND_X, y, b"???", Ink::Red),
            }
            y = y.saturating_add(CHAR_SIZE);
        }
        let row = i32::try_from(self.keys_cursor)
            .unwrap_or(0)
            .saturating_mul(CHAR_SIZE)
            .saturating_add(KEYS_TOP);
        if self.grab {
            character(out, KEYS_CURSOR_X, row, b'=');
        } else {
            character(out, KEYS_CURSOR_X, row, blinking(frame.realtime));
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
            MenuKey::Enter | MenuKey::Delete | MenuKey::Char(_) => KeyDest::Menu,
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
                // `Host_Quit_f`, which the console's `quit` runs.
                events.push(MenuEvent::Run("quit".to_owned()));
                KeyDest::Console
            }
            MenuKey::Up
            | MenuKey::Down
            | MenuKey::Left
            | MenuKey::Right
            | MenuKey::Enter
            | MenuKey::Delete
            | MenuKey::Char(_) => KeyDest::Menu,
        }
    }

    /// `M_Draw` for `frame`: over the console's background when the console is down, else
    /// the faded view; and `misc/menu2.wav` when a menu was entered since the last draw
    /// (MENU-1.3).
    pub fn draw(&mut self, frame: &MenuFrame) -> (Vec<Placed>, Option<&'static str>) {
        if self.state == MenuState::None || frame.dest != KeyDest::Menu {
            return (Vec::new(), None);
        }
        let mut out = Vec::new();
        out.push(Placed {
            anchor: Anchor::Screen,
            item: if frame.console_down && self.state != MenuState::NewGame {
                HudItem::ConsoleBack {
                    lines: frame.screen_height,
                    height: frame.screen_height,
                    forced: frame.console_forced,
                }
            } else {
                HudItem::Fade
            },
        });
        self.draw_state(self.state, frame, &mut out);
        let sound = core::mem::take(&mut self.enter_sound).then_some("misc/menu2.wav");
        (out, sound)
    }

    /// The menu `state` drawn into `out`.
    fn draw_state(&self, state: MenuState, frame: &MenuFrame, out: &mut Vec<Placed>) {
        match state {
            MenuState::None => {}
            MenuState::Main => {
                plaque(out, "gfx/ttl_main.lmp", "gfx/mainmenu.lmp");
                cursor(out, self.main_cursor, frame.host_time);
            }
            MenuState::SinglePlayer => {
                plaque(out, "gfx/ttl_sgl.lmp", "gfx/sp_menu.lmp");
                cursor(out, self.singleplayer_cursor, frame.host_time);
            }
            MenuState::NewGame => notify_string(out, NEW_GAME_QUESTION),
            MenuState::Load | MenuState::Save => {
                let title = if state == MenuState::Load {
                    "gfx/p_load.lmp"
                } else {
                    "gfx/p_save.lmp"
                };
                pic(out, title, None, TITLE_Y);
                let mut y = MENU_TOP;
                for n in 0..MENU_SAVEGAMES {
                    let name = match self.slots.get(n) {
                        Some(Some(name)) => name.as_slice(),
                        _ => b"--- UNUSED SLOT ---",
                    };
                    print(out, SLOTS_X, y, name, Ink::Red);
                    y = y.saturating_add(CHAR_SIZE);
                }
                line_cursor(out, SLOTS_CURSOR_X, self.load_cursor, frame.realtime);
            }
            MenuState::Options => draw_options(out, &frame.settings, self.options_cursor, frame),
            MenuState::Keys => self.draw_keys(out, frame),
            MenuState::Help => {
                if let Some(page) = HELP.get(self.help_page) {
                    pic(out, page, Some(0), 0);
                }
            }
            MenuState::Quit => {
                // MENU-5.1: drawn over the menu it came from, as `m_recursiveDraw` does.
                if let Some(from) = self.quit_from {
                    self.draw_state(from, frame, out);
                }
                text_box(out, QUIT_BOX_X, QUIT_BOX_Y, QUIT_BOX_WIDTH, QUIT_BOX_LINES);
                let lines = QUIT_MESSAGES.get(self.message).or(QUIT_MESSAGES.first());
                let mut y = QUIT_BOX_Y;
                for line in lines.into_iter().flatten() {
                    y = y.saturating_add(CHAR_SIZE);
                    print(out, QUIT_TEXT_X, y, line.as_bytes(), Ink::Red);
                }
            }
        }
    }
}

impl MenuState {
    /// A new game (MENU-3.2): the menus closed and `map start` run.
    fn new_game(&mut self, events: &mut Vec<MenuEvent>) -> KeyDest {
        *self = Self::None;
        events.push(MenuEvent::Run("map start".to_owned()));
        KeyDest::Game
    }
}

/// C's `value += dir * step` of a `float` setting and a `double` step, kept within `low` and
/// `high` as `M_AdjustSliders` keeps it (MENU-7.4).
fn step(value: f32, dir: i32, step: f64, low: f32, high: f32) -> f32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "spec MENU-7.4: the double sum stored back in the float setting"
    )]
    let moved = (f64::from(value) + f64::from(dir) * step) as f32;
    moved.clamp(low, high)
}

/// `!value` of a setting read as a float: 1 for 0, else 0.
fn not(value: f32) -> f32 {
    if value == 0.0 { 1.0 } else { 0.0 }
}

/// `M_AdjustSliders (dir)` at the options' item `cursor` (MENU-7.4): `misc/menu3.wav`, then
/// the item's setting moved or turned over.
fn adjust(cursor: usize, dir: i32, settings: &MenuSettings, events: &mut Vec<MenuEvent>) {
    events.push(MenuEvent::Sound("misc/menu3.wav"));
    let mut set = |name: &'static str, value: f32| events.push(MenuEvent::Set { name, value });
    match cursor {
        ROW_SCREEN_SIZE => set(
            "viewsize",
            step(
                settings.viewsize,
                dir,
                VIEWSIZE_STEP,
                VIEWSIZE_MIN,
                VIEWSIZE_MAX,
            ),
        ),
        ROW_BRIGHTNESS => set(
            "gamma",
            step(
                settings.gamma,
                dir.saturating_neg(),
                GAMMA_STEP,
                GAMMA_MIN,
                1.0,
            ),
        ),
        ROW_MOUSE_SPEED => set(
            "sensitivity",
            step(
                settings.sensitivity,
                dir,
                SENSITIVITY_STEP,
                SENSITIVITY_MIN,
                SENSITIVITY_MAX,
            ),
        ),
        ROW_MUSIC_VOLUME => set(
            "bgmvolume",
            step(settings.bgmvolume, dir, VOLUME_STEP, 0.0, 1.0),
        ),
        ROW_SOUND_VOLUME => set("volume", step(settings.volume, dir, VOLUME_STEP, 0.0, 1.0)),
        ROW_ALWAYS_RUN => {
            let speed = if settings.forwardspeed > WALK_SPEED {
                WALK_SPEED
            } else {
                RUN_SPEED
            };
            set("cl_forwardspeed", speed);
            set("cl_backspeed", speed);
        }
        ROW_INVERT_MOUSE => set("m_pitch", -settings.m_pitch),
        ROW_LOOKSPRING => set("lookspring", not(settings.lookspring)),
        ROW_LOOKSTRAFE => set("lookstrafe", not(settings.lookstrafe)),
        _ => {}
    }
}

/// The question `SCR_ModalMessage` asks before a new game while one runs (MENU-3.2).
const NEW_GAME_QUESTION: &[u8] = b"Are you sure you want to\nstart a new game?\n";

/// The options' labels, `M_Print` at x 16 from y 32, 8 apart (MENU-7.3).
const OPTIONS_LABELS: [&str; 12] = [
    "    Customize controls",
    "         Go to console",
    "     Reset to defaults",
    "           Screen size",
    "            Brightness",
    "           Mouse Speed",
    "       CD Music Volume",
    "          Sound Volume",
    "            Always Run",
    "          Invert Mouse",
    "            Lookspring",
    "            Lookstrafe",
];

/// `M_Options_Draw` (MENU-7.3): the plaque and title, the labels, the five sliders and the
/// four checkboxes at x 220, and the blinking cursor at x 200.
fn draw_options(out: &mut Vec<Placed>, settings: &MenuSettings, at: usize, frame: &MenuFrame) {
    pic(out, "gfx/qplaque.lmp", Some(PLAQUE_X), TITLE_Y);
    pic(out, "gfx/p_option.lmp", None, TITLE_Y);
    let mut y = MENU_TOP;
    for label in OPTIONS_LABELS {
        print(out, OPTIONS_LABEL_X, y, label.as_bytes(), Ink::Red);
        y = y.saturating_add(CHAR_SIZE);
    }
    let v = |value: f32| f64::from(value);
    let span = |low: f32, high: f32| v(high) - v(low);
    let sliders = [
        (
            ROW_SCREEN_SIZE,
            (v(settings.viewsize) - v(VIEWSIZE_MIN)) / span(VIEWSIZE_MIN, VIEWSIZE_MAX),
        ),
        (
            ROW_BRIGHTNESS,
            (1.0 - v(settings.gamma)) / span(GAMMA_MIN, 1.0),
        ),
        (
            ROW_MOUSE_SPEED,
            (v(settings.sensitivity) - v(SENSITIVITY_MIN)) / span(SENSITIVITY_MIN, SENSITIVITY_MAX),
        ),
        (ROW_MUSIC_VOLUME, v(settings.bgmvolume)),
        (ROW_SOUND_VOLUME, v(settings.volume)),
    ];
    for (row, range) in sliders {
        slider(out, row_y(row), range);
    }
    // `M_DrawCheckbox (220, y, on)`: `on` or `off` in red.
    let checkboxes = [
        (ROW_ALWAYS_RUN, settings.forwardspeed > WALK_SPEED),
        (ROW_INVERT_MOUSE, settings.m_pitch < 0.0),
        (ROW_LOOKSPRING, settings.lookspring != 0.0),
        (ROW_LOOKSTRAFE, settings.lookstrafe != 0.0),
    ];
    for (row, on) in checkboxes {
        let text: &[u8] = if on { b"on" } else { b"off" };
        print(out, OPTIONS_VALUE_X, row_y(row), text, Ink::Red);
    }
    line_cursor(out, OPTIONS_CURSOR_X, at, frame.realtime);
}

/// `M_DrawSlider (220, y, range)`: the range kept within 0 and 1, the ends 128 and 130 around
/// ten 129s, and 131 at `x + (SLIDER_RANGE-1)*8 * range` (MENU-7.3).
fn slider(out: &mut Vec<Placed>, y: i32, range: f64) {
    let x = OPTIONS_VALUE_X;
    let range = range.clamp(0.0, 1.0);
    character(out, x - CHAR_SIZE, y, SLIDER_LEFT);
    let mut cx = x;
    for _ in 0..SLIDER_RANGE {
        character(out, cx, y, SLIDER_TRACK);
        cx = cx.saturating_add(CHAR_SIZE);
    }
    character(out, cx, y, SLIDER_RIGHT);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "spec MENU-7.3: C's float argument to M_DrawCharacter's int, toward zero"
    )]
    let knob = (f64::from(x) + f64::from((SLIDER_RANGE - 1) * CHAR_SIZE) * range) as i32;
    character(out, knob, y, SLIDER_KNOB);
}

/// Row `row`'s top, `32 + row*8`, of the lists the load, save and options menus draw.
fn row_y(row: usize) -> i32 {
    i32::try_from(row)
        .unwrap_or(0)
        .saturating_mul(CHAR_SIZE)
        .saturating_add(MENU_TOP)
}

/// The line cursor's character at `realtime`: `12+((int)(realtime*4)&1)` (MENU-6.1,
/// MENU-7.3, MENU-8.1).
fn blinking(realtime: f64) -> u8 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "spec MENU-6.1: `(int)(realtime*4)`, toward zero"
    )]
    let blink = ((realtime * BLINK_RATE) as i64 & 1) != 0;
    LINE_CURSOR.saturating_add(u8::from(blink))
}

/// The line cursor at `x`, `32 + cursor*8`: character `12+((int)(realtime*4)&1)`, as the
/// load, save and options menus draw it (MENU-6.1, MENU-7.3).
fn line_cursor(out: &mut Vec<Placed>, x: i32, cursor: usize, realtime: f64) {
    character(out, x, row_y(cursor), blinking(realtime));
}

/// `M_DrawCharacter`: `num` at `x`, `y` of the centred menu.
fn character(out: &mut Vec<Placed>, x: i32, y: i32, num: u8) {
    out.push(Placed {
        anchor: Anchor::Centered,
        item: HudItem::Char { x, y, ch: num },
    });
}

/// `SCR_DrawNotifyString` as QuakeSpasm places it (MENU-3.2): each line of `text`, up to 40
/// characters, centred in the 320 wide menu, from y `200 * 0.35`, 8 apart.
fn notify_string(out: &mut Vec<Placed>, text: &[u8]) {
    let mut y = NOTIFY_Y;
    let mut lines = text.split(|&c| c == b'\n').peekable();
    while let Some(line) = lines.next() {
        if line.is_empty() && lines.peek().is_none() {
            break;
        }
        let line = line.get(..line.len().min(NOTIFY_LINE)).unwrap_or_default();
        let width = i32::try_from(line.len())
            .unwrap_or(0)
            .saturating_mul(CHAR_SIZE);
        // `(320 - l*8)/2` of a line of at most 40 characters, never below 0.
        print(
            out,
            MENU_WIDTH.saturating_sub(width) >> 1,
            y,
            line,
            Ink::White,
        );
        y = y.saturating_add(CHAR_SIZE);
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
    pic(out, "gfx/qplaque.lmp", Some(PLAQUE_X), TITLE_Y);
    pic(out, title, None, TITLE_Y);
    pic(out, items, Some(ITEMS_X), MENU_TOP);
}

/// The spinning cursor at item `cursor`: `menudot` `(int)(host_time * 10) % 6 + 1`, at
/// `32 + cursor * 20` (MENU-2.1).
fn cursor(out: &mut Vec<Placed>, cursor: usize, host_time: f64) {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "spec MENU-2.1: `(int)(host_time * 10)`, toward zero, of a time from 0"
    )]
    let frame = ((host_time.max(0.0) * DOT_RATE) as usize)
        .checked_rem(DOTS.len())
        .unwrap_or(0);
    let y = i32::try_from(cursor)
        .unwrap_or(0)
        .saturating_mul(DOT_SPACING)
        .saturating_add(MENU_TOP);
    if let Some(dot) = DOTS.get(frame) {
        pic(out, dot, Some(DOT_X), y);
    }
}

/// `M_Print` ([`Ink::Red`], each character `+128`) or `M_PrintWhite` of `text` at `x`, `y`.
fn print(out: &mut Vec<Placed>, x: i32, y: i32, text: &[u8], ink: Ink) {
    let mut x = x;
    for &c in text {
        out.push(Placed {
            anchor: Anchor::Centered,
            item: HudItem::Char {
                x,
                y,
                ch: match ink {
                    Ink::Red => c.wrapping_add(RED),
                    Ink::White => c,
                },
            },
        });
        x = x.saturating_add(CHAR_SIZE);
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
            cy = cy.saturating_add(CHAR_SIZE);
            pic(out, middle(n), Some(cx), cy);
        }
        pic(out, bottom, Some(cx), cy.saturating_add(CHAR_SIZE));
    };
    column(
        out,
        x,
        "gfx/box_tl.lmp",
        &|_| "gfx/box_ml.lmp",
        "gfx/box_bl.lmp",
    );
    let mut cx = x.saturating_add(CHAR_SIZE);
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
        cx = cx.saturating_add(CHAR_SIZE.saturating_mul(2));
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
