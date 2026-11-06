// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use qc_progs::Progs;

use crate::world::tests::{Running, ledge, player, player_progs, running_from};
use crate::world::{HostCommand, LoadOptions};

/// Runs `text` on `run`'s console and gives back what it printed, a line each.
fn console(run: &mut Running, text: &str) -> Vec<String> {
    run.world.console(text);
    run.world.execute_console(&Progs::read(&run.progs).unwrap());
    printed(run)
}

/// What the console printed since the last call, a line each.
fn printed(run: &mut Running) -> Vec<String> {
    run.world
        .take_console_output()
        .iter()
        .map(|line| String::from_utf8_lossy(line).into_owned())
        .collect()
}

/// The string commands the console sent the server, as text.
fn sent(run: &mut Running) -> Vec<String> {
    core::mem::take(&mut run.world.commands)
        .iter()
        .map(|text| String::from_utf8_lossy(text).into_owned())
        .collect()
}

/// A connected player on the ledge, with `files` to exec.
fn with_files(files: Vec<(String, Vec<u8>)>) -> Running {
    let mut run = running_from(
        &ledge(),
        player_progs("", "", ""),
        "",
        files,
        LoadOptions {
            search_path: vec![
                "/q/id1".to_owned(),
                "/q/id1/pak0.pak (339 files)".to_owned(),
            ],
            ..LoadOptions::default()
        },
    );
    run.connect();
    run
}

fn connected() -> Running {
    player(&ledge(), "", "", "")
}

// spec PHYS-13.1: a line ends at a line feed or at a `;` outside quotes, counted from the
// start of the line; the first word is matched ASCII case-insensitively
#[test]
fn the_buffer_splits_at_line_feeds_and_unquoted_semicolons() {
    let mut run = connected();
    assert_eq!(
        console(&mut run, "echo \"a;b\" c; ECHO d\necho e"),
        ["a;b c ", "d ", "e "]
    );
    assert_eq!(console(&mut run, "echo \"x; echo y\n"), ["x; echo y "]);
    assert!(
        console(&mut run, ";;\n\n").is_empty(),
        "empty lines do nothing"
    );
    assert!(console(&mut run, "").is_empty());
}

// spec PHYS-13.2: wait leaves the rest of the buffer for the next frame
#[test]
fn wait_leaves_the_rest_for_the_next_frame() {
    let mut run = connected();
    assert_eq!(console(&mut run, "echo a; wait; echo b\n"), ["a "]);
    run.world.execute_console(&Progs::read(&run.progs).unwrap());
    assert_eq!(printed(&mut run), ["b "]);
}

// spec PHYS-13.2: an alias's text is each argument after the name and a space, then a line
// feed, run in the alias's place; a new alias goes first in the list, a name is redefined
// only by the same case, and a name of 32 bytes or more is refused
#[test]
fn aliases_run_their_text_in_place() {
    let mut run = connected();
    assert!(console(&mut run, "alias go \"echo one; echo two\" three\n").is_empty());
    assert_eq!(
        console(&mut run, "GO; echo after\n"),
        ["one ", "two three ", "after "]
    );
    console(&mut run, "alias stop\n");
    assert_eq!(
        console(&mut run, "alias\n"),
        [
            "Current alias commands:",
            "stop : ",
            "",
            "go : echo one; echo two three ",
            ""
        ]
    );
    console(&mut run, "alias go echo again\n");
    assert_eq!(console(&mut run, "go\n"), ["again "], "redefined in place");
    let long = "a".repeat(32);
    assert_eq!(
        console(&mut run, &format!("alias {long} echo x\n")),
        ["Alias name is too long"]
    );
    let fits = "a".repeat(31);
    assert!(console(&mut run, &format!("alias {fits} echo x\n")).is_empty());
}

// spec PHYS-13.2: exec puts a file's text before the rest of the buffer
#[test]
fn exec_runs_a_file_before_the_rest() {
    let mut run = with_files(vec![(
        "go.cfg".to_owned(),
        b"echo in; echo file\n".to_vec(),
    )]);
    assert_eq!(
        console(&mut run, "exec go.cfg; echo after\n"),
        ["execing go.cfg", "in ", "file ", "after "]
    );
    assert_eq!(
        console(&mut run, "exec none.cfg\n"),
        ["couldn't exec none.cfg"]
    );
    assert_eq!(
        console(&mut run, "exec\nexec a b\n"),
        [
            "exec <filename> : execute a script file",
            "exec <filename> : execute a script file"
        ]
    );
}

// spec PHYS-13.3: a word that is no command or alias prints or sets the setting of that
// exact name, else is unknown
#[test]
fn settings_print_and_set_by_exact_name() {
    let mut run = connected();
    assert_eq!(
        console(&mut run, "sv_gravity\n"),
        ["\"sv_gravity\" is \"800\""]
    );
    assert!(console(&mut run, "sv_gravity 100 more\n").is_empty());
    assert_eq!(
        console(&mut run, "sv_gravity\n"),
        ["\"sv_gravity\" is \"100\""]
    );
    assert_eq!(
        console(&mut run, "SV_GRAVITY\n"),
        ["Unknown command \"SV_GRAVITY\""]
    );
}

// spec PHYS-13.4: a command the client's console forwards goes to the server as its name, a
// space and the rest of the line, or a line feed; `cmd` sends only the rest
#[test]
fn forwarded_commands_go_to_the_server() {
    let mut run = connected();
    assert!(console(&mut run, "god; give s 5; cmd say \"hi there\"\n").is_empty());
    assert_eq!(sent(&mut run), ["god \n", "give s 5", "say \"hi there\""]);
    for name in [
        "notarget", "fly", "noclip", "kill", "pause", "say", "say_team", "tell", "ping",
    ] {
        console(&mut run, &format!("{name}\n"));
        assert_eq!(sent(&mut run), [format!("{name} \n")], "{name}");
    }
}

// spec PHYS-13.4: with no connection a forwarded command prints that it cannot run
#[test]
fn forwarded_commands_need_a_connection() {
    let mut run = running_from(&ledge(), player_progs("", "", ""), "", Vec::new(), {
        LoadOptions::default()
    });
    assert_eq!(console(&mut run, "god\n"), ["Can't \"god\", not connected"]);
    assert_eq!(
        console(&mut run, "cmd x\n"),
        ["Can't \"cmd\", not connected"]
    );
    assert!(console(&mut run, "name bob; color 3\n").is_empty());
    assert!(sent(&mut run).is_empty(), "name and color only set");
    assert_eq!(console(&mut run, "name\n"), ["\"name\" is \"bob\""]);
}

// spec PHYS-13.4: name prints _cl_name, or sets it to the one argument or the rest of the
// line cut to 15 bytes, and forwards it unless it is unchanged
#[test]
fn name_sets_and_forwards_the_players_name() {
    let mut run = connected();
    assert_eq!(console(&mut run, "name\n"), ["\"name\" is \"player\""]);
    console(&mut run, "name player\n");
    assert!(sent(&mut run).is_empty(), "unchanged");
    console(&mut run, "name abcdefghijklmnopqrs\n");
    assert_eq!(
        sent(&mut run),
        ["name abcdefghijklmnopqrs"],
        "one word goes whole"
    );
    assert_eq!(
        console(&mut run, "name\n"),
        ["\"name\" is \"abcdefghijklmno\""]
    );
    console(&mut run, "name the long player name\n");
    assert_eq!(sent(&mut run), ["name the long player"], "the rest is cut");
}

// spec PHYS-13.4: color takes one colour for both or a top and a bottom, each its low four
// bits and at most 13, and sets _cl_color to top * 16 + bottom
#[test]
fn color_sets_both_colours() {
    let mut run = connected();
    assert_eq!(
        console(&mut run, "color\n"),
        ["\"color\" is \"0 0\"", "color <0-13> [0-13]"]
    );
    console(&mut run, "color 14\n");
    assert_eq!(sent(&mut run), ["color 14"]);
    assert_eq!(console(&mut run, "color\n")[0], "\"color\" is \"13 13\"");
    console(&mut run, "color 1 18 9\n");
    assert_eq!(console(&mut run, "color\n")[0], "\"color\" is \"1 2\"");
    console(&mut run, "color x -1\n");
    assert_eq!(console(&mut run, "color\n")[0], "\"color\" is \"0 13\"");
    assert_eq!(
        run.world.builtins.mock.engine().cvars.text("_cl_color"),
        Some("13.000000")
    );
}

// spec PHYS-13.5: status, version and the signon commands print what id's host prints from
// the server's console
#[test]
fn status_and_version_print_the_host() {
    let mut run = connected();
    assert_eq!(
        console(&mut run, "status\n"),
        [
            "host:    UNNAMED",
            "version: 1.09",
            "map:     test",
            "players: 1 active (1 max)",
            "",
            "#1  player              0   0:00:00",
            "   LOCAL"
        ]
    );
    let version = console(&mut run, "version\n");
    assert_eq!(version[0], "Version 1.09");
    assert!(version[1].starts_with("Exe: QuakeC Studio "));
    assert_eq!(
        console(&mut run, "prespawn; Spawn; begin\n"),
        [
            "prespawn is not valid from the console",
            "spawn is not valid from the console",
            "begin is not valid from the console"
        ]
    );
    assert!(console(&mut run, "kick player; stuffcmds\n").is_empty());
    assert_eq!(
        console(&mut run, "stuffcmds now\n"),
        ["stuffcmds : execute command line parameters"]
    );
}

// spec PHYS-13.6: a command the host carries out stops the buffer until it has, and the rest
// runs after
#[test]
fn host_commands_wait_for_the_host() {
    let mut run = connected();
    let host = |run: &mut Running, text: &str| {
        let out = console(run, text);
        (run.world.take_host_command(), out)
    };
    assert_eq!(
        host(&mut run, "map e1m1 x; echo later\n"),
        (Some(HostCommand::Map("e1m1".to_owned())), Vec::new())
    );
    run.world.execute_console(&Progs::read(&run.progs).unwrap());
    assert_eq!(printed(&mut run), ["later "], "the rest runs after");
    assert_eq!(host(&mut run, "map\n"), (None, Vec::new()));
    assert_eq!(
        host(&mut run, "changelevel e1m2\n").0,
        Some(HostCommand::Changelevel("e1m2".to_owned()))
    );
    assert_eq!(
        host(&mut run, "changelevel a b\n"),
        (
            None,
            vec!["changelevel <levelname> : continue game on a new level".to_owned()]
        )
    );
    assert_eq!(host(&mut run, "restart\n").0, Some(HostCommand::Restart));
    assert_eq!(
        host(&mut run, "save s1\n").0,
        Some(HostCommand::Save("s1".to_owned()))
    );
    assert_eq!(
        host(&mut run, "save ../s1\n"),
        (None, vec!["Relative pathnames are not allowed.".to_owned()])
    );
    assert_eq!(
        host(&mut run, "save\n").1,
        ["save <savename> : save a game"]
    );
    assert_eq!(
        host(&mut run, "load s1\n").0,
        Some(HostCommand::Load("s1".to_owned()))
    );
    assert_eq!(
        host(&mut run, "load a b\n").1,
        ["load <savename> : load a game"]
    );
    assert_eq!(host(&mut run, "quit\n").0, Some(HostCommand::Quit));
}

// spec PHYS-13.7: edict prints an edict's fields that are not all zero, each name padded to
// 15 and its value as PR_ValueString prints it; a number past the last edict, or below 0, is
// bad
#[test]
fn edict_prints_the_fields_that_are_set() {
    let mut run = connected();
    let lines = console(&mut run, "edict 1\n");
    assert_eq!(&lines[..2], ["", "EDICT 1:"]);
    assert!(
        lines.contains(&"health         100.0".to_owned()),
        "{lines:?}"
    );
    assert!(lines.contains(&"view_ofs       '  0.0   0.0  22.0'".to_owned()));
    assert!(
        lines.iter().all(|l| !l.starts_with("view_ofs_")),
        "no _x, _y, _z"
    );
    assert!(
        lines.contains(&"netname        player".to_owned()),
        "a string"
    );
    assert!(
        lines.contains(&"movetype         3.0".to_owned()),
        "a float"
    );
    assert_eq!(console(&mut run, "edict 999\n"), ["Bad edict number"]);
    assert_eq!(console(&mut run, "edict -1\n"), ["Bad edict number"]);
    let mut run = player(
        &ledge(),
        "self.goalentity = self; self.think = PlayerPreThink;",
        "",
        "void() PlayerPreThink;\n",
    );
    let lines = console(&mut run, "edict 1\n");
    assert!(
        lines.contains(&"goalentity     entity 1".to_owned()),
        "{lines:?}"
    );
    assert!(lines.contains(&"think          PlayerPreThink()".to_owned()));
    let world = console(&mut run, "edict\n");
    assert_eq!(world[1], "EDICT 0:", "no number is edict 0");
    let all = console(&mut run, "edicts\n");
    assert_eq!(all[0], format!("{} entities", run.world.num_edicts()));
}

// spec PHYS-13.7: edictcount counts the edicts in use, with a model, solid and MOVETYPE_STEP
#[test]
fn edictcount_counts_the_edicts() {
    let mut run = connected();
    let lines = console(&mut run, "edictcount\n");
    assert_eq!(lines[0], format!("num_edicts:{:3}", run.world.num_edicts()));
    assert_eq!(lines[1], "active    :  2");
    assert_eq!(
        lines[3], "touch     :  2",
        "the world and the player are solid"
    );
    assert_eq!(lines[4], "step      :  0");
}

// spec PHYS-13.7: profile prints the ten functions that ran the most statements, most first,
// then clears every count
#[test]
fn profile_prints_the_busiest_functions_once() {
    let mut run = connected();
    run.frames(3, 0.1).unwrap();
    let lines = console(&mut run, "profile\n");
    assert!(!lines.is_empty() && lines.len() <= 10, "{lines:?}");
    let counts: Vec<u64> = lines
        .iter()
        .map(|l| l.get(..7).unwrap().trim().parse().unwrap())
        .collect();
    assert!(counts.windows(2).all(|w| w[0] >= w[1]), "most first");
    assert!(
        lines.iter().any(|l| l.ends_with(" PlayerPreThink")),
        "{lines:?}"
    );
    assert!(console(&mut run, "profile\n").is_empty(), "cleared");
}

// spec PHYS-13.7: path prints the search path the host gave
#[test]
fn path_prints_the_search_path() {
    let mut run = with_files(Vec::new());
    assert_eq!(
        console(&mut run, "path\n"),
        [
            "Current search path:",
            "/q/id1",
            "/q/id1/pak0.pak (339 files)"
        ]
    );
}

// spec PHYS-13.8: where the preview host's console differs from id's on purpose: the Exe line,
// a negative edict, map with no name, kick, and the demo and remote-server commands
#[test]
fn the_deliberate_differences_from_ids_host() {
    let mut run = connected();
    assert_eq!(
        console(&mut run, "version\n")[1],
        format!("Exe: QuakeC Studio {}", env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(console(&mut run, "edict -1\n"), ["Bad edict number"]);
    assert!(console(&mut run, "map; kick player\n").is_empty());
    assert_eq!(run.world.take_host_command(), None);
    for name in [
        "demos",
        "startdemos",
        "stopdemo",
        "connect",
        "reconnect",
        "changelevel2",
        "please",
        "viewmodel",
        "viewframe",
        "viewnext",
        "viewprev",
        "mcache",
    ] {
        assert_eq!(
            console(&mut run, &format!("{name}\n")),
            [format!("Unknown command \"{name}\"")]
        );
    }
}

// spec PHYS-13.6: while a host command waits for the host, the buffer does not run, so a
// second one cannot replace it and nothing after it runs on the old level
#[test]
fn a_waiting_host_command_holds_the_buffer() {
    let mut run = connected();
    assert!(console(&mut run, "save s1; quit; echo after\n").is_empty());
    assert!(console(&mut run, "").is_empty(), "still waiting");
    assert_eq!(
        run.world.take_host_command(),
        Some(HostCommand::Save("s1".to_owned()))
    );
    assert!(console(&mut run, "").is_empty());
    assert_eq!(run.world.take_host_command(), Some(HostCommand::Quit));
    assert_eq!(console(&mut run, ""), ["after "]);
}

// spec PHYS-13.6: a frame that stops at a host command runs no server frame and gives back
// its random draw, as id draws once for the command and the frame after it
#[test]
fn a_frame_stopped_for_the_host_keeps_the_random_source() {
    let mut run = connected();
    let before = run.world.builtins.mock.engine().rng;
    let time = run.world.time();
    run.world.console("save s1\n");
    run.frames(1, 0.1).unwrap();
    assert_eq!(run.world.builtins.mock.engine().rng, before);
    assert!(
        (run.world.time() - time).abs() < f64::EPSILON,
        "no server frame ran"
    );
    assert!(run.world.take_host_command().is_some());
    run.frames(1, 0.1).unwrap();
    assert_ne!(
        run.world.builtins.mock.engine().rng,
        before,
        "the next frame draws"
    );
}

// spec CON-4.3: a command is completed first, then a setting, the first by name; an empty or
// unknown prefix completes nothing
#[test]
fn the_console_completes_commands_then_settings() {
    let run = connected();
    assert_eq!(run.world.complete("noc").as_deref(), Some("noclip"));
    assert_eq!(run.world.complete("edict").as_deref(), Some("edict"));
    assert_eq!(run.world.complete("sv_g").as_deref(), Some("sv_gravity"));
    assert_eq!(run.world.complete("s").as_deref(), Some("save"));
    assert_eq!(run.world.complete(""), None);
    assert_eq!(run.world.complete("zz"), None);
}
