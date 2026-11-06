// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::indexing_slicing,
    reason = "test code: a failed index is a failed test"
)]

use super::{MENU_HELP_PAGES, Menu, MenuEvent, MenuKey, MenuState};
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
    menu.key(MenuKey::Enter, 0);
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
    assert_eq!(menu.key(MenuKey::Down, 0), (KeyDest::Game, Vec::new()));
}

// spec MENU-1.3: drawn only while shown and the keys are the menu's: the fade, or the console's
// background with the console down, then the menu in the centred area; entering plays
// misc/menu2.wav once, moving misc/menu1.wav
#[test]
fn the_menu_draws_over_a_fade_with_its_sounds() {
    let mut menu = main_menu();
    let (items, sound) = menu.draw(KeyDest::Menu, 0.0, false, 480);
    assert_eq!(items[0].item, HudItem::Fade);
    assert!(items[1..].iter().all(|p| p.anchor == Anchor::Centered));
    assert_eq!(sound, Some("misc/menu2.wav"));
    assert_eq!(menu.draw(KeyDest::Menu, 0.0, false, 480).1, None);
    let (down, _) = menu.draw(KeyDest::Menu, 0.0, true, 480);
    assert_eq!(
        down[0].item,
        HudItem::ConsoleBack {
            lines: 480,
            height: 480
        }
    );
    assert!(menu.draw(KeyDest::Game, 0.0, false, 480).0.is_empty());
    let (_, events) = menu.key(MenuKey::Down, 0);
    assert_eq!(events, [MenuEvent::Sound("misc/menu1.wav")]);
}

// spec MENU-1.4: M_Print draws each character +128, M_PrintWhite as it is
// spec MENU-5.1: the quit prompt's text box and its message, rand () & 7 of eight, over the
// menu it came from
#[test]
fn the_quit_prompt_prints_its_message() {
    let mut menu = main_menu();
    for _ in 0..4 {
        menu.key(MenuKey::Down, 0);
    }
    menu.key(MenuKey::Enter, 9);
    assert_eq!(menu.state(), MenuState::Quit);
    let (items, _) = menu.draw(KeyDest::Menu, 0.0, false, 480);
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
        menu.quit(KeyDest::Menu, 3),
        KeyDest::Menu,
        "asked again, nothing changes"
    );
}

// spec MENU-2.1: the plaque, the title centred, the items and the cursor frame by host time
#[test]
fn the_main_menu_draws_its_pictures() {
    let mut menu = main_menu();
    menu.key(MenuKey::Down, 0);
    let (items, _) = menu.draw(KeyDest::Menu, 0.35, false, 480);
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
// spec MENU-2.3: Enter on multiplayer or options stays in the main menu
#[test]
fn the_main_menu_moves_and_wraps() {
    let mut menu = main_menu();
    menu.key(MenuKey::Up, 0);
    let (items, _) = menu.draw(KeyDest::Menu, 0.0, false, 480);
    assert!(pictures(&items).contains(&("gfx/menudot1.lmp".to_owned(), Some(54), 32 + 4 * 20)));
    menu.key(MenuKey::Down, 0);
    menu.key(MenuKey::Down, 0);
    assert_eq!(menu.key(MenuKey::Enter, 0).0, KeyDest::Menu);
    assert_eq!(
        menu.state(),
        MenuState::Main,
        "multiplayer is not this menu's"
    );
    assert_eq!(menu.key(MenuKey::Escape, 0).0, KeyDest::Game);
    assert_eq!(menu.state(), MenuState::None);
}

// spec MENU-3.1: the single player menu's title and items
// spec MENU-3.2: New Game closes the menus and starts the start map; Escape goes back
// spec MENU-3.3: load and save stay in the menu
#[test]
fn single_player_starts_a_new_game() {
    let mut menu = main_menu();
    menu.key(MenuKey::Enter, 0);
    let (items, _) = menu.draw(KeyDest::Menu, 0.0, false, 480);
    let found = pictures(&items);
    assert!(found.contains(&("gfx/ttl_sgl.lmp".to_owned(), None, 4)));
    assert!(found.contains(&("gfx/sp_menu.lmp".to_owned(), Some(72), 32)));
    menu.key(MenuKey::Down, 0);
    assert_eq!(menu.key(MenuKey::Enter, 0).0, KeyDest::Menu, "load");
    menu.key(MenuKey::Up, 0);
    assert_eq!(
        menu.key(MenuKey::Enter, 0),
        (KeyDest::Game, vec![MenuEvent::NewGame])
    );
    let mut back = main_menu();
    back.key(MenuKey::Enter, 0);
    back.key(MenuKey::Escape, 0);
    assert_eq!(back.state(), MenuState::Main);
}

// spec MENU-4.1: help opens on page 0, drawn at (0, 0)
// spec MENU-4.2: the arrows turn the pages round the six; Escape goes back
#[test]
fn help_turns_its_pages() {
    let mut menu = main_menu();
    for _ in 0..3 {
        menu.key(MenuKey::Down, 0);
    }
    menu.key(MenuKey::Enter, 0);
    assert_eq!(menu.state(), MenuState::Help);
    let page = |menu: &mut Menu| pictures(&menu.draw(KeyDest::Menu, 0.0, false, 480).0);
    assert_eq!(page(&mut menu), [("gfx/help0.lmp".to_owned(), Some(0), 0)]);
    menu.key(MenuKey::Left, 0);
    assert_eq!(
        page(&mut menu)[0].0,
        format!("gfx/help{}.lmp", MENU_HELP_PAGES - 1)
    );
    menu.key(MenuKey::Right, 0);
    menu.key(MenuKey::Up, 0);
    assert_eq!(page(&mut menu)[0].0, "gfx/help1.lmp");
    menu.key(MenuKey::Escape, 0);
    assert_eq!(menu.state(), MenuState::Main);
}

// spec MENU-5.2: n, N and Escape go back where the prompt came from; y quits
#[test]
fn the_quit_prompt_answers_its_keys() {
    let mut menu = main_menu();
    menu.quit(KeyDest::Menu, 0);
    assert_eq!(menu.key(MenuKey::Char(b'x'), 0).0, KeyDest::Menu);
    assert_eq!(menu.key(MenuKey::Char(b'N'), 0).0, KeyDest::Menu);
    assert_eq!(menu.state(), MenuState::Main);
    let mut game = Menu::new();
    game.quit(KeyDest::Game, 0);
    assert_eq!(game.key(MenuKey::Escape, 0).0, KeyDest::Game);
    game.quit(KeyDest::Game, 0);
    assert_eq!(
        game.key(MenuKey::Char(b'y'), 0),
        (KeyDest::Console, vec![MenuEvent::Quit])
    );
}
