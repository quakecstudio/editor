// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use super::{GameMode, GameOptions, GameOptionsError, MAX_LOCAL_PLAYERS};

/// Every client slot, players and bots together, as a count of bots.
const MAX_CLIENTS: u8 = 16;
use crate::preview::engine::BotSkillParam;

fn words(options: GameOptions) -> Vec<String> {
    options.host_words().unwrap()
}

fn options(mode: GameMode, players: u8, bots: u8) -> GameOptions {
    GameOptions {
        mode,
        players,
        bots,
        bot_skill: None,
    }
}

// spec NET-7.1: the default is single player with no bots, which adds nothing to the host's
// command line, one slot as always
#[test]
fn the_default_is_single_player_with_no_bots() {
    let default = GameOptions::default();
    assert_eq!(default.mode, GameMode::Single);
    assert_eq!(default.players, 1);
    assert_eq!(default.bots, 0);
    assert_eq!(default.bot_skill, None);
    assert!(words(default).is_empty());
    let parsed: GameOptions = serde_json::from_str("{}").unwrap();
    assert_eq!(parsed, default, "every field has its default");
}

// spec NET-7.1: deathmatch listens with -listen's eight slots, or room for every client
#[test]
fn deathmatch_listens() {
    assert_eq!(words(options(GameMode::Deathmatch, 1, 0)), ["-listen", "8"]);
    assert_eq!(
        words(options(GameMode::Deathmatch, 2, 12)),
        ["-listen", "14", "--players", "2", "--bots", "12"]
    );
}

// spec DIFF-3.2: coop is `--rules coop` beside -listen
#[test]
fn coop_says_so() {
    assert_eq!(
        words(options(GameMode::Coop, 1, 0)),
        ["-listen", "8", "--rules", "coop"]
    );
}

// spec BOT-1.4: the bots' skill goes with them, as addbot's number
#[test]
fn the_bots_skill_goes_with_them() {
    let hard = GameOptions {
        bot_skill: Some(BotSkillParam::Hard),
        ..options(GameMode::Deathmatch, 1, 3)
    };
    assert_eq!(
        words(hard),
        ["-listen", "8", "--bots", "3", "--bot-skill", "2"]
    );
    let alone = GameOptions {
        bot_skill: Some(BotSkillParam::Hard),
        ..options(GameMode::Deathmatch, 1, 0)
    };
    assert_eq!(words(alone), ["-listen", "8"], "no bots, no skill");
}

// spec NET-7.1: options that make no game are refused: others in single player, no player or
// more than four, and more than sixteen players and bots
#[test]
fn options_that_make_no_game_are_refused() {
    let refused = |o: GameOptions| o.host_words().unwrap_err();
    assert_eq!(
        refused(options(GameMode::Single, 2, 0)),
        GameOptionsError::SinglePlayerAlone
    );
    assert_eq!(
        refused(options(GameMode::Single, 1, 1)),
        GameOptionsError::SinglePlayerAlone
    );
    assert_eq!(
        refused(options(GameMode::Coop, 0, 0)),
        GameOptionsError::Players(0)
    );
    assert_eq!(
        refused(options(GameMode::Coop, MAX_LOCAL_PLAYERS + 1, 0)),
        GameOptionsError::Players(5)
    );
    assert!(
        options(GameMode::Coop, MAX_LOCAL_PLAYERS, 0)
            .host_words()
            .is_ok()
    );
    assert_eq!(
        refused(options(GameMode::Deathmatch, 4, MAX_CLIENTS - 3)),
        GameOptionsError::Clients(17)
    );
    assert!(
        options(GameMode::Deathmatch, 4, MAX_CLIENTS - 4)
            .host_words()
            .is_ok()
    );
    for error in [
        GameOptionsError::SinglePlayerAlone,
        GameOptionsError::Players(0),
        GameOptionsError::Clients(17),
    ] {
        assert!(!error.to_string().is_empty());
    }
}

// spec NET-7.1: the options are read from JSON with their names, and an unknown name is refused
#[test]
fn the_options_read_from_json() {
    let read: GameOptions = serde_json::from_str(
        r#"{"mode": "deathmatch", "players": 2, "bots": 3, "bot_skill": "nightmare"}"#,
    )
    .unwrap();
    assert_eq!(read.mode, GameMode::Deathmatch);
    assert_eq!(read.bot_skill, Some(BotSkillParam::Nightmare));
    assert!(serde_json::from_str::<GameOptions>(r#"{"teams": 2}"#).is_err());
    assert!(GameMode::ALL.iter().filter(|m| m.is_multiplayer()).count() == 2);
}
