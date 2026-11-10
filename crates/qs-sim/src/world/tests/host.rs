// SPDX-License-Identifier: GPL-3.0-or-later

//! The host's part of a frame: the random source, the frame time's double arithmetic and the
//! client's string commands.

use qc_builtins::math::random;
use qc_builtins::mock::Rng;
use qc_vm::convert::float_to_int;

use super::{P, Running, entity, ledge, player, run_on};
use crate::UserCmd;

/// The frame `Host_FilterTime` runs at most often, 1/72 of a second.
const FRAME: f64 = 1.0 / 72.0;

/// Whether the client's messages since the last call hold `text`.
fn printed(run: &mut Running, text: &[u8]) -> bool {
    run.world
        .take_messages()
        .packets
        .iter()
        .any(|packet| packet.windows(text.len()).any(|w| w == text))
}

// spec MOVE-6.1: the random source starts at state 1
// spec MOVE-6.2: each frame draws one value and discards it before its server frame
#[test]
fn the_random_source_starts_at_one_and_each_frame_draws_once() {
    let source = "void() tick = { self.frags = random(); self.nextthink = time + 0.001; };\n\
                  void() ticker = { self.frags = random(); self.think = tick; \
                  self.nextthink = time; };\n";
    let mut run = run_on(&ledge(), source, &entity("ticker", "0 0 0", ""));
    let mut rng = Rng::new(1);
    let draws: Vec<u32> = (0..5).map(|_| rng.next_rand()).collect();
    assert_eq!(run.world.float(2, "frags"), random(draws[0]));
    run.frames(1, FRAME).unwrap();
    assert_eq!(
        run.world.float(2, "frags"),
        random(draws[2]),
        "draw 1 was the frame's"
    );
    run.frames(1, FRAME).unwrap();
    assert_eq!(run.world.float(2, "frags"), random(draws[4]));
}

// spec PHYS-6.5: gravity is taken from the velocity in double and rounded to float once
#[test]
fn gravity_rounds_once() {
    let source = "void() thrown = { self.movetype = 6; self.solid = 2; \
                  setsize(self, '-1 -1 -1', '1 1 1'); setorigin(self, self.origin); \
                  self.velocity = '0 0 -66.666664'; };\n";
    let mut run = run_on(&ledge(), source, &entity("thrown", "50 0 1000", ""));
    run.frames(1, FRAME).unwrap();
    let z = run.world.vector(2, "velocity")[2];
    // -66.666664 - 800/72 in double is -77.777775, which rounds to the float -77.77778;
    // rounding the fall first gives -77.77777.
    assert_eq!(z.to_bits(), (-77.777_78_f32).to_bits(), "{z}");
}

// spec PHYS-9.7: the punch fades by 10*host_frametime taken in double
#[test]
fn the_punch_fades_in_double() {
    let put = "setorigin(self, '0 0 200'); self.punchangle = '1.4444445 0 0';";
    let mut run = player(&ledge(), put, "", "");
    run.world.set_command(UserCmd::default());
    run.frames(1, FRAME).unwrap();
    let punch = run.world.vector(P, "punchangle")[0];
    // 1.4444445 - 10/72 in double rounds to 1.3055557; in float it would be 1.3055556.
    assert_eq!(punch.to_bits(), 1.305_555_7_f32.to_bits(), "{punch}");
}

// spec PHYS-11.1: a string command is read in the next frame, its name in any case
// spec PHYS-11.3: god and notarget toggle their flags, noclip and fly the movetype, each saying
// which way
#[test]
fn the_cheats_toggle_and_say_so() {
    let mut run = player(&ledge(), "setorigin(self, '0 0 200');", "", "");
    let flags = |run: &Running| float_to_int(run.world.float(P, "flags"));
    let before = flags(&run);
    run.world.client_command("GOD");
    assert_eq!(flags(&run), before, "not until the frame");
    run.frames(1, FRAME).unwrap();
    assert_eq!(flags(&run), before | 64);
    assert!(printed(&mut run, b"godmode ON\n"));
    run.world.client_command("god");
    run.world.client_command("notarget");
    run.frames(1, FRAME).unwrap();
    assert_eq!(flags(&run), (before & !64) | 128);
    assert!(printed(&mut run, b"godmode OFF\n"));
    run.world.client_command("noclip");
    run.frames(1, FRAME).unwrap();
    assert_eq!(run.world.float(P, "movetype"), 8.0);
    assert!(printed(&mut run, b"noclip ON\n"));
    run.world.client_command("fly");
    run.frames(1, FRAME).unwrap();
    assert_eq!(run.world.float(P, "movetype"), 5.0);
    run.world.client_command("fly");
    run.frames(1, FRAME).unwrap();
    assert_eq!(run.world.float(P, "movetype"), 3.0);
    assert!(printed(&mut run, b"flymode OFF\n"));
}

// spec PHYS-11.2: in deathmatch the cheats do nothing
// spec PHYS-11.1: a command the prefixes do not allow, or one with a space before it, is not
// run, and one the server does not have does nothing
#[test]
fn deathmatch_and_unknown_commands_change_nothing() {
    let mut run = player(
        &ledge(),
        "setorigin(self, '0 0 200'); deathmatch = 1;",
        "",
        "",
    );
    let flags = run.world.float(P, "flags");
    for command in ["god", "noclip", "give h 5", "godx", ""] {
        run.world.client_command(command);
    }
    run.frames(1, FRAME).unwrap();
    assert_eq!(run.world.float(P, "flags"), flags);
    assert_eq!(run.world.float(P, "movetype"), 3.0);
    assert_eq!(run.world.float(P, "health"), 100.0);
    let mut run = player(&ledge(), "setorigin(self, '0 0 200');", "", "");
    for command in [" god", "noclip;god", "sv_gravity 5"] {
        run.world.client_command(command);
    }
    run.frames(1, FRAME).unwrap();
    assert_eq!(
        float_to_int(run.world.float(P, "flags")) & 64,
        0,
        "no godmode"
    );
    assert_eq!(
        run.world.float(P, "movetype"),
        3.0,
        "noclip;god is one unknown word"
    );
}

// spec PHYS-11.6: give sets ammunition and health to the number and adds a weapon by digit
#[test]
fn give_gives() {
    let mut run = player(
        &ledge(),
        "setorigin(self, '0 0 200'); self.items = 0;",
        "",
        "",
    );
    for command in [
        "give h 250",
        "give s 30",
        "give n 40",
        "give r 5",
        "give c 9",
        "give 3",
    ] {
        run.world.client_command(command);
    }
    run.frames(1, FRAME).unwrap();
    let w = &run.world;
    assert_eq!(w.float(P, "health"), 250.0);
    assert_eq!(
        [
            w.float(P, "ammo_shells"),
            w.float(P, "ammo_nails"),
            w.float(P, "ammo_rockets"),
            w.float(P, "ammo_cells")
        ],
        [30.0, 40.0, 5.0, 9.0]
    );
    assert_eq!(w.float(P, "items"), 2.0, "give 3: IT_SHOTGUN << 1");
}

// spec PHYS-11.7: name sets the netname, cut to 15 bytes; color sets team to bottom + 1
#[test]
fn name_and_color_change_the_player() {
    let mut run = player(&ledge(), "setorigin(self, '0 0 200');", "", "");
    run.world.client_command("name abcdefghijklmnopqrst");
    run.world.client_command("color 3 20");
    run.frames(1, FRAME).unwrap();
    let progs = qc_progs::Progs::read(&run.progs).unwrap();
    let fields = run.world.edict_fields(&progs, P).unwrap();
    let netname = fields.iter().find(|f| f.name == "netname").unwrap();
    assert_eq!(
        netname.value,
        crate::QcValue::String(b"abcdefghijklmno".to_vec())
    );
    // 20 & 15 is 4, at most 13: team 5.
    assert_eq!(run.world.float(P, "team"), 5.0);
}

// spec PHYS-11.8: pause stops the physics and the client's think until it is sent again
#[test]
fn pause_stops_the_game() {
    let mut run = player(&ledge(), "setorigin(self, '0 0 200');", "", "");
    run.world.client_command("pause");
    run.frames(1, FRAME).unwrap();
    let time = run.world.time();
    assert!(printed(&mut run, b"player paused the game\n"));
    run.frames(3, FRAME).unwrap();
    assert_eq!(run.world.time(), time, "the clock stands");
    run.world.client_command("pause");
    run.frames(1, FRAME).unwrap();
    assert!(run.world.time() > time);
}

// spec PHYS-11.9: in single player, with the keys going to the console or a menu, the clock
// stands and the physics and the client's think do not run; with them back on the game, the
// game goes on
#[test]
fn the_console_or_a_menu_pauses_the_game() {
    let mut run = player(&ledge(), "setorigin(self, '0 0 200');", "", "");
    run.frames(1, FRAME).unwrap();
    let time = run.world.time();
    run.world.set_focus(crate::KeyFocus::Elsewhere);
    run.frames(3, FRAME).unwrap();
    assert_eq!(run.world.time(), time, "the clock stands");
    run.world.set_focus(crate::KeyFocus::Game);
    run.frames(1, FRAME).unwrap();
    assert!(run.world.time() > time);
}

// spec PHYS-10.7: commands sent before the client spawns are read once it is active, and the
// signon's own commands spawn it
#[test]
fn commands_before_the_spawn_are_kept() {
    let more = "void() SetNewParms = { parm1 = 7; };\n";
    let put = "self.frags = parm1; self.movetype = 0; setorigin(self, '0 0 100');";
    let progs = super::player_progs(put, "", more);
    let mut run = super::running_from(
        &ledge(),
        progs,
        "",
        Vec::new(),
        crate::LoadOptions::default(),
    );
    run.world.client_command("god");
    run.frames(1, FRAME).unwrap();
    assert!(
        !printed(&mut run, b"godmode"),
        "not read before the client connects"
    );
    run.world.connect_next_frame();
    run.world.client_command("prespawn");
    run.frames(1, FRAME).unwrap();
    assert!(
        printed(&mut run, b"godmode ON\n"),
        "god, sent before the client connected, was read once it was"
    );
    run.world.client_command("spawn ");
    run.frames(1, FRAME).unwrap();
    assert_eq!(run.world.float(P, "frags"), 7.0, "spawned");
    // Host_Spawn_f clears the edict, so the godmode set before is gone.
    assert_eq!(float_to_int(run.world.float(P, "flags")) & 64, 0);
}

// spec PHYS-11.4: kill runs ClientKill on a living player, and only says so on a dead one
#[test]
fn kill_runs_client_kill() {
    let more = "void() ClientKill = { self.frags = self.frags + 1; };\n";
    let mut run = player(&ledge(), "setorigin(self, '0 0 200');", "", more);
    run.world.client_command("kill");
    run.frames(1, FRAME).unwrap();
    assert_eq!(run.world.float(P, "frags"), 1.0);
    let mut dead = player(
        &ledge(),
        "setorigin(self, '0 0 200'); self.health = 0;",
        "",
        more,
    );
    dead.world.client_command("kill");
    dead.frames(1, FRAME).unwrap();
    assert_eq!(dead.world.float(P, "frags"), 0.0);
    assert!(printed(&mut dead, b"Can't suicide -- allready dead!\n"));
}

// spec PHYS-11.5: once spawned, prespawn and spawn do nothing, and say changes no edict
#[test]
fn a_spawned_client_cannot_spawn_again() {
    let put = "self.frags = self.frags + 1; setorigin(self, '0 0 200');";
    let mut run = player(&ledge(), put, "", "");
    let frags = run.world.float(P, "frags");
    for command in ["prespawn", "spawn ", "say hello"] {
        run.world.client_command(command);
    }
    run.frames(1, FRAME).unwrap();
    assert_eq!(
        run.world.float(P, "frags"),
        frags,
        "PutClientInServer ran once"
    );
}
