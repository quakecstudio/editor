// SPDX-License-Identifier: GPL-3.0-or-later

//! Several client slots: connecting, the messages each client is sent, the commands that
//! reach other clients, a level change, dropping and the bot builtins (NET-7).

use qc_builtins::mock::EngineProfile;
use qc_progs::Progs;
use qc_types::Slot;
use qc_vm::convert::float_to_int;

use super::{Running, ledge, player_progs, running_from};
use crate::{ClientId, LoadOptions, MaxPlayers, QcValue, SaveError, SlotKind, UserCmd};

/// The frame `Host_FilterTime` runs at most often, 1/72 of a second.
const FRAME: f64 = 1.0 / 72.0;
/// `svc_updatename`, `svc_updatefrags` and `svc_disconnect` (`WinQuake/protocol.h`).
const SVC_UPDATENAME: u8 = 13;
const SVC_UPDATEFRAGS: u8 = 14;
const SVC_DISCONNECT: u8 = 2;

/// The bot builtins under vkQuake's numbers.
const BOT_BUILTINS: &str = "entity() spawnclient = #454;\n\
    float(entity e) clienttype = #455;\nvoid(entity e) dropclient = #453;\n";

fn id(slot: usize) -> ClientId {
    ClientId::new(slot).unwrap()
}

/// A world of `slots` client slots running the player progs with `put` and `more`, under
/// `profile`, with no client connected.
fn server(slots: i32, put: &str, more: &str, profile: EngineProfile) -> Running {
    let options = LoadOptions {
        maxplayers: MaxPlayers::new(slots),
        profile,
        ..LoadOptions::default()
    };
    running_from(
        &ledge(),
        player_progs(put, "", more),
        "",
        Vec::new(),
        options,
    )
}

/// [`server`] with `count` local clients connected, each in its own slot.
fn players(slots: i32, count: usize, put: &str, more: &str) -> Running {
    let mut run = server(slots, put, more, EngineProfile::Id);
    for n in 0..count {
        assert_eq!(connect(&mut run), Some(id(n)), "the first free slot");
    }
    run
}

fn connect(run: &mut Running) -> Option<ClientId> {
    let progs = Progs::read(&run.progs).unwrap();
    run.world.connect(&progs).unwrap()
}

/// Every packet client `id` was sent since the last call, joined.
fn sent(run: &mut Running, id: ClientId) -> Vec<u8> {
    run.world.take_messages(id).packets.concat()
}

fn holds(bytes: &[u8], text: &[u8]) -> bool {
    bytes.windows(text.len()).any(|w| w == text)
}

/// Client `id`'s string command `text`, read in the next frame.
fn says(run: &mut Running, id: ClientId, text: &str) {
    run.world.client_command(id, text);
    run.frames(1, FRAME).unwrap();
}

// spec NET-7.1: the slots are 1 to MAX_SCOREBOARD, a count below 1 being 1 and one above 16
// being 16; one is single player
#[test]
fn the_slots_are_one_to_sixteen() {
    assert_eq!(MaxPlayers::new(-3).get(), 1);
    assert_eq!(MaxPlayers::new(0).get(), 1);
    assert_eq!(MaxPlayers::new(1), MaxPlayers::SINGLE);
    assert!(!MaxPlayers::SINGLE.is_multiplayer());
    assert!(MaxPlayers::new(2).is_multiplayer());
    assert_eq!(MaxPlayers::new(16).get(), 16);
    assert_eq!(MaxPlayers::new(17).get(), 16);
    assert_eq!(MaxPlayers::default(), MaxPlayers::SINGLE);
    let run = server(4, "", "", EngineProfile::Id);
    assert_eq!(run.world.maxplayers().get(), 4);
}

// spec NET-7.2: slot i is edict i + 1, and the first edict after the slots is the first a
// spawn takes
#[test]
fn slot_i_is_edict_i_plus_one() {
    assert_eq!(ClientId::FIRST.edict(), 1);
    assert_eq!(id(3).edict(), 4);
    assert_eq!(id(3).slot(), 3);
    assert_eq!(ClientId::of_edict(0), None, "the world is no slot");
    assert_eq!(ClientId::of_edict(1), Some(ClientId::FIRST));
    assert_eq!(ClientId::of_edict(16), Some(id(15)));
    assert_eq!(ClientId::of_edict(17), None);
    assert_eq!(ClientId::new(16), None);
    let run = server(4, "", "", EngineProfile::Id);
    assert_eq!(run.world.machine.edicts().num_edicts(), 5, "maxclients + 1");
}

// spec NET-7.4: each new client takes the first slot not active; with every slot active the
// world refuses one; a dropped slot is taken again
#[test]
fn clients_take_the_first_free_slot() {
    let mut run = players(3, 3, "", "");
    assert_eq!(connect(&mut run), None, "every slot active");
    let progs = Progs::read(&run.progs).unwrap();
    run.world.disconnect(&progs, id(1)).unwrap();
    assert!(!run.world.is_spawned(id(1)));
    assert_eq!(connect(&mut run), Some(id(1)));
    assert!(run.world.is_spawned(id(1)));
}

// spec NET-7.4: only the first client sends the host's _cl_name; another keeps the name the
// connection gave it, unconnected, and a slot never connected has none
// spec NET-7.5: a client's spawn message names every slot, and its colormap is its own edict
#[test]
fn the_spawn_message_names_every_slot() {
    let mut run = players(4, 1, "", "");
    says(&mut run, ClientId::FIRST, "name alice");
    drop(sent(&mut run, ClientId::FIRST));
    assert_eq!(connect(&mut run), Some(id(1)));
    let second = sent(&mut run, id(1));
    let mut alice = vec![SVC_UPDATENAME, 0];
    alice.extend_from_slice(b"alice\0");
    let mut unconnected = vec![SVC_UPDATENAME, 1];
    unconnected.extend_from_slice(b"unconnected\0");
    assert!(holds(&second, &alice));
    assert!(holds(&second, &unconnected));
    assert!(holds(&second, &[SVC_UPDATENAME, 2, 0]), "never connected");
    assert_eq!(run.world.float(2, "colormap"), 2.0);
    assert_eq!(run.world.float(1, "colormap"), 1.0);
}

// spec NET-1.4: a change in any slot's frags goes to every active client with its slot byte
#[test]
fn frags_go_to_every_client() {
    let mut run = players(4, 2, "", "");
    run.frames(1, FRAME).unwrap();
    drop(sent(&mut run, ClientId::FIRST));
    drop(sent(&mut run, id(1)));
    run.world.set_float_field(2, "frags", 3.0);
    run.frames(1, FRAME).unwrap();
    let update = [SVC_UPDATEFRAGS, 1, 3, 0];
    assert!(holds(&sent(&mut run, ClientId::FIRST), &update));
    assert!(holds(&sent(&mut run, id(1)), &update));
    run.frames(1, FRAME).unwrap();
    assert!(!holds(&sent(&mut run, id(1)), &update), "sent once");
}

// spec NET-1.6: each active client is sent its own datagram, its own client data in it
#[test]
fn each_client_is_sent_its_own_datagram() {
    let mut run = players(4, 2, "", "");
    run.frames(1, FRAME).unwrap();
    drop(sent(&mut run, ClientId::FIRST));
    drop(sent(&mut run, id(1)));
    // svc_clientdata's health is a short (NET-4.4): 12345 and 23456.
    run.world.set_float_field(1, "health", 12345.0);
    run.world.set_float_field(2, "health", 23456.0);
    run.frames(1, FRAME).unwrap();
    let first = sent(&mut run, ClientId::FIRST);
    let second = sent(&mut run, id(1));
    let (own, other) = ([0x39, 0x30], [0xa0, 0x5b]);
    assert!(holds(&first, &own) && !holds(&first, &other));
    assert!(holds(&second, &other) && !holds(&second, &own));
}

// spec NET-7.6: say reaches every spawned client as "^A<name>: <text>"; a quoted text loses
// its quotes; say_team with teamplay reaches only the sender's team
#[test]
fn say_reaches_every_client_and_say_team_the_team() {
    let mut run = players(4, 3, "", "");
    says(&mut run, id(1), "say \"hello all\"");
    for n in 0..3 {
        assert!(
            holds(&sent(&mut run, id(n)), b"\x01unconnected: hello all\n"),
            "{n}"
        );
    }
    run.world.set_float_field(1, "team", 1.0);
    run.world.set_float_field(2, "team", 1.0);
    run.world.set_float_field(3, "team", 2.0);
    assert!(run.world.set_setting("teamplay", 1.0));
    says(&mut run, ClientId::FIRST, "say_team go");
    assert!(holds(&sent(&mut run, ClientId::FIRST), b"player: go\n"));
    assert!(holds(&sent(&mut run, id(1)), b"player: go\n"));
    assert!(
        !holds(&sent(&mut run, id(2)), b"player: go\n"),
        "another team"
    );
    assert!(run.world.set_setting("teamplay", 0.0));
    says(&mut run, ClientId::FIRST, "say_team all");
    assert!(
        holds(&sent(&mut run, id(2)), b"player: all\n"),
        "no teamplay"
    );
}

// spec NET-7.6: a say is cut to fit 64 bytes with its line feed and terminator
#[test]
fn a_long_say_is_cut() {
    let mut run = players(2, 1, "", "");
    let long = "x".repeat(100);
    says(&mut run, ClientId::FIRST, &format!("say {long}"));
    let text = sent(&mut run, ClientId::FIRST);
    // "\x01player: " is 9 bytes, so 62 - 9 = 53 of the x's.
    let kept = format!("\x01player: {}\n", "x".repeat(53));
    assert!(holds(&text, kept.as_bytes()));
    assert!(!holds(&text, "x".repeat(54).as_bytes()));
}

// spec NET-7.6: tell reaches only the client its name names, compared case-insensitively
#[test]
fn tell_reaches_one_client() {
    let mut run = players(4, 3, "", "");
    says(&mut run, id(2), "name Bob");
    for n in 0..3 {
        drop(sent(&mut run, id(n)));
    }
    says(&mut run, ClientId::FIRST, "tell bob psst");
    // `Cmd_Args` is the whole rest, the name too.
    assert!(holds(&sent(&mut run, id(2)), b"player: bob psst\n"));
    assert!(!holds(&sent(&mut run, id(1)), b"psst"));
    says(&mut run, ClientId::FIRST, "tell nobody psst");
    assert!(!holds(&sent(&mut run, id(2)), b"psst"));
}

// spec NET-7.6: a rename is reported on the host's console; the first name is not
#[test]
fn a_rename_is_reported_on_the_console() {
    let mut run = players(2, 1, "", "");
    drop(run.world.take_console_output());
    says(&mut run, ClientId::FIRST, "name alice");
    let lines = run.world.take_console_output();
    assert!(
        lines.contains(&b"player renamed to alice".to_vec()),
        "{lines:?}"
    );
    says(&mut run, ClientId::FIRST, "name alice");
    assert!(run.world.take_console_output().is_empty(), "the same name");
}

// spec NET-7.6: status from a client lists every active slot with its address
// spec PHYS-13.10: status on the console lists every slot, and kick on the console drops the
// client it names
#[test]
fn status_lists_every_client() {
    let mut run = players(4, 2, "", "");
    says(&mut run, id(1), "status");
    let text = sent(&mut run, id(1));
    assert!(holds(&text, b"players: 2 active (4 max)\n\n"));
    assert!(holds(&text, b"#1  player             "));
    assert!(holds(&text, b"#2  unconnected        "));
    assert!(holds(&text, b"   LOCAL\n"));
    run.world.console("status\n");
    run.frames(1, FRAME).unwrap();
    let lines = run.world.take_console_output();
    assert!(
        lines.contains(&b"players: 2 active (4 max)".to_vec()),
        "{lines:?}"
    );
    run.world.console("kick # 2\n");
    run.frames(1, FRAME).unwrap();
    assert!(!run.world.is_spawned(id(1)), "the console kicked slot 2");
}

// spec NET-7.6: ping prints each client's mean ping, the time back from the last datagram it
// read, which over the loopback is the server's own time: 0
#[test]
fn ping_lists_every_client() {
    let mut run = players(4, 2, "", "");
    for _ in 0..20 {
        run.world.set_command(ClientId::FIRST, UserCmd::default());
        run.frames(1, 0.1).unwrap();
    }
    says(&mut run, ClientId::FIRST, "ping");
    let text = sent(&mut run, ClientId::FIRST);
    assert!(holds(&text, b"Client ping times:\n"));
    let at = text
        .windows(18)
        .position(|w| w == b"Client ping times:")
        .unwrap();
    let tail = String::from_utf8_lossy(&text[at..]).into_owned();
    assert!(tail.contains("   0 player\n"), "{tail:?}");
}

// spec NET-7.6: kick by name or number drops another client and tells it why; a client cannot
// kick itself; in deathmatch a client cannot kick at all
#[test]
fn kick_drops_another_client() {
    let mut run = players(4, 3, "", "");
    says(&mut run, ClientId::FIRST, "kick player");
    assert!(
        run.world.is_spawned(ClientId::FIRST),
        "the first named is the sender"
    );
    says(&mut run, ClientId::FIRST, "kick # 2 go away");
    let kicked = sent(&mut run, id(1));
    assert!(holds(&kicked, b"Kicked by player: go away"), "{kicked:?}");
    assert!(holds(&kicked, &[SVC_DISCONNECT]));
    assert!(!run.world.is_spawned(id(1)));
    says(&mut run, ClientId::FIRST, "kick # 0.5");
    assert!(
        run.world.is_spawned(ClientId::FIRST),
        "0.5 - 1 truncates to slot 0, the sender"
    );
    let mut dm = server(4, "", "", EngineProfile::Id);
    dm.set_raw_global("deathmatch", 1.0_f32.to_bits().cast_signed());
    assert_eq!(connect(&mut dm), Some(ClientId::FIRST));
    assert_eq!(connect(&mut dm), Some(id(1)));
    says(&mut dm, ClientId::FIRST, "kick # 2");
    assert!(
        dm.world.is_spawned(id(1)),
        "no client is privileged in deathmatch"
    );
}

// spec NET-7.7: with more than one slot the console or a menu does not pause the game
#[test]
fn the_console_does_not_pause_a_multiplayer_game() {
    let mut run = players(2, 1, "", "");
    run.frames(1, FRAME).unwrap();
    let time = run.world.time();
    run.world.set_focus(crate::KeyFocus::Elsewhere);
    run.frames(2, FRAME).unwrap();
    assert!(run.world.time() > time);
}

// spec NET-7.7: each client in slot order runs its commands, then its move and its think; a
// client's pause stops the think of those after it in that frame
#[test]
fn clients_run_in_slot_order() {
    let put = "self.movetype = 0;";
    let more = "float order; void() ClientKill = { order = order * 10 + self.colormap; };\n";
    let mut run = players(3, 3, put, more);
    run.world.client_command(id(2), "kill");
    run.world.client_command(ClientId::FIRST, "kill");
    run.world.client_command(id(1), "kill");
    run.frames(1, FRAME).unwrap();
    assert_eq!(global_float(&run, "order"), 123.0);
}

// spec NET-7.8: a level change keeps each active client's SetChangeParms and nothing for a
// slot not active; each client of the next level takes its slot's parms
#[test]
fn a_level_change_carries_each_clients_parms() {
    let put = "self.frags = parm1;";
    let more = "void() SetChangeParms = { parm1 = self.colormap * 10; };\n\
        void() SetNewParms = { parm1 = 5; };\n";
    let mut run = players(3, 2, put, more);
    let progs = Progs::read(&run.progs).unwrap();
    let kept = run.world.save_spawn_parms(&progs).unwrap();
    assert_eq!(kept.client_parms(ClientId::FIRST).map(|p| p[0]), Some(10.0));
    assert_eq!(kept.client_parms(id(1)).map(|p| p[0]), Some(20.0));
    assert_eq!(kept.client_parms(id(2)), None, "not active");
    assert_eq!(kept.parms()[0], 10.0);
    let options = LoadOptions {
        maxplayers: MaxPlayers::new(3),
        carried: Some(kept),
        ..LoadOptions::default()
    };
    let mut next = running_from(&ledge(), run.progs.clone(), "", Vec::new(), options);
    for _ in 0..3 {
        connect(&mut next);
    }
    assert_eq!(next.world.float(1, "frags"), 10.0);
    assert_eq!(next.world.float(2, "frags"), 20.0);
    assert_eq!(
        next.world.float(3, "frags"),
        5.0,
        "SetNewParms' for a new client"
    );
}

// spec NET-7.9: a dropped client's ClientDisconnect runs with self its edict, self put back;
// its body stays; every other client is told its slot is empty
#[test]
fn a_dropped_client_is_gone_from_every_scoreboard() {
    let more = "float gone; entity before; \
        void() ClientDisconnect = { gone = self.colormap; };\n";
    let mut run = players(4, 2, "", more);
    let bytes = run.progs.clone();
    let progs = Progs::read(&bytes).unwrap();
    drop(sent(&mut run, ClientId::FIRST));
    run.world.disconnect(&progs, id(1)).unwrap();
    assert_eq!(global_float(&run, "gone"), 2.0);
    assert!(run.world.in_use(2), "the body stays around");
    run.frames(1, FRAME).unwrap();
    let first = sent(&mut run, ClientId::FIRST);
    assert!(holds(&first, &[SVC_UPDATENAME, 1, 0]));
    assert!(holds(&first, &[SVC_UPDATEFRAGS, 1, 0, 0]));
    assert!(holds(&sent(&mut run, id(1)), &[SVC_DISCONNECT]));
    run.world.disconnect(&progs, id(3)).unwrap();
    assert_eq!(
        global_float(&run, "gone"),
        2.0,
        "an empty slot is left as it is"
    );
}

// spec NET-7.10: a server of more than one slot is not saved; with one, a dead client stops
// the save
#[test]
fn a_multiplayer_game_is_not_saved() {
    let run = players(2, 1, "", "");
    let progs = Progs::read(&run.progs).unwrap();
    assert_eq!(run.world.save_game(&progs), Err(SaveError::Multiplayer));
    let mut single = players(1, 1, "", "");
    single.world.set_float_field(1, "health", 0.0);
    assert_eq!(single.world.save_game(&progs), Err(SaveError::DeadPlayer));
}

// spec NET-7.11: setspawnparms copies the client's own spawn parms to parm1 to parm16
#[test]
fn setspawnparms_copies_the_slots_parms() {
    let more = "void(entity e) setspawnparms = #78;\n\
        void() SetNewParms = { parm1 = time + 3; };\n\
        void() ClientKill = { parm1 = 0; setspawnparms(self); self.frags = parm1; };\n";
    let mut run = players(2, 1, "", more);
    says(&mut run, ClientId::FIRST, "kill");
    assert_eq!(
        run.world.float(1, "frags"),
        4.0,
        "SetNewParms ran at time 1"
    );
}

// spec BUILTIN-18.45: spawnclient takes the last free slot as a bot, spawned, its edict
// cleared and named; clienttype says 2 for it; with every slot taken it gives the world
// spec NET-7.3: a bot is sent nothing, reads no move, and thinks with its v_angle
#[test]
fn spawnclient_makes_a_bot() {
    let more = format!(
        "{BOT_BUILTINS}entity bot; entity third; float botkind; float realkind;\n\
         void() StartFrame = {{ if (!bot) {{ bot = spawnclient(); third = spawnclient(); \
         bot.v_angle = '0 90 0'; bot.movetype = 3; bot.health = 100; botkind = clienttype(bot); \
         realkind = clienttype(world); }} }};\n"
    );
    let mut run = server(3, "", &more, EngineProfile::VkQuake);
    assert_eq!(connect(&mut run), Some(ClientId::FIRST));
    run.frames(2, FRAME).unwrap();
    assert_eq!(global_entity(&run, "bot"), 3, "the last slot");
    assert_eq!(global_entity(&run, "third"), 2);
    assert_eq!(global_float(&run, "botkind"), 2.0, "CLIENTTYPE_BOT");
    assert_eq!(global_float(&run, "realkind"), 3.0, "not a client");
    assert!(run.world.is_spawned(id(2)));
    assert_eq!(run.world.float(3, "colormap"), 3.0);
    assert_eq!(
        (
            run.world.vector(3, "v_angle"),
            run.world.float(3, "movetype")
        ),
        ([0.0, 90.0, 0.0], 3.0)
    );
    assert_eq!(run.world.vector(3, "angles")[1], 90.0, "its v_angle");
    assert!(sent(&mut run, id(2)).is_empty(), "nothing goes to a bot");
    assert_eq!(connect(&mut run), None, "the bots took the free slots");
    let full = format!(
        "{BOT_BUILTINS}entity got;\n\
         void() StartFrame = {{ if (!got) got = spawnclient(); }};\n"
    );
    let mut alone = server(1, "", &full, EngineProfile::VkQuake);
    connect(&mut alone);
    alone.frames(1, FRAME).unwrap();
    assert_eq!(
        global_entity(&alone, "got"),
        0,
        "the world, every slot taken"
    );
    let bytes = run.progs.clone();
    let status = run.world.status_lines(&Progs::read(&bytes).unwrap());
    assert!(status.contains(&b"   BOT\n".to_vec()));
    assert_eq!(slot_kind(&run, id(2)), Some(SlotKind::Bot));
}

// spec BUILTIN-18.46: dropclient drops an active client slot and passes over anything else
#[test]
fn dropclient_drops_a_bot() {
    let more = format!(
        "{BOT_BUILTINS}entity bot; float done; float gone;\n\
         void() ClientDisconnect = {{ gone = gone + 1; }};\n\
         void() StartFrame = {{ if (!bot) bot = spawnclient(); \
         else if (!done) {{ dropclient(bot); dropclient(bot); dropclient(world); done = 1; }} }};\n"
    );
    let mut run = server(2, "", &more, EngineProfile::VkQuake);
    connect(&mut run);
    run.frames(3, FRAME).unwrap();
    assert_eq!(global_float(&run, "gone"), 1.0, "dropped once");
    assert!(!run.world.is_spawned(id(1)));
}

/// The progs global `name`'s value.
fn global(run: &Running, name: &str) -> QcValue {
    let progs = Progs::read(&run.progs).unwrap();
    run.world
        .named_globals(&progs)
        .into_iter()
        .find(|g| g.name == name)
        .map(|g| g.value)
        .unwrap()
}

fn global_float(run: &Running, name: &str) -> f32 {
    match global(run, name) {
        QcValue::Float(value) => value,
        other => panic!("{name} is {other:?}"),
    }
}

fn global_entity(run: &Running, name: &str) -> usize {
    match global(run, name) {
        QcValue::Entity(n) => n,
        other => panic!("{name} is {other:?}"),
    }
}

fn slot_kind(run: &Running, id: ClientId) -> Option<SlotKind> {
    run.world
        .builtins
        .messages
        .clients
        .get(id)
        .filter(|slot| slot.active)
        .map(|slot| slot.kind)
}

// spec NET-7.4: SV_ConnectClient makes no baselines, so a bot spawnclient makes while the world
// settles leaves the signon with the one set the settling writes
#[test]
fn a_bot_made_while_settling_writes_no_baselines() {
    let signon = |more: &str| {
        let mut run = server(2, "", more, EngineProfile::VkQuake);
        let progs = Progs::read(&run.progs).unwrap();
        run.world.settle(&progs).unwrap();
        run.world.builtins.messages.signon.bytes().to_vec()
    };
    let with_bot = format!(
        "{BOT_BUILTINS}entity bot;\nvoid() StartFrame = {{ if (!bot) bot = spawnclient(); }};\n"
    );
    assert_eq!(signon(&with_bot), signon(""));
}

// spec NET-7.7: a client its own command drops reads no more commands and does not move or
// think that frame, and what it left unread is not run by the next client in its slot
#[test]
fn a_client_its_command_drops_stops() {
    let more = format!("{BOT_BUILTINS}void() ClientKill = {{ dropclient(self); }};\n");
    let mut run = server(3, "", &more, EngineProfile::VkQuake);
    for _ in 0..2 {
        connect(&mut run);
    }
    let walk = UserCmd {
        forward: 200.0,
        ..UserCmd::default()
    };
    run.world.set_command(id(1), walk);
    run.world.client_command(id(1), "kill");
    run.frames(1, FRAME).unwrap();
    assert!(!run.world.is_spawned(id(1)));
    assert_eq!(
        run.world.vector(2, "velocity"),
        [0.0; 3],
        "no think after the drop"
    );
    assert_eq!(connect(&mut run), Some(id(1)));
    run.world.client_command(id(1), "kill");
    run.world.client_command(id(1), "god");
    run.frames(1, FRAME).unwrap();
    assert!(!run.world.is_spawned(id(1)), "dropped again, god unread");
    // Slot 0 kicks slot 1 before slot 1's queued `god` is read.
    assert_eq!(connect(&mut run), Some(id(1)));
    run.world.client_command(id(1), "god");
    run.world.client_command(ClientId::FIRST, "kick # 2");
    run.frames(1, FRAME).unwrap();
    assert!(!run.world.is_spawned(id(1)));
    assert_eq!(connect(&mut run), Some(id(1)));
    run.frames(1, FRAME).unwrap();
    let godmode = 64;
    let flags = float_to_int(run.world.float(2, "flags"));
    assert_eq!(flags & godmode, 0, "the kicked client's god was not read");
    assert!(!holds(&sent(&mut run, id(1)), b"godmode"));
}

// spec NET-1.4: old_frags is compared with the edict's frags as a float, so a fractional count
// is sent again every frame
#[test]
fn fractional_frags_are_sent_every_frame() {
    let mut run = players(2, 1, "", "");
    run.world.set_float_field(1, "frags", 0.5);
    for _ in 0..2 {
        run.frames(1, FRAME).unwrap();
        assert!(holds(
            &sent(&mut run, ClientId::FIRST),
            &[SVC_UPDATEFRAGS, 0, 0, 0]
        ));
    }
}

// spec BUILTIN-18.45: in a loaded game a bot takes the slot's saved parms, as SV_ConnectClient
// does under sv.loadgame, and SetNewParms does not run
#[test]
fn a_bot_in_a_loaded_game_keeps_the_saved_parms() {
    let more = format!(
        "{BOT_BUILTINS}entity bot; float fresh;\n\
         void() SetNewParms = {{ fresh = fresh + 1; }};\n\
         void() StartFrame = {{ if (!bot) bot = spawnclient(); }};\n"
    );
    let mut run = server(2, "", &more, EngineProfile::VkQuake);
    run.world.builtins.loaded = true;
    let saved = Slot::from_f32(9.0);
    run.world
        .builtins
        .messages
        .clients
        .get_mut(id(1))
        .unwrap()
        .spawn_parms[0] = saved;
    run.frames(1, FRAME).unwrap();
    assert_eq!(global_entity(&run, "bot"), 2);
    assert_eq!(global_float(&run, "fresh"), 0.0);
    let slot = run.world.builtins.messages.clients.get(id(1)).unwrap();
    assert_eq!(slot.spawn_parms[0], saved);
}

/// What the console printed for `text`, run at once, a line each.
fn typed(run: &mut Running, text: &str) -> Vec<String> {
    run.world.console(text);
    run.world.execute_console(&Progs::read(&run.progs).unwrap());
    run.world
        .take_console_output()
        .iter()
        .map(|line| String::from_utf8_lossy(line).into_owned())
        .collect()
}

/// Slot `id`'s name.
fn slot_name(run: &Running, id: ClientId) -> Vec<u8> {
    run.world
        .builtins
        .messages
        .clients
        .get(id)
        .unwrap()
        .name
        .clone()
}

// spec BOT-1.2: addbot brings a bot the host drives into the first free slot through the mod's
// own ClientConnect and PutClientInServer, spawned; with every slot taken it says so
#[test]
fn addbot_brings_a_bot_in_through_the_mods_player_code() {
    let more = "float connected;\nvoid() ClientConnect = { connected = connected + 1; };\n";
    let mut run = players(3, 1, "", more);
    assert_eq!(typed(&mut run, "addbot\n"), Vec::<String>::new());
    assert_eq!(slot_kind(&run, id(1)), Some(SlotKind::HostBot));
    assert!(run.world.is_spawned(id(1)));
    assert_eq!(
        global_float(&run, "connected"),
        2.0,
        "the player's and the bot's"
    );
    assert_eq!(run.world.float(2, "health"), 100.0, "PutClientInServer ran");
    assert!(sent(&mut run, id(1)).is_empty(), "nothing goes to a bot");
    typed(&mut run, "addbot\n");
    assert_eq!(
        typed(&mut run, "addbot\n"),
        vec!["addbot: every client slot is taken (3 max)"]
    );
    assert_eq!(run.world.host_bots(), vec![id(1), id(2)]);
    let progs = Progs::read(&run.progs).unwrap();
    assert!(
        run.world
            .status_lines(&progs)
            .contains(&b"   BOT\n".to_vec())
    );
}

// spec BOT-1.3: an unnamed bot takes the first listed name no slot has, a named one its name
// without quotes cut to 15 bytes, and each slot's bot its slot's colours
#[test]
fn bots_are_named_from_the_list_and_coloured_by_slot() {
    let mut run = players(5, 1, "", "");
    typed(
        &mut run,
        "addbot\naddbot 2 Birch\naddbot 1 a\"verylongbotname1\"\n",
    );
    assert_eq!(slot_name(&run, id(1)), b"Ash");
    assert_eq!(slot_name(&run, id(2)), b"Birch");
    assert_eq!(slot_name(&run, id(3)), b"averylongbotnam");
    typed(&mut run, "addbot\n");
    assert_eq!(slot_name(&run, id(4)), b"Cedar", "Ash and Birch are taken");
    let colors = |run: &Running, n| {
        run.world
            .builtins
            .messages
            .clients
            .get(id(n))
            .unwrap()
            .colors
    };
    assert_eq!(colors(&run, 1), 13 * 16 + 13);
    assert_eq!(colors(&run, 2), 12 * 16 + 12);
    assert_eq!(run.world.float(2, "team"), 14.0, "the pants colour + 1");
}

// spec BOT-1.4, BOT-1.5: a bot's skill is the number given, rounded and kept to 0 to 3, else
// the skill setting's; each bot brought in or dropped is told once, in order
#[test]
fn a_bots_skill_is_given_or_the_settings() {
    use crate::{BotChange, BotSkill};
    assert_eq!(BotSkill::from_setting(-1.0), BotSkill::Easy);
    assert_eq!(BotSkill::from_setting(0.4), BotSkill::Easy);
    assert_eq!(BotSkill::from_setting(0.5), BotSkill::Normal);
    assert_eq!(BotSkill::from_setting(2.0), BotSkill::Hard);
    assert_eq!(BotSkill::from_setting(9.0), BotSkill::Nightmare);
    assert_eq!(BotSkill::ALL.map(BotSkill::level), [0, 1, 2, 3]);
    let mut run = players(4, 1, "", "");
    typed(&mut run, "skill 2\naddbot\naddbot 7\naddbot Moss\n");
    let joined: Vec<BotSkill> = run
        .world
        .take_bot_changes()
        .into_iter()
        .map(|change| match change {
            BotChange::Joined { skill, .. } => skill,
            BotChange::Left(_) => panic!("none left"),
        })
        .collect();
    assert_eq!(
        joined,
        [BotSkill::Hard, BotSkill::Nightmare, BotSkill::Hard]
    );
    assert_eq!(slot_name(&run, id(3)), b"Moss");
    assert!(run.world.take_bot_changes().is_empty(), "taken once");
}

// spec BOT-1.2: removebot drops the bot named, case apart, or the one in the highest slot, and
// says when there is none; it never drops a local client
#[test]
fn removebot_drops_a_bot_by_name_or_the_last() {
    use crate::BotChange;
    let mut run = players(4, 1, "", "");
    typed(&mut run, "addbot\naddbot\n");
    drop(run.world.take_bot_changes());
    assert_eq!(typed(&mut run, "removebot ASH\n"), Vec::<String>::new());
    assert_eq!(run.world.take_bot_changes(), vec![BotChange::Left(id(1))]);
    assert_eq!(
        typed(&mut run, "removebot player\n"),
        vec!["removebot: no bot named player"]
    );
    typed(&mut run, "removebot\n");
    assert_eq!(run.world.take_bot_changes(), vec![BotChange::Left(id(2))]);
    assert_eq!(typed(&mut run, "removebot\n"), vec!["removebot: no bots"]);
    assert!(run.world.is_spawned(ClientId::FIRST));
}

// spec BOT-1.1, BOT-1.6: a bot the host drives moves by the host's command, read as a local
// client's; its mind reads every edict in use, the world first
#[test]
fn a_host_bot_moves_by_the_hosts_command() {
    let mut run = players(
        2,
        1,
        "self.origin = '0 0 24'; self.origin_x = self.colormap * 40 - 200; \
         setorigin (self, self.origin);",
        "",
    );
    let bytes = run.progs.clone();
    let progs = Progs::read(&bytes).unwrap();
    let bot = run
        .world
        .add_bot(&progs, crate::BotSkill::Normal, None)
        .unwrap()
        .unwrap();
    run.frames(10, FRAME).unwrap();
    let before = run.world.vector(bot.edict(), "origin");
    run.world.set_command(
        bot,
        UserCmd {
            angles: [0.0, 0.0, 0.0],
            forward: 200.0,
            ..UserCmd::default()
        },
    );
    run.frames(10, FRAME).unwrap();
    let after = run.world.vector(bot.edict(), "origin");
    assert!(after[0] > before[0] + 10.0, "{before:?} to {after:?}");
    let bodies = run.world.bodies(&progs);
    let body = bodies.iter().find(|b| b.number == bot.edict()).unwrap();
    assert_eq!(body.origin, after);
    assert_eq!(body.health, 100.0);
    assert_eq!(bodies.first().unwrap().number, 0, "the world first");
}

// spec BOT-1.2, BOT-1.3: a slot's kind and name are read while it is active, and none after
#[test]
fn a_bots_slot_tells_its_kind_and_name() {
    let mut run = players(3, 1, "", "");
    typed(&mut run, "addbot 1 Moss\n");
    assert_eq!(run.world.slot_kind(ClientId::FIRST), Some(SlotKind::Local));
    assert_eq!(run.world.slot_kind(id(1)), Some(SlotKind::HostBot));
    assert_eq!(run.world.client_name(id(1)), Some(b"Moss".to_vec()));
    assert_eq!(run.world.slot_kind(id(2)), None);
    assert_eq!(run.world.client_name(id(2)), None);
    typed(&mut run, "removebot\n");
    assert_eq!(run.world.slot_kind(id(1)), None);
}

// spec BOT-1.4: only a first argument of digits alone is the skill; one that starts with a
// digit is the name
#[test]
fn a_name_that_starts_with_a_digit_is_a_name() {
    let mut run = players(3, 1, "", "");
    typed(&mut run, "skill 3\naddbot 2Fast\n");
    assert_eq!(slot_name(&run, id(1)), b"2Fast");
    let skills: Vec<crate::BotChange> = run.world.take_bot_changes();
    assert_eq!(
        skills,
        [crate::BotChange::Joined {
            id: id(1),
            skill: crate::BotSkill::Nightmare
        }]
    );
}

// spec BOT-1.7: a bot comes into the next level in the slot it had, with that slot's carried
// parms, and not when the slot is taken
#[test]
fn a_bot_rejoins_in_its_own_slot() {
    let mut run = players(4, 1, "", "");
    let bytes = run.progs.clone();
    let progs = Progs::read(&bytes).unwrap();
    assert!(
        run.world
            .rejoin_bot(&progs, id(3), crate::BotSkill::Hard, b"Cedar")
            .unwrap()
    );
    assert_eq!(slot_kind(&run, id(3)), Some(SlotKind::HostBot));
    assert_eq!(slot_name(&run, id(3)), b"Cedar");
    assert_eq!(
        slot_kind(&run, id(1)),
        None,
        "the slots before it stay free"
    );
    assert!(
        !run.world
            .rejoin_bot(&progs, ClientId::FIRST, crate::BotSkill::Hard, b"Ash")
            .unwrap(),
        "the player's slot"
    );
    assert_eq!(
        run.world.take_bot_changes(),
        [crate::BotChange::Joined {
            id: id(3),
            skill: crate::BotSkill::Hard
        }]
    );
}

// spec BOT-1.1: clienttype answers CLIENTTYPE_BOT for a bot the host drives
#[test]
fn clienttype_answers_bot_for_a_host_bot() {
    let more = format!(
        "{BOT_BUILTINS}float kind;\n\
         void() StartFrame = {{ kind = clienttype(nextent(world)); }};\n"
    );
    let more = more.replace(
        "float kind;",
        "entity(entity e) nextent = #47;\nfloat kind;",
    );
    let mut run = server(2, "", &more, EngineProfile::VkQuake);
    connect(&mut run);
    typed(&mut run, "addbot\n");
    run.frames(1, FRAME).unwrap();
    assert_eq!(
        global_float(&run, "kind"),
        1.0,
        "the player, edict 1, is real"
    );
    let bot_kind = format!(
        "{BOT_BUILTINS}entity(entity e) nextent = #47;\nfloat kind;\n\
         void() StartFrame = {{ kind = clienttype(nextent(nextent(world))); }};\n"
    );
    let mut run = server(2, "", &bot_kind, EngineProfile::VkQuake);
    connect(&mut run);
    typed(&mut run, "addbot\n");
    run.frames(1, FRAME).unwrap();
    assert_eq!(global_float(&run, "kind"), 2.0, "the bot, edict 2");
}

// spec NET-7.12: a line another local player's keys run forwards to that player's slot, and a
// line typed after it is the first client's again
#[test]
fn a_local_players_line_forwards_to_its_own_slot() {
    let mut run = players(4, 2, "", "");
    let progs = Progs::read(&run.progs).unwrap();
    run.world.execute_console_as(&progs, id(1), "say hello");
    let second = run.world.builtins.messages.clients.get(id(1)).unwrap();
    assert_eq!(second.commands, vec![b"say hello".to_vec()]);
    let first = run
        .world
        .builtins
        .messages
        .clients
        .get(ClientId::FIRST)
        .unwrap();
    assert!(first.commands.is_empty(), "the first client sent nothing");
    run.world.console("say again\n");
    run.world.execute_console(&progs);
    let first = run
        .world
        .builtins
        .messages
        .clients
        .get(ClientId::FIRST)
        .unwrap();
    assert_eq!(first.commands, vec![b"say again".to_vec()]);
    run.world
        .execute_console_as(&progs, ClientId::FIRST, "say third");
    let first = run
        .world
        .builtins
        .messages
        .clients
        .get(ClientId::FIRST)
        .unwrap();
    assert_eq!(
        first.commands.len(),
        2,
        "the first player's own console is the host's"
    );
}

// spec NET-7.12: a line run as a slot that is not active is refused as not connected
#[test]
fn a_local_players_line_needs_its_slot_active() {
    let mut run = players(4, 1, "", "");
    let progs = Progs::read(&run.progs).unwrap();
    drop(run.world.take_console_output());
    run.world.execute_console_as(&progs, id(2), "say hello");
    let lines = run.world.take_console_output();
    assert!(
        lines.contains(&b"Can't \"say\", not connected".to_vec()),
        "{lines:?}"
    );
    let slot = run.world.builtins.messages.clients.get(id(2)).unwrap();
    assert!(slot.commands.is_empty());
}

// spec NET-7.12: each local player's console has its own buffer: what a wait leaves runs as
// that player in the next frame, and the host's waiting text never runs as it
#[test]
fn a_wait_keeps_each_players_console_apart() {
    let mut run = players(4, 2, "", "");
    let progs = Progs::read(&run.progs).unwrap();
    run.world.console("wait\n+attack\n");
    run.world.execute_console(&progs);
    run.world
        .execute_console_as(&progs, id(1), "+jump; wait; impulse 7");
    use crate::input::Button;
    assert_eq!(
        run.world.take_client_commands_of(id(1)),
        vec![crate::ClientCommand::Press(Button::Jump, None)],
        "the host's +attack waits on the host's console"
    );
    run.frames(1, FRAME).unwrap();
    assert_eq!(
        run.world.take_client_commands_of(id(1)),
        vec![crate::ClientCommand::Impulse(7)],
        "the rest of the line ran as player 2 in the next frame"
    );
    assert_eq!(
        run.world.take_client_commands(),
        vec![crate::ClientCommand::Press(Button::Attack, None)],
        "the host's rest ran as the host's"
    );
    let progs = Progs::read(&run.progs).unwrap();
    run.world.disconnect(&progs, id(1)).unwrap();
    assert!(
        run.world.local_consoles.is_empty(),
        "it left with its player"
    );
}

// spec NET-7.12: another local player's name and colours go to its own slot and leave the
// host's _cl_name and _cl_color as they are
#[test]
fn a_local_players_name_is_its_own() {
    let mut run = players(4, 2, "", "");
    let progs = Progs::read(&run.progs).unwrap();
    let name = |run: &Running| {
        run.world
            .builtins
            .mock
            .engine()
            .cvars
            .text("_cl_name")
            .unwrap()
            .to_owned()
    };
    let before = name(&run);
    run.world.execute_console_as(&progs, id(1), "name Bob");
    run.world.execute_console_as(&progs, id(1), "color 4 5");
    run.world.execute_console_as(&progs, id(1), "name");
    assert_eq!(name(&run), before);
    let second = run.world.builtins.messages.clients.get(id(1)).unwrap();
    assert_eq!(
        second.commands,
        vec![b"name Bob".to_vec(), b"color 4 5".to_vec()]
    );
    run.frames(1, FRAME).unwrap();
    assert_eq!(run.world.client_name(id(1)), Some(b"Bob".to_vec()));
}

// spec NET-7.13: a local player comes back to the slot it had; a slot active or past the last
// is refused and nothing changes
#[test]
fn a_local_player_rejoins_its_own_slot() {
    let mut run = players(4, 1, "", "");
    let progs = Progs::read(&run.progs).unwrap();
    assert!(run.world.connect_in(&progs, id(2)).unwrap());
    assert!(run.world.is_spawned(id(2)));
    assert!(
        !run.world.is_spawned(id(1)),
        "the slot before it stays free"
    );
    assert!(
        !run.world.connect_in(&progs, id(2)).unwrap(),
        "already active"
    );
    assert!(
        !run.world.connect_in(&progs, id(15)).unwrap(),
        "past the last"
    );
    assert_eq!(
        connect(&mut run),
        Some(id(1)),
        "the first free slot is still free"
    );
}
