// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::indexing_slicing,
    reason = "test code: a failed index is a failed test"
)]

use super::{
    MENU_HELP_PAGES, MENU_SAVEGAMES, Menu, MenuEvent, MenuFrame, MenuGame, MenuKey, MenuSettings,
    MenuState, save_comment,
};
use crate::console::KeyDest;
use crate::hud::{Anchor, HudItem, Placed};

/// The `.lmp` pictures `items` draw, with where.
fn pictures(items: &[Placed]) -> Vec<(String, Option<i32>, i32)> {
    items
        .iter()
        .filter_map(|placed| match &placed.item {
            HudItem::Lmp { path, x, y } => Some((path.clone(), *x, *y)),
            _ => None,
        })
        .collect()
}

/// id's defaults (CON-7.1).
const DEFAULTS: MenuSettings = MenuSettings {
    viewsize: 100.0,
    gamma: 1.0,
    sensitivity: 3.0,
    bgmvolume: 1.0,
    volume: 0.7,
    forwardspeed: 200.0,
    m_pitch: 0.022,
    lookspring: 0.0,
    lookstrafe: 0.0,
};

/// No saved games.
fn no_saves() -> [Option<Vec<u8>>; MENU_SAVEGAMES] {
    Default::default()
}

/// A game running and savable, the defaults, no saves, and `random` for a quit prompt.
fn game(random: i32) -> MenuGame<'static> {
    MenuGame {
        random,
        server_active: true,
        can_save: true,
        settings: DEFAULTS,
        saves: &no_saves,
    }
}

/// A frame at `time` with `dest`, the console down or not, on a 480-high screen.
fn frame(dest: KeyDest, time: f64, console_down: bool) -> MenuFrame {
    MenuFrame {
        dest,
        host_time: time,
        realtime: time,
        console_down,
        console_forced: false,
        screen_height: 480,
        settings: DEFAULTS,
    }
}

/// A menu with the main menu open.
fn main_menu() -> Menu {
    let mut menu = Menu::new();
    assert_eq!(menu.toggle(KeyDest::Game), KeyDest::Menu);
    menu
}

// spec MENU-1.1: Escape in the game opens the main menu; togglemenu in another menu opens the
// main one and in the main one closes it; from the console it is the console's toggle
#[test]
fn the_menu_toggles() {
    let mut menu = main_menu();
    assert_eq!(menu.state(), MenuState::Main);
    menu.key(MenuKey::Enter, &game(0));
    assert_eq!(menu.state(), MenuState::SinglePlayer);
    assert_eq!(menu.toggle(KeyDest::Menu), KeyDest::Menu);
    assert_eq!(menu.state(), MenuState::Main);
    assert_eq!(menu.toggle(KeyDest::Menu), KeyDest::Game);
    assert_eq!(menu.state(), MenuState::None);
    assert_eq!(menu.toggle(KeyDest::Console), KeyDest::Console);
}

// spec MENU-1.2: with no menu shown a key goes back to the game
#[test]
fn keys_go_to_the_menu_shown() {
    let mut menu = Menu::new();
    assert_eq!(
        menu.key(MenuKey::Down, &game(0)),
        (KeyDest::Game, Vec::new())
    );
}

// spec MENU-1.3: drawn only while shown and the keys are the menu's: the fade, or the console's
// background with the console down, then the menu in the centred area; entering plays
// misc/menu2.wav once, moving misc/menu1.wav
#[test]
fn the_menu_draws_over_a_fade_with_its_sounds() {
    let mut menu = main_menu();
    let (items, sound) = menu.draw(&frame(KeyDest::Menu, 0.0, false));
    assert_eq!(items[0].item, HudItem::Fade);
    assert!(items[1..].iter().all(|p| p.anchor == Anchor::Centered));
    assert_eq!(sound, Some("misc/menu2.wav"));
    assert_eq!(menu.draw(&frame(KeyDest::Menu, 0.0, false)).1, None);
    let (down, _) = menu.draw(&frame(KeyDest::Menu, 0.0, true));
    assert_eq!(
        down[0].item,
        HudItem::ConsoleBack {
            lines: 480,
            height: 480,
            forced: false
        }
    );
    assert!(menu.draw(&frame(KeyDest::Game, 0.0, false)).0.is_empty());
    let (_, events) = menu.key(MenuKey::Down, &game(0));
    assert_eq!(events, [MenuEvent::Sound("misc/menu1.wav")]);
}

// spec MENU-1.4: M_Print draws each character +128, M_PrintWhite as it is
// spec MENU-5.1: the quit prompt's text box and its message, rand () & 7 of eight, over the
// menu it came from
#[test]
fn the_quit_prompt_prints_its_message() {
    let mut menu = main_menu();
    for _ in 0..4 {
        menu.key(MenuKey::Down, &game(0));
    }
    menu.key(MenuKey::Enter, &game(9));
    assert_eq!(menu.state(), MenuState::Quit);
    let (items, _) = menu.draw(&frame(KeyDest::Menu, 0.0, false));
    let found = pictures(&items);
    assert!(
        found.contains(&("gfx/mainmenu.lmp".to_owned(), Some(72), 32)),
        "over the main menu"
    );
    assert!(found.contains(&("gfx/box_tl.lmp".to_owned(), Some(56), 76)));
    assert!(found.contains(&("gfx/box_br.lmp".to_owned(), Some(56 + 8 + 12 * 16), 76 + 40)));
    // 9 & 7 is 1: " Milord, methinks that  ".
    let first: Vec<u8> = items
        .iter()
        .filter_map(|p| match p.item {
            HudItem::Char { y: 84, ch, .. } => Some(ch.wrapping_sub(128)),
            _ => None,
        })
        .collect();
    assert_eq!(first, b" Milord, methinks that  ");
    assert_eq!(
        menu.quit(KeyDest::Console, 3),
        KeyDest::Console,
        "asked again, nothing changes, the destination neither"
    );
}

// spec MENU-2.1: the plaque, the title centred, the items and the cursor frame by host time
#[test]
fn the_main_menu_draws_its_pictures() {
    let mut menu = main_menu();
    menu.key(MenuKey::Down, &game(0));
    let (items, _) = menu.draw(&frame(KeyDest::Menu, 0.35, false));
    assert_eq!(
        pictures(&items),
        [
            ("gfx/qplaque.lmp".to_owned(), Some(16), 4),
            ("gfx/ttl_main.lmp".to_owned(), None, 4),
            ("gfx/mainmenu.lmp".to_owned(), Some(72), 32),
            ("gfx/menudot4.lmp".to_owned(), Some(54), 52),
        ]
    );
}

// spec MENU-2.2: Escape closes the menus; the arrows wrap round the five items
// spec MENU-2.4: Enter on multiplayer stays in the main menu; on the options opens them
#[test]
fn the_main_menu_moves_and_wraps() {
    let mut menu = main_menu();
    menu.key(MenuKey::Up, &game(0));
    let (items, _) = menu.draw(&frame(KeyDest::Menu, 0.0, false));
    assert!(pictures(&items).contains(&("gfx/menudot1.lmp".to_owned(), Some(54), 32 + 4 * 20)));
    menu.key(MenuKey::Down, &game(0));
    menu.key(MenuKey::Down, &game(0));
    assert_eq!(menu.key(MenuKey::Enter, &game(0)).0, KeyDest::Menu);
    assert_eq!(
        menu.state(),
        MenuState::Main,
        "multiplayer is not this menu's"
    );
    menu.key(MenuKey::Down, &game(0));
    menu.key(MenuKey::Enter, &game(0));
    assert_eq!(menu.state(), MenuState::Options);
    menu.key(MenuKey::Escape, &game(0));
    assert_eq!(menu.key(MenuKey::Escape, &game(0)).0, KeyDest::Game);
    assert_eq!(menu.state(), MenuState::None);
}

// spec MENU-3.1: the single player menu's title and items
// spec MENU-3.6: New Game asks first while a game runs: y starts the start map, n and Escape go
// back, any other key waits; with no game it starts at once
#[test]
fn single_player_asks_before_a_new_game() {
    let mut menu = main_menu();
    menu.key(MenuKey::Enter, &game(0));
    let (items, _) = menu.draw(&frame(KeyDest::Menu, 0.0, false));
    let found = pictures(&items);
    assert!(found.contains(&("gfx/ttl_sgl.lmp".to_owned(), None, 4)));
    assert!(found.contains(&("gfx/sp_menu.lmp".to_owned(), Some(72), 32)));
    assert_eq!(
        menu.key(MenuKey::Enter, &game(0)),
        (KeyDest::Menu, Vec::new())
    );
    assert_eq!(menu.state(), MenuState::NewGame);
    let (asked, _) = menu.draw(&frame(KeyDest::Menu, 0.0, true));
    assert_eq!(
        asked[0].item,
        HudItem::Fade,
        "the fade, even with the console down"
    );
    // "Are you sure you want to" is 24 characters: (320 - 192) / 2 = 64, at y 70.
    assert!(asked.contains(&Placed {
        anchor: Anchor::Centered,
        item: HudItem::Char {
            x: 64,
            y: 70,
            ch: b'A'
        },
    }));
    // "start a new game?" is 17: (320 - 136) / 2 = 92, the next line.
    assert!(asked.contains(&Placed {
        anchor: Anchor::Centered,
        item: HudItem::Char {
            x: 92,
            y: 78,
            ch: b's'
        },
    }));
    assert!(
        !asked
            .iter()
            .any(|p| matches!(p.item, HudItem::Char { y: 86, .. }))
    );
    assert_eq!(
        menu.key(MenuKey::Enter, &game(0)),
        (KeyDest::Menu, Vec::new())
    );
    assert_eq!(menu.key(MenuKey::Char(b'n'), &game(0)).0, KeyDest::Menu);
    assert_eq!(menu.state(), MenuState::SinglePlayer);
    menu.key(MenuKey::Enter, &game(0));
    menu.key(MenuKey::Escape, &game(0));
    assert_eq!(menu.state(), MenuState::SinglePlayer);
    menu.key(MenuKey::Enter, &game(0));
    assert_eq!(
        menu.key(MenuKey::Char(b'y'), &game(0)),
        (KeyDest::Game, vec![MenuEvent::Run("map start".to_owned())])
    );
    assert_eq!(menu.state(), MenuState::None);
    let mut idle = main_menu();
    idle.key(MenuKey::Enter, &game(0));
    let none = MenuGame {
        server_active: false,
        ..game(0)
    };
    assert_eq!(
        idle.key(MenuKey::Enter, &none),
        (KeyDest::Game, vec![MenuEvent::Run("map start".to_owned())])
    );
    let mut back = main_menu();
    back.key(MenuKey::Enter, &game(0));
    back.key(MenuKey::Escape, &game(0));
    assert_eq!(back.state(), MenuState::Main);
}

// spec MENU-3.5: Load opens the load menu; Save the save menu only when the game may be saved
#[test]
fn single_player_opens_load_and_save() {
    let mut menu = main_menu();
    menu.key(MenuKey::Enter, &game(0));
    menu.key(MenuKey::Down, &game(0));
    assert_eq!(menu.key(MenuKey::Enter, &game(0)).0, KeyDest::Menu);
    assert_eq!(menu.state(), MenuState::Load);
    menu.key(MenuKey::Escape, &game(0));
    menu.key(MenuKey::Down, &game(0));
    let locked = MenuGame {
        can_save: false,
        ..game(0)
    };
    assert_eq!(menu.key(MenuKey::Enter, &locked).0, KeyDest::Menu);
    assert_eq!(
        menu.state(),
        MenuState::SinglePlayer,
        "not at an intermission"
    );
    menu.key(MenuKey::Enter, &game(0));
    assert_eq!(menu.state(), MenuState::Save);
}

// spec MENU-4.1: help opens on page 0, drawn at (0, 0)
// spec MENU-4.2: the arrows turn the pages round the six; Escape goes back
#[test]
fn help_turns_its_pages() {
    let mut menu = main_menu();
    for _ in 0..3 {
        menu.key(MenuKey::Down, &game(0));
    }
    menu.key(MenuKey::Enter, &game(0));
    assert_eq!(menu.state(), MenuState::Help);
    let page = |menu: &mut Menu| pictures(&menu.draw(&frame(KeyDest::Menu, 0.0, false)).0);
    assert_eq!(page(&mut menu), [("gfx/help0.lmp".to_owned(), Some(0), 0)]);
    menu.key(MenuKey::Left, &game(0));
    assert_eq!(
        page(&mut menu)[0].0,
        format!("gfx/help{}.lmp", MENU_HELP_PAGES - 1)
    );
    menu.key(MenuKey::Right, &game(0));
    menu.key(MenuKey::Up, &game(0));
    assert_eq!(page(&mut menu)[0].0, "gfx/help1.lmp");
    menu.key(MenuKey::Escape, &game(0));
    assert_eq!(menu.state(), MenuState::Main);
}

// spec MENU-5.2: n, N and Escape go back where the prompt came from; y quits
#[test]
fn the_quit_prompt_answers_its_keys() {
    let mut menu = main_menu();
    menu.quit(KeyDest::Menu, 0);
    assert_eq!(menu.key(MenuKey::Char(b'x'), &game(0)).0, KeyDest::Menu);
    assert_eq!(menu.key(MenuKey::Char(b'N'), &game(0)).0, KeyDest::Menu);
    assert_eq!(menu.state(), MenuState::Main);
    let mut prompt = Menu::new();
    prompt.quit(KeyDest::Game, 0);
    assert_eq!(prompt.key(MenuKey::Escape, &game(0)).0, KeyDest::Game);
    prompt.quit(KeyDest::Game, 0);
    assert_eq!(
        prompt.key(MenuKey::Char(b'y'), &game(0)),
        (KeyDest::Console, vec![MenuEvent::Run("quit".to_owned())])
    );
}

// spec CON-6.4: menu_main, menu_singleplayer, menu_options and help open their menus, help on
// page 0
#[test]
fn the_menu_commands_open_their_menus() {
    let mut menu = Menu::new();
    assert_eq!(menu.help(), KeyDest::Menu);
    assert_eq!(menu.state(), MenuState::Help);
    menu.key(MenuKey::Right, &game(0));
    menu.main();
    assert_eq!(menu.state(), MenuState::Main);
    menu.help();
    let (items, sound) = menu.draw(&frame(KeyDest::Menu, 0.0, false));
    assert_eq!(pictures(&items), [("gfx/help0.lmp".to_owned(), Some(0), 0)]);
    assert_eq!(sound, Some("misc/menu2.wav"));
    assert_eq!(menu.single_player(), KeyDest::Menu);
    assert_eq!(menu.state(), MenuState::SinglePlayer);
    assert_eq!(menu.options(), KeyDest::Menu);
    assert_eq!(menu.state(), MenuState::Options);
}

/// The characters `items` draw at row `y`, by their x, as text.
fn row(items: &[Placed], y: i32) -> Vec<(i32, u8)> {
    let mut found: Vec<(i32, u8)> = items
        .iter()
        .filter_map(|p| match p.item {
            HudItem::Char { x, y: at, ch } if at == y => Some((x, ch)),
            _ => None,
        })
        .collect();
    found.sort_unstable();
    found
}

/// The red text `items` print at row `y` from x 16 to before `end`.
fn red_text(items: &[Placed], y: i32, end: i32) -> String {
    row(items, y)
        .iter()
        .filter(|(x, _)| *x >= 16 && *x < end)
        .map(|(_, ch)| char::from(ch.wrapping_sub(128)))
        .collect()
}

/// Saves in slots 0 and 3.
fn two_saves() -> [Option<Vec<u8>>; MENU_SAVEGAMES] {
    let mut saves: [Option<Vec<u8>>; MENU_SAVEGAMES] = Default::default();
    saves[0] = Some(b"5\nthe_Slipgate_Complex___kills:__3/42\n0\n".to_vec());
    saves[3] = Some(b"5\nstart_______________________kills:__0/0\n".to_vec());
    saves
}

/// The menu at the load menu with `two_saves`.
fn load_menu() -> Menu {
    let mut menu = Menu::new();
    let saved = MenuGame {
        saves: &two_saves,
        ..game(0)
    };
    assert_eq!(menu.load(&saved), KeyDest::Menu);
    menu
}

// spec MENU-6.1: the version, then the name up to its first space, 39 characters kept, `_` made
// a space
#[test]
fn a_save_reads_as_its_comment() {
    assert_eq!(save_comment(b"5\nthe_start\n"), b"the start");
    assert_eq!(
        save_comment(b"  -5 \n\n x_y z\n"),
        b"x y",
        "fscanf skips white space"
    );
    // A `%i` that fails leaves the text for `%79s`.
    assert_eq!(save_comment(&[b'a'; 90]), vec![b'a'; 39]);
    let long = [b"5\n".as_slice(), &[b'q'; 90]].concat();
    assert_eq!(save_comment(&long), vec![b'q'; 39]);
    assert_eq!(save_comment(b""), b"");
}

// spec MENU-6.1: the slots scanned, the unused ones named so, the title and the line cursor
#[test]
fn the_load_menu_lists_its_slots() {
    let mut menu = load_menu();
    let (items, sound) = menu.draw(&frame(KeyDest::Menu, 0.0, false));
    assert_eq!(sound, Some("misc/menu2.wav"));
    assert!(pictures(&items).contains(&("gfx/p_load.lmp".to_owned(), None, 4)));
    assert_eq!(
        red_text(&items, 32, 640),
        "the Slipgate Complex   kills:  3/42"
    );
    assert_eq!(red_text(&items, 40, 640), "--- UNUSED SLOT ---");
    assert_eq!(red_text(&items, 32 + 8 * 11, 640), "--- UNUSED SLOT ---");
    assert_eq!(row(&items, 32)[0], (8, 12));
    let (later, _) = menu.draw(&frame(KeyDest::Menu, 0.25, false));
    assert_eq!(row(&later, 32)[0], (8, 13), "it blinks at realtime * 4");
}

// spec MENU-6.2: Escape back to single player; the arrows move and wrap with menu1.wav
// spec MENU-6.3: Enter loads a slot that can be loaded, with menu2.wav; an unused one stays
#[test]
fn the_load_menu_loads_a_slot() {
    let mut menu = load_menu();
    assert_eq!(
        menu.key(MenuKey::Up, &game(0)),
        (KeyDest::Menu, vec![MenuEvent::Sound("misc/menu1.wav")])
    );
    let (items, _) = menu.draw(&frame(KeyDest::Menu, 0.0, false));
    assert_eq!(row(&items, 32 + 8 * 11)[0], (8, 12), "wrapped to slot 11");
    menu.key(MenuKey::Right, &game(0));
    menu.key(MenuKey::Right, &game(0));
    assert_eq!(
        menu.key(MenuKey::Enter, &game(0)),
        (KeyDest::Menu, vec![MenuEvent::Sound("misc/menu2.wav")]),
        "slot 1 is unused"
    );
    for _ in 0..2 {
        menu.key(MenuKey::Down, &game(0));
    }
    assert_eq!(
        menu.key(MenuKey::Enter, &game(0)),
        (
            KeyDest::Game,
            vec![
                MenuEvent::Sound("misc/menu2.wav"),
                MenuEvent::Run("load s3".to_owned())
            ]
        )
    );
    assert_eq!(menu.state(), MenuState::None);
    let mut back = load_menu();
    back.key(MenuKey::Escape, &game(0));
    assert_eq!(back.state(), MenuState::SinglePlayer);
}

// spec MENU-6.3: Enter in the save menu saves to the slot, used or not
#[test]
fn the_save_menu_saves_to_a_slot() {
    let mut menu = Menu::new();
    assert_eq!(menu.save(KeyDest::Game, &game(0)), KeyDest::Menu);
    let (items, _) = menu.draw(&frame(KeyDest::Menu, 0.0, false));
    assert!(pictures(&items).contains(&("gfx/p_save.lmp".to_owned(), None, 4)));
    menu.key(MenuKey::Left, &game(0));
    assert_eq!(
        menu.key(MenuKey::Enter, &game(0)),
        (KeyDest::Game, vec![MenuEvent::Run("save s11".to_owned())])
    );
    let locked = MenuGame {
        can_save: false,
        ..game(0)
    };
    assert_eq!(Menu::new().save(KeyDest::Game, &locked), KeyDest::Game);
}

/// The options menu, its cursor moved down `n` items.
fn options_at(n: usize) -> Menu {
    let mut menu = Menu::new();
    menu.options();
    for _ in 0..n {
        menu.key(MenuKey::Down, &game(0));
    }
    menu
}

/// The settings `events` set, in order.
fn sets(events: &[MenuEvent]) -> Vec<(&'static str, f32)> {
    events
        .iter()
        .filter_map(|event| match event {
            MenuEvent::Set { name, value } => Some((*name, *value)),
            _ => None,
        })
        .collect()
}

/// `settings` with one changed.
fn with(change: impl FnOnce(&mut MenuSettings)) -> MenuGame<'static> {
    let mut settings = DEFAULTS;
    change(&mut settings);
    MenuGame {
        settings,
        ..game(0)
    }
}

// spec MENU-7.1: thirteen items, the video item skipped: down from 11 goes to 0, up from 0 to 11
// spec MENU-7.2: Escape to the main menu; the arrows move with menu1.wav
#[test]
fn the_options_skip_the_video_item() {
    let mut menu = options_at(11);
    let (items, _) = menu.draw(&frame(KeyDest::Menu, 0.0, false));
    assert_eq!(row(&items, 32 + 8 * 11).first(), Some(&(16, b' ' + 128)));
    assert!(items.contains(&Placed {
        anchor: Anchor::Centered,
        item: HudItem::Char {
            x: 200,
            y: 32 + 8 * 11,
            ch: 12
        },
    }));
    assert_eq!(
        menu.key(MenuKey::Down, &game(0)),
        (KeyDest::Menu, vec![MenuEvent::Sound("misc/menu1.wav")])
    );
    let (top, _) = menu.draw(&frame(KeyDest::Menu, 0.0, false));
    assert!(row(&top, 32).contains(&(200, 12)), "at item 0");
    menu.key(MenuKey::Up, &game(0));
    menu.key(MenuKey::Up, &game(0));
    let (up, _) = menu.draw(&frame(KeyDest::Menu, 0.0, false));
    assert!(
        row(&up, 32 + 8 * 10).contains(&(200, 12)),
        "12 is skipped going up"
    );
    assert!(
        !up.iter()
            .any(|p| matches!(p.item, HudItem::Char { y: 128, .. }))
    );
    assert_eq!(menu.key(MenuKey::Escape, &game(0)).0, KeyDest::Menu);
    assert_eq!(menu.state(), MenuState::Main);
}

// spec MENU-7.2: Enter: controls stay, console opens, defaults exec, a setting goes up
#[test]
fn the_options_enter_their_items() {
    let mut menu = options_at(0);
    assert_eq!(
        menu.key(MenuKey::Enter, &game(0)),
        (KeyDest::Menu, Vec::new())
    );
    let mut menu = options_at(2);
    assert_eq!(
        menu.key(MenuKey::Enter, &game(0)),
        (
            KeyDest::Menu,
            vec![MenuEvent::Run("exec default.cfg".to_owned())]
        )
    );
    let mut menu = options_at(7);
    assert_eq!(
        sets(&menu.key(MenuKey::Enter, &game(0)).1),
        [("volume", 0.8)]
    );
    let mut menu = options_at(1);
    assert_eq!(
        menu.key(MenuKey::Enter, &game(0)),
        (KeyDest::Console, Vec::new())
    );
    assert_eq!(menu.state(), MenuState::None);
}

// spec MENU-7.3: the labels, the sliders' ends, bars and knob, the checkboxes and the cursor
#[test]
fn the_options_draw_their_sliders() {
    let mut menu = options_at(3);
    let looks = MenuFrame {
        settings: MenuSettings {
            viewsize: 75.0,
            m_pitch: -0.022,
            ..DEFAULTS
        },
        ..frame(KeyDest::Menu, 0.0, false)
    };
    let (items, _) = menu.draw(&looks);
    assert!(pictures(&items).contains(&("gfx/p_option.lmp".to_owned(), None, 4)));
    assert!(pictures(&items).contains(&("gfx/qplaque.lmp".to_owned(), Some(16), 4)));
    assert_eq!(red_text(&items, 56, 200), "           Screen size");
    let size = row(&items, 56);
    let bar: Vec<(i32, u8)> = size.iter().copied().filter(|(x, _)| *x >= 212).collect();
    // The ends at 212 and 300, ten bars from 220, and the knob at 220 + 72 * 0.5 = 256.
    assert_eq!(bar.first(), Some(&(212, 128)));
    assert_eq!(bar.iter().filter(|(_, ch)| *ch == 129).count(), 10);
    assert!(bar.contains(&(300, 130)));
    assert!(bar.contains(&(256, 131)));
    assert!(size.contains(&(200, 12)), "the cursor on screen size");
    // Brightness at gamma 1 is 0, the knob at 220; sound volume 0.7 is at 220 + 50.4.
    assert!(row(&items, 64).contains(&(220, 131)));
    assert!(row(&items, 88).contains(&(270, 131)));
    let check = |y: i32| -> String {
        row(&items, y)
            .iter()
            .filter(|(x, _)| *x >= 220)
            .map(|(_, ch)| char::from(ch.wrapping_sub(128)))
            .collect()
    };
    assert_eq!(check(96), "off", "always run at 200");
    assert_eq!(check(104), "on", "inverted");
    assert_eq!(check(112), "off");
    let out_of_range = MenuFrame {
        settings: MenuSettings {
            volume: 3.0,
            ..DEFAULTS
        },
        ..frame(KeyDest::Menu, 0.0, false)
    };
    let (clamped, _) = menu.draw(&out_of_range);
    assert!(
        row(&clamped, 88).contains(&(292, 131)),
        "the range kept within 1"
    );
}

// spec MENU-7.4: each item's step and bounds, the toggles, menu3.wav on every item
#[test]
fn the_options_adjust_their_settings() {
    let adjust = |item: usize, key: MenuKey, game: &MenuGame<'_>| {
        let mut menu = options_at(item);
        let (_, events) = menu.key(key, game);
        assert_eq!(
            events.first(),
            Some(&MenuEvent::Sound("misc/menu3.wav")),
            "{item}"
        );
        sets(&events)
    };
    assert_eq!(adjust(3, MenuKey::Right, &game(0)), [("viewsize", 110.0)]);
    assert_eq!(
        adjust(3, MenuKey::Right, &with(|s| s.viewsize = 115.0)),
        [("viewsize", 120.0)]
    );
    assert_eq!(
        adjust(3, MenuKey::Left, &with(|s| s.viewsize = 30.0)),
        [("viewsize", 30.0)]
    );
    assert_eq!(adjust(4, MenuKey::Right, &game(0)), [("gamma", 0.95)]);
    assert_eq!(adjust(4, MenuKey::Left, &game(0)), [("gamma", 1.0)]);
    assert_eq!(
        adjust(4, MenuKey::Right, &with(|s| s.gamma = 0.52)),
        [("gamma", 0.5)]
    );
    assert_eq!(adjust(5, MenuKey::Right, &game(0)), [("sensitivity", 3.5)]);
    assert_eq!(
        adjust(5, MenuKey::Left, &with(|s| s.sensitivity = 1.2)),
        [("sensitivity", 1.0)]
    );
    assert_eq!(
        adjust(5, MenuKey::Right, &with(|s| s.sensitivity = 11.0)),
        [("sensitivity", 11.0)]
    );
    assert_eq!(adjust(6, MenuKey::Left, &game(0)), [("bgmvolume", 0.9)]);
    assert_eq!(adjust(6, MenuKey::Right, &game(0)), [("bgmvolume", 1.0)]);
    // C's float 0.7 plus the double 0.1, stored back as a float.
    #[expect(clippy::cast_possible_truncation, reason = "test code: the C sum")]
    let louder = (f64::from(0.7_f32) + 0.1) as f32;
    assert_eq!(adjust(7, MenuKey::Right, &game(0)), [("volume", louder)]);
    assert_eq!(
        adjust(7, MenuKey::Left, &with(|s| s.volume = 0.05)),
        [("volume", 0.0)]
    );
    assert_eq!(
        adjust(8, MenuKey::Left, &game(0)),
        [("cl_forwardspeed", 400.0), ("cl_backspeed", 400.0)]
    );
    assert_eq!(
        adjust(8, MenuKey::Right, &with(|s| s.forwardspeed = 400.0)),
        [("cl_forwardspeed", 200.0), ("cl_backspeed", 200.0)]
    );
    assert_eq!(adjust(9, MenuKey::Right, &game(0)), [("m_pitch", -0.022)]);
    assert_eq!(adjust(10, MenuKey::Left, &game(0)), [("lookspring", 1.0)]);
    assert_eq!(
        adjust(11, MenuKey::Right, &with(|s| s.lookstrafe = 2.0)),
        [("lookstrafe", 0.0)]
    );
    for item in [0, 1, 2] {
        assert!(adjust(item, MenuKey::Right, &game(0)).is_empty(), "{item}");
    }
}
