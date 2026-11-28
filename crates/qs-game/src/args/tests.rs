// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::path::PathBuf;

use super::{Args, engine_style};
use qs_sim::protocol::ProtocolVersion;

fn parse(words: &[&str]) -> Result<Args, clap::Error> {
    let line = core::iter::once("quakec-engine")
        .chain(words.iter().copied())
        .map(str::to_owned);
    <Args as clap::Parser>::try_parse_from(engine_style(line))
}

// spec BRIDGE-1.1: the IDE launches the host with `-bridge <path>`; ADR-0008's `-basedir`,
// `-game` and `-kpf` are the engine's single-dash options
#[test]
fn the_engine_options_take_one_dash() {
    let args = parse(&[
        "-basedir",
        "/run/rerelease",
        "-game",
        "mymod",
        "-kpf",
        "/g/QuakeEX.kpf",
        "-bridge",
        "/tmp/b.sock",
        "e1m1",
    ])
    .unwrap();
    assert_eq!(args.basedir, Some(PathBuf::from("/run/rerelease")));
    assert_eq!(args.game.as_deref(), Some("mymod"));
    assert_eq!(args.kpf, Some(PathBuf::from("/g/QuakeEX.kpf")));
    assert_eq!(args.bridge, Some(PathBuf::from("/tmp/b.sock")));
    assert_eq!(args.map, "e1m1");
    assert_eq!(
        parse(&["--game", "mymod"]).unwrap().game.as_deref(),
        Some("mymod")
    );
    assert_eq!(parse(&[]).unwrap().map, "start");
}

// ADR-0008: `-game` is a single directory name, never a path
#[test]
fn a_game_that_is_a_path_is_refused() {
    for path in ["../other", "a/b", "C:\\games", "..", ".", ""] {
        assert!(parse(&["-game", path]).is_err(), "{path:?}");
    }
    assert!(parse(&["-unknown"]).is_err());
}

// ADR-0008: the game filesystem gets the engine's own options, those after `--` last
#[test]
fn the_game_filesystem_gets_the_engine_options() {
    let args = parse(&[
        "-basedir", "/g", "-game", "mod", "--rogue", "e1m1", "--", "-path", "x",
    ])
    .unwrap();
    assert_eq!(
        args.engine(),
        ["-basedir", "/g", "-game", "mod", "-rogue", "-path", "x"]
    );
    assert!(parse(&[]).unwrap().engine().is_empty());
    assert_eq!(parse(&["--hipnotic"]).unwrap().engine(), ["-hipnotic"]);
}

// spec SND-6.5: GOG's gamea.cue is Scourge of Armagon's CD and gamed.cue Dissolution of
// Eternity's, by the engine's switch or the game's directory; any other game plays game.cue
#[test]
fn each_mission_pack_plays_its_own_cd() {
    assert_eq!(parse(&["--hipnotic"]).unwrap().cd_sheet(), "gamea.cue");
    assert_eq!(
        parse(&["-game", "hipnotic"]).unwrap().cd_sheet(),
        "gamea.cue"
    );
    assert_eq!(parse(&["--rogue"]).unwrap().cd_sheet(), "gamed.cue");
    assert_eq!(parse(&["-game", "Rogue"]).unwrap().cd_sheet(), "gamed.cue");
    assert_eq!(parse(&[]).unwrap().cd_sheet(), "game.cue");
    assert_eq!(parse(&["-game", "mymod"]).unwrap().cd_sheet(), "game.cue");
}

// spec NET-6.11: --protocol, or -protocol, is the host's first sv_protocol, 666 when not given
#[test]
fn the_protocol_option_sets_sv_protocol() {
    let parse = |words: &[&str]| Args::parse_words(words.iter().map(|w| (*w).to_owned())).unwrap();
    assert_eq!(
        parse(&["--protocol", "15", "e1m1"]).protocol.version(),
        ProtocolVersion::NetQuake
    );
    assert_eq!(
        parse(&["-protocol", "999"]).protocol.version(),
        ProtocolVersion::Rmq
    );
    assert_eq!(parse(&[]).protocol.version(), ProtocolVersion::FitzQuake);
    assert!(Args::parse_words(["--protocol".to_owned(), "16".to_owned()]).is_err());
}

// spec BRIDGE-3.6: `--paused` starts the game paused when there is a bridge to resume it by;
// with none it is read and does nothing
#[test]
fn paused_needs_a_bridge_to_resume_by() {
    let with = parse(&["-bridge", "/tmp/qs.sock", "--paused", "start"]).unwrap();
    assert!(with.paused && with.starts_paused());
    let without = parse(&["--paused", "start"]).unwrap();
    assert!(without.paused && !without.starts_paused());
    let running = parse(&["-bridge", "/tmp/qs.sock", "start"]).unwrap();
    assert!(!running.starts_paused());
}

// spec NET-7.1: -listen takes an optional count of slots, 8 when it has none, after a map name
// as well as before it
#[test]
fn listen_takes_an_optional_count() {
    let parse = |words: &[&str]| parse(words).unwrap();
    assert_eq!(parse(&["-listen", "4", "e1m1"]).listen, Some(4));
    assert_eq!(parse(&["e1m1", "-listen"]).listen, Some(8));
    assert_eq!(
        parse(&["--listen", "0"]).listen,
        Some(0),
        "the load takes 0 as 8"
    );
    assert_eq!(parse(&[]).listen, None);
}

fn setting<'a>(settings: &'a [(String, String)], name: &str) -> Option<&'a str> {
    settings
        .iter()
        .rev()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
}

// spec NET-7.1: with no `-listen` the game is single player, one slot and no setting; with
// one, `-listen`'s slots and deathmatch 1
#[test]
fn the_default_is_single_player_with_no_bots() {
    let args = parse(&["e1m1"]).unwrap();
    assert_eq!(args.players, 1);
    assert_eq!(args.bots, 0);
    assert_eq!(args.rules, None);
    assert_eq!(args.maxplayers(), qs_sim::MaxPlayers::SINGLE);
    assert!(args.new_game_settings(args.maxplayers()).is_empty());
    assert!(args.bot_lines().is_empty());
    let args = parse(&["-listen", "4"]).unwrap();
    assert_eq!(args.maxplayers().get(), 4);
    let settings = args.new_game_settings(args.maxplayers());
    assert_eq!(setting(&settings, "deathmatch"), Some("1"));
    assert_eq!(setting(&settings, "coop"), None);
    assert_eq!(parse(&["-listen", "0"]).unwrap().maxplayers().get(), 8);
}

// spec DIFF-3.2: `--rules coop` sets coop 1 and deathmatch 0, the last word on deathmatch;
// `--rules deathmatch` sets deathmatch 1 even with one slot
#[test]
fn coop_sets_coop_and_not_deathmatch() {
    let args = parse(&["-listen", "4", "--rules", "coop"]).unwrap();
    let settings = args.new_game_settings(args.maxplayers());
    assert_eq!(setting(&settings, "coop"), Some("1"));
    assert_eq!(setting(&settings, "deathmatch"), Some("0"));
    let alone = parse(&["--rules", "coop"]).unwrap();
    let settings = alone.new_game_settings(alone.maxplayers());
    assert_eq!(setting(&settings, "coop"), Some("1"));
    let deathmatch = parse(&["--rules", "deathmatch"]).unwrap();
    let settings = deathmatch.new_game_settings(deathmatch.maxplayers());
    assert_eq!(setting(&settings, "deathmatch"), Some("1"));
    assert!(parse(&["--rules", "ctf"]).is_err());
}

// spec BOT-9.3: `--bots n` types `addbot` n times, with `--bot-skill` when given
#[test]
fn bots_are_typed_as_addbot() {
    let args = parse(&["-listen", "4", "--bots", "3", "--bot-skill", "2"]).unwrap();
    assert_eq!(args.bot_lines(), vec!["addbot 2\n"; 3]);
    let args = parse(&["-listen", "4", "--bots", "1"]).unwrap();
    assert_eq!(args.bot_lines(), vec!["addbot\n"]);
    assert!(parse(&["--bot-skill", "4"]).is_err(), "skill is 0 to 3");
}

// spec NET-7.4: the local players are 1 to 4
#[test]
fn the_local_players_are_one_to_four() {
    assert_eq!(parse(&["--players", "4"]).unwrap().players, 4);
    assert!(parse(&["--players", "0"]).is_err());
    assert!(parse(&["--players", "5"]).is_err());
    let two = parse(&["--players", "2"]).unwrap();
    assert!(two.check_players(two.maxplayers()).is_err(), "one slot");
    let room = parse(&["--players", "2", "-listen", "2"]).unwrap();
    assert!(room.check_players(room.maxplayers()).is_ok());
}
