// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::string_slice,
    reason = "test code: a failed unwrap or index is a failed test; the saved text is ASCII"
)]

use qc_progs::Progs;

use super::{SaveError, SaveGame, next_block};
use crate::LoadOptions;
use crate::world::tests::{Running, ledge, player_from, player_progs, running_from};

/// A progs whose player keeps a counter across saves, with a saved global and a thing spawned.
fn progs() -> Vec<u8> {
    let more = "float counter;\n.float charge;\n\
        void() SetNewParms = { parm1 = 2; parm3 = 0.5; };\n\
        void() StartFrame = { counter = counter + 1; };\n";
    player_progs(
        "self.health = 42; self.armorvalue = parm1; self.charge = 3;",
        "",
        more,
    )
}

fn running() -> Running {
    player_from(&ledge(), progs(), LoadOptions::default())
}

fn save(run: &Running) -> Vec<u8> {
    run.world
        .save_game(&Progs::read(&run.progs).unwrap())
        .unwrap()
}

fn lines(text: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(text)
        .lines()
        .map(str::to_owned)
        .collect()
}

// spec PHYS-10.5: version 5, the comment, the 16 spawn parms as %f, skill 1, the map, the
// time, the 64 light styles with m for one not set, then the globals and the edicts
#[test]
fn a_saved_game_is_written_as_host_savegame_f_writes_it() {
    let mut run = running();
    run.frames(3, 0.1).unwrap();
    let text = save(&run);
    let lines = lines(&text);
    assert_eq!(lines[0], "5");
    assert_eq!(lines[1].len(), 39);
    assert_eq!(
        &lines[1][22..],
        "kills:__0/__0____",
        "the kills from 22, spaces as _"
    );
    assert!(lines[1][..22].bytes().all(|b| b == b'_'), "no message");
    assert_eq!(lines[2], "2.000000");
    assert_eq!(lines[3], "0.000000");
    assert_eq!(lines[4], "0.500000");
    assert_eq!(lines[18], "1", "skill");
    assert_eq!(lines[19], "test");
    assert_eq!(lines[20], format!("{:.6}", run.world.time()));
    assert!(lines[21..85].iter().all(|style| style == "m"));
    assert_eq!(lines[85], "{", "the globals");
    assert!(
        lines.contains(&"\"counter\" \"3.000000\"".to_owned()),
        "{lines:?}"
    );
    // The world's edict, then the player's, which writes its non-zero fields.
    let player = lines
        .iter()
        .position(|l| l == "\"health\" \"42.000000\"")
        .unwrap();
    assert!(
        lines[..player]
            .iter()
            .any(|l| l == "\"classname\" \"worldspawn\"")
    );
    assert!(lines.contains(&"\"armorvalue\" \"2.000000\"".to_owned()));
    assert!(
        lines.contains(&"\"charge\" \"3.000000\"".to_owned()),
        "a progs field"
    );
    assert!(
        !lines.iter().any(|l| l.starts_with("\"origin_x\"")),
        "no _x fields"
    );
    assert!(
        !lines.iter().any(|l| l == "\"frame\" \"0.000000\""),
        "no zero fields"
    );
}

// spec PHYS-10.5: a saved game read back over its map restores the globals, the edicts and
// the time; the client then spawns with no SetNewParms or PutClientInServer
#[test]
fn a_saved_game_reads_back_over_its_map() {
    let mut run = running();
    run.frames(3, 0.1).unwrap();
    let progs = Progs::read(&run.progs).unwrap();
    run.world.set_float_field(1, "health", 17.0);
    let text = save(&run);
    let game = SaveGame::read(&text).unwrap();
    assert_eq!((game.map.as_str(), game.skill), ("test", 1));
    assert_eq!(game.parms[..3], [2.0, 0.0, 0.5]);
    let mut back = running_from(
        &ledge(),
        run.progs.clone(),
        "",
        Vec::new(),
        LoadOptions::default(),
    );
    back.world.restore(&progs, &game).unwrap();
    back.world.connect(&progs).unwrap();
    assert_eq!(
        back.world.float(1, "health"),
        17.0,
        "PutClientInServer did not run"
    );
    assert_eq!(back.world.float(1, "armorvalue"), 2.0);
    assert_eq!(back.world.time(), f64::from(game.time));
    assert_eq!(save(&back), text, "the same file again");
}

// spec PHYS-10.5: "Can't savegame with a dead player"
#[test]
fn a_dead_player_cannot_be_saved() {
    let mut run = running();
    run.world.set_float_field(1, "health", 0.0);
    let progs = Progs::read(&run.progs).unwrap();
    assert_eq!(run.world.save_game(&progs), Err(SaveError::DeadPlayer));
}

// spec PHYS-10.5: the header's version must be 5, the skill is (int)(f + 0.1), and a file
// that ends in its header is refused
#[test]
fn the_header_is_read_as_fscanf_reads_it() {
    let header = |version: &str, skill: &str| {
        let mut text = format!("{version}\ncomment\n");
        text.push_str(&"0.000000\n".repeat(16));
        text.push_str(&format!("{skill}\ne1m1\n12.5\n"));
        text.push_str(&"m\n".repeat(64));
        text.push_str("{\n}\n");
        SaveGame::read(text.as_bytes())
    };
    assert_eq!(header("4", "1").unwrap_err(), SaveError::Version(4));
    assert_eq!(header("5", "1.95").unwrap().skill, 2);
    assert_eq!(
        header("5", "0.9").unwrap().skill,
        1,
        "0.9 + 0.1 truncates to 1"
    );
    let game = header("5", "3").unwrap();
    assert_eq!((game.map.as_str(), game.time), ("e1m1", 12.5));
    assert_eq!(game.comment, b"comment");
    assert_eq!(
        SaveGame::read(b"5\ncomment\n1.0\n").unwrap_err(),
        SaveError::Truncated
    );
    assert_eq!(SaveGame::read(b"").unwrap_err(), SaveError::Truncated);
}

// spec PHYS-10.5: each block is read up to its first `}`, even one inside a string, or to
// the end or a zero byte; a block of 32767 bytes or more overflows
#[test]
fn blocks_end_at_their_first_closing_brace() {
    let (block, rest) = next_block(b"{ \"a\" \"x}y\" }\n{ }").unwrap();
    assert_eq!(block, b"{ \"a\" \"x}");
    assert_eq!(rest, b"y\" }\n{ }");
    let (block, rest) = next_block(b"{ \"a\" \0 \"b\" }").unwrap();
    assert_eq!(block, b"{ \"a\" ");
    assert!(rest.is_empty(), "a zero byte ends the read");
    let long = vec![b' '; 32767];
    assert_eq!(next_block(&long), Err(SaveError::BlockOverflow));
    assert!(next_block(&long[..32766]).is_ok());
}

// spec PHYS-10.5: a block that does not start with a brace, or whose pairs do not parse, is
// refused
#[test]
fn a_broken_block_is_refused() {
    let run = running();
    let progs = Progs::read(&run.progs).unwrap();
    let text = save(&run);
    let header_end = text.windows(2).position(|w| w == b"\n{").unwrap() + 1;
    let with = |blocks: &[u8]| {
        let mut text = text[..header_end].to_vec();
        text.extend_from_slice(blocks);
        let game = SaveGame::read(&text).unwrap();
        let mut back = running_from(
            &ledge(),
            run.progs.clone(),
            "",
            Vec::new(),
            LoadOptions::default(),
        );
        back.world.restore(&progs, &game)
    };
    assert_eq!(with(b"x }"), Err(SaveError::NoBrace));
    assert!(matches!(with(b"{ \"counter\" }"), Err(SaveError::Parse(_))));
    assert!(matches!(with(b"{ \"counter\""), Err(SaveError::Parse(_))));
    assert_eq!(
        with(b"{ \"not_a_global\" \"1\" }\n"),
        Ok(()),
        "an unknown global is passed over"
    );
    let many = "{\n}\n".repeat(700);
    assert_eq!(
        with(format!("{{\n}}\n{many}").as_bytes()),
        Err(SaveError::TooManyEdicts)
    );
}
