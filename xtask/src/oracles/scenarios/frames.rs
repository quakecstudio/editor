// SPDX-License-Identifier: GPL-3.0-or-later

//! Scenario tests of clients and server frames: `differential.md` DIFF-4 and DIFF-5.

use super::cases::{done, f, first_edict, named, remove, spawn, start_frame, worldspawn};
use super::{Arg, Case, Check, Program, Val, WORLDSPAWN, Want, field, global, scenario};

pub(super) fn all() -> Vec<Case> {
    let mut cases = vec![
        client_enters(),
        two_clients_enter(),
        slots_without_players(),
        muzzle_flash_cleared(),
        buffers_emptied_each_frame(),
        clock_after_100_frames(),
        force_retouch(),
        bad_movetype(),
        bad_movetype_between_values(),
        free_edicts_skipped(),
        movetypes_that_think(),
        client_turn(),
        client_freed_by_its_think(),
        tossed_client_freed_by_its_think(),
        bad_client_movetype(),
        pusher(),
        thinking(),
        linking(),
    ];
    cases.extend(client_movetypes());
    cases
}

// spec DIFF-4.1: a client enters by `SetNewParms`, its parms kept, its edict's fields zeroed,
// `colormap` its number, `team` 1, `netname` the engine string `player`, then `ClientConnect`
// with `self` the edict and `time` the clock, then `PutClientInServer`.
fn client_enters() -> Case {
    let mut p = Program::new();
    let e = p.temp();
    let five = p.float(5.0);
    let nine = p.float(9.0);
    let mut code = vec![format!("STORE_F {five} {}", global::PARM1)];
    code.extend(first_edict(&mut p, e));
    code.extend(p.write_field(e, field::HEALTH, nine));
    let new_parms = p.func("new_parms", &done(code));
    p.system(global::SET_NEW_PARMS, new_parms);
    let connect = named(&mut p, global::CLIENT_CONNECT, "connect");
    let put = named(&mut p, global::PUT_CLIENT_IN_SERVER, "put");
    Case::new("client-enters", p, scenario("clients 1\nframes 0\n"))
        .check(Check::Seq(vec![
            Want::Run(new_parms),
            Want::Field(1, field::HEALTH, f(9.0)),
            Want::Exit,
            Want::All(vec![
                Want::Run(connect),
                Want::Field(1, field::HEALTH, f(0.0)),
                Want::Field(1, field::COLORMAP, f(1.0)),
                Want::Field(1, field::TEAM, f(1.0)),
                Want::Field(1, field::NETNAME, Val::EngineText("player")),
                Want::Global(global::SELF, Val::Entity(1)),
            ]),
            Want::Exit,
            Want::Run(put),
        ]))
        .check(Check::FinalGlobal(global::PARM1, f(5.0)))
}

// spec DIFF-4.1: the client slots enter in order, each with its own edict and colormap.
fn two_clients_enter() -> Case {
    let mut p = Program::new();
    let connect = named(&mut p, global::CLIENT_CONNECT, "connect");
    Case::new(
        "two-clients-enter",
        p,
        scenario("maxclients 3\nclients 2\nframes 0\n"),
    )
    .check(Check::Seq(vec![
        Want::All(vec![
            Want::Run(connect),
            Want::Global(global::SELF, Val::Entity(1)),
            Want::Field(1, field::COLORMAP, f(1.0)),
        ]),
        Want::All(vec![
            Want::Run(connect),
            Want::Global(global::SELF, Val::Entity(2)),
            Want::Field(2, field::COLORMAP, f(2.0)),
        ]),
    ]))
    .check(Check::Count(Want::Run(connect), 2))
    .check(Check::Never(Want::FieldChanged(3, field::COLORMAP)))
}

// spec DIFF-4.2, DIFF-5.4: a slot whose player entered gets `PlayerPreThink` each frame; a slot
// with no player does nothing on its turn, so its edict never thinks.
fn slots_without_players() -> Case {
    let mut p = Program::new();
    let pre = named(&mut p, global::PLAYER_PRE_THINK, "pre");
    let think = p.func("slot_think", &done(Vec::new()));
    let e = p.temp();
    let when = p.float(1.05);
    let think_ref = p.fref(think);
    let nextent = p.builtin("nextent", 47);
    let mut code = first_edict(&mut p, e);
    for _ in 0..2 {
        code.extend(p.call(nextent, &[Arg::F(e)]));
        code.push(format!("STORE_ENT {} {e}", super::RETURN));
    }
    code.extend(p.write_field(e, field::NEXTTHINK, when));
    code.extend(p.write_field(e, field::THINK, think_ref));
    worldspawn(&mut p, code);
    Case::new(
        "slots-without-players",
        p,
        scenario("maxclients 3\nclients 2\nframes 3\n"),
    )
    .check(Check::Count(Want::Run(pre), 6))
    .check(Check::FinalField(3, field::NEXTTHINK, f(1.05)))
    .check(Check::Never(Want::Run(think)))
}

// spec DIFF-5.1: a server frame sets `frametime` to (f32)T and, at its end, clears
// EF_MUZZLEFLASH (2) in `effects` of edicts 1 and up, through an integer.
fn muzzle_flash_cleared() -> Case {
    let mut p = Program::new();
    let start = start_frame(&mut p, Vec::new());
    let e1 = p.temp();
    let e2 = p.temp();
    let three = p.float(3.0);
    let two_and_a_half = p.float(2.5);
    let mut code = first_edict(&mut p, e1);
    code.extend(spawn(&mut p, e2));
    code.extend(p.write_field(e1, field::EFFECTS, three));
    code.extend(p.write_field(e2, field::EFFECTS, two_and_a_half));
    worldspawn(&mut p, code);
    Case::new(
        "muzzle-flash-cleared",
        p,
        scenario("frames 2\nframetime 0.3\n"),
    )
    .check(Check::Seq(vec![
        Want::All(vec![
            Want::Run(start),
            Want::Global(global::FRAMETIME, f(0.3)),
        ]),
        Want::All(vec![
            Want::Run(start),
            Want::Field(1, field::EFFECTS, f(1.0)),
            Want::Field(2, field::EFFECTS, f(0.0)),
        ]),
    ]))
    .check(Check::Count(Want::FieldChanged(1, field::EFFECTS), 2))
}

// spec DIFF-5.1, DIFF-6.5: each server frame empties the unreliable datagram before its physics
// and the reliable one after, so a byte written to each every frame never fills them.
fn buffers_emptied_each_frame() -> Case {
    let mut p = Program::new();
    let write_byte = p.builtin("WriteByte", 52);
    let broadcast = p.float(0.0);
    let all = p.float(2.0);
    let one = p.float(1.0);
    let mut code = p.call(write_byte, &[Arg::F(broadcast), Arg::F(one)]);
    code.extend(p.call(write_byte, &[Arg::F(all), Arg::F(one)]));
    let start = start_frame(&mut p, code);
    Case::new("buffers-emptied", p, scenario("frames 1100\n"))
        .check(Check::Count(Want::Run(start), 1102))
        .check(Check::Never(Want::ErrorMessage(
            "SZ_GetSpace: overflow without allowoverflow set",
        )))
}

// spec DIFF-5.2: the clock advances by T in double at the end of each physics frame; after 100
// frames of 0.1 it is 11.199999999999978, whose float is 0x41333333.
fn clock_after_100_frames() -> Case {
    let mut p = Program::new();
    let start = start_frame(&mut p, Vec::new());
    Case::new("clock-after-100-frames", p, scenario("frames 101\n"))
        .check(Check::Count(Want::Run(start), 103))
        .check(Check::FinalGlobal(global::TIME, Val::Bits(0x4133_3333)))
}

// spec DIFF-5.3, DIFF-5.7: while `force_retouch` is not 0 a physics frame links every edict in
// use, then decreases it by 1.
fn force_retouch() -> Case {
    let mut p = Program::new();
    let e = p.temp();
    let two = p.float(2.0);
    let origin = p.vector([1.0, 2.0, 3.0]);
    let mut code = spawn(&mut p, e);
    code.extend(p.write_vector_field(e, field::ORIGIN, origin));
    code.push(format!("STORE_F {two} {}", global::FORCE_RETOUCH));
    worldspawn(&mut p, code);
    Case::new("force-retouch", p, scenario("frames 2\n"))
        .check(Check::Seq(vec![
            Want::Global(global::FORCE_RETOUCH, f(2.0)),
            Want::All(vec![
                Want::Global(global::FORCE_RETOUCH, f(1.0)),
                Want::Field(2, field::ABSMAX, f(2.0)),
                Want::Field(2, field::ABSMIN.saturating_add(1), f(1.0)),
                Want::Field(2, field::ABSMAX.saturating_add(2), f(4.0)),
            ]),
            Want::Global(global::FORCE_RETOUCH, f(0.0)),
        ]))
        .check(Check::Count(Want::GlobalChanged(global::FORCE_RETOUCH), 3))
}

/// A map whose spawned edict's think, at the first server frame, sets its `movetype` to
/// `movetype`; clients entered when `clients` is 1.
fn movetype_set_by_think(movetype: f32, name: &str) -> (Program, i32) {
    let mut p = Program::new();
    let value = p.float(movetype);
    let mut code = Vec::new();
    code.extend(p.write_field(global::SELF, field::MOVETYPE, value));
    let think = p.func("set_movetype", &done(code));
    let e = p.temp();
    let when = p.float(1.25);
    let think_ref = p.fref(think);
    let mut code = spawn(&mut p, e);
    code.extend(p.write_field(e, field::NEXTTHINK, when));
    code.extend(p.write_field(e, field::THINK, think_ref));
    worldspawn(&mut p, code);
    let disconnect = named(&mut p, global::CLIENT_DISCONNECT, name);
    (p, disconnect)
}

// spec DIFF-5.3, DIFF-8.1, DIFF-8.6: an edict whose movetype is not one physics knows is the
// fatal error `SV_Physics: bad movetype <n>`, raised by the host outside any run (function 0,
// statement -1); a fatal error shuts nothing down.
fn bad_movetype() -> Case {
    let (p, disconnect) = movetype_set_by_think(1.0, "disconnect");
    Case::new("bad-movetype", p, scenario("clients 1\nframes 5\n"))
        .check(Check::Last(Want::Error(
            0,
            -1,
            "SV_Physics: bad movetype 1",
        )))
        .check(Check::Never(Want::Run(disconnect)))
}

// spec DIFF-5.3: the movetypes are compared as floats, so 7.5 is a bad movetype, named by its
// integer conversion.
fn bad_movetype_between_values() -> Case {
    let (p, _) = movetype_set_by_think(7.5, "disconnect");
    Case::new("bad-movetype-7-5", p, scenario("frames 5\n")).check(Check::Last(Want::Error(
        0,
        -1,
        "SV_Physics: bad movetype 7",
    )))
}

// spec DIFF-5.3: a free edict gets no turn.
fn free_edicts_skipped() -> Case {
    let mut p = Program::new();
    let think = p.func("freed_think", &done(Vec::new()));
    let e = p.temp();
    let when = p.float(1.05);
    let think_ref = p.fref(think);
    let mut code = spawn(&mut p, e);
    code.extend(p.write_field(e, field::NEXTTHINK, when));
    code.extend(p.write_field(e, field::THINK, think_ref));
    code.extend(remove(&mut p, e));
    worldspawn(&mut p, code);
    Case::new("free-edicts-skipped", p, scenario("frames 3\n"))
        .check(Check::Never(Want::Run(think)))
}

// spec DIFF-5.3, DIFF-5.6: an edict whose movetype is 0, 8, 4, 6, 10, 5 or 9 thinks, with
// `self` the edict and `other` the world.
fn movetypes_that_think() -> Case {
    let mut p = Program::new();
    let think = p.func("think", &done(Vec::new()));
    let think_ref = p.fref(think);
    let when = p.float(1.05);
    let other = p.temp();
    let mut code = first_edict(&mut p, other);
    code.push(format!("STORE_ENT {other} {}", global::OTHER));
    for movetype in [0.0, 8.0, 4.0, 6.0, 10.0, 5.0, 9.0] {
        let e = p.temp();
        let value = p.float(movetype);
        code.extend(spawn(&mut p, e));
        code.extend(p.write_field(e, field::MOVETYPE, value));
        code.extend(p.write_field(e, field::NEXTTHINK, when));
        code.extend(p.write_field(e, field::THINK, think_ref));
    }
    worldspawn(&mut p, code);
    let mut wants = Vec::new();
    for edict in 2..=8 {
        wants.push(Want::All(vec![
            Want::Run(think),
            Want::Global(global::SELF, Val::Entity(edict)),
        ]));
    }
    Case::new("movetypes-that-think", p, scenario("frames 0\n"))
        .check(Check::Count(Want::Run(think), 7))
        .check(Check::Seq(wants))
        .check(Check::FinalGlobal(global::OTHER, Val::Entity(0)))
}

/// A program whose one client's `PutClientInServer` gives it `movetype` and a think at the
/// first frame, which removes the client when `removes` is set.
fn client_program(movetype: f32, removes: bool) -> (Program, [i32; 3]) {
    let mut p = Program::new();
    let pre = named(&mut p, global::PLAYER_PRE_THINK, "pre");
    let post = named(&mut p, global::PLAYER_POST_THINK, "post");
    let think_code = if removes {
        remove(&mut p, global::SELF)
    } else {
        Vec::new()
    };
    let think = p.func("client_think", &done(think_code));
    let think_ref = p.fref(think);
    let value = p.float(movetype);
    let when = p.float(1.25);
    let mut code = p.write_field(global::SELF, field::MOVETYPE, value);
    code.extend(p.write_field(global::SELF, field::NEXTTHINK, when));
    code.extend(p.write_field(global::SELF, field::THINK, think_ref));
    let put = p.func("put", &done(code));
    p.system(global::PUT_CLIENT_IN_SERVER, put);
    (p, [pre, think, post])
}

// spec DIFF-5.4: a client's turn is `PlayerPreThink`, thinking, linking, then
// `PlayerPostThink`, each with `self` the edict and `time` the clock.
fn client_turn() -> Case {
    let (p, [pre, think, post]) = client_program(3.0, false);
    Case::new("client-turn", p, scenario("clients 1\nframes 1\n")).check(Check::Seq(vec![
        Want::All(vec![
            Want::Run(pre),
            Want::Global(global::SELF, Val::Entity(1)),
        ]),
        Want::Run(think),
        Want::All(vec![
            Want::Run(post),
            // The think ran at its own time; PlayerPostThink runs at the clock again.
            Want::Global(global::TIME, Val::Bits(0x3f99_999a)),
            Want::Field(1, field::ABSMIN, f(-1.0)),
            Want::Field(1, field::ABSMAX, f(1.0)),
        ]),
    ]))
}

// spec DIFF-5.4: a client of movetype 0, 8 or 6 thinks too, between its two player functions.
fn client_movetypes() -> Vec<Case> {
    let mut cases = Vec::new();
    for (name, movetype) in [
        ("client-movetype-0", 0.0),
        ("client-movetype-8", 8.0),
        ("client-movetype-6", 6.0),
    ] {
        let (p, [pre, think, post]) = client_program(movetype, false);
        cases.push(
            Case::new(name, p, scenario("clients 1\nframes 1\n")).check(Check::Seq(vec![
                Want::Run(pre),
                Want::Run(think),
                Want::Run(post),
            ])),
        );
    }
    cases
}

// spec DIFF-5.4: a client of movetype 0, 3, 5 or 8 whose think frees it ends its turn there.
fn client_freed_by_its_think() -> Case {
    let (p, [_, think, post]) = client_program(5.0, true);
    Case::new("client-freed", p, scenario("clients 1\nframes 1\n"))
        .check(Check::Count(Want::Run(think), 1))
        .check(Check::Never(Want::Run(post)))
}

// spec DIFF-5.4: a client of movetype 6 or 10 goes on after its think, even if it freed it.
fn tossed_client_freed_by_its_think() -> Case {
    let (p, [_, think, post]) = client_program(10.0, true);
    Case::new("tossed-client-freed", p, scenario("clients 1\nframes 1\n"))
        .check(Check::Count(Want::Run(think), 1))
        .check(Check::Count(Want::Run(post), 1))
}

// spec DIFF-5.4: any other client movetype is the fatal error `SV_Physics_client: bad movetype
// <n>`.
fn bad_client_movetype() -> Case {
    let (p, _) = client_program(7.0, false);
    Case::new("bad-client-movetype", p, scenario("clients 1\nframes 1\n")).check(Check::Last(
        Want::Error(0, -1, "SV_Physics_client: bad movetype 7"),
    ))
}

// spec DIFF-5.5: a MOVETYPE_PUSH edict's turn is SV_Physics_Pusher with no move: `ltime`
// advances to a think time inside the frame and the think runs; a pusher with `nextthink` 0
// does not advance.
fn pusher() -> Case {
    let mut p = Program::new();
    let think = p.func("push_think", &done(Vec::new()));
    let think_ref = p.fref(think);
    let push = p.float(7.0);
    let when = p.float(0.05);
    let a = p.temp();
    let b = p.temp();
    let mut code = spawn(&mut p, a);
    code.extend(p.write_field(a, field::MOVETYPE, push));
    code.extend(p.write_field(a, field::NEXTTHINK, when));
    code.extend(p.write_field(a, field::THINK, think_ref));
    code.extend(spawn(&mut p, b));
    code.extend(p.write_field(b, field::MOVETYPE, push));
    worldspawn(&mut p, code);
    Case::new("pusher", p, scenario("frames 3\n"))
        .check(Check::Seq(vec![Want::All(vec![
            Want::Run(think),
            Want::Field(2, field::LTIME, f(0.05)),
            Want::Field(2, field::NEXTTHINK, f(0.0)),
            Want::Global(global::SELF, Val::Entity(2)),
        ])]))
        .check(Check::Count(Want::Run(think), 1))
        .check(Check::FinalField(3, field::LTIME, f(0.0)))
}

// spec DIFF-5.6: thinking runs `think` when `nextthink` is above 0 and at most clock + T, with
// `time` the `nextthink`, or the clock when `nextthink` is earlier, and `nextthink` set to 0.
fn thinking() -> Case {
    let mut p = Program::new();
    let think = p.func("think", &done(Vec::new()));
    let think_ref = p.fref(think);
    let mut code = Vec::new();
    let mut edicts = Vec::new();
    for when in [1.05, 0.5, 1.1, -1.0] {
        let e = p.temp();
        let value = p.float(when);
        code.extend(spawn(&mut p, e));
        code.extend(p.write_field(e, field::NEXTTHINK, value));
        code.extend(p.write_field(e, field::THINK, think_ref));
        edicts.push(e);
    }
    worldspawn(&mut p, code);
    Case::new("thinking", p, scenario("frames 0\n"))
        .check(Check::Seq(vec![
            Want::All(vec![
                Want::Run(think),
                Want::Global(global::TIME, f(1.05)),
                Want::Field(2, field::NEXTTHINK, f(0.0)),
                Want::Global(global::SELF, Val::Entity(2)),
            ]),
            // nextthink 0.5 is before the clock: time is the clock, 1.0.
            Want::All(vec![
                Want::Run(think),
                Want::Global(global::TIME, f(1.0)),
                Want::Global(global::SELF, Val::Entity(3)),
            ]),
            // 1.1 as a float is above 1.0 + 0.1, so it waits for the second settle frame, whose
            // `StartFrame` has already set `time` to the same float.
            Want::All(vec![
                Want::Run(super::NOOP),
                Want::Global(global::TIME, f(1.1)),
            ]),
            Want::All(vec![
                Want::Run(think),
                Want::Field(4, field::NEXTTHINK, f(0.0)),
                Want::Global(global::SELF, Val::Entity(4)),
            ]),
        ]))
        .check(Check::Count(Want::Run(think), 3))
        .check(Check::FinalField(5, field::NEXTTHINK, f(-1.0)))
}

// spec DIFF-5.7: linking writes `absmin` and `absmax` from `origin`, `mins` and `maxs`, a unit
// wider each way, or 15 wider in x and y for an FL_ITEM (256); it never links the world and
// runs no `touch`.
fn linking() -> Case {
    let mut p = Program::new();
    let touch = p.func("touched", &done(Vec::new()));
    let touch_ref = p.fref(touch);
    let setorigin = p.builtin("setorigin", 2);
    let setsize = p.builtin("setsize", 4);
    let a = p.temp();
    let b = p.temp();
    let origin = p.vector([1.0, 2.0, 3.0]);
    let mins = p.vector([-1.0, -2.0, -3.0]);
    let maxs = p.vector([4.0, 5.0, 6.0]);
    let item = p.float(256.0);
    let mut code = spawn(&mut p, a);
    code.extend(p.write_field(a, field::TOUCH, touch_ref));
    code.extend(p.call(setorigin, &[Arg::F(a), Arg::V(origin)]));
    code.extend(p.call(setsize, &[Arg::F(a), Arg::V(mins), Arg::V(maxs)]));
    code.extend(spawn(&mut p, b));
    code.extend(p.write_field(b, field::FLAGS, item));
    code.extend(p.call(setorigin, &[Arg::F(b), Arg::V(origin)]));
    code.extend(p.call(setorigin, &[Arg::F(global::WORLD), Arg::V(origin)]));
    worldspawn(&mut p, code);
    let mut case = Case::new("linking", p, scenario("frames 1\n"));
    for (axis, (min, max)) in [(-1.0, 6.0), (-1.0, 8.0), (-1.0, 10.0)]
        .into_iter()
        .enumerate()
    {
        let axis = u32::try_from(axis).unwrap_or_default();
        case = case
            .check(Check::FinalField(
                2,
                field::ABSMIN.saturating_add(axis),
                f(min),
            ))
            .check(Check::FinalField(
                2,
                field::ABSMAX.saturating_add(axis),
                f(max),
            ));
    }
    for (axis, (min, max)) in [(-14.0, 16.0), (-13.0, 17.0), (3.0, 3.0)]
        .into_iter()
        .enumerate()
    {
        let axis = u32::try_from(axis).unwrap_or_default();
        case = case
            .check(Check::FinalField(
                3,
                field::ABSMIN.saturating_add(axis),
                f(min),
            ))
            .check(Check::FinalField(
                3,
                field::ABSMAX.saturating_add(axis),
                f(max),
            ));
    }
    case.check(Check::Never(Want::FieldChanged(0, field::ABSMIN)))
        .check(Check::Never(Want::Run(touch)))
        .check(Check::Count(Want::Run(WORLDSPAWN), 1))
}
